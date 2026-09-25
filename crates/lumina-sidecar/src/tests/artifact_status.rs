use super::*;

// ----- REVIEW-SIDECAR-STATUS-1: corrupt artifacts are visible -----

#[cfg(feature = "zdata")]
#[test]
fn artifact_status_verifies_container_content_not_just_existence() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let artifact_dir = root.join("masks");
    std::fs::create_dir_all(&artifact_dir).unwrap();
    let reference = ArtifactReference {
        relative_path: "masks/subject.zdata".into(),
        format: "zdata".into(),
        checksum: "blake3:x".into(),
        width: 2,
        height: 2,
        channels: "u16".into(),
        data_version: "1".into(),
        extras: Extras::new(),
    };
    // Missing: nothing on disk.
    assert_eq!(artifact_status(root, &reference), ArtifactStatus::Missing);
    // Corrupt: an empty file can never be a valid artifact.
    std::fs::write(root.join(&reference.relative_path), b"").unwrap();
    assert_eq!(artifact_status(root, &reference), ArtifactStatus::Corrupt);
    // Available: a fully valid container passes parse + checksum checks.
    let container = ZDataContainer::new(f077_tiles()).unwrap();
    save_zdata(&root.join(&reference.relative_path), &container).unwrap();
    assert_eq!(artifact_status(root, &reference), ArtifactStatus::Available);
    // Corrupt: a flipped payload byte previously counted as Available
    // because only `is_file()` was consulted.
    let bytes = std::fs::read(root.join(&reference.relative_path)).unwrap();
    let header_len = 40usize;
    let mut corrupted = bytes.clone();
    corrupted[header_len] ^= 0xff;
    std::fs::write(root.join(&reference.relative_path), &corrupted).unwrap();
    assert_eq!(
        artifact_status(root, &reference),
        ArtifactStatus::Corrupt,
        "a bit-flipped zdata payload must not count as available"
    );
    // Restore intact bytes, then flip the stored checksum itself.
    let index_offset = u64::from_le_bytes(bytes[16..24].try_into().unwrap()) as usize;
    let record_offset = u64::from_le_bytes(
        bytes[index_offset + 20..index_offset + 28]
            .try_into()
            .unwrap(),
    ) as usize;
    let mut bad_checksum = bytes.clone();
    bad_checksum[record_offset + 36] ^= 1;
    std::fs::write(root.join(&reference.relative_path), &bad_checksum).unwrap();
    assert_eq!(
        artifact_status(root, &reference),
        ArtifactStatus::Corrupt,
        "a broken stored BLAKE3 digest must not count as available"
    );
}

/// REVIEW-SIDECAR-FOLLOWUP-1: a non-empty file shorter than the 8-byte
/// magic used to slip past the failed magic read as `Available`, and a
/// `zdata`-declared file without the magic used to fall through as an
/// unverifiable "opaque" payload.
#[cfg(feature = "zdata")]
#[test]
fn artifact_status_rejects_undersized_and_magicless_declared_zdata() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::create_dir_all(root.join("masks")).unwrap();
    let mut reference = ArtifactReference {
        relative_path: "masks/subject.zdata".into(),
        format: "zdata".into(),
        checksum: "blake3:x".into(),
        width: 2,
        height: 2,
        channels: "u16".into(),
        data_version: "1".into(),
        extras: Extras::new(),
    };
    // Non-empty but smaller than the container magic.
    std::fs::write(root.join(&reference.relative_path), b"LUM").unwrap();
    assert_eq!(
        artifact_status(root, &reference),
        ArtifactStatus::Corrupt,
        "a non-empty <8-byte file can never be a container header"
    );
    // Larger than the magic but without it, while declaring `zdata`: the
    // declared format owns the container, so this is mislabeled — corrupt,
    // not an opaque payload.
    let magicless = b"not-a-lumina-container-payload";
    std::fs::write(root.join(&reference.relative_path), magicless).unwrap();
    assert_eq!(
        artifact_status(root, &reference),
        ArtifactStatus::Corrupt,
        "a format==zdata file without LUMZDATA magic must not count as available"
    );
    // The same bytes under the producer spelling written by lumina-core
    // are covered by the same rule.
    reference.format = "lumina-zdata".into();
    assert_eq!(
        artifact_status(root, &reference),
        ArtifactStatus::Corrupt,
        "the lumina-zdata producer spelling requires the container magic too"
    );
    // The identical bytes under a genuinely opaque format stay available:
    // this crate owns no parser for them (documented limitation).
    reference.format = "opaque".into();
    reference.relative_path = "masks/opaque.bin".into();
    std::fs::write(root.join(&reference.relative_path), magicless).unwrap();
    assert_eq!(
        artifact_status(root, &reference),
        ArtifactStatus::Available,
        "opaque formats remain available once they pass the structural checks"
    );
}

