//! lens blur setters/overlay/depth tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

#[test]
fn g05_lens_blur_setters_persist_file_to_reload() {
    // DoD §1 E2E: setter edit → debounced commit → sidecar file → reload.
    // DoD §2: the 150-ms debounce path is driven headless via
    // `commit_pending_slider_save` (the same hook all sliders use).
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    std::fs::write(&source, lens_blur_striped_png()).unwrap();
    let original_bytes = std::fs::read(&source).unwrap();
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    assert!(app.lens_blur().is_none());
    assert_eq!(app.lens_blur_status_text(), "off");
    // Invalid values fail loudly and change nothing (no silent clip).
    assert!(app.set_lens_blur_amount(1.5).is_err());
    assert!(app.set_lens_blur_amount(f64::NAN).is_err());
    assert!(app.set_lens_blur_focal(0.7, 0.2).is_err());
    assert!(app.set_lens_blur_focal(-0.1, 0.5).is_err());
    assert!(app.set_lens_blur_focus_rect(0.8, 0.8, 0.5, 0.5).is_err());
    assert!(app.set_lens_blur_focus_rect(0.1, 0.1, 0.0, 0.5).is_err());
    assert!(app.lens_blur().is_none());
    // Set every field (DoD §3: all three bokeh shapes mapped).
    for (shape, name) in [
        (BokehShape::Round, "round"),
        (BokehShape::Elliptical, "elliptical"),
        (BokehShape::Hexagonal, "hexagonal"),
    ] {
        app.set_lens_blur_enabled(true);
        app.set_lens_blur_amount(0.75).unwrap();
        app.set_lens_blur_focal(0.1, 0.5).unwrap();
        app.set_lens_blur_bokeh(shape);
        app.set_lens_blur_focus_rect(0.2, 0.3, 0.4, 0.25).unwrap();
        app.commit_pending_slider_save([0, 0]);
        assert!(app.error().is_none(), "commit failed for {name}");
        let sidecar = lumina_sidecar::sidecar_path_for(&source);
        let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
        let blur = document.virtual_copies[0]
            .recipe
            .lens_blur
            .as_ref()
            .expect("stage persisted");
        assert!(blur.enabled);
        assert_eq!(blur.bokeh, shape, "bokeh {name} round-trips");
        assert_eq!(blur.blur_amount, 0.75);
        assert_eq!((blur.focal_near, blur.focal_far), (0.1, 0.5));
        // Reload leg: a fresh app restores the stage from the file alone.
        let mut reopened = new_app();
        open_and_decode(&mut reopened, source.display().to_string());
        assert_eq!(reopened.lens_blur(), Some(blur.clone()));
        assert_eq!(reopened.lens_blur_status_text(), "heuristic active");
    }
    // Disable keeps values but reports off and renders identity.
    app.set_lens_blur_enabled(false);
    app.commit_pending_slider_save([0, 0]);
    assert!(app.error().is_none());
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    let blur = document.virtual_copies[0]
        .recipe
        .lens_blur
        .as_ref()
        .unwrap();
    assert!(!blur.enabled);
    assert_eq!(app.lens_blur_status_text(), "off");
    // Clear removes the stage; reload confirms identity.
    app.clear_lens_blur();
    app.commit_pending_slider_save([0, 0]);
    assert!(app.error().is_none());
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert!(document.virtual_copies[0].recipe.lens_blur.is_none());
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert!(reopened.lens_blur().is_none());
    // The original image is byte-identical throughout.
    assert_eq!(std::fs::read(&source).unwrap(), original_bytes);
}

#[test]
fn g05_lens_blur_preview_changes_and_missing_depth_fails_loudly() {
    // Enabling the blur visibly changes the preview (same render entry
    // point as CLI/export — no second pipeline); a referenced but
    // missing depth artifact fails the commit loudly instead of
    // silently rendering the heuristic.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("stripes.png");
    std::fs::write(&source, lens_blur_striped_png()).unwrap();
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let before = app.preview().expect("preview after load").pixels.clone();
    app.set_lens_blur_enabled(true);
    app.set_lens_blur_amount(0.75).unwrap();
    app.commit_pending_slider_save([0, 0]);
    assert!(app.error().is_none());
    let after = app.preview().expect("preview after blur").pixels.clone();
    assert_ne!(before, after, "enabled blur must change the preview");
    // Missing depth artifact: the committed render aborts loudly and the
    // error stays visible (never a silent heuristic render).
    app.recipe.lens_blur.as_mut().unwrap().depth_artifact = Some(DepthArtifactRef {
        relative_path: "depth/map.bin".into(),
        sha256: "sha256:abc".into(),
    });
    assert_eq!(app.lens_blur_status_text(), "missing depth artifact");
    app.mark_dirty();
    app.commit_pending_slider_save([0, 0]);
    assert!(
        app.error().is_some(),
        "missing depth artifact must surface a visible error"
    );
}

#[test]
fn g05_lens_blur_focus_overlay_maps_normalized_rect() {
    // Pure mapping: normalized recipe rect into preview-image rect.
    let img = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(100.0, 50.0));
    assert!(LuminaApp::lens_blur_focus_overlay(img, None).is_none());
    let disabled = lumina_sidecar::LensBlur {
        version: 1,
        enabled: false,
        focus_rect: lumina_sidecar::FocusRect {
            x: 0.25,
            y: 0.25,
            width: 0.5,
            height: 0.5,
        },
        focal_near: 0.0,
        focal_far: 0.2,
        blur_amount: 0.5,
        bokeh: BokehShape::Round,
        depth_artifact: None,
    };
    assert!(LuminaApp::lens_blur_focus_overlay(img, Some(&disabled)).is_none());
    let enabled = lumina_sidecar::LensBlur {
        enabled: true,
        ..disabled
    };
    let rect = LuminaApp::lens_blur_focus_overlay(img, Some(&enabled)).unwrap();
    assert_eq!(rect.min, egui::pos2(35.0, 32.5));
    assert_eq!(rect.max, egui::pos2(85.0, 57.5));
}
