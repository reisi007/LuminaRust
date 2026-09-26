//! The per-pixel colour stages of the global recipe, and their `u8` wrappers.
//!
//! # Why these are separate from their wrappers
//!
//! Each colour stage is a pure function from one normalized (`0..=1`) `f32`
//! RGB triple to another. The global kernel calls it once per pixel and
//! immediately rounds the result back to `u8` (it has to: its input frame is a
//! `u8` buffer). The mask-local MASK-LOCAL-P1.2b chain in
//! [`crate::render::local_color`] calls **the same functions** and then rounds
//! exactly once, at the very end of its whole float chain.
//!
//! Keeping one implementation is what makes "a local colour stage is the global
//! stage at a different place in the chain" a structural property instead of
//! two kernels that have to be kept in agreement by hand. Nothing here knows
//! about masks; the local chain owns the ordering, the single quantization
//! boundary and the fractional mask blend.
//!
//! A stage that cannot change its pixel returns `None` (or, for
//! vibrance/saturation, is simply never called), so an unrelated selection
//! never round-trips a byte the kernel did not intend to touch.

use crate::{for_each_rgba_mut, CoreError};

/// sRGB → HSL. A grey input has hue `0` and saturation `0` by definition.
pub(crate) fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let mut h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    if h < 0.0 {
        h += 360.0
    }
    (h, s, l)
}

/// HSL → sRGB.
pub(crate) fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let q = if h < 60.0 {
        [c, x, 0.0]
    } else if h < 120.0 {
        [x, c, 0.0]
    } else if h < 180.0 {
        [0.0, c, x]
    } else if h < 240.0 {
        [0.0, x, c]
    } else if h < 300.0 {
        [x, 0.0, c]
    } else {
        [c, 0.0, x]
    };
    [q[0] + m, q[1] + m, q[2] + m]
}

/// The HSL stage for **one** pixel, on normalized (`0..=1`) values.
///
/// `None` means *no stored band overlaps this pixel's hue*, i.e. the stage is a
/// literal no-op. Both callers honour that.
pub(crate) fn hsl_stage(rgb: [f32; 3], h: &lumina_sidecar::HslAdjustments) -> Option<[f32; 3]> {
    let channels = [
        h.red, h.orange, h.yellow, h.green, h.cyan, h.blue, h.violet, h.magenta,
    ];
    // These are deliberately not an evenly spaced `i * 30` sequence: green
    // through blue use the conventional Lightroom-like 60 degree sectors,
    // while violet and magenta remain distinct adjacent controls.
    const CENTERS: [f32; 8] = [0.0, 30.0, 60.0, 120.0, 180.0, 240.0, 270.0, 300.0];
    let (mut hue, mut sat, mut l) = rgb_to_hsl(rgb[0], rgb[1], rgb[2]);
    let mut dh = 0.0;
    let mut ds = 0.0;
    let mut dl = 0.0;
    let weights: [f32; 8] = CENTERS.map(|center| {
        let i = CENTERS.iter().position(|&c| c == center).unwrap();
        let previous = if i == 0 {
            360.0 - CENTERS[7]
        } else {
            center - CENTERS[i - 1]
        };
        let next = if i + 1 == CENTERS.len() {
            360.0 - center + CENTERS[0]
        } else {
            CENTERS[i + 1] - center
        };
        // Piecewise-linear cyclic triangle: the weight reaches zero at
        // each neighbouring centre and is one at this centre.
        let clockwise = (hue - center).rem_euclid(360.0);
        let counterclockwise = (center - hue).rem_euclid(360.0);
        if clockwise <= next {
            1.0 - clockwise / next
        } else if counterclockwise <= previous {
            1.0 - counterclockwise / previous
        } else {
            0.0
        }
    });
    let weight_sum: f32 = weights.iter().sum();
    if weight_sum <= f32::EPSILON {
        return None;
    }
    for (i, channel) in channels.iter().enumerate() {
        let w = weights[i] / weight_sum;
        if let Some(channel) = channel {
            dh += channel.hue * 30.0 * w;
            ds += channel.saturation * w;
            dl += channel.luminance * w;
        }
    }
    hue = (hue + dh).rem_euclid(360.0);
    sat = (sat + ds).clamp(0.0, 1.0);
    l = (l + dl).clamp(0.0, 1.0);
    Some(hsl_to_rgb(hue, sat, l))
}

