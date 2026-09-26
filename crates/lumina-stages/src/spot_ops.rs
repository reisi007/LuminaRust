//! R5-DUST-23-FOLLOWUP spot mutation ops, shared by the CLI and the MCP server.
//!
//! Moved verbatim out of `crates/lumina-cli/src/spot_ops.rs` (which this slice
//! deletes) so both transports mutate spot entries through ONE implementation.
//! Every operation uses the shared sidecar entry view, which includes typed-only
//! generative operations and assigns compatibility IDs while loading. Targets
//! are required to resolve to exactly one entry; a duplicate ID is a loud
//! contract error rather than a multi-row update. Persistence remains with
//! [`crate::spot::run`], which validates and saves only after all operations
//! finish.

use crate::copy::{copy_mut, copy_ref};
use crate::error::StageError;
use lumina_sidecar::{
    set_spot_removal_entries, sidecar_path_for, spot_removal_entries, spot_removal_status,
    SidecarDocument, SpotDistraction, SPOT_REMOVAL_VERSION,
};
use serde_json::{json, Value};
use std::path::Path;

/// Adds the effective reference-derived status to a list view without changing
/// the persisted recipe. Used by both the human and the JSON output path.
pub fn display_spot_entries(spots: &[Value], bundle_root: &Path) -> Vec<Value> {
    spots
        .iter()
        .map(|entry| {
            let mut shown = entry.clone();
            let status = spot_removal_status(entry, bundle_root);
            if let Some(object) = shown.as_object_mut() {
                object.insert("status".into(), Value::String(status.as_str().to_owned()));
            }
            shown
        })
        .collect()
}

/// [`display_spot_entries`] with the bundle root derived from the source path.
pub fn display_spot_entries_for_input(spots: &[Value], input: &Path) -> Vec<Value> {
    let sidecar = sidecar_path_for(input);
    display_spot_entries(spots, sidecar.parent().unwrap_or_else(|| Path::new(".")))
}

fn entries_for_copy(document: &SidecarDocument, copy_id: &str) -> Result<Vec<Value>, StageError> {
    let copy = copy_ref(document, copy_id)?;
    Ok(spot_removal_entries(&copy.recipe))
}

fn unique_index(spots: &[Value], spot_id: &str) -> Result<usize, StageError> {
    let matches: Vec<usize> = spots
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            (entry.get("id").and_then(|value| value.as_str()) == Some(spot_id)).then_some(index)
        })
        .collect();
    match matches.as_slice() {
        [] => Err(StageError::Message(format!("unknown spot `{spot_id}`"))),
        [index] => Ok(*index),
        _ => Err(StageError::Message(format!(
            "spot `{spot_id}` is ambiguous: duplicate ids are not valid targets"
        ))),
    }
}

fn store_entries(
    document: &mut SidecarDocument,
    copy_id: &str,
    spots: &[Value],
) -> Result<(), StageError> {
    let copy = copy_mut(document, copy_id)?;
    set_spot_removal_entries(&mut copy.recipe, spots)
        .map_err(|error| StageError::Message(format!("spot recipe rejected: {error}")))
}

/// Appends one heuristic spot entry. The geometry-derived ID is deterministic;
/// adding the same geometry twice is rejected instead of creating a duplicate
/// target.
#[allow(clippy::too_many_arguments)]
pub fn add_heuristic(
    document: &mut SidecarDocument,
    copy_id: &str,
    center_x: f32,
    center_y: f32,
    radius: f32,
    feather: f32,
    offset_dx: f32,
    offset_dy: f32,
    opacity: f32,
) -> Result<(), StageError> {
    for (name, value, lo, hi) in [
        ("center_x", center_x, 0.0, 1.0),
        ("center_y", center_y, 0.0, 1.0),
        ("radius", radius, f32::MIN_POSITIVE, 512.0),
        ("feather", feather, 0.0, 1.0),
        ("offset_dx", offset_dx, -1.0, 1.0),
        ("offset_dy", offset_dy, -1.0, 1.0),
        ("opacity", opacity, 0.0, 1.0),
    ] {
        if !value.is_finite() || value < lo || value > hi {
            return Err(StageError::Message(format!(
                "invalid heuristic spot `{name}`: value {value} outside allowed range {lo}..={hi}"
            )));
        }
    }
    if radius <= 0.0 {
        return Err(StageError::Message(
            "invalid heuristic spot `radius`: must be > 0".into(),
        ));
    }
    let id = format!(
        "spot-{}",
        blake3::hash(format!("{center_x:.6},{center_y:.6},{radius:.2}").as_bytes()).to_hex()
    );
    let entry = json!({
        "id": id,
        "version": SPOT_REMOVAL_VERSION,
        "mode": "heuristic",
        "center_x": center_x,
        "center_y": center_y,
        "radius": radius,
        "feather": feather,
        "offset_dx": offset_dx,
        "offset_dy": offset_dy,
        "opacity": opacity,
        "status": "valid",
    });
    let mut spots = entries_for_copy(document, copy_id)?;
    if spots
        .iter()
        .any(|existing| existing.get("id").and_then(|value| value.as_str()) == Some(id.as_str()))
    {
        return Err(StageError::Message(format!(
            "spot `{id}` already exists; IDs must be unique"
        )));
    }
    spots.push(entry);
    store_entries(document, copy_id, &spots)
}

