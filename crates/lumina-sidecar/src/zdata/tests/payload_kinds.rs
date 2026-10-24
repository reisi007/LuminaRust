use super::*;

// =====================================================================
// GEN-ZDATA-PERSIST: generative canvas (kind 2) + spot heal (kind 3).
// =====================================================================

fn generative_canvas() -> GenerativeCanvasArtifact {
    GenerativeCanvasArtifact {
        id: "gen-canvas-1".into(),
        width: 2,
        height: 2,
        pixels: vec![
            10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255,
        ],
    }
}

fn spot_heal() -> SpotHealGenerativeArtifact {
    SpotHealGenerativeArtifact {
        id: "spot-heal-1".into(),
        width: 2,
        height: 1,
        pixels: vec![1, 2, 3, 255, 4, 5, 6, 255],
    }
}

#[test]
fn generative_canvas_roundtrip_through_bundle() {
    let canvas = generative_canvas();
    let container =
        ZDataContainer::new_with(vec![RecordSpec::GenerativeCanvas(canvas.clone())]).unwrap();
    let decoded = ZDataContainer::from_bytes(container.to_bytes()).unwrap();
    assert_eq!(decoded.tile_count(), 1);
    assert_eq!(decoded.generative_canvas("gen-canvas-1").unwrap(), canvas);
    // Strict kind separation: never visible as tile / repair / spot heal.
    assert!(decoded.tile("gen-canvas-1", 0, 0).is_err());
    assert!(decoded.repair_region("gen-canvas-1").is_err());
    assert!(decoded.spot_heal_generative("gen-canvas-1").is_err());
}

#[test]
fn spot_heal_generative_roundtrip_through_bundle() {
    let spot = spot_heal();
    let container =
        ZDataContainer::new_with(vec![RecordSpec::SpotHealGenerative(spot.clone())]).unwrap();
    let decoded = ZDataContainer::from_bytes(container.to_bytes()).unwrap();
    assert_eq!(decoded.tile_count(), 1);
    assert_eq!(decoded.spot_heal_generative("spot-heal-1").unwrap(), spot);
    assert!(decoded.tile("spot-heal-1", 0, 0).is_err());
    assert!(decoded.repair_region("spot-heal-1").is_err());
    assert!(decoded.generative_canvas("spot-heal-1").is_err());
}

#[test]
fn generative_kinds_coexist_with_tiles_and_repairs() {
    let container = ZDataContainer::new_with(vec![
        RecordSpec::MaskTile(tiles()[0].clone()),
        RecordSpec::RepairRegion(repair_region()),
        RecordSpec::GenerativeCanvas(generative_canvas()),
        RecordSpec::SpotHealGenerative(spot_heal()),
    ])
    .unwrap();
    let decoded = ZDataContainer::from_bytes(container.to_bytes()).unwrap();
    assert_eq!(decoded.tile_count(), 4);
    assert_eq!(decoded.tile("subject", 0, 0).unwrap(), tiles()[0]);
    assert_eq!(decoded.repair_region("repair-1").unwrap(), repair_region());
    assert_eq!(
        decoded.generative_canvas("gen-canvas-1").unwrap(),
        generative_canvas()
    );
    assert_eq!(
        decoded.spot_heal_generative("spot-heal-1").unwrap(),
        spot_heal()
    );
    // Container VERSION stays 1 with four kinds sharing one bundle.
    let bytes = container.to_bytes();
    assert_eq!(
        u16::from_le_bytes(bytes[8..10].try_into().unwrap()),
        VERSION
    );
}

#[test]
fn generative_checksums_are_stable_and_cover_pixels_not_ids() {
    let canvas = generative_canvas();
    let first = canvas.checksum();
    assert!(!first.is_empty());
    assert_eq!(first, canvas.checksum());
    let mut renamed = canvas.clone();
    renamed.id = "gen-canvas-2".into();
    assert_eq!(first, renamed.checksum());
    let mut changed = canvas.clone();
    changed.pixels[0] ^= 0xff;
    assert_ne!(first, changed.checksum());

    let spot = spot_heal();
    assert!(!spot.checksum().is_empty());
    assert_eq!(spot.checksum(), spot.checksum());
}

