//! The preview zoom-mode gate (`Fit` ↔ `Custom`): which gesture may leave
//! `Fit`, and that `Fit` survives a load.
//!
//! Split out of `tests/navigator.rs` (module `tests::navigator`) to buy
//! headroom under the 500-line ratchet. This file owns the *mode* tests, the
//! navigator rectangle / overview / drag math stays in the parent; the three
//! test bodies moved verbatim and pull the shared fixtures in via
//! `use super::*`.
//!
//! - `preview_wheel_ignores_pointer_outside_pane` (GUI-VIEW-2): the wheel acts
//!   only over the preview pane, never over a panel.
//! - `pan_gesture_pins_custom_matrix` (GUI-ZOOM-CUSTOM-1): the pan-gesture pin
//!   matrix.
//! - `fit_load_reads_fit_and_draw_keeps_zero_pan` (GUI-ZOOM-CUSTOM-1): `Fit`
//!   after a load, and a drawn `Fit` frame neutralizes a stale pan.

use super::*;

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
