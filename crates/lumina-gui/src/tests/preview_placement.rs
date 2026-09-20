//! draft/full preview placement and on-screen geometry tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// GUI-DRAFT-JUMP-1: a draft texture (downscaled render source) scales
/// back into full-source geometry, so draft and full share placement.
#[test]
fn preview_draw_dims_upscales_draft_to_full_placement() {
    let full = (2000.0_f32, 1500.0_f32);
    // Pass-through without source identity (legacy) and for full renders.
    assert_eq!(
        LuminaApp::preview_draw_dims(640.0, 480.0, full.0, full.1, None),
        (640.0, 480.0)
    );
    assert_eq!(
        LuminaApp::preview_draw_dims(640.0, 480.0, full.0, full.1, Some((2000, 1500))),
        (640.0, 480.0)
    );
    // Draft texture upscales by full/render_src per axis.
    let (w, h) = LuminaApp::preview_draw_dims(832.0, 624.0, full.0, full.1, Some((1280, 960)));
    assert!(
        (w - 1300.0).abs() < 1e-3,
        "draft width must upscale, got {w}"
    );
    assert!(
        (h - 975.0).abs() < 1e-3,
        "draft height must upscale, got {h}"
    );
    // Degenerate source never divides by zero.
    assert_eq!(
        LuminaApp::preview_draw_dims(10.0, 10.0, full.0, full.1, Some((0, 0))),
        (10.0, 10.0)
    );
}

/// GUI-DRAFT-JUMP-1: a draft-space ROI converts into (near-)identical
/// full-space geometry, so pointer mapping and overlay agree on both paths.
#[test]
fn roi_in_full_pixels_aligns_draft_and_full_crops() {
    // Same view (zoom 2, centred) computed in both pixel spaces.
    let full_roi = LuminaApp::roi_from_zoom(2000, 1500, 2.0, egui::Vec2::ZERO, 800.0, 600.0)
        .expect("zoomed full ROI");
    let draft_roi = LuminaApp::roi_from_zoom(1280, 960, 2.0, egui::Vec2::ZERO, 800.0, 600.0)
        .expect("zoomed draft ROI");
    let back = LuminaApp::roi_in_full_pixels(draft_roi, 2000, 1500, Some((1280, 960)));
    for i in 0..4 {
        assert!(
            (back[i] as i32 - full_roi[i] as i32).abs() <= 2,
            "axis {i}: converted {back:?} vs full {full_roi:?}"
        );
    }
    // A full-space ROI passes through unchanged.
    assert_eq!(
        LuminaApp::roi_in_full_pixels(full_roi, 2000, 1500, Some((2000, 1500))),
        full_roi
    );
}

/// GUI-DRAFT-JUMP-1: draft and full renders of the same zoomed view share
/// on-screen placement (no geometry jump on mouse-up).
///
/// R3-RENDER-SIZE-1: the full path now also caps a large source at the viewport
/// resolution × device pixel ratio (default dpr 1.0 → cap 1040×780), so
/// `preview_render_src` is the capped source, not the 2000×1500 original. Both
/// textures still draw to the same on-screen size and the ROI converts back to
/// the same full-pixel window — the placement contract this test pins.
#[test]
fn draft_and_full_share_on_screen_placement() {
    let mut app = new_app();
    // 2000×1500 source → both the cached draft source and the capped full
    // preview are downscaled, which is exactly the mismatch under test.
    let frame = ImageFrame::new(2000, 1500, [140_u8, 120, 100, 255].repeat(2000 * 1500)).unwrap();
    app.load_bytes(frame.encode(ImageFileFormat::Png).unwrap(), "draft.png")
        .unwrap();
    let (full_w, full_h) = (
        app.original.as_ref().unwrap().width,
        app.original.as_ref().unwrap().height,
    );
    assert_eq!((full_w, full_h), (2000, 1500));
    let draft_w = app.draft_original.as_ref().unwrap().width;
    assert!(
        draft_w < full_w,
        "draft source must be downscaled for this test, got {draft_w}"
    );
    app.zoom_mode = ZoomMode::Custom;
    app.preview_zoom = 2.0;
    app.preview_pane_w = 800.0;
    app.preview_pane_h = 600.0;
    app.render_draft([800, 600], None).unwrap();
    let draft_roi = app.preview_roi.expect("zoomed draft ROI");
    let draft_src = app.preview_render_src.expect("draft source recorded");
    assert_ne!(draft_src, (full_w, full_h));
    let draft_tex = (
        app.preview.as_ref().unwrap().width as f32,
        app.preview.as_ref().unwrap().height as f32,
    );
    app.render_full([800, 600], None).unwrap();
    let full_roi = app.preview_roi.expect("zoomed full ROI");
    let full_src = app.preview_render_src.expect("full source recorded");
    // R3-RENDER-SIZE-1: the full preview is capped too, so its render source is
    // the capped whole frame, not the original (2000×1500 capped to the
    // 800×600 pane → long edge 1040 → 1040×780). It must still be a real
    // downscale of the original so the placement math stays comparable.
    assert_ne!(full_src, (full_w, full_h));
    assert!(
        full_src.0 < full_w && full_src.1 < full_h,
        "capped full source must be smaller than the original: {full_src:?}"
    );
    let full_tex = (
        app.preview.as_ref().unwrap().width as f32,
        app.preview.as_ref().unwrap().height as f32,
    );
    // Same on-screen draw size from both textures at the same scale.
    let scale = 0.4_f32 * 2.0; // fit(800×600 against 2000×1500) × zoom
    let (dw0, dh0) = LuminaApp::preview_draw_dims(
        draft_tex.0,
        draft_tex.1,
        full_w as f32,
        full_h as f32,
        Some(draft_src),
    );
    let (dw1, dh1) = LuminaApp::preview_draw_dims(
        full_tex.0,
        full_tex.1,
        full_w as f32,
        full_h as f32,
        Some(full_src),
    );
    assert!(
        (dw0 * scale - dw1 * scale).abs() <= 1.5,
        "draw widths must match: draft {dw0} vs full {dw1}"
    );
    assert!(
        (dh0 * scale - dh1 * scale).abs() <= 1.5,
        "draw heights must match: draft {dh0} vs full {dh1}"
    );
    // Same ROI in full pixels (the pan-offset half of the jump). R3-RENDER-SIZE-1:
    // both ROIs now live in their own capped render-source space, so both are
    // converted back into full-source pixels before comparing.
    let back = LuminaApp::roi_in_full_pixels(draft_roi, full_w, full_h, Some(draft_src));
    let full_back = LuminaApp::roi_in_full_pixels(full_roi, full_w, full_h, Some(full_src));
    for i in 0..4 {
        assert!(
            (back[i] as i32 - full_back[i] as i32).abs() <= 2,
            "axis {i}: draft {back:?} vs full {full_back:?}"
        );
    }
}

