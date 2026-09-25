use super::*;

fn local_app() -> (tempfile::TempDir, LuminaApp, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("local.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.create_mask("Subject").unwrap();
    (directory, app, source)
}

#[test]
fn gui_exposes_all_p0_local_controls_and_validates_exact_ranges() {
    let (_directory, mut app, _source) = local_app();
    app.set_mask_local_adjustment("exposure", 10.0).unwrap();
    app.set_mask_local_adjustment("contrast", -1.0).unwrap();
    app.set_mask_local_adjustment("highlights", 1.0).unwrap();
    app.set_mask_local_adjustment("shadows", -1.0).unwrap();
    assert_eq!(
        app.selected_mask_local_adjustment("exposure").unwrap(),
        Some(10.0)
    );
    assert!(app.set_mask_local_adjustment("exposure", 10.01).is_err());
    assert!(app
        .set_mask_local_adjustment("wb_temperature", 6500.0)
        .is_err());
}

#[test]
fn local_history_snapshot_restores_previous_values_and_reset_is_explicit() {
    let (_directory, mut app, source) = local_app();
    app.set_mask_local_adjustment("exposure", 1.0).unwrap();
    app.save_sidecar();
    let first_history = app.document.as_ref().unwrap().virtual_copies[0]
        .history
        .last()
        .cloned()
        .unwrap();
    assert!(first_history.mask_state().unwrap().is_some());
    app.set_mask_local_adjustment("exposure", 2.0).unwrap();
    app.save_sidecar();
    let second_id = app.document.as_ref().unwrap().virtual_copies[0]
        .history
        .last()
        .unwrap()
        .id
        .clone();
    app.restore_history(&second_id).unwrap();
    assert_eq!(
        app.selected_mask_local_adjustment("exposure").unwrap(),
        Some(1.0)
    );

    app.reset_mask_local_adjustment("exposure").unwrap();
    assert_eq!(
        app.selected_mask_local_adjustment("exposure").unwrap(),
        Some(0.0)
    );
    let _ = source;
}

#[test]
fn local_draft_route_upgrades_to_mask_aware_cpu_and_navigator_refusal_is_visible() {
    let (_directory, mut app, _source) = local_app();
    app.set_mask_local_adjustment("exposure", 1.0).unwrap();
    app.render_draft([800, 600], None).unwrap();
    assert!(
        !app.preview_is_draft,
        "local edits may not use the maskless draft route"
    );

    app.preview_zoom = 2.0;
    assert!(app.navigator_zoomed_overview().is_none());
    assert!(app.status.contains("local mask adjustments"));
}

#[test]
fn reset_to_as_shot_removes_absolute_wb_keys() {
    let (_directory, mut app, _source) = local_app();
    app.set_white_balance_from_point(1.0, 0.5, 0.25).unwrap();
    assert!(app.recipe().adjustments.contains_key("wb_temperature"));
    app.reset_white_balance_to_as_shot().unwrap();
    assert!(!app.recipe().adjustments.contains_key("wb_temperature"));
    assert!(!app.recipe().adjustments.contains_key("wb_tint"));
}

#[test]
fn coalesced_local_edits_keep_the_first_complete_pre_edit_snapshot() {
    let (_directory, mut app, _source) = local_app();
    let initial = app.active_mask_layers_snapshot().unwrap();
    app.set_mask_local_adjustment("exposure", 1.0).unwrap();
    app.set_mask_local_adjustment("exposure", 2.0).unwrap();
    app.reset_mask_local_adjustment("exposure").unwrap();
    app.save_sidecar();

    let entry = app.document.as_ref().unwrap().virtual_copies[0]
        .history
        .last()
        .unwrap()
        .clone();
    let snapshot = entry.mask_state().unwrap().unwrap();
    assert_eq!(snapshot, MaskStateSnapshot::new(initial));
    assert!(snapshot.layers[0].local_adjustments.is_none());

    app.restore_history(&entry.id).unwrap();
    assert_eq!(
        app.selected_mask_local_adjustment("exposure").unwrap(),
        None
    );
}

#[test]
fn failed_local_save_keeps_pending_snapshot_and_retry_is_lossless() {
    let (_directory, mut app, source) = local_app();
    let initial = app.active_mask_layers_snapshot().unwrap();
    app.set_mask_local_adjustment("exposure", 1.0).unwrap();
    let attempts = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let hook_attempts = attempts.clone();
    crate::sidecar_rebase::set_save_failure_hook(Some(Box::new(move |_| {
        hook_attempts.set(hook_attempts.get() + 1);
        Some(lumina_sidecar::SidecarError::Io {
            operation: "test local save".into(),
            path: "local-save".into(),
            message: "injected failure".into(),
        })
    })));
    let first = app.save_sidecar_result();
    crate::sidecar_rebase::set_save_failure_hook(None);

    assert!(first.is_err());
    assert_eq!(attempts.get(), 1);
    assert!(app.pending_history_step.is_some());
    assert_eq!(
        app.pending_mask_state_before.as_deref(),
        Some(initial.as_slice())
    );
    assert!(app.pending_slider_commit.is_some());
    assert_eq!(
        app.document.as_ref().unwrap().virtual_copies[0].mask_layers[0]
            .local_adjustments
            .as_ref()
            .map(|value| value.exposure),
        Some(1.0)
    );

    app.save_sidecar_result().expect("retry must be permitted");
    assert!(app.pending_history_step.is_none());
    assert!(app.pending_mask_state_before.is_none());
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    let entry = document.virtual_copies[0].history.last().unwrap();
    assert_eq!(
        entry.mask_state().unwrap().unwrap(),
        MaskStateSnapshot::new(initial)
    );
}

#[test]
fn masking_reset_and_previous_restore_complete_layer_state() {
    let (_directory, mut app, _source) = local_app();
    app.set_mask_local_adjustment("exposure", 1.0).unwrap();
    app.save_sidecar();
    let before_reset = app.active_mask_layers_snapshot().unwrap();

    app.reset_section(SECTION_MASKING).unwrap();
    assert!(app.active_mask_layers_snapshot().unwrap().is_empty());
    let reset_entry = app.document.as_ref().unwrap().virtual_copies[0]
        .history
        .last()
        .unwrap()
        .clone();
    assert_eq!(
        reset_entry.mask_state().unwrap().unwrap(),
        MaskStateSnapshot::new(before_reset.clone())
    );
    app.restore_history(&reset_entry.id).unwrap();
    assert_eq!(app.active_mask_layers_snapshot().unwrap(), before_reset);
    app.save_sidecar();
    assert!(
        app.error().is_none(),
        "save restored state: {:?}",
        app.error()
    );
    assert!(app.pending_mask_state_before.is_none());

    let before_previous_edit = app.active_mask_layers_snapshot().unwrap();
    app.set_mask_local_adjustment("contrast", 0.5).unwrap();
    assert_eq!(
        app.pending_mask_state_before.as_deref(),
        Some(before_previous_edit.as_slice())
    );
    app.restore_section_previous(SECTION_MASKING).unwrap();
    assert!(
        app.error().is_none(),
        "section previous render/save: {:?}",
        app.error()
    );
    assert_eq!(app.active_mask_layers_snapshot().unwrap(), before_reset);
    let previous_entry = app.document.as_ref().unwrap().virtual_copies[0]
        .history
        .last()
        .unwrap()
        .clone();
    assert_eq!(
        previous_entry.mask_state().unwrap().unwrap(),
        MaskStateSnapshot::new(before_previous_edit.clone())
    );
    app.restore_history(&previous_entry.id).unwrap();
    assert_eq!(
        app.active_mask_layers_snapshot().unwrap(),
        before_previous_edit
    );
}

#[test]
fn virtual_copy_switch_flushes_local_only_edit_and_recaptures_baseline() {
    let (_directory, mut app, source) = local_app();
    app.set_mask_local_adjustment("exposure", 1.0).unwrap();
    app.duplicate_virtual_copy("vc-2", "Copy 2").unwrap();

    app.select_virtual_copy("vc-2").unwrap();
    assert!(app.status().contains("saved first"));
    assert!(app.error().is_none(), "switch: {:?}", app.error());
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert_eq!(
        document.virtual_copies[0].mask_layers[0]
            .local_adjustments
            .as_ref()
            .map(|value| value.exposure),
        Some(1.0)
    );

    app.select_virtual_copy("vc-original").unwrap();
    assert_eq!(
        app.selected_mask_local_adjustment("exposure").unwrap(),
        Some(1.0)
    );
    app.reset_section(SECTION_MASKING).unwrap();
    assert!(app.active_mask_layers_snapshot().unwrap().is_empty());
}

#[test]
fn failed_virtual_copy_flush_keeps_copy_pending_for_retry() {
    let (_directory, mut app, _source) = local_app();
    app.set_mask_local_adjustment("exposure", 1.0).unwrap();
    app.duplicate_virtual_copy("vc-2", "Copy 2").unwrap();
    let attempts = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let hook_attempts = attempts.clone();
    crate::sidecar_rebase::set_save_failure_hook(Some(Box::new(move |_| {
        hook_attempts.set(hook_attempts.get() + 1);
        Some(lumina_sidecar::SidecarError::Io {
            operation: "test copy switch".into(),
            path: "copy-switch".into(),
            message: "injected failure".into(),
        })
    })));
    let failed = app.select_virtual_copy("vc-2");
    crate::sidecar_rebase::set_save_failure_hook(None);

    assert!(failed.is_err());
    assert_eq!(app.virtual_copy_id, "vc-original");
    assert!(app.pending_history_step.is_some());
    assert!(app.pending_mask_state_before.is_some());
    assert!(app.pending_slider_commit.is_some());
    assert_eq!(
        app.selected_mask_local_adjustment("exposure").unwrap(),
        Some(1.0)
    );

    app.select_virtual_copy("vc-2").unwrap();
    assert_eq!(app.virtual_copy_id, "vc-2");
    assert!(app.pending_history_step.is_none());
    assert!(app.pending_mask_state_before.is_none());
}
