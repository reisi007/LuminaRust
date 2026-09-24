//! Focused R5-BRUSH-24 model/lifecycle and management regressions.

use super::*;

fn mark(x: f32, y: f32, sign: BrushMarkSign) -> BrushMark {
    BrushMark {
        x,
        y,
        radius: 1.0,
        sign,
        softness: 0.0,
        flow: 1.0,
    }
}

fn prompt_for(app: &LuminaApp, mask_id: &str) -> MaskPrompt {
    app.document
        .as_ref()
        .unwrap()
        .virtual_copies
        .iter()
        .find(|copy| copy.id == app.virtual_copy_id)
        .unwrap()
        .mask_library
        .iter()
        .find(|mask| mask.id == mask_id)
        .unwrap()
        .prompt
        .clone()
        .unwrap()
}

fn brush_marks(prompt: &MaskPrompt) -> &[BrushMark] {
    match prompt {
        MaskPrompt::Brush { marks, .. } => marks,
        other => panic!("expected brush prompt, got {other:?}"),
    }
}

#[cfg(feature = "gpu")]
fn arm_stale_gpu_preview(app: &mut LuminaApp) {
    app.vram_fresh = true;
    app.vram_mask_is_evaluated = true;
    app.vram_render_refusal = Some("stale-test-frame".into());
}

#[cfg(feature = "gpu")]
fn assert_render_and_gpu_dirty(app: &LuminaApp, operation: &str) {
    assert!(app.render_key().is_none(), "{operation}: render key");
    assert!(app.pending_full_render, "{operation}: full render");
    assert!(!app.vram_fresh, "{operation}: VRAM frame");
    assert!(
        !app.vram_mask_is_evaluated,
        "{operation}: evaluated VRAM mask"
    );
    assert!(
        app.vram_render_refusal.is_none(),
        "{operation}: stale present refusal"
    );
}

#[cfg(feature = "gpu")]
#[test]
fn create_select_and_brush_commit_invalidate_rendered_and_gpu_preview() {
    let mut app = new_app();
    app.load_bytes(png(), "brush-invalidation.png").unwrap();
    app.render().unwrap();

    arm_stale_gpu_preview(&mut app);
    let id = app.create_mask("Subject").unwrap();
    assert_render_and_gpu_dirty(&app, "create/select");

    app.render().unwrap();
    arm_stale_gpu_preview(&mut app);
    app.select_mask(&id).unwrap();
    assert_render_and_gpu_dirty(&app, "select existing mask");

    app.render().unwrap();
    arm_stale_gpu_preview(&mut app);
    app.commit_brush_stroke(vec![mark(0.5, 0.5, BrushMarkSign::Positive)])
        .unwrap();
    assert_render_and_gpu_dirty(&app, "brush commit");
}

#[test]
fn second_brush_stroke_appends_positive_and_negative_marks() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("second-stroke.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let id = app.create_mask("Subject").unwrap();
    app.commit_brush_stroke(vec![mark(0.25, 0.5, BrushMarkSign::Positive)])
        .unwrap();
    app.commit_brush_stroke(vec![
        mark(0.5, 0.5, BrushMarkSign::Positive),
        mark(0.75, 0.5, BrushMarkSign::Negative),
    ])
    .unwrap();

    let prompt = prompt_for(&app, &id);
    let marks = brush_marks(&prompt);
    assert_eq!(marks.len(), 3, "second stroke must append, not replace");
    assert!(matches!(marks[0].sign, BrushMarkSign::Positive));
    assert!(matches!(marks[1].sign, BrushMarkSign::Positive));
    assert!(matches!(marks[2].sign, BrushMarkSign::Negative));

    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let persisted = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    let persisted_masks = brush_marks(
        persisted.virtual_copies[0]
            .mask_library
            .iter()
            .find(|mask| mask.id == id)
            .unwrap()
            .prompt
            .as_ref()
            .unwrap(),
    );
    assert_eq!(persisted_masks.len(), 3);
    assert!(matches!(persisted_masks[2].sign, BrushMarkSign::Negative));
    drop(app);
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    reopened.select_mask(&id).unwrap();
    assert_eq!(brush_marks(&prompt_for(&reopened, &id)), marks);
}

