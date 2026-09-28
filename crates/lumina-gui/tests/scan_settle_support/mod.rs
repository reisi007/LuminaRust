//! KITT-SETTLE-UNIFY-53: the **one** definition of "the folder scan and the
//! auto-load decode it starts are both done", and the wait that enforces it.
//!
//! # Why this is not the file-open wait
//!
//! `kittest_decode_support::pump_until_ready` waits for a *decode* and may end
//! early on a settled decode. This waits for a *scan* as well, and the two
//! conditions genuinely differ: `LuminaApp::open_file` navigates first
//! (`prepare_open_directory` -> `list_directory`) and decodes second, so there
//! is a production state with `scan_pending() == true` while the decode is
//! already settled. A settled exit armed here would return before the listing
//! lands and the golden would capture an empty grid — a silent flake.
//! [`settle_scan`] therefore arms **no** settled exit and the wall-clock bound
//! is its only exit; the report then names the state it was in.
//!
//! Before this module the condition was written **three** times — twice
//! byte-identical (`kittest_snapshots_support`, `kittest_library_stack`) and
//! once inside a 500-frame loop in `kittest_crop_overlay` that could expire
//! silently and let the golden be taken from an unsettled state. One
//! definition, one wait, one report.
//!
//! The loop and the report are **not** repeated here: they are
//! `kittest_decode_support`'s, reached through the crate root so a target that
//! scans declares the machinery exactly once (a second `#[path]` declaration of
//! the same file is `clippy::duplicate_mod` and would compile it twice).
//!
//! # The frame count is part of the contract
//!
//! A golden is pixels, so the number of frames this wait pumps must not move
//! under it. The two `settle_scan` copies this replaces stepped, checked, and on
//! success stepped once more before returning; [`settle_scan`] keeps the trailing
//! frame on top of the shared loop (which steps before it re-reads the state), so
//! the healthy path is unchanged — measured, not assumed, on three runs of the
//! same suites (macOS/Metal 2026-09-28, instrumented then reverted):
//!
//! * The **empty-directory** settles (32 of the 41 invocations) take **3-4
//!   frames** in every run, before and after.
//! * The **staged-CR3** settles are bounded by *wall clock*, not frames, and each
//!   frame over a 12 MB source costs tens to hundreds of milliseconds. Their frame
//!   count therefore tracks machine speed: 21-176 frames before the
//!   consolidation, 21-231 after, for wall clocks of 2-13 s. That range is
//!   measurement noise in the *fixture decode*, not in this wait — the same
//!   binaries were 6x apart in wall clock between two of the runs.
//! * What did not move is the outcome: the same 44 goldens passed and the same 12
//!   (pre-existing) failed in every run.

use crate::kittest_decode_support::{pump, Ready, SETTLE_DEADLINE};
use egui_kittest::Harness;
use lumina_gui::LuminaApp;
use std::path::Path;
use std::time::Duration;

/// The one definition of the scan-and-decode settled condition.
///
/// Both halves are needed. `decode_pending()` alone would return before the
/// scan is applied at all, i.e. on an empty grid; `scan_pending()` alone would
/// return before the auto-load decode the applied listing starts, and the
/// transient decoding state would be in the golden.
pub(crate) fn scan_and_decode_settled(app: &LuminaApp) -> bool {
    !app.scan_pending() && !app.decode_pending()
}

/// Drive the folder scan the caller has already requested — and the auto-load
/// decode that listing starts — to a settled state, then pump one more frame so
/// the applied listing and status are painted, under [`SETTLE_DEADLINE`].
///
/// The caller owns the `set_directory` / `list_directory` call; this owns only
/// the waiting, so the ready state depends on the app and not on how the scan was
/// requested. The bound is not a parameter here (unlike the file-open wait, whose
/// bound differs per fixture class): every scan caller waits out the production
/// bound, so repeating the constant at three call sites would only be a chance to
/// spell it differently.
pub(crate) fn settle_scan(harness: &mut Harness<'_, LuminaApp>) {
    settle_scan_within(harness, SETTLE_DEADLINE);
}

/// [`settle_scan`] with an injectable `bound`.
///
/// The production bound is [`SETTLE_DEADLINE`]; the parameter exists so the
/// *bound* exit can be exercised (and timed) without spending five minutes —
/// which is the difference between a wait that is provably bounded and one that
/// merely looks bounded.
pub(crate) fn settle_scan_within(harness: &mut Harness<'_, LuminaApp>, bound: Duration) {
    // The report must name the folder, read from the app rather than from the
    // caller's argument: `set_directory` has already stored it, and it is the
    // folder whose listing is actually in flight.
    let subject = Path::new(harness.state().directory()).to_path_buf();
    pump(
        harness,
        &subject,
        Ready::new(
            "scan_pending() == false && decode_pending() == false",
            scan_and_decode_settled,
        ),
        bound,
        None,
    );
    // One extra frame so the applied status/list is painted: without it the
    // golden would capture the transient "Scanning folder…" status.
    harness.run_steps(1);
}
