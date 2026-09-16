//! Raw Stage-1 measurements over a decoded RGBA8 frame.
//!
//! Every measurement is deterministic, allocation-bounded and free of wall
//! clock/randomness. Exposure/clipping reuses the existing core tone/histogram
//! pass ([`lumina_core::analyze_tone_with_histogram`]) instead of a second
//! algorithm (decisions §4).

use lumina_core::{analyze_tone_with_histogram, ImageFrame};

use crate::CullError;

/// Rec.709 luminance weights for the sRGB-encoded MVP domain. Kept identical
/// to `lumina_core`'s canonical measurement so both paths agree.
const REC709_WEIGHTS: [f64; 3] = [0.2126, 0.7152, 0.0722];

/// A single-channel luminance plane in `0..=1`, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct LumaPlane {
    /// Plane width in pixels.
    pub width: u32,
    /// Plane height in pixels.
    pub height: u32,
    /// Row-major luminance samples in `0..=1`.
    pub values: Vec<f32>,
}

impl LumaPlane {
    /// Sample at `(x, y)`; `(x, y)` must be inside the plane.
    #[inline]
    #[must_use]
    pub fn at(&self, x: u32, y: u32) -> f32 {
        self.values[(y * self.width + x) as usize]
    }
}

/// Builds the Rec.709 luminance plane of a frame. Alpha is ignored. An empty
/// frame is a loud error, not a zero measurement.
pub fn luma_plane(frame: &ImageFrame) -> Result<LumaPlane, CullError> {
    if frame.width == 0 || frame.height == 0 {
        return Err(CullError::EmptyFrame {
            width: frame.width,
            height: frame.height,
        });
    }
    let values = frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| {
            ((REC709_WEIGHTS[0] * f64::from(pixel[0])
                + REC709_WEIGHTS[1] * f64::from(pixel[1])
                + REC709_WEIGHTS[2] * f64::from(pixel[2]))
                / 255.0) as f32
        })
        .collect();
    Ok(LumaPlane {
        width: frame.width,
        height: frame.height,
        values,
    })
}

/// Separable `[1, 2, 1] / 4` blur with replicated borders. Used to suppress
/// pixel-level noise before the structural sharpness measurement, so a noisy
/// image does not read as "sharp" (noise is measured separately).
#[must_use]
pub fn blur_3x3(plane: &LumaPlane) -> LumaPlane {
    let (width, height) = (plane.width, plane.height);
    if width == 0 || height == 0 {
        return plane.clone();
    }
    let mut horizontal = vec![0f32; plane.values.len()];
    for y in 0..height {
        for x in 0..width {
            let left = plane.at(x.saturating_sub(1), y);
            let center = plane.at(x, y);
            let right = plane.at((x + 1).min(width - 1), y);
            horizontal[(y * width + x) as usize] = (left + 2.0 * center + right) * 0.25;
        }
    }
    let mut values = vec![0f32; plane.values.len()];
    for y in 0..height {
        for x in 0..width {
            let index = |yy: u32| horizontal[(yy * width + x) as usize];
            let up = index(y.saturating_sub(1));
            let center = index(y);
            let down = index((y + 1).min(height - 1));
            values[(y * width + x) as usize] = (up + 2.0 * center + down) * 0.25;
        }
    }
    LumaPlane {
        width,
        height,
        values,
    }
}

/// Structural gradient statistics over the interior of a plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStats {
    /// Mean squared horizontal central difference.
    pub mean_grad_x2: f64,
    /// Mean squared vertical central difference.
    pub mean_grad_y2: f64,
    /// `mean_grad_x2 + mean_grad_y2` (Tenengrad-style structural energy).
    pub tenengrad: f64,
    /// Directional sharpness `2 * min(mean_grad_x2, mean_grad_y2)`: structural
    /// detail is limited by the weakest direction, so a one-directional smear
    /// reads as low sharpness (motion blur) even though the summed energy
    /// stays high. Documented heuristic limitation: content that is
    /// genuinely one-directional (e.g. pure vertical stripes) is treated as
    /// less sharp.
    pub directional_sharpness: f64,
    /// Variance of the 4-neighbour Laplacian (a classic focus measure).
    pub laplacian_variance: f64,
    /// `|Ex - Ey| / (Ex + Ey)`, `0` for isotropic structure and for a flat
    /// plane. High values indicate one smeared direction (motion blur).
    pub anisotropy: f64,
}

