//! geometry blocking, rotation, save and virtual-copy session tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

// ---- REVIEW-GUI-MASKGEO-1: geometry blocks source-coordinate tools ----

#[test]
fn geometry_blocks_source_mapping_flags_each_dimension() {
    let mut app = new_app();
    assert!(!app.geometry_blocks_source_mapping(), "default is neutral");
    app.recipe.geometry = Some(Geometry {
        version: 1,
        crop: None,
        rotation_degrees: 90.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    });
    assert!(app.geometry_blocks_source_mapping(), "rotation blocks");
    app.recipe.geometry = Some(Geometry {
        version: 1,
        crop: None,
        rotation_degrees: 0.0,
        mirror_horizontal: true,
        mirror_vertical: false,
    });
    assert!(app.geometry_blocks_source_mapping(), "mirror blocks");
    app.recipe.geometry = Some(Geometry {
        version: 1,
        crop: Some(Crop::Free {
            x: 0.1,
            y: 0.1,
            width: 0.5,
            height: 0.5,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    });
    assert!(app.geometry_blocks_source_mapping(), "crop blocks");
    app.recipe.geometry = None;
    app.recipe.perspective = Some(Perspective {
        version: 1,
        vertical: 0.4,
        horizontal: 0.0,
        rotation: 0.0,
        scale: 1.0,
        aspect_ratio: 1.0,
        shift_x: 0.0,
        shift_y: 0.0,
    });
    assert!(app.geometry_blocks_source_mapping(), "perspective blocks");
    app.recipe.perspective = Some(Perspective {
        version: 1,
        vertical: 0.0,
        horizontal: 0.0,
        rotation: 0.0,
        scale: 1.0,
        aspect_ratio: 1.0,
        shift_x: 0.0,
        shift_y: 0.0,
    });
    assert!(
        !app.geometry_blocks_source_mapping(),
        "a neutral perspective is not blocking"
    );
}

/// GUI-ROTATE-1: rotation is wired end to end — the setter and the ±90°
/// quick buttons share one commit path, the render honours the rotation
/// (90° swaps the frame dimensions), and the value persists through
/// Datei + Reload (DoD §1).
#[test]
fn geometry_rotation_renders_persists_and_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source); // 2×1 fixture: a 90° turn must swap dimensions.
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let (w, h) = app.image_dims().expect("image loaded");
    assert_eq!((w, h), (2, 1));

    // Quick button path: +90° from neutral.
    app.rotate_step(90.0);
    assert_eq!(app.recipe.geometry.as_ref().unwrap().rotation_degrees, 90.0);
    assert_eq!(
        app.pending_slider_commit,
        Some(("geometry.rotation_degrees".to_string(), 90.0))
    );
    app.render().unwrap();
    let (rw, rh) = (
        app.preview.as_ref().unwrap().width,
        app.preview.as_ref().unwrap().height,
    );
    assert_eq!((rw, rh), (1, 2), "90° rotation must swap dimensions");

    // Quarter turns accumulate and wrap into (-180, 180].
    app.rotate_step(90.0);
    assert_eq!(
        app.recipe.geometry.as_ref().unwrap().rotation_degrees,
        180.0
    );
    app.rotate_step(90.0);
    assert_eq!(
        app.recipe.geometry.as_ref().unwrap().rotation_degrees,
        -90.0,
        "270° must wrap to -90°"
    );
    app.rotate_step(-90.0);
    assert_eq!(
        app.recipe.geometry.as_ref().unwrap().rotation_degrees,
        180.0
    );
    let wrapped = app.recipe.geometry.as_ref().unwrap().rotation_degrees;
    assert!(
        (-180.0..=180.0).contains(&wrapped),
        "rotation stays in domain, got {wrapped}"
    );

    // Slider path persists through Datei + Reload.
    app.set_geometry_rotation(-45.0);
    let document = commit_and_load_doc(&mut app, &source);
    assert_eq!(
        document.virtual_copies[0]
            .recipe
            .geometry
            .as_ref()
            .unwrap()
            .rotation_degrees,
        -45.0
    );
    let reopened = reopen_app(&source);
    assert_eq!(
        reopened
            .recipe()
            .geometry
            .as_ref()
            .unwrap()
            .rotation_degrees,
        -45.0
    );
}

