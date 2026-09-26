//! MASK-LOCAL-P1.2d: the **independent** re-derivation of the local detail
//! chain.
//!
//! Split out of `local_detail_boundary_tests.rs` (file-size ratchet). Every
//! formula in this file is transcribed from the documented MASK-LOCAL-P1.2d
//! contract — the WB + Basic prefix in `f64`, the tone-curve composition, the
//! `0..=1`/`0..=255` domain conversions, the F-096 bilateral kernel, the F-095
//! separable Gaussian, the detail mix and the flat-area factor — and **not** read
//! out of `crate::detail_stages` nor out of the function under test
//! (`super::super::local_detail::apply_mask_local_wb_basic_tone_color_detail`).
//!
//! The *per-pixel colour* stages are the one deliberate exception: they are the
//! shared global `f32` kernels in `crate::color_stages` / `crate::monotone_curve`
//! (the same ones the P1.2a/P1.2b/P1.2c goldens already pin), called here in the
//! **documented order**. What this file proves is therefore the *chain*: which
//! stage runs before which, which domain each stage sees, and that there is
//! exactly one `u8` write. The colour arithmetic itself is not re-derived, by
//! design — it is shared code, not a second implementation.
//!
//! It exists so the "the detail stages run *after* the colour block" claim is
//! checked against material that was not produced by the function under test,
//! and so the "exactly one quantization boundary" claim has a whole-plane
//! spelling. The two tests that consume these chains are in
//! `local_detail_order_tests.rs`.

use crate::color_stages::{
    color_grading_stage, hsl_stage, point_color_stage, vibrance_saturation_stage,
};
use lumina_sidecar::LocalAdjustments;

/// The three whole-frame chains this module re-derives, all of them on the same
/// un-quantized `f32` `(0..=255)` plane, all of them with the layer's single
/// final `u8` write.
pub(super) struct Chains {
    /// `prefix → tone curve → colour → one quantization`: the chain *without*
    /// the detail stages. Used to validate the colour half of the transcription
    /// in isolation, against a recipe that carries no detail block.
    pub colour_only: Vec<u8>,
    /// The **documented** order:
    /// `prefix → tone curve → colour → noise reduction → sharpening → one
    /// quantization`.
    pub documented: Vec<u8>,
    /// The **swapped** order the implementation must not have:
    /// `prefix → noise reduction → sharpening → tone curve → colour → one
    /// quantization`.
    pub swapped: Vec<u8>,
    /// The documented order with one *extra* quantization inserted between the
    /// colour block and the noise reduction, i.e. the value the neighbourhood
    /// stages would see if the layer had a second quantization boundary.
    pub early: Vec<u8>,
}

/// Rec.709 luminance of one `(0..=255)` triple, transcribed.
fn luma(p: [f32; 3]) -> f32 {
    0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]
}

/// The documented WB + Basic prefix, in `f64`, narrowed into the shared `f32`
/// `(0..=255)` detail domain — exactly the domain the narrowing of
/// `scale_mask_local_wb_basic` into the presence/detail plane is documented to
/// produce.
fn prefix_plane(recipe: &LocalAdjustments, pixels: &[u8]) -> Vec<[f32; 3]> {
    let warmth = recipe.temperature_delta_k / 5500.0;
    let gains = [
        1.0 - warmth * 0.35,
        1.0 - recipe.tint_delta * 0.20,
        1.0 + warmth * 0.35,
    ];
    let exposure_multiplier = 2.0_f64.powf(recipe.exposure);
    let contrast_factor = 1.0 + recipe.contrast;
    let prefix = |value: f64, channel: usize| {
        let mut value = value * gains[channel];
        value = (value * exposure_multiplier).clamp(0.0, 255.0);
        value = ((value - 128.0) * contrast_factor + 128.0).clamp(0.0, 255.0);
        let x = value / 255.0;
        let shadow_weight = ((0.5 - x) / 0.5).max(0.0).powi(2);
        value = (x + recipe.shadows * shadow_weight * 0.25).clamp(0.0, 1.0) * 255.0;
        let x = value / 255.0;
        let highlight_weight = ((x - 0.5) / 0.5).max(0.0).powi(2);
        (x + recipe.highlights * highlight_weight * 0.25).clamp(0.0, 1.0) * 255.0
    };
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let mut row = [0.0_f32; 3];
            for (channel, slot) in row.iter_mut().enumerate() {
                *slot = prefix(f64::from(p[channel]), channel) as f32;
            }
            row
        })
        .collect()
}

