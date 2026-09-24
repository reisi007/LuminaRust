//! R5-MASKVIS-25 headless contracts: mask-view gating, pins/full display,
//! focus-layout growth, and persistence/interaction preservation.

use super::*;

/// Paint the same top-level panel composition as the app frame, but without
/// constructing an eframe runtime. This lets the test observe the real
/// `preview_pane_rect` before and after the production panel toggle.
fn develop_layout(app: &mut LuminaApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    if !app.side_chrome_hidden() {
        egui::Panel::left("maskvis-left")
            .exact_size(180.0)
            .show(ui, |ui| {
                ui.label("folders");
            });
        egui::Panel::right("maskvis-controls")
            .exact_size(280.0)
            .show(ui, |ui| app.draw_develop_panel(ui));
    }
    egui::CentralPanel::default().show(ui, |ui| app.draw_preview_area(&ctx, ui));
}

fn preview_pane_after_layout(app: &mut LuminaApp) -> egui::Rect {
    let mut pane = None;
    headless_frame_sized(app, 720.0, |app, ui| {
        develop_layout(app, ui);
        pane = app.preview_pane_rect();
    });
    pane.expect("the central preview must be laid out")
}

#[test]
fn mask_overlay_requires_the_open_mask_view_and_selected_full_mode() {
    let mut app = new_app();
    app.load_bytes(png(), "mask-visibility.png").unwrap();
    let id = app.create_mask("Selected").unwrap();
    app.commit_brush_stroke(vec![BrushMark {
        x: 0.5,
        y: 0.5,
        radius: 0.2,
        sign: BrushMarkSign::Positive,
        softness: 0.25,
        flow: 0.75,
    }])
    .unwrap();
    app.set_pin_visibility(PinVisibility::Always);

    // The prompt exists, but a closed Masking view owns neither its matte nor
    // its mask pins. The dedicated spot-pin regression below covers the
    // independent global spot path.
    assert!(!app.mask_view_open());
    assert!(!app.mask_overlay_allowed());
    assert!(app.effective_overlay_prompt().is_none());
    assert!(app.visible_edit_pins().is_empty());

    app.set_section_open(SECTION_MASKING, true);
    assert!(app.mask_view_open());
    assert!(app.mask_overlay_allowed());
    assert!(app.effective_overlay_prompt().is_some());
    assert_eq!(app.visible_edit_pins().len(), 1);

    app.set_mask_overlay_mode(MaskOverlayMode::PinsOnly);
    assert!(!app.mask_overlay_allowed());
    assert!(app.effective_overlay_prompt().is_none());
    assert_eq!(
        app.visible_edit_pins().len(),
        1,
        "pins-only keeps the selected mask's clickable anchor"
    );

    app.set_mask_overlay_mode(MaskOverlayMode::SelectedFull);
    assert!(app.mask_overlay_allowed());
    app.set_overlay_mode(OverlayMode::Never);
    assert!(!app.mask_overlay_allowed());
    app.set_overlay_mode(OverlayMode::Always);
    assert_eq!(app.selected_mask_id(), Some(id.as_str()));
}

#[test]
fn closed_mask_view_keeps_global_spot_pins() {
    let mut app = new_app();
    app.load_bytes(png(), "mask-visibility-spot-pin.png")
        .unwrap();
    let mask_id = app.create_mask("Mask pin").unwrap();
    app.commit_brush_stroke(vec![BrushMark {
        x: 0.25,
        y: 0.5,
        radius: 0.1,
        sign: BrushMarkSign::Positive,
        softness: 0.0,
        flow: 1.0,
    }])
    .unwrap();
    app.commit_spot_heal(
        Point2 { x: 0.75, y: 0.4 },
        4.0,
        0.5,
        Point2 { x: 0.0, y: 0.0 },
        1.0,
    )
    .unwrap();
    app.set_pin_visibility(PinVisibility::Always);

    assert!(!app.mask_view_open());
    let closed_pins = app.visible_edit_pins();
    assert_eq!(closed_pins.len(), 1);
    assert_eq!(closed_pins[0].kind, EditPinKind::Spot);
    assert!(closed_pins[0].id.starts_with("spot:"));

    app.set_section_open(SECTION_MASKING, true);
    let open_pins = app.visible_edit_pins();
    assert_eq!(open_pins.len(), 2);
    assert!(open_pins
        .iter()
        .any(|pin| { pin.kind == EditPinKind::Mask && pin.id == format!("mask:{mask_id}") }));
}

