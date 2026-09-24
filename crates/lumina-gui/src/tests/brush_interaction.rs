//! R5-BRUSH-24 real egui input and preview-interaction coverage.
//!
//! These tests deliberately drive `egui::Context` input and the production
//! preview widget. Pure brush predicates remain useful, but they cannot prove
//! that keyboard events reach the armed handler, that the cursor is painted at
//! the live radius, or that a pointer click selects a pin without creating a
//! second dab.

use super::*;

fn pointer_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
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
    // No renderer consumes texture deltas in this CPU/headless test.
    output.textures_delta.clear();
    output.shapes
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

fn install_preview_texture(app: &mut LuminaApp, ctx: &egui::Context) {
    let frame = app.original.as_ref().expect("source frame");
    app.texture = Some(ctx.load_texture(
        "brush-test-preview",
        egui::ColorImage::filled(
            [frame.width as usize, frame.height as usize],
            egui::Color32::BLACK,
        ),
        egui::TextureOptions::LINEAR,
    ));
}

fn key_event(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }
}

fn press_key(app: &mut LuminaApp, ctx: &egui::Context, key: egui::Key) {
    let mut output = ctx.run_ui(
        egui::RawInput {
            events: vec![key_event(key)],
            ..Default::default()
        },
        // This is the same handler called by LuminaApp's eframe update path.
        |ui| app.handle_brush_size_shortcuts(ui.ctx()),
    );
    output.textures_delta.clear();
}

fn prompt_marks(app: &LuminaApp, mask_id: &str) -> Vec<BrushMark> {
    let copy = app
        .document
        .as_ref()
        .unwrap()
        .virtual_copies
        .iter()
        .find(|copy| copy.id == app.virtual_copy_id)
        .unwrap();
    let prompt = copy
        .mask_library
        .iter()
        .find(|mask| mask.id == mask_id)
        .unwrap()
        .prompt
        .as_ref()
        .unwrap();
    match prompt {
        MaskPrompt::Brush { marks, .. } => marks.clone(),
        other => panic!("expected brush prompt, got {other:?}"),
    }
}

fn dab(x: f32, sign: BrushMarkSign, flow: f32) -> BrushMark {
    BrushMark {
        x,
        y: 0.5,
        radius: 0.25,
        sign,
        softness: 0.0,
        flow,
    }
}

#[test]
fn brush_size_shortcuts_use_the_real_key_handler_and_respect_arm() {
    let mut app = new_app();
    let ctx = egui::Context::default();
    app.set_brush_radius(0.2).unwrap();

    // A disarmed brush must not consume the same physical key event.
    press_key(&mut app, &ctx, egui::Key::CloseBracket);
    assert_eq!(app.brush_size(), 0.2);

    app.set_mask_tool(MaskTool::Brush);
    press_key(&mut app, &ctx, egui::Key::CloseBracket);
    assert!(
        (app.brush_size() - 0.2 * 1.1).abs() < 1e-6,
        "the armed handler must apply the documented ] factor"
    );
    press_key(&mut app, &ctx, egui::Key::OpenBracket);
    assert!(
        (app.brush_size() - 0.2).abs() < 1e-6,
        "the armed handler must apply the documented [ factor"
    );

    // Both validated limits clamp rather than producing an invalid radius.
    app.set_brush_radius(1.0).unwrap();
    press_key(&mut app, &ctx, egui::Key::CloseBracket);
    assert_eq!(app.brush_size(), 1.0);
    app.set_brush_radius(0.005).unwrap();
    press_key(&mut app, &ctx, egui::Key::OpenBracket);
    assert_eq!(app.brush_size(), 0.005);

    // A focused text widget owns the keyboard; the real handler must not
    // resize the brush while the user is typing.
    let mut text = String::new();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 480.0),
            )),
            ..Default::default()
        },
        |ui| {
            let response = ui.text_edit_singleline(&mut text);
            ui.ctx()
                .memory_mut(|memory| memory.request_focus(response.id));
        },
    );
    output.textures_delta.clear();
    assert!(ctx.egui_wants_keyboard_input());
    press_key(&mut app, &ctx, egui::Key::CloseBracket);
    assert_eq!(app.brush_size(), 0.005);
}

