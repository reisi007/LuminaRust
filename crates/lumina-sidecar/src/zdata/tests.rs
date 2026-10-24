//! The `.lumina.zdata` container tests, moved verbatim out of `zdata.rs` (SPLICE-SIDECAR-ZDATA-93); the `mod tests;` declaration carries `#[rustfmt::skip]` so this file stays byte-identical to the block it replaces.
use super::*;

mod lock_tests;
mod payload_kinds;

fn tiles() -> Vec<MaskTile> {
    vec![
        MaskTile {
            mask_id: "subject".into(),
            tile_x: 0,
            tile_y: 0,
            width: 2,
            height: 2,
            values: vec![0, 1, 32768, 65535],
        },
        MaskTile {
            mask_id: "hair".into(),
            tile_x: 1,
            tile_y: 2,
            width: 1,
            height: 3,
            values: vec![7, 8, 9],
        },
    ]
}

#[test]
fn multiple_tiles_roundtrip_and_random_access() {
    let container = ZDataContainer::new(tiles()).unwrap();
    let decoded = ZDataContainer::from_bytes(container.to_bytes()).unwrap();
    assert_eq!(decoded.tile_count(), 2);
    assert_eq!(decoded.tile("hair", 1, 2).unwrap(), tiles()[1]);
    assert!(decoded.tile("missing", 0, 0).is_err());
}

#[test]
fn header_and_metadata_are_deterministic() {
    let container = ZDataContainer::new(tiles()).unwrap();
    let bytes = container.to_bytes();
    assert_eq!(&bytes[..8], MAGIC);
    assert_eq!(
        u16::from_le_bytes(bytes[8..10].try_into().unwrap()),
        VERSION
    );
    assert_eq!(
        u16::from_le_bytes(bytes[10..12].try_into().unwrap()) as usize,
        HEADER_LEN
    );
    assert_eq!(u32::from_le_bytes(bytes[12..16].try_into().unwrap()), 2);
    assert_eq!(bytes, ZDataContainer::new(tiles()).unwrap().to_bytes());
}

#[test]
fn malformed_header_and_truncation_are_rejected() {
    let bytes = ZDataContainer::new(tiles()).unwrap().to_bytes().to_vec();
    let mut bad_magic = bytes.clone();
    bad_magic[0] = b'X';
    assert!(matches!(
        ZDataContainer::from_bytes(&bad_magic),
        Err(ZDataError::Invalid(_))
    ));
    let mut bad_version = bytes.clone();
    bad_version[8] = 2;
    assert!(matches!(
        ZDataContainer::from_bytes(&bad_version),
        Err(ZDataError::UnsupportedVersion(2))
    ));
    assert!(ZDataContainer::from_bytes(&bytes[..bytes.len() - 1]).is_err());
}

#[test]
fn empty_index_offset_must_not_be_past_end_of_container() {
    let mut bytes = vec![0; HEADER_LEN];
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..10].copy_from_slice(&VERSION.to_le_bytes());
    bytes[10..12].copy_from_slice(&(HEADER_LEN as u16).to_le_bytes());
    let index_offset = bytes.len() as u64 + 1;
    bytes[16..24].copy_from_slice(&index_offset.to_le_bytes());

    assert!(matches!(
        ZDataContainer::from_bytes(&bytes),
        Err(ZDataError::Invalid(message)) if message.contains("index is outside")
    ));
}

#[test]
fn excessive_record_count_is_rejected_before_index_allocation() {
    let mut bytes = ZDataContainer::new(tiles()).unwrap().to_bytes().to_vec();
    bytes[12..16].copy_from_slice(&((MAX_RECORDS as u32) + 1).to_le_bytes());
    assert!(matches!(
        ZDataContainer::from_bytes(&bytes),
        Err(ZDataError::Invalid(message)) if message.contains("record count")
    ));
}

#[test]
fn new_rejects_excessive_record_count_before_internal_allocation() {
    let tile = MaskTile {
        mask_id: String::new(),
        tile_x: 0,
        tile_y: 0,
        width: 0,
        height: 0,
        values: Vec::new(),
    };
    let tiles = vec![tile; MAX_RECORDS + 1];
    assert!(matches!(
        ZDataContainer::new(tiles),
        Err(ZDataError::Invalid(message)) if message.contains("record count")
    ));
}

