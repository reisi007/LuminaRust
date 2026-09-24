//! R5-BRUSH-24 mask-management persistence, copy isolation and failure tests.

use super::*;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

#[test]
fn mask_management_operations_persist_and_reload() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("mask-management.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let first = app.create_mask("First").unwrap();
    let second = app.create_mask("Second").unwrap();
    let third = app.create_mask("Third").unwrap();

    app.rename_mask(&first, "Renamed").unwrap();
    assert!(app.move_mask(&first, 1).unwrap());
    assert!(app.move_mask(&first, -1).unwrap());
    assert!(
        !app.move_mask(&first, -1).unwrap(),
        "the first row cannot wrap upward"
    );
    let copy_id = app.duplicate_mask(&first, "First copy").unwrap();
    app.set_mask_visible(&copy_id, false).unwrap();
    app.delete_mask(&copy_id).unwrap();

    let reopened = reopen_app(&source);
    let reloaded_copy = reopened.document.as_ref().unwrap().virtual_copies[0].clone();
    let ids: Vec<&str> = reloaded_copy
        .mask_library
        .iter()
        .map(|mask| mask.id.as_str())
        .collect();
    assert_eq!(ids, vec![first.as_str(), second.as_str(), third.as_str()]);
    assert_eq!(reloaded_copy.mask_library[0].name, "Renamed");
    assert!(!reloaded_copy
        .mask_layers
        .iter()
        .any(|layer| layer.mask.mask_id == copy_id));
}

#[test]
fn management_save_failure_is_visible_and_rolls_back() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("management-conflict.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let id = app.create_mask("Before conflict").unwrap();
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let hook_path = sidecar.clone();
    let attempts = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let hook_attempts = attempts.clone();
    crate::sidecar_rebase::set_conflict_hook(Some(Box::new(move |attempt| {
        hook_attempts.set(hook_attempts.get() + 1);
        let mut disk = lumina_sidecar::load_sidecar(&hook_path).unwrap();
        disk.virtual_copies[0]
            .recipe
            .adjustments
            .insert("shadows".into(), 0.25 + f64::from(attempt as u32) / 100.0);
        lumina_sidecar::save_sidecar(&hook_path, &disk).unwrap();
    })));
    let result = app.rename_mask(&id, "Should not commit");
    crate::sidecar_rebase::set_conflict_hook(None);
    assert!(
        result.is_err(),
        "a persistent CAS conflict must be returned"
    );
    assert!(
        app.error().is_some(),
        "the failed management save must be visible"
    );
    assert!(attempts.get() > 0);
    let name = app.document.as_ref().unwrap().virtual_copies[0]
        .mask_library
        .iter()
        .find(|mask| mask.id == id)
        .unwrap()
        .name
        .clone();
    assert_eq!(
        name, "Before conflict",
        "in-memory operation must roll back"
    );
}

#[test]
fn visibility_save_failure_rolls_back_the_eye_operation() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("visibility-conflict.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let id = app.create_mask("Visible before conflict").unwrap();
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let hook_path = sidecar.clone();
    let attempts = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let hook_attempts = attempts.clone();
    crate::sidecar_rebase::set_conflict_hook(Some(Box::new(move |attempt| {
        hook_attempts.set(hook_attempts.get() + 1);
        let mut disk = lumina_sidecar::load_sidecar(&hook_path).unwrap();
        disk.virtual_copies[0]
            .recipe
            .adjustments
            .insert("shadows".into(), 0.25 + f64::from(attempt as u32) / 100.0);
        lumina_sidecar::save_sidecar(&hook_path, &disk).unwrap();
    })));
    let result = app.set_mask_visible(&id, false);
    crate::sidecar_rebase::set_conflict_hook(None);

    assert!(result.is_err(), "persistent CAS conflict must be returned");
    assert!(app.error().is_some(), "save failure must remain visible");
    assert!(app.mask_visible(&id), "eye state must roll back in memory");
    let persisted = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    assert!(persisted.virtual_copies[0]
        .mask_layers
        .iter()
        .find(|layer| layer.mask.mask_id == id)
        .is_some_and(|layer| layer.visible));
    assert!(attempts.get() > 0);
}

#[cfg(feature = "gpu")]
#[test]
fn a_brush_can_replace_a_range_prompt_without_range_rasterization() {
    let mut app = new_app();
    app.load_bytes(LuminaApp::sample_image_png(), "brush-over-range.png")
        .unwrap();
    let id = app
        .create_luminance_range_mask(0.25, 0.75, 0.0, "Range first")
        .unwrap();
    app.select_mask(&id).unwrap();
    let (_, rebuilt) = app
        .stamp_live_brush_mark(BrushMark {
            x: 0.5,
            y: 0.5,
            radius: 1.0,
            sign: BrushMarkSign::Positive,
            softness: 0.0,
            flow: 1.0,
        })
        .expect("the first brush dab must not require a range rasterizer");
    assert!(rebuilt);
}

