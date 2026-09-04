//! Deterministic color/luminance range mask stages (G-03 Masking parity).
//!
//! A range prompt is a *recipe stage*, not an AI matte: the output plane is a
//! pure function of the source pixels plus the validated parameters. No model,
//! no cache, no RNG, no wall-clock — two evaluations of identical inputs are
//! byte-identical, and a parameter change implicitly yields a new plane (no
//! stale/missing lifecycle). Out-of-range parameters are loud errors, never
//! silently clamped.
//!
//! * Luminance uses Rec.709 weights on sRGB-encoded RGBA8, alpha ignored.
//! * Color converts deterministically to HSL (formula below) and multiplies
//!   three trapezoid factors (hue circle distance, saturation box,
//!   luminance box).
//! * A trapezoid `trapezoid(v, lo, hi, ramp)` is `0` outside `[lo, hi]`,
//!   `1` inside `[lo + ramp, hi - ramp]` and linear on the ramps, with
//!   `ramp = feather * (hi - lo) / 2` (`feather = 0` is a hard cut).

use crate::masks::{MaskError, MaskPlane};
use crate::ImageFrame;
use lumina_sidecar::MaskPrompt;

/// True for the deterministic recipe stages ([`MaskPrompt::ColorRange`] /
/// [`MaskPrompt::LuminanceRange`]); false for geometry prompts and `None`.
pub fn is_range_prompt(prompt: Option<&MaskPrompt>) -> bool {
    matches!(
        prompt,
        Some(MaskPrompt::ColorRange { .. } | MaskPrompt::LuminanceRange { .. })
    )
}

/// Evaluate a range prompt against `frame`. Returns
/// [`MaskError::NotRangePrompt`] for non-range prompts and
/// [`MaskError::InvalidRange`] for out-of-range parameters (loud, no
/// clamping).
pub fn evaluate_range_prompt(
    frame: &ImageFrame,
    prompt: &MaskPrompt,
) -> Result<MaskPlane, MaskError> {
    match prompt {
        MaskPrompt::LuminanceRange {
            min, max, feather, ..
        } => luminance_plane(frame, *min, *max, *feather),
        MaskPrompt::ColorRange {
            hue_center,
            hue_width,
            sat_min,
            sat_max,
            lum_min,
            lum_max,
            feather,
            ..
        } => color_range_plane(
            frame,
            *hue_center,
            *hue_width,
            *sat_min,
            *sat_max,
            *lum_min,
            *lum_max,
            *feather,
        ),
        _ => Err(MaskError::NotRangePrompt),
    }
}

/// Rec.709 luminance of sRGB-encoded bytes in `0..=1`, alpha ignored.
pub fn srgb_luminance(r: u8, g: u8, b: u8) -> f32 {
    (0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)) / 255.0
}

/// Deterministic RGB→HSL. `h` in `0..360`, `s`/`l` in `0..=1`. Achromatic
/// pixels map to `h = 0, s = 0`.
pub fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = f32::from(r) / 255.0;
    let g = f32::from(g) / 255.0;
    let b = f32::from(b) / 255.0;
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let l = (mx + mn) / 2.0;
    if mx == mn {
        return (0.0, 0.0, l);
    }
    let d = mx - mn;
    let s = if l > 0.5 {
        d / (2.0 - mx - mn)
    } else {
        d / (mx + mn)
    };
    let mut h = if mx == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h *= 60.0;
    if h < 0.0 {
        h += 360.0;
    }
    (h, s.clamp(0.0, 1.0), l.clamp(0.0, 1.0))
}

/// Trapezoid factor: `0` strictly outside `[lo, hi]`, `1` on the plateau,
/// linear on `ramp`-wide flanks. Endpoints belong to the flanks (`0` at the
/// exact edge for a feathered band, `1` for a hard cut); `span == 0` is an
/// exact delta (`1` iff `v == lo`).
fn trapezoid(v: f32, lo: f32, hi: f32, ramp: f32) -> f32 {
    if v < lo || v > hi {
        return 0.0;
    }
    if ramp <= 0.0 || lo == hi {
        return 1.0;
    }
    let rise = (v - lo) / ramp;
    let fall = (hi - v) / ramp;
    rise.min(fall).clamp(0.0, 1.0)
}

