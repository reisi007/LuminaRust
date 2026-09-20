//! R3-RENDER-SIZE-1: the preview viewport cap.
//!
//! SOLL (User-Entscheid 2026-09-20, `feature/platform/cli-gui-wasm.md`):
//! preview renders (draft **and** full) never exceed the viewport resolution
//! times the device pixel ratio. Full source resolution stays reserved for
//! export and the 1:1 loupe.
//!
//! The cap is expressed as a maximum output rectangle in **device pixels**:
//!
//! ```text
//! cap = ceil(pane_points · device_pixel_ratio · PREVIEW_CAP_MARGIN)
//! ```
//!
//! The margin is the same [`crate::PREVIEW_ROI_MARGIN`] the zoom ROI uses, so
//! the *visible* pane maps to full device resolution while the extra border
//! stays as panning headroom. A render window (the zoom ROI, or the whole
//! frame at Fit) that already fits the cap is left untouched — the cap only
//! ever downscales, never upscales.
//!
//! Missing viewport information is **loud**: [`preview_size_cap`] returns
//! `None`, the caller keeps the (correct) full-resolution render and logs a
//! `warn!` once — never a silent uncapped/empty render.
//!
//! Masks need no special casing: the core resamples every mask plane to the
//! rendered frame dimensions (`resample_plane_bilinear`), so capping only
//! changes the preview resolution, never the mask geometry.

/// Minimum long edge (device px) of a preview cap. A pane that has not been
/// measured yet (or a degenerate ratio) must not produce a thumbnail-sized
/// render; the floor keeps the preview usable while still far below a 24 MP
/// source.
pub(crate) const PREVIEW_MIN_CAP_EDGE: u32 = 256;

/// Documented default long edge (px) for the cached draft source before a
/// viewport cap is known (the pre-R3-RENDER-SIZE-1 fixed draft edge).
pub(crate) const DEFAULT_DRAFT_MAX_DIM: u32 = 1280;

/// The preview cap rectangle in device pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PreviewSizeCap {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl PreviewSizeCap {
    /// Long edge of the cap (the draft source and neighbor previews are sized
    /// by their long edge).
    #[cfg(test)]
    pub(crate) fn long_edge(self) -> u32 {
        self.width.max(self.height)
    }

    /// Downscale factor for a `window_w × window_h` render window, or `None`
    /// when the window already fits the cap (no downscale needed).
    ///
    /// Only shrinks (`<= 1.0`); a zero/one-dimension window is left untouched.
    pub(crate) fn scale_for(self, window_w: u32, window_h: u32) -> Option<f64> {
        if window_w == 0 || window_h == 0 {
            return None;
        }
        let sx = self.width as f64 / window_w as f64;
        let sy = self.height as f64 / window_h as f64;
        let scale = sx.min(sy);
        if scale >= 1.0 {
            None
        } else {
            Some(scale)
        }
    }
}

/// Compute the preview cap for a pane given in points and a device pixel ratio.
/// `None` (with a loud caller log) when the viewport information is missing or
/// degenerate.
pub(crate) fn preview_size_cap(pane_w: f32, pane_h: f32, dpr: f32) -> Option<PreviewSizeCap> {
    if !pane_w.is_finite() || !pane_h.is_finite() || !dpr.is_finite() {
        return None;
    }
    if pane_w <= 0.0 || pane_h <= 0.0 || dpr <= 0.0 {
        return None;
    }
    let margin = crate::PREVIEW_ROI_MARGIN;
    let width = (pane_w as f64 * dpr as f64 * margin).ceil();
    let height = (pane_h as f64 * dpr as f64 * margin).ceil();
    if width < 1.0 || height < 1.0 {
        return None;
    }
    let clamp =
        |value: f64| -> u32 { value.max(PREVIEW_MIN_CAP_EDGE as f64).min(u32::MAX as f64) as u32 };
    Some(PreviewSizeCap {
        width: clamp(width),
        height: clamp(height),
    })
}

/// Target long edge for a source downscaled so a `window_w × window_h` render
/// window fits `cap`, or `None` when the window already fits (no downscale).
///
/// `source_*` and `window_*` are full-source pixels (the window is the zoom
/// ROI or the whole frame). The window is clamped to the source first: a
/// preview never renders more pixels than the source has, so an oversized ROI
/// request (clamped later by `crop_region`) must not drive the cap and shrink a
/// small source below its own resolution.
pub(crate) fn capped_source_max_dim(
    source_w: u32,
    source_h: u32,
    window_w: u32,
    window_h: u32,
    cap: PreviewSizeCap,
) -> Option<u32> {
    let window_w = window_w.min(source_w);
    let window_h = window_h.min(source_h);
    let scale = cap.scale_for(window_w, window_h)?;
    let long = source_w.max(source_h) as f64;
    if long < 1.0 {
        return None;
    }
    let max_dim = (long * scale).round().max(1.0);
    let max_dim = max_dim.min(u32::MAX as f64) as u32;
    if max_dim == 0 || max_dim >= source_w.max(source_h) {
        None
    } else {
        Some(max_dim)
    }
}

