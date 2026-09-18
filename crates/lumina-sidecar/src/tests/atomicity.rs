use super::*;

// =====================================================================
// F-077: Backup / Recovery / Conflict / Data-loss release-gate tests.
//
// These exercise the *failure semantics* of the committed persistence
// machinery (atomic writes, `.bak` snapshots, the explicit migration path,
// the version/validation guards and the revision hash) so that a regression
// in any of them fails the release gate instead of shipping silently.
// =====================================================================

// ----- A) Atomic-write recovery -----

#[test]
fn sidecar_write_is_atomic_against_partial_temp_file() {
    // Simulate a crash mid-write: an orphaned, partial atomic-write
    // temporary left behind in the sidecar directory.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = SidecarDocument::new(source(), "pipeline-1");
    save_sidecar(&path, &document).unwrap();
    let temp = directory.path().join(".image.lumina.json.tmp-crash");
    std::fs::write(&temp, b"{\"schema_version\": 2, \"partial\": ").unwrap();
    backdate(&temp, TEMP_SWEEP_AGE + Duration::from_secs(1));
    assert!(temp.exists());
    // The original sidecar must remain intact and readable; load_sidecar
    // recovers it and sweeps the orphaned temporary.
    assert_eq!(load_sidecar(&path).unwrap(), document);
    assert!(!temp.exists());
}

#[cfg(feature = "zdata")]
#[test]
fn zdata_write_is_atomic_against_partial_temp_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = zdata_path_for(&directory.path().join("image.raw"));
    let container = ZDataContainer::new(f077_tiles()).unwrap();
    save_zdata(&path, &container).unwrap();
    // A crash mid-write leaves a partial temporary with the zdata prefix.
    let temp = directory.path().join(".image.raw.lumina.zdata.tmp-crash");
    std::fs::write(&temp, vec![0u8; 50]).unwrap();
    // The live container is untouched by the orphan and still loads exactly.
    let loaded = load_zdata(&path).unwrap();
    assert_eq!(loaded.tile("subject", 0, 0).unwrap(), f077_tiles()[0]);
}

#[test]
fn migration_creates_bak_before_overwrite_and_bak_is_recoverable() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = SidecarDocument::new(source(), "pipeline-1");
    let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
    value["schema_version"] = Value::from(0);
    let original_bytes = serde_json::to_vec(&value).unwrap();
    std::fs::write(&path, &original_bytes).unwrap();

    assert!(migrate_sidecar_file(&path).unwrap());
    let bak = path.with_file_name("image.lumina.json.bak");
    assert!(bak.is_file());
    // The snapshot is the PRE-migration content, verbatim.
    assert_eq!(std::fs::read(&bak).unwrap(), original_bytes);
    // The live target is now migrated to the current schema.
    assert_eq!(load_sidecar(&path).unwrap().schema_version, SCHEMA_VERSION);
    // REVIEW-SIDECAR-N2: the loader rejects historical v0 documents loudly
    // instead of silently normalizing them.
    let error = load_sidecar(&bak).unwrap_err();
    assert!(
        error.to_string().contains("schema_version 0"),
        "v0 backup must be rejected loudly, got {error}"
    );
    // Recovery from the snapshot goes through the explicit migration
    // path and reproduces exactly the original (pre-migration) content.
    let migrated = migrate_json(&String::from_utf8(original_bytes).unwrap()).unwrap();
    let recovered = SidecarDocument::from_json(&migrated).unwrap();
    assert_eq!(recovered.virtual_copies, document.virtual_copies);
    assert_eq!(recovered.schema_version, SCHEMA_VERSION);
}

// ----- B) Backup / restore -----

#[test]
fn save_sidecar_overwrites_in_place_without_bak() {
    // The released contract: `save_sidecar` is an atomic in-place replace
    // and does NOT leave a `.bak`; the explicit migration path
    // (`migrate_sidecar_file`) is the only backup-bearing operation. This
    // test pins that behavior so a future "silent backup on every save"
    // change is visible.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let mut document = SidecarDocument::new(source(), "pipeline-1");
    document.virtual_copies[0].name = "First".into();
    save_sidecar(&path, &document).unwrap();
    let bak = path.with_file_name("image.lumina.json.bak");
    assert!(!bak.exists(), "save_sidecar must not create a .bak");
    document.virtual_copies[0].name = "Second".into();
    save_sidecar(&path, &document).unwrap();
    assert!(!bak.exists());
    assert_eq!(
        load_sidecar(&path).unwrap().virtual_copies[0].name,
        "Second"
    );
}

