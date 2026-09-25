//! Cross-image Previous and Sync regressions for MASK-LOCAL-P0/P1.1.

use super::*;

#[test]
fn previous_transfers_full_local_state_but_sync_keeps_target_layers() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("previous-source.png");
    let target = directory.path().join("previous-target.png");
    let sync_target = directory.path().join("sync-target.png");
    for path in [&source, &target, &sync_target] {
        save_png(path);
    }

    // Prepare a target mask context with the same stable mask id as the source.
    let mut target_app = new_app();
    open_and_decode(&mut target_app, target.display().to_string());
    target_app.create_mask("Shared").unwrap();
    target_app.save_sidecar();
    drop(target_app);

    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.create_mask("Shared").unwrap();
    app.set_mask_local_adjustment("exposure", 0.75).unwrap();
    app.set_mask_local_wb_delta(1250.0, 0.25).unwrap();
    app.save_sidecar();
    open_and_decode_switch(&mut app, &target.display().to_string());
    assert_eq!(
        app.previous_reference
            .as_ref()
            .unwrap()
            .mask_state
            .layers
            .len(),
        1
    );
    app.filmstrip_selection.clear();

    let report = app.apply_previous_to_selection();
    assert_eq!(report.applied_count(), 1, "report: {report:?}");
    assert_eq!(report.failed_count(), 0, "report: {report:?}");
    assert_eq!(
        app.selected_mask_local_adjustment("exposure").unwrap(),
        Some(0.75)
    );
    let target_document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&target)).unwrap();
    let target_copy = &target_document.virtual_copies[0];
    assert_eq!(
        target_copy.mask_layers[0]
            .local_adjustments
            .as_ref()
            .map(|value| value.exposure),
        Some(0.75)
    );
    assert_eq!(
        target_copy.mask_layers[0]
            .local_adjustments
            .as_ref()
            .map(|value| (value.temperature_delta_k, value.tint_delta)),
        Some((1250.0, 0.25))
    );
    assert!(target_copy
        .history
        .last()
        .unwrap()
        .mask_state()
        .unwrap()
        .is_some());
    app.restore_history("previous-0").unwrap();
    assert!(!app.recipe().adjustments.contains_key("exposure"));
    assert_eq!(
        app.selected_mask_local_adjustment("exposure").unwrap(),
        None
    );
    assert_eq!(app.selected_mask_local_wb_delta().unwrap(), (0.0, 0.0));

    // Sync Settings remains recipe-only: it must not erase a target's local
    // layer state.
    let mut sync_app = new_app();
    open_and_decode(&mut sync_app, sync_target.display().to_string());
    sync_app.create_mask("Other").unwrap();
    sync_app
        .set_mask_local_adjustment("contrast", -0.4)
        .unwrap();
    sync_app.save_sidecar();
    drop(sync_app);
    app.recipe.adjustments.insert("exposure".into(), 1.25);
    app.filmstrip_selection.clear();
    app.filmstrip_selection
        .insert(sync_target.display().to_string());
    let report = app.sync_settings_to_selection();
    assert_eq!(report.applied_count(), 1, "report: {report:?}");
    let sync_document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&sync_target)).unwrap();
    assert_eq!(
        sync_document.virtual_copies[0].mask_layers[0]
            .local_adjustments
            .as_ref()
            .map(|value| value.contrast),
        Some(-0.4)
    );
}

#[test]
fn previous_rejects_missing_cross_image_mask_context_loudly() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("incompatible-source.png");
    let target = directory.path().join("incompatible-target.png");
    save_png(&source);
    save_png(&target);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.create_mask("OnlySource").unwrap();
    app.set_mask_local_adjustment("exposure", 1.0).unwrap();
    app.save_sidecar();
    open_and_decode_switch(&mut app, &target.display().to_string());
    app.filmstrip_selection.clear();
    app.filmstrip_selection.insert(target.display().to_string());

    let report = app.apply_previous_to_selection();
    assert_eq!(report.applied_count(), 0);
    assert_eq!(report.failed_count(), 1);
    assert!(report.failed[0].1.contains("missing target mask"));
    assert!(app.error().is_some());
    assert!(!lumina_sidecar::sidecar_path_for(&target).exists());
}

#[test]
fn previous_rejects_a_nonportable_cross_copy_reference_loudly() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("cross-copy-source.png");
    let target = directory.path().join("cross-copy-target.png");
    save_png(&source);
    save_png(&target);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.create_mask("Shared").unwrap();
    app.set_mask_local_adjustment("exposure", 1.0).unwrap();
    app.save_sidecar();
    open_and_decode_switch(&mut app, &target.display().to_string());
    app.filmstrip_selection.clear();
    app.filmstrip_selection.insert(target.display().to_string());
    app.previous_reference.as_mut().unwrap().copy_id = "vc-other".into();

    let report = app.apply_previous_to_selection();
    assert_eq!(report.applied_count(), 0);
    assert_eq!(report.failed_count(), 1);
    assert!(report.failed[0].1.contains("cross-copy mask transfer"));
    assert!(!lumina_sidecar::sidecar_path_for(&target).exists());
}
