//! KITT-IDENTITY-49 / KITT-SETTLE-UNIFY-53: the shared wait machinery of the
//! headless test tree — one loop, one report, and the *file-open* wait built on
//! them.
//!
//! Opening a real file is asynchronous: `open_file` starts a background decode
//! and `finish_decode` is reached from a later frame, so a suite has to wait.
//! That wait has **two** exits, and both must stay loud:
//!
//! * **settled** — the decode finished and reported, so the expected state can
//!   no longer arrive. Measured A/B on the same unfulfillable predicate and the
//!   same fixture, one line of code apart: **305 s** without it, **under 10 s** for
//!   the whole `kittest_crop_overlay` suite with it, and the single failing test
//!   reports within the first few frames. **The ratio is the claim, not the
//!   absolute seconds** — they are wall-clock and therefore machine- and
//!   load-dependent; what reproduces is that the settled exit turns a five-minute
//!   wait into a report you get in the same test run.
//! * **bound** — the wall-clock budget elapsed, for a worker that never
//!   returns.
//!
//! Both print the same state report, so an expiry is never a bare sentence and
//! never a silent return. Before this module the tree carried **six**
//! hand-written waits: one that could not even fail (`kittest_crop_overlay`,
//! whose 500-frame budget returned normally and the golden was then taken from an
//! unsettled state) and five that could fail but reported **no state at all** —
//! two byte-identical `settle_scan` copies, the two mask-local decode drains, and
//! `develop_section_denoise`'s 400-frame preview wait. All six now end up in this
//! loop and this report.
//!
//! `develop_section_denoise` shows the wrong diagnosis most clearly: with its
//! loop in place an expired budget surfaced as `Image did not match snapshot`
//! with **112 373 failing pixels** (measured, the loop having fallen through),
//! i.e. "the pixels are wrong" for a source that was never decoded at all. The
//! shared wait names the expected condition and the state it found instead.
//! `kittest_snapshots.rs` sits at its committed size baseline, so the
//! replacement had to fit into the lines the loop vacated — which is why the
//! rationale lives here and not there.
//!
//! # What lives here and what does not
//!
//! The **loop** and the **report** are here because every wait needs them, and
//! they are `pub(crate)` so the folder-scan wait (`scan_settle_support`) can
//! reuse them instead of carrying a second copy. The **folder-scan condition**
//! is *not* here: it is a different question (see [`is_settled`]), and putting it
//! here would arm the settled exit for a predicate that the settled exit cannot
//! decide.
//!
//! # The contract a caller must keep
//!
//! A ready predicate must be **decidable once the decode settled** (see
//! [`is_settled`]): it may only read state that `finish_decode` writes
//! synchronously (`preview_generation`, the error banner, the metadata
//! accessors). A predicate fed by a *later* async source — a thumbnail worker, a
//! folder scan — must not be armed with the settled exit; such a caller waits
//! with its own bound. The soundness of the early exit rests on this, so it is
//! stated here rather than assumed per call site.
//!
//! # The bound, and why it is named at the call site
//!
//! [`SETTLE_DEADLINE`] is the production value: generous on purpose
//! (GOLDEN-FIXT-31). What is **measured** is that a folder settle on staged
//! 24-megapixel CR3s runs into the **hundreds of frames and over a second** on
//! this machine, where a 4x3 smoke PNG is done in a handful — so any bound has to
//! be counted in **wall clock**, not in frames. What is **not** measured, and is
//! therefore not claimed here, is the split between LibRaw and the rest of the
//! pipeline: an earlier note in this file attributed the cost to "~0.7 s in
//! LibRaw", that figure had no measurement behind it, and it has been removed
//! rather than re-tuned. It is a parameter of [`pump_until_ready`] rather than a
//! hidden default, because the bound genuinely differs per fixture class — and a
//! call site that names its bound cannot drift.
//!
//! The honest reason for the bound is **not** speed, and nothing here pretends
//! otherwise. Measured on this machine (macOS/Metal, 2026-09-28) by counting the
//! frames each wait pumps — instrumented temporarily, then reverted — over the
//! whole set of wait call sites:
//!
//! * **No wait returns early.** Across every invocation measured (74 in one full
//!   pass, 31 decode waits and 43 folder-scan settles) the state at exit read
//!   `decode_pending=false` in **74 of 74**, and the predicate was re-read after
//!   each step, so a wait can only return when its own condition holds. This is
//!   the invariant that matters and it reproduces exactly.
//! * The **mask-local drains are 2 frames in every run**, and the **file-open
//!   waits are 2 frames**. An earlier single measurement of a 1-frame file-open
//!   wait has **not reproduced** across later runs, so it is not stated as a
//!   property. The frame *counts* are machine- and load-dependent and are **not a
//!   specification** — empty-directory settles land at 3-4 frames, CR3 settles in
//!   the hundreds — but no run has shown a wait returning before its predicate.
//! * No consumer of this helper passes a RAW file. The measured fixture names are
//!   three, not two: `photo.png` — the 4x3 `sample_image_png` — `photo.jpg` — a
//!   2x1 encoder JPEG — and `auto_fill.png`, a 64x64 image. So the old 500-frame
//!   budget had orders of magnitude of reserve and **never expired**; the
//!   consolidation fixes no such failure.
//! * The result that matters for a golden is the *state* at exit, and it did not
//!   move: `kittest_snapshots` reported 44 passed and the same 12 pre-existing
//!   failures in every run, and both `kittest_crop_overlay` goldens were green in
//!   every run.
//!
//! # Renderer-agnostic
//!
//! Its consumers are the wgpu golden suites, which are `#[ignore]`d by the same
//! policy as the goldens, and the plain-harness mask-local suites. The module
//! itself needs **no renderer** — a wait only reads `Harness` state — which is
//! why a suite without `.wgpu()` may declare it
//! (`mask_local_editors_support::settle_decode`).

