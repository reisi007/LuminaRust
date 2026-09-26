//! MASK-LOCAL-P1.2c mask-local presence kernel.
//!
//! The local presence block reuses the *global* [`lumina_sidecar::Presence`]
//! type, the *global* presence validator and — most importantly — the *global*
//! presence mathematics from [`crate::presence_stages`], so a local presence is
//! the same function at a different place in the chain. Its position inside a
//! layer is
//!
//! ```text
//! global result
//!   → local relative WB              (P1.1, `local_wb`)
//!   → local Basic                    (P1.1, `local_wb`)
//!   → local Presence                 (P1.2c, this module)
//!   → local tone curve               (P1.2a, `local_tone`)
//!   → local HSL                      (P1.2b, `local_color`)
//!   → local Point Color              (P1.2b, `local_color`)
//!   → local Vibrance / Saturation    (P1.2b, `local_color`)
//!   → local Color Grading            (P1.2b, `local_color`)
//!   → local Noise Reduction          (P1.2d, `local_detail`, optional)
//!   → local Sharpening               (P1.2d, `local_detail`, optional)
//!   → fractional mask blend          (P0, `local_adjustments`)
//! ```
//!
//! The two `local_detail` rows are optional and absent in a P1.2c-only layer;
//! they are listed because `presence_plane` below also builds the prefix the
//! P1.2d kernel continues from.
//!
//! That is **exactly** the global kernel position (channel LUT → presence →
//! curve → color), and it does not move the already-verified P1.2a/P1.2b
//! relative order: presence is inserted *before* the curve, never after.
//!
//! # Full-frame neighbourhoods, mask-only blending
//!
//! The DoG neighbourhood and the dehaze percentile are computed over the whole
//! local plane. Nothing in this module receives the mask plane, a region of
//! interest, or a window size derived from either. There is therefore **no**
//! mask-dependent statistic, **no** ROI resize fallback and **no** seam at the
//! mask edge: the mask only ever gates the blend amount, in the P0 compositor
//! ([`super::local_adjustments`]). A layer's render identity depends on the
//! persisted presence values and the mask plane, never on the image geometry.
//!
//! # One quantization boundary
//!
//! The whole chain is evaluated in floating point and quantized exactly once, at
//! the end. The global kernel rounds between its sub-stages because it operates
//! on a `u8` frame; the local chain owns a single boundary instead. This
//! divergence from the global presence stage is **deliberate and documented**,
//! and there is deliberately no test claiming byte-equality between the two
//! paths — see [`crate::presence_stages`] for the full statement.

use super::local_color::local_tone_and_color_stages;
use super::local_wb::scale_mask_local_wb_basic;
use crate::presence_stages::{
    self, airlight, clarity_radius, dark_channel, dehaze_transmission, dehaze_value, dog_channel,
    texture_radius, FloatPlane,
};
use lumina_sidecar::{MaskLocalRecipe, Presence};

/// Apply the whole local recipe (relative WB, Basic, presence, tone curve and
/// the four colour stages) with a single final RGBA8 quantization.
pub(super) fn apply_mask_local_wb_basic_presence_tone_color(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    recipe: &MaskLocalRecipe,
) {
    // The caller only selects this kernel when the layer actually carries a
    // non-neutral presence block; the `else` branch is a defensive no-op, never
    // a silent partial render.
    debug_assert!(recipe.has_local_presence());
    let Some(presence) = recipe.presence.as_ref() else {
        return;
    };
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || pixels.len() != w * h * 4 {
        // A frame whose geometry does not match its buffer cannot host a
        // neighbourhood; refuse loudly rather than read out of bounds. The P0
        // compositor already refuses a mask plane that does not match the
        // output, so this is a defensive guard only.
        debug_assert!(false, "local presence requires a {w}x{h} frame");
        return;
    }
    let mut plane = presence_plane(pixels, recipe);
    // The presence stage runs over the whole plane first, because it is a
    // *neighbourhood* stage: it must see the un-blended WB+Basic values of the
    // neighbours, which is exactly the full-frame contract. Only then does the
    // per-pixel tail (curve, colour, one quantization) run.
    apply_local_presence(&mut plane, w, h, presence);
    for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let present = plane[index];
        let out = local_tone_and_color_stages(
            [
                f64::from(present[0]),
                f64::from(present[1]),
                f64::from(present[2]),
            ],
            recipe,
        );
        pixel[..3].copy_from_slice(&out);
        // Deliberately leave pixel[3] (alpha) unchanged.
    }
}