fn check_frame(frame: &ImageFrame) -> Result<(), MaskError> {
    if frame.width == 0 || frame.height == 0 {
        return Err(MaskError::InvalidPlane {
            width: frame.width,
            height: frame.height,
            length: frame.pixels.len(),
        });
    }
    let expected = (frame.width as usize).saturating_mul(frame.height as usize) * 4;
    if frame.pixels.len() != expected {
        return Err(MaskError::InvalidPlane {
            width: frame.width,
            height: frame.height,
            length: frame.pixels.len(),
        });
    }
    let budget = crate::memory::MemoryBudget::from_env();
    budget
        .check_mask(frame.width as u64, frame.height as u64)
        .map_err(|error| MaskError::MemoryBudgetExceeded {
            required: error.required(),
            limit: error.limit(),
        })?;
    Ok(())
}

fn unit(name: &str, value: f32) -> Result<f32, MaskError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(MaskError::InvalidRange {
            reason: format!("{name} must be finite within 0..=1, got {value}"),
        });
    }
    Ok(value)
}

/// Luminance-range plane at frame resolution.
pub fn luminance_plane(
    frame: &ImageFrame,
    min: f32,
    max: f32,
    feather: f32,
) -> Result<MaskPlane, MaskError> {
    let min = unit("min", min)?;
    let max = unit("max", max)?;
    let feather = unit("feather", feather)?;
    if min > max {
        return Err(MaskError::InvalidRange {
            reason: format!("min ({min}) must not exceed max ({max})"),
        });
    }
    check_frame(frame)?;
    let ramp = feather * (max - min) / 2.0;
    let (chunks, _) = frame.pixels.as_chunks::<4>();
    let values = chunks
        .iter()
        .map(|px| {
            let lum = srgb_luminance(px[0], px[1], px[2]);
            (trapezoid(lum, min, max, ramp) * (u16::MAX as f32) + 0.5) as u16
        })
        .collect();
    MaskPlane::new(frame.width, frame.height, values)
}

/// Color-range plane at frame resolution.
#[allow(clippy::too_many_arguments)]
pub fn color_range_plane(
    frame: &ImageFrame,
    hue_center: f32,
    hue_width: f32,
    sat_min: f32,
    sat_max: f32,
    lum_min: f32,
    lum_max: f32,
    feather: f32,
) -> Result<MaskPlane, MaskError> {
    if !hue_center.is_finite() || !(0.0..=360.0).contains(&hue_center) {
        return Err(MaskError::InvalidRange {
            reason: format!("hue_center must be finite within 0..=360, got {hue_center}"),
        });
    }
    if !hue_width.is_finite() || !(0.0..=360.0).contains(&hue_width) {
        return Err(MaskError::InvalidRange {
            reason: format!("hue_width must be finite within 0..=360, got {hue_width}"),
        });
    }
    let sat_min = unit("sat_min", sat_min)?;
    let sat_max = unit("sat_max", sat_max)?;
    let lum_min = unit("lum_min", lum_min)?;
    let lum_max = unit("lum_max", lum_max)?;
    let feather = unit("feather", feather)?;
    if sat_min > sat_max {
        return Err(MaskError::InvalidRange {
            reason: format!("sat_min ({sat_min}) must not exceed sat_max ({sat_max})"),
        });
    }
    if lum_min > lum_max {
        return Err(MaskError::InvalidRange {
            reason: format!("lum_min ({lum_min}) must not exceed lum_max ({lum_max})"),
        });
    }
    check_frame(frame)?;
    let half = hue_width / 2.0;
    let hue_ramp = feather * half;
    let sat_ramp = feather * (sat_max - sat_min) / 2.0;
    let lum_ramp = feather * (lum_max - lum_min) / 2.0;
    let (chunks, _) = frame.pixels.as_chunks::<4>();
    let values = chunks
        .iter()
        .map(|px| {
            let (h, s, l) = rgb_to_hsl(px[0], px[1], px[2]);
            let hue_factor = if half == 0.0 {
                // Exact hue pin: only the centre hue passes.
                if circle_distance(h, hue_center) == 0.0 {
                    1.0
                } else {
                    0.0
                }
            } else if hue_ramp <= 0.0 {
                if circle_distance(h, hue_center) <= half {
                    1.0
                } else {
                    0.0
                }
            } else {
                let dh = circle_distance(h, hue_center);
                if dh >= half {
                    0.0
                } else if dh <= half - hue_ramp {
                    1.0
                } else {
                    (half - dh) / hue_ramp
                }
            };
            let sat_factor = trapezoid(s, sat_min, sat_max, sat_ramp);
            let lum_factor = trapezoid(l, lum_min, lum_max, lum_ramp);
            ((hue_factor * sat_factor * lum_factor) * (u16::MAX as f32) + 0.5) as u16
        })
        .collect();
    MaskPlane::new(frame.width, frame.height, values)
}

