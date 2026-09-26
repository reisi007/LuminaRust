//! `lumina_relocate` — move one image together with its sidecar companions, as
//! an MCP tool (MCP-PARITY-B).
//!
//! Mirrors `lumina relocate` by building the same
//! [`lumina_stages::relocate::RelocateRequest`] and calling the same `run`.
//!
//! The two things that must not be left out are covered structurally, because
//! they live in the shared code and not here:
//!
//! * **target collision** — an existing target image *or* an existing target
//!   companion (`.lumina.json` / `.lumina.zdata`) aborts before anything moves.
//!   Without the second check a move could leave an orphaned recipe beside the
//!   old image while the new path has none: a path without a recipe.
//! * **the sidecar move** — the companions are derived from the **target** image
//!   path and moved with it, so a rename keeps the recipe attached.
//!
//! A missing source, a non-directory target parent and a failed companion move
//! are loud too, and the companion error names the step reached (the image may
//! already sit at the target — no silent half state).

use crate::error::McpError;
use crate::tools::library_common::{map_error, result_payload};
use crate::tools::stage_common::{reject_unknown_fields, reject_unknown_op};
use crate::util::get_str;
use crate::Server;
use lumina_stages::relocate::RelocateRequest;
use serde_json::{json, Value};

pub const NAME: &str = "lumina_relocate";
pub const DESCRIPTION: &str =
    "Move one image together with its `.lumina.json` and `.lumina.zdata` \
    companions (G-09 Library-Parität) — mirrors `lumina relocate`. Path-based: one call is one \
    from/to pair, no `image_id`. Companions are derived from the TARGET image path, so a rename \
    keeps the recipe attached to the new name. Refuses before moving anything when the source is \
    missing, when the target image exists, when a target COMPANION exists (that would orphan the \
    recipe), or when the target parent is not a directory; loud (InvalidParams/SidecarError) and \
    no bytes move. A cross-volume move falls back to copy+remove. Moves no recipe fields — the \
    sidecar travels with the image.";

const ALLOWED: &[&str] = &["from", "to", "op"];

const OPS: &[&str] = &["move"];

pub fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "from": { "type": "string", "description": "Source image to move (must exist)." },
            "to": { "type": "string", "description": "Destination image path (must not exist; parent must be a directory)." },
            "op": {
                "type": "string",
                "enum": OPS,
                "description": "move is the only operation."
            }
        },
        "required": ["from", "to", "op"]
    })
}

pub fn run(_server: &mut Server, args: &Value) -> Result<Value, McpError> {
    reject_unknown_fields(args, NAME, ALLOWED)?;
    let op = match args.get("op") {
        Some(Value::String(value)) if !value.is_empty() => value.clone(),
        Some(Value::String(_)) => {
            return Err(McpError::InvalidParams("`op` must not be empty".into()))
        }
        Some(other) => {
            return Err(McpError::InvalidParams(format!(
                "`op` must be a string, got `{other}`"
            )))
        }
        None => return Err(McpError::InvalidParams("missing `op`".into())),
    };
    reject_unknown_op(NAME, &op, OPS)?;
    let from = get_str(args, "from")?.to_owned();
    let to = get_str(args, "to")?.to_owned();
    let request = RelocateRequest { from, to };
    let outcome = lumina_stages::relocate::run(&request).map_err(|error| map_error(NAME, error))?;
    Ok(result_payload(outcome.report))
}