#[test]
fn bak_can_restore_previous_valid_state_after_corrupt_write() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = SidecarDocument::new(source(), "pipeline-1");
    let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
    value["schema_version"] = Value::from(0);
    let original_json = serde_json::to_string(&value).unwrap();
    std::fs::write(&path, &original_json).unwrap();
    migrate_sidecar_file(&path).unwrap();
    let bak = path.with_file_name("image.lumina.json.bak");
    assert!(bak.is_file());
    // The live target is subsequently corrupted (e.g. a failed later write).
    std::fs::write(&path, b"{\"schema_version\": 2, \"corrupt\": true").unwrap();
    assert!(load_sidecar(&path).is_err());
    // REVIEW-SIDECAR-N2: the raw v0 snapshot is not silently accepted by
    // the loader; the previous valid state is recovered by explicitly
    // migrating the snapshot's bytes.
    assert!(load_sidecar(&bak).is_err());
    let migrated = migrate_json(&std::fs::read_to_string(&bak).unwrap()).unwrap();
    let recovered = SidecarDocument::from_json(&migrated).unwrap();
    let expected = {
        let mut expected_value: Value = serde_json::from_str(&original_json).unwrap();
        expected_value["schema_version"] = Value::from(SCHEMA_VERSION);
        SidecarDocument::from_json(&serde_json::to_string(&expected_value).unwrap()).unwrap()
    };
    assert_eq!(recovered, expected);
}

#[test]
fn bak_is_written_atomically_not_partial() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = SidecarDocument::new(source(), "pipeline-1");
    let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
    value["schema_version"] = Value::from(0);
    let original_bytes = serde_json::to_vec(&value).unwrap();
    std::fs::write(&path, &original_bytes).unwrap();
    migrate_sidecar_file(&path).unwrap();
    let bak = path.with_file_name("image.lumina.json.bak");
    // The snapshot is byte-exact (never a truncated copy of the original).
    let bak_bytes = std::fs::read(&bak).unwrap();
    assert_eq!(bak_bytes, original_bytes);
    // REVIEW-SIDECAR-N2: it is fully valid, parseable JSON — not a partial
    // file. The raw v0 snapshot is rejected by the loader (loud failure)
    // but parses and migrates cleanly through the explicit path.
    assert!(serde_json::from_str::<Value>(&String::from_utf8(bak_bytes.clone()).unwrap()).is_ok());
    assert!(SidecarDocument::from_json(&String::from_utf8(bak_bytes).unwrap()).is_err());
    assert!(migrate_json(&String::from_utf8(original_bytes).unwrap()).is_ok());
    // No partial `.bak` temporary should linger after the atomic write.
    for entry in std::fs::read_dir(directory.path()).unwrap() {
        let name = entry.unwrap().file_name();
        assert!(
            !name
                .to_string_lossy()
                .starts_with(".image.lumina.json.bak.tmp"),
            "orphaned .bak temporary: {name:?}"
        );
    }
}

// ----- C) Conflict scenarios -----

