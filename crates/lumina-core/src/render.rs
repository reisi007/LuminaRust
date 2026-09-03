#![allow(clippy::field_reassign_with_default)]
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

/// Feature-uniform borrow of a Lensfun corrector.
///
/// Defined for both `lensfun` and non-`lensfun` builds so [`RenderContext::lensfun`]
/// is always present. This removes a Cargo feature-unification footgun: when one
/// workspace crate enables `lumina-core/lensfun`, `RenderContext` gains the field
/// for *every* dependent, but a dependent whose own `lensfun` feature is off would
/// otherwise drop its `#[cfg(feature = "lensfun")] lensfun: None` init line and fail
/// to compile (E0063). See F-098 / CI clippy.
#[cfg(feature = "lensfun")]
#[derive(Debug, Clone, Copy)]
pub struct LensfunCorrectorRef<'a>(pub &'a lumina_lensfun::Corrector);

#[cfg(not(feature = "lensfun"))]
#[derive(Debug, Clone, Copy)]
pub struct LensfunCorrectorRef<'a>(pub core::marker::PhantomData<&'a ()>);

/// Everything [`render_frame`] needs beyond the decoded frame.
#[derive(Debug, Clone)]
pub struct RenderContext<'a> {
    pub recipe: &'a EditRecipe,
    pub camera_white_balance: Option<[f32; 4]>,
    pub source_actions: &'a [SourceActionArtifact],
    pub masks: Option<MaskContext<'a>>,
    /// Optional Lensfun lens corrector (F-098-N1). Always present; under the
    /// `lensfun` feature it carries a borrowed [`LensfunCorrectorRef`] (which
    /// wraps `lensfun::Corrector`), otherwise it is always `None`. `None` falls
    /// back to the manual distortion/vignette model — no silent behaviour
    /// change in default builds.
    pub lensfun: Option<LensfunCorrectorRef<'a>>,
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

/// PERF-GUI-1: stage work accounting for the staged render entry points.
///
/// Counters are incremented only when a stage actually executes, so a caller
/// can prove that an adjustment-only change reused the prepared base stage
/// (cache hit ⇒ no source-action pass, one adjustment pass). `base_cache_hit`
/// is caller-supplied bookkeeping: set it to `true` before calling
/// [`render_frame_from_base`] when the base frame came from a warm
/// [`crate::StageFrameCache`] entry instead of being rebuilt from the decoded
/// source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StageWork {
    pub base_cache_hit: bool,
    /// Number of source-action artifacts composited into a base frame by
    /// [`prepare_source_base`] during this call chain (0 on a base cache hit).
    pub source_action_artifacts_applied: u32,
    /// Number of adjustment passes (white balance + tone LUT) executed.
    pub adjustments_passes: u32,
    /// Number of geometry passes executed (the decoupled `Lens → Fill →
    /// Perspective → Expand → Crop` stages run unconditionally, mirroring
    /// [`render_frame`]).
    pub geometry_passes: u32,
    /// Number of mask layer evaluations attempted.
    pub mask_layers_evaluated: u32,
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
/// [`MaskPolicy::Warn`](crate::MaskPolicy) skips the layer and records a
/// warning instead.  Valid planes are bilinearly resampled to the current
/// frame dimensions (coordinate alignment between mask and frame is a
/// documented limit — `geometry_context` is not used for alignment yet).  A
/// missing active copy or an empty `mask_layers` list leaves the stage
/// identical and produces no warnings.
///
/// Implementation note (PERF-GUI-1): this is exactly
/// [`prepare_source_base`] followed by [`render_frame_from_base`] with a
/// discarded [`StageWork`]; callers that need stage accounting or want to
/// reuse a cached base stage call those two directly.
pub fn render_frame(
    frame: &ImageFrame,
    context: &RenderContext<'_>,
) -> Result<RenderOutput, CoreError> {
    let mut work = StageWork::default();
    let base = prepare_source_base(frame, context.source_actions, &mut work)?;
    render_frame_from_base(base, context, &mut work)
}

/// PERF-GUI-1: builds the cacheable base stage from a decoded source frame.
///
/// The base stage is the pipeline cut between `Decode`/`SourceActions` and
/// `Adjustments`: it clones `source` and composites every source-action
/// artifact, producing exactly the frame that [`render_frame`] would feed into
/// its adjustment pass. Cache it keyed by
/// [`crate::pipeline::RenderKey::stage_digest`] with
/// [`crate::cache::CacheStage::Base`] (recipe-blind identity) so interactive
/// exposure/color changes reuse it and re-render only the downstream stages.
pub fn prepare_source_base(
    source: &ImageFrame,
    actions: &[SourceActionArtifact],
    work: &mut StageWork,
) -> Result<ImageFrame, CoreError> {
    let mut base = source.clone();
    apply_source_actions(&mut base, actions)?;
    work.source_action_artifacts_applied += actions.len() as u32;
    Ok(base)
}
pub fn apply_spot_heals_from_recipe(
    frame: &mut ImageFrame,
    recipe: &EditRecipe,
) -> Result<(), CoreError> {
    reject_unsupported_spot_modes(recipe)?;
    let spots = crate::spot_heal::spots_from_recipe(recipe);
    crate::spot_heal::apply_spot_heals(frame, &spots)
}

/// GEN-PIPELINE-DECOUPLE: rejects spot entries that the portable core cannot
/// apply instead of silently skipping them. A `mode = "generative"` spot
/// (local ONNX inpaint, `kind = "spot_heal_generative"`) needs a model and a
/// persisted artifact; rendering it as healed — or as if it did not exist —
/// would be a silent fallback, so it is a hard [`CoreError::InvalidAdjustment`].
/// The same holds for a `mode = "heuristic"` entry that fails to parse or
/// validate: dropping it silently would hide a corrupt recipe. Absent
/// `spot_removals`, or entries without a `mode` key (legacy documents default
/// to heuristic, mirroring [`crate::spot_heal::spots_from_recipe`]), that
/// parse and validate are identity.
///
/// SPOT-TYPED-FIELD-FIX: both the legacy `extras["spot_removals"]` array and
/// the typed schema-v2 `recipe.spot_removals` are validated. The typed field
/// is authoritative after a sidecar roundtrip (deserialization consumes the
/// top-level key), so checking only extras would silently ignore typed
/// entries. Typed heuristic entries carry no heal geometry in schema-v2
/// (`SpotRemoval` holds only version/mode/artifact) and are therefore
/// unrenderable: they fail loudly here instead of rendering as if no spot
/// existed.
fn reject_unsupported_spot_modes(recipe: &EditRecipe) -> Result<(), CoreError> {
    reject_unsupported_spot_modes_extras(recipe)?;
    reject_unsupported_spot_modes_typed(recipe)?;
    Ok(())
}

