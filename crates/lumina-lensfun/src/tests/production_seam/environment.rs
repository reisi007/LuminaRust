//! LENSFUN-DB-33: the two properties of the production path that the *order*
//! tests in [`super`] cannot reach — the environment it reads, and the lock it
//! must not be running under (review findings MITTEL-3, NIEDRIG-4).
//!
//! Split out of `tests/production_seam/mod.rs` (file-size ratchet, User-Vorgabe
//! 2026-09-17). Both belong to the same question — "does the code production runs
//! behave the way the documentation says?" — but neither is about the order of
//! the emissions, and one of them must not be able to wedge the other.

use super::fixture_env;
use crate::db_layers::SkippedLayer;
use crate::db_path::{EnvValues, FsProbe};
use crate::system_load::Diagnostics;
use crate::tests::diagnostics::{TempTree, FIXTURE_XML};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// **The environment is read in exactly one place, and this pins which place.
///
/// `resolve_system_with` cannot be steered (that would need
/// `std::env::set_var`), so what the *rest* of the production path reads has to
/// be asserted directly: [`EnvValues::read`] is the only function that names an
/// environment variable, it must ask for exactly the three names of the
/// resolution order, and each answer must land in the field the resolution
/// consumes. A renamed variable (`XDG_DATA_HOME` → `XDG_CACHE_HOME`), a swapped
/// mapping, or a fourth lookup fails here.
///
/// The compiled datadir is deliberately **not** readable at runtime: it is a
/// compile-time constant, and this test also pins that a stand-in reader cannot
/// influence it — otherwise a test could "verify" source 2 with a value no real
/// process could ever produce.
#[test]
fn the_resolution_reads_exactly_three_named_environment_values() {
    let asked: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
    let env = EnvValues::read(|name| {
        asked.borrow_mut().push(name.to_owned());
        match name {
            "LUMINA_LENSFUN_DB" => Some(OsString::from("/override")),
            "XDG_DATA_HOME" => Some(OsString::from("/xdg")),
            "HOME" => Some(OsString::from("/home/u")),
            other => panic!("unexpected environment lookup: {other}"),
        }
    });
    assert_eq!(
        *asked.borrow(),
        vec![
            crate::db_path::DB_DIR_ENV.to_owned(),
            crate::db_path::XDG_DATA_HOME_ENV.to_owned(),
            crate::db_path::HOME_ENV.to_owned(),
        ],
        "the resolution must read exactly these three names, in this order"
    );
    assert_eq!(env.override_dir.as_deref(), Some(OsStr::new("/override")));
    assert_eq!(env.xdg_data_home.as_deref(), Some(OsStr::new("/xdg")));
    assert_eq!(env.home.as_deref(), Some(OsStr::new("/home/u")));
    assert_eq!(
        env.compiled, None,
        "source 2 is a compile-time constant, so no environment may supply it"
    );
    // …and an unset variable stays unset, which is the normal case on a machine
    // without an operator override.
    let empty = EnvValues::read(|_| None);
    assert_eq!(empty.override_dir, None);
    assert_eq!(empty.xdg_data_home, None);
    assert_eq!(empty.home, None);
    // Production fills the compiled datadir from the build-time constant and
    // nothing else; a build that baked in a different variable name would show up
    // here as a value that is not a Lensfun directory.
    let process = EnvValues::from_process();
    assert_eq!(
        process.compiled,
        crate::db_path::COMPILED_DATADIR.map(OsString::from),
        "production must use the datadir build.rs baked in under {}",
        crate::db_path::COMPILED_DATADIR_ENV
    );
    if let Some(dir) = process.compiled.as_deref() {
        assert!(
            crate::db_path::lensfun_dir_in_datadir(dir).is_some(),
            "the baked-in datadir must be usable, got {dir:?}"
        );
    }
}

/// **The fixture tree must be the database these tests actually load.**
///
/// `TempTree`'s leaf is literally `lensfun` so that `fixture_env`'s
/// compiled-datadir candidate resolves to the tree itself and the system schema
/// is `<tree>/version_1` — exactly where `TempTree::write` puts the fixture.
/// Without that leaf the candidate pointed at `<tree>/lensfun/version_1` while
/// the fixture filled `<tree>/version_1`, so the production-seam tests silently
/// resolved the machine's real database (`PlatformDefault`, measured) and their
/// system-layer fixture was dead weight. This test fails on that regression.
#[test]
fn the_fixture_tree_resolves_as_its_own_system_database() {
    let tree = TempTree::new("fixture-layout");
    tree.write("a.xml", FIXTURE_XML);
    let resolved = fixture_env(&tree.0, None)
        .resolve(&FsProbe)
        .expect("the fixture tree must resolve without the machine's database");
    assert_eq!(
        resolved.dir, tree.0,
        "the fixture tree itself must be the resolved system database"
    );
    assert_eq!(
        resolved.source,
        crate::db_path::Source::CompiledDataDir,
        "the fixture must win through the compiled-datadir candidate, not fall \
         through to the platform default"
    );
    assert_eq!(resolved.layers[0].files.len(), 1);
    assert!(resolved.pin_honored());
}

