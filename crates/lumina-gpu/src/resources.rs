//! GPU adapter/device init, the lazy pipeline builders and the encode plumbing
//! used by the render entry points, extracted from `lib.rs` (file-size ratchet
//! §8).

use super::color_tone_shader::SHADER_SRC;
use super::gpu_util::encode_fullscreen_pass;
use super::{
    geometry, lens_blur, log_gpu_init_failure, shaders, stages, GpuContext, GpuError, GpuResources,
    PipelineState, PostPipelineState, SourceActionBatches, SourceActionPipelineState,
    SpotPipelineState, UploadedGenerativeArtifact, VramPool, MAX_SOURCE_ACTIONS,
};
use lumina_sidecar::EditRecipe;

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
            // app — but never silently (REVIEW-GPU-N1, AGENTS.md: no silent
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
    pub(super) fn ensure_pipeline(&self) -> Result<(), GpuError> {
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
    pub(super) fn ensure_source_action_pipeline(&self) -> Result<(), GpuError> {
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
    pub(super) fn ensure_spot_pipeline(&self) -> Result<(), GpuError> {
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
    pub(super) fn ensure_post_pipelines(
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
    pub(super) fn ensure_geometry_pipelines(
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
    pub(super) fn encode_lens_blur_stage(
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

/// Whether the recipe uses any post-tone adjustment stage the GPU renders in
/// [`GpuContext::render_post_stages`]. The tone target is chosen from this so a
/// recipe that needs no post pass keeps rendering straight into the output.
/// Each arm mirrors the corresponding `PostWrite` enqueue exactly, so the
/// chosen tone target can never diverge from the passes that run.
#[cfg(feature = "gpu")]
pub(super) fn post_stages_needed(recipe: &EditRecipe) -> bool {
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

/// Encode the (possibly batched) source-action composites into `encoder`.
///
/// WGSL cannot index texture bindings dynamically, so the stage unrolls
/// `MAX_SOURCE_ACTIONS` slot pairs per pass. More artifacts are composited in
/// sequential batches, each sampling the previous batch's output — matching the
/// oracle's sequential `apply_source_actions` order (later artifacts win on
/// overlap) without the former hard slot limit (GPU-RENDER-PARITY-1 item 7).
#[cfg(feature = "gpu")]
#[allow(clippy::type_complexity)]
pub(super) fn encode_source_action_batches(
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
pub(super) fn encode_spot_heal(
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

/// Upload the caller's generative artifacts (auto-fill at index 0, expand at
/// index 1) as `Rgba8Unorm` textures, validating role and dimensions. Missing
/// roles stay `None` so the fixed indices the geometry plan uses are stable.
#[cfg(feature = "gpu")]
pub(super) fn upload_generative_artifacts(
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
