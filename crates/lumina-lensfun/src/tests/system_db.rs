//! LENSFUN-DB-33: the system-database tests that need the **real**, installed
//! Lensfun database.
//!
//! Extracted from `lib.rs` (file-size ratchet, User-Vorgabe 2026-09-17), same
//! pattern as `src/tests/strict_match.rs`.
//!
//! These tests deliberately assert on real database content and deliberately do
//! **not** skip. `system_db()` fails with the complete, named `SystemDbError`
//! diagnostic (every probed location, its source, the reason and the
//! remediation) when the database cannot be resolved — a silently skipped
//! assertion would make the suite green without ever having exercised a real
//! profile, which is exactly the failure mode LENSFUN-DB-33 exists to remove.
//!
//! # Why nothing here mutates the process environment
//!
//! An earlier revision set `LUMINA_LENSFUN_DB` to test the override end to end.
//! That is `unsafe` for a reason no test-local lock can fix: a concurrent
//! `getenv` in *any* other thread (e.g. `std::env::temp_dir()` in
//! `db_path::fs_probe_reports_sorted_xml_files_only`) is undefined behaviour,
//! and such a test does not take the lock. The override is therefore exercised
//! through the injected seam ([`resolve_with`]) plus the real loader
//! ([`LensfunDb::load_layers`]) instead, which covers strictly more and touches
//! no global. As a consequence these tests need **no lock at all** and stay
//! fully parallel — including
//! [`concurrent_db_load_and_search_is_safe`](super::concurrent_db_load_and_search_is_safe),
//! whose whole point is that several threads load the database at once.

use super::{Corrector, LensfunDb, LENS, MAKE, MODEL};
use crate::db_layers::user_db_dir;
use crate::db_path::{self, FsProbe, LayerOrigin};
use crate::system_load::{Diagnostics, SilentDiagnostics};
use std::ffi::OsStr;
use std::path::PathBuf;

/// Records what the production entry point reports, so a real-machine test can
/// assert on the events as well as on the loaded handle.
#[derive(Debug, Default)]
struct EventLog(Vec<String>);

impl Diagnostics for EventLog {
    fn resolved(&mut self, resolved: &db_path::Resolved) {
        self.0.push(format!(
            "resolved[{}|{}]",
            resolved.primary.as_str(),
            resolved.file_count()
        ));
    }
    fn file_rejected(&mut self, _dir: &std::path::Path, file: &std::path::Path) {
        self.0.push(format!("rejected[{}]", file.display()));
    }
    fn layer_skipped(&mut self, skipped: &crate::db_layers::SkippedLayer) {
        self.0.push(format!("skipped[{skipped}]"));
    }
    fn pin_displaced(&mut self, resolved: &db_path::Resolved) {
        self.0
            .push(format!("pin_displaced[{}]", resolved.primary.as_str()));
    }
    fn failed(&mut self, err: &db_path::SystemDbError) {
        self.0.push(format!("failed[{err}]"));
    }
}

/// The installed system database, or a **loud, named** failure.
pub(super) fn system_db() -> LensfunDb {
    LensfunDb::resolve_system().unwrap_or_else(|err| panic!("{err}"))
}

/// [`db_path::resolve`], panicking with the full named diagnostic.
///
/// `Result::expect` would only print the `Debug` form; the whole point of
/// LENSFUN-DB-33 is that a missing database says *where* it looked and *what
/// to do*, so the `Display` (with every probed location, its source and the
/// remediation) is what the test output must show.
fn resolve_db() -> db_path::Resolved {
    db_path::resolve().unwrap_or_else(|err| panic!("{err}"))
}

