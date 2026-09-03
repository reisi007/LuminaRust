//! The optional `.lumina.zdata` tile container.  This module is deliberately
//! feature-gated: Zstd and BLAKE3 are native-sidecar capabilities, not core/WASM
//! requirements.
//!
//! # Container layout (version 1)
//!
//! ```text
//! [HEADER 40 bytes]
//!   bytes 0..8    : MAGIC      = b"LUMZDATA"
//!   bytes 8..10   : VERSION    = 1 (u16 LE)
//!   bytes 10..12  : HEADER_LEN = 40 (u16 LE)
//!   bytes 12..16  : record count (u32 LE)
//!   bytes 16..24  : index offset (u64 LE)
//!   bytes 24..32  : index length (u64 LE)
//! [RECORDS ...]
//!   per record:
//!     RECORD_HEADER (68 bytes):
//!       id_len      : u16 LE
//!       kind       : u16 LE   (0 = mask tile, 1 = repair region,
//!                              2 = generative canvas, 3 = spot heal generative)
//!       tile_x     : u32 LE   (non-tile records always use 0/0)
//!       tile_y     : u32 LE
//!       width      : u32 LE
//!       height     : u32 LE
//!       uncompressed_len : u64 LE
//!       compressed_len   : u64 LE
//!       checksum   : 32 bytes BLAKE3 of the raw payload
//!     id            : id_len UTF-8 bytes
//!     payload       : zstd-compressed raw bytes
//! [INDEX ...]
//!   per entry (INDEX_ENTRY_FIXED_LEN = 36 bytes):
//!     id_len      : u16 LE
//!     kind        : u16 LE
//!     tile_x/tile_y/width/height : u32 LE each
//!     offset      : u64 LE
//!     record_len  : u64 LE
//!     id          : id_len UTF-8 bytes
//! ```
//!
//! F-042-N1 reuses this single bundle for **repair-region artifacts** (dust
//! removal / AI replacement) alongside the existing mask tiles.  A per-record
//! `kind` discriminator (reusing the previously-reserved header slot) keeps the
//! two payload types apart **without bumping the container `VERSION`**.  The
//! container keeps its existing versioning, atomic write (`save_zdata`) and
//! BLAKE3 checksum semantics; only the meaning of the reserved `kind` word
//! changes.
//!
//! ## Repair-region raw payload (before zstd)
//!
//! ```text
//!   encoding_version : u32 LE  (= 1)
//!   width            : u32 LE
//!   height           : u32 LE
//!   region           : width*height little-endian u16 values (0..=u16::MAX),
//!                      the `MaskPlane`-compatible source-action region
//!   replacement      : width*height*4 RGBA8 bytes, the replacement image
//! ```
//!
//! `region` and `replacement` MUST have identical dimensions; this is enforced
//! on write and on read (the decoded raw must split exactly into
//! `width*height` u16 values and `width*height*4` RGBA8 bytes).  The whole raw
//! payload is BLAKE3-checksummed; `RepairRegionArtifact::checksum()` returns the
//! same canonical hex digest that is stored in the recipe's
//! `SourceActionArtifactRef.checksum`, so the recipe and the bundle stay aligned.
//!
//! ## Generative RGBA payload (before zstd)
//!
//! GEN-ZDATA-PERSIST stores two further RGBA8 artifacts in the same bundle
//! with distinct `kind` discriminators (container `VERSION` stays 1):
//!
//! * `GenerativeCanvasArtifact` (`kind = 2`, GEN-EXPAND-1 `generative_canvas`):
//!   the full composited canvas including unchanged source pixels.
//! * `SpotHealGenerativeArtifact` (`kind = 3`, SPOT-REMOVE-1
//!   `spot_heal_generative`): the replaced pixels of the spot tile (no canvas
//!   expansion).
//!
//! ```text
//!   encoding_version : u32 LE  (= 1)
//!   width            : u32 LE
//!   height           : u32 LE
//!   pixels           : width*height*4 RGBA8 bytes, row-major
//! ```
//!
//! The raw payload is BLAKE3-checksummed (`checksum()`); the JSON recipe keeps
//! only a portable `ArtifactReference` (relative path, format, checksum,
//! resolution, channel type, data version) — never absolute paths, never raw
//! pixels.

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use thiserror::Error;

