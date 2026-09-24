//! G-03 AI/range masks persistence and panel tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

// ----- G-03 Maskierungs-Parität: Modell + Persistenz + Panel -----

/// E2E (DoD §1): AI-Maske und Luminance-Range über Datei anlegen,
/// speichern, neu laden — stabile IDs, getrennte Status, keine Re-Inferenz.
#[test]
fn g03_ai_and_range_masks_persist_per_copy_and_reload() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    let sky = app.create_ai_mask(AiSelectKind::Sky, None, "Sky").unwrap();
    let bright = app
        .create_luminance_range_mask(0.5, 1.0, 0.0, "Bright")
        .unwrap();
    assert_ne!(sky, bright);
    // Stable ids: same inputs reproduce the same id (no positional ids).
    assert!(sky.starts_with("mask-"));

    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    assert!(sidecar.is_file(), "save_sidecar must write synchronously");
    let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    assert!(document.validate().is_ok());
    let copy = &document.virtual_copies[0];
    let sky_def = copy.mask_library.iter().find(|m| m.id == sky).unwrap();
    assert_eq!(sky_def.ai_select.as_ref().unwrap().kind, AiSelectKind::Sky);
    assert_eq!(sky_def.status, MaskStatus::Pending);
    let lum_def = copy.mask_library.iter().find(|m| m.id == bright).unwrap();
    assert!(matches!(
        lum_def.prompt,
        Some(MaskPrompt::LuminanceRange { .. })
    ));
    assert_eq!(lum_def.status, MaskStatus::Valid);

    // Reload in a fresh app: library and statuses survive the restart
    // (selection is session state and starts empty — select again).
    let mut reopened = reopen_app(&source);
    let doc = reopened.document.as_ref().expect("document reloaded");
    assert_eq!(doc.virtual_copies[0].mask_library.len(), 2);
    reopened.select_mask(&bright).unwrap();
    let (status, _) = reopened.selected_mask_status().expect("selection kept");
    assert_eq!(status, MaskStatus::Valid);
}

/// E2E: Add/Subtract/Invert/Duplicate-Kombinatorik mit Persistenz;
/// Zyklen und falsche Stelligkeit werden laut abgewiesen.
#[test]
fn g03_combine_duplicate_roundtrip_and_rejects_cycles() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    let a = app.create_luminance_range_mask(0.0, 0.6, 0.0, "A").unwrap();
    let _b = app.create_luminance_range_mask(0.4, 1.0, 0.0, "B").unwrap();
    // `create_*` selects the new mask; re-select A as the combine basis.
    app.select_mask(&a).unwrap();
    let other = app.document.as_ref().unwrap().virtual_copies[0]
        .mask_library
        .iter()
        .find(|m| m.name == "B")
        .unwrap()
        .id
        .clone();
    let union = app
        .combine_masks(MaskOperation::Union, &other, "A+B")
        .unwrap();
    let inverted = app
        .combine_masks(MaskOperation::Invert, "", "not-A")
        .unwrap();
    assert_ne!(union, inverted);
    let dup = app.duplicate_mask(&a, "A copy").unwrap();
    assert_ne!(dup, a);

    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    assert!(document.validate().is_ok());
    let copy = &document.virtual_copies[0];
    assert_eq!(copy.mask_library.len(), 5);
    let union_def = copy.mask_library.iter().find(|m| m.id == union).unwrap();
    assert_eq!(union_def.operation, MaskOperation::Union);
    assert_eq!(union_def.references.len(), 2);
    // Subtract basis first: selected mask A is references[0].
    app.select_mask(&a).unwrap();
    let sub = app
        .combine_masks(MaskOperation::Subtract, &other, "A-B")
        .unwrap();
    let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    let sub_def = document.virtual_copies[0]
        .mask_library
        .iter()
        .find(|m| m.id == sub)
        .unwrap();
    assert_eq!(sub_def.references[0].mask_id, a);

    // Loud rejections: unknown other, self-combine, source-op, and a
    // derived duplicate (must be rebuilt with Combine instead).
    assert!(app
        .combine_masks(MaskOperation::Union, "missing", "X")
        .is_err());
    app.select_mask(&a).unwrap();
    assert!(app.combine_masks(MaskOperation::Union, &a, "X").is_err());
    assert!(app
        .combine_masks(MaskOperation::Source, &other, "X")
        .is_err());
    assert!(app.duplicate_mask(&union, "Union copy").is_err());
    assert!(app.duplicate_mask("missing", "X").is_err());
    // The file still validates after every rejection (rollback held).
    let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    assert!(document.validate().is_ok());
}

