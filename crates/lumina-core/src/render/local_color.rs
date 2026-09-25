//! MASK-LOCAL-P1.2b mask-local per-pixel colour kernel.
//!
//! The local colour block reuses the *global* colour types, the *global*
//! validators and the *global* per-pixel stage functions from
//! [`crate::color_stages`], so a local colour stage is the same function at a
//! different place in the chain. Its position inside a layer is
//!
//! ```text
//! global result
//!   → local relative WB              (P1.1, `local_wb`)
//!   → local Basic                    (P1.1, `local_wb`)
//!   → local tone curve               (P1.2a, `local_tone`)
//!   → local HSL                      (P1.2b, this module)
//!   → local Point Color              (P1.2b, this module)
//!   → local Vibrance / Saturation    (P1.2b, this module)
//!   → local Color Grading            (P1.2b, this module)
//!   → fractional mask blend          (P0, `local_adjustments`)
//! ```
//!
//! That order is exactly the global colour order
//! (`HSL → Point Color → Vibrance/Saturation → Color Grading`) evaluated at
//! exactly the global colour position (after the global tone curve, before the
//! global colour stage's neighbours) — see
//! `ImageFrame::apply_recipe_with_scale_white_balance_and_denoise`.
//!
//! The whole chain is evaluated in `f64` and quantized exactly once, at the
//! end. The global kernel rounds between its stages because it operates on a
//! `u8` frame; the local chain owns a single quantization boundary instead, so
//! no colour stage ever observes a rounded intermediate. The colour stages
//! themselves are the global `f32` kernels on normalized values — the
//! conversion `f64 (0..=255)` → `f32 (0..=1)` → stage → `f64 (0..=255)` is the
//! only place where the two numeric domains meet, and it happens once, before
//! the single final round.

use super::local_tone::apply_local_tone;
use super::local_wb::{round_local_channel, scale_mask_local_wb_basic};
use crate::color_stages::{
    color_grading_stage, hsl_stage, point_color_stage, vibrance_saturation_stage,
};
use lumina_sidecar::MaskLocalRecipe;

/// Apply the whole local recipe (relative WB, Basic, tone curve and the four
/// colour stages) with a single final RGBA8 quantization.
pub(super) fn apply_mask_local_wb_basic_tone_color(pixels: &mut [u8], recipe: &MaskLocalRecipe) {
    debug_assert!(recipe.has_local_color());
    let gains = recipe.relative_white_balance_gains();
    let curves = recipe.curves.as_ref().filter(|_| recipe.has_local_curves());
    let hsl = recipe.hsl.as_ref().filter(|_| recipe.has_local_hsl());
    let point_color = recipe
        .point_color
        .as_ref()
        .filter(|_| recipe.has_local_point_color());
    let grading = recipe
        .color_grading
        .as_ref()
        .filter(|_| recipe.has_local_color_grading());
    let vibrance = recipe.vibrance as f32;
    let saturation = recipe.saturation as f32;
    let has_vibrance_saturation = vibrance != 0.0 || saturation != 0.0;
    for pixel in pixels.as_chunks_mut::<4>().0 {
        // Stage 1+2: relative WB then Basic, in float, per channel.
        let mut scaled = [0.0_f64; 3];
        for channel in 0..3 {
            scaled[channel] =
                scale_mask_local_wb_basic(f64::from(pixel[channel]), &gains, recipe, channel);
        }
        // Stage 3: the local tone curve on the *float* intermediate. Skipped
        // entirely when the layer stores no curve, so a colour-only layer never
        // pays for a curve it does not have.
        let toned = curves.map_or(scaled, |curves| apply_local_tone(&scaled, curves));
        // Stages 4..7: the local colour block, in the global colour order, on
        // the same un-quantized float chain.
        let mut rgb = [
            toned[0] as f32 / 255.0,
            toned[1] as f32 / 255.0,
            toned[2] as f32 / 255.0,
        ];
        if let Some(hsl) = hsl {
            if let Some(staged) = hsl_stage(rgb, hsl) {
                rgb = staged;
            }
        }
        if let Some(point_color) = point_color {
            if let Some(staged) = point_color_stage(rgb, point_color) {
                rgb = staged;
            }
        }
        if has_vibrance_saturation {
            rgb = vibrance_saturation_stage(rgb, vibrance, saturation);
        }
        if let Some(grading) = grading {
            rgb = color_grading_stage(rgb, grading);
        }
        // The one and only quantization boundary of this layer.
        for channel in 0..3 {
            pixel[channel] = round_local_channel(f64::from(rgb[channel]) * 255.0);
        }
        // Deliberately leave pixel[3] (alpha) unchanged.
    }
}