#[test]
fn record_coordinates_and_dimensions_must_match_index() {
    let original = ZDataContainer::new(tiles()).unwrap().to_bytes().to_vec();
    let index = u64::from_le_bytes(original[16..24].try_into().unwrap()) as usize;
    let record =
        u64::from_le_bytes(original[index + 20..index + 28].try_into().unwrap()) as usize;
    for field in [4usize, 8, 12, 16] {
        let mut bytes = original.clone();
        let value = u32::from_le_bytes(
            bytes[record + field..record + field + 4]
                .try_into()
                .unwrap(),
        );
        bytes[record + field..record + field + 4]
            .copy_from_slice(&value.wrapping_add(1).to_le_bytes());
        assert!(matches!(
            ZDataContainer::from_bytes(&bytes),
            Err(ZDataError::Invalid(message)) if message.contains("metadata")
        ));
    }
}

#[test]
fn zstd_output_cannot_exceed_declared_or_global_limit() {
    let compressed = zstd::stream::encode_all([1u8, 2, 3, 4].as_slice(), 3).unwrap();
    assert!(matches!(
        decode_payload(&compressed, 2),
        Err(ZDataError::Invalid(message)) if message.contains("exceeds declared")
    ));
    assert!(matches!(
        decode_payload(&compressed, MAX_UNCOMPRESSED + 1),
        Err(ZDataError::Invalid(message)) if message.contains("exceeds limit")
    ));
}

#[test]
fn conservative_size_limit_is_checked_without_allocating_payload() {
    let estimated = add_conservative_size(HEADER_LEN, MAX_CONTAINER_BYTES as u64, 0).unwrap();
    assert!(matches!(
        ensure_container_size(estimated),
        Err(ZDataError::Invalid(message)) if message.contains("container exceeds")
    ));
    assert!(matches!(
        add_conservative_size(HEADER_LEN, u64::MAX, 0),
        Err(ZDataError::Invalid(message)) if message.contains("overflows")
    ));
}

#[test]
fn checksum_duplicate_dimensions_and_limits_are_rejected() {
    let mut bytes = ZDataContainer::new(tiles()).unwrap().to_bytes().to_vec();
    let index = u64::from_le_bytes(bytes[16..24].try_into().unwrap()) as usize;
    let first_record =
        u64::from_le_bytes(bytes[index + 20..index + 28].try_into().unwrap()) as usize;
    bytes[first_record + 36] ^= 1;
    let corrupt = ZDataContainer::from_bytes(&bytes).unwrap();
    assert!(matches!(
        corrupt.tile("subject", 0, 0),
        Err(ZDataError::Checksum(_))
    ));
    let duplicate = vec![tiles()[0].clone(), tiles()[0].clone()];
    assert!(matches!(
        ZDataContainer::new(duplicate),
        Err(ZDataError::DuplicateId(_))
    ));
    let wrong = MaskTile {
        width: 2,
        values: vec![1],
        ..tiles()[0].clone()
    };
    assert!(ZDataContainer::new(vec![wrong]).is_err());
    let mut oversized = ZDataContainer::new(tiles()).unwrap().to_bytes().to_vec();
    oversized[16..24].copy_from_slice(&(MAX_CONTAINER_BYTES as u64).to_le_bytes());
    assert!(ZDataContainer::from_bytes(&oversized).is_err());
}

#[test]
fn atomic_file_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("image.raw");
    let path = zdata_path_for(&source);
    let container = ZDataContainer::new(tiles()).unwrap();
    save_zdata(&path, &container).unwrap();
    assert_eq!(
        load_zdata(&path).unwrap().tile("subject", 0, 0).unwrap(),
        tiles()[0]
    );
    let replacement = ZDataContainer::new(vec![tiles()[1].clone()]).unwrap();
    save_zdata(&path, &replacement).unwrap();
    assert_eq!(
        load_zdata(&path).unwrap().tile("hair", 1, 2).unwrap(),
        tiles()[1]
    );
}

