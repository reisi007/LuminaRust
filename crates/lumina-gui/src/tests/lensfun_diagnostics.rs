//! LENSFUN-CALLER-37 (GUI half): the Lensfun diagnostics sink's three contract
//! clauses.
//!
//! 1. an unresolvable database is reported **once per process**, not once per
//!    render;
//! 2. the *database load* runs again on every rebuild — including a rebuild
//!    with an **unchanged** key after a miss — so a database installed later is
//!    picked up instead of being pinned as a stale miss;
//! 3. every event reaches the log at a **defined level**, not as bare text.
//!
//! # Coverage, and what is machine-dependent
//!
//! Clauses 1 and 3 are pinned **machine-independently** against a locally
//! constructed sink, because they are about what the sink does with an event —
//! no filesystem and no installed database involved.
//!
//! Clause 2 needs the *production* call site, and there the branch that runs
//! depends on the host: a machine with a system Lensfun database takes the
//! `resolved`/`debug` path, one without takes `failed`/`error`. The production
//! test therefore asserts the branch-independent facts (the load was entered
//! again, the cache stayed empty, no duplicate record) and says so. It does
//! **not** pretend to cover the DB-miss branch; that is covered above.
//!
//! # What clause 2 does **not** pin
//!
//! The attempt counter counts **database loads**, not profile searches. A memo
//! between the load and `for_camera` — the natural place to memoise "this
//! camera has no profile (yet)" — would leave this cache correct, would keep the
//! counter moving and would add no log record, so no assertion here could see
//! it. The production call site has no such memo, but that placement is an
//! untested gap rather than a guarantee; `lensfun_auto`'s module doc states the
//! same limit from the code side.
//!
//! What the counter alone does **not** pin is the load itself: the add sits
//! *beside* `load_system_with`, so a memo that returns before the FFI call while
//! the counter keeps advancing satisfies it. Measured 2026-09-27, not assumed:
//! that mutation left the whole `lumina-gui` lib suite green (903 passed; the
//! filtered module was 3/3 at the time of that measurement). So no rebuild here
//! is judged by the counter alone — every one
//! asserts the **pair**: the call-site counter *and*
//! [`lensfun_diag::lensfun_load_outcomes`], the count the load itself produces.
//! The second half cannot be faked by a caller-side memo; the first keeps the
//! counter wired to the call site at all.
//!
//! With that pair, the add's placement is load-bearing as well: with the add
//! *after* the closure, an "already asked" memo written into the sink's own
//! `seen` set runs this whole test green, because it freezes the outcome count
//! too. The measured per-mutation evidence is on the production test below. The
//! two outcome branches are pinned apart from each other by
//! `a_reported_miss_advances_the_load_count_and_a_deviation_does_not`, so the
//! binding does not depend on whether the host has a database.
//!
//! # How the log is observed
//!
//! Through the real `log` facade: one capture logger, installed once (see
//! `crate::tests::support` — `log` accepts a single logger per process), with
//! per-thread buckets so parallel tests cannot read each other's records.
//! Nothing here re-implements the sink's logic to test it — every assertion is
//! on records the production code emitted.
//!
//! # Why no environment variable is set to force a failure
//!
//! `std::env::set_var` is unsound next to a concurrent `getenv` in another
//! thread, which no test-local lock can repair — the same reasoning
//! `lumina-lensfun`'s own `tests/plan_events.rs` records. The DB-miss branch is
//! therefore reached by constructing the event, not by breaking the host.

use super::*;
use crate::lensfun_auto::lensfun_lookup_attempts;
use crate::lensfun_diag::{lensfun_load_outcomes, LogDiagnostics};
use crate::tests::support::{captured_logs, clear_captured_logs};
use crate::LensIdentity;
// The trait and the payload types come from the crate under test, not through
// `lensfun_diag`'s own imports: the sink module's imports stay private.
use log::Level;
use lumina_lensfun::db_layers::{Layer, LayerOrigin, SkipReason, SkippedLayer};
use lumina_lensfun::db_path::{MissReason, ProbeMiss, Resolved, Source, SystemDbError};
use lumina_lensfun::system_load::Diagnostics;

