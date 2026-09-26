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
//!   → local Presence                 (P1.2c, `local_presence`, optional)
//!   → local tone curve               (P1.2a, `local_tone`)
//!   → local HSL                      (P1.2b, this module)
//!   → local Point Color              (P1.2b, this module)
//!   → local Vibrance / Saturation    (P1.2b, this module)
//!   → local Color Grading            (P1.2b, this module)
//!   → local Noise Reduction          (P1.2d, `local_detail`, optional)
//!   → local Sharpening               (P1.2d, `local_detail`, optional)
//!   → fractional mask blend          (P0, `local_adjustments`)
//! ```
//!
//! That order is exactly the global colour order
//! (`HSL → Point Color → Vibrance/Saturation → Color Grading`) evaluated at
//! exactly the global colour position (after the global tone curve, before the
//! global colour stage's neighbours) — see
//! `ImageFrame::apply_recipe_with_scale_white_balance_and_denoise`. The two
//! optional trailing stages are the local detail block, which is why this module
//! exposes [`local_tone_and_color_normalized`]: the P1.2d kernel has to run the
//! neighbourhood stages on the *un-quantized* colour result.
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
    for pixel in pixels.as_chunks_mut::<4>().0 {
        // Stage 1+2: relative WB then Basic, in float, per channel.
        let mut scaled = [0.0_f64; 3];
        for channel in 0..3 {
            scaled[channel] =
                scale_mask_local_wb_basic(f64::from(pixel[channel]), &gains, recipe, channel);
        }
        let out = local_tone_and_color_stages(scaled, recipe);
        pixel[..3].copy_from_slice(&out);
        // Deliberately leave pixel[3] (alpha) unchanged.
    }
}

/// The chain tail shared by the P1.2b and the P1.2c local kernels: the local
/// tone curve, the four local colour stages in the global colour order, and the
/// layer's **one and only** RGBA8 quantization.
///
/// `scaled` is the un-quantized `(0..=255)` result of everything that ran
/// before it. The two callers differ only in what produced it: the P1.2b kernel
/// passes the `f64` WB+Basic result, the P1.2c kernel passes the `f32` presence
/// plane promoted back to `f64`. That is why presence can be inserted before the
/// curve without a second copy of the colour stages, and why a layer with no
/// presence block keeps its exact P1.2b bytes.
pub(super) fn local_tone_and_color_stages(scaled: [f64; 3], recipe: &MaskLocalRecipe) -> [u8; 3] {
    let rgb = local_tone_and_color_normalized(scaled, recipe);
    [
        round_local_channel(f64::from(rgb[0]) * 255.0),
        round_local_channel(f64::from(rgb[1]) * 255.0),
        round_local_channel(f64::from(rgb[2]) * 255.0),
    ]
}

/// The chain tail **without** the RGBA8 rounding: the local tone curve and the
/// four local colour stages in the global colour order, returned as the
/// un-quantized normalized `(0..=1)` triple.
///
/// This is the shared body of all three local colour tails.
/// [`local_tone_and_color_stages`] is exactly this plus the layer's one and only
/// RGBA8 quantization; the MASK-LOCAL-P1.2d detail kernel calls this per pixel
/// over the **whole** plane instead, because its noise-reduction and sharpening
/// stages have to run on the *un-quantized colour result* — the whole per-layer
/// order is `… → colour → noise reduction → sharpening`, mirroring
/// `apply_recipe`. The arithmetic, its order and its `f64`/`f32` conversions are
/// the same in all callers, which is what keeps a P1.2a/P1.2b/P1.2c layer's bytes
/// exact.
pub(super) fn local_tone_and_color_normalized(
    scaled: [f64; 3],
    recipe: &MaskLocalRecipe,
) -> [f32; 3] {
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
    rgb
}
