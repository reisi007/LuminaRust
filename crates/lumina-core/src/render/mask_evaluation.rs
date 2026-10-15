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

/// Evaluate the local mask stage for a caller-known geometry, without
/// rendering the global/geometry stages (GPU-RENDER-MASK-19 integration).
///
/// This is the *same* evaluation the CPU render uses: it validates the
/// persisted local adjustments loudly (`prevalidate_local_adjustments`) and
/// then runs the one `evaluate_mask_stage` implementation. It exists so a
/// GPU path can obtain the caller-evaluated layers the mask pass needs
/// (`LocalMaskLayer`) **without** a full CPU render and **without** a second,
/// divergent mask-evaluation implementation.
///
/// `input_width`/`input_height` are the decoded source dimensions the CPU
/// render would use as `mask_input_*`; `frame_width`/`frame_height` are the
/// post-global/post-geometry output dimensions the layers must be aligned to.
pub fn evaluate_local_mask_layers(
    context: &super::RenderContext<'_>,
    input_width: u32,
    input_height: u32,
    frame_width: u32,
    frame_height: u32,
) -> Result<(Vec<super::MaskLayerResult>, Vec<String>), crate::CoreError> {
    super::local_adjustments::prevalidate_local_adjustments(context)?;
    super::evaluate_mask_stage(
        context.masks.as_ref(),
        super::MaskStageGeometry {
            recipe: context.recipe,
            input_width,
            input_height,
            frame_width,
            frame_height,
            local_adjustment_mode: super::local_adjustments::has_visible_local_adjustments(
                context.masks.as_ref(),
            ),
        },
        &mut super::StageWork::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{render_frame, MaskPolicy, RenderContext};
    use crate::ImageFrame;
    use lumina_sidecar::{
        CoordinateSystem, DecodeFingerprint, EditRecipe, Extras, Flag, GeometryFingerprint,
        LocalAdjustments, MaskLayer, MaskOperation, ModelIdentity, Preprocessing, Resolution,
        SourceFingerprint,
    };
    use std::collections::BTreeMap;

    fn mask_definition(id: &str, width: u32, height: u32) -> MaskDefinition {
        MaskDefinition {
            id: id.into(),
            name: id.into(),
            source_fingerprint: SourceFingerprint {
                content_hash: "h".into(),
                byte_length: 1,
                extras: Extras::new(),
            },
            decode_context: DecodeFingerprint {
                decoder: "d".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            geometry_context: GeometryFingerprint {
                width,
                height,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: Extras::new(),
            },
            model: ModelIdentity {
                name: "m".into(),
                version: "1".into(),
                hash: "h".into(),
                extras: Extras::new(),
            },
            inference_resolution: Resolution {
                width,
                height,
                extras: Extras::new(),
            },
            preprocessing: Preprocessing {
                name: "p".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            rescaling_method: "none".into(),
            rescaling_parameters: BTreeMap::new(),
            coordinate_system: CoordinateSystem::SourceOriented,
            status: MaskStatus::Valid,
            created_at: "now".into(),
            generator_version: "g".into(),
            error_text: None,
            artifact: None,
            operation: MaskOperation::Source,
            references: vec![],
            prompt: None,
            extras: Extras::new(),
            ai_select: None,
        }
    }

    /// One visible, non-neutral P0 local layer over a `2x2` source definition.
    fn fixture() -> (
        Vec<VirtualCopy>,
        BTreeMap<(String, String), MaskPlane>,
        EditRecipe,
    ) {
        let definition = mask_definition("subject", 2, 2);
        let layer = MaskLayer {
            id: "layer-1".into(),
            mask: MaskReference {
                copy_id: "vc".into(),
                mask_id: "subject".into(),
                extras: Extras::new(),
            },
            inverted: false,
            feather: 0.0,
            blur: 0.0,
            density: 1.0,
            extras: Extras::new(),
            visible: true,
            local_adjustments: Some(LocalAdjustments {
                exposure: 1.0,
                ..LocalAdjustments::default()
            }),
        };
        let copies = vec![VirtualCopy {
            id: "vc".into(),
            name: "vc".into(),
            is_default: true,
            rating: 0,
            flag: Flag::Unflagged,
            recipe: EditRecipe::default(),
            mask_library: vec![definition],
            mask_layers: vec![layer],
            history: vec![],
            export_records: vec![],
            extras: Extras::new(),
        }];
        let planes = BTreeMap::from([(
            ("vc".into(), "subject".into()),
            MaskPlane::new(2, 2, vec![0, 32_768, u16::MAX, 0]).unwrap(),
        )]);
        (copies, planes, EditRecipe::default())
    }

    /// The public seam is *the same* evaluation the CPU render runs: for the
    /// identical geometry the layers and warnings are byte-for-byte equal to
    /// `RenderOutput.mask_layers`/`mask_warnings`.
    #[test]
    fn evaluate_local_mask_layers_matches_the_render_frame_mask_stage() {
        let (copies, planes, recipe) = fixture();
        let context = MaskContext {
            copies: &copies,
            active_copy_id: "vc",
            planes,
            policy: MaskPolicy::Strict,
            source_roi: None,
        };
        let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
        let render_context = RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(context),
        };

        let output = render_frame(&frame, &render_context).unwrap();
        assert_eq!(
            output.mask_layers.len(),
            1,
            "the fixture must exercise exactly one visible local layer"
        );

        let (layers, warnings) = evaluate_local_mask_layers(&render_context, 2, 2, 2, 2).unwrap();
        assert_eq!(layers, output.mask_layers);
        assert_eq!(warnings, output.mask_warnings);
    }
}
