//! GPU-RENDER-MASK-19: the GPU local-mask pixel pass.
//!
//! The CPU oracle (`lumina-core::render::local_adjustments`) applies each
//! persisted, visible, non-neutral mask layer to the post-global/geometry frame
//! in persisted order: it evaluates the layer's local recipe into an *adjusted*
//! frame and then blends that adjusted frame back through the evaluated mask
//! plane (`blend_local_layer`). This module renders **that same pass on the
//! GPU**, so a recipe with local mask adjustments no longer falls back to the
//! CPU (User-Entscheid 2026-09-12: volle GPU-Parität).
//!
//! ## Scope of this slice (honest, loud, no silent exception)
//!
//! The pass is complete for the local recipe blocks the GPU already renders:
//!
//! * the **relative WB delta** (`temperature_delta_k` / `tint_delta`) plus the
//!   four **P0 Basic** controls (`exposure`/`contrast`/`shadows`/`highlights`)
//!   run in `LOCAL_MASK_WB_SRC` (see `local_mask_gpu.rs`), mirroring
//!   `local_wb::scale_mask_local_wb_basic` with its single quantization; a
//!   delta-free P0 layer reuses the already parity-tested global tone pipeline
//!   with the layer's `as_recipe()` (the CPU oracle takes exactly that path for
//!   it);
//! * the **mask blend** runs in `LOCAL_MASK_BLEND_SRC` (see `local_mask_gpu.rs`),
//!   a literal transcription of the oracle's `blend_local_layer`.
//!
//! The P1.2 blocks (local tone curve, colour, presence, detail) are **not**
//! GPU-wired yet. A bound layer that carries one is *not* silently dropped and
//! *not* silently CPU-composited: [`unsupported_local_mask_stages`] names it as
//! a CPU-routing reason (the caller keeps the exact CPU present path), and
//! [`GpuContext::render_with_gpu_local_masks`] refuses it loudly. That is a
//! named, visible gap — the same contract shape as `render_to_vram`'s refusal
//! for un-renderable geometry — not a silent fallback.
//!
//! ## Validation
//!
//! [`validate_local_mask_layers`] is the loud entry check (a mismatched or
//! invalid layer is refused before any pixel is touched). It lives here rather
//! than in `validate.rs` because the file-size ratchet freezes that file at its
//! baseline (DoD §8); the *contract* is the same: no partial mutation, no silent
//! crop/resample.

use crate::{Frame, GpuContext, GpuError};
use lumina_core::masks::MaskPlane;
use lumina_sidecar::LocalAdjustments;

#[cfg(feature = "gpu")]
use crate::local_mask_gpu::{build_local_mask_pipeline, WbUniforms};
#[cfg(feature = "gpu")]
use crate::{gpu_util, shaders, GpuResources};

/// One caller-evaluated mask layer bound for the GPU local-mask pass.
///
/// Mask *evaluation* (mask graph, modulation, output-space alignment) stays the
/// caller's responsibility — exactly like a Lensfun map or a depth plane. The
/// caller supplies the already frame-sized `R16Uint` alpha plane and the
/// persisted local recipe, in the persisted layer order.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalMaskLayer {
    /// The persisted layer id (diagnostics and refusal messages).
    pub layer_id: String,
    /// The evaluated, frame-sized mask plane (`u16` alpha per pixel).
    pub plane: MaskPlane,
    /// The layer's typed local recipe.
    pub adjustments: LocalAdjustments,
}

impl LocalMaskLayer {
    /// A layer whose recipe cannot change a pixel is skipped by both backends.
    #[must_use]
    pub fn is_neutral(&self) -> bool {
        self.adjustments.is_neutral()
    }
}

/// The P1.2 CPU-routing reason for one persisted local recipe, or `None`.
///
/// The plane-free half of [`unsupported_local_mask_stages`]: a routing gate can
/// decide *before* any mask plane exists, i.e. without touching the GPU.
#[must_use]
pub fn unsupported_local_mask_adjustments(
    layer_id: &str,
    adjustments: &LocalAdjustments,
) -> Option<String> {
    if adjustments.has_local_curves()
        || adjustments.has_local_color()
        || adjustments.has_local_presence()
        || adjustments.has_local_detail()
    {
        Some(format!(
            "local mask layer `{layer_id}` uses a P1.2 block (tone curve/color/presence/detail) \
             not GPU-wired"
        ))
    } else {
        None
    }
}