use egui_kittest::Harness;
use lumina_gui::LuminaApp;
use std::path::Path;
use std::time::{Duration, Instant};

/// Wall-clock bound for one background wait: a file decode, or a folder scan
/// plus the auto-load decode it starts.
///
/// Exceeding it is a real hang, and the fixtures are all local.
pub(crate) const SETTLE_DEADLINE: Duration = Duration::from_secs(300);

/// Wall-clock yield between two pumped frames.
///
/// The work being waited for runs on worker threads, so a frame loop with no
/// yield busy-waits against them. This is the yield the hand-written loops this
/// module replaces used; keeping it is what keeps their timing unmoved.
const FRAME_YIELD: Duration = Duration::from_millis(1);

/// The settled-exit cause line: the decode is terminal, so the expected state
/// can no longer arrive and pumping on is pure delay. One sentence, because the
/// report is read by a human deciding between "a millisecond failure" and "a
/// five-minute hang".
const SETTLED_CAUSE: &str = "the decode settled and reported its outcome, so the expected \
     state can no longer arrive";

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

/// What may end a wait before its bound: `Some(predicate)` arms the **settled
/// exit**, `None` leaves the wall-clock bound as the only exit.
///
/// A predicate is only eligible if it is *decidable once the decode settled*:
/// it may read state `finish_decode` writes synchronously, never a later async
/// source. [`is_settled`] is the one such predicate; a predicate fed by the
/// folder scan must pass `None` (see `scan_settle_support`).
pub(crate) type SettledExit = Option<fn(&LuminaApp) -> bool>;

/// The single definition of "the background decode has finished and said so".
///
/// This is the **one** place the condition is written down, and the one place it
/// is consumed: the wait stops on it — it is the settled exit's predicate — and
/// the callers that wait for the decode *result* itself build their ready state
/// from it (see `kittest_sidecar_identity::ready_settled`), so a second,
/// drifting copy of the condition cannot appear.
///
/// It deliberately knows nothing about a **folder scan**, and that asymmetry is
/// load-bearing. `LuminaApp::open_file` navigates first
/// (`prepare_open_directory` -> `list_directory`) and decodes second, so "the
/// decode settled" and "the listing landed" are two different moments, and there
/// is a production state in which this is `true` while a scan is still in
/// flight. A settled exit armed for a scan predicate would return before the
/// listing lands and the golden would capture an empty grid; see
/// `scan_settle_support` for the wait that does not do that, and
/// `folder_scan_settle` for the test that pins the difference.
pub(crate) fn is_settled(app: &LuminaApp) -> bool {
    !app.decode_pending() && (app.error().is_some() || app.preview_generation() >= 1)
}

