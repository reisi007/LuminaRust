//! MASK-LOCAL-P1.2d mask-local detail kernel.
//!
//! The local detail block reuses the *global* [`lumina_sidecar::Sharpening`] and
//! [`lumina_sidecar::NoiseReduction`] types, the *global* detail validators and —
//! most importantly — the *global* detail mathematics from
//! [`crate::detail_stages`], so a local detail is the same function at a
//! different place in the chain. Its position inside a layer is
//!
//! ```text
//! global result
//!   → local relative WB              (P1.1, `local_wb`)
//!   → local Basic                    (P1.1, `local_wb`)
//!   → local Presence                 (P1.2c, `local_presence`, optional)
//!   → local tone curve               (P1.2a, `local_tone`)
//!   → local HSL                      (P1.2b, `local_color`)
//!   → local Point Color              (P1.2b, `local_color`)
//!   → local Vibrance / Saturation    (P1.2b, `local_color`)
//!   → local Color Grading            (P1.2b, `local_color`)
//!   → local Noise Reduction          (P1.2d, this module)
//!   → local Sharpening               (P1.2d, this module)
//!   → fractional mask blend          (P0, `local_adjustments`)
//! ```
//!
//! That is **exactly** the global kernel position — `apply_recipe` runs
//! channel-LUT (WB + Basic) → presence → curves → HSL → point colour →
//! vibrance/saturation → grading → AI-denoise → **noise reduction** → **sharpening**
//! → red-eye → effects — and in particular noise reduction runs **before**
//! sharpening, exactly as `noise_reduction_before_sharpening_order_matters` pins
//! for the global recipe. It does not move the already-verified
//! P1.2a/P1.2b/P1.2c relative order: the two detail stages are *appended after*
//! the colour block, never inserted into it.
//!
//! The order is **observable**, not cosmetic. Colour changes neighbour
//! luminance, and neighbour luminance is what the bilateral similarity weights,
//! the separable Gaussian and the whole-frame gradient maximum are computed
//! from. The detail stages therefore *must* see the colour result and must not
//! be run on the WB/Basic prefix. `local_detail_boundary_tests.rs` pins that
//! with two independently transcribed chains — the documented order and the
//! swapped one — in a single layer that carries both a colour block and a detail
//! block.
//!
//! # Full-frame neighbourhoods, mask-only blending
//!
//! The 5x5 bilateral window, the separable Gaussian support, the detail mixing
//! and the whole-frame gradient maximum are computed over the whole local plane.
//! Notice what [`apply_mask_local_wb_basic_tone_color_detail`] does **not** take:
//! a mask plane, a region of interest, or a window size derived from either. It
//! receives only the frame, the recipe and the global render scale; the mask
//! arrives nowhere near the detail maths. That signature is the load-bearing
//! proof that there is **no** mask-dependent statistic, **no** ROI resize
//! fallback and **no** seam at the mask edge — the mask only ever gates the
//! blend amount, in the P0 compositor
//! ([`super::local_adjustments`]). A layer's render identity depends on the
//! persisted detail values, the global render scale and the mask plane, never on
//! the image geometry.
//!
//! # The render scale is global, never local
//!
//! The local detail block has **no** scale option of its own. It follows the
//! same effective output scale the global F-095 sharpening stage was given, so
//! local and global see the same effective radius scaling. It never overrides
//! the global `render_scale`.
//!
//! # One quantization boundary
//!
//! The whole chain is evaluated in floating point and quantized exactly once, at
//! the end. The global kernel rounds between its sub-stages because it operates
//! on a `u8` frame; the local chain owns a single boundary instead. This
//! divergence from the global detail stages is **deliberate and documented**, and
//! there is deliberately no test claiming byte-equality between the two paths —
//! see [`crate::detail_stages`] for the full statement.
//!
//! The neighbourhood stages read the plane in the *un-quantized* `(0..=255)`
//! `f32` domain the global kernel has always used for them, which is also the
//! domain [`crate::detail_stages`] is written in (its similarity term, its
//! luminance clamp and its `max(radius · scale, 0.5)` formula all carry absolute
//! `0..=255` constants). The per-pixel colour stages keep their own normalized
//! `0..=1` `f32` domain, so
//! [`apply_local_tone_and_color_plane`] converts `0..=255 → 0..=1` before them
//! and `0..=1 → 0..=255` after them. Those are **domain conversions, not
//! quantization boundaries**: no value is rounded, clipped or truncated there,
//! and the only rounding in the whole layer is the single final `u8` write in
//! [`apply_mask_local_wb_basic_tone_color_detail`].
//!
//! The two conversions do carry an `f32` *representation* error, and that error
//! is measured rather than waved away:
//! `the_colour_domain_conversion_is_byte_neutral_for_every_input_byte` pins the
//! exact integer round trip for all 256 `u8` values, and
//! `the_colour_domain_conversion_error_cannot_move_a_byte` bounds the residual
//! error on real colour-stage output far below the 0.5 that would flip a byte.

