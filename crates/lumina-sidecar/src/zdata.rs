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
//!                              2 = generative canvas, 3 = spot heal generative,
//!                              4 = denoise RGB, 5 = face embedding)
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
//!
//! ## KI-Denoise RGB payload (before zstd)
//!
//! `DenoiseRgbArtifact` (`kind = 4`, LRPAR-G14-DENOISE-IMPL-20
//! `denoise_ai` → `kind = "denoise_rgb"`) stores the full-frame denoised RGB
//! result as row-major **RGB8** (no alpha), mirroring the generative RGBA
//! layout but with three bytes per pixel:
//!
//! ```text
//!   encoding_version : u32 LE  (= 1)
//!   width            : u32 LE
//!   height           : u32 LE
//!   pixels           : width*height*3 RGB8 bytes, row-major
//! ```
//!
//! The canonical raw stream (header + pixels) is BLAKE3-checksummed; this is
//! the exact digest the recipe's `DenoiseArtifactRef.checksum` carries, so
//! recipe and bundle stay aligned. The core artifact type
//! (`lumina_core::DenoiseRgbArtifact`) uses the identical canonical encoding
//! (`DENOISE_RGB_ENCODING_VERSION`), so the two checksums are interchangeable.
//!
//! ## Face embedding payload (before zstd)
//!
//! `FaceEmbeddingArtifact` (`kind = 5`, LRPAR-G12-FACE-20 `face_embedding`)
//! stores one L2-normalized face identity vector as row-major little-endian
//! `f32` values:
//!
//! ```text
//!   encoding_version : u32 LE  (= 1)
//!   dimension        : u32 LE
//!   values           : dimension × f32 LE
//! ```
//!
//! The canonical raw stream is BLAKE3-checksummed; the digest is exactly what
//! `FaceVectorRef.checksum` carries (no prefix), so analysis and bundle stay
//! aligned. The record id is the stable `FaceEmbedding.id`; the index stores
//! `width = dimension`, `height = 1`.

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
/// KI-Denoise RGB8 payload encoding version. Mirrors
/// `lumina_core::DENOISE_RGB_ENCODING_VERSION`; a change invalidates every
/// persisted `denoise_rgb` record visibly.
pub const RGB_ENCODING_VERSION: u32 = 1;
/// Face-embedding `f32` payload encoding version (kind 5). A change
/// invalidates every persisted `face_embedding` record visibly.
pub const FACE_EMBEDDING_ENCODING_VERSION: u32 = 1;
/// Upper bound on a face-embedding dimension (guards allocations; real
/// descriptors are 128/192/512).
pub const MAX_FACE_EMBEDDING_DIMENSION: u32 = 16_384;
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
    /// A LRPAR-G14-DENOISE-IMPL-20 KI-denoise result (full-frame RGB8).
    DenoiseRgb = 4,
    /// A LRPAR-G12-FACE-20 face identity vector (`face_embedding`).
    FaceEmbedding = 5,
}