/// Computes [`GradientStats`] on the interior pixels. Planes smaller than
/// `3x3` have no interior and report all-zero statistics.
#[must_use]
pub fn gradient_stats(plane: &LumaPlane) -> GradientStats {
    let (width, height) = (plane.width, plane.height);
    let zero = GradientStats {
        mean_grad_x2: 0.0,
        mean_grad_y2: 0.0,
        tenengrad: 0.0,
        directional_sharpness: 0.0,
        laplacian_variance: 0.0,
        anisotropy: 0.0,
    };
    if width < 3 || height < 3 {
        return zero;
    }
    let mut sum_x2 = 0.0f64;
    let mut sum_y2 = 0.0f64;
    let mut sum_lap = 0.0f64;
    let mut sum_lap2 = 0.0f64;
    let mut count = 0u64;
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let left = f64::from(plane.at(x - 1, y));
            let right = f64::from(plane.at(x + 1, y));
            let up = f64::from(plane.at(x, y - 1));
            let down = f64::from(plane.at(x, y + 1));
            let center = f64::from(plane.at(x, y));
            let grad_x = (right - left) * 0.5;
            let grad_y = (down - up) * 0.5;
            sum_x2 += grad_x * grad_x;
            sum_y2 += grad_y * grad_y;
            let laplacian = 4.0 * center - left - right - up - down;
            sum_lap += laplacian;
            sum_lap2 += laplacian * laplacian;
            count += 1;
        }
    }
    if count == 0 {
        return zero;
    }
    let n = count as f64;
    let mean_x2 = sum_x2 / n;
    let mean_y2 = sum_y2 / n;
    let mean_lap = sum_lap / n;
    let lap_variance = (sum_lap2 / n - mean_lap * mean_lap).max(0.0);
    let energy = mean_x2 + mean_y2;
    let anisotropy = if energy > f64::EPSILON {
        (mean_x2 - mean_y2).abs() / energy
    } else {
        0.0
    };
    GradientStats {
        mean_grad_x2: mean_x2,
        mean_grad_y2: mean_y2,
        tenengrad: energy,
        directional_sharpness: 2.0 * mean_x2.min(mean_y2),
        laplacian_variance: lap_variance,
        anisotropy,
    }
}

/// Fraction of the flattest interior pixels used for the noise estimate.
/// Restricting to the flattest quartile excludes structural edges, while a
/// genuinely noisy image still has noise everywhere (its flattest quartile is
/// noisy too), so noise is measured rather than structure.
const NOISE_FLAT_FRACTION: f64 = 0.25;
/// Minimum number of samples for the noise estimate; a very small frame keeps
/// at least one interior pixel.
const MIN_NOISE_SAMPLES: usize = 16;