#[test]
fn brush_softness_and_flow_setters_validate_and_preserve_previous_values() {
    let mut app = new_app();
    assert_eq!(app.brush_softness(), 0.0);
    assert_eq!(app.brush_flow(), 1.0);

    for invalid in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
        assert!(app.set_brush_softness(invalid).is_err());
        assert_eq!(app.brush_softness(), 0.0, "invalid softness changed state");
        assert!(app.set_brush_flow(invalid).is_err());
        assert_eq!(app.brush_flow(), 1.0, "invalid flow changed state");
    }

    app.set_brush_softness(0.35).unwrap();
    app.set_brush_flow(0.65).unwrap();
    assert_eq!(app.brush_softness(), 0.35);
    assert_eq!(app.brush_flow(), 0.65);

    // The GUI setters feed the same per-dab snapshot used by the pointer path.
    let mark = app.brush_mark_at(0.5, 0.5);
    assert_eq!(mark.softness, 0.35);
    assert_eq!(mark.flow, 0.65);
}

#[test]
fn cpu_live_brush_overlay_combines_committed_and_pending_marks() {
    let mut app = new_app();
    app.load_bytes(png(), "brush-live-overlay.png").unwrap();
    let id = app.create_mask("Overlay").unwrap();
    app.commit_brush_stroke(vec![dab(0.25, BrushMarkSign::Positive, 1.0)])
        .unwrap();

    app.set_mask_tool(MaskTool::Brush);
    app.drawing = true;
    app.drag_start = Some(Point2 { x: 0.25, y: 0.5 });
    app.drag_current = Some(Point2 { x: 0.25, y: 0.5 });
    app.pending_brush_marks
        .push(dab(0.25, BrushMarkSign::Negative, 0.25));

    let prompt = app
        .effective_overlay_prompt()
        .expect("the live brush overlay prompt");
    let marks = match &prompt {
        MaskPrompt::Brush { marks, .. } => marks,
        other => panic!("expected brush prompt, got {other:?}"),
    };
    assert_eq!(
        marks.len(),
        2,
        "pending marks must append to the saved stroke"
    );
    let plane = rasterize_prompt(&prompt, 2, 1).unwrap();
    assert!(
        plane.values[0] > 0 && plane.values[0] < u16::MAX,
        "the committed positive must feed the pending negative: {:?}",
        plane.values
    );
    assert_eq!(
        plane.values[1], 0,
        "the second dab must not affect the other pixel"
    );
    assert_eq!(prompt_marks(&app, &id).len(), 1, "preview must not commit");
}

#[cfg(feature = "gpu")]
#[test]
fn brush_commit_rebuilds_same_scope_plane_and_keeps_strokes_cumulative() {
    let mut app = new_app();
    app.load_bytes(png(), "brush-plane-rebuild.png").unwrap();
    app.create_mask("Cumulative").unwrap();
    let first = dab(0.25, BrushMarkSign::Positive, 1.0);
    let second = dab(0.75, BrushMarkSign::Positive, 1.0);

    app.commit_brush_stroke(vec![first]).unwrap();
    let (_, rebuilt) = app.stamp_live_brush_mark(first).unwrap();
    assert!(rebuilt);
    assert_eq!(app.brush_mask_plane.as_deref(), Some(&[u16::MAX, 0][..]));
    let scope = app.brush_mask_plane_scope.clone().unwrap();
    let dimensions = app.brush_mask_plane_dims.unwrap();

    app.commit_brush_stroke(vec![second]).unwrap();
    assert!(
        app.brush_mask_plane.is_none(),
        "same-scope commit retained pixels"
    );
    assert!(app.brush_mask_plane_scope.is_none());
    assert!(app.brush_mask_plane_dims.is_none());

    let (_, rebuilt) = app.stamp_live_brush_mark(second).unwrap();
    assert!(rebuilt, "the next dab must rebuild the committed prompt");
    assert_eq!(app.brush_mask_plane_scope.as_ref(), Some(&scope));
    assert_eq!(app.brush_mask_plane_dims, Some(dimensions));
    assert_eq!(
        app.brush_mask_plane.as_deref(),
        Some(&[u16::MAX, u16::MAX][..]),
        "the rebuilt plane must include both committed strokes"
    );
}

#[test]
fn brush_cursor_paints_at_live_radius_and_is_gated_by_a_competing_picker() {
    let mut app = new_app();
    app.load_bytes(png(), "brush-cursor.png").unwrap();
    app.set_brush_radius(0.2).unwrap();
    app.set_mask_tool(MaskTool::Brush);
    let ctx = egui::Context::default();
    app.render().unwrap();
    install_preview_texture(&mut app, &ctx);
    let pos = egui::Pos2::new(400.0, 300.0);
    preview_pass(&mut app, &ctx, vec![egui::Event::PointerMoved(pos)], 0.5);
    let scale = app.preview_effective_scale;
    let expected = app.brush_cursor_radius(scale);
    assert!(expected > 2.0, "fixture must produce a visible live radius");
    let shapes = preview_pass(&mut app, &ctx, vec![egui::Event::PointerMoved(pos)], 0.55);
    let radii = circle_radii(&shapes);
    assert!(
        radii.iter().any(|radius| (radius - expected).abs() < 0.5),
        "expected live brush circle r={expected}, got {radii:?}"
    );

    // Keep Brush selected but arm the competing red-eye gate: the production
    // preview handler must suppress the circle before it reaches the painter.
    app.red_eye_pick_mode = true;
    let shapes = preview_pass(&mut app, &ctx, vec![egui::Event::PointerMoved(pos)], 0.6);
    assert!(
        !circle_radii(&shapes)
            .iter()
            .any(|radius| (radius - expected).abs() < 0.5),
        "competing picker must gate the brush cursor"
    );
}

