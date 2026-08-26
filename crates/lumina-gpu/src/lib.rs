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
//! transparently falls back to the CPU pipeline in `lumina-core` when no adapter
//! is present (or the `gpu` feature is disabled). The shader mirrors the
//! integer-rounded per-channel math of `lumina-core::apply_channel_lut_adjustments`,
//! so the GPU and CPU outputs agree within the golden-image tolerance
//! (maxAbsDiff ≤ 1, PSNR ≥ 45 dB) for the stages the shader implements.
//!
//! **No silent divergence (REVIEW-GPU-DIVERGENCE-1).** The tone stage implements
//! only WB + seven sliders. [`unsupported_gpu_stages`] lists any recipe stage the
//! shader cannot render (Curves, HSL, Presence, Vibrance/Saturation, Effects,
//! Geometry, SourceActions, …); `render_with_gpu` then explicitly routes the
//! render to the full CPU pipeline and logs once per reason set — the GPU is an
//! accelerator, never a semantic change.
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
use lumina_sidecar::{CurvePoint, Curves, EditRecipe, HslAdjustments};
use thiserror::Error;

// Shader + tiling modules are scaffolded (empty) so parallel subagents can fill
// them in without touching this file. They are GPU-specific, hence gated.
#[cfg(feature = "gpu")]
pub mod shaders;
#[cfg(feature = "gpu")]
pub mod tiling;

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
        let ImageFrame {
            width,
            height,
            pixels,
        } = frame;
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

/// Adjustment keys the GPU color/tone stage actually implements.
///
/// This is the exact key set of `lumina-core::apply_channel_lut_adjustments`
/// mirrored by the WGSL shader. Note that `vibrance`/`saturation` have uniform
/// fields but are **not** applied by the shader yet, so they are deliberately
/// absent here.
const GPU_SUPPORTED_ADJUSTMENT_KEYS: [&str; 8] = [
    "exposure",
    "contrast",
    "highlights",
    "shadows",
    "whites",
    "blacks",
    "wb_temperature",
    "wb_tint",
];

/// Maximum number of source-action artifacts the GPU source-action stage can
/// composite in one pass. WGSL cannot index texture bindings dynamically, so
/// the shader unrolls exactly this many slot pairs guarded by a uniform count.
/// Recipes referencing more actions than this route to the CPU pipeline.
///
/// Lives outside the `gpu` feature gate so the recipe-support validator
/// ([`unsupported_gpu_stages_for`]) can reference it in pure-CPU builds too.
///
/// **Why 7:** wgpu's default `max_sampled_textures_per_shader_stage` limit is
/// 16. The stage needs 1 base texture + `2 × N` artifact textures; `N = 7`
/// stays within the default (15), so no non-default device limits are
/// required.
pub const MAX_SOURCE_ACTIONS: usize = 7;

/// Lists the recipe stages the current GPU color/tone stage cannot render.
///
/// An empty result means the GPU path produces pixels within the documented
/// golden tolerance (maxAbsDiff ≤ 1, PSNR ≥ 45 dB) of the CPU oracle. A
/// non-empty result means running the GPU path would **silently drop** those
/// stages and produce different pixels than every CPU build — callers must
/// route such renders to the CPU pipeline instead (Agents.md: no silent
/// fallbacks).
///
/// Currently detected as unsupported:
/// - any adjustment key outside [`GPU_SUPPORTED_ADJUSTMENT_KEYS`] (in
///   particular non-zero `vibrance`/`saturation`, whose uniform fields exist
///   but are not applied by the shader);
/// - non-neutral Curves, HSL, Presence;
/// - Color Grading, Noise Reduction, Sharpening, Effects (vignette/grain);
/// - Geometry / Lens Correction / Perspective;
/// - non-empty SourceActions **unless** GPU source-action artifacts are bound
///   (see [`unsupported_gpu_stages_for`]).
///
/// Not listed: `RenderContext::camera_white_balance`. In `lumina-core` the
/// As-Shot gains are validated but never re-applied to pixels (the decoder has
/// already applied them), so they cause no CPU/GPU divergence today. If that
/// ever changes, this predicate must grow a corresponding check.
pub fn unsupported_gpu_stages(recipe: &EditRecipe) -> Vec<String> {
    unsupported_gpu_stages_for(recipe, false)
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
    let mut reasons = Vec::new();
    for key in recipe.adjustments.keys() {
        if !GPU_SUPPORTED_ADJUSTMENT_KEYS.contains(&key.as_str()) {
            reasons.push(format!("adjustment `{key}` not implemented on GPU"));
        }
    }
    if let Some(curves) = &recipe.curves {
        if !curves_are_neutral(curves) {
            reasons.push("curves".into());
        }
    }
    if let Some(hsl) = &recipe.hsl {
        if !hsl_is_neutral(hsl) {
            reasons.push("hsl".into());
        }
    }
    if let Some(presence) = &recipe.presence {
        if presence.texture != 0.0 || presence.clarity != 0.0 || presence.dehaze != 0.0 {
            reasons.push("presence".into());
        }
    }
    for (active, name) in [
        (recipe.color_grading.is_some(), "color_grading"),
        (recipe.noise_reduction.is_some(), "noise_reduction"),
        (recipe.sharpening.is_some(), "sharpening"),
        (recipe.effects.is_some(), "effects"),
        (recipe.geometry.is_some(), "geometry"),
        (recipe.lens_correction.is_some(), "lens_correction"),
        (recipe.perspective.is_some(), "perspective"),
        (
            !recipe.source_actions.is_empty() && !source_actions_bound,
            "source_actions",
        ),
    ] {
        if active {
            reasons.push(name.to_string());
        }
    }
    // More actions referenced than the unrolled shader slots can composite:
    // the surplus would be dropped silently, so the whole recipe stays
    // CPU-routed even when artifacts are bound.
    if source_actions_bound && recipe.source_actions.len() > MAX_SOURCE_ACTIONS {
        reasons.push(format!(
            "source_actions ({}/{} exceed the GPU stage slot limit)",
            recipe.source_actions.len(),
            MAX_SOURCE_ACTIONS
        ));
    }
    reasons
}

/// A curve is neutral when its master is the identity (`input == output` for
/// every point) and no per-channel curve exists — the CPU then leaves the
/// pixel unchanged modulo rounding within the golden tolerance.
fn curves_are_neutral(curves: &Curves) -> bool {
    fn identity(points: &[CurvePoint]) -> bool {
        points.iter().all(|p| (p.input - p.output).abs() <= 1e-6)
    }
    identity(&curves.master)
        && curves.channels.red.is_none()
        && curves.channels.green.is_none()
        && curves.channels.blue.is_none()
}

