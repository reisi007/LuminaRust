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
use crate::local_mask_gpu::build_local_mask_pipeline;
#[cfg(feature = "gpu")]
use crate::shaders;
#[cfg(feature = "gpu")]
#[path = "local_mask_vram.rs"]
mod vram;

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

/// Validate the request before adapter lookup or command submission. The live
/// VRAM entry and adapterless tests share this production preflight seam.
fn preflight_vram_local_mask_request(
    layers: &[LocalMaskLayer],
    output_dimensions: Option<(u32, u32)>,
) -> Result<(), GpuError> {
    for layer in layers {
        layer.adjustments.validate().map_err(|error| {
            GpuError::RenderFailed(format!(
                "local mask layer `{}` has an invalid recipe: {error}",
                layer.layer_id
            ))
        })?;
        let Some(count) = (layer.plane.width as usize).checked_mul(layer.plane.height as usize)
        else {
            return Err(GpuError::RenderFailed(
                "local mask dimensions overflow".into(),
            ));
        };
        if layer.plane.width == 0 || layer.plane.height == 0 || layer.plane.values.len() != count {
            return Err(GpuError::RenderFailed(format!(
                "local mask layer `{}` has malformed plane dimensions {}x{} with {} values",
                layer.layer_id,
                layer.plane.width,
                layer.plane.height,
                layer.plane.values.len()
            )));
        }
        if let Some((width, height)) = output_dimensions {
            if layer.plane.width != width || layer.plane.height != height {
                return Err(GpuError::RenderFailed(format!(
                    "local mask layer `{}` plane {}x{} does not match the output {width}x{height}",
                    layer.layer_id, layer.plane.width, layer.plane.height
                )));
            }
        }
    }
    let unsupported = unsupported_local_mask_stages(layers);
    if !unsupported.is_empty() {
        return Err(GpuError::RenderFailed(format!(
            "GPU local-mask VRAM pass refuses GPU-unsupported local stage(s): {}; present the full CPU reference instead",
            unsupported.join("; ")
        )));
    }
    Ok(())
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

    /// Apply caller-evaluated layers directly to the active, globally rendered
    /// VRAM output. The resident presentation texture and overlay bind group stay
    /// stable; no full-frame readback is performed.
    pub fn apply_gpu_local_masks_to_vram(&self, layers: &[LocalMaskLayer]) -> Result<(), GpuError> {
        preflight_vram_local_mask_request(layers, None)?;
        if layers.iter().all(LocalMaskLayer::is_neutral) {
            return Ok(());
        }
        let unsupported = unsupported_local_mask_stages(layers);
        if !unsupported.is_empty() {
            return Err(GpuError::RenderFailed(format!(
                "GPU local-mask VRAM pass refuses GPU-unsupported local stage(s): {}; \
                 present the full CPU reference instead",
                unsupported.join("; ")
            )));
        }
        #[cfg(feature = "gpu")]
        {
            let Some(resources) = self.resources.as_ref() else {
                return Err(GpuError::AdapterUnavailable(
                    "GPU local-mask VRAM pass requires a bound adapter".into(),
                ));
            };
            if resources
                .device_lost
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return Err(GpuError::AdapterUnavailable("GPU device lost".into()));
            }

            let mut pool = self.vram.lock().unwrap();
            let Some(vram) = pool.active() else {
                return Err(GpuError::RenderFailed(
                    "GPU local-mask VRAM pass requires an active VRAM render".into(),
                ));
            };
            preflight_vram_local_mask_request(layers, Some((vram.width, vram.height)))?;

            let width = vram.width;
            let height = vram.height;
            let intermediate_scope = resources
                .device
                .push_error_scope(wgpu::ErrorFilter::Validation);
            let pipeline = build_local_mask_pipeline(&resources.device);
            let sampler =
                shaders::create_sampler(&resources.device, "lumina-gpu-localmask-sampler");
            let current = shaders::create_output_texture(
                &resources.device,
                width,
                height,
                "lumina-gpu-localmask-vram-current",
            );
            let mut copy_input =
                resources
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("lumina-gpu-localmask-vram-input"),
                    });
            copy_input.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &vram.output,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &current,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            resources.queue.submit(Some(copy_input.finish()));

            // All validation has completed. Intermediate passes only touch
            // temporary textures; the resident output is updated once the full
            // chain has encoded successfully.
            let final_texture = match self.run_local_mask_layers(
                resources, current, width, height, layers, &pipeline, &sampler,
            ) {
                Ok(texture) => texture,
                Err(error) => {
                    let _ = resources.device.poll(wgpu::PollType::wait_indefinitely());
                    let _ = pollster::block_on(intermediate_scope.pop());
                    return Err(error);
                }
            };
            // GPU validation is asynchronous. Wait for all temporary passes and
            // inspect the scope before submitting the copy into the stable
            // presentation texture; on validation failure, only temporaries
            // have been touched and the caller can keep the CPU reference.
            let _ = resources.device.poll(wgpu::PollType::wait_indefinitely());
            if let Some(error) = pollster::block_on(intermediate_scope.pop()) {
                return Err(GpuError::RenderFailed(format!(
                    "GPU local-mask VRAM intermediate pass failed: {error}"
                )));
            }
            if resources
                .device_lost
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return Err(GpuError::AdapterUnavailable("GPU device lost".into()));
            }
            let commit_scope = resources
                .device
                .push_error_scope(wgpu::ErrorFilter::Validation);
            let mut copy_output =
                resources
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("lumina-gpu-localmask-vram-output"),
                    });
            copy_output.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &final_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &vram.output,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            let commit_commands = copy_output.finish();
            let _ = resources.device.poll(wgpu::PollType::wait_indefinitely());
            if let Some(error) = pollster::block_on(commit_scope.pop()) {
                return Err(GpuError::RenderFailed(format!(
                    "GPU local-mask VRAM commit validation failed: {error}"
                )));
            }
            resources.queue.submit(Some(commit_commands));
            let _ = resources.device.poll(wgpu::PollType::wait_indefinitely());
            if resources
                .device_lost
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return Err(GpuError::AdapterUnavailable(
                    "GPU device lost during local-mask VRAM commit".into(),
                ));
            }
            Ok(())
        }
        #[cfg(not(feature = "gpu"))]
        {
            Err(GpuError::AdapterUnavailable(
                "GPU local-mask VRAM pass requires a bound adapter".into(),
            ))
        }
    }
}

#[cfg(test)]
#[path = "local_mask_pass_tests.rs"]
mod tests;
