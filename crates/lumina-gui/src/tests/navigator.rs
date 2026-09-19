//! navigator viewport/rect/zoom/pan tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// GUI-PREVIEW-NAV-1: the navigator viewport rectangle tracks zoom/pan and
/// a drag of the rectangle round-trips back through `preview_pan`.
#[test]
fn navigator_viewport_rect_roundtrip() {
    // 300×200 source shown in a 150×100 navigator cell (scale 0.5);
    // preview pane 800×600, source fit 8/3 ≈ 2.667. At 4× zoom the
    // effective scale is 32/3 ≈ 10.667 → visible 75×56.25 source px.
    let nav = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(150.0, 100.0));
    let zoomed_scale = 32.0_f32 / 3.0;
    let view = LuminaApp::navigator_viewport_rect(
        nav,
        300.0,
        200.0,
        800.0,
        600.0,
        zoomed_scale,
        egui::Vec2::ZERO,
    );
    // Centred and strictly inside the navigator while zoomed.
    assert!(nav.contains_rect(view));
    assert!(view.width() < nav.width() && view.height() < nav.height());
    assert!((view.center().x - nav.center().x).abs() < 1e-3);
    assert!((view.center().y - nav.center().y).abs() < 1e-3);
    assert!((view.width() - 37.5).abs() < 1e-3);
    assert!((view.height() - 28.125).abs() < 1e-3);

    // Fit shows the whole frame: the rectangle equals the navigator.
    let fit = LuminaApp::navigator_viewport_rect(
        nav,
        300.0,
        200.0,
        800.0,
        600.0,
        8.0 / 3.0,
        egui::Vec2::ZERO,
    );
    assert_eq!(fit, nav);

    // Dragging the rectangle 10 navigator points right moves the visible
    // window right by exactly that amount in navigator space: the pan
    // shift is `-drag * (preview_scale / nav_scale)`.
    let pan = LuminaApp::pan_for_navigator_drag(
        egui::Vec2::ZERO,
        egui::vec2(10.0, 0.0),
        0.5,
        zoomed_scale,
    );
    assert!(
        (pan.x + 10.0 * (zoomed_scale / 0.5)).abs() < 1e-3,
        "unexpected pan {pan:?}"
    );
    assert_eq!(pan.y, 0.0);
    let moved =
        LuminaApp::navigator_viewport_rect(nav, 300.0, 200.0, 800.0, 600.0, zoomed_scale, pan);
    assert!((moved.center().x - view.center().x - 10.0).abs() < 1e-3);
    assert!((moved.center().y - view.center().y).abs() < 1e-3);

    // Degenerate geometry never panics and degrades to the full rect.
    assert_eq!(
        LuminaApp::navigator_viewport_rect(nav, 0.0, 0.0, 800.0, 600.0, 0.8, egui::Vec2::ZERO),
        nav
    );
    assert_eq!(
        LuminaApp::pan_for_navigator_drag(egui::Vec2::ZERO, egui::vec2(5.0, 5.0), 0.0, 0.8),
        egui::Vec2::ZERO
    );
}

/// GUI-VIEW-2 (Scroll-Bleed): the preview wheel acts only with the
/// pointer over the preview *pane*. A wheel over a side panel (pointer
/// outside the pane — e.g. over the Basic panel while the zoomed image
/// rect extends underneath it) must never zoom or pan the image.
#[test]
fn preview_wheel_ignores_pointer_outside_pane() {
    use egui::{Event, Modifiers, MouseWheelUnit, TouchPhase};
    let ctx = egui::Context::default();
    let mut app = LuminaApp::new(ctx.clone());
    app.load_bytes(
        ImageFrame::new(200, 150, [128_u8, 128, 128, 255].repeat(200 * 150))
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap(),
        "wheel.png",
    )
    .unwrap();
    app.render().unwrap();
    app.texture = Some(ctx.load_texture(
        "preview",
        egui::ColorImage::filled([200, 150], egui::Color32::GRAY),
        egui::TextureOptions::LINEAR,
    ));
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let wheel_ctrl = Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: egui::vec2(0.0, 50.0),
        phase: TouchPhase::Move,
        modifiers: Modifiers {
            ctrl: true,
            ..Default::default()
        },
    };
    // Pointer in the window corner — inside the screen rect but outside
    // the preview pane (panel territory): zoom and pan must not move.
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(1.0),
            events: vec![
                Event::PointerMoved(egui::pos2(2.0, 2.0)),
                wheel_ctrl.clone(),
            ],
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| app.draw_preview(ui));
        },
    );
    output.textures_delta.clear();
    assert_eq!(app.zoom_mode, ZoomMode::Fit);
    assert_eq!(app.preview_zoom, 1.0);
    assert_eq!(app.preview_pan, egui::Vec2::ZERO);
    assert!(!app.pending_full_render, "no re-render may be armed");

    // Same wheel over the image centre zooms as before (the gate only
    // removes the bleed, not the feature).
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(2.0),
            events: vec![Event::PointerMoved(egui::pos2(400.0, 300.0)), wheel_ctrl],
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| app.draw_preview(ui));
        },
    );
    output.textures_delta.clear();
    assert_eq!(app.zoom_mode, ZoomMode::Custom);
    assert!(
        (app.preview_zoom - 1.1).abs() < 1e-4,
        "ctrl-wheel over the preview zooms, got {}",
        app.preview_zoom
    );
}