/// HSL is neutral when every present channel carries all-zero hue/saturation/
/// luminance (the CPU applies no visible change).
fn hsl_is_neutral(hsl: &HslAdjustments) -> bool {
    [
        hsl.red,
        hsl.orange,
        hsl.yellow,
        hsl.green,
        hsl.cyan,
        hsl.blue,
        hsl.violet,
        hsl.magenta,
    ]
    .iter()
    .flatten()
    .all(|c| c.hue == 0.0 && c.saturation == 0.0 && c.luminance == 0.0)
}

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

/// Warns once per unique reason set that the VRAM interactive path is rendering
/// a recipe whose stages the GPU tone pass does not implement.
#[cfg(feature = "gpu")]
fn warn_unsupported_vram_once(reasons: &[String]) {
    use std::collections::BTreeSet;
    use std::sync::Mutex;
    static WARNED: Mutex<Option<BTreeSet<String>>> = Mutex::new(None);
    let key = reasons.join("; ");
    let mut guard = WARNED.lock().unwrap();
    if guard.get_or_insert_with(BTreeSet::new).insert(key.clone()) {
        log::warn!(
            "GPU VRAM preview renders only the tone stage; recipe uses \
             GPU-unsupported stage(s): {key}. Interactive preview may diverge \
             from the CPU reference until these stages land on GPU."
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
/// transparently uses the CPU pipeline. The context is cheap to keep around and
/// reuse across frames once the GPU stages are implemented.
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
    /// Last recipe pushed via [`GpuContext::update_uniforms`]. Used both to feed
    /// the uniform buffer (GPU path) and as the CPU-fallback recipe.
    #[cfg(feature = "gpu")]
    recipe: Option<EditRecipe>,
}

/// VRAM-resident interactive state for GUI-60FPS-1.
///
/// Holds the output (RGBA8 tone result) and the R16Uint brush-mask textures
/// at full source resolution. The mask is uploaded incrementally per dirty
/// 512² tile via `queue.write_texture` from the GUI's persistent `Vec<u16>`
/// plane (`bytemuck::cast_slice`), or wholesale via
/// [`GpuContext::upload_mask_plane`] when evaluated pipeline planes change.
///
/// Roadmap (M2, partially landed as of GUI-WGPU-PRESENT-1): the former single
/// slot is now a small dimension-keyed LRU pool ([`VramPool`]) so alternating
/// sources and 45 MP+ images stop thrashing; a full `TiledCache`/`DraftPyramid`
/// tile pool remains the M2 target. See `docs/gpu-bootstrap.md`.
#[cfg(feature = "gpu")]
struct VramState {
    width: u32,
    height: u32,
    #[allow(dead_code)]
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    #[allow(dead_code)]
    mask: wgpu::Texture,
    mask_view: wgpu::TextureView,
    /// Filtering sampler for colour textures (overlay base).
    color_sampler: wgpu::Sampler,
    overlay_uniform: wgpu::Buffer,
    overlay_layout: wgpu::BindGroupLayout,
}

/// Dimension-keyed LRU pool of [`VramState`] entries (GUI-WGPU-PRESENT-1).
///
/// Replaces the former single-slot cache so interactive sessions with more
/// than one source (or repeated open/close of large images) keep hot entries
/// resident instead of dropping the only VRAM copy on every dimension change.
/// Eviction is bounded by two independent limits:
///
/// - entry count ([`VramPool::default_capacity`], env
///   `LUMINA_GPU_VRAM_POOL_ENTRIES`), and
/// - total resident bytes ([`VramPool::default_budget_bytes`], env
///   `LUMINA_GPU_VRAM_BUDGET_MB`; each entry costs `w*h*4` output +
///   `w*h*2` mask bytes).
///
/// The most-recently-used entry ("active") is what every render/upload call
/// operates on. A single entry that exceeds the whole budget is still kept
/// (a frame must always be renderable) — the over-budget condition is logged
/// loudly instead of silently shrinking the working set.
#[cfg(feature = "gpu")]
struct VramPool {
    entries: std::collections::HashMap<(u32, u32), VramState>,
    core: PoolCore,
}

/// LRU/bookkeeping half of [`VramPool`], generic-free and device-free so the
/// eviction policy stays unit-testable without a `wgpu::Device` (same pattern
/// as `tiling::CacheCore<T>`).
#[cfg(feature = "gpu")]
struct PoolCore {
    order: std::collections::VecDeque<(u32, u32)>,
    capacity: usize,
    budget_bytes: u64,
    resident_bytes: u64,
}

#[cfg(feature = "gpu")]
impl PoolCore {
    fn new(capacity: usize, budget_bytes: u64) -> Self {
        Self {
            order: std::collections::VecDeque::new(),
            capacity: capacity.max(1),
            budget_bytes: budget_bytes.max(1),
            resident_bytes: 0,
        }
    }

    /// Bytes one entry of `(width, height)` occupies in VRAM
    /// (RGBA8 output + R16Uint mask).
    fn entry_bytes(width: u32, height: u32) -> u64 {
        width as u64 * height as u64 * (4 + 2)
    }

    fn len(&self) -> usize {
        self.order.len()
    }

    fn contains(&self, key: &(u32, u32)) -> bool {
        self.order.iter().any(|k| k == key)
    }

    /// Mark `key` most-recently-used. No-op for unknown keys.
    fn touch(&mut self, key: &(u32, u32)) {
        if self.contains(key) {
            self.order.retain(|k| k != key);
            self.order.push_front(*key);
        }
    }

    /// Admit a new entry and evict least-recently-used entries until both the
    /// entry-count and byte-budget limits hold again. Returns the evicted keys
    /// so the caller can drop their VRAM handles and report the invalidation.
    /// The freshly admitted key is never evicted here even when it alone
    /// exceeds the budget (the caller logs that case loudly).
    fn admit(&mut self, key: (u32, u32)) -> Vec<(u32, u32)> {
        debug_assert!(!self.contains(&key), "admit must not double-insert");
        let bytes = Self::entry_bytes(key.0, key.1);
        self.order.push_front(key);
        self.resident_bytes += bytes;
        let mut evicted = Vec::new();
        while self.len() > self.capacity
            || (self.resident_bytes > self.budget_bytes && self.len() > 1)
        {
            let Some(victim) = self.order.pop_back() else {
                break;
            };
            if victim == key {
                // Only the new entry is left; keep it (must render) and log.
                self.order.push_front(victim);
                break;
            }
            self.resident_bytes -= Self::entry_bytes(victim.0, victim.1);
            evicted.push(victim);
        }
        evicted
    }
}

#[cfg(feature = "gpu")]
impl VramPool {
    fn new() -> Self {
        Self {
            entries: std::collections::HashMap::new(),
            core: PoolCore::new(Self::default_capacity(), Self::default_budget_bytes()),
        }
    }

    /// Default entry-count limit (`LUMINA_GPU_VRAM_POOL_ENTRIES`, default 4).
    fn default_capacity() -> usize {
        std::env::var("LUMINA_GPU_VRAM_POOL_ENTRIES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|&v| v >= 1)
            .unwrap_or_else(|| {
                if std::env::var("LUMINA_GPU_VRAM_POOL_ENTRIES").is_ok() {
                    log::debug!("invalid LUMINA_GPU_VRAM_POOL_ENTRIES, using default 4");
                }
                4
            })
    }

    /// Default byte budget in bytes (`LUMINA_GPU_VRAM_BUDGET_MB`, default
    /// 1024 MiB). Counts output + mask textures per pooled entry.
    fn default_budget_bytes() -> u64 {
        const DEFAULT_MB: u64 = 1024;
        std::env::var("LUMINA_GPU_VRAM_BUDGET_MB")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|&v| v >= 1)
            .map(|mb| mb.saturating_mul(1024 * 1024))
            .unwrap_or_else(|| {
                if std::env::var("LUMINA_GPU_VRAM_BUDGET_MB").is_ok() {
                    log::debug!(
                        "invalid LUMINA_GPU_VRAM_BUDGET_MB, using default {DEFAULT_MB} MiB"
                    );
                }
                DEFAULT_MB * 1024 * 1024
            })
    }

    /// Get-or-create the entry for `(width, height)` and make it the active
    /// (most-recently-used) one. `create` builds the VRAM handles; evictions
    /// are logged with their dimensions so dropped caches stay observable.
    fn get_or_create(
        &mut self,
        width: u32,
        height: u32,
        create: impl FnOnce(u32, u32) -> Result<VramState, GpuError>,
    ) -> Result<&mut VramState, GpuError> {
        let key = (width, height);
        if !self.entries.contains_key(&key) {
            let created = create(width, height)?;
            let evicted = self.core.admit(key);
            for victim in &evicted {
                if let Some(state) = self.entries.remove(victim) {
                    log::info!(
                        "vram pool: evicted {}x{} state ({:.1} MiB) — LRU/budget limit reached",
                        victim.0,
                        victim.1,
                        PoolCore::entry_bytes(victim.0, victim.1) as f64 / (1024.0 * 1024.0)
                    );
                    drop(state);
                }
            }
            let bytes = PoolCore::entry_bytes(width, height);
            if bytes > self.core.budget_bytes {
                log::warn!(
                    "vram pool: single {width}x{height} entry ({bytes} bytes) exceeds the \
                     configured budget ({} MiB); keeping it but no second source will fit",
                    self.core.budget_bytes / (1024 * 1024)
                );
            }
            self.entries.insert(key, created);
        } else {
            self.core.touch(&key);
        }
        Ok(self
            .entries
            .get_mut(&key)
            .expect("entry was just admitted/touched"))
    }

    /// The active (last ensured) entry, if any.
    fn active(&mut self) -> Option<&mut VramState> {
        let key = *self.core.order.front()?;
        self.entries.get_mut(&key)
    }
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
        Ok(Self::from_resources(Some(GpuResources {
            instance,
            adapter,
            device,
            queue,
        })))
    }

    /// Shared constructor body for [`GpuContext::new`] / [`GpuContext::from_parts`].
    fn from_resources(resources: Option<GpuResources>) -> Self {
        Self {
            resources,
            pipeline: std::sync::Mutex::new(None),
            sa_pipeline: std::sync::Mutex::new(None),
            vram: std::sync::Mutex::new(VramPool::new()),
            source_actions: None,
            recipe: None,
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
        self.resources.as_ref().map(|r| &r.device)
    }

    /// Borrow the bound [`wgpu::Queue`], if any.
    pub fn queue(&self) -> Option<&wgpu::Queue> {
        self.resources.as_ref().map(|r| &r.queue)
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
                uniform_buffer: shaders::create_source_action_uniform_buffer(&resources.device),
                bind_group_layout: shaders::create_source_action_bind_group_layout(
                    &resources.device,
                ),
            });
        }
        Ok(())
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
            if region.width != replacement.width || region.height != replacement.height {
                return Err(GpuError::RenderFailed(format!(
                    "source-action artifact {index}: region {}x{} does not match replacement {}x{}",
                    region.width, region.height, replacement.width, replacement.height
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
        self.source_actions = Some(artifacts.to_vec());
        Ok(())
    }

    /// Remove previously bound source-action artifacts. Subsequent renders of
    /// recipes with `source_actions` CPU-route again.
    pub fn clear_source_action_artifacts(&mut self) {
        self.source_actions = None;
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

    /// Ensure the VRAM-resident interactive textures exist for `width`×`height`
    /// and make them the active pool entry. Lazily (re)creates `output`
    /// (RGBA8, tone result) + `mask` (R16Uint, brush coverage) textures and
    /// the overlay uniform/layout so the hot path never allocates. Other
    /// pooled entries stay resident until the LRU/budget limits evict them
    /// (GUI-WGPU-PRESENT-1). No-op when no adapter is bound.
    pub fn ensure_vram(&self, width: u32, height: u32) -> Result<(), GpuError> {
        let Some(resources) = self.resources.as_ref() else {
            return Ok(());
        };
        let mut guard = self.vram.lock().unwrap();
        guard.get_or_create(width, height, |width, height| {
            create_vram_state(&resources.device, &resources.queue, width, height)
        })?;
        Ok(())
    }

    /// Dimensions of the active VRAM state, if one exists. The GUI uses this
    /// to size its present target without reaching into the pool.
    pub fn vram_dimensions(&self) -> Option<(u32, u32)> {
        let mut guard = self.vram.lock().unwrap();
        guard.active().map(|v| (v.width, v.height))
    }

    /// Interactive tone render that stays VRAM-resident — no `map_async` readback.
    ///
    /// When source-action artifacts are bound (GPU-STAGE-1) and match the frame
    /// dimensions, the dedicated source-action stage composites them into an
    /// intermediate texture first; the tone pass then samples *that* result.
    /// Otherwise the tone stage (`SHADER_SRC`) renders the uploaded frame
    /// directly into the cached `output` VRAM texture of the active pool entry.
    /// Caller presents via [`Self::copy_vram_to_texture`] or the overlay pass
    /// without ever mapping to CPU. Export/full-rebuild paths should use
    /// [`Self::render_with_gpu`] (which still reads back).
    pub fn render_to_vram(&self, frame: &ImageFrame, recipe: &EditRecipe) -> Result<(), GpuError> {
        // REVIEW-GPU-DIVERGENCE-1 / GPU-STAGE-1: the VRAM hot path cannot
        // CPU-route without a readback (that would defeat its purpose). Recipes
        // whose stages are unsupported *given the currently bound artifacts*
        // are surfaced with a loud, once-per-reason-set warning instead of
        // silently diverging. With bound artifacts, `source_actions` is no
        // longer "unsupported" — the dedicated GPU stage composites them.
        let sa_bound = self
            .matching_source_actions(frame.width, frame.height)
            .is_some();
        let unsupported = unsupported_gpu_stages_for(recipe, sa_bound);
        if !unsupported.is_empty() {
            warn_unsupported_vram_once(&unsupported);
        }
        let Some(resources) = self.resources.as_ref() else {
            return Err(GpuError::AdapterUnavailable(
                "no adapter for vram path".into(),
            ));
        };
        self.ensure_pipeline()?;
        self.ensure_vram(frame.width, frame.height)?;
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
        let input = shaders::create_input_texture(
            &resources.device,
            frame.width,
            frame.height,
            "lumina-gpu-vram-input",
        );
        resources.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &input,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(frame.width * 4),
                rows_per_image: Some(frame.height),
            },
            wgpu::Extent3d {
                width: frame.width,
                height: frame.height,
                depth_or_array_layers: 1,
            },
        );
        let input_view = input.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = shaders::create_sampler(&resources.device, "lumina-gpu-vram-samp");
        let mut vram_guard = self.vram.lock().unwrap();
        let Some(v) = vram_guard.active() else {
            return Err(GpuError::RenderFailed("vram not ready".into()));
        };
        let mut enc = resources
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("lumina-gpu-vram-encode"),
            });
        // GPU-STAGE-1: when source-action artifacts are bound for this frame,
        // the dedicated stage composites them into an intermediate texture in
        // the same encoder; the tone pass samples *that* result. Without bound
        // artifacts the tone pass reads the uploaded frame directly.
        let artifacts = self.matching_source_actions(frame.width, frame.height);
        // Deferred-init locals so the tone bind can reference either input.
        let sa_intermediate: wgpu::Texture;
        let sa_intermediate_view: wgpu::TextureView;
        let tone_input_view: &wgpu::TextureView = if let Some(artifacts) = artifacts {
            self.ensure_source_action_pipeline()?;
            let sa_guard = self.sa_pipeline.lock().unwrap();
            let Some(sa) = sa_guard.as_ref() else {
                return Err(GpuError::RenderFailed(
                    "source-action pipeline not built".into(),
                ));
            };
            let sa_uniforms = shaders::SourceActionUniforms {
                count: artifacts.len() as u32,
                _pad: [0; 3],
            };
            shaders::write_source_action_uniforms(
                &resources.queue,
                &sa.uniform_buffer,
                &sa_uniforms,
            );
            let mut region_views = Vec::with_capacity(artifacts.len());
            let mut replacement_views = Vec::with_capacity(artifacts.len());
            for (index, artifact) in artifacts.iter().enumerate() {
                debug_assert_eq!(
                    (artifact.region.width, artifact.region.height),
                    (frame.width, frame.height),
                    "matching_source_actions validated dimensions"
                );
                let region_tex = shaders::create_region_texture(
                    &resources.device,
                    artifact.region.width,
                    artifact.region.height,
                    &format!("lumina-gpu-sa-region-{index}"),
                );
                shaders::write_u16_plane(
                    &resources.queue,
                    &region_tex,
                    artifact.region.width,
                    artifact.region.height,
                    &artifact.region.values,
                );
                region_views.push(region_tex.create_view(&wgpu::TextureViewDescriptor::default()));
                let repl_tex = shaders::create_input_texture(
                    &resources.device,
                    artifact.replacement.width,
                    artifact.replacement.height,
                    &format!("lumina-gpu-sa-repl-{index}"),
                );
                resources.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &repl_tex,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &artifact.replacement.pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(artifact.replacement.width * 4),
                        rows_per_image: Some(artifact.replacement.height),
                    },
                    wgpu::Extent3d {
                        width: artifact.replacement.width,
                        height: artifact.replacement.height,
                        depth_or_array_layers: 1,
                    },
                );
                replacement_views
                    .push(repl_tex.create_view(&wgpu::TextureViewDescriptor::default()));
            }
            let region_refs: Vec<&wgpu::TextureView> = region_views.iter().collect();
            let replacement_refs: Vec<&wgpu::TextureView> = replacement_views.iter().collect();
            let sa_bind = shaders::create_source_action_bind_group(
                &resources.device,
                &sa.bind_group_layout,
                &sa.uniform_buffer,
                &input_view,
                &region_refs,
                &replacement_refs,
                artifacts.len() as u32,
            );
            sa_intermediate = shaders::create_output_texture(
                &resources.device,
                frame.width,
                frame.height,
                "lumina-gpu-vram-sa-out",
            );
            sa_intermediate_view =
                sa_intermediate.create_view(&wgpu::TextureViewDescriptor::default());
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("lumina-gpu-vram-sourceaction"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &sa_intermediate_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&sa.pipeline);
                pass.set_bind_group(0, &sa_bind, &[]);
                pass.draw(0..3, 0..1);
            }
            &sa_intermediate_view
        } else {
            &input_view
        };
        let tone_bind = shaders::create_color_tone_bind_group(
            &resources.device,
            &pipeline.bind_group_layout,
            &pipeline.uniform_buffer,
            tone_input_view,
            &sampler,
        );
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lumina-gpu-vram-tone"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &v.output_view,
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
        if let Some(t0) = start {
            log::info!(
                "lumina perf: render_to_vram {}x{} {:.2} ms",
                frame.width,
                frame.height,
                t0.elapsed().as_secs_f64() * 1000.0
            );
            if Self::perf_log_enabled() {
                eprintln!(
                    "LUMINA_PERF render_to_vram={:.2}ms {}x{}",
                    t0.elapsed().as_secs_f64() * 1000.0,
                    frame.width,
                    frame.height
                );
            }
        }
        Ok(())
    }

    /// Upload a single dirty 512² (or edge-clipped) mask tile into the active
    /// VRAM mask texture. `tile_data` is `u16` little-endian coverage,
    /// row-major. The tile must lie fully inside the active entry's dimensions
    /// — an out-of-bounds tile is a hard error, never silently clipped.
    pub fn upload_mask_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        tile_w: u32,
        tile_h: u32,
        tile_data: &[u8],
    ) -> Result<(), GpuError> {
        let Some(resources) = self.resources.as_ref() else {
            return Ok(());
        };
        let mut guard = self.vram.lock().unwrap();
        let Some(v) = guard.active() else {
            return Err(GpuError::RenderFailed(
                "vram not ready for mask upload".into(),
            ));
        };
        if tile_x.saturating_add(tile_w) > v.width || tile_y.saturating_add(tile_h) > v.height {
            return Err(GpuError::RenderFailed(format!(
                "mask tile ({tile_x},{tile_y} {tile_w}x{tile_h}) exceeds the active \
                 VRAM mask {}x{}",
                v.width, v.height
            )));
        }
        shaders::write_mask_tile(
            &resources.queue,
            &v.mask,
            tile_x,
            tile_y,
            tile_w,
            tile_h,
            tile_data,
        );
        Ok(())
    }

    /// Upload a complete evaluated mask plane into the active VRAM mask
    /// texture (GPU-STAGE-1).
    ///
    /// This is the data path that makes *pipeline-evaluated* masks visible in
    /// the GPU present composite: after a full render, the caller pushes the
    /// combined effective planes (`combine_mask_planes`) here and the overlay
    /// pass shows exactly the coverage the measurement semantics (F-041)
    /// attribute to the frame — instead of only live brush stamps.
    ///
    /// Errors when no VRAM state exists or the plane does not match its
    /// dimensions (no silent fallback).
    pub fn upload_mask_plane(
        &self,
        width: u32,
        height: u32,
        values: &[u16],
    ) -> Result<(), GpuError> {
        let Some(resources) = self.resources.as_ref() else {
            return Ok(());
        };
        if values.len() != (width as usize) * (height as usize) {
            return Err(GpuError::RenderFailed(format!(
                "mask plane has {} values, expected {width}*{height}",
                values.len()
            )));
        }
        let mut guard = self.vram.lock().unwrap();
        let Some(v) = guard.active() else {
            return Err(GpuError::RenderFailed(
                "vram not ready for mask plane upload".into(),
            ));
        };
        if v.width != width || v.height != height {
            return Err(GpuError::RenderFailed(format!(
                "mask plane {width}x{height} does not match the active VRAM state {}x{}",
                v.width, v.height
            )));
        }
        shaders::write_u16_plane(&resources.queue, &v.mask, width, height, values);
        Ok(())
    }

    /// GPU-GPU copy/overlay of the active VRAM tone + mask textures into an
    /// egui-managed `dest` texture. No CPU readback — the copy is an overlay
    /// render pass directly on the queue. `dest` must be created with
    /// `TEXTURE_BINDING|COPY_DST|RENDER_ATTACHMENT`, `Rgba8Unorm`, and the same
    /// dimensions as the active VRAM cache entry (a mismatch is a hard error —
    /// the previous silent stretch would have presented distorted pixels).
    ///
    /// ✅ On-screen present (GUI-WGPU-PRESENT-1): since the `eframe` wgpu
    /// renderer migration, `GpuContext` shares the renderer's device/queue via
    /// [`Self::from_parts`], so a `dest` texture created by the GUI on that
    /// same device can be registered as an egui user image
    /// (`egui_wgpu::Renderer::register_native_texture`) and drawn with
    /// `ui.painter().image(..)` — the preview never touches the CPU anymore.
    pub fn copy_vram_to_texture(&self, dest: &wgpu::Texture) -> Result<(), GpuError> {
        let Some(resources) = self.resources.as_ref() else {
            return Ok(());
        };
        let mut guard = self.vram.lock().unwrap();
        let Some(v) = guard.active() else {
            return Ok(());
        };
        if dest.size().width != v.width || dest.size().height != v.height {
            return Err(GpuError::RenderFailed(format!(
                "present target {}x{} does not match the active VRAM state {}x{}",
                dest.size().width,
                dest.size().height,
                v.width,
                v.height
            )));
        }
        // Overlay tint: Lumina accent blue with 0.45 strength matches CPU overlay.
        let uniforms = shaders::OverlayUniforms {
            color: [80.0 / 255.0, 160.0 / 255.0, 1.0, 0.45],
        };
        shaders::write_overlay_uniforms(&resources.queue, &v.overlay_uniform, &uniforms);
        let dest_view = dest.create_view(&wgpu::TextureViewDescriptor::default());
        let overlay_pipe = shaders::create_overlay_pipeline(&resources.device, dest.format())
            .map_err(|e| GpuError::RenderFailed(e.to_string()))?;
        let bind = shaders::create_overlay_bind_group(
            &resources.device,
            &v.overlay_layout,
            &v.overlay_uniform,
            &v.output_view,
            &v.mask_view,
            &v.color_sampler,
        );
        let mut enc = resources
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("lumina-gpu-overlay-present"),
            });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lumina-gpu-overlay"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &dest_view,
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
            pass.set_pipeline(&overlay_pipe);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        resources.queue.submit(Some(enc.finish()));
        Ok(())
    }

    /// Diagnostic/test helper: read the active VRAM mask plane back to the CPU
    /// (`map_async`) and return it as `(width, height, u16 values)` in the
    /// exact source domain. This is the counterpart of
    /// [`Self::upload_mask_plane`]/[`Self::upload_mask_tile`] and exists so
    /// the mask data path has a byte-exact regression net — the interactive
    /// present path itself never calls this.
    pub fn readback_mask_plane(&self) -> Result<(u32, u32, Vec<u16>), GpuError> {
        let Some(resources) = self.resources.as_ref() else {
            return Err(GpuError::AdapterUnavailable(
                "no adapter for mask readback".into(),
            ));
        };
        let mut guard = self.vram.lock().unwrap();
        let Some(v) = guard.active() else {
            return Err(GpuError::RenderFailed("vram not ready".into()));
        };
        let (width, height) = (v.width, v.height);
        let bytes_per_row = shaders::aligned_bytes_per_row(width * 2);
        let staging = resources.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lumina-gpu-mask-readback"),
            size: (bytes_per_row * height.max(1)) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = resources
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("lumina-gpu-mask-readback-enc"),
            });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &v.mask,
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
        let row_u16s = width as usize;
        let mut values = Vec::with_capacity(row_u16s * height as usize);
        for y in 0..height as usize {
            let start = y * bytes_per_row as usize;
            let row = &mapped[start..start + row_u16s * 2];
            values.extend(
                row.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes(*b)),
            );
        }
        drop(mapped);
        drop(guard);
        staging.unmap();
        Ok((width, height, values))
    }

    /// Full-frame render entry point.
    ///
    /// When a real GPU adapter is bound this runs the color/tone fragment shader
    /// (`SHADER_SRC`) on the decoded [`ImageFrame`] uploaded as an `Rgba8Unorm`
    /// texture, rendering into an `Rgba8Unorm` target and reading the result back
    /// into a [`Frame`]. The shader mirrors the integer-rounded per-channel math
    /// of `lumina-core::apply_channel_lut_adjustments`, so the output matches the
    /// CPU oracle within the golden-image tolerance (maxAbsDiff ≤ 1, PSNR ≥ 45 dB).
    ///
    /// **Recipe validation (REVIEW-GPU-DIVERGENCE-1).** The shader only implements
    /// white balance plus the seven tone sliders. When [`unsupported_gpu_stages`]
    /// reports any unsupported stage (Curves, HSL, Presence, Vibrance/Saturation,
    /// Color Grading, Noise Reduction, Sharpening, Effects, Geometry, Lens
    /// Correction, Perspective, SourceActions), the render is **explicitly routed
    /// to the full CPU pipeline** instead of silently producing divergent pixels.
    /// The routing decision is logged once per unique reason set — the GPU is an
    /// accelerator, never a semantic change (Agents.md: no silent fallbacks).
    ///
    /// When no adapter is bound (or the `gpu` feature is disabled downstream) this
    /// transparently falls back to the CPU pipeline so the public API always
    /// returns a real [`Frame`].
    ///
    /// TODO(PERF): the current path copies the render target back to a CPU buffer
    /// via `map_async`. A later stage should present directly to a swapchain /
    /// write to a persistent VRAM `Frame` and only read back for export/preview.
    pub fn render_with_gpu(
        &self,
        frame: &ImageFrame,
        recipe: &EditRecipe,
    ) -> Result<Frame, GpuError> {
        let Some(resources) = self.resources.as_ref() else {
            return Self::render_cpu(frame, recipe);
        };
        // REVIEW-GPU-DIVERGENCE-1 / GPU-STAGE-1: never let the GPU path drop
        // recipe stages. Route to the CPU oracle loudly instead of rendering
        // different pixels — with one exception: when source-action artifacts
        // are bound and match the frame, the dedicated GPU source-action stage
        // composites them and `source_actions` is no longer unsupported.
        let sa_bound = self
            .matching_source_actions(frame.width, frame.height)
            .is_some();
        let unsupported = unsupported_gpu_stages_for(recipe, sa_bound);
        if !unsupported.is_empty() {
            log_cpu_routing_once(&unsupported, "render_with_gpu");
            return Self::render_cpu(frame, recipe);
        }
        self.ensure_pipeline()?;
        let guard = self.pipeline.lock().unwrap();
        let Some(pipeline) = guard.as_ref() else {
            return Self::render_cpu(frame, recipe);
        };

        let width = frame.width;
        let height = frame.height;

        // Upload the recipe sliders into the uniform buffer.
        let uniforms = shaders::Uniforms::from_recipe(recipe);
        shaders::write_uniforms(&resources.queue, &pipeline.uniform_buffer, &uniforms);

        // Source frame → input texture.
        let input_texture =
            shaders::create_input_texture(&resources.device, width, height, "lumina-gpu-input");
        resources.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &input_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.pixels,
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
        let input_view = input_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = shaders::create_sampler(&resources.device, "lumina-gpu-sampler");

        // Render target + readback staging buffer.
        let output_texture =
            shaders::create_output_texture(&resources.device, width, height, "lumina-gpu-output");
        let output_view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bytes_per_row = shaders::aligned_bytes_per_row(width);
        let readback = shaders::create_readback_buffer(
            &resources.device,
            width,
            height,
            "lumina-gpu-readback",
        );

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
        // GPU-STAGE-1: composite bound source-action artifacts into an
        // intermediate target first; the tone pass then samples that result.
        let sa_intermediate: wgpu::Texture;
        let sa_intermediate_view: wgpu::TextureView;
        let tone_input_view: &wgpu::TextureView = match self.matching_source_actions(width, height)
        {
            Some(artifacts) => {
                self.ensure_source_action_pipeline()?;
                let sa_guard = self.sa_pipeline.lock().unwrap();
                let Some(sa) = sa_guard.as_ref() else {
                    return Err(GpuError::RenderFailed(
                        "source-action pipeline not built".into(),
                    ));
                };
                let sa_uniforms = shaders::SourceActionUniforms {
                    count: artifacts.len() as u32,
                    _pad: [0; 3],
                };
                shaders::write_source_action_uniforms(
                    &resources.queue,
                    &sa.uniform_buffer,
                    &sa_uniforms,
                );
                let mut region_views = Vec::with_capacity(artifacts.len());
                let mut replacement_views = Vec::with_capacity(artifacts.len());
                for (index, artifact) in artifacts.iter().enumerate() {
                    let region_tex = shaders::create_region_texture(
                        &resources.device,
                        artifact.region.width,
                        artifact.region.height,
                        &format!("lumina-gpu-sa-region-{index}"),
                    );
                    shaders::write_u16_plane(
                        &resources.queue,
                        &region_tex,
                        artifact.region.width,
                        artifact.region.height,
                        &artifact.region.values,
                    );
                    region_views
                        .push(region_tex.create_view(&wgpu::TextureViewDescriptor::default()));
                    let repl_tex = shaders::create_input_texture(
                        &resources.device,
                        artifact.replacement.width,
                        artifact.replacement.height,
                        &format!("lumina-gpu-sa-repl-{index}"),
                    );
                    resources.queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &repl_tex,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        &artifact.replacement.pixels,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(artifact.replacement.width * 4),
                            rows_per_image: Some(artifact.replacement.height),
                        },
                        wgpu::Extent3d {
                            width: artifact.replacement.width,
                            height: artifact.replacement.height,
                            depth_or_array_layers: 1,
                        },
                    );
                    replacement_views
                        .push(repl_tex.create_view(&wgpu::TextureViewDescriptor::default()));
                }
                let region_refs: Vec<&wgpu::TextureView> = region_views.iter().collect();
                let replacement_refs: Vec<&wgpu::TextureView> = replacement_views.iter().collect();
                let sa_bind = shaders::create_source_action_bind_group(
                    &resources.device,
                    &sa.bind_group_layout,
                    &sa.uniform_buffer,
                    &input_view,
                    &region_refs,
                    &replacement_refs,
                    artifacts.len() as u32,
                );
                sa_intermediate = shaders::create_output_texture(
                    &resources.device,
                    width,
                    height,
                    "lumina-gpu-sa-out",
                );
                sa_intermediate_view =
                    sa_intermediate.create_view(&wgpu::TextureViewDescriptor::default());
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("lumina-gpu-sourceaction"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &sa_intermediate_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(&sa.pipeline);
                    pass.set_bind_group(0, &sa_bind, &[]);
                    pass.draw(0..3, 0..1);
                }
                &sa_intermediate_view
            }
            None => &input_view,
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
                    view: &output_view,
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
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &output_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
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
    /// Sets up the GPU scaffolding (pipeline + uniforms + ROI tile set) and,
    /// because the color/tone shader stage is not implemented yet, falls back to
    /// the CPU reference to produce real pixels. The tile set is logged so the
    /// parallel tiling subagent has a concrete call site to plug into.
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
        let zoom = (frame.width as f32 / viewport.width.max(1.0)).clamp(0.01, 100.0);
        let pyramid = crate::tiling::DraftPyramid::new(frame.width, frame.height);
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

    /// CPU fallback used by the bootstrap stub. Applies the recipe with the
    /// platform-neutral core pipeline and returns a [`Frame`].
    fn render_cpu(frame: &ImageFrame, recipe: &EditRecipe) -> Result<Frame, GpuError> {
        let mut out = frame.clone();
        out.apply_recipe(recipe)?;
        Ok(Frame::from_image_frame(out))
    }

    /// CPU fallback that uses the recipe stored via [`update_uniforms`], or the
    /// untouched frame when none has been set.
    fn render_draft_cpu(&self, frame: &ImageFrame) -> Result<Frame, GpuError> {
        let mut out = frame.clone();
        if let Some(recipe) = self.recipe.as_ref() {
            out.apply_recipe(recipe)?;
        }
        Ok(Frame::from_image_frame(out))
    }
}

