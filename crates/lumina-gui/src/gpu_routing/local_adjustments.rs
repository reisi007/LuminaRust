//! Local-mask pairing and unsupported-stage refusal for GPU present.

use super::super::LuminaApp;

#[cfg(feature = "gpu")]
pub(crate) enum LiveVramPresent<E> {
    Presented,
    Refused(String),
    Failed(E),
}

impl LuminaApp {
    /// Production route seam: no GPU closure runs unless the successful CPU
    /// render and all caller preflights agree; freshness is committed only after
    /// global render and local composition both complete.
    #[cfg(feature = "gpu")]
    pub(crate) fn run_live_gpu_present<E>(
        vram_fresh: &mut bool,
        cpu_render_succeeded: bool,
        refusal: Option<String>,
        layers: &[lumina_gpu::local_mask_pass::LocalMaskLayer],
        render_global: impl FnOnce() -> Result<(), E>,
        compose: impl FnOnce(&[lumina_gpu::local_mask_pass::LocalMaskLayer]) -> Result<(), E>,
    ) -> LiveVramPresent<E> {
        *vram_fresh = false;
        if !cpu_render_succeeded {
            return LiveVramPresent::Refused("current CPU reference render failed".into());
        }
        if let Some(reason) = refusal {
            return LiveVramPresent::Refused(reason);
        }
        if let Err(error) = render_global() {
            return LiveVramPresent::Failed(error);
        }
        if !layers.is_empty() {
            if let Err(error) = compose(layers) {
                return LiveVramPresent::Failed(error);
            }
        }
        *vram_fresh = true;
        LiveVramPresent::Presented
    }

    /// Local-mask VRAM composition is valid only when the current CPU preview,
    /// evaluated planes and full source describe the same uncapped full frame.
    /// Draft-size and ROI previews are refused rather than paired with full-frame
    /// planes; the CPU preview remains authoritative until a matching contract exists.
    #[cfg(feature = "gpu")]
    pub(crate) fn local_mask_vram_geometry_refusal(
        &self,
        source_width: u32,
        source_height: u32,
    ) -> Option<String> {
        if !self.has_visible_local_adjustments() {
            return None;
        }
        let Some(preview) = self.preview.as_ref() else {
            return Some(
                "local-mask VRAM present refused: current CPU preview is unavailable".into(),
            );
        };
        if self.preview_is_draft
            || self.preview_roi.is_some()
            || preview.width() != source_width
            || preview.height() != source_height
        {
            return Some(format!(
                "local-mask VRAM present refused: CPU preview geometry {}x{} (draft={}, ROI={:?}) does not match full source {source_width}x{source_height}; keeping CPU preview",
                preview.width(),
                preview.height(),
                self.preview_is_draft,
                self.preview_roi
            ));
        }
        None
    }

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
            let expected_values =
                (width as usize)
                    .checked_mul(height as usize)
                    .ok_or_else(|| {
                        format!("local mask layer `{}` plane dimensions overflow", layer.id)
                    })?;
            if result.plane.values.len() != expected_values {
                return Err(format!(
                    "local mask layer `{}` has malformed plane data: {} values for {width}x{height}, expected {expected_values}",
                    layer.id,
                    result.plane.values.len()
                ));
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

    /// GPU routing reasons for stages that the present renderer cannot execute.
    #[cfg(feature = "gpu")]
    pub(crate) fn gpu_unsupported_reasons(&self) -> Vec<String> {
        let recipe = self.gpu_present_recipe();
        lumina_gpu::unsupported_gpu_stages_with_context(
            recipe.as_ref(),
            false,
            self.camera_white_balance.as_ref(),
        )
    }
}
