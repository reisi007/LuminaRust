//! slider/curve/color/optics commit-and-reload tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// R2-GUIMOD-04a: one coalesced drag tick records per-tick timings
/// (CPU draft / GPU / analysis) headless. Measurement only — the tick
/// renders the same draft the pointer-drag branch shows.
#[test]
fn drag_tick_records_timings() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    app.render().unwrap();
    assert!(app.last_drag_tick().is_none());
    // The exact call the pointer-drag branch makes per tick.
    app.render_draft_tick([320, 200]);
    assert!(app.preview_is_draft());
    let tick = app.last_drag_tick().expect("drag tick records timings");
    for (name, ms) in [
        ("cpu_draft_ms", tick.cpu_draft_ms),
        ("gpu_ms", tick.gpu_ms),
        ("analyse_ms", tick.analyse_ms),
    ] {
        assert!(
            ms.is_finite() && ms >= 0.0,
            "{name} must be finite non-negative ms, got {ms}"
        );
    }
    assert_eq!(app.last_analysis_ms(), tick.analyse_ms);
}

/// GUI-SLIDER-SAVE-1: a slider commit renders, writes the sidecar with the
/// committed value and clears the pending commit. Zoom/pan view state never
/// enters the recipe — panning/zooming records no commit.
#[test]
fn slider_commit_saves_sidecar_with_value() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());

    // A slider edit records its commit and dirties the render.
    app.set_adjustment("exposure", 1.5);
    assert_eq!(
        app.pending_slider_commit,
        Some(("exposure".to_string(), 1.5))
    );

    // The debounce commit renders and persists.
    app.commit_pending_slider_save([0, 0]);
    assert_eq!(app.pending_slider_commit, None);
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    assert!(sidecar.is_file(), "Sidecar must be written");
    let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    assert_eq!(
        document.virtual_copies[0].recipe.adjustments["exposure"],
        1.5
    );

    // Zoom/pan are pure view state: they record no commit and the recipe
    // carries no zoom/pan keys after exercising them.
    app.set_zoom_mode(ZoomMode::OneToOne);
    app.preview_pan = egui::vec2(24.0, -12.0);
    app.zoom_step(1.5);
    assert_eq!(app.pending_slider_commit, None);
    assert!(!app.recipe().adjustments.contains_key("zoom"));
    assert!(!app.recipe().adjustments.contains_key("preview_pan"));
    assert!(!app.recipe().adjustments.contains_key("zoom_mode"));

    // Reload: the committed value is restored from the sidecar (DoD §1).
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert_eq!(reopened.recipe().adjustments["exposure"], 1.5);
}

/// GUI-SLIDER-SAVE-1: presence sliders commit, persist and reload.
#[test]
fn presence_slider_commits_and_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_presence("clarity", 0.4);
    assert_eq!(
        app.pending_slider_commit,
        Some(("presence.clarity".to_string(), 0.4))
    );
    let document = commit_and_load_doc(&mut app, &source);
    let presence = document.virtual_copies[0]
        .recipe
        .presence
        .expect("presence persisted");
    assert!((f64::from(presence.clarity) - 0.4).abs() < 1e-6);
    let reopened = reopen_app(&source);
    let restored = reopened.recipe().presence.expect("presence reloaded");
    assert!((f64::from(restored.clarity) - 0.4).abs() < 1e-6);
}

/// GUI-SLIDER-SAVE-1: tone-curve region sliders commit, persist and reload.
#[test]
fn tone_curve_slider_commits_and_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    // NOTE: region values must keep the normative (0,0)/(1,1) endpoints —
    // positive Shadows would lift (0,0) and the save loudly refuses the
    // invalid curve (schema fact, tested by the loud-failure path, not here).
    app.set_tone_curve_region("lights", 0.2);
    assert_eq!(
        app.pending_slider_commit,
        Some(("curves.master.lights".to_string(), 0.2))
    );
    let document = commit_and_load_doc(&mut app, &source);
    let (_, _, l, _) = tone_curve_regions(&document.virtual_copies[0].recipe);
    assert!((l - 0.2).abs() < 1e-6, "lights region persisted, got {l}");
    let reopened = reopen_app(&source);
    let (_, _, rl, _) = tone_curve_regions(reopened.recipe());
    assert!((rl - 0.2).abs() < 1e-6, "lights region reloaded, got {rl}");
}

