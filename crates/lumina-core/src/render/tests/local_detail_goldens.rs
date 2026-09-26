//! MASK-LOCAL-P1.2d: the exact CPU goldens of the mask-local detail block.
//!
//! Split out of `local_detail_tests.rs` (file-size ratchet) so the pinned bytes
//! live in one auditable place. Every value here is an **exact** literal: no
//! tolerance, no PSNR, no "close enough". They are the pinned output of the
//! documented chain
//!
//! ```text
//! global result → local relative WB → local Basic → local Presence →
//! local tone curve → local colour → local Noise Reduction → local Sharpening →
//! fractional mask blend
//! ```
//!
//! evaluated in floating point with exactly **one** RGBA8 quantization at the end
//! of the layer.
//!
//! There deliberately is **no** golden here that equals the global detail
//! stage: the two paths quantize at different points on purpose, so that
//! equality is false by design (see `crate::detail_stages`).

use lumina_sidecar::{NoiseReduction, Sharpening};

// ---------------------------------------------------------------- the goldens

/// `sharpening(amount 0.75, radius 2.0, detail 0.5, masking 0.0)` over the
/// five-sample step row. The flat-area masking is off, so every pixel — flat
/// interior included — is sharpened.
pub(super) const GOLDEN_SHARPENING_AMOUNT: [u8; 20] = [
    2, 2, 2, 255, 0, 0, 0, 255, 80, 80, 80, 255, 170, 170, 170, 255, 255, 255, 255, 255,
];
/// The same block at the **lower** legal radius boundary, `0.1`.
pub(super) const GOLDEN_SHARPENING_RADIUS_MIN: [u8; 20] = [
    20, 20, 20, 255, 14, 14, 14, 255, 90, 90, 90, 255, 160, 160, 160, 255, 236, 236, 236, 255,
];
/// The same block at the **upper** legal radius boundary, `10.0`.
pub(super) const GOLDEN_SHARPENING_RADIUS_MAX: [u8; 20] = [
    0, 0, 0, 255, 0, 0, 0, 255, 68, 68, 68, 255, 182, 182, 182, 255, 255, 255, 255, 255,
];
/// `detail = 1.0`: the detail is `lum - fine_blur` only.
pub(super) const GOLDEN_SHARPENING_DETAIL_ONE: [u8; 20] = [
    17, 17, 17, 255, 1, 1, 1, 255, 87, 87, 87, 255, 163, 163, 163, 255, 249, 249, 249, 255,
];
/// `detail = 0.0`: the detail is `lum - coarse_blur` only.
pub(super) const GOLDEN_SHARPENING_DETAIL_ZERO: [u8; 20] = [
    0, 0, 0, 255, 0, 0, 0, 255, 74, 74, 74, 255, 176, 176, 176, 255, 255, 255, 255, 255,
];
/// A flat ten-sample row with one outlier at index 3, `masking = 1.0`: the flat
/// samples keep 128 and only the two samples flanking the outlier move.
pub(super) const GOLDEN_SHARPENING_MASKING_ONE: [u8; 40] = [
    128, 128, 128, 128, 128, 128, 128, 128, 93, 93, 93, 128, 200, 200, 200, 128, 93, 93, 93, 128,
    128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128,
    128,
];
/// The same row with `masking = 0.0`: the flat samples move too.
pub(super) const GOLDEN_SHARPENING_MASKING_ZERO: [u8; 40] = [
    127, 127, 127, 128, 120, 120, 120, 128, 93, 93, 93, 128, 255, 255, 255, 128, 93, 93, 93, 128,
    120, 120, 120, 128, 127, 127, 127, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128,
    128,
];
/// `noise_reduction(luminance 0.6, color 0.0)` over a five-step grey row with a
/// non-opaque image alpha of 33, which must survive untouched.
pub(super) const GOLDEN_NOISE_LUMINANCE: [u8; 20] = [
    23, 23, 23, 33, 31, 31, 31, 33, 55, 55, 55, 33, 118, 118, 118, 33, 199, 199, 199, 33,
];
/// `noise_reduction(luminance 0.0, color 0.75)` over a saturated four-step row.
pub(super) const GOLDEN_NOISE_COLOR: [u8; 16] = [
    59, 90, 181, 255, 101, 120, 156, 255, 130, 40, 61, 255, 242, 200, 87, 255,
];
/// Both stages on one layer: noise reduction **first**, then sharpening.
pub(super) const GOLDEN_NOISE_BEFORE_SHARPEN: [u8; 20] = [
    17, 17, 17, 255, 1, 1, 1, 255, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255,
];
/// The full stack: WB + Basic + Presence + curve + HSL + vibrance/saturation +
/// noise reduction + sharpening.
///
/// These exact bytes **moved** when the within-layer order was corrected to the
/// contract (`… → colour → noise reduction → sharpening`, mirroring
/// `apply_recipe`): the old value was produced by a kernel that ran the two
/// neighbourhood stages *before* the tone curve and the colour block. The new
/// value is not taken on trust: the *within-layer order* is independently
/// pinned by `the_within_layer_order_runs_the_colour_block_before_the_detail_stages`
/// and `the_full_stack_matches_the_independently_transcribed_chain` in
/// `local_detail_order_tests.rs`, but those run on a **different** fixture (a
/// 24×3 hue-sweep frame with its own scalar, colour and detail blocks), so they
/// do not re-derive *these* 36 bytes. This constant is therefore pinned by
/// construction against the documented order only — the transcriptions there
/// prove the order, and the values here prove the arithmetic.
pub(super) const GOLDEN_FULL_STACK: [u8; 36] = [
    151, 185, 215, 200, 165, 200, 223, 200, 179, 213, 230, 200, 189, 221, 234, 200, 197, 227, 237,
    200, 203, 231, 235, 200, 209, 235, 234, 200, 217, 236, 236, 200, 225, 238, 238, 200,
];
/// The sharpening golden under a **half** mask (`32768`).
pub(super) const GOLDEN_HALF_MASK: [u8; 20] = [
    11, 11, 11, 255, 10, 10, 10, 255, 85, 85, 85, 255, 165, 165, 165, 255, 243, 243, 243, 255,
];
/// The same half-mask blend over a fully transparent input row.
pub(super) const GOLDEN_TRANSPARENT_HALF_MASK: [u8; 20] = [
    11, 11, 11, 0, 10, 10, 10, 0, 85, 85, 85, 0, 165, 165, 165, 0, 243, 243, 243, 0,
];
/// Two fully-overlapping layers, sharpening first then noise reduction.
pub(super) const GOLDEN_OVERLAP_SHARPEN_FIRST: [u8; 20] = [
    6, 6, 6, 255, 45, 45, 45, 255, 112, 112, 112, 255, 223, 223, 223, 255, 252, 252, 252, 255,
];
/// The same two layers in the other persisted order.
pub(super) const GOLDEN_OVERLAP_NOISE_FIRST: [u8; 20] = [
    9, 9, 9, 255, 42, 42, 42, 255, 110, 110, 110, 255, 227, 227, 227, 255, 252, 252, 252, 255,
];
/// The colour layer first and the detail layer second: `detail(colour(global))`.
pub(super) const GOLDEN_DETAIL_AFTER_COLOUR: [u8; 20] = [
    0, 0, 0, 255, 0, 0, 0, 255, 109, 48, 36, 255, 213, 131, 116, 255, 255, 250, 246, 255,
];

