//! Binary container codec helpers for `.lumina.zdata`.

use super::*;

pub(super) fn validate_tile(tile: &MaskTile) -> Result<(), ZDataError> {
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
pub(super) fn parse_record(
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

pub(super) fn decode_payload(compressed: &[u8], declared_len: u64) -> Result<Vec<u8>, ZDataError> {
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

pub(super) fn read_u32(bytes: &[u8], at: usize) -> Result<u32, ZDataError> {
    bytes
        .get(at..at + 4)
        .and_then(|v| v.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| invalid("truncated integer"))
}

pub(super) fn read_u64(bytes: &[u8], at: usize) -> Result<u64, ZDataError> {
    bytes
        .get(at..at + 8)
        .and_then(|v| v.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or_else(|| invalid("truncated integer"))
}

pub(super) fn ensure_container_size(size: usize) -> Result<(), ZDataError> {
    if size > MAX_CONTAINER_BYTES {
        return Err(invalid("container exceeds size limit"));
    }
    Ok(())
}

pub(super) fn add_conservative_size(
    current: usize,
    raw_len: u64,
    id_len: usize,
) -> Result<usize, ZDataError> {
    let raw_len = usize::try_from(raw_len).map_err(|_| invalid("container size overflows"))?;
    current
        .checked_add(RECORD_HEADER_LEN)
        .and_then(|size| size.checked_add(id_len))
        .and_then(|size| size.checked_add(raw_len))
        .and_then(|size| size.checked_add(INDEX_ENTRY_FIXED_LEN))
        .and_then(|size| size.checked_add(id_len))
        .ok_or_else(|| invalid("container size overflows"))
}
