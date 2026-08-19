//! `lumina_preview` — fast, cache-free, deterministic downscaled PNG preview.

use crate::error::McpError;
use crate::util::{downscale_bilinear, get_str, render_copy};
use crate::Server;
use lumina_core::ImageFileFormat;
use serde_json::{json, Value};
use std::fs;

pub const NAME: &str = "lumina_preview";
pub const DESCRIPTION: &str = "Render the active recipe and write a fast, downscaled PNG preview \
to the configured preview directory (default $TMPDIR/lumina-previews/). Cache-free and \
deterministic: identical recipe + source produce byte-identical output. Intended for the \
agent's visual feedback loop.";

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "image_id": { "type": "string", "description": "Session image_id from lumina_load." },
            "virtual_copy": {
                "type": "string",
                "description": "Virtual copy name or id (default: the standard copy)."
            },
            "max_width": {
                "type": "integer",
                "minimum": 1,
                "description": "Maximum preview width in pixels (default: 1024)."
            }
        },
        "required": ["image_id"]
    })
}

pub fn run(server: &mut Server, args: &Value) -> Result<Value, McpError> {
    let image_id = get_str(args, "image_id")?;
    let state = server.session.require_id(image_id)?;

    let max_width = args
        .get("max_width")
        .and_then(|value| value.as_u64())
        .map(|value| value as u32)
        .unwrap_or(1024);

    let requested = args.get("virtual_copy").and_then(|value| value.as_str());
    let copy = state.find_copy(requested)?;
    let white_balance = state
        .raw_metadata
        .as_ref()
        .map(|meta| meta.camera_white_balance);
    let rendered = render_copy(state, copy, white_balance)?;
    let preview_frame = downscale_bilinear(&rendered, max_width);

    let png = preview_frame
        .encode(ImageFileFormat::Png)
        .map_err(crate::error::map_core_error)?;

    fs::create_dir_all(&server.preview_dir).map_err(|error| {
        McpError::Encode(format!(
            "could not create preview dir `{}`: {error}",
            server.preview_dir.display()
        ))
    })?;
    let preview_path = server.preview_dir.join(format!("{}.png", state.id));
    fs::write(&preview_path, &png).map_err(|error| {
        McpError::Encode(format!(
            "could not write preview `{preview_path:?}`: {error}"
        ))
    })?;

    Ok(json!({
        "ok": true,
        "preview_path": preview_path.to_string_lossy(),
        "width": preview_frame.width,
        "height": preview_frame.height,
        "size_bytes": png.len() as u64,
    }))
}
