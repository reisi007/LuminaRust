//! optics profile/manual fields and panel tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// GUI-OPTICS-1: the profile status names the profile when the recipe
/// carries one and reports the inactive automatic correction otherwise —
/// never a silent state.
#[test]
fn optics_profile_status_names_profile_or_reports_inactive() {
    let (text, active) = LuminaApp::lens_profile_status(&None);
    assert!(!active);
    assert_eq!(text, Str::OpticsProfileNone.t());
    let mut lc = LensCorrection {
        version: 1,
        profile: None,
        distortion_k1: None,
        distortion_k2: None,
        distortion_k3: None,
        vignette_c0: None,
        vignette_c1: None,
        vignette_c2: None,
        ca_red: None,
        ca_blue: None,
    };
    let (text, active) = LuminaApp::lens_profile_status(&Some(lc.clone()));
    assert!(!active, "profile None reads as inactive");
    assert_eq!(text, Str::OpticsProfileNone.t());
    lc.profile = Some(String::new());
    let (_, active) = LuminaApp::lens_profile_status(&Some(lc.clone()));
    assert!(!active, "empty profile reads as inactive");
    lc.profile = Some("Canon RF 24-105mm".into());
    let (text, active) = LuminaApp::lens_profile_status(&Some(lc));
    assert!(active);
    assert!(
        text.contains("Canon RF 24-105mm"),
        "status must name the profile, got {text:?}"
    );
}

/// GUI-OPTICS-1 (DoD-§3 Klassen-Vollständigkeit): all eight manual
/// optics fields commit through the single `set_lens_correction_value`
/// path; unknown names are ignored loudly without creating state.
#[test]
fn optics_all_fields_commit_through_one_path() {
    let mut app = new_app();
    assert!(app.recipe.lens_correction.is_none());
    for (field, value) in [
        ("distortion_k1", 0.1),
        ("distortion_k2", -0.2),
        ("distortion_k3", 0.3),
        ("vignette_c0", 0.05),
        ("vignette_c1", -0.05),
        ("vignette_c2", 0.01),
        ("ca_red", 0.004),
        ("ca_blue", -0.004),
    ] {
        app.set_lens_correction_value(field, value);
        let lens = app.recipe.lens_correction.as_ref().expect("lens block");
        let stored = match field {
            "distortion_k1" => lens.distortion_k1,
            "distortion_k2" => lens.distortion_k2,
            "distortion_k3" => lens.distortion_k3,
            "vignette_c0" => lens.vignette_c0,
            "vignette_c1" => lens.vignette_c1,
            "vignette_c2" => lens.vignette_c2,
            "ca_red" => lens.ca_red,
            "ca_blue" => lens.ca_blue,
            _ => unreachable!(),
        };
        assert_eq!(
            stored,
            Some(value as f32),
            "field {field} must persist {value}"
        );
        // Every optics edit arms the debounced save, like all sliders.
        assert_eq!(
            app.pending_slider_commit,
            Some((format!("lens_correction.{field}"), value))
        );
    }
    let mut fresh = new_app();
    fresh.set_lens_correction_value("bogus_field", 1.0);
    assert!(
        fresh.recipe.lens_correction.is_none(),
        "unknown optics fields must not create recipe state"
    );
}

/// GUI-OPTICS-1: a manual optics value set from the GUI path visibly
/// changes the rendered preview (the reported "no effect" is gone once
/// the panel can actually write values).
#[test]
fn optics_manual_distortion_changes_render() {
    let (png, _) = synthetic_gradient_png();
    let mut app = new_app();
    app.load_bytes(png, "gradient.png").unwrap();
    app.render().unwrap();
    let before = app.preview().unwrap().pixels.clone();
    app.set_lens_correction_value("distortion_k1", 0.5);
    app.render().unwrap();
    let after = app.preview().unwrap().pixels.clone();
    assert_ne!(
        before, after,
        "distortion_k1=0.5 must visibly change the preview"
    );
}

/// GUI-OPTICS-1 (DoD-§3 Klassen-Vollständigkeit): every remaining manual
/// optics field visibly changes the rendered preview — one fresh session
/// per field so cross-talk between fields is impossible.
#[test]
fn optics_each_manual_field_changes_render() {
    for (field, value) in [
        ("distortion_k2", 0.5),
        ("distortion_k3", 0.5),
        ("vignette_c0", 0.5),
        ("vignette_c1", 0.5),
        ("vignette_c2", 0.5),
        ("ca_red", 0.05),
        ("ca_blue", -0.05),
    ] {
        let (png, _) = synthetic_gradient_png();
        let mut app = new_app();
        app.load_bytes(png, "gradient.png").unwrap();
        app.render().unwrap();
        let before = app.preview().unwrap().pixels.clone();
        app.set_lens_correction_value(field, value);
        app.render().unwrap();
        let after = app.preview().unwrap().pixels.clone();
        assert_ne!(
            before, after,
            "{field}={value} must visibly change the preview"
        );
    }
}

