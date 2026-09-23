//! R5-DUST-23-FOLLOWUP UI/widget tests: pin-click selection (instead of
//! dabbing) and detail-only-when-selected panel painting (SOLL
//! § R5-DUST-23-FOLLOWUP). Shared model fixtures live in
//! `super::spot_followup`.

use super::spot_followup::{spot_ids, two_spot_app};
use super::*;

/// 16×16 fixture: light background with a 2×2 dark dust block at the center
/// (mirrors the `spot_tool` dust fixture so pin-click geometry matches).
fn followup_dust_png() -> Vec<u8> {
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

/// The dust fixture opened on disk (async decode drained) with a preview
/// texture registered, like the `spot_tool` widget tests.
fn followup_dust_app() -> (tempfile::TempDir, LuminaApp, egui::Context) {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("dust.png");
    std::fs::write(&source, followup_dust_png()).unwrap();
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

/// One preview pass over the real widget with synthetic pointer events.
fn followup_preview_pass(
    app: &mut LuminaApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    time: f64,
) {
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
}

fn followup_click(app: &mut LuminaApp, ctx: &egui::Context, pos: egui::Pos2, time: f64) {
    let button = |pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    followup_preview_pass(app, ctx, vec![egui::Event::PointerMoved(pos)], time);
    followup_preview_pass(
        app,
        ctx,
        vec![egui::Event::PointerMoved(pos), button(true)],
        time + 0.05,
    );
    followup_preview_pass(app, ctx, vec![button(false)], time + 0.10);
}

/// Paint the spot tool options with "Remove options" opened on a tall canvas
/// and return the settled shapes (mirrors the G-04 panel harness).
fn followup_options_shapes(app: &mut LuminaApp) -> Vec<egui::epaint::ClippedShape> {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 4096.0));
    let mut time = 0.0_f64;
    let mut run = |events: Vec<egui::Event>| {
        time += 1.0 / 60.0;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| app.draw_spot_tool_options(ui),
        );
        output.textures_delta.clear();
        output.shapes
    };
    let mut shapes = run(vec![]);
    let pos = text_shapes_for(&shapes, "Remove options")
        .into_iter()
        .next()
        .expect("Remove options header must be painted")
        .0
        .center();
    let click = |pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    run(vec![egui::Event::PointerMoved(pos), click(true)]);
    run(vec![egui::Event::PointerMoved(pos), click(false)]);
    for _ in 0..30 {
        shapes = run(vec![]);
    }
    shapes
}

#[test]
fn followup_pin_click_selects_instead_of_dabbing() {
    // Widget-level: a click on the existing pin selects (spots stay one);
    // a click far away dabs (spots grow to two).
    let (_directory, mut app, ctx) = followup_dust_app();
    app.set_spot_tool(SpotTool::Heal);
    app.set_spot_radius(2.0);
    followup_click(&mut app, &ctx, egui::Pos2::new(400.0, 300.0), 0.5);
    assert_eq!(app.spot_entries().len(), 1);
    let id = spot_ids(&app)[0].clone();
    // A fresh dab selects itself; drop the selection so the click selects.
    app.selected_spot_id = None;
    followup_click(&mut app, &ctx, egui::Pos2::new(400.0, 300.0), 1.0);
    assert_eq!(
        app.spot_entries().len(),
        1,
        "pin click must select, not dab"
    );
    assert_eq!(app.selected_spot_id(), Some(id.as_str()));
    // Far corner of the preview rect maps far from the (0.5, 0.5) spot.
    followup_click(&mut app, &ctx, egui::Pos2::new(110.0, 110.0), 2.0);
    assert_eq!(app.spot_entries().len(), 2, "far click must dab");
}

