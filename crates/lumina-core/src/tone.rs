use crate::histogram::{
    accumulate_bins_and_luminance_sum, LuminanceHistogram, BIN_COUNT, BIN_WIDTH,
};
use crate::masks::MaskPlane;
use crate::{CoreError, ImageFrame};

/// Luminance is measured from RGBA8's encoded sRGB RGB channels, normalized to
/// 0..=1, with Rec.709 weights. Alpha is deliberately ignored in this raster
/// MVP, so transparent pixels are still samples of their RGB values.
///
/// Since R2-PERF-01 the quantiles of [`analyze_tone`] are class-mark
/// estimates over the shared 256-bin luminance histogram (universal accuracy
/// bound: one bin width `1/256` against the sorted-sample interpolation); the
/// mean stays the exact pixel-order sum. Normative contract:
/// `feature/architecture/pipeline.md` § Auto-Tone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToneAnalysis {
    pub mean: f64,
    pub median: f64,
    pub p01: f64,
    pub p99: f64,
    pub sample_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoToneConfig {
    pub target_luminance: f64,
    pub epsilon: f64,
    pub exposure_bounds: (f64, f64),
    pub contrast_bounds: (f64, f64),
}

impl Default for AutoToneConfig {
    fn default() -> Self {
        Self {
            target_luminance: 0.5,
            epsilon: 1e-6,
            exposure_bounds: (-10.0, 10.0),
            contrast_bounds: (-1.0, 1.0),
        }
    }
}

