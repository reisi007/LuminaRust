//! G-11 Edit-Pins and mask session state tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

#[test]
fn g11_overlay_modes_gate_the_draw_prompt() {
    // G-11 Tool-Overlay-Modi: Always shows the saved prompt without an
    // armed tool, Never hides it, Auto shows it only with an armed tool.
    // The mode setters are session-only: the recipe never changes.
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    assert_eq!(app.overlay_mode(), OverlayMode::Always);
    // No prompt yet: nothing to show in any mode.
    assert!(app.effective_overlay_prompt().is_none());
    let _mask_id = mask_with_box_prompt(&mut app, "m1", (0.2, 0.3, 0.4, 0.2));
    let recipe = app.recipe().clone();
    // Default Always: prompt visible without an armed tool.
    assert!(app.overlay_visible());
    assert!(app.effective_overlay_prompt().is_some());
    // Never: hidden even with a prompt and an armed tool.
    app.set_overlay_mode(OverlayMode::Never);
    assert_eq!(app.overlay_mode(), OverlayMode::Never);
    assert!(!app.overlay_visible());
    assert!(app.effective_overlay_prompt().is_none());
    app.set_mask_tool(MaskTool::Brush);
    assert!(app.effective_overlay_prompt().is_none());
    app.set_mask_tool(MaskTool::None);
    // Auto: hidden without a tool, visible with one.
    app.set_overlay_mode(OverlayMode::Auto);
    assert!(!app.overlay_visible());
    assert!(app.effective_overlay_prompt().is_none());
    app.set_mask_tool(MaskTool::LinearGradient);
    assert!(app.overlay_visible());
    assert!(app.effective_overlay_prompt().is_some());
    // The spot-heal tool also counts as an armed retouch tool.
    app.set_mask_tool(MaskTool::None);
    assert!(!app.overlay_visible());
    app.set_spot_tool(SpotTool::Heal);
    assert!(app.overlay_visible());
    app.set_spot_tool(SpotTool::None);
    assert!(!app.overlay_visible());
    // Session-only: mode switches never touched the recipe.
    assert_eq!(*app.recipe(), recipe);
    assert_eq!(app.status(), "Tool overlay: Auto");
}

#[test]
fn g11_pin_visibility_modes_cover_masks_and_spots() {
    // G-11 Edit-Pins: Always shows pins without an armed tool, Never shows
    // none, Auto only with an armed tool. Covers one mask pin (anchor from
    // the box geometry, selected flag) plus one spot pin.
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    assert_eq!(app.pin_visibility(), PinVisibility::Auto);
    let mask_id = mask_with_box_prompt(&mut app, "m1", (0.2, 0.3, 0.4, 0.2));
    app.commit_spot_heal(
        Point2 { x: 0.25, y: 0.5 },
        2.0,
        0.5,
        Point2 { x: 0.5, y: 0.0 },
        1.0,
    )
    .unwrap();
    let recipe = app.recipe().clone();
    // Default Auto without a tool: no pins.
    assert!(!app.pins_visible());
    assert!(app.visible_edit_pins().is_empty());
    // Always: both pins, no tool needed.
    app.set_pin_visibility(PinVisibility::Always);
    assert!(app.pins_visible());
    let pins = app.visible_edit_pins();
    assert_eq!(pins.len(), 2);
    assert_eq!(pins[0].id, format!("mask:{mask_id}"));
    assert_eq!(pins[0].label, "1");
    assert_eq!(pins[0].kind, EditPinKind::Mask);
    assert!((pins[0].pos.0 - 0.4).abs() < 1e-6);
    assert!((pins[0].pos.1 - 0.4).abs() < 1e-6);
    assert!(pins[0].selected);
    assert_eq!(pins[1].kind, EditPinKind::Spot);
    assert_eq!(pins[1].label, "2");
    assert!((pins[1].pos.0 - 0.25).abs() < 1e-6);
    assert!((pins[1].pos.1 - 0.5).abs() < 1e-6);
    // R5-DUST-23-FOLLOWUP: a fresh dab selects itself, so the spot pin
    // paints selected (like the selected mask pin above).
    assert!(pins[1].selected);
    // Never: no pins even with an armed tool.
    app.set_pin_visibility(PinVisibility::Never);
    app.set_mask_tool(MaskTool::Brush);
    assert!(!app.pins_visible());
    assert!(app.visible_edit_pins().is_empty());
    // Auto with an armed tool: both pins again.
    app.set_pin_visibility(PinVisibility::Auto);
    assert!(app.pins_visible());
    assert_eq!(app.visible_edit_pins().len(), 2);
    app.set_mask_tool(MaskTool::None);
    assert!(app.visible_edit_pins().is_empty());
    // Session-only: visibility switches never touched the recipe.
    assert_eq!(*app.recipe(), recipe);
}

