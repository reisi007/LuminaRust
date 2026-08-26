//! `lumina_save` — render the edited frame and export it to disk.

use crate::error::McpError;
use crate::util::{
    encode_with_quality, get_str, parse_bounded_uint, parse_output_format, render_copy,
    validate_output_extension, write_output_guarded,
};
use crate::Server;
use lumina_sidecar::paths_resolve_equal;
use serde_json::{json, Value};
use std::path::Path;

pub const NAME: &str = "lumina_save";
pub const DESCRIPTION: &str = "Render the active recipe and export the result to a PNG, JPEG or \
WebP file. Uses the shared render entry point (no reimplementation).";

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "image_id": { "type": "string", "description": "Session image_id from lumina_load." },
            "output_path": { "type": "string", "description": "Destination path for the export." },
            "virtual_copy": {
                "type": "string",
                "description": "Virtual copy name or id (default: the standard copy)."
            },
            "format": {
                "type": "string",
                "enum": ["png", "jpeg", "webp"],
                "description": "Output format (default: png)."
            },
            "quality": {
                "type": "integer",
                "minimum": 1,
                "maximum": 100,
                "description": "JPEG/WebP quality, 1..=100 (default: 90)."
            }
        },
        "required": ["image_id", "output_path"]
    })
}

pub fn run(server: &mut Server, args: &Value) -> Result<Value, McpError> {
    let image_id = get_str(args, "image_id")?;
    let state = server.session.require_id(image_id)?;

    let output_path = args
        .get("output_path")
        .and_then(|value| value.as_str())
        .ok_or_else(|| McpError::InvalidParams("missing `output_path`".into()))?;
    let output = Path::new(output_path);

    // Validate arguments before rendering (fail fast): format and extension
    // must agree, quality must be an integer in 1..=100. The schema
    // annotations are advisory for clients; this check is authoritative — a
    // value like 256 fails here instead of truncating to 0.
    let format_str = args
        .get("format")
        .and_then(|value| value.as_str())
        .unwrap_or("png");
    let format = parse_output_format(format_str)?;
    let quality = parse_bounded_uint(args, "quality", 1, 100)?.unwrap_or(90) as u8;
    validate_output_extension(output, format)?;

    // Never overwrite the original image (non-destructive guarantee). Unlike
    // plain canonicalization this also catches not-yet-existing outputs that
    // resolve onto the source's own name. Runs before rendering (fail fast).
    match paths_resolve_equal(&state.source_path, output) {
        Ok(true) => {
            return Err(McpError::Encode(
                "output_path equals the source image; refusing to overwrite the original".into(),
            ))
        }
        Ok(false) => {}
        Err(error) => {
            return Err(McpError::Encode(format!(
                "could not resolve output path `{output_path}`: {error}"
            )))
        }
    }

    let requested = args.get("virtual_copy").and_then(|value| value.as_str());
    let copy = state.find_copy(requested)?;
    let white_balance = state
        .raw_metadata
        .as_ref()
        .map(|meta| meta.camera_white_balance);
    let rendered = render_copy(state, copy, white_balance)?;
    let bytes = encode_with_quality(&rendered, format, quality)?;

    // Guarded atomic write (REVIEW R2-MCP-02, defense in depth): beyond the
    // extension gate and the pre-render source check above, the target must
    // never resolve onto the source or one of its Lumina bundle files —
    // including symlink and hard-link aliases (`reject_protected_target`
    // via `write_output_guarded`, same guard as batch/dust-removal). A
    // crash mid-export still cannot leave a torn artifact behind.
    write_output_guarded(&state.source_path, output, &bytes)?;

    Ok(json!({
        "ok": true,
        "bytes_written": bytes.len() as u64,
        "path": output_path,
    }))
}
