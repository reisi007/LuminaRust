//! Pointer-to-source coordinate mapping for the preview tools.

use super::*;

impl LuminaApp {
    /// Map a pointer position to normalized (0..=1) *full-frame* source
    /// coordinates. The displayed rect may be only a zoomed/panned sub-crop of
    /// the source (see [`Self::preview_roi`]); `roi`/`full` translate the local
    /// rect fraction into absolute source space so the WB eyedropper and mask
    /// tools stay accurate at any zoom/offset.
    pub(crate) fn to_normalized(
        pos: egui::Pos2,
        rect: egui::Rect,
        roi: Option<[u32; 4]>,
        full: (u32, u32),
    ) -> (f32, f32) {
        // Guard the pointer→source division against a zero-width/height rect
        // (e.g. a momentarily empty texture) so we never divide by zero and
        // produce NaN/Infinity into the normalized coordinates.
        let rw = rect.width().max(1e-6);
        let rh = rect.height().max(1e-6);
        let fx = ((pos.x - rect.min.x) / rw).clamp(0.0, 1.0);
        let fy = ((pos.y - rect.min.y) / rh).clamp(0.0, 1.0);
        let roi = roi.unwrap_or([0, 0, full.0, full.1]);
        // L1: the picker contract is normalized `0..=1`; clamp the internal
        // mapping so floating-point rounding can never push a click past the
        // (now loud) `add_red_eye_region` validation.
        let nx = ((roi[0] as f32 + fx * roi[2] as f32) / full.0 as f32).clamp(0.0, 1.0);
        let ny = ((roi[1] as f32 + fy * roi[3] as f32) / full.1 as f32).clamp(0.0, 1.0);
        (nx, ny)
    }
}