/// Open `path` and wait for `ready` under `bound`.
///
/// `bound` is a parameter, not a hidden constant, because the bound genuinely
/// differs per fixture class: a 24-megapixel RAW needs seconds and takes
/// [`SETTLE_DEADLINE`], while the mask-local suites decode a 4x3 smoke PNG in
/// ~30 ms and name a tighter one. Every call site therefore says which bound it
/// obeys, and the value itself exists once, here.
#[allow(
    dead_code,
    reason = "only `kittest_library_stack` declares this module without a file-open wait \
              (it scans a folder and never opens a file). The allow is on this one item \
              because the alternative — a second `#[path]` declaration of the shared loop — \
              is `clippy::duplicate_mod` and would compile the machinery twice per target."
)]
pub(crate) fn pump_until_ready<F: FnMut(&LuminaApp) -> bool>(
    harness: &mut Harness<'_, LuminaApp>,
    path: &Path,
    ready: Ready<F>,
    bound: Duration,
) {
    harness.state_mut().open_file(path.display().to_string());
    pump(harness, path, ready, bound, Some(is_settled));
}

/// Pump `harness` until `ready` holds, then return; give up loudly otherwise.
///
/// The one wait loop in the tree, so the two exits cannot drift apart again: a
/// **settled exit** when `settled_exit` is armed and the decode is terminal (a
/// millisecond failure), and the **wall-clock bound** otherwise. Both print the
/// same state report.
///
/// `subject` names what is being waited for in that report: the file being
/// opened, or the directory whose scan is landing. It is a snapshot taken by the
/// caller, so a wait on state the app owns can still name its target.
///
/// # Frame counts are part of the contract
///
/// A golden is pixels, so *how many* frames a wait pumps is part of what it
/// pins. This loop steps **before** it re-reads the ready state, which is the
/// order the four replaced loops used; reordering it to check first would
/// silently save one frame per wait and could change a golden. Do not
/// "optimise" the order. `develop_section_denoise` is the one caller that used
/// the harness's own `run()` (drain-until-quiet) rather than a single step, so
/// it pumps strictly less; its golden is unaffected (2 frames, unchanged).
pub(crate) fn pump<F: FnMut(&LuminaApp) -> bool>(
    harness: &mut Harness<'_, LuminaApp>,
    subject: &Path,
    mut ready: Ready<F>,
    bound: Duration,
    settled_exit: SettledExit,
) {
    let deadline = Instant::now() + bound;
    loop {
        // `step()` (not `run()`): a scheduled thumbnail/scan repaint would make
        // `run()` exceed its max_steps bound.
        harness.run_steps(1);
        if (ready.predicate)(harness.state()) {
            return;
        }
        // The settled exit is tested *before* the bound: when both hold, the
        // settled cause is the more specific one and the state cannot improve.
        let cause = settled_exit
            .filter(|is_settled| is_settled(harness.state()))
            .map(|_| SETTLED_CAUSE.to_owned())
            .or_else(|| (Instant::now() >= deadline).then(|| bound_elapsed(bound)));
        if let Some(cause) = cause {
            panic!(
                "{}",
                state_report(harness.state(), subject, &ready.description, &cause, bound)
            );
        }
        std::thread::sleep(FRAME_YIELD);
    }
}

/// The wall-clock cause line. The bound is a budget, not a diagnosis, so this
/// line deliberately says nothing about the decode; the found state in the
/// report is what says whether one was still running.
fn bound_elapsed(bound: Duration) -> String {
    format!("the wall-clock bound of {bound:?} elapsed")
}

/// The state the harness was actually in when the wait gave up.
///
/// Reports found values, never just the expectation — the point of the report is
/// that a failure is distinguishable both from a slow machine and from a
/// terminal state. `metadata_history_len` / `metadata_draft_len` are the numbers
/// a History/metadata suite waits for, and `error` carries the production
/// refusal message verbatim (including both fingerprints) when one was raised.
fn state_report(
    app: &LuminaApp,
    subject: &Path,
    expecting: &str,
    cause: &str,
    bound: Duration,
) -> String {
    format!(
        "settle of {} never finished in headed harness (bound {bound:?})\n\
         cause:     {cause}\n\
         expected:  {expecting}\n\
         found:     preview_generation={}, metadata_history_len={}, metadata_draft_len={}, \
         scan_pending={}, decode_pending={}\n\
         status:    {:?}\n\
         error:     {}",
        subject.display(),
        app.preview_generation(),
        app.metadata_history().len(),
        app.metadata_draft().len(),
        app.scan_pending(),
        app.decode_pending(),
        app.status(),
        app.error().unwrap_or("<no error banner>"),
    )
}