/// GUI-VIEW-2 (N6): the navigator rail (overview + viewport rectangle,
/// F-100) is visible by default — default-hidden made the rectangle
/// unfindable.
#[test]
fn navigator_rail_open_by_default() {
    assert!(new_app().navigator_open);
}

/// GUI-ZOOM-CUSTOM-1: the pan-gesture gate matrix — `Custom` pins only
/// for a genuinely magnified, overflowing view. At Fit (or zoomed out)
/// no pan gesture may flip the mode, however far the drawn image
/// overflows (e.g. a stale oversized texture right after load).
#[test]
fn pan_gesture_pins_custom_matrix() {
    // At Fit: never, even with a hugely overflowing draw rect.
    assert!(!LuminaApp::pan_gesture_pins_custom(
        1.0, 800.0, 600.0, 800.0, 600.0
    ));
    assert!(!LuminaApp::pan_gesture_pins_custom(
        1.0, 5000.0, 4000.0, 800.0, 600.0
    ));
    // Zoomed out: never.
    assert!(!LuminaApp::pan_gesture_pins_custom(
        0.5, 900.0, 700.0, 800.0, 600.0
    ));
    // Zoomed in but fully visible: nothing to pan, never.
    assert!(!LuminaApp::pan_gesture_pins_custom(
        2.0, 800.0, 600.0, 800.0, 600.0
    ));
    assert!(!LuminaApp::pan_gesture_pins_custom(
        2.0, 800.4, 600.4, 800.0, 600.0
    ));
    // Zoomed in and overflowing: the only pinning case.
    assert!(LuminaApp::pan_gesture_pins_custom(
        2.0, 1600.0, 1200.0, 800.0, 600.0
    ));
    assert!(LuminaApp::pan_gesture_pins_custom(
        2.0, 800.0, 600.6, 800.0, 600.0
    ));
    assert!(LuminaApp::pan_gesture_pins_custom(
        1.01, 810.0, 600.0, 800.0, 600.0
    ));
}

/// GUI-ZOOM-CUSTOM-1: a fresh load reads Fit, and a drawn Fit frame
/// keeps the mode Fit with zero pan even when a stale pan offset is
/// pending (the load-window caricature of the user finding).
#[test]
fn fit_load_reads_fit_and_draw_keeps_zero_pan() {
    let (png, _) = synthetic_gradient_png();
    let mut app = new_app();
    app.load_bytes(png, "gradient.png").unwrap();
    assert_eq!(app.zoom_mode, ZoomMode::Fit);
    assert_eq!(app.zoom_label(), Str::ZoomFit.t());
    // Stale pan offset (e.g. carried geometry before the load reset).
    app.preview_pan = egui::vec2(42.0, -17.0);
    app.render().unwrap();
    let shapes = headless_shapes(&mut app, |app, ui| {
        let ctx = ui.ctx().clone();
        app.update_texture(&ctx);
        app.draw_preview(ui);
    });
    assert!(!shapes.is_empty(), "preview must paint");
    assert_eq!(app.zoom_mode, ZoomMode::Fit, "drawing at Fit keeps Fit");
    assert_eq!(
        app.preview_pan,
        egui::Vec2::ZERO,
        "drawing at Fit neutralizes pan"
    );
}