/// GUI-OPTICS-1 + GUI-SLIDER-SAVE-1: all eight manual optics fields
/// persist through the sidecar file and reload in a fresh session.
#[test]
fn optics_fields_persist_across_save_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let fields = [
        ("distortion_k1", 0.1f32),
        ("distortion_k2", -0.2f32),
        ("distortion_k3", 0.3f32),
        ("vignette_c0", 0.5f32),
        ("vignette_c1", -0.5f32),
        ("vignette_c2", 0.25f32),
        ("ca_red", 0.02f32),
        ("ca_blue", -0.02f32),
    ];
    for (field, value) in fields {
        app.set_lens_correction_value(field, f64::from(value));
    }
    let document = commit_and_load_doc(&mut app, &source);
    let lens = document.virtual_copies[0]
        .recipe
        .lens_correction
        .clone()
        .expect("lens correction persisted");
    for (field, value) in fields {
        assert_eq!(
            lens_field(&lens, field),
            Some(value),
            "{field} must persist to the sidecar file"
        );
    }
    let reopened = reopen_app(&source);
    let reloaded = reopened
        .recipe()
        .lens_correction
        .clone()
        .expect("lens correction reloaded");
    for (field, value) in fields {
        assert_eq!(
            lens_field(&reloaded, field),
            Some(value),
            "{field} must survive the reload"
        );
    }
}

/// GUI-OPTICS-1: the Develop Optics section paints its profile status and
/// all three parameter groups; the hint texts exist, are distinct from
/// each other and from the group labels they annotate.
#[test]
fn optics_groups_and_hints_painted() {
    for (hint, group) in [
        (
            Str::OpticsDistortionHint.t(),
            Str::OpticsDistortionGroup.t(),
        ),
        (Str::OpticsVignetteHint.t(), Str::OpticsVignetteGroup.t()),
        (Str::OpticsCaHint.t(), Str::OpticsCaGroup.t()),
    ] {
        assert!(!hint.is_empty(), "optics hint must not be empty");
        assert_ne!(hint, group, "hint must differ from its group label");
    }
    assert_ne!(Str::OpticsDistortionHint.t(), Str::OpticsVignetteHint.t());
    assert_ne!(Str::OpticsVignetteHint.t(), Str::OpticsCaHint.t());
    assert_ne!(Str::OpticsDistortionHint.t(), Str::OpticsCaHint.t());
    let mut app = new_app();
    if !cfg!(feature = "lensfun") {
        // G-06: the manual sliders paint WITHOUT the native feature
        // (core-only model); only the auto status names the missing
        // capability (no silent empty section, no hidden sliders).
        app.set_section_open(SECTION_OPTICS, true);
        let shapes = headless_shapes(&mut app, |app, ui| app.draw_optics(ui));
        let texts = painted_texts(&shapes);
        assert!(
            texts.iter().any(|t| t == Str::OpticsDistortionGroup.t()),
            "manual distortion group must paint without lensfun, got {texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t.contains("unavailable in this build")),
            "missing lensfun capability must be named in the auto status, got {texts:?}"
        );
        return;
    }
    // G-11: section openness is explicit app state (`section_open`), not
    // egui-implicit memory — open the section through the setter so the
    // groups paint on a fresh headless context.
    app.set_section_open(SECTION_OPTICS, true);
    let ctx = egui::Context::default();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1024.0, 720.0),
        )),
        ..Default::default()
    };
    let mut output = ctx.run_ui(raw, |ui| {
        app.draw_optics(ui);
    });
    output.textures_delta.clear();
    let texts = painted_texts(&output.shapes);
    for group in [
        Str::OpticsDistortionGroup.t(),
        Str::OpticsVignetteGroup.t(),
        Str::OpticsCaGroup.t(),
    ] {
        assert!(
            texts.iter().any(|t| t == group),
            "optics group {group:?} must be painted, got {texts:?}"
        );
    }
    assert!(
        texts.iter().any(|t| t == Str::OpticsProfileNone.t()),
        "inactive profile status must be painted, got {texts:?}"
    );
}

/// GUI-OPTICS-1: without a profile the automatic correction is inactive —
/// a profile-less lens block renders byte-identical to no block at all,
/// never a silent auto-correction.
#[test]
fn auto_optics_without_profile_leaves_render_untouched() {
    let (png, _) = synthetic_gradient_png();
    let mut app = new_app();
    app.load_bytes(png, "gradient.png").unwrap();
    app.render().unwrap();
    let base = app.preview().unwrap().pixels.clone();
    let empty = LensCorrection {
        version: 1,
        profile: None,
        distortion_k1: None,
        distortion_k2: None,
        distortion_k3: None,
        vignette_c0: None,
        vignette_c1: None,
        vignette_c2: None,
        ca_red: None,
        ca_blue: None,
    };
    app.recipe.lens_correction = Some(empty);
    app.render().unwrap();
    assert_eq!(
        app.preview().unwrap().pixels,
        base,
        "a profile-less lens block must not touch the render"
    );
    // An empty profile string is not a silent inactive state at render
    // time: core validation refuses it loudly (no silent fallback).
    app.recipe.lens_correction.as_mut().unwrap().profile = Some(String::new());
    assert!(
        app.render().is_err(),
        "an empty profile must fail loudly, never render silently"
    );
    let (text, active) = LuminaApp::lens_profile_status(&app.recipe.lens_correction);
    assert!(!active);
    assert_eq!(text, Str::OpticsProfileNone.t());
}