#[test]
fn live_brush_visibility_obeys_eye_and_editorial_gates() {
    let mut app = new_app();
    app.load_bytes(png(), "mask-visibility-live-gates.png")
        .unwrap();
    let id = app.create_mask("Live gate").unwrap();
    app.commit_brush_stroke(vec![BrushMark {
        x: 0.4,
        y: 0.5,
        radius: 0.1,
        sign: BrushMarkSign::Positive,
        softness: 0.0,
        flow: 1.0,
    }])
    .unwrap();
    app.set_section_open(SECTION_MASKING, true);
    app.set_mask_tool(MaskTool::Brush);
    app.drawing = true;
    app.drag_start = Some(Point2 { x: 0.4, y: 0.5 });
    app.drag_current = Some(Point2 { x: 0.4, y: 0.5 });
    app.pending_brush_marks.push(BrushMark {
        x: 0.4,
        y: 0.5,
        radius: 0.1,
        sign: BrushMarkSign::Positive,
        softness: 0.0,
        flow: 1.0,
    });
    assert!(app.mask_overlay_allowed());
    assert!(app.effective_overlay_prompt().is_some());

    app.set_mask_visible(&id, false).unwrap();
    assert!(!app.mask_overlay_allowed());
    assert!(app.effective_overlay_prompt().is_none());
    app.set_mask_visible(&id, true).unwrap();

    app.set_show_mask_overlay(false);
    assert!(!app.mask_overlay_allowed());
    assert!(app.effective_overlay_prompt().is_none());
    app.set_show_mask_overlay(true);

    app.set_overlay_mode(OverlayMode::Never);
    assert!(!app.mask_overlay_allowed());
    assert!(app.effective_overlay_prompt().is_none());
}