use super::local_color::local_tone_and_color_normalized;
use super::local_presence::{apply_local_presence, presence_plane};
use super::local_wb::round_local_channel;
use crate::detail_stages::{self, FloatPlane};
use lumina_sidecar::{MaskLocalRecipe, NoiseReduction, Sharpening};

/// Apply the whole local recipe — relative WB, Basic, presence, the tone curve,
/// the four colour stages, noise reduction and sharpening — in the global
/// kernel's own order, with a single final RGBA8 quantization.
pub(super) fn apply_mask_local_wb_basic_tone_color_detail(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    recipe: &MaskLocalRecipe,
    effective_scale: f32,
) {
    // The caller only selects this kernel when the layer actually carries a
    // non-neutral detail block; the `else` branches are defensive no-ops, never a
    // silent partial render.
    debug_assert!(recipe.has_local_detail());
    if !recipe.has_local_detail() {
        return;
    }
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || pixels.len() != w * h * 4 {
        // A frame whose geometry does not match its buffer cannot host a
        // neighbourhood; refuse loudly rather than read out of bounds. The P0
        // compositor already refuses a mask plane that does not match the
        // output, so this is a defensive guard only.
        debug_assert!(false, "local detail requires a {w}x{h} frame");
        return;
    }
    // The whole per-layer chain, in the global kernel's order, on one
    // un-quantized whole-frame plane:
    //
    //   1. local WB -> local Basic -> local Presence  (`detail_plane`)
    //   2. local tone curve -> local colour           (`tone_and_color_plane`)
    //   3. local Noise Reduction                      (`apply_local_noise_reduction`)
    //   4. local Sharpening                           (`apply_local_sharpening`)
    //   5. the one and only RGBA8 quantization
    let mut plane = detail_plane(pixels, w, h, recipe);
    // The per-pixel tail runs over the **whole** plane and stays un-quantized:
    // the neighbourhood stages below need the colour result, not a rounded one.
    apply_local_tone_and_color_plane(&mut plane, recipe);
    // Noise reduction, then sharpening, in the global kernel's own order. Both
    // are *neighbourhood* stages, so they see the un-blended values of their
    // neighbours over the whole frame — which is exactly the full-frame
    // contract.
    if let Some(noise) = recipe
        .detail
        .as_ref()
        .and_then(|d| d.noise_reduction.as_ref())
    {
        if !noise_reduction_is_pure_identity(noise) {
            apply_local_noise_reduction(&mut plane, w, h, noise);
        }
    }
    if let Some(sharpening) = recipe.detail.as_ref().and_then(|d| d.sharpening.as_ref()) {
        apply_local_sharpening(&mut plane, w, h, sharpening, effective_scale);
    }
    for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let present = plane[index];
        pixel[..3].copy_from_slice(&[
            round_local_channel(f64::from(present[0])),
            round_local_channel(f64::from(present[1])),
            round_local_channel(f64::from(present[2])),
        ]);
        // Deliberately leave pixel[3] (alpha) unchanged.
    }
}

/// The un-quantized `(0..=255)` `f32` chain the shared detail stages run on,
/// up to and including the local presence block.
///
/// It is deliberately the *same* builder the presence-only kernel uses, so a
/// layer that carries both blocks sees identical WB/Basic/presence values before
/// its detail stages instead of a second copy of that arithmetic. Without a
/// presence block it is the same WB + Basic prefix the P1.1 kernel evaluates,
/// narrowed into the shared `f32` detail domain (see the module docs on why that
/// narrowing is not an extra quantization boundary).
fn detail_plane(
    pixels: &[u8],
    width: usize,
    height: usize,
    recipe: &MaskLocalRecipe,
) -> Vec<[f32; 3]> {
    if let Some(presence) = recipe
        .presence
        .as_ref()
        .filter(|_| recipe.has_local_presence())
    {
        let mut plane = presence_plane(pixels, recipe);
        apply_local_presence(&mut plane, width, height, presence);
        return plane;
    }
    let gains = recipe.relative_white_balance_gains();
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| {
            let mut scaled = [0.0_f32; 3];
            for (channel, slot) in scaled.iter_mut().enumerate() {
                *slot = super::local_wb::scale_mask_local_wb_basic(
                    f64::from(pixel[channel]),
                    &gains,
                    recipe,
                    channel,
                ) as f32;
            }
            scaled
        })
        .collect()
}

