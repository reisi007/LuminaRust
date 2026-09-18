use super::*;

/// F-019: `validate --migrate` must run through
/// `lumina_sidecar::migrate_sidecar_file` — the `.bak` backup and the
/// released write lock are the observable proof (the old CLI path used
/// `migrate_json` + `write_atomically` and created neither).
#[test]
fn cli_migrate_flag_uses_library_path_and_creates_backup() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let (_, frame) = png_input(directory.path(), "input.png", 90);
    drop(frame);
    let path = write_legacy_sidecar(&input);
    let original = fs::read(&path).unwrap();

    validate(IndexArgs {
        input: path.clone(),
        json: false,
        migrate: true,
    })
    .unwrap();

    // The library migration wrote a `.bak` containing the pre-migration
    // bytes verbatim…
    let bak = path.with_file_name("input.png.lumina.json.bak");
    assert!(bak.is_file(), "migration must create a `.bak` backup");
    assert_eq!(fs::read(&bak).unwrap(), original);
    // …and the live sidecar is migrated to the current schema.
    let document = load_sidecar(&path).unwrap();
    assert_eq!(document.schema_version, lumina_sidecar::SCHEMA_VERSION);
    // The per-sidecar write lock is released after the migration.
    assert!(
        !path.with_file_name(".input.png.lumina.json.lock").exists(),
        "the write lock must be released after the migration"
    );
}

/// F-019: a fresh (non-stale) per-sidecar lock must make the CLI migration
/// fail with an explicit conflict — never a silent in-place rewrite and
/// never a stolen lock.
#[test]
fn cli_migrate_flag_is_blocked_by_a_fresh_lock() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let (_, frame) = png_input(directory.path(), "input.png", 90);
    drop(frame);
    let path = write_legacy_sidecar(&input);
    let original = fs::read(&path).unwrap();

    let lock_path = path.with_file_name(".input.png.lumina.json.lock");
    fs::write(&lock_path, b"").unwrap();

    let error = validate(IndexArgs {
        input: path.clone(),
        json: false,
        migrate: true,
    })
    .unwrap_err();
    assert!(error.to_string().contains("locked"), "{error}");

    // The locked migration created no backup and touched nothing.
    assert!(!path.with_file_name("input.png.lumina.json.bak").exists());
    assert_eq!(fs::read(&path).unwrap(), original);
    // A fresh lock is never stolen — it survives for the real writer.
    assert!(lock_path.exists(), "fresh lock must survive the contender");
}

/// F-019: an already-current sidecar is a no-op for the CLI migration —
/// no `.bak` is created and the on-disk bytes stay untouched.
#[test]
fn cli_migrate_flag_is_noop_for_current_schema() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let (_, frame) = png_input(directory.path(), "input.png", 90);
    // A current-version sidecar written by the library itself.
    let bytes = fs::read(&input).unwrap();
    let document = SidecarDocument::new(
        source_identity(&input, &bytes, &frame, None).unwrap(),
        "raster-mvp-1",
    );
    let path = sidecar_path_for(&input);
    save_sidecar(&path, &document).unwrap();
    let before = fs::read(&path).unwrap();

    validate(IndexArgs {
        input: path.clone(),
        json: false,
        migrate: true,
    })
    .unwrap();

    assert!(!path.with_file_name("input.png.lumina.json.bak").exists());
    assert_eq!(fs::read(&path).unwrap(), before);
}
