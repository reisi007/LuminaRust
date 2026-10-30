//! GPU-first rendering path for Lumina.
//!
//! `lumina-gpu` is the native, GPU-accelerated sibling of the platform-neutral
//! `lumina-core` CPU pipeline. It owns the wgpu context (Metal on Apple
//! Silicon, Vulkan/DX12 elsewhere) and — once the parallel shader/tiling
//! subagents land — the GPU compute/render DAG for decode, color/tone, LUT and
//! tiling stages.
//!
//! **Bootstrap scope.** This crate currently exposes the [`GpuContext`] handle
//! and the adapter/device init. [`GpuContext::render_with_gpu`] runs the real
//! color/tone fragment shader (`SHADER_SRC`) when a GPU adapter is bound, and
//! when no adapter is present (or the `gpu` feature is disabled) falls back to
//! the complete `lumina-core` CPU reference (`render_cpu`; see the fallback note
//! below). The shader mirrors the
//! integer-rounded per-channel math of `lumina-core::apply_channel_lut_adjustments`,
//! so the tone stage matches the CPU oracle within the golden-image tolerance
//! (maxAbsDiff ≤ 1, PSNR ≥ 45 dB; `tests/golden.rs`). The post-tone stages are
//! pinned separately by `tests/parity.rs` (below).
//!
//! **Adjustment stages (GPU-RENDER-PARITY-1).** After the tone/WB pass the
//! pipeline runs the [`stages`] post-tone chain in the CPU oracle's order:
//! Presence (Texture/Clarity box DoG + Dehaze), the per-pixel color pass
//! (Curves → HSL → Point Color → vibrance/saturation → Color Grading), then the
//! stage-2 detail chain (Noise Reduction → Sharpening → Red-Eye → vignette →
//! grain).
//!
//! The equivalence is **per stage**, asserted by `tests/parity.rs` at the
//! bound each recipe enforces: Point Color, Color Grading,
//! neutral vibrance/saturation, Presence Clarity and positive Dehaze,
//! vignette and grain are byte-identical to the oracle (measured 0 on Metal);
//! Curves, HSL, non-neutral vibrance/saturation,
//! Presence Texture, Noise Reduction, Sharpening, a vignette+grain stack and
//! the stacked detail/color recipes are bounded at
//! maxAbsDiff ≤ 1 (≤ 2 for the fully stacked recipes) with a mean signed
//! error ≤ 0.05 (no systematic bias). The residual comes from the oracle
//! evaluating the curves ratio in `f64` (the GPU is `f32`), from `exp`/FMA
//! rounding in the bilateral/Gaussian kernels, and from the Metal backend
//! rounding one ulp differently at a `round()` tie.
//!
//! **No silent divergence (REVIEW-GPU-DIVERGENCE-1).** [`unsupported_gpu_stages`]
//! lists any recipe stage the pipeline cannot yet render (unbound
//! SourceActions, invalid red-eye, non-schema adjustment
//! keys, …) and
//! [`validate_gpu_recipe`] rejects every schema-invalid recipe with the CPU
//! oracle's own error. On every entry point the outcome is loud and pixel-safe:
//!
//! - [`GpuContext::render_with_gpu`] routes such a recipe to the free
//!   `render_cpu`, which runs the **full** `lumina_core::render_frame` chain
//!   (spot healing, all adjustments, geometry, generative artifact
//!   compositing), so the
//!   CPU fallback is the complete reference — not a partial `apply_recipe`.
//! - [`GpuContext::render_to_vram`] cannot CPU-route without a readback, so it
//!   **refuses** the recipe with [`GpuError::RenderFailed`] rather than writing
//!   divergent pixels into VRAM; the caller falls back to the CPU reference.
//!
//! Both log once per reason set, and a stage is either fully parity-tested or
//! reported here — there is no third state. Context the recipe-only API does not
//! carry (mask layers, Lensfun correctors, depth planes) remains the caller's
//! responsibility ([`unsupported_gpu_stages_with_context`]); the decoder
//! As-Shot white balance is now an explicit caller-bound GPU input
//! ([`GpuContext::set_camera_white_balance`]) and therefore no longer a routing
//! reason for valid gains.
//!
//! **GPU-RENDER-PARITY-1 follow-up.** Two former CPU-only classes are gone:
//! the legacy `extras["spot_removals"]` heal geometry is rendered by the
//! [`stages`] spot-heal pass (with the typed geometry-free shadow tolerated and
//! an isolated typed entry a hard error on both backends), and the
//! source-action stage composites **any** number of bound artifacts in batches
//! of `MAX_SOURCE_ACTIONS`.
//!
//! **CAMERA-WB-WELLE (R2-MCP-01).** The decoder's As-Shot white balance
//! (`RenderContext::camera_white_balance`) is carried as an explicit,
//! caller-bound GPU input like the Lensfun corrector / depth plane: the caller
//! binds the gains via [`GpuContext::set_camera_white_balance`] and both GPU
//! entry points validate them with the oracle's own error. The gains are never
//! re-applied in the shader — matching `lumina_core`, which validates them
//! before deriving the recipe white balance but leaves the already-decoder-
//! applied frame untouched — so a valid As-Shot context is pixel-neutral and
//! the former presence-based CPU-routing reason is gone. An invalid context
//! (`inf`/`nan`/non-positive) is still flagged so unbound callers keep the
//! oracle's loud rejection.
//!
//! **GPU-RENDER-PARITY-1 geometry wave.** Geometry (crop → rotation → mirror),
//! the manual lens correction (distortion + vignette + CA) and perspective are
//! rendered by the [`geometry`] passes in the CPU oracle's order
//! (`lens → perspective → CA → crop → rotation → mirror`) and reproduce the
//! oracle's **output dimensions** exactly. A strictly matched Lensfun corrector
//! is rendered by the [`lensfun`] resample pass over a caller-bound
//! [`lumina_core::LensfunMap`] (CPU-precomputed warp/gain map; see
//! [`GpuContext::set_lensfun_map`]), so a corrector recipe is no longer a
//! blanket CPU route. The readback-free VRAM present texture is source-sized,
//! so [`GpuContext::render_to_vram`] renders dimension-**preserving** geometry
//! (lens/Lensfun, identity crop/rotation) into the resident output and refuses a
//! dimension-changing chain loudly (the caller uses the exact CPU present path).
//!
//! **GPU-MAXRECT-WELLE (CROP-MAXRECT-1).** When a lens or perspective
//! correction is active and no explicit crop is set, the CPU oracle applies its
//! content-based **maximum-content-rect default crop**
//! ([`lumina_core::maximum_content_rect`]) *before* rotation/mirroring. The
//! resulting rectangle depends on the resampled alpha channel (which output
//! pixels fall outside the source), so the recipe-only [`geometry`] plan cannot
//! predict it up front: the transparent wedge of a keystone/pincushion
//! correction (and the frame shrink it implies) is only known after the
//! resample. Such recipes are therefore **CPU-routed loudly** through
//! [`unsupported_gpu_stages_with_context`] (`geometry (default content crop)`)
//! and refused by [`GpuContext::render_to_vram`] — never rendered with silently
//! different dimensions/pixels. An explicit `geometry.crop` is always
//! authoritative and keeps the recipe GPU-eligible.
//!
//! **GPU-RENDER-PARITY-1 lens-blur wave.** G-05 Lens Blur runs as the
//! sub-stage of Crop (after geometry, before masks/output) in the `lens_blur`
//! pass: the deterministic focus-rect heuristic or, when
//! `recipe.lens_blur.depth_artifact` is set, a caller-supplied
//! [`lumina_core::DepthPlane`] bound via [`GpuContext::set_depth_plane`]. Lens
//! blur preserves the frame dimensions, so [`GpuContext::render_to_vram`]
//! renders it into the resident output exactly like the CPU present path. A
//! referenced depth artifact without a matching bound plane is a **loud
//! error** (the oracle's missing-artifact abort), never a silent heuristic
//! fallback.
//!
//! [`unsupported_gpu_stages_with_context`] extends that verdict with the
//! render-context features the routing mirrors (`lumina-cli`, `lumina-mcp`)
//! must honor — an unbound/invalid As-Shot white balance (R2-MCP-01) and mask
//! layers. A caller-bound Lensfun map is GPU-carried
//! ([`GpuContext::set_lensfun_map`]) and is only refused when it is
//! dimension-mismatched or would need the CPU content default crop.
//!
//! The public API is therefore stable and always
//! returns a [`Frame`], which keeps the CPU and GPU return types identical for
//! callers.
//!
//! See `docs/gpu-bootstrap.md` for the planned DAG and `docs/gpu-shaders.md` for
//! the shader-stage design.

