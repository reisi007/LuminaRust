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
use crate::preview_size::{capped_source_max_dim, scale_roi_to_source};
use log::{trace, warn};

// R3-RENDER-SIZE-1 test seam: count the loud "viewport info missing" warnings so
// a headless test can prove the loud path fires exactly once (the production
// line stays `warn!`). Same capture-seam pattern as `render_tick`'s draft-error
// and `jank_log` counters.
#[cfg(test)]
thread_local! {
    static PREVIEW_CAP_MISSING_VIEWPORT_WARNS: std::cell::Cell<u32> =
        const { std::cell::Cell::new(0) };
}

fn note_preview_cap_missing_viewport() {
    #[cfg(test)]
    PREVIEW_CAP_MISSING_VIEWPORT_WARNS.with(|count| count.set(count.get() + 1));
}

/// Drains and returns the captured "missing viewport" warning count.
#[cfg(test)]
pub(crate) fn take_preview_cap_missing_viewport_warns() -> u32 {
    PREVIEW_CAP_MISSING_VIEWPORT_WARNS.with(|c| c.replace(0))
}

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
        if self.original.is_none() && self.sidecar_resolution_pending() {
            return Err(GuiError::Io(
                "cannot render while the file-backed sidecar is unresolved".into(),
            ));
        }
        let Some(original) = self.original.take() else {
            self.status = Str::NoImageLoaded.t().into();
            return Ok(());
        };
        // GUI-SRCACC-1: repair regions are full-source artifacts. Resolve and
        // validate them before changing any preview/session state; any missing,
        // stale, corrupt, or invalid record restores the original frame and
        // aborts loudly rather than rendering a recipe-only approximation.
        let source_actions = match self.resolve_current_source_actions(&original) {
            Ok(actions) => actions,
            Err(error) => {
                self.invalidate_source_action_preview();
                self.original = Some(original);
                return Err(GuiError::Io(error.to_string()));
            }
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
        //
        // R3-RENDER-SIZE-1 (B1): the same two stages are **absolute-frame**
        // stages whose artifacts are dimension-checked against the full source
        // (`apply_denoise_blend`: "frame dimensions must match artifact
        // exactly"; `composite_auto_fill`/`composite_expand`: artifact
        // width/height must match the frame / canvas exactly). Downscaling the
        // source would make the artifact blend/expand fail loudly or adopt a
        // mismatched canvas, so the preview cap is **suspended** for them (the
        // documented exception, alongside export and the 1:1 loupe) — the
        // artifact-aware CPU render is authoritative and runs at full frame
        // geometry.
        // GUI-SRCACC-1: repair-region dimensions are defined against the full
        // decoded source. Keep the same absolute-frame policy as denoise and
        // generative artifacts instead of downscaling a source while silently
        // leaving its action behind.
        let absolute_stage_active = self.generative_stage_active()
            || self.denoise_stage_active()
            || !source_actions.is_empty();
        let roi = if absolute_stage_active {
            if roi.is_some() {
                trace!(
                    "GUI render: absolute-frame stage active (generative/denoise/source action) — zoom ROI disabled"
                );
            }
            None
        } else {
            roi
        };
        self.preview_is_draft = false;
        self.pending_full_render = false;
        // R3-RENDER-SIZE-1 (User-Entscheid 2026-09-20): a preview render is
        // capped at the viewport resolution × device pixel ratio. Full source
        // resolution stays reserved for export, the 1:1 loupe and the
        // absolute-frame stages (denoise/generative) — none of these reach the
        // cap below. The cap downscales the *source* (the same leak-free path
        // the draft uses) and rescales the ROI window into that space; the
        // pan/zoom geometry is rescaled back identically, so the on-screen
        // placement is unchanged. Mask planes are full-resolution in core and
        // get bilinearly resampled to the render frame.
        //
        // A missing/degenerate viewport is loud: the render stays at source
        // resolution (correct, just not capped) and warns once per source —
        // never a silent uncapped or empty render.
        let (source, roi) = if absolute_stage_active {
            trace!(
                "GUI render: absolute-frame stage active — preview cap suspended (generative/denoise/source action)"
            );
            self.preview_cap_state.capped_src = None;
            (original.clone(), roi)
        } else {
            match self.preview_cap() {
                Some(cap) => {
                    match capped_source_max_dim(
                        original.width,
                        original.height,
                        roi.map_or(original.width, |r| r[2]),
                        roi.map_or(original.height, |r| r[3]),
                        cap,
                    ) {
                        Some(max_dim) => {
                            let capped = self.capped_preview_source(&original, max_dim);
                            let scaled_roi = roi.map(|r| {
                                scale_roi_to_source(
                                    r,
                                    (original.width, original.height),
                                    (capped.width, capped.height),
                                )
                            });
                            trace!(
                                "GUI render: full preview capped to {}x{} (viewport {}x{} @ dpr {:.2}, roi {:?} -> {:?})",
                                capped.width,
                                capped.height,
                                self.preview_pane_w,
                                self.preview_pane_h,
                                self.preview_cap_state.dpr,
                                roi,
                                scaled_roi
                            );
                            (capped, scaled_roi)
                        }
                        None => {
                            self.preview_cap_state.capped_src = None;
                            (original.clone(), roi)
                        }
                    }
                }
                None => {
                    if !self.preview_cap_state.warned {
                        self.preview_cap_state.warned = true;
                        note_preview_cap_missing_viewport();
                        warn!(
                            "GUI render: viewport info missing (pane {}x{} @ dpr {:.2}); rendering the full-resolution preview (R3-RENDER-SIZE-1 cap not applied)",
                            self.preview_pane_w, self.preview_pane_h, self.preview_cap_state.dpr
                        );
                    }
                    (original.clone(), roi)
                }
            }
        };
        let generative = match self.resolve_generative_artifacts(&original, &source_actions) {
            Ok(artifacts) => artifacts,
            Err(error) => {
                self.original = Some(original);
                return Err(error);
            }
        };
        // R3-LOG-1: wall time of the committed full render plus the output
        // dimensions — the heavy work behind a module switch / settled edit.
        let render_stopwatch = timing::Stopwatch::now();
        let result = self.render_from(&source, true, roi, generative, source_actions);
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
