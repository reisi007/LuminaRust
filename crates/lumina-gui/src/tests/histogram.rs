//! histogram plot points, full-frame and draft marks tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// GUI-HISTOGRAM-1: stored 256-bin histograms map onto non-empty plot
/// points inside the plot rect, with the peak reaching the top.
#[test]
fn histogram_plot_points_follow_bins() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    app.render().unwrap();
    let histogram = app
        .preview_histogram
        .clone()
        .expect("render stores the histogram");
    assert_eq!(histogram.bins.len(), 256);
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(256.0, 72.0));
    let points = LuminaApp::histogram_plot_points(&histogram.bins, rect);
    assert_eq!(points.len(), 256, "one point per bin");
    for point in &points {
        assert!(rect.contains(*point), "point {point:?} outside plot rect");
    }
    let top = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    assert!(
        top <= rect.top() + 1.0,
        "peak bin must reach the plot top, got y={top}"
    );

    // An empty histogram still yields baseline points (never empty, never
    // NaN) so the panel cannot collapse.
    let empty = LuminaApp::histogram_plot_points(&[0u64; 256], rect);
    assert_eq!(empty.len(), 256);
    assert!(empty.iter().all(|p| (p.y - rect.bottom()).abs() < 1e-4));
    assert!(empty.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
}