#[test]
fn generative_missing_ids_return_invalid() {
    let container =
        ZDataContainer::new_with(vec![RecordSpec::GenerativeCanvas(generative_canvas())])
            .unwrap();
    assert!(matches!(
        container.generative_canvas("absent"),
        Err(ZDataError::Invalid(_))
    ));
    assert!(matches!(
        container.spot_heal_generative("absent"),
        Err(ZDataError::Invalid(_))
    ));
}

#[test]
fn generative_dimension_mismatch_rejected_on_write() {
    let bad = GenerativeCanvasArtifact {
        id: "bad".into(),
        width: 2,
        height: 2,
        pixels: vec![0; 12],
    };
    assert!(bad.validate().is_err());
    assert!(ZDataContainer::new_with(vec![RecordSpec::GenerativeCanvas(bad)]).is_err());

    let bad_spot = SpotHealGenerativeArtifact {
        id: "bad".into(),
        width: 2,
        height: 2,
        pixels: vec![0; 15],
    };
    assert!(bad_spot.validate().is_err());
    assert!(ZDataContainer::new_with(vec![RecordSpec::SpotHealGenerative(bad_spot)]).is_err());

    let empty = GenerativeCanvasArtifact {
        id: String::new(),
        width: 1,
        height: 1,
        pixels: vec![0; 4],
    };
    assert!(empty.validate().is_err());
}

#[test]
fn generative_checksum_corruption_detected_eagerly_and_lazily() {
    for (spec, read_canvas) in [
        (RecordSpec::GenerativeCanvas(generative_canvas()), true),
        (RecordSpec::SpotHealGenerative(spot_heal()), false),
    ] {
        let id = match &spec {
            RecordSpec::GenerativeCanvas(c) => c.id.clone(),
            RecordSpec::SpotHealGenerative(s) => s.id.clone(),
            _ => unreachable!(),
        };
        let mut bytes = ZDataContainer::new_with(vec![spec])
            .unwrap()
            .to_bytes()
            .to_vec();
        let index = u64::from_le_bytes(bytes[16..24].try_into().unwrap()) as usize;
        let record =
            u64::from_le_bytes(bytes[index + 20..index + 28].try_into().unwrap()) as usize;
        // Flip a stored-digest byte: payload still decodes, so the
        // mismatch surfaces as Checksum (lazy path).
        bytes[record + 36] ^= 1;
        let corrupt = ZDataContainer::from_bytes(&bytes).unwrap();
        let error = if read_canvas {
            corrupt.generative_canvas(&id).unwrap_err()
        } else {
            corrupt.spot_heal_generative(&id).unwrap_err()
        };
        assert!(
            matches!(error, ZDataError::Checksum(_)),
            "digest corruption must be Checksum, got {error}"
        );

        // Eager path: corrupt the compressed payload on disk so
        // `load_zdata` fails up front.
        let directory = tempfile::tempdir().unwrap();
        let path = zdata_path_for(&directory.path().join("image.raw"));
        let intact = if read_canvas {
            ZDataContainer::new_with(vec![RecordSpec::GenerativeCanvas(generative_canvas())])
                .unwrap()
        } else {
            ZDataContainer::new_with(vec![RecordSpec::SpotHealGenerative(spot_heal())]).unwrap()
        };
        save_zdata(&path, &intact).unwrap();
        let mut on_disk = fs::read(&path).unwrap();
        let index_offset = u64::from_le_bytes(on_disk[16..24].try_into().unwrap()) as usize;
        on_disk[index_offset - 1] ^= 0xff;
        fs::write(&path, &on_disk).unwrap();
        assert!(
            load_zdata(&path).is_err(),
            "corrupt generative payload must fail at load time"
        );
    }
}