#[test]
fn strokes_and_layers_remain_separate_per_mask() {
    let mut app = new_app();
    app.load_bytes(png(), "mask-separation.png").unwrap();
    let first = app.create_mask("First").unwrap();
    app.commit_brush_stroke(vec![mark(0.25, 0.5, BrushMarkSign::Positive)])
        .unwrap();
    let second = app.create_mask("Second").unwrap();
    app.commit_brush_stroke(vec![mark(0.75, 0.5, BrushMarkSign::Negative)])
        .unwrap();
    let first_before = prompt_for(&app, &first);
    let second_before = prompt_for(&app, &second);

    app.select_mask(&first).unwrap();
    app.commit_brush_stroke(vec![mark(0.5, 0.5, BrushMarkSign::Negative)])
        .unwrap();
    assert_eq!(brush_marks(&prompt_for(&app, &first)).len(), 2);
    assert_eq!(prompt_for(&app, &second), second_before);
    assert_eq!(brush_marks(&prompt_for(&app, &second)).len(), 1);
    assert_ne!(first_before, prompt_for(&app, &first));

    let copy = app.document.as_ref().unwrap().virtual_copies[0].clone();
    let first_layer = copy
        .mask_layers
        .iter()
        .find(|layer| layer.mask.mask_id == first)
        .unwrap();
    let second_layer = copy
        .mask_layers
        .iter()
        .find(|layer| layer.mask.mask_id == second)
        .unwrap();
    assert_ne!(first_layer.id, second_layer.id);
}

#[test]
fn invert_uses_the_automatic_slider_save_path_and_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("invert-auto-save.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let id = app.create_mask("Subject").unwrap();
    app.set_mask_inverted(true).unwrap();
    assert_eq!(
        app.pending_slider_commit,
        Some(("mask.inverted".to_string(), 1.0))
    );

    // Drive the real scheduler, not save_sidecar directly. The first frame may
    // only settle the module-switch deferral; the next one performs the full
    // render and CAS sidecar commit.
    let ctx = egui::Context::default();
    app.schedule_render(&ctx);
    app.schedule_render(&ctx);
    assert_eq!(app.pending_slider_commit, None);
    assert!(
        app.error().is_none(),
        "invert save failed: {:?}",
        app.error()
    );
    let persisted = lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source))
        .unwrap()
        .virtual_copies[0]
        .mask_layers
        .iter()
        .find(|layer| layer.mask.mask_id == id)
        .unwrap()
        .inverted;
    assert!(persisted);

    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert!(reopened
        .selected_mask_layer()
        .is_some_and(|layer| layer.inverted));
}

#[test]
fn selected_mask_layer_never_falls_back_to_a_sibling() {
    let mut app = new_app();
    app.load_bytes(png(), "no-layer-fallback.png").unwrap();
    let first = app.create_mask("First").unwrap();
    let second = app.create_mask("Second").unwrap();
    {
        let copy = app.active_copy_mut().unwrap();
        copy.mask_layers
            .retain(|layer| layer.mask.mask_id != second);
    }
    app.selected_mask_id = Some(second.clone());

    assert!(app.set_mask_inverted(true).is_err());
    assert!(!app.document.as_ref().unwrap().virtual_copies[0]
        .mask_layers
        .iter()
        .find(|layer| layer.mask.mask_id == first)
        .is_some_and(|layer| layer.inverted));
}

#[test]
fn selecting_a_layerless_mask_materializes_and_reloads_it() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("layer-reload.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let first = app.create_mask("First").unwrap();
    let second = app.create_mask("Second").unwrap();

    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let mut document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    document.virtual_copies[0]
        .mask_layers
        .retain(|layer| layer.mask.mask_id != second);
    lumina_sidecar::save_sidecar(&sidecar, &document).unwrap();
    drop(app);

    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    reopened.select_mask(&second).unwrap();
    let persisted = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    assert!(persisted.virtual_copies[0]
        .mask_layers
        .iter()
        .any(|layer| layer.mask.mask_id == second));
    assert!(persisted.virtual_copies[0]
        .mask_layers
        .iter()
        .any(|layer| layer.mask.mask_id == first));
    drop(reopened);

    let mut reloaded = new_app();
    open_and_decode(&mut reloaded, source.display().to_string());
    reloaded.select_mask(&second).unwrap();
    assert_eq!(reloaded.selected_mask_layer().unwrap().mask.mask_id, second);
}

fn cursor_flags(app: &mut LuminaApp) -> (bool, bool) {
    let ctx = egui::Context::default();
    let mut flags = (false, false);
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 480.0),
            )),
            ..Default::default()
        },
        |ui| flags = (app.brush_cursor_allowed(ui), app.spot_cursor_allowed(ui)),
    );
    output.textures_delta.clear();
    flags
}

