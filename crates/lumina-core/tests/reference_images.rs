//! Reference-image tests for auto-tone and exposure matching (F-043).
//!
//! The fixtures in `fixtures/` are tiny (8x8), deterministically generated
//! PNGs with the exact pixel functions documented in `fixtures/README.md`
//! (programmatic provenance, no external sources, no license obligations).
//! Each fixture is embedded via `include_bytes!` and decoded with
//! `ImageFrame::decode` (= `image::load_from_memory` + `to_rgba8` + RGBA8
//! frame construction; the codebase has no separate `from_rgba8` constructor).
//!
//! Expected values are DERIVED from the documented fixture formulas first
//! (closed forms in the comments), then asserted with documented tolerances:
//!
//! - `analyze_tone` **mean**: 1e-9 — since R2-PERF-01 it is the exact
//!   pixel-order Rec.709 sum, deviating from the closed form only by f64
//!   rounding of the Rec.709 weight sum (which is `1.0` only up to one ULP).
//! - `analyze_tone` **median/p01/p99**: one bin width `1/256` (+1e-9 slack).
//!   Since R2-PERF-01 the quantiles are documented class-mark estimates over
//!   the shared 256-bin luminance histogram with a universal bound of exactly
//!   one bin width against the sorted-sample interpolation for EVERY frame
//!   (pipeline.md § Auto-Tone). The closed forms remain the physical
//!   reference; the checker fixture additionally pins exact reproduction
//!   (outer-bin edge marks) at the tight tolerance below.
//! - `suggest_auto_tone` exposure/contrast and `match_total_exposure`:
//!   0.01 (the task's default), absorbing `f64::log2` rounding and the
//!   clamped boundary cases. Where the value is pinned by a clamp or is
//!   exactly representable, a tighter bound is documented in the test.
//!   The gradient fixture's auto-tone exposure uses the bin-quantized median,
//!   so its tolerance carries the documented amplification
//!   `(1/256)/(m·ln2)` ≈ 0.046 EV at m ≈ 0.1235 → 0.05.

use lumina_core::{
    analyze_tone, match_total_exposure, suggest_auto_tone, AutoToneConfig, ImageFrame,
};

/// Documented default tolerance for log2/clamp-derived expectations.
const LOG_TOLERANCE: f64 = 0.01;
/// Documented tolerance for formula-derived `analyze_tone` **mean**
/// (exact closed forms; only f64 rounding of the Rec.709 weight sum).
const STAT_TOLERANCE: f64 = 1e-9;
/// Documented R2-PERF-01 tolerance for formula-derived `analyze_tone`
/// **quantiles**: the universal one-bin bound of the 256-bin class-mark
/// estimator plus floating-point slack.
const QUANTILE_TOLERANCE: f64 = 1.0 / 256.0 + 1e-9;
/// Tolerance for auto-tone **exposure** expectations that flow through a
/// bin-quantized median: one bin of median shift is amplified by the log2
/// sensitivity `1/(m·ln2)`; at the smallest fixture median (gradient,
/// m ≈ 0.1235) that is ≈ 0.046 EV, plus f64 rounding → 0.05.
const BIN_MEDIAN_EXPOSURE_TOLERANCE: f64 = 0.05;

fn load_gradient() -> ImageFrame {
    ImageFrame::decode(include_bytes!("fixtures/reference_gradient.png")).unwrap()
}

fn load_checker() -> ImageFrame {
    ImageFrame::decode(include_bytes!("fixtures/reference_checker.png")).unwrap()
}

fn load_zone() -> ImageFrame {
    ImageFrame::decode(include_bytes!("fixtures/reference_zone.png")).unwrap()
}