use lumina_core::masks::MaskPlane;
#[cfg(feature = "gpu")]
use lumina_core::render::SourceActionArtifact;
use lumina_core::ImageFrame;
use lumina_sidecar::EditRecipe;

// Full schema validation at the GPU entry (GPU-RENDER-PARITY-1 follow-up).
// Feature-independent: it only depends on `lumina-core`/`lumina-sidecar`, so a
// pure-CPU (`--no-default-features`) build can validate too.
mod routing;
mod validate;
#[cfg(test)]
pub(crate) use routing::adjustment_neutral_value;
use routing::validate_camera_white_balance_gains;
pub use routing::{
    unsupported_gpu_stages, unsupported_gpu_stages_for, unsupported_gpu_stages_with_context,
    CpuFallbackReason, GpuError, RenderExecutionReport, MAX_SOURCE_ACTIONS,
};

#[cfg(feature = "gpu")]
mod local_mask_gpu;
pub mod local_mask_pass;

// Shader + tiling modules are scaffolded (empty) so parallel subagents can fill
// them in without touching this file. They are GPU-specific, hence gated.
#[cfg(feature = "gpu")]
pub mod shaders;
// WGSL source of the base color/tone pass, extracted from this file
// (file-size ratchet §8, SPLICE-GPU-LIB-91). The pass wiring stays here.
#[cfg(feature = "gpu")]
mod color_tone_shader;
// GPU post-tone adjustment stages (GPU-RENDER-PARITY-1): per-pixel color
// (curves/HSL/Point Color/vibrance/saturation/Color Grading) and the
// neighborhood Presence stages (Texture/Clarity DoG + Dehaze).
#[cfg(feature = "gpu")]
pub mod stages;
// GPU geometry stages (GPU-RENDER-PARITY-1, geometry wave): lens correction
// (distortion/vignette + CA), perspective (homography) and crop/rotation/mirror
// — including the dimension-changing output the oracle produces.
#[cfg(feature = "gpu")]
mod geometry;
// WGSL sources/uniform blocks for [`geometry`] (extracted, file-size ratchet §8).
#[cfg(feature = "gpu")]
mod geometry_shaders;
// GPU G-05 lens-blur pass (GPU-RENDER-PARITY-1, lens-blur wave): deterministic
// depth bokeh (focus-rect heuristic or a caller-supplied external depth plane)
// as the sub-stage of Crop, after geometry and before masks/output.
#[cfg(feature = "gpu")]
mod lens_blur;
// GPU-LENSFUN-PARITY-1: resample pass over a caller-bound, CPU-precomputed
// `lumina_core::LensfunMap` (warp coordinates + vignetting gains) for the strict
// Lensfun corrector. No WGSL re-implementation of the Lensfun models.
#[cfg(feature = "gpu")]
mod lensfun;
// GPU encoding/readback helpers extracted from this file (file-size ratchet §8).
#[cfg(feature = "gpu")]
mod gpu_util;
// Mask texture upload/present/readback methods extracted from this file.
#[cfg(feature = "gpu")]
mod mask_io;
#[cfg(feature = "gpu")]
mod vram_pool;
#[cfg(all(test, feature = "gpu"))]
use vram_pool::PoolCore;
#[cfg(feature = "gpu")]
use vram_pool::{DepthPlaneGpu, VramPool};
#[cfg(feature = "gpu")]
mod post_stages;
#[cfg(feature = "gpu")]
mod sharpening_encode;

