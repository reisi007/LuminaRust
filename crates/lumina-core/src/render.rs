//! Shared render entry point (F-042).
//!
//! [`render_frame`] executes the documented pipeline order
//! `SourceActions → Adjustments (WB context before tonal values, including
//! geometry/crop) → Masks → Output` on an already decoded frame. Auto-tone
//! computation and Match Total Exposure remain recipe/caller orchestration
//! (F-041); this entry point applies the (possibly auto-toned) recipe. GUI and
//! CLI use the same entry point.

use crate::masks::{MaskGraph, MaskPlane};
use crate::{CoreError, ImageFrame};
use lumina_sidecar::{EditRecipe, MaskDefinition, MaskReference, MaskStatus, VirtualCopy};
use std::collections::BTreeMap;

/// A source-sized repair artifact: a u16 region plane (`0..=u16::MAX`) and an
/// RGBA8 replacement image with identical dimensions.  Applied after decode
/// and before auto-analysis/adjustments: `out = replacement` where
/// `region >= 32768` (50% threshold), otherwise the source pixel stays.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceActionArtifact {
    pub region: MaskPlane,
    pub replacement: ImageFrame,
}

/// How missing or invalid mask artifacts are handled (F-042).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskPolicy {
    /// Missing/invalid artifact aborts the render with an error.
    Strict,
    /// The layer is skipped, a warning is recorded, and the render continues
    /// (matches the conflict matrix: „Export trotzdem erlauben").
    Warn,
}

/// Mask stage context: the virtual copies, the active copy id, the artifact
/// planes keyed by `(copy_id, mask_id)`, and the policy.
#[derive(Debug, Clone)]
pub struct MaskContext<'a> {
    pub copies: &'a [VirtualCopy],
    pub active_copy_id: &'a str,
    pub planes: BTreeMap<(String, String), MaskPlane>,
    pub policy: MaskPolicy,
}

/// Everything [`render_frame`] needs beyond the decoded frame.
#[derive(Debug, Clone)]
pub struct RenderContext<'a> {
    pub recipe: &'a EditRecipe,
    pub camera_white_balance: Option<[f32; 4]>,
    pub source_actions: &'a [SourceActionArtifact],
    pub masks: Option<MaskContext<'a>>,
}

/// One effective mask layer: the evaluated and frame-sized (bilinearly
/// resampled) plane.  F-042 delivers these planes for F-049; the layer does
/// not modulate pixels yet.
#[derive(Debug, Clone, PartialEq)]
pub struct MaskLayerResult {
    pub layer_id: String,
    pub plane: MaskPlane,
}

/// Result of [`render_frame`].
#[derive(Debug, Clone, PartialEq)]
pub struct RenderOutput {
    pub frame: ImageFrame,
    /// Effective layers after resample (valid layers under `Warn`, all layers
    /// that evaluated under `Strict`).
    pub mask_layers: Vec<MaskLayerResult>,
    /// Missing/invalid layers, only populated under `MaskPolicy::Warn`.
    pub mask_warnings: Vec<String>,
}

/// Applies source actions, the recipe (adjustments incl. geometry/crop) and
/// the mask stage in the documented order.
///
/// # SourceActions stage
///
/// Every artifact is validated before any pixel is touched: `region` and
/// `replacement` must have identical dimensions, the value/byte counts must
/// match `width * height` (`* 4` for the replacement), and the region must
/// match the decoded frame dimensions (the MVP applies source actions at
/// source resolution; F-042-N1 defines the persisted artifact format and may
/// add resampling semantics there).  Any violation returns
/// [`CoreError::InvalidSourceAction`] — no silent fallback.
///
/// # Masks stage (only with `Some(masks)`)
///
/// For every `mask_layers` entry of the active copy the referenced
/// `MaskDefinition` is checked: only `MaskStatus::Valid` definitions are
/// evaluated; the artifact plane must be present in `planes` and the whole
/// `MaskGraph` evaluation (including Union/Intersect/Subtract/Invert) must
/// succeed.  `MaskPolicy::Strict` turns each failure into
/// [`CoreError::MaskUnavailable`] / [`CoreError::MaskEvaluation`];
/// `MaskPolicy::Warn` skips the layer and records a warning instead.  Valid
/// planes are bilinearly resampled to the current frame dimensions
/// (coordinate alignment between mask and frame is a documented limit —
/// `geometry_context` is not used for alignment yet).  A missing active copy
/// or an empty `mask_layers` list leaves the stage identical and produces no
/// warnings.
pub fn render_frame(
    frame: &ImageFrame,
    context: &RenderContext<'_>,
) -> Result<RenderOutput, CoreError> {
    let mut frame = frame.clone();
    apply_source_actions(&mut frame, context.source_actions)?;
    frame.apply_recipe_with_white_balance(context.recipe, context.camera_white_balance)?;

    let mut mask_layers = Vec::new();
    let mut mask_warnings = Vec::new();
    if let Some(masks) = &context.masks {
        if let Some(copy) = masks.copies.iter().find(|c| c.id == masks.active_copy_id) {
            for layer in &copy.mask_layers {
                match evaluate_layer(masks, layer, frame.width, frame.height) {
                    Ok(plane) => {
                        mask_layers.push(MaskLayerResult {
                            layer_id: layer.id.clone(),
                            plane,
                        });
                    }
                    Err(LayerFailure::Unavailable {
                        copy_id,
                        mask_id,
                        status,
                        message,
                    }) => match masks.policy {
                        MaskPolicy::Strict => {
                            return Err(CoreError::MaskUnavailable {
                                copy_id,
                                mask_id,
                                status,
                            });
                        }
                        MaskPolicy::Warn => mask_warnings.push(message),
                    },
                    Err(LayerFailure::Evaluation {
                        copy_id,
                        mask_id,
                        reason,
                        message,
                    }) => match masks.policy {
                        MaskPolicy::Strict => {
                            return Err(CoreError::MaskEvaluation {
                                copy_id,
                                mask_id,
                                reason,
                            });
                        }
                        MaskPolicy::Warn => mask_warnings.push(message),
                    },
                }
            }
        }
    }

    Ok(RenderOutput {
        frame,
        mask_layers,
        mask_warnings,
    })
}

