//! `lumina_reindex` — recursive sidecar scan (expanded MCP scope).
//!
//! IMPORTANT LIMIT (documented in `feature/platform/mcp-server.md`): there is
//! NO index module in this workspace yet. This tool wraps exactly today's CLI
//! `reindex` behavior — collect `*.lumina.json` files recursively, load and
//! validate each document, count and report. It builds no catalog, writes
//! nothing and caches nothing. A real index adapter remains post-MVP work and
//! must be fully reconstructible from sidecars.

use crate::error::McpError;
use crate::util::{collect_tree_files, get_str};
use crate::Server;
use lumina_sidecar::load_sidecar;
use serde_json::{json, Value};
use std::path::Path;

pub const NAME: &str = "lumina_reindex";
pub const DESCRIPTION: &str = "Scan a directory tree for Lumina sidecars (*.lumina.json) and \
validate each one. Reports counts plus every invalid sidecar with path and error — corrupt \
sidecars are never ignored silently (status mirrors the CLI exit code). NOTE: this is a pure \
sidecar scan; no index/catalog is built or maintained.";

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "input": { "type": "string", "description": "Directory to scan recursively." }
        },
        "required": ["input"]
    })
}

pub fn run(_server: &mut Server, args: &Value) -> Result<Value, McpError> {
    let input_str = get_str(args, "input")?;
    let input_dir = Path::new(input_str);
    if !input_dir.is_dir() {
        return Err(McpError::FileNotFound(format!(
            "`{input_str}` is not a directory"
        )));
    }

    let mut files = Vec::new();
    collect_tree_files(input_dir, &mut files, &|path| {
        crate::util::is_sidecar_json_path(path)
    })?;

    let mut valid = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for path in &files {
        match load_sidecar(path) {
            Ok(_) => valid += 1,
            // REVIEW-CLI-N4 parity: corrupt sidecars are never skipped
            // silently — each one is reported individually.
            Err(error) => errors.push(format!("{}: {error}", path.display())),
        }
    }

    Ok(json!({
        "input": input_str,
        "sidecars": valid,
        "invalid": errors.len(),
        "errors": errors,
        "status": if errors.is_empty() { "ok" } else { "invalid-sidecars" },
    }))
}
