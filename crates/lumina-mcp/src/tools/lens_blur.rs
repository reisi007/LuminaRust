//! `lumina_lens_blur` — the G-05 depth-bokeh stage editor as an MCP tool.
//!
//! Wraps the CLI's `lumina lens-blur` by building a
//! [`lumina_stages::LensBlurRequest`] and calling
//! [`lumina_stages::lens_blur::run`] — the same function the CLI calls.
//!
//! Read and write are separate: `op = "list"` is read-only; every other op sets
//! exactly one named field or removes exactly one named thing. There is no op
//! that writes a whole stage object.
//!
//! The loud contracts are the CLI's, unchanged: an out-of-range amount, a
//! malformed focus rect, a non-portable (absolute) depth-artifact path and an
//! **inverted focal range** (`focal_near > focal_far`) are all rejected by the
//! shared validator before a single byte is written. A referenced-but-unresolved
//! depth artifact is reported as `status: "missing depth artifact"` by the
//! shared `lumina_core::lens_blur_status` decision layer and aborts renders
//! loudly — the tool never silently falls back to the heuristic.

use crate::error::McpError;
use crate::tools::stage_common::{
    self, optional_f32, optional_str, reject_unknown_fields, reject_unknown_op, required_str,
    virtual_copy,
};
use crate::util::get_str;
use crate::Server;
use lumina_stages::lens_blur::{run as shared_run, LensBlurRequest};
use lumina_stages::Persist;
use serde_json::{json, Value};

pub const NAME: &str = "lumina_lens_blur";
pub const DESCRIPTION: &str = "Inspect and edit the G-05 depth-bokeh (lens blur) stage of one \
virtual copy: enabled state, focus rectangle, focal range, blur amount, bokeh shape and the \
optional external depth-artifact reference. Non-destructive. `op=\"list\"` is read-only; every other \
op changes exactly one named field. Unknown copy/shape, an out-of-range amount or focal edge, a \
malformed or non-portable (absolute) depth-artifact path and an inverted focal range abort loudly \
(InvalidParams) and change no bytes. A referenced-but-missing depth artifact is reported as \
status `missing depth artifact`, never silently replaced by the heuristic. Writes are a \
compare-and-swap against the revision seen at lumina_load (SidecarConflict on a miss).";

const ALLOWED: &[&str] = &[
    "image_id",
    "virtual_copy",
    "op",
    "amount",
    "focal_near",
    "focal_far",
    "bokeh",
    "focus_rect",
    "depth_artifact",
];

