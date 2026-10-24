#![allow(clippy::field_reassign_with_default)]
//! Shared render entry point (F-042).
//!
//! [`render_frame`] executes the documented pipeline order
//! `SourceActions → Adjustments (WB context before tonal values, including
//! geometry/crop) → Masks → Output` on an already decoded frame. Auto-tone
//! computation and Match Total Exposure remain recipe/caller orchestration
//! (F-041); this entry point applies the (possibly auto-toned) recipe. GUI and
//! CLI use the same entry point.

use crate::masks::MaskPlane;
use crate::{CoreError, ImageFrame};
use lumina_sidecar::{EditRecipe, VirtualCopy};
use std::collections::BTreeMap;

mod local_adjustments;
mod local_color;
mod local_detail;
mod local_presence;
mod local_tone;
mod local_wb;
pub mod mask_evaluation;
mod source_stage;
use mask_evaluation::{evaluate_layer, LayerFailure};
pub use source_stage::render_frame_from_base_with_source_stage;
use source_stage::EffectiveSourceStage;

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
    /// Normalized source-space zoom ROI `[x, y, width, height]` used by the
    /// interactive preview. `None` is the full source frame. The value is
    /// consumed by the explicit local-adjustment alignment; it is never used
    /// as an implicit wrong-coordinate resize fallback.
    pub source_roi: Option<[f32; 4]>,
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
    /// Optional G-05 external depth map for `recipe.lens_blur.depth_artifact`.
    /// `None` selects the deterministic focus-rect heuristic; a referenced
    /// artifact without a plane aborts the render loudly (no silent fallback).
    pub depth: Option<&'a crate::lens_blur::DepthPlane>,
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
    /// The post-global, post-geometry frame immediately before local mask
    /// recipes are composited, or `None` when the caller did not ask for it.
    ///
    /// MASK-LOCAL-P1.1: retaining this stage costs a full-frame `memcpy`, so it
    /// is **opt-in**. Only callers that sample from it (the GUI mask-local
    /// white-balance picker) request it through
    /// [`render_frame_from_base_with_source_stage`]; every other render entry
    /// point leaves it `None` instead of cloning a frame nobody reads. When
    /// present, GUI white-balance sampling uses this explicit effective source
    /// stage; it is never reconstructed from a final local preview or silently
    /// replaced by the raw decode.
    pub effective_source_stage: Option<ImageFrame>,
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
    render_frame_with_generative(
        frame,
        context,
        crate::generative::GenerativeCanvasInput::default(),
    )
}

/// GEN-ONNX-1: [`render_frame`] with caller-supplied, model-produced
/// generative canvas artifacts.
///
/// A generative edit (`expand_beyond_image` / `auto_fill_transparent`) is
/// rendered by *compositing* the supplied artifacts — never by the former
/// heuristic BFS. Without a matching artifact for an active generative edit the
/// render aborts loudly (see [`crate::generative::composite_expand`] /
/// [`crate::generative::composite_auto_fill`]).
pub fn render_frame_with_generative(
    frame: &ImageFrame,
    context: &RenderContext<'_>,
    generative: crate::generative::GenerativeCanvasInput<'_>,
) -> Result<RenderOutput, CoreError> {
    render_frame_with_generative_and_denoise(
        frame,
        context,
        generative,
        &crate::DenoiseStageInput::inactive(),
    )
}

/// LRPAR-G14-DENOISE-IMPL-20: [`render_frame`] with a caller-resolved
/// KI-Denoise stage (artifact + §6 status + fallback policy). `inactive` on an
/// active `denoise_ai` is a loud `unavailable` under the default `Strict`
/// policy — never a silent no-op.
pub fn render_frame_with_denoise(
    frame: &ImageFrame,
    context: &RenderContext<'_>,
    denoise: &crate::DenoiseStageInput<'_>,
) -> Result<RenderOutput, CoreError> {
    render_frame_with_generative_and_denoise(
        frame,
        context,
        crate::generative::GenerativeCanvasInput::default(),
        denoise,
    )
}

