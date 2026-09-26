//! `lumina_smart_collections` — evaluate a portable smart-collection catalogue
//! against the sidecars under a path, as an MCP tool (MCP-PARITY-B).
//!
//! Mirrors `lumina smart-collections` by building the same
//! [`lumina_stages::smart_collections::SmartCollectionsRequest`] and calling the
//! same `run`, so the same catalogue file, the same deterministic target list
//! and the same per-item verdicts come out of both transports.
//!
//! **Read-only by contract:** the command evaluates rules and reports; it never
//! writes a sidecar, so the tree is byte-identical after any successful call.

use crate::error::McpError;
use crate::tools::library_common::{map_error, result_payload};
use crate::tools::stage_common::{reject_unknown_fields, reject_unknown_op};
use crate::util::get_str;
use crate::Server;
use lumina_stages::smart_collections::SmartCollectionsRequest;
use serde_json::{json, Value};

pub const NAME: &str = "lumina_smart_collections";
pub const DESCRIPTION: &str = "Evaluate a portable smart-collection catalogue against every sidecar \
    under a path (G-15 META-MVP Slice 2) — mirrors `lumina smart-collections`. Path-based and \
    READ-ONLY: one call is one path (a sidecar, an image, or a directory scanned recursively in \
    deterministic order), no `image_id`, and no sidecar byte is ever written. Reports per-item \
    `{ sidecar, status, matches }`. An unreadable or corrupt sidecar and a rule whose version the \
    document does not speak are recorded per item and make the run PARTIAL: the report is still \
    returned, but the tool then fails (SidecarError) instead of reporting a silently empty success. \
    An invalid catalogue file, an empty target set and an unknown field abort loudly \
    (InvalidParams) with no output.";

const ALLOWED: &[&str] = &["path", "op", "catalog"];

/// The single op, named so the call shape matches the other five tools.
const OPS: &[&str] = &["evaluate"];

pub fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "path": {
                "type": "string",
                "description": "A sidecar file, an image, or a directory to scan recursively."
            },
            "op": {
                "type": "string",
                "enum": OPS,
                "description": "evaluate is read-only; there is no write path."
            },
            "catalog": {
                "type": "string",
                "description": "The portable catalogue file ({\"format\":\"lumina-smart-catalog\",\"version\":1,\"collections\":[...]})."
            }
        },
        "required": ["path", "op", "catalog"]
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
    let catalog = get_str(args, "catalog")?.to_owned();
    let request = SmartCollectionsRequest {
        input: path,
        catalog,
    };
    let (outcome, failures) =
        lumina_stages::smart_collections::run(&request).map_err(|error| map_error(NAME, error))?;
    let payload = result_payload(outcome.report);
    if failures.is_partial() {
        // The report says exactly which items failed; the call still fails, so a
        // partial evaluation can never be read as a complete one.
        return Err(McpError::Sidecar(format!(
            "{}: {failed} of the evaluated sidecars failed; the per-item report names each one \
             (a partial run is never a silent success)",
            NAME,
            failed = failures.failed
        )));
    }
    Ok(payload)
}
