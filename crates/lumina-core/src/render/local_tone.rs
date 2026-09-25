//! MASK-LOCAL-P1.2a mask-local tone-curve kernel.
//!
//! The local curve reuses the *global* curve types and the *global* curve
//! evaluator (`crate::monotone_curve`), so a local curve is the same function
//! at a different place in the chain. Its position inside a layer is
//!
//! ```text
//! global result
//!   → local relative WB        (P1.1, `local_wb`)
//!   → local Basic              (P1.1, `local_wb`)
//!   → local tone curve         (P1.2a, this module)
//!   → local color              (P1.2b — deliberately not implemented)
//!   → fractional mask blend    (P0, `local_adjustments`)
//! ```
//!
//! The whole chain is evaluated in `f64` and quantized exactly once, at the
//! end. The global kernel rounds between its stages because it operates on an
//! `u8` frame; the local kernel owns a single quantization boundary instead, so
//! the tone stage never observes a rounded intermediate.

use super::local_wb::{round_local_channel, scale_mask_local_wb_basic};
use crate::monotone_curve;
use lumina_sidecar::{CurvePoint, Curves};

/// Apply local relative WB, the P0 Basic controls and the local tone curve
/// with a single final RGBA8 quantization.
pub(super) fn apply_mask_local_wb_basic_and_tone(
    pixels: &mut [u8],
    recipe: &lumina_sidecar::MaskLocalRecipe,
) {
    // The caller only selects this kernel when the layer actually carries a
    // non-identity curve, so the block is always present here; the `else`
    // branch is a defensive no-op, never a silent partial render.
    debug_assert!(recipe.has_local_curves());
    let Some(curves) = recipe.curves.as_ref() else {
        return;
    };
    let gains = recipe.relative_white_balance_gains();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        // Stage 1+2: relative WB then Basic, in float, per channel.
        let mut scaled = [0.0_f64; 3];
        for channel in 0..3 {
            scaled[channel] =
                scale_mask_local_wb_basic(f64::from(pixel[channel]), &gains, recipe, channel);
        }
        // Stage 3: the local tone curve on the *float* intermediate.
        let toned = apply_local_tone(&scaled, curves);
        for channel in 0..3 {
            pixel[channel] = round_local_channel(toned[channel]);
        }
        // Deliberately leave pixel[3] (alpha) unchanged.
    }
}

/// Apply the local tone curve to one pixel's un-quantized channel values.
///
/// This mirrors the global curve stage in
/// [`crate::ImageFrame::apply_recipe_with_scale_white_balance_and_denoise`]
/// exactly: the Rec.709 luminance of the *current* values feeds the master
/// (PCHIP) curve, the per-channel curve is evaluated on the channel itself,
/// and the master is applied as `value * master / luminance` with the
/// documented `luminance > 1e-9` guard. Only the input differs — the global
/// stage reads rounded `u8` values, the local stage reads the float chain
/// result.
fn apply_local_tone(scaled: &[f64; 3], curves: &Curves) -> [f64; 3] {
    let original = [scaled[0] / 255.0, scaled[1] / 255.0, scaled[2] / 255.0];
    let luminance = 0.2126 * original[0] + 0.7152 * original[1] + 0.0722 * original[2];
    let master = f64::from(monotone_curve(&curves.master, luminance as f32));
    let channels = [
        curves.channels.red.as_deref(),
        curves.channels.green.as_deref(),
        curves.channels.blue.as_deref(),
    ];
    let mut toned = [0.0_f64; 3];
    for (channel, points) in channels.iter().enumerate() {
        let value = points.map_or(original[channel], |points| {
            f64::from(sample_channel_curve(points, original[channel]))
        });
        toned[channel] = if luminance > 1e-9 {
            value * master / luminance
        } else {
            master
        }
        .clamp(0.0, 1.0)
            * 255.0;
    }
    toned
}

/// Sample one channel curve, reusing the global PCHIP evaluator. The value is
/// not clamped here: the global kernel clamps the *composed* result, and doing
/// it in the same place keeps the two stages numerically identical.
fn sample_channel_curve(points: &[CurvePoint], value: f64) -> f32 {
    monotone_curve(points, value as f32)
}
