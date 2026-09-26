//! LENSFUN-DB-33: the **production emission path** (review finding MITTEL-3).
//!
//! Split out of `tests/plan_events.rs` (file-size ratchet, User-Vorgabe
//! 2026-09-17). That file tests the two plan events at the seam; this one drives
//! `system_load::report_with` — **the single implementation** that
//! `LensfunDb::resolve_system_with` itself calls, i.e. the code that decides what
//! a caller is told. What is left here is the **order** of the emissions; the
//! environment the production path reads, and the lock it must not run under,
//! are in [`environment`].
//!
//! # Why the tests drive `report_with` and not `resolve_system_with`
//!
//! Not because they are different code. Round 4 kept a **byte-identical second
//! copy** of the emission order inside `resolve_system_with` while the tests
//! drove `report_with`; deleting `diag.pin_displaced(...)` or the whole
//! `layer_skipped` loop from the *production* function left all 84 tests green.
//! `resolve_system_with` is now a three-line wiring of the same function, and the
//! environment values it feeds in are read by `db_path::EnvValues::read` — also
//! driven here, with a reader of our own.
//!
//! What cannot be driven is `std::env::var_os` itself: the only alternative is
//! `std::env::set_var`, which is undefined behaviour next to a concurrent
//! `getenv` in any other thread, and no test-local lock can make that safe. The
//! real environment entry point is therefore covered from the other side, by
//! `tests::system_db::the_production_entry_point_loads_and_reports_the_real_database`.

use super::diagnostics::TempTree;
use super::plan_events::FIXTURE_XML_FOR_SEAM as FIXTURE_XML;
use crate::db_layers::{LayerOrigin, SkipReason, SkippedLayer};
use crate::db_path::{EnvValues, FsProbe, DB_DIR_ENV, HOME_ENV, XDG_DATA_HOME_ENV};
use crate::system_load::Diagnostics;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The two properties of the same production path that are not about ordering.
mod environment;

/// The environment a fixture needs, built through the **production** reader
/// [`EnvValues::read`] with a stand-in for the process environment.
///
/// `override` is the `LUMINA_LENSFUN_DB` value (`None` = unset, the normal
/// case); `tree` stands in for the build-time datadir *and* for the user-data
/// base, which is what puts the fixture's `lensfun/updates/version_1` where
/// upstream would look for it. Every name is asked for through the constants, so
/// a renamed variable fails here rather than silently resolving nothing.
fn fixture_env(tree: &Path, override_dir: Option<&Path>) -> EnvValues {
    let mut env = EnvValues::read(|name| match name {
        DB_DIR_ENV => override_dir.map(|p| p.as_os_str().to_owned()),
        XDG_DATA_HOME_ENV | HOME_ENV => Some(OsString::from(tree.as_os_str())),
        other => panic!(
            "the resolution asked for an environment variable this test does not know: {other}"
        ),
    });
    // Source 2 of the resolution order is a compile-time constant in production;
    // a fixture overrides it to aim the *system database* at the temp tree.
    env.compiled = Some(OsString::from(tree.as_os_str()));
    env
}

struct StrictRecorder {
    events: Arc<Mutex<Vec<String>>>,
    lock_seen_held: Arc<Mutex<Vec<bool>>>,
}

impl StrictRecorder {
    fn new() -> Self {
        Self {
            events: Arc::default(),
            lock_seen_held: Arc::default(),
        }
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().expect("recorder lock").clone()
    }

    fn any_sink_saw_the_lock_held(&self) -> bool {
        self.lock_seen_held
            .lock()
            .expect("recorder lock")
            .iter()
            .any(|held| *held)
    }

    fn note(&mut self, label: String) {
        self.lock_seen_held
            .lock()
            .expect("recorder lock")
            .push(crate::system_load::load_layers_holds_the_lock());
        self.events.lock().expect("recorder lock").push(label);
    }
}