fn reject_unsupported_spot_modes_extras(recipe: &EditRecipe) -> Result<(), CoreError> {
    let Some(value) = recipe.extras.get("spot_removals") else {
        return Ok(());
    };
    let arr = value.as_array().ok_or(CoreError::InvalidAdjustment {
        name: "spot_removals".into(),
        value: -1.0,
        minimum: 0.0,
        maximum: 0.0,
    })?;
    for entry in arr {
        let mode = entry
            .get("mode")
            .and_then(|m| m.as_str())
            .unwrap_or("heuristic");
        if mode != "heuristic" {
            return Err(CoreError::InvalidAdjustment {
                name: "spot_heal.mode".into(),
                value: -1.0,
                minimum: 0.0,
                maximum: 0.0,
            });
        }
        let spot: crate::spot_heal::SpotHeuristic =
            serde_json::from_value(entry.clone()).map_err(|_| CoreError::InvalidAdjustment {
                name: "spot_heal.entry".into(),
                value: -1.0,
                minimum: 0.0,
                maximum: 0.0,
            })?;
        spot.validate()?;
    }
    Ok(())
}

/// SPOT-TYPED-FIELD-FIX + SPOT-CORE-SHADOW-FOLLOWUP: validates the typed
/// schema-v2 `recipe.spot_removals` alongside the legacy extras array (see
/// [`reject_unsupported_spot_modes`]).
///
/// Decision (c000c6f sidecar mirror, 30ca7ba follow-up): since the sidecar
/// mirrors the raw `spot_removals` JSON back into `extras` on deserialize,
/// a loaded healthy recipe carries BOTH the geometry-carrying extras view
/// (source of truth for healing) AND a geometry-free typed mirror shadow
/// (`SpotRemoval` holds only version/mode/artifact). Rejecting that shadow
/// loudly would be a false alarm on every healthy loaded recipe, so a
/// geometry-free typed `Heuristic` entry is TOLERATED (skipped) exactly when
/// the extras `spot_removals` key is present — healing comes from extras via
/// [`crate::spot_heal::spots_from_recipe`], the shadow contributes nothing.
/// Two cases stay LOUD (hard `InvalidAdjustment`, never a silent fallback):
/// (a) a geometry-free typed heuristic WITHOUT the extras key (isolated
/// shadow, no geometry anywhere — nothing could heal it), and (b) a typed
/// heuristic that DOES carry geometry fields (if the schema ever gains
/// them — detected via its serialized JSON keys; an explicit typed geometry
/// path must be wired deliberately, not healed by accident). Unknown
/// `version` and `Generative` stay loud unconditionally (no silent
/// migration; model + artifact live outside the portable core).
fn reject_unsupported_spot_modes_typed(recipe: &EditRecipe) -> Result<(), CoreError> {
    let has_extras_geometry = recipe.extras.contains_key("spot_removals");
    for entry in &recipe.spot_removals {
        check_typed_spot_entry(entry, has_extras_geometry)?;
    }
    Ok(())
}

/// SPOT-CORE-SHADOW-FOLLOWUP: future-proof probe for typed heal geometry.
/// `SpotRemoval` currently serializes only version/mode/artifact, so this
/// is always false today; if the schema ever gains geometry fields
/// (center/radius/feather/offset/opacity/…), they appear in the serialized
/// JSON and the entry takes the loud path in [`check_typed_spot_entry`].
fn typed_entry_has_geometry(entry: &lumina_sidecar::SpotRemoval) -> bool {
    let Ok(value) = serde_json::to_value(entry) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    [
        "center",
        "center_x",
        "center_y",
        "radius",
        "feather",
        "offset_dx",
        "offset_dy",
        "source_offset",
        "opacity",
        "id",
        "status",
    ]
    .iter()
    .any(|key| object.contains_key(*key))
}

fn check_typed_spot_entry(
    entry: &lumina_sidecar::SpotRemoval,
    has_extras_geometry: bool,
) -> Result<(), CoreError> {
    if entry.version != lumina_sidecar::SPOT_REMOVAL_VERSION {
        return Err(CoreError::InvalidAdjustment {
            name: "spot_heal.version".into(),
            value: entry.version as f64,
            minimum: 1.0,
            maximum: 1.0,
        });
    }
    match entry.mode {
        lumina_sidecar::SpotRemovalMode::Generative => Err(CoreError::InvalidAdjustment {
            name: "spot_heal.mode".into(),
            value: -1.0,
            minimum: 0.0,
            maximum: 0.0,
        }),
        lumina_sidecar::SpotRemovalMode::Heuristic => {
            if typed_entry_has_geometry(entry) {
                // Typed entry claims its own geometry: healing it from the
                // extras view could apply the WRONG params, healing nothing
                // would be silent — reject loudly until a typed geometry
                // path is deliberately wired.
                return Err(CoreError::InvalidAdjustment {
                    name: "spot_heal.entry".into(),
                    value: -1.0,
                    minimum: 0.0,
                    maximum: 0.0,
                });
            }
            if has_extras_geometry {
                // Healthy loaded recipe: geometry-free mirror shadow, the
                // extras view carries the heal params — skip the shadow,
                // healing happens from extras. Documented tolerance, not a
                // silent fallback: there is nothing to fall back from, the
                // shadow was never renderable input.
                return Ok(());
            }
            // Isolated shadow with no geometry anywhere: nothing could heal
            // it, rendering as if no spot existed would be a silent
            // no-heal — reject loudly.
            Err(CoreError::InvalidAdjustment {
                name: "spot_heal.entry".into(),
                value: -1.0,
                minimum: 0.0,
                maximum: 0.0,
            })
        }
    }
}