#[test]
fn g11_pin_anchor_covers_all_prompt_variants() {
    // G-11: every prompt variant maps to its documented anchor; prompts
    // without geometry yield no pin instead of an invented position.
    let anchor = |prompt: &MaskPrompt| pin_anchor_for_prompt(prompt);
    let boxed = MaskPrompt::Box {
        rect: NormalizedRect {
            x: 0.2,
            y: 0.3,
            width: 0.4,
            height: 0.2,
        },
        transformation: PromptTransform::default(),
    };
    assert_eq!(anchor(&boxed), Some((0.4, 0.4)));
    let brush = MaskPrompt::Brush {
        marks: vec![BrushMark {
            x: 0.1,
            y: 0.9,
            radius: 0.05,
            sign: BrushMarkSign::Positive,
            softness: 0.0,
            flow: 1.0,
        }],
        resolution: (8, 8),
        transformation: PromptTransform::default(),
    };
    assert_eq!(anchor(&brush), Some((0.1, 0.9)));
    let empty_brush = MaskPrompt::Brush {
        marks: Vec::new(),
        resolution: (8, 8),
        transformation: PromptTransform::default(),
    };
    assert_eq!(anchor(&empty_brush), None);
    let polygon = MaskPrompt::Polygon {
        points: vec![Point2 { x: 0.7, y: 0.1 }, Point2 { x: 0.8, y: 0.2 }],
        transformation: PromptTransform::default(),
    };
    assert_eq!(anchor(&polygon), Some((0.7, 0.1)));
    let empty_polygon = MaskPrompt::Polygon {
        points: Vec::new(),
        transformation: PromptTransform::default(),
    };
    assert_eq!(anchor(&empty_polygon), None);
    let ellipse = MaskPrompt::Ellipse {
        center: Point2 { x: 0.6, y: 0.6 },
        radii: Point2 { x: 0.1, y: 0.2 },
        transformation: PromptTransform::default(),
    };
    assert_eq!(anchor(&ellipse), Some((0.6, 0.6)));
    // Gradient 0° from 0..=1: midpoint of the stretch is the frame centre.
    let gradient = MaskPrompt::Gradient {
        angle_deg: 0.0,
        start: 0.0,
        end: 1.0,
        transformation: PromptTransform::default(),
    };
    assert_eq!(anchor(&gradient), Some((0.5, 0.5)));
    // Gradient 0° from 0.5..=1: midpoint shifts right by a quarter.
    let gradient_half = MaskPrompt::Gradient {
        angle_deg: 0.0,
        start: 0.5,
        end: 1.0,
        transformation: PromptTransform::default(),
    };
    assert_eq!(anchor(&gradient_half), Some((0.75, 0.5)));
    // Non-finite geometry yields no pin.
    let nan_box = MaskPrompt::Box {
        rect: NormalizedRect {
            x: f32::NAN,
            y: 0.0,
            width: 0.1,
            height: 0.1,
        },
        transformation: PromptTransform::default(),
    };
    assert_eq!(anchor(&nan_box), None);
}

#[test]
fn g11_solo_mode_keeps_a_single_open_section() {
    // G-11 Solo-Mode: opening a section closes the others; enabling with
    // several open keeps the first; disabling restores independence.
    // Out-of-range indices are refused without a state change.
    let mut app = new_app();
    assert!(!app.solo_mode());
    assert_eq!(SECTION_COUNT, 8);
    assert_eq!(section_name(SECTION_BASIC), Some("Basic"));
    assert_eq!(section_name(SECTION_MASKING), Some("Masking"));
    assert_eq!(section_name(SECTION_COUNT), None);
    // Independent without solo.
    app.set_section_open(SECTION_BASIC, true);
    app.set_section_open(SECTION_COLOR, true);
    assert!(app.is_section_open(SECTION_BASIC));
    assert!(app.is_section_open(SECTION_COLOR));
    // Enabling keeps the first open section only.
    app.set_solo_mode(true);
    assert!(app.solo_mode());
    assert!(app.is_section_open(SECTION_BASIC));
    assert!(!app.is_section_open(SECTION_COLOR));
    // Opening another one closes the first.
    app.set_section_open(SECTION_DETAIL, true);
    assert!(app.is_section_open(SECTION_DETAIL));
    assert!(!app.is_section_open(SECTION_BASIC));
    // Closing keeps the rest closed (never re-opens).
    app.set_section_open(SECTION_DETAIL, false);
    assert!(!app.is_section_open(SECTION_DETAIL));
    // Disabled again: sections stay independent.
    app.set_solo_mode(false);
    app.set_section_open(SECTION_BASIC, true);
    app.set_section_open(SECTION_DETAIL, true);
    assert!(app.is_section_open(SECTION_BASIC));
    assert!(app.is_section_open(SECTION_DETAIL));
    // Out-of-range: refused, nothing changes.
    app.set_section_open(SECTION_COUNT, true);
    assert!(!app.is_section_open(SECTION_COUNT));
    assert!(app.is_section_open(SECTION_BASIC));
    // Session-only: no recipe involvement by construction (display state).
    assert_eq!(app.status(), "Solo mode off");
}