#[cfg(not(feature = "gpu"))]
impl GpuContext {
    /// Create a CPU-only context (the `gpu` feature is disabled, so no adapter
    /// is ever bound). Rendering always uses the CPU fallback.
    pub fn new() -> Result<Self, GpuError> {
        Ok(Self {})
    }

    /// Always `false` without the `gpu` feature.
    pub fn is_available(&self) -> bool {
        false
    }

    /// Always `None` without the `gpu` feature (no adapter can be bound).
    pub fn adapter_info(&self) -> Option<String> {
        None
    }

    /// CPU fallback render (the only path when the `gpu` feature is off).
    pub fn render_with_gpu(
        &self,
        frame: &ImageFrame,
        recipe: &EditRecipe,
    ) -> Result<Frame, GpuError> {
        let mut out = frame.clone();
        out.apply_recipe(recipe)?;
        Ok(Frame::from_image_frame(out))
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
    pub fn copy_vram_to_texture(&self, _d: &()) -> Result<(), GpuError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// GPU backend init (only compiled under the `gpu` feature).
// ---------------------------------------------------------------------------

#[cfg(feature = "gpu")]
struct GpuResources {
    #[allow(dead_code)]
    instance: wgpu::Instance,
    #[allow(dead_code)]
    adapter: wgpu::Adapter,
    #[allow(dead_code)]
    device: wgpu::Device,
    #[allow(dead_code)]
    queue: wgpu::Queue,
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
    uniform_buffer: wgpu::Buffer,
    #[allow(dead_code)]
    bind_group_layout: wgpu::BindGroupLayout,
}

/// Create one pooled VRAM state: output (RGBA8) + mask (R16Uint) textures
/// plus the overlay uniform/layout. The mask is cleared to zero so compositing
/// stays identity until a brush tile or evaluated plane is uploaded.
#[cfg(feature = "gpu")]
fn create_vram_state(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
) -> Result<VramState, GpuError> {
    let output = shaders::create_output_texture(device, width, height, "lumina-gpu-vram-output");
    let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
    let mask = shaders::create_mask_texture(device, width, height, "lumina-gpu-vram-mask");
    let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
    // The overlay pass samples the colour output with a filtering sampler and
    // reads the R16Uint mask via exact `textureLoad` (no sampler needed).
    let color_sampler = shaders::create_sampler(device, "lumina-gpu-vram-color-samp");
    let overlay_uniform = shaders::create_overlay_uniform_buffer(device);
    let overlay_layout = shaders::create_overlay_bind_group_layout(device);
    // Clear mask to zero so compositing is identity until a brush writes.
    let zero_rows = vec![0u8; (width as usize) * (height as usize) * 2];
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &mask,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &zero_rows,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 2),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    Ok(VramState {
        width,
        height,
        output,
        output_view,
        mask,
        mask_view,
        color_sampler,
        overlay_uniform,
        overlay_layout,
    })
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

/// Real WGSL color/tone shader for the color/tone stage.
///
/// Mirrors the integer-rounded per-channel math of
/// `lumina-core::apply_channel_lut_adjustments` (white balance → exposure →
/// contrast → shadows → highlights → whites → blacks) in the sRGB-encoded
/// RGBA8 byte domain. The fullscreen-triangle vertex stage covers the target;
/// the fragment stage samples the uploaded source texture at its exact texel
/// centre (nearest sampler) and applies the tone mapping, writing the graded
/// RGBA8 result.
///
/// Rounding uses `floor(x + 0.5)` (registered as `roundi`) to match Rust's
/// `f64::round` (ties away from zero) for the non-negative values this kernel
/// produces, keeping the GPU path within the golden-image tolerance of the CPU
/// oracle.
#[cfg(feature = "gpu")]
const SHADER_SRC: &str = r#"
struct Params {
  exposure : f32,
  contrast : f32,
  highlights : f32,
  shadows : f32,
  whites : f32,
  blacks : f32,
  wb_temperature : f32,
  wb_tint : f32,
  vibrance : f32,
  saturation : f32,
  pad0 : f32,
  pad1 : f32,
  pad2 : f32,
  pad3 : f32,
  pad4 : f32,
  pad5 : f32,
};
@group(0) @binding(0) var<uniform> params : Params;
@group(0) @binding(1) var input_tex : texture_2d<f32>;
@group(0) @binding(2) var input_samp : sampler;

struct VsOut {
  @builtin(position) pos : vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid : u32) -> VsOut {
  var p = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 3.0, -1.0),
    vec2<f32>(-1.0,  3.0)
  );
  var out : VsOut;
  out.pos = vec4<f32>(p[vid], 0.0, 1.0);
  return out;
}