/// The Point Color stage for **one** pixel, on normalized values.
///
/// `None` means no entry's hue selection reached this pixel, so the stage is a
/// literal no-op.
pub(crate) fn point_color_stage(
    rgb: [f32; 3],
    point_color: &lumina_sidecar::PointColor,
) -> Option<[f32; 3]> {
    let (mut hue, mut sat, mut light) = rgb_to_hsl(rgb[0], rgb[1], rgb[2]);
    let mut touched = false;
    for entry in &point_color.entries {
        let distance = (hue - entry.hue_center)
            .rem_euclid(360.0)
            .min((entry.hue_center - hue).rem_euclid(360.0));
        let weight = if entry.hue_range <= f32::EPSILON {
            if distance <= f32::EPSILON {
                1.0
            } else {
                0.0
            }
        } else if distance >= entry.hue_range {
            0.0
        } else {
            1.0 - distance / entry.hue_range
        };
        if weight <= f32::EPSILON {
            continue;
        }
        touched = true;
        hue = (hue + entry.hue_shift * 30.0 * weight).rem_euclid(360.0);
        sat = (sat + entry.saturation_shift * weight).clamp(0.0, 1.0);
        light = (light + entry.luminance_shift * weight).clamp(0.0, 1.0);
    }
    if !touched {
        return None;
    }
    Some(hsl_to_rgb(hue, sat, light))
}

/// The vibrance/saturation stage for **one** pixel, on normalized values.
pub(crate) fn vibrance_saturation_stage(rgb: [f32; 3], vibrance: f32, saturation: f32) -> [f32; 3] {
    let (hue, mut sat, lightness) = rgb_to_hsl(rgb[0], rgb[1], rgb[2]);
    if vibrance != 0.0 {
        // Skin protection is 0 in the soft core [15°,55°], ramps linearly
        // to 1 in [5°,15°] and [55°,65°], and is 1 outside those ramps.
        let skin_protection = if !(5.0..=65.0).contains(&hue) {
            1.0
        } else if hue < 15.0 {
            (15.0 - hue) / 10.0
        } else if hue <= 55.0 {
            0.0
        } else {
            (hue - 55.0) / 10.0
        };
        // The low-saturation factor protects already vivid colours. For a
        // negative value, multiplying by sat also avoids a linear desaturator.
        let protection = (1.0 - sat) * skin_protection;
        let direction_weight = if vibrance >= 0.0 { 1.0 - sat } else { sat };
        sat = (sat + vibrance * protection * direction_weight).clamp(0.0, 1.0);
    }
    sat = (sat * (1.0 + saturation)).clamp(0.0, 1.0);
    hsl_to_rgb(hue, sat, lightness)
}

