use lumina_sidecar::EditRecipe;
use thiserror::Error;

#[path = "execution_report.rs"]
mod execution_report;
pub use execution_report::{CpuFallbackReason, RenderExecutionReport};

/// Errors produced by the GPU path.
///
/// Every variant carries only `String`/core payloads so the error type is
/// available even when the `gpu` feature (and with it `wgpu`) is disabled.
#[derive(Debug, Error)]
pub enum GpuError {
    /// No suitable GPU adapter could be enumerated (e.g. missing Metal/Vulkan).
    #[error("GPU adapter unavailable: {0}")]
    AdapterUnavailable(String),
    /// An adapter was found but device/queue creation failed.
    #[error("GPU device unavailable: {0}")]
    DeviceUnavailable(String),
    /// The CPU fallback render failed.
    #[error("CPU fallback render failed: {0}")]
    Core(#[from] lumina_core::CoreError),
    /// The GPU color/tone pass failed (e.g. buffer map, encoder or readback).
    #[error("GPU render failed: {0}")]
    RenderFailed(String),
}

// ---------------------------------------------------------------------------
// Recipe-support validation (REVIEW-GPU-DIVERGENCE-1)
// ---------------------------------------------------------------------------

/// Adjustment keys the GPU adjustment pipeline actually implements.
///
/// The first eight are the exact key set of
/// `lumina-core::apply_channel_lut_adjustments` mirrored by the tone shader.
/// `vibrance`/`saturation` are applied by the [`stages`] per-pixel color pass
/// (GPU-RENDER-PARITY-1), so they are supported too — including a present but
/// neutral (`0.0`) value, which the CPU oracle still routes through its HSL
/// roundtrip.
pub(super) const GPU_SUPPORTED_ADJUSTMENT_KEYS: [&str; 10] = [
    "exposure",
    "contrast",
    "highlights",
    "shadows",
    "whites",
    "blacks",
    "wb_temperature",
    "wb_tint",
    "vibrance",
    "saturation",
];

/// Maximum number of source-action artifacts the GPU source-action stage can
/// composite in a single pass. WGSL cannot index texture bindings dynamically,
/// so the shader unrolls exactly this many slot pairs guarded by a uniform
/// count.
///
/// This is a **per-pass batch size**, not a recipe limit (GPU-RENDER-PARITY-1
/// item 7): [`GpuContext::render_with_gpu`]/[`GpuContext::render_to_vram`]
/// composite any number of bound artifacts in sequential batches of this size,
/// each sampling the previous batch's output. A recipe with `source_actions` is
/// only flagged when **no** matching artifacts are bound.
///
/// **Why 7:** wgpu's default `max_sampled_textures_per_shader_stage` limit is
/// 16. The stage needs 1 base texture + `2 × N` artifact textures; `N = 7`
/// stays within the default (15), so no non-default device limits are
/// required.
pub const MAX_SOURCE_ACTIONS: usize = 7;

/// Lists the recipe stages the GPU pipeline cannot render.
///
/// An empty result means the GPU path produces pixels within the per-stage
/// equivalence declared in `tests/parity.rs`: Point Color, Color Grading,
/// neutral vibrance/saturation, Presence Clarity/positive Dehaze, Effects
/// (vignette/grain), the legacy spot-heal geometry and the source-action
/// compositing are byte-identical; Curves, HSL, non-neutral
/// vibrance/saturation and Presence Texture/stacked Presence are bounded at
/// maxAbsDiff ≤ 1 (≤ 2 for a fully stacked recipe) with `PSNR ≥ 48 dB` and
/// `|mean signed error| ≤ 0.05`. A non-empty result means running the GPU path
/// would **silently drop** those stages and produce different pixels than every
/// CPU build — callers must route such renders to the CPU pipeline instead
/// (AGENTS.md: no silent fallbacks). Schema-invalid recipes are not "unsupported
/// stages": [`validate_gpu_recipe`] rejects them at the GPU entry with the CPU
/// oracle's own error.
///
/// Rendered by the GPU ([`stages`]/[`geometry`], `lens_blur`,
/// GPU-RENDER-PARITY-1) and therefore **not** flagged: Curves, HSL, Point
/// Color, vibrance/saturation, Color Grading, Presence (Texture / Clarity /
/// Dehaze), Noise Reduction, Sharpening, Effects (vignette + grain), Red-Eye,
/// the legacy spot-heal geometry, source-action compositing (batched), geometry
/// (crop / rotation / mirror), the manual lens correction
/// (distortion/vignette/CA), perspective, Lens Blur (heuristic **and**
/// external depth — the latter requires the caller to bind the depth plane) and
/// `generative_edit` since GEN-ONNX-1 Welle 2a (mid-geometry artifact
/// compositing via [`GpuContext::render_with_gpu_and_generative`]; an
/// artifact-blind render fails loudly in the geometry plan instead of routing
/// to the CPU).
///
/// Currently detected as unsupported:
/// - any adjustment key outside [`GPU_SUPPORTED_ADJUSTMENT_KEYS`] **at a
///   non-neutral value** (R2-GPU-05: sliders the GUI touched and reset store
///   their neutral default back into the recipe map; at that value the CPU
///   stage is pixel-identical to not having the key, so it must not block the
///   GPU route; keys outside the schema have no neutral value and always flag);
/// - an **active** `recipe.adjustments.denoise_ai` (`denoise_ai (not
///   GPU-wired)`, LRPAR-G14-DENOISE-IMPL-20): the KI-Denoise stage has no WGSL
///   pass yet, and the CPU oracle aborts an active stage loudly under the
///   default `Strict` policy. Routing to the CPU propagates that abort; an
///   identity/disabled/zero-strength stage is pixel-neutral and stays eligible;
/// - non-empty SourceActions **unless** matching GPU source-action artifacts are
///   bound (see [`unsupported_gpu_stages_with_context`]);
/// - an **invalid** `red_eye` (out-of-range/NaN/duplicate id): a schema-valid
///   correction is GPU-rendered, but invalid values stay CPU-routed so the
///   oracle's loud rejection is preserved;
/// - a **default content crop** (`geometry (default content crop)`): a lens or
///   perspective correction is active and `geometry.crop` is `None`, so the CPU
///   oracle would crop to the maximum-content rectangle before
///   rotation/mirroring. That rectangle depends on the post-resample alpha and
///   cannot be planned from the recipe alone (see the module docs); such
///   recipes route to the CPU loudly instead of rendering an uncropped,
///   wrong-sized frame. An explicit `geometry.crop` is authoritative and keeps
///   the recipe GPU-eligible.
///
/// This predicate sees only the recipe. Render-context state the GPU stage
/// cannot express from the recipe alone — mask layers, a caller-bound Lensfun
/// map ([`GpuContext::set_lensfun_map`]), an unbound external depth plane — is
/// the caller's responsibility ([`unsupported_gpu_stages_with_context`]), which
/// the routing mirrors in `lumina-cli`/`lumina-mcp` consult before entering the
/// GPU path. A decoder As-Shot white balance is GPU-carryable (see below), so a
/// valid one is no longer a reason.
pub fn unsupported_gpu_stages(recipe: &EditRecipe) -> Vec<String> {
    unsupported_gpu_stages_with_context(recipe, false, None)
}

/// [`unsupported_gpu_stages`] with explicit source-action awareness (GPU-STAGE-1).
///
/// Since the dedicated GPU source-action stage landed, a recipe with
/// `source_actions` is renderable on the GPU when the caller has bound matching
/// artifacts via `GpuContext::set_source_action_artifacts`
/// (`source_actions_bound = true`). With `false` (no/insufficient artifacts)
/// the stage would silently drop the compositing and is flagged exactly as
/// before — the CPU route keeps such renders pixel-safe.
pub fn unsupported_gpu_stages_for(recipe: &EditRecipe, source_actions_bound: bool) -> Vec<String> {
    unsupported_gpu_stages_with_context(recipe, source_actions_bound, None)
}

/// [`unsupported_gpu_stages_for`] plus the render-context features the GPU
/// pipeline cannot reproduce at all.
///
/// CAMERA-WB-WELLE (R2-MCP-01): a decoder As-Shot white balance
/// (`RenderContext::camera_white_balance`) is now an explicit GPU input the
/// caller binds via [`GpuContext::set_camera_white_balance`] (like the Lensfun
/// corrector / depth plane), so a **valid** context is GPU-eligible and the
/// former presence-based reason is gone. `lumina-core` derives its white
/// balance from the recipe keys and validates the As-Shot gains *before* that
/// derivation without ever re-applying them (the decoder already multiplied
/// them in), so the GPU tone stage matches exactly: it validates the bound
/// context and never adds the gains to its pixel math. An **invalid** context
/// (`inf`/`nan`/non-positive) stays flagged so a caller that does not bind it
/// still CPU-routes into the oracle's loud rejection rather than silently
/// ignoring it.
pub fn unsupported_gpu_stages_with_context(
    recipe: &EditRecipe,
    source_actions_bound: bool,
    camera_white_balance: Option<&[f32; 4]>,
) -> Vec<String> {
    let mut reasons = Vec::new();
    for (key, value) in recipe.adjustments.iter() {
        if GPU_SUPPORTED_ADJUSTMENT_KEYS.contains(&key.as_str()) {
            continue;
        }
        // R2-GPU-05: value neutrality. The GUI writes slider defaults back
        // into the map instead of removing keys ("touched once" must not mean
        // "GPU-forbidden forever"): at its neutral value the CPU stage leaves
        // every pixel unchanged, so skipping it drops nothing.
        if adjustment_neutral_value(key) == Some(*value) {
            continue;
        }
        reasons.push(format!("adjustment `{key}` not implemented on GPU"));
    }
    // GPU-RENDER-PARITY-1: curves, HSL, Point Color, Presence (Texture /
    // Clarity / Dehaze), vibrance/saturation and Color Grading are rendered by
    // the [`stages`] post-tone passes; stage 2 adds Noise Reduction, Sharpening
    // and Effects (vignette + grain) to that set — so none of them route to the
    // CPU anymore.
    // GPU-RENDER-PARITY-1 geometry wave: geometry (crop/rotation/mirror),
    // manual lens correction (distortion/vignette + CA) and perspective are
    // rendered by the [`geometry`] passes and no longer route to the CPU —
    // except when a lens/perspective correction would activate the
    // content-based default crop, which is handled below (GPU-MAXRECT-WELLE).
    // GPU-RENDER-PARITY-1 lens-blur wave: heuristic and external-depth lens
    // blur are rendered by the [`lens_blur`] pass, so the former `lens_blur`
    // reason is gone. An external `depth_artifact` is render-context state the
    // recipe-only gate cannot see (like the Lensfun corrector / As-Shot WB):
    // the caller binds the plane via [`GpuContext::set_depth_plane`], and a
    // referenced-but-unbound artifact is a loud render error, never a silent
    // heuristic fallback. `source_actions` without bound artifacts is the only
    // recipe-expressible stage that still stays CPU-routed.
    if !recipe.source_actions.is_empty() && !source_actions_bound {
        reasons.push("source_actions".to_string());
    }
    // GPU-RENDER-PARITY-1 follow-up (gate completeness): the CPU reference
    // applies/validates these recipe stages (`apply_spot_heals_from_recipe` and
    // the generative artifact compositing in `render_frame_from_base`). The
    // legacy `extras["spot_removals"]` heal geometry is now rendered by the
    // dedicated GPU spot stage and validated at the entry
    // ([`validate_gpu_recipe`]), so it no longer flags; a typed geometry-free
    // `spot_removals` mirror shadow is tolerated exactly like the CPU oracle,
    // and an isolated typed/generative entry is a hard error on **both**
    // backends.
    //
    // GEN-ONNX-1 Welle 2a: `generative_edit` is **no longer a CPU-routing
    // reason**. The GPU geometry chain injects the caller-supplied
    // `generative_canvas` at the oracle's mid-geometry positions
    // ([`GpuContext::render_with_gpu_and_generative`]). The recipe-only gate
    // cannot see the artifacts, so it does not flag the stage at all; a
    // recipe-only render (no artifacts) of an active generative edit fails
    // loudly in the geometry plan (`generative_artifact.missing`) instead of
    // silently rendering unexpanded — never a blanket CPU route.
    // GPU-RENDER-PARITY-1 stage 3: Red-Eye (G-14) is now rendered by the
    // dedicated [stages::RedEyeParams] pass (inserted after Sharpening, before
    // Effects), so a schema-valid correction no longer routes to the CPU. An
    // invalid correction (out-of-range/NaN/duplicate id) must keep routing
    // there: the CPU oracle rejects it loudly, and a GPU pass without that
    // validation would silently render divergent pixels instead of erroring.
    if let Some(red_eye) = recipe.red_eye.as_ref() {
        if !red_eye_is_valid(red_eye) {
            reasons.push("red_eye (invalid)".into());
        }
    }
    // LRPAR-G14-DENOISE-IMPL-20: the additive KI-Denoise stage
    // (`recipe.adjustments.denoise_ai`) is **not GPU-wired** yet. The CPU
    // oracle rejects an active stage loudly under the default
    // [`lumina_core::DenoisePolicy::Strict`] (`CoreError::Denoise`), so the GPU
    // gate must list it as an explicit CPU-routing reason: the routable
    // `render_with_gpu` path then reaches that same loud abort instead of
    // rendering pixel-equal to the source while silently dropping the stage.
    // An identity stage (`None`/`enabled:false`/`strength:0`) leaves every pixel
    // unchanged on both backends and is therefore GPU-eligible — exactly like a
    // neutral adjustment key above.
    if let Some(denoise) = recipe.denoise_ai.as_ref() {
        if !denoise.is_identity() {
            reasons.push("denoise_ai (not GPU-wired)".into());
        }
    }
    // GPU-RENDER-PARITY-1 follow-up (item 7): the former `MAX_SOURCE_ACTIONS`
    // slot-limit reason is gone. The source-action stage now composites in
    // batches of `MAX_SOURCE_ACTIONS` (ping-pong), so any number of bound
    // artifacts is GPU-eligible exactly like the CPU reference.
    // CAMERA-WB-WELLE (R2-MCP-01): a **valid** decoder As-Shot white balance is
    // no longer a routing reason. The GPU path carries the context explicitly
    // (the caller binds it via [`GpuContext::set_camera_white_balance`], like
    // the Lensfun corrector / depth plane) and reproduces the oracle's contract:
    // `lumina-core` validates those gains and does **not** re-apply them (the
    // decoder already did), so valid gains are pixel-neutral on both backends
    // and never enter the shader math. An **invalid** context stays flagged: a
    // caller that never binds it would otherwise let the shader silently ignore
    // non-finite/non-positive gains that abort the CPU reference.
    if let Some(gains) = camera_white_balance {
        if !camera_white_balance_gains_valid(gains) {
            reasons.push("camera_white_balance (invalid As-Shot gains)".into());
        }
    }
    // CROP-MAXRECT-1 / GPU-MAXRECT-WELLE: a lens/perspective correction without
    // an explicit crop activates the CPU oracle's content-based default crop
    // (`render.rs::default_crop_active`). Its rectangle is derived from the
    // resampled alpha channel, so the recipe-only GPU geometry plan cannot
    // reproduce its (possibly smaller) output dimensions — the transparent
    // wedge only exists after the resample. Route such recipes to the CPU
    // loudly rather than present an uncropped, differently-sized frame.
    if default_content_crop_active(recipe) {
        reasons.push("geometry (default content crop)".into());
    }
    reasons
}

/// Whether the CPU oracle would apply its content-based default crop for
/// `recipe` (`render.rs::default_crop_active` combined with
/// `apply_crop_stage`'s `geometry.crop` precedence): a lens or perspective
/// correction is present and the user set **no** explicit crop.
///
/// This mirrors the oracle's recipe-level trigger exactly (a present — even
/// neutral — correction counts, matching `default_crop_active`; the oracle
/// still runs `default_content_crop` on the resulting frame). The result of
/// that crop is pixel-dependent (the resampled alpha), so it is never predicted
/// here; the caller routes the whole recipe to the exact CPU reference instead.
pub(super) fn default_content_crop_active(recipe: &EditRecipe) -> bool {
    let correction_active =
        recipe.lens_correction.is_some() || recipe.effective_perspective().is_some();
    let explicit_crop = recipe
        .geometry
        .as_ref()
        .and_then(|geometry| geometry.crop.as_ref())
        .is_some();
    correction_active && !explicit_crop
}

/// Whether a `red_eye` correction is schema-valid per the CPU oracle's
/// `validate_nested_adjustments` (G-14). Invalid corrections keep CPU-routing so
/// the oracle's loud rejection is preserved; only valid ones are GPU-eligible.
pub(super) fn red_eye_is_valid(r: &lumina_sidecar::RedEyeCorrection) -> bool {
    if r.version != 1 || r.regions.len() > lumina_sidecar::RED_EYE_MAX_REGIONS {
        return false;
    }
    let mut seen = std::collections::HashSet::new();
    for region in &r.regions {
        if region.id.is_empty() || !seen.insert(region.id.as_str()) {
            return false;
        }
        for value in [region.x, region.y, region.desaturate, region.darken] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return false;
            }
        }
        if !region.radius.is_finite() || region.radius <= 0.0 || region.radius > 1.0 {
            return false;
        }
    }
    true
}

