use super::*;

#[test]
fn explicit_file_migration_creates_backup_and_rejects_newer_schema() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = SidecarDocument::new(source(), "pipeline-1");
    let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
    value["schema_version"] = Value::from(0);
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(migrate_sidecar_file(&path).unwrap());
    assert!(path.with_file_name("image.lumina.json.bak").is_file());
    assert_eq!(load_sidecar(&path).unwrap().schema_version, SCHEMA_VERSION);

    value["schema_version"] = Value::from(99);
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(migrate_sidecar_file(&path).is_err());
}

#[test]
fn atomic_compare_and_swap_and_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let d = SidecarDocument::new(source(), "pipeline-1");
    let revision = save_sidecar_if_unchanged(&path, &d, None).unwrap();
    let mut changed = d.clone();
    changed.virtual_copies[0].name = "Changed".into();
    assert!(save_sidecar_if_unchanged(&path, &changed, Some("wrong")).is_err());
    save_sidecar_if_unchanged(&path, &changed, Some(&revision)).unwrap();
    // REVIEW-SIDECAR-TMP-1: only temporaries older than the sweep age are
    // orphaned; a fresh one is treated as a live writer's temporary.
    std::fs::write(
        directory.path().join(".image.lumina.json.tmp-crash"),
        b"partial",
    )
    .unwrap();
    assert!(directory
        .path()
        .join(".image.lumina.json.tmp-crash")
        .exists());
    backdate(
        &directory.path().join(".image.lumina.json.tmp-crash"),
        Duration::from_secs(60),
    );
    assert_eq!(
        load_sidecar(&path).unwrap().virtual_copies[0].name,
        "Changed"
    );
    assert!(!directory
        .path()
        .join(".image.lumina.json.tmp-crash")
        .exists());
}

#[test]
fn recovery_never_promotes_partial_temporary_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let temp = directory.path().join(".image.lumina.json.tmp-crash");
    std::fs::write(&temp, b"{\"partial\": true}").unwrap();
    backdate(&temp, Duration::from_secs(60));
    assert!(matches!(load_sidecar(&path), Err(SidecarError::Missing(_))));
    assert!(!temp.exists());
}

/// REVIEW-SIDECAR-TMP-1 regression: a temporary belonging to a *live*
/// writer (fresh mtime) must survive a concurrent reader's recovery sweep.
#[test]
fn recovery_spares_fresh_temporary_of_live_writer() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = SidecarDocument::new(source(), "pipeline-1");
    save_sidecar(&path, &document).unwrap();
    // Simulate another process mid-save: a fresh temporary with content.
    let live_temp = directory.path().join(".image.lumina.json.tmp-live");
    std::fs::write(&live_temp, b"{\"in-flight\": true").unwrap();
    // A load (which sweeps) must not delete the live writer's temporary...
    let loaded = load_sidecar(&path).unwrap();
    assert_eq!(loaded, document);
    assert!(
        live_temp.exists(),
        "recover_sidecar deleted a live writer's fresh temporary"
    );
    // ...while recover_sidecar reports nothing removed...
    let report = recover_sidecar(&path).unwrap();
    assert_eq!(report.removed_temporary_files, 0);
    assert!(live_temp.exists());
    // ...and once aged past the threshold it is swept again.
    backdate(&live_temp, TEMP_SWEEP_AGE + Duration::from_secs(1));
    let report = recover_sidecar(&path).unwrap();
    assert_eq!(report.removed_temporary_files, 1);
    assert!(!live_temp.exists());
}

#[test]
fn compare_and_swap_detects_external_change() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = SidecarDocument::new(source(), "pipeline-1");
    let revision = save_sidecar_if_unchanged(&path, &document, None).unwrap();
    let mut external = document.clone();
    external.virtual_copies[0].name = "Edited elsewhere".into();
    save_sidecar(&path, &external).unwrap();
    let mut local = document;
    local.virtual_copies[0].name = "Local edit".into();
    assert!(matches!(
        save_sidecar_if_unchanged(&path, &local, Some(&revision)),
        Err(SidecarError::Conflict(_))
    ));
}

#[test]
fn concurrent_compare_and_swap_allows_only_one_writer() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = SidecarDocument::new(source(), "pipeline-1");
    let revision = save_sidecar_if_unchanged(&path, &document, None).unwrap();
    let first_path = path.clone();
    let first_revision = revision.clone();
    let first = std::thread::spawn(move || {
        let mut edited = document.clone();
        edited.virtual_copies[0].name = "first".into();
        save_sidecar_if_unchanged(&first_path, &edited, Some(&first_revision))
    });
    let second_path = path.clone();
    let second_revision = revision;
    let second = std::thread::spawn(move || {
        let mut edited = SidecarDocument::new(source(), "pipeline-1");
        edited.virtual_copies[0].name = "second".into();
        save_sidecar_if_unchanged(&second_path, &edited, Some(&second_revision))
    });
    let results = [first.join().unwrap(), second.join().unwrap()];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(SidecarError::Conflict(_))))
            .count(),
        1
    );
}

#[test]
fn source_and_artifact_conflicts_are_visible() {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("source.raw");
    let bytes = b"source";
    std::fs::write(&source_path, bytes).unwrap();
    let mut identity = source();
    identity.byte_length = bytes.len() as u64;
    identity.content_hash = format!("blake3:{}", blake3::hash(bytes).to_hex());
    assert_eq!(
        source_status(&source_path, &identity).unwrap(),
        SourceStatus::Unchanged
    );
    std::fs::write(&source_path, b"changed").unwrap();
    assert_eq!(
        source_status(&source_path, &identity).unwrap(),
        SourceStatus::SourceChanged
    );
    std::fs::remove_file(&source_path).unwrap();
    assert_eq!(
        source_status(&source_path, &identity).unwrap(),
        SourceStatus::Missing
    );
    let artifact = ArtifactReference {
        relative_path: "masks/a.zdata".into(),
        format: "zdata".into(),
        checksum: "hash".into(),
        width: 1,
        height: 1,
        channels: "u16".into(),
        data_version: "1".into(),
        extras: Extras::new(),
    };
    assert_eq!(
        artifact_status(directory.path(), &artifact),
        ArtifactStatus::Missing
    );
}

#[test]
fn xmp_is_explicitly_unsupported() {
    assert!(!xmp_supported());
    assert!(matches!(
        SidecarError::XmpUnsupported,
        SidecarError::XmpUnsupported
    ));
}