/// **The production entry point, on the real machine, with a caller sink.**
///
/// The hermetic suites drive `report_with` with injected values because steering
/// the real process environment is undefined behaviour next to a concurrent
/// `getenv` (see the module docs). That leaves exactly one production statement
/// no hermetic test can reach: `resolve_system_with` feeding
/// `EnvValues::from_process()` into that function. This test closes it from the
/// other side — it calls the real entry point, so the wiring, the real reader and
/// the real `FsProbe` all execute, and it asserts that the caller-supplied sink
/// receives the success event for a database that a real profile lookup then
/// proves usable.
///
/// Without it, a production change that stopped routing through `report_with`
/// (or dropped the sink) would only be caught by the fact that some other test
/// still compiles.
#[test]
fn the_production_entry_point_loads_and_reports_the_real_database() {
    let mut log = EventLog::default();
    let db = LensfunDb::resolve_system_with(&mut log)
        .unwrap_or_else(|err| panic!("the production entry point must load: {err}; {log:?}"));
    let events = log.0;
    assert_eq!(
        events.len(),
        1,
        "a clean machine reports exactly one outcome event, and it is `resolved`: {events:?}"
    );
    assert!(events[0].starts_with("resolved["), "{events:?}");
    assert!(
        events[0].contains(db_path::LayerOrigin::SystemSchema.as_str())
            || events[0].contains(db_path::LayerOrigin::SystemUpdates.as_str())
            || events[0].contains(db_path::LayerOrigin::UserUpdates.as_str()),
        "the event must name the layer that was loaded: {events:?}"
    );
    // The handle is genuinely usable, not merely returned.
    assert!(
        Corrector::for_camera(&db, MAKE, MODEL, Some(LENS), 1000, 750, 18.0, 5.6, 10.0).is_some(),
        "a real profile must be reachable through the production path"
    );
    // And the same call through `resolve()` (the `stderr` default) works, so the
    // two documented entry points cannot drift apart.
    let _ = system_db();
}

/// The resolution must report *where* it loaded from and that the location
/// really holds XML profiles — not merely that some handle came back.
#[test]
fn system_database_loads_from_a_reported_location() {
    let resolved = resolve_db();
    assert!(
        !resolved.layers.is_empty(),
        "resolved dir {} produced no load plan",
        resolved.dir.display()
    );
    assert!(
        resolved.file_count() > 0,
        "resolved dir {} yielded no XML files",
        resolved.dir.display()
    );
    assert_eq!(
        resolved.schema_dir,
        resolved.dir.join(db_path::SCHEMA_SUBDIR),
        "the schema dir must be the version_1 subdir of the reported dir"
    );
    // Exactly one `version_1` layer is ever chosen (upstream newest-wins), plus
    // an optional user layer. `primary` is the single source of truth for which
    // one it is, and `layers[0]` must agree with it.
    let system_layers = resolved
        .layers
        .iter()
        .filter(|l| l.origin != LayerOrigin::UserData)
        .count();
    assert_eq!(system_layers, 1, "got {:?}", resolved.layers);
    assert_eq!(
        resolved.layers[0].origin, resolved.primary,
        "layers[0] must be the layer named by `primary`"
    );
    assert_eq!(
        resolved.pin_honored(),
        resolved.primary == LayerOrigin::SystemSchema
    );
    for layer in &resolved.layers {
        assert!(
            layer.files.iter().all(|f| f.is_file()),
            "every reported file of {} must exist on disk",
            layer.dir.display()
        );
        assert!(
            !layer.files.is_empty(),
            "empty layer {}",
            layer.dir.display()
        );
    }
    // The handle must be usable for a real lookup afterwards.
    let db = system_db();
    assert!(
        Corrector::for_camera(&db, MAKE, MODEL, Some(LENS), 1000, 750, 18.0, 5.6, 10.0).is_some()
    );
}

