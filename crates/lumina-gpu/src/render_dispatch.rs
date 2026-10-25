//! Readback render entry points, extracted from `lib.rs` (file-size ratchet §8).

use super::resources::{
    encode_source_action_batches, encode_spot_heal, post_stages_needed, upload_generative_artifacts,
};
use super::{
    geometry, lens_blur, log_cpu_routing_once, readback_texture, render_cpu_with_generative,
    shaders, unsupported_gpu_stages_for, validate_gpu_recipe, CpuFallbackReason, Frame, GpuContext,
    GpuError, ImageFrame, RenderExecutionReport, RenderWithGpuResources, SourceActionBatches,
    UploadedGenerativeArtifact,
};
// Doc-link only: the moved docs name the `stages` module and the routing verdicts
// they referred to at the crate root, where all three were in scope.
#[allow(unused_imports)]
use super::{stages, unsupported_gpu_stages, unsupported_gpu_stages_with_context};
use lumina_sidecar::EditRecipe;

impl GpuContext {
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
        self.render_with_gpu_report(frame, recipe)
            .map(|(frame, _report)| frame)
    }

    /// Render one frame and report whether this call completed the GPU pipeline
    /// or returned a CPU-fallback frame, with the internal fallback reason.
    ///
    /// `GpuPipelineCompleted` is returned only after the final GPU output has
    /// been submitted and successfully read back. It is not a pixel-parity
    /// claim; render and readback failures remain errors with no report.
    pub fn render_with_gpu_report(
        &self,
        frame: &ImageFrame,
        recipe: &EditRecipe,
    ) -> Result<(Frame, RenderExecutionReport), GpuError> {
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
        self.render_with_gpu_and_generative_report(frame, recipe, generative)
            .map(|(frame, _report)| frame)
    }

    /// Artifact-aware render with the execution outcome for this output frame.
    pub fn render_with_gpu_and_generative_report(
        &self,
        frame: &ImageFrame,
        recipe: &EditRecipe,
        generative: &lumina_core::GenerativeCanvasInput<'_>,
    ) -> Result<(Frame, RenderExecutionReport), GpuError> {
        self.render_with_gpu_inner(frame, recipe, Some(generative))
    }

    fn render_with_gpu_inner(
        &self,
        frame: &ImageFrame,
        recipe: &EditRecipe,
        generative: Option<&lumina_core::GenerativeCanvasInput<'_>>,
    ) -> Result<(Frame, RenderExecutionReport), GpuError> {
        // GPU-LENSFUN-PARITY-1: the free `render_cpu` reference does not carry a
        // caller-bound Lensfun map (the corrector is render-context state,
        // F-098-N1), so a fallback would silently drop the correction. Refuse
        // loudly instead — the caller owns the full CPU reference for a bound
        // map (the GUI presents the exact CPU frame itself).
        let cpu_fallback = |frame: &ImageFrame, reason: CpuFallbackReason| {
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
            .map(|frame| (frame, RenderExecutionReport::CpuFallback { reason }))
        };
        // CAMERA-WB-WELLE (R2-MCP-01): validate the caller-bound As-Shot context
        // exactly like the CPU oracle (invalid gains abort before any pixel) and
        // thread it into every CPU-fallback render so a routed render keeps the
        // complete context contract. Valid gains are pixel-neutral on both
        // backends (the decoder already applied them).
        self.validate_bound_camera_white_balance()?;
        let Some(resources) = self.resources.as_ref() else {
            return cpu_fallback(frame, CpuFallbackReason::AdapterOrDeviceUnavailable);
        };
        // R2-GPU-06: a lost device must not panic — degrade to the CPU oracle.
        if resources
            .device_lost
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            log::warn!("GPU device lost; routing render_with_gpu to CPU");
            return cpu_fallback(frame, CpuFallbackReason::DeviceLost);
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
            return cpu_fallback(frame, CpuFallbackReason::UnsupportedStages(unsupported));
        }
        self.ensure_pipeline()?;
        let guard = self.pipeline.lock().unwrap();
        let Some(pipeline) = guard.as_ref() else {
            return cpu_fallback(frame, CpuFallbackReason::PipelineUnavailable);
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
                )
                .map(|frame| (frame, RenderExecutionReport::GpuPipelineCompleted));
            }
            drop(cache_guard);
            return readback_texture(
                resources,
                &final_texture,
                plan.output_width,
                plan.output_height,
            )
            .map(|frame| (frame, RenderExecutionReport::GpuPipelineCompleted));
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

        Ok((
            Frame {
                width,
                height,
                pixels,
            },
            RenderExecutionReport::GpuPipelineCompleted,
        ))
    }
}
