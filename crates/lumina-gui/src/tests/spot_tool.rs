//! R5-DUST-23: headless tests for the Dust-Removal toolbar tool — toolbar
//! arming, size → dab radius, dab → healing → reload, the `[`/`]` size alias
//! and the live-size circle cursor.
//!
//! The former sidebar panel never wired a pointer handler, so an armed tool
//! did nothing on the image ("Dust Removal funktioniert nicht"). These tests
//! drive the real preview widget with synthetic pointer events, exactly like
//! the red-eye picker coverage.

use super::*;
use crate::spot_tool::spot_size_factor_for_key;

/// 16×16 fixture: light background with a 2×2 dark dust block at the center.
/// A dab at (0.5, 0.5) with the auto-clone offset samples the clean
/// background, so the healed pixels visibly change.
fn dust_spot_png() -> Vec<u8> {
    let mut pixels = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16u32 {
        for x in 0..16u32 {
            let dust = (7..=8).contains(&x) && (7..=8).contains(&y);
            let v = if dust { 40u8 } else { 220u8 };
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    ImageFrame::new(16, 16, pixels)
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap()
}

/// Write the dust fixture to a tempdir and open it (async decode drained).
fn dust_app() -> (tempfile::TempDir, LuminaApp, egui::Context) {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("dust.png");
    std::fs::write(&source, dust_spot_png()).unwrap();
    let ctx = egui::Context::default();
    let mut app = LuminaApp::new(ctx.clone());
    open_and_decode(&mut app, source.display().to_string());
    app.render().unwrap();
    app.texture = Some(ctx.load_texture(
        "preview",
        egui::ColorImage::filled([16, 16], egui::Color32::BLACK),
        egui::TextureOptions::LINEAR,
    ));
    (directory, app, ctx)
}

fn preview_pass(
    app: &mut LuminaApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    time: f64,
) -> Vec<egui::epaint::ClippedShape> {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(time),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| app.draw_preview(ui));
        },
    );
    output.textures_delta.clear();
    output.shapes
}

fn pointer_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

/// Click the preview center on the real widget (warm-up, press, release).
fn click_preview_center(app: &mut LuminaApp, ctx: &egui::Context) {
    let pos = egui::Pos2::new(400.0, 300.0);
    let mut t = 0.5;
    preview_pass(app, ctx, vec![egui::Event::PointerMoved(pos)], t);
    t += 0.05;
    preview_pass(
        app,
        ctx,
        vec![egui::Event::PointerMoved(pos), pointer_button(pos, true)],
        t,
    );
    preview_pass(app, ctx, vec![pointer_button(pos, false)], t + 0.05);
}

fn circle_radii(shapes: &[egui::epaint::ClippedShape]) -> Vec<f32> {
    shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Circle(circle) => Some(circle.radius),
            _ => None,
        })
        .collect()
}

#[test]
fn spot_size_shortcut_mapping_is_exact_and_collision_free() {
    assert_eq!(
        spot_size_factor_for_key(egui::Key::OpenBracket),
        Some(1.0 / 1.1)
    );
    assert_eq!(spot_size_factor_for_key(egui::Key::CloseBracket), Some(1.1));
    for key in [
        egui::Key::Q,
        egui::Key::K,
        egui::Key::M,
        egui::Key::P,
        egui::Key::X,
        egui::Key::U,
        egui::Key::Y,
        egui::Key::R,
        egui::Key::F,
        egui::Key::L,
        egui::Key::J,
        egui::Key::G,
        egui::Key::D,
        egui::Key::E,
        egui::Key::S,
        egui::Key::V,
    ] {
        assert_eq!(spot_size_factor_for_key(key), None, "{key:?}");
    }
}

#[test]
fn spot_size_shortcut_resizes_only_the_armed_tool() {
    let mut app = new_app();
    app.set_spot_radius(10.0);
    let ctx = egui::Context::default();
    let press = |app: &mut LuminaApp, ctx: &egui::Context, key: egui::Key| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }],
                ..Default::default()
            },
            |ui| app.handle_spot_size_shortcuts(ui.ctx()),
        );
        output.textures_delta.clear();
    };
    // Disarmed: `[`/`]` are a no-op.
    press(&mut app, &ctx, egui::Key::CloseBracket);
    assert_eq!(app.spot_radius, 10.0);
    // Armed: `]` grows, `[` shrinks back (clamped to the validated range).
    app.set_spot_tool(SpotTool::Heal);
    press(&mut app, &ctx, egui::Key::CloseBracket);
    assert!(app.spot_radius > 10.0);
    press(&mut app, &ctx, egui::Key::OpenBracket);
    assert!((app.spot_radius - 10.0).abs() < 1e-3);
    // The clamp holds on both ends.
    app.set_spot_radius(1.0);
    for _ in 0..40 {
        press(&mut app, &ctx, egui::Key::OpenBracket);
    }
    assert_eq!(app.spot_radius, 1.0);
    app.set_spot_radius(512.0);
    for _ in 0..40 {
        press(&mut app, &ctx, egui::Key::CloseBracket);
    }
    assert_eq!(app.spot_radius, 512.0);
}

#[test]
fn arming_a_mask_tool_disarms_the_spot_tool() {
    let mut app = new_app();
    app.set_spot_tool(SpotTool::Heal);
    assert_eq!(app.spot_tool(), SpotTool::Heal);
    app.set_mask_tool(MaskTool::Brush);
    assert_eq!(app.mask_tool, MaskTool::Brush);
    assert_eq!(
        app.spot_tool(),
        SpotTool::None,
        "arming a mask tool must disarm the spot tool"
    );
    app.set_spot_tool(SpotTool::Heal);
    assert_eq!(
        app.mask_tool,
        MaskTool::None,
        "arming the spot tool must disarm the mask tool"
    );
}

