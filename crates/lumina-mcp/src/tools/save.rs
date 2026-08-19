//! `lumina_save` — render the edited frame and export it to disk.

use crate::error::McpError;
use crate::util::{encode_with_quality, get_str, parse_output_format, render_copy};
use crate::Server;
use serde_json::{json, Value};
use std::fs;
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

    // Never overwrite the original image.
    if let Ok(canonical_output) = std::fs::canonicalize(output) {
        if let Ok(canonical_source) = std::fs::canonicalize(&state.source_path) {
            if canonical_output == canonical_source {
                return Err(McpError::Encode(
                    "output_path equals the source image; refusing to overwrite the original"
                        .into(),
                ));
            }
        }
    }

    let format_str = args
        .get("format")
        .and_then(|value| value.as_str())
        .unwrap_or("png");
    let format = parse_output_format(format_str)?;
    let quality = args
        .get("quality")
        .and_then(|value| value.as_u64())
        .map(|value| value as u8)
        .unwrap_or(90);

    let requested = args.get("virtual_copy").and_then(|value| value.as_str());
    let copy = state.find_copy(requested)?;
    let white_balance = state
        .raw_metadata
        .as_ref()
        .map(|meta| meta.camera_white_balance);
    let rendered = render_copy(state, copy, white_balance)?;
    let bytes = encode_with_quality(&rendered, format, quality)?;

    fs::write(output, &bytes)
        .map_err(|error| McpError::Encode(format!("could not write `{output_path}`: {error}")))?;

    Ok(json!({
        "ok": true,
        "bytes_written": bytes.len() as u64,
        "path": output_path,
    }))
}