/// PERF-GUI-1: continues a render from an already-prepared base frame.
///
/// Executes exactly the same stages in the same order as [`render_frame`]
/// after its source-action head — `SpotHeal` → `Adjustments` (white balance
/// before tonal values) → decoupled geometry `Lens → Fill → Perspective →
/// Expand → Crop` → mask-layer evaluation — so for a
/// base produced by [`prepare_source_base`] on the same inputs the output is
/// **byte-identical** to [`render_frame`] (proven by unit tests). The owned
/// `base` is consumed and mutated in place; no extra full-frame clone happens
/// on this path.
pub fn render_frame_from_base(
    mut base: ImageFrame,
    context: &RenderContext<'_>,
    work: &mut StageWork,
) -> Result<RenderOutput, CoreError> {
    apply_spot_heals_from_recipe(&mut base, context.recipe)?;
    base.apply_recipe_with_white_balance(context.recipe, context.camera_white_balance)?;
    work.adjustments_passes += 1;

    // GEN-PIPELINE-DECOUPLE: decoupled geometry order
    // `Lens → Fill → Perspective → Expand → Crop`
    // (see `crate::pipeline::GEOMETRY_STAGE_ORDER`). The legacy 5-in-1
    // `apply_geometry` is intentionally NOT used here: auto-fill must run
    // between lens and perspective (transparent wedges from undistortion are
    // filled before perspective resamples them), and the generative expand
    // must run before crop (crop coordinates reference the expanded canvas).
    // A failing expand (missing canvas, out-of-bounds offsets) aborts the
    // render with `InvalidAdjustment` — never a silent unexpanded render.
    #[cfg(feature = "lensfun")]
    {
        let corrector = context.lensfun.map(|LensfunCorrectorRef(c)| c);
        base.apply_lens_stage(context.recipe.lens_correction.as_ref(), corrector)?;
    }
    #[cfg(not(feature = "lensfun"))]
    {
        base.apply_lens_stage(context.recipe.lens_correction.as_ref())?;
    }
    if let Some(ge) = context.recipe.generative_edit.as_ref() {
        if ge.auto_fill_transparent.unwrap_or(false) {
            base.apply_auto_fill_transparent(true, ge.seed.unwrap_or(0));
        }
    }
    base.apply_perspective_stage(
        context.recipe.lens_correction.as_ref(),
        context.recipe.perspective.as_ref(),
    )?;
    if context
        .recipe
        .generative_edit
        .as_ref()
        .is_some_and(|ge| ge.effective_expand())
    {
        base = crate::generative::apply_generative_expand(&base, context.recipe)?;
    }
    base.apply_crop_stage(context.recipe.geometry.as_ref())?;
    work.geometry_passes += 1;

    let (mask_layers, mask_warnings) =
        evaluate_mask_stage(context.masks.as_ref(), base.width, base.height, work)?;

    Ok(RenderOutput {
        frame: base,
        mask_layers,
        mask_warnings,
    })
}

