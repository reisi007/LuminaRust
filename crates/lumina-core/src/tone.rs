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
    /// Lightroom-style end/balance sliders (AUTO-TONE-2, normativ:
    /// `feature/architecture/pipeline.md` § Auto-Tone, Mehrregler-Absatz
    /// 2026-09-04). Domains match the recipe validation in `lib.rs`:
    /// each in `-1..=1`. Consumers persist them into `recipe.adjustments`
    /// under the keys `whites`, `blacks`, `highlights`, `shadows`
    /// (GUI/CLI map `result.<field>` 1:1 onto the same-named key).
    pub whites: f64,
    pub blacks: f64,
    pub highlights: f64,
    pub shadows: f64,
}

/// Targets of the AUTO-TONE-2 six-slider distribution (same 256-bin
/// p01/median/p99 pass, no extra measurement):
/// - contrast drives the p01/p99 span toward `0.8` (unchanged target);
/// - whites/blacks straighten the p99/p01 ends toward `0.95`/`0.05`;
/// - highlights/shadows balance each half-span (p99−median, median−p01)
///   toward `0.4` (= half the span target, so a balanced frame yields `0.0`).
const AUTO_TONE_TARGET_SPAN: f64 = 0.8;
const AUTO_TONE_WHITE_TARGET: f64 = 0.95;
const AUTO_TONE_BLACK_TARGET: f64 = 0.05;
const AUTO_TONE_HALF_SPAN_TARGET: f64 = 0.4;
/// Soft-limit shape (AUTO-TONE-2): the raw value passes through unchanged
/// inside `|raw| <= linear`; beyond that it compresses with shoulder `0.5`
/// toward the asymptote `cap` without ever reaching it. Contrast caps at
/// `0.9`, the four end/balance sliders at `0.8` — auto-tone therefore never
/// pins a slider at the hard `±1` recipe limit on reachable targets (the
/// observed `Contrast = 100` bug). Exposure keeps its documented
/// black/white fallback bounds (`±10` EV); those are the explicit
/// unreachable-target branches, not silent clamps.
const CONTRAST_LINEAR_REGION: f64 = 0.6;
const CONTRAST_SOFT_CAP: f64 = 0.9;
const TONE_LINEAR_REGION: f64 = 0.6;
const TONE_SOFT_CAP: f64 = 0.8;
const SOFT_SHOULDER: f64 = 0.5;

/// Soft limiter for auto-tone sliders: odd, monotone, continuous at
/// `linear` (`linear + 0 = linear`), asymptotically approaching `cap`.
/// Deterministic closed form; finite in, finite out.
fn soft_limit(raw: f64, linear: f64, cap: f64) -> f64 {
    let magnitude = raw.abs();
    if magnitude <= linear {
        return raw;
    }
    let excess = magnitude - linear;
    raw.signum() * (linear + (cap - linear) * excess / (excess + SOFT_SHOULDER))
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

/// Shared AUTO-TONE-2 slider formula over an already computed analysis
/// (used by [`suggest_auto_tone`] with the 256-bin analysis and by the test
/// reference with the sorted-sample analysis, so the drift comparison stays
/// meaningful after the algorithm change).
///
/// - `exposure`: `log2(target/median)` with the documented empty/black/white
///   fallback branches, clamped to `exposure_bounds` (bit-identical to before).
/// - `contrast`: `target_span/span − 1`, softly limited (never hard `±1`).
/// - `whites`/`blacks`: `(white_target − p99)` / `(p01 − black_target)`.
/// - `highlights`/`shadows`: `(half_span_target − upper_half)` /
///   `(half_span_target − lower_half)` with `upper_half = p99 − median`,
///   `lower_half = median − p01`.
/// - Degenerate frames (`sample_count == 0`) and zero-span frames
///   (`span <= epsilon`, covering uniform black/white) yield `0.0` for all
///   five sliders except the exposure fallback — the uniform-frame identity
///   (`contrast == 0.0`) is preserved bit-exactly and extended to the four
///   new sliders.
fn auto_tone_from_analysis(analysis: &ToneAnalysis, config: &AutoToneConfig) -> AutoToneResult {
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
    let (contrast, whites, blacks, highlights, shadows) =
        if analysis.sample_count == 0 || span <= epsilon {
            (0.0, 0.0, 0.0, 0.0, 0.0)
        } else {
            let contrast = soft_limit(
                AUTO_TONE_TARGET_SPAN / span - 1.0,
                CONTRAST_LINEAR_REGION,
                CONTRAST_SOFT_CAP,
            )
            .clamp(config.contrast_bounds.0, config.contrast_bounds.1);
            let whites = soft_limit(
                AUTO_TONE_WHITE_TARGET - analysis.p99,
                TONE_LINEAR_REGION,
                TONE_SOFT_CAP,
            )
            .clamp(-1.0, 1.0);
            let blacks = soft_limit(
                analysis.p01 - AUTO_TONE_BLACK_TARGET,
                TONE_LINEAR_REGION,
                TONE_SOFT_CAP,
            )
            .clamp(-1.0, 1.0);
            let highlights = soft_limit(
                AUTO_TONE_HALF_SPAN_TARGET - (analysis.p99 - analysis.median),
                TONE_LINEAR_REGION,
                TONE_SOFT_CAP,
            )
            .clamp(-1.0, 1.0);
            let shadows = soft_limit(
                AUTO_TONE_HALF_SPAN_TARGET - (analysis.median - analysis.p01),
                TONE_LINEAR_REGION,
                TONE_SOFT_CAP,
            )
            .clamp(-1.0, 1.0);
            (contrast, whites, blacks, highlights, shadows)
        };
    AutoToneResult {
        analysis: *analysis,
        exposure: finite(exposure),
        contrast: finite(contrast),
        whites: finite(whites),
        blacks: finite(blacks),
        highlights: finite(highlights),
        shadows: finite(shadows),
    }
}

pub fn suggest_auto_tone(
    frame: &ImageFrame,
    config: AutoToneConfig,
) -> Result<AutoToneResult, CoreError> {
    config.validate()?;
    let analysis = analyze_tone(frame);
    Ok(auto_tone_from_analysis(&analysis, &config))
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
mod tests;
#[cfg(test)]
mod tests_reference;