/// GEN-ONNX-1 + LRPAR-G14-DENOISE-IMPL-20: [`render_frame`] with both a
/// caller-supplied generative canvas and a caller-resolved KI-Denoise stage.
pub fn render_frame_with_generative_and_denoise(
    frame: &ImageFrame,
    context: &RenderContext<'_>,
    generative: crate::generative::GenerativeCanvasInput<'_>,
    denoise: &crate::DenoiseStageInput<'_>,
) -> Result<RenderOutput, CoreError> {
    let mut work = StageWork::default();
    let base = prepare_source_base(frame, context.source_actions, &mut work)?;
    render_frame_from_base_with_generative_and_denoise(
        base, context, &mut work, generative, denoise,
    )
}

/// GEN-ONNX-1: compute the frames entering the generative stage, mirroring the
/// decoupled geometry head of [`render_frame_from_base`] exactly.
///
/// Returns `(after_lens, after_perspective)`: the frame a `auto_fill_transparent`
/// artifact consumes and the frame an `expand_beyond_image` artifact consumes.
/// Callers (CLI/GUI) derive the exact generative operation identity from these
/// frames without duplicating pipeline logic; the function is I/O-free and
/// platform-neutral. The feature-uniform [`LensfunCorrectorRef`] parameter keeps
/// it usable from crates whose own `lensfun` feature may differ (Cargo feature
/// unification).
pub fn generative_input_frames(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    camera_white_balance: Option<[f32; 4]>,
    source_actions: &[SourceActionArtifact],
    lensfun: Option<LensfunCorrectorRef<'_>>,
) -> Result<(ImageFrame, ImageFrame), CoreError> {
    let mut work = StageWork::default();
    let mut staged = prepare_source_base(frame, source_actions, &mut work)?;
    apply_spot_heals_from_recipe(&mut staged, recipe)?;
    staged.apply_recipe_with_white_balance(recipe, camera_white_balance)?;
    #[cfg(feature = "lensfun")]
    let lensfun = lensfun.map(|LensfunCorrectorRef(corrector)| corrector);
    #[cfg(not(feature = "lensfun"))]
    let _ = lensfun;
    staged.apply_lens_stage(
        recipe.lens_correction.as_ref(),
        #[cfg(feature = "lensfun")]
        lensfun,
    )?;
    let after_lens = staged.clone();
    let perspective = recipe.effective_perspective();
    staged.apply_perspective_stage(
        recipe.lens_correction.as_ref(),
        perspective.as_ref(),
        #[cfg(feature = "lensfun")]
        lensfun,
    )?;
    Ok((after_lens, staged))
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
/// (`SpotRemoval` holds only id/version/mode/artifact) and are therefore
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
/// (`SpotRemoval` holds only id/version/mode/artifact). Rejecting that shadow
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
/// `SpotRemoval` currently serializes id/version/mode/artifact, so this is
/// always false today; if the schema ever gains geometry fields
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

/// CROP-MAXRECT-1: whether the crop stage should fall back to the
/// maximum-content default. Only an active lens/perspective correction can
/// introduce transparent wedges, so without one the crop default stays the
/// identity full frame — "no crop without a reason". Auto-fill / generative
/// expand may fill the wedges again; because the default is content-based a
/// fully filled frame yields the identity by itself, so no special case is
/// needed here.
fn default_crop_active(context: &RenderContext<'_>) -> bool {
    let lens = context.recipe.lens_correction.is_some();
    let perspective = context.recipe.effective_perspective().is_some();
    #[cfg(feature = "lensfun")]
    {
        lens || perspective || context.lensfun.is_some()
    }
    #[cfg(not(feature = "lensfun"))]
    {
        lens || perspective
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
    base: ImageFrame,
    context: &RenderContext<'_>,
    work: &mut StageWork,
) -> Result<RenderOutput, CoreError> {
    render_frame_from_base_with_generative_and_denoise(
        base,
        context,
        work,
        crate::generative::GenerativeCanvasInput::default(),
        &crate::DenoiseStageInput::inactive(),
    )
}

/// LRPAR-G14-DENOISE-IMPL-20: [`render_frame_from_base`] with a caller-resolved
/// KI-Denoise stage.
pub fn render_frame_from_base_with_denoise(
    base: ImageFrame,
    context: &RenderContext<'_>,
    work: &mut StageWork,
    denoise: &crate::DenoiseStageInput<'_>,
) -> Result<RenderOutput, CoreError> {
    render_frame_from_base_with_generative_and_denoise(
        base,
        context,
        work,
        crate::generative::GenerativeCanvasInput::default(),
        denoise,
    )
}

/// GEN-ONNX-1: [`render_frame_from_base`] with caller-supplied generative
/// canvas artifacts (see [`render_frame_with_generative`] for the contract).
pub fn render_frame_from_base_with_generative(
    base: ImageFrame,
    context: &RenderContext<'_>,
    work: &mut StageWork,
    generative: crate::generative::GenerativeCanvasInput<'_>,
) -> Result<RenderOutput, CoreError> {
    render_frame_from_base_with_generative_and_denoise(
        base,
        context,
        work,
        generative,
        &crate::DenoiseStageInput::inactive(),
    )
}

/// GEN-ONNX-1 + LRPAR-G14-DENOISE-IMPL-20: [`render_frame_from_base`] with both
/// a caller-supplied generative canvas and a caller-resolved KI-Denoise stage.
///
/// The KI-Denoise stage runs inside the `Adjustments` pass immediately before
/// the manual F-096 noise reduction (see
/// [`ImageFrame::apply_recipe_with_scale_white_balance_and_denoise`]).
pub fn render_frame_from_base_with_generative_and_denoise(
    base: ImageFrame,
    context: &RenderContext<'_>,
    work: &mut StageWork,
    generative: crate::generative::GenerativeCanvasInput<'_>,
    denoise: &crate::DenoiseStageInput<'_>,
) -> Result<RenderOutput, CoreError> {
    render_frame_from_base_impl(
        base,
        context,
        work,
        generative,
        denoise,
        EffectiveSourceStage::Skip,
    )
}

/// MASK-LOCAL-P1.1: the shared hub behind the staged entry points. It runs the
/// identical stage sequence in every case; the only difference is whether the
/// pre-local frame is cloned into [`RenderOutput::effective_source_stage`]
/// (see [`source_stage`]).
pub(super) fn render_frame_from_base_impl(
    mut base: ImageFrame,
    context: &RenderContext<'_>,
    work: &mut StageWork,
    generative: crate::generative::GenerativeCanvasInput<'_>,
    denoise: &crate::DenoiseStageInput<'_>,
    effective_source_stage: EffectiveSourceStage,
) -> Result<RenderOutput, CoreError> {
    // MASK-LOCAL-P0: validate every persisted local layer and the geometry
    // contract before *any* pixel mutation.  This is deliberately separate
    // from mask artifact policy: a bad range/version/conflict is never a
    // recoverable warning, and a local edit is never silently turned into a
    // global-only render.
    local_adjustments::prevalidate_local_adjustments(context)?;
    let local_adjustment_mode =
        local_adjustments::has_visible_local_adjustments(context.masks.as_ref());
    let mask_input_width = base.width;
    let mask_input_height = base.height;
    // MASK-LOCAL-P1.2d / F-096: one effective output scale for this whole
    // render. The global F-095 sharpening stage below and the mask-local detail
    // kernel in `apply_local_adjustments` are handed **the same value**, which is
    // the structural proof that the local detail block follows the global render
    // scale, has no scale option of its own, and never overrides it.
    let effective_scale = 1.0_f32;
    apply_spot_heals_from_recipe(&mut base, context.recipe)?;
    base.apply_recipe_with_scale_white_balance_and_denoise(
        context.recipe,
        effective_scale,
        context.camera_white_balance,
        denoise,
    )?;
    work.adjustments_passes += 1;

    // GEN-PIPELINE-DECOUPLE: decoupled geometry order
    // `Lens → Fill → Perspective → Expand → Crop → LensBlur(G-05)`
    // (see `crate::pipeline::GEOMETRY_STAGE_ORDER`). The legacy 5-in-1
    // `apply_geometry` is intentionally NOT used here: auto-fill must run
    // between lens and perspective (transparent wedges from undistortion are
    // filled before perspective resamples them), and the generative expand
    // must run before crop (crop coordinates reference the expanded canvas).
    //
    // GEN-ONNX-1: the generative stage is artifact compositing, not the former
    // heuristic BFS. It adopts the caller-supplied model-produced canvas
    // (`generative`) after validating the recipe geometry and the artifact
    // dimensions. A missing/mismatched artifact aborts the render with
    // `InvalidAdjustment` — never a silent unexpanded/„as if not generated"
    // render.
    #[cfg(feature = "lensfun")]
    {
        let corrector = context.lensfun.map(|LensfunCorrectorRef(c)| c);
        base.apply_lens_stage(context.recipe.lens_correction.as_ref(), corrector)?;
    }
    #[cfg(not(feature = "lensfun"))]
    {
        base.apply_lens_stage(context.recipe.lens_correction.as_ref())?;
    }
    if context
        .recipe
        .generative_edit
        .as_ref()
        .is_some_and(|ge| ge.auto_fill_transparent.unwrap_or(false))
    {
        base = crate::generative::composite_auto_fill(base, generative.auto_fill)?;
    }
    // GEN-PIPELINE-DECOUPLE position of the LRPAR-G06-UPRIGHT-15 stage: the
    // persisted upright analysis (when enabled) supplies the effective F-099
    // perspective here, immediately before the perspective sub-stage. The
    // manual `recipe.perspective` stays authoritative when upright is off.
    let perspective = context.recipe.effective_perspective();
    base.apply_perspective_stage(
        context.recipe.lens_correction.as_ref(),
        perspective.as_ref(),
        #[cfg(feature = "lensfun")]
        context.lensfun.map(|LensfunCorrectorRef(c)| c),
    )?;
    if context
        .recipe
        .generative_edit
        .as_ref()
        .is_some_and(|ge| ge.effective_expand())
    {
        base = crate::generative::composite_expand(base, context.recipe, generative.expand)?;
    }
    base.apply_crop_stage(
        context.recipe.geometry.as_ref(),
        default_crop_active(context),
    )?;
    work.geometry_passes += 1;

    // G-05 Lens Blur: sub-stage of `Crop`, after crop/rotation/mirroring and
    // before masks/output. A referenced depth artifact without a supplied
    // plane aborts here (never a silent heuristic render).
    if let Some(blur) = context.recipe.lens_blur.as_ref() {
        crate::lens_blur::apply_lens_blur(&mut base, blur, context.depth)?;
    }

    let (mask_layers, mask_warnings) = evaluate_mask_stage(
        context.masks.as_ref(),
        MaskStageGeometry {
            recipe: context.recipe,
            input_width: mask_input_width,
            input_height: mask_input_height,
            frame_width: base.width,
            frame_height: base.height,
            local_adjustment_mode,
        },
        work,
    )?;
    // MASK-LOCAL-P1.1: keep the exact post-global/geometry stage available to
    // an explicit sampler (the GUI local WB picker) only when the caller asked
    // for it. Every other path skips the full-frame copy entirely.
    let effective_source_stage = match effective_source_stage {
        EffectiveSourceStage::Skip => None,
        // This clone is taken before any local layer can mutate the working
        // frame.
        EffectiveSourceStage::Capture => Some(base.clone()),
    };
    local_adjustments::apply_local_adjustments(
        &mut base,
        context.masks.as_ref(),
        &mask_layers,
        local_adjustment_mode,
        effective_scale,
    )?;

    Ok(RenderOutput {
        frame: base,
        effective_source_stage,
        mask_layers,
        mask_warnings,
    })
}

/// Geometry and local-mode inputs shared by mask evaluation and alignment.
#[derive(Clone, Copy)]
struct MaskStageGeometry<'a> {
    recipe: &'a EditRecipe,
    input_width: u32,
    input_height: u32,
    frame_width: u32,
    frame_height: u32,
    local_adjustment_mode: bool,
}

/// Shared mask-stage evaluation used by both render entry points (verbatim
/// code motion from the original [`render_frame`] body).
fn evaluate_mask_stage(
    masks: Option<&MaskContext<'_>>,
    geometry: MaskStageGeometry<'_>,
    work: &mut StageWork,
) -> Result<(Vec<MaskLayerResult>, Vec<String>), CoreError> {
    let mut mask_layers = Vec::new();
    let mut mask_warnings = Vec::new();
    if let Some(masks) = masks {
        if let Some(copy) = masks.copies.iter().find(|c| c.id == masks.active_copy_id) {
            for layer in &copy.mask_layers {
                // G-03 visibility eye: an invisible layer is an explicit user
                // choice and is skipped without a warning (unlike missing or
                // corrupt mattes, which stay loud).
                if !layer.visible {
                    continue;
                }
                work.mask_layers_evaluated += 1;
                match evaluate_layer(masks, layer, geometry) {
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
mod tests;