/// GUI-HISTOGRAM-FULL-1 (F-100): the histogram is always computed from the
/// full frame — never from the zoomed viewport/ROI crop. Zoom+pan produce
/// an ROI-cropped display texture, but the stored histogram keeps
/// full-frame dims, full-frame sample count and full-frame bins, and it
/// must not move when only the view changes. The draft flag still
/// describes the render path (REVIEW-GUI-N5).
#[test]
fn histogram_uses_full_frame_despite_zoom_pan() {
    // 64×40 luminance gradient: an ROI crop of the middle covers a
    // different luminance range than the whole frame, so an ROI-fed
    // histogram is measurably distinct from the full-frame one.
    let (w, h) = (64_u32, 40_u32);
    let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        for x in 0..w {
            let r = (x * 255 / (w - 1)) as u8;
            let g = (y * 255 / (h - 1)) as u8;
            let b = ((x + y) * 255 / (w - 1 + h - 1)) as u8;
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    let png = ImageFrame::new(w, h, pixels)
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap();
    let mut app = new_app();
    app.load_bytes(png, "gradient.png").unwrap();
    // Settled Fit render: full-frame preview, full-frame histogram baseline.
    app.render().unwrap();
    assert!(app.preview_roi.is_none());
    let fit_preview = app.preview().expect("preview after load").clone();
    assert_eq!((fit_preview.width, fit_preview.height), (w, h));
    let fit_histogram = app.current_histogram().expect("stored histogram at Fit");
    assert_eq!((fit_histogram.width, fit_histogram.height), (w, h));
    assert_eq!(fit_histogram.bins.iter().sum::<u64>(), u64::from(w * h));

    // Zoom into the frame with a pan offset: the display preview becomes
    // an ROI crop while the histogram must stay full-frame.
    app.preview_zoom = 4.0;
    app.preview_pan = egui::vec2(120.0, 40.0);
    app.render_full([800, 600], None).unwrap();
    let roi = app
        .preview_roi
        .expect("zoomed render must record an ROI crop");
    assert!(
        roi[2] < w && roi[3] < h,
        "ROI must be a true crop, got {roi:?}"
    );
    let zoomed_preview = app.preview().expect("zoomed preview").clone();
    assert_eq!(
        (zoomed_preview.width, zoomed_preview.height),
        (roi[2], roi[3]),
        "the display texture stays ROI-cropped (viewport render)"
    );
    assert!(
        !app.preview_is_draft(),
        "render_full is never a draft even when zoomed"
    );
    let zoomed_histogram = app
        .current_histogram()
        .expect("stored histogram when zoomed");
    // Analysis-input dims == full dims, never the ROI dims.
    assert_eq!((zoomed_histogram.width, zoomed_histogram.height), (w, h));
    assert_eq!(zoomed_histogram.bins.iter().sum::<u64>(), u64::from(w * h));
    assert_eq!(
        app.tone_analysis().expect("tone analysis").sample_count,
        (w * h) as usize,
        "tone sample count must cover the full frame, not the ROI crop"
    );
    // Full-frame invariant: zooming must not move the histogram.
    assert_eq!(
        zoomed_histogram.bins, fit_histogram.bins,
        "zoom/pan must not change the full-frame histogram"
    );
    // …and the stored histogram must NOT describe the ROI crop that is
    // actually displayed (the pre-fix behaviour). Normalized L1 distance
    // (`0` = identical, `2` = disjoint) between the two distributions.
    let (_, roi_histogram) = analyze_tone_with_histogram(&zoomed_preview);
    let sum_full: u64 = zoomed_histogram.bins.iter().sum();
    let sum_roi: u64 = roi_histogram.bins.iter().sum();
    let distance: f64 = zoomed_histogram
        .bins
        .iter()
        .zip(roi_histogram.bins.iter())
        .map(|(&a, &b)| (a as f64 / sum_full as f64 - b as f64 / sum_roi as f64).abs())
        .sum();
    assert!(
        distance > 0.2,
        "stored histogram must differ from the ROI-crop histogram (L1 {distance:.3} <= 0.2)"
    );

    // Draft path: same full-frame guarantee, draft marking intact. The
    // 64×40 draft source is un-downscaled (`downscale` never upscales),
    // so the full draft frame matches the full render pixel-for-pixel
    // under the default recipe with no masks.
    app.render_draft([800, 600], None).unwrap();
    assert!(
        app.preview_is_draft(),
        "render_draft keeps the draft marking (REVIEW-GUI-N5)"
    );
    assert!(
        app.preview_roi.is_some(),
        "the zoomed draft display stays ROI-cropped"
    );
    let draft_histogram = app
        .current_histogram()
        .expect("stored histogram for the zoomed draft");
    assert_eq!(
        (draft_histogram.width, draft_histogram.height),
        (w, h),
        "draft analysis input is the full draft frame, not the ROI"
    );
    assert_eq!(
        draft_histogram.bins, fit_histogram.bins,
        "draft histogram must match the full-frame histogram"
    );
}

#[test]
fn normalized_histogram_l1_unit() {
    // Identical distributions measure 0; disjoint ones 2; degenerate
    // inputs refuse loudly instead of reporting a silent 0.
    assert_eq!(normalized_histogram_l1(&[4, 6], &[4, 6]), Some(0.0));
    assert_eq!(normalized_histogram_l1(&[10, 0], &[0, 10]), Some(2.0));
    assert_eq!(normalized_histogram_l1(&[1, 2], &[1]), None);
    assert_eq!(normalized_histogram_l1(&[0, 0], &[3, 4]), None);
    assert_eq!(normalized_histogram_l1(&[], &[]), None);
}

#[test]
fn g10_original_histogram_uses_decode_and_delta_is_real() {
    // Effect test (no layout assert): with a strong edit rendered, the
    // armed "Show original" switch must surface the unedited decode
    // measurement — byte-equal to a direct `LuminanceHistogram` of the
    // decode — and the delta must carry real analysis values.
    let (png, _) = synthetic_gradient_png();
    let mut app = new_app();
    app.load_bytes(png, "gradient.png").unwrap();
    app.render().unwrap();
    app.set_adjustment("exposure", 2.0);
    app.render().unwrap();
    assert!(!app.show_original_histogram());
    app.toggle_original_histogram();
    assert!(app.show_original_histogram());
    // The switch surfaces the decode, not a re-render of the recipe.
    let direct = LuminanceHistogram::new(app.original.as_ref().expect("decode loaded"));
    let shown = app
        .current_histogram()
        .expect("original histogram while armed");
    assert_eq!(
        shown.bins, direct.bins,
        "armed switch must show the unedited decode"
    );
    assert_eq!(
        app.current_analysis().expect("analysis").mean,
        app.original_analysis().expect("original analysis").mean
    );
    // Real delta: +2 EV brightens the render, so mean drift is positive
    // and the bin distribution moves.
    let (mean_delta, l1) = app.histogram_delta().expect("delta with edit + render");
    assert!(
        mean_delta > 0.0,
        "brighter edit must drift the mean up, got {mean_delta}"
    );
    assert!(
        l1 > 0.0,
        "edited bins must differ from the decode, got L1 {l1}"
    );
    assert_eq!(
        app.recipe().adjustments.get("exposure"),
        Some(&2.0),
        "the compare switch never touches the recipe"
    );
    // Disarming restores the edited-render measurement.
    app.toggle_original_histogram();
    assert!(!app.show_original_histogram());
    assert_eq!(
        app.current_histogram().expect("edited histogram").bins,
        app.preview_histogram
            .as_ref()
            .expect("stored render histogram")
            .bins
    );
}

#[test]
fn g10_display_toggles_leave_recipe_sidecar_and_pixels_untouched() {
    // G-10 Ausbau on the G-16 softproof path: flipping either display
    // switch must leave the recipe, the sidecar bytes and the rendered
    // pixels untouched (display-only assertable, not just documented).
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    let (png, _) = synthetic_gradient_png();
    std::fs::write(&source, &png).unwrap();
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("exposure", 1.0);
    app.commit_pending_slider_save([0, 0]);
    assert!(app.error().is_none());
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let recipe_before = app.recipe().adjustments.clone();
    let sidecar_before = std::fs::read(&sidecar).expect("sidecar committed");
    let pixels_before = app.preview().expect("preview rendered").pixels.clone();
    let generation_before = app.preview_generation();
    app.toggle_softproof_preview();
    app.toggle_original_histogram();
    assert!(app.softproof_preview());
    assert!(app.show_original_histogram());
    assert_eq!(
        app.recipe().adjustments,
        recipe_before,
        "display toggles never touch the recipe"
    );
    assert_eq!(
        std::fs::read(&sidecar).expect("sidecar still there"),
        sidecar_before,
        "display toggles never rewrite the sidecar"
    );
    assert_eq!(
        app.preview().expect("preview kept").pixels,
        pixels_before,
        "display toggles never re-render"
    );
    assert_eq!(
        app.preview_generation(),
        generation_before,
        "display toggles never bump the generation"
    );
    // Even a fresh render with softproof armed is pixel-identical: the
    // badge never leaks into the pipeline.
    app.render().unwrap();
    assert_eq!(
        app.preview().expect("preview re-rendered").pixels,
        pixels_before,
        "softproof must not leak into the render"
    );
    assert_eq!(
        std::fs::read(&sidecar).expect("sidecar still there"),
        sidecar_before,
        "re-render under softproof never rewrites the sidecar"
    );
}

#[test]
fn g10_histogram_section_paints_switches_and_original_badge() {
    // Both G-10 switches are painted in the histogram section (mouse
    // path next to the `S` shortcut); armed, the unedited-decode badge
    // and a real delta line are painted.
    let (png, _) = synthetic_gradient_png();
    let mut app = new_app();
    app.load_bytes(png, "gradient.png").unwrap();
    app.render().unwrap();
    let shapes = headless_shapes(&mut app, |app, ui| app.draw_histogram_section(ui));
    let texts = painted_texts(&shapes);
    assert!(
        texts.iter().any(|t| t == Str::HistogramShowOriginal.t()),
        "Show-original switch must be painted, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == Str::SoftproofToggle.t()),
        "softproof mouse switch must be painted, got {texts:?}"
    );
    app.toggle_original_histogram();
    let shapes = headless_shapes(&mut app, |app, ui| app.draw_histogram(ui));
    let texts = painted_texts(&shapes);
    assert!(
        texts.iter().any(|t| t == Str::HistogramOriginalBadge.t()),
        "original badge must be painted while armed, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.starts_with("Δ vs edited")),
        "real delta line must be painted while armed, got {texts:?}"
    );
}

/// GUI-PREVIEW-NOISE-1: at Fit the main preview must show the full frame
/// (ROI `None`, no draft) and its pixel histogram must match the
/// navigator/filmstrip thumbnail histogram — the exact user finding (gray
/// noise in the main preview while the thumbnail is correct).
#[test]
fn fit_preview_histogram_matches_thumbnail() {
    let (png, original) = synthetic_gradient_png();
    let mut app = new_app();
    app.load_bytes(png, "gradient.png").unwrap();
    app.render().unwrap();
    // Fit state: full-frame render, no ROI crop, no draft.
    assert_eq!(app.zoom_mode, ZoomMode::Fit);
    assert!(
        app.preview_roi.is_none(),
        "Fit must render the full frame (ROI None), got {:?}",
        app.preview_roi
    );
    assert!(
        !app.preview_is_draft,
        "a settled Fit render is never a draft"
    );
    let preview = app.preview().expect("preview after load").clone();
    assert_eq!((preview.width, preview.height), (64, 40));
    // Thumbnail pipeline (mirrors `decode_thumbnail_frame` sans disk
    // cache): downscale + default-recipe render.
    let (small, w, h) = crate::filmstrip::downscale_rgba(
        &original.pixels,
        original.width,
        original.height,
        crate::filmstrip::THUMBNAIL_MAX_DIM,
    );
    let small_frame = ImageFrame::new(w, h, small).unwrap();
    let thumb_ctx = RenderContext {
        recipe: &EditRecipe::default(),
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    let thumb = render_frame(&small_frame, &thumb_ctx).unwrap().frame;
    let (_, preview_hist) = analyze_tone_with_histogram(&preview);
    let (_, thumb_hist) = analyze_tone_with_histogram(&thumb);
    // The app's own stored histogram must describe the preview.
    let stored = app.current_histogram().expect("stored preview histogram");
    assert_eq!(
        stored.bins, preview_hist.bins,
        "stored histogram must describe the displayed preview"
    );
    // Same content at different scales: distributions must be close.
    let distance = histogram_l1(&preview_hist.bins, &thumb_hist.bins);
    assert!(
        distance < 0.35,
        "Fit preview histogram must match the thumbnail (L1 {distance:.3} >= 0.35)"
    );
    // Noise-flat guard: content spread over many bins, no dominant spike
    // (gray noise / a flat field would concentrate into few bins).
    let total: u64 = preview_hist.bins.iter().sum();
    let populated = preview_hist
        .bins
        .iter()
        .filter(|&&c| c as f64 > total as f64 * 0.001)
        .count();
    let peak = *preview_hist.bins.iter().max().unwrap() as f64 / total as f64;
    assert!(
        populated >= 16,
        "preview histogram must spread over >= 16 bins, got {populated}"
    );
    assert!(
        peak < 0.5,
        "no single bin may dominate the preview histogram (peak {peak:.3})"
    );
}

/// REVIEW-GUI-N5: a draft analysis render is flagged as draft and the
/// histogram panel says so instead of posing as the final render state.
#[test]
fn draft_analysis_render_marks_histogram_draft() {
    let (png, _) = synthetic_gradient_png();
    let mut app = new_app();
    app.load_bytes(png, "gradient.png").unwrap();
    app.render().unwrap();
    assert!(
        !app.preview_is_draft(),
        "a settled full render is never a draft"
    );
    app.render_draft([64, 40], None).unwrap();
    assert!(
        app.preview_is_draft(),
        "a drag render must flag the preview as draft"
    );
    assert!(
        app.current_histogram().is_some(),
        "the draft keeps its full-frame analysis"
    );
    let shapes = headless_shapes(&mut app, |app, ui| app.draw_histogram(ui));
    let texts = painted_texts(&shapes);
    assert!(
        texts.iter().any(|t| t == Str::HistogramDraft.t()),
        "the draft histogram badge must be painted, got {texts:?}"
    );
}

/// GUI-PREVIEW-NOISE-1 (portrait): the same full-frame guarantee as the
/// landscape case — at Fit a portrait preview shows the whole frame and
/// its histogram matches the thumbnail histogram.
#[test]
fn fit_preview_portrait_histogram_matches_thumbnail() {
    let (w, h) = (40u32, 64u32);
    let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        for x in 0..w {
            let r = (x * 255 / (w - 1)) as u8;
            let g = (y * 255 / (h - 1)) as u8;
            let b = ((x + y) * 255 / (w - 1 + h - 1)) as u8;
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    let original = ImageFrame::new(w, h, pixels).unwrap();
    let png = original.encode(ImageFileFormat::Png).unwrap();
    let mut app = new_app();
    app.load_bytes(png, "portrait.png").unwrap();
    app.render().unwrap();
    assert_eq!(app.zoom_mode, ZoomMode::Fit);
    assert!(
        app.preview_roi.is_none(),
        "Fit must render the full frame (ROI None), got {:?}",
        app.preview_roi
    );
    assert!(
        !app.preview_is_draft,
        "a settled Fit render is never a draft"
    );
    let preview = app.preview().expect("preview after load").clone();
    assert_eq!((preview.width, preview.height), (40, 64));
    let (small, sw, sh) = crate::filmstrip::downscale_rgba(
        &original.pixels,
        original.width,
        original.height,
        crate::filmstrip::THUMBNAIL_MAX_DIM,
    );
    let small_frame = ImageFrame::new(sw, sh, small).unwrap();
    let thumb_ctx = RenderContext {
        recipe: &EditRecipe::default(),
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    let thumb = render_frame(&small_frame, &thumb_ctx).unwrap().frame;
    let (_, preview_hist) = analyze_tone_with_histogram(&preview);
    let (_, thumb_hist) = analyze_tone_with_histogram(&thumb);
    let stored = app.current_histogram().expect("stored preview histogram");
    assert_eq!(
        stored.bins, preview_hist.bins,
        "stored histogram must describe the displayed preview"
    );
    let distance = histogram_l1(&preview_hist.bins, &thumb_hist.bins);
    assert!(
        distance < 0.35,
        "portrait Fit preview histogram must match the thumbnail (L1 {distance:.3} >= 0.35)"
    );
    let total: u64 = preview_hist.bins.iter().sum();
    let populated = preview_hist
        .bins
        .iter()
        .filter(|&&c| c as f64 > total as f64 * 0.001)
        .count();
    let peak = *preview_hist.bins.iter().max().unwrap() as f64 / total as f64;
    assert!(
        populated >= 16,
        "preview histogram must spread over >= 16 bins, got {populated}"
    );
    assert!(
        peak < 0.5,
        "no single bin may dominate the preview histogram (peak {peak:.3})"
    );
}