#[test]
fn g11_shift_tab_mapping_toggle_and_no_collision() {
    // G-11 `Shift+Tab`: pure mapping, toggle effect (side panels +
    // navigator + filmstrip hide, header stays) and no collision with the
    // existing Shift-combos (`Shift+M` radial, `Shift+Y` split,
    // `Shift+C/V/I/E` clipboard/import/export use other keys).
    assert!(all_panels_toggle_for_key(egui::Key::Tab, true));
    assert!(!all_panels_toggle_for_key(egui::Key::Tab, false));
    assert!(!all_panels_toggle_for_key(egui::Key::R, true));
    assert!(!all_panels_toggle_for_key(egui::Key::F, false));
    assert!(!all_panels_toggle_for_key(egui::Key::M, true));
    // Plain `Tab` mapping is untouched (disambiguation happens in update).
    assert_eq!(
        panel_toggle_for_key(egui::Key::Tab),
        Some(PanelToggle::PanelsHidden)
    );
    // Existing Shift-combos are unaffected (other keys).
    assert_eq!(
        mask_tool_for_key(egui::Key::M, true),
        Some(MaskTool::Radial)
    );
    assert_eq!(
        mask_tool_for_key(egui::Key::M, false),
        Some(MaskTool::LinearGradient)
    );
    assert_eq!(
        clipboard_action_for_key(egui::Key::C, true, true),
        Some(ClipboardAction::Copy)
    );
    assert_eq!(
        import_export_for_key(egui::Key::E, true, true),
        Some(ImportExportAction::Export)
    );
    // Effect: all-panels-hide covers filmstrip + side chrome, while plain
    // `Tab` keeps the filmstrip.
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    let recipe = app.recipe().clone();
    assert!(app.shows_filmstrip());
    assert!(!app.side_chrome_hidden());
    app.toggle_all_panels_hidden();
    assert!(app.all_panels_hidden());
    assert!(!app.shows_filmstrip());
    assert!(app.side_chrome_hidden());
    // Plain Tab state stayed off: the two hides are independent.
    assert!(!app.panels_hidden);
    assert!(!app.chrome_hidden());
    app.toggle_all_panels_hidden();
    assert!(!app.all_panels_hidden());
    assert!(app.shows_filmstrip());
    assert!(!app.side_chrome_hidden());
    app.toggle_panels_hidden();
    assert!(app.panels_hidden);
    assert!(app.shows_filmstrip(), "plain Tab keeps the filmstrip");
    assert_eq!(*app.recipe(), recipe);
}

#[test]
fn g11_session_state_survives_no_sidecar_roundtrip() {
    // G-11 E2E-Anker (DoD §1): mode switches are visible session state,
    // the mask prompt persists through save/reload, and the modes reset
    // to defaults on reopen (never sidecar keys).
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let _mask_id = mask_with_box_prompt(&mut app, "m1", (0.2, 0.3, 0.4, 0.2));
    app.set_overlay_mode(OverlayMode::Never);
    app.set_pin_visibility(PinVisibility::Never);
    app.set_solo_mode(true);
    app.set_section_open(SECTION_COLOR, true);
    app.toggle_all_panels_hidden();
    assert!(app.effective_overlay_prompt().is_none());
    assert!(app.visible_edit_pins().is_empty());
    // Persist the document (mask prompt rides along as recipe data).
    app.save_sidecar();
    let sidecar_path = lumina_sidecar::sidecar_path_for(&source);
    assert!(sidecar_path.exists());
    let raw = std::fs::read_to_string(&sidecar_path).unwrap();
    for key in [
        "overlay_mode",
        "pin_visibility",
        "solo_mode",
        "all_panels_hidden",
        "section_open",
    ] {
        assert!(!raw.contains(key), "session-only key leaked: {key}");
    }
    // Reload: the prompt survived, the session modes reset to defaults.
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert_eq!(reopened.overlay_mode(), OverlayMode::Always);
    assert_eq!(reopened.pin_visibility(), PinVisibility::Auto);
    assert!(!reopened.solo_mode());
    assert!(!reopened.all_panels_hidden());
    assert!(!reopened.is_section_open(SECTION_COLOR));
    let pins = {
        reopened.set_pin_visibility(PinVisibility::Always);
        reopened.visible_edit_pins()
    };
    assert_eq!(pins.len(), 1, "saved mask prompt reopens with its pin");
    assert_eq!(pins[0].kind, EditPinKind::Mask);
}