enum LayerFailure {
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

fn evaluate_layer(
    masks: &MaskContext<'_>,
    layer: &lumina_sidecar::MaskLayer,
    frame_width: u32,
    frame_height: u32,
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
    Ok(resample_plane_bilinear(&plane, frame_width, frame_height))
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

/// Deterministic inverse bilinear resample of a u16 mask plane to the target
/// frame dimensions.  Each target pixel samples the source at its pixel
/// centre (`(x + 0.5) * sw / tw - 0.5`), clamped to the source edge; the four
/// neighbours are combined with `f32` weights and rounded to `u16`.  This is
/// the same convention used by the geometry resampling in `pipeline.rs`.
fn resample_plane_bilinear(plane: &MaskPlane, target_width: u32, target_height: u32) -> MaskPlane {
    let (sw, sh) = (plane.width as f32, plane.height as f32);
    let (tw, th) = (target_width as f32, target_height as f32);
    let mut values = Vec::with_capacity(target_width as usize * target_height as usize);
    for y in 0..target_height {
        let sy = ((y as f32 + 0.5) * sh / th - 0.5).clamp(0.0, sh - 1.0);
        let y0 = sy.floor() as u32;
        let y1 = (y0 + 1).min(plane.height - 1);
        let fy = sy - y0 as f32;
        for x in 0..target_width {
            let sx = ((x as f32 + 0.5) * sw / tw - 0.5).clamp(0.0, sw - 1.0);
            let x0 = sx.floor() as u32;
            let x1 = (x0 + 1).min(plane.width - 1);
            let fx = sx - x0 as f32;
            let at = |xx: u32, yy: u32| plane.values[(yy * plane.width + xx) as usize] as f32;
            let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
            let bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
            let value = (top * (1.0 - fy) + bottom * fy).round();
            values.push(value.clamp(0.0, u16::MAX as f32) as u16);
        }
    }
    MaskPlane {
        width: target_width,
        height: target_height,
        values,
    }
}

/// Applies the source-action compositing.
///
/// `out = replacement` for pixels with `region >= 32768` (50% threshold),
/// otherwise the source stays.  Alpha comes from the replacement for replaced
/// pixels, otherwise from the source.
fn apply_source_actions(
    frame: &mut ImageFrame,
    actions: &[SourceActionArtifact],
) -> Result<(), CoreError> {
    for (index, action) in actions.iter().enumerate() {
        let region = &action.region;
        let replacement = &action.replacement;
        if region.width != replacement.width || region.height != replacement.height {
            return Err(CoreError::InvalidSourceAction(format!(
                "artifact {index}: region {region_width}x{region_height} does not match replacement {replacement_width}x{replacement_height}",
                region_width = region.width,
                region_height = region.height,
                replacement_width = replacement.width,
                replacement_height = replacement.height,
            )));
        }
        let expected_values = region.width as usize * region.height as usize;
        if region.values.len() != expected_values {
            return Err(CoreError::InvalidSourceAction(format!(
                "artifact {index}: region has {} values, expected {expected_values}",
                region.values.len()
            )));
        }
        if replacement.pixels.len() != expected_values * 4 {
            return Err(CoreError::InvalidSourceAction(format!(
                "artifact {index}: replacement has {} bytes, expected {}",
                replacement.pixels.len(),
                expected_values * 4
            )));
        }
        if region.width != frame.width || region.height != frame.height {
            return Err(CoreError::InvalidSourceAction(format!(
                "artifact {index}: region {region_width}x{region_height} does not match frame {frame_width}x{frame_height}",
                region_width = region.width,
                region_height = region.height,
                frame_width = frame.width,
                frame_height = frame.height,
            )));
        }
        for (i, (pixel, value)) in frame
            .pixels
            .chunks_exact_mut(4)
            .zip(region.values.iter())
            .enumerate()
        {
            if *value >= 32768 {
                pixel.copy_from_slice(&replacement.pixels[i * 4..i * 4 + 4]);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tone::{analyze_tone, match_total_exposure, suggest_auto_tone, AutoToneConfig};
    use lumina_sidecar::{
        AnalysisFingerprint, AutoFeatures, CoordinateSystem, DecodeFingerprint, Extras,
        GeometryFingerprint, MaskLayer, MaskOperation, ModelIdentity, Preprocessing, Resolution,
        SourceFingerprint,
    };

    fn mask_definition(
        id: &str,
        status: MaskStatus,
        operation: MaskOperation,
        references: Vec<lumina_sidecar::MaskReference>,
    ) -> MaskDefinition {
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
                width: 2,
                height: 1,
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
                width: 2,
                height: 1,
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
            status,
            created_at: "now".into(),
            generator_version: "g".into(),
            error_text: None,
            artifact: None,
            operation,
            references,
            extras: Extras::new(),
        }
    }

    fn reference(copy_id: &str, mask_id: &str) -> lumina_sidecar::MaskReference {
        lumina_sidecar::MaskReference {
            copy_id: copy_id.into(),
            mask_id: mask_id.into(),
            extras: Extras::new(),
        }
    }

    fn copy_with(
        id: &str,
        definitions: Vec<MaskDefinition>,
        layers: Vec<MaskLayer>,
    ) -> VirtualCopy {
        VirtualCopy {
            id: id.into(),
            name: id.into(),
            is_default: id == "vc-original",
            recipe: EditRecipe::default(),
            mask_library: definitions,
            mask_layers: layers,
            history: vec![],
            export_records: vec![],
            extras: Extras::new(),
        }
    }

    fn layer(id: &str, mask: lumina_sidecar::MaskReference) -> MaskLayer {
        MaskLayer {
            id: id.into(),
            mask,
            inverted: false,
            feather: 0.0,
            blur: 0.0,
            density: 1.0,
            extras: Extras::new(),
        }
    }

    fn base_frame() -> ImageFrame {
        ImageFrame::new(2, 2, vec![100; 16]).unwrap()
    }

    fn default_context<'a>(
        recipe: &'a EditRecipe,
        masks: Option<MaskContext<'a>>,
    ) -> RenderContext<'a> {
        RenderContext {
            recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks,
        }
    }

    // ---- SourceActions stage ----

    #[test]
    fn source_action_composites_above_threshold_and_keeps_alpha() {
        let frame = ImageFrame::new(2, 1, vec![100, 100, 100, 255, 200, 200, 200, 40]).unwrap();
        let action = SourceActionArtifact {
            region: MaskPlane::new(2, 1, vec![32768, 32767]).unwrap(),
            replacement: ImageFrame::new(2, 1, vec![10, 20, 30, 128, 1, 2, 3, 9]).unwrap(),
        };
        let recipe = EditRecipe::default();
        let output = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[action],
                masks: None,
            },
        )
        .unwrap();
        // Pixel 0: region 32768 >= threshold -> replacement incl. its alpha.
        assert_eq!(&output.frame.pixels[0..4], &[10, 20, 30, 128]);
        // Pixel 1: region 32767 < threshold -> source incl. its alpha.
        assert_eq!(&output.frame.pixels[4..8], &[200, 200, 200, 40]);
    }