impl RecordKind {
    fn from_u16(value: u16) -> Result<Self, ZDataError> {
        match value {
            0 => Ok(RecordKind::MaskTile),
            1 => Ok(RecordKind::RepairRegion),
            2 => Ok(RecordKind::GenerativeCanvas),
            3 => Ok(RecordKind::SpotHealGenerative),
            4 => Ok(RecordKind::DenoiseRgb),
            5 => Ok(RecordKind::FaceEmbedding),
            other => Err(invalid(format!("unsupported zdata record kind {other}"))),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            RecordKind::MaskTile => "mask_tile",
            RecordKind::RepairRegion => "repair_region",
            RecordKind::GenerativeCanvas => "generative_canvas",
            RecordKind::SpotHealGenerative => "spot_heal_generative",
            RecordKind::DenoiseRgb => "denoise_rgb",
            RecordKind::FaceEmbedding => "face_embedding",
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

/// Shared validation for RGB8 denoise payloads: non-empty portable id,
/// non-zero dimensions within [`MAX_DIMENSION`], and exactly
/// `width*height*3` pixel bytes inside the container size budget.
fn validate_rgb_payload(
    id: &str,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<(), ZDataError> {
    if id.is_empty() || id.len() > MAX_ID_LEN || !id.is_char_boundary(id.len()) {
        return Err(invalid("invalid denoise artifact id"));
    }
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(invalid("invalid denoise artifact dimensions"));
    }
    let count = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| invalid("denoise artifact dimensions overflow"))?;
    if count > MAX_UNCOMPRESSED / 3 {
        return Err(invalid("denoise artifact exceeds payload size limit"));
    }
    if pixels.len() != count as usize * 3 {
        return Err(invalid("denoise artifact pixels do not match dimensions"));
    }
    Ok(())
}

/// Shared canonical RGB8 encoding (`encoding_version + width + height +
/// pixels`).  Exactly what the container checksum and
/// [`DenoiseRgbArtifact::checksum`] cover, and byte-identical to
/// `lumina_core::DenoiseRgbArtifact::encode_raw`.
fn encode_rgb_raw(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(12 + pixels.len());
    raw.extend_from_slice(&RGB_ENCODING_VERSION.to_le_bytes());
    raw.extend_from_slice(&width.to_le_bytes());
    raw.extend_from_slice(&height.to_le_bytes());
    raw.extend_from_slice(pixels);
    raw
}

/// Shared RGB8 decoding: verifies the encoding version and that the payload
/// splits exactly into `width*height*3` bytes.
fn decode_rgb_raw(raw: &[u8]) -> Result<(u32, u32, Vec<u8>), ZDataError> {
    if raw.len() < 12 {
        return Err(invalid("denoise artifact payload is truncated"));
    }
    let version = u32::from_le_bytes(raw[0..4].try_into().unwrap());
    if version != RGB_ENCODING_VERSION {
        return Err(invalid("unsupported denoise artifact encoding version"));
    }
    let width = u32::from_le_bytes(raw[4..8].try_into().unwrap());
    let height = u32::from_le_bytes(raw[8..12].try_into().unwrap());
    let count = width as usize * height as usize;
    if raw.len() != 12 + count * 3 {
        return Err(invalid(
            "denoise artifact payload length does not match dimensions",
        ));
    }
    Ok((width, height, raw[12..].to_vec()))
}

/// A LRPAR-G14-DENOISE-IMPL-20 KI-denoise result stored in the
/// `.lumina.zdata` bundle.
///
/// `pixels` is the full-frame denoised result as row-major **RGB8**
/// (`width*height*3` bytes, no alpha — the source alpha is preserved by the
/// render pipeline).  The JSON recipe keeps only a portable
/// `DenoiseArtifactRef` (relative path, format, BLAKE3 checksum over the
/// canonical uncompressed stream, resolution, channel type, data version).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenoiseRgbArtifact {
    pub id: String,
    pub width: u32,
    pub height: u32,
    /// Row-major RGB8 pixels, `width * height * 3` long.
    pub pixels: Vec<u8>,
}

impl DenoiseRgbArtifact {
    /// Validates id, dimensions and pixel-byte count.
    pub fn validate(&self) -> Result<(), ZDataError> {
        validate_rgb_payload(&self.id, self.width, self.height, &self.pixels)
    }

    fn encode_raw(&self) -> Vec<u8> {
        encode_rgb_raw(self.width, self.height, &self.pixels)
    }