/// The local tone curve followed by the four local colour stages, over a whole
/// plane, still un-quantized.
///
/// The curve is transcribed from the documented composition (Rec.709 luminance
/// → PCHIP master → per-channel curve → `value · master / luminance` with the
/// `luminance > 1e-9` guard → clamp → back to `0..=255`); the four colour stages
/// are the shared global `f32` kernels, called in the documented order. The
/// `0..=255 → 0..=1 → 0..=255` conversions are the documented domain changes
/// between the neighbourhood domain and the colour domain, and they round
/// nothing.
fn colour_plane(recipe: &LocalAdjustments, plane: &[[f32; 3]]) -> Vec<[f32; 3]> {
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
    let (vibrance, saturation) = (recipe.vibrance as f32, recipe.saturation as f32);
    let has_vibrance_saturation = vibrance != 0.0 || saturation != 0.0;
    plane
        .iter()
        .map(|p| {
            // The tone curve, in `f64` on the `0..=255` plane.
            let toned = curves.map_or(
                [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])],
                |curves| {
                    let original = [
                        f64::from(p[0]) / 255.0,
                        f64::from(p[1]) / 255.0,
                        f64::from(p[2]) / 255.0,
                    ];
                    let luminance =
                        0.2126 * original[0] + 0.7152 * original[1] + 0.0722 * original[2];
                    let master = f64::from(crate::monotone_curve(&curves.master, luminance as f32));
                    let channels = [
                        curves.channels.red.as_deref(),
                        curves.channels.green.as_deref(),
                        curves.channels.blue.as_deref(),
                    ];
                    let mut toned = [0.0_f64; 3];
                    for (channel, points) in channels.iter().enumerate() {
                        let value = points.map_or(original[channel], |points| {
                            f64::from(crate::monotone_curve(points, original[channel] as f32))
                        });
                        toned[channel] = (if luminance > 1e-9 {
                            value * master / luminance
                        } else {
                            master
                        })
                        .clamp(0.0, 1.0)
                            * 255.0;
                    }
                    toned
                },
            );
            // The four colour stages, on the normalized `0..=1` domain.
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
            // Back into the un-quantized `0..=255` domain the neighbourhood
            // stages are written in. This is a domain conversion, not a
            // quantization: nothing is rounded here.
            [rgb[0] * 255.0, rgb[1] * 255.0, rgb[2] * 255.0]
        })
        .collect()
}

/// The documented F-096 bilateral kernel, transcribed, over a whole plane.
fn noise_plane(
    plane: &[[f32; 3]],
    width: usize,
    height: usize,
    noise: &lumina_sidecar::NoiseReduction,
) -> Vec<[f32; 3]> {
    let mut out = Vec::with_capacity(plane.len());
    for (index, source) in plane.iter().enumerate() {
        let (x, y) = (index % width, index / width);
        let base = luma(*source);
        let (mut ly, mut cwr, mut cwb, mut sum, mut csum) = (0.0_f32, 0.0, 0.0, 0.0, 0.0);
        for dy in -2i32..=2 {
            for dx in -2i32..=2 {
                let xx = (x as i32 + dx).clamp(0, width as i32 - 1) as usize;
                let yy = (y as i32 + dy).clamp(0, height as i32 - 1) as usize;
                let neighbour = plane[yy * width + xx];
                let jy = luma(neighbour);
                let d2 = (dx * dx + dy * dy) as f32;
                let spatial = (-d2 / (2.0 * 1.5 * 1.5)).exp();
                let similar = (-((base - jy).powi(2)) / (2.0 * 0.12 * 255.0 * 0.12 * 255.0)).exp();
                let weight = spatial * similar;
                ly += weight * jy;
                sum += weight;
                let cw = (-d2 / (2.0 * 2.0 * 2.0)).exp();
                csum += cw;
                cwr += cw * (neighbour[0] - jy);
                cwb += cw * (neighbour[2] - jy);
            }
        }
        let filtered = ly / sum;
        let yv = base * (1.0 - noise.luminance) + filtered * noise.luminance;
        let cr = (source[0] - base) * (1.0 - noise.color) + (cwr / csum) * noise.color;
        let cb = (source[2] - base) * (1.0 - noise.color) + (cwb / csum) * noise.color;
        let cg = source[1] - base;
        out.push([yv + cr, yv + cg, yv + cb]);
    }
    out
}