/// GUI-NAV-RECT-1 (Zoom×Pan-Matrix, gemeinsam mit GUI-ZOOM-CUSTOM-1
/// diagnostiziert): the navigator rectangle is the visible window in
/// source pixels mapped into the overview — full at Fit, smaller and
/// centred at 100 %, shifted by pan, always clamped inside.
#[test]
fn navigator_rect_zoom_pan_matrix() {
    let nav = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 200.0));
    // Fit: the whole frame is visible — the rect equals the overview.
    let fit =
        LuminaApp::navigator_viewport_rect(nav, 600.0, 400.0, 600.0, 400.0, 1.0, egui::Vec2::ZERO);
    assert_eq!(fit, nav, "Fit must show the full frame");
    // 100 % in a half-size pane: quarter-area window, centred without pan.
    let zoomed =
        LuminaApp::navigator_viewport_rect(nav, 600.0, 400.0, 300.0, 200.0, 1.0, egui::Vec2::ZERO);
    assert!(
        nav.contains_rect(zoomed),
        "zoomed rect must stay inside the overview"
    );
    assert!(
        (zoomed.width() - 150.0).abs() < 1.0 && (zoomed.height() - 100.0).abs() < 1.0,
        "100 % in a half pane shows a quarter window, got {zoomed:?}"
    );
    assert!(
        (zoomed.center().x - nav.center().x).abs() < 1.0
            && (zoomed.center().y - nav.center().y).abs() < 1.0,
        "zero pan centres the window, got {zoomed:?}"
    );
    // Custom zoom 2 + pan: same-size window as above (pane/scale equal),
    // shifted against the pan direction.
    let custom = LuminaApp::navigator_viewport_rect(
        nav,
        600.0,
        400.0,
        600.0,
        400.0,
        2.0,
        egui::vec2(60.0, -40.0),
    );
    assert!(nav.contains_rect(custom));
    assert!(
        (custom.width() - zoomed.width()).abs() < 1.0,
        "equal pane/scale ratios show equal windows: {custom:?} vs {zoomed:?}"
    );
    assert!(
        custom.center().x < zoomed.center().x,
        "positive pan.x shifts the window left in source space: {custom:?} vs {zoomed:?}"
    );
    assert!(
        custom.center().y > zoomed.center().y,
        "negative pan.y shifts the window down: {custom:?}"
    );
    // Higher zoom: strictly smaller window.
    let closer =
        LuminaApp::navigator_viewport_rect(nav, 600.0, 400.0, 600.0, 400.0, 4.0, egui::Vec2::ZERO);
    assert!(closer.width() < zoomed.width() && closer.height() < zoomed.height());
    // Absurd pan clamps inside instead of leaving the overview.
    let clamped = LuminaApp::navigator_viewport_rect(
        nav,
        600.0,
        400.0,
        600.0,
        400.0,
        4.0,
        egui::vec2(5000.0, -5000.0),
    );
    assert!(
        nav.contains_rect(clamped),
        "clamped rect must stay inside, got {clamped:?}"
    );
    // Degenerate geometry falls back to the full overview, never NaN.
    let degenerate =
        LuminaApp::navigator_viewport_rect(nav, 600.0, 400.0, 0.0, 0.0, 0.0, egui::Vec2::ZERO);
    assert_eq!(degenerate, nav);
    assert!(degenerate.is_finite());
}

/// GUI-NAV-RECT-1: the navigator overview is the FULL source even when
/// the preview is an ROI crop (zoomed) — the rect math maps full-source
/// coordinates and must see a full-source image.
#[test]
fn navigator_overview_is_full_source_despite_roi_crop() {
    let (png, _) = synthetic_gradient_png();
    let mut app = new_app();
    app.load_bytes(png, "gradient.png").unwrap();
    // Zoom into an ROI crop.
    app.preview_zoom = 8.0;
    app.zoom_mode = ZoomMode::Custom;
    app.preview_pan = egui::vec2(42.0, -17.0);
    app.render().unwrap();
    let preview = app.preview().unwrap().clone();
    assert!(
        app.preview_roi.is_some(),
        "zoomed render must carry an ROI crop"
    );
    assert!(
        (preview.width, preview.height) != (64, 40),
        "the preview texture is a crop, not the full frame"
    );
    let overview = app.navigator_frame().expect("navigator source").clone();
    assert_eq!(
        (overview.width, overview.height),
        (64, 40),
        "navigator overview must stay full-frame under zoom"
    );
    // The drawn overview serves the same full-frame source (headless
    // draw of the viewport, zoomed path): key + texture follow the
    // source identity, never the ROI crop.
    let shapes = headless_shapes(&mut app, |app, ui| {
        let ctx = ui.ctx().clone();
        app.draw_navigator_viewport(&ctx, ui);
    });
    assert!(!shapes.is_empty(), "navigator viewport must paint");
    let key = app.navigator_texture_key.clone().expect("navigator key");
    assert_eq!(key.1, 64);
    assert_eq!(key.2, 40);
    assert!(
        app.navigator_texture.is_some(),
        "navigator overview texture must exist"
    );
}

