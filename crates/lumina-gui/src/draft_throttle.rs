//! R2-JANK-1 F1/F4: frame-budget throttle for the interactive draft render
//! (F1) and cadence throttle for the draft analysis pass (F4).
//!
//! The pointer-drag hot path (`render_draft_tick`) used to run the CPU draft
//! render on **every** repaint, so a fast drag re-rendered the draft inside the
//! same frame budget and starved the UI. [`DraftThrottle`] is the pure,
//! headless-testable decision state: at most one draft render per
//! [`DRAFT_RENDER_BUDGET_SECONDS`], and the tone/histogram analysis pass at
//! most once per [`DRAFT_ANALYSIS_PERIOD_SECONDS`] while dragging.
//!
//! No silent state: a throttled draft render leaves the render key invalid, so
//! the existing "Stale" badge keeps advertising that the displayed pixels
//! trail the recipe; a skipped analysis pass keeps the previous analysis and
//! sets the visible [`DRAFT_ANALYSIS_PENDING_LABEL`] marker.
//!
//! The state is timesource-injected (`observe(now)`), never reads a wall clock
//! itself, so `DoD.md` §2 is satisfied: a test drives the time-based path
//! directly.

/// F1: minimum interval between two CPU draft renders (one per frame budget).
pub(crate) const DRAFT_RENDER_BUDGET_SECONDS: f64 = 0.016;

/// F4: minimum interval between two draft analysis passes while dragging.
/// The previous analysis stays displayed in between (marked pending).
pub(crate) const DRAFT_ANALYSIS_PERIOD_SECONDS: f64 = 0.150;

/// F4: visible preview-state marker while the displayed tone analysis predates
/// the currently displayed draft pixels.
pub(crate) const DRAFT_ANALYSIS_PENDING_LABEL: &str = "Draft (analysis pending)";

/// Timesource-injected state of the F1 frame-budget throttle and the F4
/// analysis cadence. `Default` allows the very first render/analysis
/// immediately (`NEG_INFINITY` anchors).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DraftThrottle {
    /// Current frame time (egui seconds), set once per draft tick.
    now: f64,
    /// Timestamp of the last executed draft render (`NEG_INFINITY` = none yet).
    last_render_time: f64,
    /// Timestamp of the last executed draft analysis pass.
    last_analysis_time: f64,
    /// F4: the previous analysis is older than the displayed draft pixels.
    analysis_pending: bool,
    /// One-shot request consumed by `render_from` to skip the analysis pass.
    skip_analysis: bool,
    /// F4: the last completed analysis. A throttled draft restores it (marked
    /// pending) instead of flickering to an empty histogram, so the displayed
    /// measurement is never silently blank.
    retained_analysis: Option<lumina_core::ToneAnalysis>,
    /// F4: histogram companion of [`Self::retained_analysis`].
    retained_histogram: Option<lumina_core::LuminanceHistogram>,
}

impl Default for DraftThrottle {
    fn default() -> Self {
        Self {
            now: 0.0,
            last_render_time: f64::NEG_INFINITY,
            last_analysis_time: f64::NEG_INFINITY,
            analysis_pending: false,
            skip_analysis: false,
            retained_analysis: None,
            retained_histogram: None,
        }
    }
}

impl DraftThrottle {
    /// Set the frame's time base (egui seconds). Called once per draft tick
    /// before the due checks; never reads a clock itself.
    pub(crate) fn observe(&mut self, now: f64) {
        self.now = now;
    }

    /// F1: true when the frame budget elapsed since the last draft render (or
    /// none ran yet), so this tick may render.
    pub(crate) fn render_due(&self) -> bool {
        self.now - self.last_render_time >= DRAFT_RENDER_BUDGET_SECONDS
    }

    /// F1: record that a draft render just started at `now`.
    pub(crate) fn note_render(&mut self) {
        self.last_render_time = self.now;
    }

