//! recipe, session and lifecycle basics tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

#[test]
fn recipe_change_and_render() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    app.set_adjustment("exposure", 1.0);
    app.render().unwrap();
    assert_eq!(app.recipe().adjustments["exposure"], 1.0);
    assert_eq!(app.preview().unwrap().pixels[0], 20);
}

#[test]
fn auto_and_matching_use_core_and_persist_recipe_state() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    app.auto_tone().unwrap();
    assert!(app.recipe().auto_features.enable_auto_tone);
    app.match_total_exposure(0.5).unwrap();
    assert!(app.recipe().auto_features.match_total_exposure);
    assert!(app.recipe().auto_features.matched_exposure.is_some());
}

#[test]
fn reset_restores_original_preview() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    app.set_adjustment("contrast", 1.0);
    app.render().unwrap();
    app.reset();
    assert!(app.recipe().adjustments.is_empty());
    assert_eq!(app.preview().unwrap().pixels[0], 10);
}

#[test]
fn set_rating_and_flag_persist_across_save_and_reopen() {
    // LR-01: rating/flag of the active copy survive a sidecar roundtrip
    // and are restored on reopen; out-of-range ratings fail loudly.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    // No sidecar on disk yet: no active rating (the rating section shows
    // "No sidecar loaded" in this state).
    assert_eq!(app.active_rating_flag(), None);
    app.set_rating(4).unwrap();
    app.set_flag(Flag::Pick).unwrap();
    assert_eq!(app.active_rating_flag(), Some((4, Flag::Pick)));
    assert!(app.set_rating(6).is_err());

    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert_eq!(document.virtual_copies[0].rating, 4);
    assert_eq!(document.virtual_copies[0].flag, Flag::Pick);

    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert_eq!(reopened.active_rating_flag(), Some((4, Flag::Pick)));
    // Clearing works too and persists.
    reopened.set_rating(0).unwrap();
    reopened.set_flag(Flag::Unflagged).unwrap();
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert_eq!(document.virtual_copies[0].rating, 0);
    assert_eq!(document.virtual_copies[0].flag, Flag::Unflagged);
}

#[test]
fn duplicate_active_copy_inherits_visible_recipe_and_rating() {
    // LR-09: the shortcut path saves unsaved edits first (the duplicate
    // inherits what the user sees), selects the new copy, and carries
    // over the rating/flag starting values.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("exposure", 1.5);
    app.set_rating(5).unwrap();
    app.set_flag(Flag::Reject).unwrap();
    let new_id = app.duplicate_active_copy().unwrap();
    assert_ne!(new_id, "vc-original");
    assert_eq!(app.active_rating_flag(), Some((5, Flag::Reject)));
    // The visible recipe (incl. the not-yet-saved exposure) was inherited.
    assert_eq!(app.recipe().adjustments["exposure"], 1.5);
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert_eq!(document.virtual_copies.len(), 2);
    let copy = document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == new_id)
        .unwrap();
    assert_eq!(copy.recipe.adjustments["exposure"], 1.5);
    assert_eq!(copy.rating, 5);
    assert_eq!(copy.flag, Flag::Reject);
}

#[test]
fn scan_entry_reports_default_copy_rating_flag() {
    // LR-01: the Library grid badge reads the default copy's rating/flag
    // through the normal directory scan (no separate code path).
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_rating(3).unwrap();
    app.set_flag(Flag::Pick).unwrap();
    app.set_directory(directory.path().display().to_string());
    let entry = app
        .entries
        .iter()
        .find(|entry| entry.name == "photo.png")
        .unwrap();
    assert_eq!(entry.rating, 3);
    assert_eq!(entry.flag, Flag::Pick);
}

