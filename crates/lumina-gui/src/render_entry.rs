//! GUI-REFACTOR-W1-20 S1.2b (entry): the committed full-quality render entry
//! points `render` / `render_full`, extracted verbatim from `lib.rs`.
//!
//! `render` is the viewport-less convenience entry; `render_full` resolves the
//! zoom ROI, forces the un-cropped frame for absolute-frame stages
//! (generative/denoise) and delegates to `render_pipeline::render_from`. Both
//! stay `pub` (the app root, the GUI harness and the integration tests call
//! them). The pipeline hub itself lives in `render_pipeline.rs` — split out
//! because the combined file would exceed the strict 500-line rule for new
//! files (no new baseline entry).

use super::*;
use log::trace;

impl LuminaApp {
    pub fn render(&mut self) -> Result<(), GuiError> {
        self.render_full([0, 0], None)
    }

    /// Full-resolution render of the committed source. Used on load, after a
    /// slider drag settles (mouse-up / idle) and by every explicit re-render.
    /// `viewport` is the preview pane size (used for ROI clamping / logging);
    /// `roi` is an optional `(x, y, w, h)` crop (source pixels) when zoomed in.
    pub fn render_full(
        &mut self,
        _viewport: [u32; 2],
        roi: Option<[u32; 4]>,
    ) -> Result<(), GuiError> {
        let Some(original) = self.original.take() else {
            self.status = Str::NoImageLoaded.t().into();
            return Ok(());
        };
        // GUI-JANKLOG-19: a full render outside an action scope (the 150 ms
        // debounce commit) is an outermost `kind=render` scope; inside an
        // action it nests and fills that action's record.
        #[cfg(all(feature = "janklog", debug_assertions))]
        let _jank = {
            let (route, badge) = self.jank_route_and_badge();
            let key = self
                .pending_slider_commit
                .as_ref()
                .map(|(key, _)| key.as_str());
            jank_log::JankScope::enter(jank_log::JankKind::Render, None, route, badge, key)
        };
        // Derive the ROI from the zoom factor and pan offset when no explicit
        // crop was given (PERF-GUI-5, REVIEW-GUI-PANROI-1); the full render
        // always honours masks.
        let roi = roi.or_else(|| {
            Self::roi_from_zoom(
                original.width,
                original.height,
                self.preview_zoom,
                self.preview_pan,
                self.preview_pane_w,
                self.preview_pane_h,
            )
        });
        // GEN-ONNX-1 Welle 2b: a generative canvas is absolute geometry on the
        // full-resolution source (its identity and dimensions are defined
        // there). A zoom ROI crop cannot host it, so the full frame is
        // rendered instead of silently skipping the generative stage.
        // LRPAR-G14-DENOISE-IMPL-20: the persisted `denoise_rgb` artifact is
        // likewise full-frame (its checksum/dimensions are defined there), so
        // an active denoise stage also forces the un-cropped render.
        let roi = if self.generative_stage_active() || self.denoise_stage_active() {
            if roi.is_some() {
                trace!(
                    "GUI render: absolute-frame stage active (generative/denoise) — zoom ROI disabled"
                );
            }
            None
        } else {
            roi
        };
        self.preview_is_draft = false;
        self.pending_full_render = false;
        let generative = match self.resolve_generative_artifacts(&original) {
            Ok(artifacts) => artifacts,
            Err(error) => {
                self.original = Some(original);
                return Err(error);
            }
        };
        // R3-LOG-1: wall time of the committed full render plus the output
        // dimensions — the heavy work behind a module switch / settled edit.
        let render_stopwatch = timing::Stopwatch::now();
        let result = self.render_from(&original, true, roi, generative);
        let render_ms = render_stopwatch.elapsed_ms();
        self.original = Some(original);
        if result.is_ok() {
            if let Some(preview) = self.preview.as_ref() {
                timing::emit(|| timing::full_render_line(render_ms, preview.width, preview.height));
            }
        }
        // GUI-JANKLOG-19: the full render's analysis pass flows into the same
        // jank record as the enclosing action/render scope.
        #[cfg(all(feature = "janklog", debug_assertions))]
        jank_log::note_analyse_ms(self.last_analysis_ms);
        result
    }
}