/// REVIEW-SIDECAR-STATUS-1 / REVIEW-SIDECAR-FOLLOWUP-1: runs with and
/// without the `zdata` feature so both `artifact_status` builds enforce
/// the same structural floor.
#[test]
fn empty_artifact_file_is_corrupt_even_without_zdata_support() {
    let directory = tempfile::tempdir().unwrap();
    let artifact_dir = directory.path().join("masks");
    std::fs::create_dir_all(&artifact_dir).unwrap();
    let mut reference = ArtifactReference {
        relative_path: "masks/a.bin".into(),
        format: "opaque".into(),
        checksum: "blake3:x".into(),
        width: 1,
        height: 1,
        channels: "u16".into(),
        data_version: "1".into(),
        extras: Extras::new(),
    };
    std::fs::write(directory.path().join(&reference.relative_path), b"").unwrap();
    assert_eq!(
        artifact_status(directory.path(), &reference),
        ArtifactStatus::Corrupt
    );
    // REVIEW-SIDECAR-FOLLOWUP-1: non-empty but shorter than the container
    // magic — previously misread as `Available` when the magic read failed.
    std::fs::write(directory.path().join(&reference.relative_path), b"ab").unwrap();
    assert_eq!(
        artifact_status(directory.path(), &reference),
        ArtifactStatus::Corrupt,
        "a non-empty <8-byte file must stay corrupt in every build"
    );
    std::fs::write(
        directory.path().join(&reference.relative_path),
        b"opaque-payload",
    )
    .unwrap();
    assert_eq!(
        artifact_status(directory.path(), &reference),
        ArtifactStatus::Available
    );
    // A zdata-declared path without the container magic is corrupt in both
    // builds; with the feature it additionally fails the declared-format
    // rule before any parse would run.
    reference.format = "zdata".into();
    reference.relative_path = "masks/b.zdata".into();
    std::fs::write(
        directory.path().join(&reference.relative_path),
        b"opaque-payload",
    )
    .unwrap();
    assert_eq!(
        artifact_status(directory.path(), &reference),
        ArtifactStatus::Corrupt,
        "a zdata-declared file without LUMZDATA magic must be corrupt"
    );
}

// ----- REVIEW-SIDECAR-N1: migration temporaries are sweepable -----

#[test]
fn migration_leaves_no_stray_temporary_files() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = SidecarDocument::new(source(), "pipeline-1");
    let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
    value["schema_version"] = Value::from(0);
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    migrate_sidecar_file(&path).unwrap();
    for entry in std::fs::read_dir(directory.path()).unwrap() {
        let name = entry.unwrap().file_name();
        assert!(
            !name.to_string_lossy().contains(".tmp"),
            "migration temporary leaked with crate-default prefix: {name:?}"
        );
    }
}

#[test]
fn interrupted_migration_temporary_is_sweepable_by_recovery() {
    // An interrupted migration used to leave `<crate-default>.tmp` files
    // that recover_sidecar could never recognize. With the aligned
    // `.{name}.tmp-` prefix the sweep now cleans them up.
    let directory = tempfile::tempdir().unwrap();
    let bak = directory.path().join("image.lumina.json.bak");
    std::fs::write(&bak, b"{\"schema_version\": 0, \"partial\": ").unwrap();
    let orphaned = directory.path().join(".image.lumina.json.bak.tmp-crash");
    std::fs::write(&orphaned, b"partial migration temp").unwrap();
    backdate(&orphaned, TEMP_SWEEP_AGE + Duration::from_secs(1));
    let report = recover_sidecar(&bak).unwrap();
    assert_eq!(report.removed_temporary_files, 1);
    assert!(!orphaned.exists());
}

