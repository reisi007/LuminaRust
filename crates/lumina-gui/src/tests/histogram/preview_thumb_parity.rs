//! The preview↔thumbnail parity: the full preview's histogram must equal the
//! navigator/filmstrip thumbnail's histogram (GUI-PREVIEW-NOISE-1).
//!
//! Split out of `tests/histogram.rs` (module `tests::histogram`) to buy
//! headroom under the 500-line ratchet. The boundary is the *parity contract*
//! — "the full frame the user sees and the thumbnail the filmstrip shows
//! describe the same picture" — with the fixture that makes it failable
//! (PREVIEW-THUMB-PARITY-VACUOUS-76): a source whose long edge is above the
//! thumbnail cap, so the thumbnail is really resampled. The other seven test
//! bodies of `tests::histogram.rs` moved verbatim; the two parity tests were
//! rewritten (new fixture, non-vacuity guards, new bounds). The shared
//! fixtures/helpers are pulled in via `use super::*`.

use super::*;

/// PREVIEW-THUMB-PARITY-VACUOUS-76: synthetic **step-1 ramp** frame whose long
/// edge is **above** the thumbnail cap.
///
/// The shared 64×40 gradient (`synthetic_gradient_png`) has a long edge of
/// 64 px — below `THUMBNAIL_MAX_DIM` (200). `downscale(..., LongEdge(200))`
/// therefore took the early-return clone (`crates/lumina-core/src/lib.rs:238`),
/// the "thumbnail" was the *unchanged original* and `histogram_l1(preview,
/// thumb)` was exactly 0: the parity assertion was vacuous (measured: the
/// kernel mutation `.round()` → `.floor()` left both tests green).
///
/// This fixture is 256 px on its long edge, so the thumbnail is really
/// resampled. Red is the horizontal step-1 ramp `x`, green the vertical
/// step-1 ramp `y` — both exactly predictable, which lets the resample path be
/// pinned byte-exactly (`assert_ramp_thumbnail_is_bilinear`) instead of only
/// statistically. Blue adds a diagonal component, so the luminance is not a
/// pure one-dimensional ramp and the histogram guards stay meaningful.
fn synthetic_ramp_frame(w: u32, h: u32) -> ImageFrame {
    assert!(
        w <= 256 && h <= 256,
        "the step-1 ramps must stay inside u8 (got {w}x{h})"
    );
    let mut pixels = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        for x in 0..w {
            let r = x as u8;
            let g = y as u8;
            let b = ((x + y) / 2) as u8;
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    ImageFrame::new(w, h, pixels).unwrap()
}

/// PREVIEW-THUMB-PARITY-VACUOUS-76: the non-vacuum conditions of the
/// preview↔thumbnail parity, asserted **explicitly**.
///
/// The parity statement ("the full preview's histogram equals the thumbnail's")
/// only carries information when the thumbnail is a real **resample** of the
/// source. Below the cap (`THUMBNAIL_MAX_DIM`) `downscale` returns the
/// unchanged original and the comparison degenerates to "the preview equals
/// itself" — the measured distance was exactly 0. These assertions make that
/// state unreachable: a later fixture change that shrinks the source below the
/// cap fails here instead of silently emptying the parity statement.
fn assert_thumbnail_is_really_resampled(original: &ImageFrame, thumb: &ImageFrame) {
    let cap = crate::filmstrip::THUMBNAIL_MAX_DIM;
    assert!(
        original.width().max(original.height()) > cap,
        "the source long edge must exceed THUMBNAIL_MAX_DIM ({cap}) — got {}x{}; \
         below the cap the thumbnail is the unchanged original and the parity \
         assertion is vacuous",
        original.width(),
        original.height()
    );
    assert!(
        thumb.width() < original.width() && thumb.height() < original.height(),
        "the thumbnail ({}x{}) must be strictly smaller than the source ({}x{}) — \
         an unchanged copy would make the parity assertion vacuous",
        thumb.width(),
        thumb.height(),
        original.width(),
        original.height()
    );
    assert_eq!(
        thumb.width().max(thumb.height()),
        cap,
        "the thumbnail's long edge must be the cap ({cap})"
    );
}

/// PREVIEW-THUMB-PARITY-VACUOUS-76: pin the thumbnail resample **byte-exactly**.
///
/// The histogram parity is a *distribution* statement — it tolerates a kernel
/// that shifts every pixel by a level. This helper is the sharp counterpart:
/// for the step-1 ramp fixture the resample is analytically predictable. The
/// thumbnail pixel in column `j` (row `i`) must be the source ramp evaluated
/// at the half-pixel-centre source position
/// `sx = (j + 0.5) * width / new_width - 0.5`, rounded half-up; the green
/// channel does the same along the rows.
///
/// **What this pin covers — and what it does not:** the **rounding** of the
/// kernel (`.round()` → `.floor()` moves these bytes) and the **sampling
/// geometry** of the red/green ramp (a shifted half-pixel centre moves them
/// too). It does **not** pin the *kind* of interpolation: on a linear ramp
/// bilinear and nearest neighbour are byte-identical (same half-pixel-centre
/// geometry, same rounding — measured: 0 of 25 000 pixels differ in red and
/// in green), so a switch between the two would stay green here despite the
/// helper's name. The blue channel is not pinned either. The parity test
/// therefore asserts the **mean-luminance gap** as well — but that assertion
/// covers a *systematic level shift* of the thumbnail (the `.floor()` mutation,
/// a global brightness offset: mean gap 0.018412 levels clean vs. 0.499971
/// under the mutation), **not** the kind of interpolation: there the gap is
/// 0.020029 levels, so a bilinear→nearest switch would stay green here too.
///
/// The frame handed in is the *rendered* thumbnail; the default recipe is
/// pixel-preserving, so the pinned bytes are the resample's own (a rendering
/// that altered them would fail here as well).
fn assert_ramp_thumbnail_is_bilinear(thumb: &ImageFrame, source_w: u32, source_h: u32) {
    let (tw, th) = (thumb.width(), thumb.height());
    let pixels = thumb.pixels();
    for j in 0..tw {
        let expected = ((j as f64 + 0.5) * source_w as f64 / tw as f64 - 0.5)
            .round()
            .clamp(0.0, 255.0) as u8;
        for i in 0..th {
            let red = pixels[((i * tw + j) * 4) as usize];
            assert_eq!(
                red, expected,
                "thumbnail red at ({i},{j}) must be the bilinear resample of the ramp \
                 (expected {expected}, got {red})"
            );
        }
    }
    for i in 0..th {
        let expected = ((i as f64 + 0.5) * source_h as f64 / th as f64 - 0.5)
            .round()
            .clamp(0.0, 255.0) as u8;
        for j in 0..tw {
            let green = pixels[((i * tw + j) * 4 + 1) as usize];
            assert_eq!(
                green, expected,
                "thumbnail green at ({i},{j}) must be the bilinear resample of the ramp \
                 (expected {expected}, got {green})"
            );
        }
    }
}

/// GUI-PREVIEW-NOISE-1: at Fit the main preview must show the full frame
/// (ROI `None`, no draft) and its pixel histogram must match the
/// navigator/filmstrip thumbnail histogram — the exact user finding (gray
/// noise in the main preview while the thumbnail is correct).
///
/// PREVIEW-THUMB-PARITY-VACUOUS-76: the source is 256×160, its long edge lies
/// **above** the thumbnail cap (200) — the thumbnail is really resampled, so
/// the parity statement is failable (before: the 64×40 source was below the
/// cap, the "thumbnail" was the unchanged original and the L1 distance was
/// exactly 0 — the kernel mutation `.round()` → `.floor()` stayed green).
///
/// The parity is pinned by three assertions, each with a measured bound:
/// the non-vacuum conditions (`assert_thumbnail_is_really_resampled`: the
/// source's long edge is above the cap and the thumbnail is strictly
/// smaller), the distribution distance (L1 0.0101 measured, bound 0.05) and
/// the mean of the same histogram pass (0.018 levels measured, bound 0.15 —
/// the kernel mutation moves it to 0.500 levels). On top of that the
/// resampled thumbnail is pinned byte-exactly
/// (`assert_ramp_thumbnail_is_bilinear`), where the kernel mutation fails
/// immediately.
#[test]
fn fit_preview_histogram_matches_thumbnail() {
    let original = synthetic_ramp_frame(256, 160);
    let png = original.encode(ImageFileFormat::Png).unwrap();
    let mut app = new_app();
    app.load_bytes(png, "ramp.png").unwrap();
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
    // The Fit preview is the **full source**: the app's default 800×600 pane
    // yields a 1040×780 preview cap (`PREVIEW_ROI_MARGIN` 1.3), far above the
    // source — so no preview downscale happens (R3-RENDER-SIZE-1).
    assert_eq!((preview.width(), preview.height()), (256, 160));
    // Thumbnail pipeline (mirrors `decode_thumbnail_frame` sans disk
    // cache): canonical-path downscale + default-recipe render.
    let small_frame = downscale(
        &original,
        DownscaleTarget::LongEdge(crate::filmstrip::THUMBNAIL_MAX_DIM),
    )
    .unwrap();
    assert_thumbnail_is_really_resampled(&original, &small_frame);
    let thumb_ctx = RenderContext {
        recipe: &EditRecipe::default(),
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    let thumb = render_frame(&small_frame, &thumb_ctx).unwrap().frame;
    // The resampled thumbnail is pinned byte-exactly — a changed kernel
    // (rounding, sampling geometry) fails here instead of hiding behind the
    // tolerances below.
    assert_ramp_thumbnail_is_bilinear(&thumb, original.width(), original.height());
    let (preview_analysis, preview_hist) = analyze_tone_with_histogram(&preview);
    let (thumb_analysis, thumb_hist) = analyze_tone_with_histogram(&thumb);
    // The app's own stored histogram must describe the preview.
    let stored = app.current_histogram().expect("stored preview histogram");
    assert_eq!(
        stored.bins, preview_hist.bins,
        "stored histogram must describe the displayed preview"
    );
    // Same content at different scales: the distributions must be close.
    // Measured on this fixture (source 256x160 -> thumbnail 200x125): L1 =
    // 0.0101. The old bound 0.35 was 35x the measured value — it would have
    // passed almost any broken distribution. 0.05 keeps a 5x margin over the
    // measurement and is 7x tighter than the old bound.
    let distance = histogram_l1(&preview_hist.bins, &thumb_hist.bins);
    assert!(
        distance < 0.05,
        "Fit preview histogram must match the thumbnail (L1 {distance:.3} >= 0.05)"
    );
    // The mean of the *same* histogram pass is the sharp parity statement: a
    // resample preserves the mean, so both renderings must agree within 0.15
    // of a luminance level. Measured: 0.018 levels; the kernel mutation
    // `.round()` -> `.floor()` moves the gap to 0.500 levels. The L1 above is
    // a smooth statistic (0.010 -> 0.016 under the same mutation), so this
    // mean gap is what makes the parity assertion failable.
    let mean_gap = (preview_analysis.mean - thumb_analysis.mean).abs() * 255.0;
    assert!(
        mean_gap < 0.15,
        "Fit preview and thumbnail must agree in mean luminance \
         (gap {mean_gap:.3} levels >= 0.15)"
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

/// GUI-PREVIEW-NOISE-1 (portrait): the same full-frame guarantee as the
/// landscape case — at Fit a portrait preview shows the whole frame and
/// its histogram matches the thumbnail histogram.
///
/// PREVIEW-THUMB-PARITY-VACUOUS-76: the portrait source is 160×256, its long
/// edge lies **above** the thumbnail cap (200) — the thumbnail is really
/// resampled, so the parity statement is failable (before: the 40×64 source
/// was below the cap, the "thumbnail" was the unchanged original and the L1
/// distance was exactly 0 — the kernel mutation `.round()` → `.floor()` stayed
/// green). The parity is pinned exactly as in the landscape case: the
/// non-vacuum conditions (`assert_thumbnail_is_really_resampled`), the
/// distribution distance (L1 0.0111 measured, bound 0.05), the mean of the
/// same histogram pass (0.018 levels measured, bound 0.15 — the kernel
/// mutation moves it to 0.500 levels) and the byte-exact thumbnail
/// (`assert_ramp_thumbnail_is_bilinear`).
#[test]
fn fit_preview_portrait_histogram_matches_thumbnail() {
    let original = synthetic_ramp_frame(160, 256);
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
    // The Fit preview is the full source (the 1040×780 preview cap is far
    // above it), portrait geometry included.
    assert_eq!((preview.width(), preview.height()), (160, 256));
    let small_frame = downscale(
        &original,
        DownscaleTarget::LongEdge(crate::filmstrip::THUMBNAIL_MAX_DIM),
    )
    .unwrap();
    assert_thumbnail_is_really_resampled(&original, &small_frame);
    let thumb_ctx = RenderContext {
        recipe: &EditRecipe::default(),
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    let thumb = render_frame(&small_frame, &thumb_ctx).unwrap().frame;
    // The resampled thumbnail is pinned byte-exactly — a changed kernel
    // (rounding, sampling geometry) fails here instead of hiding behind the
    // tolerances below.
    assert_ramp_thumbnail_is_bilinear(&thumb, original.width(), original.height());
    let (preview_analysis, preview_hist) = analyze_tone_with_histogram(&preview);
    let (thumb_analysis, thumb_hist) = analyze_tone_with_histogram(&thumb);
    let stored = app.current_histogram().expect("stored preview histogram");
    assert_eq!(
        stored.bins, preview_hist.bins,
        "stored histogram must describe the displayed preview"
    );
    // Measured on this fixture (source 160x256 -> thumbnail 125x200): L1 =
    // 0.0111; the old bound 0.35 was 31x the measured value. 0.05 keeps a
    // 4.5x margin over the measurement (see the landscape test for the
    // reasoning).
    let distance = histogram_l1(&preview_hist.bins, &thumb_hist.bins);
    assert!(
        distance < 0.05,
        "portrait Fit preview histogram must match the thumbnail (L1 {distance:.3} >= 0.05)"
    );
    // The mean of the same histogram pass is the sharp parity statement:
    // measured 0.018 levels, 0.500 levels under the kernel mutation
    // `.round()` -> `.floor()`.
    let mean_gap = (preview_analysis.mean - thumb_analysis.mean).abs() * 255.0;
    assert!(
        mean_gap < 0.15,
        "portrait Fit preview and thumbnail must agree in mean luminance \
         (gap {mean_gap:.3} levels >= 0.15)"
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
