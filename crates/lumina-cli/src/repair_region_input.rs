//! Bounded file-backed input for `dust-removal --repair-region`.

use lumina_sidecar::SourceActionKind;
use serde::Deserialize;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_DEFINITION_BYTES: u64 = 64 * 1024;
// Keep these limits aligned with MCP and RepairRegionArtifact::validate().
const MAX_DIMENSION: u32 = 16_384;
const MAX_UNCOMPRESSED_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RepairRegionInput {
    pub id: String,
    #[serde(default)]
    kind: Option<String>,
    pub region_width: u32,
    pub region_height: u32,
    pub region_path: PathBuf,
    pub replacement_path: PathBuf,
}

impl RepairRegionInput {
    pub fn load(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let mut bytes = Vec::new();
        file.take(MAX_DEFINITION_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("could not read `{}`: {error}", path.display()))?;
        if bytes.len() as u64 > MAX_DEFINITION_BYTES {
            return Err(format!(
                "definition exceeds {MAX_DEFINITION_BYTES} byte limit"
            ));
        }
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())
    }

    pub fn source_action_kind(&self) -> Result<SourceActionKind, String> {
        match self.kind.as_deref() {
            None | Some("dust-removal") | Some("dustremoval") => Ok(SourceActionKind::DustRemoval),
            Some("ai-replacement") | Some("aireplacement") => Ok(SourceActionKind::AiReplacement),
            Some(other) => Err(format!(
                "repair-region kind `{other}` is not one of `dust-removal`, `ai-replacement`"
            )),
        }
    }
}

/// Reads an exact headerless row-major LE-u16 plane, bounded to the existing
/// repair-artifact payload limit. Metadata is checked before allocation and the
/// extra-byte read catches files that grow after the metadata check.
pub(super) fn read_region_values(path: &Path, width: u32, height: u32) -> Result<Vec<u16>, String> {
    let count = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| "repair region dimensions overflow".to_string())?;
    let byte_len = count
        .checked_mul(2)
        .filter(|_| width != 0 && height != 0)
        .ok_or_else(|| "invalid repair region dimensions".to_string())?;
    if width > MAX_DIMENSION || height > MAX_DIMENSION || count > MAX_UNCOMPRESSED_BYTES / 6 {
        return Err("repair region exceeds payload size limit".into());
    }

    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("could not inspect `{}`: {error}", path.display()))?;
    if metadata.len() != byte_len {
        return Err(format!(
            "repair region file must contain exactly {byte_len} bytes, got {}",
            metadata.len()
        ));
    }
    let capacity = usize::try_from(byte_len)
        .map_err(|_| "repair region file is too large for this platform".to_string())?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|error| format!("could not allocate repair region: {error}"))?;
    File::open(path)
        .map_err(|error| format!("could not open `{}`: {error}", path.display()))?
        .take(byte_len + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("could not read repair region: {error}"))?;
    if bytes.len() != capacity {
        return Err(format!(
            "repair region file must contain exactly {byte_len} bytes, got {}",
            bytes.len()
        ));
    }
    Ok(bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| u16::from_le_bytes(*bytes))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reads_asymmetric_little_endian_values() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("region.u16le");
        fs::write(&path, [0x34, 0x12, 0xcd, 0xab]).unwrap();
        assert_eq!(read_region_values(&path, 2, 1).unwrap(), [0x1234, 0xabcd]);
    }

    #[test]
    fn rejects_short_trailing_and_oversized_planes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("region.u16le");
        fs::write(&path, [0x34]).unwrap();
        assert!(read_region_values(&path, 1, 1)
            .unwrap_err()
            .contains("exactly 2 bytes"));
        fs::write(&path, [0x34, 0x12, 0]).unwrap();
        assert!(read_region_values(&path, 1, 1)
            .unwrap_err()
            .contains("exactly 2 bytes"));
        assert!(read_region_values(&path, 16_385, 1).is_err());
    }

    #[test]
    fn definition_is_strict_and_small_bounded() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("repair.json");
        fs::write(&path, br#"{"id":"x","region_width":1,"region_height":1,"region_values":[1],"replacement_path":"r.png"}"#).unwrap();
        assert!(RepairRegionInput::load(&path).is_err());
        fs::write(&path, vec![b' '; MAX_DEFINITION_BYTES as usize + 1]).unwrap();
        assert!(RepairRegionInput::load(&path)
            .unwrap_err()
            .contains("byte limit"));
    }
}
