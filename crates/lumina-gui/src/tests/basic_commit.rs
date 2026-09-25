//! basic/auto-tone/exposure commit, dirty and export validation tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// GUI-SLIDER-SAVE-1: mask layer sliders (feather/blur/density) and local
/// adjustments commit, persist and reload.
#[test]
fn mask_layer_sliders_commit_and_reload() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.create_mask("Subject").unwrap();
    app.set_mask_feather(0.5).unwrap();
    app.set_mask_blur(0.2).unwrap();
    app.set_mask_density(0.8).unwrap();
    app.set_mask_local_adjustment("exposure", 0.7).unwrap();
    assert!(app.pending_slider_commit.is_some(), "mask edit commits");
    let document = commit_and_load_doc(&mut app, &source);
    let layer = &document.virtual_copies[0].mask_layers[0];
    assert_eq!(layer.feather, 0.5);
    assert_eq!(layer.blur, 0.2);
    assert_eq!(layer.density, 0.8);
    assert_eq!(layer.local_adjustments.as_ref().unwrap().exposure, 0.7);
    let reopened = reopen_app(&source);
    let rlayer = &reopened
        .document
        .as_ref()
        .expect("document reloaded")
        .virtual_copies[0]
        .mask_layers[0];
    assert_eq!(rlayer.feather, 0.5);
    assert_eq!(rlayer.local_adjustments.as_ref().unwrap().exposure, 0.7);
}

/// GUI-SLIDER-SAVE-1: tool-only settings (brush size, spot defaults)
/// record a commit and trigger the sidecar write; the values themselves
/// are session state, so the test pins the app fields plus the file.
#[test]
fn tool_settings_record_commit_and_write_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_brush_radius(0.1).unwrap();
    assert_eq!(
        app.pending_slider_commit,
        Some(("mask.brush_radius".to_string(), f64::from(0.1_f32)))
    );
    app.set_spot_radius(24.0);
    app.set_spot_feather(0.7);
    app.set_spot_opacity(0.9);
    assert_eq!(
        app.pending_slider_commit,
        Some(("spot.opacity".to_string(), f64::from(0.9_f32)))
    );
    commit_and_load_doc(&mut app, &source);
    assert_eq!(app.brush_radius, 0.1);
    assert_eq!(app.spot_radius, 24.0);
    assert_eq!(app.spot_feather, 0.7);
    assert_eq!(app.spot_opacity, 0.9);
}

/// GUI-SLIDER-SAVE-1: the WB eyedropper pick commits both fields, persists
/// and reloads.
#[test]
fn white_balance_pick_commits_and_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_white_balance_from_point(0.5, 0.5, 0.5).unwrap();
    // GUI-SIDECAR-READ-1: the pick commits synchronously (render + save),
    // so no commit stays armed and the sidecar file already holds both
    // fields without a manual debounce drive.
    assert_eq!(app.pending_slider_commit, None);
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    assert!(sidecar.is_file(), "WB pick must save the sidecar file");
    let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    assert_eq!(
        document.virtual_copies[0].recipe.adjustments["wb_temperature"],
        6500.0
    );
    let reopened = reopen_app(&source);
    assert_eq!(reopened.recipe().adjustments["wb_temperature"], 6500.0);
}