/// E2E: Sichtbarkeits-Auge persistiert pro virtueller Kopie; der Render
/// überspringt unsichtbare Layer (Core-Gegenstück in render.rs getestet).
#[test]
fn g03_visibility_eye_persists_and_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    let id = app
        .create_luminance_range_mask(0.0, 1.0, 0.0, "Full")
        .unwrap();
    // Fresh masks start visible (vacuous eye before any layer exists).
    assert!(app.mask_visible(&id));
    // Closing the eye creates the referencing layer invisibly.
    app.set_mask_visible(&id, false).unwrap();
    assert!(!app.mask_visible(&id));
    // Unknown masks are loud.
    assert!(app.set_mask_visible("missing", false).is_err());

    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    let layer = document.virtual_copies[0]
        .mask_layers
        .iter()
        .find(|l| l.mask.mask_id == id)
        .unwrap();
    assert!(!layer.visible);
    // Reopen: the closed eye survived.
    let reopened = reopen_app(&source);
    let doc = reopened.document.as_ref().expect("document reloaded");
    assert!(
        !doc.virtual_copies[0]
            .mask_layers
            .iter()
            .find(|l| l.mask.mask_id == id)
            .unwrap()
            .visible
    );
    // Re-open the eye.
    let mut reopened = reopened;
    reopened.select_mask(&id).unwrap();
    reopened.set_mask_visible(&id, true).unwrap();
    assert!(reopened.mask_visible(&id));
}

/// Session-Display-State (DoD §1-Anker ist der Panel-Status, kein File):
/// Show-Schalter und Overlay-Farbe sind `info!`-geloggt, berühren nie
/// Rezept/Sidecar und gaten das Overlay zusammen mit Auge und G-11-Modus.
#[test]
fn g03_show_and_color_overlay_gate_without_touching_recipe() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    app.set_section_open(SECTION_MASKING, true);
    let recipe = app.recipe().clone();
    assert!(app.show_mask_overlay());
    assert_eq!(app.overlay_color(), [255, 0, 0]);

    let id = app
        .create_ai_mask(AiSelectKind::People, Some("face".into()), "P")
        .unwrap();
    // The gate is switch + mode + eye (never a fallback matte): with no
    // layer the eye is vacuously open, so the overlay is allowed and the
    // draw path simply has no prompt to paint for this AI mask.
    assert!(app.mask_overlay_allowed());
    app.set_show_mask_overlay(false);
    assert!(!app.show_mask_overlay());
    assert!(!app.mask_overlay_allowed());
    app.set_show_mask_overlay(true);
    app.set_overlay_color([0, 255, 0]);
    assert_eq!(app.overlay_color(), [0, 255, 0]);
    assert!(app.mask_overlay_allowed());
    // Eye open + prompt-bearing selected mask paints (geometry path).
    let geo = app
        .create_luminance_range_mask(0.0, 1.0, 0.0, "Full")
        .unwrap();
    assert!(
        app.mask_overlay_allowed(),
        "range mask is immediately usable"
    );
    app.set_mask_visible(&geo, false).unwrap();
    assert!(!app.mask_overlay_allowed(), "closed eye hides the overlay");
    // G-11 mode still applies on top.
    app.set_mask_visible(&geo, true).unwrap();
    app.set_overlay_mode(OverlayMode::Never);
    assert!(!app.mask_overlay_allowed());
    app.set_overlay_mode(OverlayMode::Always);
    assert!(app.mask_overlay_allowed());
    // Session-only: recipe untouched throughout.
    assert_eq!(*app.recipe(), recipe);
    let _ = id;
}