#[test]
fn copy_paste_settings_roundtrip_persists_and_bumps_generation() {
    // Welle 2 (LR-09): copy snapshots the visible recipe, paste applies
    // it through save/render (generation bump + sidecar persistence).
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    assert!(!app.clipboard_has_settings());
    assert!(app.paste_settings().is_err());
    let generation = app.preview_generation();
    app.set_adjustment("exposure", 2.0);
    app.copy_settings().unwrap();
    assert!(app.clipboard_has_settings());
    app.set_adjustment("exposure", -1.0);
    app.paste_settings().unwrap();
    assert_eq!(app.recipe().adjustments["exposure"], 2.0);
    assert!(
        app.preview_generation() > generation,
        "paste must re-render the preview"
    );
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert_eq!(
        document.virtual_copies[0].recipe.adjustments["exposure"],
        2.0
    );
}

#[test]
fn clipboard_and_bw_without_image_fail_loudly() {
    // No silent no-ops: copy/paste/B&W without a loaded image are errors.
    let mut app = new_app();
    assert!(app.copy_settings().is_err());
    assert!(app.paste_settings().is_err());
    assert!(app.toggle_black_white().is_err());
    assert!(!app.clipboard_has_settings());
    assert!(!app.bw_active());
}

#[test]
fn black_white_treatment_sets_and_restores_saturation() {
    // Welle 2 (`V`): enabling drives saturation/vibrance to -1 through
    // the shared pipeline (grayscale preview pixels), disabling restores
    // the exact previous values; the marker persists in the sidecar.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("saturation", 0.4);
    let generation = app.preview_generation();
    app.toggle_black_white().unwrap();
    assert!(app.bw_active());
    assert_eq!(app.recipe().adjustments["saturation"], -1.0);
    assert_eq!(app.recipe().adjustments["vibrance"], -1.0);
    assert!(
        app.preview_generation() > generation,
        "B&W toggle must re-render the preview"
    );
    let preview = app.preview().unwrap();
    for px in preview.pixels.chunks_exact(4) {
        let (lo, hi) = (px[0].min(px[1]).min(px[2]), px[0].max(px[1]).max(px[2]));
        assert!(hi - lo <= 1, "B&W preview must be (near-)grayscale");
    }
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert_eq!(
        document.virtual_copies[0].recipe.extras["treatment"],
        serde_json::Value::String("bw".into())
    );
    app.toggle_black_white().unwrap();
    assert!(!app.bw_active());
    assert_eq!(app.recipe().adjustments["saturation"], 0.4);
    // `vibrance` was absent before `V` — it is removed again, never left
    // at -1.
    assert!(!app.recipe().adjustments.contains_key("vibrance"));
}

#[test]
fn view_toggles_flip_status_without_touching_recipe() {
    // Welle 2 (`J`/`L`/`R`/`Tab`): pure view state — flags flip, status
    // is visible, the recipe (and its sidecar lineage) never changes.
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    let recipe = app.recipe().clone();
    app.toggle_clipping_overlay();
    assert!(app.clipping_overlay);
    app.toggle_lights_out();
    assert!(app.lights_out);
    app.toggle_panels_hidden();
    assert!(app.panels_hidden);
    app.toggle_crop_mode();
    assert!(app.crop_mode);
    assert_eq!(*app.recipe(), recipe);
    assert_eq!(app.clipping_detail(), Some((0.0, 0.0)));
    // Second press disarms again; the recipe is still untouched.
    app.toggle_clipping_overlay();
    app.toggle_lights_out();
    app.toggle_panels_hidden();
    app.toggle_crop_mode();
    assert!(!app.clipping_overlay);
    assert!(!app.lights_out);
    assert!(!app.panels_hidden);
    assert!(!app.crop_mode);
    assert_eq!(*app.recipe(), recipe);
}

#[test]
fn preset_requires_name_and_validates_relative_exposure() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    assert!(app.create_preset("").is_err());
    app.preset_relative_exposure = true;
    assert!(app.create_preset("relative").is_err());
    app.recipe.auto_features.enable_auto_tone = true;
    let preset = app.create_preset("relative").unwrap();
    assert_eq!(preset.recipe.options["exposure_semantics"], "relative");
}

#[test]
fn decode_error_is_visible() {
    let mut app = new_app();
    let result = app.load_bytes(vec![1, 2, 3], "bad.png");
    assert!(result.is_err());
    app.show_error(result.unwrap_err());
    assert_eq!(app.status(), Str::Error.t());
    assert!(app.error().is_some());
}

