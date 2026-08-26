//! `lumina_import` — materialize/validate a sidecar for a source image
//! (expanded MCP scope, mirrors `lumina import`).
//!
//! Unlike [`crate::tools::load`] this tool is **path-based**: it never touches
//! the single-image session, so an agent can prepare many files while keeping
//! its currently loaded image. Behavior mirrors the CLI's `import_file`:
//! an existing sidecar is validated against the current source identity and
//! reported loudly when the source changed; a missing sidecar is created as
//! an empty standard document.

use crate::error::McpError;
use crate::tools::load::{open_existing_sidecar, PIPELINE_VERSION};
use crate::util::{build_source_identity, detect_format, read_and_decode};
use crate::Server;
use lumina_sidecar::{save_sidecar_if_unchanged, sidecar_path_for, SidecarDocument, SidecarError};
use serde_json::{json, Value};
use std::path::Path;

pub const NAME: &str = "lumina_import";
pub const DESCRIPTION: &str = "Ensure a valid Lumina sidecar exists for a source image WITHOUT \
loading it into the session (mirrors `lumina import`). Creates an empty standard sidecar or \
validates an existing one against the current file contents; a changed source is a loud error. \
Path-based: does not discard the currently loaded image.";

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

pub fn run(_server: &mut Server, args: &Value) -> Result<Value, McpError> {
    let path_str = args
        .get("path")
        .and_then(|value| value.as_str())
        .ok_or_else(|| McpError::InvalidParams("missing `path`".into()))?;
    let path = Path::new(path_str);

    if !path.exists() {
        return Err(McpError::FileNotFound(path_str.to_string()));
    }
    let format = detect_format(path)?;
    let (_bytes, frame, raw_metadata) = read_and_decode(path)?;

    let sidecar_path = sidecar_path_for(path);
    let identity = build_source_identity(path, &_bytes, &frame, raw_metadata.as_ref())?;
    let status = if sidecar_path.exists() {
        // Existing sidecar: validate against the CURRENT contents. A mismatch
        // is a loud SidecarError ("source changed"), never silently blessed —
        // identical to the CLI import guard.
        open_existing_sidecar(&sidecar_path, &identity)?;
        "validated"
    } else {
        // Compare-and-swap create: if another process materialized a default
        // sidecar in this race window, adopt it instead of clobbering.
        let fresh = SidecarDocument::new(identity.clone(), PIPELINE_VERSION);
        match save_sidecar_if_unchanged(&sidecar_path, &fresh, None) {
            Ok(_) => "created",
            Err(SidecarError::Conflict(_)) => {
                log::warn!(
                    "sidecar appeared concurrently while importing `{}`; validating it",
                    path.display()
                );
                open_existing_sidecar(&sidecar_path, &identity)?;
                "validated"
            }
            Err(error) => return Err(McpError::Sidecar(format!("{error}"))),
        }
    };

    Ok(json!({
        "ok": true,
        "input": path_str,
        "format": format,
        "sidecar": sidecar_path.to_string_lossy(),
        "status": status,
    }))
}
