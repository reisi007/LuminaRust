//! `spot` — the G-04 spot-heal stage editor (`lumina spot` / `lumina_spot`).
//!
//! Moved verbatim out of `crates/lumina-cli/src/main.rs` (`fn spot`,
//! `fn spot_list`) and `crates/lumina-cli/src/spot_ops.rs`; the conflict
//! matrix, the entry mutations, the detection defaults, the visualize /
//! distraction extras, the list payload, the human lines and the `info!` log
//! lines are unchanged. Read-only calls never touch the sidecar; writes run the
//! full `document.validate()` gate and then one atomic `save_sidecar`.

use crate::copy::{copy_mut, copy_ref, resolve_copy};
use crate::decode::read_and_decode;
use crate::error::StageError;
use crate::report::{Persist, StageReport, StageRun};
use crate::spot_ops::{
    add_heuristic, display_spot_entries_for_input, parse_distraction_spec, regenerate_variant,
    remove_entry, update_params,
};
use log::info;
use lumina_core::{detect_spots_heuristic, generative_variant_seed, DetectedSpot};
use lumina_sidecar::{
    load_sidecar, save_sidecar, sidecar_path_for, spot_removal_entries, SidecarDocument,
};
use serde_json::json;
use std::path::Path;

/// Transport-neutral `spot` request. The CLI fills it from `SpotArgs`, the MCP
/// tool from its validated JSON schema; every field maps 1:1 onto one CLI flag.
#[derive(Debug, Clone, Default)]
pub struct SpotRequest {
    /// Source image path; the sidecar lives next to it.
    pub input: String,
    /// Requested virtual-copy id (`None` = default copy).
    pub virtual_copy: Option<String>,
    /// List spots + G-04 settings (read-only).
    pub list: bool,
    /// Add one heuristic spot (needs `center_x`, `center_y`, `radius`).
    pub add_heuristic: bool,
    pub center_x: Option<f32>,
    pub center_y: Option<f32>,
    pub radius: Option<f32>,
    pub feather: Option<f32>,
    pub offset_dx: Option<f32>,
    pub offset_dy: Option<f32>,
    pub opacity: Option<f32>,
    /// Remove all spot entries of the copy.
    pub clear: bool,
    /// Select one spot by id for the `set_*` updates (heuristic entries only).
    pub spot_id: Option<String>,
    pub set_radius: Option<f32>,
    pub set_feather: Option<f32>,
    pub set_opacity: Option<f32>,
    pub set_offset_dx: Option<f32>,
    pub set_offset_dy: Option<f32>,
    /// Remove one spot entry by id (loud on unknown ids).
    pub remove_spot: Option<String>,
    pub set_visualize_threshold: Option<f32>,
    pub clear_visualize: bool,
    /// `k=v,...` with keys `reflections|people|dust|auto`, values `true|false`.
    pub set_distraction: Option<String>,
    /// List heuristic spot candidates (stage 1, no model).
    pub detect_objects: bool,
    /// Persist the detected candidates as heuristic spots (explicit only).
    pub detect_apply: bool,
    pub detect_threshold: Option<f32>,
    pub detect_max: Option<usize>,
    /// Regenerate the generative spot `<id>`: `seed = variant_seed(base, variant)`.
    pub regenerate_variant: Option<String>,
    pub variant: Option<u64>,
    pub seed: Option<u64>,
}

impl SpotRequest {
    /// True when the request can change the sidecar.
    pub fn wants_mutation(&self) -> bool {
        self.add_heuristic
            || self.clear
            || self.set_visualize_threshold.is_some()
            || self.clear_visualize
            || self.set_distraction.is_some()
            || self.detect_apply
            || self.regenerate_variant.is_some()
            || self.spot_id.is_some()
            || self.remove_spot.is_some()
    }
}