/// The sink's records on this thread, as `(level, message)`.
///
/// Through the shared capture logger in `crate::tests::support` (see the note
/// there: `log` allows one logger per process, so there is exactly one
/// installer and both capturing test modules read from it). Bucket scope is the
/// **thread**, because a sink is called synchronously from the thread that
/// drives the lookup — so "this test's records" is exact and the three tests
/// stay free to run in parallel.
fn sink_records() -> Vec<(Level, String)> {
    captured_logs()
        .into_iter()
        .filter_map(|line| {
            // Shared format: `"<LEVEL>: <message>"`.
            let (level, message) = line.split_once(": ")?;
            let level = level.parse::<Level>().ok()?;
            (level == Level::Error
                || level == Level::Warn
                || level == Level::Debug
                || level == Level::Info)
                .then(|| (level, message.to_string()))
        })
        .filter(|(_, message)| !message.is_empty())
        .collect()
}

/// The captured records at `level`, in order.
fn records_at(level: Level) -> Vec<String> {
    sink_records()
        .into_iter()
        .filter(|(seen, _)| *seen == level)
        .map(|(_, message)| message)
        .collect()
}

/// Records shortened for an assertion message: a Lensfun error text is a
/// multi-line paragraph (every probed location plus the remedy), so a
/// failure that dumps 100 of them is unreadable.
fn summarise(records: &[String]) -> Vec<String> {
    const SHOWN: usize = 3;
    let mut out: Vec<String> = records
        .iter()
        .take(SHOWN)
        .map(|message| {
            let line = message.lines().next().unwrap_or_default();
            if line.chars().count() > 90 {
                format!("{}…", line.chars().take(90).collect::<String>())
            } else {
                line.to_string()
            }
        })
        .collect();
    if records.len() > SHOWN {
        out.push(format!("… and {} more", records.len() - SHOWN));
    }
    out
}

/// A `SystemDbError` whose text is unique per `name`, so two failures never
/// share a de-duplication key across tests.
fn not_found(name: &str) -> SystemDbError {
    SystemDbError::NotFound {
        misses: vec![ProbeMiss {
            dir: std::path::PathBuf::from(format!("/nonexistent/{name}")),
            source: Source::PlatformDefault,
            reason: MissReason::Absent,
        }],
    }
}

/// **Clause 2, machine-independent half:** the *miss* outcome feeds the load
/// count, and only the outcome events do.
///
/// The production test reaches `resolved` on a host **with** a system database
/// and `failed` on one **without**, so on any single machine just one outcome
/// branch runs — and the miss branch is the one this cache contract is about.
/// Deleting the increment from `failed` is therefore invisible to the production
/// test here, so that half is pinned where no database is involved.
#[test]
fn a_reported_miss_advances_the_load_count_and_a_deviation_does_not() {
    let mut sink = LogDiagnostics::new();
    let before = lensfun_load_outcomes();
    sink.failed(&not_found("outcome-count"));
    assert_eq!(
        lensfun_load_outcomes(),
        before + 1,
        "a load whose outcome is a miss must advance the count exactly like a \
         resolved one: on a host without a system database it is the only \
         branch the count ever takes"
    );
    // The same miss again: de-duplicated away in the log, still a performed load.
    sink.failed(&not_found("outcome-count"));
    assert_eq!(
        lensfun_load_outcomes(),
        before + 2,
        "de-duplicating the record must not de-duplicate the load: a second miss \
         is a second load, and the whole contract is about the second one"
    );
    // A deviation is reported *around* a load, not as its outcome: counting it
    // would make the count stop meaning "a load finished".
    let dir = std::path::PathBuf::from("/nonexistent/lensfun-outcome-count");
    sink.layer_skipped(&SkippedLayer {
        dir: Some(dir),
        origin: LayerOrigin::UserData,
        reason: SkipReason::Unreadable,
    });
    assert_eq!(
        lensfun_load_outcomes(),
        before + 2,
        "a skipped layer is not an outcome, or the production test's exact +1 \
         per rebuild would stop being exact"
    );
}

