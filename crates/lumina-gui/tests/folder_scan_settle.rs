//! KITT-SETTLE-UNIFY-53: the folder-scan wait must mean something, and it must
//! not be able to pass silently.
//!
//! Three claims, one target, **no GPU required** — so unlike the wgpu goldens
//! (all `#[ignore]`d, verified nowhere in CI) this runs in the normal test run:
//!
//! 1. **A settled decode is not a settled folder scan.** `is_settled` (the
//!    file-open wait's exit) and `scan_and_decode_settled` (this wait's ready
//!    state) are different conditions, and production reaches a state where the
//!    first holds and the second does not: `LuminaApp::open_file` navigates
//!    *before* it decodes. Routing this wait through the settled exit would
//!    return in that window, before the listing lands, and the golden would
//!    capture an empty grid.
//! 2. **The wait is not a no-op.** It returns only once the listing is applied
//!    *and* the auto-load decode that listing starts has finished.
//! 3. **It cannot pass silently.** An unfulfillable condition is *reported* —
//!    cause, expectation, five found state values, status line, error banner —
//!    rather than returning as if it had settled. That is the defect this task
//!    removes: `kittest_crop_overlay` exhausted a 500-frame budget, returned
//!    normally, and the golden was then taken from an unsettled state with the
//!    test green.
//!
//! **Every fixture below is non-empty, on purpose.** `tests/fixtures/library`
//! is empty and its scan settles within a few frames, so a proof measured against
//! it is close to a tautology: it cannot tell a wait that works from a wait that
//! does nothing. The difference is real and reproducible **in its order of
//! magnitude**, not in its absolute value. Same code, same wait, same directory
//! path (macOS/Metal, 2026-09-28, two independent runs): the **empty directory
//! settles in 3-4 frames**; a directory with **staged 24-megapixel CR3s settles
//! in the hundreds of frames and well over a second** (256 frames / 1.38 s for
//! one CR3, 205 frames / 1.58 s for the three CR3s of `kittest_library_stack`).
//! That is a factor >100 in frames and >100 in wall clock. **The absolute counts
//! are machine- and load-dependent and are not a specification** — an earlier
//! single run reported 24 and 78 frames, which has not reproduced, and whose
//! deviation ran in opposite directions (frames too low, wall clock too high),
//! so it is not a harness difference either. What a test may rely on is the gap.

mod kittest_decode_support;
mod scan_settle_support;

use egui_kittest::Harness;
use kittest_decode_support::{
    is_settled, pump, pump_until_ready, DecodeSettled, Ready, SETTLE_DEADLINE,
};
use lumina_gui::LuminaApp;
use scan_settle_support::{scan_and_decode_settled, settle_scan, settle_scan_within};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Every state field the report owes the reader. Asserted on the message text
/// here and in `kittest_decode_bound`, so a report that loses a field is caught
/// wherever it is produced.
const REPORT_FIELDS: &[&str] = &[
    "cause:",
    "expected:",
    "preview_generation=",
    "metadata_history_len=",
    "metadata_draft_len=",
    "scan_pending=",
    "decode_pending=",
    "status:",
    "error:",
];

/// A 4x3 synthetic PNG on disk — the same fixture class the mask-local suites
/// use, decodable in milliseconds.
fn write_png(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, LuminaApp::sample_image_png()).expect("write png fixture");
    path
}

/// Stage the licensed 24-megapixel RAW fixture into `dir`.
///
/// A CR3 where a PNG would do, for one reason: the loudness test needs a decode
/// that is **still in flight** after the wait's first frame, and only a real RAW
/// guarantees that — on this machine a staged CR3 folder settle runs into the
/// hundreds of frames and over a second, against 3-4 frames for the 4x3 PNG. The
/// gap is the load-bearing fact; the exact counts vary with machine and load.
fn stage_raw(dir: &Path, name: &str) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../sample-data/raw")
        .join(name);
    std::fs::copy(&source, dir.join(name))
        .unwrap_or_else(|error| panic!("stage {}: {error}", source.display()));
}

/// A headless harness **without** the wgpu renderer.
///
/// Every claim here is about `LuminaApp` state — flags, entries, generations —
/// which the frame loop produces without a renderer, the same shape the non-GPU
/// mask-local suites use. That is what lets this target run in CI.
fn build_harness() -> Harness<'static, LuminaApp> {
    Harness::builder()
        .with_size([1024.0_f32, 720.0_f32])
        .build_eframe(|cc| LuminaApp::new(cc.egui_ctx.clone()))
}