const MAGIC: &[u8; 8] = b"LUMZDATA";
const VERSION: u16 = 1;
const HEADER_LEN: usize = 40;
const REPAIR_ENCODING_VERSION: u32 = 1;
const RGBA_ENCODING_VERSION: u32 = 1;
const RECORD_HEADER_LEN: usize = 68;
const INDEX_ENTRY_FIXED_LEN: usize = 36;
const MAX_CONTAINER_BYTES: usize = 512 * 1024 * 1024;
const MAX_ID_LEN: usize = 4096;
const MAX_DIMENSION: u32 = 16_384;
const MAX_TILE_VALUES: u64 = 64 * 1024 * 1024;
const MAX_UNCOMPRESSED: u64 = MAX_TILE_VALUES * 2;
const MAX_COMPRESSED: u64 = 128 * 1024 * 1024;
const MAX_RECORDS: usize = 1_000_000;

/// A single non-empty little-endian `uint16` mask tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskTile {
    pub mask_id: String,
    pub tile_x: u32,
    pub tile_y: u32,
    pub width: u32,
    pub height: u32,
    pub values: Vec<u16>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ZDataError {
    #[error("zdata is truncated or malformed: {0}")]
    Invalid(String),
    #[error("unsupported zdata format version {0}")]
    UnsupportedVersion(u16),
    #[error("zdata I/O failed while {operation} `{path}`: {message}")]
    Io {
        operation: String,
        path: String,
        message: String,
    },
    #[error("zdata checksum mismatch for tile `{0}`")]
    Checksum(String),
    #[error("duplicate zdata tile id `{0}`")]
    DuplicateId(String),
}

/// Discriminator for the payload types that share one `.lumina.zdata`
/// bundle.  Stored in the record-header / index slot that was previously a
/// reserved zero word, so the on-disk container `VERSION` (1) is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    /// A `uint16` mask tile (the original payload type).
    MaskTile = 0,
    /// An F-042-N1 repair-region artifact (u16 region + RGBA8 replacement).
    RepairRegion = 1,
    /// A GEN-EXPAND-1 generative canvas (full composited RGBA8 canvas).
    GenerativeCanvas = 2,
    /// A SPOT-REMOVE-1 generative spot-heal tile (RGBA8, no canvas expansion).
    SpotHealGenerative = 3,
}

impl RecordKind {
    fn from_u16(value: u16) -> Result<Self, ZDataError> {
        match value {
            0 => Ok(RecordKind::MaskTile),
            1 => Ok(RecordKind::RepairRegion),
            2 => Ok(RecordKind::GenerativeCanvas),
            3 => Ok(RecordKind::SpotHealGenerative),
            other => Err(invalid(format!("unsupported zdata record kind {other}"))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            RecordKind::MaskTile => "mask_tile",
            RecordKind::RepairRegion => "repair_region",
            RecordKind::GenerativeCanvas => "generative_canvas",
            RecordKind::SpotHealGenerative => "spot_heal_generative",
        }
    }
}

/// An F-042-N1 repair-region artifact stored in the `.lumina.zdata` bundle.
///
/// `region` is a `MaskPlane`-compatible `uint16` plane (`0..=u16::MAX`) and
/// `replacement` is an RGBA8 image.  Both MUST have identical dimensions; the
/// relation is enforced on write and read.  The artifact is applied by the core
/// render path as `out = replacement` for pixels with `region >= 32768`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairRegionArtifact {
    pub id: String,
    pub width: u32,
    pub height: u32,
    /// Row-major little-endian `u16` region values, `width * height` long.
    pub region: Vec<u16>,
    /// Row-major RGBA8 replacement pixels, `width * height * 4` long.
    pub replacement: Vec<u8>,
}