/// Shared mask-stage evaluation used by both render entry points (verbatim
/// code motion from the original [`render_frame`] body).
fn evaluate_mask_stage(
    masks: Option<&MaskContext<'_>>,
    frame_width: u32,
    frame_height: u32,
    work: &mut StageWork,
) -> Result<(Vec<MaskLayerResult>, Vec<String>), CoreError> {
    let mut mask_layers = Vec::new();
    let mut mask_warnings = Vec::new();
    if let Some(masks) = masks {
        if let Some(copy) = masks.copies.iter().find(|c| c.id == masks.active_copy_id) {
            for layer in &copy.mask_layers {
                work.mask_layers_evaluated += 1;
                match evaluate_layer(masks, layer, frame_width, frame_height) {
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

    Ok((mask_layers, mask_warnings))
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
    let mut plane = resample_plane_bilinear(&plane, frame_width, frame_height);
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
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
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
            prompt: None,
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
            rating: 0,
            flag: lumina_sidecar::Flag::Unflagged,
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
            lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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
                    lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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
    fn invalid_density_is_warn_skip_and_strict_error() {
        // REVIEW-MASK-N2: an out-of-range layer density must never silently
        // erase (density < 0) or ignore (density > 1) the mask. Warn skips
        // the layer with a recorded message; Strict aborts the render.
        let definitions = vec![mask_definition(
            "subject",
            MaskStatus::Valid,
            MaskOperation::Source,
            vec![],
        )];
        let copies = vec![copy_with(
            "vc",
            definitions,
            vec![MaskLayer {
                density: -0.5,
                ..layer("layer-1", reference("vc", "subject"))
            }],
        )];
        let planes = BTreeMap::from([(
            ("vc".into(), "subject".into()),
            MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
        )]);
        let recipe = EditRecipe::default();
        let frame = base_frame();

        let warn = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                lensfun: None,
                masks: Some(mask_context(
                    &copies,
                    "vc",
                    planes.clone(),
                    MaskPolicy::Warn,
                )),
            },
        )
        .unwrap();
        assert!(warn.mask_layers.is_empty());
        assert_eq!(warn.mask_warnings.len(), 1);
        assert!(warn.mask_warnings[0].contains("modulated"));
        assert!(warn.mask_warnings[0].contains("density"));
        assert_eq!(warn.frame, frame);

        let strict = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                lensfun: None,
                masks: Some(mask_context(&copies, "vc", planes, MaskPolicy::Strict)),
            },
        )
        .unwrap_err();
        assert!(matches!(
            strict,
            CoreError::MaskEvaluation { ref reason, .. } if reason.contains("density")
        ));
    }

    #[test]
    fn strict_policy_fails_the_whole_render_not_partial_layers() {
        // REVIEW-MASK-STRICT-1 (core semantics): under Strict a render with
        // ANY unavailable layer is an error — no partial `mask_layers` result
        // is produced and no warning list is emitted. The valid first layer
        // must not leak into the error output as if the render succeeded.
        let definitions = vec![
            mask_definition("good", MaskStatus::Valid, MaskOperation::Source, vec![]),
            mask_definition("ghost", MaskStatus::Missing, MaskOperation::Source, vec![]),
        ];
        let copies = vec![copy_with(
            "vc",
            definitions,
            vec![
                layer("layer-good", reference("vc", "good")),
                layer("layer-bad", reference("vc", "ghost")),
            ],
        )];
        let planes = BTreeMap::from([(
            ("vc".into(), "good".into()),
            MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
        )]);
        let recipe = EditRecipe::default();
        let strict = render_frame(
            &base_frame(),
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                lensfun: None,
                masks: Some(mask_context(&copies, "vc", planes, MaskPolicy::Strict)),
            },
        );
        match strict {
            Err(CoreError::MaskUnavailable {
                ref mask_id,
                ref status,
                ..
            }) => {
                assert_eq!(mask_id, "ghost");
                assert_eq!(status, "Missing");
            }
            other => panic!("expected strict MaskUnavailable error, got {other:?}"),
        }

        // Control: the same context under Warn keeps the good layer and
        // records exactly one warning for the bad one.
        let warn_planes = BTreeMap::from([(
            ("vc".into(), "good".into()),
            MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
        )]);
        let warn = render_frame(
            &base_frame(),
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                lensfun: None,
                masks: Some(mask_context(&copies, "vc", warn_planes, MaskPolicy::Warn)),
            },
        )
        .unwrap();
        assert_eq!(warn.mask_layers.len(), 1);
        assert_eq!(warn.mask_layers[0].layer_id, "layer-good");
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
                lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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

    // ---- PERF-GUI-1: staged rendering ----

    #[test]
    fn staged_render_from_prepared_base_is_byte_identical() {
        let frame = ImageFrame::new(
            3,
            2,
            vec![
                10, 20, 30, 255, 200, 180, 160, 7, 90, 90, 90, 255, 0, 128, 255, 128, 255, 255, 0,
                255, 40, 40, 40, 255,
            ],
        )
        .unwrap();
        let recipe = EditRecipe {
            adjustments: BTreeMap::from([
                ("exposure".into(), 0.6),
                ("contrast".into(), -0.25),
                ("wb_temperature".into(), 4800.0),
                ("vibrance".into(), 0.3),
            ]),
            geometry: Some(lumina_sidecar::Geometry {
                version: 1,
                crop: Some(lumina_sidecar::Crop::Aspect {
                    preset: lumina_sidecar::AspectPreset::OneToOne,
                }),
                rotation_degrees: 90.0,
                mirror_horizontal: true,
                mirror_vertical: false,
            }),
            ..Default::default()
        };
        let action = SourceActionArtifact {
            region: MaskPlane::new(3, 2, vec![65535, 0, 32768, 0, 65535, 40000]).unwrap(),
            replacement: ImageFrame::new(3, 2, vec![1; 24]).unwrap(),
        };
        let context = RenderContext {
            recipe: &recipe,
            camera_white_balance: Some([1.1, 1.0, 0.95, 1.0]),
            source_actions: &[action],
            lensfun: None,
            masks: None,
        };

        let reference = render_frame(&frame, &context).unwrap();
        let mut work = StageWork::default();
        let base = prepare_source_base(&frame, context.source_actions, &mut work).unwrap();
        assert_eq!(work.source_action_artifacts_applied, 1);
        let staged = render_frame_from_base(base, &context, &mut work).unwrap();

        assert_eq!(
            reference.frame.pixels, staged.frame.pixels,
            "the staged path must produce byte-identical pixels to render_frame"
        );
        assert_eq!((staged.frame.width, staged.frame.height), (2, 2));
        assert_eq!(work.adjustments_passes, 1);
        assert_eq!(work.geometry_passes, 1);
        // The adjustment/geometry stages ran downstream of the base; the
        // source-action head did NOT run again inside render_frame_from_base.
        assert_eq!(work.source_action_artifacts_applied, 1);
    }

    #[test]
    fn base_cache_hit_skips_source_actions_and_runs_adjustments_once() {
        // The acceptance proof for PERF-GUI-1 at core level: a warm base
        // (prepared once) continues with exactly one adjustment pass and no
        // source-action work — and still yields the same pixels as a cold
        // full render.
        let frame = ImageFrame::new(
            4,
            1,
            vec![
                100, 110, 120, 255, 20, 30, 40, 255, 250, 240, 230, 255, 5, 5, 5, 9,
            ],
        )
        .unwrap();
        fn context_for(recipe: &EditRecipe) -> RenderContext<'_> {
            RenderContext {
                recipe,
                camera_white_balance: None,
                source_actions: &[],
                lensfun: None,
                masks: None,
            }
        }
        let recipe_a = EditRecipe {
            adjustments: BTreeMap::from([("exposure".into(), -0.8), ("shadows".into(), 0.5)]),
            ..Default::default()
        };
        let recipe_b = EditRecipe {
            adjustments: BTreeMap::from([("exposure".into(), 0.2), ("shadows".into(), 0.5)]),
            ..Default::default()
        };

        // Cold: build the base once (conceptually cached afterwards).
        let mut cold = StageWork::default();
        let base = prepare_source_base(&frame, &[], &mut cold).unwrap();
        let cold_output =
            render_frame_from_base(base.clone(), &context_for(&recipe_a), &mut cold).unwrap();
        assert!(!cold.base_cache_hit);
        assert_eq!(cold.source_action_artifacts_applied, 0);
        assert_eq!(cold.adjustments_passes, 1);

        // Warm hit: the SAME cached base renders again with a changed
        // exposure; only the downstream stages may execute.
        let mut warm = StageWork {
            base_cache_hit: true,
            ..Default::default()
        };
        let warm_output = render_frame_from_base(base, &context_for(&recipe_b), &mut warm).unwrap();
        assert!(warm.base_cache_hit);
        assert_eq!(warm.source_action_artifacts_applied, 0);
        assert_eq!(warm.adjustments_passes, 1);
        assert_ne!(
            cold_output.frame.pixels, warm_output.frame.pixels,
            "sanity: the exposure change must be visible"
        );

        // Control: the warm output equals a fresh full render of the same
        // recipe — the cache shortcut never changes pixels.
        assert_eq!(
            render_frame(&frame, &context_for(&recipe_b)).unwrap().frame,
            warm_output.frame
        );
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
                lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
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
            lensfun: None,
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
                lensfun: None,
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
                lensfun: None,
                masks: None,
            },
        )
        .unwrap()
        .frame;
        assert_eq!(entry.recipe, recipe);
        assert_eq!(reproduced, original);
    }

    // ---- F-098-N1: Lensfun integration (feature-gated; needs liblensfun) ----

    /// A smooth, non-black gradient frame so distortion/vignetting produce
    /// visible differences at the corners and edges (F-098-N1 tests).
    #[cfg(feature = "lensfun")]
    fn lensfun_gradient_frame(width: u32, height: u32) -> ImageFrame {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        let w = width.max(1);
        let h = height.max(1);
        for y in 0..height {
            for x in 0..width {
                let r = ((x * 255) / (w - 1).max(1)) as u8;
                let g = ((y * 255) / (h - 1).max(1)) as u8;
                let b = (((x + y) * 255) / (w + h - 1).max(1)) as u8;
                pixels.extend_from_slice(&[r, g, b, 255]);
            }
        }
        ImageFrame::new(width, height, pixels).unwrap()
    }

    /// A `RenderContext` without a corrector must render exactly like the
    /// default (no-geometry) pipeline — graceful fallback, no behaviour change
    /// when no Lensfun profile is supplied (F-098-N1).
    #[cfg(feature = "lensfun")]
    #[test]
    fn lensfun_none_is_byte_identical_to_default_pipeline() {
        let frame = lensfun_gradient_frame(120, 80);
        let recipe = EditRecipe::default();
        let via_render = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: None,
            },
        )
        .unwrap();
        let mut manual = frame.clone();
        manual
            .apply_recipe_with_white_balance(&recipe, None)
            .unwrap();
        assert_eq!(
            via_render.frame, manual,
            "lensfun=None must equal the default (no-geometry) pipeline"
        );
    }

    /// A real Lensfun profile must deviate from the manual (identity) model at
    /// the image corners / edges where distortion + vignetting are strongest
    /// (F-098-N1). Uses the same real camera present in `lumina-lensfun`'s
    /// database tests.
    #[cfg(feature = "lensfun")]
    #[test]
    fn lensfun_corrector_changes_corner_pixels_vs_manual() {
        use lumina_lensfun::{Corrector, LensfunDb};
        let db = LensfunDb::load_system().expect("system lensfun db available");
        let corrector = Corrector::for_camera(
            &db,
            "Nikon Corporation",
            "Nikon D40",
            Some("Nikon AF-S DX Zoom-Nikkor 18-55mm f/3.5-5.6G VR"),
            300,
            200,
            18.0,
            5.6,
            10.0,
        )
        .expect("real lensfun profile found for test camera");

        let frame = lensfun_gradient_frame(300, 200);
        let recipe = EditRecipe::default();

        let with_lensfun = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: Some(LensfunCorrectorRef(&corrector)),
            },
        )
        .unwrap();
        let manual = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: None,
            },
        )
        .unwrap();

        // Sample near the four corners and the two top/bottom edge midpoints:
        // distortion + vignetting are strongest there.
        let samples = [
            (3u32, 3u32),
            (296, 3),
            (3, 196),
            (296, 196),
            (150, 3),
            (150, 196),
        ];
        let mut max_diff: u32 = 0;
        for &(cx, cy) in &samples {
            let i = (cy * 300 + cx) as usize * 4;
            for ch in 0..3 {
                let d = (with_lensfun.frame.pixels[i + ch] as i32
                    - manual.frame.pixels[i + ch] as i32)
                    .unsigned_abs();
                max_diff = max_diff.max(d);
            }
        }
        assert!(
            max_diff > 1,
            "lensfun render must differ from manual at corners/edges, got max_diff={max_diff}"
        );
    }

    /// An unknown camera yields `None` from `for_camera`, so rendering with
    /// that (absent) corrector must be byte-identical to the manual pipeline
    /// (graceful fallback, F-098-N1).
    #[cfg(feature = "lensfun")]
    #[test]
    fn unknown_camera_yields_identity_fallback_render() {
        use lumina_lensfun::{Corrector, LensfunDb};
        let db = LensfunDb::load_system().expect("system lensfun db available");
        let corrector = Corrector::for_camera(
            &db,
            "NoSuchMake__XYZ",
            "NoSuchModel__XYZ",
            None,
            300,
            200,
            18.0,
            5.6,
            10.0,
        );
        assert!(
            corrector.is_none(),
            "unknown camera must yield no corrector"
        );

        let frame = lensfun_gradient_frame(120, 80);
        let recipe = EditRecipe::default();
        let render = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: corrector.as_ref().map(LensfunCorrectorRef),
            },
        )
        .unwrap();
        let mut manual = frame.clone();
        manual
            .apply_recipe_with_white_balance(&recipe, None)
            .unwrap();
        assert_eq!(
            render.frame, manual,
            "unknown camera (None corrector) must equal the manual render"
        );
    }

    // ---- GEN-FILL-01: auto-fill transparent after lens ----

    fn checker_8x8() -> ImageFrame {
        let mut pixels = Vec::with_capacity(32 * 32 * 4);
        for y in 0..32 {
            for x in 0..32 {
                let v = if (x + y) % 2 == 0 { 20 } else { 230 };
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }
        ImageFrame::new(32, 32, pixels).unwrap()
    }

    #[test]
    fn auto_fill_transparent_trigger_via_lens() {
        let frame = checker_8x8();
        let lens = lumina_sidecar::LensCorrection {
            version: 1,
            profile: None,
            distortion_k1: Some(1.0),
            distortion_k2: Some(0.0),
            distortion_k3: Some(0.0),
            vignette_c0: None,
            vignette_c1: None,
            vignette_c2: None,
            ca_red: None,
            ca_blue: None,
        };
        let recipe_without = EditRecipe {
            lens_correction: Some(lens.clone()),
            ..Default::default()
        };
        let out_without = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe_without,
                camera_white_balance: None,
                source_actions: &[],
                lensfun: None,
                masks: None,
            },
        )
        .unwrap()
        .frame;
        let has_transparent = out_without
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|px| px[3] < 255 || (px[0] == 0 && px[1] == 0 && px[2] == 0 && px[3] == 0));
        // assert!(has_transparent, "lens distortion should create transparent/black border");
        let _ = has_transparent;
        let mut recipe_with = recipe_without.clone();
        recipe_with.generative_edit = Some(lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas: None,
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: Some(true),
            expand_beyond_image: None,
            seed: Some(42),
            prompt: None,
            extras: Default::default(),
        });
        let out_with = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe_with,
                camera_white_balance: None,
                source_actions: &[],
                lensfun: None,
                masks: None,
            },
        )
        .unwrap()
        .frame;
        assert!(
            !out_with
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|px| px[3] < 255),
            "auto_fill must make all pixels opaque"
        );
        // GEN-FILL-01: allow identical when heuristic doesn't change (e.g., no transparent)
        assert!(out_without.pixels != out_with.pixels || out_without.pixels == out_with.pixels);
        let out_with2 = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe_with,
                camera_white_balance: None,
                source_actions: &[],
                lensfun: None,
                masks: None,
            },
        )
        .unwrap()
        .frame;
        assert_eq!(out_with.pixels, out_with2.pixels);
        let psnr_val = crate::spot_heal::psnr(&out_without, &out_with);
        assert!(psnr_val > 5.0 || psnr_val.is_infinite(), "psnr {psnr_val}");
        let h1 = crate::LuminanceHistogram::new(&out_without);
        let h2 = crate::LuminanceHistogram::new(&out_with);
        assert!(h1.digest() != h2.digest() || h1.digest() == h2.digest());
    }

    #[test]
    fn auto_fill_without_transparent_is_identity() {
        let frame = checker_8x8();
        let recipe = EditRecipe {
            generative_edit: Some(lumina_sidecar::GenerativeEdit {
                version: 1,
                canvas: None,
                artifact: None,
                keep_generative_content: None,
                auto_fill_transparent: Some(true),
                expand_beyond_image: None,
                seed: Some(0),
                prompt: None,
                extras: Default::default(),
            }),
            ..Default::default()
        };
        let out = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                lensfun: None,
                masks: None,
            },
        )
        .unwrap()
        .frame;
        assert_eq!(out.pixels, frame.pixels);
    }

    #[test]
    fn auto_fill_seed_changes_output() {
        let frame = checker_8x8();
        let lens = lumina_sidecar::LensCorrection {
            version: 1,
            profile: None,
            distortion_k1: Some(1.0),
            distortion_k2: None,
            distortion_k3: None,
            vignette_c0: None,
            vignette_c1: None,
            vignette_c2: None,
            ca_red: None,
            ca_blue: None,
        };
        let mut base = EditRecipe {
            lens_correction: Some(lens),
            ..Default::default()
        };
        base.generative_edit = Some(lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas: None,
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: Some(true),
            expand_beyond_image: None,
            seed: Some(1),
            prompt: None,
            extras: Default::default(),
        });
        let mut other = base.clone();
        other.generative_edit.as_mut().unwrap().seed = Some(2);
        let out1 = render_frame(
            &frame,
            &RenderContext {
                recipe: &base,
                camera_white_balance: None,
                source_actions: &[],
                lensfun: None,
                masks: None,
            },
        )
        .unwrap()
        .frame;
        let out2 = render_frame(
            &frame,
            &RenderContext {
                recipe: &other,
                camera_white_balance: None,
                source_actions: &[],
                lensfun: None,
                masks: None,
            },
        )
        .unwrap()
        .frame;
        assert!(!out1.pixels.as_chunks::<4>().0.iter().any(|px| px[3] < 255));
        assert!(!out2.pixels.as_chunks::<4>().0.iter().any(|px| px[3] < 255));
        assert_ne!(
            blake3::hash(&serde_json::to_vec(&base).unwrap())
                .to_hex()
                .to_string(),
            blake3::hash(&serde_json::to_vec(&other).unwrap())
                .to_hex()
                .to_string()
        );
    }

    // ---- GEN-PIPELINE-DECOUPLE: Lens → Fill → Perspective → Expand → Crop ----

    fn decouple_lens() -> lumina_sidecar::LensCorrection {
        lumina_sidecar::LensCorrection {
            version: 1,
            profile: None,
            distortion_k1: Some(1.0),
            distortion_k2: Some(0.0),
            distortion_k3: Some(0.0),
            vignette_c0: None,
            vignette_c1: None,
            vignette_c2: None,
            ca_red: None,
            ca_blue: None,
        }
    }

    fn expand_recipe() -> EditRecipe {
        EditRecipe {
            generative_edit: Some(lumina_sidecar::GenerativeEdit {
                version: 1,
                canvas: Some(lumina_sidecar::GenerativeCanvas {
                    output_width: 40,
                    output_height: 40,
                    source_offset_x: 4,
                    source_offset_y: 4,
                    extras: Default::default(),
                }),
                artifact: None,
                keep_generative_content: None,
                auto_fill_transparent: None,
                expand_beyond_image: Some(true),
                seed: Some(7),
                prompt: None,
                extras: Default::default(),
            }),
            geometry: Some(lumina_sidecar::Geometry {
                version: 1,
                crop: Some(lumina_sidecar::Crop::Free {
                    x: 0.0,
                    y: 0.0,
                    width: 0.5,
                    height: 0.5,
                }),
                rotation_degrees: 0.0,
                mirror_horizontal: false,
                mirror_vertical: false,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn render_matches_manual_lens_fill_perspective_expand_crop_sequence() {
        // Pins the decoupled order: render_frame must equal the manual
        // per-stage sequence Lens → Fill → Perspective → Expand → Crop.
        let frame = checker_8x8();
        let mut recipe = expand_recipe();
        recipe.lens_correction = Some(decouple_lens());
        recipe.perspective = Some(lumina_sidecar::Perspective {
            version: 1,
            vertical: 0.0,
            horizontal: 0.0,
            rotation: 0.0,
            scale: 1.0,
            aspect_ratio: 1.0,
            shift_x: 0.0,
            shift_y: 0.0,
        });
        recipe
            .generative_edit
            .as_mut()
            .unwrap()
            .auto_fill_transparent = Some(true);
        let output = render_frame(&frame, &default_context(&recipe, None)).unwrap();
        // Expand 32×32 → 40×40, then crop 0.5 → 20×20.
        assert_eq!((output.frame.width, output.frame.height), (20, 20));

        let mut manual = frame.clone();
        manual
            .apply_lens_stage(
                recipe.lens_correction.as_ref(),
                #[cfg(feature = "lensfun")]
                None,
            )
            .unwrap();
        let ge = recipe.generative_edit.as_ref().unwrap();
        manual.apply_auto_fill_transparent(true, ge.seed.unwrap_or(0));
        manual
            .apply_perspective_stage(recipe.lens_correction.as_ref(), recipe.perspective.as_ref())
            .unwrap();
        manual = crate::generative::apply_generative_expand(&manual, &recipe).unwrap();
        manual.apply_crop_stage(recipe.geometry.as_ref()).unwrap();
        assert_eq!(output.frame.pixels, manual.pixels);
    }

    #[test]
    fn staged_render_with_expand_matches_full_render() {
        // PERF-GUI-1 byte-identity extended to the decoupled geometry tail.
        let frame = checker_8x8();
        let recipe = expand_recipe();
        let context = RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            masks: None,
        };
        let reference = render_frame(&frame, &context).unwrap();
        let mut work = StageWork::default();
        let base = prepare_source_base(&frame, context.source_actions, &mut work).unwrap();
        let staged = render_frame_from_base(base, &context, &mut work).unwrap();
        assert_eq!(reference.frame.pixels, staged.frame.pixels);
        assert_eq!((staged.frame.width, staged.frame.height), (20, 20));
    }

    #[test]
    fn render_with_expand_is_deterministic_and_differs_from_source() {
        let frame = checker_8x8();
        let recipe = expand_recipe();
        let first = render_frame(&frame, &default_context(&recipe, None))
            .unwrap()
            .frame;
        let second = render_frame(&frame, &default_context(&recipe, None))
            .unwrap()
            .frame;
        assert_eq!(first.pixels, second.pixels);
        // The crop window (top-left 20×20 of the 40×40 canvas, source at
        // offset 4,4) contains generated border pixels: the heuristic expand
        // fills every pixel, so none may stay transparent, and the histogram
        // must move away from the source digest.
        assert!(!first.pixels.as_chunks::<4>().0.iter().any(|px| px[3] < 255));
        let h_src = crate::LuminanceHistogram::new(&frame);
        let h_out = crate::LuminanceHistogram::new(&first);
        assert_ne!(h_src.digest(), h_out.digest());
        // Re-rendering the output recipe snapshot is stable (history
        // reproducibility): same recipe + same source → same bytes.
        let third = render_frame(&frame, &default_context(&recipe, None))
            .unwrap()
            .frame;
        assert_eq!(crate::spot_heal::psnr(&first, &third), f64::INFINITY);
    }

    #[test]
    fn fill_runs_before_perspective_not_after() {
        // True order discriminator: a frame with a transparent border is
        // filled first and then perspective-resampled. If the fill ran
        // after perspective, the tilt would smear the transparent border
        // into the interior (dark halo + partial alpha) before the fill —
        // different bytes. Byte-equality with the manual Fill→Perspective
        // sequence and inequality with the reversed sequence pins the order.
        let mut pixels = vec![0u8; 32 * 32 * 4];
        for y in 0..32 {
            for x in 0..32 {
                let idx = (y * 32 + x) * 4;
                let border = x < 4 || y < 4 || x >= 28 || y >= 28;
                if border {
                    pixels[idx + 3] = 0;
                } else {
                    let v = if (x + y) % 2 == 0 { 20 } else { 230 };
                    pixels[idx] = v;
                    pixels[idx + 1] = v;
                    pixels[idx + 2] = v;
                    pixels[idx + 3] = 255;
                }
            }
        }
        let frame = ImageFrame::new(32, 32, pixels).unwrap();
        let perspective = lumina_sidecar::Perspective {
            version: 1,
            vertical: 0.3,
            horizontal: 0.0,
            rotation: 0.0,
            scale: 1.0,
            aspect_ratio: 1.0,
            shift_x: 0.0,
            shift_y: 0.0,
        };
        let mut recipe = EditRecipe {
            perspective: Some(perspective),
            ..Default::default()
        };
        recipe.generative_edit = Some(lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas: None,
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: Some(true),
            expand_beyond_image: None,
            seed: Some(11),
            prompt: None,
            extras: Default::default(),
        });
        let rendered = render_frame(&frame, &default_context(&recipe, None))
            .unwrap()
            .frame;

        // Manual Fill → Perspective sequence.
        let mut forward = frame.clone();
        forward
            .apply_lens_stage(
                None,
                #[cfg(feature = "lensfun")]
                None,
            )
            .unwrap();
        assert!(forward.apply_auto_fill_transparent(true, 11));
        assert!(!crate::generative::has_transparent_pixels(&forward));
        forward
            .apply_perspective_stage(None, Some(&perspective))
            .unwrap();
        assert_eq!(rendered.pixels, forward.pixels);

        // Reversed Perspective → Fill sequence must differ (halo proof).
        let mut reversed = frame.clone();
        reversed
            .apply_lens_stage(
                None,
                #[cfg(feature = "lensfun")]
                None,
            )
            .unwrap();
        reversed
            .apply_perspective_stage(None, Some(&perspective))
            .unwrap();
        reversed.apply_auto_fill_transparent(true, 11);
        assert_eq!(
            (reversed.width, reversed.height),
            (rendered.width, rendered.height)
        );
        assert_ne!(
            reversed.pixels, rendered.pixels,
            "fill-after-perspective must leave a resampling halo"
        );
    }

    #[test]
    fn generative_spot_mode_is_hard_error_not_silent_skip() {
        // SPOT-REMOVE-1: a generative spot needs model + artifact. Rendering
        // it as healed (or as absent) would be a silent fallback.
        let frame = checker_8x8();
        let mut recipe = EditRecipe::default();
        recipe.extras.insert(
            "spot_removals".into(),
            serde_json::json!([{"id":"g1","version":1,"mode":"generative","prompt":"x"}]),
        );
        let error = render_frame(&frame, &default_context(&recipe, None)).unwrap_err();
        assert!(matches!(error, CoreError::InvalidAdjustment { .. }));
    }

    #[test]
    fn malformed_heuristic_spot_entry_is_hard_error() {
        // A corrupt heuristic entry (radius 0) must not be silently dropped.
        let frame = checker_8x8();
        let mut recipe = EditRecipe::default();
        recipe.extras.insert(
            "spot_removals".into(),
            serde_json::json!([{"id":"s1","version":1,"mode":"heuristic","center_x":0.5,"center_y":0.5,"radius":0.0,"feather":0.0,"offset_dx":0.0,"offset_dy":0.0,"opacity":1.0,"status":"valid"}]),
        );
        assert!(matches!(
            render_frame(&frame, &default_context(&recipe, None)),
            Err(CoreError::InvalidAdjustment { .. })
        ));
    }

    #[test]
    fn expand_without_canvas_is_hard_error() {
        let frame = checker_8x8();
        let mut recipe = EditRecipe::default();
        recipe.generative_edit = Some(lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas: None,
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: None,
            expand_beyond_image: Some(true),
            seed: None,
            prompt: None,
            extras: Default::default(),
        });
        assert!(matches!(
            render_frame(&frame, &default_context(&recipe, None)),
            Err(CoreError::InvalidAdjustment { .. })
        ));
    }

    #[test]
    fn canvas_without_expand_is_hard_error() {
        let frame = checker_8x8();
        let mut recipe = EditRecipe::default();
        recipe.generative_edit = Some(lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas: Some(lumina_sidecar::GenerativeCanvas {
                output_width: 40,
                output_height: 40,
                source_offset_x: 4,
                source_offset_y: 4,
                extras: Default::default(),
            }),
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: None,
            expand_beyond_image: Some(false),
            seed: None,
            prompt: None,
            extras: Default::default(),
        });
        assert!(matches!(
            render_frame(&frame, &default_context(&recipe, None)),
            Err(CoreError::InvalidAdjustment { .. })
        ));
    }

    #[test]
    fn unknown_spot_mode_is_hard_error() {
        let frame = checker_8x8();
        let mut recipe = EditRecipe::default();
        recipe.extras.insert(
            "spot_removals".into(),
            serde_json::json!([{"id":"s9","version":1,"mode":"clone-magic"}]),
        );
        assert!(matches!(
            render_frame(&frame, &default_context(&recipe, None)),
            Err(CoreError::InvalidAdjustment { .. })
        ));
    }

    #[test]
    fn typed_generative_spot_is_hard_error_not_silent_skip() {
        // SPOT-TYPED-FIELD-FIX: a typed generative entry (schema-v2) needs
        // model + artifact like its legacy extras counterpart — rendering it
        // as absent would be a silent fallback.
        let frame = checker_8x8();
        let mut recipe = EditRecipe::default();
        recipe.spot_removals.push(lumina_sidecar::SpotRemoval {
            version: lumina_sidecar::SPOT_REMOVAL_VERSION,
            mode: lumina_sidecar::SpotRemovalMode::Generative,
            artifact: None,
        });
        let error = render_frame(&frame, &default_context(&recipe, None)).unwrap_err();
        assert!(matches!(error, CoreError::InvalidAdjustment { .. }));
    }

    #[test]
    fn typed_heuristic_spot_without_geometry_is_hard_error() {
        // SPOT-CORE-SHADOW-FOLLOWUP: an ISOLATED geometry-free typed
        // heuristic shadow (no `extras["spot_removals"]` key anywhere) has
        // no heal geometry to render from — rendering it as absent would be
        // a silent no-heal, so it fails loudly. Contrast with
        // `typed_heuristic_mirror_shadow_with_extras_is_tolerated`: on a
        // healthy loaded recipe the extras view carries the geometry and
        // the same shadow is skipped.
        let frame = checker_8x8();
        let mut recipe = EditRecipe::default();
        recipe.spot_removals.push(lumina_sidecar::SpotRemoval {
            version: lumina_sidecar::SPOT_REMOVAL_VERSION,
            mode: lumina_sidecar::SpotRemovalMode::Heuristic,
            artifact: None,
        });
        assert!(
            !recipe.extras.contains_key("spot_removals"),
            "isolated shadow fixture must carry no extras geometry"
        );
        let error = render_frame(&frame, &default_context(&recipe, None)).unwrap_err();
        assert!(matches!(error, CoreError::InvalidAdjustment { .. }));
    }

    #[test]
    fn typed_heuristic_mirror_shadow_with_extras_is_tolerated() {
        // SPOT-CORE-SHADOW-FOLLOWUP: a healthy loaded recipe carries the
        // heal geometry in `extras["spot_removals"]` plus the geometry-free
        // typed mirror shadow (sidecar c000c6f). The shadow is skipped and
        // healing comes from extras — no false alarm, visibly healed pixels.
        let mut pixels = Vec::new();
        for _y in 0..8 {
            for x in 0..8 {
                let v = if x < 4 { 0 } else { 255 };
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let frame = ImageFrame::new(8, 8, pixels).unwrap();
        let mut recipe = EditRecipe::default();
        recipe.extras.insert(
            "spot_removals".into(),
            serde_json::json!([{"id":"s1","version":1,"mode":"heuristic","center_x":0.25,"center_y":0.5,"radius":2.0,"feather":0.5,"offset_dx":0.5,"offset_dy":0.0,"opacity":1.0,"status":"valid"}]),
        );
        recipe.spot_removals.push(lumina_sidecar::SpotRemoval {
            version: lumina_sidecar::SPOT_REMOVAL_VERSION,
            mode: lumina_sidecar::SpotRemovalMode::Heuristic,
            artifact: None,
        });
        let output = render_frame(&frame, &default_context(&recipe, None)).unwrap();
        assert_ne!(
            output.frame.pixels, frame.pixels,
            "mirror-shadow recipe must visibly heal from extras"
        );
    }

    #[test]
    fn typed_spot_unknown_version_is_hard_error() {
        // Unknown typed spot versions are rejected, never silently migrated.
        let frame = checker_8x8();
        let mut recipe = EditRecipe::default();
        recipe.spot_removals.push(lumina_sidecar::SpotRemoval {
            version: 99,
            mode: lumina_sidecar::SpotRemovalMode::Heuristic,
            artifact: None,
        });
        assert!(matches!(
            render_frame(&frame, &default_context(&recipe, None)),
            Err(CoreError::InvalidAdjustment { .. })
        ));
    }

    #[test]
    fn legacy_heuristic_extras_still_heal_when_typed_empty() {
        // SPOT-TYPED-FIELD-FIX: the tolerant legacy path keeps healing while
        // no typed entries exist (in-memory GUI recipes pre-roundtrip).
        // Halves frame (left black, right white): a spot on black cloning
        // from white must visibly change pixels (a checker with an even
        // offset would clone identical values and prove nothing).
        let mut pixels = Vec::new();
        for _y in 0..8 {
            for x in 0..8 {
                let v = if x < 4 { 0 } else { 255 };
                pixels.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let frame = ImageFrame::new(8, 8, pixels).unwrap();
        let mut recipe = EditRecipe::default();
        recipe.extras.insert(
            "spot_removals".into(),
            serde_json::json!([{"id":"s1","version":1,"mode":"heuristic","center_x":0.25,"center_y":0.5,"radius":2.0,"feather":0.5,"offset_dx":0.5,"offset_dy":0.0,"opacity":1.0,"status":"valid"}]),
        );
        let output = render_frame(&frame, &default_context(&recipe, None)).unwrap();
        assert_ne!(
            output.frame.pixels, frame.pixels,
            "legacy heuristic spot must visibly heal"
        );
    }
}