/// The documented F-095 separable Gaussian over a whole luminance plane,
/// replicating edges, with the documented `max(radius · scale, 0.5)` sigma and
/// the three-sigma support.
fn blur(lum: &[f32], width: usize, height: usize, radius: f32, effective_scale: f32) -> Vec<f32> {
    let sigma = (radius * effective_scale).max(0.5);
    let r = (sigma * 3.0).ceil() as i32;
    let mut kernel = Vec::new();
    for k in -r..=r {
        kernel.push((-(k * k) as f32 / (2.0 * sigma * sigma)).exp());
    }
    let z: f32 = kernel.iter().sum();
    for value in &mut kernel {
        *value /= z;
    }
    let mut tmp = vec![0.0_f32; lum.len()];
    let mut out = vec![0.0_f32; lum.len()];
    for y in 0..height {
        for x in 0..width {
            for (i, k) in (-r..=r).enumerate() {
                tmp[y * width + x] +=
                    kernel[i] * lum[y * width + (x as i32 + k).clamp(0, width as i32 - 1) as usize];
            }
        }
    }
    for y in 0..height {
        for x in 0..width {
            for (i, k) in (-r..=r).enumerate() {
                out[y * width + x] += kernel[i]
                    * tmp[(y as i32 + k).clamp(0, height as i32 - 1) as usize * width + x];
            }
        }
    }
    out
}