/// Immerkaer (1996) fast noise-variance estimate over the interior pixels.
///
/// Deterministic and model-free: `sigma ≈ sqrt(pi/2) * mean(|K * I|) / 6` for
/// the 3x3 Laplacian-like kernel `K`, averaged over the **flattest quartile**
/// of the interior (ranked by blurred-plane gradient) so sharp structure is
/// not mistaken for noise while sensor noise — present even in flat areas —
/// still dominates its own flattest quartile. Planes smaller than `3x3` report
/// `0`.
#[must_use]
pub fn noise_sigma(raw: &LumaPlane, blurred: &LumaPlane) -> f64 {
    let (width, height) = (raw.width, raw.height);
    if width < 3 || height < 3 || blurred.width != width || blurred.height != height {
        return 0.0;
    }
    let interior = ((width - 2) as usize) * ((height - 2) as usize);
    let mut samples: Vec<(f32, f32)> = Vec::with_capacity(interior);
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let grad_x = f64::from(blurred.at(x + 1, y)) - f64::from(blurred.at(x - 1, y));
            let grad_y = f64::from(blurred.at(x, y + 1)) - f64::from(blurred.at(x, y - 1));
            let gradient2 = (grad_x * grad_x + grad_y * grad_y) as f32;
            let response = immerkaer_at(raw, x, y).abs() as f32;
            samples.push((gradient2, response));
        }
    }
    if samples.is_empty() {
        return 0.0;
    }
    let keep = ((interior as f64 * NOISE_FLAT_FRACTION) as usize)
        .max(MIN_NOISE_SAMPLES.min(interior))
        .min(interior);
    // Deterministic partition: the `keep` smallest gradients are `samples[..keep]`.
    samples.select_nth_unstable_by(keep - 1, |a, b| a.0.total_cmp(&b.0));
    let sum: f64 = samples[..keep]
        .iter()
        .map(|(_, response)| f64::from(*response))
        .sum();
    // sqrt(pi/2) ≈ 1.2533141373155003.
    (sum / keep as f64) * 1.253_314_137_315_500_3 / 6.0
}

/// Laplacian-like Immerkaer kernel response at an interior pixel.
#[inline]
fn immerkaer_at(plane: &LumaPlane, x: u32, y: u32) -> f64 {
    let corner = |xx: u32, yy: u32| f64::from(plane.at(xx, yy));
    corner(x - 1, y - 1) + corner(x + 1, y - 1) + corner(x - 1, y + 1) + corner(x + 1, y + 1)
        - 2.0 * corner(x, y - 1)
        - 2.0 * corner(x - 1, y)
        - 2.0 * corner(x + 1, y)
        - 2.0 * corner(x, y + 1)
        + 4.0 * corner(x, y)
}

/// Exposure/clipping metrics derived from the shared tone+histogram pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExposureMetrics {
    /// Mean Rec.709 luminance of the analysis frame.
    pub mean: f64,
    /// Median luminance.
    pub median: f64,
    /// 1st percentile luminance.
    pub p01: f64,
    /// 99th percentile luminance.
    pub p99: f64,
    /// Fraction of samples in the darkest bin (`[0, 1/256)`).
    pub shadow_clip_fraction: f64,
    /// Fraction of samples in the brightest bin (`[255/256, 1]`).
    pub highlight_clip_fraction: f64,
    /// `shadow + highlight` clipping fraction.
    pub clip_fraction: f64,
    /// Number of samples in the analysis frame.
    pub sample_count: usize,
}