#[test]
#[ignore = "native wgpu adapter required; run: cargo test -p lumina-gui --lib mask_management_controls_have_a_representative_kittest_golden -- --ignored"]
fn mask_management_controls_have_a_representative_kittest_golden() {
    // R5-BRUSH-24 native visual gate: the two real production sub-panels are
    // rendered together at the reference 1024x720 viewport. The adapter is
    // intentionally required for the pixel snapshot; normal CPU/no-adapter
    // tests use the structural companion in `brush_management_ui` instead of
    // invoking this path.
    let mut app = new_app();
    app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .unwrap();
    let subject = app
        .create_luminance_range_mask(0.0, 0.5, 0.0, "Subject")
        .unwrap();
    let background = app
        .create_luminance_range_mask(0.5, 1.0, 0.0, "Background")
        .unwrap();
    app.rename_mask(&subject, "Subject renamed").unwrap();
    assert!(app.move_mask(&subject, 1).unwrap());
    let copy = app.duplicate_mask(&subject, "Subject copy").unwrap();
    app.set_mask_visible(&copy, false).unwrap();
    app.set_pin_visibility(PinVisibility::Always);
    app.set_brush_radius(0.25).unwrap();
    app.set_brush_softness(0.40).unwrap();
    app.set_brush_flow(0.70).unwrap();
    app.set_mask_tool(MaskTool::Brush);
    assert_ne!(background, subject);

    let mut harness = Harness::builder()
        .with_size([1024.0_f32, 720.0_f32])
        .wgpu()
        .build_ui_state(
            |ui, app: &mut LuminaApp| {
                super::brush_management_ui::draw_representative_management_surface(app, ui)
            },
            app,
        );
    harness.run();
    for label in [
        Str::MaskEye.t(),
        Str::MoveMaskUp.t(),
        Str::MoveMaskDown.t(),
        Str::RenameMask.t(),
        Str::DeleteMaskButton.t(),
        Str::DuplicateMask.t(),
        Str::DuplicateGroup.t(),
        Str::BrushSize.t(),
        Str::BrushSoftness.t(),
        Str::BrushFlow.t(),
    ] {
        assert!(
            harness.query_all_by_label(label).next().is_some(),
            "representative native golden must paint {label:?}"
        );
    }
    harness.snapshot("mask_management_controls");
}

#[test]
fn copy_selection_and_reload_ignore_cross_copy_layers() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("cross-copy-layers.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let local_id = app.create_mask("Local").unwrap();
    let target_copy = app.duplicate_active_copy().unwrap();
    // The cloned graph layers intentionally remain cross-copy references;
    // materialize a local layer so the fixture exercises selection precedence.
    app.select_virtual_copy(&target_copy).unwrap();
    assert_eq!(app.selected_mask_id(), None);
    app.select_mask(&local_id).unwrap();

    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let mut document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    for copy in &mut document.virtual_copies {
        let local_layer = copy
            .mask_layers
            .iter()
            .find(|layer| layer.mask.copy_id == copy.id && layer.mask.mask_id == local_id)
            .cloned()
            .expect("each copied mask has a local layer");
        let foreign = MaskLayer {
            id: "foreign-first".into(),
            mask: MaskReference {
                copy_id: if copy.id == "vc-original" {
                    target_copy.clone()
                } else {
                    "vc-original".into()
                },
                mask_id: local_id.clone(),
                extras: BTreeMap::new(),
            },
            inverted: true,
            feather: 0.0,
            blur: 0.0,
            density: 1.0,
            extras: BTreeMap::new(),
            visible: true,
        };
        copy.mask_layers.insert(0, foreign);
        assert!(copy
            .mask_layers
            .iter()
            .skip(1)
            .any(|layer| *layer == local_layer));
    }
    assert!(
        document.validate().is_ok(),
        "cross-copy layer sidecar must be valid"
    );
    lumina_sidecar::save_sidecar(&sidecar, &document).unwrap();
    drop(app);

    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert_eq!(reopened.selected_mask_id(), Some(local_id.as_str()));
    reopened.select_virtual_copy(&target_copy).unwrap();
    assert_eq!(reopened.selected_mask_id(), Some(local_id.as_str()));
}