/// A **settled decode is not a settled folder scan**, and the wait pays for it.
///
/// The state is one production really produces: a decoded source, then a folder
/// navigation. What this test pins — and what it measurably catches, N=25 runs
/// each, `KITT-SCAN-M2-DOC-59`:
///
/// | Mutation | Caught | Rate | Source |
/// | --- | --- | --- | --- |
/// | **M1** — collapse `scan_and_decode_settled` onto `is_settled`, so the wait accepts a settled decode as a settled scan | yes | red | **measured here**, N=3 |
/// | **M-B** — rewire `settle_scan` onto the file-open wait's exit | yes | 23/25 red | quoted, `KITT-SCAN-PREMISSE-58` (N=25) |
///
/// The rates are deliberately **not** on one scale. M1 was re-measured on this
/// host while correcting this comment; M-B is quoted from that task's
/// verification round and was **not** re-run here. Presenting a quoted number as
/// if it were a local measurement is the exact failure this comment is being
/// corrected for, so the column says which is which.
///
/// N=3 is enough to show M1 is caught *reliably*; it is not the 25-run
/// flake rate of M-B and must not be read as one.
///
/// Both reach the same product claim from different directions, and both make
/// the `assert!`s below fire: the decode is terminal before the folder scan is
/// applied, so a wait that conflates the two returns early with `entries()`
/// still holding the *previous* folder's single entry. The assertion *after*
/// `settle_scan` is what turns "the two conditions differ" into "the wait behaves
/// accordingly".
///
/// **Named limit, not a covered claim (measured, N=25):** a silent early return
/// of the *wait itself* is **not** caught by this test. The earlier version of
/// this comment claimed it was, and that was false. Placing the early return in
/// the **source** folder leaves the run 25/25 green, because
/// `settle_scan_within` pumps one extra frame after the wait
/// (`scan_settle_support/mod.rs:104`) and the two-file scan lands exactly in
/// that trailing frame — by then `entries() == 2` holds and the assertion is
/// satisfied by the real result rather than by the wait having waited. It turns
/// red only when the *target* folder is the slow one. Catching it structurally
/// would mean removing the trailing frame, which exists to paint the settled
/// status for the golden; that trade is a different task, and until it is taken
/// the honest statement is this limit, not the earlier one.
///
/// The *previous* folder's listing is itself produced by a later async source
/// (the scan worker `open_file` arms), so it is a **wait condition** here and
/// not a fixed-point assertion (KITT-SCAN-PREMISSE-58): `preview_generation()`
/// is bumped by a render and implies nothing about the applied listing, and a
/// fixed `entries().len() == 1` therefore flaked as `left: 0 right: 1` on `main`.
#[test]
fn settle_scan_waits_for_the_listing_even_when_the_decode_is_already_settled() {
    let source_dir = tempfile::tempdir().expect("temp dir");
    let photo = write_png(source_dir.path(), "photo.png");
    let library = tempfile::tempdir().expect("temp dir");
    write_png(library.path(), "one.png");
    write_png(library.path(), "two.png");

    let mut harness = build_harness();
    // A decoded source: the decode is terminal, so `is_settled` holds. The ready
    // state is the *declared* form — a render generation is written by the render
    // `finish_decode` schedules, so the settled exit can decide it.
    pump_until_ready(
        &mut harness,
        &photo,
        DecodeSettled::new("preview_generation() >= 1", |app: &LuminaApp| {
            app.preview_generation() >= 1
        }),
        SETTLE_DEADLINE,
    );
    // The opened file's folder is listed by a *second* async source — the scan
    // worker — so wait for that listing instead of assuming it. The bound-only
    // wait: `pump` has no settled exit at all, so this cannot be cut short by a
    // decode that settled a frame earlier (which is precisely the state we are
    // still in — KITT-DECODE-CONTRACT-52, and KITT-SCAN-PREMISSE-58 for the
    // measured panic that the type split now makes unrepresentable).
    pump(
        &mut harness,
        source_dir.path(),
        Ready::new("the source folder's listing landed", |app: &LuminaApp| {
            app.entries().len() == 1
        }),
        SETTLE_DEADLINE,
    );
    // A navigation, with no frame pumped yet: the scan worker is armed.
    harness
        .state_mut()
        .set_directory(library.path().display().to_string());

    {
        let app = harness.state();
        assert!(
            is_settled(app),
            "premise: the decode is terminal before the new listing is applied \
             (preview_generation={}, decode_pending={}, error={:?})",
            app.preview_generation(),
            app.decode_pending(),
            app.error(),
        );
        assert!(
            app.scan_pending(),
            "premise: the folder scan is in flight while the decode is settled"
        );
        assert!(
            !scan_and_decode_settled(app),
            "the two conditions must differ here; if they did not, the settled exit \
             would be sound for a folder scan and the two waits would be \
             interchangeable"
        );
        // `entries().len() == 1` is deliberately **not** asserted here: it is the
        // exit condition of the listing wait above, so after it the previous
        // folder's single entry is a consequence and not a second assumption. A
        // fixed-point assert on it was the flake this task removes.
    }

    settle_scan(&mut harness);
    assert_eq!(
        harness.state().entries().len(),
        2,
        "the wait must not return before the listing lands (KITT-SETTLE-UNIFY-53: \
         a settled decode is not a settled scan)"
    );
}