impl Diagnostics for StrictRecorder {
    fn resolved(&mut self, resolved: &crate::db_path::Resolved) {
        self.note(format!("resolved[{}]", resolved.primary.as_str()));
    }

    fn file_rejected(&mut self, _layer_dir: &std::path::Path, file: &std::path::Path) {
        self.note(format!("rejected[{}]", file.display()));
    }

    fn layer_skipped(&mut self, skipped: &SkippedLayer) {
        self.note(format!("skipped[{}]", skipped.reason));
    }

    fn pin_displaced(&mut self, resolved: &crate::db_path::Resolved) {
        self.note(format!("pin_displaced[{}]", resolved.primary.as_str()));
    }

    fn failed(&mut self, err: &crate::db_path::SystemDbError) {
        self.note(format!("failed[{}]", err));
    }
}

/// **MITTEL-3: deleting either loud emission must fail this test — in the code
/// production runs.**
///
/// `report_with` is the *only* implementation of the emission order;
/// `LensfunDb::resolve_system_with` is a three-line wiring that calls it. The
/// fixture is a real system database plus a real user *update package* that
/// legitimately wins the competition (a genuinely newer `timestamp.txt`), so the
/// loaded set differs from the resolved one — the measured F1-2 shape. Asserting
/// the exact event sequence covers both emissions and their documented ordering
/// at once.
#[test]
fn production_emits_pin_displaced_and_layer_skipped_before_the_load() {
    let tree = TempTree::new("prodpin");
    tree.write("a.xml", FIXTURE_XML);
    // A user update package that genuinely out-dates the system database.
    let updates = tree.0.join("lensfun/updates/version_1");
    std::fs::create_dir_all(&updates).expect("create updates dir");
    std::fs::write(updates.join("u.xml"), FIXTURE_XML).expect("write update xml");
    // One file the load will reject, so the sequence also contains a
    // **load-time** event. Without it, "the deviations came before the load"
    // would be indistinguishable from "the deviations came before the outcome":
    // both stay true if `pin_displaced` is moved behind `load_layers`.
    std::fs::write(updates.join("junk.xml"), "not xml at all\n").expect("write junk xml");
    std::fs::write(
        updates.join(crate::db_timestamp::TIMESTAMP_FILE),
        "2000000000\n",
    )
    .expect("write stamp");
    // …and a system update directory that exists but cannot compete, so
    // `layer_skipped` has a real reason to report.
    let sys_updates = std::path::Path::new(crate::db_path::SYSTEM_UPDATES_DIR);
    if sys_updates.exists() {
        eprintln!(
            "note: {} exists on this machine, so the `layer_skipped` assertion \
             is covered by the user-data skip instead",
            sys_updates.display()
        );
    }

    // Path arithmetic, spelled out because both halves matter:
    // `compiled` is a *datadir*, and `lensfun_dir_in_datadir` appends `/lensfun`
    // to it, so the system database lands in `tree/lensfun/version_1`. `xdg` is
    // the *user data dir's* base, so passing `tree` puts `HomeDataDir` at the
    // same `tree/lensfun` and the user-update package at
    // `tree/lensfun/updates/version_1` — exactly where the fixture wrote it.
    let mut sink = StrictRecorder::new();
    // No `LUMINA_LENSFUN_DB`: an override *pins* the system layer by design (see
    // `db_layers`), so it could never produce a displacement. The compiled
    // datadir is the upstream-parity case where an update package may win.
    let outcome = crate::system_load::report_with(&fixture_env(&tree.0, None), &FsProbe, &mut sink);

    assert!(outcome.is_ok(), "the plan must load: {outcome:?}");
    let events = sink.events();
    assert!(
        !sink.any_sink_saw_the_lock_held(),
        "a caller-supplied sink must never run under the non-reentrant global \
         lock; events: {events:?}"
    );

    // The displacement of the resolved directory is reported…
    let displaced = events
        .iter()
        .position(|e| e.starts_with("pin_displaced"))
        .unwrap_or_else(|| {
            panic!(
                "the loaded set diverges from the resolved one, so \
                    `pin_displaced` must be emitted: {events:?}"
            )
        });
    // …and it is reported for the directory that actually won.
    assert!(
        events[displaced].contains("Benutzer-Update-Paket"),
        "the event must name the layer that won: {events:?}"
    );

    // The documented ordering, in its strong form: the deviations precede **the
    // load itself**, not merely the outcome event. `rejected` is emitted from
    // inside `load_layers`, so `pin_displaced < rejected` is what distinguishes
    // "emitted before the load" from "emitted after it but before `resolved`" —
    // a reordering behind `load_layers` keeps the weaker assertion true.
    let first_rejected = events
        .iter()
        .position(|e| e.starts_with("rejected"))
        .unwrap_or_else(|| panic!("the fixture's junk file must be reported: {events:?}"));
    assert!(
        displaced < first_rejected,
        "a displaced pin must be reported before the load runs, not after: {events:?}"
    );
    let resolved_at = events
        .iter()
        .position(|e| e.starts_with("resolved"))
        .unwrap_or_else(|| panic!("the success path must be reported: {events:?}"));
    assert!(
        first_rejected < resolved_at,
        "the load's own rejections precede its outcome: {events:?}"
    );
    assert!(
        displaced < resolved_at,
        "deviations must precede the outcome: {events:?}"
    );
    // The same holds for the other deviation: a *reported* layer skip belongs to
    // the plan, so it is emitted before the load and not after it.
    let first_skipped = events
        .iter()
        .position(|e| e.starts_with("skipped"))
        .unwrap_or_else(|| {
            panic!("the fixture's XML-less user database must be reported: {events:?}")
        });
    assert!(
        first_skipped < first_rejected,
        "a reported layer skip must precede the load's own events, not follow \
         them: {events:?}"
    );

    // The skipped layers reach the sink too — here the user data dir is present,
    // so the reportable reason is the system-update directory being absent or
    // undated. Either way at least one `skipped` must be present, because the
    // plan always considers both update directories.
    assert!(
        events.iter().any(|e| e.starts_with("skipped")),
        "the update directories are always considered, so a skip must be \
         reported: {events:?}"
    );
}