const OPS: &[&str] = &[
    "list",
    "enable",
    "disable",
    "set_amount",
    "set_focal_near",
    "set_focal_far",
    "set_bokeh",
    "set_focus_rect",
    "set_depth_artifact",
    "clear_depth_artifact",
    "clear",
];

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
                "description": "list is read-only; every other op changes exactly one field."
            },
            "amount": { "type": "number", "minimum": 0, "maximum": 1, "description": "op=set_amount: blur strength (0 is identity)." },
            "focal_near": { "type": "number", "minimum": 0, "maximum": 1, "description": "op=set_focal_near: near edge of the sharp depth band." },
            "focal_far": { "type": "number", "minimum": 0, "maximum": 1, "description": "op=set_focal_far: far edge; must stay >= focal_near." },
            "bokeh": { "type": "string", "enum": ["round", "elliptical", "hexagonal"], "description": "op=set_bokeh: bokeh shape." },
            "focus_rect": {
                "type": "object",
                "description": "op=set_focus_rect: focus rectangle as x,y,width,height in normalized 0..=1 coordinates.",
                "properties": {
                    "x": { "type": "number", "minimum": 0, "maximum": 1 },
                    "y": { "type": "number", "minimum": 0, "maximum": 1 },
                    "width": { "type": "number", "exclusiveMinimum": 0, "maximum": 1 },
                    "height": { "type": "number", "exclusiveMinimum": 0, "maximum": 1 }
                },
                "required": ["x", "y", "width", "height"],
                "additionalProperties": false
            },
            "depth_artifact": {
                "type": "object",
                "description": "op=set_depth_artifact: external depth reference. `relative_path` must be a portable RELATIVE path (an absolute path is rejected loudly).",
                "properties": {
                    "relative_path": { "type": "string", "description": "Portable relative path of the depth artifact." },
                    "sha256": { "type": "string", "description": "Content hash of the depth artifact." }
                },
                "required": ["relative_path", "sha256"],
                "additionalProperties": false
            },
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
    let amount = optional_f32(args, "amount")?;
    let focal_near = optional_f32(args, "focal_near")?;
    let focal_far = optional_f32(args, "focal_far")?;
    let bokeh = optional_str(args, "bokeh")?;
    // A nested object is the natural MCP shape; the CLI takes the `x,y,w,h` /
    // `PATH:SHA256` text forms. Render the object into those exact text forms so
    // the shared parsers — and therefore the shared error messages — stay the
    // single implementation.
    let focus_rect = match args.get("focus_rect") {
        None | Some(Value::Null) => None,
        Some(value) => Some(render_focus_rect(value)?),
    };
    let depth_artifact = match args.get("depth_artifact") {
        None | Some(Value::Null) => None,
        Some(value) => Some(render_depth_artifact(value)?),
    };
    // A write op that names no value must abort loudly instead of degrading
    // into a successful read.
    stage_common::require_value(
        NAME,
        &op,
        &[
            ("amount", op == "set_amount" && amount.is_none()),
            ("focal_near", op == "set_focal_near" && focal_near.is_none()),
            ("focal_far", op == "set_focal_far" && focal_far.is_none()),
            ("bokeh", op == "set_bokeh" && bokeh.is_none()),
            ("focus_rect", op == "set_focus_rect" && focus_rect.is_none()),
            (
                "depth_artifact",
                op == "set_depth_artifact" && depth_artifact.is_none(),
            ),
        ],
    )?;
    let source = server.session.require_id(image_id)?.source_path.clone();

    stage_common::execute(
        server,
        NAME,
        image_id,
        &source,
        |input| LensBlurRequest {
            input: input.to_owned(),
            virtual_copy: copy,
            list: op == "list",
            enable: op == "enable",
            disable: op == "disable",
            set_amount: if op == "set_amount" { amount } else { None },
            set_focal_near: if op == "set_focal_near" {
                focal_near
            } else {
                None
            },
            set_focal_far: if op == "set_focal_far" {
                focal_far
            } else {
                None
            },
            set_bokeh: if op == "set_bokeh" { bokeh } else { None },
            set_focus_rect: if op == "set_focus_rect" {
                focus_rect
            } else {
                None
            },
            set_depth_artifact: if op == "set_depth_artifact" {
                depth_artifact
            } else {
                None
            },
            clear_depth_artifact: op == "clear_depth_artifact",
            clear: op == "clear",
        },
        |request| shared_run(request, Persist::Deferred),
    )
}

/// Renders the `focus_rect` object into the CLI's `x,y,width,height` text form.
/// A missing or non-numeric member is a loud `InvalidParams` here, before the
/// shared editor is even reached.
fn render_focus_rect(value: &Value) -> Result<String, McpError> {
    let object = value.as_object().ok_or_else(|| {
        McpError::InvalidParams("`focus_rect` must be an object with x, y, width, height".into())
    })?;
    let mut parts = Vec::with_capacity(4);
    for key in ["x", "y", "width", "height"] {
        let member = object
            .get(key)
            .ok_or_else(|| McpError::InvalidParams(format!("`focus_rect` requires `{key}`")))?;
        let number = member.as_f64().ok_or_else(|| {
            McpError::InvalidParams(format!(
                "`focus_rect.{key}` must be a number, got `{member}`"
            ))
        })?;
        if !number.is_finite() {
            return Err(McpError::InvalidParams(format!(
                "`focus_rect.{key}` must be a finite number"
            )));
        }
        parts.push(number.to_string());
    }
    Ok(parts.join(","))
}

/// Renders the `depth_artifact` object into the CLI's `RELATIVE_PATH:SHA256`
/// text form.
fn render_depth_artifact(value: &Value) -> Result<String, McpError> {
    let object = value
        .as_object()
        .ok_or_else(|| McpError::InvalidParams("`depth_artifact` must be an object".into()))?;
    let path = object
        .get("relative_path")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            McpError::InvalidParams("`depth_artifact.relative_path` must be a string".into())
        })?;
    let sha = object
        .get("sha256")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            McpError::InvalidParams("`depth_artifact.sha256` must be a string".into())
        })?;
    if path.contains(':') {
        return Err(McpError::InvalidParams(format!(
            "`depth_artifact.relative_path` must not contain `:` (the CLI form is `PATH:SHA256`): `{path}`"
        )));
    }
    Ok(format!("{path}:{sha}"))
}