/// GUI-SLIDER-SAVE-1: HSL mixer sliders commit, persist and reload.
#[test]
fn hsl_slider_commits_and_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_hsl_value("red", "hue", 0.5);
    assert_eq!(
        app.pending_slider_commit,
        Some(("hsl.red.hue".to_string(), 0.5))
    );
    let document = commit_and_load_doc(&mut app, &source);
    let red = document.virtual_copies[0]
        .recipe
        .hsl
        .as_ref()
        .and_then(|hsl| hsl.red)
        .expect("hsl.red persisted");
    assert!((f64::from(red.hue) - 0.5).abs() < 1e-6);
    let reopened = reopen_app(&source);
    let rred = reopened
        .recipe()
        .hsl
        .as_ref()
        .and_then(|hsl| hsl.red)
        .expect("hsl.red reloaded");
    assert!((f64::from(rred.hue) - 0.5).abs() < 1e-6);
}

/// GUI-SLIDER-SAVE-1: color-grading sliders (range + balance) commit,
/// persist and reload.
#[test]
fn color_grading_sliders_commit_and_reload() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_color_grading_value("shadows", "hue_degrees", 120.0);
    app.set_color_grading_balance(0.3);
    let document = commit_and_load_doc(&mut app, &source);
    let cg = document.virtual_copies[0]
        .recipe
        .color_grading
        .clone()
        .expect("color grading persisted");
    assert!((f64::from(cg.shadows.hue_degrees) - 120.0).abs() < 1e-4);
    assert!((f64::from(cg.balance) - 0.3).abs() < 1e-6);
    let reopened = reopen_app(&source);
    let rcg = reopened
        .recipe()
        .color_grading
        .clone()
        .expect("grading reloaded");
    assert!((f64::from(rcg.shadows.hue_degrees) - 120.0).abs() < 1e-4);
    assert!((f64::from(rcg.balance) - 0.3).abs() < 1e-6);
}

/// G-02 Feinschliff: grading luminance + blending commit, persist and
/// reload (GUI-SLIDER-SAVE-1, Datei → Reload).
#[test]
fn color_grading_refinement_commits_and_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_color_grading_value("highlights", "luminance", 0.4);
    app.set_color_grading_blending(0.7);
    let document = commit_and_load_doc(&mut app, &source);
    let cg = document.virtual_copies[0]
        .recipe
        .color_grading
        .clone()
        .expect("color grading persisted");
    assert!((f64::from(cg.highlights.luminance) - 0.4).abs() < 1e-6);
    assert!((f64::from(cg.blending) - 0.7).abs() < 1e-6);
    let reopened = reopen_app(&source);
    let rcg = reopened
        .recipe()
        .color_grading
        .clone()
        .expect("grading reloaded");
    assert!((f64::from(rcg.highlights.luminance) - 0.4).abs() < 1e-6);
    assert!((f64::from(rcg.blending) - 0.7).abs() < 1e-6);
}

/// G-02 Kurve je Kanal: rote Parametrik + freier Punkt committen,
/// persistieren und laden; Master bleibt unberührt.
#[test]
fn channel_curve_param_and_points_commit_and_reload() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_tone_curve_channel_region("red", "lights", 0.2);
    assert_eq!(
        app.pending_slider_commit,
        Some(("curves.red.lights".to_string(), 0.2))
    );
    // Alle Kanäle teilen sich denselben parametrischen Pfad (Klassen-
    // Vollständigkeit): Grün läuft zusätzlich durch.
    app.set_tone_curve_channel_region("green", "darks", -0.1);
    app.add_curve_point("red", 0.25, 0.3);
    let document = commit_and_load_doc(&mut app, &source);
    let curves = document.virtual_copies[0]
        .recipe
        .curves
        .clone()
        .expect("curves persisted");
    let red = curves.channels.red.expect("red channel persisted");
    assert!(
        red.iter().any(|p| (f64::from(p.input) - 0.25).abs() < 1e-6),
        "added red point persisted, got {red:?}"
    );
    // Master stammt aus der Default-Identität (2 Punkte, unberührt), Rot
    // trägt die Parametrik-Liste (4 Punkte) plus den freien Punkt.
    assert_eq!(curves.master.len(), 2);
    assert_eq!(red.len(), 5);
    let green = curves.channels.green.expect("green channel persisted");
    assert_eq!(green.len(), 4, "green parametric persisted: {green:?}");
    let reopened = reopen_app(&source);
    let rred = reopened
        .recipe()
        .curves
        .clone()
        .expect("curves reloaded")
        .channels
        .red
        .expect("red channel reloaded");
    assert_eq!(rred.len(), 5);
    let rgreen = reopened
        .recipe()
        .curves
        .clone()
        .expect("curves reloaded")
        .channels
        .green
        .expect("green channel reloaded");
    assert_eq!(rgreen.len(), 4);
}

