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
    GpuError, MAX_SOURCE_ACTIONS,
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
#[cfg(feature = "gpu")]
pub mod tiling;
#[cfg(feature = "gpu")]
use color_tone_shader::SHADER_SRC;
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
/// resampling or cropping (`Agents.md`: no silent fallbacks). An empty slice
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

#[cfg(feature = "gpu")]
impl GpuContext {
    /// Create a GPU context.
    ///
    /// On success this returns an `Ok` context whose [`is_available`](Self::is_available)
    /// reports whether a real adapter/device was bound. Adapter or device
    /// creation failures are handled gracefully: the context is still returned,
    /// just without GPU resources, so rendering falls back to the CPU path
    /// instead of erroring out.
    pub fn new() -> Result<Self, GpuError> {
        match init_gpu_resources() {
            Ok(resources) => Ok(Self::from_resources(Some(resources))),
            // Degrade gracefully to the CPU fallback rather than failing the
            // app — but never silently (REVIEW-GPU-N1, Agents.md: no silent
            // fallbacks): the adapter/device failure is logged loudly so
            // headless or misconfigured machines stay diagnosable.
            Err(err) => {
                log_gpu_init_failure(&err);
                Ok(Self::from_resources(None))
            }
        }
    }

    /// Build a [`GpuContext`] from externally owned wgpu resources
    /// (GUI-WGPU-PRESENT-1).
    ///
    /// This is the additive constructor for the shared-device migration: the
    /// native GUI runs `eframe` with the **wgpu** renderer and hands this
    /// crate the renderer's `Instance`/`Adapter`/`Device`/`Queue` (`eframe`'s
    /// `CreationContext::wgpu_render_state`). All VRAM textures then live on
    /// the *same* device that presents to the swapchain, which is what makes
    /// the readback-free present path (`copy_vram_to_texture` into a texture
    /// registered as an egui user image) possible at all.
    ///
    /// Standalone consumers (CLI, tests) keep using [`GpuContext::new`], which
    /// creates its own Metal-restricted instance — the two construction paths
    /// are mutually exclusive by ownership, never mixed in one process.
    pub fn from_parts(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
    ) -> Result<Self, GpuError> {
        let mut resources = GpuResources {
            instance: std::mem::ManuallyDrop::new(instance),
            adapter: std::mem::ManuallyDrop::new(adapter),
            device: std::mem::ManuallyDrop::new(device),
            queue: std::mem::ManuallyDrop::new(queue),
            device_lost: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        // The GUI shares eframe's device/queue (GUI-WGPU-PRESENT-1). A lost or
        // erroring device must not panic the app (R2-GPU-06): install the same
        // non-panicking handlers as the standalone path.
        register_device_handlers(&mut resources);
        Ok(Self::from_resources(Some(resources)))
    }

    /// Shared constructor body for [`GpuContext::new`] / [`GpuContext::from_parts`].
    fn from_resources(resources: Option<GpuResources>) -> Self {
        Self {
            resources,
            pipeline: std::sync::Mutex::new(None),
            sa_pipeline: std::sync::Mutex::new(None),
            spot_pipeline: std::sync::Mutex::new(None),
            post_pipeline: std::sync::Mutex::new(None),
            geometry_pipeline: std::sync::Mutex::new(None),
            lens_blur_pipeline: std::sync::Mutex::new(None),
            depth_plane: None,
            lensfun_map: None,
            vram: std::sync::Mutex::new(VramPool::new()),
            source_actions: None,
            #[cfg(feature = "gpu")]
            sa_textures: None,
            recipe: None,
            #[cfg(feature = "gpu")]
            rwgpu_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
            camera_white_balance: std::sync::Mutex::new(None),
        }
    }

    /// Whether a real GPU adapter/device is bound. When `false`, all renders use
    /// the CPU fallback.
    pub fn is_available(&self) -> bool {
        self.resources.is_some()
    }

    /// Human-readable adapter description for backend-selection logging.
    /// Returns `None` when no adapter is bound (CPU fallback only).
    pub fn adapter_info(&self) -> Option<String> {
        self.resources.as_ref().map(|resources| {
            let info = resources.adapter.get_info();
            format!(
                "{} (vendor 0x{:x}, device 0x{:x}, driver {})",
                info.name, info.vendor, info.device, info.driver
            )
        })
    }

    /// Borrow the bound [`wgpu::Device`], if any.
    pub fn device(&self) -> Option<&wgpu::Device> {
        self.resources.as_ref().map(|r| &*r.device)
    }

    /// Borrow the bound [`wgpu::Queue`], if any.
    pub fn queue(&self) -> Option<&wgpu::Queue> {
        self.resources.as_ref().map(|r| &*r.queue)
    }

    /// Build (once) the color/tone render pipeline: uniform buffer, bind group
    /// layout (uniform + input texture + sampler), pipeline layout and the real
    /// WGSL color/tone shader, rendering into an `Rgba8Unorm` target. No-op when
    /// no adapter is bound.
    pub fn create_pipeline(&mut self) -> Result<(), GpuError> {
        let Some(resources) = self.resources.as_ref() else {
            return Ok(());
        };
        let mut guard = self.pipeline.lock().unwrap();
        if guard.is_some() {
            return Ok(());
        }
        *guard = Some(build_pipeline(resources)?);
        Ok(())
    }

    /// Lazily build the color/tone pipeline from `&self` (used by the GPU render
    /// path, which must keep a `&self` signature for the CLI/MCP call sites).
    fn ensure_pipeline(&self) -> Result<(), GpuError> {
        let Some(resources) = self.resources.as_ref() else {
            return Ok(());
        };
        let mut guard = self.pipeline.lock().unwrap();
        if guard.is_none() {
            *guard = Some(build_pipeline(resources)?);
        }
        Ok(())
    }

    /// Lazily build the source-action stage pipeline from `&self` (GPU-STAGE-1).
    /// Only invoked on renders that actually run with bound artifacts.
    fn ensure_source_action_pipeline(&self) -> Result<(), GpuError> {
        let Some(resources) = self.resources.as_ref() else {
            return Ok(());
        };
        let mut guard = self.sa_pipeline.lock().unwrap();
        if guard.is_none() {
            *guard = Some(SourceActionPipelineState {
                pipeline: shaders::create_source_action_pipeline(
                    &resources.device,
                    shaders::RGBA8_FORMAT,
                )?,
                bind_group_layout: shaders::create_source_action_bind_group_layout(
                    &resources.device,
                ),
            });
        }
        Ok(())
    }

    /// Lazily build the spot-heal stage pipeline from `&self`
    /// (GPU-RENDER-PARITY-1 follow-up). Only invoked on renders that actually
    /// carry legacy spot geometry.
    fn ensure_spot_pipeline(&self) -> Result<(), GpuError> {
        let Some(resources) = self.resources.as_ref() else {
            return Ok(());
        };
        let mut guard = self.spot_pipeline.lock().unwrap();
        if guard.is_none() {
            *guard = Some(SpotPipelineState {
                pipeline: stages::create_spot_heal_pipeline(
                    &resources.device,
                    shaders::RGBA8_FORMAT,
                )?,
                bind_group_layout: stages::create_spot_heal_bind_group_layout(&resources.device),
            });
        }
        Ok(())
    }

    /// Lazily build the GPU-RENDER-PARITY-1 post-tone pipelines.
    fn ensure_post_pipelines(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Option<PostPipelineState>>, GpuError> {
        let mut guard = self.post_pipeline.lock().unwrap();
        if guard.is_none() {
            let Some(resources) = self.resources.as_ref() else {
                return Ok(guard);
            };
            *guard = Some(build_post_pipelines(&resources.device)?);
        }
        Ok(guard)
    }

    /// Lazily build the GPU-RENDER-PARITY-1 geometry pipelines (lens,
    /// perspective, CA, crop, rotation, mirror).
    fn ensure_geometry_pipelines(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Option<geometry::GeometryPipelineState>>, GpuError> {
        let mut guard = self.geometry_pipeline.lock().unwrap();
        if guard.is_none() {
            let Some(resources) = self.resources.as_ref() else {
                return Ok(guard);
            };
            *guard = Some(geometry::build_geometry_pipelines(&resources.device)?);
        }
        Ok(guard)
    }

    /// Lazily build the G-05 lens-blur pipeline (GPU-RENDER-PARITY-1, lens-blur
    /// wave). Only invoked on renders that actually run the active stage.
    fn ensure_lens_blur_pipeline(&self) -> Result<(), GpuError> {
        let Some(resources) = self.resources.as_ref() else {
            return Ok(());
        };
        let mut guard = self.lens_blur_pipeline.lock().unwrap();
        if guard.is_none() {
            *guard = Some(lens_blur::build_pipeline(
                &resources.device,
                &resources.queue,
            )?);
        }
        Ok(())
    }

    /// Encode the G-05 lens-blur pass (lazily building its pipeline) from
    /// `input_view` into `dst`. `external_view` is the bound depth plane when
    /// the recipe references one, else `None` (heuristic; the pipeline's 1×1
    /// dummy depth is bound instead).
    fn encode_lens_blur_stage(
        &self,
        resources: &GpuResources,
        blur: &lumina_sidecar::LensBlur,
        external_view: Option<&wgpu::TextureView>,
        input_view: &wgpu::TextureView,
        dst: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<(), GpuError> {
        self.ensure_lens_blur_pipeline()?;
        let guard = self.lens_blur_pipeline.lock().unwrap();
        let Some(state) = guard.as_ref() else {
            return Err(GpuError::RenderFailed(
                "lens-blur pipeline not built".into(),
            ));
        };
        let depth_view = external_view.unwrap_or(&state.dummy_depth_view);
        encode_lens_blur(resources, state, blur, depth_view, input_view, dst, encoder);
        Ok(())
    }

    /// Bind a caller-supplied external depth plane for
    /// `recipe.lens_blur.depth_artifact` (G-05), or clear it with `None`.
    ///
    /// Validation mirrors `lumina_core::lens_blur::apply_lens_blur`: every value
    /// must be finite and in `0..=1`, otherwise the bind is rejected with the
    /// oracle's `InvalidAdjustment` and **no** state changes (no silent
    /// clamping). The plane must match the **post-geometry** frame dimensions,
    /// which are only known at render time, so a dimension mismatch is reported
    /// loudly by the render entry points.
    ///
    /// A recipe that references a depth artifact without a matching bound plane
    /// stays unrenderable on the GPU: the render entry point returns the CPU
    /// oracle's missing-artifact error instead of silently falling back to the
    /// focus-rect heuristic. Call [`Self::set_depth_plane`] with `None` to
    /// release the texture.
    pub fn set_depth_plane(
        &mut self,
        plane: Option<&lumina_core::DepthPlane>,
    ) -> Result<(), GpuError> {
        let Some(plane) = plane else {
            self.depth_plane = None;
            return Ok(());
        };
        for value in &plane.values {
            if !value.is_finite() || !(0.0..=1.0).contains(value) {
                return Err(GpuError::Core(lumina_core::CoreError::InvalidAdjustment {
                    name: "lens_blur.depth_plane.value".into(),
                    value: f64::from(*value),
                    minimum: 0.0,
                    maximum: 1.0,
                }));
            }
        }
        let (texture, view) = match self.resources.as_ref() {
            Some(resources) => {
                let texture = lens_blur::create_depth_texture(
                    &resources.device,
                    plane.width,
                    plane.height,
                    "lumina-gpu-depth-plane",
                );
                resources.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    bytemuck::cast_slice(&plane.values),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(plane.width * 4),
                        rows_per_image: Some(plane.height),
                    },
                    wgpu::Extent3d {
                        width: plane.width,
                        height: plane.height,
                        depth_or_array_layers: 1,
                    },
                );
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                (Some(texture), Some(view))
            }
            // No adapter: keep the values for the CPU fallback, no texture.
            None => (None, None),
        };
        self.depth_plane = Some(DepthPlaneGpu {
            plane: plane.clone(),
            texture,
            view,
        });
        Ok(())
    }

    /// Bind (or clear with `None`) the caller-owned Lensfun warp/gain map
    /// (GPU-LENSFUN-PARITY-1), built from a strict corrector via
    /// [`lumina_core::LensfunMap::from_corrector`].
    ///
    /// Mirroring [`Self::set_depth_plane`], the map is validated loudly on bind
    /// (dimensions, plane lengths, TCA pairing, finiteness): an invalid map
    /// returns the oracle's own [`lumina_core::CoreError`] and changes **no**
    /// state. The GPU textures are uploaded once here and reused by every render
    /// until the map is re-bound or cleared. A bound dimension-preserving map
    /// replaces the recipe's manual lens step; a dimension-changing chain or a
    /// distortion corrector without an explicit crop is refused loudly at the
    /// render entry (never divergent pixels).
    pub fn set_lensfun_map(
        &mut self,
        map: Option<&lumina_core::LensfunMap>,
    ) -> Result<(), GpuError> {
        self.lensfun_map = lensfun::bind_map(self.resources.as_ref(), map)?;
        Ok(())
    }

    /// Validate a bound external depth plane against the frame the oracle would
    /// blur. `out_width`/`out_height` are the planned post-geometry output
    /// dimensions the caller computed from the geometry plan (source dims when
    /// no geometry is active).
    fn external_depth_view_for(
        &self,
        recipe: &EditRecipe,
        out_width: u32,
        out_height: u32,
    ) -> Result<Option<&wgpu::TextureView>, GpuError> {
        let Some(blur) = recipe
            .lens_blur
            .as_ref()
            .filter(|b| lens_blur::stage_active(b))
        else {
            return Ok(None);
        };
        if blur.depth_artifact.is_none() {
            return Ok(None);
        }
        let Some(depth) = self.depth_plane.as_ref() else {
            return Err(GpuError::Core(lumina_core::CoreError::InvalidAdjustment {
                name: "lens_blur.depth_artifact".into(),
                value: -1.0,
                minimum: 0.0,
                maximum: 0.0,
            }));
        };
        if depth.plane.width != out_width || depth.plane.height != out_height {
            return Err(GpuError::Core(lumina_core::CoreError::InvalidMaskPlane {
                width: depth.plane.width,
                height: depth.plane.height,
                length: depth.plane.values.len(),
            }));
        }
        Ok(depth.view.as_ref())
    }

    /// Bind the source-action artifacts the GPU source-action stage composites
    /// before the tone pass (GPU-STAGE-1).
    ///
    /// Validation mirrors `lumina_core`'s `apply_source_actions`: every
    /// artifact's region and replacement must share dimensions, and all
    /// artifacts must target the same frame geometry — otherwise
    /// [`GpuError::RenderFailed`] is returned and **no** binding changes
    /// (no silent fallback, no partial mutation).
    ///
    /// While artifacts are bound, [`unsupported_gpu_stages_for`] stops flagging
    /// `source_actions`, so `render_with_gpu`/`render_to_vram` composite them
    /// on the GPU instead of CPU-routing. Call [`Self::clear_source_action_artifacts`]
    /// to return to the strict recipe-only view.
    pub fn set_source_action_artifacts(
        &mut self,
        artifacts: &[SourceActionArtifact],
    ) -> Result<(), GpuError> {
        for (index, action) in artifacts.iter().enumerate() {
            let region = &action.region;
            let replacement = &action.replacement;
            if region.width != replacement.width() || region.height != replacement.height() {
                return Err(GpuError::RenderFailed(format!(
                    "source-action artifact {index}: region {}x{} does not match replacement {}x{}",
                    region.width,
                    region.height,
                    replacement.width(),
                    replacement.height()
                )));
            }
            if index > 0 {
                let first = &artifacts[0].region;
                if first.width != region.width || first.height != region.height {
                    return Err(GpuError::RenderFailed(format!(
                        "source-action artifact {index}: region {}x{} does not match \
                         artifact 0 region {}x{}",
                        region.width, region.height, first.width, first.height
                    )));
                }
            }
        }
        // R2-GPU-03: the region/replacement textures don't change between drags,
        // so upload them once here and cache them on the context. The render
        // path reuses the cached textures instead of re-creating + re-uploading
        // a full-size texture per render tick.
        if let Some(resources) = self.resources.as_ref() {
            let mut cached = Vec::with_capacity(artifacts.len());
            for (index, action) in artifacts.iter().enumerate() {
                let region_tex = shaders::create_region_texture(
                    &resources.device,
                    action.region.width,
                    action.region.height,
                    &format!("lumina-gpu-sa-region-{index}"),
                );
                shaders::write_u16_plane(
                    &resources.queue,
                    &region_tex,
                    action.region.width,
                    action.region.height,
                    &action.region.values,
                );
                let region_view = region_tex.create_view(&wgpu::TextureViewDescriptor::default());
                let repl_tex = shaders::create_input_texture(
                    &resources.device,
                    action.replacement.width(),
                    action.replacement.height(),
                    &format!("lumina-gpu-sa-repl-{index}"),
                );
                resources.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &repl_tex,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    action.replacement.pixels(),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(action.replacement.width() * 4),
                        rows_per_image: Some(action.replacement.height()),
                    },
                    wgpu::Extent3d {
                        width: action.replacement.width(),
                        height: action.replacement.height(),
                        depth_or_array_layers: 1,
                    },
                );
                let repl_view = repl_tex.create_view(&wgpu::TextureViewDescriptor::default());
                cached.push((region_tex, region_view, repl_tex, repl_view));
            }
            self.sa_textures = Some(cached);
        }
        self.source_actions = Some(artifacts.to_vec());
        Ok(())
    }

