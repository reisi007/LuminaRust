//! CPU composition of typed local mask recipes (P0 Basic + P1.1 relative WB).

use super::{MaskContext, MaskLayerResult, RenderContext};
use crate::masks::MaskPlane;
use crate::{CoreError, ImageFrame};

/// Returns the active copy's layers in persisted order. The vector order is
/// part of the local compositor contract and is never sorted by mask id.
fn active_mask_layers<'a>(
    masks: Option<&'a MaskContext<'_>>,
) -> Option<&'a [lumina_sidecar::MaskLayer]> {
    let masks = masks?;
    masks
        .copies
        .iter()
        .find(|copy| copy.id == masks.active_copy_id)
        .map(|copy| copy.mask_layers.as_slice())
}

fn layer_local_adjustments(
    layer: &lumina_sidecar::MaskLayer,
) -> Result<Option<lumina_sidecar::LocalAdjustments>, CoreError> {
    layer
        .effective_local_adjustments()
        .map_err(|error| CoreError::InvalidLocalAdjustment {
            reason: error.to_string(),
        })
}

pub(super) fn has_visible_local_adjustments(masks: Option<&MaskContext<'_>>) -> bool {
    active_mask_layers(masks)
        .unwrap_or(&[])
        .iter()
        .filter(|layer| layer.visible)
        .filter_map(|layer| layer_local_adjustments(layer).ok().flatten())
        .any(|adjustments| !adjustments.is_neutral())
}

/// Validate all local state before source actions, global adjustments, or
/// geometry can touch a pixel. Invalid hidden layers are checked too: hiding a
/// layer must not turn malformed persisted state into an unreported future
/// error.
pub(super) fn prevalidate_local_adjustments(context: &RenderContext<'_>) -> Result<(), CoreError> {
    let Some(layers) = active_mask_layers(context.masks.as_ref()) else {
        return Ok(());
    };
    for layer in layers {
        let Some(adjustments) = layer_local_adjustments(layer)? else {
            continue;
        };
        adjustments
            .validate()
            .map_err(|error| CoreError::InvalidLocalAdjustment {
                reason: format!("mask layer `{}`: {error}", layer.id),
            })?;
    }

    let visible_local = has_visible_local_adjustments(context.masks.as_ref());
    if !visible_local {
        return Ok(());
    }
    if let Some(reason) = unsupported_local_geometry(context) {
        return Err(CoreError::LocalAdjustmentUnsupported { reason });
    }
    if let Some(roi) = context.masks.as_ref().and_then(|masks| masks.source_roi) {
        crate::mask_alignment::validate_source_roi(roi).map_err(|reason| {
            CoreError::LocalAdjustmentUnsupported {
                reason: format!("invalid preview source ROI: {reason}"),
            }
        })?;
    }
    Ok(())
}

fn unsupported_local_geometry(context: &RenderContext<'_>) -> Option<String> {
    if context.lensfun.is_some() {
        return Some(
            "Lensfun lens correction geometry is not supported for local adjustments".into(),
        );
    }
    if context.recipe.lens_correction.is_some() {
        return Some("lens correction geometry is not supported for local adjustments".into());
    }
    if context.recipe.effective_perspective().is_some() {
        return Some("perspective/upright geometry is not supported for local adjustments".into());
    }
    if context.recipe.generative_edit.is_some() {
        return Some("generative geometry is not supported for local adjustments".into());
    }
    if let Some(geometry) = context.recipe.geometry.as_ref() {
        if !geometry.rotation_degrees.is_finite()
            || ((geometry.rotation_degrees / 90.0).round() * 90.0 - geometry.rotation_degrees).abs()
                > 1e-4
        {
            return Some(format!(
                "rotation {} degrees is not supported; local adjustments require a 90-degree rotation",
                geometry.rotation_degrees
            ));
        }
    }
    None
}

/// Apply the local recipes in persisted order to a private working frame. A
/// failed layer therefore cannot leave an earlier overlapping layer partially
/// composited into the returned frame.
pub(super) fn apply_local_adjustments(
    frame: &mut ImageFrame,
    masks: Option<&MaskContext<'_>>,
    evaluated: &[MaskLayerResult],
    enabled: bool,
) -> Result<(), CoreError> {
    if !enabled {
        return Ok(());
    }
    let Some(layers) = active_mask_layers(masks) else {
        return Ok(());
    };
    let mut working = frame.clone();
    for result in evaluated {
        let Some(layer) = layers.iter().find(|layer| layer.id == result.layer_id) else {
            return Err(CoreError::InvalidLocalAdjustment {
                reason: format!(
                    "evaluated mask layer `{}` has no persisted layer",
                    result.layer_id
                ),
            });
        };
        let Some(adjustments) = layer_local_adjustments(layer)? else {
            continue;
        };
        if adjustments.is_neutral() {
            continue;
        }
        if result.plane.width != working.width || result.plane.height != working.height {
            return Err(CoreError::LocalAdjustmentUnsupported {
                reason: format!(
                    "layer `{}` mask {}x{} does not match output {}x{}",
                    result.layer_id,
                    result.plane.width,
                    result.plane.height,
                    working.width,
                    working.height
                ),
            });
        }
        let mut adjusted = working.clone();
        // P1.1 uses a dedicated local kernel: relative WB is evaluated after
        // the already-global frame and before the P0 Basic controls.  When a
        // delta is present the kernel keeps float gains through the whole
        // local pass and quantizes once; neutral recipes retain the exact P0
        // byte path above.
        adjusted.apply_mask_local_recipe(&adjustments)?;
        blend_local_layer(&mut working, &adjusted, &result.plane);
    }
    *frame = working;
    Ok(())
}

/// Fractional u16-alpha blend with byte-exact endpoints. Only RGB is written;
/// the frame alpha channel is copied from the current global/geometry result.
fn blend_local_layer(current: &mut ImageFrame, adjusted: &ImageFrame, plane: &MaskPlane) {
    debug_assert_eq!(current.pixels.len(), adjusted.pixels.len());
    debug_assert_eq!(current.pixels.len(), plane.values.len() * 4);
    for ((pixel, adjusted_pixel), &alpha) in current
        .pixels
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(adjusted.pixels.as_chunks::<4>().0.iter())
        .zip(plane.values.iter())
    {
        if alpha == 0 {
            continue;
        }
        if alpha == u16::MAX {
            pixel[..3].copy_from_slice(&adjusted_pixel[..3]);
            continue;
        }
        let inverse = u32::from(u16::MAX - alpha);
        let alpha = u32::from(alpha);
        for channel in 0..3 {
            pixel[channel] = ((u32::from(pixel[channel]) * inverse
                + u32::from(adjusted_pixel[channel]) * alpha
                + u32::from(u16::MAX / 2))
                / u32::from(u16::MAX)) as u8;
        }
        // Deliberately do not touch pixel[3].
    }
}