/// The GPU-routing reasons for a set of bound local mask layers.
///
/// Empty when every non-neutral layer is GPU-renderable. A non-empty result is
/// the caller's cue to keep the exact CPU present path: the GPU local-mask pass
/// would otherwise drop a stage. This is the local-mask-context companion of
/// [`crate::unsupported_gpu_stages_with_context`] — the mask-layer context the
/// recipe-only gate cannot see. The plane-free half is
/// [`unsupported_local_mask_adjustments`]; this function only adds the
/// neutrality skip.
#[must_use]
pub fn unsupported_local_mask_stages(layers: &[LocalMaskLayer]) -> Vec<String> {
    let mut reasons = Vec::new();
    for layer in layers {
        if layer.is_neutral() {
            continue;
        }
        if let Some(reason) =
            unsupported_local_mask_adjustments(&layer.layer_id, &layer.adjustments)
        {
            reasons.push(reason);
        }
    }
    reasons
}

/// Loud validation of bound local mask layers against the output frame.
///
/// Every layer's recipe is schema-validated with the sidecar's own validator,
/// and its plane must match the output dimensions exactly (no silent resample or
/// crop — the oracle refuses a mismatched plane too). Called before any pixel is
/// touched, so a rejected bind leaves the target untouched.
pub fn validate_local_mask_layers(
    width: u32,
    height: u32,
    layers: &[LocalMaskLayer],
) -> Result<(), GpuError> {
    for layer in layers {
        layer.adjustments.validate().map_err(|error| {
            GpuError::RenderFailed(format!(
                "local mask layer `{}` has an invalid recipe: {error}",
                layer.layer_id
            ))
        })?;
        if layer.plane.width != width || layer.plane.height != height {
            return Err(GpuError::RenderFailed(format!(
                "local mask layer `{}` plane {}x{} does not match the output {width}x{height}",
                layer.layer_id, layer.plane.width, layer.plane.height
            )));
        }
        if layer.plane.values.len() != (width as usize) * (height as usize) {
            return Err(GpuError::RenderFailed(format!(
                "local mask layer `{}` plane carries {} values, expected {}x{}",
                layer.layer_id,
                layer.plane.values.len(),
                width,
                height
            )));
        }
    }
    Ok(())
}

/// Whether a layer takes the relative-WB float chain rather than the delta-free
/// global tone path (`local_wb::apply_mask_local_recipe_with_scale`).
#[cfg(feature = "gpu")]
fn has_wb_delta(adjustments: &LocalAdjustments) -> bool {
    adjustments.temperature_delta_k != 0.0 || adjustments.tint_delta != 0.0
}

impl GpuContext {
    /// Render `frame` with `recipe` and apply the caller-evaluated local mask
    /// layers **on the GPU**, in persisted order.
    ///
    /// The global/geometry chain runs through [`GpuContext::render_with_gpu`]
    /// (so every existing stage and its loud CPU-routing contract is unchanged);
    /// this method then delegates to [`GpuContext::apply_gpu_local_masks`] on
    /// that result. Layers whose recipe is neutral are skipped; a non-neutral
    /// layer whose recipe carries a P1.2 block is a **loud** refusal (see
    /// [`unsupported_local_mask_stages`]) rather than a silent drop, and the
    /// caller keeps the exact CPU present path.
    ///
    /// Without a bound adapter the pass cannot run: the method returns
    /// [`GpuError::AdapterUnavailable`] for a non-neutral layer set instead of
    /// silently returning the un-modulated global frame (an empty/all-neutral
    /// set is a pure [`GpuContext::render_with_gpu`] and keeps its CPU
    /// fallback).
    pub fn render_with_gpu_local_masks(
        &self,
        frame: &lumina_core::ImageFrame,
        recipe: &lumina_sidecar::EditRecipe,
        layers: &[LocalMaskLayer],
    ) -> Result<Frame, GpuError> {
        let global = self.render_with_gpu(frame, recipe)?;
        self.apply_gpu_local_masks(&global, layers)
    }