#[cfg(feature = "gpu")]
#[test]
fn gpu_present_respects_all_mask_overlay_gates() {
    let mut app = new_app();
    app.load_bytes(png(), "mask-visibility-gpu-gates.png")
        .unwrap();
    let id = app.create_mask("GPU gate").unwrap();
    app.commit_brush_stroke(vec![BrushMark {
        x: 0.5,
        y: 0.5,
        radius: 0.1,
        sign: BrushMarkSign::Positive,
        softness: 0.0,
        flow: 1.0,
    }])
    .unwrap();
    app.set_section_open(SECTION_MASKING, true);
    app.render().unwrap();
    let virtual_copy_id = app.virtual_copy_id.clone();
    // The routing predicate is pure session/render-state logic; mark the
    // evaluated-plane flag directly so this test does not need an adapter.
    app.vram_mask_is_evaluated = false;
    assert!(!app.gpu_mask_overlay_is_selected());
    app.vram_mask_is_evaluated = true;
    assert!(app.gpu_mask_overlay_is_selected());

    // A missing evaluated prompt must not let a resident plane masquerade as
    // the selected mask. Restore it before checking the independent layer gate.
    let selected_prompt = app
        .document
        .as_ref()
        .and_then(|document| {
            document
                .virtual_copies
                .iter()
                .find(|copy| copy.id == virtual_copy_id)
        })
        .and_then(|copy| copy.mask_library.iter().find(|mask| mask.id == id))
        .and_then(|mask| mask.prompt.clone())
        .expect("selected mask prompt");
    if let Some(mask) = app
        .document
        .as_mut()
        .and_then(|document| {
            document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == virtual_copy_id)
        })
        .and_then(|copy| copy.mask_library.iter_mut().find(|mask| mask.id == id))
    {
        mask.prompt = None;
    }
    assert!(app.effective_overlay_prompt().is_none());
    assert!(!app.gpu_mask_overlay_is_selected());
    if let Some(mask) = app
        .document
        .as_mut()
        .and_then(|document| {
            document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == virtual_copy_id)
        })
        .and_then(|copy| copy.mask_library.iter_mut().find(|mask| mask.id == id))
    {
        mask.prompt = Some(selected_prompt);
    }

    // A stale plane for another layer is equally ineligible: GPU may composite
    // only the exact selected layer, never a combined/different mask set.
    let selected_layer_id = app.render_mask_layers[0].layer_id.clone();
    app.render_mask_layers[0].layer_id = "layer-not-selected".into();
    assert!(!app.gpu_mask_overlay_is_selected());
    app.render_mask_layers[0].layer_id = selected_layer_id;
    app.render_mask_layers.clear();
    assert!(!app.gpu_mask_overlay_is_selected());

    // The non-default tint is session state consumed by both present paths;
    // visibility gates still decide whether either path may composite it.
    app.set_overlay_color([17, 129, 231]);
    assert_eq!(app.overlay_color(), [17, 129, 231]);
    app.set_show_mask_overlay(false);
    assert!(!app.gpu_mask_overlay_is_selected());
    app.set_show_mask_overlay(true);
    app.set_overlay_mode(OverlayMode::Never);
    assert!(!app.gpu_mask_overlay_is_selected());
    app.set_overlay_mode(OverlayMode::Always);
    app.set_mask_visible(&id, false).unwrap();
    assert!(!app.gpu_mask_overlay_is_selected());
    app.set_mask_visible(&id, true).unwrap();

    // Gradient/radial live prompts are CPU-only; an old evaluated plane must
    // not swallow their live matte while the GPU texture is present.
    app.set_mask_tool(MaskTool::LinearGradient);
    app.drawing = true;
    app.drag_start = Some(Point2 { x: 0.2, y: 0.5 });
    app.drag_current = Some(Point2 { x: 0.8, y: 0.5 });
    assert!(!app.gpu_mask_overlay_is_selected());
}

#[test]
fn pins_only_keeps_all_mask_anchors_while_full_mode_uses_the_selection() {
    let mut app = new_app();
    app.load_bytes(png(), "mask-visibility-all-pins.png")
        .unwrap();
    let first = app.create_mask("First").unwrap();
    app.commit_brush_stroke(vec![BrushMark {
        x: 0.25,
        y: 0.5,
        radius: 0.1,
        sign: BrushMarkSign::Positive,
        softness: 0.0,
        flow: 1.0,
    }])
    .unwrap();
    let second = app.create_mask("Second").unwrap();
    app.commit_brush_stroke(vec![BrushMark {
        x: 0.75,
        y: 0.5,
        radius: 0.1,
        sign: BrushMarkSign::Positive,
        softness: 0.0,
        flow: 1.0,
    }])
    .unwrap();
    app.set_section_open(SECTION_MASKING, true);
    app.set_pin_visibility(PinVisibility::Always);

    app.set_mask_overlay_mode(MaskOverlayMode::PinsOnly);
    let pins = app.visible_edit_pins();
    assert_eq!(pins.len(), 2);
    assert!(pins.iter().any(|pin| pin.id == format!("mask:{first}")));
    assert!(pins.iter().any(|pin| pin.id == format!("mask:{second}")));
    assert!(!app.mask_overlay_allowed());

    app.set_mask_overlay_mode(MaskOverlayMode::SelectedFull);
    assert!(app.mask_overlay_allowed());
    let selected = app.selected_mask_id().unwrap().to_owned();
    assert_eq!(app.effective_overlay_prompt(), app.selected_mask_prompt());
    assert_eq!(app.visible_edit_pins().len(), 2);
    assert!(app
        .visible_edit_pins()
        .iter()
        .any(|pin| pin.id == format!("mask:{selected}") && pin.selected));
}