// Round half away from zero, matching Rust's `f64::round` for non-negative x.
fn roundi(x : f32) -> f32 {
  return floor(x + 0.5);
}

fn clamp01(x : f32) -> f32 {
  return clamp(x, 0.0, 1.0);
}

// Per-channel tone mapping, identical in order and rounding to
// `lumina-core::apply_channel_lut_adjustments`. `in_norm` is the source channel
// normalised to [0,1]; `wb_gain` is this channel's white-balance multiplier.
fn tone_channel(in_norm : f32, wb_gain : f32) -> f32 {
  var v : f32 = in_norm * 255.0;
  // 1) White balance.
  v = roundi(v * wb_gain);
  v = clamp(v, 0.0, 255.0);
  // 2) Exposure (multiplier = 2^exposure).
  v = roundi(v * pow(2.0, params.exposure));
  v = clamp(v, 0.0, 255.0);
  // 3) Contrast (factor = 1 + c).
  v = roundi((v - 128.0) * (1.0 + params.contrast) + 128.0);
  v = clamp(v, 0.0, 255.0);
  // 4) Shadows.
  if (params.shadows != 0.0) {
    let x = v / 255.0;
    let w = pow(max(0.0, (0.5 - x) / 0.5), 2.0);
    v = roundi(clamp01(x + params.shadows * w * 0.25) * 255.0);
  }
  // 5) Highlights.
  if (params.highlights != 0.0) {
    let x = v / 255.0;
    let w = pow(max(0.0, (x - 0.5) / 0.5), 2.0);
    v = roundi(clamp01(x + params.highlights * w * 0.25) * 255.0);
  }
  // 6) Whites.
  if (params.whites != 0.0) {
    let x = v / 255.0;
    let w = max(0.0, (x - 0.5) / 0.5);
    v = roundi(clamp01(x + params.whites * w * 0.25) * 255.0);
  }
  // 7) Blacks.
  if (params.blacks != 0.0) {
    let x = v / 255.0;
    let w = max(0.0, (0.5 - x) / 0.5);
    v = roundi(clamp01(x - params.blacks * w * 0.25) * 255.0);
  }
  return v;
}