// ----- REVIEW-SIDECAR-N2: schema_version 0 requires explicit migration -----

#[test]
fn schema_version_zero_is_rejected_with_migration_hint() {
    let d = SidecarDocument::new(source(), "pipeline-1");
    let mut value: Value = serde_json::from_str(&d.to_json().unwrap()).unwrap();
    value["schema_version"] = Value::from(0);
    let json = serde_json::to_string(&value).unwrap();
    let error = SidecarDocument::from_json(&json).unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("schema_version 0") && message.contains("explicit migration"),
        "loader must reject v0 loudly with a migration hint, got {message}"
    );
    // The explicit migration path still performs the historical bump.
    let migrated = migrate_json(&json).unwrap();
    assert_eq!(
        SidecarDocument::from_json(&migrated)
            .unwrap()
            .schema_version,
        SCHEMA_VERSION
    );
}

// ----- REVIEW-SIDECAR-N3: finite/range validation for local values -----

#[test]
fn mask_layer_local_adjustments_are_range_validated() {
    let build = |feather: f32, blur: f32, density: f32| {
        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].mask_library.push(mask("m"));
        d.virtual_copies[0].mask_layers.push(MaskLayer {
            id: "layer".into(),
            mask: MaskReference {
                copy_id: "vc-original".into(),
                mask_id: "m".into(),
                extras: Extras::new(),
            },
            inverted: false,
            feather,
            blur,
            density,
            extras: Extras::new(),
            visible: true,
            local_adjustments: None,
        });
        d.validate()
    };
    // Defaults of every existing valid sidecar stay accepted.
    assert!(build(0.0, 0.0, 1.0).is_ok());
    assert!(build(0.5, 2.0, 0.25).is_ok());
    // feather/blur must be finite and >= 0.
    assert!(build(-0.1, 0.0, 1.0).is_err());
    assert!(build(f32::NAN, 0.0, 1.0).is_err());
    assert!(build(f32::INFINITY, 0.0, 1.0).is_err());
    // blur must be finite and >= 0.
    assert!(build(0.0, -1.0, 1.0).is_err());
    assert!(build(0.0, f32::NAN, 1.0).is_err());
    // density must be finite within 0..=1.
    assert!(build(0.0, 0.0, 1.5).is_err());
    assert!(build(0.0, 0.0, -0.01).is_err());
    assert!(build(0.0, 0.0, f32::NAN).is_err());
}

#[test]
fn target_luminance_must_be_finite() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.auto_features.target_luminance = f64::NAN;
    assert!(d.validate().is_err());
    d.virtual_copies[0].recipe.auto_features.target_luminance = f64::INFINITY;
    assert!(d.validate().is_err());
    d.virtual_copies[0].recipe.auto_features.target_luminance = 0.42;
    assert!(d.validate().is_ok());
}

#[test]
fn unknown_adjustment_keys_must_be_finite_but_stay_forward_compatible() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    // Unknown keys remain accepted for forward compatibility...
    d.virtual_copies[0]
        .recipe
        .adjustments
        .insert("future_slider".into(), 3.0);
    let json = d.to_json().unwrap();
    assert_eq!(
        SidecarDocument::from_json(&json).unwrap().virtual_copies[0]
            .recipe
            .adjustments["future_slider"],
        3.0
    );
    // ...but NaN/∞ slider states are never meaningful.
    d.virtual_copies[0]
        .recipe
        .adjustments
        .insert("future_slider".into(), f64::NAN);
    assert!(d.validate().is_err());
    d.virtual_copies[0]
        .recipe
        .adjustments
        .insert("future_slider".into(), f64::NEG_INFINITY);
    assert!(d.validate().is_err());
}

#[test]
fn zero_inference_resolution_is_rejected() {
    let mut d = SidecarDocument::new(source(), "p");
    let mut m = mask("zerores");
    m.inference_resolution.width = 0;
    d.virtual_copies[0].mask_library.push(m);
    assert!(d
        .validate()
        .unwrap_err()
        .to_string()
        .contains("inference_resolution"));

    let mut d = SidecarDocument::new(source(), "p");
    let mut m = mask("zerores-h");
    m.inference_resolution.height = 0;
    d.virtual_copies[0].mask_library.push(m);
    assert!(d.validate().is_err());
}

