//! `lumina_geometry` — the G-06 geometry stage editor as an MCP tool.
//!
//! Wraps the CLI's `lumina geometry` by building a
//! [`lumina_stages::GeometryRequest`] and calling
//! [`lumina_stages::geometry::run`] — the same function the CLI calls, so the
//! three geometry sub-stages (crop/rotation/mirrors, manual lens correction,
//! manual perspective), the exactly-one-history-entry-per-call rule, the loud
//! rejections and the resulting sidecar bytes are identical.
//!
//! Read and write are separate: `op = "list"` is read-only; every other op
//! changes exactly one named field or clears exactly one named sub-stage. There
//! is no op that writes a whole stage object.
//!
//! **Deliberate limit:** the CLI's read-only `--lensfun-status` report
//! (EXIF → Lensfun auto-profile resolution) is **not** exposed. It needs the
//! native `lensfun` capability that `lumina-mcp` does not link, and reporting a
//! hard-coded "unavailable" string from a tool would be a silent fallback. The
//! field therefore reports `null`, exactly like `lumina geometry --json` without
//! `--lensfun-status`.

use crate::error::McpError;
use crate::tools::stage_common::{
    self, optional_f64, optional_str, reject_unknown_fields, reject_unknown_op, required_str,
    virtual_copy,
};
use crate::util::get_str;
use crate::Server;
use lumina_stages::geometry::{run as shared_run, GeometryRequest};
use lumina_stages::Persist;
use serde_json::{json, Value};

pub const NAME: &str = "lumina_geometry";
pub const DESCRIPTION: &str = "Inspect and edit the G-06 geometry stages of one virtual copy: crop \
(aspect preset or free rectangle), rotation/straighten, mirrors, manual lens correction and manual \
perspective. Non-destructive; the original image is never modified. `op=\"list\"` is read-only; every \
other op changes exactly one named field or clears one named sub-stage, and each call appends exactly \
one history entry. Unknown copy/preset/field, a malformed or out-of-frame rectangle, an unknown \
mirror word, a non-finite or out-of-range rotation and every contradiction abort loudly \
(InvalidParams) and change no bytes. Writes are a compare-and-swap against the revision seen at \
lumina_load (SidecarConflict on a miss).";

const ALLOWED: &[&str] = &[
    "image_id",
    "virtual_copy",
    "op",
    "preset",
    "rect",
    "value",
    "mirror",
    "profile",
    "field",
];

