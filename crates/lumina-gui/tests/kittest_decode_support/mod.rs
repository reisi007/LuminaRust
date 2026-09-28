//! KITT-IDENTITY-49: the *file-open* wait of the headless kittest suites —
//! the bound it obeys and, above all, the report it prints when it gives up.
//!
//! Opening a real file is asynchronous: `open_file` starts a background decode
//! and `finish_decode` is reached from a later frame, so a suite has to wait.
//!
//! # Why one shared wall-clock bound (MITTEL-2)
//!
//! Before this module the two consumers had two different mechanisms:
//! `kittest_snapshots` waited out a 300 s wall-clock deadline,
//! `kittest_crop_overlay` a 500-frame budget. One shared [`DECODE_DEADLINE`]
//! replaces both, so a wait cannot drift apart again.
//!
//! The honest reason is **not** speed, and this module does not pretend
//! otherwise. Measured on this machine (macOS/Metal, 2026-09-28) by counting
//! `run_steps` per wait — instrumented temporarily in
//! `pump_until_ready_within`, then reverted: **14 wait invocations across the
//! three suites, every one exactly 2 frames**. No consumer of this helper
//! passes a RAW file. The measured fixture names are three, not two:
//! `photo.png` — the 4x3 `sample_image_png` — `photo.jpg` — a 2x1 encoder
//! JPEG — and `auto_fill.png`, a 64x64 image. So the
//! 500-frame budget had ~250x of reserve and **never expired**; this change
//! fixes no such failure.
//!
//! What it does buy, measured:
//!
//! * **The state report.** Before, a timeout printed one line — `decode of
//!   <path> never settled in headed harness` — with **zero** state values. The
//!   cheapest possible fault (a fixture whose sidecar was refused as stale,
//!   leaving the app in a terminal state that can never satisfy the predicate)
//!   cost five minutes to recognise, because a timeout looks exactly like a
//!   slow machine. [`pump_until_ready`] now names the cause, the expectation,
//!   the values found, the status line and the error banner.
//! * **The settled exit (MITTEL-3).** The same unreachable state used to burn
//!   the whole bound. Measured A/B on the same unfulfillable predicate and the
//!   same fixture, one line of code apart: **305.41 s** without the settled
//!   exit, **5.67 s** for the whole `kittest_crop_overlay` suite with it (the
//!   single failing test reports after 8.96 ms).
//!
//! The a-priori worry behind a wall-clock bound — that a real 24-megapixel
//! RAW decode could exhaust a 500-frame budget, which is why `settle_scan`
//! (GOLDEN-FIXT-31) is wall-clock — stays an **open concern, not the
//! justification**. It is real elsewhere: `kittest_library_stack` decodes the
//! committed CR3s through its own `settle_scan`. It is simply **not exercised
//! here**, so no number is claimed for it, and the 2 frames above are the
//! only decode demand this module has measured.
//!
//! # The contract a caller must keep
//!
//! A ready predicate must be **decidable once the decode settled** (see
//! [`is_settled`]): it may only read state that `finish_decode` writes
//! synchronously (`preview_generation`, the error banner, the metadata
//! accessors). A predicate fed by a *later* async source — a thumbnail worker,
//! a folder scan — would be cut off by the settled exit; such a caller waits
//! with its own loop. The soundness of the early exit rests on this, so it is
//! stated here rather than assumed per call site.
//!
//! Requires a working GPU / headless wgpu backend — every consumer is
//! `#[ignore]`d by the same policy as the goldens.

use egui_kittest::Harness;
use lumina_gui::LuminaApp;
use std::path::Path;
use std::time::{Duration, Instant};

/// Wall-clock bound for one background decode plus its settle.
pub(crate) const DECODE_DEADLINE: Duration = Duration::from_secs(300);

/// A ready predicate together with the sentence a timeout report must print.
///
/// Built through [`Ready::new`], so the two halves always sit next to each
/// other at the definition site and cannot describe different states.
pub(crate) struct Ready<F> {
    description: String,
    predicate: F,
}

impl<F: FnMut(&LuminaApp) -> bool> Ready<F> {
    /// Pair `predicate` with the `description` an expiry report prints.
    pub(crate) fn new(description: impl Into<String>, predicate: F) -> Self {
        Ready {
            description: description.into(),
            predicate,
        }
    }
}

