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

mod codec;
mod container;
mod payloads;
pub use container::ZDataContainer;

// Wiring only: the container `impl` lives in `container.rs`; these three codec
// helpers stay imported here because the `tests` child module reaches them
// through `use super::*`.
#[cfg(test)]
use codec::{add_conservative_size, decode_payload, ensure_container_size};

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use thiserror::Error;

const MAGIC: &[u8; 8] = b"LUMZDATA";
const VERSION: u16 = 1;
const HEADER_LEN: usize = 40;
pub(super) const REPAIR_ENCODING_VERSION: u32 = 1;
pub(super) const RGBA_ENCODING_VERSION: u32 = 1;
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
pub(super) const MAX_ID_LEN: usize = 4096;
pub(super) const MAX_DIMENSION: u32 = 16_384;
const MAX_TILE_VALUES: u64 = 64 * 1024 * 1024;
pub(super) const MAX_UNCOMPRESSED: u64 = MAX_TILE_VALUES * 2;
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

    pub(super) fn as_str(self) -> &'static str {
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

pub use payloads::*;

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

pub fn zdata_path_for(source: &Path) -> PathBuf {
    let filename = source
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    source.with_file_name(format!("{filename}{}", crate::ZDATA_FILE_SUFFIX))
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

pub(super) fn invalid(message: impl Into<String>) -> ZDataError {
    ZDataError::Invalid(message.into())
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