/// G-02 Punkteditor: ungültige Punkte (Duplikat, Endpunkt-Entfernung)
/// werden laut verweigert — kein Commit, kein Save.
#[test]
fn curve_point_editor_refuses_invalid_edits_loudly() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    app.add_curve_point("red", 0.25, 0.3);
    assert!(app.pending_slider_commit.is_some());
    // Duplikat-Input verstößt gegen streng aufsteigende Inputs.
    app.add_curve_point("red", 0.25, 0.4);
    assert!(app.status.contains("not saved"), "status: {}", app.status);
    // Endpunkte sind Pflicht und können nicht entfernt werden.
    app.remove_curve_point("red", 0);
    assert!(app.status.contains("mandatory"), "status: {}", app.status);
    let red = app
        .recipe
        .curves
        .clone()
        .expect("curves")
        .channels
        .red
        .expect("red");
    assert_eq!(red.len(), 3, "invalid edits left no trace: {red:?}");
    // Unbekannter Kanal warnt ohne Commit.
    app.set_tone_curve_channel_region("bogus", "lights", 0.2);
    app.set_curve_point("bogus", 0, "output", 0.5);
    app.add_curve_point("bogus", 0.5, 0.5);
    app.remove_curve_point("bogus", 0);
}

/// G-02 Point Color: Hinzufügen/Setzen/Entfernen committet, persistiert
/// und lädt (Datei → Reload); der Preview ändert sich sichtbar.
#[test]
fn point_color_add_set_remove_commit_and_reload() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let original_bytes = std::fs::read(&source).unwrap();
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let preview_before = app.preview().expect("preview after load").pixels.clone();
    app.add_point_color();
    app.set_point_color_value("pc-1", "hue_center", 30.0);
    app.set_point_color_value("pc-1", "hue_range", 20.0);
    app.set_point_color_value("pc-1", "hue_shift", 0.2);
    app.set_point_color_value("pc-1", "saturation_shift", -0.5);
    app.set_point_color_value("pc-1", "luminance_shift", 0.1);
    assert_eq!(
        app.pending_slider_commit,
        Some(("point_color.pc-1.luminance_shift".to_string(), 0.1))
    );
    let document = commit_and_load_doc(&mut app, &source);
    let preview_after = app.preview().expect("preview after commit").pixels.clone();
    assert_ne!(
        preview_before, preview_after,
        "point color desaturation must change the preview"
    );
    let entries = document.virtual_copies[0]
        .recipe
        .point_color
        .clone()
        .expect("point color persisted")
        .entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "pc-1");
    assert!((f64::from(entries[0].hue_center) - 30.0).abs() < 1e-6);
    assert!((f64::from(entries[0].hue_range) - 20.0).abs() < 1e-6);
    assert!((f64::from(entries[0].hue_shift) - 0.2).abs() < 1e-6);
    assert!((f64::from(entries[0].saturation_shift) + 0.5).abs() < 1e-6);
    assert!((f64::from(entries[0].luminance_shift) - 0.1).abs() < 1e-6);
    let reopened = reopen_app(&source);
    let rentries = reopened
        .recipe()
        .point_color
        .clone()
        .expect("point color reloaded")
        .entries;
    assert_eq!(rentries.len(), 1);
    assert_eq!(rentries[0].id, "pc-1");
    // Entfernen des letzten Eintrags löscht den Block (Absent = Identität).
    let mut app = reopened;
    app.remove_point_color("pc-1");
    assert!(app.recipe.point_color.is_none());
    let document = commit_and_load_doc(&mut app, &source);
    assert!(document.virtual_copies[0].recipe.point_color.is_none());
    // Das Original bleibt byte-identisch (nur Sidecar geschrieben).
    assert_eq!(std::fs::read(&source).unwrap(), original_bytes);
}