#[test]
fn generative_duplicate_id_rejected_across_all_kinds() {
    let container =
        ZDataContainer::new_with(vec![RecordSpec::GenerativeCanvas(generative_canvas())])
            .unwrap();
    assert!(matches!(
        container.add_generative_canvas(generative_canvas()),
        Err(ZDataError::DuplicateId(_))
    ));
    // Spot heal with the canvas id clashes.
    let mut clash = spot_heal();
    clash.id = "gen-canvas-1".into();
    assert!(matches!(
        container.add_spot_heal_generative(clash),
        Err(ZDataError::DuplicateId(_))
    ));
    // Mask tile with the canvas id clashes at construction.
    let tile_clash = MaskTile {
        mask_id: "gen-canvas-1".into(),
        tile_x: 0,
        tile_y: 0,
        width: 1,
        height: 1,
        values: vec![0],
    };
    assert!(matches!(
        ZDataContainer::new_with(vec![
            RecordSpec::MaskTile(tile_clash),
            RecordSpec::GenerativeCanvas(generative_canvas()),
        ]),
        Err(ZDataError::DuplicateId(_))
    ));
    // Canvas vs spot with identical ids clash at construction.
    let mut spot_dup = spot_heal();
    spot_dup.id = "gen-canvas-1".into();
    assert!(matches!(
        ZDataContainer::new_with(vec![
            RecordSpec::GenerativeCanvas(generative_canvas()),
            RecordSpec::SpotHealGenerative(spot_dup),
        ]),
        Err(ZDataError::DuplicateId(_))
    ));
}

#[test]
fn generative_atomic_append_preserves_bundle_and_uses_relative_path() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("image.raw");
    let path = zdata_path_for(&source);
    // Relative bundle reference: file name only, never absolute.
    assert_eq!(
        path.file_name().unwrap().to_string_lossy(),
        "image.raw.lumina.zdata"
    );
    assert!(!path.is_absolute() || path.starts_with(directory.path()));

    // Seed bundle with tiles, then append both generative kinds.
    save_zdata(&path, &ZDataContainer::new(tiles()).unwrap()).unwrap();
    append_generative_canvas(&path, generative_canvas()).unwrap();
    append_spot_heal_generative(&path, spot_heal()).unwrap();

    let reloaded = load_zdata(&path).unwrap();
    assert_eq!(reloaded.tile_count(), 4);
    assert_eq!(reloaded.tile("subject", 0, 0).unwrap(), tiles()[0]);
    assert_eq!(
        reloaded.generative_canvas("gen-canvas-1").unwrap(),
        generative_canvas()
    );
    assert_eq!(
        reloaded.spot_heal_generative("spot-heal-1").unwrap(),
        spot_heal()
    );

    // Atomic write leaves no temp files; the rendezvous lock file persists.
    let leftovers: Vec<_> = fs::read_dir(directory.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp-") || (name.ends_with(".lock") && name != ".image.raw.lumina.zdata.lock"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "no temp files or unexpected lock files may leak, found {leftovers:?}"
    );

    // Appending under a fresh lock fails explicitly and leaves the
    // bundle untouched.
    let lock_path = directory.path().join(".image.raw.lumina.zdata.lock");
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();
    lock_file.lock().unwrap();
    let mut blocked = generative_canvas();
    blocked.id = "blocked".into();
    let error = append_generative_canvas(&path, blocked).unwrap_err();
    assert!(
        matches!(&error, ZDataError::Io { operation, .. } if operation.contains("lock")),
        "generative append must report the held lock, got {error}"
    );
    assert_eq!(load_zdata(&path).unwrap().tile_count(), 4);
    drop(lock_file);

    // Missing bundle file surfaces as I/O, never as an empty container.
    let missing = directory.path().join("absent.lumina.zdata");
    assert!(matches!(load_zdata(&missing), Err(ZDataError::Io { .. })));
}

// =====================================================================
// LRPAR-G14-DENOISE-IMPL-20: denoise_rgb (kind 4) RGB8 codec.
// =====================================================================

fn denoise_rgb() -> DenoiseRgbArtifact {
    DenoiseRgbArtifact {
        id: "denoise-1".into(),
        width: 2,
        height: 2,
        pixels: vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120],
    }
}