/// **Why the deviations are emitted *before* the load: a displaced pin has to
/// stay visible when the load then fails.** This is the documented rationale, and
/// it is the half of the ordering that the success-path test above cannot reach:
/// there, moving `pin_displaced` behind `load_layers` still leaves it in front of
/// the outcome event.
///
/// Fixture: the user update package genuinely wins (a newer `timestamp.txt`) and
/// its only profile file is corrupt, so the plan loads it and *nothing* loads —
/// a hard `AllFilesRejected`. The system database is not loaded at all (it lost
/// the competition), so exactly one file is rejected and the sequence is
/// unambiguous: `pin_displaced`, then that rejection, then `failed`.
#[test]
fn a_displaced_pin_is_reported_even_when_the_load_then_fails() {
    let tree = TempTree::new("prodfail");
    tree.write("a.xml", FIXTURE_XML);
    let updates = tree.0.join("lensfun/updates/version_1");
    std::fs::create_dir_all(&updates).expect("create updates dir");
    std::fs::write(updates.join("u.xml"), "not xml at all\n").expect("write junk xml");
    std::fs::write(
        updates.join(crate::db_timestamp::TIMESTAMP_FILE),
        "2000000000\n",
    )
    .expect("write stamp");

    let mut sink = StrictRecorder::new();
    let outcome = crate::system_load::report_with(&fixture_env(&tree.0, None), &FsProbe, &mut sink);
    let events = sink.events();
    assert!(outcome.is_err(), "nothing loadable must be a hard error");
    assert!(
        events.iter().any(|e| e.starts_with("pin_displaced")),
        "the resolved directory is not the one that was loaded, and the caller \
         must learn that even though the load failed: {events:?}"
    );
    assert!(
        events.iter().any(|e| e.starts_with("failed")),
        "the failure must be reported: {events:?}"
    );
    assert!(
        !events.iter().any(|e| e.starts_with("resolved")),
        "a failed load must never be reported as resolved: {events:?}"
    );
    let at = |prefix: &str| {
        events
            .iter()
            .position(|e| e.starts_with(prefix))
            .unwrap_or_else(|| panic!("{prefix} must be reported: {events:?}"))
    };
    // The user data dir exists but holds no `*.xml` (only the `updates`
    // subdirectory), so there is a reportable layer skip as well — and it must
    // be reported *before* the load, exactly like the displacement.
    assert!(
        at("skipped") < at("rejected"),
        "a reportable layer skip must precede the load: {events:?}"
    );
    assert!(
        at("pin_displaced") < at("rejected") && at("rejected") < at("failed"),
        "the documented order is deviations, then the load's own rejections, \
         then the outcome: {events:?}"
    );
    assert!(!sink.any_sink_saw_the_lock_held());
}