    /// Remove previously bound source-action artifacts. Subsequent renders of
    /// recipes with `source_actions` CPU-route again.
    pub fn clear_source_action_artifacts(&mut self) {
        self.source_actions = None;
        self.sa_textures = None;
    }

    /// The bound source-action artifacts that match `width`×`height`, or `None`
    /// when nothing is bound (or any artifact targets different geometry — in
    /// which case such renders must stay on the CPU route; enforced via
    /// [`unsupported_gpu_stages_for`] with `false`).
    fn matching_source_actions(&self, width: u32, height: u32) -> Option<&[SourceActionArtifact]> {
        let artifacts = self.source_actions.as_ref()?;
        let all_match = artifacts
            .iter()
            .all(|a| a.region.width == width && a.region.height == height);
        if all_match && !artifacts.is_empty() {
            Some(artifacts)
        } else {
            None
        }
    }

    /// Push a recipe into the context and, when an adapter is bound, upload its
    /// slider parameters to the uniform buffer.
    pub fn update_uniforms(&mut self, recipe: &EditRecipe) -> Result<(), GpuError> {
        self.recipe = Some(recipe.clone());
        // Build the pipeline first before borrowing `resources`.
        self.create_pipeline()?;
        if let Some(resources) = self.resources.as_ref() {
            if let Some(pipeline) = self.pipeline.lock().unwrap().as_ref() {
                let uniforms = shaders::Uniforms::from_recipe(recipe);
                shaders::write_uniforms(&resources.queue, &pipeline.uniform_buffer, &uniforms);
            }
        }
        Ok(())
    }

    /// Whether frame-time perf logging is enabled (`LUMINA_PERF_LOG=1`).
    pub fn perf_log_enabled() -> bool {
        std::env::var("LUMINA_PERF_LOG").as_deref() == Ok("1")
    }

