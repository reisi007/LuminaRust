//! `lens-blur` — the G-05 depth-bokeh stage editor (`lumina lens-blur` /
//! `lumina_lens_blur`).
//!
//! Moved verbatim out of `crates/lumina-cli/src/main.rs` (`fn lens_blur`,
//! `lens_blur_mut`, `parse_bokeh_shape`, `parse_focus_rect`,
//! `parse_depth_artifact`, `lens_blur_list`). The loud gate is unchanged: an
//! out-of-range amount, a malformed focus rect, a non-portable depth-artifact
//! path and an **inverted focal range** are all rejected by
//! `document.validate()` before the single atomic `save_sidecar`, so an
//! aborted call leaves zero bytes behind. A referenced-but-unresolvable depth
//! artifact is reported as `missing depth artifact` by the shared
//! `lumina_core::lens_blur_status` decision layer and aborts renders loudly
//! instead of silently falling back to the heuristic.

use crate::copy::{copy_mut, copy_ref, resolve_copy};
use crate::error::StageError;
use crate::report::{Persist, StageReport, StageRun};
use log::info;
use lumina_core::lens_blur_status;
use lumina_sidecar::{
    load_sidecar, save_sidecar, sidecar_path_for, BokehShape, DepthArtifactRef, FocusRect,
    LensBlur, SidecarDocument,
};
use serde_json::{json, Value};
use std::path::Path;

/// Transport-neutral `lens-blur` request. Every field maps 1:1 onto one CLI
/// flag; the MCP tool fills the same struct from its validated JSON schema.
#[derive(Debug, Clone, Default)]
pub struct LensBlurRequest {
    /// Source image path; the sidecar lives next to it.
    pub input: String,
    /// Requested virtual-copy id (`None` = default copy).
    pub virtual_copy: Option<String>,
    /// List values + depth status (read-only).
    pub list: bool,
    /// Enable the stage (keeps stored values).
    pub enable: bool,
    /// Disable the stage (keeps stored values, renders identity).
    pub disable: bool,
    /// Set the blur strength (`0..=1`, 0 is identity).
    pub set_amount: Option<f32>,
    /// Set the near edge of the sharp depth band (`0..=1`).
    pub set_focal_near: Option<f32>,
    /// Set the far edge of the sharp depth band (`0..=1`, `>= near`).
    pub set_focal_far: Option<f32>,
    /// Set the bokeh shape (`round|elliptical|hexagonal`).
    pub set_bokeh: Option<String>,
    /// Set the focus rectangle as `x,y,w,h` (normalized `0..=1`).
    pub set_focus_rect: Option<String>,
    /// Reference an external depth map as `RELATIVE_PATH:SHA256`.
    pub set_depth_artifact: Option<String>,
    /// Remove the external depth reference (back to the heuristic).
    pub clear_depth_artifact: bool,
    /// Remove the whole lens-blur stage (identity).
    pub clear: bool,
}

impl LensBlurRequest {
    /// True when the request can change the sidecar.
    pub fn wants_mutation(&self) -> bool {
        self.enable
            || self.disable
            || self.set_amount.is_some()
            || self.set_focal_near.is_some()
            || self.set_focal_far.is_some()
            || self.set_bokeh.is_some()
            || self.set_focus_rect.is_some()
            || self.set_depth_artifact.is_some()
            || self.clear_depth_artifact
            || self.clear
    }
}

