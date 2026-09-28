//! MITTEL-3: the two exit paths of the shared file-open wait must stay
//! distinguishable — and both must stay real.
//!
//! KITT-SETTLE-UNIFY-53: the loop and the report moved to
//! `kittest_decode_support` -> `settle_support`; this file still pins both exits,
//! now including the one thing the move could have broken — the frame count of a
//! healthy wait, which the settled exit must not shorten.
//!
//! `kittest_decode_support::pump_until_ready` gives up for exactly two
//! reasons, and this file pins both:
//!
//! 1. **Settled.** The decode finished and reported, so the expected state can
//!    no longer arrive. Measured before the settled exit existed: the same
//!    unreachable state cost **305.39 s** in `kittest_crop_overlay` (against
//!    6.02 s for the old 500-frame budget) — the same report, 300 s later.
//!    `settled_decode_with_unmet_predicate_is_reported_at_once` proves the
//!    report arrives at once **and** that it names every state field.
//! 2. **Bound.** The decode is still in flight, so the wall-clock bound is the
//!    only exit. `decode_in_flight_waits_for_the_bound` proves the helper still
//!    *spends* that bound instead of cutting it off — a helper that stopped
//!    after a second would be no more correct than one that waits five minutes.
//!
//! The predicate is the same in both tests (`needs a second render`, so a
//! single decode can never satisfy it). Only the **state** differs, which is
//! the point: a terminal decode is reported at once, a non-terminal one waits.
//! The second test therefore also documents where the line runs —
//! `open_file("")` returns before the decode receiver is armed, so no decode,
//! no banner and no render ever arrive.
//!
//! Requires a working GPU / headless wgpu backend, so both tests are
//! `#[ignore]`d by the same policy as the goldens. Run locally with:
//!
//! ```text
//! cargo test -p lumina-gui --test kittest_decode_bound -- --ignored --test-threads=1
//! ```

mod kittest_decode_support;

use egui_kittest::Harness;
use kittest_decode_support::{is_settled, pump_until_ready, Ready, SETTLE_DEADLINE};
use lumina_gui::LuminaApp;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A 4x3 synthetic PNG on disk: the same class of fixture every consumer of
/// the wait actually passes (no RAW file, see the module docs).
fn png_fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let photo = dir.path().join("photo.png");
    std::fs::write(&photo, LuminaApp::sample_image_png()).expect("write png fixture");
    (dir, photo)
}

/// A predicate no single decode can satisfy: the second render never comes.
fn needs_a_second_render() -> Ready<impl FnMut(&LuminaApp) -> bool> {
    Ready::new("preview_generation() >= 2", |app: &LuminaApp| {
        app.preview_generation() >= 2
    })
}

fn build_harness() -> Harness<'static, LuminaApp> {
    Harness::builder()
        .with_size([1024.0_f32, 720.0_f32])
        .wgpu()
        .build_eframe(|cc| LuminaApp::new(cc.egui_ctx.clone()))
}

/// Run `wait` and return the panic message it produced (plus how long it took).
fn timed_panic_message(wait: impl FnOnce()) -> (String, Duration) {
    let started = Instant::now();
    let payload = catch_unwind(AssertUnwindSafe(wait)).expect_err("the wait must give up");
    let message = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
        })
        .expect("the wait must panic with a message");
    (message, started.elapsed())
}

/// Every state field the report owes the reader (MITTEL-3/4).
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

/// A **settled** decode with an unmet predicate is a fault, not a wait: the
/// report arrives at once, names every state field, and the cause says the
/// decode is terminal.
///
/// Run through the **production** wrapper with the production 300 s bound: a
/// stand-in bound would only prove the mechanism, not the shipped default.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_decode_bound -- --ignored"]
fn settled_decode_with_unmet_predicate_is_reported_at_once() {
    let (_dir, photo) = png_fixture();
    let mut harness = build_harness();
    let (message, elapsed) = timed_panic_message(|| {
        pump_until_ready(
            &mut harness,
            &photo,
            needs_a_second_render(),
            SETTLE_DEADLINE,
        )
    });
    assert!(
        message.contains("cause:     the decode settled"),
        "the cause must name the settled exit, got:\n{message}"
    );
    for field in REPORT_FIELDS {
        assert!(
            message.contains(field),
            "the report must carry {field:?}:\n{message}"
        );
    }
    assert!(
        message.contains("expected:  preview_generation() >= 2"),
        "the report must name the expected predicate:\n{message}"
    );
    // The decode really is terminal: the report must say so, otherwise
    // "settled" would be an assumption of this test rather than a finding.
    assert!(
        message.contains("preview_generation=1"),
        "the fixture must have rendered exactly once, so the predicate is \
         unreachable and the decode is terminal:\n{message}"
    );
    assert!(
        is_settled(harness.state()),
        "the settled exit must only fire on a terminal state"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "a settled decode must be reported at once, took {elapsed:?} \
         (production bound {SETTLE_DEADLINE:?})"
    );
    // The timings are the point of this test, so they are logged, not just
    // asserted: `--nocapture` shows the number that the defect would inflate
    // from milliseconds to five minutes.
    eprintln!(
        "KITT-IDENTITY-49 settled exit: gave up after {elapsed:?} (bound {SETTLE_DEADLINE:?})"
    );
}

/// A decode that is **not** settled must still spend the bound: the wall-clock
/// deadline is the exit for a worker that never returns, and the settled exit
/// must not swallow it.
///
/// `open_file("")` is the reachable non-terminal state: `begin_load_path`
/// returns before arming the receiver, so no decode, no banner and no render
/// ever arrive — `is_settled` stays false and pumping changes nothing.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_decode_bound -- --ignored"]
fn decode_in_flight_waits_for_the_bound() {
    let mut harness = build_harness();
    let bound = Duration::from_secs(2);
    let (message, elapsed) = timed_panic_message(|| {
        pump_until_ready(&mut harness, Path::new(""), needs_a_second_render(), bound)
    });
    assert!(
        message.contains("cause:     the wall-clock bound of 2s elapsed"),
        "the cause must name the wall-clock bound, got:\n{message}"
    );
    assert!(
        message.contains("preview_generation=0"),
        "no render arrived, so the state really is non-terminal:\n{message}"
    );
    // The bound is the lower bound of the wait, not a target it may undercut:
    // a helper that gave up after 5 ms would pass the assertion above and be
    // just as wrong as one that waited five minutes.
    assert!(
        elapsed >= bound,
        "an unsettled decode must spend the bound of {bound:?}, gave up after {elapsed:?}"
    );
    eprintln!("KITT-IDENTITY-49 bound exit: gave up after {elapsed:?} (bound {bound:?})");
}