// ----- REVIEW-SIDECAR-N4: rejected mutations roll back -----

#[test]
fn rejected_delete_virtual_copy_leaves_document_unchanged() {
    // vc-original owns a layer that references vc-target's mask. Deleting
    // vc-target would strand that layer, so validation must reject the
    // delete *and* the document must stay byte-for-byte at its prior
    // state (previously the copy had already been moved to the deleted
    // list when validation failed).
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].mask_library.push(mask("original-mask"));
    d.virtual_copies[0].mask_layers.push(MaskLayer {
        id: "original-layer".into(),
        mask: MaskReference {
            copy_id: "vc-target".into(),
            mask_id: "target-mask".into(),
            extras: Extras::new(),
        },
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        extras: Extras::new(),
        visible: true,
        local_adjustments: None,
    });
    d.virtual_copies.push(VirtualCopy {
        id: "vc-target".into(),
        name: "Target".into(),
        is_default: false,
        rating: 0,
        flag: Flag::Unflagged,
        recipe: EditRecipe::default(),
        mask_library: vec![mask("target-mask")],
        mask_layers: vec![],
        history: vec![],
        export_records: vec![],
        extras: Extras::new(),
    });
    let before = d.clone();
    assert!(d.validate().is_ok());

    // Deleting vc-target breaks the surviving layer -> rejected.
    let error = d.delete_virtual_copy("vc-target").unwrap_err();
    assert!(
        error.to_string().contains("unknown copy"),
        "delete must fail on the stranded reference, got {error}"
    );
    assert_eq!(d, before, "a rejected delete must not mutate the document");

    // Deleting vc-original stays forbidden by the explicit guard.
    assert!(d.delete_virtual_copy("vc-original").is_err());
    assert_eq!(d, before);

    // Deleting vc-original's *masks* is not a copy deletion; instead
    // verify the successful path once more: removing the dependent layer
    // makes the same delete legal.
    d.virtual_copies[0].mask_layers.clear();
    d.delete_virtual_copy("vc-target").unwrap();
    assert_eq!(d.virtual_copies.len(), 1);
    assert_eq!(d.deleted_virtual_copies.len(), 1);
    assert_eq!(d.deleted_virtual_copies[0].id, "vc-target");

    // A rejected restore also leaves the document unchanged.
    let after_delete = d.clone();
    d.restore_virtual_copy("does-not-exist").unwrap_err();
    assert_eq!(d, after_delete);
}

#[test]
fn rejected_duplicate_and_rename_leave_document_unchanged() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.duplicate_virtual_copy("vc-original", "vc-copy", "Copy")
        .unwrap();
    let before = d.clone();

    // Duplicate id -> precheck rejects without inserting.
    assert!(d
        .duplicate_virtual_copy("vc-original", "vc-copy", "Other")
        .is_err());
    assert_eq!(d, before);

    // Invalid recipe content in the duplicate -> rollback on validate.
    let mut poisoned_source = d.clone();
    poisoned_source.virtual_copies[0]
        .recipe
        .adjustments
        .insert("exposure".into(), 99.0);
    assert!(poisoned_source
        .duplicate_virtual_copy("vc-original", "vc-poison", "Poison")
        .is_err());

    // Empty rename -> rejected before mutating.
    assert!(d.rename_virtual_copy("vc-copy", "   ").is_err());
    assert_eq!(d, before);

    // Unknown id rename -> rejected.
    assert!(d.rename_virtual_copy("missing", "X").is_err());
    assert_eq!(d, before);
}

// ----- REVIEW-SIDECAR-N5: bounded sidecar reads -----

#[test]
fn oversized_sidecar_file_is_rejected_without_full_read() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("huge.lumina.json");
    {
        let file = fs::File::create(&path).unwrap();
        // Sparse file: reserves length without occupying disk space.
        file.set_len(MAX_SIDECAR_BYTES as u64 + 1).unwrap();
    } // Handle closed here; an explicit `drop()` trips `clippy::drop_non_drop`.
    let error = load_sidecar(&path).unwrap_err();
    assert!(
        matches!(&error, SidecarError::Invalid(message) if message.contains("size limit")),
        "oversized sidecar must be rejected by size, got {error}"
    );
}