/// **MITTEL-3, second half: `layer_skipped` needs a fixture with an ACTIONABLE
/// skip.** The displacement test above does *not* cover it: on a normal machine
/// both update directories are simply absent, and `Absent` is deliberately not
/// reported (it is the normal case). Deleting the whole `layer_skipped` loop
/// therefore left the suite green.
///
/// Here the user *update package exists but holds no XML* — a real, actionable
/// defect (`NoXmlFiles`) that must be reported by name.
#[test]
fn production_emits_an_actionable_layer_skip() {
    let tree = TempTree::new("prodskip");
    tree.write("a.xml", FIXTURE_XML);
    // One corrupt file in the system layer, so this fixture also exercises a
    // **load-time** `file_rejected` event: the counter assertion at the end
    // must observe that event too, not only the pre-load ones.
    tree.write("junk.xml", "not xml at all\n");
    // A real timestamp.txt for the system database, so it wins on merit.
    std::fs::write(
        tree.0
            .join(crate::db_path::SCHEMA_SUBDIR)
            .join(crate::db_timestamp::TIMESTAMP_FILE),
        "1645386247\n",
    )
    .expect("write stamp");
    // The user update package exists and is non-empty as a *directory*, but has
    // no `*.xml` — exactly `SkipReason::NoXmlFiles`.
    let updates = tree.0.join("lensfun/updates/version_1");
    std::fs::create_dir_all(&updates).expect("create updates dir");
    std::fs::write(updates.join("notes.txt"), "no profiles here").expect("write notes");

    let mut sink = StrictRecorder::new();
    let outcome = crate::system_load::report_with(&fixture_env(&tree.0, None), &FsProbe, &mut sink);
    assert!(
        outcome.is_ok(),
        "the system database must still load: {outcome:?}"
    );

    let events = sink.events();
    let skipped: Vec<&String> = events.iter().filter(|e| e.starts_with("skipped")).collect();
    // Two XML-less directories here: the user update package and the user
    // database itself. Both are real defects, both must be reported.
    assert_eq!(
        skipped.len(),
        2,
        "both XML-less layers must be reported: {events:?}"
    );
    for event in &skipped {
        assert!(
            event.contains("NoXmlFiles"),
            "each report must name its reason: {event:?}"
        );
        assert!(
            !event.contains("Absent"),
            "a non-actionable `Absent` must never be reported: {event:?}"
        );
    }
    assert!(
        events.iter().any(|e| e.starts_with("resolved")),
        "the system database must still be loaded and reported: {events:?}"
    );
    assert!(!sink.any_sink_saw_the_lock_held());
}