/// The single definition of "the background decode has finished and said so".
///
/// This is the **one** place the condition is written down, and the one place
/// it is consumed: `pump_until_ready` stops on it, and the callers that wait
/// for the decode *result* itself build their ready state from it (see
/// `kittest_sidecar_identity::ready_settled`), so a second, drifting copy of
/// the condition cannot appear.
pub(crate) fn is_settled(app: &LuminaApp) -> bool {
    !app.decode_pending() && (app.error().is_some() || app.preview_generation() >= 1)
}

// A `Ready` that waits on `is_settled` is declared next to its only caller;
// the condition itself stays here, so there is never a second copy of it.

/// Why the wait gave up. Both variants print the same report; the cause line is
/// what separates "a millisecond failure" from "a five-minute hang".
#[derive(Clone, Copy)]
enum Expiry {
    /// The decode finished and reported, so the expected state can no longer
    /// arrive: pumping on is pure delay.
    Settled,
    /// The wall-clock bound elapsed. Deliberately says nothing about the
    /// decode: the bound is a budget, not a diagnosis, and the found state
    /// below it is what says whether a decode was still running (a mutation
    /// that drops the settled exit reaches this variant with
    /// `decode_pending=false` — measured, 305.41 s — which is exactly why the
    /// two causes are separate).
    Bound,
}

impl Expiry {
    fn explanation(self, bound: Duration) -> String {
        match self {
            Expiry::Settled => "the decode settled and reported its outcome, so the expected \
                 state can no longer arrive"
                .to_owned(),
            Expiry::Bound => format!("the wall-clock bound of {bound:?} elapsed"),
        }
    }
}

/// Wait for `ready` under the shared [`DECODE_DEADLINE`].
pub(crate) fn pump_until_ready<F: FnMut(&LuminaApp) -> bool>(
    harness: &mut Harness<'_, LuminaApp>,
    path: &Path,
    ready: Ready<F>,
) {
    pump_until_ready_within(harness, path, ready, DECODE_DEADLINE);
}

/// [`pump_until_ready`] with an injectable `bound`.
///
/// The production bound is [`DECODE_DEADLINE`]; the parameter exists so the
/// *bound* exit can be exercised (and timed) without spending five minutes,
/// which is the difference between a helper that is provably still bounded and
/// one that merely looks bounded.
pub(crate) fn pump_until_ready_within<F: FnMut(&LuminaApp) -> bool>(
    harness: &mut Harness<'_, LuminaApp>,
    path: &Path,
    mut ready: Ready<F>,
    bound: Duration,
) {
    harness.state_mut().open_file(path.display().to_string());
    let deadline = Instant::now() + bound;
    while !(ready.predicate)(harness.state()) {
        // `is_settled` is tested *before* the bound: when both hold, the
        // settled cause is the more specific one and the state cannot improve.
        let cause = if is_settled(harness.state()) {
            Some(Expiry::Settled)
        } else if Instant::now() >= deadline {
            Some(Expiry::Bound)
        } else {
            None
        };
        if let Some(cause) = cause {
            panic!(
                "{}",
                decode_state_report(harness.state(), path, &ready.description, bound, cause)
            );
        }
        harness.run_steps(1);
    }
}

/// The state the harness was actually in when the wait gave up.
///
/// Reports found values, never just the expectation — the point of the report
/// is that a failure is distinguishable both from a slow machine and from a
/// terminal state. `metadata_history_len` / `metadata_draft_len` are the
/// numbers a History/metadata suite waits for, and `error` carries the
/// production refusal message verbatim (including both fingerprints) when one
/// was raised.
fn decode_state_report(
    app: &LuminaApp,
    path: &Path,
    expecting: &str,
    bound: Duration,
    cause: Expiry,
) -> String {
    format!(
        "decode of {} never settled in headed harness (bound {bound:?})\n\
         cause:     {}\n\
         expected:  {expecting}\n\
         found:     preview_generation={}, metadata_history_len={}, metadata_draft_len={}, \
         scan_pending={}, decode_pending={}\n\
         status:    {:?}\n\
         error:     {}",
        path.display(),
        cause.explanation(bound),
        app.preview_generation(),
        app.metadata_history().len(),
        app.metadata_draft().len(),
        app.scan_pending(),
        app.decode_pending(),
        app.status(),
        app.error().unwrap_or("<no error banner>"),
    )
}