/// Runs one `lens-blur` call: conflict matrix, load, mutate, validate, persist,
/// report. List-only mode is read-only (sidecar bytes unchanged).
pub fn run(request: &LensBlurRequest, persist: Persist) -> Result<StageRun, StageError> {
    if request.enable && request.disable {
        return Err(StageError::Message(
            "--enable and --disable are mutually exclusive".into(),
        ));
    }
    if request.set_depth_artifact.is_some() && request.clear_depth_artifact {
        return Err(StageError::Message(
            "--set-depth-artifact and --clear-depth-artifact are mutually exclusive".into(),
        ));
    }
    // `--clear` removes the whole stage and short-circuits every other setter
    // below, so combining it with one is a contradiction, not a silent no-op.
    if request.clear
        && (request.enable
            || request.disable
            || request.set_amount.is_some()
            || request.set_focal_near.is_some()
            || request.set_focal_far.is_some()
            || request.set_bokeh.is_some()
            || request.set_focus_rect.is_some()
            || request.set_depth_artifact.is_some()
            || request.clear_depth_artifact)
    {
        return Err(StageError::Message(
            "--clear removes the whole lens-blur stage and contradicts every other mutation flag"
                .into(),
        ));
    }
    let wants_mutation = request.wants_mutation();
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
    if request.clear {
        let copy = copy_mut(&mut document, &copy_id)?;
        copy.recipe.lens_blur = None;
        info!("lens-blur: cleared stage on copy `{copy_id}`");
        actions.push("clear".into());
    } else {
        if request.enable || request.disable {
            stage_mut(&mut document, &copy_id)?.enabled = request.enable;
            info!(
                "lens-blur: {} on copy `{copy_id}`",
                if request.enable {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            actions.push(if request.enable { "enable" } else { "disable" }.into());
        }
        if let Some(amount) = request.set_amount {
            stage_mut(&mut document, &copy_id)?.blur_amount = amount;
            info!("lens-blur: amount {amount} on copy `{copy_id}`");
            actions.push(format!("amount:{amount}"));
        }
        if let Some(near) = request.set_focal_near {
            stage_mut(&mut document, &copy_id)?.focal_near = near;
            info!("lens-blur: focal_near {near} on copy `{copy_id}`");
            actions.push(format!("focal-near:{near}"));
        }
        if let Some(far) = request.set_focal_far {
            stage_mut(&mut document, &copy_id)?.focal_far = far;
            info!("lens-blur: focal_far {far} on copy `{copy_id}`");
            actions.push(format!("focal-far:{far}"));
        }
        if let Some(shape) = request.set_bokeh.as_deref() {
            stage_mut(&mut document, &copy_id)?.bokeh = parse_bokeh_shape(shape)?;
            info!("lens-blur: bokeh {shape} on copy `{copy_id}`");
            actions.push(format!("bokeh:{shape}"));
        }
        if let Some(rect) = request.set_focus_rect.as_deref() {
            stage_mut(&mut document, &copy_id)?.focus_rect = parse_focus_rect(rect)?;
            info!("lens-blur: focus_rect {rect} on copy `{copy_id}`");
            actions.push(format!("focus-rect:{rect}"));
        }
        if let Some(spec) = request.set_depth_artifact.as_deref() {
            stage_mut(&mut document, &copy_id)?.depth_artifact = Some(parse_depth_artifact(spec)?);
            info!("lens-blur: depth_artifact {spec} on copy `{copy_id}`");
            actions.push("depth-artifact:set".into());
        }
        if request.clear_depth_artifact {
            stage_mut(&mut document, &copy_id)?.depth_artifact = None;
            info!("lens-blur: depth artifact cleared on copy `{copy_id}`");
            actions.push("depth-artifact:clear".into());
        }
    }
    if wants_mutation {
        // Loud gate: ranges, focal order, focus-rect geometry and portable
        // (relative) depth paths are rejected before anything is written.
        document.validate()?;
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

/// Mutable access to one virtual copy's lens-blur stage, creating an enabled
/// stage with centered defaults when none exists (loud on unknown ids).
pub fn stage_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut LensBlur, StageError> {
    let copy = copy_mut(document, copy_id)?;
    Ok(copy.recipe.lens_blur.get_or_insert(LensBlur {
        version: 1,
        enabled: true,
        focus_rect: FocusRect {
            x: 0.25,
            y: 0.25,
            width: 0.5,
            height: 0.5,
        },
        focal_near: 0.0,
        focal_far: 0.2,
        blur_amount: 0.5,
        bokeh: BokehShape::Round,
        depth_artifact: None,
    }))
}

/// Parses a bokeh shape name (loud on unknown values — never a guess).
pub fn parse_bokeh_shape(value: &str) -> Result<BokehShape, StageError> {
    match value {
        "round" => Ok(BokehShape::Round),
        "elliptical" => Ok(BokehShape::Elliptical),
        "hexagonal" => Ok(BokehShape::Hexagonal),
        _ => Err(StageError::Message(format!(
            "invalid bokeh shape `{value}`: expected round|elliptical|hexagonal"
        ))),
    }
}

/// Parses a focus rectangle as `x,y,w,h` (loud on malformed input; range
/// geometry is validated on save, not guessed here).
pub fn parse_focus_rect(value: &str) -> Result<FocusRect, StageError> {
    let parts: Vec<&str> = value.split(',').collect();
    let numbers: Option<Vec<f32>> = parts
        .iter()
        .map(|part| part.trim().parse::<f32>().ok())
        .collect();
    match numbers.as_deref() {
        Some([x, y, width, height]) => Ok(FocusRect {
            x: *x,
            y: *y,
            width: *width,
            height: *height,
        }),
        _ => Err(StageError::Message(format!(
            "invalid focus rect `{value}`: expected `x,y,w,h` with finite numbers"
        ))),
    }
}

/// Parses an external depth reference as `RELATIVE_PATH:SHA256` (loud on
/// malformed input; portability is validated on save).
pub fn parse_depth_artifact(value: &str) -> Result<DepthArtifactRef, StageError> {
    match value.split_once(':') {
        Some((path, sha)) if !path.trim().is_empty() && !sha.trim().is_empty() => {
            Ok(DepthArtifactRef {
                relative_path: path.trim().into(),
                sha256: sha.trim().into(),
            })
        }
        _ => Err(StageError::Message(format!(
            "invalid depth artifact `{value}`: expected `RELATIVE_PATH:SHA256`"
        ))),
    }
}

/// The `--json` payload of one lens-blur state, shared by both transports.
pub fn stage_payload(
    blur: Option<&LensBlur>,
    status: &str,
    actions: &[String],
    input: &str,
    copy_id: &str,
) -> Value {
    let payload = blur.map(|b| {
        json!({
            "enabled": b.enabled,
            "focus_rect": {"x": b.focus_rect.x, "y": b.focus_rect.y,
                "width": b.focus_rect.width, "height": b.focus_rect.height},
            "focal_near": b.focal_near,
            "focal_far": b.focal_far,
            "blur_amount": b.blur_amount,
            "bokeh": match b.bokeh {
                BokehShape::Round => "round",
                BokehShape::Elliptical => "elliptical",
                BokehShape::Hexagonal => "hexagonal",
            },
            "depth_artifact": b.depth_artifact.as_ref().map(|d| json!({
                "relative_path": d.relative_path, "sha256": d.sha256,
            })),
        })
    });
    json!({
        "command": "lens-blur",
        "input": input,
        "copy": copy_id,
        "lens_blur": payload,
        "status": status,
        "actions": actions,
    })
}

/// Reports the copy's lens-blur stage plus the shared depth status. Read-only:
/// the sidecar is never written here.
fn list(
    request: &LensBlurRequest,
    document: &SidecarDocument,
    copy_id: &str,
    actions: &[String],
    wrote: bool,
) -> Result<StageReport, StageError> {
    let copy = copy_ref(document, copy_id)?;
    let blur = copy.recipe.lens_blur.as_ref();
    // The CLI never resolves external depth files (no depth format in v1):
    // a referenced artifact reports `missing` until a loader exists.
    let status = lens_blur_status(blur, false);
    let payload = stage_payload(blur, status, actions, &request.input, copy_id);
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("copy: {} [{}]", copy.name, copy.id));
    match blur {
        Some(b) => {
            lines.push(format!("  enabled: {}", b.enabled));
            lines.push(format!(
                "  focus_rect: x={} y={} w={} h={}",
                b.focus_rect.x, b.focus_rect.y, b.focus_rect.width, b.focus_rect.height
            ));
            lines.push(format!(
                "  focal: near={} far={}",
                b.focal_near, b.focal_far
            ));
            lines.push(format!("  amount: {}", b.blur_amount));
            lines.push(format!(
                "  bokeh: {}",
                match b.bokeh {
                    BokehShape::Round => "round",
                    BokehShape::Elliptical => "elliptical",
                    BokehShape::Hexagonal => "hexagonal",
                }
            ));
            match &b.depth_artifact {
                Some(d) => lines.push(format!(
                    "  depth_artifact: {} ({})",
                    d.relative_path, d.sha256
                )),
                None => lines.push("  depth_artifact: none (heuristic)".into()),
            }
        }
        None => lines.push("  lens_blur: none".into()),
    }
    lines.push(format!("  status: {status}"));
    let summary = if actions.is_empty() {
        "lens-blur status listed".to_string()
    } else {
        format!("lens-blur updated: {}", actions.join(", "))
    };
    Ok(StageReport::new(
        "lens-blur",
        copy_id,
        actions.to_vec(),
        payload,
        lines,
        summary,
        wrote,
    ))
}
