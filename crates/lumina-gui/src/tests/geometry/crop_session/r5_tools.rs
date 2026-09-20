//! R5-FIX-WELLE-20 (F-103-N6 Runde 5) crop-tool tests: the straighten commit
//! (R5-STRAIGHTEN-1), the tool-flow switch (R5-TOOLFLOW-1) and the Esc/slider
//! commit boundary (F4).
//!
//! Nested under `tests::geometry::crop_session` so the 500-line-ratcheted parent
//! file stays unchanged.

use super::*;

/// F4 (R5-FIX-WELLE-20): `Esc` after a slider edit discards only the crop
/// **rectangle** draft; the already slider-committed straighten angle stays in
/// the recipe (normative in `feature/platform/lightroom-ux-parity.md`).
#[test]
fn escape_after_slider_edit_keeps_the_rotation_and_drops_only_the_rect() {
    let mut harness = CropHarness::new();
    let (_directory, mut app) = crop_app(&harness.ctx);
    // A slider edit commits the rotation live (R5-STRAIGHTEN-1).
    drag_straighten_slider(&mut app, 0.75);
    let degrees = app
        .recipe()
        .geometry
        .as_ref()
        .map(|geometry| geometry.rotation_degrees)
        .expect("the slider commit must write geometry.rotation_degrees");
    assert!(degrees > 0.0, "drag raised the angle, got {degrees}");

    // A crop-rectangle draft is in flight.
    drag_corner(&mut harness, &mut app, (0.0, 0.0), (0.3, 0.3));
    assert!(
        crop_draft(&harness.ctx).is_some(),
        "rect draft after the drag"
    );

    harness.key_pass(&mut app, egui::Key::Escape);

    assert!(
        crop_draft(&harness.ctx).is_none(),
        "Esc must discard the rectangle draft"
    );
    assert!(
        app.recipe()
            .geometry
            .as_ref()
            .and_then(|geometry| geometry.crop.as_ref())
            .is_none(),
        "the rectangle must not reach the recipe"
    );
    assert_eq!(
        app.recipe()
            .geometry
            .as_ref()
            .map(|geometry| geometry.rotation_degrees),
        Some(degrees),
        "the slider-committed rotation must stay committed"
    );
}

/// One toolbar-icon click through the **real** `draw_preview_area` toolbar in a
/// persistent context (so a crop draft stored in that context is present).
fn click_view_toolbar_icon(
    ctx: &egui::Context,
    app: &mut LuminaApp,
    icon: crate::icon_toolbar::ToolbarIcon,
    time: &mut f64,
) {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));
    let mut run = |app: &mut LuminaApp, events: Vec<egui::Event>| {
        *time += 1.0 / 60.0;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(*time),
                events,
                ..Default::default()
            },
            |ui| {
                let ctx = ui.ctx().clone();
                app.draw_preview_area(&ctx, ui);
            },
        );
        output.textures_delta.clear();
    };
    run(app, vec![]);
    let pos = ctx
        .read_response(icon.id())
        .unwrap_or_else(|| panic!("icon {icon:?} must be painted"))
        .rect
        .center();
    let click = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    run(app, vec![egui::Event::PointerMoved(pos), click(true)]);
    run(app, vec![egui::Event::PointerMoved(pos), click(false)]);
    run(app, vec![]);
}