#[test]
fn brush_cursor_is_gated_by_keyboard_focus_and_competing_tools() {
    let mut app = new_app();
    app.load_bytes(png(), "cursor-gates.png").unwrap();
    app.set_mask_tool(MaskTool::Brush);
    assert_eq!(cursor_flags(&mut app), (true, false));

    let ctx = egui::Context::default();
    let mut focused_gates = (true, true);
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 480.0),
            )),
            ..Default::default()
        },
        |ui| {
            let mut text = String::new();
            let response = ui.text_edit_singleline(&mut text);
            ui.ctx()
                .memory_mut(|memory| memory.request_focus(response.id));
            focused_gates = (app.brush_cursor_allowed(ui), app.spot_cursor_allowed(ui));
        },
    );
    output.textures_delta.clear();
    assert!(ctx.egui_wants_keyboard_input());
    assert_eq!(focused_gates, (false, false));

    app.set_spot_tool(SpotTool::Heal);
    assert_eq!(app.mask_tool, MaskTool::None, "arming stays exclusive");
    assert_eq!(cursor_flags(&mut app), (false, true));
    app.set_mask_tool(MaskTool::Brush);
    app.toggle_crop_mode();
    assert_eq!(
        app.mask_tool,
        MaskTool::None,
        "crop keeps its arming behavior"
    );
    assert_eq!(app.spot_tool, SpotTool::None);
    assert_eq!(cursor_flags(&mut app), (false, false));
}

#[test]
fn brush_size_slider_paints_percent_while_storage_stays_normalized() {
    let mut app = new_app();
    app.load_bytes(png(), "brush-percent.png").unwrap();
    app.ensure_document_loaded().unwrap();
    app.set_mask_tool(MaskTool::Brush);
    app.set_section_open(SECTION_MASKING, true);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 2400.0));
    let mut shapes = Vec::new();
    for frame in 0..3 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(frame as f64 / 60.0),
                ..Default::default()
            },
            |ui| app.draw_masking(ui),
        );
        output.textures_delta.clear();
        shapes = output.shapes;
    }
    let texts = painted_texts(&shapes);
    assert!(
        texts.iter().any(|text| text == "5%" || text == "5.0%"),
        "brush size must paint actual percent, got {texts:?}"
    );
    assert_eq!(app.brush_size(), 0.05);
}

#[test]
fn arming_wb_red_eye_spot_or_brush_clears_competing_picker_state() {
    let mut app = new_app();
    app.load_bytes(png(), "exclusive-tools.png").unwrap();

    app.set_mask_tool(MaskTool::Brush);
    app.arm_wb_picker();
    assert!(app.wb_pick_mode);
    assert!(!app.red_eye_pick_mode);
    assert_eq!(app.mask_tool, MaskTool::None);
    assert_eq!(app.spot_tool, SpotTool::None);

    app.set_red_eye_pick_mode(true);
    assert!(app.red_eye_pick_mode && !app.wb_pick_mode);
    assert_eq!(app.mask_tool, MaskTool::None);
    assert_eq!(app.spot_tool, SpotTool::None);

    app.set_spot_tool(SpotTool::Heal);
    assert_eq!(app.spot_tool, SpotTool::Heal);
    assert_eq!(app.mask_tool, MaskTool::None);
    assert!(!app.wb_pick_mode && !app.red_eye_pick_mode);

    app.set_mask_tool(MaskTool::Brush);
    assert_eq!(app.mask_tool, MaskTool::Brush);
    assert_eq!(app.spot_tool, SpotTool::None);
    assert!(!app.wb_pick_mode && !app.red_eye_pick_mode);
}

#[test]
fn hidden_mask_pins_are_not_hit_testable() {
    let mut app = new_app();
    app.load_bytes(png(), "hidden-pin.png").unwrap();
    let id = app.create_mask("Pinned").unwrap();
    app.commit_brush_stroke(vec![mark(0.5, 0.5, BrushMarkSign::Positive)])
        .unwrap();
    app.set_section_open(SECTION_MASKING, true);

    app.set_pin_visibility(PinVisibility::Never);
    assert!(app.visible_edit_pins().is_empty());
    assert_eq!(app.mask_pin_hit_at(0.5, 0.5, 1.0), None);
    app.set_pin_visibility(PinVisibility::Auto);
    app.set_mask_tool(MaskTool::None);
    assert_eq!(app.mask_pin_hit_at(0.5, 0.5, 1.0), None);
    app.set_pin_visibility(PinVisibility::Always);
    assert_eq!(
        app.mask_pin_hit_at(0.5, 0.5, 1.0).as_deref(),
        Some(id.as_str())
    );
}

