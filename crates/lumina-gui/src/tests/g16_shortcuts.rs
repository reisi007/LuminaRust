//! G-16 auto-endpoint/softproof mappings and apply tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

// ---- LR-PARITY-01 Welle 3 (lumina-gui only, no schema change) --------

#[test]
fn w3_shortcut_mappings_are_exact() {
    // Compare/survey (LR-20 light) and import/export (LR-13 light) pure
    // key mappings: every bound key maps, neighbours don't.
    assert_eq!(
        compare_mode_for_key(egui::Key::C),
        Some(CompareMode::Compare)
    );
    assert_eq!(
        compare_mode_for_key(egui::Key::N),
        Some(CompareMode::Survey)
    );
    assert_eq!(compare_mode_for_key(egui::Key::G), None);
    assert_eq!(compare_mode_for_key(egui::Key::Y), None);
    assert_eq!(compare_mode_for_key(egui::Key::V), None);
    assert_eq!(
        import_export_for_key(egui::Key::I, true, true),
        Some(ImportExportAction::Import)
    );
    assert_eq!(
        import_export_for_key(egui::Key::E, true, true),
        Some(ImportExportAction::Export)
    );
    assert_eq!(import_export_for_key(egui::Key::I, false, true), None);
    assert_eq!(import_export_for_key(egui::Key::I, true, false), None);
    assert_eq!(import_export_for_key(egui::Key::E, true, false), None);
    assert_eq!(import_export_for_key(egui::Key::C, true, true), None);
    // G-16 collision check: plain `S` is the softproof preview — no Welle-3
    // mapping may claim it, and the snapshot chord keeps its modifiers.
    assert_eq!(compare_mode_for_key(egui::Key::S), None);
    assert_eq!(import_export_for_key(egui::Key::S, true, true), None);
    assert_eq!(import_export_for_key(egui::Key::S, false, false), None);
}

// ---- LRPAR-G16-POWER (G-16 power-shortcut rest, lumina-gui only) -----

#[test]
fn g16_auto_endpoint_mapping_is_exact() {
    // Only whites/blacks with Shift+double-click route to the auto end
    // point; everything else falls back to the normal single reset.
    assert_eq!(
        auto_endpoint_for_slider("whites", true, true),
        Some(AutoEndpoint::White)
    );
    assert_eq!(
        auto_endpoint_for_slider("blacks", true, true),
        Some(AutoEndpoint::Black)
    );
    // Without Shift: normal reset, never the auto path (negative test).
    assert_eq!(auto_endpoint_for_slider("whites", false, true), None);
    assert_eq!(auto_endpoint_for_slider("blacks", false, true), None);
    // Without a double-click: normal path (negative test).
    assert_eq!(auto_endpoint_for_slider("whites", true, false), None);
    assert_eq!(auto_endpoint_for_slider("blacks", false, false), None);
    // Every other slider keeps the plain reset under Shift+double-click.
    for key in [
        "exposure",
        "contrast",
        "highlights",
        "shadows",
        "wb_temperature",
        "wb_tint",
        "texture",
        "",
    ] {
        assert_eq!(auto_endpoint_for_slider(key, true, true), None, "{key}");
    }
}

#[test]
fn g16_masking_preview_mapping_scope_is_exact() {
    // Scope decision (F-100): the six Basic tone sliders preview.
    for key in [
        "exposure",
        "contrast",
        "highlights",
        "shadows",
        "whites",
        "blacks",
    ] {
        assert!(masking_preview_for_slider(key), "{key}");
    }
    // Everything else (WB, presence, effects, empty) stays out of scope.
    for key in [
        "wb_temperature",
        "wb_tint",
        "texture",
        "clarity",
        "saturation",
        "vignette",
        "",
    ] {
        assert!(!masking_preview_for_slider(key), "{key}");
    }
}

#[test]
fn g16_softproof_mapping_is_exact_and_collision_free() {
    // Plain `S` arms the softproof preview; any modifier refuses it so no
    // existing chord (`Cmd/Ctrl+Alt+S` snapshot, copy-settings-adjacent
    // chords) is hijacked.
    assert!(softproof_for_key(egui::Key::S, false, false, false));
    assert!(!softproof_for_key(egui::Key::S, true, false, false));
    assert!(!softproof_for_key(egui::Key::S, false, true, false));
    assert!(!softproof_for_key(egui::Key::S, false, false, true));
    // No other bound key routes to softproof.
    for key in [
        egui::Key::G,
        egui::Key::D,
        egui::Key::E,
        egui::Key::Y,
        egui::Key::Q,
        egui::Key::K,
        egui::Key::M,
        egui::Key::P,
        egui::Key::X,
        egui::Key::U,
        egui::Key::C,
        egui::Key::N,
        egui::Key::V,
        egui::Key::J,
        egui::Key::L,
        egui::Key::R,
        egui::Key::F,
    ] {
        assert!(!softproof_for_key(key, false, false, false), "{key:?}");
    }
    // Full collision sweep: `S` is claimed by no pre-existing mapping.
    assert_eq!(module_for_key(egui::Key::S), None);
    assert_eq!(rating_for_key(egui::Key::S), None);
    assert_eq!(flag_for_key(egui::Key::S), None);
    assert_eq!(mask_tool_for_key(egui::Key::S, false), None);
    assert_eq!(mask_tool_for_key(egui::Key::S, true), None);
}

#[test]
fn g16_auto_endpoint_without_image_fails_loudly() {
    // No silent no-op: the end point needs a loaded source frame.
    let mut app = new_app();
    assert!(app.apply_auto_endpoint(AutoEndpoint::White).is_err());
    assert!(app.apply_auto_endpoint(AutoEndpoint::Black).is_err());
}

#[test]
fn g16_masking_preview_arms_clipping_badge_display_only() {
    // Effect test: Alt+slider arms the clipping badge through the shared
    // `J` gate; releasing disarms; out-of-scope keys are refused loudly.
    let mut app = new_app();
    assert!(!app.clipping_effective());
    app.set_masking_preview(Some("exposure")).unwrap();
    assert_eq!(app.masking_preview_key(), Some("exposure"));
    assert!(
        app.clipping_effective(),
        "preview reuses the J clipping badge"
    );
    assert!(
        app.recipe().adjustments.is_empty(),
        "preview never touches the recipe"
    );
    app.set_masking_preview(None).unwrap();
    assert_eq!(app.masking_preview_key(), None);
    assert!(!app.clipping_effective());
    // The armed `J` overlay keeps the gate independent of the preview.
    app.toggle_clipping_overlay();
    assert!(app.clipping_effective());
    app.toggle_clipping_overlay();
    assert!(!app.clipping_effective());
    // Out-of-scope keys fail loudly instead of arming a nameless preview.
    assert!(app.set_masking_preview(Some("texture")).is_err());
    assert!(app.set_masking_preview(Some("")).is_err());
    assert_eq!(app.masking_preview_key(), None);
}

#[test]
fn g16_softproof_toggle_is_display_only() {
    // Effect test: `S` flips a session-only badge; recipe and disk stay
    // untouched (full simulation is G-10 follow-up work).
    let mut app = new_app();
    assert!(!app.softproof_preview());
    app.toggle_softproof_preview();
    assert!(app.softproof_preview());
    assert!(
        app.recipe().adjustments.is_empty(),
        "softproof never touches the recipe"
    );
    app.toggle_softproof_preview();
    assert!(!app.softproof_preview());
}