// Render dispatch, the VRAM present path, the caller-bound render state and the
// pipeline/backend plumbing live in child modules (file-size ratchet).
#[cfg(not(feature = "gpu"))]
mod render_cpu_stub;
#[cfg(feature = "gpu")]
mod render_dispatch;
#[cfg(feature = "gpu")]
mod render_draft;
#[cfg(feature = "gpu")]
mod render_vram;
#[cfg(feature = "gpu")]
mod resources;
#[cfg(feature = "gpu")]
mod state;
#[cfg(feature = "gpu")]
pub mod tiling;
#[cfg(feature = "gpu")]
use gpu_util::{
    encode_fullscreen_pass, encode_geometry_pass, readback_gradient_max, readback_texture,
};

/// A rendered frame.
///
/// For the bootstrap this is a CPU-owned RGBA8 buffer with the same row-major
/// layout as [`ImageFrame`]. Once the GPU pipeline lands it may additionally
/// carry a VRAM handle, but keeping the CPU buffer as the canonical output
/// means the fallback and GPU paths share one return type and callers need no
/// special-casing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA8 bytes, four bytes per pixel (same layout as [`ImageFrame`]).
    pub pixels: Vec<u8>,
}

impl Frame {
    /// Build a [`Frame`] from a core [`ImageFrame`] (no copy of pixel semantics;
    /// the buffer is moved).
    pub fn from_image_frame(frame: ImageFrame) -> Self {
        let width = frame.width();
        let height = frame.height();
        let pixels = frame.into_pixels();
        Self {
            width,
            height,
            pixels,
        }
    }

    /// Convert back into a core [`ImageFrame`].
    ///
    /// `Frame` is always constructed from a valid `ImageFrame`, so the pixel
    /// buffer length matches `width * height * 4` and this cannot fail.
    pub fn to_image_frame(self) -> ImageFrame {
        ImageFrame::new(self.width, self.height, self.pixels)
            .expect("Frame pixels always match width*height*4")
    }
}

/// Recipes the GPU adjustment pipeline would render with silently clamped
/// values are rejected here up front (GPU-RENDER-PARITY-1 stage-2 follow-up).
///
/// Superseded by the full [`validate::validate_gpu_recipe`] port (GPU-RENDER-
/// PARITY-1 follow-up, item 8): every schema range the CPU reference validates —
/// top-level adjustment keys, nested curve/HSL/point-color/presence/grading,
/// noise reduction, sharpening (all four fields), red-eye, effects, lens,
/// perspective, geometry, lens blur, generative edit and the spot modes — is now
/// checked at the GPU entry and returns the **same** [`lumina_core::CoreError`]
/// the oracle produces. The old sharpening-radius-only check is kept as a
/// documented alias so external callers keep compiling.
pub use validate::validate_gpu_recipe;

