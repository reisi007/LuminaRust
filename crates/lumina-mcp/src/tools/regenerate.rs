//! `lumina_regenerate` — explicit per-module regeneration of the 1.0 derivable
//! AI/analysis values, as an MCP tool (MCP-PARITY-B, GUI-GEN-GRANULAR-10 /
//! F-100).
//!
//! Mirrors `lumina regenerate` by building the same
//! [`lumina_stages::regenerate::RegenerateRequest`] and calling the same `run`,
//! including the shared Auto-Tone write path — the same six sliders, the same
//! six `auto_features` mirrors and the same analysis fingerprint, through the
//! same presence-based freshness predicate. A recipe the CLI calls fresh is
//! fresh here too, so an agent can never silently destroy a user override.
//!
//! # The artefact gate
//!
//! `op="matching"` is the module that loads an artefact: it re-derives
//! `matched_exposure` from a real render of the current recipe, with the
//! persisted mask planes and source actions resolved as a render resolves them.
//! If that render cannot be produced — an active generative role whose artefact
//! is missing, an invalid recipe, a corrupt `.lumina.zdata` bundle — the module
//! aborts loudly and **no** sidecar byte is written. The tool therefore cannot
//! report a derived `matched_exposure` for a frame the renderer refused.
//!
//! `op="masks"` is a **refresh request**, not an inference: it marks the
//! selected source masks `Pending` and (explicit call only) arms the copy-wide
//! one-shot flag. It never persists a stub matte as a valid artefact.
//!
//! `op="all"` is the collective default: only stale or missing modules run.

use crate::error::McpError;
use crate::tools::library_common::{map_error, result_payload, PATH_PERSIST};
use crate::tools::stage_common::{
    optional_f64, reject_unknown_fields, reject_unknown_op, virtual_copy,
};
use crate::util::get_str;
use crate::Server;
use lumina_stages::generative_artifact::NoCorrector;
use lumina_stages::regenerate::{CpuRender, RegenerateModule, RegenerateRequest};
use serde_json::{json, Value};

pub const NAME: &str = "lumina_regenerate";
pub const DESCRIPTION: &str = "Regenerate the explicit or stale derivable AI/analysis modules of \
    one image (GUI-GEN-GRANULAR-10 / F-100) — mirrors `lumina regenerate`. Path-based: one call is \
    one path, no `image_id`. `op=\"all\"` is the collective default and regenerates only stale or \
    missing values; `op=\"masks\"`, `op=\"auto_tone\"` and `op=\"matching\"` force exactly that module \
    and leave every other persisted artefact untouched. Nothing is recomputed implicitly and the \
    sidecar is written atomically only when something actually changed. `masks` is a refresh \
    REQUEST (the next render re-infers; no stub matte is persisted); `auto_tone` writes the six \
    sliders plus the six mirrors and the analysis fingerprint through the single shared write path; \
    `matching` re-derives `matched_exposure` from a real render of the current recipe, which is \
    rendered WITHOUT a generative canvas input — the pre-existing CLI behaviour this mirrors. So a \
    copy with an ACTIVE generative role cannot be rendered at all and the module ABORTS LOUDLY \
    (writing no bytes) with `generative_artifact.*.missing`; the same holds for an invalid recipe \
    or a corrupt bundle. A non-finite `target_luminance` outside 0..=1, an \
    unknown `op`/`virtual_copy` and an unknown field abort loudly.";

const ALLOWED: &[&str] = &["path", "op", "virtual_copy", "target_luminance"];

const OPS: &[&str] = &["all", "masks", "auto_tone", "matching"];

pub fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "path": { "type": "string", "description": "Source image path; the sidecar lives next to it." },
            "op": {
                "type": "string",
                "enum": OPS,
                "description": "all = collective default (stale/missing only); the other three force exactly that module."
            },
            "virtual_copy": {
                "type": "string",
                "description": "Virtual copy id (default: the first copy, as in the CLI). An unknown id aborts loudly."
            },
            "target_luminance": {
                "type": "number",
                "minimum": 0,
                "maximum": 1,
                "description": "Target luminance of the tone analysis and the exposure matching (default 0.5, as in the CLI)."
            }
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
    // The CLI default is 0.5 (`--target-luminance`); the shared code validates
    // the range, so an out-of-range value is refused with the CLI's message.
    let target_luminance = optional_f64(args, "target_luminance")?.unwrap_or(0.5);
    let modules = match op.as_str() {
        "all" => Vec::new(),
        "masks" => vec![RegenerateModule::Masks],
        "auto_tone" => vec![RegenerateModule::AutoTone],
        "matching" => vec![RegenerateModule::Matching],
        _ => unreachable!("reject_unknown_op above already refused every other word"),
    };
    let request = RegenerateRequest {
        input: path,
        virtual_copy: copy,
        modules,
        target_luminance,
    };
    // `CpuRender` is the CPU oracle — the same `lumina_core::render_frame` the
    // CLI reaches in a build without the optional `gpu` capability, which is
    // every default build. The GPU is an accelerator and its parity is a
    // separate, still-open gate (`GPU-PARITY-HW-28`); a tool must not pretend
    // to be on it.
    let mut correctors = NoCorrector;
    let outcome =
        lumina_stages::regenerate::run(&request, &mut correctors, &CpuRender, PATH_PERSIST)
            .map_err(|error| map_error(NAME, error))?;
    Ok(result_payload(outcome.report))
}
