//! G-06 crop/straighten/lens/perspective tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

// ---- G-06 Geometrie-Parität (LRPAR-G06-GEO) ----

/// G-06: crop aspect + straighten + mirror commit through the debounced
/// save path (DoD §1: setter → commit → file → reload), each committed
/// step leaves exactly one visible history entry.
#[test]
fn g06_crop_aspect_straighten_mirror_commit_and_reload() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_crop_aspect("16:9").unwrap();
    app.set_straighten(2.5);
    app.set_geometry_mirror(true, false);
    let document = commit_and_load_doc(&mut app, &source);
    let recipe = &document.virtual_copies[0].recipe;
    assert!(matches!(
        recipe.geometry.as_ref().and_then(|g| g.crop.as_ref()),
        Some(Crop::Aspect {
            preset: AspectPreset::SixteenToNine
        })
    ));
    assert_eq!(
        recipe.geometry.as_ref().map(|g| g.rotation_degrees),
        Some(2.5_f32)
    );
    assert_eq!(
        recipe.geometry.as_ref().map(|g| g.mirror_horizontal),
        Some(true)
    );
    // Three setters before one debounce commit coalesce to ONE step.
    assert_eq!(document.virtual_copies[0].history.len(), 1);
    let entry = &document.virtual_copies[0].history[0];
    assert!(entry.id.starts_with("geometry-"), "got {}", entry.id);
    assert_eq!(entry.recipe, *recipe);
    let reopened = reopen_app(&source);
    assert!(matches!(
        reopened
            .recipe()
            .geometry
            .as_ref()
            .and_then(|g| g.crop.as_ref()),
        Some(Crop::Aspect {
            preset: AspectPreset::SixteenToNine
        })
    ));
    assert_eq!(
        reopened
            .recipe()
            .geometry
            .as_ref()
            .map(|g| g.rotation_degrees),
        Some(2.5_f32)
    );
}

/// G-06: free-crop rect round-trips; clearing removes only the rectangle
/// and keeps rotation/mirrors; each commit is its own history step.
#[test]
fn g06_crop_free_and_clear_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_crop_free(0.1, 0.2, 0.5, 0.5).unwrap();
    app.set_geometry_rotation(30.0);
    let document = commit_and_load_doc(&mut app, &source);
    assert!(matches!(
        document.virtual_copies[0]
            .recipe
            .geometry
            .as_ref()
            .and_then(|g| g.crop.as_ref()),
        Some(Crop::Free { .. })
    ));
    assert_eq!(document.virtual_copies[0].history.len(), 1);
    // Clear the rect in a second commit: rotation survives, second step.
    app.clear_crop();
    let document = commit_and_load_doc(&mut app, &source);
    let recipe = &document.virtual_copies[0].recipe;
    assert!(recipe
        .geometry
        .as_ref()
        .and_then(|g| g.crop.as_ref())
        .is_none());
    assert_eq!(
        recipe.geometry.as_ref().map(|g| g.rotation_degrees),
        Some(30.0_f32)
    );
    assert_eq!(document.virtual_copies[0].history.len(), 2);
    let reopened = reopen_app(&source);
    assert!(reopened
        .recipe()
        .geometry
        .as_ref()
        .and_then(|g| g.crop.as_ref())
        .is_none());
    assert_eq!(
        reopened
            .recipe()
            .geometry
            .as_ref()
            .map(|g| g.rotation_degrees),
        Some(30.0_f32)
    );
}

/// G-06: lens profile + coefficients + perspective commit and reload;
/// the profile picker rejects unknown names loudly without touching
/// the recipe.
#[test]
fn g06_lens_profile_and_perspective_commit_and_reload() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_lens_profile("wide-light").unwrap();
    app.set_lens_correction_value("ca_red", 0.01);
    app.set_perspective_value("shift_x", -0.3);
    let document = commit_and_load_doc(&mut app, &source);
    let recipe = &document.virtual_copies[0].recipe;
    assert_eq!(
        recipe
            .lens_correction
            .as_ref()
            .and_then(|l| l.profile.as_deref()),
        Some("wide-light")
    );
    assert_eq!(
        recipe.lens_correction.as_ref().and_then(|l| l.ca_red),
        Some(0.01_f32)
    );
    assert_eq!(
        recipe.perspective.as_ref().map(|p| p.shift_x),
        Some(-0.3_f32)
    );
    assert_eq!(document.virtual_copies[0].history.len(), 1);
    let reopened = reopen_app(&source);
    assert_eq!(
        reopened
            .recipe()
            .lens_correction
            .as_ref()
            .and_then(|l| l.profile.as_deref()),
        Some("wide-light")
    );
    // Unknown profile: loud error, recipe untouched, nothing armed.
    assert!(app.set_lens_profile("fisheye-extreme").is_err());
    assert!(app.set_crop_aspect("21:9").is_err());
    assert!(app.set_crop_free(f64::NAN, 0.0, 1.0, 1.0).is_err());
    assert_eq!(
        app.recipe
            .lens_correction
            .as_ref()
            .and_then(|l| l.profile.as_deref()),
        Some("wide-light"),
        "rejected edits must not touch the recipe"
    );
}

