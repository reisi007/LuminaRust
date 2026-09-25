//! Mask-layer evaluation and output-plane alignment helpers.

use super::{resample_plane_bilinear, MaskContext};
use crate::masks::{MaskGraph, MaskPlane};
use lumina_sidecar::{MaskDefinition, MaskReference, MaskStatus, VirtualCopy};

pub(super) enum LayerFailure {
    /// The referenced definition is missing or not `MaskStatus::Valid`.
    Unavailable {
        copy_id: String,
        mask_id: String,
        status: String,
        message: String,
    },
    /// `MaskGraph` evaluation or the frame-resize guard failed.
    Evaluation {
        copy_id: String,
        mask_id: String,
        reason: String,
        message: String,
    },
}

pub(super) fn evaluate_layer(
    masks: &MaskContext<'_>,
    layer: &lumina_sidecar::MaskLayer,
    geometry: super::MaskStageGeometry<'_>,
) -> Result<MaskPlane, LayerFailure> {
    let definition = find_definition(masks.copies, &layer.mask);
    let available = definition
        .map(|d| matches!(d.status, MaskStatus::Valid))
        .unwrap_or(false);
    if !available {
        let status = definition
            .map(|d| format!("{:?}", d.status))
            .unwrap_or_else(|| "Missing".into());
        let message = format!(
            "mask layer `{}` references unavailable mask `{}/{}` (status {status}); layer skipped",
            layer.id, layer.mask.copy_id, layer.mask.mask_id
        );
        return Err(LayerFailure::Unavailable {
            copy_id: layer.mask.copy_id.clone(),
            mask_id: layer.mask.mask_id.clone(),
            status,
            message,
        });
    }
    let graph = MaskGraph::new(masks.copies, masks.planes.clone());
    let plane = match graph.evaluate(&layer.mask) {
        Ok(plane) => plane,
        Err(error) => {
            let message = format!(
                "mask layer `{}` could not be evaluated (`{}/{}`): {error}; layer skipped",
                layer.id, layer.mask.copy_id, layer.mask.mask_id
            );
            return Err(LayerFailure::Evaluation {
                copy_id: layer.mask.copy_id.clone(),
                mask_id: layer.mask.mask_id.clone(),
                reason: error.to_string(),
                message,
            });
        }
    };
    // A degenerate (zero-dimension) plane cannot be resampled or composited
    // meaningfully; refuse deterministically instead of panicking or silently
    // falling back to an empty mask.
    if plane.width == 0 || plane.height == 0 {
        let message = format!(
            "mask layer `{}` evaluated to a zero-dimension plane (`{}/{}`); layer skipped",
            layer.id, layer.mask.copy_id, layer.mask.mask_id
        );
        return Err(LayerFailure::Evaluation {
            copy_id: layer.mask.copy_id.clone(),
            mask_id: layer.mask.mask_id.clone(),
            reason: "invalid zero-dimension plane".into(),
            message,
        });
    }
    let mut plane = if geometry.local_adjustment_mode {
        let definition =
            find_definition(masks.copies, &layer.mask).expect("available definition checked above");
        crate::mask_alignment::align_local_mask_plane(
            &plane,
            definition,
            geometry.input_width,
            geometry.input_height,
            geometry.recipe,
            masks.source_roi,
        )
        .map_err(|reason| LayerFailure::Evaluation {
            copy_id: layer.mask.copy_id.clone(),
            mask_id: layer.mask.mask_id.clone(),
            reason: reason.clone(),
            message: format!(
                "mask layer `{}` local-adjustment alignment failed (`{}/{}`): {reason}; layer skipped",
                layer.id, layer.mask.copy_id, layer.mask.mask_id
            ),
        })?
    } else {
        // Existing non-local mask behavior remains available for old recipes;
        // the explicit source→ROI→geometry path above is mandatory as soon as
        // a local adjustment is present.
        resample_plane_bilinear(&plane, geometry.frame_width, geometry.frame_height)
    };
    if plane.width != geometry.frame_width || plane.height != geometry.frame_height {
        let reason = format!(
            "aligned mask dimensions {}x{} do not match output {}x{}",
            plane.width, plane.height, geometry.frame_width, geometry.frame_height
        );
        return Err(LayerFailure::Evaluation {
            copy_id: layer.mask.copy_id.clone(),
            mask_id: layer.mask.mask_id.clone(),
            reason: reason.clone(),
            message: format!(
                "mask layer `{}` could not be aligned (`{}/{}`): {reason}; layer skipped",
                layer.id, layer.mask.copy_id, layer.mask.mask_id
            ),
        });
    }
    // F-049: apply the per-layer modulation (invert → feather → blur → density)
    // to the resolved, frame-sized plane before it weights the adjustments.
    // REVIEW-MASK-N2: an invalid modulation (e.g. a density outside 0..=1) is
    // an evaluation failure like any other — Strict aborts the render, Warn
    // skips the layer with a recorded message. No silent fallback.
    if let Err(error) = crate::mask_modulation::modulate_mask_plane(&mut plane, layer) {
        let message = format!(
            "mask layer `{}` could not be modulated (`{}/{}`): {error}; layer skipped",
            layer.id, layer.mask.copy_id, layer.mask.mask_id
        );
        return Err(LayerFailure::Evaluation {
            copy_id: layer.mask.copy_id.clone(),
            mask_id: layer.mask.mask_id.clone(),
            reason: error.to_string(),
            message,
        });
    }
    Ok(plane)
}

/// Finds the `MaskDefinition` referenced by `reference` across all copies
/// (mirrors `MaskGraph`'s `(copy_id, mask_id)` keying).
fn find_definition<'a>(
    copies: &'a [VirtualCopy],
    reference: &MaskReference,
) -> Option<&'a MaskDefinition> {
    copies
        .iter()
        .find(|c| c.id == reference.copy_id)
        .and_then(|c| c.mask_library.iter().find(|m| m.id == reference.mask_id))
}