// ---------------------------------------------------------------------------
// reference_gradient.png: 8x8 row-major linear gray ramp, value = y*8 + x
// (0..=63). Sorted values are exactly 0, 1, …, 63.
//
//   mean   = (0+..+63)/64        = 31.5/255   ≈ 0.1235294118
//   median = (v[31]+v[32])/2     = (31+32)/2  = 31.5/255   (position 31.5)
//   p01    = v[0] + 1*0.63       = 0.63/255   ≈ 0.0024705882 (position 0.63)
//   p99    = v[62] + 1*0.37      = 62.37/255  ≈ 0.2445882353 (position 62.37)
//   span   = (62.37-0.63)/255    = 61.74/255  ≈ 0.2421176471
//   exposure = log2(0.5/mean)    = log2(255/63) ≈ 2.0171
//   contrast  = 0.8/span - 1     ≈ 2.3042  -> clamped to contrast_bounds.1 = 1.0
// ---------------------------------------------------------------------------
#[test]
fn gradient_analyze_tone_matches_formula() {
    let frame = load_gradient();
    assert_eq!((frame.width, frame.height), (8, 8));
    // Structural fixture guard (formula spot check): first and last pixel.
    assert_eq!(&frame.pixels[0..4], &[0, 0, 0, 255]);
    assert_eq!(&frame.pixels[frame.pixels.len() - 4..], &[63, 63, 63, 255]);

    let analysis = analyze_tone(&frame);
    assert_eq!(analysis.sample_count, 64);
    assert!(
        (analysis.mean - 31.5 / 255.0).abs() < STAT_TOLERANCE,
        "gradient mean {} != 31.5/255",
        analysis.mean
    );
    // R2-PERF-01: median/p01/p99 are documented class-mark estimates over the
    // shared 256-bin histogram; universal bound one bin width against the
    // closed forms. Actual values here: median 32/256 = 0.125, p01 = 0.0
    // (bin-0 lower edge), p99 = 62.5/256.
    assert!(
        (analysis.median - 31.5 / 255.0).abs() < QUANTILE_TOLERANCE,
        "gradient median {} != 31.5/255 within {QUANTILE_TOLERANCE}",
        analysis.median
    );
    assert!(
        (analysis.p01 - 0.63 / 255.0).abs() < QUANTILE_TOLERANCE,
        "gradient p01 {} != 0.63/255 within {QUANTILE_TOLERANCE}",
        analysis.p01
    );
    assert!(
        (analysis.p99 - 62.37 / 255.0).abs() < QUANTILE_TOLERANCE,
        "gradient p99 {} != 62.37/255 within {QUANTILE_TOLERANCE}",
        analysis.p99
    );
}

#[test]
fn gradient_auto_tone_and_matching_follow_formula() {
    let frame = load_gradient();
    let result = suggest_auto_tone(&frame, AutoToneConfig::default()).unwrap();
    // exposure = log2(0.5 / (31.5/255)) = log2(255/63) ≈ 2.0171.
    // R2-PERF-01: the auto exposure derives from the bin-quantized median
    // (32/256 = 0.125 → log2 = 2.0), so the documented amplified tolerance
    // applies (see BIN_MEDIAN_EXPOSURE_TOLERANCE).
    let expected_exposure = (0.5f64 / (31.5f64 / 255.0)).log2();
    assert!(
        (result.exposure - expected_exposure).abs() < BIN_MEDIAN_EXPOSURE_TOLERANCE,
        "gradient exposure {} != ~2.0171 ({expected_exposure})",
        result.exposure
    );
    // contrast = 0.8/span - 1 ≈ 2.3042 -> clamped exactly to the upper bound
    // contrast_bounds.1 = 1.0 of the default configuration. The span
    // (p99 - p01 = 62.5/256) stays far above the clamp threshold, so the
    // pinned exact bound is preserved.
    assert!(
        (result.contrast - 1.0).abs() < 1e-12,
        "gradient contrast {} != clamped upper bound 1.0",
        result.contrast
    );
    let delta = match_total_exposure(&frame, 0.5).unwrap();
    assert!(
        (delta - expected_exposure).abs() < LOG_TOLERANCE,
        "gradient match delta {delta} != ~2.0171 ({expected_exposure})"
    );
}