/// Map a render-window ROI from full-source pixels into the downscaled source
/// space actually rendered, clamping to the downscaled frame.
pub(crate) fn scale_roi_to_source(
    roi: [u32; 4],
    source: (u32, u32),
    capped: (u32, u32),
) -> [u32; 4] {
    if source.0 == 0 || source.1 == 0 || capped.0 == 0 || capped.1 == 0 {
        return roi;
    }
    let sx = capped.0 as f64 / source.0 as f64;
    let sy = capped.1 as f64 / source.1 as f64;
    let x = ((roi[0] as f64 * sx).round() as u32).min(capped.0 - 1);
    let y = ((roi[1] as f64 * sy).round() as u32).min(capped.1 - 1);
    let w = (((roi[2] as f64 * sx).round() as u32).max(1)).min(capped.0 - x);
    let h = (((roi[3] as f64 * sy).round() as u32).max(1)).min(capped.1 - y);
    [x, y, w, h]
}

/// The preview viewport cap of the last painted frame, or `None` when the pane
/// has not been measured yet / is degenerate. Loud at the call site, never a
/// silent uncapped render.
impl crate::LuminaApp {
    pub(crate) fn preview_cap(&self) -> Option<PreviewSizeCap> {
        preview_size_cap(
            self.preview_pane_w,
            self.preview_pane_h,
            self.preview_cap_state.dpr,
        )
    }

    /// R3-RENDER-SIZE-1: the capped downscaled source for a full preview.
    ///
    /// `max_dim` is the long edge computed by [`capped_source_max_dim`].
    /// Reuses the cached downscale when it was built for the same edge;
    /// otherwise rebuilds it from the original. Callers must have checked that
    /// `max_dim` is smaller than the original long edge (hence a real cap).
    pub(crate) fn capped_preview_source(
        &mut self,
        original: &crate::ImageFrame,
        max_dim: u32,
    ) -> crate::ImageFrame {
        if let Some((built_for, frame)) = self.preview_cap_state.capped_src.as_ref() {
            if *built_for == max_dim
                && frame.width == original.width
                && frame.height == original.height
            {
                return frame.clone();
            }
        }
        let frame = original.downscale(max_dim);
        self.preview_cap_state.capped_src = Some((max_dim, frame.clone()));
        frame
    }

    /// R3-RENDER-SIZE-1: record the frame's device pixel ratio and rebuild the
    /// cached draft source when the viewport cap changed, keeping
    /// `draft_max_dim`/`draft_built_for_dim` in sync. Called once per frame
    /// before the render scheduler. When the viewport is unknown the previous
    /// cap (and previous draft) are kept; the loud "missing viewport" warning is
    /// emitted by [`Self::render_full`].
    pub(crate) fn refresh_preview_cap(&mut self, dpr: f32) {
        self.preview_cap_state.dpr = dpr;
        let Some(cap) = self.preview_cap() else {
            return;
        };
        let Some(original) = self.original.as_ref() else {
            return;
        };
        let window = (original.width, original.height);
        let Some(max_dim) =
            capped_source_max_dim(original.width, original.height, window.0, window.1, cap)
        else {
            return; // source already fits the cap: nothing to rebuild
        };
        self.preview_cap_state.draft_max_dim = max_dim;
        if self.preview_cap_state.draft_built_for_dim == max_dim {
            return;
        }
        let draft = original.downscale(max_dim);
        self.preview_cap_state.draft_built_for_dim = max_dim;
        self.draft_original = Some(draft);
    }
}

/// R3-RENDER-SIZE-1: per-session preview-cap state.
///
/// Grouped so the (ratcheted) crate root gains one field, not five; the
/// doc comments for each member live here.
#[derive(Debug)]
pub(crate) struct PreviewCapState {
    /// Long-edge cap (px) for the cached draft source (the viewport cap when a
    /// large source is loaded, else the documented 1280-px default).
    pub(crate) draft_max_dim: u32,
    /// Requested long edge the cached `draft_original` was built for, so
    /// [`crate::LuminaApp::refresh_preview_cap`] rebuilds exactly when the
    /// viewport cap changes (and never thrashes on a source smaller than the
    /// cap, where `downscale` legitimately returns the un-scaled source).
    pub(crate) draft_built_for_dim: u32,
    /// Device pixel ratio of the last painted frame (`ctx.pixels_per_point()`),
    /// so the cap is computed in device pixels; 1.0 for headless/direct renders.
    pub(crate) dpr: f32,
    /// Cached full-source downscale for a capped full preview, keyed by the
    /// long edge it was built for. A debounced settle at a stable zoom reuses
    /// it instead of re-downscaling the (up to 24 MP) original.
    pub(crate) capped_src: Option<(u32, crate::ImageFrame)>,
    /// Dedup for the loud "viewport info missing" warning — once per source, so
    /// a degenerate pane cannot spam, but is never silently uncapped.
    pub(crate) warned: bool,
}

impl Default for PreviewCapState {
    fn default() -> Self {
        Self {
            draft_max_dim: DEFAULT_DRAFT_MAX_DIM,
            draft_built_for_dim: DEFAULT_DRAFT_MAX_DIM,
            dpr: 1.0,
            capped_src: None,
            warned: false,
        }
    }
}

