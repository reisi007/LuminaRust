use super::*;

#[test]
fn import_rejects_changed_source_against_existing_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let (input, _) = png_input(directory.path(), "input.png", 80);
    let args = ImportArgs {
        input: input.clone(),
        json: false,
        migrate: false,
    };
    import_file(args.clone()).unwrap();

    // Change the file contents behind the same path: a second import must
    // fail loudly instead of blessing a sidecar for foreign contents.
    let changed = ImageFrame::new(2, 2, vec![81; 16]).unwrap();
    fs::write(&input, changed.encode(ImageFileFormat::Png).unwrap()).unwrap();
    let error = import_file(args).unwrap_err();
    assert!(error.to_string().contains("source changed"));
}

/// LRPAR-G08-PREVIOUS: `previous` copies the reference recipe onto every
/// target (file → reload), tags one `previous` history step per target
/// and leaves the reference sidecar untouched.
#[test]
fn previous_copies_reference_recipe_to_targets_with_history() {
    let directory = tempfile::tempdir().unwrap();
    let (reference, _) = png_input(directory.path(), "reference.png", 100);
    let (target_a, _) = png_input(directory.path(), "target-a.png", 120);
    let (target_b, _) = png_input(directory.path(), "target-b.png", 140);
    for input in [&reference, &target_a, &target_b] {
        import_file(ImportArgs {
            input: input.clone(),
            json: false,
            migrate: false,
        })
        .unwrap();
    }
    develop(DevelopArgs {
        input: reference.clone(),
        virtual_copy: None,
        exposure: Some(1.5),
        contrast: None,
        treatment: None,
        profile: None,
        update_masks: false,
        migrate: false,
        json: false,
    })
    .unwrap();

    previous(PreviousArgs {
        from: reference.clone(),
        to: vec![target_a.clone(), target_b.clone()],
        from_copy: None,
        to_copy: None,
        json: false,
    })
    .unwrap();

    for target in [&target_a, &target_b] {
        let document = load_sidecar(&sidecar_path_for(target)).unwrap();
        assert!(document.validate().is_ok());
        let copy = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == "vc-original")
            .unwrap();
        assert_eq!(copy.recipe.adjustments["exposure"], 1.5);
        let last = copy.history.last().unwrap();
        assert_eq!(last.id, "previous");
        assert_eq!(last.recipe.adjustments["exposure"], 1.5);
        // Portable by construction: the history extra carries the file
        // name, never a path.
        let source = last.extras["source"].as_str().unwrap();
        assert_eq!(source, "reference.png");
        assert!(!source.contains('/'));
    }
    // The reference sidecar is untouched (no history step added there).
    let reference_document = load_sidecar(&sidecar_path_for(&reference)).unwrap();
    assert!(reference_document.virtual_copies[0].history.is_empty());
}

/// LRPAR-G08-PREVIOUS: one missing target sidecar is a loud per-target
/// failure (exit 3) — the healthy target is still updated.
#[test]
fn previous_reports_per_target_failure_without_aborting_rest() {
    let directory = tempfile::tempdir().unwrap();
    let (reference, _) = png_input(directory.path(), "reference.png", 100);
    let (good, _) = png_input(directory.path(), "good.png", 120);
    for input in [&reference, &good] {
        import_file(ImportArgs {
            input: input.clone(),
            json: false,
            migrate: false,
        })
        .unwrap();
    }
    develop(DevelopArgs {
        input: reference.clone(),
        virtual_copy: None,
        exposure: Some(2.0),
        contrast: None,
        treatment: None,
        profile: None,
        update_masks: false,
        migrate: false,
        json: false,
    })
    .unwrap();
    let missing = directory.path().join("gone.png");

    let error = previous(PreviousArgs {
        from: reference.clone(),
        to: vec![good.clone(), missing],
        from_copy: None,
        to_copy: None,
        json: false,
    })
    .unwrap_err();
    assert!(
        matches!(error, CliError::BatchPartial { failed: 1 }),
        "partial failure must map to exit 3, got {error}"
    );
    assert_eq!(error.exit_code(), 3);
    let document = load_sidecar(&sidecar_path_for(&good)).unwrap();
    let copy = document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == "vc-original")
        .unwrap();
    assert_eq!(copy.recipe.adjustments["exposure"], 2.0);
    assert_eq!(copy.history.last().unwrap().id, "previous");
}