    /// Interactive tone render that stays VRAM-resident — no `map_async` readback.
    ///
    /// When source-action artifacts are bound (GPU-STAGE-1) and match the frame
    /// dimensions, the dedicated source-action stage composites them into an
    /// intermediate texture first; the tone pass then samples *that* result.
    /// Otherwise the tone stage (`SHADER_SRC`) renders the uploaded frame
    /// directly into the cached `output` VRAM texture of the active pool entry.
    /// When the recipe uses GPU-RENDER-PARITY-1 post-tone stages (Presence,
    /// Curves, HSL, Point Color, vibrance/saturation, Color Grading) the tone
    /// result feeds the same post chain as [`Self::render_with_gpu`] before the
    /// final pixels land in the resident output texture. Dehaze is the one
    /// post stage that needs a host round-trip (the deterministic dark-channel
    /// percentile).
    ///
    /// Caller presents via [`Self::copy_vram_to_texture`] or the overlay pass
    /// without ever mapping to CPU. Export/full-rebuild paths should use
    /// [`Self::render_with_gpu`] (which still reads back).
    ///
    /// **Unsupported recipes are refused.** The VRAM path cannot CPU-route
    /// without a readback, so a recipe with GPU-unsupported stages returns
    /// [`GpuError::RenderFailed`] (after a loud, once-per-reason-set warning)
    /// instead of writing divergent pixels into the resident output. The caller
    /// must render such a recipe through the full CPU reference instead; the
    /// GUI already drops `vram_fresh` and falls back on this error.
    ///
    /// **Geometry.** The resident present texture is source-sized, so a geometry
    /// chain that changes the output dimensions (crop/rotation/perspective) is
    /// also refused loudly ([`warn_vram_dimension_change_once`]); the caller
    /// then uses the exact CPU present path. Dimension-preserving geometry
    /// (mirror/identity rotation) is rendered into the resident output and
    /// matches the CPU oracle.
    ///
    /// **Default content crop (GPU-MAXRECT-WELLE).** A lens/perspective
    /// correction without an explicit crop activates the CPU oracle's
    /// content-based default crop, whose (possibly smaller) dimensions depend on
    /// the resampled alpha. The readback-free path cannot plan it, so
    /// [`unsupported_gpu_stages_for`] flags those recipes and this entry refuses
    /// them before any write (the module-level `geometry (default content crop)`
    /// reason). The GUI then presents the exact CPU reference.
    pub fn render_to_vram(&self, frame: &ImageFrame, recipe: &EditRecipe) -> Result<(), GpuError> {
        // CAMERA-WB-WELLE (R2-MCP-01): the VRAM path cannot CPU-route without a
        // readback, so it must validate the caller-bound As-Shot context itself
        // (the setter already rejects invalid gains; this is the entry-level
        // guarantee) instead of letting a shader that has no notion of them
        // silently ignore an invalid context.
        self.validate_bound_camera_white_balance()?;
        // REVIEW-GPU-DIVERGENCE-1 / GPU-STAGE-1: the VRAM hot path cannot
        // CPU-route without a readback (that would defeat its purpose). A
        // recipe whose stages are unsupported *given the currently bound
        // artifacts* is therefore rejected loudly (no divergent pixels are ever
        // written). With bound artifacts, `source_actions` is no longer
        // "unsupported" — the dedicated GPU stage composites them.
        validate_gpu_recipe(recipe)?;
        let sa_bound = self
            .matching_source_actions(frame.width(), frame.height())
            .is_some();
        let unsupported = unsupported_gpu_stages_for(recipe, sa_bound);
        if !unsupported.is_empty() {
            warn_unsupported_vram_once(&unsupported);
            return Err(GpuError::RenderFailed(format!(
                "VRAM path refuses a recipe with GPU-unsupported stage(s): {}; \
                 render it through the full CPU reference (render_frame) instead",
                unsupported.join("; ")
            )));
        }
        // GPU-RENDER-PARITY-1 geometry wave: plan the geometry chain. The
        // readback-free VRAM present texture is source-sized, so geometry that
        // changes the output dimensions cannot be presented here yet. That is
        // an honest, loud limitation — no divergent pixels are ever written and
        // the caller (GUI) falls back to the exact CPU present path.
        let geometry_plan = match self.lensfun_map.as_ref() {
            Some(bound) => geometry::GeometryPlan::from_recipe_with(
                recipe,
                frame.width(),
                frame.height(),
                &geometry::GenerativePlanInput::default(),
                false,
                Some(&bound.map),
            )?,
            None => geometry::GeometryPlan::from_recipe(recipe, frame.width(), frame.height())?,
        };
        let vram_geometry = geometry_plan.as_ref().is_some_and(|plan| {
            plan.output_width == frame.width() && plan.output_height == frame.height()
        });
        if geometry_plan.is_some() && !vram_geometry {
            let reason =
                "geometry (dimension-changing output; the VRAM present texture is source-sized)";
            warn_vram_dimension_change_once(reason);
            return Err(GpuError::RenderFailed(format!(
                "VRAM path cannot present {reason}; render it through the full CPU \
                 reference (render_frame) instead"
            )));
        }
        // GPU-RENDER-PARITY-1 lens-blur wave: G-05 preserves the frame
        // dimensions, so the resident output can hold it. The external-plane
        // requirement is resolved *before* any bytes are written (a referenced
        // but unbound/mismatched depth plane is a loud error — no silent
        // heuristic). Identity configurations skip the pass entirely.
        let lens_blur_recipe = recipe
            .lens_blur
            .as_ref()
            .filter(|blur| lens_blur::stage_active(blur));
        let lens_external_view = self.external_depth_view_for(
            recipe,
            geometry_plan
                .as_ref()
                .map_or(frame.width(), |plan| plan.output_width),
            geometry_plan
                .as_ref()
                .map_or(frame.height(), |plan| plan.output_height),
        )?;
        let needs_lens_blur = lens_blur_recipe.is_some_and(|blur| {
            lens_blur::radius_for(blur.blur_amount) > 0 && !lens_blur::has_no_effect(blur)
        });
        let Some(resources) = self.resources.as_ref() else {
            return Err(GpuError::AdapterUnavailable(
                "no adapter for vram path".into(),
            ));
        };
        // R2-GPU-06: a lost device must not panic — signal the caller (the GUI
        // then drops `vram_fresh` and falls back to the CPU present path).
        if resources
            .device_lost
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(GpuError::AdapterUnavailable("GPU device lost".into()));
        }
        self.ensure_pipeline()?;
        self.ensure_vram(frame.width(), frame.height())?;
        let guard = self.pipeline.lock().unwrap();
        let Some(pipeline) = guard.as_ref() else {
            return Err(GpuError::RenderFailed("pipeline not built".into()));
        };
        let uniforms = shaders::Uniforms::from_recipe(recipe);
        shaders::write_uniforms(&resources.queue, &pipeline.uniform_buffer, &uniforms);
        let start = if Self::perf_log_enabled() {
            Some(std::time::Instant::now())
        } else {
            None
        };
        // R2-GPU-01: reuse the cached base texture and re-upload it only when
        // the source frame changes. During a slider drag the same source is
        // passed every tick, so this skips the ~96 MB CPU→GPU upload on all but
        // the first tick.
        let mut vram_guard = self.vram.lock().unwrap();
        let Some(v) = vram_guard.active() else {
            return Err(GpuError::RenderFailed("vram not ready".into()));
        };
        let src_identity = (frame.pixels().as_ptr() as usize, frame.pixels().len());
        if v.input_source_identity != Some(src_identity) {
            resources.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &v.input,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                frame.pixels(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(frame.width() * 4),
                    rows_per_image: Some(frame.height()),
                },
                wgpu::Extent3d {
                    width: frame.width(),
                    height: frame.height(),
                    depth_or_array_layers: 1,
                },
            );
            v.input_source_identity = Some(src_identity);
        }
        let input_view = &v.input_view;
        let sampler = &v.input_sampler;
        let mut enc = resources
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("lumina-gpu-vram-encode"),
            });
        // GPU-STAGE-1 / item 7: when source-action artifacts are bound for this
        // frame, the dedicated stage composites them (batched) into intermediate
        // textures in the same encoder, then the spot-heal pass; the tone pass
        // samples the result. Without bound artifacts the tone pass reads the
        // uploaded frame directly.
        let sa_batches: Option<SourceActionBatches> = if self
            .matching_source_actions(frame.width(), frame.height())
            .is_some()
        {
            self.ensure_source_action_pipeline()?;
            let sa_guard = self.sa_pipeline.lock().unwrap();
            let Some(sa) = sa_guard.as_ref() else {
                return Err(GpuError::RenderFailed(
                    "source-action pipeline not built".into(),
                ));
            };
            // R2-GPU-03: reuse the cached region/replacement textures
            // uploaded in `set_source_action_artifacts` instead of
            // re-creating + re-uploading them every render tick.
            let sa_cache = self
                .sa_textures
                .as_ref()
                .expect("source-action textures are cached when artifacts are bound");
            Some(encode_source_action_batches(
                resources,
                sa,
                sa_cache,
                input_view,
                frame.width(),
                frame.height(),
                &mut enc,
            )?)
        } else {
            None
        };
        let sa_view: &wgpu::TextureView = match sa_batches.as_ref() {
            Some(batches) => batches.final_view(),
            None => input_view,
        };
        // Spot heal: legacy `extras["spot_removals"]` geometry, before the tone
        // pass (mirrors `apply_spot_heals_from_recipe`).
        let spots = lumina_core::spots_from_recipe(recipe);
        let spot_texture: Option<wgpu::Texture> = if spots.is_empty() {
            None
        } else {
            Some(shaders::create_output_texture(
                &resources.device,
                frame.width(),
                frame.height(),
                "lumina-gpu-vram-spot-out",
            ))
        };
        let spot_view_owned: Option<wgpu::TextureView> = spot_texture
            .as_ref()
            .map(|texture| texture.create_view(&wgpu::TextureViewDescriptor::default()));
        let tone_input_view: &wgpu::TextureView = match spot_view_owned.as_ref() {
            Some(spot_view) => {
                self.ensure_spot_pipeline()?;
                let spot_guard = self.spot_pipeline.lock().unwrap();
                let Some(spot) = spot_guard.as_ref() else {
                    return Err(GpuError::RenderFailed("spot pipeline not built".into()));
                };
                encode_spot_heal(resources, spot, &spots, sa_view, spot_view, &mut enc);
                spot_view
            }
            None => sa_view,
        };
        let tone_bind = shaders::create_color_tone_bind_group(
            &resources.device,
            &pipeline.bind_group_layout,
            &pipeline.uniform_buffer,
            tone_input_view,
            sampler,
        );
        // GPU-RENDER-PARITY-1: when the recipe uses post-tone stages, the tone
        // pass renders into a transient texture and the post chain writes the
        // final pixels into the resident VRAM output (which the overlay/present
        // path samples); without post stages the tone pass writes it directly.
        let needs_post = post_stages_needed(recipe);
        // The lens-blur pass always writes the resident output, so any earlier
        // stage (tone and/or post) must land in a transient when it is active.
        let tone_texture: Option<wgpu::Texture> = if needs_post || vram_geometry || needs_lens_blur
        {
            Some(shaders::create_output_texture(
                &resources.device,
                frame.width(),
                frame.height(),
                "lumina-gpu-vram-tone-intermediate",
            ))
        } else {
            None
        };
        let tone_view_owned: Option<wgpu::TextureView> = tone_texture
            .as_ref()
            .map(|texture| texture.create_view(&wgpu::TextureViewDescriptor::default()));
        let tone_target_view: &wgpu::TextureView =
            tone_view_owned.as_ref().unwrap_or(&v.output_view);
        // When geometry or the lens blur writes the resident output as its
        // final pass, the post chain must not write it too, so it lands in a
        // transient instead.
        let post_target: Option<wgpu::Texture> = if (vram_geometry || needs_lens_blur) && needs_post
        {
            Some(shaders::create_output_texture(
                &resources.device,
                frame.width(),
                frame.height(),
                "lumina-gpu-vram-post-intermediate",
            ))
        } else {
            None
        };
        let post_target_view: Option<wgpu::TextureView> = post_target
            .as_ref()
            .map(|texture| texture.create_view(&wgpu::TextureViewDescriptor::default()));
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lumina-gpu-vram-tone"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: tone_target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline.pipeline);
            pass.set_bind_group(0, &tone_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        resources.queue.submit(Some(enc.finish()));
        if needs_post {
            let post_dst: &wgpu::TextureView = post_target_view.as_ref().unwrap_or(&v.output_view);
            self.render_post_stages(
                resources,
                frame.width(),
                frame.height(),
                recipe,
                tone_target_view,
                post_dst,
            )?;
        }
        // Kept alive until after the lens-blur pass reads them: when the blur
        // follows geometry, the geometry chain writes this transient instead of
        // the resident output.
        let mut geometry_lens_intermediate: Option<wgpu::Texture> = None;
        let mut geometry_lens_intermediate_view: Option<wgpu::TextureView> = None;
        let mut lens_input_view: Option<&wgpu::TextureView> = None;
        if let Some(plan) = geometry_plan.as_ref() {
            // `vram_geometry` holds here (dimension-changing geometry was
            // refused above).
            let geo_input: &wgpu::TextureView = if needs_post {
                post_target_view
                    .as_ref()
                    .expect("post target exists when geometry and post stages are active")
            } else {
                tone_target_view
            };
            if needs_lens_blur {
                let texture = shaders::create_output_texture(
                    &resources.device,
                    frame.width(),
                    frame.height(),
                    "lumina-gpu-vram-geometry-intermediate",
                );
                geometry_lens_intermediate_view =
                    Some(texture.create_view(&wgpu::TextureViewDescriptor::default()));
                geometry_lens_intermediate = Some(texture);
            }
            let geo_target: &wgpu::TextureView = geometry_lens_intermediate_view
                .as_ref()
                .unwrap_or(&v.output_view);
            let geo_final_texture: &wgpu::Texture =
                geometry_lens_intermediate.as_ref().unwrap_or(&v.output);
            let mut geo_enc =
                resources
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("lumina-gpu-vram-geometry"),
                    });
            {
                let geo_guard = self.ensure_geometry_pipelines()?;
                let Some(geo) = geo_guard.as_ref() else {
                    return Err(GpuError::RenderFailed("geometry pipeline not built".into()));
                };
                self.encode_geometry_chain(
                    resources,
                    geo,
                    plan,
                    geo_input,
                    geo_final_texture,
                    geo_target,
                    &[None, None],
                    &mut geo_enc,
                )?;
            }
            resources.queue.submit(Some(geo_enc.finish()));
            if needs_lens_blur {
                lens_input_view = geometry_lens_intermediate_view.as_ref();
            }
        } else if needs_lens_blur {
            lens_input_view = Some(if needs_post {
                post_target_view
                    .as_ref()
                    .expect("post target exists when lens blur and post stages are active")
            } else {
                tone_target_view
            });
        }
        if needs_lens_blur {
            let blur = lens_blur_recipe.expect("pass implies an active recipe");
            let input = lens_input_view.expect("lens blur input view selected above");
            let mut blur_enc =
                resources
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("lumina-gpu-vram-lens-blur"),
                    });
            self.encode_lens_blur_stage(
                resources,
                blur,
                lens_external_view,
                input,
                &v.output_view,
                &mut blur_enc,
            )?;
            resources.queue.submit(Some(blur_enc.finish()));
            // The geometry intermediate was only kept alive for the blur pass.
            drop(geometry_lens_intermediate.take());
        }
        if let Some(t0) = start {
            log::info!(
                "lumina perf: render_to_vram {}x{} {:.2} ms",
                frame.width(),
                frame.height(),
                t0.elapsed().as_secs_f64() * 1000.0
            );
            if Self::perf_log_enabled() {
                eprintln!(
                    "LUMINA_PERF render_to_vram={:.2}ms {}x{}",
                    t0.elapsed().as_secs_f64() * 1000.0,
                    frame.width(),
                    frame.height()
                );
            }
        }
        Ok(())
    }

    /// Diagnostic/test helper: read the active VRAM output (the result of
    /// [`Self::render_to_vram`], including any GPU-RENDER-PARITY-1 post-tone
    /// stages) back to the CPU as a [`Frame`]. This is the counterpart of the
    /// readback-free present path and exists so the interactive VRAM chain has
    /// a byte-level regression net; the present path itself never calls it.
    pub fn readback_output_frame(&self) -> Result<Frame, GpuError> {
        let Some(resources) = self.resources.as_ref() else {
            return Err(GpuError::AdapterUnavailable(
                "no adapter for output readback".into(),
            ));
        };
        let mut guard = self.vram.lock().unwrap();
        let Some(v) = guard.active() else {
            return Err(GpuError::RenderFailed("vram not ready".into()));
        };
        let (width, height) = (v.width, v.height);
        // `v.output` is RGBA8; `aligned_bytes_per_row` already includes the
        // 4 bytes per texel (same 4× over-allocation class as the dark readback).
        let bytes_per_row = shaders::aligned_bytes_per_row(width);
        let staging = resources.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lumina-gpu-output-readback"),
            size: (bytes_per_row * height.max(1)) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = resources
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("lumina-gpu-output-readback-enc"),
            });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &v.output,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        resources.queue.submit(Some(enc.finish()));
        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        resources
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| GpuError::RenderFailed(format!("device poll: {e}")))?;
        rx.recv()
            .map_err(|e| GpuError::RenderFailed(format!("map channel: {e}")))?
            .map_err(|e| GpuError::RenderFailed(format!("buffer map: {e}")))?;
        let mapped = slice
            .get_mapped_range()
            .map_err(|e| GpuError::RenderFailed(format!("mapped view: {e}")))?;
        let row_bytes = (width * 4) as usize;
        let mut pixels = Vec::with_capacity(row_bytes * height as usize);
        for y in 0..height as usize {
            let start = y * bytes_per_row as usize;
            pixels.extend_from_slice(&mapped[start..start + row_bytes]);
        }
        drop(mapped);
        drop(guard);
        staging.unmap();
        Ok(Frame {
            width,
            height,
            pixels,
        })
    }

    /// Full-frame render entry point.
    ///
    /// When a real GPU adapter is bound this runs the color/tone fragment shader
    /// (`SHADER_SRC`) on the decoded [`ImageFrame`] uploaded as an `Rgba8Unorm`
    /// texture, then the [`stages`] post-tone chain, rendering into an
    /// `Rgba8Unorm` target and reading the result back into a [`Frame`]. The
    /// shaders mirror the integer-rounded per-channel math of the CPU oracle;
    /// the measured CPU↔GPU equivalence is pinned by `tests/golden.rs` (tone)
    /// and `tests/parity.rs` (post-tone stages).
    ///
    /// **Recipe validation (REVIEW-GPU-DIVERGENCE-1).** The tone shader runs
    /// white balance plus the seven tone sliders; the [`stages`] post-tone
    /// chain then runs Presence and the per-pixel color stages; the source-action
    /// and legacy spot-heal stages run before the tone pass. A schema-invalid
    /// recipe is rejected up front by [`validate_gpu_recipe`] with the CPU
    /// oracle's own error. When [`unsupported_gpu_stages`] reports any remaining
    /// unsupported stage (unbound SourceActions, generative edit, …), the render
    /// is **explicitly routed to the `render_cpu` fallback** rather than
    /// silently GPU-rendering with the stage dropped. The routing decision is
    /// logged once per unique reason set. Lens blur is GPU-rendered; a
    /// referenced external depth artifact without a bound plane is a loud error,
    /// not a fallback.
    ///
    /// **Complete CPU reference on fallback.** `render_cpu` (this method's
    /// fallback) runs the full `lumina_core::render_frame` chain — spot
    /// healing, every adjustment stage (including Red-Eye), the decoupled
    /// geometry stages (lens/fill/perspective/crop) and generative artifact
    /// compositing (which needs a caller-supplied artifact) — so
    /// a CPU-routed render is the complete reference, not a partial
    /// `apply_recipe`. Context the recipe-only API does not carry (mask layers,
    /// Lensfun correctors) stays the caller's responsibility (see
    /// [`unsupported_gpu_stages_with_context`]); the decoder As-Shot white
    /// balance and the external depth plane are carried through when the caller
    /// bound them via [`GpuContext::set_camera_white_balance`] /
    /// [`GpuContext::set_depth_plane`].
    ///
    /// This method only sees the recipe. Mask layers cannot be expressed here,
    /// so callers carrying them must consult
    /// [`unsupported_gpu_stages_with_context`] (plus their own context checks)
    /// *before* calling this method; the CLI/MCP routing mirrors do exactly
    /// that (R2-MCP-01). A caller-bound Lensfun map is carried into the geometry
    /// chain when bound via [`GpuContext::set_lensfun_map`] (a bound map makes
    /// the internal CPU fallback refuse loudly, since the free `render_cpu`
    /// reference has no corrector). The bound As-Shot context is validated here
    /// ([`GpuContext::set_camera_white_balance`]) with the oracle's own error.
    ///
    /// When no adapter is bound (or the `gpu` feature is disabled downstream) it
    /// likewise uses the `render_cpu` fallback, so the public API always returns
    /// a real [`Frame`].
    ///
    /// TODO(PERF): the current path copies the render target back to a CPU buffer
    /// via `map_async`. A later stage should present directly to a swapchain /
    /// write to a persistent VRAM `Frame` and only read back for export/preview.
    pub fn render_with_gpu(
        &self,
        frame: &ImageFrame,
        recipe: &EditRecipe,
    ) -> Result<Frame, GpuError> {
        self.render_with_gpu_inner(frame, recipe, None)
    }

    /// GEN-ONNX-1 Welle 2a: [`Self::render_with_gpu`] with the caller-supplied
    /// generative canvas artifacts.
    ///
    /// The artifact-aware entry point injects the composited canvases at the CPU
    /// oracle's mid-geometry positions (`Lens → [auto-fill] → Perspective → CA →
    /// [expand] → Crop`) instead of the former blanket CPU route. An active
    /// `expand` without an artifact, a role/dimension mismatch, or an
    /// artifact-blind call on an active generative recipe is a loud
    /// [`GpuError`] — never a silent unexpanded render.
    pub fn render_with_gpu_and_generative(
        &self,
        frame: &ImageFrame,
        recipe: &EditRecipe,
        generative: &lumina_core::GenerativeCanvasInput<'_>,
    ) -> Result<Frame, GpuError> {
        self.render_with_gpu_inner(frame, recipe, Some(generative))
    }

    fn render_with_gpu_inner(
        &self,
        frame: &ImageFrame,
        recipe: &EditRecipe,
        generative: Option<&lumina_core::GenerativeCanvasInput<'_>>,
    ) -> Result<Frame, GpuError> {
        // GPU-LENSFUN-PARITY-1: the free `render_cpu` reference does not carry a
        // caller-bound Lensfun map (the corrector is render-context state,
        // F-098-N1), so a fallback would silently drop the correction. Refuse
        // loudly instead — the caller owns the full CPU reference for a bound
        // map (the GUI presents the exact CPU frame itself).
        let cpu_fallback = |frame: &ImageFrame| {
            if self.lensfun_map.is_some() {
                return Err(GpuError::RenderFailed(
                    "a bound Lensfun map cannot use the internal CPU fallback; \
                     the caller owns the full CPU reference"
                        .into(),
                ));
            }
            render_cpu_with_generative(
                frame,
                recipe,
                self.depth_plane.as_ref().map(|plane| &plane.plane),
                self.camera_white_balance(),
                generative,
            )
        };
        // CAMERA-WB-WELLE (R2-MCP-01): validate the caller-bound As-Shot context
        // exactly like the CPU oracle (invalid gains abort before any pixel) and
        // thread it into every CPU-fallback render so a routed render keeps the
        // complete context contract. Valid gains are pixel-neutral on both
        // backends (the decoder already applied them).
        self.validate_bound_camera_white_balance()?;
        let Some(resources) = self.resources.as_ref() else {
            return cpu_fallback(frame);
        };
        // R2-GPU-06: a lost device must not panic — degrade to the CPU oracle.
        if resources
            .device_lost
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            log::warn!("GPU device lost; routing render_with_gpu to CPU");
            return cpu_fallback(frame);
        }
        // REVIEW-GPU-DIVERGENCE-1 / GPU-STAGE-1: never let the GPU path drop
        // recipe stages. Route to the CPU oracle loudly instead of rendering
        // different pixels — with one exception: when source-action artifacts
        // are bound and match the frame, the dedicated GPU source-action stage
        // composites them and `source_actions` is no longer unsupported.
        validate_gpu_recipe(recipe)?;
        let sa_bound = self
            .matching_source_actions(frame.width(), frame.height())
            .is_some();
        let unsupported = unsupported_gpu_stages_for(recipe, sa_bound);
        if !unsupported.is_empty() {
            log_cpu_routing_once(&unsupported, "render_with_gpu");
            return cpu_fallback(frame);
        }
        self.ensure_pipeline()?;
        let guard = self.pipeline.lock().unwrap();
        let Some(pipeline) = guard.as_ref() else {
            return cpu_fallback(frame);
        };

        let width = frame.width();
        let height = frame.height();

        // Upload the recipe sliders into the uniform buffer.
        let uniforms = shaders::Uniforms::from_recipe(recipe);
        shaders::write_uniforms(&resources.queue, &pipeline.uniform_buffer, &uniforms);

        // R2-GPU-04: reuse the input/output/readback for the same dimensions
        // across calls instead of re-creating them every render. The dominant
        // per-call cost on small frames is wgpu object churn; pooling removes
        // it. The blocking readback round-trip remains — the readback-free
        // present path (`render_to_vram` + `copy_vram_to_texture`) avoids it.
        let bytes_per_row = shaders::aligned_bytes_per_row(width);
        let mut cache_guard = self.rwgpu_cache.lock().unwrap();
        let pooled = cache_guard.entry((width, height)).or_insert_with(|| {
            let input =
                shaders::create_input_texture(&resources.device, width, height, "lumina-gpu-input");
            let output = shaders::create_output_texture(
                &resources.device,
                width,
                height,
                "lumina-gpu-output",
            );
            let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
            let readback = shaders::create_readback_buffer(
                &resources.device,
                width,
                height,
                "lumina-gpu-readback",
            );
            RenderWithGpuResources {
                input,
                output,
                output_view,
                readback,
                input_source_identity: None,
            }
        });
        // Upload the current source frame into the pooled input texture — but
        // only when the source actually changed (R2-GPU-04, mirroring
        // R2-GPU-01). A benchmark loop or repeated exports of the same frame
        // skip the ≈16 MB CPU→GPU transfer on every call after the first.
        let src_identity = (frame.pixels().as_ptr() as usize, frame.pixels().len());
        if pooled.input_source_identity != Some(src_identity) {
            resources.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &pooled.input,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                frame.pixels(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            pooled.input_source_identity = Some(src_identity);
        }
        let input_view = pooled
            .input
            .create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = shaders::create_sampler(&resources.device, "lumina-gpu-sampler");
        let output_texture = &pooled.output;
        let output_view = &pooled.output_view;
        let readback = &pooled.readback;

        // GPU-RENDER-PARITY-1 geometry wave: plan the lens/perspective/CA/crop/
        // rotation/mirror chain (including its dimension math) before encoding.
        // A plan error mirrors the CPU oracle's own rejection.
        //
        // GEN-ONNX-1 Welle 2a: an artifact-aware caller injects the generative
        // canvas via `Substitute` steps at the oracle's mid-geometry positions.
        // Without artifacts an active generative role is a loud plan error (no
        // silent unexpanded render).
        let plan_input = generative
            .map(|g| geometry::GenerativePlanInput {
                auto_fill: g.auto_fill.map(|a| (a.frame.width(), a.frame.height())),
                expand: g.expand.map(|a| (a.frame.width(), a.frame.height())),
            })
            .unwrap_or_default();
        let geometry_plan = geometry::GeometryPlan::from_recipe_with(
            recipe,
            width,
            height,
            &plan_input,
            generative.is_some(),
            self.lensfun_map.as_ref().map(|bound| &bound.map),
        )?;
        let (plan_out_width, plan_out_height) =
            geometry_plan.as_ref().map_or((width, height), |plan| {
                (plan.output_width, plan.output_height)
            });

        // GPU-RENDER-PARITY-1 lens-blur wave: the pass runs after geometry at
        // the oracle's post-crop dimensions. `external_depth_view_for` mirrors
        // `apply_lens_blur`'s ordering — a referenced depth artifact without a
        // matching bound plane is a hard error even when the blur would round
        // to radius 0. The pass itself is skipped for identity configurations
        // (`radius <= 0` or an empty weight band).
        let lens_blur_recipe = recipe
            .lens_blur
            .as_ref()
            .filter(|blur| lens_blur::stage_active(blur));
        let lens_external_view =
            self.external_depth_view_for(recipe, plan_out_width, plan_out_height)?;
        let lens_blur_pass = lens_blur_recipe.is_some_and(|blur| {
            lens_blur::radius_for(blur.blur_amount) > 0 && !lens_blur::has_no_effect(blur)
        });

        // GPU-RENDER-PARITY-1: recipes that use post-tone stages (Presence,
        // curves/HSL/Point Color/vibrance/saturation/Color Grading) render the
        // tone pass into a transient texture first; the post chain then writes
        // the final pixels into `output`. Without post stages the tone pass
        // keeps writing `output` directly.
        let needs_post = post_stages_needed(recipe);
        let tone_texture: Option<wgpu::Texture> = if needs_post {
            Some(shaders::create_output_texture(
                &resources.device,
                width,
                height,
                "lumina-gpu-tone-intermediate",
            ))
        } else {
            None
        };
        let tone_view_owned: Option<wgpu::TextureView> = tone_texture
            .as_ref()
            .map(|texture| texture.create_view(&wgpu::TextureViewDescriptor::default()));
        let tone_target_view: &wgpu::TextureView = tone_view_owned.as_ref().unwrap_or(output_view);

        // Bind group: uniform (0) + tone input texture (1) + sampler (2).
        // Built below once the (possibly source-action-composited) tone input
        // view is chosen — see `tone_bind`.

        // Encode: draw the fullscreen triangle into the RGBA8 target, then copy
        // it back to the staging buffer.
        let mut encoder =
            resources
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("lumina-gpu-encode"),
                });
        // GPU-STAGE-1 / GPU-RENDER-PARITY-1 item 7: composite bound
        // source-action artifacts into an intermediate target first, in batches
        // of `MAX_SOURCE_ACTIONS`; then the spot-heal pass; the tone pass
        // samples the result.
        let sa_batches: Option<SourceActionBatches> =
            if self.matching_source_actions(width, height).is_some() {
                self.ensure_source_action_pipeline()?;
                let sa_guard = self.sa_pipeline.lock().unwrap();
                let Some(sa) = sa_guard.as_ref() else {
                    return Err(GpuError::RenderFailed(
                        "source-action pipeline not built".into(),
                    ));
                };
                // R2-GPU-03: reuse the cached region/replacement textures
                // uploaded in `set_source_action_artifacts` instead of
                // re-creating + re-uploading them every render call.
                let sa_cache = self
                    .sa_textures
                    .as_ref()
                    .expect("source-action textures are cached when artifacts are bound");
                Some(encode_source_action_batches(
                    resources,
                    sa,
                    sa_cache,
                    &input_view,
                    width,
                    height,
                    &mut encoder,
                )?)
            } else {
                None
            };
        let sa_view: &wgpu::TextureView = match sa_batches.as_ref() {
            Some(batches) => batches.final_view(),
            None => &input_view,
        };
        // Spot heal (GPU-RENDER-PARITY-1 follow-up): the legacy
        // `extras["spot_removals"]` geometry is applied before the tone pass,
        // exactly like `apply_spot_heals_from_recipe` in `render_frame_from_base`.
        let spots = lumina_core::spots_from_recipe(recipe);
        let spot_texture: Option<wgpu::Texture> = if spots.is_empty() {
            None
        } else {
            Some(shaders::create_output_texture(
                &resources.device,
                width,
                height,
                "lumina-gpu-spot-out",
            ))
        };
        let spot_view_owned: Option<wgpu::TextureView> = spot_texture
            .as_ref()
            .map(|texture| texture.create_view(&wgpu::TextureViewDescriptor::default()));
        let tone_input_view: &wgpu::TextureView = match spot_view_owned.as_ref() {
            Some(spot_view) => {
                self.ensure_spot_pipeline()?;
                let spot_guard = self.spot_pipeline.lock().unwrap();
                let Some(spot) = spot_guard.as_ref() else {
                    return Err(GpuError::RenderFailed("spot pipeline not built".into()));
                };
                encode_spot_heal(resources, spot, &spots, sa_view, spot_view, &mut encoder);
                spot_view
            }
            None => sa_view,
        };
        let tone_bind = shaders::create_color_tone_bind_group(
            &resources.device,
            &pipeline.bind_group_layout,
            &pipeline.uniform_buffer,
            tone_input_view,
            &sampler,
        );
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lumina-gpu-color-tone"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: tone_target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline.pipeline);
            pass.set_bind_group(0, &tone_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        if needs_post {
            // Flush source-action + tone first: `render_post_stages` samples the
            // tone result and writes `output` (submitting its own encoders).
            resources.queue.submit(Some(encoder.finish()));
            self.render_post_stages(
                resources,
                width,
                height,
                recipe,
                tone_target_view,
                output_view,
            )?;
            encoder = resources
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("lumina-gpu-readback-enc"),
                });
        }
        // GEN-ONNX-1 Welle 2a: upload the caller's generative canvases once so
        // the geometry chain can substitute them without a shader pass.
        let uploaded_artifacts: [Option<UploadedGenerativeArtifact>; 2] = match generative {
            Some(generative) if geometry_plan.is_some() => {
                upload_generative_artifacts(resources, generative)?
            }
            _ => [None, None],
        };
        if let Some(plan) = geometry_plan.as_ref() {
            // GPU-RENDER-PARITY-1 geometry wave: run the lens/perspective/CA/
            // crop/rotation/mirror chain after the tone/post result. The final
            // texture carries the oracle's output dimensions, so it is read back
            // directly (the pooled readback buffer is sized for the source dims).
            let final_texture = shaders::create_output_texture(
                &resources.device,
                plan.output_width,
                plan.output_height,
                "lumina-gpu-geometry-out",
            );
            let final_view = final_texture.create_view(&wgpu::TextureViewDescriptor::default());
            {
                let geo_guard = self.ensure_geometry_pipelines()?;
                let Some(geo) = geo_guard.as_ref() else {
                    return Err(GpuError::RenderFailed("geometry pipeline not built".into()));
                };
                self.encode_geometry_chain(
                    resources,
                    geo,
                    plan,
                    output_view,
                    &final_texture,
                    &final_view,
                    &uploaded_artifacts,
                    &mut encoder,
                )?;
            }
            resources.queue.submit(Some(encoder.finish()));
            // G-05 lens blur: a sub-stage of Crop, i.e. after the geometry
            // chain and before output. It preserves the geometry output's
            // dimensions, so it runs as one more fullscreen pass and is read
            // back directly at those dimensions.
            if lens_blur_pass {
                let blur = lens_blur_recipe.expect("pass implies an active recipe");
                let blur_texture = shaders::create_output_texture(
                    &resources.device,
                    plan.output_width,
                    plan.output_height,
                    "lumina-gpu-lens-blur-out",
                );
                let blur_view = blur_texture.create_view(&wgpu::TextureViewDescriptor::default());
                let mut blur_encoder =
                    resources
                        .device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("lumina-gpu-lens-blur"),
                        });
                self.encode_lens_blur_stage(
                    resources,
                    blur,
                    lens_external_view,
                    &final_view,
                    &blur_view,
                    &mut blur_encoder,
                )?;
                resources.queue.submit(Some(blur_encoder.finish()));
                drop(cache_guard);
                return readback_texture(
                    resources,
                    &blur_texture,
                    plan.output_width,
                    plan.output_height,
                );
            }
            drop(cache_guard);
            return readback_texture(
                resources,
                &final_texture,
                plan.output_width,
                plan.output_height,
            );
        }
        // No geometry: the tone/post result lives in `output_view`. Run the
        // lens blur into a fresh texture (its input is `output_view`, so it
        // cannot write the same texture) and read that back instead.
        let mut blur_texture: Option<wgpu::Texture> = None;
        if lens_blur_pass {
            let blur = lens_blur_recipe.expect("pass implies an active recipe");
            let texture = shaders::create_output_texture(
                &resources.device,
                width,
                height,
                "lumina-gpu-lens-blur-out",
            );
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.encode_lens_blur_stage(
                resources,
                blur,
                lens_external_view,
                output_view,
                &view,
                &mut encoder,
            )?;
            blur_texture = Some(texture);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: blur_texture.as_ref().unwrap_or(output_texture),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        resources.queue.submit(Some(encoder.finish()));

        // Map the staging buffer and copy out the RGBA8 rows (stripping any
        // 256-byte-row padding).
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        // wgpu 25+ removed `Maintain`; `Device::poll(PollType)` returns a
        // `Result` and blocks until the mapped read is complete.
        resources
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| GpuError::RenderFailed(format!("device poll: {e}")))?;
        rx.recv()
            .map_err(|e| GpuError::RenderFailed(format!("map channel: {e}")))?
            .map_err(|e| GpuError::RenderFailed(format!("buffer map: {e}")))?;

        let mapped = slice
            .get_mapped_range()
            .map_err(|e| GpuError::RenderFailed(format!("mapped view: {e}")))?;
        let row_bytes = (width * 4) as usize;
        let mut pixels = Vec::with_capacity(row_bytes * height as usize);
        for y in 0..height as usize {
            let start = y * bytes_per_row as usize;
            pixels.extend_from_slice(&mapped[start..start + row_bytes]);
        }
        drop(mapped);
        drop(guard);
        readback.unmap();

        Ok(Frame {
            width,
            height,
            pixels,
        })
    }

    /// Draft render for an interactive viewport (ROI) using the draft pyramid.
    ///
    /// Sets up the GPU scaffolding (pipeline + uniforms + ROI tile set) and
    /// produces real pixels through the CPU reference (the draft path stays
    /// on the CPU scaffold even though the tone shader stage exists). The
    /// tile set is logged so the parallel tiling subagent has a concrete
    /// call site to plug into.
    pub fn render_draft(
        &self,
        frame: &ImageFrame,
        viewport: crate::tiling::Viewport,
    ) -> Result<Frame, GpuError> {
        // No adapter → CPU fallback via lumina-core (keeps the non-GPU path
        // correct and is the test oracle for the GPU stages).
        let Some(resources) = self.resources.as_ref() else {
            return self.render_draft_cpu(frame);
        };

        // GPU draft-path scaffolding: if a pipeline was built (via
        // `create_pipeline`/`update_uniforms`), push the current recipe into the
        // uniform buffer; then compute the ROI tile set against the draft pyramid.
        if let Some(recipe) = self.recipe.as_ref() {
            let uniforms = shaders::Uniforms::from_recipe(recipe);
            if let Some(pipeline) = self.pipeline.lock().unwrap().as_ref() {
                shaders::write_uniforms(&resources.queue, &pipeline.uniform_buffer, &uniforms);
            }
        }
        let zoom = (frame.width() as f32 / viewport.width.max(1.0)).clamp(0.01, 100.0);
        let pyramid = crate::tiling::DraftPyramid::new(frame.width(), frame.height());
        let lvl = pyramid.level_for_zoom(zoom);
        // REVIEW-GPU-LEVELS-1: the ROI expansion routes through the pyramid, so
        // the logged level and the produced tile keys can no longer diverge.
        let cache = crate::tiling::TiledCache::new(64);
        let keys = cache.keys_for_viewport(&pyramid, &viewport, zoom);
        log::debug!(
            "render_draft: gpu scaffold (adapter present), {} tiles for viewport {:?} @ zoom {:.3} (pyramid level {}, cache generation {})",
            keys.len(),
            viewport,
            zoom,
            lvl,
            cache.generation()
        );
        // Real GPU tile upload + draw is filled in by the shader/tiling subagents.
        // Bootstrapping: produce real pixels via the CPU reference.
        self.render_draft_cpu(frame)
    }

    /// CPU fallback that uses the recipe stored via [`update_uniforms`], or the
    /// untouched frame when none has been set. Runs the full CPU reference
    /// chain (the free `render_cpu`), not just `apply_recipe`.
    fn render_draft_cpu(&self, frame: &ImageFrame) -> Result<Frame, GpuError> {
        match self.recipe.as_ref() {
            Some(recipe) => render_cpu(
                frame,
                recipe,
                self.depth_plane.as_ref().map(|plane| &plane.plane),
                self.camera_white_balance(),
            ),
            None => Ok(Frame::from_image_frame(frame.clone())),
        }
    }
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