/// GUI-AUTOTONE-SAVE-1: `auto_tone` records a save commit, persists the
/// sidecar (Datei + Wert) and reloads (DoD §1-§4, F-100 „Auto-Tone
/// schreiben anschließend das Sidecar"). Zoom/pan stay untouched.
/// AUTO-TONE-2: all six sliders (`exposure`, `contrast`, `whites`,
/// `blacks`, `highlights`, `shadows`) persist 1:1 through Datei + Reload.
#[test]
fn auto_tone_commits_and_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.auto_tone().unwrap();
    assert!(app.recipe().auto_features.enable_auto_tone);
    let values: [(String, f64); 6] = [
        ("exposure".into(), app.recipe().adjustments["exposure"]),
        ("contrast".into(), app.recipe().adjustments["contrast"]),
        ("whites".into(), app.recipe().adjustments["whites"]),
        ("blacks".into(), app.recipe().adjustments["blacks"]),
        ("highlights".into(), app.recipe().adjustments["highlights"]),
        ("shadows".into(), app.recipe().adjustments["shadows"]),
    ];
    // Spec domains: exposure ±10 EV, the other five `-1..=1`.
    for (key, value) in &values {
        let (lo, hi) = if key == "exposure" {
            (-10.0, 10.0)
        } else {
            (-1.0, 1.0)
        };
        assert!(
            value.is_finite() && (lo..=hi).contains(value),
            "{key}={value} outside {lo}..={hi}"
        );
    }
    // GUI-SIDECAR-READ-1: `auto_tone` commits synchronously (render +
    // save + INFO log) — no stranded commit stays armed, and the sidecar
    // file already holds the values without a manual debounce drive.
    assert_eq!(app.pending_slider_commit, None);
    assert!(
        app.error().is_none(),
        "auto_tone commit must not fail, got {:?}",
        app.error()
    );
    let sidecar_path = lumina_sidecar::sidecar_path_for(&source);
    assert!(
        sidecar_path.is_file(),
        "auto_tone must write the sidecar file synchronously"
    );
    // AUTO-TONE-2: the mirrors mark all six adjustments as auto-written.
    let mirrors = [
        app.recipe().auto_features.auto_exposure,
        app.recipe().auto_features.auto_contrast,
        app.recipe().auto_features.auto_whites,
        app.recipe().auto_features.auto_blacks,
        app.recipe().auto_features.auto_highlights,
        app.recipe().auto_features.auto_shadows,
    ];
    for ((key, value), mirror) in values.iter().zip(mirrors) {
        assert_eq!(
            mirror,
            Some(*value),
            "{key} mirror must track the adjustment"
        );
    }
    let document = commit_and_load_doc(&mut app, &source);
    let persisted = &document.virtual_copies[0].recipe;
    assert!(persisted.auto_features.enable_auto_tone);
    // NOTE: f64 values cross a JSON roundtrip here, so the last bit may
    // differ (`0.2396484375` vs `...998`) — compare with a tight epsilon
    // instead of bit-exact `assert_eq`.
    for (key, value) in &values {
        let roundtripped = persisted.adjustments[key.as_str()];
        assert!(
            (roundtripped - value).abs() <= 1e-12,
            "{key} must persist to the sidecar file: {roundtripped} vs {value}"
        );
    }
    assert_eq!(
        persisted.auto_features.auto_exposure,
        app.recipe().auto_features.auto_exposure
    );
    assert_eq!(
        persisted.auto_features.auto_contrast,
        app.recipe().auto_features.auto_contrast
    );
    for (key, mirror) in [
        ("whites", persisted.auto_features.auto_whites),
        ("blacks", persisted.auto_features.auto_blacks),
        ("highlights", persisted.auto_features.auto_highlights),
        ("shadows", persisted.auto_features.auto_shadows),
    ] {
        let expected = values
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| *v)
            .unwrap();
        assert!(
            mirror.is_some_and(|m| (m - expected).abs() <= 1e-12),
            "{key} mirror must persist to the sidecar file"
        );
    }
    let reopened = reopen_app(&source);
    assert!(reopened.recipe().auto_features.enable_auto_tone);
    for (key, value) in &values {
        let reloaded = reopened.recipe().adjustments[key.as_str()];
        assert!(
            (reloaded - value).abs() <= 1e-12,
            "{key} must survive the reload: {reloaded} vs {value}"
        );
    }
    // Stale-clear on the reloaded recipe: auto-written values go (mirrors
    // reset), manual edits without a mirror survive.
    let mut stale = reopened.recipe().clone();
    stale.adjustments.insert("highlights".into(), -0.5);
    stale.auto_features.auto_highlights = None;
    clear_stale_auto_tone(&mut stale);
    for key in ["exposure", "contrast", "whites", "blacks", "shadows"] {
        assert!(
            !stale.adjustments.contains_key(key),
            "auto-written {key} must clear on stale"
        );
    }
    assert_eq!(stale.adjustments["highlights"], -0.5);
    assert!(stale.auto_features.auto_exposure.is_none());
    assert!(stale.auto_features.auto_whites.is_none());
}