/// Combines evaluated mask-layer planes into one effective coverage plane
/// (GPU-STAGE-1).
///
/// Semantics mirror the F-041/F-043 measurement weights exactly: each pixel of
/// the combined plane is `∏_layer (plane[pixel]) / u16::MAX`, rounded back to
/// the `u16` domain — an intersection product where a fully-masked-out pixel
/// (`0`) in any layer kills the coverage. An all-`u16::MAX` set of layers
/// yields the identity plane.
///
/// Errors carry the offending layer index/dimensions instead of silently
/// resampling or cropping (`AGENTS.md`: no silent fallbacks). An empty slice
/// is `Ok(None)` — "no effective mask" is a valid state, distinct from an
/// error.
pub fn combine_mask_planes(planes: &[MaskPlane]) -> Result<Option<MaskPlane>, String> {
    let Some(first) = planes.first() else {
        return Ok(None);
    };
    let (width, height) = (first.width, first.height);
    for (index, plane) in planes.iter().enumerate().skip(1) {
        if plane.width != width || plane.height != height {
            return Err(format!(
                "mask plane {index} ({}x{}) does not match plane 0 ({width}x{height})",
                plane.width, plane.height
            ));
        }
    }
    const MAX_F: f32 = u16::MAX as f32;
    let mut values = Vec::with_capacity(width as usize * height as usize);
    for i in 0..(width as usize * height as usize) {
        let mut weight = 1.0f32;
        for plane in planes {
            weight *= plane.values[i] as f32 / MAX_F;
        }
        values.push((weight * MAX_F).round().clamp(0.0, MAX_F) as u16);
    }
    Ok(Some(MaskPlane {
        width,
        height,
        values,
    }))
}

/// Logs a CPU-routing decision once per unique reason set (not per frame).
///
/// Keyed on the joined reasons so different recipes with the same unsupported
/// stages log only once, while genuinely new divergences stay visible. Public
/// so embedders (CLI/MCP routing layers) report their context-level reasons
/// through the same deduplicated channel.
pub fn log_cpu_routing_once(reasons: &[String], context: &str) {
    use std::collections::BTreeSet;
    use std::sync::Mutex;
    static LOGGED: Mutex<Option<BTreeSet<String>>> = Mutex::new(None);
    let key = reasons.join("; ");
    let mut guard = LOGGED.lock().unwrap();
    if guard
        .get_or_insert_with(BTreeSet::new)
        .insert(format!("{context}: {key}"))
    {
        log::info!(
            "render backend: cpu (recipe uses GPU-unsupported stage(s): {key}); \
             routed to the CPU pipeline to keep pixels identical"
        );
    }
}

/// Warns once per unique reason set that the VRAM interactive path refuses
/// a recipe with stages the GPU adjustment pipeline (tone + [`stages`]) does
/// not implement. Refusal happens before any VRAM write, so no divergent
/// pixels are ever presented; the caller falls back to the CPU reference.
#[cfg(feature = "gpu")]
fn warn_unsupported_vram_once(reasons: &[String]) {
    use std::collections::BTreeSet;
    use std::sync::Mutex;
    static WARNED: Mutex<Option<BTreeSet<String>>> = Mutex::new(None);
    let key = reasons.join("; ");
    let mut guard = WARNED.lock().unwrap();
    if guard.get_or_insert_with(BTreeSet::new).insert(key.clone()) {
        log::warn!(
            "GPU VRAM preview refuses recipes with GPU-unsupported stage(s): \
             {key}. No VRAM pixels are written; the caller falls back to \
             the CPU reference."
        );
    }
}

/// Warns once per reason that the readback-free VRAM present path refuses a
/// geometry chain whose output dimensions differ from the source (the present
/// texture is source-sized). No VRAM pixels are written; the caller falls back
/// to the exact CPU present path (GPU-RENDER-PARITY-1 geometry wave).
#[cfg(feature = "gpu")]
fn warn_vram_dimension_change_once(reason: &str) {
    use std::collections::BTreeSet;
    use std::sync::Mutex;
    static WARNED: Mutex<Option<BTreeSet<String>>> = Mutex::new(None);
    let mut guard = WARNED.lock().unwrap();
    if guard
        .get_or_insert_with(BTreeSet::new)
        .insert(reason.to_string())
    {
        log::warn!(
            "GPU VRAM preview cannot present {reason}. No VRAM pixels are \
             written; the caller falls back to the exact CPU reference."
        );
    }
}

/// Logs the GPU init failure loudly once per failure text (REVIEW-GPU-N1).
///
/// Extracted from [`GpuContext::new`] so the "no silent fallback" contract has
/// a directly testable seam: a regression test installs a capturing logger and
/// asserts this warning is emitted when an adapter/device error degrades the
/// context to CPU rendering.
pub fn log_gpu_init_failure(err: &GpuError) {
    log::warn!("GPU initialization failed, falling back to CPU rendering: {err}");
}

