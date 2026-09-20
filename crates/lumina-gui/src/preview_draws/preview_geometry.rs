//! R5-FIX-WELLE-20 / R4-NAV-1: pure preview pan/placement geometry helpers,
//! extracted from `preview_draws.rs` (file-size ratchet).
//!
//! The two functions here pin the two root causes behind the "navigator box
//! drag does not move the view" finding, on the real draw path:
//!
//! * [`LuminaApp::clamp_preview_pan`] — the gutter guard. It must compare
//!   against the **full source** draw size. The previous inline clamp used the
//!   on-screen size of the painted texture, which at zoom is an ROI crop only
//!   [`crate::PREVIEW_ROI_MARGIN`]× the pane — that capped every pan at ~15 %
//!   of a pane, so the navigator window (mapped 1:1 from the pan) barely moved
//!   even though the R4 gate was satisfied.
//! * [`LuminaApp::roi_rendered_pan`] — the pan already baked into the current
//!   ROI crop. `roi_from_zoom` centres the crop on `w/2 - pan/scale`, so its
//!   inverse recovers the pan the texture was rendered for. An ROI crop must be
//!   placed by `live_pan - rendered_pan`: adding the live pan on top of the
//!   crop (the old code) moved the image 2× the cursor and made the navigator
//!   box disagree with the visible view.
//!
//! Both are `pub(crate)` pure helpers so the headless navigator-drag test can
//! drive the real `draw_preview`/`draw_navigator` path and assert on them.

use super::*;

impl LuminaApp {
    /// Clamp the preview pan (screen points) so the FULL source image still
    /// covers the pane (no empty gutters). A view that is not genuinely
    /// magnified (`zoom <= 1`) or whose full draw fits the pane on an axis
    /// stays centred on that axis (pan `0`). This is the R4-NAV-1 fix: the
    /// guard is about the image that is being viewed, never the ROI-crop
    /// stand-in that only samples part of it.
    pub(crate) fn clamp_preview_pan(
        pan: egui::Vec2,
        zoom: f32,
        full_w: f32,
        full_h: f32,
        scale: f32,
        pane_w: f32,
        pane_h: f32,
    ) -> egui::Vec2 {
        if zoom <= 1.0
            || full_w <= 0.0
            || full_h <= 0.0
            || scale <= 0.0
            || pane_w <= 0.0
            || pane_h <= 0.0
        {
            return egui::Vec2::ZERO;
        }
        let limit_x = ((full_w * scale - pane_w) / 2.0).max(0.0);
        let limit_y = ((full_h * scale - pane_h) / 2.0).max(0.0);
        egui::vec2(
            pan.x.clamp(-limit_x, limit_x),
            pan.y.clamp(-limit_y, limit_y),
        )
    }

    /// The pan offset already baked into the current ROI crop (`ZERO` without
    /// one). `roi_from_zoom` sets the crop centre to `w/2 - pan/scale`; the
    /// inverse gives the pan the texture was rendered for, in screen points.
    /// Used to place an ROI crop as `live_pan - rendered_pan`, which keeps it
    /// glued to the pane in steady state while a fresh drag still moves the
    /// stale texture for immediate feedback.
    pub(crate) fn roi_rendered_pan(
        roi: Option<[u32; 4]>,
        full_w: u32,
        full_h: u32,
        render_src: Option<(u32, u32)>,
        scale: f32,
    ) -> egui::Vec2 {
        let Some(roi) = roi else {
            return egui::Vec2::ZERO;
        };
        if scale <= 0.0 || full_w == 0 || full_h == 0 {
            return egui::Vec2::ZERO;
        }
        let full = Self::roi_in_full_pixels(roi, full_w, full_h, render_src);
        let cx = full[0] as f32 + full[2] as f32 / 2.0;
        let cy = full[1] as f32 + full[3] as f32 / 2.0;
        egui::vec2(
            (full_w as f32 / 2.0 - cx) * scale,
            (full_h as f32 / 2.0 - cy) * scale,
        )
    }
}
