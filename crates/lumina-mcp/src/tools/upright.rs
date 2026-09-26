//! `lumina_upright` — the LRPAR-G06-UPRIGHT-15 automatic-upright stage editor as
//! an MCP tool.
//!
//! Wraps the CLI's `lumina upright` by building a
//! [`lumina_stages::UprightRequest`] and calling
//! [`lumina_stages::upright::run`] — the same function the CLI calls. The
//! analysis is classic, model-free and deterministic
//! (`lumina_core::analyze_upright`), and it is bound to the current source
//! identity by the shared `upright_input_fingerprint`, so an MCP call persists
//! the *same* fingerprint a CLI call would.
//!
//! Read and write are separate: `op = "list"` is read-only and reports
//! `fresh`/`stale` against the current source (a stale analysis is reported,
//! never silently recomputed). The write ops are `analyze`, `enable`, `disable`
//! and `clear`.

use crate::error::McpError;
use crate::tools::stage_common::{
    self, optional_bool, reject_unknown_fields, reject_unknown_op, required_str, virtual_copy,
};
use crate::util::get_str;
use crate::Server;
use lumina_stages::upright::{run as shared_run, UprightRequest};
use lumina_stages::Persist;
use serde_json::{json, Value};

pub const NAME: &str = "lumina_upright";
pub const DESCRIPTION: &str = "Inspect and edit the persisted automatic-upright stage of one virtual \
copy. `op=\"list\"` is read-only and reports the stage plus fresh/stale against the current source \
(a stale analysis is reported, never silently recomputed). `op=\"analyze\"` runs the deterministic, \
model-free upright-lines-v1 analysis on the decoded source and persists it bound to the source \
fingerprint; `enable`/`disable` toggle whether the persisted suggestion supplies the effective \
perspective; `clear` removes the stage. `enable` without a persisted analysis aborts loudly. Writes \
are a compare-and-swap against the revision seen at lumina_load (SidecarConflict on a miss).";

const ALLOWED: &[&str] = &["image_id", "virtual_copy", "op", "disable_after_analyze"];

const OPS: &[&str] = &["list", "analyze", "enable", "disable", "clear"];

pub fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "image_id": { "type": "string", "description": "Session image_id from lumina_load." },
            "virtual_copy": {
                "type": "string",
                "description": "Virtual copy id (default: the standard copy). An unknown id aborts loudly."
            },
            "op": {
                "type": "string",
                "enum": OPS,
                "description": "list is read-only; analyze/enable/disable/clear each change the stage in exactly one named way."
            },
            "disable_after_analyze": {
                "type": "boolean",
                "default": false,
                "description": "op=analyze only: keep the fresh analysis persisted but inactive (the CLI's `upright --analyze --disable`)."
            }
        },
        "required": ["image_id", "op"]
    })
}

pub fn run(server: &mut Server, args: &Value) -> Result<Value, McpError> {
    reject_unknown_fields(args, NAME, ALLOWED)?;
    let image_id = get_str(args, "image_id")?;
    let op = required_str(args, "op")?;
    reject_unknown_op(NAME, &op, OPS)?;
    let copy = virtual_copy(args)?;
    let disable_after_analyze = optional_bool(args, "disable_after_analyze")?.unwrap_or(false);
    if disable_after_analyze && op != "analyze" {
        return Err(McpError::InvalidParams(format!(
            "`disable_after_analyze` only applies to op=analyze, got `{op}`"
        )));
    }
    let source = server.session.require_id(image_id)?.source_path.clone();

    stage_common::execute(
        server,
        NAME,
        image_id,
        &source,
        |input| UprightRequest {
            input: input.to_owned(),
            virtual_copy: copy,
            list: op == "list",
            analyze: op == "analyze",
            enable: op == "enable",
            disable: op == "disable" || (op == "analyze" && disable_after_analyze),
            clear: op == "clear",
        },
        |request| shared_run(request, Persist::Deferred),
    )
}