    fn decode_raw(id: String, raw: &[u8]) -> Result<Self, ZDataError> {
        let (width, height, pixels) = decode_rgb_raw(raw)?;
        let artifact = DenoiseRgbArtifact {
            id,
            width,
            height,
            pixels,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    /// BLAKE3 hex digest of the canonical raw payload (uncompressed RGB8
    /// stream).  Stored in the recipe's `DenoiseArtifactRef.checksum`; equal to
    /// the core artifact's `checksum()` for the same dimensions/pixels.
    pub fn checksum(&self) -> String {
        blake3::hash(&self.encode_raw()).to_hex().to_string()
    }

    /// Stable kind string used by diagnostics (`denoise_rgb`).
    pub fn kind_str(&self) -> &'static str {
        RecordKind::DenoiseRgb.as_str()
    }
}

/// A LRPAR-G12-FACE-20 face identity vector stored in the `.lumina.zdata`
/// bundle.
///
/// `values` is one L2-normalized descriptor (`dimension` long). The JSON
/// sidecar keeps only a portable `FaceVectorRef` (relative path, format,
/// BLAKE3 checksum over the canonical uncompressed stream, dimension, channel
/// type, data version); the numbers never appear in the JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceEmbeddingArtifact {
    pub id: String,
    pub dimension: u32,
    /// Row-major little-endian `f32` values, `dimension` long.
    pub values: Vec<f32>,
}

impl FaceEmbeddingArtifact {
    /// Validates the id, the dimension bounds and the value count. Non-finite
    /// values are rejected loudly: a `NaN`/`Inf` descriptor would poison every
    /// downstream distance computation.
    pub fn validate(&self) -> Result<(), ZDataError> {
        if self.id.is_empty()
            || self.id.len() > MAX_ID_LEN
            || !self.id.is_char_boundary(self.id.len())
        {
            return Err(invalid("invalid face embedding id"));
        }
        if self.dimension == 0 || self.dimension > MAX_FACE_EMBEDDING_DIMENSION {
            return Err(invalid("invalid face embedding dimension"));
        }
        if self.values.len() != self.dimension as usize {
            return Err(invalid(
                "face embedding values do not match the declared dimension",
            ));
        }
        if self.values.iter().any(|value| !value.is_finite()) {
            return Err(invalid("face embedding values must be finite"));
        }
        Ok(())
    }

    fn encode_raw(&self) -> Vec<u8> {
        let mut raw = Vec::with_capacity(8 + self.values.len() * 4);
        raw.extend_from_slice(&FACE_EMBEDDING_ENCODING_VERSION.to_le_bytes());
        raw.extend_from_slice(&self.dimension.to_le_bytes());
        for value in &self.values {
            raw.extend_from_slice(&value.to_le_bytes());
        }
        raw
    }