@fragment
fn fs_main(@builtin(position) frag_coord : vec4<f32>) -> @location(0) vec4<f32> {
  let dims = vec2<f32>(textureDimensions(input_tex));
  let uv = frag_coord.xy / dims;
  let src = textureSampleLevel(input_tex, input_samp, uv, 0.0);

  let warmth = (params.wb_temperature - 6500.0) / 5500.0;
  let wb_r = 1.0 - warmth * 0.35;
  let wb_g = 1.0 - params.wb_tint * 0.20;
  let wb_b = 1.0 + warmth * 0.35;

  let r = tone_channel(src.r, wb_r);
  let g = tone_channel(src.g, wb_g);
  let b = tone_channel(src.b, wb_b);

  return vec4<f32>(r / 255.0, g / 255.0, b / 255.0, src.a);
}
"#;

/// Enumerate a GPU adapter and create a device/queue.
///
/// Restricted to the **Metal** backend for the M-series native path
/// (Apple Silicon). On other native targets the backend list can be widened
/// later. Returns [`GpuError::AdapterUnavailable`] when no adapter matches, and
/// [`GpuError::DeviceUnavailable`] when device/queue creation fails — callers
/// are expected to treat either as "use the CPU fallback".
#[cfg(feature = "gpu")]
fn init_gpu_resources() -> Result<GpuResources, GpuError> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        // Metal is the primary backend on Apple Silicon (M5 Pro). Restricting
        // the backend list avoids pulling Vulkan/DX12/WGPU-GL on platforms where
        // they are unavailable and keeps adapter selection deterministic.
        backends: wgpu::Backends::METAL,
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
    .map_err(|err| GpuError::AdapterUnavailable(format!("no Metal adapter found: {err}")))?;

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("lumina-gpu"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .map_err(|err| GpuError::DeviceUnavailable(err.to_string()))?;

    Ok(GpuResources {
        instance,
        adapter,
        device,
        queue,
    })
}