#[test]
fn denoise_rgb_roundtrip_through_bundle() {
    let artifact = denoise_rgb();
    let container =
        ZDataContainer::new_with(vec![RecordSpec::DenoiseRgb(artifact.clone())]).unwrap();
    let decoded = ZDataContainer::from_bytes(container.to_bytes()).unwrap();
    assert_eq!(decoded.tile_count(), 1);
    assert_eq!(decoded.denoise_rgb("denoise-1").unwrap(), artifact);
    // Strict kind separation: never visible as any other kind.
    assert!(decoded.tile("denoise-1", 0, 0).is_err());
    assert!(decoded.repair_region("denoise-1").is_err());
    assert!(decoded.generative_canvas("denoise-1").is_err());
    assert!(decoded.spot_heal_generative("denoise-1").is_err());
    // Container VERSION stays 1: kind discriminator, not a format bump.
    let bytes = container.to_bytes();
    assert_eq!(
        u16::from_le_bytes(bytes[8..10].try_into().unwrap()),
        VERSION
    );
}

#[test]
fn denoise_rgb_coexists_with_all_other_kinds() {
    let container = ZDataContainer::new_with(vec![
        RecordSpec::MaskTile(tiles()[0].clone()),
        RecordSpec::RepairRegion(repair_region()),
        RecordSpec::GenerativeCanvas(generative_canvas()),
        RecordSpec::SpotHealGenerative(spot_heal()),
        RecordSpec::DenoiseRgb(denoise_rgb()),
    ])
    .unwrap();
    let decoded = ZDataContainer::from_bytes(container.to_bytes()).unwrap();
    assert_eq!(decoded.tile_count(), 5);
    assert_eq!(decoded.denoise_rgb("denoise-1").unwrap(), denoise_rgb());
    assert_eq!(decoded.tile("subject", 0, 0).unwrap(), tiles()[0]);
    assert_eq!(decoded.repair_region("repair-1").unwrap(), repair_region());
    assert_eq!(
        decoded.generative_canvas("gen-canvas-1").unwrap(),
        generative_canvas()
    );
    assert_eq!(
        decoded.spot_heal_generative("spot-heal-1").unwrap(),
        spot_heal()
    );
}

#[test]
fn denoise_rgb_checksum_is_canonical_and_covers_pixels_not_ids() {
    let artifact = denoise_rgb();
    let first = artifact.checksum();
    assert_eq!(first.len(), 64, "BLAKE3 hex digest");
    assert_eq!(first, artifact.checksum());
    // id/kind do not enter the digest; a pixel change does.
    let mut renamed = artifact.clone();
    renamed.id = "denoise-2".into();
    assert_eq!(first, renamed.checksum());
    let mut changed = artifact.clone();
    changed.pixels[0] ^= 0xff;
    assert_ne!(first, changed.checksum());
    assert_eq!(artifact.kind_str(), "denoise_rgb");
    assert_eq!(
        RecordKind::DenoiseRgb.as_str(),
        crate::DENOISE_ARTIFACT_KIND,
        "zdata kind string matches the recipe kind"
    );
}

/// Cross-crate layout guard: the canonical raw stream is
/// `encoding_version(1) || width || height || RGB8`; the literal below is
/// the pre-compression byte sequence and its BLAKE3 digest. The core crate
/// (`lumina_core::DenoiseRgbArtifact`) asserts the **same** digest, so a
/// silent layout drift between core and sidecar fails a test instead of
/// producing mismatching recipe/bundle checksums.
#[test]
fn denoise_rgb_canonical_bytes_and_digest_are_stable() {
    let artifact = denoise_rgb();
    let mut canonical = Vec::new();
    canonical.extend_from_slice(&1u32.to_le_bytes());
    canonical.extend_from_slice(&2u32.to_le_bytes());
    canonical.extend_from_slice(&2u32.to_le_bytes());
    canonical.extend_from_slice(&artifact.pixels);
    assert_eq!(canonical, artifact.encode_raw());
    assert_eq!(
        artifact.checksum(),
        blake3::hash(&canonical).to_hex().to_string()
    );
}

#[test]
fn denoise_rgb_dimension_mismatch_and_empty_id_rejected_on_write() {
    let bad = DenoiseRgbArtifact {
        id: "bad".into(),
        width: 2,
        height: 2,
        pixels: vec![0; 11],
    };
    assert!(bad.validate().is_err());
    assert!(ZDataContainer::new_with(vec![RecordSpec::DenoiseRgb(bad)]).is_err());

    let empty = DenoiseRgbArtifact {
        id: String::new(),
        width: 1,
        height: 1,
        pixels: vec![0; 3],
    };
    assert!(empty.validate().is_err());
    assert!(ZDataContainer::new_with(vec![RecordSpec::DenoiseRgb(empty)]).is_err());
}