/// The wait is a real wait: it returns only once the listing is applied **and**
/// the auto-load decode that listing starts has finished.
#[test]
fn settle_scan_returns_only_after_the_listing_and_the_auto_load_decode_landed() {
    let library = tempfile::tempdir().expect("temp dir");
    write_png(library.path(), "one.png");
    write_png(library.path(), "two.png");

    let mut harness = build_harness();
    harness
        .state_mut()
        .set_directory(library.path().display().to_string());
    settle_scan(&mut harness);

    let app = harness.state();
    assert_eq!(app.entries().len(), 2, "the listing must be applied");
    assert!(!app.scan_pending(), "the scan must be drained");
    assert!(
        !app.decode_pending(),
        "the auto-load decode the listing starts must be drained too"
    );
    assert!(
        app.preview_generation() >= 1,
        "the auto-loaded first entry must have rendered, so the wait really \
         covered a decode and not only a listing (preview_generation={})",
        app.preview_generation()
    );
    assert!(
        app.error().is_none(),
        "a staged PNG fixture must decode without a banner: {:?}",
        app.error()
    );
}

/// The wait cannot pass silently: an unfulfillable condition is **reported**.
///
/// The bound is one millisecond: long enough to be a real budget, and the
/// fixture is a real RAW so the condition cannot be satisfied by pumping at all —
/// after the first frame either the scan is still in flight, or it has landed and
/// the 24-megapixel decode it started is. Both are the production state this
/// wait exists for.
#[test]
fn an_unfulfillable_folder_scan_wait_reports_the_state_it_found() {
    let library = tempfile::tempdir().expect("temp dir");
    stage_raw(library.path(), "aircraft-landscape.cr3");

    let mut harness = build_harness();
    harness
        .state_mut()
        .set_directory(library.path().display().to_string());
    let bound = Duration::from_millis(1);
    let payload = catch_unwind(AssertUnwindSafe(|| settle_scan_within(&mut harness, bound)))
        .expect_err("an unfulfillable wait must give up loudly, not return");
    let message = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
        })
        .expect("the wait must panic with a message");

    for field in REPORT_FIELDS {
        assert!(
            message.contains(field),
            "the report must carry {field:?}:\n{message}"
        );
    }
    assert!(
        message.contains("cause:     the wall-clock bound of 1ms elapsed"),
        "the cause must name the bound that expired:\n{message}"
    );
    assert!(
        message.contains("expected:  scan_pending() == false && decode_pending() == false"),
        "the report must name the expected condition:\n{message}"
    );
    assert!(
        message.contains(library.path().to_str().expect("utf-8 temp path")),
        "the report must name the folder it was waiting for:\n{message}"
    );
    // The state really was in flight, so "the bound elapsed" is not masking an
    // already-settled scan that the wait simply misread.
    assert!(
        message.contains("scan_pending=true") || message.contains("decode_pending=true"),
        "the reported state must show work still in flight, otherwise the \
         expiry would prove nothing:\n{message}"
    );
}