    /// Apply the caller-evaluated local mask layers to an **already globally
    /// rendered** frame (GPU-RENDER-MASK-19).
    ///
    /// `global` is the output of [`GpuContext::render_with_gpu`] or
    /// [`GpuContext::render_with_gpu_and_generative`] for the same recipe — the
    /// exact post-global/post-geometry stage the CPU oracle composites local
    /// layers onto. Returns `global` unchanged when every layer is neutral.
    /// Validates loudly (no silent drop) and requires a bound adapter for a
    /// non-neutral set.
    pub fn apply_gpu_local_masks(
        &self,
        global: &Frame,
        layers: &[LocalMaskLayer],
    ) -> Result<Frame, GpuError> {
        if layers.iter().all(LocalMaskLayer::is_neutral) {
            return Ok(global.clone());
        }
        validate_local_mask_layers(global.width, global.height, layers)?;
        let unsupported = unsupported_local_mask_stages(layers);
        if !unsupported.is_empty() {
            return Err(GpuError::RenderFailed(format!(
                "GPU local-mask pass refuses GPU-unsupported local stage(s): {}; \
                 render the layer through the full CPU reference instead",
                unsupported.join("; ")
            )));
        }
        #[cfg(feature = "gpu")]
        {
            let Some(resources) = self.resources.as_ref() else {
                return Err(GpuError::AdapterUnavailable(
                    "GPU local-mask pass requires a bound adapter".into(),
                ));
            };
            self.run_local_mask_chain(resources, global, layers)
        }
        #[cfg(not(feature = "gpu"))]
        {
            Err(GpuError::AdapterUnavailable(
                "GPU local-mask pass requires a bound adapter".into(),
            ))
        }
    }
}

#[cfg(feature = "gpu")]
impl GpuContext {
    /// Run the mask-local chain (recipe → blend per layer) over the global
    /// result and read the final pixels back.
    fn run_local_mask_chain(
        &self,
        resources: &GpuResources,
        global: &Frame,
        layers: &[LocalMaskLayer],
    ) -> Result<Frame, GpuError> {
        let (width, height) = (global.width, global.height);
        let pipeline = build_local_mask_pipeline(&resources.device);
        let sampler = shaders::create_sampler(&resources.device, "lumina-gpu-localmask-sampler");

        let mut current = shaders::create_output_texture(
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
                        &sampler,
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

        gpu_util::readback_texture(resources, &current, width, height)
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

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_sidecar::CurvePoint;

    fn layer_with(adjustments: LocalAdjustments) -> LocalMaskLayer {
        LocalMaskLayer {
            layer_id: "layer-1".into(),
            plane: MaskPlane::new(2, 2, vec![u16::MAX; 4]).unwrap(),
            adjustments,
        }
    }

    /// A neutral recipe is never a route reason — plane-free or bound.
    #[test]
    fn neutral_adjustments_have_no_plane_free_route_reason() {
        let adjustments = LocalAdjustments::default();
        assert_eq!(
            unsupported_local_mask_adjustments("layer-1", &adjustments),
            None
        );
        assert!(unsupported_local_mask_stages(&[layer_with(adjustments)]).is_empty());
    }

    /// Every P1.2 block reports through the plane-free predicate, and the bound
    /// helper is exactly that predicate plus the neutrality skip — same message.
    #[test]
    fn each_p12_block_is_the_same_plane_free_and_bound_reason() {
        let curve = LocalAdjustments {
            curves: Some(lumina_sidecar::Curves {
                version: 1,
                master: vec![
                    CurvePoint {
                        input: 0.0,
                        output: 0.0,
                    },
                    CurvePoint {
                        input: 1.0,
                        output: 0.5,
                    },
                ],
                channels: lumina_sidecar::CurveChannels {
                    red: None,
                    green: None,
                    blue: None,
                },
            }),
            ..LocalAdjustments::default()
        };
        let color = LocalAdjustments {
            vibrance: 0.5,
            ..LocalAdjustments::default()
        };
        let mut presence = LocalAdjustments::default();
        presence
            .set_local_presence_field("texture", 0.5)
            .expect("valid local presence");
        let mut detail = LocalAdjustments::default();
        detail
            .set_local_sharpening_field("amount", 0.5)
            .expect("valid local sharpening");

        for adjustments in [curve, color, presence, detail] {
            let plane_free = unsupported_local_mask_adjustments("layer-1", &adjustments)
                .expect("a P1.2 block must be a plane-free route reason");
            assert_eq!(
                plane_free,
                "local mask layer `layer-1` uses a P1.2 block \
                 (tone curve/color/presence/detail) not GPU-wired"
            );
            let bound = unsupported_local_mask_stages(&[layer_with(adjustments)]);
            assert_eq!(bound, vec![plane_free]);
        }
    }
}
