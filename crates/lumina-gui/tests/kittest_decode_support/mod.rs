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
//! The **wait**, the **loop** and the **report** are here because every wait
//! needs them, and they are `pub(crate)` so the folder-scan wait
//! (`scan_settle_support`) reuses them instead of carrying a second copy. The
//! **folder-scan condition** is *not* here: it is a different question (see
//! [`is_settled`]), and putting it here would give a wait a settled exit for a
//! state that exit cannot decide.
//!
//! # The contract, and what holds a caller to it
//!
//! A ready state may be paired with the **settled exit** only if it is
//! *decidable once the decode settled* (see [`is_settled`]): it may read nothing
//! but state `finish_decode` writes synchronously — the render it schedules
//! (`preview_generation()`, `preview()`), the document it adopts
//! (`metadata_history()`, `metadata_draft()`), the banner it raises (`error()`),
//! and `decode_pending()` itself. A ready state fed by a **later** async
//! source — the folder scan, a thumbnail worker — is decided later, and the
//! settled exit reports it as unreachable while that worker is still in flight.
//!
//! That was prose, and prose is not a gate. **Measured 2026-09-29** (headless,
//! no adapter, N=6 runs of one scenario on the shape this module had *before*
//! the change below): a folder-listing ready state run under the settled exit
//! was **reported unreachable in 7.2-33.4 ms**, its own found line reading
//! `scan_pending=true` — and the **same harness** then observed exactly that
//! state arrive **4.7-8.3 ms** later. The old fail direction was therefore
//! loud but **wrong in its cause**: "the expected state can no longer arrive" is
//! false while the scan is still in flight, and it sends the reader hunting a
//! defect that does not exist. KITT-DECODE-CONTRACT-52 makes it structural:
//!
//! * [`DecodeSettled`] is the **only** type [`pump_until_ready`] accepts, and it
//!   is built only through `DecodeSettled::new`, whose doc restates the
//!   obligation where a caller chooses it. A later-async ready state is a
//!   [`Ready`], and handing that to `pump_until_ready` is a **type error** — the
//!   KITT-SCAN-PREMISSE-58 mutation, measured: `error[E0308]: expected
//!   `DecodeSettled<_>`, found `Ready<_>``. The violating shape no longer
//!   compiles, so it can no longer reach a red run.
//! * [`pump`] — the one wait loop — takes **no exit parameter at all**. What may
//!   end it early is a property of the ready state it is handed, and the settled
//!   exit is attached in exactly one private place,
//!   [`DecodeSettled::into_ready`], whose only caller is [`pump_until_ready`].
//!   So no call site can end a wait early for a state that exit cannot decide:
//!   the `SettledExit` argument, and the `None` every scan caller had to write,
//!   are gone with it.
//!
//! What the contract *permits* is a closed set of reads, and the frame on which
//! a settled decode decides each of them is **measured** in
//! `tests/kittest_decode_contract.rs` — the headless coverage of the settled
//! exit that this mechanism had none of on a host without an adapter.
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

/// A ready state whose predicate reads **only** state the decode itself
/// decides, and the sentence a timeout report must print.
///
/// Built through [`DecodeSettled::new`], so the two halves always sit next to
/// each other at the definition site and cannot describe different states.
///
/// This is the *declared* form of the contract (see [`is_settled`] and the module
/// docs): it exists so that a ready state fed by a **later** async source — a
/// folder scan, a thumbnail worker — is a different *type* and therefore cannot
/// be routed through the wait that arms the settled exit.
#[allow(
    dead_code,
    reason = "only `kittest_library_stack` declares this module without a file-open wait \
              (it scans a folder and never opens a file), so it builds no declared ready \
              state. Same structural cause as the allow on `pump_until_ready`, and the same \
              alternative: a second `#[path]` copy of the shared loop is `clippy::duplicate_mod`."
)]
pub(crate) struct DecodeSettled<F> {
    ready: Ready<F>,
}