/// GUI-FIT-1: Fit always renders the whole frame and neutralizes pan —
/// switching back from a zoomed Custom crop shows the full image again
/// (the navigator content), never the stale crop corner.
#[test]
fn fit_renders_full_frame_and_neutralizes_pan() {
    let ctx = egui::Context::default();
    let mut app = LuminaApp::new(ctx.clone());
    app.load_bytes(
        ImageFrame::new(200, 150, [128_u8, 128, 128, 255].repeat(200 * 150))
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap(),
        "fit.png",
    )
    .unwrap();
    // Zoomed Custom crop first (the stale-texture setup).
    app.zoom_mode = ZoomMode::Custom;
    app.preview_zoom = 2.0;
    app.preview_pane_w = 800.0;
    app.preview_pane_h = 600.0;
    app.render_full([800, 600], None).unwrap();
    assert!(app.preview_roi.is_some(), "zoomed render must crop");
    // Back to Fit: the mode switch invalidates the crop (re-render arms
    // via `mark_dirty`) and the fresh render covers the whole frame —
    // the same content the navigator shows.
    app.set_zoom_mode(ZoomMode::Fit);
    app.sync_zoom();
    assert_eq!(app.preview_zoom, 1.0);
    assert_eq!(app.preview_pan, egui::Vec2::ZERO);
    app.render_full([800, 600], None).unwrap();
    assert_eq!(app.preview_roi, None, "Fit must render the whole frame");
    assert_eq!(app.preview_render_src, Some((200, 150)));
    // A stale pan offset has no effect on the Fit placement: the draw
    // clamps a smaller-than-pane image to the pane centre and writes the
    // pan back to zero.
    app.preview_pan = egui::vec2(60.0, -40.0);
    app.texture = Some(ctx.load_texture(
        "preview",
        egui::ColorImage::filled([200, 150], egui::Color32::GRAY),
        egui::TextureOptions::LINEAR,
    ));
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(1.0),
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| app.draw_preview(ui));
        },
    );
    output.textures_delta.clear();
    assert_eq!(
        app.preview_pan,
        egui::Vec2::ZERO,
        "pan must be neutralized in Fit"
    );
}