impl RepairRegionArtifact {
    /// Validates id, dimensions and value/byte counts (region and replacement
    /// must have identical dimensions).
    pub fn validate(&self) -> Result<(), ZDataError> {
        if self.id.is_empty()
            || self.id.len() > MAX_ID_LEN
            || !self.id.is_char_boundary(self.id.len())
        {
            return Err(invalid("invalid repair region id"));
        }
        if self.width == 0
            || self.height == 0
            || self.width > MAX_DIMENSION
            || self.height > MAX_DIMENSION
        {
            return Err(invalid("invalid repair region dimensions"));
        }
        let count = u64::from(self.width)
            .checked_mul(u64::from(self.height))
            .ok_or_else(|| invalid("repair region dimensions overflow"))?;
        if count > MAX_UNCOMPRESSED / 6 {
            return Err(invalid("repair region exceeds payload size limit"));
        }
        if self.region.len() != count as usize {
            return Err(invalid("repair region plane does not match dimensions"));
        }
        if self.replacement.len() != count as usize * 4 {
            return Err(invalid(
                "repair region replacement does not match dimensions",
            ));
        }
        Ok(())
    }

    /// Canonical pre-compression encoding (encoding version + dimensions +
    /// region + replacement).  This is exactly what the container checksum and
    /// [`RepairRegionArtifact::checksum`] cover.
    fn encode_raw(&self) -> Vec<u8> {
        let mut raw = Vec::with_capacity(12 + self.region.len() * 2 + self.replacement.len());
        raw.extend_from_slice(&REPAIR_ENCODING_VERSION.to_le_bytes());
        raw.extend_from_slice(&self.width.to_le_bytes());
        raw.extend_from_slice(&self.height.to_le_bytes());
        for value in &self.region {
            raw.extend_from_slice(&value.to_le_bytes());
        }
        raw.extend_from_slice(&self.replacement);
        raw
    }

    /// Reconstructs an artifact from its canonical raw payload.  Verifies the
    /// encoding version and that the payload splits exactly into `width*height`
    /// u16 region values and `width*height*4` RGBA8 replacement bytes.
    fn decode_raw(id: String, raw: &[u8]) -> Result<Self, ZDataError> {
        if raw.len() < 12 {
            return Err(invalid("repair region payload is truncated"));
        }
        let version = u32::from_le_bytes(raw[0..4].try_into().unwrap());
        if version != REPAIR_ENCODING_VERSION {
            return Err(invalid("unsupported repair region encoding version"));
        }
        let width = u32::from_le_bytes(raw[4..8].try_into().unwrap());
        let height = u32::from_le_bytes(raw[8..12].try_into().unwrap());
        let count = width as usize * height as usize;
        let region_end = 12 + count * 2;
        if raw.len() != region_end + count * 4 {
            return Err(invalid(
                "repair region payload length does not match dimensions",
            ));
        }
        let region = raw[12..region_end]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|bytes| u16::from_le_bytes(*bytes))
            .collect();
        let replacement = raw[region_end..].to_vec();
        let artifact = RepairRegionArtifact {
            id,
            width,
            height,
            region,
            replacement,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    /// BLAKE3 hex digest of the canonical raw payload.  This is the checksum
    /// stored in the recipe's `SourceActionArtifactRef.checksum` and must match
    /// the container record checksum, so recipe and bundle stay aligned.
    pub fn checksum(&self) -> String {
        blake3::hash(&self.encode_raw()).to_hex().to_string()
    }

    /// Stable kind string used by diagnostics (`repair_region`).
    pub fn kind_str(&self) -> &'static str {
        RecordKind::RepairRegion.as_str()
    }
}

/// Shared validation for RGBA8 generative payloads (canvas + spot heal):
/// non-empty portable id, non-zero dimensions within [`MAX_DIMENSION`], and
/// exactly `width*height*4` pixel bytes inside the container size budget.
fn validate_rgba_payload(
    id: &str,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<(), ZDataError> {
    if id.is_empty() || id.len() > MAX_ID_LEN || !id.is_char_boundary(id.len()) {
        return Err(invalid("invalid generative artifact id"));
    }
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(invalid("invalid generative artifact dimensions"));
    }
    let count = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| invalid("generative artifact dimensions overflow"))?;
    if count > MAX_UNCOMPRESSED / 4 {
        return Err(invalid("generative artifact exceeds payload size limit"));
    }
    if pixels.len() != count as usize * 4 {
        return Err(invalid(
            "generative artifact pixels do not match dimensions",
        ));
    }
    Ok(())
}

