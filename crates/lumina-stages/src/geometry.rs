//! `geometry` — the G-06 geometry stage editor (`lumina geometry` /
//! `lumina_geometry`).
//!
//! Covers the three geometry sub-stages (crop/rotation/mirrors, manual lens
//! correction, manual perspective) plus the read-only Lensfun auto-profile
//! report. Moved verbatim out of `crates/lumina-cli/src/main.rs` (`fn
//! geometry`, `geometry_mut`, `lens_mut`, `perspective_mut`,
//! `parse_aspect_preset`, `parse_crop_free`, `parse_mirror`, `parse_lens_field`,
//! `set_lens_field`, `parse_perspective_field`, `set_perspective_field`,
//! `geometry_list`, `resolve_lensfun_report`).
//!
//! Every mutation appends **exactly one** history entry (G-06 "one visible step
//! per call") and runs the full `document.validate()` gate — aspect names, rect
//! geometry, mirror words, field names and every range — before the single
//! atomic `save_sidecar`. An unknown field name is never a silent no-op: it is a
//! loud rejection before any byte is written.

use crate::copy::{copy_mut, copy_ref, resolve_copy};
use crate::decode::timestamp;
use crate::error::StageError;
use crate::geometry_fields::{
    lens_mut, parse_aspect_preset, parse_crop_free, parse_lens_field, parse_mirror,
    parse_perspective_field, perspective_mut, set_lens_field, set_perspective_field, stage_mut,
};
use crate::report::{Persist, StageReport, StageRun};
use log::info;
use lumina_sidecar::{
    load_sidecar, save_sidecar, sidecar_path_for, Crop, HistoryEntry, SidecarDocument,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// Transport-neutral `geometry` request. Every field maps 1:1 onto one CLI
/// flag; `lensfun_status` is the read-only Lensfun report.
#[derive(Debug, Clone, Default)]
pub struct GeometryRequest {
    /// Source image path; the sidecar lives next to it.
    pub input: String,
    /// Requested virtual-copy id (`None` = default copy).
    pub virtual_copy: Option<String>,
    /// List crop/lens/perspective values (read-only).
    pub list: bool,
    pub set_crop_aspect: Option<String>,
    /// Free crop rectangle as `x,y,w,h` (normalized `0..=1`).
    pub set_crop_free: Option<String>,
    pub clear_crop: bool,
    /// Rotation in degrees (`-180..=180`).
    pub set_rotation: Option<f64>,
    /// Documented alias of `set_rotation` (same field, same validation).
    pub straighten: Option<f64>,
    /// Mirror flags (`h|v|hv|none`).
    pub set_mirror: Option<String>,
    pub clear_geometry: bool,
    /// Manual lens profile (`wide-light|tele-light|standard-neutral`).
    pub set_lens_profile: Option<String>,
    /// `FIELD:VALUE` lens fields (repeatable).
    pub set_lens: Vec<String>,
    pub clear_lens: bool,
    /// `FIELD:VALUE` perspective fields (repeatable).
    pub set_perspective: Vec<String>,
    pub clear_perspective: bool,
    /// Read-only Lensfun auto-profile resolution report for the input.
    pub lensfun_status: bool,
    /// The already-resolved Lensfun report text, supplied by the caller.
    ///
    /// The report needs the CLI's `lensfun`-gated EXIF→profile resolution, so
    /// it is resolved *by the caller* and passed in; this crate stays free of
    /// the optional native capability. `None` serialises as `null`, which is
    /// what the CLI prints when `--lensfun-status` was not requested.
    pub lensfun_report: Option<String>,
}

impl GeometryRequest {
    /// True when the request can change the sidecar.
    pub fn wants_mutation(&self) -> bool {
        self.set_crop_aspect.is_some()
            || self.set_crop_free.is_some()
            || self.clear_crop
            || self.set_rotation.is_some()
            || self.straighten.is_some()
            || self.set_mirror.is_some()
            || self.clear_geometry
            || self.set_lens_profile.is_some()
            || !self.set_lens.is_empty()
            || self.clear_lens
            || !self.set_perspective.is_empty()
            || self.clear_perspective
    }
}

/// Runs one `geometry` call: conflict matrix, load, mutate, validate, one
/// history entry, persist, report. List-only mode is read-only (sidecar bytes
/// unchanged).
pub fn run(request: &GeometryRequest, persist: Persist) -> Result<StageRun, StageError> {
    if request.set_rotation.is_some() && request.straighten.is_some() {
        return Err(StageError::Message(
            "--set-rotation and --straighten are aliases; pass only one".into(),
        ));
    }
    if request.set_crop_aspect.is_some() && request.set_crop_free.is_some() {
        return Err(StageError::Message(
            "--set-crop-aspect and --set-crop-free are mutually exclusive".into(),
        ));
    }
    let wants_mutation = request.wants_mutation();
    if request.lensfun_status && wants_mutation {
        return Err(StageError::Message(
            "--lensfun-status is read-only; pass no mutation flags with it".into(),
        ));
    }
    // `--list` is the read-only view (the default when nothing mutates);
    // combined with a mutation flag it would silently do the wrong thing.
    if request.list && wants_mutation {
        return Err(StageError::Message(
            "--list is read-only; pass no mutation flags with it".into(),
        ));
    }
    // A clear and a set of the SAME stage contradict each other (loud, no
    // half-apply); clears of different stages compose freely.
    if request.clear_crop && (request.set_crop_aspect.is_some() || request.set_crop_free.is_some())
    {
        return Err(StageError::Message(
            "--clear-crop contradicts --set-crop-aspect/--set-crop-free".into(),
        ));
    }
    if request.clear_lens && (request.set_lens_profile.is_some() || !request.set_lens.is_empty()) {
        return Err(StageError::Message(
            "--clear-lens contradicts --set-lens-profile/--set-lens".into(),
        ));
    }
    if request.clear_perspective && !request.set_perspective.is_empty() {
        return Err(StageError::Message(
            "--clear-perspective contradicts --set-perspective".into(),
        ));
    }
    if request.clear_geometry
        && (request.set_crop_aspect.is_some()
            || request.set_crop_free.is_some()
            || request.clear_crop
            || request.set_rotation.is_some()
            || request.straighten.is_some()
            || request.set_mirror.is_some())
    {
        return Err(StageError::Message(
            "--clear-geometry contradicts the crop/rotation/mirror flags".into(),
        ));
    }
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
    // Geometry stage: whole-stage clear or per-field mutations.
    if request.clear_geometry {
        let copy = copy_mut(&mut document, &copy_id)?;
        copy.recipe.geometry = None;
        info!("geometry: cleared stage on copy `{copy_id}`");
        actions.push("clear-geometry".into());
    } else {
        if let Some(preset) = request.set_crop_aspect.as_deref() {
            stage_mut(&mut document, &copy_id)?.crop = Some(Crop::Aspect {
                preset: parse_aspect_preset(preset)?,
            });
            info!("geometry: crop aspect {preset} on copy `{copy_id}`");
            actions.push(format!("crop-aspect:{preset}"));
        }
        if let Some(rect) = request.set_crop_free.as_deref() {
            let (x, y, width, height) = parse_crop_free(rect)?;
            stage_mut(&mut document, &copy_id)?.crop = Some(Crop::Free {
                x,
                y,
                width,
                height,
            });
            info!("geometry: crop free {rect} on copy `{copy_id}`");
            actions.push(format!("crop-free:{rect}"));
        }
        if request.clear_crop {
            stage_mut(&mut document, &copy_id)?.crop = None;
            info!("geometry: crop cleared on copy `{copy_id}`");
            actions.push("crop:clear".into());
        }
        // `--straighten` is a documented alias of `--set-rotation`: same
        // field (`geometry.rotation_degrees`), same validation, one step.
        if let Some(degrees) = request.set_rotation.or(request.straighten) {
            if !degrees.is_finite() {
                return Err(StageError::Message(format!(
                    "invalid rotation `{degrees}`: expected a finite number in -180..=180"
                )));
            }
            stage_mut(&mut document, &copy_id)?.rotation_degrees = degrees as f32;
            info!("geometry: rotation {degrees} on copy `{copy_id}`");
            actions.push(format!("rotation:{degrees}"));
        }
        if let Some(mirror) = request.set_mirror.as_deref() {
            let (horizontal, vertical) = parse_mirror(mirror)?;
            let geo = stage_mut(&mut document, &copy_id)?;
            geo.mirror_horizontal = horizontal;
            geo.mirror_vertical = vertical;
            info!("geometry: mirror {mirror} on copy `{copy_id}`");
            actions.push(format!("mirror:{mirror}"));
        }
    }
    // Manual lens stage: whole-stage clear or per-field mutations.
    if request.clear_lens {
        let copy = copy_mut(&mut document, &copy_id)?;
        copy.recipe.lens_correction = None;
        info!("geometry: lens correction cleared on copy `{copy_id}`");
        actions.push("lens:clear".into());
    } else {
        if let Some(profile) = request.set_lens_profile.as_deref() {
            lens_mut(&mut document, &copy_id)?.profile = Some(profile.into());
            info!("geometry: lens profile {profile} on copy `{copy_id}`");
            actions.push(format!("lens-profile:{profile}"));
        }
        for spec in &request.set_lens {
            let (field, value) = parse_lens_field(spec)?;
            set_lens_field(lens_mut(&mut document, &copy_id)?, &field, value);
            info!("geometry: lens {field}={value} on copy `{copy_id}`");
            actions.push(format!("lens:{field}={value}"));
        }
    }
    // Manual perspective stage: whole-stage clear or per-field mutations.
    if request.clear_perspective {
        let copy = copy_mut(&mut document, &copy_id)?;
        copy.recipe.perspective = None;
        info!("geometry: perspective cleared on copy `{copy_id}`");
        actions.push("perspective:clear".into());
    } else {
        for spec in &request.set_perspective {
            let (field, value) = parse_perspective_field(spec)?;
            set_perspective_field(perspective_mut(&mut document, &copy_id)?, &field, value);
            info!("geometry: perspective {field}={value} on copy `{copy_id}`");
            actions.push(format!("perspective:{field}={value}"));
        }
    }
    if wants_mutation {
        // Loud gate: aspect names, rect geometry, mirror words, field names
        // and every range are rejected before anything is written. Exactly
        // one history entry per call keeps every step visible (G-06).
        document.validate()?;
        let copy = copy_mut(&mut document, &copy_id)?;
        let final_recipe = copy.recipe.clone();
        let mut id = format!("geometry-{}", timestamp());
        let mut suffix = 0u32;
        while copy.history.iter().any(|entry| entry.id == id) {
            suffix += 1;
            id = format!("geometry-{}-{suffix}", timestamp());
        }
        let mut extras = BTreeMap::new();
        extras.insert("step".into(), Value::String("geometry".into()));
        extras.insert("actions".into(), Value::String(actions.join(",")));
        copy.history.push(HistoryEntry {
            id,
            recipe: final_recipe,
            recorded_at: Some(timestamp()),
            extras,
        });
        if persist == Persist::Immediately {
            save_sidecar(&path, &document)?;
        }
    }
    let report = list(request, &document, &copy_id, &actions, wants_mutation)?;
    let document = (wants_mutation && persist == Persist::Deferred).then_some(document);
    Ok(StageRun {
        report,
        sidecar_path: path,
        document,
    })
}

/// Resolves the Lensfun auto-profile status for one input file (G-06): which
/// corrector a render would build from the input's EXIF, or the loud reason none
/// applies. Read-only — never a correction.
pub fn lensfun_report_of(request: &GeometryRequest) -> Option<String> {
    request
        .lensfun_status
        .then(|| request.lensfun_report.clone())
        .flatten()
}

/// Reports the copy's geometry / lens / perspective values. Read-only: the
/// sidecar is never written here.
fn list(
    request: &GeometryRequest,
    document: &SidecarDocument,
    copy_id: &str,
    actions: &[String],
    wrote: bool,
) -> Result<StageReport, StageError> {
    let copy = copy_ref(document, copy_id)?;
    let recipe = &copy.recipe;
    // `--lensfun-status` resolves the EXIF→profile match for the input
    // (read-only): which corrector a render would use, or the loud reason
    // none applies. Never a guessed correction.
    let lensfun_report = lensfun_report_of(request);
    let payload = json!({
        "command": "geometry",
        "input": request.input,
        "copy": copy_id,
        "geometry": recipe.geometry,
        "lens_correction": recipe.lens_correction,
        "perspective": recipe.perspective,
        "lensfun": lensfun_report,
        "actions": actions,
    });
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("copy: {} [{}]", copy.name, copy.id));
    match &recipe.geometry {
        Some(geo) => {
            match &geo.crop {
                Some(Crop::Aspect { preset }) => lines.push(format!("  crop: aspect {preset:?}")),
                Some(Crop::Free {
                    x,
                    y,
                    width,
                    height,
                }) => lines.push(format!("  crop: free x={x} y={y} w={width} h={height}")),
                None => lines.push("  crop: none (full frame)".into()),
            }
            lines.push(format!(
                "  rotation: {} mirror_h={} mirror_v={}",
                geo.rotation_degrees, geo.mirror_horizontal, geo.mirror_vertical
            ));
        }
        None => lines.push("  geometry: none".into()),
    }
    match &recipe.lens_correction {
        Some(lens) => lines.push(format!(
            "  lens: profile={:?} k1={:?} k2={:?} k3={:?} c0={:?} c1={:?} c2={:?} ca_r={:?} ca_b={:?}",
            lens.profile,
            lens.distortion_k1,
            lens.distortion_k2,
            lens.distortion_k3,
            lens.vignette_c0,
            lens.vignette_c1,
            lens.vignette_c2,
            lens.ca_red,
            lens.ca_blue
        )),
        None => lines.push("  lens: none".into()),
    }
    match &recipe.perspective {
        Some(p) => lines.push(format!(
            "  perspective: v={} h={} rot={} scale={} aspect={} sx={} sy={}",
            p.vertical, p.horizontal, p.rotation, p.scale, p.aspect_ratio, p.shift_x, p.shift_y
        )),
        None => lines.push("  perspective: none".into()),
    }
    if let Some(report) = &lensfun_report {
        lines.push(format!("  lensfun: {report}"));
    }
    let summary = if actions.is_empty() {
        "geometry status listed".to_string()
    } else {
        format!("geometry updated: {}", actions.join(", "))
    };
    Ok(StageReport::new(
        "geometry",
        copy_id,
        actions.to_vec(),
        payload,
        lines,
        summary,
        wrote,
    ))
}