/// Measures exposure/clipping on `frame` via the existing core path.
pub fn exposure_metrics(frame: &ImageFrame) -> Result<ExposureMetrics, CullError> {
    if frame.width == 0 || frame.height == 0 {
        return Err(CullError::EmptyFrame {
            width: frame.width,
            height: frame.height,
        });
    }
    let (analysis, histogram) = analyze_tone_with_histogram(frame);
    let sample_count = analysis.sample_count;
    let (shadow, highlight) = if sample_count == 0 {
        (0.0, 0.0)
    } else {
        let n = sample_count as f64;
        (
            histogram.bins[0] as f64 / n,
            histogram.bins[histogram.bins.len() - 1] as f64 / n,
        )
    };
    Ok(ExposureMetrics {
        mean: analysis.mean,
        median: analysis.median,
        p01: analysis.p01,
        p99: analysis.p99,
        shadow_clip_fraction: shadow,
        highlight_clip_fraction: highlight,
        clip_fraction: shadow + highlight,
        sample_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::analyze_tone;

    fn gray(width: u32, height: u32, f: impl Fn(u32, u32) -> u8) -> ImageFrame {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let value = f(x, y);
                pixels.extend_from_slice(&[value, value, value, 255]);
            }
        }
        ImageFrame::new(width, height, pixels).expect("exact buffer")
    }

    fn splitmix(state: &mut u64) -> u64 {
        *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = *state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    #[test]
    fn luma_plane_matches_core_tone_mean() {
        let frame = gray(16, 16, |x, y| ((x * 13 + y * 7) % 256) as u8);
        let plane = luma_plane(&frame).expect("plane");
        let mean: f64 = plane
            .values
            .iter()
            .map(|value| f64::from(*value))
            .sum::<f64>()
            / plane.values.len() as f64;
        let tone = analyze_tone(&frame);
        assert!((mean - tone.mean).abs() < 1e-6);
    }

    #[test]
    fn empty_frame_is_a_loud_error() {
        let frame = ImageFrame::new(0, 0, vec![]).unwrap();
        assert!(matches!(
            luma_plane(&frame),
            Err(CullError::EmptyFrame { .. })
        ));
        assert!(matches!(
            exposure_metrics(&frame),
            Err(CullError::EmptyFrame { .. })
        ));
    }

    #[test]
    fn blur_of_a_constant_plane_is_constant() {
        let plane = LumaPlane {
            width: 5,
            height: 5,
            values: vec![0.4; 25],
        };
        let blurred = blur_3x3(&plane);
        assert!(blurred
            .values
            .iter()
            .all(|value| (*value - 0.4).abs() < 1e-6));
    }

    #[test]
    fn gradients_are_zero_on_flat_and_positive_on_structure() {
        let flat = LumaPlane {
            width: 8,
            height: 8,
            values: vec![0.5; 64],
        };
        let stats = gradient_stats(&flat);
        assert_eq!(stats.tenengrad, 0.0);
        assert_eq!(stats.directional_sharpness, 0.0);
        assert_eq!(stats.anisotropy, 0.0);

        let frame = gray(16, 16, |x, _| if x < 8 { 0 } else { 255 });
        let plane = luma_plane(&frame).expect("plane");
        let stats = gradient_stats(&blur_3x3(&plane));
        assert!(stats.tenengrad > 0.0);
    }

    #[test]
    fn one_directional_structure_has_zero_directional_sharpness() {
        // Vertical edges only: strong grad_x, zero grad_y.
        let mut values = vec![0f32; 64];
        for y in 0..8 {
            for x in 0..8 {
                values[y * 8 + x] = if x < 4 { 0.0 } else { 1.0 };
            }
        }
        let stats = gradient_stats(&LumaPlane {
            width: 8,
            height: 8,
            values,
        });
        assert_eq!(stats.mean_grad_y2, 0.0);
        assert!(stats.anisotropy > 0.9, "anisotropy={}", stats.anisotropy);
        assert_eq!(stats.directional_sharpness, 0.0);
    }

    #[test]
    fn noise_estimate_is_zero_on_flat_and_high_on_noise() {
        let flat = LumaPlane {
            width: 16,
            height: 16,
            values: vec![0.5; 256],
        };
        let blurred_flat = blur_3x3(&flat);
        assert!(noise_sigma(&flat, &blurred_flat) < 1e-6);

        let mut state = 7u64;
        let mut pixels = Vec::with_capacity(24 * 24 * 4);
        for _ in 0..(24 * 24) {
            let noise = (splitmix(&mut state) % 121) as i32 - 60;
            let value = (128 + noise).clamp(0, 255) as u8;
            pixels.extend_from_slice(&[value, value, value, 255]);
        }
        let frame = ImageFrame::new(24, 24, pixels).expect("exact buffer");
        let plane = luma_plane(&frame).expect("plane");
        let blurred = blur_3x3(&plane);
        assert!(noise_sigma(&plane, &blurred) > 0.05);
    }

    #[test]
    fn exposure_metrics_count_clipping() {
        let black = gray(8, 8, |_, _| 0);
        let metrics = exposure_metrics(&black).expect("metrics");
        assert_eq!(metrics.shadow_clip_fraction, 1.0);
        assert_eq!(metrics.clip_fraction, 1.0);

        let white = gray(8, 8, |_, _| 255);
        let metrics = exposure_metrics(&white).expect("metrics");
        assert_eq!(metrics.highlight_clip_fraction, 1.0);
        assert_eq!(metrics.clip_fraction, 1.0);
    }
}