// ---------------------------------------------------------------------------
// reference_checker.png: 8x8 black/white checkerboard, white where (x+y) is
// even -> exactly 32 black (luminance 0.0) and 32 white (luminance 1.0 up to
// one ULP of the Rec.709 weight sum) pixels.
//
//   mean    = 32*1.0/64  = 0.5 (up to ~1e-16)
//   median  = (v[31]+v[32])/2 = (0+1)/2 = 0.5        (position 31.5)
//   p01     = v[0] + 0*(0.63) = 0.0                  (position 0.63)
//   p99     = v[62] + 0*(0.37) = 1.0                 (position 62.37)
//   span    = 1.0
//   exposure = log2(0.5/0.5) = log2(1) = 0.0
//   contrast = 0.8/1.0 - 1 = -0.2
//   match_total_exposure(frame, 0.5): mean ~ 0.5 ->
//     delta = log2(0.5/0.5) = log2(1) = 0.0.
//     NOTE: the task brief's example "checker -> -1.0" would correspond to a
//     fully WHITE frame (mean 1.0 -> log2(0.5/1.0) = -1.0); for this 50/50
//     checker the formula-first expectation is 0.0.
// ---------------------------------------------------------------------------
#[test]
fn checker_analyze_tone_matches_formula() {
    let frame = load_checker();
    assert_eq!((frame.width, frame.height), (8, 8));
    assert_eq!(&frame.pixels[0..4], &[255, 255, 255, 255]); // (0,0) even -> white
    assert_eq!(
        &frame.pixels[frame.pixels.len() - 4..],
        &[255, 255, 255, 255]
    ); // (7,7) even

    let analysis = analyze_tone(&frame);
    assert_eq!(analysis.sample_count, 64);
    // R2-PERF-01: this bimodal fixture pins the exactness properties of the
    // class-mark estimator: bin 0's lower edge mark reproduces p01 = 0.0
    // exactly, bin 255's upper edge mark reproduces p99 = 1.0 exactly, and
    // the cross-gap interpolation between the two edge marks places the
    // median exactly at 0.5 (the sorted-sample value).
    assert!(
        (analysis.mean - 0.5).abs() < STAT_TOLERANCE,
        "checker mean {} != 0.5",
        analysis.mean
    );
    assert!(
        (analysis.median - 0.5).abs() < STAT_TOLERANCE,
        "checker median {} != 0.5",
        analysis.median
    );
    assert!(
        (analysis.p01 - 0.0).abs() < STAT_TOLERANCE,
        "checker p01 {} != 0.0",
        analysis.p01
    );
    assert!(
        (analysis.p99 - 1.0).abs() < STAT_TOLERANCE,
        "checker p99 {} != 1.0",
        analysis.p99
    );
}

#[test]
fn checker_auto_tone_and_matching_follow_formula() {
    let frame = load_checker();
    let result = suggest_auto_tone(&frame, AutoToneConfig::default()).unwrap();
    assert!(
        (result.exposure - 0.0).abs() < LOG_TOLERANCE,
        "checker exposure {} != 0.0",
        result.exposure
    );
    assert!(
        (result.contrast - (-0.2)).abs() < LOG_TOLERANCE,
        "checker contrast {} != -0.2",
        result.contrast
    );
    // mean is 0.5 up to ~1e-16, so log2(0.5/mean) = log2(1 ± 2e-16) ≈ 0.0;
    // the residual is far below 1e-9 (delta = log2 of a value within a few
    // ULP of 1.0).
    let delta = match_total_exposure(&frame, 0.5).unwrap();
    assert!(
        (delta - 0.0).abs() < 1e-9,
        "checker match delta {delta} != 0.0 within 1e-9"
    );
}

