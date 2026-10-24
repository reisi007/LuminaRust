use super::*;

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
            (0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])) / 255.0
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

/// Current `suggest_auto_tone` formula applied to the ORIGINAL
/// (pre-R2-PERF-01) sorted-sample analysis. Both sides share
/// [`super::auto_tone_from_analysis`], so the drift comparison below
/// isolates the analysis-method difference (256-bin class marks vs.
/// sorted samples) from the AUTO-TONE-2 formula itself.
fn reference_suggest_auto_tone(
    frame: &ImageFrame,
    config: AutoToneConfig,
) -> Result<AutoToneResult, CoreError> {
    config.validate()?;
    let analysis = reference_analyze_tone(frame);
    Ok(super::auto_tone_from_analysis(&analysis, &config))
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
                3, 17, 91, 0, 42, 128, 211, 255, 255, 64, 7, 32, 99, 101, 203, 17, 180, 220, 12,
                200, 71, 33, 88, 250,
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
        // AUTO-TONE-2: both sides share `auto_tone_from_analysis`, so the
        // same 0.05 log-sensitivity budget covers contrast; the four
        // end/balance sliders are gain-1.0 linear maps of the quantiles
        // (input drift ≤ 2/256 through the soft limiter, whose slope never
        // exceeds 1), hence the same bound with wide margin.
        for (name, value, expected) in [
            ("contrast", opt_auto.contrast, ref_auto.contrast),
            ("whites", opt_auto.whites, ref_auto.whites),
            ("blacks", opt_auto.blacks, ref_auto.blacks),
            ("highlights", opt_auto.highlights, ref_auto.highlights),
            ("shadows", opt_auto.shadows, ref_auto.shadows),
        ] {
            assert!(
                (value - expected).abs() <= 0.05,
                "auto {name}: optimized {value} vs reference {expected}"
            );
        }

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
                3, 17, 91, 0, 42, 128, 211, 255, 255, 64, 7, 32, 99, 101, 203, 17, 180, 220, 12,
                200, 71, 33, 88, 250,
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
        // AUTO-TONE-2: zero span yields identity for all end/balance
        // sliders as well (uniform frames carry no end/balance signal).
        assert_eq!(result.whites, 0.0, "value {value}");
        assert_eq!(result.blacks, 0.0, "value {value}");
        assert_eq!(result.highlights, 0.0, "value {value}");
        assert_eq!(result.shadows, 0.0, "value {value}");
        if value == 0 {
            assert_eq!(result.exposure, AutoToneConfig::default().exposure_bounds.1);
        } else if value == 255 {
            assert_eq!(result.exposure, AutoToneConfig::default().exposure_bounds.0);
        } else {
            let expected = (AutoToneConfig::default().target_luminance / result.analysis.median)
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
    // AUTO-TONE-2: p01 = 0.0, median = 0.5, p99 = 1.0 sit in the linear
    // region, so the end/balance sliders are exact:
    // whites = 0.95 − 1.0 = −0.05, blacks = 0.0 − 0.05 = −0.05,
    // highlights = shadows = 0.4 − 0.5 = −0.1.
    assert!((result.whites - (-0.05)).abs() <= 1e-9);
    assert!((result.blacks - (-0.05)).abs() <= 1e-9);
    assert!((result.highlights - (-0.1)).abs() <= 1e-9);
    assert!((result.shadows - (-0.1)).abs() <= 1e-9);
}

#[test]
fn auto_tone_sets_all_six_sliders_on_an_asymmetric_frame() {
    // AUTO-TONE-2: an asymmetric photo-like ramp (dark-shifted median,
    // narrow span, lifted blacks, unbalanced halves) must populate all
    // six sliders with finite, in-domain, non-identity values — and none
    // may sit on a hard recipe limit although every target is reachable.
    // Bin mapping for gray g < 255 is bin g with mark (g+0.5)/256, so:
    // p01 ≈ 31.2/256, median = 65.5/256, p99 ≈ 194.9/256.
    let grays = [30u8, 40, 50, 60, 70, 80, 120, 200];
    let mut pixels = Vec::with_capacity(8 * 4);
    for gray in grays {
        pixels.extend_from_slice(&[gray, gray, gray, 255]);
    }
    let frame = ImageFrame::new(8, 1, pixels).unwrap();
    let result = suggest_auto_tone(&frame, AutoToneConfig::default()).unwrap();
    for (name, value) in [
        ("exposure", result.exposure),
        ("contrast", result.contrast),
        ("whites", result.whites),
        ("blacks", result.blacks),
        ("highlights", result.highlights),
        ("shadows", result.shadows),
    ] {
        assert!(value.is_finite(), "{name} must be finite");
        assert_ne!(value, 0.0, "{name} must be set on this frame");
    }
    // Median ≈ 0.256 < 0.5 → positive exposure, reachable (far from ±10).
    assert!(result.exposure > 0.5 && result.exposure < 10.0);
    // Span ≈ 0.64 → raw contrast ≈ 0.25 (linear region, exact passthrough).
    assert!((result.contrast - 0.25).abs() < 0.02);
    // Ends: whites = 0.95 − p99 ≈ 0.19, blacks = p01 − 0.05 ≈ 0.07.
    assert!(result.whites > 0.1 && result.whites < 0.3);
    assert!(result.blacks > 0.03 && result.blacks < 0.15);
    // Halves: upper ≈ 0.51 → highlights ≈ −0.11; lower ≈ 0.13 → shadows ≈ 0.27.
    assert!(result.highlights < -0.05 && result.highlights > -0.2);
    assert!(result.shadows > 0.2 && result.shadows < 0.35);
    // No slider on a hard limit.
    assert!(result.contrast.abs() < 1.0);
    assert!(result.whites.abs() < 1.0);
    assert!(result.blacks.abs() < 1.0);
    assert!(result.highlights.abs() < 1.0);
    assert!(result.shadows.abs() < 1.0);
    // Bit-exact determinism over the full six-slider result.
    for _ in 0..8 {
        assert_eq!(
            suggest_auto_tone(&frame, AutoToneConfig::default()).unwrap(),
            result
        );
    }
}

#[test]
fn narrow_span_contrast_is_soft_and_never_pins_to_one() {
    // Regression for the observed `Contrast = 100` bug: the old formula
    // `0.8/span − 1` hard-clamped to exactly 1.0 for narrow spans. The
    // soft limiter must compress toward the 0.9 asymptote instead.
    // Values 100..=140 → span ≈ 40/256 ≈ 0.156 → raw ≈ 4.1.
    let mut pixels = Vec::with_capacity(41 * 4);
    for gray in 100u8..=140 {
        pixels.extend_from_slice(&[gray, gray, gray, 255]);
    }
    let frame = ImageFrame::new(41, 1, pixels).unwrap();
    let result = suggest_auto_tone(&frame, AutoToneConfig::default()).unwrap();
    assert!(
        result.contrast > 0.8 && result.contrast < 1.0,
        "narrow-span contrast {} must be strong but strictly below 1.0",
        result.contrast
    );
    // Soft-cap asymptote is 0.9: even a near-degenerate span cannot pin.
    let mut pixels = Vec::with_capacity(64 * 4);
    for i in 0..64 {
        let gray = if i < 62 { 128u8 } else { 129u8 };
        pixels.extend_from_slice(&[gray, gray, gray, 255]);
    }
    let frame = ImageFrame::new(64, 1, pixels).unwrap();
    let result = suggest_auto_tone(&frame, AutoToneConfig::default()).unwrap();
    assert!(
        result.contrast <= 0.9 + 1e-12,
        "contrast {} must respect the 0.9 soft cap",
        result.contrast
    );
    assert!(result.contrast < 1.0);
}

#[test]
fn degenerate_frames_yield_zero_for_all_end_and_balance_sliders() {
    // Empty frames: all six sliders are identity (exposure 0.0 is the
    // documented `sample_count == 0` path).
    let empty = ImageFrame::new(0, 0, vec![]).unwrap();
    let result = suggest_auto_tone(&empty, AutoToneConfig::default()).unwrap();
    assert_eq!(result.exposure, 0.0);
    assert_eq!(result.contrast, 0.0);
    assert_eq!(result.whites, 0.0);
    assert_eq!(result.blacks, 0.0);
    assert_eq!(result.highlights, 0.0);
    assert_eq!(result.shadows, 0.0);
    // Uniform black/white: exposure takes the documented fallback bound,
    // every other slider stays identity (span = 0 ≤ epsilon gate).
    let black = ImageFrame::new(
        2,
        2,
        vec![0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255],
    )
    .unwrap();
    let result = suggest_auto_tone(&black, AutoToneConfig::default()).unwrap();
    assert_eq!(result.exposure, 10.0);
    assert_eq!(result.contrast, 0.0);
    assert_eq!(result.whites, 0.0);
    assert_eq!(result.blacks, 0.0);
    assert_eq!(result.highlights, 0.0);
    assert_eq!(result.shadows, 0.0);
    let white = ImageFrame::new(1, 1, vec![255, 255, 255, 255]).unwrap();
    let result = suggest_auto_tone(&white, AutoToneConfig::default()).unwrap();
    assert_eq!(result.exposure, -10.0);
    assert_eq!(result.contrast, 0.0);
    assert_eq!(result.whites, 0.0);
    assert_eq!(result.blacks, 0.0);
    assert_eq!(result.highlights, 0.0);
    assert_eq!(result.shadows, 0.0);
}

#[test]
fn soft_limit_is_identity_in_the_linear_region_and_capped_outside() {
    // Shape contract of the AUTO-TONE-2 limiter (used for every slider).
    assert_eq!(soft_limit(0.0, 0.6, 0.9), 0.0);
    assert_eq!(soft_limit(0.6, 0.6, 0.9), 0.6);
    assert_eq!(soft_limit(-0.6, 0.6, 0.9), -0.6);
    assert_eq!(soft_limit(0.25, 0.6, 0.9), 0.25);
    // Continuity at the shoulder: just above 0.6 the value barely moves.
    assert!((soft_limit(0.61, 0.6, 0.9) - 0.6058823529).abs() < 1e-9);
    // Oddness and asymptote: large magnitudes approach but never reach cap.
    assert_eq!(soft_limit(-3.0, 0.6, 0.9), -soft_limit(3.0, 0.6, 0.9));
    assert!(soft_limit(1e6, 0.6, 0.9) < 0.9);
    assert!(soft_limit(1e6, 0.6, 0.9) > 0.899);
    assert!(soft_limit(-1e6, 0.6, 0.9) > -0.9);
    // Monotone in magnitude.
    let mut previous = 0.0;
    for raw in [0.0, 0.3, 0.6, 1.0, 2.0, 5.0, 50.0] {
        let value = soft_limit(raw, 0.6, 0.9);
        assert!(value >= previous);
        previous = value;
    }
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
                3, 17, 91, 0, 42, 128, 211, 255, 255, 64, 7, 32, 99, 101, 203, 17, 180, 220, 12,
                200, 71, 33, 88, 250,
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