#[test]
fn denoise_rgb_missing_id_returns_invalid() {
    let container =
        ZDataContainer::new_with(vec![RecordSpec::DenoiseRgb(denoise_rgb())]).unwrap();
    assert!(matches!(
        container.denoise_rgb("absent"),
        Err(ZDataError::Invalid(_))
    ));
}

#[test]
fn denoise_rgb_checksum_corruption_detected_eagerly_and_lazily() {
    let mut bytes = ZDataContainer::new_with(vec![RecordSpec::DenoiseRgb(denoise_rgb())])
        .unwrap()
        .to_bytes()
        .to_vec();
    let index = u64::from_le_bytes(bytes[16..24].try_into().unwrap()) as usize;
    let record = u64::from_le_bytes(bytes[index + 20..index + 28].try_into().unwrap()) as usize;
    bytes[record + 36] ^= 1;
    let corrupt = ZDataContainer::from_bytes(&bytes).unwrap();
    assert!(matches!(
        corrupt.denoise_rgb("denoise-1"),
        Err(ZDataError::Checksum(_))
    ));

    // Eager path: corrupt the compressed payload on disk so `load_zdata`
    // fails up front instead of handing out a container lazily.
    let directory = tempfile::tempdir().unwrap();
    let path = zdata_path_for(&directory.path().join("image.raw"));
    save_zdata(
        &path,
        &ZDataContainer::new_with(vec![RecordSpec::DenoiseRgb(denoise_rgb())]).unwrap(),
    )
    .unwrap();
    let mut on_disk = fs::read(&path).unwrap();
    let index_offset = u64::from_le_bytes(on_disk[16..24].try_into().unwrap()) as usize;
    on_disk[index_offset - 1] ^= 0xff;
    fs::write(&path, &on_disk).unwrap();
    assert!(
        load_zdata(&path).is_err(),
        "corrupt denoise payload must fail at load time"
    );
}

#[test]
fn denoise_rgb_duplicate_id_rejected_across_all_kinds() {
    let container =
        ZDataContainer::new_with(vec![RecordSpec::DenoiseRgb(denoise_rgb())]).unwrap();
    assert!(matches!(
        container.add_denoise_rgb(denoise_rgb()),
        Err(ZDataError::DuplicateId(_))
    ));
    let mut clash = spot_heal();
    clash.id = "denoise-1".into();
    assert!(matches!(
        container.add_spot_heal_generative(clash),
        Err(ZDataError::DuplicateId(_))
    ));
    let tile_clash = MaskTile {
        mask_id: "denoise-1".into(),
        tile_x: 0,
        tile_y: 0,
        width: 1,
        height: 1,
        values: vec![0],
    };
    assert!(matches!(
        ZDataContainer::new_with(vec![
            RecordSpec::MaskTile(tile_clash),
            RecordSpec::DenoiseRgb(denoise_rgb()),
        ]),
        Err(ZDataError::DuplicateId(_))
    ));
}