/// G-06: non-geometry sliders keep the established no-history-commit
/// behaviour — only armed geometry steps append entries.
#[test]
fn g06_only_geometry_commits_append_history() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("exposure", 1.0);
    let document = commit_and_load_doc(&mut app, &source);
    assert!(
        document.virtual_copies[0].history.is_empty(),
        "plain slider commits must not append history"
    );
    app.set_straighten(5.0);
    let document = commit_and_load_doc(&mut app, &source);
    assert_eq!(document.virtual_copies[0].history.len(), 1);
    let entry = &document.virtual_copies[0].history[0];
    assert_eq!(
        entry.extras.get("step").and_then(|v| v.as_str()),
        Some("geometry")
    );
    assert_eq!(
        entry.extras.get("action").and_then(|v| v.as_str()),
        Some("geometry.straighten")
    );
}

/// G-06: the crop overlay maps normalized rects into preview pixels
/// (free direct, aspect centered like the core crop, invalid → None).
#[test]
fn g06_crop_overlay_maps_normalized_rect() {
    let img = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(200.0, 100.0));
    assert!(LuminaApp::crop_overlay_rect(img, None, 400, 300).is_none());
    let free = Crop::Free {
        x: 0.25,
        y: 0.5,
        width: 0.5,
        height: 0.25,
    };
    let rect = LuminaApp::crop_overlay_rect(img, Some(&free), 400, 300).unwrap();
    assert_eq!(rect.min, egui::pos2(60.0, 70.0));
    assert_eq!(rect.max, egui::pos2(160.0, 95.0));
    // 1:1 on a 4:3 source centres horizontally (same math as core):
    // x=(1-3/4)/2=0.125, w=0.75 → min.x=10+25=35, max.x=10+175=185.
    let square = Crop::Aspect {
        preset: AspectPreset::OneToOne,
    };
    let rect = LuminaApp::crop_overlay_rect(img, Some(&square), 400, 300).unwrap();
    assert_eq!(rect.min, egui::pos2(35.0, 20.0));
    assert_eq!(rect.max, egui::pos2(185.0, 120.0));
    // Invalid rects map to None (never a painted lie); aspect presets
    // need source dimensions, free rects do not.
    let bad = Crop::Free {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.5,
    };
    assert!(LuminaApp::crop_overlay_rect(img, Some(&bad), 400, 300).is_none());
    assert!(LuminaApp::crop_overlay_rect(img, Some(&square), 0, 300).is_none());
    assert!(LuminaApp::crop_overlay_rect(img, Some(&free), 0, 300).is_some());
}

/// G-06: the Geometry section paints crop, aspect, straighten,
/// mirrors, perspective and the Lensfun auto status WITHOUT any
/// feature gate (both builds — F-093/F-099 are core-only).
#[test]
fn g06_geometry_section_paints_crop_and_straighten() {
    let mut app = new_app();
    app.set_section_open(SECTION_GEOMETRY, true);
    // "Clear Crop" paints only while a crop is set — arm one first
    // (paint-only, no commit needed).
    app.set_crop_aspect("3:2").unwrap();
    let shapes = headless_shapes(&mut app, |app, ui| app.draw_geometry(ui));
    let texts = painted_texts(&shapes);
    for want in [
        Str::Crop.t(),
        Str::Aspect.t(),
        Str::Straighten.t(),
        Str::MirrorHorizontal.t(),
        Str::MirrorVertical.t(),
        Str::Perspective.t(),
        Str::ClearCrop.t(),
    ] {
        assert!(
            texts.iter().any(|t| t == want),
            "geometry control {want:?} must be painted, got {texts:?}"
        );
    }
    assert!(
        texts.iter().any(|t| t.starts_with("Lensfun auto:")),
        "lensfun auto status must be painted, got {texts:?}"
    );
}

/// G-06: the Lensfun auto status names the EXIF state (no EXIF vs.
/// snapshot) and the missing-capability reason without the feature.
#[test]
fn g06_lensfun_auto_status_names_exif_state() {
    let app = new_app();
    let status = app.lensfun_auto_status_text();
    #[cfg(not(feature = "lensfun"))]
    assert!(
        status.contains("unavailable in this build"),
        "got `{status}`"
    );
    #[cfg(feature = "lensfun")]
    assert!(status.contains("no EXIF"), "got `{status}`");
    let mut app = new_app();
    app.loaded_lens_identity = Some(LensIdentity {
        camera_make: Some("Canon".into()),
        camera_model: Some("EOS R1".into()),
        lens: Some("RF200-800".into()),
        focal_length: Some(800.0),
        aperture: Some(9.0),
    });
    let status = app.lensfun_auto_status_text();
    #[cfg(feature = "lensfun")]
    {
        assert!(status.contains("Canon"), "got `{status}`");
        assert!(status.contains("EOS R1"), "got `{status}`");
        assert!(status.contains("RF200-800"), "got `{status}`");
    }
}
