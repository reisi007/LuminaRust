//! Caller-bound GPU render state: external depth plane, Lensfun map,
//! source-action artifacts and the uniform push, extracted from `lib.rs`
//! (file-size ratchet §8).

// Doc-link only: the moved artifact-setter doc names the routing verdict it
// referenced at the crate root, where it was in scope.
#[allow(unused_imports)]
use super::unsupported_gpu_stages_for;
use super::{
    lens_blur, lensfun, shaders, DepthPlaneGpu, EditRecipe, GpuContext, GpuError,
    SourceActionArtifact,
};

impl GpuContext {
    /// Bind a caller-supplied external depth plane for
    /// `recipe.lens_blur.depth_artifact` (G-05), or clear it with `None`.
    ///
    /// Validation mirrors `lumina_core::lens_blur::apply_lens_blur`: every value
    /// must be finite and in `0..=1`, otherwise the bind is rejected with the
    /// oracle's `InvalidAdjustment` and **no** state changes (no silent
    /// clamping). The plane must match the **post-geometry** frame dimensions,
    /// which are only known at render time, so a dimension mismatch is reported
    /// loudly by the render entry points.
    ///
    /// A recipe that references a depth artifact without a matching bound plane
    /// stays unrenderable on the GPU: the render entry point returns the CPU
    /// oracle's missing-artifact error instead of silently falling back to the
    /// focus-rect heuristic. Call [`Self::set_depth_plane`] with `None` to
    /// release the texture.
    pub fn set_depth_plane(
        &mut self,
        plane: Option<&lumina_core::DepthPlane>,
    ) -> Result<(), GpuError> {
        let Some(plane) = plane else {
            self.depth_plane = None;
            return Ok(());
        };
        for value in &plane.values {
            if !value.is_finite() || !(0.0..=1.0).contains(value) {
                return Err(GpuError::Core(lumina_core::CoreError::InvalidAdjustment {
                    name: "lens_blur.depth_plane.value".into(),
                    value: f64::from(*value),
                    minimum: 0.0,
                    maximum: 1.0,
                }));
            }
        }
        let (texture, view) = match self.resources.as_ref() {
            Some(resources) => {
                let texture = lens_blur::create_depth_texture(
                    &resources.device,
                    plane.width,
                    plane.height,
                    "lumina-gpu-depth-plane",
                );
                resources.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    bytemuck::cast_slice(&plane.values),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(plane.width * 4),
                        rows_per_image: Some(plane.height),
                    },
                    wgpu::Extent3d {
                        width: plane.width,
                        height: plane.height,
                        depth_or_array_layers: 1,
                    },
                );
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                (Some(texture), Some(view))
            }
            // No adapter: keep the values for the CPU fallback, no texture.
            None => (None, None),
        };
        self.depth_plane = Some(DepthPlaneGpu {
            plane: plane.clone(),
            texture,
            view,
        });
        Ok(())
    }

    /// Bind (or clear with `None`) the caller-owned Lensfun warp/gain map
    /// (GPU-LENSFUN-PARITY-1), built from a strict corrector via
    /// [`lumina_core::LensfunMap::from_corrector`].
    ///
    /// Mirroring [`Self::set_depth_plane`], the map is validated loudly on bind
    /// (dimensions, plane lengths, TCA pairing, finiteness): an invalid map
    /// returns the oracle's own [`lumina_core::CoreError`] and changes **no**
    /// state. The GPU textures are uploaded once here and reused by every render
    /// until the map is re-bound or cleared. A bound dimension-preserving map
    /// replaces the recipe's manual lens step; a dimension-changing chain or a
    /// distortion corrector without an explicit crop is refused loudly at the
    /// render entry (never divergent pixels).
    pub fn set_lensfun_map(
        &mut self,
        map: Option<&lumina_core::LensfunMap>,
    ) -> Result<(), GpuError> {
        self.lensfun_map = lensfun::bind_map(self.resources.as_ref(), map)?;
        Ok(())
    }

    /// Validate a bound external depth plane against the frame the oracle would
    /// blur. `out_width`/`out_height` are the planned post-geometry output
    /// dimensions the caller computed from the geometry plan (source dims when
    /// no geometry is active).
    pub(super) fn external_depth_view_for(
        &self,
        recipe: &EditRecipe,
        out_width: u32,
        out_height: u32,
    ) -> Result<Option<&wgpu::TextureView>, GpuError> {
        let Some(blur) = recipe
            .lens_blur
            .as_ref()
            .filter(|b| lens_blur::stage_active(b))
        else {
            return Ok(None);
        };
        if blur.depth_artifact.is_none() {
            return Ok(None);
        }
        let Some(depth) = self.depth_plane.as_ref() else {
            return Err(GpuError::Core(lumina_core::CoreError::InvalidAdjustment {
                name: "lens_blur.depth_artifact".into(),
                value: -1.0,
                minimum: 0.0,
                maximum: 0.0,
            }));
        };
        if depth.plane.width != out_width || depth.plane.height != out_height {
            return Err(GpuError::Core(lumina_core::CoreError::InvalidMaskPlane {
                width: depth.plane.width,
                height: depth.plane.height,
                length: depth.plane.values.len(),
            }));
        }
        Ok(depth.view.as_ref())
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
            if region.width != replacement.width() || region.height != replacement.height() {
                return Err(GpuError::RenderFailed(format!(
                    "source-action artifact {index}: region {}x{} does not match replacement {}x{}",
                    region.width,
                    region.height,
                    replacement.width(),
                    replacement.height()
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
        // R2-GPU-03: the region/replacement textures don't change between drags,
        // so upload them once here and cache them on the context. The render
        // path reuses the cached textures instead of re-creating + re-uploading
        // a full-size texture per render tick.
        if let Some(resources) = self.resources.as_ref() {
            let mut cached = Vec::with_capacity(artifacts.len());
            for (index, action) in artifacts.iter().enumerate() {
                let region_tex = shaders::create_region_texture(
                    &resources.device,
                    action.region.width,
                    action.region.height,
                    &format!("lumina-gpu-sa-region-{index}"),
                );
                shaders::write_u16_plane(
                    &resources.queue,
                    &region_tex,
                    action.region.width,
                    action.region.height,
                    &action.region.values,
                );
                let region_view = region_tex.create_view(&wgpu::TextureViewDescriptor::default());
                let repl_tex = shaders::create_input_texture(
                    &resources.device,
                    action.replacement.width(),
                    action.replacement.height(),
                    &format!("lumina-gpu-sa-repl-{index}"),
                );
                resources.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &repl_tex,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    action.replacement.pixels(),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(action.replacement.width() * 4),
                        rows_per_image: Some(action.replacement.height()),
                    },
                    wgpu::Extent3d {
                        width: action.replacement.width(),
                        height: action.replacement.height(),
                        depth_or_array_layers: 1,
                    },
                );
                let repl_view = repl_tex.create_view(&wgpu::TextureViewDescriptor::default());
                cached.push((region_tex, region_view, repl_tex, repl_view));
            }
            self.sa_textures = Some(cached);
        }
        self.source_actions = Some(artifacts.to_vec());
        Ok(())
    }

    /// Remove previously bound source-action artifacts. Subsequent renders of
    /// recipes with `source_actions` CPU-route again.
    pub fn clear_source_action_artifacts(&mut self) {
        self.source_actions = None;
        self.sa_textures = None;
    }

    /// The bound source-action artifacts that match `width`×`height`, or `None`
    /// when nothing is bound (or any artifact targets different geometry — in
    /// which case such renders must stay on the CPU route; enforced via
    /// [`unsupported_gpu_stages_for`] with `false`).
    pub(super) fn matching_source_actions(
        &self,
        width: u32,
        height: u32,
    ) -> Option<&[SourceActionArtifact]> {
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
}
