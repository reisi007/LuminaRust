//! GPU execution for caller-evaluated local-mask layers.

use crate::local_mask_gpu::{build_local_mask_pipeline, LocalMaskPipeline, WbUniforms};
use crate::local_mask_pass::LocalMaskLayer;
use crate::{gpu_util, shaders, Frame, GpuContext, GpuError, GpuResources};
use lumina_sidecar::LocalAdjustments;

/// Whether a layer takes the relative-WB float chain rather than the delta-free
/// global tone path (`local_wb::apply_mask_local_recipe_with_scale`).
#[cfg(feature = "gpu")]
fn has_wb_delta(adjustments: &LocalAdjustments) -> bool {
    adjustments.temperature_delta_k != 0.0 || adjustments.tint_delta != 0.0
}

#[cfg(feature = "gpu")]
impl GpuContext {
    /// Run the mask-local chain (recipe → blend per layer) over the global
    /// result and read the final pixels back.
    pub(crate) fn run_local_mask_chain(
        &self,
        resources: &GpuResources,
        global: &Frame,
        layers: &[LocalMaskLayer],
    ) -> Result<Frame, GpuError> {
        let (width, height) = (global.width, global.height);
        let pipeline = build_local_mask_pipeline(&resources.device);
        let sampler = shaders::create_sampler(&resources.device, "lumina-gpu-localmask-sampler");

        let current = shaders::create_output_texture(
            &resources.device,
            width,
            height,
            "lumina-gpu-localmask-current",
        );
        resources.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &current,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &global.pixels,
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

        let current = self.run_local_mask_layers(
            resources, current, width, height, layers, &pipeline, &sampler,
        )?;
        gpu_util::readback_texture(resources, &current, width, height)
    }

    /// Apply the layer chain to an owned RGBA texture without reading pixels
    /// back. The input texture is never used as a render target; each stage
    /// writes a distinct texture and returns the last result for readback or
    /// copy-back into the resident present texture.
    #[allow(clippy::too_many_arguments)] // Explicit pass inputs keep texture ownership visible.
    pub(crate) fn run_local_mask_layers(
        &self,
        resources: &GpuResources,
        mut current: wgpu::Texture,
        width: u32,
        height: u32,
        layers: &[LocalMaskLayer],
        pipeline: &LocalMaskPipeline,
        sampler: &wgpu::Sampler,
    ) -> Result<wgpu::Texture, GpuError> {
        for layer in layers.iter().filter(|layer| !layer.is_neutral()) {
            let mask = shaders::create_mask_texture(
                &resources.device,
                width,
                height,
                "lumina-gpu-localmask-plane",
            );
            shaders::write_u16_plane(&resources.queue, &mask, width, height, &layer.plane.values);
            let adjusted = shaders::create_output_texture(
                &resources.device,
                width,
                height,
                "lumina-gpu-localmask-adjusted",
            );
            let next = {
                let current_view = current.create_view(&wgpu::TextureViewDescriptor::default());
                let adjusted_view = adjusted.create_view(&wgpu::TextureViewDescriptor::default());
                let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
                let mut encoder =
                    resources
                        .device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("lumina-gpu-localmask-enc"),
                        });
                if has_wb_delta(&layer.adjustments) {
                    let uniforms = WbUniforms::from_adjustments(&layer.adjustments);
                    resources.queue.write_buffer(
                        &pipeline.wb_uniforms,
                        0,
                        bytemuck::bytes_of(&uniforms),
                    );
                    let bind = resources
                        .device
                        .create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("lumina-gpu-localmask-wb-bind"),
                            layout: &pipeline.wb_layout,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: pipeline.wb_uniforms.as_entire_binding(),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: wgpu::BindingResource::TextureView(&current_view),
                                },
                            ],
                        });
                    gpu_util::encode_fullscreen_pass(
                        &mut encoder,
                        &pipeline.wb,
                        &bind,
                        &adjusted_view,
                    );
                } else {
                    self.encode_local_tone(
                        resources,
                        &mut encoder,
                        &layer.adjustments,
                        &current_view,
                        &adjusted_view,
                        sampler,
                    )?;
                }
                let next = shaders::create_output_texture(
                    &resources.device,
                    width,
                    height,
                    "lumina-gpu-localmask-next",
                );
                let next_view = next.create_view(&wgpu::TextureViewDescriptor::default());
                let bind = resources
                    .device
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("lumina-gpu-localmask-blend-bind"),
                        layout: &pipeline.blend_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&current_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::TextureView(&adjusted_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: wgpu::BindingResource::TextureView(&mask_view),
                            },
                        ],
                    });
                gpu_util::encode_fullscreen_pass(&mut encoder, &pipeline.blend, &bind, &next_view);
                resources.queue.submit(Some(encoder.finish()));
                next
            };
            current = next;
        }

        Ok(current)
    }

    /// Apply one delta-free P0 layer through the already parity-tested global
    /// tone pipeline with the layer's `as_recipe()` — the exact path the CPU
    /// oracle's `apply_mask_local_recipe` takes for such a layer.
    fn encode_local_tone(
        &self,
        resources: &GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        adjustments: &LocalAdjustments,
        input_view: &wgpu::TextureView,
        target_view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> Result<(), GpuError> {
        let guard = self.pipeline.lock().unwrap();
        let Some(tone) = guard.as_ref() else {
            return Err(GpuError::RenderFailed(
                "local mask pass: tone pipeline not built".into(),
            ));
        };
        let uniforms = shaders::Uniforms::from_recipe(&adjustments.as_recipe());
        shaders::write_uniforms(&resources.queue, &tone.uniform_buffer, &uniforms);
        let bind = shaders::create_color_tone_bind_group(
            &resources.device,
            &tone.bind_group_layout,
            &tone.uniform_buffer,
            input_view,
            sampler,
        );
        gpu_util::encode_fullscreen_pass(encoder, &tone.pipeline, &bind, target_view);
        Ok(())
    }
}
