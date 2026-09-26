//! `lumina_generative` — produce / report / remove the persisted
//! `generative_canvas` artefact, as an MCP tool (MCP-PARITY-B, GEN-ONNX-1
//! Welle 1).
//!
//! Mirrors `lumina generative` by building the same
//! [`lumina_stages::generative::GenerativeRequest`] and calling the same `run`
//! with the same fixture model, the same deterministic record id and the same
//! portable artefact link.
//!
//! # The model/artefact gate
//!
//! This tool **stays behind that gate** and aborts with the CLI's own error
//! instead of faking a result:
//!
//! * `op="generate"` with no active role (`expand`/`auto_fill` both false) is a
//!   loud refusal, not a successful no-op;
//! * `op="generate"` where the role cannot produce a canvas — an
//!   `auto_fill` request with no transparent pixels after the lens stage, or an
//!   `expand` without a `canvas` — writes **no** record, links **no** artefact
//!   and changes **no** sidecar byte;
//! * `op="status"` on a role whose artefact is not `available`/`not-required`
//!   fails (SidecarError) with the named status, exactly like the CLI, whose
//!   message ends in "no silent fallback";
//! * an unsupported `generative_edit` version, an unknown `virtual_copy` and a
//!   source that changed since the sidecar was written are all loud.
//!
//! `op="remove"` unlinks the artefact and keeps the bundle record, which is the
//! CLI's deliberate state: the role stays `missing` until `--generate` runs
//! again, and the tool never re-adopts the still-present record on its own.

use crate::error::McpError;
use crate::tools::library_common::{map_error, result_payload, PATH_PERSIST};
use crate::tools::stage_common::{
    optional_bool, optional_str, optional_u64, reject_unknown_fields, reject_unknown_op,
    require_value, virtual_copy,
};
use crate::util::get_str;
use crate::Server;
use lumina_stages::generative::GenerativeRequest;
use lumina_stages::generative_artifact::NoCorrector;
use serde_json::{json, Value};

pub const NAME: &str = "lumina_generative";
pub const DESCRIPTION: &str = "Produce, report or remove the persisted `generative_canvas` \
    artefact of one virtual copy (GEN-ONNX-1 Welle 1) — mirrors `lumina generative`. Path-based: \
    one call is one path, no `image_id`. `op=\"status\"` is read-only; `op=\"generate\"` produces the \
    canvas through the same ONNX producer the CLI uses and links it; `op=\"remove\"` unlinks it and \
    keeps the bundle record. `generate` REQUIRES an active role (`expand` or `auto_fill`) and \
    refuses loudly — writing no record, no link and no sidecar byte — when no canvas can be \
    produced; `status` fails loudly when a role is not available instead of reporting an empty \
    success. `op` is required: a call without one is a refusal, never a read. Unknown copy, \
    unsupported `generative_edit` version and a source changed since the sidecar was written abort \
    loudly. `virtual_copy` defaults to the first copy, as in the CLI.";

const ALLOWED: &[&str] = &[
    "path",
    "op",
    "virtual_copy",
    "force",
    "prompt",
    "negative_prompt",
    "seed",
    "auto_fill",
    "expand",
    "canvas",
    "keep",
];

const OPS: &[&str] = &["status", "generate", "remove"];

pub fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "path": { "type": "string", "description": "Source image path; the sidecar and the .lumina.zdata bundle live next to it." },
            "op": {
                "type": "string",
                "enum": OPS,
                "description": "Required. status is read-only, generate produces + links, remove unlinks."
            },
            "virtual_copy": {
                "type": "string",
                "description": "Virtual copy id (default: the first copy, as in the CLI). An unknown id aborts loudly."
            },
            "force": {
                "type": "boolean",
                "description": "op=generate: replace an existing record for the same identity."
            },
            "prompt": { "type": "string", "description": "Identity-bearing prompt (may be empty)." },
            "negative_prompt": { "type": "string", "description": "Identity-bearing negative prompt." },
            "seed": { "type": "integer", "minimum": 0, "description": "Deterministic, identity-bearing seed." },
            "auto_fill": { "type": "boolean", "description": "auto_fill_transparent: fill transparent pixels after the lens stage." },
            "expand": { "type": "boolean", "description": "expand_beyond_image: enlarge the canvas (requires canvas)." },
            "canvas": { "type": "string", "description": "Target canvas `WxH+X+Y` (offsets may be negative), e.g. 48x48+8+8." },
            "keep": { "type": "boolean", "description": "keep_generative_content crop decision." }
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
    let copy = virtual_copy(args)?;
    let expand = optional_bool(args, "expand")?.unwrap_or(false);
    let auto_fill = optional_bool(args, "auto_fill")?.unwrap_or(false);
    let canvas = optional_str(args, "canvas")?;
    // `expand` without a canvas can never produce a canvas, so it is refused
    // here rather than after the model call.
    require_value(
        NAME,
        &op,
        &[
            ("canvas", expand && op == "generate" && canvas.is_none()),
            (
                "expand or auto_fill",
                op == "generate" && !expand && !auto_fill,
            ),
        ],
    )?;
    let request = GenerativeRequest {
        input: path,
        virtual_copy: copy,
        status: op == "status",
        generate: op == "generate",
        force: optional_bool(args, "force")?.unwrap_or(false),
        remove: op == "remove",
        prompt: optional_str(args, "prompt")?,
        negative_prompt: optional_str(args, "negative_prompt")?,
        seed: optional_u64(args, "seed")?,
        auto_fill,
        expand,
        canvas,
        keep: optional_bool(args, "keep")?,
    };
    // `lumina-mcp` does not link the optional native `lensfun` capability, so
    // there is no corrector to resolve: `NoCorrector` is a real "no profile
    // applies" answer, and it is what every build without that capability
    // already passes. A hard-coded "correction" string would be a fake.
    let mut correctors = NoCorrector;
    let outcome = lumina_stages::generative::run(&request, &mut correctors, PATH_PERSIST)
        .map_err(|error| map_error(NAME, error))?;
    Ok(result_payload(outcome.report))
}