/// Shared canonical RGBA8 encoding (`encoding_version + width + height +
/// pixels`).  Exactly what the container checksum covers.
fn encode_rgba_raw(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(12 + pixels.len());
    raw.extend_from_slice(&RGBA_ENCODING_VERSION.to_le_bytes());
    raw.extend_from_slice(&width.to_le_bytes());
    raw.extend_from_slice(&height.to_le_bytes());
    raw.extend_from_slice(pixels);
    raw
}

/// Shared RGBA8 decoding: verifies the encoding version and that the payload
/// splits exactly into `width*height*4` bytes.
fn decode_rgba_raw(raw: &[u8]) -> Result<(u32, u32, Vec<u8>), ZDataError> {
    if raw.len() < 12 {
        return Err(invalid("generative artifact payload is truncated"));
    }
    let version = u32::from_le_bytes(raw[0..4].try_into().unwrap());
    if version != RGBA_ENCODING_VERSION {
        return Err(invalid("unsupported generative artifact encoding version"));
    }
    let width = u32::from_le_bytes(raw[4..8].try_into().unwrap());
    let height = u32::from_le_bytes(raw[8..12].try_into().unwrap());
    let count = width as usize * height as usize;
    if raw.len() != 12 + count * 4 {
        return Err(invalid(
            "generative artifact payload length does not match dimensions",
        ));
    }
    Ok((width, height, raw[12..].to_vec()))
}

/// A GEN-EXPAND-1 generative canvas stored in the `.lumina.zdata` bundle.
///
/// `pixels` is the **full composited canvas** (including unchanged source
/// pixels) as row-major RGBA8, `width*height*4` bytes.  The JSON recipe keeps
/// only a portable `ArtifactReference` (relative path, format, BLAKE3
/// checksum, resolution, channel type, data version).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerativeCanvasArtifact {
    pub id: String,
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA8 canvas pixels, `width * height * 4` long.
    pub pixels: Vec<u8>,
}

impl GenerativeCanvasArtifact {
    /// Validates id, dimensions and pixel-byte count.
    pub fn validate(&self) -> Result<(), ZDataError> {
        validate_rgba_payload(&self.id, self.width, self.height, &self.pixels)
    }

    fn encode_raw(&self) -> Vec<u8> {
        encode_rgba_raw(self.width, self.height, &self.pixels)
    }

    fn decode_raw(id: String, raw: &[u8]) -> Result<Self, ZDataError> {
        let (width, height, pixels) = decode_rgba_raw(raw)?;
        let artifact = GenerativeCanvasArtifact {
            id,
            width,
            height,
            pixels,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    /// BLAKE3 hex digest of the canonical raw payload (uncompressed RGBA8
    /// stream).  Stored in the recipe's `ArtifactReference.checksum`.
    pub fn checksum(&self) -> String {
        blake3::hash(&self.encode_raw()).to_hex().to_string()
    }

    /// Stable kind string used by diagnostics (`generative_canvas`).
    pub fn kind_str(&self) -> &'static str {
        RecordKind::GenerativeCanvas.as_str()
    }
}

/// A SPOT-REMOVE-1 generative spot-heal tile stored in the `.lumina.zdata`
/// bundle.
///
/// Unlike [`GenerativeCanvasArtifact`] this carries only the replaced spot
/// tile (no canvas expansion) as row-major RGBA8, `width*height*4` bytes.
/// Separate `kind` discriminator so canvas and spot records never mix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotHealGenerativeArtifact {
    pub id: String,
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA8 replacement pixels, `width * height * 4` long.
    pub pixels: Vec<u8>,
}

impl SpotHealGenerativeArtifact {
    /// Validates id, dimensions and pixel-byte count.
    pub fn validate(&self) -> Result<(), ZDataError> {
        validate_rgba_payload(&self.id, self.width, self.height, &self.pixels)
    }

    fn encode_raw(&self) -> Vec<u8> {
        encode_rgba_raw(self.width, self.height, &self.pixels)
    }

