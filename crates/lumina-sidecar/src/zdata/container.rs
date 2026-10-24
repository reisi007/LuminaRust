//! The indexed `.lumina.zdata` container: construction, parsing and record
//! reads.
//!
//! Pure move out of `crates/lumina-sidecar/src/zdata.rs`: the struct and its
//! inherent `impl` are byte-identical to the block that used to live there; only
//! the module wiring changed. `zdata.rs` re-exports the type, so
//! `lumina_sidecar::zdata::ZDataContainer` and every public method keep their
//! path.

use std::collections::HashSet;

use super::codec::{
    add_conservative_size, decode_payload, ensure_container_size, parse_record, read_u32, read_u64,
    validate_tile,
};
use super::{
    invalid, spec_id, DenoiseRgbArtifact, FaceEmbeddingArtifact, GenerativeCanvasArtifact,
    MaskTile, Record, RecordKind, RecordSpec, RepairRegionArtifact, SpotHealGenerativeArtifact,
    ZDataError, HEADER_LEN, INDEX_ENTRY_FIXED_LEN, MAGIC, MAX_CONTAINER_BYTES, MAX_ID_LEN,
    MAX_RECORDS, RECORD_HEADER_LEN, VERSION,
};

/// An indexed container. It owns compressed bytes and metadata, not duplicate
/// decompressed tile values; values are allocated only for the requested tile.
#[derive(Debug, Clone)]
pub struct ZDataContainer {
    bytes: Vec<u8>,
    records: Vec<Record>,
}

impl ZDataContainer {
    /// Builds a container from mask tiles only (original API).  Repair-region
    /// artifacts are added through [`ZDataContainer::new_with`] / the
    /// [`append_repair_region`](crate::zdata::append_repair_region) helper.
    pub fn new(tiles: Vec<MaskTile>) -> Result<Self, ZDataError> {
        let records = tiles.into_iter().map(RecordSpec::MaskTile).collect();
        Self::new_with(records)
    }