/// End-to-end through the real loader, **without** touching the environment:
/// the operator override must steer the *loaded content*, not merely the
/// reported path.
///
/// The override points at a copy of the **real** resolved database, so a
/// genuine profile lookup has to succeed through the resolved plan. Two
/// regressions are caught here: one that ignores the override (the `assert_eq!`
/// on `resolved.dir` already fails) and, more importantly, one that *reports*
/// the override but still loads the system directory — only the negative
/// assertion below (a Canon body that exists solely in the system database must
/// become unfindable) distinguishes those.
#[test]
fn override_directory_is_actually_loaded_through_the_real_loader() {
    let baseline = resolve_db();
    let mirror = std::env::temp_dir().join(format!(
        "lumina-lensfun-override-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let schema = mirror.join(db_path::SCHEMA_SUBDIR);
    std::fs::create_dir_all(&schema).expect("create override mirror");
    // Copy only the files the camera under test needs: the point of the test is
    // the override's effect, not re-reading 55 XML files on every run.
    let wanted: Vec<_> = baseline
        .layers
        .iter()
        .flat_map(|l| l.files.iter())
        .filter(|f| {
            let name = f.file_name().unwrap_or_default().to_string_lossy();
            name == "slr-nikon.xml" || name == "mil-nikon.xml"
        })
        .cloned()
        .collect();
    assert!(
        !wanted.is_empty(),
        "expected Nikon XML in the system database"
    );
    for file in &wanted {
        let name = file.file_name().expect("xml file name");
        std::fs::copy(file, schema.join(name)).expect("copy real database file");
    }

    // The same seam `resolve_system` uses, with the override injected.
    let resolved = db_path::resolve_with(Some(mirror.as_os_str()), None, None, None, &FsProbe)
        .expect("the override must resolve");
    assert_eq!(
        resolved.dir, mirror,
        "the override must win over every other source"
    );
    assert_eq!(resolved.source, db_path::Source::EnvOverride);
    assert_eq!(
        resolved.file_count(),
        wanted.len(),
        "only the mirrored files may be loaded, not the system database"
    );

    // …and the real loader must serve the camera from exactly those files.
    let mut diag = SilentDiagnostics;
    let db = LensfunDb::load_layers(&resolved, &mut diag).expect("the mirrored database must load");
    assert!(
        Corrector::for_camera(&db, MAKE, MODEL, Some(LENS), 1000, 750, 18.0, 5.6, 10.0).is_some(),
        "a real profile must be found through the overridden directory"
    );
    // A camera that only exists in the *system* database must NOT be found now,
    // proving the override really replaced the file set.
    assert!(Corrector::for_camera(
        &db,
        "Canon",
        "Canon EOS 5D Mark IV",
        None,
        1000,
        750,
        24.0,
        4.0,
        10.0
    )
    .is_none());
    let _ = std::fs::remove_dir_all(&mirror);
}

/// F1 on real data: when the machine has a user database, its profiles must be
/// part of the load plan. When it has none, the plan is the system layer alone —
/// either way the user layer is *decided*, never silently ignored.
#[test]
fn the_user_layer_decision_is_explicit_and_consistent_with_the_filesystem() {
    let resolved = resolve_db();
    let user_dir = user_db_dir();
    let on_disk = user_dir
        .as_deref()
        .and_then(|d| db_path::xml_files_in(d).ok().flatten())
        .is_some_and(|f| !f.is_empty());
    match resolved.user_layer() {
        Some(layer) => {
            assert!(
                on_disk,
                "a user layer was planned without a user DB on disk"
            );
            assert_eq!(layer.origin, LayerOrigin::UserData);
            assert_eq!(layer.dir, user_dir.expect("user dir must be known"));
            assert!(!layer.files.is_empty());
        }
        None => assert!(
            !on_disk,
            "a user database exists on disk but was dropped from the plan: {}",
            user_dir
                .map(|d| d.display().to_string())
                .unwrap_or_default()
        ),
    }
}

/// **NIEDRIG-5: the real-machine half of the override contract.**
///
/// The test that used to live here asserted that the constant `DB_DIR_ENV`
/// starts with `"LUMINA_"` and that a hard-coded literal is relative — it never
/// called `resolve_with`, so no production change could make it fail. That is the
/// "test that checks a constant against itself" the test policy forbids.
///
/// The hermetic counterpart (`tests::db_path::a_relative_override_is_rejected`)
/// already pins the reason. What only a machine with a **real** database can
/// show is the part that matters operationally: a relative override is a hard,
/// named error *even though a perfectly good database is right there*. Silently
/// ignoring it would hand the operator a different database than they named — the
/// exact failure class LENSFUN-DB-33 exists to prevent.
#[test]
fn a_relative_override_is_a_hard_error_despite_a_working_database() {
    let baseline = resolve_db();
    assert!(
        !baseline.layers.is_empty(),
        "this test needs a resolvable real database to mean anything"
    );

    let err = db_path::resolve_with(
        Some(OsStr::new("some/relative/lensfun")),
        None,
        None,
        None,
        &FsProbe,
    )
    .expect_err("a relative override must not silently defer to the working database");
    assert_eq!(
        err,
        db_path::SystemDbError::OverrideUnusable {
            dir: PathBuf::from("some/relative/lensfun"),
            reason: db_path::MissReason::NotADirectory,
        },
        "the error must name the override and the reason"
    );
    let text = err.to_string();
    assert!(text.contains("OverrideUnusable"), "{text}");
    assert!(
        text.contains("some/relative/lensfun"),
        "the operator must see which value was rejected: {text}"
    );

    // And the baseline is untouched: the rejected override changed nothing about
    // what an unset override resolves to.
    let without_override = resolve_db();
    assert_eq!(without_override.dir, baseline.dir);
    assert_eq!(without_override.file_count(), baseline.file_count());
}