/// A live GPU rendering context.
///
/// Construct with [`GpuContext::new`]. Use [`GpuContext::is_available`] to learn
/// whether a real adapter/device is bound; if not, [`GpuContext::render_with_gpu`]
/// falls back to the complete CPU reference (`render_cpu`). The context is
/// cheap to keep around and reuse across frames.
pub struct GpuContext {
    /// Bound GPU resources. `None` means "no adapter → CPU fallback only".
    #[cfg(feature = "gpu")]
    resources: Option<GpuResources>,
    /// Compiled render pipeline + uniform buffer. Built lazily (once) on the
    /// first GPU render via [`GpuContext::ensure_pipeline`]; `None` until then
    /// (or when no adapter). Wrapped in a `Mutex` so the GPU path can build it
    /// lazily from `render_with_gpu(&self)` without requiring `&mut self`.
    #[cfg(feature = "gpu")]
    pipeline: std::sync::Mutex<Option<PipelineState>>,
    /// Compiled source-action stage pipeline (GPU-STAGE-1). Built lazily on the
    /// first render that runs with bound artifacts; `None` otherwise.
    #[cfg(feature = "gpu")]
    sa_pipeline: std::sync::Mutex<Option<SourceActionPipelineState>>,
    /// Compiled spot-heal stage pipeline (GPU-RENDER-PARITY-1 follow-up). Built
    /// lazily on the first render whose recipe carries legacy spot geometry.
    #[cfg(feature = "gpu")]
    spot_pipeline: std::sync::Mutex<Option<SpotPipelineState>>,
    /// Compiled GPU-RENDER-PARITY-1 post-tone pipelines (per-pixel color,
    /// Presence DoG, dark channel, Dehaze). Built lazily on the first render
    /// whose recipe uses one of those stages.
    #[cfg(feature = "gpu")]
    post_pipeline: std::sync::Mutex<Option<PostPipelineState>>,
    /// Compiled GPU-RENDER-PARITY-1 geometry pipelines (lens correction,
    /// perspective, CA, crop, rotation, mirror). Built lazily on the first
    /// render whose recipe activates one of those stages.
    #[cfg(feature = "gpu")]
    geometry_pipeline: std::sync::Mutex<Option<geometry::GeometryPipelineState>>,
    /// Compiled G-05 lens-blur pipeline (GPU-RENDER-PARITY-1, lens-blur wave).
    /// Built lazily on the first render whose recipe activates the stage.
    #[cfg(feature = "gpu")]
    lens_blur_pipeline: std::sync::Mutex<Option<lens_blur::LensBlurPipelineState>>,
    /// Caller-supplied external depth plane for `recipe.lens_blur.depth_artifact`
    /// (G-05), bound via [`GpuContext::set_depth_plane`]. `None` leaves a
    /// recipe that references a depth artifact unrenderable on the GPU (it
    /// errors loudly, matching the CPU oracle's missing-artifact abort) and is
    /// irrelevant for the focus-rect heuristic.
    #[cfg(feature = "gpu")]
    depth_plane: Option<DepthPlaneGpu>,
    /// Caller-bound Lensfun warp/gain map (GPU-LENSFUN-PARITY-1), built from a
    /// strict corrector via [`lumina_core::LensfunMap::from_corrector`] and
    /// bound with [`GpuContext::set_lensfun_map`]. `None` means the recipe's
    /// manual lens model (if any) applies. A bound map replaces the manual
    /// lens step in the GPU geometry chain and is a loud render input: a
    /// dimension mismatch or a distortion corrector without an explicit crop
    /// errors instead of rendering divergent pixels.
    #[cfg(feature = "gpu")]
    lensfun_map: Option<lensfun::LensfunMapGpu>,
    /// VRAM-resident interactive state pool (GPU-60FPS-1 / GUI-WGPU-PRESENT-1):
    /// output + mask textures and overlay uniforms for a small LRU set of
    /// source dimensions, kept resident across frames so slider drags and brush
    /// strokes never read back to CPU. Lazily (re)created per dimensions.
    #[cfg(feature = "gpu")]
    vram: std::sync::Mutex<VramPool>,
    /// Source-action artifacts bound for the GPU source-action stage
    /// (GPU-STAGE-1). `None` means no artifacts are bound — recipes with
    /// `source_actions` then CPU-route exactly as before the stage existed.
    #[cfg(feature = "gpu")]
    source_actions: Option<Vec<SourceActionArtifact>>,
    /// Cached GPU textures for the bound source-action artifacts (R2-GPU-03):
    /// region (R16Uint) + replacement (RGBA8) per artifact, uploaded once in
    /// [`GpuContext::set_source_action_artifacts`] and reused by every render
    /// until the artifacts are re-bound or cleared. `None` until bound.
    #[cfg(feature = "gpu")]
    sa_textures: Option<
        Vec<(
            wgpu::Texture,
            wgpu::TextureView,
            wgpu::Texture,
            wgpu::TextureView,
        )>,
    >,
    /// Last recipe pushed via [`GpuContext::update_uniforms`]. Used both to feed
    /// the uniform buffer (GPU path) and as the CPU-fallback recipe.
    #[cfg(feature = "gpu")]
    recipe: Option<EditRecipe>,
    /// Per-size pool of input/output/readback resources for
    /// [`GpuContext::render_with_gpu`] (R2-GPU-04). Reusing them across calls
    /// avoids recreating wgpu objects every render; the dominant residual cost
    /// on small frames is the blocking readback, which the readback-free present
    /// path avoids entirely.
    #[cfg(feature = "gpu")]
    rwgpu_cache: std::sync::Mutex<std::collections::HashMap<(u32, u32), RenderWithGpuResources>>,
    /// Caller-bound decoder As-Shot white balance (`RawMetadata.camera_white_balance`,
    /// cam_mul), set via [`GpuContext::set_camera_white_balance`] and shared by
    /// the adapter and no-adapter CPU-fallback paths.
    ///
    /// The GPU tone stage consumes the already-As-Shot-applied decoded frame
    /// exactly like the CPU oracle (which validates the gains but never
    /// re-applies them — see `lumina_core::apply_recipe_with_white_balance`), so
    /// this field is *validation* state, not a pixel multiplier: binding the
    /// context makes the GPU entry reject non-finite/non-positive gains with the
    /// oracle's own error instead of silently ignoring them. Kept ungated so the
    /// no-`gpu`-feature context validates identically. Interior mutability
    /// (`Mutex`, like the pipeline) lets the CLI/MCP/GUI bind through `&self` on
    /// their immutable render paths.
    camera_white_balance: std::sync::Mutex<Option<[f32; 4]>>,
}

