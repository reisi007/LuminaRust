//! `lumina_spot` — the G-04 spot-heal stage editor as an MCP tool.
//!
//! Wraps the CLI's `lumina spot` by building a [`lumina_stages::SpotRequest`]
//! and calling [`lumina_stages::spot::run`] — the same function the CLI calls,
//! so the mutation, the loud rejections, the list payload and the resulting
//! sidecar bytes are identical.
//!
//! Read and write are separate: `op = "list"` and `op = "detect"` are read-only;
//! every other op performs exactly one named mutation. There is no op that
//! writes a whole spot list.
//!
//! **Deliberate limit:** the generative variant-seed path
//! (`spot --regenerate-variant`) is NOT exposed. It is part of the
//! generative/regenerate surface that MCP-PARITY-B closes; exposing half of it
//! here would be a stub, not parity.

use crate::error::McpError;
use crate::tools::stage_common::{
    self, optional_f32, optional_str, optional_usize, reject_unknown_fields, reject_unknown_op,
    required_str, virtual_copy,
};
use crate::util::get_str;
use crate::Server;
use lumina_stages::spot::{run as shared_run, SpotRequest};
use lumina_stages::Persist;
use serde_json::{json, Value};

pub const NAME: &str = "lumina_spot";
pub const DESCRIPTION: &str = "Inspect and edit the G-04 spot-heal stage of one virtual copy \
(heuristic spots, visualize threshold, distraction switches, heuristic detection). Non-destructive: \
the original image is never modified. `op=\"list\"` and `op=\"detect\"` are read-only; every other op \
performs exactly one named mutation. Unknown copy/spot/field, out-of-range geometry, an unknown \
distraction key and every contradiction abort loudly (InvalidParams) and change no bytes. The write \
is a compare-and-swap against the revision seen at lumina_load (SidecarConflict on a miss).";

/// Every argument key this tool accepts; anything else is refused.
const ALLOWED: &[&str] = &[
    "image_id",
    "virtual_copy",
    "op",
    "center_x",
    "center_y",
    "radius",
    "feather",
    "offset_dx",
    "offset_dy",
    "opacity",
    "spot_id",
    "threshold",
    "distraction",
    "detect_threshold",
    "detect_max",
];

