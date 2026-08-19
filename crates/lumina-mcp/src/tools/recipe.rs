//! `lumina_get_recipe` — return the full recipe of a virtual copy plus its hash.

use crate::error::McpError;
use crate::util::{get_str, recipe_hash};
use crate::Server;
use serde_json::{json, Value};

pub const NAME: &str = "lumina_get_recipe";
pub const DESCRIPTION: &str = "Return the full EditRecipe of a virtual copy together with a \
recipe_hash so the agent can track changes across edits.";

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "image_id": { "type": "string", "description": "Session image_id from lumina_load." },
            "virtual_copy": {
                "type": "string",
                "description": "Virtual copy name or id (default: the standard copy)."
            }
        },
        "required": ["image_id"]
    })
}

pub fn run(server: &mut Server, args: &Value) -> Result<Value, McpError> {
    let image_id = get_str(args, "image_id")?;
    let state = server.session.require_id(image_id)?;
    let requested = args.get("virtual_copy").and_then(|value| value.as_str());
    let copy = state.find_copy(requested)?;
    let hash = recipe_hash(&copy.recipe);
    Ok(json!({
        "recipe": copy.recipe,
        "recipe_hash": hash,
    }))
}