/// Shortest circle distance of two hues in degrees (`0..=180`).
fn circle_distance(a: f32, b: f32) -> f32 {
    let mut dh = (a - b).abs() % 360.0;
    if dh > 180.0 {
        dh = 360.0 - dh;
    }
    dh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(pixels: &[[u8; 4]], width: u32, height: u32) -> ImageFrame {
        ImageFrame {
            width,
            height,
            pixels: pixels.iter().flatten().copied().collect(),
        }
    }

    #[test]
    fn luminance_black_white_endpoints() {
        let f = frame(&[[0, 0, 0, 255], [255, 255, 255, 255]], 2, 1);
        let plane = luminance_plane(&f, 0.5, 1.0, 0.0).unwrap();
        assert_eq!(plane.values, vec![0, u16::MAX]);
    }

    #[test]
    fn luminance_hard_cut_is_binary() {
        // Mid grey must be fully inside or outside a hard band — never feathered.
        let f = frame(&[[128, 128, 128, 255]], 1, 1);
        let inside = luminance_plane(&f, 0.0, 1.0, 0.0).unwrap();
        assert_eq!(inside.values, vec![u16::MAX]);
        let outside = luminance_plane(&f, 0.6, 1.0, 0.0).unwrap();
        assert_eq!(outside.values, vec![0]);
    }

    #[test]
    fn luminance_feather_ramps_monotonically() {
        // A luminance staircase must rise then fall symmetrically.
        let gradient: Vec<[u8; 4]> = (0..=10).map(|i| [i * 25, i * 25, i * 25, 255]).collect();
        let f = frame(&gradient, 11, 1);
        let plane = luminance_plane(&f, 0.2, 0.8, 0.5).unwrap();
        // Peak at the centre, zeros at both ends.
        assert_eq!(plane.values[0], 0);
        assert_eq!(plane.values[10], 0);
        let peak = *plane.values.iter().max().unwrap();
        assert_eq!(peak, u16::MAX);
        // Rise side is monotone non-decreasing up to the peak index.
        let peak_idx = plane.values.iter().position(|v| *v == peak).unwrap();
        for pair in plane.values[..=peak_idx].windows(2) {
            assert!(pair[0] <= pair[1], "rise must be monotone: {pair:?}");
        }
        for pair in plane.values[peak_idx..].windows(2) {
            assert!(pair[0] >= pair[1], "fall must be monotone: {pair:?}");
        }
    }

    #[test]
    fn luminance_degenerate_span_is_delta() {
        let f = frame(&[[0, 0, 0, 255], [255, 255, 255, 255]], 2, 1);
        let plane = luminance_plane(&f, 0.0, 0.0, 0.0).unwrap();
        assert_eq!(plane.values, vec![u16::MAX, 0]);
    }

    #[test]
    fn luminance_rejects_bad_params_loudly() {
        let f = frame(&[[0, 0, 0, 255]], 1, 1);
        assert!(luminance_plane(&f, 0.8, 0.2, 0.0).is_err());
        assert!(luminance_plane(&f, f32::NAN, 1.0, 0.0).is_err());
        assert!(luminance_plane(&f, 0.0, 1.0, 2.0).is_err());
    }

    #[test]
    fn color_red_pin_selects_only_red() {
        let f = frame(
            &[
                [255, 0, 0, 255],
                [0, 255, 0, 255],
                [0, 0, 255, 255],
                [128, 128, 128, 255],
            ],
            4,
            1,
        );
        let plane = color_range_plane(&f, 0.0, 60.0, 0.5, 1.0, 0.0, 1.0, 0.0).unwrap();
        assert_eq!(plane.values[0], u16::MAX);
        assert_eq!(plane.values[1], 0);
        assert_eq!(plane.values[2], 0);
        // Achromatic grey has saturation 0 and is excluded by sat_min 0.5.
        assert_eq!(plane.values[3], 0);
    }

    #[test]
    fn color_hue_wraps_around_360() {
        // hue_center 350 with width 40 must catch hue 0 (red) and 350-ish magenta.
        let f = frame(
            &[[255, 0, 0, 255], [255, 0, 128, 255], [0, 255, 0, 255]],
            3,
            1,
        );
        let plane = color_range_plane(&f, 350.0, 60.0, 0.5, 1.0, 0.0, 1.0, 0.0).unwrap();
        assert_eq!(plane.values[0], u16::MAX);
        assert!(plane.values[1] > 0, "magenta near 350 must pass");
        assert_eq!(plane.values[2], 0);
    }

    #[test]
    fn color_rejects_bad_params_loudly() {
        let f = frame(&[[0, 0, 0, 255]], 1, 1);
        assert!(color_range_plane(&f, 400.0, 60.0, 0.0, 1.0, 0.0, 1.0, 0.0).is_err());
        assert!(color_range_plane(&f, 0.0, 60.0, 0.9, 0.1, 0.0, 1.0, 0.0).is_err());
        assert!(color_range_plane(&f, 0.0, 60.0, 0.0, 1.0, 0.0, 1.0, f32::INFINITY).is_err());
    }

    #[test]
    fn range_prompt_dispatch_and_non_range_rejection() {
        use lumina_sidecar::{MaskPrompt, PromptTransform};
        let f = frame(&[[255, 255, 255, 255]], 1, 1);
        let range = MaskPrompt::LuminanceRange {
            min: 0.0,
            max: 1.0,
            feather: 0.0,
            transformation: PromptTransform::default(),
        };
        assert!(is_range_prompt(Some(&range)));
        assert_eq!(
            evaluate_range_prompt(&f, &range).unwrap().values,
            vec![u16::MAX]
        );
        let geo = MaskPrompt::Box {
            rect: lumina_sidecar::NormalizedRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            transformation: PromptTransform::default(),
        };
        assert!(!is_range_prompt(Some(&geo)));
        assert!(!is_range_prompt(None));
        assert_eq!(
            evaluate_range_prompt(&f, &geo),
            Err(MaskError::NotRangePrompt)
        );
    }

    #[test]
    fn rgb_to_hsl_primaries_are_exact() {
        assert_eq!(rgb_to_hsl(255, 0, 0), (0.0, 1.0, 0.5));
        assert_eq!(rgb_to_hsl(0, 255, 0), (120.0, 1.0, 0.5));
        assert_eq!(rgb_to_hsl(0, 0, 255), (240.0, 1.0, 0.5));
        assert_eq!(rgb_to_hsl(128, 128, 128), (0.0, 0.0, 128.0 / 255.0));
    }

    #[test]
    fn determinism_byte_identical_reruns() {
        let pixels: Vec<[u8; 4]> = (0..64).map(|i| [i, 255 - i, i / 2, 255]).collect();
        let f = frame(&pixels, 8, 8);
        let a = color_range_plane(&f, 120.0, 90.0, 0.1, 0.9, 0.1, 0.9, 0.5).unwrap();
        let b = color_range_plane(&f, 120.0, 90.0, 0.1, 0.9, 0.1, 0.9, 0.5).unwrap();
        assert_eq!(a, b);
        let c = luminance_plane(&f, 0.2, 0.8, 0.3).unwrap();
        let d = luminance_plane(&f, 0.2, 0.8, 0.3).unwrap();
        assert_eq!(c, d);
    }
}