/// The color-grading stage for **one** pixel, on normalized values.
pub(crate) fn color_grading_stage(
    rgb: [f32; 3],
    grading: &lumina_sidecar::ColorGrading,
) -> [f32; 3] {
    // Positive balance moves both transition points downward (0.15 max): the
    // highlight region expands toward shadows, matching Lightroom's direction.
    // `blending == 0.5` reproduces the pre-refinement edges exactly; higher
    // blending widens the midtones symmetrically.
    let shadow_edge = 0.65 - grading.balance * 0.15 + (grading.blending - 0.5) * 0.2;
    let highlight_edge = 0.35 - grading.balance * 0.15 - (grading.blending - 0.5) * 0.2;
    let apply_luminance = grading.shadows.luminance != 0.0
        || grading.midtones.luminance != 0.0
        || grading.highlights.luminance != 0.0;
    let smooth = |edge: f32, value: f32| {
        let t = (value / edge).clamp(0.0, 1.0);
        1.0 - t * t * (3.0 - 2.0 * t)
    };
    let luminance = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
    let shadow = smooth(shadow_edge, luminance);
    let highlight = {
        let t = ((luminance - highlight_edge) / (1.0 - highlight_edge)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let midtone = (1.0 - shadow - highlight).max(0.0);
    let sum = shadow + midtone + highlight;
    let weights = [shadow / sum, midtone / sum, highlight / sum];
    let ranges = [grading.shadows, grading.midtones, grading.highlights];
    let mut output = rgb;
    for (weight, range) in weights.into_iter().zip(ranges) {
        if range.saturation == 0.0 || weight == 0.0 {
            continue;
        }
        // Tint is the fully saturated HSL colour at L=0.5. Mixing is
        // channel-wise: x' = x + (tint - x) * weight * saturation.
        let tint = hsl_to_rgb(range.hue_degrees.rem_euclid(360.0), 1.0, 0.5);
        let amount = weight * range.saturation;
        for channel in 0..3 {
            output[channel] += (tint[channel] - output[channel]) * amount;
        }
    }
    if apply_luminance {
        let lum_shift = weights[0] * grading.shadows.luminance
            + weights[1] * grading.midtones.luminance
            + weights[2] * grading.highlights.luminance;
        let (hue, sat, light) = rgb_to_hsl(output[0], output[1], output[2]);
        output = hsl_to_rgb(hue, sat, (light + lum_shift).clamp(0.0, 1.0));
    }
    output
}

/// Global HSL stage over a whole `u8` frame. Validation happens in
/// `validate_nested_adjustments`; the stage itself cannot fail.
pub(crate) fn apply_hsl(
    pixels: &mut [u8],
    h: &lumina_sidecar::HslAdjustments,
) -> Result<(), CoreError> {
    for_each_rgba_mut(pixels, |px| {
        let Some(rgb) = hsl_stage(
            [
                px[0] as f32 / 255.0,
                px[1] as f32 / 255.0,
                px[2] as f32 / 255.0,
            ],
            h,
        ) else {
            return;
        };
        px[0] = (rgb[0] * 255.0).round() as u8;
        px[1] = (rgb[1] * 255.0).round() as u8;
        px[2] = (rgb[2] * 255.0).round() as u8;
    });
    Ok(())
}

/// F-090b Point Color over a whole `u8` frame: targeted color selection with a
/// free hue center. Each entry weights pixels by a cyclic triangular function
/// of the hue distance to `hue_center` (`1` at the center, linearly to `0` at
/// `hue_range`; `hue_range == 0` matches only the exact center hue) and applies
/// its shifts weighted: hue rotation (`hue_shift * 30°`), additive saturation
/// and luminance. Entries apply sequentially in list order in sRGB-codified
/// HSL; all-zero shifts are identity. Outputs clip to `0..=1`.
pub(crate) fn apply_point_color(pixels: &mut [u8], point_color: &lumina_sidecar::PointColor) {
    if point_color.entries.is_empty() {
        return;
    }
    for_each_rgba_mut(pixels, |px| {
        let Some(rgb) = point_color_stage(
            [
                px[0] as f32 / 255.0,
                px[1] as f32 / 255.0,
                px[2] as f32 / 255.0,
            ],
            point_color,
        ) else {
            return;
        };
        px[0] = (rgb[0] * 255.0).round().clamp(0.0, 255.0) as u8;
        px[1] = (rgb[1] * 255.0).round().clamp(0.0, 255.0) as u8;
        px[2] = (rgb[2] * 255.0).round().clamp(0.0, 255.0) as u8;
    });
}

/// F-092 vibrance (selective, skin-protected) followed by the global saturation
/// scale over a whole `u8` frame.
pub(crate) fn apply_vibrance_and_saturation(
    pixels: &mut [u8],
    vibrance: Option<&f64>,
    saturation: Option<&f64>,
) {
    if vibrance.is_none() && saturation.is_none() {
        return;
    }
    let vibrance = vibrance.copied().unwrap_or(0.0) as f32;
    let saturation = saturation.copied().unwrap_or(0.0) as f32;
    for_each_rgba_mut(pixels, |px| {
        let rgb = vibrance_saturation_stage(
            [
                px[0] as f32 / 255.0,
                px[1] as f32 / 255.0,
                px[2] as f32 / 255.0,
            ],
            vibrance,
            saturation,
        );
        px[0] = (rgb[0] * 255.0).round() as u8;
        px[1] = (rgb[1] * 255.0).round() as u8;
        px[2] = (rgb[2] * 255.0).round() as u8;
    });
}

/// Color Grading over a whole `u8` frame: three range-weighted tints plus the
/// optional additive luminance offset of each range.
pub(crate) fn apply_color_grading(pixels: &mut [u8], grading: &lumina_sidecar::ColorGrading) {
    for_each_rgba_mut(pixels, |px| {
        let output = color_grading_stage(
            [
                px[0] as f32 / 255.0,
                px[1] as f32 / 255.0,
                px[2] as f32 / 255.0,
            ],
            grading,
        );
        for channel in 0..3 {
            px[channel] = (output[channel].clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    });
}