/// The other half of the contract: an update directory that is merely **absent**
/// is the normal case and must stay silent, or the sink would spam every lookup.
/// The user data dir here exists and is a valid database, and there is no user
/// update package at all.
#[test]
fn production_does_not_report_an_absent_update_directory() {
    let tree = TempTree::new("prodabsent");
    tree.write("a.xml", FIXTURE_XML);
    std::fs::write(
        tree.0
            .join(crate::db_path::SCHEMA_SUBDIR)
            .join(crate::db_timestamp::TIMESTAMP_FILE),
        "1645386247\n",
    )
    .expect("write stamp");
    let user = tree.0.join("lensfun");
    std::fs::create_dir_all(&user).expect("create user dir");
    std::fs::write(user.join("u.xml"), FIXTURE_XML).expect("write user xml");

    let mut sink = StrictRecorder::new();
    let outcome = crate::system_load::report_with(&fixture_env(&tree.0, None), &FsProbe, &mut sink);
    assert!(outcome.is_ok(), "{outcome:?}");
    let events = sink.events();
    assert!(
        !events.iter().any(|e| e.starts_with("skipped")),
        "an absent update directory is the normal case and must not be \
         reported: {events:?}"
    );
    assert!(
        events.iter().any(|e| e.starts_with("resolved")),
        "the success path must still be reported: {events:?}"
    );
}

/// The failure path of the same production seam: a broken override must reach
/// the sink **and** the `Result`, so a caller that only logs the event and one
/// that only checks the return value see the same thing.
#[test]
fn production_reports_a_broken_override_through_the_sink_and_the_result() {
    let mut sink = StrictRecorder::new();
    let broken = PathBuf::from("/definitely/not/a/lensfun/db");
    let err = crate::system_load::report_with(
        &fixture_env(Path::new("/nonexistent-fixture-root"), Some(&broken)),
        &FsProbe,
        &mut sink,
    )
    .expect_err("a broken override must be a hard error");
    let events = sink.events();
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(events[0].starts_with("failed"), "{events:?}");
    assert!(events[0].contains("OverrideUnusable"), "{events:?}");
    assert_eq!(
        err,
        crate::db_path::SystemDbError::OverrideUnusable {
            dir: PathBuf::from("/definitely/not/a/lensfun/db"),
            reason: crate::db_path::MissReason::Absent,
        }
    );
    assert!(!sink.any_sink_saw_the_lock_held());
}

/// `ReportOnce` is the sink the SOLL hands to callers, and it was instantiated
/// nowhere in the repository — so the "construct once, keep it alive" obligation
/// was untested. Drive it through the production seam and assert it collapses
/// repeats while still reporting a *different* later failure.
#[test]
fn report_once_is_a_usable_sink_for_the_production_seam() {
    let tree = TempTree::new("reportonce");
    tree.write("a.xml", FIXTURE_XML);
    let mut once = crate::db_sinks::ReportOnce::new();
    for _ in 0..5 {
        let outcome = crate::system_load::report_with(
            &fixture_env(&tree.0, Some(&tree.0)),
            &FsProbe,
            &mut once,
        );
        assert!(outcome.is_ok(), "{outcome:?}");
    }
    {
        let seen = once.seen.lock().expect("lock");
        assert_eq!(
            seen.len(),
            1,
            "five identical loads must collapse to one report: {seen:?}"
        );
        assert!(
            seen.iter().any(|k| k.starts_with("resolved:")),
            "the success path must be the reported event: {seen:?}"
        );
    }
    // A genuinely different event is still reportable afterwards, so a
    // de-duplicating sink can never permanently hide a real problem.
    Diagnostics::layer_skipped(
        &mut once,
        &SkippedLayer {
            dir: Some(PathBuf::from("/x")),
            origin: LayerOrigin::UserUpdates,
            reason: SkipReason::Unreadable,
        },
    );
    assert_eq!(
        once.seen.lock().expect("lock").len(),
        2,
        "a new event must not be suppressed"
    );
}
