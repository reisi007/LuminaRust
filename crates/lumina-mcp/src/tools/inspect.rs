//! `lumina_inspect` — read sidecar status and metadata without decoding.

use crate::error::McpError;
use crate::util::get_str;
use crate::Server;
use serde_json::{json, Value};

pub const NAME: &str = "lumina_inspect";
pub const DESCRIPTION: &str = "Inspect the loaded image's sidecar status and metadata WITHOUT \
decoding the pixels. Reports schema/recipe/pipeline versions, the number of virtual copies, \
and the status of any persisted AI masks.";

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "image_id": { "type": "string", "description": "Session image_id from lumina_load." }
        },
        "required": ["image_id"]
    })
}

pub fn run(server: &mut Server, args: &Value) -> Result<Value, McpError> {
    let image_id = get_str(args, "image_id")?;
    let state = server.session.require_id(image_id)?;
    let document = &state.document;

    let recipe_version = document
        .virtual_copies
        .first()
        .and_then(|copy| copy.recipe.recipe_version.parse::<u32>().ok())
        .unwrap_or(document.schema_version);

    let masks: Vec<Value> = document
        .virtual_copies
        .iter()
        .flat_map(|copy| {
            copy.mask_library.iter().map(|mask| {
                json!({
                    "layer": mask.name,
                    "copy": copy.name,
                    "status": format!("{:?}", mask.status),
                })
            })
        })
        .collect();

    Ok(json!({
        "source_path": state.source_path.to_string_lossy(),
        "sidecar_path": state.sidecar_path.to_string_lossy(),
        "schema_version": document.schema_version,
        "recipe_version": recipe_version,
        "pipeline_version": document.pipeline_version,
        "virtual_copies": document.virtual_copies.len(),
        "ai_masks": masks,
    }))
}