#[test]
fn denoise_rgb_atomic_append_preserves_bundle_and_honors_lock() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("image.raw");
    let path = zdata_path_for(&source);
    save_zdata(&path, &ZDataContainer::new(tiles()).unwrap()).unwrap();
    append_denoise_rgb(&path, denoise_rgb()).unwrap();

    let reloaded = load_zdata(&path).unwrap();
    assert_eq!(reloaded.tile_count(), 3);
    assert_eq!(reloaded.tile("subject", 0, 0).unwrap(), tiles()[0]);
    assert_eq!(reloaded.denoise_rgb("denoise-1").unwrap(), denoise_rgb());

    // A held lock blocks the append explicitly and leaves the bundle.
    let lock_path = directory.path().join(".image.raw.lumina.zdata.lock");
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();
    lock_file.lock().unwrap();
    let mut blocked = denoise_rgb();
    blocked.id = "blocked".into();
    let error = append_denoise_rgb(&path, blocked).unwrap_err();
    assert!(
        matches!(&error, ZDataError::Io { operation, .. } if operation.contains("lock")),
        "denoise append must report the held lock, got {error}"
    );
    assert_eq!(load_zdata(&path).unwrap().tile_count(), 3);
    drop(lock_file);

    // Explicit replace regenerates the same record without duplication.
    let mut regenerated = denoise_rgb();
    regenerated.pixels[0] = 200;
    save_denoise_rgb(&path, regenerated.clone(), true).unwrap();
    let replaced = load_zdata(&path).unwrap();
    assert_eq!(replaced.tile_count(), 3, "replace must not duplicate");
    assert_eq!(replaced.denoise_rgb("denoise-1").unwrap(), regenerated);

    // No temp files or unexpected lock files may leak.
    let leftovers: Vec<_> = fs::read_dir(directory.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp-") || (name.ends_with(".lock") && name != ".image.raw.lumina.zdata.lock"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "no temp files or unexpected lock files, found {leftovers:?}"
    );
}

// =====================================================================
// FACE-20-IMPL-20-REST: face_embedding (kind 5) f32 codec.
// =====================================================================

fn face_embedding() -> FaceEmbeddingArtifact {
    FaceEmbeddingArtifact {
        id: "emb-test".into(),
        dimension: 4,
        values: vec![0.5, -0.25, 0.125, 0.75],
    }
}

#[test]
fn face_embedding_roundtrip_through_bundle() {
    let artifact = face_embedding();
    let container =
        ZDataContainer::new_with(vec![RecordSpec::FaceEmbedding(artifact.clone())]).unwrap();
    let decoded = ZDataContainer::from_bytes(container.to_bytes()).unwrap();
    assert_eq!(decoded.tile_count(), 1);
    assert_eq!(decoded.face_embedding("emb-test").unwrap(), artifact);
    assert!(decoded.has_record(RecordKind::FaceEmbedding, "emb-test"));
    assert!(!decoded.has_record(RecordKind::FaceEmbedding, "absent"));
    // Strict kind separation: never visible as any other kind.
    assert!(decoded.tile("emb-test", 0, 0).is_err());
    assert!(decoded.repair_region("emb-test").is_err());
    assert!(decoded.generative_canvas("emb-test").is_err());
    assert!(decoded.spot_heal_generative("emb-test").is_err());
    assert!(decoded.denoise_rgb("emb-test").is_err());
    // Container VERSION stays 1: kind discriminator, not a format bump.
    let bytes = container.to_bytes();
    assert_eq!(
        u16::from_le_bytes(bytes[8..10].try_into().unwrap()),
        VERSION
    );
}

#[test]
fn face_embedding_coexists_with_all_other_kinds() {
    let container = ZDataContainer::new_with(vec![
        RecordSpec::MaskTile(tiles()[0].clone()),
        RecordSpec::RepairRegion(repair_region()),
        RecordSpec::GenerativeCanvas(generative_canvas()),
        RecordSpec::SpotHealGenerative(spot_heal()),
        RecordSpec::DenoiseRgb(denoise_rgb()),
        RecordSpec::FaceEmbedding(face_embedding()),
    ])
    .unwrap();
    let decoded = ZDataContainer::from_bytes(container.to_bytes()).unwrap();
    assert_eq!(decoded.tile_count(), 6);
    assert_eq!(
        decoded.face_embedding("emb-test").unwrap(),
        face_embedding()
    );
    assert_eq!(decoded.denoise_rgb("denoise-1").unwrap(), denoise_rgb());
    assert_eq!(decoded.tile("subject", 0, 0).unwrap(), tiles()[0]);
}

