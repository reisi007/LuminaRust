//! Interactive draft rendering path (currently backed by the CPU oracle).

use super::{render_cpu, shaders, Frame, GpuContext, GpuError, ImageFrame};

impl GpuContext {
    /// Draft render for an interactive viewport (ROI) using the draft pyramid.
    ///
    /// Sets up the GPU scaffolding (pipeline + uniforms + ROI tile set) and
    /// produces real pixels through the CPU reference (the draft path stays
    /// on the CPU scaffold even though the tone shader stage exists). The
    /// tile set is logged so the parallel tiling subagent has a concrete
    /// call site to plug into.
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
        let zoom = (frame.width() as f32 / viewport.width.max(1.0)).clamp(0.01, 100.0);
        let pyramid = crate::tiling::DraftPyramid::new(frame.width(), frame.height());
        let lvl = pyramid.level_for_zoom(zoom);
        // REVIEW-GPU-LEVELS-1: the ROI expansion routes through the pyramid, so
        // the logged level and the produced tile keys can no longer diverge.
        let cache = crate::tiling::TiledCache::new(64);
        let keys = cache.keys_for_viewport(&pyramid, &viewport, zoom);
        log::debug!(
            "render_draft: gpu scaffold (adapter present), {} tiles for viewport {:?} @ zoom {:.3} (pyramid level {}, cache generation {})",
            keys.len(),
            viewport,
            zoom,
            lvl,
            cache.generation()
        );
        // Real GPU tile upload + draw is filled in by the shader/tiling subagents.
        // Bootstrapping: produce real pixels via the CPU reference.
        self.render_draft_cpu(frame)
    }

    /// CPU fallback that uses the recipe stored via [`update_uniforms`], or the
    /// untouched frame when none has been set. Runs the full CPU reference
    /// chain (the free `render_cpu`), not just `apply_recipe`.
    fn render_draft_cpu(&self, frame: &ImageFrame) -> Result<Frame, GpuError> {
        match self.recipe.as_ref() {
            Some(recipe) => render_cpu(
                frame,
                recipe,
                self.depth_plane.as_ref().map(|plane| &plane.plane),
                self.camera_white_balance(),
            ),
            None => Ok(Frame::from_image_frame(frame.clone())),
        }
    }
}