#[test]
fn followup_panel_shows_detail_only_when_selected() {
    // Without a selection the list rows paint but no detail; with one the
    // type/status/parameter editor paints for exactly that removal.
    let (_directory, mut app, _path) = two_spot_app();
    let ids = spot_ids(&app);
    app.selected_spot_id = None;
    let texts = painted_texts(&followup_options_shapes(&mut app));
    assert!(
        texts.iter().any(|t| t == "Select"),
        "rows must offer Select, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.starts_with("Type:")),
        "no detail without a selection, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t == "Apply spot edits"),
        "no editor without a selection, got {texts:?}"
    );
    // Select the first removal: its detail paints, with type + status.
    app.select_spot(&ids[0]).unwrap();
    let texts = painted_texts(&followup_options_shapes(&mut app));
    for needle in [
        "Select",
        "Type: Heal",
        "Status: valid",
        "Apply spot edits",
        "Delete spot",
    ] {
        assert!(
            texts.iter().any(|t| t == needle),
            "{needle:?} must be painted with a selection, got {texts:?}"
        );
    }
    assert!(
        texts.iter().any(|t| t.starts_with("Target: ")),
        "regenerate target must name the selection, got {texts:?}"
    );
}

#[test]
fn followup_selected_controls_are_visible_in_real_1024x720_preview_layout() {
    let (_directory, mut app, _path) = two_spot_app();
    let id = spot_ids(&app)[0].clone();
    app.set_spot_tool(SpotTool::Heal);
    app.select_spot(&id).unwrap();
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 720.0));
    let draw_ctx = ctx.clone();
    let mut central = egui::Rect::NOTHING;
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
            egui::Panel::right("histogram")
                .exact_size(315.0)
                .show(ui, |_ui| {});
            egui::CentralPanel::default().show(ui, |ui| {
                central = ui.max_rect();
                app.draw_preview_area(&draw_ctx, ui);
            });
        },
    );
    output.textures_delta.clear();
    assert!(central.is_positive());
    for label in ["Regenerate variant", "Apply spot edits", "Delete spot"] {
        let (rect, clip) = text_shapes_for(&output.shapes, label)
            .first()
            .copied()
            .unwrap_or_else(|| panic!("{label:?} must be painted in the real preview layout"));
        assert!(
            rect.min.y >= central.min.y - 0.5 && rect.max.y <= central.max.y + 0.5,
            "{label:?} {rect:?} must remain inside the 1024x720 central pane {central:?}"
        );
        assert!(
            clip.max.y + 0.5 >= rect.max.y,
            "{label:?} must not be clipped"
        );
    }
}

#[test]
fn followup_long_ids_do_not_clip_row_buttons() {
    // R5-DUST-23 B1 lesson: a full `spot-<64 hex>` id in a row label pushed
    // the row buttons out of the center panel (clipped, unclickable). Rows
    // show a short id, so Select/Regenerate/Apply/Delete stay painted and
    // unclipped at the 1024x720 reference viewport with a committed spot.
    let (_directory, mut app, _path) = two_spot_app();
    let ids = spot_ids(&app);
    assert!(ids[0].len() > 12, "fixture ids must be long, got {ids:?}");
    app.select_spot(&ids[0]).unwrap();
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 720.0));
    let mut center = egui::Rect::NOTHING;
    let mut time = 0.0_f64;
    let mut run = |events: Vec<egui::Event>| {
        time += 1.0 / 60.0;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(time),
                events,
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
        output.shapes
    };
    let mut shapes = run(vec![]);
    let pos = text_shapes_for(&shapes, "Remove options")
        .into_iter()
        .next()
        .expect("Remove options header must be painted")
        .0
        .center();
    let click = |pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    run(vec![egui::Event::PointerMoved(pos), click(true)]);
    run(vec![egui::Event::PointerMoved(pos), click(false)]);
    for _ in 0..30 {
        shapes = run(vec![]);
    }
    assert!(center.is_positive(), "central panel must be laid out");
    for needle in [
        "Select",
        "Regenerate variant",
        "Apply spot edits",
        "Delete spot",
    ] {
        let (rect, clip) = text_shapes_for(&shapes, needle)
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