/// LRPAR-G08-PREVIOUS: a missing reference sidecar is a hard error
/// (exit 1) and no target is touched.
#[test]
fn previous_missing_reference_fails_before_touching_targets() {
    let directory = tempfile::tempdir().unwrap();
    let (target, _) = png_input(directory.path(), "target.png", 120);
    import_file(ImportArgs {
        input: target.clone(),
        json: false,
        migrate: false,
    })
    .unwrap();
    let before = fs::read_to_string(sidecar_path_for(&target)).unwrap();

    let error = previous(PreviousArgs {
        from: directory.path().join("gone.png"),
        to: vec![target.clone()],
        from_copy: None,
        to_copy: None,
        json: false,
    })
    .unwrap_err();
    assert!(
        !matches!(error, CliError::BatchPartial { .. }),
        "a missing reference is a hard error, got {error}"
    );
    assert_eq!(error.exit_code(), 1);
    assert_eq!(
        fs::read_to_string(sidecar_path_for(&target)).unwrap(),
        before,
        "no target may be touched without a valid reference"
    );
}

/// LRPAR-G09-LIB: `relocate` moves the image with its sidecar
/// companion; the recipe roundtrips and `inspect` stays valid.
/// The cross-volume helper must move a file with no residue in the
/// same-volume (rename) path. The `CrossesDevices` fallback needs two
/// filesystems and is exercised manually, not unit-testable here.
#[test]
fn move_file_cross_volume_moves_a_file() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("a.bin");
    let target = directory.path().join("b.bin");
    fs::write(&source, b"payload").unwrap();
    move_file_cross_volume(&source, &target).unwrap();
    assert!(!source.exists());
    assert_eq!(fs::read(&target).unwrap(), b"payload");
    // A missing source stays a loud error.
    assert!(move_file_cross_volume(&source, &target).is_err());
}

#[test]
fn relocate_moves_image_with_sidecar_and_roundtrips() {
    let directory = tempfile::tempdir().unwrap();
    let (source, _) = png_input(directory.path(), "photo.png", 100);
    import_file(ImportArgs {
        input: source.clone(),
        json: false,
        migrate: false,
    })
    .unwrap();
    develop(DevelopArgs {
        input: source.clone(),
        virtual_copy: None,
        exposure: Some(1.5),
        contrast: None,
        treatment: None,
        profile: None,
        update_masks: false,
        migrate: false,
        json: false,
    })
    .unwrap();
    let album = directory.path().join("album");
    std::fs::create_dir(&album).unwrap();
    let target = album.join("photo.png");

    relocate(RelocateArgs {
        from: source.clone(),
        to: target.clone(),
        json: false,
    })
    .unwrap();

    assert!(!source.exists(), "the source must be gone");
    assert!(target.is_file(), "the image must sit at the target");
    assert!(!sidecar_path_for(&source).exists());
    let moved = sidecar_path_for(&target);
    assert!(moved.is_file(), "the sidecar must follow the image");
    let document = load_sidecar(&moved).unwrap();
    assert!(document.validate().is_ok());
    assert_eq!(
        document.virtual_copies[0].recipe.adjustments["exposure"],
        1.5
    );
    // Rezept-relevant roundtrip: inspect the moved image.
    inspect(InspectArgs {
        input: target,
        json: true,
    })
    .unwrap();
}