#[test]
fn load_rejects_oversized_sparse_file_before_reading_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("oversized.lumina.zdata");
    let file = fs::File::create(&path).unwrap();
    file.set_len(MAX_CONTAINER_BYTES as u64 + 1).unwrap();

    assert!(matches!(
        load_zdata(&path),
        Err(ZDataError::Invalid(message)) if message.contains("exceeds size limit")
    ));
}

fn repair_region() -> RepairRegionArtifact {
    RepairRegionArtifact {
        id: "repair-1".into(),
        width: 2,
        height: 2,
        // region >= 32768 marks replaced pixels (top row replaced)
        region: vec![0, 65535, 65535, 0],
        replacement: vec![
            10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255,
        ],
    }
}

#[test]
fn repair_region_roundtrip_through_bundle() {
    let artifact = repair_region();
    let container =
        ZDataContainer::new_with(vec![RecordSpec::RepairRegion(artifact.clone())]).unwrap();
    let decoded = ZDataContainer::from_bytes(container.to_bytes()).unwrap();
    assert_eq!(decoded.tile_count(), 1);
    assert_eq!(decoded.repair_region("repair-1").unwrap(), artifact);
    // The same bundle must NOT expose the artifact as a mask tile.
    assert!(decoded.tile("repair-1", 0, 0).is_err());
}

#[test]
fn repair_region_preserved_alongside_mask_tiles() {
    let artifact = repair_region();
    let existing = ZDataContainer::new(tiles()).unwrap();
    let combined = existing.add_repair_region(artifact.clone()).unwrap();
    assert_eq!(combined.tile_count(), 3);
    // mask tiles are intact
    assert_eq!(combined.tile("subject", 0, 0).unwrap(), tiles()[0]);
    assert_eq!(combined.tile("hair", 1, 2).unwrap(), tiles()[1]);
    // repair region is present
    assert_eq!(combined.repair_region("repair-1").unwrap(), artifact);

    // roundtrip through disk
    let directory = tempfile::tempdir().unwrap();
    let path = zdata_path_for(&directory.path().join("image.raw"));
    save_zdata(&path, &combined).unwrap();
    let reloaded = load_zdata(&path).unwrap();
    assert_eq!(reloaded.repair_region("repair-1").unwrap(), artifact);
    assert_eq!(reloaded.tile("subject", 0, 0).unwrap(), tiles()[0]);
}

#[test]
fn repair_region_dimension_mismatch_rejected_on_write() {
    // region too short for the declared dimensions
    let bad_region = RepairRegionArtifact {
        id: "bad".into(),
        width: 2,
        height: 2,
        region: vec![0, 1, 2],
        replacement: vec![0; 16],
    };
    assert!(bad_region.validate().is_err());
    assert!(ZDataContainer::new_with(vec![RecordSpec::RepairRegion(bad_region)]).is_err());

    // region/replacement dimension disagreement
    let mismatched = RepairRegionArtifact {
        id: "bad".into(),
        width: 2,
        height: 2,
        region: vec![0, 1, 2, 3],
        replacement: vec![0; 12],
    };
    assert!(mismatched.validate().is_err());
    assert!(ZDataContainer::new_with(vec![RecordSpec::RepairRegion(mismatched)]).is_err());
}

#[test]
fn repair_region_checksum_corruption_detected() {
    let artifact = repair_region();
    let mut bytes = ZDataContainer::new_with(vec![RecordSpec::RepairRegion(artifact)])
        .unwrap()
        .to_bytes()
        .to_vec();
    let index = u64::from_le_bytes(bytes[16..24].try_into().unwrap()) as usize;
    let record = u64::from_le_bytes(bytes[index + 20..index + 28].try_into().unwrap()) as usize;
    // flip a byte of the stored BLAKE3 checksum (record header); the payload
    // stays intact so decompression succeeds and the mismatch is reported
    // as a checksum error, not a decode failure.
    bytes[record + 36] ^= 1;
    let corrupt = ZDataContainer::from_bytes(&bytes).unwrap();
    assert!(matches!(
        corrupt.repair_region("repair-1"),
        Err(ZDataError::Checksum(_))
    ));
}

