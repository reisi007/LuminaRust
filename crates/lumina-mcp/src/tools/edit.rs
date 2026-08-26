//! `lumina_edit` — set tonal adjustments in the active recipe and write the
//! sidecar atomically (write-through).

use crate::error::McpError;
use crate::session::ImageState;
use crate::util::{get_str, recipe_hash};
use crate::Server;
use lumina_sidecar::{save_sidecar_if_unchanged, SidecarError};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const NAME: &str = "lumina_edit";
pub const DESCRIPTION: &str = "Set global tonal adjustments in the recipe of a virtual copy and \
write the sidecar atomically (write-through). Idempotent: identical input yields an \
identical recipe_hash. Rejects out-of-range values with InvalidAdjustment. The write is a \
compare-and-swap against the revision seen at lumina_load; an externally modified sidecar \
surfaces as SidecarConflict instead of being overwritten.";

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "image_id": { "type": "string", "description": "Session image_id from lumina_load." },
            "virtual_copy": {
                "type": "string",
                "description": "Virtual copy name or id (default: the standard copy)."
            },
            "adjustments": {
                "type": "object",
                "description": "Tonal adjustments. Only provided keys are overwritten.",
                "properties": {
                    "exposure": { "type": "number", "minimum": -10, "maximum": 10 },
                    "contrast": { "type": "number", "minimum": -1, "maximum": 1 },
                    "highlights": { "type": "number", "minimum": -1, "maximum": 1 },
                    "shadows": { "type": "number", "minimum": -1, "maximum": 1 },
                    "whites": { "type": "number", "minimum": -1, "maximum": 1 },
                    "blacks": { "type": "number", "minimum": -1, "maximum": 1 },
                    "wb_temperature": { "type": "number", "minimum": 1500, "maximum": 12000 },
                    "wb_tint": { "type": "number", "minimum": -1, "maximum": 1 },
                    "vibrance": { "type": "number", "minimum": -1, "maximum": 1 },
                    "saturation": { "type": "number", "minimum": -1, "maximum": 1 }
                }
            }
        },
        "required": ["image_id", "adjustments"]
    })
}

/// Validates adjustment keys and value ranges against the pipeline spec (F-036).
pub fn validate_adjustments(map: &BTreeMap<String, f64>) -> Result<(), McpError> {
    for (key, value) in map {
        let (minimum, maximum) = match key.as_str() {
            "exposure" => (-10.0, 10.0),
            "contrast" | "highlights" | "shadows" | "whites" | "blacks" | "wb_tint"
            | "vibrance" | "saturation" => (-1.0, 1.0),
            "wb_temperature" => (1500.0, 12000.0),
            other => {
                return Err(McpError::InvalidAdjustment {
                    name: other.to_string(),
                    value: *value,
                    minimum: f64::MIN,
                    maximum: f64::MAX,
                })
            }
        };
        if !value.is_finite() || !(minimum..=maximum).contains(value) {
            return Err(McpError::InvalidAdjustment {
                name: key.clone(),
                value: *value,
                minimum,
                maximum,
            });
        }
    }
    Ok(())
}

pub fn run(server: &mut Server, args: &Value) -> Result<Value, McpError> {
    let image_id = get_str(args, "image_id")?;
    let state = server.session.require_id_mut(image_id)?;

    let object = args
        .get("adjustments")
        .and_then(|value| value.as_object())
        .ok_or_else(|| McpError::InvalidParams("missing `adjustments` object".into()))?;

    let mut adjustments = BTreeMap::new();
    for (key, value) in object {
        let parsed = value.as_f64().ok_or_else(|| {
            McpError::InvalidParams(format!("adjustment `{key}` must be a number"))
        })?;
        adjustments.insert(key.clone(), parsed);
    }
    validate_adjustments(&adjustments)?;

    let requested = args
        .get("virtual_copy")
        .and_then(|value| value.as_str())
        .map(str::to_owned);
    let index = resolve_copy_index(state, requested.as_deref())?;

    // Mutate a clone first: if the compare-and-swap below fails, the session
    // document stays exactly as loaded and can be re-based by a new load.
    let mut updated = state.document.clone();
    for (key, value) in &adjustments {
        updated.virtual_copies[index]
            .recipe
            .adjustments
            .insert(key.clone(), *value);
    }
    let hash = recipe_hash(&updated.virtual_copies[index].recipe);

    // Compare-and-swap write (REVIEW-MCP-SESSION-1): the save only lands when
    // the on-disk sidecar still matches the revision this session loaded. An
    // external writer between lumina_load and now surfaces as SidecarConflict
    // instead of being silently overwritten (lost update).
    let expected_revision = state.sidecar_revision.clone();
    let revision =
        save_sidecar_if_unchanged(&state.sidecar_path, &updated, Some(&expected_revision))
            .map_err(map_sidecar_error)?;
    state.document = updated;
    state.sidecar_revision = revision;
    Ok(json!({ "ok": true, "recipe_hash": hash }))
}

/// Resolves a virtual copy selection to its index in the sidecar's copy list.
fn resolve_copy_index(state: &ImageState, requested: Option<&str>) -> Result<usize, McpError> {
    let copies = &state.document.virtual_copies;
    match requested {
        Some(name) => copies
            .iter()
            .position(|copy| copy.name == name || copy.id == name)
            .ok_or_else(|| McpError::UnknownCopy(name.to_string())),
        None => Ok(copies.iter().position(|copy| copy.is_default).unwrap_or(0)),
    }
}

fn map_sidecar_error(error: SidecarError) -> McpError {
    match error {
        SidecarError::Conflict(path) => McpError::SidecarConflict(path),
        other => McpError::Sidecar(other.to_string()),
    }
}
