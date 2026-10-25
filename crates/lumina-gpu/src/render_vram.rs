//! Readback-free VRAM present path and its byte-level diagnostic readback
//! (`GpuContext::render_to_vram` / `readback_output_frame`), extracted from
//! `lib.rs` (file-size ratchet §8).

use super::resources::{encode_source_action_batches, encode_spot_heal, post_stages_needed};
use super::{
    geometry, lens_blur, shaders, unsupported_gpu_stages_for, validate_gpu_recipe,
    vram_pool::{should_upload, upload_source_texture, UploadIdentity},
    warn_unsupported_vram_once, warn_vram_dimension_change_once, Frame, GpuContext, GpuError,
    ImageFrame, SourceActionBatches,
};
use lumina_sidecar::EditRecipe;

impl GpuContext {
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
        // the source frame instance or the target geometry changes
        // (GPU-UPLOAD-IDENTITY-92: instance identity, never address/content).
        // During a slider drag the same source is passed every tick, so this
        // skips the ~96 MB CPU→GPU upload on all but the first tick.
        let mut vram_guard = self.vram.lock().unwrap();
        let Some(v) = vram_guard.active() else {
            return Err(GpuError::RenderFailed("vram not ready".into()));
        };
        if should_upload(
            v.input_source_identity,
            frame.width(),
            frame.height(),
            frame,
        ) {
            upload_source_texture(&resources.queue, &v.input, frame);
            v.input_source_identity =
                Some(UploadIdentity::of(frame, frame.width(), frame.height()));
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
}