#[allow(
    dead_code,
    reason = "the constructor, like the type, is dead only in `kittest_library_stack`: the \
              one target that declares this module for a folder scan and never declares a \
              ready state. See the allow on `pump_until_ready` for the structural cause."
)]
impl<F: FnMut(&LuminaApp) -> bool> DecodeSettled<F> {
    /// Declare `predicate` **decidable once the decode settled** and pair it with
    /// the `description` an expiry report prints.
    ///
    /// # The obligation this constructor accepts
    ///
    /// `predicate` may read nothing but what `finish_decode` writes
    /// synchronously, or what the render it schedules writes: the render
    /// generation and `preview()`, the adopted document
    /// (`metadata_history()`, `metadata_draft()`), the error banner, and
    /// `decode_pending()`. Anything a **later** worker owns — the folder scan,
    /// a thumbnail — is decided *after* the decode settled, so a wait armed with
    /// the settled exit would report it as unreachable while the worker is still
    /// in flight (measured 2026-09-29, see the module docs). A caller that needs
    /// such a state uses [`Ready::new`] and [`pump`], whose only exit is the
    /// wall-clock bound.
    pub(crate) fn new(description: impl Into<String>, predicate: F) -> Self {
        DecodeSettled {
            ready: Ready::new(description, predicate),
        }
    }

    /// Arm the settled exit — **the only place in the tree that does**.
    ///
    /// Private, and reached only from [`pump_until_ready`], so an exit cannot be
    /// attached to a ready state a caller built for a later async source. The
    /// declared form is what this consumes: there is no path from
    /// [`Ready::new`] to here.
    fn into_ready(self) -> Ready<F> {
        Ready {
            settled_exit: Some(is_settled),
            ..self.ready
        }
    }
}

/// A ready state for a wait, plus the exit — if any — that may end it early.
///
/// The two forms of the same thing, and the difference is the contract:
///
/// * [`Ready::new`] builds one with **no** exit, so the wall-clock bound is all a
///   later-async state (a folder scan, a thumbnail worker) can be waited out by.
/// * [`DecodeSettled::new`] builds one *declared* decode-settled, and
///   [`DecodeSettled::into_ready`] is the single private door to the settled
///   exit — reachable only from [`pump_until_ready`], which takes nothing but
///   the declared form.
///
/// A caller that wants a scan to land cannot hand that state to
/// [`pump_until_ready`] at all: it is a type error, and the settled exit has no
/// other way in. See the module docs for the measurement behind that split.
pub(crate) struct Ready<F> {
    description: String,
    predicate: F,
    settled_exit: Option<fn(&LuminaApp) -> bool>,
}

impl<F: FnMut(&LuminaApp) -> bool> Ready<F> {
    /// Pair `predicate` with the `description` an expiry report prints, with **no**
    /// exit armed.
    ///
    /// Use this whenever the state comes from something other than the decode
    /// being waited for — a folder scan, a thumbnail worker. The wait that takes
    /// it ([`pump`]) has no other way to end early, so a later-async state is
    /// waited for instead of being declared unreachable.
    pub(crate) fn new(description: impl Into<String>, predicate: F) -> Self {
        Ready {
            description: description.into(),
            predicate,
            settled_exit: None,
        }
    }
}

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

/// Open `path` and wait for `ready` under `bound`, with the **settled exit**
/// armed.
///
/// The one wait in the tree that may end before its bound, and the only place in
/// the tree that arms an exit at all — which is why it takes [`DecodeSettled`]
/// and not [`Ready`]. A ready state fed by a later async source cannot reach
/// this function (type error), so the exit it arms can never report a state that
/// is still on its way as unreachable; see the module docs for the measurement
/// that moved the contract from prose to that split.
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
    ready: DecodeSettled<F>,
    bound: Duration,
) {
    harness.state_mut().open_file(path.display().to_string());
    // The single door to the settled exit: the declared form is converted here and
    // nowhere else, so no call site can arm it for a state of its own choosing.
    pump(harness, path, ready.into_ready(), bound);
}

/// Pump `harness` until `ready` holds, then return; give up loudly otherwise.
///
/// The one wait in the tree. It takes **no exit parameter**: what may end it early
/// is a property of the ready state it was handed, and only a state declared
/// decode-settled ([`DecodeSettled::new`], reachable from
/// [`pump_until_ready`]) carries one. A ready state built with [`Ready::new`] —
/// a folder scan, a thumbnail worker, anything the decode cannot decide — is
/// therefore waited out to the bound, and no call site can hand that state a
/// settled exit.
///
/// `subject` names what is being waited for in that report: the file being opened,
/// or the directory whose scan is landing. It is a snapshot taken by the caller, so
/// a wait on state the app owns can still name its target.
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
) {
    let deadline = Instant::now() + bound;
    let settled_exit = ready.settled_exit;
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