/// Runs one `spot` call: conflict matrix, load, mutate, validate, persist,
/// report. The CLI passes [`Persist::Immediately`], the MCP server
/// [`Persist::Deferred`] so it can write under a compare-and-swap.
pub fn run(request: &SpotRequest, persist: Persist) -> Result<StageRun, StageError> {
    reject_spot_remove_conflicts(request)?;
    if request.regenerate_variant.is_some() && (request.variant.is_none() || request.seed.is_none())
    {
        return Err(StageError::Message(
            "--regenerate-variant requires --variant <N> and --seed <N>".into(),
        ));
    }
    if request.detect_apply && !request.detect_objects {
        return Err(StageError::Message(
            "--detect-apply requires --detect-objects".into(),
        ));
    }
    if request.set_visualize_threshold.is_some() && request.clear_visualize {
        return Err(StageError::Message(
            "--set-visualize-threshold and --clear-visualize are mutually exclusive".into(),
        ));
    }
    // `--clear` removes every spot after the adders ran, so combining it with
    // an adder is a contradiction (the requested edit would be discarded).
    // R5-DUST-23-FOLLOWUP: the single-spot editor (`--spot-id` + `--set-*`,
    // `--remove-spot`) contradicts `--clear` the same way.
    if request.clear
        && (request.add_heuristic
            || request.detect_apply
            || request.regenerate_variant.is_some()
            || request.remove_spot.is_some()
            || request.spot_id.is_some())
    {
        return Err(StageError::Message(
            "--clear removes every spot and contradicts --add-heuristic/--detect-apply/--regenerate-variant/--spot-id/--remove-spot"
                .into(),
        ));
    }
    // R5-DUST-23-FOLLOWUP: `--set-*` needs `--spot-id` (and vice versa) —
    // either alone would be a silent no-op.
    let wants_update = request.set_radius.is_some()
        || request.set_feather.is_some()
        || request.set_opacity.is_some()
        || request.set_offset_dx.is_some()
        || request.set_offset_dy.is_some();
    if wants_update && request.spot_id.is_none() {
        return Err(StageError::Message(
            "--set-radius/--set-feather/--set-opacity/--set-offset-dx/--set-offset-dy require --spot-id <ID>".into(),
        ));
    }
    if request.spot_id.is_some() && !wants_update {
        return Err(StageError::Message(
            "--spot-id requires at least one --set-radius/--set-feather/--set-opacity/--set-offset-dx/--set-offset-dy".into(),
        ));
    }
    let wants_mutation = request.wants_mutation();
    // Detection needs the decoded frame even in list-only mode.
    let needs_frame = request.detect_objects || request.add_heuristic || request.detect_apply;
    let frame = if needs_frame {
        let (_bytes, frame, _raw) = read_and_decode(Path::new(&request.input))?;
        Some(frame)
    } else {
        None
    };
    let path = sidecar_path_for(Path::new(&request.input));
    let mut document = match load_sidecar(&path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            return Err(StageError::Message(format!(
                "no sidecar for `{}`; run `import` first",
                request.input
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let copy_id = resolve_copy(&document, request.virtual_copy.as_deref())?;
    let mut actions: Vec<String> = Vec::new();
    // Detection results are computed before mutation so `--detect-apply`
    // persists exactly what was listed.
    let mut detected: Vec<DetectedSpot> = Vec::new();
    if request.detect_objects {
        let frame = frame.as_ref().expect("decoded for detection");
        // G04-FOLLOWUP-1: without an explicit flag the recipe visualize
        // threshold is the default (else 0.5); an out-of-range recipe value
        // fails loudly in the detector below, never silently.
        let recipe_threshold = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == copy_id)
            .and_then(|copy| copy.recipe.spot_visualize_threshold());
        let threshold = request.detect_threshold.or(recipe_threshold).unwrap_or(0.5);
        let max = request.detect_max.unwrap_or(32);
        detected = detect_spots_heuristic(frame, threshold, max)
            .map_err(|error| StageError::Message(format!("spot detection rejected: {error}")))?;
        info!(
            "spot: detected {} candidate(s) on copy `{copy_id}` (threshold {threshold}, max {max})",
            detected.len()
        );
    }
    if request.add_heuristic {
        let (cx, cy, radius) = match (request.center_x, request.center_y, request.radius) {
            (Some(x), Some(y), Some(r)) => (x, y, r),
            _ => {
                return Err(StageError::Message(
                    "--add-heuristic requires --center-x, --center-y and --radius".into(),
                ));
            }
        };
        add_heuristic(
            &mut document,
            &copy_id,
            cx,
            cy,
            radius,
            request.feather.unwrap_or(0.0),
            request.offset_dx.unwrap_or(0.0),
            request.offset_dy.unwrap_or(0.0),
            request.opacity.unwrap_or(1.0),
        )?;
        info!("spot: added heuristic spot on copy `{copy_id}`");
        actions.push("add-heuristic".into());
    }
    if request.detect_apply {
        let mut added = 0usize;
        for candidate in &detected {
            add_heuristic(
                &mut document,
                &copy_id,
                candidate.x,
                candidate.y,
                candidate.radius.max(1.0),
                0.0,
                0.05,
                0.0,
                1.0,
            )?;
            added += 1;
        }
        info!("spot: applied {added} detected candidate(s) on copy `{copy_id}`");
        actions.push(format!("detect-apply:{added}"));
    }
    if let Some(threshold) = request.set_visualize_threshold {
        copy_mut(&mut document, &copy_id)?
            .recipe
            .set_spot_visualize_threshold(Some(threshold))
            .map_err(|error| StageError::Message(error.to_string()))?;
        info!("spot: visualize threshold {threshold} on copy `{copy_id}`");
        actions.push(format!("visualize:{threshold}"));
    }
    if request.clear_visualize {
        copy_mut(&mut document, &copy_id)?
            .recipe
            .set_spot_visualize_threshold(None)
            .map_err(|error| StageError::Message(error.to_string()))?;
        info!("spot: visualize cleared on copy `{copy_id}`");
        actions.push("visualize:off".into());
    }
    if let Some(spec) = request.set_distraction.as_deref() {
        // G04-FOLLOWUP-1 merge decision: deltas apply on top of the stored
        // switches (consistent with the GUI single-checkbox toggles), so an
        // unnamed key is never silently reset.
        let current = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == copy_id)
            .map(|copy| copy.recipe.spot_distraction())
            .unwrap_or_default();
        let setting = parse_distraction_spec(spec, current)?;
        copy_mut(&mut document, &copy_id)?
            .recipe
            .set_spot_distraction(setting);
        info!("spot: distraction {setting:?} on copy `{copy_id}`");
        actions.push("distraction".into());
    }
    if let Some(spot_id) = request.regenerate_variant.as_deref() {
        let base = request.seed.expect("guarded above");
        let variant = request.variant.expect("guarded above");
        let derived = generative_variant_seed(base, variant);
        regenerate_variant(&mut document, &copy_id, spot_id, base, variant, derived)?;
        info!("spot: regenerated variant {variant} (seed {derived}) for `{spot_id}` on copy `{copy_id}`");
        actions.push(format!("regenerate-variant:{spot_id}:{variant}"));
    }
    // R5-DUST-23-FOLLOWUP: per-spot param edits (heuristic only, loud
    // otherwise) and single-spot removal (loud on unknown ids).
    if let Some(spot_id) = request.spot_id.as_deref() {
        update_params(
            &mut document,
            &copy_id,
            spot_id,
            request.set_radius,
            request.set_feather,
            request.set_opacity,
            request.set_offset_dx,
            request.set_offset_dy,
        )?;
        info!("spot: updated `{spot_id}` on copy `{copy_id}`");
        actions.push(format!("update-spot:{spot_id}"));
    }
    if let Some(spot_id) = request.remove_spot.as_deref() {
        remove_entry(&mut document, &copy_id, spot_id)?;
        info!("spot: removed `{spot_id}` on copy `{copy_id}`");
        actions.push(format!("remove-spot:{spot_id}"));
    }
    if request.clear {
        let copy = copy_mut(&mut document, &copy_id)?;
        copy.recipe.extras.remove("spot_removals");
        copy.recipe.spot_removals.clear();
        info!("spot: cleared all spots on copy `{copy_id}`");
        actions.push("clear".into());
    }
    if wants_mutation {
        // Loud gate: geometry, visualize/distraction extras and variant
        // controls are rejected before anything is written.
        document.validate()?;
        if persist == Persist::Immediately {
            save_sidecar(&path, &document)?;
        }
    }
    let report = list(
        request,
        &document,
        &copy_id,
        &detected,
        &actions,
        wants_mutation,
    )?;
    let document = (wants_mutation && persist == Persist::Deferred).then_some(document);
    Ok(StageRun {
        report,
        sidecar_path: path,
        document,
    })
}

/// R5-DUST-23-FOLLOWUP: rejects contradictory spot-list mutations before any
/// input is decoded or a sidecar is loaded. Keeping this matrix beside the
/// writers makes the no-partial-mutation rule explicit and testable.
pub fn reject_spot_remove_conflicts(request: &SpotRequest) -> Result<(), StageError> {
    if request.remove_spot.is_none() {
        return Ok(());
    }
    let conflicts = [
        ("--add-heuristic", request.add_heuristic),
        ("--detect-apply", request.detect_apply),
        (
            "--spot-id/--set-*",
            request.spot_id.is_some()
                || request.set_radius.is_some()
                || request.set_feather.is_some()
                || request.set_opacity.is_some()
                || request.set_offset_dx.is_some()
                || request.set_offset_dy.is_some(),
        ),
        ("--regenerate-variant", request.regenerate_variant.is_some()),
        ("--clear", request.clear),
    ];
    let conflicts = conflicts
        .into_iter()
        .filter_map(|(flag, present)| present.then_some(flag))
        .collect::<Vec<_>>();
    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(StageError::Message(format!(
            "--remove-spot cannot be combined with {}; each spot-list mutation is exclusive",
            conflicts.join(", ")
        )))
    }
}

/// Reports the copy's spots, G-04 settings and (when requested) detection
/// candidates. Read-only: the sidecar is never written here.
fn list(
    request: &SpotRequest,
    document: &SidecarDocument,
    copy_id: &str,
    detected: &[DetectedSpot],
    actions: &[String],
    wrote: bool,
) -> Result<StageReport, StageError> {
    let copy = copy_ref(document, copy_id)?;
    let spots = spot_removal_entries(&copy.recipe);
    // Preserve every entry (including null/missing references) and annotate
    // status from the same decision layer used by the GUI.
    let display_spots = display_spot_entries_for_input(&spots, Path::new(&request.input));
    let distraction = copy.recipe.spot_distraction();
    // reflections/people without a model are visibly NeedsModel (F-078 gate,
    // heuristic stage 1 covers dust only) — surfaced in both formats.
    let mut needs_model: Vec<&str> = Vec::new();
    if distraction.reflections {
        needs_model.push("reflections");
    }
    if distraction.people {
        needs_model.push("people");
    }
    let payload = json!({
        "command": "spot",
        "input": request.input,
        "copy": copy_id,
        "spots": display_spots,
        "visualize_threshold": copy.recipe.spot_visualize_threshold(),
        "distraction": distraction,
        "distraction_needs_model": needs_model,
        "detected": detected.iter().map(|d| json!({
            "x": d.x, "y": d.y, "radius": d.radius, "confidence": d.confidence,
        })).collect::<Vec<_>>(),
        "actions": actions,
        "status": "ok",
    });
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("copy: {} [{}]", copy.name, copy.id));
    lines.push(format!("  spots: {}", spots.len()));
    for entry in &display_spots {
        let id = entry.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let mode = entry
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("heuristic");
        let status = entry
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("valid");
        lines.push(format!("    spot {id}: mode={mode} status={status}"));
    }
    match copy.recipe.spot_visualize_threshold() {
        Some(t) => lines.push(format!("  visualize: threshold={t}")),
        None => lines.push("  visualize: off".into()),
    }
    lines.push(format!(
        "  distraction: reflections={} people={} dust={} auto={}",
        distraction.reflections, distraction.people, distraction.dust, distraction.auto_mode
    ));
    if !needs_model.is_empty() {
        lines.push(format!(
            "  distraction needs model (F-078 gate, heuristic covers dust only): {}",
            needs_model.join(", ")
        ));
    }
    if request.detect_objects {
        lines.push(format!("  detected candidates: {}", detected.len()));
        for candidate in detected {
            lines.push(format!(
                "    candidate x={:.4} y={:.4} r={:.1} conf={:.2}",
                candidate.x, candidate.y, candidate.radius, candidate.confidence
            ));
        }
    }
    let summary = if actions.is_empty() {
        "spot status listed".to_string()
    } else {
        format!("spot updated: {}", actions.join(", "))
    };
    Ok(StageReport::new(
        "spot",
        copy_id,
        actions.to_vec(),
        payload,
        lines,
        summary,
        wrote,
    ))
}