/// LRPAR-G09-LIB (B1): same-directory rename derives companion targets
/// from `--to` — the sidecar follows the new name instead of colliding
/// with the source companion.
#[test]
fn relocate_same_dir_rename_moves_sidecar_to_new_name() {
    let directory = tempfile::tempdir().unwrap();
    let (source, _) = png_input(directory.path(), "photo.png", 100);
    import_file(ImportArgs {
        input: source.clone(),
        json: false,
        migrate: false,
    })
    .unwrap();
    let target = directory.path().join("renamed.png");

    relocate(RelocateArgs {
        from: source.clone(),
        to: target.clone(),
        json: false,
    })
    .unwrap();

    assert!(!source.exists());
    assert!(target.is_file());
    assert!(!sidecar_path_for(&source).exists());
    let moved = sidecar_path_for(&target);
    assert!(moved.is_file(), "sidecar must follow the new name");
    inspect(InspectArgs {
        input: target,
        json: true,
    })
    .unwrap();
}

/// LRPAR-G09-LIB (B1): cross-directory rename with both companions —
/// `.lumina.json` and `.lumina.zdata` land under the target-derived
/// names, never under the source names.
#[test]
fn relocate_cross_dir_rename_moves_json_and_zdata() {
    let directory = tempfile::tempdir().unwrap();
    let (source, _) = png_input(directory.path(), "photo.png", 100);
    import_file(ImportArgs {
        input: source.clone(),
        json: false,
        migrate: false,
    })
    .unwrap();
    let source_zdata = zdata_path_for(&source);
    lumina_sidecar::save_zdata(
        &source_zdata,
        &lumina_sidecar::ZDataContainer::new(vec![]).unwrap(),
    )
    .unwrap();
    let album = directory.path().join("album");
    std::fs::create_dir(&album).unwrap();
    let target = album.join("renamed.png");

    relocate(RelocateArgs {
        from: source.clone(),
        to: target.clone(),
        json: false,
    })
    .unwrap();

    assert!(!source.exists());
    assert!(!sidecar_path_for(&source).exists());
    assert!(!source_zdata.exists());
    assert!(target.is_file());
    let moved_json = sidecar_path_for(&target);
    let moved_zdata = zdata_path_for(&target);
    assert!(moved_json.is_file(), "json must follow the target name");
    assert!(moved_zdata.is_file(), "zdata must follow the target name");
    // No stale source-named companions linger next to the target.
    assert!(!album.join("photo.png.lumina.json").exists());
    assert!(!album.join("photo.png.lumina.zdata").exists());
    inspect(InspectArgs {
        input: target,
        json: true,
    })
    .unwrap();
}

/// LRPAR-G09-LIB: an existing target aborts loudly (exit 1) before
/// anything is moved — never a silent overwrite.
#[test]
fn relocate_refuses_existing_target_without_moving() {
    let directory = tempfile::tempdir().unwrap();
    let (source, _) = png_input(directory.path(), "photo.png", 100);
    let (blocker, _) = png_input(directory.path(), "blocker.png", 120);
    import_file(ImportArgs {
        input: source.clone(),
        json: false,
        migrate: false,
    })
    .unwrap();
    let before = fs::read(&source).unwrap();
    let before_sidecar = fs::read(sidecar_path_for(&source)).unwrap();

    let error = relocate(RelocateArgs {
        from: source.clone(),
        to: blocker,
        json: false,
    })
    .unwrap_err();
    assert_eq!(error.exit_code(), 1);
    assert_eq!(fs::read(&source).unwrap(), before);
    assert_eq!(fs::read(sidecar_path_for(&source)).unwrap(), before_sidecar);
}

/// LRPAR-G09-LIB: a missing source is a loud error (exit 1).
#[test]
fn relocate_missing_source_fails_loudly() {
    let directory = tempfile::tempdir().unwrap();
    let error = relocate(RelocateArgs {
        from: directory.path().join("gone.png"),
        to: directory.path().join("elsewhere.png"),
        json: false,
    })
    .unwrap_err();
    assert_eq!(error.exit_code(), 1);
}