/// The local tone curve and the four local colour stages, over the **whole**
/// plane and still un-quantized.
///
/// This is the P1.2a/P1.2b per-pixel tail, run as a whole-plane pass so that
/// the two neighbourhood stages downstream see the colour result *before* they
/// run. It calls literally the same [`local_tone_and_color_normalized`] the
/// P1.2b and P1.2c kernels use — the per-pixel arithmetic, its order and its
/// `f64`/`f32` conversions are identical, which is exactly what keeps a
/// P1.2a/P1.2b/P1.2c layer's bytes exact.
///
/// The two domain conversions are the only difference from the quantizing
/// callers: the shared colour stages are `f32` kernels on normalized
/// `0..=1` values, while the shared detail stages are `f32` kernels on
/// un-quantized `0..=255` values. Both directions are a plain multiply/divide by
/// 255 in `f32` — nothing is rounded, clipped or truncated — so this is not an
/// extra quantization boundary and the layer still has exactly one `u8` write.
fn apply_local_tone_and_color_plane(plane: &mut [[f32; 3]], recipe: &MaskLocalRecipe) {
    for pixel in plane.iter_mut() {
        let out = local_tone_and_color_normalized(
            [
                f64::from(pixel[0]),
                f64::from(pixel[1]),
                f64::from(pixel[2]),
            ],
            recipe,
        );
        for (channel, slot) in pixel.iter_mut().enumerate() {
            // Back into the un-quantized `(0..=255)` domain the shared detail
            // stages are written in. No rounding: see the module docs.
            *slot = out[channel] * 255.0;
        }
    }
}

/// The global F-096 stage's own early return: a fully zero block is identity.
fn noise_reduction_is_pure_identity(noise: &NoiseReduction) -> bool {
    noise.luminance == 0.0 && noise.color == 0.0
}

/// Run the *shared* bilateral noise-reduction mathematics over a whole
/// un-quantized plane.
///
/// This is the only place the local path touches the NR maths, and it calls
/// literally the same [`detail_stages::noise_reduction_write`] as the global
/// kernel. The 5x5 window is full-frame; the caller has not passed and does not
/// have access to a mask.
fn apply_local_noise_reduction(
    plane: &mut [[f32; 3]],
    width: usize,
    height: usize,
    noise: &NoiseReduction,
) {
    let source = plane.to_vec();
    let view = FloatPlane {
        pixels: &source,
        width,
        height,
    };
    for (index, pixel) in plane.iter_mut().enumerate() {
        let (x, y) = (index % width, index / width);
        let out = detail_stages::noise_reduction_write(&view, x, y, noise);
        pixel[0] = out.red;
        pixel[1] = out.green;
        pixel[2] = out.blue;
    }
}

/// Run the *shared* sharpening mathematics over a whole un-quantized plane, in
/// the global kernel's own order: fine blur, coarse blur, whole-frame gradient
/// maximum, then the per-pixel detail mix and flat-area factor.
///
/// The radius formula is [`detail_stages::sharpen_sigma`], i.e. the same
/// `max(radius · effective_scale, 0.5)` the global F-095 stage uses, with
/// `effective_scale` being the **global** render scale.
fn apply_local_sharpening(
    plane: &mut [[f32; 3]],
    width: usize,
    height: usize,
    sharpening: &Sharpening,
    effective_scale: f32,
) {
    if sharpening.amount == 0.0 {
        return;
    }
    let lum: Vec<f32> = plane
        .iter()
        .map(|pixel| detail_stages::luminance(pixel[0], pixel[1], pixel[2]))
        .collect();
    let (fine_radius, coarse_radius) = detail_stages::sharpen_blur_radii(sharpening);
    let fine = detail_stages::gaussian_blur(&lum, width, height, fine_radius, effective_scale);
    let coarse = detail_stages::gaussian_blur(&lum, width, height, coarse_radius, effective_scale);
    let (gradients, max_gradient) = detail_stages::gradient_plane(&lum, width, height);
    for (index, pixel) in plane.iter_mut().enumerate() {
        let detail = detail_stages::sharpen_detail(
            lum[index],
            fine[index],
            coarse[index],
            sharpening.detail,
        );
        let edge = detail_stages::sharpen_edge_factor(gradients[index], max_gradient);
        let ratio = detail_stages::sharpen_ratio(
            lum[index],
            detail_stages::sharpen_amount(sharpening, edge) * detail,
        );
        for channel in pixel.iter_mut() {
            *channel *= ratio;
        }
    }
}
