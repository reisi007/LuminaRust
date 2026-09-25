//! MASK-LOCAL-P1.1 relative white-balance kernel.
//!
//! The local recipe is applied to the already global/geometry-processed
//! frame. Relative gains stay in `f64` until the single RGBA8 write, so a
//! local Basic control cannot observe an intermediate quantized WB value.
//!
//! This lives beside `local_adjustments` (the P0 compositor) because it is the
//! same per-layer stage: the relative-WB pass runs inside the local compositing
//! and shares its validation and quantization contract.

use crate::{for_each_rgba_mut, CoreError, ImageFrame};

impl ImageFrame {
    /// Apply one typed mask-local recipe to the current post-global frame.
    ///
    /// The local white balance is a relative delta, not a second absolute
    /// global WB recipe. When a delta is present, the WB gains and the four
    /// P0 Basic controls are evaluated in `f64` and rounded to RGBA8 only at
    /// the end of the local pass. With a neutral delta this delegates to the
    /// established P0 kernel so existing local-adjustment bytes remain
    /// unchanged. Alpha is never touched.
    pub fn apply_mask_local_recipe(
        &mut self,
        recipe: &lumina_sidecar::MaskLocalRecipe,
    ) -> Result<(), CoreError> {
        recipe
            .validate()
            .map_err(|error| CoreError::InvalidLocalAdjustment {
                reason: error.to_string(),
            })?;
        if recipe.temperature_delta_k == 0.0 && recipe.tint_delta == 0.0 {
            return self.apply_recipe(&recipe.as_recipe());
        }
        apply_mask_local_wb_and_basic(&mut self.pixels, recipe);
        Ok(())
    }
}

/// Apply a local relative-WB delta and the P0 Basic controls with one final
/// quantization boundary. The global kernel intentionally rounds between
/// stages for backwards-compatible absolute-WB bytes; the local path instead
/// keeps the relative gains and scalar math in f64, clamps without rounding
/// between stages, and rounds once when writing the destination byte.
fn apply_mask_local_wb_and_basic(pixels: &mut [u8], recipe: &lumina_sidecar::MaskLocalRecipe) {
    let gains = recipe.relative_white_balance_gains();
    let exposure_multiplier = 2.0_f64.powf(recipe.exposure);
    let contrast_factor = 1.0 + recipe.contrast;
    for_each_rgba_mut(pixels, |pixel| {
        for channel in 0..3 {
            let mut value = f64::from(pixel[channel]) * gains[channel];
            value = (value * exposure_multiplier).clamp(0.0, 255.0);
            value = ((value - 128.0) * contrast_factor + 128.0).clamp(0.0, 255.0);
            let x = value / 255.0;
            let shadow_weight = ((0.5 - x) / 0.5).max(0.0).powi(2);
            value = (x + recipe.shadows * shadow_weight * 0.25).clamp(0.0, 1.0) * 255.0;
            let x = value / 255.0;
            let highlight_weight = ((x - 0.5) / 0.5).max(0.0).powi(2);
            value = (x + recipe.highlights * highlight_weight * 0.25).clamp(0.0, 1.0) * 255.0;
            pixel[channel] = value.round().clamp(0.0, 255.0) as u8;
        }
        // Deliberately leave pixel[3] (alpha) unchanged.
    });
}
