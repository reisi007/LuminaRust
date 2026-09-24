//! R5-BRUSH-24 non-adapter structural coverage for the combined management UI.

use super::*;

pub(super) fn draw_representative_management_surface(app: &mut LuminaApp, ui: &mut egui::Ui) {
    let document = app.document.clone().expect("document loaded");
    ui.columns(2, |columns| {
        columns[0].label("Mask library");
        let _ = app.draw_mask_library(&mut columns[0], &document);
        columns[1].label("Brush controls");
        app.draw_brush_controls(&mut columns[1]);
    });
}

#[test]
fn mask_management_controls_have_headless_structural_action_coverage() {
    let mut app = new_app();
    app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .unwrap();
    let subject = app
        .create_luminance_range_mask(0.0, 0.5, 0.0, "Subject")
        .unwrap();
    let background = app
        .create_luminance_range_mask(0.5, 1.0, 0.0, "Background")
        .unwrap();
    app.set_mask_tool(MaskTool::Brush);
    app.set_brush_softness(0.4).unwrap();
    app.set_brush_flow(0.7).unwrap();

    // This is deliberately a non-adapter egui frame at the 1024x720 reference
    // size. It proves that the representative controls are laid out and not
    // clipped before the native pixel snapshot is attempted.
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 720.0));
    let mut shapes = Vec::new();
    for frame in 0..3 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(frame as f64 / 60.0),
                ..Default::default()
            },
            |ui| draw_representative_management_surface(&mut app, ui),
        );
        output.textures_delta.clear();
        shapes = output.shapes;
    }
    for label in [
        Str::MaskEye.t(),
        Str::MoveMaskUp.t(),
        Str::MoveMaskDown.t(),
        Str::RenameMask.t(),
        Str::DeleteMaskButton.t(),
        Str::DuplicateMask.t(),
        Str::DuplicateGroup.t(),
        Str::BrushSize.t(),
        Str::BrushSoftness.t(),
        Str::BrushFlow.t(),
        Str::BrushEraser.t(),
    ] {
        assert_fully_visible(&shapes, label);
    }
    for clipped in &shapes {
        if let egui::Shape::Text(text) = &clipped.shape {
            assert!(
                text.pos.x <= screen.max.x + 0.5 && text.pos.y <= screen.max.y + 0.5,
                "representative controls escaped the 1024x720 layout: {:?}",
                text.pos
            );
        }
    }

    // Drive the actual row widgets, not just their labels: rename, reorder,
    // visibility, copy, and delete all route through the production methods.
    app.mask_rename_inputs
        .insert(subject.clone(), "Renamed by widget".into());
    headless_click_labels_sized(
        &mut app,
        1200.0,
        &[Str::RenameMask.t()],
        draw_representative_management_surface,
    );
    assert_eq!(
        app.document.as_ref().unwrap().virtual_copies[0].mask_library[0].name,
        "Renamed by widget"
    );
    headless_click_labels_sized(
        &mut app,
        1200.0,
        &[Str::MoveMaskDown.t()],
        draw_representative_management_surface,
    );
    assert_eq!(
        app.document.as_ref().unwrap().virtual_copies[0].mask_library[1].id,
        subject
    );
    let count = app.document.as_ref().unwrap().virtual_copies[0]
        .mask_library
        .len();
    headless_click_labels_sized(
        &mut app,
        1200.0,
        &[Str::DuplicateMask.t()],
        draw_representative_management_surface,
    );
    assert_eq!(
        app.document.as_ref().unwrap().virtual_copies[0]
            .mask_library
            .len(),
        count + 1
    );
    headless_click_labels_sized(
        &mut app,
        1200.0,
        &[Str::MaskEye.t()],
        draw_representative_management_surface,
    );
    assert!(!app.mask_visible(&background));
    headless_click_labels_sized(
        &mut app,
        1200.0,
        &[Str::DeleteMaskButton.t()],
        draw_representative_management_surface,
    );
    assert_eq!(
        app.document.as_ref().unwrap().virtual_copies[0]
            .mask_library
            .len(),
        count
    );
    assert!(
        app.error().is_none(),
        "headless actions failed: {:?}",
        app.error()
    );
}