#[test]
fn overlapping_pins_are_ambiguous_and_a_single_pin_is_nearest_hit() {
    let mut app = new_app();
    app.load_bytes(png(), "pin-ambiguity.png").unwrap();
    let first = app.create_mask("First pin").unwrap();
    app.commit_brush_stroke(vec![mark(0.5, 0.5, BrushMarkSign::Positive)])
        .unwrap();
    let second = app.create_mask("Second pin").unwrap();
    app.commit_brush_stroke(vec![mark(0.5, 0.5, BrushMarkSign::Positive)])
        .unwrap();
    app.set_section_open(SECTION_MASKING, true);
    app.set_pin_visibility(PinVisibility::Always);
    assert_eq!(
        app.mask_pin_hit_at(0.5, 0.5, 100.0),
        None,
        "two pins in the hit tolerance must not choose by array order or distance"
    );

    let third = app.create_mask("Ordinary pin").unwrap();
    app.commit_brush_stroke(vec![mark(0.25, 0.5, BrushMarkSign::Positive)])
        .unwrap();
    assert_eq!(app.mask_pin_hit_at(0.25, 0.5, 100.0), Some(third));
    assert_ne!(app.mask_pin_hit_at(0.25, 0.5, 100.0), Some(first));
    assert_ne!(app.mask_pin_hit_at(0.25, 0.5, 100.0), Some(second));
}

#[test]
fn an_invisible_mask_pin_is_not_hit_testable() {
    let mut app = new_app();
    app.load_bytes(png(), "invisible-eye-pin.png").unwrap();
    let id = app.create_mask("Hidden eye").unwrap();
    app.commit_brush_stroke(vec![mark(0.5, 0.5, BrushMarkSign::Positive)])
        .unwrap();
    app.set_section_open(SECTION_MASKING, true);
    app.set_pin_visibility(PinVisibility::Always);
    app.set_mask_visible(&id, false).unwrap();
    assert!(!app.mask_visible(&id));
    assert_eq!(app.mask_pin_hit_at(0.5, 0.5, 1.0), None);
}

#[cfg(feature = "gpu")]
#[test]
fn live_plane_rebuilds_for_selected_mask_copy_and_source() {
    let mut app = new_app();
    app.load_bytes(png(), "plane-a.png").unwrap();
    let first = app.create_mask("First").unwrap();
    app.commit_brush_stroke(vec![mark(0.25, 0.5, BrushMarkSign::Positive)])
        .unwrap();
    let second = app.create_mask("Second").unwrap();
    app.commit_brush_stroke(vec![mark(0.75, 0.5, BrushMarkSign::Positive)])
        .unwrap();

    app.select_mask(&first).unwrap();
    let (tiles, rebuilt) = app
        .stamp_live_brush_mark(mark(0.25, 0.5, BrushMarkSign::Negative))
        .unwrap();
    assert!(rebuilt && !tiles.is_empty());
    assert_eq!(app.brush_mask_plane.as_ref().unwrap(), &vec![0u16; 2]);
    assert_eq!(
        app.brush_mask_plane_scope.as_ref().unwrap().1,
        app.virtual_copy_id
    );
    assert_eq!(app.brush_mask_plane_scope.as_ref().unwrap().2, first);

    app.select_mask(&second).unwrap();
    let (_, rebuilt) = app
        .stamp_live_brush_mark(mark(0.25, 0.5, BrushMarkSign::Positive))
        .unwrap();
    assert!(rebuilt);
    assert_eq!(app.brush_mask_plane.as_ref().unwrap(), &vec![u16::MAX; 2]);
    assert_eq!(app.brush_mask_plane_scope.as_ref().unwrap().2, second);

    let copy_id = app.duplicate_active_copy().unwrap();
    // A duplicated copy starts with cloned graph layers that still point at
    // the source copy; selection must ignore them, then materialize a local
    // layer explicitly before testing the live plane.
    app.select_mask(&second).unwrap();
    let (_, rebuilt) = app
        .stamp_live_brush_mark(mark(0.5, 0.5, BrushMarkSign::Positive))
        .unwrap();
    assert!(rebuilt);
    assert_eq!(app.brush_mask_plane_scope.as_ref().unwrap().1, copy_id);

    app.load_bytes(png(), "plane-b.png").unwrap();
    assert!(app.brush_mask_plane.is_none());
    assert!(app.brush_mask_plane_scope.is_none());
}
