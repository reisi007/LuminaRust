//! R3-LOG-1 (Release 1.0): wall-clock instrumentation for module switches and
//! the heavy work behind them (background decode, preview-index build,
//! thumbnail enqueue→ready, full render, texture upload, VRAM present refusal).
//!
//! **Logging only.** Every helper here measures and logs; no render, recipe,
//! sidecar, persistence or routing decision reads a timing value. The two
//! fields in [`TimingState`] are pure anchors that must survive across frames
//! or methods (the switch event until the first painted frame, the decode start
//! until the worker result lands); everything else is a local [`Stopwatch`].
//!
//! Levels follow the project rule: hot-path/per-tick detail is `trace!`, a
//! user-visible state change stays `info!`/`warn!`. The wall-clock reads stay
//! off the per-pixel path — only at the named boundaries (switch, decode,
//! folder-index build, thumbnail-ready, committed full render, one texture
//! upload per identity change). All deltas are formatted with one decimal
//! millisecond via [`format_ms`], so a manual `RUST_LOG=trace` run is greppable.
//!
//! [`Stopwatch`] takes its start [`Instant`] explicitly, so headless tests pin
//! the delta math against an injected clock (DoD §2) instead of racing the
//! scheduler. [`emit`] both logs and (test-only) records every line so each
//! call site has a firing assertion.

use super::*;
use log::{info, trace};
use std::time::Instant;

/// One-decimal millisecond format shared by every R3-LOG-1 line.
pub(crate) fn format_ms(milliseconds: f64) -> String {
    format!("{milliseconds:.1}")
}

/// Wall-clock stopwatch with an injectable start instant.
///
/// Production uses [`Self::now`]; tests use [`Self::at`] with a fixed instant
/// and [`Self::elapsed_ms_at`] to prove the delta math exactly.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stopwatch {
    start: Instant,
}

impl Stopwatch {
    /// Start from the current wall clock.
    pub(crate) fn now() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Start at an explicit instant (test seam / cross-method anchor).
    pub(crate) fn at(start: Instant) -> Self {
        Self { start }
    }

    /// Elapsed milliseconds since the start at the current wall clock.
    pub(crate) fn elapsed_ms(&self) -> f64 {
        Self::delta_ms(self.start, Instant::now())
    }

    /// Elapsed milliseconds since the start at the given instant (test seam).
    #[cfg(test)]
    pub(crate) fn elapsed_ms_at(&self, now: Instant) -> f64 {
        Self::delta_ms(self.start, now)
    }

    /// Pure delta in milliseconds (never negative, even if a caller passes an
    /// out-of-order pair).
    pub(crate) fn delta_ms(start: Instant, end: Instant) -> f64 {
        end.saturating_duration_since(start).as_secs_f64() * 1000.0
    }
}

/// Cross-frame timing anchors. `Default` = no switch/decode in flight.
#[derive(Default)]
pub(crate) struct TimingState {
    /// Pending module-switch event: module + event instant, consumed by the
    /// first painted frame after the switch.
    module_switch: Option<(Module, Instant)>,
    /// In-flight background decode: path + enqueue instant.
    decode: Option<(String, Instant)>,
    /// R4-WARN-1: the last VRAM present-refusal reason that already produced a
    /// `warn!`. Survives `mark_dirty`/`set_adjustment` (which clear the
    /// per-frame `vram_render_refusal`), so a render-key change with an
    /// unchanged reason does not re-warn; a successful present re-arms it.
    /// Hosted next to the [`LuminaApp::note_vram_refusal`] throttle (same
    /// cohesion), never read by a render/routing decision. GPU-only: the
    /// writer lives in the `gpu`-gated `note_vram_refusal`.
    #[cfg(feature = "gpu")]
    present_refusal_warned: Option<String>,
    /// GPU-ROUTE-LOG-54: the last **editorial** present-refusal reason
    /// (`mask_gate`, `before_after`, `preview_roi`, `vram_stale`) that already
    /// produced a `trace!`. A separate memo from
    /// [`TimingState::present_refusal_warned`] on purpose: that one throttles
    /// *capability* `warn!`s, this one throttles *editorial* `trace!`s, and the
    /// two throttle independently so neither can suppress the other. Same
    /// cohesion as its writer (the `gpu`-gated
    /// `note_editorial_present_refusal`) and never read by a routing decision.
    #[cfg(feature = "gpu")]
    editorial_refusal_traced: Option<String>,
    /// R5-WARN-2: the last denoise fallback `(status, reason)` that already
    /// produced a core `warn!`. The GUI render loop resolves the denoise state
    /// every tick; without this memo the core `fail_or_fallback` warned once per
    /// tick inside a slider drag. `Ready`/`Inactive` re-arm it (R4-WARN-1
    /// pattern) so a genuine recurrence stays visible.
    denoise_refusal_warned: Option<(String, String)>,
}