/// Whether decoder As-Shot gains are valid per the CPU oracle's check in
/// `apply_recipe_with_white_balance`: all four values finite and strictly
/// positive. Invalid gains abort the CPU render before any pixel mutation, so
/// the GPU gate must not let them slip through to a shader that has no notion
/// of them.
pub(super) fn camera_white_balance_gains_valid(gains: &[f32; 4]) -> bool {
    gains.iter().all(|gain| gain.is_finite() && *gain > 0.0)
}

/// [`camera_white_balance_gains_valid`] with the oracle's exact
/// [`lumina_core::CoreError::InvalidAdjustment`] for the first offending gain
/// (same iteration order, same bounds: `f32::MIN_POSITIVE..=f64::MAX`).
pub(super) fn validate_camera_white_balance_gains(gains: &[f32; 4]) -> Result<(), GpuError> {
    for gain in gains {
        if !gain.is_finite() || *gain <= 0.0 {
            return Err(GpuError::Core(lumina_core::CoreError::InvalidAdjustment {
                name: "camera_white_balance".into(),
                value: f64::from(*gain),
                minimum: f32::MIN_POSITIVE as f64,
                maximum: f64::MAX,
            }));
        }
    }
    Ok(())
}

/// The identity ("no visible change") value of an adjustment key.
///
/// Every slider is centered at `0.0`; the temperature slider's identity point
/// is the reference temperature 6500 K, from which the CPU derives exactly
/// `[1.0, 1.0, 1.0]` channel gains (`wb_gains` derivation in
/// `lumina-core::apply_recipe…`). Keys outside the recipe schema have no
/// neutral value ([`None`]) and are therefore always flagged — the CPU
/// pipeline rejects them outright, so no value could ever make them safe to
/// drop on the GPU.
///
/// Note: `wb_temperature` is currently GPU-supported
/// ([`GPU_SUPPORTED_ADJUSTMENT_KEYS`]), so its entry is unreachable through
/// the gate today; it documents the correct neutral so future key-set changes
/// cannot silently reintroduce the R2-GPU-05 value-blindness for it.
pub(super) fn adjustment_neutral_value(key: &str) -> Option<f64> {
    match key {
        "wb_temperature" => Some(6500.0),
        "exposure" | "contrast" | "highlights" | "shadows" | "whites" | "blacks" | "wb_tint"
        | "vibrance" | "saturation" => Some(0.0),
        _ => None,
    }
}