#[test]
fn repair_region_duplicate_id_rejected() {
    let artifact = repair_region();
    let container =
        ZDataContainer::new_with(vec![RecordSpec::RepairRegion(artifact.clone())]).unwrap();
    assert!(matches!(
        container.add_repair_region(artifact),
        Err(ZDataError::DuplicateId(_))
    ));
    // also rejected across kinds (mask tile with the same id)
    let clash = MaskTile {
        mask_id: "repair-1".into(),
        tile_x: 0,
        tile_y: 0,
        width: 1,
        height: 1,
        values: vec![0],
    };
    assert!(matches!(
        ZDataContainer::new_with(vec![
            RecordSpec::MaskTile(clash),
            RecordSpec::RepairRegion(repair_region()),
        ]),
        Err(ZDataError::DuplicateId(_))
    ));
}

#[test]
fn repair_region_missing_returns_error_and_checksum_is_stable() {
    let artifact = repair_region();
    let container =
        ZDataContainer::new_with(vec![RecordSpec::RepairRegion(artifact.clone())]).unwrap();
    assert!(matches!(
        container.repair_region("absent"),
        Err(ZDataError::Invalid(_))
    ));
    // checksum is canonical and stable across calls; it covers the artifact
    // payload (region + replacement), not the record id, so it is invariant
    // under id changes.
    let first = artifact.checksum();
    assert!(!first.is_empty());
    assert_eq!(first, artifact.checksum());
    let mut other = artifact.clone();
    other.id = "repair-2".into();
    assert_eq!(first, other.checksum());
}

#[test]
fn repair_region_kind_must_match_container_record() {
    // A mask tile whose id collides with a repair region must not be
    // returned by `repair_region`, even with matching coordinates.
    let artifact = repair_region();
    let container = ZDataContainer::new_with(vec![RecordSpec::RepairRegion(artifact)]).unwrap();
    // `tile` is restricted to MaskTile records:
    assert!(container.tile("repair-1", 0, 0).is_err());
}

// =====================================================================
// REVIEW-SIDECAR-ZDATA-1: eager checksum verification at load time and
// a serialized read-modify-write path.
// =====================================================================

#[test]
fn load_zdata_verifies_checksums_eagerly_not_lazily() {
    let directory = tempfile::tempdir().unwrap();
    let path = zdata_path_for(&directory.path().join("image.raw"));
    save_zdata(&path, &ZDataContainer::new(tiles()).unwrap()).unwrap();
    let bytes = fs::read(&path).unwrap();

    // Corrupt the LAST compressed payload byte (records end where the
    // index begins). Parsing alone still succeeds because verification was
    // lazy; loading from disk must now fail up front instead of handing
    // out a container whose first untouched-tile access would explode.
    let index_offset = u64::from_le_bytes(bytes[16..24].try_into().unwrap()) as usize;
    let mut bad_payload = bytes.clone();
    let last_payload = index_offset - 1;
    bad_payload[last_payload] ^= 0xff;
    fs::write(&path, &bad_payload).unwrap();
    let error = load_zdata(&path).unwrap_err();
    assert!(
        matches!(error, ZDataError::Checksum(_) | ZDataError::Invalid(_)),
        "corrupt payload must be detected at load time, got {error}"
    );

    // Corrupting the STORED digest is always reported as a checksum
    // failure (the intact payload still decodes).
    fs::write(&path, &bytes).unwrap();
    let fresh = fs::read(&path).unwrap();
    let record_offset = u64::from_le_bytes(
        fresh[index_offset + 20..index_offset + 28]
            .try_into()
            .unwrap(),
    ) as usize;
    let mut bad_digest = fresh.clone();
    bad_digest[record_offset + 36] ^= 1;
    fs::write(&path, &bad_digest).unwrap();
    assert!(matches!(load_zdata(&path), Err(ZDataError::Checksum(_))));

    // The intact container still loads.
    fs::write(&path, &bytes).unwrap();
    assert!(load_zdata(&path).is_ok());
}