    /// Builds a container that may hold both mask tiles and repair-region
    /// artifacts.  A per-record `kind` discriminator keeps the two payload
    /// types apart inside one `.lumina.zdata` bundle without changing the
    /// container `VERSION`.  Duplicate ids (across either kind) are rejected.
    pub fn new_with(records: Vec<RecordSpec>) -> Result<Self, ZDataError> {
        if records.len() > MAX_RECORDS {
            return Err(invalid("record count exceeds limit"));
        }
        let mut conservative_size = HEADER_LEN;
        let mut ids = HashSet::new();
        for record in &records {
            match record {
                RecordSpec::MaskTile(tile) => {
                    validate_tile(tile)?;
                    if !ids.insert(tile.mask_id.clone()) {
                        return Err(ZDataError::DuplicateId(tile.mask_id.clone()));
                    }
                    conservative_size = add_conservative_size(
                        conservative_size,
                        u64::from(tile.width)
                            .checked_mul(u64::from(tile.height))
                            .and_then(|values| values.checked_mul(2))
                            .ok_or_else(|| invalid("tile dimensions overflow"))?,
                        tile.mask_id.len(),
                    )?;
                }
                RecordSpec::RepairRegion(region) => {
                    region.validate()?;
                    if !ids.insert(region.id.clone()) {
                        return Err(ZDataError::DuplicateId(region.id.clone()));
                    }
                    let count = u64::from(region.width)
                        .checked_mul(u64::from(region.height))
                        .ok_or_else(|| invalid("repair region dimensions overflow"))?;
                    conservative_size = add_conservative_size(
                        conservative_size,
                        // raw payload = region (2 bytes/px) + replacement (4 bytes/px)
                        count
                            .checked_mul(6)
                            .ok_or_else(|| invalid("repair region dimensions overflow"))?,
                        region.id.len(),
                    )?;
                }
                RecordSpec::GenerativeCanvas(canvas) => {
                    canvas.validate()?;
                    if !ids.insert(canvas.id.clone()) {
                        return Err(ZDataError::DuplicateId(canvas.id.clone()));
                    }
                    let count = u64::from(canvas.width)
                        .checked_mul(u64::from(canvas.height))
                        .ok_or_else(|| invalid("generative canvas dimensions overflow"))?;
                    conservative_size = add_conservative_size(
                        conservative_size,
                        // raw payload = 12-byte header + RGBA8 (4 bytes/px)
                        count
                            .checked_mul(4)
                            .and_then(|pixels| pixels.checked_add(12))
                            .ok_or_else(|| invalid("generative canvas dimensions overflow"))?,
                        canvas.id.len(),
                    )?;
                }
                RecordSpec::SpotHealGenerative(spot) => {
                    spot.validate()?;
                    if !ids.insert(spot.id.clone()) {
                        return Err(ZDataError::DuplicateId(spot.id.clone()));
                    }
                    let count = u64::from(spot.width)
                        .checked_mul(u64::from(spot.height))
                        .ok_or_else(|| invalid("spot heal dimensions overflow"))?;
                    conservative_size = add_conservative_size(
                        conservative_size,
                        // raw payload = 12-byte header + RGBA8 (4 bytes/px)
                        count
                            .checked_mul(4)
                            .and_then(|pixels| pixels.checked_add(12))
                            .ok_or_else(|| invalid("spot heal dimensions overflow"))?,
                        spot.id.len(),
                    )?;
                }
                RecordSpec::DenoiseRgb(denoise) => {
                    denoise.validate()?;
                    if !ids.insert(denoise.id.clone()) {
                        return Err(ZDataError::DuplicateId(denoise.id.clone()));
                    }
                    let count = u64::from(denoise.width)
                        .checked_mul(u64::from(denoise.height))
                        .ok_or_else(|| invalid("denoise dimensions overflow"))?;
                    conservative_size = add_conservative_size(
                        conservative_size,
                        // raw payload = 12-byte header + RGB8 (3 bytes/px)
                        count
                            .checked_mul(3)
                            .and_then(|pixels| pixels.checked_add(12))
                            .ok_or_else(|| invalid("denoise dimensions overflow"))?,
                        denoise.id.len(),
                    )?;
                }
                RecordSpec::FaceEmbedding(embedding) => {
                    embedding.validate()?;
                    if !ids.insert(embedding.id.clone()) {
                        return Err(ZDataError::DuplicateId(embedding.id.clone()));
                    }
                    conservative_size = add_conservative_size(
                        conservative_size,
                        // raw payload = 8-byte header + f32 (4 bytes/value)
                        u64::from(embedding.dimension)
                            .checked_mul(4)
                            .and_then(|bytes| bytes.checked_add(8))
                            .ok_or_else(|| invalid("face embedding dimension overflow"))?,
                        embedding.id.len(),
                    )?;
                }
            }
        }
        ensure_container_size(conservative_size)?;

        let mut bytes = vec![0; HEADER_LEN];
        let mut out_records = Vec::with_capacity(records.len());
        for record in records {
            let (kind, id, tile_x, tile_y, width, height, raw) = match record {
                RecordSpec::MaskTile(tile) => {
                    let mut raw = Vec::with_capacity(tile.values.len() * 2);
                    for value in tile.values {
                        raw.extend_from_slice(&value.to_le_bytes());
                    }
                    (
                        RecordKind::MaskTile,
                        tile.mask_id,
                        tile.tile_x,
                        tile.tile_y,
                        tile.width,
                        tile.height,
                        raw,
                    )
                }
                RecordSpec::RepairRegion(region) => {
                    let raw = region.encode_raw();
                    let id = region.id;
                    (
                        RecordKind::RepairRegion,
                        id,
                        0,
                        0,
                        region.width,
                        region.height,
                        raw,
                    )
                }
                RecordSpec::GenerativeCanvas(canvas) => {
                    let raw = canvas.encode_raw();
                    let id = canvas.id;
                    (
                        RecordKind::GenerativeCanvas,
                        id,
                        0,
                        0,
                        canvas.width,
                        canvas.height,
                        raw,
                    )
                }
                RecordSpec::SpotHealGenerative(spot) => {
                    let raw = spot.encode_raw();
                    let id = spot.id;
                    (
                        RecordKind::SpotHealGenerative,
                        id,
                        0,
                        0,
                        spot.width,
                        spot.height,
                        raw,
                    )
                }
                RecordSpec::DenoiseRgb(denoise) => {
                    let raw = denoise.encode_raw();
                    let id = denoise.id;
                    (
                        RecordKind::DenoiseRgb,
                        id,
                        0,
                        0,
                        denoise.width,
                        denoise.height,
                        raw,
                    )
                }
                RecordSpec::FaceEmbedding(embedding) => {
                    let raw = embedding.encode_raw();
                    let id = embedding.id;
                    (
                        RecordKind::FaceEmbedding,
                        id,
                        0,
                        0,
                        embedding.dimension,
                        1,
                        raw,
                    )
                }
            };
            let compressed = zstd::stream::encode_all(raw.as_slice(), 3)
                .map_err(|e| invalid(format!("zstd compression failed: {e}")))?;
            let id_bytes = id.as_bytes();
            let offset = bytes.len();
            // The final size is only known after compression, so enforce the
            // container limit before every payload append.
            let record_len = RECORD_HEADER_LEN
                .checked_add(id_bytes.len())
                .and_then(|len| len.checked_add(compressed.len()))
                .ok_or_else(|| invalid("container size overflows"))?;
            ensure_container_size(
                bytes
                    .len()
                    .checked_add(record_len)
                    .ok_or_else(|| invalid("container size overflows"))?,
            )?;
            bytes.extend_from_slice(&(id_bytes.len() as u16).to_le_bytes());
            bytes.extend_from_slice(&(kind as u16).to_le_bytes());
            bytes.extend_from_slice(&tile_x.to_le_bytes());
            bytes.extend_from_slice(&tile_y.to_le_bytes());
            bytes.extend_from_slice(&width.to_le_bytes());
            bytes.extend_from_slice(&height.to_le_bytes());
            bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
            bytes.extend_from_slice(&(compressed.len() as u64).to_le_bytes());
            bytes.extend_from_slice(blake3::hash(&raw).as_bytes());
            bytes.extend_from_slice(id_bytes);
            out_records.push(Record {
                kind,
                mask_id: id,
                tile_x,
                tile_y,
                width,
                height,
                offset,
                uncompressed_len: raw.len() as u64,
                compressed_len: compressed.len() as u64,
                checksum: *blake3::hash(&raw).as_bytes(),
            });
            bytes.extend_from_slice(&compressed);
        }
        let index_offset = bytes.len();
        let index_len = out_records.iter().try_fold(0usize, |len, record| {
            let id_len = record.mask_id.len();
            len.checked_add(INDEX_ENTRY_FIXED_LEN)
                .and_then(|len| len.checked_add(id_len))
                .ok_or_else(|| invalid("container size overflows"))
        })?;
        ensure_container_size(
            index_offset
                .checked_add(index_len)
                .ok_or_else(|| invalid("container size overflows"))?,
        )?;
        for record in &out_records {
            let id_bytes = record.mask_id.as_bytes();
            bytes.extend_from_slice(&(id_bytes.len() as u16).to_le_bytes());
            bytes.extend_from_slice(&(record.kind as u16).to_le_bytes());
            bytes.extend_from_slice(&record.tile_x.to_le_bytes());
            bytes.extend_from_slice(&record.tile_y.to_le_bytes());
            bytes.extend_from_slice(&record.width.to_le_bytes());
            bytes.extend_from_slice(&record.height.to_le_bytes());
            bytes.extend_from_slice(&(record.offset as u64).to_le_bytes());
            bytes.extend_from_slice(
                &((RECORD_HEADER_LEN + id_bytes.len() + record.compressed_len as usize) as u64)
                    .to_le_bytes(),
            );
            bytes.extend_from_slice(id_bytes);
        }
        debug_assert_eq!(bytes.len() - index_offset, index_len);
        bytes[0..8].copy_from_slice(MAGIC);
        bytes[8..10].copy_from_slice(&VERSION.to_le_bytes());
        bytes[10..12].copy_from_slice(&(HEADER_LEN as u16).to_le_bytes());
        bytes[12..16].copy_from_slice(&(out_records.len() as u32).to_le_bytes());
        bytes[16..24].copy_from_slice(&(index_offset as u64).to_le_bytes());
        bytes[24..32].copy_from_slice(&(index_len as u64).to_le_bytes());
        Ok(Self {
            bytes,
            records: out_records,
        })
    }

