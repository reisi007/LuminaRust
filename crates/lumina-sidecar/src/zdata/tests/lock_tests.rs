use super::*;

#[test]
fn concurrent_appends_serialize_instead_of_losing_updates() {
    // REVIEW-SIDECAR-ZDATA-1 regression: two read-modify-write appends
    // used to race without a lock, so one could silently overwrite the
    // other's freshly appended region (lost update / dangling recipe
    // reference). Under the `.zdata.lock` both regions must survive.
    let directory = tempfile::tempdir().unwrap();
    let path = zdata_path_for(&directory.path().join("image.raw"));
    save_zdata(&path, &ZDataContainer::new(tiles()).unwrap()).unwrap();

    let mut first = repair_region();
    first.id = "repair-a".into();
    let mut second = repair_region();
    second.id = "repair-b".into();

    let p1 = path.clone();
    let t1 = std::thread::spawn(move || append_repair_region(&p1, first));
    let p2 = path.clone();
    let t2 = std::thread::spawn(move || append_repair_region(&p2, second));
    t1.join().unwrap().expect("first append must succeed");
    t2.join().unwrap().expect("second append must succeed");

    let final_container = load_zdata(&path).unwrap();
    assert_eq!(
        final_container.tile_count(),
        4,
        "both tiles AND both appended regions must survive"
    );
    assert!(final_container.repair_region("repair-a").is_ok());
    assert!(final_container.repair_region("repair-b").is_ok());
    // The rendezvous lock file persists so its inode stays stable.
    assert!(directory
        .path()
        .join(".image.raw.lumina.zdata.lock")
        .is_file());
}

#[test]
fn fresh_zdata_lock_blocks_append_and_save_with_explicit_error() {
    let directory = tempfile::tempdir().unwrap();
    let path = zdata_path_for(&directory.path().join("image.raw"));
    save_zdata(&path, &ZDataContainer::new(tiles()).unwrap()).unwrap();
    // A live writer holds a fresh lock.
    let lock_path = directory.path().join(".image.raw.lumina.zdata.lock");
    let lock_file = std::fs::OpenOptions::new().create(true).write(true).open(&lock_path).unwrap();
    lock_file.lock().unwrap();

    let error = append_repair_region(&path, {
        let mut r = repair_region();
        r.id = "blocked".into();
        r
    })
    .unwrap_err();
    assert!(
        matches!(&error, ZDataError::Io { operation, .. } if operation.contains("lock")),
        "append must report the held lock explicitly, got {error}"
    );
    let save_error = save_zdata(&path, &ZDataContainer::new(tiles()).unwrap()).unwrap_err();
    assert!(
        matches!(&save_error, ZDataError::Io { operation, .. } if operation.contains("lock")),
        "plain save must take the same lock, got {save_error}"
    );

    // The bundle is untouched by both rejected writers.
    let unchanged = load_zdata(&path).unwrap();
    assert_eq!(unchanged.tile_count(), 2);
    drop(lock_file);
    // After release the append works again.
    let mut ok_region = repair_region();
    ok_region.id = "after-release".into();
    append_repair_region(&path, ok_region).unwrap();
    assert_eq!(load_zdata(&path).unwrap().tile_count(), 3);
}