/// G-02 Point Color: Out-of-Range und Limit werden laut verweigert —
/// kein Commit, kein Save.
#[test]
fn point_color_refuses_invalid_edits_loudly() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    app.set_point_color_value("pc-99", "hue_center", 30.0);
    assert_eq!(app.pending_slider_commit, None);
    app.add_point_color();
    app.set_point_color_value("pc-1", "hue_center", 400.0);
    assert!(app.status.contains("not saved"), "status: {}", app.status);
    assert_eq!(
        app.pending_slider_commit,
        Some(("point_color.add".to_string(), 1.0)),
        "refused edit recorded no commit"
    );
    // Neunter Eintrag wird verweigert (Limit 8).
    for _ in 0..7 {
        app.add_point_color();
    }
    assert_eq!(
        app.recipe.point_color.clone().expect("block").entries.len(),
        8
    );
    app.add_point_color();
    assert_eq!(
        app.recipe.point_color.clone().expect("block").entries.len(),
        8,
        "ninth entry refused"
    );
    app.remove_point_color("pc-99");
    assert_eq!(
        app.recipe.point_color.clone().expect("block").entries.len(),
        8,
        "unknown remove left no trace"
    );
}

/// GUI-SLIDER-SAVE-1: effects sliders (vignette + grain seed) commit,
/// persist and reload.
#[test]
fn effects_sliders_commit_and_reload() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_effects_value("vignette", "amount", -0.5);
    app.set_effects_value("grain", "seed", 42.0);
    let document = commit_and_load_doc(&mut app, &source);
    let effects = document.virtual_copies[0]
        .recipe
        .effects
        .clone()
        .expect("effects persisted");
    assert!((f64::from(effects.vignette.expect("vignette").amount) + 0.5).abs() < 1e-6);
    assert_eq!(effects.grain.expect("grain").seed, 42);
    let reopened = reopen_app(&source);
    let reffects = reopened.recipe().effects.clone().expect("effects reloaded");
    assert!((f64::from(reffects.vignette.expect("vignette").amount) + 0.5).abs() < 1e-6);
    assert_eq!(reffects.grain.expect("grain").seed, 42);
}

/// GUI-SLIDER-SAVE-1: detail sliders (sharpening + noise reduction)
/// commit, persist and reload.
#[test]
fn detail_sliders_commit_and_reload() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_sharpening_value("amount", 1.5);
    app.set_noise_reduction_value("luminance", 0.25);
    let document = commit_and_load_doc(&mut app, &source);
    let recipe = &document.virtual_copies[0].recipe;
    let sh = recipe.sharpening.expect("sharpening persisted");
    let nr = recipe.noise_reduction.expect("noise reduction persisted");
    assert!((f64::from(sh.amount) - 1.5).abs() < 1e-6);
    assert!((f64::from(nr.luminance) - 0.25).abs() < 1e-6);
    let reopened = reopen_app(&source);
    let rsh = reopened.recipe().sharpening.expect("sharpening reloaded");
    let rnr = reopened.recipe().noise_reduction.expect("nr reloaded");
    assert!((f64::from(rsh.amount) - 1.5).abs() < 1e-6);
    assert!((f64::from(rnr.luminance) - 0.25).abs() < 1e-6);
}

/// GUI-SLIDER-SAVE-1: optics, geometry and perspective sliders commit,
/// persist and reload.
#[test]
fn optics_geometry_perspective_sliders_commit_and_reload() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_lens_correction_value("distortion_k1", 0.1);
    app.set_geometry_rotation(15.0);
    app.set_perspective_value("vertical", 0.2);
    let document = commit_and_load_doc(&mut app, &source);
    let recipe = &document.virtual_copies[0].recipe;
    assert_eq!(
        recipe
            .lens_correction
            .as_ref()
            .and_then(|lc| lc.distortion_k1),
        Some(0.1_f32)
    );
    assert_eq!(
        recipe.geometry.as_ref().map(|g| g.rotation_degrees),
        Some(15.0_f32)
    );
    assert_eq!(
        recipe.perspective.as_ref().map(|p| p.vertical),
        Some(0.2_f32)
    );
    let reopened = reopen_app(&source);
    assert_eq!(
        reopened
            .recipe()
            .lens_correction
            .as_ref()
            .and_then(|lc| lc.distortion_k1),
        Some(0.1_f32)
    );
    assert_eq!(
        reopened
            .recipe()
            .geometry
            .as_ref()
            .map(|g| g.rotation_degrees),
        Some(15.0_f32)
    );
    assert_eq!(
        reopened.recipe().perspective.as_ref().map(|p| p.vertical),
        Some(0.2_f32)
    );
}
