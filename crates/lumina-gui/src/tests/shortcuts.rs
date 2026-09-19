//! module/shortcut key mappings tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

#[test]
fn module_for_key_maps_lightroom_module_shortcuts() {
    // `G` -> Library, `D` -> Develop, `E` -> Library (Loupe alias).
    assert_eq!(module_for_key(egui::Key::G), Some(Module::Library));
    assert_eq!(module_for_key(egui::Key::D), Some(Module::Develop));
    assert_eq!(module_for_key(egui::Key::E), Some(Module::Library));
    // Existing non-module shortcuts must not collide with the mapping.
    assert_eq!(module_for_key(egui::Key::Y), None);
    assert_eq!(module_for_key(egui::Key::Escape), None);
    // Arbitrary other keys resolve to no module change.
    assert_eq!(module_for_key(egui::Key::A), None);
}

#[test]
fn module_shortcut_switch_changes_module_without_mutating_recipe() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    app.active_module = Module::Library;
    app.set_adjustment("exposure", 0.75);
    let adjustments_before = app.recipe().adjustments.clone();

    // Simulate the `D` shortcut resolving to its target module.
    app.active_module = module_for_key(egui::Key::D).unwrap();

    // Module changed...
    assert_eq!(app.active_module, Module::Develop);
    // ...but the recipe (and therefore any sidecar state) is untouched.
    assert_eq!(app.recipe().adjustments, adjustments_before);
    assert_eq!(app.recipe().adjustments["exposure"], 0.75);
}

#[test]
fn rating_flag_mask_keys_map_lightroom_shortcuts() {
    // LR-01: `0` clears, `1`–`5` set the star rating.
    assert_eq!(rating_for_key(egui::Key::Num0), Some(0));
    assert_eq!(rating_for_key(egui::Key::Num1), Some(1));
    assert_eq!(rating_for_key(egui::Key::Num2), Some(2));
    assert_eq!(rating_for_key(egui::Key::Num3), Some(3));
    assert_eq!(rating_for_key(egui::Key::Num4), Some(4));
    assert_eq!(rating_for_key(egui::Key::Num5), Some(5));
    assert_eq!(rating_for_key(egui::Key::Num6), None);
    assert_eq!(rating_for_key(egui::Key::G), None);
    assert_eq!(rating_for_key(egui::Key::Y), None);
    // LR-01: `P` pick, `X` reject, `U` unflag.
    assert_eq!(flag_for_key(egui::Key::P), Some(Flag::Pick));
    assert_eq!(flag_for_key(egui::Key::X), Some(Flag::Reject));
    assert_eq!(flag_for_key(egui::Key::U), Some(Flag::Unflagged));
    assert_eq!(flag_for_key(egui::Key::G), None);
    assert_eq!(flag_for_key(egui::Key::Y), None);
    // LR-10: `K` brush, `M` linear, `Shift+M` radial.
    assert_eq!(
        mask_tool_for_key(egui::Key::K, false),
        Some(MaskTool::Brush)
    );
    assert_eq!(mask_tool_for_key(egui::Key::K, true), Some(MaskTool::Brush));
    assert_eq!(
        mask_tool_for_key(egui::Key::M, false),
        Some(MaskTool::LinearGradient)
    );
    assert_eq!(
        mask_tool_for_key(egui::Key::M, true),
        Some(MaskTool::Radial)
    );
    assert_eq!(mask_tool_for_key(egui::Key::Q, false), None);
    assert_eq!(mask_tool_for_key(egui::Key::G, false), None);
}

#[test]
fn w2_shortcut_mappings_are_exact() {
    // Welle 2 pure key mappings: every bound key maps, neighbours don't.
    assert_eq!(color_label_for_key(egui::Key::Num6), Some(1));
    assert_eq!(color_label_for_key(egui::Key::Num7), Some(2));
    assert_eq!(color_label_for_key(egui::Key::Num8), Some(3));
    assert_eq!(color_label_for_key(egui::Key::Num9), Some(4));
    assert_eq!(color_label_for_key(egui::Key::Num5), None);
    assert_eq!(color_label_for_key(egui::Key::P), None);
    assert_eq!(
        clipboard_action_for_key(egui::Key::C, true, true),
        Some(ClipboardAction::Copy)
    );
    assert_eq!(
        clipboard_action_for_key(egui::Key::V, true, true),
        Some(ClipboardAction::Paste)
    );
    assert_eq!(clipboard_action_for_key(egui::Key::C, false, true), None);
    assert_eq!(clipboard_action_for_key(egui::Key::C, true, false), None);
    assert_eq!(clipboard_action_for_key(egui::Key::V, true, false), None);
    assert_eq!(clipboard_action_for_key(egui::Key::X, true, true), None);
    assert_eq!(
        view_toggle_for_key(egui::Key::V),
        Some(ViewToggle::BlackWhite)
    );
    assert_eq!(
        view_toggle_for_key(egui::Key::J),
        Some(ViewToggle::Clipping)
    );
    assert_eq!(
        view_toggle_for_key(egui::Key::L),
        Some(ViewToggle::LightsOut)
    );
    assert_eq!(view_toggle_for_key(egui::Key::Y), None);
    assert_eq!(view_toggle_for_key(egui::Key::K), None);
    assert_eq!(
        panel_toggle_for_key(egui::Key::R),
        Some(PanelToggle::CropMode)
    );
    assert_eq!(
        panel_toggle_for_key(egui::Key::Tab),
        Some(PanelToggle::PanelsHidden)
    );
    assert_eq!(panel_toggle_for_key(egui::Key::T), None);
    assert_eq!(panel_toggle_for_key(egui::Key::F), None);
    // G-16 collision check: plain `S` belongs to the softproof preview —
    // no Welle-2 mapping may claim it.
    assert_eq!(color_label_for_key(egui::Key::S), None);
    assert_eq!(clipboard_action_for_key(egui::Key::S, true, true), None);
    assert_eq!(view_toggle_for_key(egui::Key::S), None);
    assert_eq!(panel_toggle_for_key(egui::Key::S), None);
    // Label names route through i18n, never literals.
    assert_eq!(color_label_name(1), "Red");
    assert_eq!(color_label_name(2), "Yellow");
    assert_eq!(color_label_name(3), "Green");
    assert_eq!(color_label_name(4), "Blue");
    assert_eq!(color_label_name(0), "Color Label");
    assert_eq!(color_label_name(9), "Color Label");
}