/// Updates one heuristic spot's heal parameters in place. Generative entries
/// carry no Heal geometry, so updating one fails loudly instead of silently
/// rewriting it into a Clone.
#[allow(clippy::too_many_arguments)]
pub fn update_params(
    document: &mut SidecarDocument,
    copy_id: &str,
    spot_id: &str,
    radius: Option<f32>,
    feather: Option<f32>,
    opacity: Option<f32>,
    offset_dx: Option<f32>,
    offset_dy: Option<f32>,
) -> Result<(), StageError> {
    for (name, value, lo, hi) in [
        ("radius", radius, f32::MIN_POSITIVE, 512.0),
        ("feather", feather, 0.0, 1.0),
        ("opacity", opacity, 0.0, 1.0),
        ("offset_dx", offset_dx, -1.0, 1.0),
        ("offset_dy", offset_dy, -1.0, 1.0),
    ] {
        if let Some(value) = value {
            if !value.is_finite() || value < lo || value > hi {
                return Err(StageError::Message(format!(
                    "invalid heuristic spot `{name}`: value {value} outside allowed range {lo}..={hi}"
                )));
            }
        }
    }
    if let Some(radius) = radius {
        if radius <= 0.0 {
            return Err(StageError::Message(
                "invalid heuristic spot `radius`: must be > 0".into(),
            ));
        }
    }
    let mut spots = entries_for_copy(document, copy_id)?;
    let index = unique_index(&spots, spot_id)
        .map_err(|error| StageError::Message(format!("{error} on copy `{copy_id}`")))?;
    let mode = spots[index]
        .get("mode")
        .and_then(|value| value.as_str())
        .unwrap_or("heuristic");
    if mode != "heuristic" {
        return Err(StageError::Message(format!(
            "spot `{spot_id}` is {mode}, not heuristic: geometry updates apply to Heal spots only (use --regenerate-variant for Generate spots)"
        )));
    }
    if let Some(value) = radius {
        spots[index]["radius"] = json!(value);
    }
    if let Some(value) = feather {
        spots[index]["feather"] = json!(value);
    }
    if let Some(value) = opacity {
        spots[index]["opacity"] = json!(value);
    }
    if let Some(value) = offset_dx {
        spots[index]["offset_dx"] = json!(value);
    }
    if let Some(value) = offset_dy {
        spots[index]["offset_dy"] = json!(value);
    }
    store_entries(document, copy_id, &spots)
}

/// Removes exactly one spot entry by id. Unknown or duplicate ids fail loudly
/// and leave the in-memory recipe untouched.
pub fn remove_entry(
    document: &mut SidecarDocument,
    copy_id: &str,
    spot_id: &str,
) -> Result<(), StageError> {
    let mut spots = entries_for_copy(document, copy_id)?;
    let index = unique_index(&spots, spot_id)
        .map_err(|error| StageError::Message(format!("{error} on copy `{copy_id}`")))?;
    spots.remove(index);
    store_entries(document, copy_id, &spots)
}

/// Parses `k=v,...` distraction switches as deltas merged into `setting`.
pub fn parse_distraction_spec(
    spec: &str,
    mut setting: SpotDistraction,
) -> Result<SpotDistraction, StageError> {
    if spec.trim().is_empty() {
        return Err(StageError::Message(
            "invalid distraction spec: expected `k=v,...` with keys reflections|people|dust|auto"
                .into(),
        ));
    }
    for part in spec.split(',') {
        let (key, value) = part.split_once('=').ok_or_else(|| {
            StageError::Message(format!(
                "invalid distraction assignment `{part}`: expected `k=v`"
            ))
        })?;
        let enabled = match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => true,
            "false" | "0" | "no" => false,
            _ => {
                return Err(StageError::Message(format!(
                    "invalid distraction value `{value}`: expected true|false"
                )));
            }
        };
        match key.trim().to_ascii_lowercase().as_str() {
            "reflections" => setting.reflections = enabled,
            "people" => setting.people = enabled,
            "dust" => setting.dust = enabled,
            "auto" | "auto_mode" => setting.auto_mode = enabled,
            _ => {
                return Err(StageError::Message(format!(
                    "unknown distraction key `{key}`: expected reflections|people|dust|auto"
                )));
            }
        }
    }
    Ok(setting)
}

/// Sets `seed = derived` plus the explicit variant provenance on one generative
/// entry. A heuristic target or duplicate ID fails loudly.
pub fn regenerate_variant(
    document: &mut SidecarDocument,
    copy_id: &str,
    spot_id: &str,
    base: u64,
    variant: u64,
    derived: u64,
) -> Result<(), StageError> {
    let mut spots = entries_for_copy(document, copy_id)?;
    let index = unique_index(&spots, spot_id)
        .map_err(|error| StageError::Message(format!("{error} on copy `{copy_id}`")))?;
    let mode = spots[index]
        .get("mode")
        .and_then(|value| value.as_str())
        .unwrap_or("heuristic");
    if mode != "generative" {
        return Err(StageError::Message(format!(
            "spot `{spot_id}` is not generative (mode `{mode}`); variants apply to generative spots only"
        )));
    }
    spots[index]["seed"] = json!(derived);
    spots[index]["variant"] = json!(variant);
    spots[index]["base_seed"] = json!(base);
    store_entries(document, copy_id, &spots)
}