/// Build the un-quantized `(0..=255)` `f32` chain the shared presence stage runs
/// on: local relative WB, then the four Basic controls, then the shared
/// presence mathematics, with **no** `u8` rounding anywhere in between.
///
/// The narrowing from the `f64` WB/Basic result to `f32` is not an extra
/// quantization boundary — it is the numeric domain of the *shared* presence
/// stage, which the global kernel has always evaluated in `f32` (see
/// [`crate::presence_stages`]). The chain is widened back to `f64` for the tone
/// curve and the colour stages, and the single `u8` rounding still happens only
/// at the very end of the layer.
///
/// The MASK-LOCAL-P1.2d detail kernel reuses this exact builder so a layer that
/// carries both blocks sees the *same* un-quantized WB/Basic/presence values
/// before its detail stages, instead of a second copy of the arithmetic.
pub(super) fn presence_plane(pixels: &[u8], recipe: &MaskLocalRecipe) -> Vec<[f32; 3]> {
    let gains = recipe.relative_white_balance_gains();
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| {
            let mut scaled = [0.0_f32; 3];
            for (channel, slot) in scaled.iter_mut().enumerate() {
                *slot =
                    scale_mask_local_wb_basic(f64::from(pixel[channel]), &gains, recipe, channel)
                        as f32;
            }
            scaled
        })
        .collect()
}

/// Run the *shared* presence mathematics over a whole un-quantized plane, in
/// the global kernel's own order: texture DoG, clarity DoG, then dehaze.
///
/// This is the only place the local path touches the presence maths, and it
/// calls literally the same functions as the global kernel. The three
/// neighbourhoods/statistics are full-frame; the caller has not passed and does
/// not have access to a mask.
pub(super) fn apply_local_presence(
    plane: &mut [[f32; 3]],
    width: usize,
    height: usize,
    p: &Presence,
) {
    // Each DoG pass reads a snapshot of the plane, exactly like the global
    // kernel's `pixels.to_vec()`, so a pixel's neighbours always carry the
    // pre-pass value.
    dog_plane(plane, width, height, texture_radius(p.texture), p.texture);
    dog_plane(plane, width, height, clarity_radius(p.clarity), p.clarity);
    if p.dehaze == 0.0 {
        return;
    }
    let dark = dark_channel(&FloatPlane {
        pixels: plane,
        width,
        height,
    });
    let airlight = airlight(&dark);
    for (index, pixel) in plane.iter_mut().enumerate() {
        let transmission = dehaze_transmission(dark[index], airlight, p.dehaze);
        for channel in pixel.iter_mut() {
            *channel = dehaze_value(*channel / 255.0, airlight, transmission) * 255.0;
        }
    }
}

/// One full-frame DoG pass over the un-quantized plane. The result is kept as
/// `f32`; the global kernel rounds the very same value to `u8` at this exact
/// point, and that deliberate difference is the documented quantization
/// divergence between the two paths.
fn dog_plane(
    plane: &mut [[f32; 3]],
    width: usize,
    height: usize,
    radius: presence_stages::Radius,
    amount: f32,
) {
    if amount == 0.0 {
        return;
    }
    let source = plane.to_vec();
    let view = FloatPlane {
        pixels: &source,
        width,
        height,
    };
    for (index, pixel) in plane.iter_mut().enumerate() {
        let (x, y) = (index % width, index / width);
        for (c, channel) in pixel.iter_mut().enumerate() {
            *channel = dog_channel(&view, x, y, c, radius, amount);
        }
    }
}
