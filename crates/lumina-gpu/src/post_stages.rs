//! GPU post-tone and geometry chain encoding (GPU-RENDER-PARITY-1).

use super::{
    encode_fullscreen_pass, encode_geometry_pass, geometry, shaders, stages, GpuContext, GpuError,
    GpuResources, PostWrite, UploadedGenerativeArtifact,
};
use lumina_sidecar::EditRecipe;

#[rustfmt::skip]
impl GpuContext {
    /// Encode the post-tone adjustment chain (GPU-RENDER-PARITY-1) so `output`
    /// ends up holding the pixel the CPU oracle would produce for `recipe`
    /// after the tone pass.
    ///
    /// Pass order mirrors `apply_recipe_with_scale_and_white_balance` exactly:
    /// `Presence Texture DoG → Presence Clarity DoG → Dehaze → color
    /// (curves → HSL → Point Color → vibrance/saturation → Color Grading)
    /// → Noise Reduction → Sharpening → Red-Eye → vignette → grain`.
    /// Each pass is a self-contained fullscreen draw into a ping-pong scratch
    /// texture; the last pass writes `output`. Dehaze (dark-channel percentile)
    /// and Sharpening (gradient maximum) are the only passes that need a host
    /// round-trip, so those pass groups are split across submissions — every
    /// other pass shares one encoder. No-op when the recipe uses none of these
    /// stages.
    pub(super) fn render_post_stages(
        &self,
        resources: &GpuResources,
        width: u32,
        height: u32,
        recipe: &EditRecipe,
        tone_view: &wgpu::TextureView,
        output_view: &wgpu::TextureView,
    ) -> Result<(), GpuError> {
        // Resolve the ordered list of writing passes.
        let mut writes: Vec<PostWrite> = Vec::new();
        if let Some(presence) = recipe.presence {
            if presence.texture != 0.0 {
                writes.push(PostWrite::Dog(stages::DogParams::texture(presence.texture)));
            }
            if presence.clarity != 0.0 {
                writes.push(PostWrite::Dog(stages::DogParams::clarity(presence.clarity)));
            }
            if presence.dehaze != 0.0 {
                writes.push(PostWrite::Dehaze(presence.dehaze));
            }
        }
        if stages::ColorParams::needs_stage(recipe) {
            writes.push(PostWrite::Color(Box::new(
                stages::ColorParams::from_recipe(recipe),
            )));
        }
        // GPU-RENDER-PARITY-1 stage 2: detail stages in the oracle's order.
        if let Some(noise) = recipe
            .noise_reduction
            .as_ref()
            .filter(|n| stages::NoiseParams::needs_stage(n))
        {
            writes.push(PostWrite::Noise(stages::NoiseParams::from_recipe(noise)));
        }
        if let Some(sharpening) = recipe
            .sharpening
            .as_ref()
            .filter(|s| stages::SharpenParams::needs_stage(s))
        {
            writes.push(PostWrite::Sharpen(Box::new(
                stages::SharpenParams::from_sharpening(sharpening),
            )));
        }
        // G-14: red-eye runs after sharpening and before effects, exactly like
        // `apply_recipe` (grain then applies uniformly over corrected pupils).
        if let Some(red_eye) = recipe
            .red_eye
            .as_ref()
            .filter(|r| stages::RedEyeParams::needs_stage(r))
        {
            writes.push(PostWrite::RedEye(Box::new(
                stages::RedEyeParams::from_red_eye(red_eye),
            )));
        }
        if let Some(effects) = recipe.effects.as_ref() {
            if let Some(vignette) = effects.vignette.as_ref().filter(|v| v.amount != 0.0) {
                writes.push(PostWrite::Vignette(stages::VignetteParams::from_vignette(
                    vignette, width, height,
                )));
            }
            if let Some(grain) = effects.grain.as_ref().filter(|g| g.amount != 0.0) {
                writes.push(PostWrite::Grain(stages::GrainParams::from_grain(
                    grain, width, height,
                )));
            }
        }
        if writes.is_empty() {
            return Ok(());
        }

        let post = self.ensure_post_pipelines()?;
        let Some(post) = post.as_ref() else {
            return Ok(());
        };

        // Ping-pong scratch targets for the intermediate passes. `create_output_texture`
        // already carries RENDER_ATTACHMENT|TEXTURE_BINDING|COPY_SRC.
        let scratch0 = shaders::create_output_texture(
            &resources.device,
            width,
            height,
            "lumina-gpu-post-scratch0",
        );
        let scratch0_view = scratch0.create_view(&wgpu::TextureViewDescriptor::default());
        let scratch1 = shaders::create_output_texture(
            &resources.device,
            width,
            height,
            "lumina-gpu-post-scratch1",
        );
        let scratch1_view = scratch1.create_view(&wgpu::TextureViewDescriptor::default());
        let dark = shaders::create_output_texture(
            &resources.device,
            width,
            height,
            "lumina-gpu-post-dark",
        );
        let dark_view = dark.create_view(&wgpu::TextureViewDescriptor::default());
        let scratch_views = [&scratch0_view, &scratch1_view];

        let count = writes.len();
        let mut current: &wgpu::TextureView = tone_view;
        let mut ping = 0usize;
        let mut encoder =
            resources
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("lumina-gpu-post"),
                });
        for (index, write) in writes.iter().enumerate() {
            let is_last = index + 1 == count;
            let dst = if is_last {
                output_view
            } else {
                scratch_views[ping]
            };
            match write {
                PostWrite::Dog(params) => {
                    // A dedicated uniform buffer per DoG pass: two passes can
                    // share one encoder, and a shared buffer would make the
                    // second `write_buffer` retroactively change the first draw.
                    let dog_params = stages::create_dog_params_buffer(&resources.device);
                    stages::write_dog_params(&resources.queue, &dog_params, params);
                    let bind = stages::create_dog_bind_group(
                        &resources.device,
                        &post.dog_layout,
                        &dog_params,
                        current,
                    );
                    encode_fullscreen_pass(&mut encoder, &post.dog_pipeline, &bind, dst);
                }
                PostWrite::Dehaze(strength) => {
                    // The dark-channel percentile has to reach the host before
                    // the apply pass, so flush the encoder built so far (which
                    // holds every preceding DoG pass) together with the dark
                    // pass, then continue in a fresh encoder.
                    let dark_bind = stages::create_dark_bind_group(
                        &resources.device,
                        &post.dark_layout,
                        current,
                    );
                    encode_fullscreen_pass(
                        &mut encoder,
                        &post.dark_pipeline,
                        &dark_bind,
                        &dark_view,
                    );
                    resources.queue.submit(Some(encoder.finish()));
                    let airlight = self.readback_dark_airlight(resources, &dark, width, height)?;
                    encoder =
                        resources
                            .device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("lumina-gpu-post-dehaze"),
                            });
                    let params = stages::DehazeParams {
                        airlight,
                        strength: *strength,
                        _pad: [0; 2],
                    };
                    stages::write_dehaze_params(&resources.queue, &post.dehaze_params, &params);
                    let bind = stages::create_dehaze_bind_group(
                        &resources.device,
                        &post.dehaze_layout,
                        &post.dehaze_params,
                        current,
                        &dark_view,
                    );
                    encode_fullscreen_pass(&mut encoder, &post.dehaze_pipeline, &bind, dst);
                }
                PostWrite::Color(params) => {
                    stages::write_color_params(&resources.queue, &post.color_params, params);
                    let bind = stages::create_color_bind_group(
                        &resources.device,
                        &post.color_layout,
                        &post.color_params,
                        current,
                    );
                    encode_fullscreen_pass(&mut encoder, &post.color_pipeline, &bind, dst);
                }
                PostWrite::Noise(params) => {
                    stages::write_noise_params(&resources.queue, &post.noise_params, params);
                    let bind = stages::create_noise_bind_group(
                        &resources.device,
                        &post.noise_layout,
                        &post.noise_params,
                        current,
                    );
                    encode_fullscreen_pass(&mut encoder, &post.noise_pipeline, &bind, dst);
                }
                PostWrite::Vignette(params) => {
                    stages::write_vignette_params(&resources.queue, &post.vignette_params, params);
                    let bind = stages::create_vignette_bind_group(
                        &resources.device,
                        &post.vignette_layout,
                        &post.vignette_params,
                        current,
                    );
                    encode_fullscreen_pass(&mut encoder, &post.vignette_pipeline, &bind, dst);
                }
                PostWrite::Grain(params) => {
                    stages::write_grain_params(&resources.queue, &post.grain_params, params);
                    let bind = stages::create_grain_bind_group(
                        &resources.device,
                        &post.grain_layout,
                        &post.grain_params,
                        current,
                    );
                    encode_fullscreen_pass(&mut encoder, &post.grain_pipeline, &bind, dst);
                }
                PostWrite::Sharpen(params) => {
                    // Sharpening needs the gradient maximum on the host before
                    // its apply pass. Flush the encoder built so far (every
                    // preceding pass) and run the self-contained multi-pass
                    // chain; continue afterwards in a fresh encoder.
                    resources.queue.submit(Some(encoder.finish()));
                    encoder =
                        resources
                            .device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("lumina-gpu-post-sharpen"),
                            });
                    stages::write_sharpen_params(&resources.queue, &post.sharpen_params, params);
                    self.encode_sharpening(resources, post, (width, height), params, current, dst)?;
                }
                PostWrite::RedEye(params) => {
                    stages::write_red_eye_params(&resources.queue, &post.red_eye_params, params);
                    let bind = stages::create_red_eye_bind_group(
                        &resources.device,
                        &post.red_eye_layout,
                        &post.red_eye_params,
                        current,
                    );
                    encode_fullscreen_pass(&mut encoder, &post.red_eye_pipeline, &bind, dst);
                }
            }
            current = dst;
            if !is_last {
                ping ^= 1;
            }
        }
        resources.queue.submit(Some(encoder.finish()));
        Ok(())
    }

    /// Encode the planned geometry pass chain (GPU-RENDER-PARITY-1 geometry
    /// wave) into `encoder`, sampling `input_view` and writing the final pass
    /// into `final_view` (whose dimensions must equal `plan.output_dims()`).
    ///
    /// Each sub-stage is a self-contained fullscreen pass at its own output
    /// dimensions; earlier passes land in transient ping-pong textures that the
    /// next pass samples, mirroring the oracle's per-stage `ImageFrame` chain.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn encode_geometry_chain(
        &self,
        resources: &GpuResources,
        geo: &geometry::GeometryPipelineState,
        plan: &geometry::GeometryPlan,
        input_view: &wgpu::TextureView,
        final_texture: &wgpu::Texture,
        final_view: &wgpu::TextureView,
        artifacts: &[Option<UploadedGenerativeArtifact>; 2],
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<(), GpuError> {
        use geometry::GeometryStep;
        // GEN-ONNX-1 Welle 2a: `Substitute` steps swap the current texture to a
        // caller-uploaded artifact (no shader pass). When the plan **ends** with
        // a `Substitute` (trailing), that artifact is the render result: any
        // earlier real render pass writes an intermediate and the trailing
        // artifact is copied into `final_view` afterwards. Otherwise the last
        // real pass writes `final_view` directly.
        let trailing_substitute = plan.steps.last().is_some_and(GeometryStep::is_substitute);
        let last_render = plan.steps.iter().rposition(|step| !step.is_substitute());
        // The pass that writes `final_view`; `None` when a trailing substitute
        // is the actual result, so the last real pass must go to an
        // intermediate.
        let final_pass = if trailing_substitute {
            None
        } else {
            last_render
        };
        let mut intermediates: Vec<Option<wgpu::Texture>> = Vec::with_capacity(plan.steps.len());
        for (index, step) in plan.steps.iter().enumerate() {
            if Some(index) != final_pass && !step.is_substitute() {
                let (width, height) = step.out_dims();
                intermediates.push(Some(shaders::create_output_texture(
                    &resources.device,
                    width,
                    height,
                    &format!("lumina-gpu-geometry-intermediate-{index}"),
                )));
            } else {
                intermediates.push(None);
            }
        }
        let intermediate_views: Vec<Option<wgpu::TextureView>> = intermediates
            .iter()
            .map(|texture| {
                texture
                    .as_ref()
                    .map(|texture| texture.create_view(&wgpu::TextureViewDescriptor::default()))
            })
            .collect();

        let mut current: &wgpu::TextureView = input_view;
        for (index, step) in plan.steps.iter().enumerate() {
            if let GeometryStep::Substitute { artifact_index, .. } = step {
                let artifact = artifacts
                    .get(*artifact_index)
                    .and_then(Option::as_ref)
                    .ok_or_else(|| {
                        GpuError::RenderFailed(format!(
                            "generative artifact {artifact_index} was not uploaded for the geometry chain"
                        ))
                    })?;
                current = &artifact.view;
                continue;
            }
            let dst: &wgpu::TextureView = if Some(index) == final_pass {
                final_view
            } else {
                intermediate_views[index]
                    .as_ref()
                    .expect("non-final render step is preallocated")
            };
            match step {
                GeometryStep::Substitute { .. } => unreachable!("handled above"),
                GeometryStep::Lens { params, .. } => encode_geometry_pass(
                    resources,
                    &geo.layout,
                    &geo.lens,
                    params,
                    current,
                    dst,
                    encoder,
                ),
                GeometryStep::Perspective { params, .. } => encode_geometry_pass(
                    resources,
                    &geo.layout,
                    &geo.perspective,
                    params,
                    current,
                    dst,
                    encoder,
                ),
                GeometryStep::Ca { params, .. } => encode_geometry_pass(
                    resources,
                    &geo.layout,
                    &geo.ca,
                    params,
                    current,
                    dst,
                    encoder,
                ),
                GeometryStep::Crop { params, .. } => encode_geometry_pass(
                    resources,
                    &geo.layout,
                    &geo.crop,
                    params,
                    current,
                    dst,
                    encoder,
                ),
                GeometryStep::Rotate { params, .. } => encode_geometry_pass(
                    resources,
                    &geo.layout,
                    &geo.rotate,
                    params,
                    current,
                    dst,
                    encoder,
                ),
                GeometryStep::Mirror { params, .. } => encode_geometry_pass(
                    resources,
                    &geo.layout,
                    &geo.mirror,
                    params,
                    current,
                    dst,
                    encoder,
                ),
                // GPU-LENSFUN-PARITY-1: resample with the caller-bound map. The
                // plan only contains a Lensfun step when a map is bound (the
                // planner receives it), so a missing binding is a loud error.
                GeometryStep::Lensfun { .. } => {
                    let map = self.lensfun_map.as_ref().ok_or_else(|| {
                        GpuError::RenderFailed(
                            "geometry chain planned a Lensfun step without a bound map".into(),
                        )
                    })?;
                    crate::lensfun::encode_lensfun_pass(
                        resources,
                        &geo.lensfun,
                        map,
                        current,
                        dst,
                        encoder,
                    )?;
                }
            }
            current = dst;
        }

        if trailing_substitute {
            // The plan ends with a substitute: the trailing artifact is the
            // render result (this also covers the substitute-only chain).
            let Some(GeometryStep::Substitute { artifact_index, .. }) = plan.steps.last() else {
                return Ok(());
            };
            let artifact = artifacts
                .get(*artifact_index)
                .and_then(Option::as_ref)
                .ok_or_else(|| {
                    GpuError::RenderFailed(format!(
                        "generative artifact {artifact_index} was not uploaded for the geometry chain"
                    ))
                })?;
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &artifact.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: final_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: plan.output_width,
                    height: plan.output_height,
                    depth_or_array_layers: 1,
                },
            );
        }
        Ok(())
    }

    /// Read the GPU dark-channel texture back and return the oracle's
    /// deterministic Dehaze airlight ([`stages::dehaze_airlight`]).
    fn readback_dark_airlight(
        &self,
        resources: &GpuResources,
        dark: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<f32, GpuError> {
        // `aligned_bytes_per_row` already accounts for the 4 bytes per RGBA8
        // texel; the dark-channel texture is RGBA8, so `aligned(width)` — not
        // `aligned(width * 4)`, which over-allocated 4× (≈372 MiB at 24 MP) and
        // exceeded the adapter's 256 MiB `max_buffer_size`.
        let bytes_per_row = shaders::aligned_bytes_per_row(width);
        let staging = resources.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lumina-gpu-dark-readback"),
            size: (bytes_per_row * height.max(1)) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder =
            resources
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("lumina-gpu-dark-readback-enc"),
                });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: dark,
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
        resources.queue.submit(Some(encoder.finish()));
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
        let mut dark_bytes = Vec::with_capacity((width * height) as usize);
        for y in 0..height as usize {
            let row = &mapped[y * bytes_per_row as usize..(y + 1) * bytes_per_row as usize];
            for x in 0..width as usize {
                dark_bytes.push(row[x * 4]);
            }
        }
        drop(mapped);
        staging.unmap();
        Ok(stages::dehaze_airlight(&dark_bytes))
    }

}