#[test]
fn mask_constructor_and_management_flows_use_one_checked_write() {
    for case in ["create", "ai", "luminance", "color", "combine", "duplicate"] {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join(format!("{case}-write-count.png"));
        save_png(&source);
        let mut app = new_app();
        open_and_decode(&mut app, source.display().to_string());

        // Establish any input needed by the operation before counting. The
        // operation itself must now perform exactly one CAS attempt.
        let mut input = None;
        if matches!(case, "combine" | "duplicate") {
            input = Some(app.create_mask("input-mask").unwrap());
        }
        let attempts = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let hook_attempts = attempts.clone();
        crate::sidecar_rebase::set_conflict_hook(Some(Box::new(move |_| {
            hook_attempts.set(hook_attempts.get() + 1);
        })));
        let result = match case {
            "create" => app.create_mask("created-mask").map(|_| ()),
            "ai" => app
                .create_ai_mask(AiSelectKind::Subject, None, "ai-mask")
                .map(|_| ()),
            "luminance" => app
                .create_luminance_range_mask(0.1, 0.9, 0.2, "luminance-mask")
                .map(|_| ()),
            "color" => app
                .create_color_range_mask(10.0, 20.0, 0.1, 0.9, 0.1, 0.9, 0.2, "color-mask")
                .map(|_| ()),
            "combine" => app
                .combine_masks(MaskOperation::Invert, "", "combined-mask")
                .map(|_| ()),
            "duplicate" => app
                .duplicate_mask(input.as_deref().unwrap(), "copied-mask")
                .map(|_| ()),
            _ => unreachable!(),
        };
        crate::sidecar_rebase::set_conflict_hook(None);
        result.unwrap();
        assert_eq!(attempts.get(), 1, "{case} must use one sidecar write");
    }
}

#[test]
fn failed_mask_constructor_restores_definition_selection_layer_and_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("constructor-failure.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let _seed = app.create_mask("seed-mask").unwrap();
    let document_before = app.document.clone();
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let before = std::fs::read(&sidecar).unwrap();
    let selected_before = app.selected_mask_id().map(str::to_owned);
    let attempts = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let hook_attempts = attempts.clone();
    crate::sidecar_rebase::set_save_failure_hook(Some(Box::new(move |_| {
        hook_attempts.set(hook_attempts.get() + 1);
        Some(lumina_sidecar::SidecarError::Io {
            operation: "test constructor".into(),
            path: "constructor".into(),
            message: "injected failure".into(),
        })
    })));
    let result = app.create_ai_mask(AiSelectKind::Subject, None, "must-roll-back");
    crate::sidecar_rebase::set_save_failure_hook(None);

    assert!(result.is_err());
    assert_eq!(
        attempts.get(),
        1,
        "failed constructor must attempt one write"
    );
    assert_eq!(std::fs::read(&sidecar).unwrap(), before);
    assert_eq!(app.document, document_before);
    assert_eq!(app.selected_mask_id().map(str::to_owned), selected_before);
    let copy = &app.document.as_ref().unwrap().virtual_copies[0];
    assert!(!copy
        .mask_library
        .iter()
        .any(|mask| mask.name == "must-roll-back"));
    assert!(app.error().is_some(), "save failure must remain visible");
}

#[test]
fn brush_prompt_failure_is_checked_and_rolls_back_prompt_and_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("prompt-failure.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let id = app.create_mask("Prompt target").unwrap();
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let before = std::fs::read(&sidecar).unwrap();
    let attempts = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let hook_attempts = attempts.clone();
    crate::sidecar_rebase::set_save_failure_hook(Some(Box::new(move |_| {
        hook_attempts.set(hook_attempts.get() + 1);
        Some(lumina_sidecar::SidecarError::Io {
            operation: "test prompt".into(),
            path: "prompt".into(),
            message: "injected failure".into(),
        })
    })));
    let result = app.commit_brush_stroke(vec![BrushMark {
        x: 0.5,
        y: 0.5,
        radius: 0.1,
        sign: BrushMarkSign::Positive,
        softness: 0.0,
        flow: 1.0,
    }]);
    crate::sidecar_rebase::set_save_failure_hook(None);

    assert!(result.is_err(), "a failed CAS/IO save must not return Ok");
    assert_eq!(attempts.get(), 1, "brush commit must use one checked save");
    assert_eq!(std::fs::read(&sidecar).unwrap(), before);
    let copy = &app.document.as_ref().unwrap().virtual_copies[0];
    let mask = copy.mask_library.iter().find(|mask| mask.id == id).unwrap();
    assert!(
        mask.prompt.is_none(),
        "failed prompt must roll back in memory"
    );
    assert!(app.error().is_some());
    assert_ne!(app.status(), Str::MaskPromptSaved.t());
}