/// P0-Audit (DoD §3, GUI-PREVIEW-NAV-1): Navigator-Drag Ende-zu-Ende —
/// derselbe Dreischritt wie `draw_navigator_viewport` (Helper → Custom-Pin
/// → dirty) bewegt das sichtbare Fenster mit dem Cursor.
#[test]
fn navigator_drag_pans_preview_and_pins_custom() {
    let mut app = new_app();
    app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .unwrap();
    // Zoomed Zustand wie im laufenden Betrieb (Custom trägt den ROI).
    app.zoom_mode = ZoomMode::Custom;
    app.preview_zoom = 4.0;
    app.preview_pan = egui::Vec2::ZERO;
    app.preview_effective_scale = 32.0 / 3.0;
    let nav_scale = 0.5_f32;
    let preview_scale = app.preview_effective_scale;
    let drag = egui::vec2(10.0, -4.0);
    // Produktions-Dreischritt aus `draw_navigator_viewport`.
    app.preview_pan =
        LuminaApp::pan_for_navigator_drag(app.preview_pan, drag, nav_scale, preview_scale);
    app.zoom_mode = ZoomMode::Custom;
    app.mark_dirty();
    let expect_x = -drag.x * (preview_scale / nav_scale);
    let expect_y = -drag.y * (preview_scale / nav_scale);
    assert!(
        (app.preview_pan.x - expect_x).abs() < 1e-3,
        "pan.x = {}, erwartet {expect_x}",
        app.preview_pan.x
    );
    assert!(
        (app.preview_pan.y - expect_y).abs() < 1e-3,
        "pan.y = {}, erwartet {expect_y}",
        app.preview_pan.y
    );
    assert_eq!(
        app.zoom_mode,
        ZoomMode::Custom,
        "navigator drag must pin Custom so sync_zoom keeps the pan"
    );
    assert_ne!(
        app.preview_pan,
        egui::Vec2::ZERO,
        "drag must move the visible window"
    );
    // Roundtrip durch das Viewport-Rechteck: das Fenster folgt dem Cursor
    // exakt um den Drag in Navigator-Punkten.
    let nav = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(150.0, 100.0));
    let before = LuminaApp::navigator_viewport_rect(
        nav,
        300.0,
        200.0,
        800.0,
        600.0,
        preview_scale,
        egui::Vec2::ZERO,
    );
    let after = LuminaApp::navigator_viewport_rect(
        nav,
        300.0,
        200.0,
        800.0,
        600.0,
        preview_scale,
        app.preview_pan,
    );
    assert!((after.center().x - before.center().x - drag.x).abs() < 1e-2);
    assert!((after.center().y - before.center().y - drag.y).abs() < 1e-2);
}

/// GUI-NAV-RECT-1 + PERF-GUI-5: the navigator rectangle and the render ROI
/// describe the same visible window — their centres coincide and the ROI
/// is the navigator window expanded by exactly the pan margin.
#[test]
fn navigator_rect_matches_roi_from_zoom() {
    let (src_w, src_h) = (600.0f32, 400.0f32);
    let (pane_w, pane_h) = (300.0f32, 200.0f32);
    let zoom = 2.0f32;
    let pan = egui::Vec2::ZERO;
    let roi = LuminaApp::roi_from_zoom(600, 400, zoom, pan, pane_w, pane_h)
        .expect("a 2x zoom must crop an ROI");
    let fit = (f64::from(pane_w) / 600.0).min(f64::from(pane_h) / 400.0);
    let scale = zoom * fit as f32;
    let nav = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 200.0));
    let rect = LuminaApp::navigator_viewport_rect(nav, src_w, src_h, pane_w, pane_h, scale, pan);
    // Back to source pixels (the overview maps 600x400 onto 300x200).
    let to_src_x = 600.0 / nav.width();
    let to_src_y = 400.0 / nav.height();
    let nav_src = [
        (rect.min.x - nav.min.x) * to_src_x,
        (rect.min.y - nav.min.y) * to_src_y,
        rect.width() * to_src_x,
        rect.height() * to_src_y,
    ];
    let roi_cx = roi[0] as f32 + roi[2] as f32 / 2.0;
    let roi_cy = roi[1] as f32 + roi[3] as f32 / 2.0;
    let nav_cx = nav_src[0] + nav_src[2] / 2.0;
    let nav_cy = nav_src[1] + nav_src[3] / 2.0;
    assert!(
        (roi_cx - nav_cx).abs() < 1.0 && (roi_cy - nav_cy).abs() < 1.0,
        "ROI centre ({roi_cx},{roi_cy}) must match the navigator window ({nav_cx},{nav_cy})"
    );
    let margin = PREVIEW_ROI_MARGIN as f32;
    assert!(
        (roi[2] as f32 - nav_src[2] * margin).abs() < 2.0
            && (roi[3] as f32 - nav_src[3] * margin).abs() < 2.0,
        "ROI {:?} must be the navigator window {nav_src:?} expanded by the margin {margin}",
        roi
    );
}
