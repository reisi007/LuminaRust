//! Local-mask CPU-route refusal for the GPU present path.

use super::super::LuminaApp;

impl LuminaApp {
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