/// `list` and `detect` are the read ops; every other entry mutates.
const OPS: &[&str] = &[
    "list",
    "detect",
    "add",
    "update",
    "remove",
    "set_visualize",
    "set_distraction",
    "detect_apply",
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
                "description": "list/detect are read-only; every other op performs exactly one mutation."
            },
            "center_x": { "type": "number", "minimum": 0, "maximum": 1, "description": "op=add: spot centre x." },
            "center_y": { "type": "number", "minimum": 0, "maximum": 1, "description": "op=add: spot centre y." },
            "radius": { "type": "number", "exclusiveMinimum": 0, "maximum": 512, "description": "op=add/op=update: heal radius." },
            "feather": { "type": "number", "minimum": 0, "maximum": 1, "description": "op=add/op=update: edge feather." },
            "offset_dx": { "type": "number", "minimum": -1, "maximum": 1, "description": "op=add/op=update: source offset x." },
            "offset_dy": { "type": "number", "minimum": -1, "maximum": 1, "description": "op=add/op=update: source offset y." },
            "opacity": { "type": "number", "minimum": 0, "maximum": 1, "description": "op=add/op=update: heal opacity." },
            "spot_id": { "type": "string", "description": "op=update/op=remove: the spot id from the list view (heuristic entries only)." },
            "threshold": { "type": "number", "minimum": 0, "maximum": 1, "description": "op=set_visualize: visualize threshold; omit to turn visualization off." },
            "distraction": { "type": "string", "description": "op=set_distraction: `k=v,...` with keys reflections|people|dust|auto and values true|false, merged onto the stored switches." },
            "detect_threshold": { "type": "number", "minimum": 0, "maximum": 1, "description": "op=detect/op=detect_apply: detection threshold (default: the recipe visualize threshold, else 0.5)." },
            "detect_max": { "type": "integer", "minimum": 1, "maximum": 4096, "description": "op=detect/op=detect_apply: candidate cap (default 32)." }
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
    let spot_id = optional_str(args, "spot_id")?;
    let distraction = optional_str(args, "distraction")?;
    let threshold = optional_f32(args, "threshold")?;
    let detect_threshold = optional_f32(args, "detect_threshold")?;
    let detect_max = optional_usize(args, "detect_max")?;
    let center_x = optional_f32(args, "center_x")?;
    let center_y = optional_f32(args, "center_y")?;
    let radius = optional_f32(args, "radius")?;
    let feather = optional_f32(args, "feather")?;
    let offset_dx = optional_f32(args, "offset_dx")?;
    let offset_dy = optional_f32(args, "offset_dy")?;
    let opacity = optional_f32(args, "opacity")?;
    let adding = op == "add";
    let updating = op == "update";

    // A write op that names no value must abort loudly instead of degrading
    // into a successful read. `op=add` needs the full geometry, `op=update` needs
    // the target id plus at least one field to change.
    stage_common::require_value(
        NAME,
        &op,
        &[
            ("center_x", adding && center_x.is_none()),
            ("center_y", adding && center_y.is_none()),
            ("radius", adding && radius.is_none()),
            ("spot_id", updating && spot_id.is_none()),
            ("spot_id", op == "remove" && spot_id.is_none()),
            (
                "distraction",
                op == "set_distraction" && distraction.is_none(),
            ),
            (
                "radius|feather|opacity|offset_dx|offset_dy",
                updating
                    && radius.is_none()
                    && feather.is_none()
                    && opacity.is_none()
                    && offset_dx.is_none()
                    && offset_dy.is_none(),
            ),
        ],
    )?;
    // The shared editor resolves the sidecar from the *source* path, exactly
    // like the CLI, so the MCP call and the CLI call act on the same file.
    let source = server.session.require_id(image_id)?.source_path.clone();

    // The conflict matrix (`--clear` vs. the adders, `--spot-id` without a
    // `--set-*`, `--remove-spot` exclusivity, an unknown distraction key, ...) is
    // the shared editor's job. This only names the op and hands over the values;
    // it never decides on its own what is a conflict.
    stage_common::execute(
        server,
        NAME,
        image_id,
        &source,
        |input| SpotRequest {
            input: input.to_owned(),
            virtual_copy: copy,
            list: op == "list",
            add_heuristic: adding,
            center_x: if adding { center_x } else { None },
            center_y: if adding { center_y } else { None },
            radius: if adding { radius } else { None },
            feather: if adding { feather } else { None },
            offset_dx: if adding { offset_dx } else { None },
            offset_dy: if adding { offset_dy } else { None },
            opacity: if adding { opacity } else { None },
            clear: op == "clear",
            spot_id: if updating { spot_id.clone() } else { None },
            set_radius: if updating { radius } else { None },
            set_feather: if updating { feather } else { None },
            set_opacity: if updating { opacity } else { None },
            set_offset_dx: if updating { offset_dx } else { None },
            set_offset_dy: if updating { offset_dy } else { None },
            remove_spot: if op == "remove" { spot_id } else { None },
            set_visualize_threshold: if op == "set_visualize" {
                threshold
            } else {
                None
            },
            // `op=set_visualize` without a threshold is the documented
            // "visualization off" form of the CLI's `--clear-visualize`.
            clear_visualize: op == "set_visualize" && threshold.is_none(),
            set_distraction: if op == "set_distraction" {
                distraction
            } else {
                None
            },
            // `detect` is the read form of the detection pass: the heuristic runs
            // and the candidates are reported, but nothing is persisted because
            // only `detect_apply` sets `detect_apply` in the shared request.
            detect_objects: matches!(op.as_str(), "detect" | "detect_apply"),
            detect_apply: op == "detect_apply",
            detect_threshold,
            detect_max,
            // Slice B (MCP-PARITY-B) owns the generative/regenerate surface; a
            // half-covered variant path would be a stub, not parity.
            regenerate_variant: None,
            variant: None,
            seed: None,
        },
        |request| shared_run(request, Persist::Deferred),
    )
}
