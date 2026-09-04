//! `lumina_get_metadata_draft` — read-only draft view of one image (path-based).

use crate::error::McpError;
use crate::tools::meta_common::draft_payload_for_path;
use crate::util::get_str;
use crate::Server;
use serde_json::{json, Value};

pub const NAME: &str = "lumina_get_metadata_draft";
pub const DESCRIPTION: &str = "Read the IPTC metadata draft of an image by path: embedded JPEG \
values (IIM/XMP; non-JPEG sources report embedded.available=false), the sidecar draft overlay per \
field, keywords and the history length. Read-only; never touches the session.";

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Path to the source image." }
        },
        "required": ["path"]
    })
}

pub fn run(_server: &mut Server, args: &Value) -> Result<Value, McpError> {
    let path_str = get_str(args, "path")?;
    let payload = draft_payload_for_path(path_str)?;
    log::info!(
        "lumina_get_metadata_draft for `{path_str}` (history: {} entries)",
        payload["history_len"].as_u64().unwrap_or(0)
    );
    Ok(payload)
}

/// Builds the `resources/read` result for a `metadata://draft/` URI: the same
/// JSON as [`run`], exposed as a text content block (MCP `resources/read`
/// shape). Failures (unknown URI, missing file/sidecar, broken JPEG segments)
/// are loud [`McpError`]s answered at the protocol layer. Used by `lib.rs`.
pub fn resource_read(uri: &str) -> Result<Value, McpError> {
    use crate::tools::meta_common::path_from_resource_uri;
    let path_str = path_from_resource_uri(uri)?;
    let payload = draft_payload_for_path(&path_str)?;
    let text = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    log::info!("metadata resource read for `{path_str}`");
    Ok(json!({
        "contents": [{
            "uri": uri,
            "mimeType": "application/json",
            "text": text,
        }]
    }))
}