    fn decode_raw(id: String, raw: &[u8]) -> Result<Self, ZDataError> {
        let (width, height, pixels) = decode_rgba_raw(raw)?;
        let artifact = SpotHealGenerativeArtifact {
            id,
            width,
            height,
            pixels,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    /// BLAKE3 hex digest of the canonical raw payload (uncompressed RGBA8
    /// stream).  Stored in the recipe's `ArtifactReference.checksum`.
    pub fn checksum(&self) -> String {
        blake3::hash(&self.encode_raw()).to_hex().to_string()
    }

    /// Stable kind string used by diagnostics (`spot_heal_generative`).
    pub fn kind_str(&self) -> &'static str {
        RecordKind::SpotHealGenerative.as_str()
    }
}

/// A single bundle entry, used by the unified constructor and re-reads.
#[derive(Debug, Clone)]
pub enum RecordSpec {
    MaskTile(MaskTile),
    RepairRegion(RepairRegionArtifact),
    GenerativeCanvas(GenerativeCanvasArtifact),
    SpotHealGenerative(SpotHealGenerativeArtifact),
}

#[derive(Debug, Clone)]
struct Record {
    kind: RecordKind,
    mask_id: String,
    tile_x: u32,
    tile_y: u32,
    width: u32,
    height: u32,
    offset: usize,
    uncompressed_len: u64,
    compressed_len: u64,
    checksum: [u8; 32],
}

/// An indexed container. It owns compressed bytes and metadata, not duplicate
/// decompressed tile values; values are allocated only for the requested tile.
#[derive(Debug, Clone)]
pub struct ZDataContainer {
    bytes: Vec<u8>,
    records: Vec<Record>,
}

pub fn zdata_path_for(source: &Path) -> PathBuf {
    let filename = source
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    source.with_file_name(format!("{filename}.lumina.zdata"))
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
        if specs.iter().any(|spec| match spec {
            RecordSpec::MaskTile(tile) => tile.mask_id == region.id,
            RecordSpec::RepairRegion(existing) => existing.id == region.id,
            RecordSpec::GenerativeCanvas(existing) => existing.id == region.id,
            RecordSpec::SpotHealGenerative(existing) => existing.id == region.id,
        }) {
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

    fn has_id(&self, specs: &[RecordSpec], id: &str) -> bool {
        specs.iter().any(|spec| match spec {
            RecordSpec::MaskTile(tile) => tile.mask_id == id,
            RecordSpec::RepairRegion(existing) => existing.id == id,
            RecordSpec::GenerativeCanvas(existing) => existing.id == id,
            RecordSpec::SpotHealGenerative(existing) => existing.id == id,
        })
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
}

pub fn load_zdata(path: &Path) -> Result<ZDataContainer, ZDataError> {
    let metadata = fs::metadata(path).map_err(|e| io_error("reading metadata for", path, e))?;
    if metadata.len() > MAX_CONTAINER_BYTES as u64 {
        return Err(invalid(format!(
            "container file exceeds size limit of {MAX_CONTAINER_BYTES} bytes"
        )));
    }
    let mut file = fs::File::open(path).map_err(|e| io_error("opening", path, e))?;
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(MAX_CONTAINER_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io_error("reading", path, e))?;
    if bytes.len() > MAX_CONTAINER_BYTES {
        return Err(invalid(format!(
            "container file exceeds size limit of {MAX_CONTAINER_BYTES} bytes"
        )));
    }
    let container = ZDataContainer::from_bytes(&bytes)?;
    // REVIEW-SIDECAR-ZDATA-1: prove the whole bundle intact at load time
    // instead of discovering corruption lazily on first tile access.
    container.verify_checksums()?;
    Ok(container)
}

/// Maps the shared write-lock error into the zdata error domain. The lock
/// itself (`.zdata.lock`) and its stale-reclaim semantics live in `lib.rs` so
/// JSON sidecar and zdata bundle writers serialize identically.
fn lock_error(path: &Path, error: crate::SidecarError) -> ZDataError {
    ZDataError::Io {
        operation: "acquiring zdata write lock".into(),
        path: path.display().to_string(),
        message: error.to_string(),
    }
}

/// Appends a repair-region artifact to the bundle at `path` (creating the
/// bundle if it does not yet exist) and writes it atomically.  Pre-existing
/// mask tiles and repair regions are preserved.  A duplicate id or a
/// region/replacement dimension mismatch is reported as an error.
///
/// REVIEW-SIDECAR-ZDATA-1: this read-modify-write runs under the bundle's
/// `.zdata.lock`, so two concurrent appends serialize instead of one silently
/// overwriting the other's freshly appended region (lost update / dangling
/// recipe references). The same lock guards [`save_zdata`].
pub fn append_repair_region(path: &Path, region: RepairRegionArtifact) -> Result<(), ZDataError> {
    let _lock = crate::acquire_write_lock(path).map_err(|error| lock_error(path, error))?;
    let existing = if path.exists() {
        Some(load_zdata(path)?)
    } else {
        None
    };
    let container = match existing {
        Some(container) => container.add_repair_region(region)?,
        None => ZDataContainer::new(vec![])?.add_repair_region(region)?,
    };
    save_zdata_locked(path, &container)
}

/// Appends a GEN-EXPAND-1 generative canvas to the bundle at `path` (creating
/// it if needed), preserving every pre-existing record.  Runs under the
/// bundle's `.zdata.lock` and writes atomically (Temp + Rename); a duplicate
/// id or a pixel/dimension mismatch is reported as an error.
pub fn append_generative_canvas(
    path: &Path,
    canvas: GenerativeCanvasArtifact,
) -> Result<(), ZDataError> {
    let _lock = crate::acquire_write_lock(path).map_err(|error| lock_error(path, error))?;
    let existing = if path.exists() {
        Some(load_zdata(path)?)
    } else {
        None
    };
    let container = match existing {
        Some(container) => container.add_generative_canvas(canvas)?,
        None => ZDataContainer::new(vec![])?.add_generative_canvas(canvas)?,
    };
    save_zdata_locked(path, &container)
}

/// Appends a SPOT-REMOVE-1 generative spot-heal tile to the bundle at `path`
/// (creating it if needed), preserving every pre-existing record.  Same
/// locking/atomicity contract as [`append_generative_canvas`]; the two
/// generative kinds stay strictly separated by their `kind` discriminator.
pub fn append_spot_heal_generative(
    path: &Path,
    spot: SpotHealGenerativeArtifact,
) -> Result<(), ZDataError> {
    let _lock = crate::acquire_write_lock(path).map_err(|error| lock_error(path, error))?;
    let existing = if path.exists() {
        Some(load_zdata(path)?)
    } else {
        None
    };
    let container = match existing {
        Some(container) => container.add_spot_heal_generative(spot)?,
        None => ZDataContainer::new(vec![])?.add_spot_heal_generative(spot)?,
    };
    save_zdata_locked(path, &container)
}

pub fn save_zdata(path: &Path, container: &ZDataContainer) -> Result<(), ZDataError> {
    // REVIEW-SIDECAR-ZDATA-1: plain saves take the same `.zdata.lock` as
    // `append_repair_region`, so an append can never be overwritten mid-flight
    // by a concurrent whole-container save.
    let _lock = crate::acquire_write_lock(path).map_err(|error| lock_error(path, error))?;
    save_zdata_locked(path, container)
}

/// Atomic replace; caller must hold the bundle's write lock.
fn save_zdata_locked(path: &Path, container: &ZDataContainer) -> Result<(), ZDataError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_else(|| "zdata".into());
    let mut temporary = tempfile::Builder::new()
        .prefix(&format!(".{filename}.tmp-"))
        .tempfile_in(parent)
        .map_err(|e| io_error("creating temporary file", parent, e))?;
    let temporary_path = temporary.path().to_path_buf();
    temporary
        .write_all(container.to_bytes())
        .map_err(|e| io_error("writing temporary file", &temporary_path, e))?;
    temporary
        .flush()
        .map_err(|e| io_error("flushing temporary file", &temporary_path, e))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|e| io_error("syncing temporary file", &temporary_path, e))?;
    temporary
        .persist(path)
        .map_err(|e| io_error("renaming temporary file", path, e.error))?;
    Ok(())
}

fn validate_tile(tile: &MaskTile) -> Result<(), ZDataError> {
    if tile.mask_id.is_empty()
        || tile.mask_id.len() > MAX_ID_LEN
        || !tile.mask_id.is_char_boundary(tile.mask_id.len())
    {
        return Err(invalid("invalid tile id"));
    }
    if tile.width == 0
        || tile.height == 0
        || tile.width > MAX_DIMENSION
        || tile.height > MAX_DIMENSION
    {
        return Err(invalid("invalid tile dimensions"));
    }
    let count = u64::from(tile.width)
        .checked_mul(u64::from(tile.height))
        .ok_or_else(|| invalid("tile dimensions overflow"))?;
    if count > MAX_TILE_VALUES || count != tile.values.len() as u64 {
        return Err(invalid("tile dimensions do not match value count"));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn parse_record(
    bytes: &[u8],
    offset: usize,
    record_len: usize,
    id: &str,
    kind: RecordKind,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<Record, ZDataError> {
    let end = offset + record_len;
    let id_len = u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) as usize;
    if id_len != id.len()
        || offset + RECORD_HEADER_LEN + id_len > end
        || &bytes[offset + RECORD_HEADER_LEN..offset + RECORD_HEADER_LEN + id_len] != id.as_bytes()
    {
        return Err(invalid("index and record metadata disagree"));
    }
    let record_kind = RecordKind::from_u16(u16::from_le_bytes(
        bytes[offset + 2..offset + 4].try_into().unwrap(),
    ))?;
    if record_kind != kind {
        return Err(invalid("index and record metadata disagree"));
    }
    let record_x = read_u32(bytes, offset + 4)?;
    let record_y = read_u32(bytes, offset + 8)?;
    let record_width = read_u32(bytes, offset + 12)?;
    let record_height = read_u32(bytes, offset + 16)?;
    if record_x != x || record_y != y || record_width != width || record_height != height {
        return Err(invalid("index and record metadata disagree"));
    }
    let uncompressed_len = read_u64(bytes, offset + 20)?;
    let compressed_len = read_u64(bytes, offset + 28)?;
    if uncompressed_len > MAX_UNCOMPRESSED
        || compressed_len > MAX_COMPRESSED
        || compressed_len != (record_len - RECORD_HEADER_LEN - id_len) as u64
    {
        return Err(invalid("tile payload length exceeds limit or disagrees"));
    }
    match kind {
        RecordKind::MaskTile => {
            // mask tiles carry a single u16 plane: 2 bytes per pixel.
            if uncompressed_len % 2 != 0
                || u64::from(width) * u64::from(height) * 2 != uncompressed_len
            {
                return Err(invalid("tile dimensions disagree with payload length"));
            }
        }
        RecordKind::RepairRegion => {
            // raw = 12-byte encoding header + region (2 bytes/px) + replacement
            // (4 bytes/px); the exact split is verified in
            // `RepairRegionArtifact::decode_raw`.
            let expected = 12u64 + u64::from(width) * u64::from(height) * 6;
            if uncompressed_len != expected {
                return Err(invalid(
                    "repair region payload length disagrees with dimensions",
                ));
            }
        }
        RecordKind::GenerativeCanvas | RecordKind::SpotHealGenerative => {
            // raw = 12-byte encoding header + RGBA8 (4 bytes/px); the exact
            // split is verified in the RGBA `decode_raw` helpers.
            let expected = 12u64 + u64::from(width) * u64::from(height) * 4;
            if uncompressed_len != expected {
                return Err(invalid(
                    "generative payload length disagrees with dimensions",
                ));
            }
        }
    }
    let mut checksum = [0; 32];
    checksum.copy_from_slice(&bytes[offset + 36..offset + 68]);
    Ok(Record {
        kind,
        mask_id: id.into(),
        tile_x: x,
        tile_y: y,
        width,
        height,
        offset,
        uncompressed_len,
        compressed_len,
        checksum,
    })
}

fn decode_payload(compressed: &[u8], declared_len: u64) -> Result<Vec<u8>, ZDataError> {
    if declared_len > MAX_UNCOMPRESSED {
        return Err(invalid("declared uncompressed length exceeds limit"));
    }
    let limit = declared_len as usize;
    let mut decoder = zstd::stream::read::Decoder::new(compressed)
        .map_err(|e| invalid(format!("zstd decompression failed: {e}")))?;
    let mut raw = Vec::with_capacity(limit);
    let mut chunk = [0u8; 8192];
    loop {
        let read = decoder
            .read(&mut chunk)
            .map_err(|e| invalid(format!("zstd decompression failed: {e}")))?;
        if read == 0 {
            break;
        }
        if raw.len().saturating_add(read) > limit {
            return Err(invalid("zstd output exceeds declared length or limit"));
        }
        raw.extend_from_slice(&chunk[..read]);
    }
    if raw.len() as u64 != declared_len {
        return Err(invalid("zstd output length disagrees with declaration"));
    }
    Ok(raw)
}

fn read_u32(bytes: &[u8], at: usize) -> Result<u32, ZDataError> {
    bytes
        .get(at..at + 4)
        .and_then(|v| v.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| invalid("truncated integer"))
}
fn read_u64(bytes: &[u8], at: usize) -> Result<u64, ZDataError> {
    bytes
        .get(at..at + 8)
        .and_then(|v| v.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or_else(|| invalid("truncated integer"))
}
fn invalid(message: impl Into<String>) -> ZDataError {
    ZDataError::Invalid(message.into())
}
fn ensure_container_size(size: usize) -> Result<(), ZDataError> {
    if size > MAX_CONTAINER_BYTES {
        return Err(invalid("container exceeds size limit"));
    }
    Ok(())
}
fn add_conservative_size(current: usize, raw_len: u64, id_len: usize) -> Result<usize, ZDataError> {
    let raw_len = usize::try_from(raw_len).map_err(|_| invalid("container size overflows"))?;
    current
        .checked_add(RECORD_HEADER_LEN)
        .and_then(|size| size.checked_add(id_len))
        .and_then(|size| size.checked_add(raw_len))
        .and_then(|size| size.checked_add(INDEX_ENTRY_FIXED_LEN))
        .and_then(|size| size.checked_add(id_len))
        .ok_or_else(|| invalid("container size overflows"))
}
fn io_error(operation: &str, path: &Path, error: std::io::Error) -> ZDataError {
    ZDataError::Io {
        operation: operation.into(),
        path: path.display().to_string(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // No lock file may leak after the operations complete.
        assert!(!directory
            .path()
            .join(".image.raw.lumina.zdata.lock")
            .exists());
    }

    #[test]
    fn fresh_zdata_lock_blocks_append_and_save_with_explicit_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = zdata_path_for(&directory.path().join("image.raw"));
        save_zdata(&path, &ZDataContainer::new(tiles()).unwrap()).unwrap();
        // A live writer holds a fresh lock.
        let lock_path = directory.path().join(".image.raw.lumina.zdata.lock");
        std::fs::File::create(&lock_path).unwrap();

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
        drop(fs::remove_file(&lock_path));
        // After release the append works again.
        let mut ok_region = repair_region();
        ok_region.id = "after-release".into();
        append_repair_region(&path, ok_region).unwrap();
        assert_eq!(load_zdata(&path).unwrap().tile_count(), 3);
    }

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

        // Atomic write leaves no temp files and no lock behind.
        let leftovers: Vec<_> = fs::read_dir(directory.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp-") || name.ends_with(".lock"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "no temp/lock files may leak, found {leftovers:?}"
        );

        // Appending under a fresh lock fails explicitly and leaves the
        // bundle untouched.
        let lock_path = directory.path().join(".image.raw.lumina.zdata.lock");
        std::fs::File::create(&lock_path).unwrap();
        let mut blocked = generative_canvas();
        blocked.id = "blocked".into();
        let error = append_generative_canvas(&path, blocked).unwrap_err();
        assert!(
            matches!(&error, ZDataError::Io { operation, .. } if operation.contains("lock")),
            "generative append must report the held lock, got {error}"
        );
        assert_eq!(load_zdata(&path).unwrap().tile_count(), 4);
        drop(fs::remove_file(&lock_path));

        // Missing bundle file surfaces as I/O, never as an empty container.
        let missing = directory.path().join("absent.lumina.zdata");
        assert!(matches!(load_zdata(&missing), Err(ZDataError::Io { .. })));
    }
}