    pub fn to_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn write_to_bytes(&self) -> &[u8] {
        self.to_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ZDataError> {
        if bytes.len() > MAX_CONTAINER_BYTES {
            return Err(invalid("container exceeds size limit"));
        }
        if bytes.len() < HEADER_LEN {
            return Err(invalid("header is truncated"));
        }
        if &bytes[..8] != MAGIC {
            return Err(invalid("invalid magic"));
        }
        let version = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
        if version != VERSION {
            return Err(ZDataError::UnsupportedVersion(version));
        }
        if u16::from_le_bytes(bytes[10..12].try_into().unwrap()) as usize != HEADER_LEN {
            return Err(invalid("invalid header length"));
        }
        let count = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        if count > MAX_RECORDS {
            return Err(invalid("record count exceeds limit"));
        }
        let index_offset = read_u64(bytes, 16)? as usize;
        let index_len = read_u64(bytes, 24)? as usize;
        if index_offset < HEADER_LEN
            || index_offset > bytes.len()
            || index_len > bytes.len().saturating_sub(index_offset)
        {
            return Err(invalid("index is outside container"));
        }
        let mut pos = index_offset;
        let mut records = Vec::with_capacity(count);
        let mut ids = HashSet::new();
        for _ in 0..count {
            if pos.checked_add(INDEX_ENTRY_FIXED_LEN).is_none()
                || pos + INDEX_ENTRY_FIXED_LEN > bytes.len()
                || pos + INDEX_ENTRY_FIXED_LEN > index_offset + index_len
            {
                return Err(invalid("index is truncated"));
            }
            let id_len = u16::from_le_bytes(bytes[pos..pos + 2].try_into().unwrap()) as usize;
            if id_len == 0
                || id_len > MAX_ID_LEN
                || pos + INDEX_ENTRY_FIXED_LEN + id_len > index_offset + index_len
            {
                return Err(invalid("invalid index id length"));
            }
            let kind = RecordKind::from_u16(u16::from_le_bytes(
                bytes[pos + 2..pos + 4].try_into().unwrap(),
            ))?;
            let tile_x = read_u32(bytes, pos + 4)?;
            let tile_y = read_u32(bytes, pos + 8)?;
            let width = read_u32(bytes, pos + 12)?;
            let height = read_u32(bytes, pos + 16)?;
            let offset = read_u64(bytes, pos + 20)? as usize;
            let record_len = read_u64(bytes, pos + 28)? as usize;
            let mask_id = String::from_utf8(
                bytes[pos + INDEX_ENTRY_FIXED_LEN..pos + INDEX_ENTRY_FIXED_LEN + id_len].to_vec(),
            )
            .map_err(|_| invalid("tile id is not UTF-8"))?;
            if !ids.insert(mask_id.clone()) {
                return Err(ZDataError::DuplicateId(mask_id));
            }
            let record_end = offset
                .checked_add(record_len)
                .ok_or_else(|| invalid("record length overflows"))?;
            if offset < HEADER_LEN
                || record_end > index_offset
                || record_len < RECORD_HEADER_LEN + id_len
            {
                return Err(invalid("record is outside data region"));
            }
            let r = parse_record(
                bytes, offset, record_len, &mask_id, kind, tile_x, tile_y, width, height,
            )?;
            records.push(r);
            pos += INDEX_ENTRY_FIXED_LEN + id_len;
        }
        if pos != index_offset + index_len {
            return Err(invalid("index length does not match records"));
        }
        Ok(Self {
            bytes: bytes.to_vec(),
            records,
        })
    }

    pub fn tile(&self, mask_id: &str, tile_x: u32, tile_y: u32) -> Result<MaskTile, ZDataError> {
        let record = self
            .records
            .iter()
            .find(|r| {
                r.kind == RecordKind::MaskTile
                    && r.mask_id == mask_id
                    && r.tile_x == tile_x
                    && r.tile_y == tile_y
            })
            .ok_or_else(|| invalid("tile not found"))?;
        let payload_start = record.offset + RECORD_HEADER_LEN + mask_id.len();
        let payload_end = payload_start + record.compressed_len as usize;
        let raw = decode_payload(
            &self.bytes[payload_start..payload_end],
            record.uncompressed_len,
        )?;
        if blake3::hash(&raw).as_bytes() != &record.checksum {
            return Err(ZDataError::Checksum(mask_id.into()));
        }
        let values = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|v| u16::from_le_bytes(*v))
            .collect();
        Ok(MaskTile {
            mask_id: record.mask_id.clone(),
            tile_x: record.tile_x,
            tile_y: record.tile_y,
            width: record.width,
            height: record.height,
            values,
        })
    }