/// The documented F-095 sharpening pass, transcribed: whole-frame gradient
/// magnitude and its maximum, the detail mix, the flat-area factor and the
/// luminance-preserving ratio.
fn sharpen_plane(
    plane: &[[f32; 3]],
    width: usize,
    height: usize,
    sharp: &lumina_sidecar::Sharpening,
    effective_scale: f32,
) -> Vec<[f32; 3]> {
    if sharp.amount == 0.0 {
        return plane.to_vec();
    }
    let lum: Vec<f32> = plane.iter().map(|p| luma(*p)).collect();
    let fine = blur(
        &lum,
        width,
        height,
        (sharp.radius * 0.5).max(0.5),
        effective_scale,
    );
    let coarse = blur(
        &lum,
        width,
        height,
        (sharp.radius * 1.5).max(0.5),
        effective_scale,
    );
    let mut gradients = vec![0.0_f32; lum.len()];
    let mut maxg: f32 = 0.0;
    for y in 0..height {
        for x in 0..width {
            let gx = lum[y * width + (x as i32 + 1).min(width as i32 - 1) as usize]
                - lum[y * width + x.saturating_sub(1)];
            let gy = lum[(y as i32 + 1).min(height as i32 - 1) as usize * width + x]
                - lum[y.saturating_sub(1) * width + x];
            gradients[y * width + x] = gx.abs() + gy.abs();
            maxg = maxg.max(gradients[y * width + x]);
        }
    }
    plane
        .iter()
        .enumerate()
        .map(|(index, p)| {
            let d = sharp.detail * (lum[index] - fine[index])
                + (1.0 - sharp.detail) * (lum[index] - coarse[index]);
            let edge = if maxg > 0.0 {
                (gradients[index] / maxg).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let amount = sharp.amount * ((1.0 - sharp.masking) + sharp.masking * edge);
            let ny = (lum[index] + amount * d).clamp(0.0, 255.0);
            let ratio = if lum[index] > 1e-6 {
                ny / lum[index]
            } else {
                0.0
            };
            [p[0] * ratio, p[1] * ratio, p[2] * ratio]
        })
        .collect()
}

/// The layer's one and only RGBA8 write: round and clamp, alpha untouched.
fn quantise(plane: &[[f32; 3]], pixels: &[u8]) -> Vec<u8> {
    plane
        .iter()
        .zip(pixels.as_chunks::<4>().0)
        .flat_map(|(p, original)| {
            [
                (f64::from(p[0])).round().clamp(0.0, 255.0) as u8,
                (f64::from(p[1])).round().clamp(0.0, 255.0) as u8,
                (f64::from(p[2])).round().clamp(0.0, 255.0) as u8,
                original[3],
            ]
        })
        .collect()
}

/// The four chains of [`Chains`], all re-derived from the documented formulas.
///
/// A recipe without a neutral-able detail block still works: the two detail
/// passes are simply skipped, so `documented` and `swapped` both collapse onto
/// `colour_only`.
pub(super) fn independent_chains(
    recipe: &LocalAdjustments,
    pixels: &[u8],
    width: usize,
    height: usize,
    effective_scale: f32,
) -> Chains {
    let prefix = prefix_plane(recipe, pixels);
    independent_chains_from(recipe, pixels, width, height, effective_scale, prefix)
}

/// [`independent_chains`] with the caller supplying the prefix plane, for the one
/// stage this file deliberately does not re-transcribe: the shared presence
/// stage, whose maths is already pinned by the P1.2c goldens. Passing the prefix
/// in keeps the rest of the chain — the curve, the four colour stages, the
/// bilateral kernel, the Gaussian and the single final round — independent.
pub(super) fn independent_chains_from(
    recipe: &LocalAdjustments,
    pixels: &[u8],
    width: usize,
    height: usize,
    effective_scale: f32,
    prefix: Vec<[f32; 3]>,
) -> Chains {
    let colour = colour_plane(recipe, &prefix);
    let detail = |plane: &[[f32; 3]]| -> Vec<[f32; 3]> {
        let mut plane = plane.to_vec();
        if let Some(noise) = recipe
            .detail
            .as_ref()
            .and_then(|d| d.noise_reduction)
            .filter(|noise| noise.luminance != 0.0 || noise.color != 0.0)
        {
            plane = noise_plane(&plane, width, height, &noise);
        }
        if let Some(sharp) = recipe
            .detail
            .as_ref()
            .and_then(|d| d.sharpening)
            .filter(|sharp| sharp.amount != 0.0)
        {
            plane = sharpen_plane(&plane, width, height, &sharp, effective_scale);
        }
        plane
    };
    // The colour half on its own, for the isolated validation.
    let colour_only = quantise(&colour, pixels);
    // Documented: colour first, then the neighbourhood stages.
    let documented = quantise(&detail(&colour), pixels);
    // The order the implementation must NOT have: the neighbourhood stages see
    // the WB + Basic prefix, and the colour block runs on their result.
    let swapped = quantise(&colour_plane(recipe, &detail(&prefix)), pixels);
    // The documented order with a second, forbidden quantization boundary
    // between the colour block and the noise reduction.
    let rounded: Vec<[f32; 3]> = colour
        .iter()
        .map(|p| {
            let mut row = [0.0_f32; 3];
            for (c, slot) in row.iter_mut().enumerate() {
                *slot = f32::from(p[c].round().clamp(0.0, 255.0) as u8);
            }
            row
        })
        .collect();
    let early = quantise(&detail(&rounded), pixels);
    Chains {
        colour_only,
        documented,
        swapped,
        early,
    }
}

/// The L1 distance between two byte strings, used for the "the order is
/// observable" non-vacuity margin.
pub(super) fn l1(left: &[u8], right: &[u8]) -> u64 {
    left.iter()
        .zip(right)
        .map(|(a, b)| u64::from(a.abs_diff(*b)))
        .sum()
}
