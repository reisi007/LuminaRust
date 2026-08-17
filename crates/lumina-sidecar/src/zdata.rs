//! The optional `.lumina.zdata` tile container.  This module is deliberately
//! feature-gated: Zstd and BLAKE3 are native-sidecar capabilities, not core/WASM
//! requirements.

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use thiserror::Error;

const MAGIC: &[u8; 8] = b"LUMZDATA";
const VERSION: u16 = 1;
const HEADER_LEN: usize = 40;
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

#[derive(Debug, Clone)]
struct Record {
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
    pub fn new(tiles: Vec<MaskTile>) -> Result<Self, ZDataError> {
        if tiles.len() > MAX_RECORDS {
            return Err(invalid("record count exceeds limit"));
        }
        let mut conservative_size = HEADER_LEN;
        let mut ids = HashSet::new();
        for tile in &tiles {
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
        ensure_container_size(conservative_size)?;

        let mut bytes = vec![0; HEADER_LEN];
        let mut records = Vec::with_capacity(tiles.len());
        for tile in tiles {
            let mut raw = Vec::with_capacity(tile.values.len() * 2);
            for value in tile.values {
                raw.extend_from_slice(&value.to_le_bytes());
            }
            let compressed = zstd::stream::encode_all(raw.as_slice(), 3)
                .map_err(|e| invalid(format!("zstd compression failed: {e}")))?;
            let id = tile.mask_id.as_bytes();
            let offset = bytes.len();
            // The final size is only known after compression, so enforce the
            // container limit before every payload append.
            let record_len = RECORD_HEADER_LEN
                .checked_add(id.len())
                .and_then(|len| len.checked_add(compressed.len()))
                .ok_or_else(|| invalid("container size overflows"))?;
            ensure_container_size(
                bytes
                    .len()
                    .checked_add(record_len)
                    .ok_or_else(|| invalid("container size overflows"))?,
            )?;
            bytes.extend_from_slice(&(id.len() as u16).to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes.extend_from_slice(&tile.tile_x.to_le_bytes());
            bytes.extend_from_slice(&tile.tile_y.to_le_bytes());
            bytes.extend_from_slice(&tile.width.to_le_bytes());
            bytes.extend_from_slice(&tile.height.to_le_bytes());
            bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
            bytes.extend_from_slice(&(compressed.len() as u64).to_le_bytes());
            bytes.extend_from_slice(blake3::hash(&raw).as_bytes());
            bytes.extend_from_slice(id);
            records.push(Record {
                mask_id: tile.mask_id,
                tile_x: tile.tile_x,
                tile_y: tile.tile_y,
                width: tile.width,
                height: tile.height,
                offset,
                uncompressed_len: raw.len() as u64,
                compressed_len: compressed.len() as u64,
                checksum: *blake3::hash(&raw).as_bytes(),
            });
            bytes.extend_from_slice(&compressed);
        }
        let index_offset = bytes.len();
        let index_len = records.iter().try_fold(0usize, |len, record| {
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
        for record in &records {
            let id = record.mask_id.as_bytes();
            bytes.extend_from_slice(&(id.len() as u16).to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes.extend_from_slice(&record.tile_x.to_le_bytes());
            bytes.extend_from_slice(&record.tile_y.to_le_bytes());
            bytes.extend_from_slice(&record.width.to_le_bytes());
            bytes.extend_from_slice(&record.height.to_le_bytes());
            bytes.extend_from_slice(&(record.offset as u64).to_le_bytes());
            bytes.extend_from_slice(
                &((RECORD_HEADER_LEN + id.len() + record.compressed_len as usize) as u64)
                    .to_le_bytes(),
            );
            bytes.extend_from_slice(id);
        }
        debug_assert_eq!(bytes.len() - index_offset, index_len);
        bytes[0..8].copy_from_slice(MAGIC);
        bytes[8..10].copy_from_slice(&VERSION.to_le_bytes());
        bytes[10..12].copy_from_slice(&(HEADER_LEN as u16).to_le_bytes());
        bytes[12..16].copy_from_slice(&(records.len() as u32).to_le_bytes());
        bytes[16..24].copy_from_slice(&(index_offset as u64).to_le_bytes());
        bytes[24..32].copy_from_slice(&(index_len as u64).to_le_bytes());
        Ok(Self { bytes, records })
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
                bytes, offset, record_len, &mask_id, tile_x, tile_y, width, height,
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
            .find(|r| r.mask_id == mask_id && r.tile_x == tile_x && r.tile_y == tile_y)
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
            .chunks_exact(2)
            .map(|v| u16::from_le_bytes([v[0], v[1]]))
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
    ZDataContainer::from_bytes(&bytes)
}

pub fn save_zdata(path: &Path, container: &ZDataContainer) -> Result<(), ZDataError> {
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
    if uncompressed_len % 2 != 0 || u64::from(width) * u64::from(height) * 2 != uncompressed_len {
        return Err(invalid("tile dimensions disagree with payload length"));
    }
    let mut checksum = [0; 32];
    checksum.copy_from_slice(&bytes[offset + 36..offset + 68]);
    Ok(Record {
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
}
