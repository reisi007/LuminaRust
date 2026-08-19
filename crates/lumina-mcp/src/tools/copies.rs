//! `lumina_list_virtual_copies` — list the virtual copies of the loaded image.

use crate::error::McpError;
use crate::util::{get_str, recipe_hash};
use crate::Server;
use serde_json::{json, Value};

pub const NAME: &str = "lumina_list_virtual_copies";
pub const DESCRIPTION: &str = "List all virtual copies of the loaded image with their id, name \
and current recipe_hash.";

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
    let copies: Vec<Value> = state
        .document
        .virtual_copies
        .iter()
        .map(|copy| {
            json!({
                "id": copy.id,
                "name": copy.name,
                "recipe_hash": recipe_hash(&copy.recipe),
            })
        })
        .collect();
    Ok(json!({ "copies": copies }))
}