const OPS: &[&str] = &[
    "list",
    "set_crop_aspect",
    "set_crop_free",
    "clear_crop",
    "set_rotation",
    "set_mirror",
    "clear_geometry",
    "set_lens_profile",
    "set_lens_field",
    "clear_lens",
    "set_perspective_field",
    "clear_perspective",
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
                "description": "list is read-only; every other op changes exactly one field or clears one sub-stage."
            },
            "preset": {
                "type": "string",
                "enum": ["original", "1:1", "4:5", "5:4", "3:2", "2:3", "4:3", "3:4", "16:9", "9:16"],
                "description": "op=set_crop_aspect: aspect preset."
            },
            "rect": {
                "type": "object",
                "description": "op=set_crop_free: free crop rectangle in normalized 0..=1 coordinates.",
                "properties": {
                    "x": { "type": "number", "minimum": 0, "maximum": 1 },
                    "y": { "type": "number", "minimum": 0, "maximum": 1 },
                    "width": { "type": "number", "exclusiveMinimum": 0, "maximum": 1 },
                    "height": { "type": "number", "exclusiveMinimum": 0, "maximum": 1 }
                },
                "required": ["x", "y", "width", "height"],
                "additionalProperties": false
            },
            "value": {
                "type": "number",
                "description": "op=set_rotation (degrees, -180..=180) / op=set_lens_field / op=set_perspective_field."
            },
            "mirror": { "type": "string", "enum": ["h", "v", "hv", "none"], "description": "op=set_mirror." },
            "profile": { "type": "string", "description": "op=set_lens_profile: wide-light|tele-light|standard-neutral." },
            "field": {
                "type": "string",
                "description": "op=set_lens_field: distortion_k1|distortion_k2|distortion_k3|vignette_c0|vignette_c1|vignette_c2|ca_red|ca_blue. op=set_perspective_field: vertical|horizontal|rotation|scale|aspect_ratio|shift_x|shift_y."
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
    let preset = optional_str(args, "preset")?;
    let mirror = optional_str(args, "mirror")?;
    let profile = optional_str(args, "profile")?;
    let field = optional_str(args, "field")?;
    let value = optional_f64(args, "value")?;
    // The nested `rect` object is rendered into the CLI's `x,y,w,h` text form so
    // the shared parser (and its error text) stays the single implementation.
    let rect = match args.get("rect") {
        None | Some(Value::Null) => None,
        Some(value) => Some(render_rect(value)?),
    };
    // A write op that names no value must abort loudly instead of degrading
    // into a successful read.
    stage_common::require_value(
        NAME,
        &op,
        &[
            ("preset", op == "set_crop_aspect" && preset.is_none()),
            ("rect", op == "set_crop_free" && rect.is_none()),
            ("value", op == "set_rotation" && value.is_none()),
            ("mirror", op == "set_mirror" && mirror.is_none()),
            ("profile", op == "set_lens_profile" && profile.is_none()),
            ("field", op == "set_lens_field" && field.is_none()),
            ("value", op == "set_lens_field" && value.is_none()),
            ("field", op == "set_perspective_field" && field.is_none()),
            ("value", op == "set_perspective_field" && value.is_none()),
        ],
    )?;
    let source = server.session.require_id(image_id)?.source_path.clone();

    // One `FIELD:VALUE` spec per call: the CLI's repeatable `--set-lens` /
    // `--set-perspective` are passed as a single-element list here, which keeps
    // "one explicit field per call" and the one-history-entry-per-call rule.
    let lens = if op == "set_lens_field" {
        Some(vec![render_field_value(NAME, field.as_deref(), value)?])
    } else {
        None
    };
    let perspective = if op == "set_perspective_field" {
        Some(vec![render_field_value(NAME, field.as_deref(), value)?])
    } else {
        None
    };

    stage_common::execute(
        server,
        NAME,
        image_id,
        &source,
        |input| GeometryRequest {
            input: input.to_owned(),
            virtual_copy: copy,
            list: op == "list",
            set_crop_aspect: if op == "set_crop_aspect" {
                preset
            } else {
                None
            },
            set_crop_free: if op == "set_crop_free" { rect } else { None },
            clear_crop: op == "clear_crop",
            set_rotation: if op == "set_rotation" { value } else { None },
            // The MCP tool has one rotation op; `--straighten` is only an alias of
            // `--set-rotation` in the CLI, so there is nothing to lose.
            straighten: None,
            set_mirror: if op == "set_mirror" { mirror } else { None },
            clear_geometry: op == "clear_geometry",
            set_lens_profile: if op == "set_lens_profile" {
                profile
            } else {
                None
            },
            set_lens: lens.unwrap_or_default(),
            clear_lens: op == "clear_lens",
            set_perspective: perspective.unwrap_or_default(),
            clear_perspective: op == "clear_perspective",
            lensfun_status: false,
            lensfun_report: None,
        },
        |request| shared_run(request, Persist::Deferred),
    )
}

/// Renders the `rect` object into the CLI's `x,y,width,height` text form.
fn render_rect(value: &Value) -> Result<String, McpError> {
    let object = value.as_object().ok_or_else(|| {
        McpError::InvalidParams("`rect` must be an object with x, y, width, height".into())
    })?;
    let mut parts = Vec::with_capacity(4);
    for key in ["x", "y", "width", "height"] {
        let member = object
            .get(key)
            .ok_or_else(|| McpError::InvalidParams(format!("`rect` requires `{key}`")))?;
        let number = member.as_f64().ok_or_else(|| {
            McpError::InvalidParams(format!("`rect.{key}` must be a number, got `{member}`"))
        })?;
        if !number.is_finite() {
            return Err(McpError::InvalidParams(format!(
                "`rect.{key}` must be a finite number"
            )));
        }
        parts.push(number.to_string());
    }
    Ok(parts.join(","))
}

/// Renders the `field` + `value` pair into the CLI's `FIELD:VALUE` text form.
/// A missing member is a loud `InvalidParams` here; an unknown field name is
/// rejected by the shared parser with the CLI's own message.
fn render_field_value(
    tool: &str,
    field: Option<&str>,
    value: Option<f64>,
) -> Result<String, McpError> {
    let field =
        field.ok_or_else(|| McpError::InvalidParams(format!("`{tool}`: op requires `field`")))?;
    if field.contains(':') {
        return Err(McpError::InvalidParams(format!(
            "`field` must not contain `:` (the CLI form is `FIELD:VALUE`): `{field}`"
        )));
    }
    let value =
        value.ok_or_else(|| McpError::InvalidParams(format!("`{tool}`: op requires `value`")))?;
    Ok(format!("{field}:{value}"))
}