    #[test]
    fn empty_source_actions_are_byte_identical_to_apply_recipe() {
        let frame = ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 7]).unwrap();
        let recipe = EditRecipe {
            adjustments: BTreeMap::from([("exposure".into(), 0.5), ("contrast".into(), -0.2)]),
            ..Default::default()
        };
        let mut expected = frame.clone();
        expected
            .apply_recipe_with_white_balance(&recipe, Some([1.0, 1.0, 1.0, 1.0]))
            .unwrap();
        let output = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: Some([1.0, 1.0, 1.0, 1.0]),
                source_actions: &[],
                masks: None,
            },
        )
        .unwrap();
        assert_eq!(output.frame, expected);
        assert!(output.mask_layers.is_empty());
        assert!(output.mask_warnings.is_empty());
    }

    #[test]
    fn source_actions_run_before_adjustments() {
        // Pixel value 100. Exposure +1 doubles whatever the source-actions
        // stage left in the frame. With the action applied BEFORE adjustments
        // the replaced value 10 becomes 20 (not 10 = action after adjustments,
        // not 200 = no action at all).
        let frame = ImageFrame::new(1, 1, vec![100, 100, 100, 255]).unwrap();
        let recipe = EditRecipe {
            adjustments: BTreeMap::from([("exposure".into(), 1.0)]),
            ..Default::default()
        };
        let action = SourceActionArtifact {
            region: MaskPlane::new(1, 1, vec![65535]).unwrap(),
            replacement: ImageFrame::new(1, 1, vec![10, 10, 10, 255]).unwrap(),
        };
        let output = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[action],
                masks: None,
            },
        )
        .unwrap();
        assert_eq!(output.frame.pixels, vec![20, 20, 20, 255]);

        // Control: no action -> 100 * 2 = 200.
        let control = render_frame(&frame, &default_context(&recipe, None)).unwrap();
        assert_eq!(control.frame.pixels, vec![200, 200, 200, 255]);
    }

    #[test]
    fn source_action_rejects_mismatched_artifacts() {
        let frame = base_frame();
        let recipe = EditRecipe::default();
        let mismatched_dims = SourceActionArtifact {
            region: MaskPlane::new(2, 2, vec![0; 4]).unwrap(),
            replacement: ImageFrame::new(1, 4, vec![0; 16]).unwrap(),
        };
        let error = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[mismatched_dims],
                masks: None,
            },
        )
        .unwrap_err();
        assert!(matches!(error, CoreError::InvalidSourceAction(_)));

        let wrong_frame_dims = SourceActionArtifact {
            region: MaskPlane::new(1, 1, vec![0]).unwrap(),
            replacement: ImageFrame::new(1, 1, vec![0; 4]).unwrap(),
        };
        assert!(matches!(
            render_frame(
                &frame,
                &RenderContext {
                    recipe: &recipe,
                    camera_white_balance: None,
                    source_actions: &[wrong_frame_dims],
                    masks: None,
                },
            ),
            Err(CoreError::InvalidSourceAction(_))
        ));
    }

    // ---- Masks stage ----

    fn mask_context<'a>(
        copies: &'a [VirtualCopy],
        active_copy_id: &'a str,
        planes: BTreeMap<(String, String), MaskPlane>,
        policy: MaskPolicy,
    ) -> MaskContext<'a> {
        MaskContext {
            copies,
            active_copy_id,
            planes,
            policy,
        }
    }

    #[test]
    fn valid_layer_is_resampled_to_frame_dimensions() {
        let definitions = vec![mask_definition(
            "subject",
            MaskStatus::Valid,
            MaskOperation::Source,
            vec![],
        )];
        let copies = vec![copy_with(
            "vc",
            definitions,
            vec![layer("layer-1", reference("vc", "subject"))],
        )];
        let planes = BTreeMap::from([(
            ("vc".into(), "subject".into()),
            MaskPlane::new(1, 1, vec![32768]).unwrap(),
        )]);
        let recipe = EditRecipe::default();
        let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
        let output = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: Some(mask_context(&copies, "vc", planes, MaskPolicy::Strict)),
            },
        )
        .unwrap();
        assert_eq!(output.mask_layers.len(), 1);
        let result = &output.mask_layers[0];
        assert_eq!(result.layer_id, "layer-1");
        assert_eq!((result.plane.width, result.plane.height), (2, 2));
        assert_eq!(result.plane.values, vec![32768; 4]);
        assert!(output.mask_warnings.is_empty());
    }

    #[test]
    fn missing_plane_is_strict_error_and_warn_ok() {
        let definitions = vec![mask_definition(
            "subject",
            MaskStatus::Valid,
            MaskOperation::Source,
            vec![],
        )];
        let copies = vec![copy_with(
            "vc",
            definitions,
            vec![layer("layer-1", reference("vc", "subject"))],
        )];
        let recipe = EditRecipe::default();
        let frame = base_frame();
        let strict = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: Some(mask_context(
                    &copies,
                    "vc",
                    BTreeMap::new(),
                    MaskPolicy::Strict,
                )),
            },
        )
        .unwrap_err();
        assert!(matches!(
            strict,
            CoreError::MaskEvaluation {
                ref copy_id,
                ref mask_id,
                ..
            } if copy_id == "vc" && mask_id == "subject"
        ));

        let warn = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: Some(mask_context(
                    &copies,
                    "vc",
                    BTreeMap::new(),
                    MaskPolicy::Warn,
                )),
            },
        )
        .unwrap();
        assert!(warn.mask_layers.is_empty());
        assert_eq!(warn.mask_warnings.len(), 1);
        assert!(warn.mask_warnings[0].contains("layer-1"));
        assert_eq!(warn.frame, frame);
    }

    #[test]
    fn non_valid_definition_is_strict_error_and_warn_ok() {
        let definitions = vec![mask_definition(
            "subject",
            MaskStatus::Missing,
            MaskOperation::Source,
            vec![],
        )];
        let copies = vec![copy_with(
            "vc",
            definitions,
            vec![layer("layer-1", reference("vc", "subject"))],
        )];
        let recipe = EditRecipe::default();
        let frame = base_frame();
        let strict = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: Some(mask_context(
                    &copies,
                    "vc",
                    BTreeMap::new(),
                    MaskPolicy::Strict,
                )),
            },
        )
        .unwrap_err();
        assert!(matches!(
            strict,
            CoreError::MaskUnavailable {
                ref copy_id,
                ref mask_id,
                ref status,
            } if copy_id == "vc" && mask_id == "subject" && status == "Missing"
        ));

        let warn = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: Some(mask_context(
                    &copies,
                    "vc",
                    BTreeMap::new(),
                    MaskPolicy::Warn,
                )),
            },
        )
        .unwrap();
        assert!(warn.mask_layers.is_empty());
        assert_eq!(warn.mask_warnings.len(), 1);
        assert!(warn.mask_warnings[0].contains("unavailable"));
    }

    #[test]
    fn missing_definition_is_strict_error_and_warn_ok() {
        // The layer references a mask that exists in no copy at all.
        let copies = vec![copy_with(
            "vc",
            vec![],
            vec![layer("layer-1", reference("vc", "ghost"))],
        )];
        let recipe = EditRecipe::default();
        let frame = base_frame();
        let strict = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: Some(mask_context(
                    &copies,
                    "vc",
                    BTreeMap::new(),
                    MaskPolicy::Strict,
                )),
            },
        )
        .unwrap_err();
        assert!(matches!(
            strict,
            CoreError::MaskUnavailable { ref mask_id, .. } if mask_id == "ghost"
        ));

        let warn = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: Some(mask_context(
                    &copies,
                    "vc",
                    BTreeMap::new(),
                    MaskPolicy::Warn,
                )),
            },
        )
        .unwrap();
        assert_eq!(warn.mask_warnings.len(), 1);
    }

    #[test]
    fn graph_dimension_mismatch_is_rejected() {
        let definitions = vec![
            mask_definition("a", MaskStatus::Valid, MaskOperation::Source, vec![]),
            mask_definition("b", MaskStatus::Valid, MaskOperation::Source, vec![]),
            mask_definition(
                "union",
                MaskStatus::Valid,
                MaskOperation::Union,
                vec![reference("vc", "a"), reference("vc", "b")],
            ),
        ];
        let copies = vec![copy_with(
            "vc",
            definitions,
            vec![layer("layer-1", reference("vc", "union"))],
        )];
        let planes = BTreeMap::from([
            (
                ("vc".into(), "a".into()),
                MaskPlane::new(2, 1, vec![0, 1]).unwrap(),
            ),
            (
                ("vc".into(), "b".into()),
                MaskPlane::new(1, 2, vec![2, 3]).unwrap(),
            ),
        ]);
        let recipe = EditRecipe::default();
        let frame = base_frame();
        let strict = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: Some(mask_context(&copies, "vc", planes, MaskPolicy::Strict)),
            },
        )
        .unwrap_err();
        assert!(matches!(
            strict,
            CoreError::MaskEvaluation {
                ref copy_id,
                ref mask_id,
                ..
            } if copy_id == "vc" && mask_id == "union"
        ));
    }

    #[test]
    fn graph_operations_and_cross_copy_references_evaluate() {
        let source = mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
        let copies = vec![
            copy_with(
                "vc",
                vec![],
                vec![layer("layer-1", reference("other", "subject"))],
            ),
            copy_with("other", vec![source], vec![]),
        ];
        let planes = BTreeMap::from([(
            ("other".into(), "subject".into()),
            MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
        )]);
        let recipe = EditRecipe::default();
        let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
        let output = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: Some(mask_context(&copies, "vc", planes, MaskPolicy::Warn)),
            },
        )
        .unwrap();
        assert_eq!(output.mask_layers.len(), 1);
        assert_eq!(output.mask_layers[0].plane.values, vec![u16::MAX; 4]);
        assert!(output.mask_warnings.is_empty());
    }

    #[test]
    fn no_layers_is_identical_to_no_mask_context() {
        let copies = vec![copy_with("vc", vec![], vec![])];
        let recipe = EditRecipe {
            adjustments: BTreeMap::from([("exposure".into(), 0.25)]),
            ..Default::default()
        };
        let frame = base_frame();
        let with_masks = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: Some(mask_context(
                    &copies,
                    "vc",
                    BTreeMap::new(),
                    MaskPolicy::Strict,
                )),
            },
        )
        .unwrap();
        let without_masks = render_frame(&frame, &default_context(&recipe, None)).unwrap();
        assert_eq!(with_masks.frame, without_masks.frame);
        assert!(with_masks.mask_layers.is_empty());
        assert!(with_masks.mask_warnings.is_empty());
    }

    // ---- backward compatibility ----

    #[test]
    fn backward_compatible_with_apply_recipe() {
        let frame = ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 7]).unwrap();
        let recipe = EditRecipe {
            adjustments: BTreeMap::from([
                ("exposure".into(), 0.5),
                ("contrast".into(), 0.2),
                ("highlights".into(), -0.1),
            ]),
            geometry: Some(lumina_sidecar::Geometry {
                version: 1,
                crop: None,
                rotation_degrees: 0.0,
                mirror_horizontal: false,
                mirror_vertical: false,
            }),
            ..Default::default()
        };
        let mut expected = frame.clone();
        expected.apply_recipe(&recipe).unwrap();
        let output = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
            },
        )
        .unwrap();
        assert_eq!(output.frame, expected);
        assert!(output.mask_layers.is_empty());
        assert!(output.mask_warnings.is_empty());
    }

    #[test]
    fn resample_is_deterministic_and_bounds_safe() {
        // 2x1 source plane with an edge value; every target pixel must be in
        // 0..=u16::MAX and repeated runs must be identical.
        let plane = MaskPlane::new(2, 1, vec![0, u16::MAX]).unwrap();
        let first = resample_plane_bilinear(&plane, 4, 2);
        let second = resample_plane_bilinear(&plane, 4, 2);
        assert_eq!(first, second);
        assert_eq!((first.width, first.height), (4, 2));
        // Bilinear horizontal upscale across two equal rows: x=0 keeps the
        // left value, x=3 keeps the right value, the middle columns
        // interpolate between 0 and u16::MAX.
        assert_eq!(first.values[0], 0);
        assert_eq!(first.values[3], u16::MAX);
        assert_eq!(first.values[1], (u16::MAX as f32 * 0.25).round() as u16);
        assert_eq!(first.values[2], (u16::MAX as f32 * 0.75).round() as u16);
        assert_eq!(first.values[4..6], first.values[0..2]);
        assert_eq!(first.values[6..8], first.values[2..4]);

        // Same-size resample is the identity.
        assert_eq!(
            resample_plane_bilinear(&plane, 2, 1),
            MaskPlane::new(2, 1, vec![0, u16::MAX]).unwrap()
        );
    }

    // ---- F-085: source actions × auto-WB / auto-tone / exposure matching ----

    fn action(region: MaskPlane, replacement: ImageFrame) -> SourceActionArtifact {
        SourceActionArtifact {
            region,
            replacement,
        }
    }

    #[test]
    fn source_action_runs_before_white_balance() {
        // Both pixels start at (100,100,100). The action replaces pixel 0 with
        // (10,20,30); the WB recipe (wb_temperature 3000 -> warmth -0.63636,
        // gains [1.22273, 1.0, 0.77727]) is applied afterwards. Order proof:
        //   - action first + WB:  replaced -> (12,20,23), source -> (122,100,78)
        //   - action only:        replaced -> (10,20,30) (WB not applied)
        //   - WB only:            every pixel -> (122,100,78)
        let frame = ImageFrame::new(2, 1, vec![100, 100, 100, 255, 100, 100, 100, 255]).unwrap();
        let recipe = EditRecipe {
            adjustments: BTreeMap::from([("wb_temperature".into(), 3000.0)]),
            ..Default::default()
        };
        let actions = [action(
            MaskPlane::new(2, 1, vec![65535, 0]).unwrap(),
            ImageFrame::new(2, 1, vec![10, 20, 30, 255, 0, 0, 0, 0]).unwrap(),
        )];
        let with_action = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &actions,
                masks: None,
            },
        )
        .unwrap();
        let wb_only = render_frame(&frame, &default_context(&recipe, None)).unwrap();
        let action_only = render_frame(
            &frame,
            &RenderContext {
                recipe: &EditRecipe::default(),
                camera_white_balance: None,
                source_actions: &actions,
                masks: None,
            },
        )
        .unwrap();

        // Exact values: the replaced pixel receives the WB gain on the
        // replacement's values, the non-replaced pixel on the source's values.
        assert_eq!(&with_action.frame.pixels[0..4], &[12, 20, 23, 255]);
        assert_eq!(&with_action.frame.pixels[4..8], &[122, 100, 78, 255]);
        // Differential proofs of the order:
        // WB changed the action output (10,20,30) -> (12,20,23) ...
        assert_ne!(
            &with_action.frame.pixels[0..4],
            &action_only.frame.pixels[0..4]
        );
        // ... and the action changed the WB input (100,100,100) -> (10,20,30).
        assert_ne!(&with_action.frame.pixels[0..4], &wb_only.frame.pixels[0..4]);
        // The non-replaced pixel is identical to WB-only (same source value).
        assert_eq!(&with_action.frame.pixels[4..8], &wb_only.frame.pixels[4..8]);
    }

    #[test]
    fn source_action_changes_auto_tone_and_measurement_is_post_action() {
        // 1x4 frame; the action replaces the bright pixel 255 with 20. The
        // post-action median (80/255 ~= 0.314) differs clearly from the
        // pre-action median (114/255 ~= 0.447), so suggest_auto_tone must
        // produce different exposure/contrast.
        let frame = ImageFrame::new(
            4,
            1,
            vec![
                255, 255, 255, 255, 128, 128, 128, 255, 100, 100, 100, 255, 60, 60, 60, 255,
            ],
        )
        .unwrap();
        let actions = [action(
            MaskPlane::new(4, 1, vec![65535, 0, 0, 0]).unwrap(),
            ImageFrame::new(
                4,
                1,
                vec![20, 20, 20, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            )
            .unwrap(),
        )];
        let config = AutoToneConfig::default();

        let pre = suggest_auto_tone(&frame, config).unwrap();
        let post_frame = render_frame(
            &frame,
            &RenderContext {
                recipe: &EditRecipe::default(),
                camera_white_balance: None,
                source_actions: &actions,
                masks: None,
            },
        )
        .unwrap()
        .frame;
        let post = suggest_auto_tone(&post_frame, config).unwrap();

        assert_eq!(post.analysis.sample_count, 4);
        assert!(
            (post.exposure - pre.exposure).abs() > 0.1,
            "auto exposure must differ between pre-action ({}) and post-action ({}) frames",
            pre.exposure,
            post.exposure
        );
        assert!(
            (post.contrast - pre.contrast).abs() > 0.1,
            "auto contrast must differ between pre-action ({}) and post-action ({}) frames",
            pre.contrast,
            post.contrast
        );

        // Caller semantics (CLI/GUI): auto-tone measures the post-action
        // frame, the result is written into the recipe, then the full recipe
        // is rendered with the same source actions. The median of the result
        // must hit the documented target (0.5) within tolerance ±0.02.
        let recipe = EditRecipe {
            adjustments: BTreeMap::from([
                ("exposure".into(), post.exposure),
                ("contrast".into(), post.contrast),
            ]),
            ..Default::default()
        };
        let rendered = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &actions,
                masks: None,
            },
        )
        .unwrap();
        let median = analyze_tone(&rendered.frame).median;
        assert!(
            (median - 0.5).abs() <= 0.02,
            "median {median} not within 0.02 of the 0.5 auto-tone target"
        );
        // Applying the recipe to the post-action frame directly is equivalent
        // to rendering the original with action + recipe.
        let direct = render_frame(
            &post_frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
            },
        )
        .unwrap();
        assert_eq!(direct.frame, rendered.frame);
    }

    #[test]
    fn source_action_changes_matching_delta_and_application_reaches_target() {
        // Same frame as above: the matching delta measured on the post-action
        // frame (mean 77/255 ~= 0.302) differs clearly from the delta on the
        // pre-action frame (mean 135.75/255 ~= 0.532).
        let frame = ImageFrame::new(
            4,
            1,
            vec![
                255, 255, 255, 255, 128, 128, 128, 255, 100, 100, 100, 255, 60, 60, 60, 255,
            ],
        )
        .unwrap();
        let actions = [action(
            MaskPlane::new(4, 1, vec![65535, 0, 0, 0]).unwrap(),
            ImageFrame::new(
                4,
                1,
                vec![20, 20, 20, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            )
            .unwrap(),
        )];
        let post_frame = render_frame(
            &frame,
            &RenderContext {
                recipe: &EditRecipe::default(),
                camera_white_balance: None,
                source_actions: &actions,
                masks: None,
            },
        )
        .unwrap()
        .frame;
        let delta_post = match_total_exposure(&post_frame, 0.5).unwrap();
        let delta_pre = match_total_exposure(&frame, 0.5).unwrap();
        assert!(
            (delta_post - delta_pre).abs() > 0.2,
            "matching delta must differ between pre-action ({delta_pre}) and post-action ({delta_post}) frames"
        );

        // CLI semantics: matching measures the rendered (post-action) frame
        // and applies the exposure delta to that same frame. The result must
        // reach the target luminance within tolerance ±0.02.
        let mut matched = post_frame.clone();
        matched
            .apply_recipe_with_white_balance(
                &EditRecipe {
                    adjustments: BTreeMap::from([("exposure".into(), delta_post)]),
                    ..Default::default()
                },
                None,
            )
            .unwrap();
        let mean = analyze_tone(&matched).mean;
        assert!(
            (mean - 0.5).abs() <= 0.02,
            "mean {mean} not within 0.02 of the 0.5 matching target"
        );
    }

    #[test]
    fn render_with_source_actions_does_not_mutate_inputs() {
        let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
        let actions = [action(
            MaskPlane::new(2, 2, vec![0, 32768, 65535, 32767]).unwrap(),
            ImageFrame::new(
                2,
                2,
                vec![
                    10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160,
                ],
            )
            .unwrap(),
        )];
        let frame_before = frame.clone();
        let region_before = actions[0].region.clone();
        let replacement_before = actions[0].replacement.clone();
        let recipe = EditRecipe {
            adjustments: BTreeMap::from([("exposure".into(), 0.5)]),
            ..Default::default()
        };
        render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &actions,
                masks: None,
            },
        )
        .unwrap();
        // Byte-identical comparisons: neither the input frame nor the artifact
        // (region/replacement) may be mutated by the render.
        assert_eq!(frame, frame_before);
        assert_eq!(actions[0].region, region_before);
        assert_eq!(actions[0].replacement, replacement_before);
    }

    #[test]
    fn source_action_threshold_boundaries_are_exact() {
        let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
        let actions = [action(
            MaskPlane::new(2, 2, vec![32768, 32767, 0, 65535]).unwrap(),
            ImageFrame::new(
                2,
                2,
                vec![10, 20, 30, 128, 1, 2, 3, 9, 4, 5, 6, 7, 8, 9, 10, 11],
            )
            .unwrap(),
        )];
        let rendered = render_frame(
            &frame,
            &RenderContext {
                recipe: &EditRecipe::default(),
                camera_white_balance: None,
                source_actions: &actions,
                masks: None,
            },
        )
        .unwrap();
        // 32768 (exact threshold) -> replaced incl. replacement alpha;
        // 32767 -> source; 0 -> source; u16::MAX -> replaced.
        assert_eq!(&rendered.frame.pixels[0..4], &[10, 20, 30, 128]);
        assert_eq!(&rendered.frame.pixels[4..8], &[100, 100, 100, 100]);
        assert_eq!(&rendered.frame.pixels[8..12], &[100, 100, 100, 100]);
        assert_eq!(&rendered.frame.pixels[12..16], &[8, 9, 10, 11]);
    }

    #[test]
    fn zero_source_action_region_is_byte_identical_to_no_action() {
        let frame = ImageFrame::new(
            2,
            2,
            vec![
                7, 13, 29, 255, 200, 100, 50, 3, 1, 2, 3, 4, 250, 251, 252, 253,
            ],
        )
        .unwrap();
        let actions = [action(
            MaskPlane::new(2, 2, vec![0; 4]).unwrap(),
            ImageFrame::new(2, 2, vec![9; 16]).unwrap(),
        )];
        let with = render_frame(
            &frame,
            &RenderContext {
                recipe: &EditRecipe::default(),
                camera_white_balance: None,
                source_actions: &actions,
                masks: None,
            },
        )
        .unwrap();
        let without = render_frame(&frame, &default_context(&EditRecipe::default(), None)).unwrap();
        assert_eq!(with.frame, without.frame);
        assert_eq!(with.frame, frame);
    }

    #[test]
    fn full_source_action_region_replaces_every_pixel() {
        let frame = ImageFrame::new(1, 3, vec![100; 12]).unwrap();
        let replacement =
            ImageFrame::new(1, 3, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]).unwrap();
        let actions = [action(
            MaskPlane::new(1, 3, vec![65535; 3]).unwrap(),
            replacement.clone(),
        )];
        let rendered = render_frame(
            &frame,
            &RenderContext {
                recipe: &EditRecipe::default(),
                camera_white_balance: None,
                source_actions: &actions,
                masks: None,
            },
        )
        .unwrap();
        assert_eq!(rendered.frame, replacement);
    }

    #[test]
    fn render_frame_is_deterministic_with_source_actions_and_masks() {
        let frame = ImageFrame::new(
            2,
            2,
            vec![
                90, 91, 92, 255, 40, 41, 42, 128, 200, 201, 202, 7, 30, 31, 32, 255,
            ],
        )
        .unwrap();
        let actions = [action(
            MaskPlane::new(2, 2, vec![0, 32768, 65535, 32767]).unwrap(),
            ImageFrame::new(
                2,
                2,
                vec![
                    10, 20, 30, 255, 11, 21, 31, 255, 12, 22, 32, 255, 13, 23, 33, 255,
                ],
            )
            .unwrap(),
        )];
        let recipe = EditRecipe {
            adjustments: BTreeMap::from([
                ("wb_temperature".into(), 5200.0),
                ("exposure".into(), 0.25),
            ]),
            ..Default::default()
        };
        // A valid mask layer so the full order actions -> adjustments -> masks
        // is exercised twice.
        let definitions = vec![mask_definition(
            "subject",
            MaskStatus::Valid,
            MaskOperation::Source,
            vec![],
        )];
        let copies = vec![copy_with(
            "vc",
            definitions,
            vec![layer("layer-1", reference("vc", "subject"))],
        )];
        let planes = BTreeMap::from([(
            ("vc".into(), "subject".into()),
            MaskPlane::new(2, 2, vec![0, 1, 32768, 65535]).unwrap(),
        )]);
        let context = RenderContext {
            recipe: &recipe,
            camera_white_balance: Some([1.2, 1.0, 0.9, 1.0]),
            source_actions: &actions,
            masks: Some(mask_context(&copies, "vc", planes, MaskPolicy::Warn)),
        };
        let first = render_frame(&frame, &context).unwrap();
        let second = render_frame(&frame, &context).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.frame, second.frame);
        assert!(first.mask_warnings.is_empty());
    }

    #[test]
    fn history_recipe_snapshot_reproduces_the_original_render() {
        // A recipe snapshot exactly as stored in a HistoryEntry: the final
        // adjustments plus the persisted auto_features (including the
        // analysis fingerprint). Re-rendering with only this snapshot on the
        // same frame must be byte-identical — the snapshot contains everything
        // the render needs, no external state.
        let frame = ImageFrame::new(
            2,
            2,
            vec![
                64, 96, 128, 255, 10, 20, 30, 255, 200, 100, 50, 255, 128, 128, 128, 255,
            ],
        )
        .unwrap();
        let recipe = EditRecipe {
            adjustments: BTreeMap::from([
                ("exposure".into(), 0.6),
                ("contrast".into(), -0.3),
                ("wb_temperature".into(), 4300.0),
            ]),
            auto_features: AutoFeatures {
                enable_auto_tone: true,
                auto_exposure: Some(0.6),
                auto_contrast: Some(-0.3),
                target_luminance: 0.5,
                analysis_fingerprint: Some(AnalysisFingerprint {
                    algorithm: "tone-rgba8-rec709".into(),
                    version: "1".into(),
                    input_fingerprint: "tone-rgba8-rec709:snapshot".into(),
                    extras: BTreeMap::new(),
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let original = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
            },
        )
        .unwrap()
        .frame;
        let entry = lumina_sidecar::HistoryEntry {
            id: "h-1".into(),
            recipe: recipe.clone(),
            recorded_at: Some("t".into()),
            extras: BTreeMap::new(),
        };
        let reproduced = render_frame(
            &frame,
            &RenderContext {
                recipe: &entry.recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
            },
        )
        .unwrap()
        .frame;
        assert_eq!(entry.recipe, recipe);
        assert_eq!(reproduced, original);
    }
}