/// **Clause 1 + 3:** 100 identical failures produce exactly one `error`
/// record, and a *different* failure is still reported afterwards.
#[test]
fn an_identical_failure_is_reported_once_per_session() {
    clear_captured_logs();
    let mut sink = LogDiagnostics::new();
    let first = not_found("dedup-alpha");
    for _ in 0..100 {
        sink.failed(&first);
    }
    let errors = records_at(Level::Error);
    assert_eq!(
        errors.len(),
        1,
        "100 identical failures must collapse into one error record, got {:?}",
        summarise(&errors)
    );
    assert!(
        errors[0].contains("no system Lensfun database"),
        "the record must name the consequence, got {}",
        summarise(&errors).first().cloned().unwrap_or_default()
    );

    // A later, genuinely different failure is **not** suppressed: the key is
    // the error identity, not a "reported once ever" flag. This is what lets
    // a database installed (or removed) later be reported at all.
    sink.failed(&not_found("dedup-beta"));
    assert_eq!(
        records_at(Level::Error).len(),
        2,
        "a different failure must still be reported"
    );
}

/// **Clause 3:** every event reaches the log at its documented level, and
/// the *expected* case is not louder than `debug`.
#[test]
fn every_lensfun_event_is_logged_at_its_documented_level() {
    clear_captured_logs();
    let mut sink = LogDiagnostics::new();
    let dir = std::path::PathBuf::from("/nonexistent/lensfun");
    let layer = Layer {
        dir: dir.clone(),
        origin: LayerOrigin::SystemSchema,
        files: vec![dir.join("version_1")],
    };
    let resolved = Resolved {
        dir: dir.clone(),
        schema_dir: dir.join("version_1"),
        source: Source::PlatformDefault,
        primary: LayerOrigin::SystemSchema,
        layers: vec![layer],
        skipped: Vec::new(),
    };
    let skipped = SkippedLayer {
        dir: Some(dir.clone()),
        origin: lumina_lensfun::db_layers::LayerOrigin::UserData,
        reason: SkipReason::Unreadable,
    };
    sink.resolved(&resolved);
    sink.file_rejected(&dir, &dir.join("broken.xml"));
    sink.layer_skipped(&skipped);
    sink.pin_displaced(&resolved);
    sink.failed(&not_found("levels"));
    // The levels are the contract; the exact wording is not.
    assert_eq!(
        records_at(Level::Debug).len(),
        1,
        "a resolved database is the expected case: debug, not info"
    );
    assert_eq!(
        records_at(Level::Warn).len(),
        3,
        "file_rejected, layer_skipped and pin_displaced are actionable: warn"
    );
    assert_eq!(
        records_at(Level::Error).len(),
        1,
        "a missing database is a hard failure: error"
    );
    assert_eq!(
        records_at(Level::Info).len(),
        0,
        "no Lensfun event may be logged at info: a per-render lookup would \
         then spam the default level"
    );
}