#[test]
fn preview_pointer_drag_commits_a_real_brush_mark_with_live_controls() {
    let mut app = new_app();
    app.load_bytes(png(), "brush-pointer.png").unwrap();
    let mask_id = app.create_mask("Pointer target").unwrap();
    app.set_brush_radius(0.2).unwrap();
    app.set_brush_softness(0.35).unwrap();
    app.set_brush_flow(0.65).unwrap();
    app.set_mask_tool(MaskTool::Brush);
    let ctx = egui::Context::default();
    app.render().unwrap();
    install_preview_texture(&mut app, &ctx);
    // Establish the real preview rect before sending pointer coordinates.
    preview_pass(&mut app, &ctx, vec![], 0.1);
    let rect = app
        .preview_screen_rect
        .expect("preview rect must be painted");
    let start = rect.center();
    let end = egui::pos2((start.x + 20.0).min(rect.max.x - 1.0), start.y);
    preview_pass(&mut app, &ctx, vec![egui::Event::PointerMoved(start)], 0.2);
    preview_pass(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(start),
            pointer_button(start, true),
        ],
        0.25,
    );
    preview_pass(&mut app, &ctx, vec![egui::Event::PointerMoved(end)], 0.30);
    preview_pass(&mut app, &ctx, vec![pointer_button(end, false)], 0.35);

    let marks = prompt_marks(&app, &mask_id);
    assert_eq!(
        marks.len(),
        1,
        "pointer release must finish one real stroke"
    );
    assert!((marks[0].radius - 0.2).abs() < 1e-6);
    assert!((marks[0].softness - 0.35).abs() < 1e-6);
    assert!((marks[0].flow - 0.65).abs() < 1e-6);
    assert!(!app.drawing);
    assert!(app.pending_brush_marks.is_empty());
    assert!(
        app.error().is_none(),
        "pointer stroke error: {:?}",
        app.error()
    );
}

#[test]
fn preview_pin_click_selects_the_pin_without_committing_a_new_dab() {
    let mut app = new_app();
    app.load_bytes(png(), "brush-pin-click.png").unwrap();
    let first = app.create_mask("First pin").unwrap();
    app.commit_brush_stroke(vec![BrushMark {
        x: 0.25,
        y: 0.5,
        radius: 0.1,
        sign: BrushMarkSign::Positive,
        softness: 0.2,
        flow: 0.7,
    }])
    .unwrap();
    let second = app.create_mask("Second pin").unwrap();
    app.commit_brush_stroke(vec![BrushMark {
        x: 0.75,
        y: 0.5,
        radius: 0.1,
        sign: BrushMarkSign::Positive,
        softness: 0.2,
        flow: 0.7,
    }])
    .unwrap();
    let first_before = prompt_marks(&app, &first);
    let second_before = prompt_marks(&app, &second);
    assert_eq!(app.selected_mask_id(), Some(second.as_str()));
    app.set_pin_visibility(PinVisibility::Always);
    app.set_mask_tool(MaskTool::Brush);
    let ctx = egui::Context::default();
    app.render().unwrap();
    install_preview_texture(&mut app, &ctx);
    preview_pass(&mut app, &ctx, vec![], 0.1);
    let rect = app
        .preview_screen_rect
        .expect("preview rect must be painted");
    let pin_pos = egui::pos2(
        rect.min.x + rect.width() * 0.25,
        rect.min.y + rect.height() * 0.5,
    );
    preview_pass(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(pin_pos)],
        0.2,
    );
    preview_pass(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(pin_pos),
            pointer_button(pin_pos, true),
        ],
        0.25,
    );
    preview_pass(&mut app, &ctx, vec![pointer_button(pin_pos, false)], 0.3);

    assert_eq!(app.selected_mask_id(), Some(first.as_str()));
    assert_eq!(prompt_marks(&app, &first), first_before);
    assert_eq!(prompt_marks(&app, &second), second_before);
    assert!(!app.drawing);
    assert!(app.pending_brush_marks.is_empty());
}