/// Caller-bound As-Shot white-balance context (R2-MCP-01 / CAMERA-WB-WELLE).
///
/// Available in every build so the no-adapter CPU-fallback path validates
/// identically. This is the explicit GPU input for
/// `RenderContext::camera_white_balance`: the caller owns obtaining the decoder
/// gains (CLI/MCP from `RawMetadata::camera_white_balance`, GUI from the decode
/// path) and binds them here, exactly like the Lensfun corrector / depth plane.
#[cfg_attr(not(feature = "gpu"), allow(dead_code))]
impl GpuContext {
    /// Bind (or clear with `None`) the decoder's As-Shot white-balance gains.
    ///
    /// `lumina-core` validates `RenderContext::camera_white_balance` before any
    /// pixel mutation and then does **not** re-apply the gains (the decoder
    /// already multiplied them in; see `apply_recipe_with_white_balance`). The
    /// GPU tone stage honours exactly that contract: binding the context makes
    /// [`Self::render_with_gpu`]/[`Self::render_to_vram`] reproduce the oracle's
    /// *validation* (As-Shot is validated **before** the recipe white balance is
    /// derived/applied), while the gains stay out of the shader's pixel math so
    /// the As-Shot-applied decoded frame is never double-graded.
    ///
    /// `Some(gains)` requires all four values finite and strictly positive;
    /// otherwise the oracle's own [`lumina_core::CoreError::InvalidAdjustment`]
    /// is returned and **no** state changes (no silent clamping). Binding a
    /// valid context is pixel-neutral in both backends, so the GPU route stays
    /// byte-compatible with the CPU reference.
    pub fn set_camera_white_balance(&self, gains: Option<[f32; 4]>) -> Result<(), GpuError> {
        if let Some(gains) = gains.as_ref() {
            validate_camera_white_balance_gains(gains)?;
        }
        *self.camera_white_balance.lock().unwrap() = gains;
        Ok(())
    }

    /// The currently bound As-Shot gains, if any (`None` = no context).
    pub fn camera_white_balance(&self) -> Option<[f32; 4]> {
        *self.camera_white_balance.lock().unwrap()
    }

    /// Re-validate the bound context at a render entry (defense in depth: the
    /// setter already rejects invalid gains, so this can only fire if a future
    /// path writes the field directly). Returns the oracle's own error.
    #[cfg(feature = "gpu")]
    fn validate_bound_camera_white_balance(&self) -> Result<(), GpuError> {
        if let Some(gains) = self.camera_white_balance() {
            validate_camera_white_balance_gains(&gains)?;
        }
        Ok(())
    }
}

/// The **full** CPU reference render used as the fallback whenever the GPU
/// cannot run a recipe.
///
/// Runs the complete `lumina_core::render_frame` chain — spot healing, every
/// adjustment stage (tone, color, Presence, Red-Eye), the decoupled geometry
/// stages (lens/fill/perspective/crop) and generative artifact compositing —
/// not just
/// `ImageFrame::apply_recipe`. That keeps the CPU fallback a *complete*
/// reference for every recipe-driven stage the GPU reports as unsupported
/// (AGENTS.md: CPU bleibt vollständige Referenz; kein stiller Fallback).
///
/// Render-context inputs the recipe-only GPU API does not carry (mask layers,
/// Lensfun correctors) are `None`/empty here; a caller that owns them must
/// re-gate on [`unsupported_gpu_stages_with_context`] and run its own
/// full-chain render. The G-05 external depth plane *is* carried through when
/// bound via [`GpuContext::set_depth_plane`], so the CPU fallback renders a
/// referenced depth artifact identically instead of erroring on a dropped
/// plane. Since CAMERA-WB-WELLE the decoder As-Shot white balance is carried
/// through as well (bound via [`GpuContext::set_camera_white_balance`]), so a
/// CPU-routed render validates and renders the same context the GPU entry saw.
fn render_cpu(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    depth: Option<&lumina_core::DepthPlane>,
    camera_white_balance: Option<[f32; 4]>,
) -> Result<Frame, GpuError> {
    render_cpu_with_generative(frame, recipe, depth, camera_white_balance, None)
}