/// The canonical raw stream is
/// `encoding_version(1) || dimension || f32 LE values`; the digest is
/// dimension/value dependent but identity-independent (id/kind do not enter
/// it), so the persisted `FaceVectorRef.checksum` stays reproducible.
#[test]
fn face_embedding_canonical_bytes_and_checksum_are_stable() {
    let artifact = face_embedding();
    let mut canonical = Vec::new();
    canonical.extend_from_slice(&1u32.to_le_bytes());
    canonical.extend_from_slice(&4u32.to_le_bytes());
    for value in &artifact.values {
        canonical.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(canonical, artifact.encode_raw());
    assert_eq!(
        artifact.checksum(),
        blake3::hash(&canonical).to_hex().to_string()
    );
    assert_eq!(artifact.checksum().len(), 64, "BLAKE3 hex digest");
    let mut renamed = artifact.clone();
    renamed.id = "emb-other".into();
    assert_eq!(artifact.checksum(), renamed.checksum());
    let mut changed = artifact.clone();
    changed.values[0] += 0.5;
    assert_ne!(artifact.checksum(), changed.checksum());
    assert_eq!(artifact.kind_str(), "face_embedding");
    assert_eq!(RecordKind::FaceEmbedding.as_str(), "face_embedding");
}

#[test]
fn face_embedding_invalid_payloads_rejected_on_write() {
    // Value count disagrees with the declared dimension.
    let bad = FaceEmbeddingArtifact {
        id: "bad".into(),
        dimension: 4,
        values: vec![0.0, 1.0],
    };
    assert!(bad.validate().is_err());
    assert!(ZDataContainer::new_with(vec![RecordSpec::FaceEmbedding(bad)]).is_err());
    // Zero dimension.
    let empty = FaceEmbeddingArtifact {
        id: "empty".into(),
        dimension: 0,
        values: vec![],
    };
    assert!(empty.validate().is_err());
    // Empty id.
    let unnamed = FaceEmbeddingArtifact {
        id: String::new(),
        dimension: 1,
        values: vec![1.0],
    };
    assert!(unnamed.validate().is_err());
    // Non-finite values must never be persisted silently.
    let nan = FaceEmbeddingArtifact {
        id: "nan".into(),
        dimension: 2,
        values: vec![f32::NAN, 0.0],
    };
    assert!(nan.validate().is_err());
    let infinite = FaceEmbeddingArtifact {
        id: "inf".into(),
        dimension: 2,
        values: vec![f32::INFINITY, 0.0],
    };
    assert!(infinite.validate().is_err());
}

#[test]
fn face_embedding_missing_id_returns_invalid() {
    let container =
        ZDataContainer::new_with(vec![RecordSpec::FaceEmbedding(face_embedding())]).unwrap();
    assert!(matches!(
        container.face_embedding("absent"),
        Err(ZDataError::Invalid(_))
    ));
    // Strict kind separation: never returned by another kind's accessor.
    assert!(matches!(
        container.denoise_rgb("emb-test"),
        Err(ZDataError::Invalid(_))
    ));
}

#[test]
fn face_embedding_checksum_corruption_detected_eagerly() {
    // Tamper the *raw* (uncompressed) payload of the record: rebuild the
    // same container from a value-mutated artifact (same dimensions, so the
    // record sizes match), then transplant the pristine record checksum.
    // The payload no longer hashes to it, and both the lazy accessor and
    // the eager `load_zdata` path must catch it.
    let mutate_minus_one = |value: f32| {
        // Adjacent f32 with identical little-endian byte length.
        f32::from_bits(value.to_bits() - 1)
    };
    let mut tampered = face_embedding();
    tampered.values[0] = mutate_minus_one(tampered.values[0]);
    let pristine_bytes =
        ZDataContainer::new_with(vec![RecordSpec::FaceEmbedding(face_embedding())])
            .unwrap()
            .to_bytes()
            .to_vec();
    let mut bytes = ZDataContainer::new_with(vec![RecordSpec::FaceEmbedding(tampered)])
        .unwrap()
        .to_bytes()
        .to_vec();
    assert_eq!(
        bytes.len(),
        pristine_bytes.len(),
        "same dimensions must produce the same record size"
    );
    let index = u64::from_le_bytes(bytes[16..24].try_into().unwrap()) as usize;
    let record = u64::from_le_bytes(bytes[index + 20..index + 28].try_into().unwrap()) as usize;
    // Checksum lives in the record header at offset 36.
    bytes[record + 36..record + 68].copy_from_slice(&pristine_bytes[record + 36..record + 68]);

    let corrupt = ZDataContainer::from_bytes(&bytes).unwrap();
    assert!(matches!(
        corrupt.face_embedding("emb-test"),
        Err(ZDataError::Checksum(_))
    ));
    let directory = tempfile::tempdir().unwrap();
    let path = zdata_path_for(&directory.path().join("image.raw"));
    fs::write(&path, &bytes).unwrap();
    assert!(
        load_zdata(&path).is_err(),
        "corrupt face embedding payload must fail at load time"
    );
}

#[test]
fn face_embedding_duplicate_id_rejected_across_all_kinds() {
    let container =
        ZDataContainer::new_with(vec![RecordSpec::FaceEmbedding(face_embedding())]).unwrap();
    assert!(matches!(
        container.add_face_embedding(face_embedding()),
        Err(ZDataError::DuplicateId(_))
    ));
    let mut clash = denoise_rgb();
    clash.id = "emb-test".into();
    assert!(matches!(
        container.add_denoise_rgb(clash),
        Err(ZDataError::DuplicateId(_))
    ));
    assert!(matches!(
        ZDataContainer::new_with(vec![
            RecordSpec::FaceEmbedding(face_embedding()),
            RecordSpec::FaceEmbedding(face_embedding()),
        ]),
        Err(ZDataError::DuplicateId(_))
    ));
}

#[test]
fn face_embeddings_batch_write_is_atomic_replaceable_and_locked() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("image.raw");
    let path = zdata_path_for(&source);
    save_zdata(&path, &ZDataContainer::new(tiles()).unwrap()).unwrap();

    let mut second = face_embedding();
    second.id = "emb-two".into();
    save_face_embeddings(&path, vec![face_embedding(), second.clone()]).unwrap();
    let reloaded = load_zdata(&path).unwrap();
    assert_eq!(
        reloaded.tile_count(),
        4,
        "two face records + two mask tiles"
    );
    assert_eq!(reloaded.tile("subject", 0, 0).unwrap(), tiles()[0]);
    assert_eq!(
        reloaded.face_embedding("emb-test").unwrap(),
        face_embedding()
    );
    assert_eq!(reloaded.face_embedding("emb-two").unwrap(), second);

    // The explicit re-analysis path upserts by id instead of duplicating.
    let mut regenerated = face_embedding();
    regenerated.values[0] = -0.5;
    save_face_embeddings(&path, vec![regenerated.clone()]).unwrap();
    let replaced = load_zdata(&path).unwrap();
    assert_eq!(replaced.tile_count(), 4, "upsert must not duplicate");
    assert_eq!(replaced.face_embedding("emb-test").unwrap(), regenerated);
    assert_eq!(
        replaced.face_embedding("emb-two").unwrap(),
        second,
        "other ids stay untouched"
    );

    // A duplicate id inside one batch is rejected, and an id collision with
    // a record of a different kind is rejected too.
    assert!(matches!(
        save_face_embeddings(&path, vec![face_embedding(), face_embedding()]),
        Err(ZDataError::DuplicateId(_))
    ));
    let mut collide = face_embedding();
    collide.id = "subject".into();
    assert!(matches!(
        save_face_embeddings(&path, vec![collide]),
        Err(ZDataError::DuplicateId(_))
    ));
    assert!(!load_zdata(&path)
        .unwrap()
        .has_record(RecordKind::FaceEmbedding, "subject"));

    let lock_path = directory.path().join(".image.raw.lumina.zdata.lock");
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();
    lock_file.lock().unwrap();
    let mut blocked = face_embedding();
    blocked.id = "emb-blocked".into();
    let error = save_face_embeddings(&path, vec![blocked]).unwrap_err();
    assert!(
        matches!(&error, ZDataError::Io { operation, .. } if operation.contains("lock")),
        "batch write must report the held lock, got {error}"
    );
    drop(lock_file);

    // Belt and braces: the clash record did not enter the bundle.
    assert!(!load_zdata(&path)
        .unwrap()
        .has_record(RecordKind::FaceEmbedding, "denoise-1"));

    // No temp files or unexpected lock files may leak.
    let leftovers: Vec<_> = fs::read_dir(directory.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp-") || (name.ends_with(".lock") && name != ".image.raw.lumina.zdata.lock"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "no temp files or unexpected lock files, found {leftovers:?}"
    );
}