#[cfg(not(feature = "gpu"))]
impl GpuContext {
    /// Create a CPU-only context (the `gpu` feature is disabled, so no adapter
    /// is ever bound). Rendering always uses the CPU fallback.
    pub fn new() -> Result<Self, GpuError> {
        Ok(Self {
            camera_white_balance: std::sync::Mutex::new(None),
        })
    }

    /// Always `false` without the `gpu` feature.
    pub fn is_available(&self) -> bool {
        false
    }

    /// Always `None` without the `gpu` feature (no adapter can be bound).
    pub fn adapter_info(&self) -> Option<String> {
        None
    }

    /// CPU fallback render (the only path when the `gpu` feature is off): runs
    /// the full CPU reference chain (the free `render_cpu`).
    pub fn render_with_gpu(
        &self,
        frame: &ImageFrame,
        recipe: &EditRecipe,
    ) -> Result<Frame, GpuError> {
        render_cpu(frame, recipe, None, self.camera_white_balance())
    }

    /// GEN-ONNX-1 Welle 2a: without the `gpu` feature every render is the full
    /// CPU reference; the generative canvas artifacts are carried through so the
    /// artifact-aware contract is identical to a GPU build.
    pub fn render_with_gpu_and_generative(
        &self,
        frame: &ImageFrame,
        recipe: &EditRecipe,
        generative: &lumina_core::GenerativeCanvasInput<'_>,
    ) -> Result<Frame, GpuError> {
        render_cpu_with_generative(
            frame,
            recipe,
            None,
            self.camera_white_balance(),
            Some(generative),
        )
    }