// ---------------------------------------------------------------------------
// reference_zone.png: 8x8 with four 4x4 tone zones: top-left 20, top-right
// 90, bottom-left 160, bottom-right 230. Sorted values: 16x 20, 16x 90,
// 16x 160, 16x 230.
//
//   mean    = (20+90+160+230)/4 = 125/255    ≈ 0.4901960784
//   median  = (v[31]+v[32])/2   = (90+160)/2 = 125/255  (position 31.5:
//             v[16..31] = 90, v[32..47] = 160)
//   p01     = 20/255            ≈ 0.0784313725 (position 0.63 -> v[0]=v[1]=20)
//   p99     = 230/255           ≈ 0.9019607843 (position 62.37 -> v[62]=v[63]=230)
//   span    = 210/255           ≈ 0.8235294118
//   exposure = log2(0.5/(125/255)) = log2(1.02) ≈ 0.02857
//   contrast = 0.8/span - 1 = 0.8*255/210 - 1 = -1/35 ≈ -0.0285714
// ---------------------------------------------------------------------------
#[test]
fn zone_analyze_tone_matches_formula() {
    let frame = load_zone();
    assert_eq!((frame.width, frame.height), (8, 8));
    assert_eq!(&frame.pixels[0..4], &[20, 20, 20, 255]); // top-left zone
    assert_eq!(
        &frame.pixels[frame.pixels.len() - 4..],
        &[230, 230, 230, 255]
    ); // bottom-right

    let analysis = analyze_tone(&frame);
    assert_eq!(analysis.sample_count, 64);
    assert!(
        (analysis.mean - 125.0 / 255.0).abs() < STAT_TOLERANCE,
        "zone mean {} != 125/255",
        analysis.mean
    );
    // R2-PERF-01: quantiles are documented class-mark estimates (universal
    // one-bin bound). Actual values here: median 125.5/256, p01 = 20.5/256,
    // p99 = 230.5/256.
    assert!(
        (analysis.median - 125.0 / 255.0).abs() < QUANTILE_TOLERANCE,
        "zone median {} != 125/255 within {QUANTILE_TOLERANCE}",
        analysis.median
    );
    assert!(
        (analysis.p01 - 20.0 / 255.0).abs() < QUANTILE_TOLERANCE,
        "zone p01 {} != 20/255 within {QUANTILE_TOLERANCE}",
        analysis.p01
    );
    assert!(
        (analysis.p99 - 230.0 / 255.0).abs() < QUANTILE_TOLERANCE,
        "zone p99 {} != 230/255 within {QUANTILE_TOLERANCE}",
        analysis.p99
    );
}

#[test]
fn zone_auto_tone_and_matching_follow_formula() {
    let frame = load_zone();
    let result = suggest_auto_tone(&frame, AutoToneConfig::default()).unwrap();
    let expected_exposure = (0.5f64 / (125.0f64 / 255.0)).log2(); // log2(1.02) ≈ 0.02857
    assert!(
        (result.exposure - expected_exposure).abs() < LOG_TOLERANCE,
        "zone exposure {} != ~0.02857 ({expected_exposure})",
        result.exposure
    );
    let expected_contrast = 0.8 / (210.0 / 255.0) - 1.0; // -1/35 ≈ -0.0285714
    assert!(
        (result.contrast - expected_contrast).abs() < LOG_TOLERANCE,
        "zone contrast {} != -1/35 ({expected_contrast})",
        result.contrast
    );
    let delta = match_total_exposure(&frame, 0.5).unwrap();
    assert!(
        (delta - expected_exposure).abs() < LOG_TOLERANCE,
        "zone match delta {delta} != ~0.02857 ({expected_exposure})"
    );
}

/// Documentation probe (F-043): the three fixtures are ordered by their
/// formula-derived means — gradient (0.1235) < zone (0.4902) < checker (0.5)
/// — and `analyze_tone` must reproduce exactly that ordering.
#[test]
fn fixture_means_are_monotone_across_fixtures() {
    let gradient = analyze_tone(&load_gradient()).mean;
    let zone = analyze_tone(&load_zone()).mean;
    let checker = analyze_tone(&load_checker()).mean;
    assert!(
        gradient < zone && zone < checker,
        "expected mean gradient {gradient} < zone {zone} < checker {checker}"
    );
}
