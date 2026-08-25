//! `lumina_load` — load an image and (re)materialize its sidecar.

use crate::error::McpError;
use crate::session::ImageState;
use crate::util::{build_source_identity, detect_format, is_raw_path};
use crate::Server;
use lumina_core::ImageFrame;
use lumina_raw::RawMetadata;
use lumina_sidecar::{
    document_revision, load_sidecar, save_sidecar_if_unchanged, sidecar_path_for, SidecarDocument,
    SidecarError, SourceIdentity,
};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

pub const NAME: &str = "lumina_load";
pub const DESCRIPTION: &str = "Load an image (RAW, PNG, JPEG, WebP) and return its metadata. \
Assigns a process-local image_id and (re)loads the sidecar. A new lumina_load \
discards the previously loaded image.";

/// Pipeline identifier for sidecars materialized by this server (same value as
/// the CLI's import path).
const PIPELINE_VERSION: &str = "raster-mvp-1";

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "Path to the source image (RAW, PNG, JPEG or WebP)."
            }
        },
        "required": ["path"]
    })
}

pub fn run(server: &mut Server, args: &Value) -> Result<Value, McpError> {
    let path_str = args
        .get("path")
        .and_then(|value| value.as_str())
        .ok_or_else(|| McpError::InvalidParams("missing `path`".into()))?;
    let path = Path::new(path_str);

    if !path.exists() {
        return Err(McpError::FileNotFound(path_str.to_string()));
    }
    let format = detect_format(path)?;

    let bytes =
        fs::read(path).map_err(|error| McpError::FileNotFound(format!("{path_str}: {error}")))?;

    // Decode (RAW via libraw, raster via the image crate).
    let (frame, raw_metadata): (ImageFrame, Option<RawMetadata>) = if is_raw_path(path) {
        let image =
            lumina_raw::decode_file(path).map_err(|error| McpError::Decode(format!("{error}")))?;
        (image.frame, Some(image.metadata))
    } else {
        let frame =
            ImageFrame::decode(&bytes).map_err(|error| McpError::Decode(format!("{error}")))?;
        (frame, None)
    };

    let sidecar_path = sidecar_path_for(path);
    let identity = build_source_identity(path, &bytes, &frame, raw_metadata.as_ref());
    let (document, sidecar_revision, status) = if sidecar_path.exists() {
        open_existing_sidecar(&sidecar_path, &identity)?
    } else {
        // Compare-and-swap create: if another process materialized the default
        // sidecar in this race window, adopt its document instead of clobbering
        // it (both are fresh defaults; the identity check still applies).
        let fresh = SidecarDocument::new(identity.clone(), PIPELINE_VERSION);
        match save_sidecar_if_unchanged(&sidecar_path, &fresh, None) {
            Ok(revision) => (fresh, revision, "created".to_string()),
            Err(SidecarError::Conflict(_)) => {
                log::warn!(
                    "sidecar appeared concurrently while loading `{}`; adopting it",
                    path.display()
                );
                open_existing_sidecar(&sidecar_path, &identity)?
            }
            Err(error) => return Err(McpError::Sidecar(format!("{error}"))),
        }
    };

    let id = server.generate_image_id(path);
    let virtual_copies: Vec<String> = document
        .virtual_copies
        .iter()
        .map(|copy| copy.name.clone())
        .collect();
    let width = frame.width;
    let height = frame.height;
    let sidecar_status = status.clone();

    server.session.current = Some(ImageState {
        id: id.clone(),
        source_path: path.to_path_buf(),
        sidecar_path,
        frame,
        raw_metadata,
        document,
        sidecar_revision,
        sidecar_status: status,
    });

    Ok(json!({
        "image_id": id,
        "width": width,
        "height": height,
        "format": format,
        "virtual_copies": virtual_copies,
        "sidecar_status": sidecar_status,
    }))
}

/// Loads an existing sidecar and verifies it against the source's current
/// identity. A sidecar whose recorded content hash or byte length no longer
/// matches the source is stale; per F-001/F-101 it is reported loudly and
/// never silently used (same rule as the CLI develop/export path).
fn open_existing_sidecar(
    sidecar_path: &Path,
    identity: &SourceIdentity,
) -> Result<(SidecarDocument, String, String), McpError> {
    let document =
        load_sidecar(sidecar_path).map_err(|error| McpError::Sidecar(format!("{error}")))?;
    if document.source.content_hash != identity.content_hash
        || document.source.byte_length != identity.byte_length
    {
        return Err(McpError::Sidecar(format!(
            "source changed since sidecar was written: `{}` (sidecar content hash `{}`, current `{}`)",
            sidecar_path.display(),
            document.source.content_hash,
            identity.content_hash
        )));
    }
    let revision =
        document_revision(&document).map_err(|error| McpError::Sidecar(format!("{error}")))?;
    Ok((document, revision, "loaded".to_string()))
}