/// R5-TOOLFLOW-1 (User-Entscheid 2026-09-20), direction Crop → Masking: arming
/// the mask tool through the real toolbar commits the active crop/straighten
/// draft (recipe + one history step + debounced save) and opens Masking. The
/// former geometry lock refused the tool with a dead-end banner.
#[test]
fn masking_switch_commits_the_crop_draft_and_opens_masking() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let ctx = egui::Context::default();
    let mut app = LuminaApp::new(ctx.clone());
    open_and_decode(&mut app, source.display().to_string());
    app.render().unwrap();
    app.texture = Some(ctx.load_texture(
        "preview",
        egui::ColorImage::filled([2, 1], egui::Color32::GRAY),
        egui::TextureOptions::LINEAR,
    ));
    // A crop-mode straighten draft that the tool switch must commit.
    set_crop_rotation_draft(&ctx, Some(6.5));
    app.toggle_crop_mode();
    assert!(app.crop_mode, "precondition: the crop tool is armed");

    let mut time = 0.0;
    click_view_toolbar_icon(
        &ctx,
        &mut app,
        crate::icon_toolbar::ToolbarIcon::Masking,
        &mut time,
    );

    assert_eq!(app.mask_tool, MaskTool::Brush, "Masking must be open");
    assert!(!app.crop_mode, "the crop tool must be left by the switch");
    assert_eq!(
        app.recipe()
            .geometry
            .as_ref()
            .map(|geometry| geometry.rotation_degrees),
        Some(6.5),
        "the crop draft must be committed by the switch"
    );
    assert!(
        crop_rotation_draft(&ctx).is_none(),
        "the committed draft must be consumed"
    );

    // The commit is persisted and shows as one history step (DoD §1 anchor).
    let document = commit_and_load_doc(&mut app, &source);
    assert!(
        document.virtual_copies[0].history.iter().any(|entry| entry
            .extras
            .get("action")
            .and_then(|value| value.as_str())
            == Some("geometry.straighten")),
        "the switch commit must produce a geometry history step"
    );
}

/// R5-TOOLFLOW-1, direction Masking → Crop: arming the geometry tool disarms
/// the source-coordinate tools instead of leaving them armed-but-refused.
#[test]
fn arming_crop_disarms_masking_and_wb() {
    let mut app = new_app();
    app.set_mask_tool(MaskTool::Brush);
    assert_eq!(app.mask_tool, MaskTool::Brush);
    app.arm_wb_picker();
    assert!(app.wb_pick_mode, "WB eyedropper armed");

    app.toggle_crop_mode();

    assert!(app.crop_mode, "the crop tool is armed");
    assert_eq!(app.mask_tool, MaskTool::None, "arming crop disarms Masking");
    assert!(!app.wb_pick_mode, "arming crop disarms the WB eyedropper");
}

/// R5-STRAIGHTEN-1 end-to-end (DoD §1): the crop-bar straighten drag changes
/// the committed render and the value survives a sidecar reload. Before the fix
/// the drag only wrote a ctx draft — no log, no recipe, no render change, and
/// "Save Recipe" persisted nothing.
#[test]
fn crop_bar_straighten_changes_render_and_survives_reload() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.render().unwrap();
    let before = app.preview().expect("baseline render").clone();

    drag_straighten_slider(&mut app, 0.8);
    let degrees = app
        .recipe()
        .geometry
        .as_ref()
        .map(|geometry| geometry.rotation_degrees)
        .expect("the drag must commit geometry.rotation_degrees");
    assert!(degrees > 5.0, "drag must raise the angle, got {degrees}");

    // The committed value reaches the render (crop mode is not armed here, so
    // the geometry stage is applied).
    app.render().unwrap();
    let after = app.preview().expect("rotated render").clone();
    assert_ne!(
        (before.width, before.height, &before.pixels),
        (after.width, after.height, &after.pixels),
        "the render must change once the straighten value is committed"
    );

    // Persist + reload: the value is in the sidecar (DoD §1 end-to-end anchor).
    let document = commit_and_load_doc(&mut app, &source);
    assert!(
        document.virtual_copies[0].history.iter().any(|entry| entry
            .extras
            .get("action")
            .and_then(|value| value.as_str())
            == Some("geometry.straighten")),
        "the straighten commit must produce a history step"
    );
    let reopened = reopen_app(&source);
    assert_eq!(
        reopened
            .recipe()
            .geometry
            .as_ref()
            .map(|geometry| geometry.rotation_degrees),
        Some(degrees),
        "the straighten value must survive a reload"
    );
}