    pub fn perf_log_enabled() -> bool {
        false
    }
    pub fn ensure_vram(&self, _w: u32, _h: u32) -> Result<(), GpuError> {
        Ok(())
    }
    pub fn render_to_vram(&self, _f: &ImageFrame, _r: &EditRecipe) -> Result<(), GpuError> {
        Ok(())
    }
    pub fn upload_mask_tile(
        &self,
        _x: u32,
        _y: u32,
        _w: u32,
        _h: u32,
        _d: &[u8],
    ) -> Result<(), GpuError> {
        Ok(())
    }
    pub fn copy_vram_to_texture(&self, _d: &(), _overlay_color: [u8; 3]) -> Result<(), GpuError> {
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
/// (Agents.md: CPU bleibt vollständige Referenz; kein stiller Fallback).
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

/// Build all post-tone pipelines targeting [`shaders::RGBA8_FORMAT`].
#[cfg(feature = "gpu")]
fn build_post_pipelines(device: &wgpu::Device) -> Result<PostPipelineState, GpuError> {
    let format = shaders::RGBA8_FORMAT;
    Ok(PostPipelineState {
        color_pipeline: stages::create_color_pipeline(device, format)?,
        color_layout: stages::create_color_bind_group_layout(device),
        color_params: stages::create_color_params_buffer(device),
        dog_pipeline: stages::create_dog_pipeline(device, format)?,
        dog_layout: stages::create_dog_bind_group_layout(device),
        dark_pipeline: stages::create_dark_pipeline(device, format)?,
        dark_layout: stages::create_dark_bind_group_layout(device),
        dehaze_pipeline: stages::create_dehaze_pipeline(device, format)?,
        dehaze_layout: stages::create_dehaze_bind_group_layout(device),
        dehaze_params: stages::create_dehaze_params_buffer(device),
        noise_pipeline: stages::create_noise_pipeline(device, format)?,
        noise_layout: stages::create_noise_bind_group_layout(device),
        noise_params: stages::create_noise_params_buffer(device),
        vignette_pipeline: stages::create_vignette_pipeline(device, format)?,
        vignette_layout: stages::create_vignette_bind_group_layout(device),
        vignette_params: stages::create_vignette_params_buffer(device),
        grain_pipeline: stages::create_grain_pipeline(device, format)?,
        grain_layout: stages::create_grain_bind_group_layout(device),
        grain_params: stages::create_grain_params_buffer(device),
        sharpen_blur_pipeline: stages::create_sharpen_blur_pipeline(
            device,
            stages::SHARPEN_SCALAR_FORMAT,
        )?,
        sharpen_blur_layout: stages::create_sharpen_blur_bind_group_layout(device),
        sharpen_apply_pipeline: stages::create_sharpen_apply_pipeline(device, format)?,
        sharpen_apply_layout: stages::create_sharpen_apply_bind_group_layout(device),
        sharpen_gradient_pipeline: stages::create_sharpen_gradient_pipeline(device)?,
        sharpen_gradient_layout: stages::create_sharpen_gradient_bind_group_layout(device),
        sharpen_params: stages::create_sharpen_params_buffer(device),
        sharpen_maxg: stages::create_sharpen_maxg_buffer(device),
        red_eye_pipeline: stages::create_red_eye_pipeline(device, format)?,
        red_eye_layout: stages::create_red_eye_bind_group_layout(device),
        red_eye_params: stages::create_red_eye_params_buffer(device),
    })
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

/// Whether the recipe uses any post-tone adjustment stage the GPU renders in
/// [`GpuContext::render_post_stages`]. The tone target is chosen from this so a
/// recipe that needs no post pass keeps rendering straight into the output.
/// Each arm mirrors the corresponding `PostWrite` enqueue exactly, so the
/// chosen tone target can never diverge from the passes that run.
#[cfg(feature = "gpu")]
fn post_stages_needed(recipe: &EditRecipe) -> bool {
    let presence = recipe.presence.is_some_and(|presence| {
        presence.texture != 0.0 || presence.clarity != 0.0 || presence.dehaze != 0.0
    });
    let noise = recipe
        .noise_reduction
        .as_ref()
        .is_some_and(stages::NoiseParams::needs_stage);
    let sharpen = recipe
        .sharpening
        .as_ref()
        .is_some_and(stages::SharpenParams::needs_stage);
    let effects = recipe.effects.as_ref().is_some_and(|fx| {
        fx.vignette.as_ref().is_some_and(|v| v.amount != 0.0)
            || fx.grain.as_ref().is_some_and(|g| g.amount != 0.0)
    });
    let red_eye = recipe
        .red_eye
        .as_ref()
        .is_some_and(stages::RedEyeParams::needs_stage);
    presence || stages::ColorParams::needs_stage(recipe) || noise || sharpen || red_eye || effects
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

/// Encode the (possibly batched) source-action composites into `encoder`.
///
/// WGSL cannot index texture bindings dynamically, so the stage unrolls
/// `MAX_SOURCE_ACTIONS` slot pairs per pass. More artifacts are composited in
/// sequential batches, each sampling the previous batch's output — matching the
/// oracle's sequential `apply_source_actions` order (later artifacts win on
/// overlap) without the former hard slot limit (GPU-RENDER-PARITY-1 item 7).
#[cfg(feature = "gpu")]
#[allow(clippy::type_complexity)]
fn encode_source_action_batches(
    resources: &GpuResources,
    sa: &SourceActionPipelineState,
    cached: &[(
        wgpu::Texture,
        wgpu::TextureView,
        wgpu::Texture,
        wgpu::TextureView,
    )],
    input_view: &wgpu::TextureView,
    width: u32,
    height: u32,
    encoder: &mut wgpu::CommandEncoder,
) -> Result<SourceActionBatches, GpuError> {
    let total = cached.len();
    let batch_count = total.div_ceil(MAX_SOURCE_ACTIONS);
    let mut textures = Vec::with_capacity(batch_count);
    for index in 0..batch_count {
        let texture = shaders::create_output_texture(
            &resources.device,
            width,
            height,
            &format!("lumina-gpu-sa-batch-{index}"),
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        textures.push((texture, view));
    }
    let mut current: &wgpu::TextureView = input_view;
    for (batch, (_, dst_view)) in textures.iter().enumerate() {
        let start = batch * MAX_SOURCE_ACTIONS;
        let end = (start + MAX_SOURCE_ACTIONS).min(total);
        let count = end - start;
        // A dedicated uniform buffer per batch: multiple batches share one
        // encoder, so a shared buffer would make a later `write_buffer`
        // retroactively change an earlier draw.
        let uniforms = shaders::create_source_action_uniform_buffer(&resources.device);
        shaders::write_source_action_uniforms(
            &resources.queue,
            &uniforms,
            &shaders::SourceActionUniforms {
                count: count as u32,
                _pad: [0; 3],
            },
        );
        let region_views: Vec<&wgpu::TextureView> =
            cached[start..end].iter().map(|entry| &entry.1).collect();
        let replacement_views: Vec<&wgpu::TextureView> =
            cached[start..end].iter().map(|entry| &entry.3).collect();
        let bind = shaders::create_source_action_bind_group(
            &resources.device,
            &sa.bind_group_layout,
            &uniforms,
            current,
            &region_views,
            &replacement_views,
            count as u32,
        );
        encode_fullscreen_pass(encoder, &sa.pipeline, &bind, dst_view);
        current = dst_view;
    }
    Ok(SourceActionBatches {
        final_index: batch_count - 1,
        textures,
    })
}

/// Encode the spot-heal pass (GPU-RENDER-PARITY-1 follow-up) into `encoder`,
/// sampling `input_view` and writing `dst`.
///
/// `spots` are the validated legacy heal entries; the caller only invokes this
/// when the list is non-empty (an empty list is the oracle's identity).
#[cfg(feature = "gpu")]
fn encode_spot_heal(
    resources: &GpuResources,
    spot: &SpotPipelineState,
    spots: &[lumina_core::SpotHeuristic],
    input_view: &wgpu::TextureView,
    dst: &wgpu::TextureView,
    encoder: &mut wgpu::CommandEncoder,
) {
    let params = stages::create_spot_heal_buffer(&resources.device, spots.len());
    resources
        .queue
        .write_buffer(&params, 0, &stages::spot_heal_params_bytes(spots));
    let bind = stages::create_spot_heal_bind_group(
        &resources.device,
        &spot.bind_group_layout,
        &params,
        input_view,
    );
    encode_fullscreen_pass(encoder, &spot.pipeline, &bind, dst);
}

/// Encode the G-05 lens-blur pass (GPU-RENDER-PARITY-1, lens-blur wave) into
/// `encoder`, sampling `input_view` and writing `dst`.
///
/// The caller only invokes this for an active, non-identity stage
/// (`radius > 0`, at least one non-zero weight) at the **post-geometry** frame
/// dimensions. `depth_view` is the caller-bound external depth plane when the
/// recipe references one; the pipeline's 1×1 dummy is passed otherwise (the
/// shader's `use_external == 0` branch never samples it).
#[cfg(feature = "gpu")]
#[allow(clippy::too_many_arguments)]
fn encode_lens_blur(
    resources: &GpuResources,
    state: &lens_blur::LensBlurPipelineState,
    blur: &lumina_sidecar::LensBlur,
    depth_view: &wgpu::TextureView,
    input_view: &wgpu::TextureView,
    dst: &wgpu::TextureView,
    encoder: &mut wgpu::CommandEncoder,
) {
    let radius = lens_blur::radius_for(blur.blur_amount);
    let taps = lens_blur::bokeh_kernel(blur.bokeh, radius);
    let params = lens_blur::LensBlurParams::from_blur(blur, blur.depth_artifact.is_some());
    let params_buffer = lens_blur::create_params_buffer(&resources.device);
    resources
        .queue
        .write_buffer(&params_buffer, 0, bytemuck::bytes_of(&params));
    let taps_buffer = lens_blur::create_taps_buffer(&resources.device, taps.len());
    resources
        .queue
        .write_buffer(&taps_buffer, 0, &lens_blur::taps_bytes(&taps));
    let bind = lens_blur::create_bind_group(
        &resources.device,
        &state.layout,
        &params_buffer,
        input_view,
        depth_view,
        &taps_buffer,
    );
    encode_fullscreen_pass(encoder, &state.pipeline, &bind, dst);
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
    /// Identity (`(pixels_ptr, pixels_len)`) of the last frame uploaded into
    /// `input`, so repeated renders of the same source skip the CPU→GPU upload
    /// (R2-GPU-04, mirroring R2-GPU-01).
    input_source_identity: Option<(usize, usize)>,
}

/// GEN-ONNX-1 Welle 2a: one caller-supplied generative canvas uploaded to VRAM
/// for the geometry-chain substitution (`Substitute` step).
#[cfg(feature = "gpu")]
struct UploadedGenerativeArtifact {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// Upload the caller's generative artifacts (auto-fill at index 0, expand at
/// index 1) as `Rgba8Unorm` textures, validating role and dimensions. Missing
/// roles stay `None` so the fixed indices the geometry plan uses are stable.
#[cfg(feature = "gpu")]
fn upload_generative_artifacts(
    resources: &GpuResources,
    generative: &lumina_core::GenerativeCanvasInput<'_>,
) -> Result<[Option<UploadedGenerativeArtifact>; 2], GpuError> {
    use lumina_core::GenerativeRole;
    let mut slots: [Option<UploadedGenerativeArtifact>; 2] = [None, None];
    let upload = |artifact: &lumina_core::GenerativeCanvasArtifact,
                  expected_role: GenerativeRole,
                  label: &str|
     -> Result<UploadedGenerativeArtifact, GpuError> {
        if artifact.role != expected_role {
            return Err(GpuError::RenderFailed(format!(
                "{label}: generative artifact role {:?} does not match {expected_role:?}",
                artifact.role
            )));
        }
        let frame = &artifact.frame;
        if frame.pixels().len() != (frame.width() as usize) * (frame.height() as usize) * 4 {
            return Err(GpuError::RenderFailed(format!(
                "{label}: generative artifact pixel length does not match {}x{}",
                frame.width(),
                frame.height()
            )));
        }
        let texture = resources.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: frame.width(),
                height: frame.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        resources.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            frame.pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(frame.width() * 4),
                rows_per_image: Some(frame.height()),
            },
            wgpu::Extent3d {
                width: frame.width(),
                height: frame.height(),
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Ok(UploadedGenerativeArtifact { texture, view })
    };
    if let Some(artifact) = generative.auto_fill {
        slots[0] = Some(upload(
            artifact,
            GenerativeRole::AutoFillTransparent,
            "auto-fill artifact",
        )?);
    }
    if let Some(artifact) = generative.expand {
        slots[1] = Some(upload(artifact, GenerativeRole::Expand, "expand artifact")?);
    }
    Ok(slots)
}

/// Build the color/tone render pipeline: uniform buffer, a 3-entry bind group
/// layout (uniform block + input texture + sampler), the pipeline layout and the
/// real WGSL color/tone shader targeting `Rgba8Unorm`.
#[cfg(feature = "gpu")]
fn build_pipeline(resources: &GpuResources) -> Result<PipelineState, GpuError> {
    let device = &resources.device;
    let uniform_buffer = shaders::create_uniform_buffer(device);
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lumina-gpu-bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("lumina-gpu-pl"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lumina-gpu-shader"),
        source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("lumina-gpu-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: shaders::RGBA8_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    Ok(PipelineState {
        pipeline,
        uniform_buffer,
        bind_group_layout,
        _pipeline_layout: pipeline_layout,
    })
}

/// Select the wgpu backend set for the standalone (CLI/MCP/test) GPU init
/// (R2-GPU-07).
///
/// Historically this was hard-restricted to `wgpu::Backends::METAL`, which
/// silently denied GPU acceleration to Windows/Linux CLI/MCP consumers (their
/// adapters are Vulkan/DX12). We now default to `wgpu::Backends::all()` so
/// wgpu's own adapter enumeration picks the best available backend per
/// platform; an explicit `LUMINA_GPU_BACKENDS` (`metal`|`vulkan`|`dx12`|`gl`|`all`)
/// can narrow the set (e.g. to pin Metal on Apple Silicon). The GUI path
/// (`from_parts`) is unaffected — it reuses the eframe renderer's already-chosen
/// backend.
#[cfg(feature = "gpu")]
fn select_backends() -> wgpu::Backends {
    match std::env::var("LUMINA_GPU_BACKENDS").as_deref() {
        Ok("metal") => wgpu::Backends::METAL,
        Ok("vulkan") => wgpu::Backends::VULKAN,
        Ok("dx12") => wgpu::Backends::DX12,
        Ok("gl") => wgpu::Backends::GL,
        _ => wgpu::Backends::all(),
    }
}

/// Register wgpu error handlers so GPU problems never take down the app
/// (R2-GPU-06).
///
/// wgpu's default uncaptured-error handler panics (debug) / logs (release); a
/// shader/validation error from `lumina-gpu` would otherwise crash the GUI. We
/// install a non-panicking handler that logs loudly instead of panicking. For
/// device loss we install the dedicated device-lost callback, which flips
/// [`GpuResources::device_lost`] so the render methods degrade to the CPU
/// pipeline instead of producing further errors.
#[cfg(feature = "gpu")]
fn register_device_handlers(resources: &mut GpuResources) {
    resources
        .device
        .on_uncaptured_error(std::sync::Arc::new(move |err: wgpu::Error| {
            // Log loudly but never panic: the GPU path is an accelerator, and the
            // render methods fall back to the CPU oracle on any error.
            match &err {
                wgpu::Error::OutOfMemory { .. } => {
                    log::error!("GPU out of memory: {err}");
                }
                _ => {
                    log::warn!(
                    "GPU uncaptured error (GPU path will fall back to CPU where possible): {err}"
                );
                }
            }
        }));
    // Explicit device-lost callback (R2-GPU-06): when the adapter/device is
    // revoked (driver update, GPU reset, monitor unplug), flip the flag so
    // subsequent renders route to the CPU path.
    let lost_flag = resources.device_lost.clone();
    resources.device.set_device_lost_callback(
        move |reason: wgpu::DeviceLostReason, message: String| {
            log::error!("GPU device lost: {reason:?} - {message}");
            lost_flag.store(true, std::sync::atomic::Ordering::SeqCst);
        },
    );
}

/// Enumerate a GPU adapter and create a device/queue.
///
/// Enumerates all available backends (R2-GPU-07) for the M-series native path
/// and beyond. Returns [`GpuError::AdapterUnavailable`] when no adapter
/// matches, and [`GpuError::DeviceUnavailable`] when device/queue creation
/// fails — callers are expected to treat either as "use the CPU fallback".
#[cfg(feature = "gpu")]
fn init_gpu_resources() -> Result<GpuResources, GpuError> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        // Metal is the primary backend on Apple Silicon, but we now enumerate all
        // available backends so Windows/Linux CLI/MCP consumers also get GPU
        // acceleration (R2-GPU-07). `LUMINA_GPU_BACKENDS` can pin a specific
        // backend.
        backends: select_backends(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });

    // Since wgpu 25, `request_adapter` returns a `Result` with a descriptive
    // error for the "no adapter" case. The cause is preserved in the payload so
    // the CPU-fallback warning (`GpuContext::new`) can surface it.
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .map_err(|err| GpuError::AdapterUnavailable(format!("no GPU adapter found: {err}")))?;

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("lumina-gpu"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .map_err(|err| GpuError::DeviceUnavailable(err.to_string()))?;

    let mut resources = GpuResources {
        instance: std::mem::ManuallyDrop::new(instance),
        adapter: std::mem::ManuallyDrop::new(adapter),
        device: std::mem::ManuallyDrop::new(device),
        queue: std::mem::ManuallyDrop::new(queue),
        device_lost: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    register_device_handlers(&mut resources);
    Ok(resources)
}

#[cfg(test)]
mod tests;