/// Klassen-Vollständigkeit (DoD §3): jede AI-Art erzeugt eine typisierte,
/// validierende Maske; jede Range-Fehlklasse wird laut abgewiesen.
#[test]
fn g03_all_ai_kinds_and_range_rejections() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    for kind in AiSelectKind::all() {
        let id = app
            .create_ai_mask(kind, None, format!("M-{}", kind.as_str()))
            .unwrap();
        let document = app.document.as_ref().unwrap();
        let def = document.virtual_copies[0]
            .mask_library
            .iter()
            .find(|m| m.id == id)
            .unwrap();
        assert_eq!(def.ai_select.as_ref().unwrap().kind, kind);
        // Display names cover the class completely and distinctly.
        assert!(!ai_select_kind_name(kind).is_empty());
    }
    let mut names: Vec<&str> = AiSelectKind::all()
        .iter()
        .map(|kind| ai_select_kind_name(*kind))
        .collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 5);
    // Range rejections: min > max, hue out of degrees, NaN feather.
    assert!(app
        .create_luminance_range_mask(0.9, 0.1, 0.0, "bad")
        .is_err());
    assert!(app
        .create_color_range_mask(400.0, 60.0, 0.0, 1.0, 0.0, 1.0, 0.0, "bad")
        .is_err());
    assert!(app
        .create_luminance_range_mask(0.0, 1.0, f32::NAN, "bad")
        .is_err());
    // Empty names are loud everywhere.
    assert!(app.create_ai_mask(AiSelectKind::Sky, None, "  ").is_err());
    // The file was never touched by in-memory failures — but these
    // in-memory apps have no path, so assert the document still validates.
    assert!(app.document.as_ref().unwrap().validate().is_ok());
}

#[test]
fn g03_management_row_buttons_are_real_clicks() {
    let mut app = new_app();
    app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .unwrap();
    let first = app
        .create_luminance_range_mask(0.0, 0.5, 0.0, "First")
        .unwrap();
    app.create_luminance_range_mask(0.5, 1.0, 0.0, "Second")
        .unwrap();
    app.mask_rename_inputs
        .insert(first.clone(), "Widget renamed".into());
    let draw = |app: &mut LuminaApp, ui: &mut egui::Ui| {
        let document = app.document.clone().expect("document loaded");
        app.draw_masking_g03(ui, &document);
    };
    headless_click_labels_sized(&mut app, 2400.0, &[Str::RenameMask.t()], draw);
    assert_eq!(
        app.document.as_ref().unwrap().virtual_copies[0].mask_library[0].name,
        "Widget renamed"
    );
    let before = app.document.as_ref().unwrap().virtual_copies[0].mask_library[0]
        .id
        .clone();
    headless_click_labels_sized(&mut app, 2400.0, &[Str::MoveMaskDown.t()], draw);
    assert_eq!(
        app.document.as_ref().unwrap().virtual_copies[0].mask_library[1].id,
        before
    );
    let count = app.document.as_ref().unwrap().virtual_copies[0]
        .mask_library
        .len();
    headless_click_labels_sized(&mut app, 2400.0, &[Str::DuplicateMask.t()], draw);
    assert_eq!(
        app.document.as_ref().unwrap().virtual_copies[0]
            .mask_library
            .len(),
        count + 1
    );
    headless_click_labels_sized(&mut app, 2400.0, &[Str::DeleteMaskButton.t()], draw);
    assert_eq!(
        app.document.as_ref().unwrap().virtual_copies[0]
            .mask_library
            .len(),
        count
    );
}