/// **NIEDRIG-4, second line of defence — and the one that does not rest on the
/// instrument.**
///
/// [`StrictRecorder`] above proves the invariant with a thread-local depth
/// counter. That counter is *this crate's own* code, so hard-wiring it (always
/// `false`) leaves the assertion green while the invariant is broken: a test
/// that can be satisfied by editing the thing it measures proves nothing.
///
/// This test measures the invariant by its **consequence** instead.
/// `LENSFUN_GLOBAL_LOCK` is a plain, non-reentrant `std::sync::Mutex`, so a sink
/// that re-enters this crate while the load still holds the lock *deadlocks*.
/// The sink below therefore loads a database from inside `file_rejected`. If the
/// emission ever moved back under the guard, the re-entrant load would block
/// forever and this test fails on the deadline.
///
/// - no process-global flag: the assertion is a wall-clock deadline on a thread
///   of its own, so a parallel test cannot produce a false positive;
/// - a false alarm would need the machine to be ~200× slower than the ~1 ms the
///   happy path takes, so the deadline is deliberately generous;
/// - a hung thread cannot wedge the suite: `cargo test` exits the process when
///   the harness finishes, and the thread holds no lock other than the one that
///   is already stuck.
#[test]
fn a_sink_that_re_enters_the_crate_must_not_deadlock() {
    const DEADLINE: std::time::Duration = std::time::Duration::from_secs(20);

    /// A sink that touches the crate again — exactly what a host logger might
    /// do (a profile lookup in a log formatter, a metrics probe, …).
    struct ReentrantSink {
        fixture: PathBuf,
        entered: Arc<Mutex<Vec<String>>>,
    }

    impl Diagnostics for ReentrantSink {
        fn resolved(&mut self, _resolved: &crate::db_path::Resolved) {}

        fn file_rejected(&mut self, _layer_dir: &Path, _file: &Path) {
            // `load_file` takes `lensfun_global_lock` itself. Under the guard
            // this never returns. It must also *succeed*, or the test could not
            // tell "ran outside the lock" from "ran outside the lock but got
            // nothing back".
            let reloaded = crate::LensfunDb::load_file(&self.fixture.join("a.xml"));
            let usable = reloaded.is_some_and(|db| {
                crate::Corrector::for_camera(
                    &db,
                    "Probe Corp",
                    "Probe Body",
                    Some("Probe Lens 50mm f/2.8"),
                    400,
                    300,
                    50.0,
                    2.8,
                    10.0,
                )
                .is_some()
            });
            self.entered
                .lock()
                .expect("reentrancy log")
                .push(format!("rejected(load_usable={usable})"));
        }

        fn layer_skipped(&mut self, _skipped: &SkippedLayer) {}
        fn pin_displaced(&mut self, _resolved: &crate::db_path::Resolved) {}
        fn failed(&mut self, _err: &crate::db_path::SystemDbError) {}
    }

    let tree = TempTree::new("reentrant");
    tree.write("a.xml", FIXTURE_XML);
    tree.write("broken.xml", "not xml at all\n");
    let entered = Arc::default();
    let (tx, rx) = std::sync::mpsc::channel();
    // `TempTree` holds no lock and its `Drop` does not touch the crate, so the
    // fixture stays alive for the worker thread — which outlives the test body
    // if it ever has to block.
    let env = fixture_env(&tree.0, Some(&tree.0));
    let mut sink = ReentrantSink {
        fixture: tree.0.join(crate::db_path::SCHEMA_SUBDIR),
        entered,
    };
    std::thread::spawn(move || {
        let outcome = crate::system_load::report_with(&env, &FsProbe, &mut sink);
        // The re-entrant load must have worked, which is only possible outside
        // the guard. `entered` is a separate `Arc`, so sending it needs no lock
        // and cannot block behind the sink's own.
        let _ = tx.send((outcome.is_ok(), Arc::clone(&sink.entered)));
    });

    let (ok, entered) = rx.recv_timeout(DEADLINE).unwrap_or_else(|_| {
        panic!(
            "a caller-supplied sink deadlocked the load, so it ran while the \
                 non-reentrant global lock was held (deadline {DEADLINE:?})"
        )
    });
    assert!(ok, "the fixture database must load");
    let entered = entered.lock().expect("reentrancy log").clone();
    assert_eq!(
        entered.len(),
        1,
        "the re-entrant sink must have been called exactly once, for the one \
         rejected file: {entered:?}"
    );
    assert_eq!(
        entered[0], "rejected(load_usable=true)",
        "the re-entrant load must have returned a *usable* database, not just \
         any handle: {entered:?}"
    );
    assert!(
        !crate::system_load::load_layers_holds_the_lock(),
        "and this thread is not inside the load at all"
    );
}
