//! VRAM-resident render state and dimension-keyed resource pool.

use super::{shaders, GpuContext, GpuError};

mod source_upload;
mod upload_identity;
pub(crate) use source_upload::upload_source_texture;
pub(crate) use upload_identity::{should_upload, UploadIdentity};

/// A caller-supplied external depth plane for `recipe.lens_blur.depth_artifact`
/// (G-05), uploaded once via [`GpuContext::set_depth_plane`].
///
/// The owned [`lumina_core::DepthPlane`] is kept so the CPU fallback
/// (`render_cpu`) can pass it into the oracle's `RenderContext` too — the
/// external depth is part of the render contract, not just the GPU pass. The
/// uploaded `R32Float` texture is only present when an adapter is bound; a
/// no-adapter context still keeps the values for the CPU reference.
#[cfg(feature = "gpu")]
pub(super) struct DepthPlaneGpu {
    pub(super) plane: lumina_core::DepthPlane,
    #[allow(dead_code)]
    pub(super) texture: Option<wgpu::Texture>,
    pub(super) view: Option<wgpu::TextureView>,
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
pub(super) struct VramState {
    pub(super) width: u32,
    pub(super) height: u32,
    #[allow(dead_code)]
    pub(super) output: wgpu::Texture,
    pub(super) output_view: wgpu::TextureView,
    #[allow(dead_code)]
    pub(super) mask: wgpu::Texture,
    pub(super) overlay_uniform: wgpu::Buffer,
    /// Cached base (source) texture for the tone pass (R2-GPU-01). Uploaded only
    /// when the source frame changes; reused across draft ticks so the 96 MB
    /// CPU→GPU upload does not run on every slider tick.
    #[allow(dead_code)]
    pub(super) input: wgpu::Texture,
    pub(super) input_view: wgpu::TextureView,
    /// Nearest sampler for the base texture (`textureSampleLevel`, exact texel).
    pub(super) input_sampler: wgpu::Sampler,
    /// Identity of the last uploaded source frame (instance ID plus geometry).
    /// Lets us skip the re-upload while the same source is dragged
    /// (`upload_identity`; GPU-UPLOAD-IDENTITY-92).
    pub(super) input_source_identity: Option<UploadIdentity>,
    /// Overlay present pipelines cached per target format (R2-GPU-02) so the
    /// present shader is not recompiled on every repaint.
    pub(super) overlay_pipelines:
        std::collections::HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    /// Overlay bind group built once (all its parts are stable in VRAM); the hot
    /// present path only reuses it (R2-GPU-02). `mask_view`/`color_sampler`/
    /// `overlay_layout` are consumed into this group at construction and so are
    /// kept as locals in `create_vram_state` rather than struct fields.
    pub(super) overlay_bind_group: wgpu::BindGroup,
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
pub(super) struct VramPool {
    entries: std::collections::HashMap<(u32, u32), VramState>,
    core: PoolCore,
}

/// LRU/bookkeeping half of [`VramPool`], generic-free and device-free so the
/// eviction policy stays unit-testable without a `wgpu::Device` (same pattern
/// as `tiling::CacheCore<T>`).
#[cfg(feature = "gpu")]
pub(super) struct PoolCore {
    order: std::collections::VecDeque<(u32, u32)>,
    capacity: usize,
    budget_bytes: u64,
    pub(super) resident_bytes: u64,
}

#[cfg(feature = "gpu")]
impl PoolCore {
    pub(super) fn new(capacity: usize, budget_bytes: u64) -> Self {
        Self {
            order: std::collections::VecDeque::new(),
            capacity: capacity.max(1),
            budget_bytes: budget_bytes.max(1),
            resident_bytes: 0,
        }
    }

    /// Bytes one entry of `(width, height)` occupies in VRAM
    /// (RGBA8 output + R16Uint mask).
    pub(super) fn entry_bytes(width: u32, height: u32) -> u64 {
        width as u64 * height as u64 * (4 + 2)
    }

    pub(super) fn len(&self) -> usize {
        self.order.len()
    }

    pub(super) fn contains(&self, key: &(u32, u32)) -> bool {
        self.order.iter().any(|k| k == key)
    }

    /// Mark `key` most-recently-used. No-op for unknown keys.
    pub(super) fn touch(&mut self, key: &(u32, u32)) {
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
    pub(super) fn admit(&mut self, key: (u32, u32)) -> Vec<(u32, u32)> {
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
    pub(super) fn new() -> Self {
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
    pub(super) fn get_or_create(
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
    pub(super) fn active(&mut self) -> Option<&mut VramState> {
        let key = *self.core.order.front()?;
        self.entries.get_mut(&key)
    }
}

#[cfg(feature = "gpu")]
impl GpuContext {
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
    // R2-GPU-01: cache the base (source) texture per pool entry so the tone
    // pass reuses it across draft ticks instead of re-creating + re-uploading
    // the full frame every slider tick.
    let input = shaders::create_input_texture(device, width, height, "lumina-gpu-vram-base");
    let input_view = input.create_view(&wgpu::TextureViewDescriptor::default());
    let input_sampler = shaders::create_sampler(device, "lumina-gpu-vram-base-samp");
    // R2-GPU-02: build the overlay bind group once — every part it references is
    // stable in VRAM, so the per-frame present path only reuses it.
    let overlay_bind_group = shaders::create_overlay_bind_group(
        device,
        &overlay_layout,
        &overlay_uniform,
        &output_view,
        &mask_view,
        &color_sampler,
    );
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
        overlay_uniform,
        input,
        input_view,
        input_sampler,
        input_source_identity: None,
        overlay_pipelines: std::collections::HashMap::new(),
        overlay_bind_group,
    })
}
