//! Local-mask CPU-route refusal for the GPU present path.

use super::super::LuminaApp;

impl LuminaApp {
    /// Pair current evaluated planes with persisted recipes before VRAM composition.
    #[cfg(feature = "gpu")]
    pub(crate) fn gpu_local_mask_layers(
        &self,
        width: u32,
        height: u32,
    ) -> Result<Vec<lumina_gpu::local_mask_pass::LocalMaskLayer>, String> {
        use lumina_gpu::local_mask_pass::{unsupported_local_mask_stages, LocalMaskLayer};

        if self.render_key.is_none() && self.has_visible_local_adjustments() {
            return Err("visible local masks have no current successful render".into());
        }
        let copy = self.document.as_ref().and_then(|document| {
            document
                .virtual_copies
                .iter()
                .find(|copy| copy.id == self.virtual_copy_id)
        });
        let Some(copy) = copy else {
            return if self.has_visible_local_adjustments() {
                Err("active mask document is unavailable".into())
            } else {
                Ok(Vec::new())
            };
        };
        let mut layers = Vec::new();
        for result in &self.render_mask_layers {
            let layer = copy
                .mask_layers
                .iter()
                .find(|layer| layer.id == result.layer_id)
                .ok_or_else(|| {
                    format!(
                        "evaluated mask layer `{}` has no active persisted layer",
                        result.layer_id
                    )
                })?;
            let Some(adjustments) = layer
                .effective_local_adjustments()
                .map_err(|error| format!("mask layer `{}`: {error}", layer.id))?
            else {
                continue;
            };
            if adjustments.is_neutral() {
                continue;
            }
            if result.plane.width != width || result.plane.height != height {
                return Err(format!("local mask layer `{}` plane {}x{} does not align with source VRAM {width}x{height}", layer.id, result.plane.width, result.plane.height));
            }
            layers.push(LocalMaskLayer {
                layer_id: result.layer_id.clone(),
                plane: result.plane.clone(),
                adjustments,
            });
        }
        let unsupported = unsupported_local_mask_stages(&layers);
        if !unsupported.is_empty() {
            return Err(unsupported.join("; "));
        }
        if self.has_visible_local_adjustments() && layers.is_empty() {
            return Err(
                "visible local masks have no evaluated planes from the current render".into(),
            );
        }
        Ok(layers)
    }

    /// Add the CPU-compositor limitation to the visible GPU routing reasons.
    #[cfg(feature = "gpu")]
    pub(crate) fn gpu_unsupported_reasons(&self) -> Vec<String> {
        let recipe = self.gpu_present_recipe();
        let mut reasons = lumina_gpu::unsupported_gpu_stages_with_context(
            recipe.as_ref(),
            false,
            self.camera_white_balance.as_ref(),
        );
        if let Some(reason) = self.local_adjustment_route_reason() {
            reasons.push(reason);
        }
        reasons
    }
}