    /// F4: arm the one-shot analysis skip when the last analysis is younger
    /// than [`DRAFT_ANALYSIS_PERIOD_SECONDS`]. Consumed by `render_from`.
    pub(crate) fn prepare_analysis(&mut self) {
        self.skip_analysis = self.now - self.last_analysis_time < DRAFT_ANALYSIS_PERIOD_SECONDS;
    }

    /// F4: read and clear the one-shot skip request (always clears, so a later
    /// full render can never inherit a stale skip).
    pub(crate) fn take_skip_analysis(&mut self) -> bool {
        std::mem::take(&mut self.skip_analysis)
    }

    /// F4: an analysis pass ran — the displayed analysis is current again.
    /// The retained snapshot is updated separately by
    /// [`Self::retain_analysis`] (it survives the next `mark_dirty`).
    pub(crate) fn note_analysis(&mut self) {
        self.last_analysis_time = self.now;
        self.analysis_pending = false;
        self.skip_analysis = false;
    }

    /// F4: remember the last completed analysis so a later throttled draft can
    /// keep displaying it (marked pending) instead of an empty histogram.
    pub(crate) fn retain_analysis(
        &mut self,
        analysis: lumina_core::ToneAnalysis,
        histogram: &lumina_core::LuminanceHistogram,
    ) {
        self.retained_analysis = Some(analysis);
        self.retained_histogram = Some(histogram.clone());
    }

    /// F4: the retained analysis + histogram, if any analysis ever completed.
    pub(crate) fn retained(
        &self,
    ) -> Option<(lumina_core::ToneAnalysis, lumina_core::LuminanceHistogram)> {
        Some((self.retained_analysis?, self.retained_histogram.clone()?))
    }

    /// F4: the analysis pass was skipped — the previous analysis stays
    /// displayed and is visibly marked as pending.
    pub(crate) fn note_analysis_pending(&mut self) {
        self.analysis_pending = true;
    }

    /// F4: whether the preview must advertise the pending analysis marker.
    pub(crate) fn analysis_pending(&self) -> bool {
        self.analysis_pending
    }

    /// Test/inspection readout of the F1 anchor.
    #[cfg(test)]
    pub(crate) fn last_render_time(&self) -> f64 {
        self.last_render_time
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_render_is_always_due_then_budget_gates_it() {
        let mut throttle = DraftThrottle::default();
        throttle.observe(10.0);
        assert!(throttle.render_due(), "no render yet → always due");
        throttle.note_render();
        throttle.observe(10.005);
        assert!(!throttle.render_due(), "inside the 16 ms budget");
        throttle.observe(10.016);
        assert!(throttle.render_due(), "at the budget boundary");
        throttle.observe(10.020);
        assert!(throttle.render_due(), "past the budget");
    }

    #[test]
    fn analysis_skip_is_one_shot_and_reset_by_a_real_pass() {
        let mut throttle = DraftThrottle::default();
        throttle.observe(1.0);
        throttle.prepare_analysis();
        assert!(
            !throttle.take_skip_analysis(),
            "the first analysis is always due"
        );
        throttle.note_analysis();
        assert!(!throttle.analysis_pending());

        // 50 ms later: render allowed, analysis still inside the 150 ms window.
        throttle.observe(1.05);
        throttle.prepare_analysis();
        assert!(throttle.take_skip_analysis());
        throttle.note_analysis_pending();
        assert!(throttle.analysis_pending());
        assert!(!throttle.take_skip_analysis(), "skip is one-shot");

        // A completed pass clears the marker.
        throttle.note_analysis();
        assert!(!throttle.analysis_pending());
    }

    #[test]
    fn analysis_due_after_period() {
        let mut throttle = DraftThrottle::default();
        throttle.observe(5.0);
        throttle.prepare_analysis();
        let _ = throttle.take_skip_analysis();
        throttle.note_analysis();
        throttle.observe(5.0 + DRAFT_ANALYSIS_PERIOD_SECONDS);
        throttle.prepare_analysis();
        assert!(
            !throttle.take_skip_analysis(),
            "at the period boundary the analysis runs again"
        );
    }
}