    fn decode_raw(id: String, raw: &[u8]) -> Result<Self, ZDataError> {
        if raw.len() < 8 {
            return Err(invalid("face embedding payload is truncated"));
        }
        let version = u32::from_le_bytes(raw[0..4].try_into().unwrap());
        if version != FACE_EMBEDDING_ENCODING_VERSION {
            return Err(invalid("unsupported face embedding encoding version"));
        }
        let dimension = u32::from_le_bytes(raw[4..8].try_into().unwrap());
        if raw.len() != 8 + dimension as usize * 4 {
            return Err(invalid(
                "face embedding payload length does not match the dimension",
            ));
        }
        let values = raw[8..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| f32::from_le_bytes(*bytes))
            .collect();
        let artifact = FaceEmbeddingArtifact {
            id,
            dimension,
            values,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    /// BLAKE3 hex digest of the canonical raw payload (uncompressed
    /// `encoding_version || dimension || f32` stream). Stored in the analysis'
    /// `FaceVectorRef.checksum` — the exact value the evidence helper verifies.
    pub fn checksum(&self) -> String {
        blake3::hash(&self.encode_raw()).to_hex().to_string()
    }

    /// Stable kind string used by diagnostics (`face_embedding`).
    pub fn kind_str(&self) -> &'static str {
        RecordKind::FaceEmbedding.as_str()
    }
}

/// A single bundle entry, used by the unified constructor and re-reads.
#[derive(Debug, Clone)]
pub enum RecordSpec {
    MaskTile(MaskTile),
    RepairRegion(RepairRegionArtifact),
    GenerativeCanvas(GenerativeCanvasArtifact),
    SpotHealGenerative(SpotHealGenerativeArtifact),
    DenoiseRgb(DenoiseRgbArtifact),
    FaceEmbedding(FaceEmbeddingArtifact),
}

/// Stable record id of a [`RecordSpec`] of any kind.
fn spec_id(spec: &RecordSpec) -> &str {
    match spec {
        RecordSpec::MaskTile(tile) => &tile.mask_id,
        RecordSpec::RepairRegion(artifact) => &artifact.id,
        RecordSpec::GenerativeCanvas(artifact) => &artifact.id,
        RecordSpec::SpotHealGenerative(artifact) => &artifact.id,
        RecordSpec::DenoiseRgb(artifact) => &artifact.id,
        RecordSpec::FaceEmbedding(artifact) => &artifact.id,
    }
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
    source.with_file_name(format!("{filename}{}", crate::ZDATA_FILE_SUFFIX))
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

/// GEN-ONNX-1: writes a generative canvas to the bundle at `path` (creating it
/// if needed) under the `.zdata.lock`.
///
/// `replace = false` appends and rejects a duplicate id (the default,
/// non-destructive path); `replace = true` is the explicit regeneration path
/// ([`ZDataContainer::replace_generative_canvas`]). Both write atomically
/// (Temp + Rename). Pre-existing records of every kind are preserved.
pub fn save_generative_canvas(
    path: &Path,
    canvas: GenerativeCanvasArtifact,
    replace: bool,
) -> Result<(), ZDataError> {
    let _lock = crate::acquire_write_lock(path).map_err(|error| lock_error(path, error))?;
    let existing = if path.exists() {
        Some(load_zdata(path)?)
    } else {
        None
    };
    let container = match existing {
        Some(container) if replace => container.replace_generative_canvas(canvas)?,
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

/// Appends a LRPAR-G14-DENOISE-IMPL-20 `denoise_rgb` artifact to the bundle at
/// `path` (creating it if needed), preserving every pre-existing record.  Runs
/// under the bundle's `.zdata.lock` and writes atomically (Temp + Rename); a
/// duplicate id or a pixel/dimension mismatch is reported as an error.  Same
/// contract as [`append_generative_canvas`]; `load_zdata` verifies every
/// record checksum eagerly, so a corrupt denoise record is visible at load.
pub fn append_denoise_rgb(path: &Path, denoise: DenoiseRgbArtifact) -> Result<(), ZDataError> {
    let _lock = crate::acquire_write_lock(path).map_err(|error| lock_error(path, error))?;
    let existing = if path.exists() {
        Some(load_zdata(path)?)
    } else {
        None
    };
    let container = match existing {
        Some(container) => container.add_denoise_rgb(denoise)?,
        None => ZDataContainer::new(vec![])?.add_denoise_rgb(denoise)?,
    };
    save_zdata_locked(path, &container)
}

/// Writes a `denoise_rgb` artifact to the bundle at `path` (creating it if
/// needed) under the `.zdata.lock`.
///
/// `replace = false` appends and rejects a duplicate id (the default,
/// non-destructive path); `replace = true` is the explicit regeneration path
/// ([`ZDataContainer::replace_denoise_rgb`]).  Both write atomically
/// (Temp + Rename) and preserve pre-existing records of every kind.
pub fn save_denoise_rgb(
    path: &Path,
    denoise: DenoiseRgbArtifact,
    replace: bool,
) -> Result<(), ZDataError> {
    let _lock = crate::acquire_write_lock(path).map_err(|error| lock_error(path, error))?;
    let existing = if path.exists() {
        Some(load_zdata(path)?)
    } else {
        None
    };
    let container = match existing {
        Some(container) if replace => container.replace_denoise_rgb(denoise)?,
        Some(container) => container.add_denoise_rgb(denoise)?,
        None => ZDataContainer::new(vec![])?.add_denoise_rgb(denoise)?,
    };
    save_zdata_locked(path, &container)
}

/// Writes a `face_embedding` artifact to the bundle at `path` (creating it if
/// needed) under the `.zdata.lock`.
///
/// `replace = false` appends and rejects a duplicate id (the default,
/// non-destructive path); `replace = true` is the explicit re-analysis path
/// ([`ZDataContainer::replace_face_embedding`]).  Both write atomically
/// (Temp + Rename) and preserve pre-existing records of every kind.
pub fn save_face_embedding(
    path: &Path,
    embedding: FaceEmbeddingArtifact,
    replace: bool,
) -> Result<(), ZDataError> {
    let _lock = crate::acquire_write_lock(path).map_err(|error| lock_error(path, error))?;
    let existing = if path.exists() {
        Some(load_zdata(path)?)
    } else {
        None
    };
    let container = match existing {
        Some(container) if replace => container.replace_face_embedding(embedding)?,
        Some(container) => container.add_face_embedding(embedding)?,
        None => ZDataContainer::new(vec![])?.add_face_embedding(embedding)?,
    };
    save_zdata_locked(path, &container)
}

/// Writes a whole set of `face_embedding` records in one atomic bundle write
/// under a single `.zdata.lock`.
///
/// This is the face-analysis write path: every record of the batch is
/// upserted (an existing `face_embedding` record with the same id is replaced,
/// so an explicit `--force` re-analysis is idempotent), records of every other
/// kind and `face_embedding` records with other ids are preserved.  Duplicate
/// ids within the batch and id collisions with a different kind are rejected
/// loudly.  Because the references are only recorded in the JSON sidecar after
/// this write returns, a failure can never leave a dangling reference.
pub fn save_face_embeddings(
    path: &Path,
    embeddings: Vec<FaceEmbeddingArtifact>,
) -> Result<(), ZDataError> {
    let _lock = crate::acquire_write_lock(path).map_err(|error| lock_error(path, error))?;
    let existing = if path.exists() {
        Some(load_zdata(path)?)
    } else {
        None
    };
    let mut specs = match existing {
        Some(container) => container.decode_all()?,
        None => Vec::new(),
    };
    let mut batch_ids = HashSet::new();
    for embedding in embeddings {
        embedding.validate()?;
        if !batch_ids.insert(embedding.id.clone()) {
            return Err(ZDataError::DuplicateId(embedding.id));
        }
        // Replace only this id; a collision with another kind stays an error.
        specs.retain(
            |spec| !matches!(spec, RecordSpec::FaceEmbedding(existing) if existing.id == embedding.id),
        );
        if specs.iter().any(|spec| spec_id(spec) == embedding.id) {
            return Err(ZDataError::DuplicateId(embedding.id));
        }
        specs.push(RecordSpec::FaceEmbedding(embedding));
    }
    let container = ZDataContainer::new_with(specs)?;
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
        RecordKind::DenoiseRgb => {
            // raw = 12-byte encoding header + RGB8 (3 bytes/px); the exact
            // split is verified in the RGB `decode_raw` helper.
            let expected = 12u64 + u64::from(width) * u64::from(height) * 3;
            if uncompressed_len != expected {
                return Err(invalid("denoise payload length disagrees with dimensions"));
            }
        }
        RecordKind::FaceEmbedding => {
            // raw = 8-byte encoding header + f32 (4 bytes/value); the index
            // stores width = dimension, height = 1.
            let expected = 8u64 + u64::from(width) * u64::from(height) * 4;
            if height != 1 || width == 0 || uncompressed_len != expected {
                return Err(invalid(
                    "face embedding payload length disagrees with dimensions",
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
#[rustfmt::skip] mod tests;