/// The two reference orders of the detail stages, transcribed **independently**
/// of the kernel under test from the documented F-096/F-095 contract.
pub(super) fn independent_order_chains(
    pixels: &[u8],
    noise: &NoiseReduction,
    sharp: &Sharpening,
) -> (Vec<u8>, Vec<u8>) {
    // Transcribed from the contract: the whole chain, in f64, with a single
    // rounding at the end. The local P0 scalars are all zero, so the WB + Basic
    // prefix is the identity.
    let base: Vec<[f64; 3]> = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])])
        .collect();
    let luma = |p: [f64; 3]| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2];
    let noise_pass = |plane: &[[f64; 3]]| -> Vec<[f64; 3]> {
        let w = plane.len();
        let mut out = Vec::with_capacity(w);
        for (index, source) in plane.iter().enumerate() {
            let x = index as i64;
            let base_y = luma(*source);
            let mut ly = 0.0;
            let mut cwr = 0.0;
            let mut cwb = 0.0;
            let mut sum = 0.0;
            let mut csum = 0.0;
            for dy in -2i64..=2 {
                for dx in -2i64..=2 {
                    let xx = (x + dx).clamp(0, w as i64 - 1) as usize;
                    let jy = luma(plane[xx]);
                    let d2 = (dx * dx + dy * dy) as f64;
                    let spatial = (-d2 / (2.0 * 1.5 * 1.5)).exp();
                    let similar =
                        (-((base_y - jy).powi(2)) / (2.0 * 0.12 * 255.0 * 0.12 * 255.0)).exp();
                    let weight = spatial * similar;
                    ly += weight * jy;
                    sum += weight;
                    let cw = (-d2 / (2.0 * 2.0 * 2.0)).exp();
                    csum += cw;
                    cwr += cw * (plane[xx][0] - jy);
                    cwb += cw * (plane[xx][2] - jy);
                }
            }
            let filtered = ly / sum;
            let yv =
                base_y * (1.0 - f64::from(noise.luminance)) + filtered * f64::from(noise.luminance);
            let cr = (source[0] - base_y) * (1.0 - f64::from(noise.color))
                + (cwr / csum) * f64::from(noise.color);
            let cb = (source[2] - base_y) * (1.0 - f64::from(noise.color))
                + (cwb / csum) * f64::from(noise.color);
            let cg = source[1] - base_y;
            out.push([yv + cr, yv + cg, yv + cb]);
        }
        out
    };
    // The F-095 separable Gaussian, transcribed: a horizontal pass over the
    // clamped row, then a vertical pass. The frame is a single row, so the
    // vertical pass clamps every tap onto row 0 and is therefore the identity on
    // the horizontal result.
    let blur = |plane: &[[f64; 3]], radius: f64| -> Vec<f64> {
        let sigma = radius.max(0.5);
        let r = (sigma * 3.0).ceil() as i64;
        let mut kernel = Vec::new();
        for k in -r..=r {
            kernel.push((-(k * k) as f64 / (2.0 * sigma * sigma)).exp());
        }
        let z: f64 = kernel.iter().sum();
        for value in &mut kernel {
            *value /= z;
        }
        let w = plane.len();
        let lum: Vec<f64> = plane.iter().map(|p| luma(*p)).collect();
        let mut tmp = vec![0.0; w];
        for (x, slot) in tmp.iter_mut().enumerate() {
            for k in -r..=r {
                *slot +=
                    kernel[(k + r) as usize] * lum[(x as i64 + k).clamp(0, w as i64 - 1) as usize];
            }
        }
        let mut out = vec![0.0; w];
        for (x, slot) in out.iter_mut().enumerate() {
            // The vertical pass over a one-row frame: every tap clamps to row 0.
            for k in -r..=r {
                *slot += kernel[(k + r) as usize] * tmp[x];
            }
        }
        out
    };
    let sharpen_pass = |plane: &Vec<[f64; 3]>| -> Vec<[f64; 3]> {
        let w = plane.len();
        let lum: Vec<f64> = plane.iter().map(|p| luma(*p)).collect();
        let fine = blur(plane, (f64::from(sharp.radius) * 0.5).max(0.5));
        let coarse = blur(plane, (f64::from(sharp.radius) * 1.5).max(0.5));
        let mut gradients = vec![0.0; w];
        let mut maxg: f64 = 0.0;
        for y in 0..w {
            let x = y as i64;
            let gx = lum[(x + 1).clamp(0, w as i64 - 1) as usize] - lum[(x - 1).max(0) as usize];
            let gy = lum[(y as i64 + 1).clamp(0, w as i64 - 1) as usize]
                - lum[(y as i64 - 1).max(0) as usize];
            gradients[y] = gx.abs() + gy.abs();
            maxg = maxg.max(gradients[y]);
        }
        plane
            .iter()
            .enumerate()
            .map(|(index, p)| {
                let d = f64::from(sharp.detail) * (lum[index] - fine[index])
                    + (1.0 - f64::from(sharp.detail)) * (lum[index] - coarse[index]);
                let edge = if maxg > 0.0 {
                    (gradients[index] / maxg).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let amount = f64::from(sharp.amount)
                    * ((1.0 - f64::from(sharp.masking)) + f64::from(sharp.masking) * edge);
                let ny = (lum[index] + amount * d).clamp(0.0, 255.0);
                let ratio = if lum[index] > 1e-6 {
                    ny / lum[index]
                } else {
                    0.0
                };
                [p[0] * ratio, p[1] * ratio, p[2] * ratio]
            })
            .collect()
    };
    let quantise = |plane: &Vec<[f64; 3]>| -> Vec<u8> {
        plane
            .iter()
            .zip(pixels.as_chunks::<4>().0)
            .flat_map(|(p, original)| {
                let rgb = [
                    (p[0] / 255.0) as f32,
                    (p[1] / 255.0) as f32,
                    (p[2] / 255.0) as f32,
                ];
                [
                    (f64::from(rgb[0]) * 255.0).round().clamp(0.0, 255.0) as u8,
                    (f64::from(rgb[1]) * 255.0).round().clamp(0.0, 255.0) as u8,
                    (f64::from(rgb[2]) * 255.0).round().clamp(0.0, 255.0) as u8,
                    original[3],
                ]
            })
            .collect()
    };
    // Documented order: noise reduction, then sharpening.
    let right = quantise(&sharpen_pass(&noise_pass(&base)));
    // The wrong order, for the non-vacuity check.
    let wrong = quantise(&noise_pass(&sharpen_pass(&base)));
    (right, wrong)
}