/// GUI-PREVIEW-SCALE-1: the CPU preview must fill the fitted `rect` exactly
/// like the GPU-present path, not paint at the texture's native texel size.
/// `Image::new` derives `ImageFit::Exact(tex.size)` for a texture source and
/// `Ui::put` only supplies `max_rect`, so the old
/// `ui.put(rect, Image::from_texture(..))` drew the 4x3 sample as a 4x3
/// quad while the overlays mapped the fitted `full_rect`. Assert the
/// painted preview shape spans the fit (the 4:3 source fills the 800x600
/// pane's constraining axis) instead of the 4x3 native size; the old code
/// fails every bound below.
#[test]
fn cpu_preview_blit_fills_fitted_rect() {
    let ctx = egui::Context::default();
    let mut app = LuminaApp::new(ctx.clone());
    app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .unwrap();
    assert_eq!(app.image_dims().unwrap(), (4, 3), "sample is 4x3");
    let preview = ctx.load_texture(
        "preview",
        egui::ColorImage::filled([4, 3], egui::Color32::GRAY),
        egui::TextureOptions::LINEAR,
    );
    let preview_id = preview.id();
    app.texture = Some(preview);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(1.0),
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| app.draw_preview(ui));
        },
    );
    output.textures_delta.clear();
    // `painter().image` emits a textured `Shape::Mesh`; the alternative
    // `.fit_to_exact_size(rect.size())` (and the old native-size `Image`
    // widget) emits a textured `Shape::Rect`. Union the bounds of whichever
    // textured shape carries the preview texture id, so the assertion pins
    // the fitted geometry, not the painting primitive.
    let painted = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id == preview_id => Some(mesh.calc_bounds()),
            egui::Shape::Rect(rect_shape) if rect_shape.fill_texture_id() == preview_id => {
                Some(rect_shape.rect)
            }
            _ => None,
        })
        .fold(egui::Rect::NOTHING, |acc, rect| acc.union(rect));
    assert!(
        !painted.is_negative(),
        "the preview texture must be painted, got {painted:?}"
    );
    assert!(
        painted.width() > 100.0 && painted.height() > 100.0,
        "CPU preview must be drawn at the fitted size, got {painted:?} (native-texel-size regression)"
    );
    let (w, h) = (painted.width(), painted.height());
    assert!(
        (w / h - 4.0 / 3.0).abs() < 0.02,
        "fitted preview must preserve the 4:3 source aspect, got {w}x{h}"
    );
    assert!(
        (painted.center().x - screen.center().x).abs() < 1.0
            && (painted.center().y - screen.center().y).abs() < 1.0,
        "fitted preview must be centred in the pane, got {painted:?}"
    );
    assert!(
        painted.height() > 550.0,
        "the constraining axis must nearly fill the 600px pane, got {painted:?}"
    );
}

/// KITTEST-PARITY-PATHS-1: the geometry readouts describe the painted CPU
/// frame headlessly (no GPU): the preview fills the constraining pane axis
/// at its source aspect and the overlay canvas coincides with the photo
/// rect at Fit. This is the adapter-free anchor behind the parity
/// framework's absolute checks (the GPU matrix itself is `#[ignore]`d).
#[test]
fn preview_geometry_readouts_fill_fit_and_anchor_overlays() {
    let ctx = egui::Context::default();
    let mut app = LuminaApp::new(ctx.clone());
    // 2x1 source: unambiguous single constraining axis in an 800x600 pane.
    app.load_bytes(png(), "geometry.png").unwrap();
    app.render().unwrap();
    let preview = ctx.load_texture(
        "preview",
        egui::ColorImage::filled([2, 1], egui::Color32::GRAY),
        egui::TextureOptions::LINEAR,
    );
    app.texture = Some(preview);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(1.0),
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| app.draw_preview(ui));
        },
    );
    output.textures_delta.clear();

    let rect = app.preview_screen_rect().expect("preview was painted");
    let pane = app.preview_pane_rect().expect("preview pane was laid out");
    let overlay = app
        .overlay_full_rect()
        .expect("overlay canvas was laid out");
    // Source aspect preserved and centred in the pane.
    let aspect = rect.width() / rect.height();
    assert!(
        (aspect - 2.0).abs() < 0.02,
        "preview must preserve the 2:1 source aspect, got {rect:?}"
    );
    assert!(
        rect.width() <= pane.width() + 1.0 && rect.height() <= pane.height() + 1.0,
        "preview {rect:?} must fit inside the pane {pane:?}"
    );
    assert!(
        (rect.width() - pane.width()).abs() < 1.0,
        "the 2:1 source must fill the pane width (constraining axis), got {rect:?} in {pane:?}"
    );
    assert!(
        (rect.center() - pane.center()).length() < 1.0,
        "preview must be centred in the pane"
    );
    // Overlays are mapped onto the full photo rect; at Fit it equals the
    // painted preview quad (overlays on the photo, not beside it).
    assert!(
        (overlay.min - rect.min).length() < 1.0 && (overlay.max - rect.max).length() < 1.0,
        "overlay canvas {overlay:?} must coincide with the painted photo {rect:?}"
    );
}

/// GUI-PREVIEW-NAV-1: the wheel only zooms with Ctrl/Cmd held; without a
/// modifier it must scroll/pan and never switch the zoom to `Custom`.
#[test]
fn wheel_zoom_requires_modifier() {
    assert!(!LuminaApp::wants_wheel_zoom(&egui::Modifiers::default()));
    assert!(LuminaApp::wants_wheel_zoom(&egui::Modifiers {
        ctrl: true,
        ..Default::default()
    }));
    assert!(LuminaApp::wants_wheel_zoom(&egui::Modifiers {
        command: true,
        ..Default::default()
    }));
    // Shift alone (horizontal scroll) never zooms.
    assert!(!LuminaApp::wants_wheel_zoom(&egui::Modifiers {
        shift: true,
        ..Default::default()
    }));
}
