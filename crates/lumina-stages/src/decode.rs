//! Source decode + identity helpers shared by the CLI and the MCP server.
//!
//! The stage editors need a decoded frame (`spot --detect-objects`,
//! `upright --analyze`) and a `SourceIdentity` (`upright --analyze` binds the
//! analysis to the source fingerprint). Both callers must derive the very same
//! values from the very same bytes, otherwise `upright --analyze` through MCP
//! would persist a different `input_fingerprint` than the CLI — a silent
//! divergence that no byte-comparison of a golden would catch. Hence this is
//! shared code, not a re-implementation.

use crate::error::StageError;
use lumina_core::ImageFrame;
use lumina_raw::RawMetadata;
use lumina_sidecar::{DecodeFingerprint, GeometryFingerprint, SourceIdentity};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Decoder version recorded in the source identity of a non-RAW source.
///
/// MCP-PARITY-A: this was `env!("CARGO_PKG_VERSION")` of `lumina-cli`. It is
/// now a pinned literal because the value is *persisted* inside every source
/// identity — bumping this crate's version would silently rewrite the recorded
/// decoder of every new sidecar. Keep it in lockstep with `lumina-cli`'s
/// package version.
const IMAGE_DECODER_VERSION: &str = "0.1.0";

/// True when the path carries an extension `lumina-raw` decodes.
pub fn is_raw_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(lumina_raw::is_raw_extension)
}

/// Decodes already-read bytes: RAW via libraw, raster via the image crate.
pub fn decode_input(
    path: &Path,
    bytes: &[u8],
) -> Result<(ImageFrame, Option<RawMetadata>), StageError> {
    if is_raw_path(path) {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("input.raw");
        let image = lumina_raw::decode_bytes(bytes, name)?;
        Ok((image.frame, Some(image.metadata)))
    } else {
        Ok((ImageFrame::decode(bytes)?, None))
    }
}

/// Reads and decodes `path` in one step.
pub fn read_and_decode(
    path: &Path,
) -> Result<(Vec<u8>, ImageFrame, Option<RawMetadata>), StageError> {
    let bytes = fs::read(path).map_err(|error| StageError::io(path, error))?;
    let (frame, raw) = decode_input(path, &bytes)?;
    Ok((bytes, frame, raw))
}

/// The canonical source identity for an already-decoded input.
pub fn source_identity(
    path: &Path,
    bytes: &[u8],
    frame: &ImageFrame,
    raw_metadata: Option<&RawMetadata>,
) -> Result<SourceIdentity, StageError> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| StageError::Message("input must have a file name".into()))?;
    let metadata = fs::metadata(path).map_err(|error| StageError::io(path, error))?;
    Ok(SourceIdentity {
        relative_name: name.into(),
        content_hash: format!("blake3:{}", blake3::hash(bytes).to_hex()),
        byte_length: metadata.len(),
        modified_at: None,
        raw_format: path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_uppercase(),
        orientation: raw_metadata.map_or(1, |metadata| metadata.orientation),
        decode_fingerprint: DecodeFingerprint {
            decoder: if raw_metadata.is_some() {
                "libraw"
            } else {
                "image"
            }
            .into(),
            version: if raw_metadata.is_some() {
                lumina_raw::libraw_decode_version()
            } else {
                IMAGE_DECODER_VERSION.into()
            },
            parameters: BTreeMap::from([(
                "geometry".into(),
                format!("{}x{}", frame.width, frame.height),
            )]),
            extras: BTreeMap::from([("orientation_applied".into(), "true".into())]),
        },
        geometry_fingerprint: GeometryFingerprint {
            width: frame.width,
            height: frame.height,
            orientation: raw_metadata.map_or(1, |metadata| metadata.orientation),
            pixel_aspect_ratio: 1.0,
            extras: BTreeMap::new(),
        },
        extras: BTreeMap::new(),
    })
}

/// Milliseconds since the Unix epoch, used for history-entry ids and
/// timestamps.
pub fn timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis().to_string())
        .unwrap_or_else(|_| "0".into())
}