// ---- Pure log-line builders ----
//
// File-size-ratchet extraction (GPU-ROUTE-LOG-54): the pure format functions
// moved to `timing_log_lines.rs` so the format of every trace line lives in one
// readable place, separate from the stateful anchors/throttles below. Re-exported
// rather than re-imported at the call sites, so this stays a relocation.
pub(crate) use super::timing_log_lines::*;

// ---- Emission + test capture seam ----

/// Emit one measurement line at `trace!` (hot-path-safe) and, in tests, record
/// it so every instrumented call site has a firing assertion. The closure is
/// only evaluated when it will be logged (or captured), so a disabled `trace`
/// level costs no formatting/allocation.
pub(crate) fn emit(line: impl FnOnce() -> String) {
    #[cfg(test)]
    {
        let line = line();
        trace!("{line}");
        TIMING_LOG.with(|log| log.borrow_mut().push(line));
    }
    #[cfg(not(test))]
    {
        if log::log_enabled!(log::Level::Trace) {
            trace!("{}", line());
        }
    }
}

#[cfg(test)]
thread_local! {
    static TIMING_LOG: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Drains and returns the captured R3-LOG-1 lines of the current test thread.
#[cfg(test)]
pub(crate) fn take_timing_log() -> Vec<String> {
    TIMING_LOG.with(|log| std::mem::take(&mut *log.borrow_mut()))
}

// ---- R3-DENOISE-1: single app-level neighbor-failure event ----

/// Count one app-level neighbor-failure `warn!` (no-op outside tests, so the
/// call site stays a single unconditional line). Proves the former two-level
/// double warning (controller + app) is now exactly one event.
pub(crate) fn note_neighbor_failure_warn() {
    #[cfg(test)]
    NEIGHBOR_FAILURE_WARNS.with(|warns| warns.set(warns.get() + 1));
}

#[cfg(test)]
thread_local! {
    static NEIGHBOR_FAILURE_WARNS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Drains the test-only app-level failure-warn count.
#[cfg(test)]
pub(crate) fn take_neighbor_failure_warns() -> u32 {
    NEIGHBOR_FAILURE_WARNS.with(|warns| warns.replace(0))
}

// ---- Module switch (state setter hosted here, cohesive with its timing) ----

impl LuminaApp {
    /// Set the active top-level module (Library / Develop / Export). Used by the
    /// headless snapshot tests (F-103-N9) to render a specific module; this is a
    /// pure state assignment with no recipe/sidecar side effects.
    ///
    /// R3-LOG-1: records the switch instant and logs the event with the module
    /// name; [`Self::note_first_paint_after_switch`] closes the event → first
    /// paint delta.
    ///
    /// R3-OPEN-1: a switch *to* Develop with exactly one filmstrip selection
    /// (≠ the loaded image) opens that image through the shared
    /// [`Self::open_file`] path — see
    /// [`Self::open_develop_selection_on_switch`]. This is the single funnel
    /// for every Develop entry point (module bar, `D`, grid double-click,
    /// startup wiring).
    pub fn set_module(&mut self, module: Module) {
        instrument_gui_action!(self, GuiAction::SetModule);
        let changed = self.active_module != module;
        self.timing.module_switch = Some((module, Instant::now()));
        emit(|| module_switch_event_line(module));
        self.active_module = module;
        if changed {
            self.open_develop_selection_on_switch(module);
        }
    }

    /// Current top-level module (read-only accessor for the `main()` startup
    /// wiring and headless tests; mirrors [`Self::set_module`]).
    pub fn module(&self) -> Module {
        self.active_module
    }

    /// R3-LOG-1: close a pending module switch at the end of the first painted
    /// frame after it. A no-op without a pending switch, so it can be called
    /// unconditionally from the frame body.
    pub(crate) fn note_first_paint_after_switch(&mut self) {
        let Some((module, at)) = self.timing.module_switch.take() else {
            return;
        };
        emit(|| module_first_paint_line(module, Stopwatch::at(at).elapsed_ms()));
    }

    /// R3-LOG-1: start the wall clock for a background decode (the existing
    /// `info!` user-visible line moves here unchanged).
    pub(crate) fn note_decode_start(&mut self, path: &str) {
        info!("decoding (background) {path}");
        self.timing.decode = Some((path.to_owned(), Instant::now()));
        emit(|| decode_start_line(path));
    }

    /// R3-LOG-1: finish the pending decode with the decoded resolution.
    pub(crate) fn note_decode_finish(&mut self, width: u32, height: u32) {
        let Some((path, at)) = self.timing.decode.take() else {
            return;
        };
        emit(|| decode_done_line(&path, Stopwatch::at(at).elapsed_ms(), width, height));
    }

    /// R3-LOG-1: drop the pending decode anchor on failure (the error banner
    /// already carries the loud user-visible report).
    pub(crate) fn note_decode_failed(&mut self) {
        let Some((path, at)) = self.timing.decode.take() else {
            return;
        };
        emit(|| decode_failed_line(&path, Stopwatch::at(at).elapsed_ms()));
    }
}

// ---- VRAM present-refusal throttle (R3-ROUTING-1 immediate measure) ----

#[cfg(all(test, feature = "gpu"))]
thread_local! {
    static VRAM_REFUSAL_WARNS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Test-only count of `warn!` emissions from [`LuminaApp::note_vram_refusal`].
#[cfg(all(test, feature = "gpu"))]
pub(crate) fn take_vram_refusal_warns() -> u32 {
    VRAM_REFUSAL_WARNS.with(|warns| warns.replace(0))
}

#[cfg(all(test, feature = "gpu"))]
thread_local! {
    static GPU_GATE_ROUTE_WARNS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// R3-DENOISE-2: count one recipe-gate CPU-route `warn!` (the deduped line in
/// `refresh_gpu_stage_gate`). Test-only, so the call site stays a single line.
#[cfg(all(test, feature = "gpu"))]
pub(crate) fn note_gpu_gate_route_warn() {
    GPU_GATE_ROUTE_WARNS.with(|warns| warns.set(warns.get() + 1));
}

/// Drains the test-only recipe-gate CPU-route warn count.
#[cfg(all(test, feature = "gpu"))]
pub(crate) fn take_gpu_gate_route_warns() -> u32 {
    GPU_GATE_ROUTE_WARNS.with(|warns| warns.replace(0))
}

#[cfg(feature = "gpu")]
impl LuminaApp {
    /// R3-ROUTING-1: record a *known* VRAM present refusal. Returns `true`
    /// exactly once per refusal-state change (the single loud `warn!` lives
    /// here) and `false` for a per-tick repeat of the same stage, which is only
    /// `trace!`d. This collapses the former per-tick `warn!` spam (38 identical
    /// lines/session) while the stage name (`geometry`, `generative_edit`, …)
    /// stays in every line.
    ///
    /// R4-WARN-1: the state-change check above re-armed on every edit, because
    /// `mark_dirty`/`set_adjustment` clear `vram_render_refusal` (the recipe may
    /// have changed). A persistent reason (e.g. a committed dimension-changing
    /// crop outside the crop tool) then warned once per zoom-drag tick (35× in
    /// ~6 s). A separate memo ([`TimingState::present_refusal_warned`]) survives
    /// the render-key change and warns once per reason; a successful present
    /// re-arms it via [`Self::clear_present_refusal_warn`].
    pub(crate) fn note_vram_refusal(&mut self, stage: &str) -> bool {
        self.vram_fresh = false;
        let changed = self.vram_render_refusal.as_deref() != Some(stage);
        self.vram_render_refusal = Some(stage.to_owned());
        let already_warned = self.timing.present_refusal_warned.as_deref() == Some(stage);
        if changed && !already_warned {
            log::warn!("gpu present refused, keeping CPU route: {stage}");
            self.timing.present_refusal_warned = Some(stage.to_owned());
            #[cfg(test)]
            VRAM_REFUSAL_WARNS.with(|warns| warns.set(warns.get() + 1));
        } else {
            trace!("GUI timing: vram present refusal unchanged stage={stage}");
        }
        changed
    }

    /// R4-WARN-1: re-arm the present-refusal warn after a successful VRAM
    /// present (`vram_fresh = true`). A reason that reappears after the GPU
    /// path worked again is a genuinely new occurrence and must be visible.
    pub(crate) fn clear_present_refusal_warn(&mut self) {
        self.timing.present_refusal_warned = None;
    }

    /// GPU-ROUTE-LOG-54: name the **editorial** gate that closed the VRAM
    /// present path this frame, once per reason change.
    ///
    /// The editorial routes are silent on screen by design (see
    /// `present::editorial_refusal`), so this trace line is the only place the
    /// decision is explainable — an `RUST_LOG=trace` acceptance run otherwise
    /// cannot tell "the GPU was never asked" from "the GPU said no". The
    /// throttle is the [`TimingState::present_refusal_warned`] pattern, and it
    /// throttles the **counted emit**, not the log volume: a repeat of the same
    /// reason still logs one `unchanged` line per frame, so a long before/after
    /// session stays traceable frame by frame rather than silent. What the memo
    /// buys is that a *change* of reason is never lost in that stream, and that
    /// the counter a test observes means "reason changes", not "frames". A reason
    /// that recurs after a GPU-present frame is re-armed by
    /// [`Self::clear_editorial_present_refusal`].
    ///
    /// `trace!` rather than `warn!` on purpose: these are deliberate, expected
    /// routes, and a warning per frame is exactly the spam this task removes.
    /// The capability routes that *do* warn keep their own `warn!`.
    #[cfg(feature = "gpu")]
    pub(crate) fn note_editorial_present_refusal(&mut self, reason: &str) {
        if self.timing.editorial_refusal_traced.as_deref() == Some(reason) {
            trace!("GUI timing: editorial present refusal unchanged reason={reason}");
            return;
        }
        trace!("GUI timing: editorial present refusal, keeping CPU route reason={reason}");
        self.timing.editorial_refusal_traced = Some(reason.to_owned());
        #[cfg(all(test, feature = "gpu"))]
        EDITORIAL_REFUSAL_TRACES.with(|traces| traces.set(traces.get() + 1));
    }

    /// GPU-ROUTE-LOG-54: re-arm the editorial trace after a frame in which no
    /// editorial gate closed. Without this, toggling Before/After repeatedly
    /// would report `before_after` only once for the whole session.
    #[cfg(feature = "gpu")]
    pub(crate) fn clear_editorial_present_refusal(&mut self) {
        self.timing.editorial_refusal_traced = None;
    }
}

// GPU-ROUTE-LOG-54: test-only count of the *counted* editorial refusal emits,
// i.e. reason CHANGES, not frames: the counter sits behind the `unchanged`
// early-return, and an `unchanged` frame still logs a line. A test can therefore
// prove the throttle fires on a change and stays quiet on a repeat. Without the
// counter the debounce could silently degrade into "never logs" while every
// test still passed. A plain `//` comment rather than `///`: a doc comment on a
// `thread_local!` item is an `unused_doc_comments` warning, because the macro
// cannot emit documentation for it.
#[cfg(all(test, feature = "gpu"))]
thread_local! {
    static EDITORIAL_REFUSAL_TRACES: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Drains the test-only editorial-refusal trace count.
#[cfg(all(test, feature = "gpu"))]
pub(crate) fn take_editorial_refusal_traces() -> u32 {
    EDITORIAL_REFUSAL_TRACES.with(|traces| traces.replace(0))
}

// ---- R5-WARN-2: denoise fallback-warn throttle (the Denoise-Gate path) ----

#[cfg(test)]
thread_local! {
    static DENOISE_REFUSAL_WARNS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Test-only count of denoise fallback warnings that would be emitted (the
/// first occurrence of each distinct `(status, reason)`).
#[cfg(test)]
pub(crate) fn take_denoise_refusal_warns() -> u32 {
    DENOISE_REFUSAL_WARNS.with(|warns| warns.replace(0))
}

impl LuminaApp {
    /// R5-WARN-2 (R4-WARN-1 pattern on the Denoise-Gate path): record a
    /// denoise fallback `(status, reason)`. Returns `true` exactly once per
    /// distinct pair — the `DenoiseStageInput::quiet` flag is then `false`, so
    /// the core emits its `warn!` — and `false` for a per-tick repeat, which is
    /// suppressed. `Ready`/`Inactive` re-arm via
    /// [`Self::clear_denoise_refusal_warn`].
    pub(crate) fn note_denoise_refusal(&mut self, status: &str, reason: &str) -> bool {
        let key = (status.to_owned(), reason.to_owned());
        if self.timing.denoise_refusal_warned.as_ref() == Some(&key) {
            return false;
        }
        self.timing.denoise_refusal_warned = Some(key);
        #[cfg(test)]
        DENOISE_REFUSAL_WARNS.with(|warns| warns.set(warns.get() + 1));
        true
    }

    /// R5-WARN-2: re-arm the denoise fallback warn once the stage is
    /// `Ready`/`Inactive` (a later recurrence must be visible).
    pub(crate) fn clear_denoise_refusal_warn(&mut self) {
        self.timing.denoise_refusal_warned = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> LuminaApp {
        let mut app = LuminaApp::new(egui::Context::default());
        let pixels = ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap();
        app.load_bytes(pixels, "timing-test.png").unwrap();
        app
    }

    /// R3-LOG-1: a decoded frame closes the pending decode anchor (the full
    /// value/format assertions live in `tests::timing_instrumentation`).
    #[test]
    fn decode_anchor_is_consumed_by_a_finish() {
        let mut app = app();
        let _ = take_timing_log();
        app.note_decode_start("photo.png");
        app.note_decode_finish(2, 1);
        assert!(
            take_timing_log()
                .iter()
                .any(|line| line.contains("decode done") && line.contains("resolution=2x1")),
            "the decode finish must be logged"
        );
    }
}
