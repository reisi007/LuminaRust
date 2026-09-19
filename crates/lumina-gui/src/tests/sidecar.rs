//! sidecar persistence, virtual copies, file-browser, regenerate tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

#[test]
fn gui_persists_virtual_copies_across_save_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("contrast", 0.3);
    app.save_sidecar();
    app.duplicate_virtual_copy("vc-2", "Copy 2").unwrap();
    app.save_sidecar();
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    assert_eq!(document.virtual_copies.len(), 2);
    assert!(document
        .virtual_copies
        .iter()
        .any(|copy| copy.id == "vc-2" && copy.name == "Copy 2"));
    assert_eq!(
        document.virtual_copies[0].recipe.adjustments["contrast"],
        0.3
    );

    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert_eq!(reopened.entries().len(), 1);
    let reloaded = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    assert_eq!(reloaded.virtual_copies.len(), 2);
}

#[test]
fn file_browser_index_reports_sidecar_and_copy_count() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.save_sidecar();
    app.duplicate_virtual_copy("vc-2", "Two").unwrap();
    app.duplicate_virtual_copy("vc-3", "Three").unwrap();
    app.save_sidecar();
    app.set_directory(directory.path().display().to_string());
    let entry = app
        .entries()
        .iter()
        .find(|e| e.name == "photo.png")
        .unwrap();
    assert!(entry.has_sidecar);
    assert_eq!(entry.virtual_copies, 3);
    assert!(!entry.conflict);
    assert!(!entry.is_offline());
    assert_eq!(entry.status_label(), Str::Sidecar.t());
}

#[test]
fn file_browser_detects_offline_source_and_conflict() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.save_sidecar();
    app.set_directory(directory.path().display().to_string());
    let entry = app
        .entries()
        .iter()
        .find(|e| e.name == "photo.png")
        .unwrap();
    assert!(!entry.is_offline());
    assert!(!entry.conflict);

    std::fs::remove_file(&source).unwrap();
    app.set_directory(directory.path().display().to_string());
    let entry = app
        .entries()
        .iter()
        .find(|e| e.name == "photo.png")
        .unwrap();
    assert!(entry.is_offline());
    assert!(entry.conflict);
    assert_eq!(entry.source_status, SourceStatus::Missing);
    assert_eq!(entry.status_label(), "Conflict");
}

#[test]
fn file_browser_reports_missing_mask_models() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.save_sidecar();
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let mut document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    document.virtual_copies[0]
        .mask_library
        .push(MaskDefinition {
            id: "m1".into(),
            name: "subject".into(),
            source_fingerprint: SourceFingerprint {
                content_hash: "blake3:x".into(),
                byte_length: 1,
                extras: BTreeMap::new(),
            },
            decode_context: DecodeFingerprint {
                decoder: "libraw".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: BTreeMap::new(),
            },
            geometry_context: GeometryFingerprint {
                width: 2,
                height: 1,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: BTreeMap::new(),
            },
            model: ModelIdentity {
                name: "birefnet".into(),
                version: "1".into(),
                hash: "h".into(),
                extras: BTreeMap::new(),
            },
            inference_resolution: Resolution {
                width: 2,
                height: 1,
                extras: BTreeMap::new(),
            },
            preprocessing: Preprocessing {
                name: "std".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: BTreeMap::new(),
            },
            rescaling_method: "bilinear".into(),
            rescaling_parameters: BTreeMap::new(),
            coordinate_system: CoordinateSystem::SourceOriented,
            status: MaskStatus::Missing,
            created_at: "2026-01-01T00:00:00Z".into(),
            generator_version: "g".into(),
            error_text: None,
            artifact: None,
            operation: MaskOperation::Source,
            references: vec![],
            prompt: None,
            extras: BTreeMap::new(),
            ai_select: None,
        });
    lumina_sidecar::save_sidecar(&sidecar, &document).unwrap();
    app.set_directory(directory.path().display().to_string());
    let entry = app
        .entries()
        .iter()
        .find(|e| e.name == "photo.png")
        .unwrap();
    assert_eq!(entry.missing_models, 1);
    assert!(entry.has_sidecar);

    let reloaded = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    assert_eq!(reloaded.virtual_copies[0].mask_library.len(), 1);
    assert_eq!(
        reloaded.virtual_copies[0].mask_library[0].status,
        MaskStatus::Missing
    );
}

#[test]
fn mask_selection_and_name_roundtrip_through_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let id = app.create_mask("Subject").unwrap();
    app.rename_mask(&id, "Main subject").unwrap();
    app.save_sidecar();

    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert_eq!(
        document.virtual_copies[0].mask_library[0].name,
        "Main subject"
    );
    assert_eq!(document.virtual_copies[0].mask_layers[0].mask.mask_id, id);
}

#[test]
fn mask_layer_parameters_are_non_destructive_and_persisted() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.create_mask("Subject").unwrap();
    app.set_mask_inverted(true).unwrap();
    app.set_mask_feather(0.25).unwrap();
    app.save_sidecar();

    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    let layer = &document.virtual_copies[0].mask_layers[0];
    assert!(layer.inverted);
    assert_eq!(layer.feather, 0.25);
    assert_eq!(
        document.virtual_copies[0].mask_library[0].status,
        MaskStatus::Pending
    );
}

