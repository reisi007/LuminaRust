use super::*;
use std::sync::{Arc, Barrier};

#[test]
fn force_never_replaces_a_foreign_target_sidecar() {
    let dir = tempfile::tempdir().unwrap();

    // Regression for the old 30-second mtime reclaim: age the rendezvous
    // file while its guard remains live. The contender must still fail.
    let lock_target = dir.path().join("live-lock.dng");
    let lock_sidecar = sidecar_path_for(&lock_target);
    let lock_name = lock_sidecar.file_name().unwrap().to_string_lossy();
    let lock_path = lock_sidecar.with_file_name(format!(".{lock_name}.lock"));
    let live_guard = acquire_sidecar_write_lock(&lock_sidecar).unwrap();
    fs::File::open(&lock_path)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(31))
        .unwrap();
    let contender_sidecar = lock_sidecar.clone();
    let contender = std::thread::spawn(move || acquire_sidecar_write_lock(&contender_sidecar));
    assert!(matches!(
        contender.join().unwrap(),
        Err(lumina_sidecar::SidecarError::Conflict(_))
    ));

    // Simulate replacement of the rendezvous path while an old guard still
    // owns its open inode. Dropping that guard must not unlink its successor.
    fs::remove_file(&lock_path).unwrap();
    let successor_guard = acquire_sidecar_write_lock(&lock_sidecar).unwrap();
    drop(live_guard);
    assert!(lock_path.exists(), "old guard must preserve successor lock");
    assert!(matches!(
        acquire_sidecar_write_lock(&lock_sidecar),
        Err(lumina_sidecar::SidecarError::Conflict(_))
    ));
    drop(successor_guard);
    drop(acquire_sidecar_write_lock(&lock_sidecar).unwrap());

    // Produce a real, internally consistent bundle for the standard-sidecar
    // spoof and transplant probes below.
    let owned_dir = dir.path().join("owned");
    fs::create_dir(&owned_dir).unwrap();
    let owned_inputs = vec![
        write_gradient(&owned_dir, "x_a.png", 40),
        write_gradient(&owned_dir, "x_b.png", 160),
    ];
    let owned_target = owned_dir.join("victim.dng");
    let owned = run_hdr_with(&owned_inputs, &options_with_output(&owned_target)).unwrap();
    assert!(!owned.cached);
    let owned_sidecar = fs::read(&owned.sidecar_path).unwrap();
    let owned_document = load_sidecar(&owned.sidecar_path).unwrap();
    let owned_dng = fs::read(&owned.dng_path).unwrap();

    // A valid ordinary sidecar with only a forged merge marker is not an
    // ownership proof: it lacks the validated recipe and artifact anchors.
    let mut spoofed_document = owned_document.clone();
    spoofed_document.extras.remove(MERGE_RECIPE_KEY);
    spoofed_document.extras.remove(MERGE_ARTIFACT_KEY);
    spoofed_document
        .extras
        .insert("type".into(), serde_json::Value::from(MERGE_ENVELOPE_TYPE));
    let spoofed_sidecar = spoofed_document.to_json().unwrap().into_bytes();
    SidecarDocument::from_json(std::str::from_utf8(&spoofed_sidecar).unwrap())
        .expect("spoof fixture is a valid standard sidecar");

    let spoofed_dir = dir.path().join("spoofed");
    fs::create_dir(&spoofed_dir).unwrap();
    let spoofed_inputs = vec![
        write_gradient(&spoofed_dir, "x_a.png", 40),
        write_gradient(&spoofed_dir, "x_b.png", 160),
    ];
    let spoofed_target = spoofed_dir.join("victim.dng");
    let spoofed_path = sidecar_path_for(&spoofed_target);
    fs::write(&spoofed_target, b"FOREIGN-DNG-BYTES").unwrap();
    fs::write(&spoofed_path, &spoofed_sidecar).unwrap();
    let before_spoofed_dng = fs::read(&spoofed_target).unwrap();
    let before_spoofed_sidecar = fs::read(&spoofed_path).unwrap();

    // Counter-probe: without force this valid sidecar is also refused.
    let error = run_hdr_with(&spoofed_inputs, &options_with_output(&spoofed_target)).unwrap_err();
    assert!(error.to_string().contains("merge stale"), "{error}");
    assert_eq!(fs::read(&spoofed_target).unwrap(), before_spoofed_dng);
    assert_eq!(fs::read(&spoofed_path).unwrap(), before_spoofed_sidecar);

    let mut forced = options_with_output(&spoofed_target);
    forced.force = true;
    let error = run_hdr_with(&spoofed_inputs, &forced).unwrap_err();
    assert!(error.to_string().contains("merge stale"), "{error}");
    assert_eq!(fs::read(&spoofed_target).unwrap(), before_spoofed_dng);
    assert_eq!(fs::read(&spoofed_path).unwrap(), before_spoofed_sidecar);

    // A genuine sidecar transplanted beside a foreign DNG has the right
    // filename and schema, but its checksum does not match these bytes.
    let transplanted_dir = dir.path().join("transplanted");
    fs::create_dir(&transplanted_dir).unwrap();
    let transplanted_inputs = vec![
        write_gradient(&transplanted_dir, "x_a.png", 40),
        write_gradient(&transplanted_dir, "x_b.png", 160),
    ];
    let transplanted_target = transplanted_dir.join("victim.dng");
    let transplanted_path = sidecar_path_for(&transplanted_target);
    let foreign_dng = vec![0xA5; owned_dng.len()];
    assert_ne!(merge_checksum(&foreign_dng), owned.checksum);
    fs::write(&transplanted_target, &foreign_dng).unwrap();
    fs::write(&transplanted_path, &owned_sidecar).unwrap();
    let before_transplanted_dng = fs::read(&transplanted_target).unwrap();
    let before_transplanted_sidecar = fs::read(&transplanted_path).unwrap();

    let mut forced = options_with_output(&transplanted_target);
    forced.force = true;
    let error = run_hdr_with(&transplanted_inputs, &forced).unwrap_err();
    assert!(error.to_string().contains("checksum"), "{error}");
    assert_eq!(
        fs::read(&transplanted_target).unwrap(),
        before_transplanted_dng
    );
    assert_eq!(
        fs::read(&transplanted_path).unwrap(),
        before_transplanted_sidecar
    );

    // A merge envelope plus recipe/artifact checksums is not enough when
    // the DNG source identity's decode or geometry fingerprint was edited.
    for tamper in ["decoder", "version", "geometry"] {
        let mut tampered_document = owned_document.clone();
        match tamper {
            "decoder" => {
                tampered_document.source.decode_fingerprint.decoder = "foreign-decoder".into();
            }
            "version" => {
                tampered_document.source.decode_fingerprint.version.clear();
            }
            "geometry" => {
                tampered_document
                    .source
                    .decode_fingerprint
                    .parameters
                    .insert("geometry".into(), "1x1".into());
            }
            _ => unreachable!(),
        }
        let tampered_sidecar = tampered_document.to_json().unwrap().into_bytes();
        fs::write(&owned.sidecar_path, &tampered_sidecar).unwrap();
        let before_tampered_dng = fs::read(&owned.dng_path).unwrap();
        let before_tampered_sidecar = fs::read(&owned.sidecar_path).unwrap();

        let mut forced = options_with_output(&owned_target);
        forced.force = true;
        let error = run_hdr_with(&owned_inputs, &forced).unwrap_err();
        assert!(
            error.to_string().contains("merge stale"),
            "{tamper} source identity must be rejected: {error}"
        );
        assert_eq!(fs::read(&owned.dng_path).unwrap(), before_tampered_dng);
        assert_eq!(
            fs::read(&owned.sidecar_path).unwrap(),
            before_tampered_sidecar
        );
    }

    // The historical decoder version is a non-empty metadata value, not an
    // independently verifiable provenance claim; other identity anchors remain
    // coherent, so this version-only change is still forceable.
    let mut version_only_change = owned_document.clone();
    version_only_change.source.decode_fingerprint.version = "other-libraw-build".into();
    fs::write(
        &owned.sidecar_path,
        version_only_change.to_json().unwrap().as_bytes(),
    )
    .unwrap();
    let mut force_version_change = options_with_output(&owned_target);
    force_version_change.force = true;
    assert!(
        !run_hdr_with(&owned_inputs, &force_version_change)
            .unwrap()
            .cached
    );
    fs::write(&owned.sidecar_path, &owned_sidecar).unwrap();

    // Force may rebuild a genuinely missing DNG from its validated intact
    // sidecar, but it must never repair present bytes that fail integrity.
    fs::remove_file(&owned.dng_path).unwrap();
    let mut force_missing = options_with_output(&owned_target);
    force_missing.force = true;
    let rebuilt = run_hdr_with(&owned_inputs, &force_missing).unwrap();
    assert!(!rebuilt.cached);
    assert_eq!(fs::read(&owned.dng_path).unwrap(), owned_dng);

    let intact_sidecar_after_rebuild = fs::read(&owned.sidecar_path).unwrap();
    let damaged_dng = b"damaged-present-dng";
    fs::write(&owned.dng_path, damaged_dng).unwrap();
    let error = run_hdr_with(&owned_inputs, &force_missing).unwrap_err();
    assert!(error.to_string().contains("merge stale"), "{error}");
    assert_eq!(fs::read(&owned.dng_path).unwrap(), damaged_dng);
    assert_eq!(
        fs::read(&owned.sidecar_path).unwrap(),
        intact_sidecar_after_rebuild
    );
    fs::write(&owned.dng_path, &owned_dng).unwrap();
    fs::write(&owned.sidecar_path, &owned_sidecar).unwrap();

    // An intact own bundle stays cached normally and force may regenerate it.
    let cached_dng_before = fs::read(&owned.dng_path).unwrap();
    let cached_sidecar_before = fs::read(&owned.sidecar_path).unwrap();
    let cached = run_hdr_with(&owned_inputs, &options_with_output(&owned_target)).unwrap();
    assert!(cached.cached);
    assert_eq!(fs::read(&owned.dng_path).unwrap(), cached_dng_before);
    assert_eq!(
        fs::read(&owned.sidecar_path).unwrap(),
        cached_sidecar_before
    );
    let mut force_owned = options_with_output(&owned_target);
    force_owned.force = true;
    let regenerated = run_hdr_with(&owned_inputs, &force_owned).unwrap();
    assert!(!regenerated.cached);
    let cached_after_regenerate =
        run_hdr_with(&owned_inputs, &options_with_output(&owned_target)).unwrap();
    assert!(cached_after_regenerate.cached);

    // Exercise the final post-staging cache-hit path deterministically. The
    // staged DNG must be dropped and no sidecar/reclaim temp may be left.
    let staged = publication::stage_dng(&owned_target, &owned_dng).unwrap();
    let staged_path = staged.path().to_path_buf();
    assert!(staged_path.exists());
    let cache_hit_dng_before = fs::read(&owned.dng_path).unwrap();
    let cache_hit_sidecar_before = fs::read(&owned.sidecar_path).unwrap();
    let cached_document = load_sidecar(&owned.sidecar_path).unwrap();
    let post_stage_hit = publish_staged_dng_or_return_cached(
        BundleGate {
            command: "merge-hdr",
            mode: MergeMode::Hdr,
            force: false,
            sidecar_path: &owned.sidecar_path,
            dng_path: &owned.dng_path,
            digest: &cached_after_regenerate.digest,
            residual_px: cached_after_regenerate.residual_px,
            residual_warning: cached_after_regenerate.residual_warning,
        },
        staged,
        &cached_document,
    )
    .unwrap()
    .unwrap();
    assert!(post_stage_hit.cached);
    assert!(!staged_path.exists(), "staged DNG tempfile must be removed");
    assert_eq!(fs::read(&owned.dng_path).unwrap(), cache_hit_dng_before);
    assert_eq!(
        fs::read(&owned.sidecar_path).unwrap(),
        cache_hit_sidecar_before
    );
    let sidecar_file_name = owned.sidecar_path.file_name().unwrap().to_string_lossy();
    let target_file_name = owned_target.file_name().unwrap().to_string_lossy();
    let leftover_temporary = fs::read_dir(&owned_dir)
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(&format!(".{target_file_name}.tmp-"))
                || name.starts_with(&format!(".{sidecar_file_name}.tmp-"))
                || name.contains(".lock.reclaim-")
        });
    assert!(!leftover_temporary, "no staged or reclaim temp may remain");
    drop(acquire_sidecar_write_lock(&owned.sidecar_path).unwrap());

    // Concurrent Lumina writers serialize check and publication: exactly
    // one publishes; the other sees the bundle or loudly conflicts.
    let concurrent_dir = dir.path().join("concurrent");
    fs::create_dir(&concurrent_dir).unwrap();
    let concurrent_inputs = vec![
        write_gradient(&concurrent_dir, "x_a.png", 40),
        write_gradient(&concurrent_dir, "x_b.png", 160),
    ];
    let concurrent_target = concurrent_dir.join("victim.dng");
    let barrier = Arc::new(Barrier::new(3));
    let left_inputs = concurrent_inputs.clone();
    let left_target = concurrent_target.clone();
    let left_barrier = Arc::clone(&barrier);
    let left = std::thread::spawn(move || {
        left_barrier.wait();
        run_hdr_with(&left_inputs, &options_with_output(&left_target))
    });
    let right_inputs = concurrent_inputs.clone();
    let right_target = concurrent_target.clone();
    let right_barrier = Arc::clone(&barrier);
    let right = std::thread::spawn(move || {
        right_barrier.wait();
        run_hdr_with(&right_inputs, &options_with_output(&right_target))
    });
    barrier.wait();
    let outcomes = [left.join().unwrap(), right.join().unwrap()];
    let publications = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, Ok(outcome) if !outcome.cached))
        .count();
    assert_eq!(
        publications, 1,
        "exactly one writer publishes: {outcomes:?}"
    );
    let mut cached = 0;
    let mut conflicts = 0;
    for outcome in &outcomes {
        match outcome {
            Ok(outcome) if outcome.cached => cached += 1,
            Ok(outcome) => assert!(!outcome.cached, "unexpected second publication"),
            Err(error) => {
                assert!(
                    error.to_string().contains("sidecar is locked"),
                    "unexpected concurrent error: {error}"
                );
                conflicts += 1;
            }
        }
    }
    assert_eq!(cached + conflicts, 1, "loser is cached or conflicts");
    let concurrent_document = load_sidecar(&sidecar_path_for(&concurrent_target)).unwrap();
    let concurrent_artifact = stored_merge_artifact(&concurrent_document).unwrap();
    assert_eq!(
        merge_checksum(&fs::read(&concurrent_target).unwrap()),
        concurrent_artifact.checksum
    );
}