#[test]
fn set_mask_tool_refused_visibly_while_geometry_active() {
    let mut app = new_app();
    app.load_bytes(png(), "geo.png").unwrap();
    app.recipe.geometry = Some(Geometry {
        version: 1,
        crop: Some(Crop::Free {
            x: 0.0,
            y: 0.0,
            width: 0.6,
            height: 0.6,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    });
    app.set_mask_tool(MaskTool::Brush);
    assert_eq!(app.mask_tool, MaskTool::None, "arming must be refused");
    assert!(
        app.status().contains("unavailable"),
        "refusal must be visible, got {:?}",
        app.status()
    );
    // Without geometry arming works again.
    app.recipe.geometry = None;
    app.set_mask_tool(MaskTool::Brush);
    assert_eq!(app.mask_tool, MaskTool::Brush);
    // Disarming stays possible in every state.
    app.set_mask_tool(MaskTool::None);
    assert_eq!(app.mask_tool, MaskTool::None);
}

// ---- REVIEW-GUI-SAVEMSG-1 / REVIEW-GUI-N1: save status + CAS ----

#[test]
fn failed_save_reports_error_and_never_claims_sidecar_saved() {
    use lumina_sidecar::{load_sidecar, save_sidecar as raw_save, sidecar_path_for};
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("exposure", 1.0);
    app.save_sidecar();
    assert!(app.error().is_none());
    assert_eq!(app.status(), Str::SidecarSaved.t());

    // External modification behind the GUI's back.
    let sidecar = sidecar_path_for(&source);
    let mut external = load_sidecar(&sidecar).unwrap();
    external.virtual_copies[0]
        .recipe
        .adjustments
        .insert("contrast".into(), 0.9);
    raw_save(&sidecar, &external).unwrap();

    // Local unsaved edit + save → the CAS must refuse with a visible
    // conflict instead of silently overwriting the external change.
    app.set_adjustment("exposure", 2.0);
    app.save_sidecar();
    assert_eq!(
        app.status(),
        Str::Error.t(),
        "conflicting save must surface as an error status"
    );
    assert!(app.error().is_some(), "conflict must be visible");
    assert_ne!(
        app.status(),
        Str::SidecarSaved.t(),
        "REVIEW-GUI-SAVEMSG-1: a failed save must never report success"
    );

    // The on-disk document is untouched by the refused write.
    let after = load_sidecar(&sidecar).unwrap();
    assert_eq!(
        after.virtual_copies[0].recipe.adjustments.get("exposure"),
        Some(&1.0)
    );
    assert_eq!(
        after.virtual_copies[0].recipe.adjustments.get("contrast"),
        Some(&0.9)
    );
}

#[test]
fn successful_save_keeps_loaded_source_identity_instead_of_recomputing_it() {
    use lumina_sidecar::{load_sidecar, sidecar_path_for};
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    // First session writes an initial sidecar.
    let mut first = new_app();
    open_and_decode(&mut first, source.display().to_string());
    first.save_sidecar();
    assert!(first.error().is_none());

    // Second session LOADS the sidecar; its identity must survive a
    // subsequent save untouched (REVIEW-GUI-N1: no silent recompute /
    // conflict laundering).
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let loaded_hash = app.document.as_ref().unwrap().source.content_hash.clone();
    assert!(
        loaded_hash.starts_with("blake3:"),
        "precondition: a real loaded identity, got {loaded_hash}"
    );
    app.set_adjustment("exposure", 0.5);
    app.save_sidecar();
    assert!(app.error().is_none());
    assert_eq!(app.status(), Str::SidecarSaved.t());
    let stored = load_sidecar(&sidecar_path_for(&source)).unwrap();
    assert_eq!(
        stored.source.content_hash, loaded_hash,
        "saving must not rewrite the loaded source identity"
    );
}

// ---- REVIEW-GUI-VCSWITCH-1: copy switch resets state, surfaces errors ----

#[test]
fn select_virtual_copy_resets_session_state_and_notes_discarded_edits() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("contrast", 0.3);
    app.save_sidecar();
    app.duplicate_virtual_copy("vc-2", "Copy 2").unwrap();
    app.save_sidecar();

    // Previous-copy session state that must not leak across the switch.
    app.history_selected = Some("history-stale".into());
    app.drag_start = Some(Point2 { x: 0.2, y: 0.2 });
    app.drag_current = Some(Point2 { x: 0.4, y: 0.4 });
    app.drawing = true;
    // Unsaved edit relative to vc-original.
    app.set_adjustment("exposure", 3.0);

    app.select_virtual_copy("vc-2").unwrap();
    assert_eq!(app.virtual_copy_id, "vc-2");
    assert_eq!(app.history_selected, None, "history selection must reset");
    assert_eq!(app.drag_start, None, "drag gesture state must reset");
    assert!(!app.drawing, "in-progress drag flag must reset");
    assert!(
        app.status().contains("discarded"),
        "discarding unsaved edits must be stated, got {:?}",
        app.status()
    );

    // A clean switch (no unsaved edits) reports without the warning.
    app.select_virtual_copy("vc-original").unwrap();
    assert!(app.status().starts_with("Switched to copy"));
    assert!(!app.status().contains("discarded"));

    // Unknown ids fail visibly instead of being swallowed.
    assert!(app.select_virtual_copy("nope").is_err());
}