/// GEN-ONNX-1 Welle 2a: the CPU reference render with caller-supplied
/// generative canvas artifacts (used by the GPU fallback paths so a routed
/// render carries the same generative context).
fn render_cpu_with_generative(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    depth: Option<&lumina_core::DepthPlane>,
    camera_white_balance: Option<[f32; 4]>,
    generative: Option<&lumina_core::GenerativeCanvasInput<'_>>,
) -> Result<Frame, GpuError> {
    let output = lumina_core::render_frame_with_generative(
        frame,
        &lumina_core::RenderContext {
            recipe,
            camera_white_balance,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth,
        },
        generative.copied().unwrap_or_default(),
    )?;
    Ok(Frame::from_image_frame(output.frame))
}

// ---------------------------------------------------------------------------
// GPU backend init (only compiled under the `gpu` feature).
// ---------------------------------------------------------------------------

#[cfg(feature = "gpu")]
pub(crate) struct GpuResources {
    #[allow(dead_code)]
    instance: std::mem::ManuallyDrop<wgpu::Instance>,
    #[allow(dead_code)]
    adapter: std::mem::ManuallyDrop<wgpu::Adapter>,
    #[allow(dead_code)]
    pub(crate) device: std::mem::ManuallyDrop<wgpu::Device>,
    #[allow(dead_code)]
    pub(crate) queue: std::mem::ManuallyDrop<wgpu::Queue>,
    /// Mirrors device loss into a flag the render methods poll, so a revoked
    /// adapter/device (driver update, GPU reset, monitor unplug) degrades to the
    /// CPU path instead of erroring or panicking (R2-GPU-06).
    ///
    /// SIGTRAP-GPU-TESTS: the wgpu handles above are intentionally leaked
    /// (`ManuallyDrop`). On Metal the teardown is thread-affine and dropping
    /// `Device`/`Queue` on a Rayon worker thread raises SIGTRAP (signal 5)
    /// while the tests themselves stay green. Leaking the per-process handles
    /// avoids that destructor on worker exit; the OS reclaims them at process
    /// exit. `device_lost` stays droppable (plain `Arc`).
    device_lost: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// Compiled color/tone render pipeline plus its uniform buffer and bind group
/// layout.
///
/// Created by [`build_pipeline`]. The WGSL shader (`SHADER_SRC`) is the real
/// color/tone stage; it samples the uploaded source texture and writes the
/// graded result into an `Rgba8Unorm` target. The bind group itself is rebuilt
/// per render (it references the per-frame input texture), so only the *layout*
/// is stored here.
#[cfg(feature = "gpu")]
struct PipelineState {
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group_layout: wgpu::BindGroupLayout,
    #[allow(dead_code)]
    _pipeline_layout: wgpu::PipelineLayout,
}

/// Compiled source-action stage pipeline (GPU-STAGE-1).
///
/// The bind group is rebuilt per render (it references per-artifact textures),
/// so only pipeline + uniform buffer + layout are kept.
#[cfg(feature = "gpu")]
struct SourceActionPipelineState {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

/// Compiled spot-heal stage pipeline (GPU-RENDER-PARITY-1 follow-up).
///
/// The parameter storage buffer is sized per recipe (the spot count is
/// unbounded), so only the pipeline + layout are cached here; the bind group is
/// rebuilt per render from the recipe's spot list.
#[cfg(feature = "gpu")]
struct SpotPipelineState {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

/// Compiled GPU-RENDER-PARITY-1 post-tone pipelines.
///
/// Groups the fullscreen adjustment passes that run *after* the tone pass in
/// the CPU oracle's order: the per-pixel color pass (curves → HSL → Point Color
/// → vibrance/saturation → Color Grading), the box Difference-of-Gaussians used
/// for Presence Texture/Clarity, the dark-channel pass, the Dehaze apply pass
/// and — since stage 2 — Noise Reduction, Sharpening (blur/apply + gradient
/// reduction compute) and the Effects (vignette/grain). Built once and reused;
/// only the bind groups (which reference per-frame input textures) are rebuilt
/// per pass.
#[cfg(feature = "gpu")]
struct PostPipelineState {
    color_pipeline: wgpu::RenderPipeline,
    color_layout: wgpu::BindGroupLayout,
    color_params: wgpu::Buffer,
    dog_pipeline: wgpu::RenderPipeline,
    dog_layout: wgpu::BindGroupLayout,
    // NOTE: no shared dog uniform buffer — a render can run two DoG passes
    // (Texture then Clarity) in one encoder, and a shared buffer would make
    // `queue.write_buffer` apply the *last* params to both draws. Each pass
    // allocates its own tiny uniform buffer instead.
    dark_pipeline: wgpu::RenderPipeline,
    dark_layout: wgpu::BindGroupLayout,
    dehaze_pipeline: wgpu::RenderPipeline,
    dehaze_layout: wgpu::BindGroupLayout,
    dehaze_params: wgpu::Buffer,
    noise_pipeline: wgpu::RenderPipeline,
    noise_layout: wgpu::BindGroupLayout,
    noise_params: wgpu::Buffer,
    vignette_pipeline: wgpu::RenderPipeline,
    vignette_layout: wgpu::BindGroupLayout,
    vignette_params: wgpu::Buffer,
    grain_pipeline: wgpu::RenderPipeline,
    grain_layout: wgpu::BindGroupLayout,
    grain_params: wgpu::Buffer,
    sharpen_blur_pipeline: wgpu::RenderPipeline,
    sharpen_blur_layout: wgpu::BindGroupLayout,
    sharpen_apply_pipeline: wgpu::RenderPipeline,
    sharpen_apply_layout: wgpu::BindGroupLayout,
    sharpen_gradient_pipeline: wgpu::ComputePipeline,
    sharpen_gradient_layout: wgpu::BindGroupLayout,
    sharpen_params: wgpu::Buffer,
    sharpen_maxg: wgpu::Buffer,
    red_eye_pipeline: wgpu::RenderPipeline,
    red_eye_layout: wgpu::BindGroupLayout,
    red_eye_params: wgpu::Buffer,
}

/// One post-tone fullscreen writing pass (GPU-RENDER-PARITY-1).
#[cfg(feature = "gpu")]
enum PostWrite {
    /// Presence Texture/Clarity box Difference-of-Gaussians.
    Dog(stages::DogParams),
    /// Presence Dehaze apply pass (holds the signed strength).
    Dehaze(f32),
    /// Per-pixel curves/HSL/Point Color/vibrance/saturation/Color Grading.
    /// Boxed: the fixed-capacity parameter block is far larger than the other
    /// variants (clippy::large_enum_variant).
    Color(Box<stages::ColorParams>),
    /// Noise Reduction (5x5 bilateral luminance + chroma).
    Noise(stages::NoiseParams),
    /// Sharpening (multi-pass separable Gaussian unsharp mask).
    /// Boxed: the kernel block is large (clippy::large_enum_variant).
    Sharpen(Box<stages::SharpenParams>),
    /// Red-eye correction (G-14): per-pixel region-local desaturation/darkening.
    RedEye(Box<stages::RedEyeParams>),
    /// Effects vignette.
    Vignette(stages::VignetteParams),
    /// Effects grain.
    Grain(stages::GrainParams),
}

/// Result of the batched source-action stage: the scratch textures that hold the
/// per-batch outputs and the index of the final one the tone/spot pass samples.
#[cfg(feature = "gpu")]
struct SourceActionBatches {
    textures: Vec<(wgpu::Texture, wgpu::TextureView)>,
    final_index: usize,
}

#[cfg(feature = "gpu")]
impl SourceActionBatches {
    fn final_view(&self) -> &wgpu::TextureView {
        &self.textures[self.final_index].1
    }
}

/// Pooled resources for a single [`GpuContext::render_with_gpu`] size bucket
/// (R2-GPU-04).
///
/// `render_with_gpu` is a one-shot readback render used by exports/CLI; before
/// this pool it re-created the input/output textures and readback buffer on
/// every call. Those objects are reused per `(width, height)` so repeated
/// renders (e.g. a benchmark loop or a batch export of same-size frames) don't
/// pay the wgpu object-churn cost each time. The GPU *allocations* are pooled;
/// the input texture is re-uploaded with the current frame only when the source
/// actually changes (identity-gated, mirroring R2-GPU-01), so a benchmark loop
/// that renders the same frame repeatedly skips the per-call CPU→GPU transfer.
///
/// **Bottleneck (R2-GPU-04).** At 2048² the GPU path is not faster than the CPU
/// oracle because the dominant cost is the *synchronous* readback round-trip
/// this one-shot API is forced into: `queue.write_texture` (≈16 MB upload) →
/// tone pass → `copy_texture_to_buffer` (≈16 MB) → `map_async` +
/// `device.poll(wait_indefinitely)` which stalls the calling thread until the
/// GPU finishes and the staging buffer is mapped. The per-pixel tone math on the
/// GPU is cheaper than the CPU, but the transfer + blocking-sync overhead
/// outweighs that win at this resolution. This is inherent to an API that must
/// return a CPU-owned `Frame` (export/CLI); the readback-free present path
/// (`render_to_vram` + `copy_vram_to_texture`, which composites straight into an
/// egui-registered texture) avoids the round-trip entirely and is where the GPU
/// actually wins for interactive preview. Making `render_with_gpu` faster for
/// export therefore requires async readback or a persistent VRAM `Frame` (M2),
/// not just pooled resources — which is why this pool removes churn but leaves
/// the readback as the floor.
#[cfg(feature = "gpu")]
struct RenderWithGpuResources {
    input: wgpu::Texture,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    readback: wgpu::Buffer,
    /// Instance identity (`upload_identity::UploadIdentity`) of the last frame
    /// uploaded into `input`, so repeated renders of the same source skip the
    /// CPU→GPU upload (R2-GPU-04, mirroring R2-GPU-01).
    input_source_identity: Option<vram_pool::UploadIdentity>,
}

/// GEN-ONNX-1 Welle 2a: one caller-supplied generative canvas uploaded to VRAM
/// for the geometry-chain substitution (`Substitute` step).
#[cfg(feature = "gpu")]
struct UploadedGenerativeArtifact {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

#[cfg(test)]
mod tests;