    pub fn tile_count(&self) -> usize {
        self.records.len()
    }

    /// Reads a repair-region artifact by record id.  Returns
    /// [`ZDataError::Invalid`] if no repair-region record with that id exists,
    /// and [`ZDataError::Checksum`] if the stored payload fails its BLAKE3
    /// check.  The decoded dimensions must agree with the container record.
    pub fn repair_region(&self, id: &str) -> Result<RepairRegionArtifact, ZDataError> {
        let record = self
            .records
            .iter()
            .find(|r| r.kind == RecordKind::RepairRegion && r.mask_id == id)
            .ok_or_else(|| invalid("repair region not found"))?;
        let payload_start = record.offset + RECORD_HEADER_LEN + record.mask_id.len();
        let payload_end = payload_start + record.compressed_len as usize;
        let raw = decode_payload(
            &self.bytes[payload_start..payload_end],
            record.uncompressed_len,
        )?;
        if blake3::hash(&raw).as_bytes() != &record.checksum {
            return Err(ZDataError::Checksum(id.into()));
        }
        let artifact = RepairRegionArtifact::decode_raw(id.to_string(), &raw)?;
        if artifact.width != record.width || artifact.height != record.height {
            return Err(invalid("repair region dimensions disagree with record"));
        }
        Ok(artifact)
    }