#[cfg(all(test, feature = "gpu"))]
mod vram_pool_tests {
    use super::*;

    /// GUI-WGPU-PRESENT-1: LRU eviction honours both the entry-count limit and
    /// the byte budget, always keeping the freshly admitted entry.
    #[test]
    fn pool_core_admits_touches_and_evicts_lru() {
        let mut core = PoolCore::new(2, u64::MAX);
        // 4x4 entry = 4*4*(4+2) = 96 bytes.
        assert_eq!(PoolCore::entry_bytes(4, 4), 96);
        assert!(core.admit((0, 0)).is_empty());
        assert!(core.admit((0, 1)).is_empty());
        // Touch (0,0) so (0,1) becomes the LRU entry.
        core.touch(&(0, 0));
        let evicted = core.admit((0, 2));
        assert_eq!(evicted, vec![(0, 1)], "the least-recently-used entry goes");
        assert!(core.contains(&(0, 0)));
        assert!(!core.contains(&(0, 1)));
        assert!(core.contains(&(0, 2)));

        // Re-admitting an existing key is a caller bug; touch is the API.
        core.touch(&(0, 2));
        assert_eq!(core.len(), 2);
    }

    /// Byte-budget eviction: entries are dropped until resident bytes fit,
    /// but a lone oversized entry is never evicted by itself.
    #[test]
    fn pool_core_budget_eviction_keeps_last_entry() {
        // All three keys cost exactly 96 bytes (w*h*6).
        assert_eq!(PoolCore::entry_bytes(4, 4), PoolCore::entry_bytes(8, 2));
        assert_eq!(PoolCore::entry_bytes(4, 4), PoolCore::entry_bytes(16, 1));
        let mut core = PoolCore::new(8, 200);
        core.admit((4, 4));
        core.admit((8, 2));
        assert_eq!(core.len(), 2);
        assert_eq!(core.resident_bytes, 192);
        // This admission pushes past the budget → evict the LRU entry.
        let evicted = core.admit((16, 1));
        assert_eq!(evicted, vec![(4, 4)]);
        assert_eq!(core.resident_bytes, 192);

        // A single huge entry stays (must render) even over budget…
        let mut solo = PoolCore::new(8, 10);
        let evicted = solo.admit((1000, 1));
        assert!(evicted.is_empty(), "the last remaining entry is kept");
        assert_eq!(solo.len(), 1);
    }
}