#[test]
fn stale_mask_offers_recalculation_without_running_inference() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.create_mask("Subject").unwrap();
    assert!(app.offer_mask_recalculation().unwrap());
    assert!(app.status().contains("recalculation"));
    app.mark_mask_for_recalculation().unwrap();
    assert!(app.status().contains("requested"));
}

/// GUI-GEN-GRANULAR-10 (F-100): the collective regeneration is a no-op on a
/// fully fresh document — no module is recomputed implicitly.
#[test]
fn collective_regenerate_is_a_noop_on_a_fresh_document() {
    let mut app = new_app();
    app.load_bytes(png(), "fresh.png").unwrap();
    let done = app.regenerate_stale().unwrap();
    assert!(done.is_empty(), "fresh document regenerated {done:?}");
}

/// GUI-GEN-GRANULAR-10: the collective action runs exactly the stale module
/// actions (stale mask + enabled-but-stale Auto-Tone) and leaves the
/// disabled module (`matching`) alone.
#[test]
fn collective_regenerate_runs_stale_modules_only() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    // A freshly created mask is `Pending` (stale/missing artifact).
    app.create_mask("Subject").unwrap();
    // Enabled Auto-Tone without values/fingerprint is stale.
    app.recipe.auto_features.enable_auto_tone = true;

    let done = app.regenerate_stale().unwrap();

    assert!(done.contains(&"masks"), "{done:?}");
    assert!(done.contains(&"auto-tone"), "{done:?}");
    assert!(
        !done.contains(&"matching"),
        "matching is disabled: {done:?}"
    );
    assert!(app.recipe.auto_features.auto_exposure.is_some());
    let status = app.document.as_ref().unwrap().virtual_copies[0].mask_library[0]
        .status
        .clone();
    assert_eq!(status, MaskStatus::Pending);
}

/// GUI-GEN-GRANULAR-10 / M1: a two-slider Auto-Tone artifact (historic
/// `process --auto-tone`) is completed to the full AUTO-TONE-2 contract by
/// the collective action.
#[test]
fn collective_regenerate_completes_two_slider_auto_tone() {
    let mut app = new_app();
    app.load_bytes(png(), "two-slider.png").unwrap();
    {
        let auto = &mut app.recipe.auto_features;
        auto.enable_auto_tone = true;
        auto.auto_exposure = Some(0.1);
        auto.auto_contrast = Some(0.1);
    }

    let done = app.regenerate_stale().unwrap();

    assert_eq!(done, vec!["auto-tone"]);
    assert!(app.recipe.auto_features.auto_whites.is_some());
    assert!(app.recipe.auto_features.auto_blacks.is_some());
    assert!(app.recipe.auto_features.auto_highlights.is_some());
    assert!(app.recipe.auto_features.auto_shadows.is_some());
}

/// GUI-GEN-GRANULAR-10: every stale mask is marked — the collective action
/// is not limited to the currently selected mask.
#[test]
fn collective_regenerate_marks_every_stale_mask() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.create_mask("Sky").unwrap();
    app.create_mask("Tree").unwrap();

    let done = app.regenerate_stale().unwrap();

    // The module is reported once (deduplicated status message), even
    // though both masks were marked.
    assert_eq!(done, vec!["masks"]);
    for mask in &app.document.as_ref().unwrap().virtual_copies[0].mask_library {
        assert_eq!(mask.status, MaskStatus::Pending, "mask `{}`", mask.id);
    }
}

/// GUI-GEN-GRANULAR-10: each module action (Auto, Match Exposure) and the
/// collective default are reachable through painted, fully visible Develop
/// controls.
#[test]
fn per_module_and_collective_regenerate_actions_are_painted() {
    let mut app = new_app();
    app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .unwrap();
    app.set_module(Module::Develop);
    // The Auto-Tone action lives in the collapsible Basic section body.
    app.set_section_open(SECTION_BASIC, true);
    let shapes = headless_shapes_sized(&mut app, 4096.0, |app, ctx| {
        egui::Panel::right("controls")
            .resizable(true)
            .default_size(320.0)
            .show(ctx, |ui| app.draw_develop_panel(ui));
    });
    // Auto-Tone module action (Basic section).
    assert_fully_visible(&shapes, Str::Auto.t());
    // Matching module action (footer).
    assert_fully_visible(&shapes, Str::MatchExposure.t());
    // Collective default (footer).
    assert_fully_visible(&shapes, Str::RegenerateStale.t());
}

#[test]
fn local_mask_adjustments_roundtrip_through_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.create_mask("Subject").unwrap();
    app.set_mask_local_adjustment("exposure", 1.25).unwrap();
    app.set_mask_local_adjustment("contrast", -0.35).unwrap();
    app.save_sidecar();

    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    let layer = &reopened.document.as_ref().unwrap().virtual_copies[0].mask_layers[0];
    assert_eq!(layer.extras["adjustment_exposure"].as_f64(), Some(1.25));
    assert_eq!(layer.extras["adjustment_contrast"].as_f64(), Some(-0.35));
}