#[test]
fn stale_auto_tone_clears_active_adjustments_but_keeps_status_state() {
    let mut recipe = EditRecipe::default();
    recipe.auto_features.enable_auto_tone = true;
    // Auto-written values carry mirrors ...
    recipe.auto_features.auto_exposure = Some(1.25);
    recipe.auto_features.auto_contrast = Some(-0.2);
    recipe.auto_features.auto_whites = Some(0.3);
    recipe.auto_features.auto_shadows = Some(-0.1);
    recipe.adjustments.insert("exposure".into(), 1.25);
    recipe.adjustments.insert("contrast".into(), -0.2);
    recipe.adjustments.insert("whites".into(), 0.3);
    recipe.adjustments.insert("shadows".into(), -0.1);
    // ... manual edits carry none and must survive the clear.
    recipe.adjustments.insert("highlights".into(), -0.5);
    recipe.adjustments.insert("blacks".into(), 0.1);

    clear_stale_auto_tone(&mut recipe);

    assert!(recipe.auto_features.enable_auto_tone);
    assert!(recipe.auto_features.auto_exposure.is_none());
    assert!(recipe.auto_features.auto_contrast.is_none());
    assert!(recipe.auto_features.auto_whites.is_none());
    assert!(recipe.auto_features.auto_blacks.is_none());
    assert!(recipe.auto_features.auto_highlights.is_none());
    assert!(recipe.auto_features.auto_shadows.is_none());
    assert!(!recipe.adjustments.contains_key("exposure"));
    assert!(!recipe.adjustments.contains_key("contrast"));
    assert!(!recipe.adjustments.contains_key("whites"));
    assert!(!recipe.adjustments.contains_key("shadows"));
    assert_eq!(recipe.adjustments["highlights"], -0.5);
    assert_eq!(recipe.adjustments["blacks"], 0.1);
}

#[test]
fn stale_auto_tone_validation_checks_input_fingerprint() {
    let frame = ImageFrame::new(1, 1, vec![128, 128, 128, 255]).unwrap();
    let input = tone_fingerprint(&frame, AutoToneConfig::default());
    let valid = AnalysisFingerprint {
        algorithm: "tone-rgba8-rec709".into(),
        version: "1".into(),
        input_fingerprint: input.clone(),
        extras: BTreeMap::new(),
    };
    assert!(is_current_tone_analysis(&valid, &input));
    for stored_input in [input.as_str(), "wrong"] {
        let stored = AnalysisFingerprint {
            algorithm: "arbitrary-pre-mvp-label".into(),
            version: "arbitrary-pre-mvp-value".into(),
            input_fingerprint: stored_input.into(),
            extras: BTreeMap::new(),
        };
        assert_eq!(
            is_current_tone_analysis(&stored, &input),
            stored_input == input
        );
    }
}

#[test]
fn sidecar_decoder_identity_distinguishes_raw_from_raster() {
    assert_eq!(decoder_identity(true), "libraw");
    assert_eq!(decoder_identity(false), "image");
}

#[test]
fn to_normalized_is_finite_for_zero_size_rect() {
    // Regression guard for the division-by-zero / NaN protection in
    // `to_normalized`: a momentarily empty preview rect (zero width/height)
    // must not yield non-finite normalized coordinates, which would
    // otherwise propagate into the recipe through the WB eyedropper / mask
    // tool mapping.
    let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(0.0, 0.0));
    let (nx, ny) = LuminaApp::to_normalized(egui::pos2(10.0, 20.0), rect, None, (100, 100));
    assert!(nx.is_finite(), "nx must be finite, got {nx}");
    assert!(ny.is_finite(), "ny must be finite, got {ny}");
}

#[test]
fn gui_writes_sidecar_and_restores_recipe_on_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("exposure", 1.5);
    app.save_sidecar();
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    assert!(sidecar.is_file(), "Sidecar must be written");
    let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    assert_eq!(
        document.virtual_copies[0].recipe.adjustments["exposure"],
        1.5
    );

    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert_eq!(reopened.recipe().adjustments["exposure"], 1.5);
}