impl PreviewCapState {
    /// Fresh state for a newly opened source: the draft was built for the
    /// default edge, the capped-preview cache is empty and the loud
    /// missing-viewport warning is re-armed.
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_2x1() -> crate::ImageFrame {
        crate::ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255]).unwrap()
    }

    #[test]
    fn cap_scales_pane_and_ratio_with_margin_and_floor() {
        let cap = preview_size_cap(800.0, 600.0, 1.0).expect("valid pane yields a cap");
        // 800·1.0·1.3 = 1040, 600·1.0·1.3 = 780.
        assert_eq!(cap.width, 1040);
        assert_eq!(cap.height, 780);
        assert_eq!(cap.long_edge(), 1040);
        // Retina doubles the device-pixel budget.
        let retina = preview_size_cap(800.0, 600.0, 2.0).unwrap();
        assert_eq!(retina.width, 2080);
        assert_eq!(retina.height, 1560);
        // A tiny/unmeasured pane is floored, never a thumbnail.
        let tiny = preview_size_cap(10.0, 10.0, 1.0).unwrap();
        assert_eq!(tiny.width, PREVIEW_MIN_CAP_EDGE);
        assert_eq!(tiny.height, PREVIEW_MIN_CAP_EDGE);
    }

    #[test]
    fn missing_or_degenerate_viewport_is_none_not_a_silent_cap() {
        for (w, h, dpr) in [
            (0.0, 600.0, 1.0),
            (800.0, 0.0, 1.0),
            (800.0, 600.0, 0.0),
            (800.0, 600.0, -2.0),
            (f32::NAN, 600.0, 1.0),
            (800.0, f32::INFINITY, 1.0),
            (f32::NAN, 600.0, f32::NAN),
        ] {
            assert_eq!(
                preview_size_cap(w, h, dpr),
                None,
                "pane {w}x{h} @ {dpr} must yield no cap (loud fallback, not silence)"
            );
        }
    }

    #[test]
    fn scale_is_none_when_the_window_already_fits() {
        let cap = PreviewSizeCap {
            width: 1600,
            height: 1200,
        };
        assert_eq!(cap.scale_for(800, 600), None);
        assert_eq!(cap.scale_for(1600, 1200), None);
        assert_eq!(cap.scale_for(0, 600), None);
        let scale = cap.scale_for(3200, 2400).unwrap();
        assert!((scale - 0.5).abs() < 1e-9);
        // The smaller axis decides (aspect preserved).
        let scale = cap.scale_for(3200, 600).unwrap();
        assert!((scale - 0.5).abs() < 1e-9);
    }

    #[test]
    fn capped_source_max_dim_only_downscales() {
        let cap = PreviewSizeCap {
            width: 1600,
            height: 1200,
        };
        // A 6032×4024 source rendered as the whole frame: the width-based scale
        // (1600/6032 = 0.265) is tighter than the height-based (1200/4024 =
        // 0.298), so the long edge → 6032 · 1600/6032 = 1600.
        let max_dim = capped_source_max_dim(6032, 4024, 6032, 4024, cap).unwrap();
        assert_eq!(max_dim, 1600);
        // An ROI that already fits the cap is not downscaled.
        assert_eq!(capped_source_max_dim(6032, 4024, 900, 700, cap), None);
        // Degenerate window: untouched.
        assert_eq!(capped_source_max_dim(6032, 4024, 0, 700, cap), None);
        // An oversized ROI request is clamped to the source *before* the cap, so
        // it can never shrink a small source below its own resolution
        // (REVIEW-GUI-N6 oversized-clamp regression).
        assert_eq!(capped_source_max_dim(2, 1, 9999, 9999, cap), None);
    }

    #[test]
    fn scale_roi_maps_and_clamps_into_the_downscaled_frame() {
        let roi = [100, 200, 400, 300];
        let scaled = scale_roi_to_source(roi, (2000, 1500), (1000, 750));
        assert_eq!(scaled, [50, 100, 200, 150]);
        // A right/bottom edge ROI never overflows the downscaled frame.
        let scaled = scale_roi_to_source([1900, 1400, 100, 100], (2000, 1500), (1000, 750));
        assert!(scaled[0] + scaled[2] <= 1000, "width clamped: {scaled:?}");
        assert!(scaled[1] + scaled[3] <= 750, "height clamped: {scaled:?}");
        // Degenerate source passes the ROI through unchanged.
        assert_eq!(
            scale_roi_to_source(roi, (0, 0), (10, 10)),
            roi,
            "unknown source dims must not silently resize the ROI"
        );
    }

    /// The cap is a pure size rule: a small source (smaller than the cap) is
    /// never touched, so existing small-fixture renders stay byte-identical.
    #[test]
    fn small_sources_are_never_capped() {
        let frame = png_2x1();
        let cap = preview_size_cap(800.0, 600.0, 1.0).unwrap();
        assert_eq!(
            capped_source_max_dim(frame.width, frame.height, frame.width, frame.height, cap),
            None
        );
    }
}