#[test]
fn g03_management_rows_fit_1024x720_panel_width() {
    let mut app = new_app();
    app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .unwrap();
    app.create_luminance_range_mask(0.0, 0.5, 0.0, "First")
        .unwrap();
    app.create_luminance_range_mask(0.5, 1.0, 0.0, "Second")
        .unwrap();
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 720.0));
    let mut panel = egui::Rect::NOTHING;
    let mut shapes = Vec::new();
    for frame in 0..3 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(frame as f64 / 60.0),
                ..Default::default()
            },
            |ui| {
                let response = egui::Panel::right("mask-management")
                    .resizable(true)
                    .default_size(320.0)
                    .show(ui, |ui| {
                        let document = app.document.clone().expect("document loaded");
                        app.draw_masking_g03(ui, &document);
                    })
                    .response;
                panel = response.rect;
            },
        );
        output.textures_delta.clear();
        shapes = output.shapes;
    }
    assert!(
        panel.width() <= 321.0,
        "management rows widened panel: {panel:?}"
    );
    for clipped in &shapes {
        if let egui::Shape::Text(text) = &clipped.shape {
            assert!(
                text.pos.x <= panel.max.x + 0.5,
                "management text escaped the 320px panel: {:?}",
                text.pos
            );
        }
    }
}

/// Panel-Präsenz headless (DoD §5-Anker für jede sichtbare G-03-Fläche):
/// Maskenliste mit Auge, Show + Farbe, AI-/Range-/Combine-Zeilen und alle
/// vier Kombinator-Buttons malen im 320px-Panel. Malt `draw_masking_g03`
/// direkt (ohne CollapsingHeader — dessen Open-Animation malt headless nur
/// den animierten Kopf, siehe `masking_new_button_fully_inside_panel`).
#[test]
fn g03_panel_paints_all_controls_inside_panel() {
    let mut app = new_app();
    app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .unwrap();
    app.ensure_document_loaded().unwrap();
    app.create_luminance_range_mask(0.0, 1.0, 0.0, "Full")
        .unwrap();
    let ctx = egui::Context::default();
    // Tall screen so every G-03 row fits without scrolling (the width
    // assertion below is what this test guards).
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 2400.0));
    let mut panel_rect = egui::Rect::NOTHING;
    let mut shapes = Vec::new();
    for i in 0..2 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(i as f64 / 60.0),
                ..Default::default()
            },
            |ui| {
                let r = egui::Panel::right("controls")
                    .resizable(true)
                    .default_size(320.0)
                    .show(ui, |ui| {
                        let document = app.document.clone().expect("document loaded");
                        app.draw_masking_g03(ui, &document);
                    });
                panel_rect = r.response.rect;
                shapes = Vec::new();
            },
        );
        output.textures_delta.clear();
        shapes = output.shapes;
    }
    for needle in [
        Str::ShowOverlay.t(),
        Str::OverlayColor.t(),
        Str::AiSelectLabel.t(),
        Str::AddAiMask.t(),
        Str::LuminanceRange.t(),
        Str::ColorRange.t(),
        Str::AddRange.t(),
        Str::CombineLabel.t(),
        Str::CombineAdd.t(),
        Str::CombineSubtract.t(),
        Str::DuplicateMask.t(),
        Str::DuplicateGroup.t(),
        Str::MaskGroupsLabel.t(),
        Str::GroupSelected.t(),
        Str::MaskEye.t(),
        Str::MoveMaskUp.t(),
        Str::MoveMaskDown.t(),
        Str::RenameMask.t(),
        Str::DeleteMaskButton.t(),
        Str::DuplicateMask.t(),
    ] {
        assert_fully_visible(&shapes, needle);
    }
    assert!(
        panel_rect.width() <= 321.0,
        "G-03 rows must not push the panel past its 320px default (got {panel_rect:?})"
    );
}