#[test]
fn concurrent_plain_saves_do_not_corrupt_sidecar() {
    // Two plain `save_sidecar` calls race on the same target. Each writes a
    // private temporary and renames it over the target, so the final file is
    // always one complete serialization — never an interleaved/corrupt one.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let base = SidecarDocument::new(source(), "pipeline-1");
    let mut first = base.clone();
    first.virtual_copies[0].name = "First".into();
    let mut second = base;
    second.virtual_copies[0].name = "Second".into();
    let handles: Vec<_> = [first.clone(), second.clone()]
        .into_iter()
        .map(|doc| {
            let path = path.clone();
            std::thread::spawn(move || save_sidecar(&path, &doc).unwrap())
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let loaded = load_sidecar(&path).unwrap();
    assert!(loaded == first || loaded == second);
}

#[test]
fn newer_schema_version_is_rejected_not_silently_downgraded() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = SidecarDocument::new(source(), "pipeline-1");
    let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
    value["schema_version"] = Value::from(99);
    let bytes = serde_json::to_vec(&value).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    // Reading explicitly rejects the unsupported version.
    assert_eq!(
        load_sidecar(&path).unwrap_err(),
        SidecarError::Invalid(
            "unsupported schema_version 99; explicit migration is required".into()
        )
    );
    // migrate_json also rejects rather than silently downgrading.
    assert_eq!(
        migrate_json(&String::from_utf8(bytes.clone()).unwrap()).unwrap_err(),
        SidecarError::Invalid(
            "unsupported schema_version 99; explicit migration is required".into()
        )
    );
    // A failed migration must not touch the original file at all.
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn invalid_sidecar_is_rejected_not_silently_accepted() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    // Truncated JSON.
    std::fs::write(&path, r#"{"format":"lumina-sidecar","schema_version":2,"#).unwrap();
    assert!(matches!(load_sidecar(&path), Err(SidecarError::Json(_))));
    // Valid JSON but missing the required `virtual_copies`.
    std::fs::write(
        &path,
        r#"{"format":"lumina-sidecar","schema_version":2,"source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"RAW","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","virtual_copies":[],"presets":[]}"#,
    )
    .unwrap();
    assert!(matches!(load_sidecar(&path), Err(SidecarError::Invalid(_))));
    // Missing `schema_version` entirely.
    std::fs::write(
        &path,
        r#"{"format":"lumina-sidecar","source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"RAW","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{"adjustments":{},"options":{}},"mask_library":[],"mask_layers":[],"history":[],"export_records":[]}],"presets":[]}"#,
    )
    .unwrap();
    assert!(matches!(load_sidecar(&path), Err(SidecarError::Invalid(_))));
    // Wrong `format` string.
    let wrong_format = r#"{"format":"other","source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"RAW","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{"adjustments":{},"options":{}},"mask_library":[],"mask_layers":[],"history":[],"export_records":[]}],"presets":[]}"#;
    std::fs::write(&path, wrong_format).unwrap();
    assert!(matches!(load_sidecar(&path), Err(SidecarError::Invalid(_))));
}

// ----- D) Data-loss prevention -----

#[test]
fn deleting_sidecar_does_not_affect_original_image() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("IMG_0001.ARW");
    let original_bytes = b"RAW-PIXEL-DATA-12345";
    std::fs::write(&source_path, original_bytes).unwrap();
    let path = sidecar_path_for(&source_path);
    save_sidecar(&path, &SidecarDocument::new(source(), "pipeline-1")).unwrap();
    assert!(path.exists());
    // Delete ONLY the sidecar.
    std::fs::remove_file(&path).unwrap();
    assert!(!path.exists());
    // The original image is byte-identical; persistence is non-destructive.
    assert_eq!(std::fs::read(&source_path).unwrap(), original_bytes);
}

#[cfg(feature = "zdata")]
#[test]
fn zdata_bundle_can_be_deleted_without_affecting_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("IMG_0001.ARW");
    std::fs::write(&source_path, b"RAW").unwrap();
    let sidecar_path = sidecar_path_for(&source_path);
    let zdata_path = zdata_path_for(&source_path);
    let document = SidecarDocument::new(source(), "pipeline-1");
    save_sidecar(&sidecar_path, &document).unwrap();
    let container = ZDataContainer::new(f077_tiles()).unwrap();
    save_zdata(&zdata_path, &container).unwrap();
    assert!(zdata_path.exists());
    let before = std::fs::read(&sidecar_path).unwrap();
    // Delete ONLY the zdata bundle (mask tile data).
    std::fs::remove_file(&zdata_path).unwrap();
    assert!(!zdata_path.exists());
    // Sidecar metadata is unchanged and still fully readable.
    assert_eq!(std::fs::read(&sidecar_path).unwrap(), before);
    assert!(load_sidecar(&sidecar_path).is_ok());
}

#[cfg(feature = "zdata")]
#[test]
fn partial_zdata_is_detected_not_silent_garbage() {
    let directory = tempfile::tempdir().unwrap();
    let path = zdata_path_for(&directory.path().join("image.raw"));
    let container = ZDataContainer::new(f077_tiles()).unwrap();
    save_zdata(&path, &container).unwrap();
    let full = std::fs::read(&path).unwrap();
    // Truncate the container body so the index/records are incomplete.
    let truncated = &full[..full.len() / 2];
    std::fs::write(&path, truncated).unwrap();
    // A truncated container must be reported, never silently returned as a
    // "valid" container that yields garbage tiles.
    assert!(load_zdata(&path).is_err());
}