    /// Decompresses and returns every record (mask tiles, repair regions,
    /// generative canvases and spot-heal tiles) of
    /// the container.  Used to rebuild a bundle when a new record is appended.
    pub fn decode_all(&self) -> Result<Vec<RecordSpec>, ZDataError> {
        let mut specs = Vec::with_capacity(self.records.len());
        for record in &self.records {
            let payload_start = record.offset + RECORD_HEADER_LEN + record.mask_id.len();
            let payload_end = payload_start + record.compressed_len as usize;
            let raw = decode_payload(
                &self.bytes[payload_start..payload_end],
                record.uncompressed_len,
            )?;
            if blake3::hash(&raw).as_bytes() != &record.checksum {
                return Err(ZDataError::Checksum(record.mask_id.clone()));
            }
            let spec = match record.kind {
                RecordKind::MaskTile => {
                    let values = raw
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|v| u16::from_le_bytes(*v))
                        .collect();
                    RecordSpec::MaskTile(MaskTile {
                        mask_id: record.mask_id.clone(),
                        tile_x: record.tile_x,
                        tile_y: record.tile_y,
                        width: record.width,
                        height: record.height,
                        values,
                    })
                }
                RecordKind::RepairRegion => RecordSpec::RepairRegion(
                    RepairRegionArtifact::decode_raw(record.mask_id.clone(), &raw)?,
                ),
                RecordKind::GenerativeCanvas => RecordSpec::GenerativeCanvas(
                    GenerativeCanvasArtifact::decode_raw(record.mask_id.clone(), &raw)?,
                ),
                RecordKind::SpotHealGenerative => RecordSpec::SpotHealGenerative(
                    SpotHealGenerativeArtifact::decode_raw(record.mask_id.clone(), &raw)?,
                ),
                RecordKind::DenoiseRgb => RecordSpec::DenoiseRgb(DenoiseRgbArtifact::decode_raw(
                    record.mask_id.clone(),
                    &raw,
                )?),
                RecordKind::FaceEmbedding => RecordSpec::FaceEmbedding(
                    FaceEmbeddingArtifact::decode_raw(record.mask_id.clone(), &raw)?,
                ),
            };
            specs.push(spec);
        }
        Ok(specs)
    }

    /// Verifies the BLAKE3 checksum of *every* record eagerly.
    ///
    /// REVIEW-SIDECAR-ZDATA-1: checksums used to be checked only lazily when a
    /// tile/region payload was decoded, so a corrupt container could load
    /// "successfully" and the corruption surfaced much later (or never, for
    /// records nobody touched). Loading a container from disk now proves the
    /// whole bundle intact up front; random access afterwards still re-checks
    /// as before. Cost: one decompression pass over all payloads at load time
    /// — accepted deliberately, because a silently usable-but-corrupt mask
    /// violates the no-silent-fallback principle.
    pub fn verify_checksums(&self) -> Result<(), ZDataError> {
        for record in &self.records {
            let payload_start = record.offset + RECORD_HEADER_LEN + record.mask_id.len();
            let payload_end = payload_start + record.compressed_len as usize;
            let raw = decode_payload(
                &self.bytes[payload_start..payload_end],
                record.uncompressed_len,
            )?;
            if blake3::hash(&raw).as_bytes() != &record.checksum {
                return Err(ZDataError::Checksum(record.mask_id.clone()));
            }
        }
        Ok(())
    }

    /// Returns a new container with `region` appended to this one.  The existing
    /// records are decompressed and re-serialized, so a single bundle keeps both
    /// mask tiles and repair regions.  A duplicate id is rejected.
    pub fn add_repair_region(&self, region: RepairRegionArtifact) -> Result<Self, ZDataError> {
        let mut specs = self.decode_all()?;
        if self.has_id(&specs, &region.id) {
            return Err(ZDataError::DuplicateId(region.id));
        }
        specs.push(RecordSpec::RepairRegion(region));
        Self::new_with(specs)
    }

    /// Reads a GEN-EXPAND-1 generative canvas by record id.  Returns
    /// [`ZDataError::Invalid`] if no canvas record with that id exists, and
    /// [`ZDataError::Checksum`] if the stored payload fails its BLAKE3 check.
    pub fn generative_canvas(&self, id: &str) -> Result<GenerativeCanvasArtifact, ZDataError> {
        let record = self
            .records
            .iter()
            .find(|r| r.kind == RecordKind::GenerativeCanvas && r.mask_id == id)
            .ok_or_else(|| invalid("generative canvas not found"))?;
        let payload_start = record.offset + RECORD_HEADER_LEN + record.mask_id.len();
        let payload_end = payload_start + record.compressed_len as usize;
        let raw = decode_payload(
            &self.bytes[payload_start..payload_end],
            record.uncompressed_len,
        )?;
        if blake3::hash(&raw).as_bytes() != &record.checksum {
            return Err(ZDataError::Checksum(id.into()));
        }
        let artifact = GenerativeCanvasArtifact::decode_raw(id.to_string(), &raw)?;
        if artifact.width != record.width || artifact.height != record.height {
            return Err(invalid("generative canvas dimensions disagree with record"));
        }
        Ok(artifact)
    }

    /// Reads a SPOT-REMOVE-1 generative spot-heal tile by record id.  Kind
    /// separation is strict: a `generative_canvas` record with the same id is
    /// never returned here.
    pub fn spot_heal_generative(&self, id: &str) -> Result<SpotHealGenerativeArtifact, ZDataError> {
        let record = self
            .records
            .iter()
            .find(|r| r.kind == RecordKind::SpotHealGenerative && r.mask_id == id)
            .ok_or_else(|| invalid("spot heal artifact not found"))?;
        let payload_start = record.offset + RECORD_HEADER_LEN + record.mask_id.len();
        let payload_end = payload_start + record.compressed_len as usize;
        let raw = decode_payload(
            &self.bytes[payload_start..payload_end],
            record.uncompressed_len,
        )?;
        if blake3::hash(&raw).as_bytes() != &record.checksum {
            return Err(ZDataError::Checksum(id.into()));
        }
        let artifact = SpotHealGenerativeArtifact::decode_raw(id.to_string(), &raw)?;
        if artifact.width != record.width || artifact.height != record.height {
            return Err(invalid("spot heal dimensions disagree with record"));
        }
        Ok(artifact)
    }

    /// Reads a LRPAR-G14-DENOISE-IMPL-20 `denoise_rgb` record by id.  Kind
    /// separation is strict: records of any other kind with the same id are
    /// never returned here.  Returns [`ZDataError::Invalid`] when no such
    /// record exists and [`ZDataError::Checksum`] when the stored payload
    /// fails its BLAKE3 check.  The decoded dimensions must agree with the
    /// container record.
    pub fn denoise_rgb(&self, id: &str) -> Result<DenoiseRgbArtifact, ZDataError> {
        let record = self
            .records
            .iter()
            .find(|r| r.kind == RecordKind::DenoiseRgb && r.mask_id == id)
            .ok_or_else(|| invalid("denoise artifact not found"))?;
        let payload_start = record.offset + RECORD_HEADER_LEN + record.mask_id.len();
        let payload_end = payload_start + record.compressed_len as usize;
        let raw = decode_payload(
            &self.bytes[payload_start..payload_end],
            record.uncompressed_len,
        )?;
        if blake3::hash(&raw).as_bytes() != &record.checksum {
            return Err(ZDataError::Checksum(id.into()));
        }
        let artifact = DenoiseRgbArtifact::decode_raw(id.to_string(), &raw)?;
        if artifact.width != record.width || artifact.height != record.height {
            return Err(invalid("denoise dimensions disagree with record"));
        }
        Ok(artifact)
    }

    /// Reads a LRPAR-G12-FACE-20 `face_embedding` record by id.  Kind
    /// separation is strict: records of any other kind with the same id are
    /// never returned here.  Returns [`ZDataError::Invalid`] when no such
    /// record exists and [`ZDataError::Checksum`] when the stored payload
    /// fails its BLAKE3 check.  The decoded dimension must agree with the
    /// container record (`width`).
    pub fn face_embedding(&self, id: &str) -> Result<FaceEmbeddingArtifact, ZDataError> {
        let record = self
            .records
            .iter()
            .find(|r| r.kind == RecordKind::FaceEmbedding && r.mask_id == id)
            .ok_or_else(|| invalid("face embedding not found"))?;
        let payload_start = record.offset + RECORD_HEADER_LEN + record.mask_id.len();
        let payload_end = payload_start + record.compressed_len as usize;
        let raw = decode_payload(
            &self.bytes[payload_start..payload_end],
            record.uncompressed_len,
        )?;
        if blake3::hash(&raw).as_bytes() != &record.checksum {
            return Err(ZDataError::Checksum(id.into()));
        }
        let artifact = FaceEmbeddingArtifact::decode_raw(id.to_string(), &raw)?;
        if artifact.dimension != record.width || record.height != 1 {
            return Err(invalid("face embedding dimension disagrees with record"));
        }
        Ok(artifact)
    }

    /// Whether a record of `kind` with `id` exists. Used by the face evidence
    /// contract to distinguish an absent payload (`missing`) from a corrupt
    /// one (`corrupt`) without interpreting error strings.
    #[must_use]
    pub fn has_record(&self, kind: RecordKind, id: &str) -> bool {
        self.records
            .iter()
            .any(|record| record.kind == kind && record.mask_id == id)
    }

    fn has_id(&self, specs: &[RecordSpec], id: &str) -> bool {
        specs.iter().any(|spec| spec_id(spec) == id)
    }

    /// Returns a new container with `canvas` appended.  Existing records of
    /// every kind are preserved; a duplicate id (across all kinds) is
    /// rejected.
    pub fn add_generative_canvas(
        &self,
        canvas: GenerativeCanvasArtifact,
    ) -> Result<Self, ZDataError> {
        let mut specs = self.decode_all()?;
        if self.has_id(&specs, &canvas.id) {
            return Err(ZDataError::DuplicateId(canvas.id));
        }
        specs.push(RecordSpec::GenerativeCanvas(canvas));
        Self::new_with(specs)
    }

    /// GEN-ONNX-1: returns a new container with the `generative_canvas` record
    /// `canvas.id` *explicitly replaced*.
    ///
    /// Unlike [`Self::add_generative_canvas`] this is the deliberate
    /// regeneration path (the caller explicitly asked to re-generate). It never
    /// touches records of other kinds and still rejects an id collision with a
    /// different kind, so nothing is overwritten silently.
    pub fn replace_generative_canvas(
        &self,
        canvas: GenerativeCanvasArtifact,
    ) -> Result<Self, ZDataError> {
        canvas.validate()?;
        let mut specs = self.decode_all()?;
        specs.retain(|spec| {
            !matches!(spec, RecordSpec::GenerativeCanvas(existing) if existing.id == canvas.id)
        });
        if self.has_id(&specs, &canvas.id) {
            return Err(ZDataError::DuplicateId(canvas.id));
        }
        specs.push(RecordSpec::GenerativeCanvas(canvas));
        Self::new_with(specs)
    }

    /// Returns a new container with `spot` appended.  Existing records of
    /// every kind are preserved; a duplicate id (across all kinds) is
    /// rejected.
    pub fn add_spot_heal_generative(
        &self,
        spot: SpotHealGenerativeArtifact,
    ) -> Result<Self, ZDataError> {
        let mut specs = self.decode_all()?;
        if self.has_id(&specs, &spot.id) {
            return Err(ZDataError::DuplicateId(spot.id));
        }
        specs.push(RecordSpec::SpotHealGenerative(spot));
        Self::new_with(specs)
    }

    /// Returns a new container with `denoise` appended.  Existing records of
    /// every kind are preserved; a duplicate id (across all kinds) is
    /// rejected.
    pub fn add_denoise_rgb(&self, denoise: DenoiseRgbArtifact) -> Result<Self, ZDataError> {
        let mut specs = self.decode_all()?;
        if self.has_id(&specs, &denoise.id) {
            return Err(ZDataError::DuplicateId(denoise.id));
        }
        specs.push(RecordSpec::DenoiseRgb(denoise));
        Self::new_with(specs)
    }

    /// Explicit regeneration path for a `denoise_rgb` record: replaces the
    /// record `denoise.id` and only that record, never silently touching other
    /// kinds.  An id collision with a different kind is still rejected.
    pub fn replace_denoise_rgb(&self, denoise: DenoiseRgbArtifact) -> Result<Self, ZDataError> {
        denoise.validate()?;
        let mut specs = self.decode_all()?;
        specs.retain(
            |spec| !matches!(spec, RecordSpec::DenoiseRgb(existing) if existing.id == denoise.id),
        );
        if self.has_id(&specs, &denoise.id) {
            return Err(ZDataError::DuplicateId(denoise.id));
        }
        specs.push(RecordSpec::DenoiseRgb(denoise));
        Self::new_with(specs)
    }

    /// Returns a new container with `embedding` appended.  Existing records of
    /// every kind are preserved; a duplicate id (across all kinds) is
    /// rejected.
    pub fn add_face_embedding(&self, embedding: FaceEmbeddingArtifact) -> Result<Self, ZDataError> {
        let mut specs = self.decode_all()?;
        if self.has_id(&specs, &embedding.id) {
            return Err(ZDataError::DuplicateId(embedding.id));
        }
        specs.push(RecordSpec::FaceEmbedding(embedding));
        Self::new_with(specs)
    }

    /// Explicit replacement path for a `face_embedding` record: replaces only
    /// the record with the same id, never silently touching other records or
    /// kinds.  An id collision with a different kind is still rejected.
    pub fn replace_face_embedding(
        &self,
        embedding: FaceEmbeddingArtifact,
    ) -> Result<Self, ZDataError> {
        embedding.validate()?;
        let mut specs = self.decode_all()?;
        specs.retain(
            |spec| !matches!(spec, RecordSpec::FaceEmbedding(existing) if existing.id == embedding.id),
        );
        if self.has_id(&specs, &embedding.id) {
            return Err(ZDataError::DuplicateId(embedding.id));
        }
        specs.push(RecordSpec::FaceEmbedding(embedding));
        Self::new_with(specs)
    }
}