/// GUI-AUTOTONE-SAVE-1: `match_total_exposure` records a save commit,
/// persists the sidecar (Datei + Wert) and reloads (DoD §1-§4, F-100).
/// Zoom/pan stay untouched.
#[test]
fn match_total_exposure_commits_and_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.render().unwrap();
    app.match_total_exposure(0.5).unwrap();
    assert!(app.recipe().auto_features.match_total_exposure);
    let delta = app.recipe().auto_features.matched_exposure.unwrap();
    let exposure = app.recipe().adjustments["exposure"];
    // GUI-SIDECAR-READ-1: synchronous commit — nothing stays armed and
    // the sidecar file already holds the match without a debounce drive.
    assert_eq!(app.pending_slider_commit, None);
    assert!(
        app.error().is_none(),
        "match commit must not fail, got {:?}",
        app.error()
    );
    let sidecar_path = lumina_sidecar::sidecar_path_for(&source);
    assert!(
        sidecar_path.is_file(),
        "match_total_exposure must write the sidecar file synchronously"
    );
    let document = commit_and_load_doc(&mut app, &source);
    let persisted = &document.virtual_copies[0].recipe;
    assert!(persisted.auto_features.match_total_exposure);
    assert_eq!(persisted.auto_features.matched_exposure, Some(delta));
    assert_eq!(persisted.adjustments["exposure"], exposure);
    let reopened = reopen_app(&source);
    assert!(reopened.recipe().auto_features.match_total_exposure);
    assert_eq!(
        reopened.recipe().auto_features.matched_exposure,
        Some(delta)
    );
    assert_eq!(reopened.recipe().adjustments["exposure"], exposure);
}

/// GUI-SIDECAR-READ-1 (N6 regression): a flat slider edit
/// (`set_adjustment`, the Basic-panel path) must survive the full DoD
/// chain Edit → Commit → Sidecar-Datei → Reload.
#[test]
fn exposure_slider_commits_and_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("exposure", 1.5);
    assert_eq!(
        app.pending_slider_commit,
        Some(("exposure".to_string(), 1.5))
    );
    // The debounced update-loop path (`commit_pending_slider_save`) is
    // driven here directly — headless has no pointer-release timer.
    let document = commit_and_load_doc(&mut app, &source);
    assert_eq!(
        document.virtual_copies[0].recipe.adjustments["exposure"],
        1.5
    );
    let reopened = reopen_app(&source);
    assert_eq!(reopened.recipe().adjustments["exposure"], 1.5);
}

/// GUI-SIDECAR-READ-1 (N6 regression): switching images with an
/// uncommitted slider drag must flush the edit to the old image's
/// sidecar instead of dropping it in `apply_decoded_frame`.
#[test]
fn switching_image_flushes_pending_slider_edit() {
    let directory = tempfile::tempdir().unwrap();
    let source_a = directory.path().join("a.png");
    let source_b = directory.path().join("b.png");
    save_png(&source_a);
    save_png(&source_b);
    let mut app = new_app();
    open_and_decode(&mut app, source_a.display().to_string());
    app.set_adjustment("exposure", 2.0);
    assert!(app.pending_slider_commit.is_some());
    // Switching arms the background decode of B; the flush to A's
    // sidecar happens synchronously inside `open_file`.
    open_and_decode(&mut app, source_b.display().to_string());
    assert_eq!(app.pending_slider_commit, None);
    let sidecar_a = lumina_sidecar::sidecar_path_for(&source_a);
    assert!(sidecar_a.is_file(), "A's edit must be flushed on switch");
    let document_a = lumina_sidecar::load_sidecar(&sidecar_a).unwrap();
    assert_eq!(
        document_a.virtual_copies[0].recipe.adjustments["exposure"],
        2.0
    );
}

/// GUI-SLIDER-SAVE-1: unknown struct field names warn loudly but record no
/// commit and mutate nothing (no silent fallback into a wrong field).
#[test]
fn unknown_recipe_fields_warn_without_commit() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    app.set_tone_curve_region("bogus", 0.5);
    app.set_hsl_value("red", "bogus", 0.5);
    app.set_hsl_value("bogus", "hue", 0.5);
    app.set_color_grading_value("shadows", "bogus", 0.5);
    app.set_effects_value("grain", "bogus", 0.5);
    app.set_sharpening_value("bogus", 0.5);
    app.set_noise_reduction_value("bogus", 0.5);
    app.set_lens_correction_value("bogus", 0.5);
    app.set_perspective_value("bogus", 0.5);
    assert_eq!(app.pending_slider_commit, None);
    assert!(app.recipe().curves.is_none());
    assert!(app.recipe().hsl.is_none());
    assert!(app.recipe().color_grading.is_none());
    assert!(app.recipe().effects.is_none());
    assert!(app.recipe().sharpening.is_none());
}