#[test]
fn spot_dab_persists_heals_and_reloads() {
    let (directory, mut app, ctx) = dust_app();
    app.set_spot_tool(SpotTool::Heal);
    app.set_spot_radius(2.0);
    let source = directory.path().join("dust.png");
    let before = app.preview().expect("preview after load").pixels.clone();
    let generation = app.preview_generation();
    click_preview_center(&mut app, &ctx);
    // Recipe: exactly one heuristic spot with the armed size at the center.
    let spots: Vec<serde_json::Value> =
        serde_json::from_value(app.recipe().extras["spot_removals"].clone())
            .expect("spot_removals array");
    assert_eq!(spots.len(), 1);
    assert_eq!(spots[0]["mode"], "heuristic");
    assert_eq!(spots[0]["radius"].as_f64(), Some(2.0));
    let cx = spots[0]["center_x"].as_f64().unwrap();
    let cy = spots[0]["center_y"].as_f64().unwrap();
    assert!(
        (cx - 0.5).abs() < 0.02 && (cy - 0.5).abs() < 0.02,
        "{cx},{cy}"
    );
    // Render leg: the dab visibly changed the preview (no stale frame).
    assert!(app.preview_generation() > generation);
    let after = app.preview().expect("preview after dab").pixels.clone();
    assert_ne!(before, after, "dab must heal the preview");
    let center = (8 * 16 + 8) as usize * 4;
    assert!(after[center] > before[center], "dust pixel must brighten");
    // Reload leg: the spot survives a fresh open from the same sidecar.
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    let spots: Vec<serde_json::Value> =
        serde_json::from_value(reopened.recipe().extras["spot_removals"].clone())
            .expect("spot_removals after reload");
    assert_eq!(spots.len(), 1);
    assert_eq!(spots[0]["radius"].as_f64(), Some(2.0));
}

#[test]
fn spot_dab_is_a_no_op_when_disarmed() {
    let (_directory, mut app, ctx) = dust_app();
    click_preview_center(&mut app, &ctx);
    assert!(
        !app.recipe().extras.contains_key("spot_removals"),
        "a disarmed tool must never place a spot"
    );
}

#[test]
fn spot_tool_options_keep_all_three_labels_in_the_center_panel() {
    // R5-DUST-23 (B1): a single wrapped tool row overflowed the center panel
    // at 1024×720 and the histogram panel clipped the Feather/Opacity controls
    // (their labels disappeared). The rows are now one control each and must
    // keep Size, Feather and Opacity visible WITH their labels inside the
    // central pane at the reference viewport.
    let mut app = new_app();
    app.set_spot_tool(SpotTool::Heal);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 720.0));
    let mut center = egui::Rect::NOTHING;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(1.0),
            ..Default::default()
        },
        |ui| {
            egui::Panel::left("rail")
                .exact_size(239.0)
                .show(ui, |_ui| {});
            egui::Panel::right("hist")
                .exact_size(315.0)
                .show(ui, |_ui| {});
            egui::CentralPanel::default().show(ui, |ui| {
                center = ui.max_rect();
                app.draw_spot_tool_options(ui);
            });
        },
    );
    output.textures_delta.clear();
    assert!(center.is_positive(), "central panel must be laid out");
    // Each row paints `label <slider> value`; the value label is the rightmost
    // element, so asserting BOTH ends inside the unclipped panel proves the
    // whole control (label, slider track, value) fits — a re-clip fails here.
    for (label, value) in [("Size", "18 px"), ("Feather", "0.50"), ("Opacity", "1.00")] {
        for needle in [label, value] {
            let (rect, clip) = text_shapes_for(&output.shapes, needle)
                .first()
                .copied()
                .unwrap_or_else(|| panic!("{needle:?} must be painted"));
            assert!(
                rect.max.x <= center.max.x + 0.5,
                "{needle:?} {rect:?} must stay inside the center panel {center:?}"
            );
            assert!(
                clip.max.x + 0.5 >= rect.max.x,
                "{needle:?} {rect:?} must not be clipped (clip {clip:?})"
            );
        }
    }
}

#[test]
fn spot_tool_cursor_paints_at_live_size() {
    let (_directory, mut app, ctx) = dust_app();
    app.set_spot_tool(SpotTool::Heal);
    let pos = egui::Pos2::new(400.0, 300.0);
    app.set_spot_radius(10.0);
    // First pass registers the preview widget; the second has hover state.
    preview_pass(&mut app, &ctx, vec![egui::Event::PointerMoved(pos)], 0.5);
    let shapes = preview_pass(&mut app, &ctx, vec![egui::Event::PointerMoved(pos)], 0.55);
    let scale = app.preview_effective_scale;
    assert!(scale > 0.0, "the preview must have been laid out");
    let radii = circle_radii(&shapes);
    assert!(
        radii.iter().any(|r| (r - 10.0 * scale).abs() < 1.0),
        "cursor at size 10 must paint r≈{} got {radii:?}",
        10.0 * scale
    );
    app.set_spot_radius(40.0);
    preview_pass(&mut app, &ctx, vec![egui::Event::PointerMoved(pos)], 0.6);
    let shapes = preview_pass(&mut app, &ctx, vec![egui::Event::PointerMoved(pos)], 0.65);
    let radii = circle_radii(&shapes);
    assert!(
        radii.iter().any(|r| (r - 40.0 * scale).abs() < 1.0),
        "cursor must follow the live size, got {radii:?}"
    );
}
