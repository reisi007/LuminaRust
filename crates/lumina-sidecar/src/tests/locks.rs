use super::*;

// ----- REVIEW-SIDECAR-LOCK-1: atomic stale-lock reclaim (TOCTOU) -----

#[test]
fn stale_lock_reclaim_is_atomic_no_lost_update() {
    use std::sync::{Arc, Barrier};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = SidecarDocument::new(source(), "pipeline-1");
    let revision = save_sidecar_if_unchanged(&path, &document, None).unwrap();
    let lock_path = directory.path().join(".image.lumina.json.lock");
    assert!(!lock_path.exists(), "lock must be released after save");
    // Create a stale lock (mtime 60s ago) that both contenders will see.
    {
        let file = std::fs::File::create(&lock_path).unwrap();
        let old = SystemTime::now() - Duration::from_secs(60);
        file.set_modified(old).unwrap();
    }
    let barrier = Arc::new(Barrier::new(2));
    let path1 = path.clone();
    let rev1 = revision.clone();
    let b1 = Arc::clone(&barrier);
    let t1 = std::thread::spawn(move || {
        b1.wait();
        let mut edited = SidecarDocument::new(source(), "pipeline-1");
        edited.virtual_copies[0].name = "first".into();
        save_sidecar_if_unchanged(&path1, &edited, Some(&rev1))
    });
    let path2 = path.clone();
    let rev2 = revision;
    let b2 = Arc::clone(&barrier);
    let t2 = std::thread::spawn(move || {
        b2.wait();
        let mut edited = SidecarDocument::new(source(), "pipeline-1");
        edited.virtual_copies[0].name = "second".into();
        save_sidecar_if_unchanged(&path2, &edited, Some(&rev2))
    });
    let r1 = t1.join().unwrap();
    let r2 = t2.join().unwrap();
    let successes = [&r1, &r2].iter().filter(|r| r.is_ok()).count();
    let conflicts = [&r1, &r2]
        .iter()
        .filter(|r| matches!(r, Err(SidecarError::Conflict(_))))
        .count();
    assert_eq!(
        successes, 1,
        "exactly one contender must win the atomic stale reclaim, got {r1:?} {r2:?}"
    );
    assert_eq!(
        conflicts, 1,
        "the loser must receive an explicit Conflict, not a silent lost update"
    );
    // No stale reclaim artifact must remain.
    let reclaim_leftover = std::fs::read_dir(directory.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_string_lossy().contains("reclaim"));
    assert!(
        !reclaim_leftover,
        "no .reclaim temporary must survive after atomic reclaim"
    );
    // Sidecar must be one of the two valid outcomes, not corrupted.
    let loaded = load_sidecar(&path).unwrap();
    assert!(loaded.virtual_copies[0].name == "first" || loaded.virtual_copies[0].name == "second");
    // Lock must be released after the winner's WriteLock is dropped.
    assert!(!lock_path.exists(), "lock must be cleaned up after winner");
    // No absolute path leaked.
    assert!(!loaded.source.relative_name.contains('/'));
}

#[test]
fn fresh_lock_is_not_stolen_and_yields_explicit_conflict() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = SidecarDocument::new(source(), "pipeline-1");
    let revision = save_sidecar_if_unchanged(&path, &document, None).unwrap();
    let lock_path = directory.path().join(".image.lumina.json.lock");
    // Create a fresh lock (mtime = now) – must NOT be reclaimed.
    std::fs::File::create(&lock_path).unwrap();
    let mut edited = SidecarDocument::new(source(), "pipeline-1");
    edited.virtual_copies[0].name = "contender".into();
    let result = save_sidecar_if_unchanged(&path, &edited, Some(&revision));
    assert!(
        matches!(result, Err(SidecarError::Conflict(_))),
        "fresh lock must produce explicit Conflict, got {result:?}"
    );
    // Fresh lock must survive the failed reclaim attempt (not silently deleted).
    assert!(
        lock_path.exists(),
        "fresh lock must not have been deleted by contender"
    );
    // Winner's sidecar is unchanged.
    let loaded = load_sidecar(&path).unwrap();
    assert_eq!(loaded.virtual_copies[0].name, "Original");
    let _ = std::fs::remove_file(&lock_path);
}