#[test]
fn mask_view_controls_are_real_clickable_toggles() {
    let mut app = new_app();
    app.load_bytes(png(), "mask-visibility-controls.png")
        .unwrap();
    app.create_mask("Controls").unwrap();
    app.set_section_open(SECTION_MASKING, true);

    headless_click_labels_sized(&mut app, 2400.0, &[Str::OverlayModeLabel.t()], |app, ui| {
        app.draw_masking(ui)
    });
    assert_eq!(app.mask_overlay_mode(), MaskOverlayMode::PinsOnly);

    headless_click_labels_sized(
        &mut app,
        2400.0,
        &[Str::ViewToolbarPanels.t()],
        |app, ui| app.draw_masking(ui),
    );
    assert!(app.panels_hidden());
    assert!(app.error().is_none());
}

#[test]
fn panel_toggle_enlarges_the_real_preview_pane_without_disarming_tools() {
    let mut app = new_app();
    app.load_bytes(png(), "mask-visibility-layout.png").unwrap();
    app.render().unwrap();
    app.set_module(Module::Develop);
    app.set_mask_tool(MaskTool::Brush);
    let recipe = app.recipe().clone();

    let normal = preview_pane_after_layout(&mut app);
    let normal_screen = app.preview_screen_rect.expect("normal preview screen rect");
    app.toggle_panels_hidden();
    let focused = preview_pane_after_layout(&mut app);
    let focused_screen = app
        .preview_screen_rect
        .expect("focused preview screen rect");

    assert!(app.panels_hidden());
    assert!(
        focused.width() > normal.width() + 100.0,
        "panel-hide must enlarge the preview pane: normal={normal:?}, focused={focused:?}"
    );
    assert!(
        focused_screen.width() > normal_screen.width() + 100.0,
        "panel-hide must enlarge the painted image rect: normal={normal_screen:?}, focused={focused_screen:?}"
    );
    assert!(
        focused.height() >= normal.height(),
        "plain Tab keeps the filmstrip and must not shrink the pane vertically"
    );
    assert_eq!(app.mask_tool, MaskTool::Brush);
    app.set_spot_tool(SpotTool::Heal);
    assert_eq!(app.spot_tool, SpotTool::Heal);
    assert_eq!(*app.recipe(), recipe);
    assert!(app.error().is_none());
}

#[test]
fn display_toggles_do_not_change_brush_or_spot_persistence() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("mask-visibility-roundtrip.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let id = app.create_mask("Persisted brush").unwrap();
    let expected_marks = vec![BrushMark {
        x: 0.4,
        y: 0.6,
        radius: 0.12,
        sign: BrushMarkSign::Positive,
        softness: 0.4,
        flow: 0.6,
    }];
    app.commit_brush_stroke(expected_marks.clone()).unwrap();
    app.set_mask_overlay_mode(MaskOverlayMode::PinsOnly);
    app.toggle_panels_hidden();
    app.set_mask_tool(MaskTool::Brush);
    app.commit_spot_heal(
        Point2 { x: 0.2, y: 0.3 },
        4.0,
        0.5,
        Point2 { x: 0.3, y: 0.0 },
        1.0,
    )
    .unwrap();
    app.save_sidecar();

    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    reopened.select_mask(&id).unwrap();
    let persisted = reopened
        .document
        .as_ref()
        .unwrap()
        .virtual_copies
        .iter()
        .find(|copy| copy.id == reopened.virtual_copy_id)
        .unwrap()
        .mask_library
        .iter()
        .find(|mask| mask.id == id)
        .unwrap()
        .prompt
        .clone()
        .unwrap();
    assert!(matches!(
        persisted,
        MaskPrompt::Brush { ref marks, .. } if marks == &expected_marks
    ));
    assert_eq!(reopened.mask_overlay_mode(), MaskOverlayMode::SelectedFull);
    assert!(!reopened.panels_hidden());
    assert_eq!(reopened.spot_entries().len(), 1);
}
