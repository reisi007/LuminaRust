//! `lumina_collections` — the source-level collection memberships of one
//! sidecar, as an MCP tool (MCP-PARITY-B).
//!
//! Mirrors `lumina collections` by building the same
//! [`lumina_stages::collections::CollectionsRequest`] and calling the same
//! `run`, so the memberships an agent writes are the memberships the CLI
//! writes, through the same `lumina_sidecar::save_sidecar`.
//!
//! Read and write are separate: `op="list"` never writes, and every write op
//! names exactly one membership. There is deliberately no `virtual_copy` field:
//! memberships are a **source-level** field of the document, not a per-copy one,
//! so there is nothing to resolve and a caller must not invent a copy scope.

use crate::error::McpError;
use crate::tools::library_common::{map_error, result_payload, PATH_PERSIST};
use crate::tools::stage_common::{
    optional_str, reject_unknown_fields, reject_unknown_op, require_value,
};
use crate::util::get_str;
use crate::Server;
use lumina_stages::collections::CollectionsRequest;
use serde_json::{json, Value};

pub const NAME: &str = "lumina_collections";
pub const DESCRIPTION: &str = "List and edit the static collection memberships of one sidecar \
    (G-15 META-MVP Slice 2) — mirrors `lumina collections`. Path-based: one call is one path, no \
    `image_id`. `op=\"list\"` is read-only and never writes; `op=\"add\"` takes one `membership` \
    as `id=name` (adds it, or renames an existing id); `op=\"remove\"` takes one `id` (idempotent). \
    Every write goes through `document.validate()` and the same atomic `lumina_sidecar::save_sidecar` \
    the CLI uses. A missing sidecar, a malformed `id=name`, an invalid id/name and an unknown field \
    abort loudly (InvalidParams) and change no bytes. Reported `changed: false` is an idempotent \
    no-op, not a success that wrote something.";

const ALLOWED: &[&str] = &["path", "op", "membership", "id"];

const OPS: &[&str] = &["list", "add", "remove"];

pub fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "path": { "type": "string", "description": "Source image path; the sidecar lives next to it." },
            "op": {
                "type": "string",
                "enum": OPS,
                "description": "list is read-only; add/remove change exactly one membership."
            },
            "membership": {
                "type": "string",
                "description": "op=add: `id=name` (split at the first `=`). Adds the membership, or renames an existing id."
            },
            "id": { "type": "string", "description": "op=remove: the membership id (idempotent)." }
        },
        "required": ["path", "op"]
    })
}

pub fn run(_server: &mut Server, args: &Value) -> Result<Value, McpError> {
    reject_unknown_fields(args, NAME, ALLOWED)?;
    let path = get_str(args, "path")?.to_owned();
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
    let membership = optional_str(args, "membership")?;
    let id = optional_str(args, "id")?;
    // A write op that names no value must abort loudly instead of degrading
    // into a successful read.
    require_value(
        NAME,
        &op,
        &[
            ("membership", op == "add" && membership.is_none()),
            ("id", op == "remove" && id.is_none()),
        ],
    )?;
    // The CLI's `--add-to`/`--remove-from` are repeatable; the MCP tool takes
    // exactly one membership per call so "one explicit thing per call" holds.
    let mut request = CollectionsRequest {
        input: path,
        ..Default::default()
    };
    match op.as_str() {
        "add" => request.add_to = vec![membership.expect("required above")],
        "remove" => request.remove_from = vec![id.expect("required above")],
        _ => {}
    }
    let outcome = lumina_stages::collections::run(&request, PATH_PERSIST)
        .map_err(|error| map_error(NAME, error))?;
    Ok(result_payload(outcome.report))
}