/// **Clause 2 + the call site:** the production lookup routes through the
/// app's log facade — never through `stderr` — and a miss on the *database
/// load* caches nothing, so the next rebuild retries instead of pinning a stale
/// miss across a database install.
///
/// # Why the attempt counter, and not just "no new record"
///
/// The obvious assertion — "the second rebuild adds no second record" — cannot
/// distinguish a *retried* lookup from an *abandoned* one: both leave the
/// record count unchanged. A de-duplication sink that accidentally **gated**
/// the call would pass that test while pinning a stale miss for the whole
/// session, which is the precise regression this clause exists to prevent. So
/// the production call site counts its attempts
/// ([`lensfun_auto::lensfun_lookup_attempts`]) and this test watches the
/// counter, not the log — and, because that counter is written by the *caller*
/// and could not be shown to witness the load, the count the load itself
/// produces ([`lensfun_diag::lensfun_load_outcomes`]) as well. Both, per rebuild.
///
/// # Non-vacuity: the memos this test actually kills
///
/// Every rebuild is judged by two numbers, not one: the **call-site** counter
/// ([`lensfun_lookup_attempts`], incremented beside the call) and the **load's own**
/// outcome count ([`lensfun_load_outcomes`], incremented by the sink methods that
/// `lumina_lensfun::system_load::report_with` calls once per load). The memos
/// below were applied to the production call site, run, and observed to fail
/// here; each was then reverted. The first four carry the earlier commit's
/// measurement — row 1 was spot-checked again on 2026-09-27 and still goes red
/// at the counter assertion named in the table; the fifth was measured for this
/// binding (2026-09-27):
///
/// | memo | first red assertion |
/// |---|---|
/// | inside the closure, **unkeyed** (the sink's own `seen` set) | "a second rebuild with a changed key …" |
/// | inside the closure, **keyed on the cache key** | "an unchanged key after a miss …" |
/// | in front of `with_diagnostics`, **keyed on the cache key** | "an unchanged key after a miss …" |
/// | in front of `with_diagnostics`, **unkeyed** | "a second rebuild with a changed key …" |
/// | **skips the FFI call, attempt counter keeps advancing** | "the second rebuild has to produce a **load outcome** …" |
///
/// The third rebuild earns its keep: a memo *keyed on the cache key* cannot fire
/// on the first two calls (they change the key), so only the repeat of the same
/// key catches it. An unkeyed memo needs no such help — it fails one call
/// earlier.
///
/// The first row is also the one the previous production layout let through:
/// with the add *after* the closure, the unkeyed in-closure memo runs this test
/// **green** (measured, same machine, same commit's test file). That is the
/// regression the current layout closes.
///
/// The **last** row is the one a second observable had to be added for, and it is
/// the only one of the five no single counter can see: by construction the
/// counter keeps advancing. With the counter as the only witness it left the
/// whole lib suite green (903 passed) — the same regression the rows above
/// close, reopened. The load's own outcome
/// count is what makes it visible; `lensfun_auto`'s `LOOKUP_ATTEMPTS` doc
/// records that measurement where the claim used to stand.
///
/// What stays green, and is therefore an untested gap rather than a covered
/// claim: a memo between the load and `for_camera` (see the module doc), and a
/// memo *below* the sink, inside `lumina_lensfun::system_load::report_with`.
#[test]
fn the_production_lookup_logs_through_the_facade_and_caches_no_miss() {
    clear_captured_logs();
    // `LuminaApp::new` on a plain context — the same one-liner the shared
    // `tests::support::new_app()` helper uses. Duplicated rather than
    // widening that helper's `pub(super)` visibility across modules.
    let mut app = LuminaApp::new(egui::Context::default());
    // A camera that cannot match any profile, so `for_camera` returns
    // `None` and the miss path is taken on every machine.
    app.loaded_lens_identity = Some(LensIdentity {
        camera_make: Some("LuminaRust".to_string()),
        camera_model: Some("NoSuchCamera".to_string()),
        lens: Some("no-such-lens".to_string()),
        focal_length: Some(50.0),
        aperture: Some(4.0),
    });
    // Read the counter once before the first call so that "the counter is wired
    // to the lookup at all" is a *separately* reported fact. It is defence in
    // depth, not the load-bearing check: with the counter frozen at zero the
    // second assertion below is still red (`0 > 0` is false), which a mutation
    // confirmed.
    let before_any_lookup = lensfun_lookup_attempts();
    let outcomes_before_first = lensfun_load_outcomes();
    app.ensure_lensfun_cache(64, 48);
    let after_first = records_at(Level::Error).len() + records_at(Level::Debug).len();
    // Which branch this takes depends on the **machine**: a host with a system
    // Lensfun database reports `resolved` at `debug`, a host without one
    // reports `failed` at `error`. Both are legitimate, so the assertion is "a
    // record arrived at a real level" — not "the error arrived". Pinning the
    // error specifically would make this test vacuous on any developer machine
    // that has lensfun installed (this one does), which is exactly how a broken
    // error path would stay green here forever. The error branch is covered
    // machine-independently by the two tests above.
    assert!(
        after_first >= 1,
        "the lookup must report through the log facade, not stderr: a \
         Dock-launched app has no stderr, so an unrecorded outcome is invisible"
    );
    let after_first_attempts = lensfun_lookup_attempts();
    assert!(
        after_first_attempts > before_any_lookup,
        "the first rebuild must have entered the lookup, otherwise the \
         assertion below would be vacuous"
    );
    // The second half of the pair. `lensfun_load_outcomes` is written by the
    // **sink**, from the two callbacks `report_with` makes exactly once per
    // completed load, so it cannot be moved by anything the caller does. One
    // rebuild → exactly one outcome: not "at least one", because a second
    // outcome would mean the sink was driven by something that is not this
    // rebuild. It is per thread, so a parallel test's real load cannot stand in
    // for this one.
    assert_eq!(
        lensfun_load_outcomes(),
        outcomes_before_first + 1,
        "the first rebuild must have loaded the database, not merely reached the \
         call site: the sink reports one outcome per load, and this one is what \
         makes the assertions below non-vacuous"
    );

    // Second rebuild with **different dimensions**, so the key changes and the
    // `!fresh` early-out cannot swallow the call.
    let outcomes_before_second = lensfun_load_outcomes();
    app.ensure_lensfun_cache(65, 49);
    let after_second = records_at(Level::Error).len() + records_at(Level::Debug).len();
    // The *decisive* clause: the second rebuild really ran the load again.
    // Asserting only "no new record" cannot tell a retried lookup from an
    // abandoned one — both leave the record count unchanged — so a
    // de-duplicating sink that accidentally **gated** the call, or an "already
    // tried" memo in front of the load, would satisfy such a test while pinning
    // a stale miss for the rest of the session. The counter, which the
    // production call site increments *inside* the load's closure and right
    // after the load returns, separates the memos that skip the counting
    // together with the load; the one that only keeps the counter **moving** is
    // caught by the outcome assertion right below, not by this one.
    //
    // `>` rather than `== +1`. As of this writing no other test both sets
    // `loaded_lens_identity` and drives a render, so an exact `+1` would be
    // deterministic today — but the counter is process-wide, and a future test
    // rendering a RAW with EXIF would make `== +1` flaky. Freezing is the
    // regression, and freezing fails this.
    assert!(
        lensfun_lookup_attempts() > after_first_attempts,
        "a second rebuild with a changed key must run the lookup again: a miss \
         must cache nothing, or a database installed later would stay invisible \
         for the rest of the session"
    );
    // The decisive assertion of the pair, and the reason the outcome count
    // exists. The counter above cannot tell this mutation from a real reload: a
    // memo in front of `load_system_with` that keeps the `fetch_add` advancing
    // satisfies it. The outcome count cannot be satisfied that way — the sink
    // only hears from a load that actually ran. Measured: with the counter as
    // the only witness, that mutation kept this module green (3 passed) and the
    // whole lib suite green (903 passed).
    assert_eq!(
        lensfun_load_outcomes(),
        outcomes_before_second + 1,
        "the second rebuild has to produce a **load outcome** in the sink, not \
         just an attempt: a memo that skips the FFI call while the attempt \
         counter keeps advancing would pin a stale miss for the whole session, \
         and `report_with` emits an outcome only for a load it really performed"
    );
    assert!(
        app.lensfun_cache.is_none(),
        "a miss must leave the cache empty, never a placeholder entry"
    );
    assert_eq!(
        after_second, after_first,
        "a repeated identical outcome must not add a second record — the \
         reporting is de-duplicated even though the lookup did run again"
    );

    // Third rebuild with the **same** dimensions as the second. This is the
    // case a key-scoped memo would hide: the first two calls changed the key, so
    // a memo keyed on the cache key never fires and the assertion above cannot
    // see it. Repeating a key is exactly the real-world shape — the user keeps
    // previewing the same photo at the same size — and it is the shape that
    // decides whether a database installed *later* is ever noticed. Because the
    // miss left `lensfun_cache` empty, `fresh` is still true here, so the load
    // must run again. A memo inside the closure that is keyed on the cache key
    // fails here for the same reason; the unkeyed variant needs no such help and
    // already fails one call earlier.
    let after_second_attempts = lensfun_lookup_attempts();
    let outcomes_before_third = lensfun_load_outcomes();
    app.ensure_lensfun_cache(65, 49);
    assert!(
        lensfun_lookup_attempts() > after_second_attempts,
        "rebuilding with an unchanged key after a miss must still run the \
         lookup: nothing is cached, so an 'already tried' memo keyed on the \
         cache key would pin the miss and hide a database installed later"
    );
    // Same pairing as above, on the key repeat: the counter proves the call site
    // ran, the outcome count proves the load did.
    assert_eq!(
        lensfun_load_outcomes(),
        outcomes_before_third + 1,
        "rebuilding with an unchanged key after a miss must still load the \
         database: no outcome means no load, so a database installed later would \
         stay invisible"
    );
    assert!(
        app.lensfun_cache.is_none(),
        "a miss must leave the cache empty, never a placeholder entry"
    );
    assert_eq!(
        records_at(Level::Error).len() + records_at(Level::Debug).len(),
        after_first,
        "the third lookup produced the same outcome and must be de-duplicated \
         like the second"
    );
}