// ----- E) Migration safety -----

#[test]
fn migrate_sidecar_file_creates_backup_before_migrating() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = SidecarDocument::new(source(), "pipeline-1");
    let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
    value["schema_version"] = Value::from(0);
    let original_bytes = serde_json::to_vec(&value).unwrap();
    std::fs::write(&path, &original_bytes).unwrap();
    assert!(migrate_sidecar_file(&path).unwrap());
    let bak = path.with_file_name("image.lumina.json.bak");
    assert!(bak.is_file());
    // The backup is the pre-migration original, verbatim.
    assert_eq!(std::fs::read(&bak).unwrap(), original_bytes);
    // The live target is now migrated and differs from the backup.
    let live = std::fs::read(&path).unwrap();
    assert_ne!(live, original_bytes);
    assert_eq!(load_sidecar(&path).unwrap().schema_version, SCHEMA_VERSION);
}

#[test]
fn failed_migration_leaves_original_intact() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = SidecarDocument::new(source(), "pipeline-1");
    let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
    value["schema_version"] = Value::from(99);
    let bytes = serde_json::to_vec(&value).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    // An unsupported version fails the migration...
    assert!(migrate_sidecar_file(&path).is_err());
    // ...without creating a `.bak` or touching the original file.
    assert!(!path.with_file_name("image.lumina.json.bak").exists());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn migrating_already_current_version_is_noop() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = SidecarDocument::new(source(), "pipeline-1");
    let bytes = document.to_json().unwrap();
    std::fs::write(&path, &bytes).unwrap();
    // Already at the current schema: no migration, no writes.
    assert!(!migrate_sidecar_file(&path).unwrap());
    // No backup is created for a no-op.
    assert!(!path.with_file_name("image.lumina.json.bak").exists());
    // And the on-disk content is unchanged.
    assert_eq!(std::fs::read(&path).unwrap(), bytes.as_bytes());
}

// ----- F) Recipe integrity under corruption -----

#[test]
fn corrupted_recipe_json_is_rejected_by_validate() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    // The recipe object is truncated mid-JSON.
    std::fs::write(
        &path,
        r#"{"format":"lumina-sidecar","schema_version":2,"source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"RAW","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{"adjustments":{"exposure":},"options":{}},"mask_library":[],"mask_layers":[],"history":[],"export_records":[]}],"presets":[]}"#,
    )
    .unwrap();
    assert!(matches!(load_sidecar(&path), Err(SidecarError::Json(_))));
}

#[test]
fn recipe_with_out_of_range_adjustment_is_rejected() {
    // A recipe carrying a value outside its validated range must be rejected
    // by `validate()`, and therefore cannot be serialized into a valid
    // sidecar either.
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0]
        .recipe
        .adjustments
        .insert("exposure".into(), 99.0); // outside [-10, 10]
    assert!(matches!(d.validate(), Err(SidecarError::Invalid(_))));
    assert!(d.to_json().is_err());
}

#[test]
fn truncated_recipe_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let valid = SidecarDocument::new(source(), "pipeline-1")
        .to_json()
        .unwrap();
    // Chop the tail so the recipe / virtual_copies section is incomplete.
    let truncated = &valid.as_bytes()[..valid.len() - 30];
    std::fs::write(&path, truncated).unwrap();
    assert!(load_sidecar(&path).is_err());
}

#[test]
fn document_revision_detects_tampered_recipe() {
    // The sidecar-level integrity hash (`document_revision`, a BLAKE3 over
    // the serialized document) is the gate used by the compare-and-swap
    // write. Tampering with the recipe must change the hash, while identical
    // documents must hash identically and stably.
    let mut a = SidecarDocument::new(source(), "pipeline-1");
    a.virtual_copies[0]
        .recipe
        .adjustments
        .insert("exposure".into(), 0.5);
    let mut b = a.clone();
    b.virtual_copies[0]
        .recipe
        .adjustments
        .insert("exposure".into(), -0.5);
    let ra = document_revision(&a).unwrap();
    let rb = document_revision(&b).unwrap();
    assert_ne!(
        ra, rb,
        "tampering with the recipe must change its revision hash"
    );
    assert_eq!(ra, document_revision(&a).unwrap());
    assert_eq!(rb, document_revision(&b).unwrap());
}