#[cfg(test)]
mod combine_tests {
    use super::*;

    /// GPU-STAGE-1: combining evaluated layer planes follows the F-041
    /// intersection-product semantics.
    #[test]
    fn combine_mask_planes_product_semantics() {
        use lumina_core::masks::MaskPlane;
        let plane = |values: &[u16]| MaskPlane {
            width: 2,
            height: 2,
            values: values.to_vec(),
        };
        // Empty input = no effective mask (valid state).
        assert!(combine_mask_planes(&[]).unwrap().is_none());

        // Single plane passes through unchanged.
        let single = plane(&[0, 32768, 65535, 123]);
        assert_eq!(
            combine_mask_planes(std::slice::from_ref(&single))
                .unwrap()
                .unwrap()
                .values,
            single.values
        );

        // Product: 50% ∩ full = 50%; anything ∩ 0 = 0; all-MAX = identity.
        let half = plane(&[u16::MAX, 32768, u16::MAX, 40000]);
        let full = plane(&[u16::MAX, u16::MAX, u16::MAX, u16::MAX]);
        let zero = plane(&[0, 0, 0, 0]);
        let combined = combine_mask_planes(&[half.clone(), full]).unwrap().unwrap();
        assert_eq!(combined.values, vec![u16::MAX, 32768, u16::MAX, 40000]);
        let killed = combine_mask_planes(&[half, zero]).unwrap().unwrap();
        assert_eq!(killed.values, vec![0, 0, 0, 0]);

        // Dimension mismatch is an explicit error, never a silent resample.
        let other = MaskPlane {
            width: 1,
            height: 4,
            values: vec![0; 4],
        };
        let err = combine_mask_planes(&[single, other]).unwrap_err();
        assert!(err.contains("does not match"), "got: {err}");
    }
}
