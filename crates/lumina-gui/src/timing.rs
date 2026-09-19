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
use std::path::Path;
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
}

// ---- Pure log-line builders (single source of truth for the format) ----

pub(crate) fn module_switch_event_line(module: Module) -> String {
    format!("GUI timing: module switch event module={module:?}")
}

pub(crate) fn module_first_paint_line(module: Module, ms: f64) -> String {
    format!(
        "GUI timing: module switch first paint module={module:?} switch_to_paint_ms={}",
        format_ms(ms)
    )
}

pub(crate) fn decode_start_line(path: &str) -> String {
    format!("GUI timing: decode start path={path}")
}

pub(crate) fn decode_done_line(path: &str, ms: f64, width: u32, height: u32) -> String {
    format!(
        "GUI timing: decode done path={path} decode_ms={} resolution={width}x{height}",
        format_ms(ms)
    )
}

pub(crate) fn decode_failed_line(path: &str, ms: f64) -> String {
    format!(
        "GUI timing: decode failed path={path} decode_ms={}",
        format_ms(ms)
    )
}

pub(crate) fn preview_index_line(folder: &Path, entries: usize, ms: f64) -> String {
    format!(
        "GUI timing: preview index built folder={} entries={entries} build_ms={}",
        folder.display(),
        format_ms(ms)
    )
}

pub(crate) fn thumbnail_ready_line(key: &str, ms: f64) -> String {
    format!(
        "GUI timing: thumbnail ready key={key} enqueue_to_ready_ms={}",
        format_ms(ms)
    )
}

pub(crate) fn full_render_line(ms: f64, width: u32, height: u32) -> String {
    format!(
        "GUI timing: full render done render_ms={} output={width}x{height}",
        format_ms(ms)
    )
}

pub(crate) fn texture_upload_line(target: &str, bytes: usize) -> String {
    format!("GUI timing: texture upload target={target} bytes={bytes}")
}

pub(crate) fn texture_upload_skip_line(target: &str, saved_bytes: usize) -> String {
    format!("GUI timing: texture upload skipped target={target} saved_bytes={saved_bytes}")
}

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

#[cfg(feature = "gpu")]
impl LuminaApp {
    /// R3-ROUTING-1: record a *known* VRAM present refusal. Returns `true`
    /// exactly once per refusal-state change (the single loud `warn!` lives
    /// here) and `false` for a per-tick repeat of the same stage, which is only
    /// `trace!`d. This collapses the former per-tick `warn!` spam (38 identical
    /// lines/session) while the stage name (`geometry`, `generative_edit`, …)
    /// stays in every line.
    pub(crate) fn note_vram_refusal(&mut self, stage: &str) -> bool {
        self.vram_fresh = false;
        let changed = self.vram_render_refusal.as_deref() != Some(stage);
        self.vram_render_refusal = Some(stage.to_owned());
        if changed {
            log::warn!("gpu present refused, keeping CPU route: {stage}");
            #[cfg(test)]
            VRAM_REFUSAL_WARNS.with(|warns| warns.set(warns.get() + 1));
        } else {
            trace!("GUI timing: vram present refusal unchanged stage={stage}");
        }
        changed
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
