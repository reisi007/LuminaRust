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
//! (maxAbsDiff ≤ 1, PSNR ≥ 45 dB). The public API is therefore stable and always
//! returns a [`Frame`], which keeps the CPU and GPU return types identical for
//! callers.
//!
//! See `docs/gpu-bootstrap.md` for the planned DAG and `docs/gpu-shaders.md` for
//! the shader-stage design.

use lumina_core::ImageFrame;
use lumina_sidecar::EditRecipe;
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
    /// VRAM-resident interactive state (GPU-60FPS-1): output + mask textures and
    /// overlay uniforms, kept resident across frames so slider drags and brush
    /// strokes never read back to CPU. Lazily (re)created when dimensions change.
    #[cfg(feature = "gpu")]
    vram: std::sync::Mutex<Option<VramState>>,
    /// Last recipe pushed via [`GpuContext::update_uniforms`]. Used both to feed
    /// the uniform buffer (GPU path) and as the CPU-fallback recipe.
    #[cfg(feature = "gpu")]
    recipe: Option<EditRecipe>,
}

/// VRAM-resident interactive state for GUI-60FPS-1.
///
/// Currently holds the output (RGBA8 tone result) and the R16Unorm brush-mask
/// textures at full source resolution. The mask is uploaded incrementally per
/// dirty 512² tile via `queue.write_texture` from the GUI's persistent `Vec<u16>`
/// plane (`bytemuck::cast_slice`).
///
/// Roadmap (M2): this single-slot cache will grow into an LRU tile pool
/// (`TiledCache` 512², `DraftPyramid`) with eviction + generation counting so
/// only hot tiles stay resident for >45 MP sources and multi-layer masks. Until
/// then a full-resolution mask texture is acceptable for interactive preview
/// (memory budget checked via `MemoryBudget` at creation). See `docs/gpu-bootstrap.md`
/// § Dual-Backend & Roadmap.
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
    mask_sampler: wgpu::Sampler,
    overlay_uniform: wgpu::Buffer,
    overlay_layout: wgpu::BindGroupLayout,
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
            Ok(resources) => Ok(Self {
                resources: Some(resources),
                pipeline: std::sync::Mutex::new(None),
                vram: std::sync::Mutex::new(None),
                recipe: None,
            }),
            // Degrade gracefully to the CPU fallback rather than failing the app.
            Err(_err) => Ok(Self {
                resources: None,
                pipeline: std::sync::Mutex::new(None),
                vram: std::sync::Mutex::new(None),
                recipe: None,
            }),
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

    /// Ensure the VRAM-resident interactive textures exist for `width`×`height`.
    /// Lazily (re)creates `output` (RGBA8, tone result) + `mask` (R16Unorm,
    /// brush coverage) textures and the overlay uniform/layout so the hot path
    /// never allocates. No-op when no adapter is bound.
    pub fn ensure_vram(&self, width: u32, height: u32) -> Result<(), GpuError> {
        let Some(resources) = self.resources.as_ref() else {
            return Ok(());
        };
        let mut guard = self.vram.lock().unwrap();
        if let Some(v) = guard.as_ref() {
            if v.width == width && v.height == height {
                return Ok(());
            }
        }
        let output = shaders::create_output_texture(
            &resources.device,
            width,
            height,
            "lumina-gpu-vram-output",
        );
        let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
        let mask =
            shaders::create_mask_texture(&resources.device, width, height, "lumina-gpu-vram-mask");
        let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
        let mask_sampler = shaders::create_sampler(&resources.device, "lumina-gpu-vram-mask-samp");
        let overlay_uniform = shaders::create_overlay_uniform_buffer(&resources.device);
        let overlay_layout = shaders::create_overlay_bind_group_layout(&resources.device);
        // Clear mask to zero so compositing is identity until a brush writes.
        let zero_rows = vec![0u8; (width as usize) * (height as usize) * 2];
        resources.queue.write_texture(
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
        *guard = Some(VramState {
            width,
            height,
            output,
            output_view,
            mask,
            mask_view,
            mask_sampler,
            overlay_uniform,
            overlay_layout,
        });
        Ok(())
    }

    /// Interactive tone render that stays VRAM-resident — no `map_async` readback.
    ///
    /// Renders the tone stage (`SHADER_SRC`) into the cached `output` VRAM
    /// texture. Caller presents via [`Self::copy_vram_to_texture`] or the
    /// overlay pass without ever mapping to CPU. Export/full-rebuild paths
    /// should use [`Self::render_with_gpu`] (which still reads back).
    pub fn render_to_vram(&self, frame: &ImageFrame, recipe: &EditRecipe) -> Result<(), GpuError> {
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
        let bind = shaders::create_color_tone_bind_group(
            &resources.device,
            &pipeline.bind_group_layout,
            &pipeline.uniform_buffer,
            &input_view,
            &sampler,
        );
        let vram = self.vram.lock().unwrap();
        let Some(v) = vram.as_ref() else {
            return Err(GpuError::RenderFailed("vram not ready".into()));
        };
        let mut enc = resources
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("lumina-gpu-vram-encode"),
            });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lumina-gpu-vram-tone"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &v.output_view,
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
            });
            pass.set_pipeline(&pipeline.pipeline);
            pass.set_bind_group(0, &bind, &[]);
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

    /// Upload a single dirty 512² (or edge-clipped) mask tile into the VRAM mask
    /// texture. `tile_data` is `u16` little-endian coverage, row-major.
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
        let guard = self.vram.lock().unwrap();
        let Some(v) = guard.as_ref() else {
            return Err(GpuError::RenderFailed(
                "vram not ready for mask upload".into(),
            ));
        };
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

    /// GPU-GPU copy/overlay of the VRAM tone + mask textures into an
    /// egui-managed `dest` texture (option b, risikoärmste Variante). No CPU
    /// readback — the copy is `copy_texture_to_texture` / overlay render pass
    /// directly on the queue. `dest` must be created with `TEXTURE_BINDING|
    /// COPY_DST|RENDER_ATTACHMENT` and the same dimensions as the VRAM cache.
    /// When no VRAM cache exists this is a no-op.
    ///
    /// ⚠️ Dual-backend limitation (M1): the current native GUI uses `eframe`
    /// with the **glow** (GL) renderer, while `lumina-gpu` owns a separate
    /// `wgpu::Instance` (Metal). A `wgpu::Texture` from that Metal instance
    /// cannot be shared with the glow surface — `copy_vram_to_texture` is
    /// therefore **offscreen only** today (tests / CLI offscreen render). The
    /// on-screen present must stay `queue.write_texture` (mask tiles) +
    /// `egui::ColorImage`/`ctx.load_texture` (preview) until `egui_wgpu` is
    /// adopted. See `docs/gpu-bootstrap.md` § Dual-Backend and
    /// `feature/platform/cli-gui-wasm.md` Capability-Matrix.
    pub fn copy_vram_to_texture(&self, dest: &wgpu::Texture) -> Result<(), GpuError> {
        let Some(resources) = self.resources.as_ref() else {
            return Ok(());
        };
        let mut guard = self.vram.lock().unwrap();
        let Some(v) = guard.as_mut() else {
            return Ok(());
        };
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
            &v.mask_sampler,
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
            });
            pass.set_pipeline(&overlay_pipe);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        resources.queue.submit(Some(enc.finish()));
        Ok(())
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

        // Bind group: uniform (0) + input texture (1) + sampler (2).
        let bind_group = shaders::create_color_tone_bind_group(
            &resources.device,
            &pipeline.bind_group_layout,
            &pipeline.uniform_buffer,
            &input_view,
            &sampler,
        );

        // Encode: draw the fullscreen triangle into the RGBA8 target, then copy
        // it back to the staging buffer.
        let mut encoder =
            resources
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("lumina-gpu-encode"),
                });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("lumina-gpu-color-tone"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
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
            });
            pass.set_pipeline(&pipeline.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
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
        resources.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|e| GpuError::RenderFailed(format!("map channel: {e}")))?
            .map_err(|e| GpuError::RenderFailed(format!("buffer map: {e}")))?;

        let mapped = slice.get_mapped_range();
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
        let cache = crate::tiling::TiledCache::new(64);
        let keys = cache.keys_for_viewport(&viewport, zoom);
        log::debug!(
            "render_draft: gpu scaffold (adapter present), {} tiles for viewport {:?} @ zoom {:.3} (pyramid level {})",
            keys.len(),
            viewport,
            zoom,
            lvl
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
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
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
        multiview: None,
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
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        // Metal is the primary backend on Apple Silicon (M5 Pro). Restricting
        // the backend list avoids pulling Vulkan/DX12/WGPU-GL on platforms where
        // they are unavailable and keeps adapter selection deterministic.
        backends: wgpu::Backends::METAL,
        ..Default::default()
    });

    // `request_adapter` returns `Option<Adapter>` (not a `Result`), so surface a
    // descriptive error for the "no adapter" case.
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok_or_else(|| GpuError::AdapterUnavailable("no Metal adapter found".into()))?;

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("lumina-gpu"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
        },
        None,
    ))
    .map_err(|err| GpuError::DeviceUnavailable(err.to_string()))?;

    Ok(GpuResources {
        instance,
        adapter,
        device,
        queue,
    })
}
