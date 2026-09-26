//! MASK-LOCAL-P1.1 relative white-balance kernel.
//!
//! The local recipe is applied to the already global/geometry-processed
//! frame. Relative gains stay in `f64` until the single RGBA8 write, so a
//! local Basic control cannot observe an intermediate quantized WB value.
//!
//! This lives beside `local_adjustments` (the P0 compositor) because it is the
//! same per-layer stage: the relative-WB pass runs inside the local compositing
//! and shares its validation and quantization contract. The P1.2a tone stage
//! extends the very same float chain in `local_tone`, and the P1.2c presence
//! stage inserts the shared global presence mathematics between the Basic
//! controls and the curve in `local_presence`.

use crate::{for_each_rgba_mut, CoreError, ImageFrame};

impl ImageFrame {
    /// Apply one typed mask-local recipe to the current post-global frame.
    ///
    /// The local white balance is a relative delta, not a second absolute
    /// global WB recipe, and the local tone curve (MASK-LOCAL-P1.2a), the
    /// local colour block (MASK-LOCAL-P1.2b) and the local presence block
    /// (MASK-LOCAL-P1.2c) are separate blocks that run after the local Basic
    /// controls — mirroring the global kernel, where presence follows the
    /// scalar stage, the curve follows presence, and the colour stages follow
    /// the curve.
    ///
    /// The kernel path is selected by *what the layer actually contains*, so
    /// every previously pinned P0/P1.1/P1.2a/P1.2b byte is preserved:
    ///
    /// * no relative delta and no local curve → the established P0
    ///   `apply_recipe` delegation (global fused LUT, byte-identical),
    /// * a relative delta but no local curve and no local colour → the P1.1
    ///   float chain (WB → Basic, one quantization),
    /// * a local curve but no local colour → the P1.2a float chain
    ///   (WB → Basic → tone curve, one quantization), whether or not a delta
    ///   is present,
    /// * any local colour block but no local presence → the P1.2b float chain
    ///   (WB → Basic → tone curve → HSL → Point Color → Vibrance/Saturation →
    ///   Color Grading, one quantization),
    /// * any local presence block → the P1.2c float chain
    ///   (WB → Basic → presence → tone curve → … → Color Grading, one
    ///   quantization), with or without a colour block.
    ///
    /// A layer whose presence block is absent **or** persisted all-zero reads
    /// as "no presence" and keeps the P1.2b path exactly.
    ///
    /// Alpha is never touched.
    pub fn apply_mask_local_recipe(
        &mut self,
        recipe: &lumina_sidecar::MaskLocalRecipe,
    ) -> Result<(), CoreError> {
        recipe
            .validate()
            .map_err(|error| CoreError::InvalidLocalAdjustment {
                reason: error.to_string(),
            })?;
        let has_wb_delta = recipe.temperature_delta_k != 0.0 || recipe.tint_delta != 0.0;
        let has_curves = recipe.has_local_curves();
        let has_color = recipe.has_local_color();
        let has_presence = recipe.has_local_presence();
        if !has_wb_delta && !has_curves && !has_color && !has_presence {
            return self.apply_recipe(&recipe.as_recipe());
        }
        if has_presence {
            super::local_presence::apply_mask_local_wb_basic_presence_tone_color(
                &mut self.pixels,
                self.width,
                self.height,
                recipe,
            );
            return Ok(());
        }
        if has_color {
            super::local_color::apply_mask_local_wb_basic_tone_color(&mut self.pixels, recipe);
            return Ok(());
        }
        if !has_curves {
            apply_mask_local_wb_and_basic(&mut self.pixels, recipe);
            return Ok(());
        }
        super::local_tone::apply_mask_local_wb_basic_and_tone(&mut self.pixels, recipe);
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
    for_each_rgba_mut(pixels, |pixel| {
        for (channel, value) in pixel.iter_mut().enumerate().take(3) {
            *value = scale_and_round_local_channel(f64::from(*value), &gains, recipe, channel);
        }
        // Deliberately leave pixel[3] (alpha) unchanged.
    });
}

/// The relative-WB gain, then the four P0 Basic controls, evaluated in `f64`
/// on one channel. This is the shared prefix of the P1.1 and P1.2a local
/// kernels: both start from exactly this value, and it is never quantized, so
/// the tone stage can continue from the same float the WB/Basic stage saw.
pub(super) fn scale_mask_local_wb_basic(
    value: f64,
    gains: &[f64; 3],
    recipe: &lumina_sidecar::MaskLocalRecipe,
    channel: usize,
) -> f64 {
    let exposure_multiplier = 2.0_f64.powf(recipe.exposure);
    let contrast_factor = 1.0 + recipe.contrast;
    let mut value = value * gains[channel];
    value = (value * exposure_multiplier).clamp(0.0, 255.0);
    value = ((value - 128.0) * contrast_factor + 128.0).clamp(0.0, 255.0);
    let x = value / 255.0;
    let shadow_weight = ((0.5 - x) / 0.5).max(0.0).powi(2);
    value = (x + recipe.shadows * shadow_weight * 0.25).clamp(0.0, 1.0) * 255.0;
    let x = value / 255.0;
    let highlight_weight = ((x - 0.5) / 0.5).max(0.0).powi(2);
    (x + recipe.highlights * highlight_weight * 0.25).clamp(0.0, 1.0) * 255.0
}

/// The one and only local quantization boundary: round and clamp into `u8`.
pub(super) fn round_local_channel(value: f64) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

/// [`scale_mask_local_wb_basic`] followed by [`round_local_channel`].
pub(super) fn scale_and_round_local_channel(
    value: f64,
    gains: &[f64; 3],
    recipe: &lumina_sidecar::MaskLocalRecipe,
    channel: usize,
) -> u8 {
    round_local_channel(scale_mask_local_wb_basic(value, gains, recipe, channel))
}