impl AutoToneConfig {
    pub fn validate(&self) -> Result<(), CoreError> {
        if !self.target_luminance.is_finite() || !(0.0..=1.0).contains(&self.target_luminance) {
            return Err(CoreError::InvalidAutoToneConfig(
                "target_luminance must be finite and in 0..=1".into(),
            ));
        }
        // REVIEW-CORE-N3: epsilon also drives the black/white branch
        // thresholds (`median <= epsilon` → +max EV, `median >= 1 - epsilon`
        // → -min EV). The two branches overlap for epsilon > 0.5 — an
        // epsilon near 1.0 would push EVERY image into the "+10 EV" black
        // fallback. Restrict epsilon to (0, 0.5) so the branches stay
        // disjoint and the threshold semantics remain meaningful.
        if !self.epsilon.is_finite() || self.epsilon <= 0.0 || self.epsilon >= 0.5 {
            return Err(CoreError::InvalidAutoToneConfig(
                "epsilon must be finite, greater than zero, and less than 0.5 \
                 (the black/white branches must not overlap)"
                    .into(),
            ));
        }
        for (name, (minimum, maximum)) in [
            ("exposure_bounds", self.exposure_bounds),
            ("contrast_bounds", self.contrast_bounds),
        ] {
            if !minimum.is_finite() || !maximum.is_finite() || minimum > maximum {
                return Err(CoreError::InvalidAutoToneConfig(format!(
                    "{name} must contain finite values in ascending order"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoToneResult {
    pub analysis: ToneAnalysis,
    pub exposure: f64,
    pub contrast: f64,
}

/// Per-pixel Rec.709 luminance of an RGBA8 sample in `0..=1` (alpha ignored).
///
/// This is the single, canonical definition of the raster MVP measurement
/// domain shared by [`analyze_tone`], [`suggest_auto_tone`] and the exposure
/// matchers. It is written exactly as the historical formula so the computed
/// values stay bit-identical to earlier releases.
#[inline]
fn luminance_of(pixel: &[u8]) -> f64 {
    (0.2126 * f64::from(pixel[0]) + 0.7152 * f64::from(pixel[1]) + 0.0722 * f64::from(pixel[2]))
        / 255.0
}

/// Single-pass mean Rec.709 luminance over all pixels (alpha ignored).
///
/// The exposure matchers only need the mean, so this avoids the full histogram
/// aggregation that [`analyze_tone`] performs for its quantiles. The
/// arithmetic mean is taken in pixel order; since R2-PERF-01,
/// `analyze_tone(frame).mean` is computed by exactly the same expression in
/// the same order, so the two are **bit-identical** (unit-tested). For
/// constant frames it is bit-exact.
fn mean_luminance(frame: &ImageFrame) -> f64 {
    let count = frame.pixels.len() / 4;
    if count == 0 {
        return 0.0;
    }
    let sum: f64 = frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| luminance_of(pixel))
        .sum();
    sum / count as f64
}

/// Class mark of a populated bin for the tone quantile estimator
/// (R2-PERF-01): bin `0` reports its lower edge (`0.0`) and bin `255` its
/// upper edge (`1.0`) because those edges are exact bounds of the samples
/// they contain — this keeps degenerate uniform black/white frames bit-exact
/// and preserves the documented Auto-Tone fallback branches. Interior bins
/// report their center (the standard class mark of grouped data), whose
/// distance from any sample in the bin is at most half a bin width.
fn class_mark(bin: usize) -> f64 {
    match bin {
        0 => 0.0,
        b if b >= BIN_COUNT - 1 => 1.0,
        b => (b as f64 + 0.5) * BIN_WIDTH,
    }
}

/// Value estimate for the `rank`-th smallest luminance sample (0-based) from
/// the 256-bin counts: the class mark of the bin holding that rank.
fn rank_class_mark(bins: &[u64; BIN_COUNT], rank: usize) -> f64 {
    let mut cumulative = 0u64;
    for (bin, &count) in bins.iter().enumerate() {
        cumulative += count;
        if (rank as u64) < cumulative {
            return class_mark(bin);
        }
    }
    class_mark(BIN_COUNT - 1)
}

/// Quantile estimator over the shared 256-bin histogram (R2-PERF-01).
///
/// The bracketing order statistics `floor(q·(n−1))` and `ceil(q·(n−1))` are
/// estimated by their bins' class marks and linearly interpolated with the
/// exact fractional rank — the same interpolation shape as the historical
/// sorted-sample implementation, applied to class-mark estimates.
///
/// # Accuracy (documented contract)
///
/// Every class mark lies inside the bin of the order statistic it estimates,
/// so the estimate deviates from that sample by at most one bin width
/// `1/256`; linear interpolation is convex in both brackets, which yields the
/// **universal bound** `|estimate − exact_sorted_interpolation| ≤ 1/256` for
/// every frame (dense or sparse). The estimator is monotone non-decreasing in
/// `q` (the marks are ordered by bin index), constant frames give
/// `p01 == median == p99`, and empty frames return `0.0`.
fn histogram_class_mark_quantile(bins: &[u64; BIN_COUNT], sample_count: usize, q: f64) -> f64 {
    let position = q * (sample_count - 1) as f64;
    let low = position.floor() as usize;
    let high = position.ceil() as usize;
    let low_value = rank_class_mark(bins, low);
    if high == low {
        return low_value;
    }
    low_value + (rank_class_mark(bins, high) - low_value) * (position - low as f64)
}

/// Tone statistics of a frame in a single pass (R2-PERF-01).
///
/// Replaces the historical per-pixel `Vec<f64>` allocation (~8 bytes per
/// pixel: ~192 MB at 24 MP) plus full O(n log n) sort with the shared 256-bin
/// pass of [`crate::histogram::accumulate_bins_and_luminance_sum`] (2 KB of
/// stack state, O(n)); see [`analyze_tone_with_histogram`] for the combined
/// form that also returns the persisted histogram.
///
/// # Result semantics (normative: `feature/architecture/pipeline.md` § Auto-Tone)
///
/// - `mean`: exact pixel-order Rec.709 luminance sum divided by the sample
///   count — the same measurement domain and summation order as
///   [`match_total_exposure`], i.e. `analyze_tone(f).mean ==
///   mean_luminance(f)` bit-exactly.
/// - `median`/`p01`/`p99`: interpolated from the shared 256-bin luminance
///   histogram via [`histogram_class_mark_quantile`]. Deviation from the
///   historical sorted-sample interpolation is bounded by one bin width
///   (`1/256`) for **every** frame; monotone in `q`; uniform black/white
///   frames report exactly `0.0`/`1.0`, so the Auto-Tone fallback branches
///   behave bit-identically to before.
pub fn analyze_tone(frame: &ImageFrame) -> ToneAnalysis {
    let sample_count = frame.pixels.len() / 4;
    if sample_count == 0 {
        return ToneAnalysis {
            mean: 0.0,
            median: 0.0,
            p01: 0.0,
            p99: 0.0,
            sample_count: 0,
        };
    }
    let (bins, sum) = accumulate_bins_and_luminance_sum(frame);
    ToneAnalysis {
        mean: sum / sample_count as f64,
        median: histogram_class_mark_quantile(&bins, sample_count, 0.5),
        p01: histogram_class_mark_quantile(&bins, sample_count, 0.01),
        p99: histogram_class_mark_quantile(&bins, sample_count, 0.99),
        sample_count,
    }
}

/// Combined single-pass analysis (R2-PERF-01): one iteration over the pixels
/// yields BOTH the persisted [`LuminanceHistogram`] AND the [`ToneAnalysis`]
/// derived from the very same bins.
///
/// This is the coupling point for consumers that display the histogram panel
/// and the tone panel together: instead of running `LuminanceHistogram::new`
/// and [`analyze_tone`] as two separate full passes over the frame (as the GUI
/// and `lumina_analyze` historically did), one pass produces both results.
/// The returned pair satisfies
/// `(analysis, histogram) == (analyze_tone(frame), LuminanceHistogram::new(frame))`
/// exactly (unit-tested), and the analysis quantiles are derived from exactly
/// `histogram.bins`, so both panels share one consistent pass structure.
pub fn analyze_tone_with_histogram(frame: &ImageFrame) -> (ToneAnalysis, LuminanceHistogram) {
    let (bins, sum) = accumulate_bins_and_luminance_sum(frame);
    let sample_count = frame.pixels.len() / 4;
    let analysis = if sample_count == 0 {
        ToneAnalysis {
            mean: 0.0,
            median: 0.0,
            p01: 0.0,
            p99: 0.0,
            sample_count: 0,
        }
    } else {
        ToneAnalysis {
            mean: sum / sample_count as f64,
            median: histogram_class_mark_quantile(&bins, sample_count, 0.5),
            p01: histogram_class_mark_quantile(&bins, sample_count, 0.01),
            p99: histogram_class_mark_quantile(&bins, sample_count, 0.99),
            sample_count,
        }
    };
    let histogram = LuminanceHistogram {
        width: frame.width,
        height: frame.height,
        bins: bins.to_vec(),
    };
    (analysis, histogram)
}

pub fn suggest_auto_tone(
    frame: &ImageFrame,
    config: AutoToneConfig,
) -> Result<AutoToneResult, CoreError> {
    config.validate()?;
    let analysis = analyze_tone(frame);
    let epsilon = config.epsilon;
    let target = config.target_luminance.clamp(epsilon, 1.0);
    let exposure = if analysis.sample_count == 0 {
        0.0
    } else if analysis.median <= epsilon {
        config.exposure_bounds.1
    } else if analysis.median >= 1.0 - config.epsilon {
        config.exposure_bounds.0
    } else {
        (target / analysis.median).log2()
    }
    .clamp(config.exposure_bounds.0, config.exposure_bounds.1);
    let span = analysis.p99 - analysis.p01;
    let contrast = if span <= epsilon {
        0.0
    } else {
        (0.8 / span - 1.0).clamp(config.contrast_bounds.0, config.contrast_bounds.1)
    };
    Ok(AutoToneResult {
        analysis,
        exposure: finite(exposure),
        contrast: finite(contrast),
    })
}

pub fn match_total_exposure(frame: &ImageFrame, target_luminance: f64) -> Result<f64, CoreError> {
    validate_target_luminance(target_luminance)?;
    Ok(matching_delta(mean_luminance(frame), target_luminance))
}

/// Weighted measurement-domain variant of [`match_total_exposure`] (F-041).
///
/// Measures the final visible domain: `frame` is the render result AFTER
/// crop/geometry (as delivered by [`crate::render_frame`]) and `mask_layers`
/// are the effective mask planes already resampled to the frame dimensions.
/// Every pixel receives the weight `w = ∏_layer plane_layer[pixel] / u16::MAX`
/// (product over all active layers — the intersection: a pixel fully masked in
/// any layer has weight 0 and is not part of the global visible measurement
/// domain). The weighted mean uses Rec.709 luminance and ignores alpha, like
/// [`analyze_tone`]. An empty `mask_layers` slice yields exactly the
/// [`match_total_exposure`] result (delegation), so callers without active
/// masks keep the raster semantics bit-exactly.
///
/// A non-empty `mask_layers` slice whose **every** plane is entirely
/// `u16::MAX` (each pixel weight `w = 1.0`, the mask has no effect) is also
/// delegated to the plain [`match_total_exposure`] path before the weighted
/// loop runs. This is a documented fast path, not a fallback: with every
/// weight `1.0` the weighted loop reduces to the very same pixel-order
/// luminance sum that the plain matcher computes, so the result is bit-exact
/// (`All-MAX ≡ unmasked`). The dimension validation below still runs first —
/// a mismatched all-`u16::MAX` plane is rejected like any other invalid plane.
///
/// # Validation
///
/// Every plane must match the frame dimensions (`width == frame.width`,
/// `height == frame.height`, `values.len() == width * height`); a mismatch is
/// rejected with [`CoreError::InvalidMaskPlane`] before any pixel is touched —
/// no silent fallback.
///
/// # Fallback (fully masked)
///
/// If the weight sum is at most the epsilon (`1e-6`, no visible pixel), the
/// documented fallback is `Ok(0.0)` — an identity delta that performs no
/// adjustment. This is consistent with the `sample_count == 0` path of
/// [`suggest_auto_tone`] (exposure `0.0`). No NaN, no panic, no silent
/// adjustment on an invisible image.
pub fn match_total_exposure_masked(
    frame: &ImageFrame,
    target_luminance: f64,
    mask_layers: &[MaskPlane],
) -> Result<f64, CoreError> {
    validate_target_luminance(target_luminance)?;
    if mask_layers.is_empty() {
        return Ok(matching_delta(mean_luminance(frame), target_luminance));
    }
    let pixel_count = frame.width as usize * frame.height as usize;
    for plane in mask_layers {
        if plane.width != frame.width
            || plane.height != frame.height
            || plane.values.len() != pixel_count
        {
            return Err(CoreError::InvalidMaskPlane {
                width: plane.width,
                height: plane.height,
                length: plane.values.len(),
            });
        }
    }
    // All-MAX fast path (documented above): a mask whose every plane is
    // entirely u16::MAX has no effect — every pixel weight is exactly 1.0, so
    // the weighted mean equals the plain mean. Delegating to the same
    // single-pass mean as the plain matcher (both sum the per-pixel luminance
    // in pixel order) keeps `All-MAX ≡ unmasked` bit-exact, which matches the
    // row-major weighted loop exactly (weight 1.0) but avoids the redundant
    // per-plane weight multiplication.
    if mask_layers
        .iter()
        .all(|plane| plane.values.iter().all(|&value| value == u16::MAX))
    {
        return Ok(matching_delta(mean_luminance(frame), target_luminance));
    }
    let mut weighted_sum = 0.0;
    let mut weight_sum = 0.0;
    for index in 0..pixel_count {
        let pixel = &frame.pixels[index * 4..index * 4 + 4];
        let luminance = luminance_of(pixel);
        let mut weight = 1.0;
        for plane in mask_layers {
            weight *= f64::from(plane.values[index]) / f64::from(u16::MAX);
        }
        weighted_sum += luminance * weight;
        weight_sum += weight;
    }
    // All weights are non-negative, so a zero weight sum means every pixel is
    // fully masked. The epsilon guard mirrors the existing matching semantics.
    if weight_sum <= 1e-6 {
        return Ok(0.0);
    }
    let internal_mean = weighted_sum / weight_sum;
    Ok(matching_delta(internal_mean, target_luminance))
}

fn validate_target_luminance(target_luminance: f64) -> Result<(), CoreError> {
    if !target_luminance.is_finite() || !(0.0..=1.0).contains(&target_luminance) {
        return Err(CoreError::InvalidAdjustment {
            name: "target_luminance".into(),
            value: target_luminance,
            minimum: 0.0,
            maximum: 1.0,
        });
    }
    Ok(())
}

/// Shared delta logic of both matching entry points (F-041): identical
/// epsilon (`1e-6`), finite guard and `-10..=10` clamping, so the plain and
/// the masked variant protect identically.
fn matching_delta(current: f64, target: f64) -> f64 {
    let epsilon = 1e-6;
    let value = if target <= epsilon {
        -10.0
    } else if current <= epsilon {
        10.0
    } else {
        (target / current.max(epsilon)).log2()
    };
    finite(value).clamp(-10.0, 10.0)
}

/// Stable fingerprint for deciding whether persisted analysis belongs to this frame.
pub fn tone_fingerprint(frame: &ImageFrame, config: AutoToneConfig) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    // Include the complete analysis configuration in a canonical byte stream.
    for value in [
        config.target_luminance,
        config.epsilon,
        config.exposure_bounds.0,
        config.exposure_bounds.1,
        config.contrast_bounds.0,
        config.contrast_bounds.1,
    ] {
        for byte in value.to_bits().to_le_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    for value in [frame.width, frame.height] {
        for byte in value.to_le_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    for byte in &frame.pixels {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    format!("tone-rgba8-rec709:{hash:016x}")
}

fn finite(value: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn percentile_and_mean_are_stable() {
        let f = ImageFrame::new(
            4,
            1,
            vec![
                0, 0, 0, 0, 64, 64, 64, 0, 128, 128, 128, 0, 255, 255, 255, 0,
            ],
        )
        .unwrap();
        let a = analyze_tone(&f);
        // R2-PERF-01: the median is now the documented 256-bin class-mark
        // estimate. Its universal bound against the exact sorted-sample
        // interpolation (96/255 for this frame) is one bin width (1/256);
        // pipeline.md § Auto-Tone. The exact value here is
        // 96.5/256 = 0.376953…, deviation 4.8e-4.
        assert!((a.median - 96.0 / 255.0).abs() <= 1.0 / 256.0 + 1e-9);
        assert!(a.p01 < a.p99);
    }
    #[test]
    fn dark_bright_and_targets_are_bounded() {
        for value in [0, 255] {
            let f = ImageFrame::new(1, 1, vec![value, value, value, 255]).unwrap();
            let r = suggest_auto_tone(&f, AutoToneConfig::default()).unwrap();
            assert!((-10.0..=10.0).contains(&r.exposure));
        }
        let f = ImageFrame::new(1, 1, vec![128, 128, 128, 255]).unwrap();
        assert_eq!(match_total_exposure(&f, 0.0).unwrap(), -10.0);
    }
    #[test]
    fn empty_and_near_black_are_safe() {
        let f = ImageFrame::new(0, 0, vec![]).unwrap();
        assert_eq!(
            suggest_auto_tone(&f, AutoToneConfig::default())
                .unwrap()
                .exposure,
            0.0
        );
        let f = ImageFrame::new(1, 1, vec![0, 0, 0, 255]).unwrap();
        assert_eq!(match_total_exposure(&f, 1.0).unwrap(), 10.0);
    }

    #[test]
    fn invalid_config_is_rejected() {
        for config in [
            AutoToneConfig {
                epsilon: f64::NAN,
                ..Default::default()
            },
            AutoToneConfig {
                epsilon: f64::INFINITY,
                ..Default::default()
            },
            AutoToneConfig {
                epsilon: 0.0,
                ..Default::default()
            },
            // REVIEW-CORE-N3: epsilon ≥ 0.5 would let the black/white
            // branches overlap (epsilon near 1.0 sends every image to the
            // "+10 EV" black fallback), so the upper bound is exclusive.
            AutoToneConfig {
                epsilon: 0.5,
                ..Default::default()
            },
            AutoToneConfig {
                epsilon: 1.0,
                ..Default::default()
            },
            AutoToneConfig {
                target_luminance: -0.1,
                ..Default::default()
            },
            AutoToneConfig {
                target_luminance: 1.1,
                ..Default::default()
            },
            AutoToneConfig {
                exposure_bounds: (1.0, -1.0),
                ..Default::default()
            },
            AutoToneConfig {
                contrast_bounds: (f64::NEG_INFINITY, 1.0),
                ..Default::default()
            },
        ] {
            assert!(config.validate().is_err());
        }
    }

    #[test]
    fn epsilon_below_one_half_is_valid_and_branches_stay_disjoint() {
        // REVIEW-CORE-N3: the largest legal epsilon keeps the black/white
        // fallback thresholds (`<= eps`, `>= 1 - eps`) strictly disjoint.
        let config = AutoToneConfig {
            epsilon: 0.499,
            ..Default::default()
        };
        config.validate().unwrap();
        // Every uniform frame stays on a well-defined branch and yields the
        // documented zero-contrast identity (span = 0 ≤ epsilon).
        for value in [0u8, 1, 64, 127, 128, 200, 254, 255] {
            let frame = ImageFrame::new(1, 1, vec![value, value, value, 255]).unwrap();
            let result = suggest_auto_tone(&frame, config).unwrap();
            assert_eq!(result.contrast, 0.0);
            assert!((-10.0..=10.0).contains(&result.exposure));
        }
    }

    #[test]
    fn fingerprint_includes_analysis_config() {
        let frame = ImageFrame::new(1, 1, vec![128, 128, 128, 255]).unwrap();
        let base = AutoToneConfig::default();
        assert_ne!(
            tone_fingerprint(&frame, base),
            tone_fingerprint(
                &frame,
                AutoToneConfig {
                    target_luminance: 0.6,
                    ..base
                }
            )
        );
        assert_ne!(
            tone_fingerprint(&frame, base),
            tone_fingerprint(
                &frame,
                AutoToneConfig {
                    epsilon: 1e-4,
                    ..base
                }
            )
        );
        assert_ne!(
            tone_fingerprint(&frame, base),
            tone_fingerprint(
                &frame,
                AutoToneConfig {
                    exposure_bounds: (-5.0, 5.0),
                    ..base
                }
            )
        );
    }

    #[test]
    fn fingerprint_includes_frame_geometry() {
        let pixels = vec![128, 128, 128, 255, 64, 64, 64, 255];
        let wide = ImageFrame::new(2, 1, pixels.clone()).unwrap();
        let tall = ImageFrame::new(1, 2, pixels).unwrap();
        assert_ne!(
            tone_fingerprint(&wide, AutoToneConfig::default()),
            tone_fingerprint(&tall, AutoToneConfig::default())
        );
    }

    #[test]
    fn percentiles_interpolate_and_alpha_is_ignored() {
        let frame = ImageFrame::new(2, 1, vec![0, 0, 0, 0, 255, 255, 255, 0]).unwrap();
        let analysis = analyze_tone(&frame);
        assert!((analysis.p01 - 0.01).abs() < 1e-9);
        assert!((analysis.p99 - 0.99).abs() < 1e-9);
    }

    #[test]
    fn matching_clips_and_handles_positive_and_negative_values() {
        let dark = ImageFrame::new(1, 1, vec![32, 32, 32, 255]).unwrap();
        let bright = ImageFrame::new(1, 1, vec![224, 224, 224, 255]).unwrap();
        assert!(match_total_exposure(&dark, 0.8).unwrap() > 0.0);
        assert!(match_total_exposure(&bright, 0.2).unwrap() < 0.0);
        assert_eq!(
            match_total_exposure(&dark, f64::NAN)
                .unwrap_err()
                .to_string(),
            "invalid target_luminance: must be finite and in 0e0..=1e0, got NaN"
        );
    }

    #[test]
    fn auto_tone_is_finite_and_bounded_for_deterministic_rgba8_frames() {
        let config = AutoToneConfig {
            target_luminance: 0.62,
            epsilon: 1e-5,
            exposure_bounds: (-3.5, 4.25),
            contrast_bounds: (-0.75, 0.8),
        };
        for frame_index in 0..12u8 {
            let width = 3 + u32::from(frame_index % 4);
            let height = 2 + u32::from(frame_index % 3);
            let pixels = (0..width * height)
                .flat_map(|pixel_index| {
                    let value = frame_index
                        .wrapping_mul(37)
                        .wrapping_add((pixel_index as u8).wrapping_mul(19));
                    [
                        value,
                        value.wrapping_mul(3).wrapping_add(11),
                        value.wrapping_mul(7).wrapping_add(23),
                        value.wrapping_mul(13),
                    ]
                })
                .collect();
            let frame = ImageFrame::new(width, height, pixels).unwrap();
            let result = suggest_auto_tone(&frame, config).unwrap();

            assert_eq!(result.analysis.sample_count, (width * height) as usize);
            for value in [
                result.analysis.mean,
                result.analysis.median,
                result.analysis.p01,
                result.analysis.p99,
            ] {
                assert!(value.is_finite());
                assert!((0.0..=1.0).contains(&value));
            }
            assert!(result.exposure.is_finite());
            assert!(
                (config.exposure_bounds.0..=config.exposure_bounds.1).contains(&result.exposure)
            );
            assert!(result.contrast.is_finite());
            assert!(
                (config.contrast_bounds.0..=config.contrast_bounds.1).contains(&result.contrast)
            );
        }
    }

    #[test]
    fn auto_tone_is_independent_of_alpha_for_multiple_rgb_values() {
        let rgb_values = [[0, 0, 0], [17, 129, 241], [64, 96, 128], [255, 128, 1]];
        let reference_pixels: Vec<u8> = rgb_values
            .iter()
            .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
            .collect();
        let reference = ImageFrame::new(4, 1, reference_pixels).unwrap();
        let expected = suggest_auto_tone(&reference, AutoToneConfig::default()).unwrap();

        for alpha_set in [[0, 1, 127, 255], [255, 128, 32, 0], [7, 99, 200, 250]] {
            let pixels: Vec<u8> = rgb_values
                .iter()
                .zip(alpha_set)
                .flat_map(|(rgb, alpha)| [rgb[0], rgb[1], rgb[2], alpha])
                .collect();
            let result = suggest_auto_tone(
                &ImageFrame::new(4, 1, pixels).unwrap(),
                AutoToneConfig::default(),
            )
            .unwrap();
            assert_eq!(result.analysis, expected.analysis);
            assert_eq!(result.exposure, expected.exposure);
            assert_eq!(result.contrast, expected.contrast);
        }
    }

    #[test]
    fn auto_tone_and_exposure_matching_are_bit_exactly_deterministic() {
        let frame = ImageFrame::new(
            3,
            2,
            vec![
                3, 17, 91, 0, 42, 128, 211, 255, 255, 64, 7, 32, 99, 101, 203, 17, 180, 220, 12,
                200, 71, 33, 88, 250,
            ],
        )
        .unwrap();
        let config = AutoToneConfig::default();
        let reference = suggest_auto_tone(&frame, config).unwrap();
        let reference_match = match_total_exposure(&frame, 0.63).unwrap();
        let fingerprint = tone_fingerprint(&frame, config);
        for _ in 0..16 {
            assert_eq!(suggest_auto_tone(&frame, config).unwrap(), reference);
            assert_eq!(match_total_exposure(&frame, 0.63).unwrap(), reference_match);
            assert_eq!(tone_fingerprint(&frame, config), fingerprint);
        }
    }

    #[test]
    fn matching_exposure_is_bounded_and_monotonic_for_increasing_targets() {
        let frame = ImageFrame::new(
            5,
            1,
            vec![
                7, 31, 99, 0, 32, 64, 128, 17, 96, 128, 160, 127, 192, 224, 240, 200, 255, 255,
                255, 255,
            ],
        )
        .unwrap();
        let targets = [0.0, 0.01, 0.05, 0.15, 0.3, 0.5, 0.7, 0.9, 1.0];
        let values: Vec<f64> = targets
            .iter()
            .map(|target| match_total_exposure(&frame, *target).unwrap())
            .collect();

        for value in &values {
            assert!(value.is_finite());
            assert!((-10.0..=10.0).contains(value));
        }
        for pair in values.windows(2) {
            assert!(
                pair[0] <= pair[1],
                "matching exposure is not monotonic: {pair:?}"
            );
        }
    }

    // ---- F-041: weighted measurement domain ----

    #[test]
    fn masked_without_layers_is_identical_to_plain_matching() {
        let frame = ImageFrame::new(
            4,
            1,
            vec![
                200, 200, 200, 255, 60, 60, 60, 255, 128, 128, 128, 0, 17, 129, 241, 128,
            ],
        )
        .unwrap();
        for target in [0.0, 0.01, 0.25, 0.5, 0.9, 1.0] {
            assert_eq!(
                match_total_exposure_masked(&frame, target, &[]).unwrap(),
                match_total_exposure(&frame, target).unwrap(),
                "target {target}"
            );
        }
    }

    #[test]
    fn fully_masked_pixel_is_excluded_from_the_measurement() {
        // 2x1 frame: pixel A = 200 (bright), pixel B = 60 (dark). The plane
        // masks A fully (0) and keeps B fully visible (u16::MAX). The weighted
        // mean is exactly the luminance of pixel B alone, so the result must be
        // bit-identical to plain matching on the 1x1 frame [60]:
        //   log2(0.5 / (60/255)) = log2(2.125) ≈ 1.08746  (for target 0.5)
        let frame = ImageFrame::new(2, 1, vec![200, 200, 200, 255, 60, 60, 60, 255]).unwrap();
        let plane = MaskPlane::new(2, 1, vec![0, u16::MAX]).unwrap();
        let expected_frame = ImageFrame::new(1, 1, vec![60, 60, 60, 255]).unwrap();
        for target in [0.01, 0.5, 0.99] {
            assert_eq!(
                match_total_exposure_masked(&frame, target, std::slice::from_ref(&plane)).unwrap(),
                match_total_exposure(&expected_frame, target).unwrap(),
                "target {target}"
            );
        }
    }

    #[test]
    fn partial_weight_half_counts_half() {
        // 3x1 frame: A = 240 (fully masked, weight 0), B = 16 (weight 1.0),
        // C = 128 (plane 32768 ≈ u16::MAX/2). u16::MAX is odd, so the exact
        // weight is w = 32768/65535 ≈ 0.50000763 (a pixel with 32768 counts
        // half, exactly the documented product semantics). Weighted mean:
        //   (lum(16) * 1.0 + lum(128) * w) / (1.0 + w) ≈ 0.209151816123538
        //   delta = log2(0.5 / mean) ≈ 1.2573775695277125
        let frame = ImageFrame::new(
            3,
            1,
            vec![240, 240, 240, 255, 16, 16, 16, 255, 128, 128, 128, 255],
        )
        .unwrap();
        let plane = MaskPlane::new(3, 1, vec![0, u16::MAX, 32768]).unwrap();
        let delta = match_total_exposure_masked(&frame, 0.5, &[plane]).unwrap();
        let weight = 32768.0 / f64::from(u16::MAX);
        assert!((weight - 0.5).abs() < 1e-5, "weight {weight} ~= half");
        let luminance = |value: u8| {
            (0.2126 * f64::from(value) + 0.7152 * f64::from(value) + 0.0722 * f64::from(value))
                / 255.0
        };
        let expected_mean = (luminance(16) * 1.0 + luminance(128) * weight) / (1.0 + weight);
        let implied_mean = 0.5 / 2.0_f64.powf(delta);
        assert!(
            (implied_mean - expected_mean).abs() < 1e-9,
            "weighted mean {implied_mean} != expected {expected_mean} (delta {delta})"
        );
        assert!(
            (delta - 1.2573775695277125).abs() < 1e-9,
            "delta {delta} must be 1.2573775695277125"
        );
    }

    #[test]
    fn product_weights_intersect_layers() {
        // Pixel A is fully masked in layer 1 (0) — it must be excluded even
        // though layer 2 gives it full weight (u16::MAX): the product over the
        // layers is the intersection. Only pixel B (60) remains visible.
        let frame = ImageFrame::new(2, 1, vec![200, 200, 200, 255, 60, 60, 60, 255]).unwrap();
        let layer1 = MaskPlane::new(2, 1, vec![0, u16::MAX]).unwrap();
        let layer2 = MaskPlane::new(2, 1, vec![u16::MAX, u16::MAX]).unwrap();
        let expected_frame = ImageFrame::new(1, 1, vec![60, 60, 60, 255]).unwrap();
        assert_eq!(
            match_total_exposure_masked(&frame, 0.5, &[layer1, layer2]).unwrap(),
            match_total_exposure(&expected_frame, 0.5).unwrap()
        );
        // Layer order must not matter (multiplication is commutative).
        let swapped = MaskPlane::new(2, 1, vec![u16::MAX, u16::MAX]).unwrap();
        assert_eq!(
            match_total_exposure_masked(
                &frame,
                0.5,
                &[swapped, MaskPlane::new(2, 1, vec![0, u16::MAX]).unwrap()],
            )
            .unwrap(),
            match_total_exposure(&expected_frame, 0.5).unwrap()
        );
    }

    #[test]
    fn fully_masked_frame_uses_documented_zero_delta_fallback() {
        // Both pixels fully masked (0): no visible pixel. The documented
        // fallback is delta 0.0 (identity, like the sample_count == 0 path of
        // suggest_auto_tone) — finite, no panic, no silent NaN.
        let frame = ImageFrame::new(2, 1, vec![200, 200, 200, 255, 60, 60, 60, 255]).unwrap();
        let plane = MaskPlane::new(2, 1, vec![0, 0]).unwrap();
        for target in [0.0, 0.5, 1.0] {
            let delta =
                match_total_exposure_masked(&frame, target, std::slice::from_ref(&plane)).unwrap();
            assert_eq!(delta, 0.0, "target {target}");
            assert!(delta.is_finite());
        }
        // Target validation still runs before the fallback.
        assert!(matches!(
            match_total_exposure_masked(&frame, f64::NAN, &[plane]),
            Err(CoreError::InvalidAdjustment { .. })
        ));
    }

    #[test]
    fn mismatched_plane_dimensions_are_rejected() {
        let frame = ImageFrame::new(2, 1, vec![60; 8]).unwrap();
        // Constructed directly because `MaskPlane::new` rejects invalid
        // dimensions itself; the matching entry point must reject them too.
        for plane in [
            MaskPlane {
                width: 1,
                height: 1,
                values: vec![0],
            },
            MaskPlane {
                width: 2,
                height: 2,
                values: vec![0; 4],
            },
            MaskPlane {
                width: 2,
                height: 1,
                values: vec![0],
            },
        ] {
            let error = match_total_exposure_masked(&frame, 0.5, &[plane]).unwrap_err();
            assert!(matches!(error, CoreError::InvalidMaskPlane { .. }));
            assert!(error.to_string().contains("mask plane"));
        }
        // A valid plane passes.
        let plane = MaskPlane::new(2, 1, vec![u16::MAX; 2]).unwrap();
        assert!(match_total_exposure_masked(&frame, 0.5, &[plane]).is_ok());
    }

    #[test]
    fn masked_matching_is_deterministic() {
        let frame = ImageFrame::new(
            3,
            2,
            vec![
                3, 17, 91, 0, 42, 128, 211, 255, 255, 64, 7, 32, 99, 101, 203, 17, 180, 220, 12,
                200, 71, 33, 88, 250,
            ],
        )
        .unwrap();
        let plane = MaskPlane::new(3, 2, vec![0, 65535, 32768, 65535, 0, 32768]).unwrap();
        let reference =
            match_total_exposure_masked(&frame, 0.63, std::slice::from_ref(&plane)).unwrap();
        for _ in 0..8 {
            assert_eq!(
                match_total_exposure_masked(&frame, 0.63, std::slice::from_ref(&plane)).unwrap(),
                reference
            );
        }
    }

    #[test]
    fn measurement_uses_the_post_crop_frame() {
        // 4x1 frame with the brightest pixel (255) at the right edge. A crop
        // that removes the edge yields the 3x1 post-crop render result; the
        // function measures exactly the frame it receives (the post-crop
        // render output, F-041 measurement domain), not the decoded original.
        //   post-crop mean: 64/255 ≈ 0.25098  -> delta ≈ log2(1.9922) ≈ 0.994
        //   full mean:      111.75/255 ≈ 0.438 -> delta ≈ log2(1.1409) ≈ 0.190
        let full = ImageFrame::new(
            4,
            1,
            vec![
                64, 64, 64, 255, 64, 64, 64, 255, 64, 64, 64, 255, 255, 255, 255, 255,
            ],
        )
        .unwrap();
        let post_crop = ImageFrame::new(
            3,
            1,
            vec![64, 64, 64, 255, 64, 64, 64, 255, 64, 64, 64, 255],
        )
        .unwrap();
        let target = 0.5;
        assert_eq!(
            match_total_exposure_masked(&post_crop, target, &[]).unwrap(),
            match_total_exposure(&post_crop, target).unwrap()
        );
        // The bright edge would dominate the un-cropped measurement, so the
        // deltas must differ clearly: the function measures the passed frame.
        assert!(
            (match_total_exposure_masked(&post_crop, target, &[]).unwrap()
                - match_total_exposure(&full, target).unwrap())
            .abs()
                > 0.5
        );
    }

    // ---- F-074-A3: byte-/value-identity against the original logic ----

    /// Independent reimplementation of the ORIGINAL (pre-R2-PERF-01)
    /// `analyze_tone`: per-pixel vector + stable sort + sorted-sum mean +
    /// linear-interpolation percentiles. Used by the bounded-drift comparison
    /// below to prove the 256-bin class-mark kernel stays inside its
    /// documented tolerances against the original semantics.
    fn reference_analyze_tone(frame: &ImageFrame) -> ToneAnalysis {
        let mut v: Vec<f64> = frame
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| {
                (0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]))
                    / 255.0
            })
            .collect();
        if v.is_empty() {
            return ToneAnalysis {
                mean: 0.0,
                median: 0.0,
                p01: 0.0,
                p99: 0.0,
                sample_count: 0,
            };
        }
        v.sort_by(f64::total_cmp);
        let percentile = |q: f64| {
            let position = q * (v.len() - 1) as f64;
            let low = position.floor() as usize;
            let high = position.ceil() as usize;
            v[low] + (v[high] - v[low]) * (position - low as f64)
        };
        ToneAnalysis {
            mean: v.iter().sum::<f64>() / v.len() as f64,
            median: percentile(0.5),
            p01: percentile(0.01),
            p99: percentile(0.99),
            sample_count: v.len(),
        }
    }

    /// Original `suggest_auto_tone`, delegating to [`reference_analyze_tone`].
    fn reference_suggest_auto_tone(
        frame: &ImageFrame,
        config: AutoToneConfig,
    ) -> Result<AutoToneResult, CoreError> {
        config.validate()?;
        let analysis = reference_analyze_tone(frame);
        let epsilon = config.epsilon;
        let target = config.target_luminance.clamp(epsilon, 1.0);
        let exposure = if analysis.sample_count == 0 {
            0.0
        } else if analysis.median <= epsilon {
            config.exposure_bounds.1
        } else if analysis.median >= 1.0 - config.epsilon {
            config.exposure_bounds.0
        } else {
            (target / analysis.median).log2()
        }
        .clamp(config.exposure_bounds.0, config.exposure_bounds.1);
        let span = analysis.p99 - analysis.p01;
        let contrast = if span <= epsilon {
            0.0
        } else {
            (0.8 / span - 1.0).clamp(config.contrast_bounds.0, config.contrast_bounds.1)
        };
        Ok(AutoToneResult {
            analysis,
            exposure: super::finite(exposure),
            contrast: super::finite(contrast),
        })
    }

    /// Original `match_total_exposure`: sorted-sum mean via the original
    /// `analyze_tone`, then the shared matching delta.
    fn reference_match_total_exposure(
        frame: &ImageFrame,
        target_luminance: f64,
    ) -> Result<f64, CoreError> {
        super::validate_target_luminance(target_luminance)?;
        Ok(super::matching_delta(
            reference_analyze_tone(frame).mean,
            target_luminance,
        ))
    }

    /// Deterministic, randomized `size × size` RGBA8 frame (alpha 255) mirroring
    /// the bench fixture seed, so the identity check also exercises the large,
    /// fully-populated measurement domain.
    fn bench_like_frame(size: u32) -> ImageFrame {
        let mut state = 0x5EED_u64;
        let mut rng = || {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            (z ^ (z >> 31)) as u8
        };
        let count = (size * size) as usize;
        let mut pixels = Vec::with_capacity(count * 4);
        for _ in 0..count {
            pixels.extend_from_slice(&[rng(), rng(), rng(), 255]);
        }
        ImageFrame::new(size, size, pixels).unwrap()
    }

    #[test]
    fn tone_analyzers_track_original_reference_within_documented_bounds() {
        // R2-PERF-01 replaced the per-pixel sort with the shared 256-bin
        // class-mark pass. The ORIGINAL semantics are kept as the reference
        // implementations above and compared with the documented contract
        // (pipeline.md § Auto-Tone):
        //
        // - `mean`: same measurement domain, but the summation changed from
        //   the sorted vector to pixel order (identical to the exposure
        //   matchers). f64 addition is not associative, so last-bit
        //   differences are possible → 1e-9 (far below every golden
        //   tolerance; the matchers' own mean was already pixel-order).
        // - `median`/`p01`/`p99`: class-mark estimates over 256 bins. Every
        //   class mark lies in the bin of the order statistic it estimates,
        //   so the universal bound against the sorted-sample interpolation is
        //   exactly one bin width `1/256` for EVERY frame — asserted here.
        // - `suggest_auto_tone` exposure/contrast derive from those
        //   statistics; log-sensitivity amplifies a one-bin median shift by
        //   `1/(m·ln2)` (worst case here ≈ 0.02 EV) → documented 0.05.
        // - `match_total_exposure` is untouched (single-pass mean + shared
        //   delta); compared against the original sorted-sum reference with
        //   the established 1e-6 tolerance.
        let frames = vec![
            ImageFrame::new(0, 0, vec![]).unwrap(),
            ImageFrame::new(1, 1, vec![0, 0, 0, 255]).unwrap(),
            ImageFrame::new(1, 1, vec![255, 255, 255, 255]).unwrap(),
            ImageFrame::new(2, 1, vec![200, 200, 200, 255, 60, 60, 60, 255]).unwrap(),
            ImageFrame::new(
                3,
                2,
                vec![
                    3, 17, 91, 0, 42, 128, 211, 255, 255, 64, 7, 32, 99, 101, 203, 17, 180, 220,
                    12, 200, 71, 33, 88, 250,
                ],
            )
            .unwrap(),
            bench_like_frame(512),
        ];
        const BIN_BOUND: f64 = 1.0 / 256.0 + 1e-9;

        for frame in &frames {
            let optimized = analyze_tone(frame);
            let reference = reference_analyze_tone(frame);
            assert_eq!(optimized.sample_count, reference.sample_count);
            assert!(
                (optimized.mean - reference.mean).abs() <= 1e-9,
                "mean: optimized {} vs reference {}",
                optimized.mean,
                reference.mean
            );
            assert!(
                optimized.p01 <= optimized.median && optimized.median <= optimized.p99,
                "quantile ordering broken: {:?}",
                optimized
            );
            for (name, value, expected) in [
                ("median", optimized.median, reference.median),
                ("p01", optimized.p01, reference.p01),
                ("p99", optimized.p99, reference.p99),
            ] {
                assert!(
                    (value - expected).abs() <= BIN_BOUND,
                    "{name}: optimized {value} vs reference {expected}, bound {BIN_BOUND}"
                );
            }

            let config = AutoToneConfig::default();
            let opt_auto = suggest_auto_tone(frame, config).unwrap();
            let ref_auto = reference_suggest_auto_tone(frame, config).unwrap();
            assert!(
                (opt_auto.exposure - ref_auto.exposure).abs() <= 0.05,
                "auto exposure: optimized {} vs reference {}",
                opt_auto.exposure,
                ref_auto.exposure
            );
            assert!(
                (opt_auto.contrast - ref_auto.contrast).abs() <= 0.05,
                "auto contrast: optimized {} vs reference {}",
                opt_auto.contrast,
                ref_auto.contrast
            );

            for target in [0.0, 1e-6, 0.25, 0.5, 0.63, 0.9, 1.0] {
                let diff = (match_total_exposure(frame, target).unwrap()
                    - reference_match_total_exposure(frame, target).unwrap())
                .abs();
                assert!(
                    diff <= 1e-6,
                    "match_total_exposure target {target}: optimized vs reference, diff {diff}"
                );
            }
        }
    }

    #[test]
    fn mean_is_bit_identical_to_the_pixel_order_matcher_mean() {
        // R2-PERF-01: `analyze_tone.mean` and the exposure matcher's internal
        // `mean_luminance` use exactly the same expression and summation
        // order, so they must agree bit-exactly — this is what keeps the
        // Auto-Tone analysis display and `match_total_exposure` consistent.
        let frames = vec![
            ImageFrame::new(0, 0, vec![]).unwrap(),
            ImageFrame::new(1, 1, vec![128, 128, 128, 255]).unwrap(),
            ImageFrame::new(
                3,
                2,
                vec![
                    3, 17, 91, 0, 42, 128, 211, 255, 255, 64, 7, 32, 99, 101, 203, 17, 180, 220,
                    12, 200, 71, 33, 88, 250,
                ],
            )
            .unwrap(),
            bench_like_frame(64),
        ];
        for frame in &frames {
            assert_eq!(analyze_tone(frame).mean, mean_luminance(frame));
        }
    }

    #[test]
    fn degenerate_frames_stay_exact_and_preserve_the_fallback_branches() {
        // Constant frames: p01 == median == p99 (span 0 → contrast identity),
        // uniform black reports median exactly 0.0 and uniform white exactly
        // 1.0 (outer-bin edge class marks), so the documented Auto-Tone
        // black/white fallback branches fire bit-identically to the
        // sorted-sample implementation.
        for value in [0u8, 1, 64, 128, 200, 254, 255] {
            let mut pixels = Vec::with_capacity(16 * 4);
            for _ in 0..16 {
                pixels.extend_from_slice(&[value, value, value, 255]);
            }
            let frame = ImageFrame::new(4, 4, pixels).unwrap();
            let analysis = analyze_tone(&frame);
            assert_eq!(analysis.p01, analysis.median);
            assert_eq!(analysis.median, analysis.p99);
            match value {
                0 => assert_eq!(analysis.median, 0.0),
                255 => assert_eq!(analysis.median, 1.0),
                v => assert!((analysis.median - (f64::from(v) + 0.5) / 256.0).abs() <= 1e-12),
            }
            let result = suggest_auto_tone(&frame, AutoToneConfig::default()).unwrap();
            assert_eq!(result.contrast, 0.0, "value {value}");
            if value == 0 {
                assert_eq!(result.exposure, AutoToneConfig::default().exposure_bounds.1);
            } else if value == 255 {
                assert_eq!(result.exposure, AutoToneConfig::default().exposure_bounds.0);
            } else {
                let expected = (AutoToneConfig::default().target_luminance
                    / result.analysis.median)
                    .log2()
                    .clamp(
                        AutoToneConfig::default().exposure_bounds.0,
                        AutoToneConfig::default().exposure_bounds.1,
                    );
                assert_eq!(result.exposure, expected);
            }
        }
    }

    #[test]
    fn bimodal_frames_get_an_exact_median_across_the_gap() {
        // 8x8 black/white checkerboard: two populated bins far apart. The
        // historical failure mode of naive intra-bin quantiles would place
        // the median inside bin 0 (~0.004); the class-mark bracket estimator
        // interpolates between the outer-bin edge marks and reproduces the
        // exact sorted-sample statistics bit-near-exactly.
        let mut pixels = Vec::with_capacity(64 * 4);
        for y in 0..8u32 {
            for x in 0..8u32 {
                let value = if (x + y) % 2 == 0 { 255 } else { 0 };
                pixels.extend_from_slice(&[value, value, value, 255]);
            }
        }
        let frame = ImageFrame::new(8, 8, pixels).unwrap();
        let analysis = analyze_tone(&frame);
        assert_eq!(analysis.sample_count, 64);
        assert_eq!(analysis.p01, 0.0);
        assert!((analysis.median - 0.5).abs() < 1e-9);
        assert_eq!(analysis.p99, 1.0);

        let result = suggest_auto_tone(&frame, AutoToneConfig::default()).unwrap();
        assert!((result.exposure - 0.0).abs() <= 1e-9);
        assert!((result.contrast - (-0.2)).abs() <= 1e-9);
    }

    #[test]
    fn analyze_tone_with_histogram_matches_the_individual_calls() {
        // Coupling contract (R2-PERF-01): the combined single-pass call is
        // exactly equivalent to the two individual calls it replaces, and the
        // analysis quantiles are derived from exactly the returned histogram.
        let frames = vec![
            ImageFrame::new(0, 0, vec![]).unwrap(),
            ImageFrame::new(1, 1, vec![128, 128, 128, 255]).unwrap(),
            ImageFrame::new(
                3,
                2,
                vec![
                    3, 17, 91, 0, 42, 128, 211, 255, 255, 64, 7, 32, 99, 101, 203, 17, 180, 220,
                    12, 200, 71, 33, 88, 250,
                ],
            )
            .unwrap(),
            bench_like_frame(32),
        ];
        for frame in &frames {
            let combined = analyze_tone_with_histogram(frame);
            assert_eq!(combined.0, analyze_tone(frame));
            assert_eq!(combined.1, LuminanceHistogram::new(frame));
            // Same digest identity the histogram cache relies on.
            assert_eq!(
                combined.1.digest(),
                crate::LuminanceHistogram::new(frame).digest()
            );
        }
    }
}