#[test]
fn concurrent_fresh_lock_only_one_writer_wins() {
    use std::sync::{Arc, Barrier};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = SidecarDocument::new(source(), "pipeline-1");
    let revision = save_sidecar_if_unchanged(&path, &document, None).unwrap();
    let lock_path = directory.path().join(".image.lumina.json.lock");
    // Hold a fresh lock in a background thread for ~300ms to simulate a
    // concurrent writer that has not yet released its lock.
    let holder_path = lock_path.clone();
    let holder = std::thread::spawn(move || {
        std::fs::File::create(&holder_path).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let _ = std::fs::remove_file(&holder_path);
    });
    // Give holder a head start so its fresh lock is visible.
    std::thread::sleep(Duration::from_millis(20));
    assert!(lock_path.exists());
    let barrier = Arc::new(Barrier::new(2));
    let p1 = path.clone();
    let r1 = revision.clone();
    let b1 = Arc::clone(&barrier);
    let t1 = std::thread::spawn(move || {
        b1.wait();
        let mut edited = SidecarDocument::new(source(), "pipeline-1");
        edited.virtual_copies[0].name = "contender-a".into();
        save_sidecar_if_unchanged(&p1, &edited, Some(&r1))
    });
    let p2 = path.clone();
    let r2 = revision;
    let b2 = Arc::clone(&barrier);
    let t2 = std::thread::spawn(move || {
        b2.wait();
        let mut edited = SidecarDocument::new(source(), "pipeline-1");
        edited.virtual_copies[0].name = "contender-b".into();
        save_sidecar_if_unchanged(&p2, &edited, Some(&r2))
    });
    let a = t1.join().unwrap();
    let b = t2.join().unwrap();
    holder.join().unwrap();
    // Both contenders raced against the holder's fresh lock; at least one
    // must have seen an explicit Conflict. Neither may have silently stolen
    // the fresh lock.
    let conflicts = [&a, &b]
        .iter()
        .filter(|r| matches!(r, Err(SidecarError::Conflict(_))))
        .count();
    assert!(
        conflicts >= 1,
        "at least one contender must get Conflict against fresh lock, got {a:?} {b:?}"
    );
    // If one contender won after holder released, the sidecar is valid; if
    // both lost, the original remains. No lost update: sidecar is never
    // corrupted or partially written.
    let loaded = load_sidecar(&path).unwrap();
    assert!(loaded.validate().is_ok());
}

// =====================================================================
// REVIEW-SIDECAR batch: lock serialization, artifact verification,
// migration temp prefixes, v0 rejection, range validation, mutation
// rollback and bounded sidecar reads.
// =====================================================================

// ----- REVIEW-SIDECAR-CAS-1: plain saves serialize against CAS -----

#[test]
fn serialized_writes_reject_concurrent_lock_holder() {
    // A fresh lock means another writer is mid-save. Both a plain
    // `save_sidecar` and a compare-and-swap must report an explicit
    // Conflict instead of writing concurrently (previously only the CAS
    // path locked, so a plain save could silently overwrite an in-flight
    // compare-and-swap result).
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = SidecarDocument::new(source(), "pipeline-1");
    let revision = save_sidecar_if_unchanged(&path, &document, None).unwrap();
    let lock_path = directory.path().join(".image.lumina.json.lock");
    std::fs::File::create(&lock_path).unwrap();

    let mut edited = document.clone();
    edited.virtual_copies[0].name = "plain".into();
    let plain = save_sidecar(&path, &edited);
    assert!(
        matches!(&plain, Err(SidecarError::Conflict(message)) if message.contains("locked")),
        "plain save must not bypass the write lock, got {plain:?}"
    );

    let mut cas_edited = document.clone();
    cas_edited.virtual_copies[0].name = "cas".into();
    let cas = save_sidecar_if_unchanged(&path, &cas_edited, Some(&revision));
    assert!(
        matches!(cas, Err(SidecarError::Conflict(_))),
        "CAS must see the same lock, got {cas:?}"
    );

    // The holder's sidecar is untouched by both rejected writers.
    assert_eq!(
        load_sidecar(&path).unwrap().virtual_copies[0].name,
        "Original"
    );
    std::fs::remove_file(&lock_path).unwrap();
    // After the lock is released both writers work again.
    save_sidecar(&path, &edited).unwrap();
    assert_eq!(load_sidecar(&path).unwrap().virtual_copies[0].name, "plain");
}

#[test]
fn mixed_plain_and_cas_writes_stay_serialized() {
    // Threads racing plain saves against a compare-and-swap must produce
    // exactly one complete document — never a torn or lost mixture.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = SidecarDocument::new(source(), "pipeline-1");
    let revision = save_sidecar_if_unchanged(&path, &document, None).unwrap();
    let mut plain_doc = document.clone();
    plain_doc.virtual_copies[0].name = "plain-winner".into();
    let mut cas_doc = document.clone();
    cas_doc.virtual_copies[0].name = "cas-writer".into();

    let plain_path = path.clone();
    let plain_payload = plain_doc.clone();
    let plain_thread = std::thread::spawn(move || save_sidecar(&plain_path, &plain_payload));
    let cas_path = path.clone();
    let cas_revision = revision;
    let cas_payload = cas_doc.clone();
    let cas_thread = std::thread::spawn(move || {
        save_sidecar_if_unchanged(&cas_path, &cas_payload, Some(&cas_revision))
    });
    let plain_result = plain_thread.join().unwrap();
    let cas_result = cas_thread.join().unwrap();
    plain_result.unwrap();
    // The CAS either won before the plain save or lost with an explicit
    // Conflict; a silent lost update is forbidden.
    assert!(cas_result.is_ok() || matches!(cas_result, Err(SidecarError::Conflict(_))));
    let loaded = load_sidecar(&path).unwrap();
    assert!(loaded == plain_doc || loaded == cas_doc);
}
