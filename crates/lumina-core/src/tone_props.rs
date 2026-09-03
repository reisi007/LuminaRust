//! Property tests for auto-tone and exposure matching (F-043).
//!
//! These are real proptest properties (in contrast to the deterministic
//! invariant tests in `tone.rs`): every property is checked over generated
//! frames, configs and targets. Every property is formed **exactly** from the
//! implementation in `tone.rs` — the source semantics are quoted or mirrored
//! branch-for-branch in the comments where a boundary matters (epsilon
//! fallbacks, clamps, identity paths). None of the properties asserts a
//! guessed expectation; each is the documented invariant of the code.
//!
//! Frames are deliberately tiny (<= 16x16) so the whole module stays well
//! below the ~20 s CI budget: the heavy properties (frame generation, masked
//! matching) run with 64 cases, the light ones with proptest's default (256).
//! The `proptest!` macro reports the failing seed on any failure, so every
//! counter-example is replayable without a fixed global seed.

use crate::masks::MaskPlane;
use crate::tone::{
    analyze_tone, match_total_exposure, match_total_exposure_masked, suggest_auto_tone,
    tone_fingerprint, AutoToneConfig,
};
use crate::{CoreError, ImageFrame};
use proptest::prelude::*;

/// Second configuration for the bounds property: non-default bounds so the
/// property is not tied to the default `(-10, 10)` / `(-1, 1)` values.
/// `contrast_bounds` deliberately contains `0.0` — see the source-boundary
/// note in [`auto_tone_uniform_frame_has_zero_contrast`].
const CUSTOM_CONFIG: AutoToneConfig = AutoToneConfig {
    target_luminance: 0.62,
    epsilon: 1e-5,
    exposure_bounds: (-3.5, 4.25),
    contrast_bounds: (-0.75, 0.8),
};

/// Small random RGBA8 frames (1..=16 pixels per side) with arbitrary
/// per-channel values — including arbitrary alphas (alpha is ignored by the
/// tone domain, which the properties exercise implicitly).
fn frame_strategy() -> impl Strategy<Value = ImageFrame> {
    (1u32..=16, 1u32..=16).prop_flat_map(|(width, height)| {
        prop::collection::vec(any::<u8>(), width as usize * height as usize * 4)
            .prop_map(move |pixels| ImageFrame::new(width, height, pixels).unwrap())
    })
}

/// Like [`frame_strategy`], plus the empty 0x0 frame (the documented
/// `sample_count == 0` path of `suggest_auto_tone`).
fn any_frame_strategy() -> impl Strategy<Value = ImageFrame> {
    prop_oneof![
        Just(ImageFrame::new(0, 0, vec![]).unwrap()).boxed(),
        frame_strategy().boxed(),
    ]
}

/// A `(darker, brighter)` pair: `brighter` is `darker` with every pixel's red
/// channel raised by 10 (saturating), and the first pixel's red channel is
/// forced below 246 first so the brightening is strictly effective at least
/// once. Because the Rec.709 weights are all positive, luminance is pointwise
/// `>=` and strictly greater for pixel 0 — the premise of the monotonicity
/// properties.
fn brighter_pair_strategy() -> impl Strategy<Value = (ImageFrame, ImageFrame)> {
    frame_strategy().prop_map(|mut darker| {
        darker.pixels[0] = darker.pixels[0].min(245);
        let mut brighter = darker.clone();
        for pixel in brighter.pixels.as_chunks_mut::<4>().0 {
            pixel[0] = pixel[0].saturating_add(10);
        }
        (darker, brighter)
    })
}

/// Targets outside the valid `0..=1` domain (including the non-finite
/// specials) that both validation entry points must reject.
fn invalid_target_strategy() -> impl Strategy<Value = f64> {
    prop_oneof![
        (-100.0f64..0.0).boxed(),
        (1.0..100.0).boxed(),
        Just(f64::NAN).boxed(),
        Just(f64::INFINITY).boxed(),
        Just(f64::NEG_INFINITY).boxed(),
    ]
}

/// A frame together with 1..=2 binary (0 / `u16::MAX`) planes covering it.
/// The first pixel is always fully visible so the reduced subframe is
/// non-empty and the weighted-vs-plain comparison is well-defined.
fn frame_with_binary_planes_strategy() -> impl Strategy<Value = (ImageFrame, Vec<MaskPlane>)> {
    frame_strategy().prop_flat_map(|frame| {
        let count = frame.width as usize * frame.height as usize;
        let plane_strategy = prop::collection::vec(any::<bool>(), count).prop_map(move |bits| {
            let values: Vec<u16> = bits
                .iter()
                .enumerate()
                .map(
                    |(index, visible)| {
                        if index == 0 || *visible {
                            u16::MAX
                        } else {
                            0
                        }
                    },
                )
                .collect();
            MaskPlane::new(frame.width, frame.height, values).unwrap()
        });
        (Just(frame), prop::collection::vec(plane_strategy, 1..=2))
    })
}

/// Extracts the pixels of `frame` that are fully visible (weight `u16::MAX`)
/// in every layer as a one-row `ImageFrame` — the expected equivalent of the
/// weighted measurement for 0/`u16::MAX` planes.
fn visible_subframe(frame: &ImageFrame, layers: &[MaskPlane]) -> ImageFrame {
    let count = frame.width as usize * frame.height as usize;
    let mut pixels = Vec::with_capacity(count * 4);
    for index in 0..count {
        if layers.iter().all(|plane| plane.values[index] == u16::MAX) {
            pixels.extend_from_slice(&frame.pixels[index * 4..index * 4 + 4]);
        }
    }
    let n = pixels.len() / 4;
    ImageFrame::new(n as u32, 1, pixels).unwrap()
}

/// A plane that is guaranteed to mismatch `frame` in width, height or value
/// count. Constructed directly (bypassing `MaskPlane::new`, which would
/// reject the invalid dimensions itself): the matching entry point must
/// reject what the constructor would never accept.
fn mismatched_plane_strategy(frame: &ImageFrame) -> impl Strategy<Value = MaskPlane> {
    let fw = frame.width;
    let fh = frame.height;
    let fcount = fw as usize * fh as usize;
    prop_oneof![
        Just(MaskPlane {
            width: fw + 1,
            height: fh,
            values: vec![0; fcount],
        }),
        Just(MaskPlane {
            width: fw,
            height: fh + 1,
            values: vec![0; fcount],
        }),
        Just(MaskPlane {
            width: fw,
            height: fh,
            values: vec![0; fcount + 1],
        }),
        Just(MaskPlane {
            width: fw,
            height: fh,
            values: vec![],
        }),
    ]
}

// Heavy properties: 64 cases each. Frame generation and masked matching are
// the expensive parts; 64 cases x <= 16x16 frames keep the total runtime far
// below the ~20 s budget while still exploring the value space.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Invariant (F-043, Wertebereiche; AUTO-TONE-2): for every frame
    /// (including the empty frame) and both configurations,
    /// `suggest_auto_tone` returns `Ok` with a finite exposure inside
    /// `exposure_bounds`, a finite contrast inside `contrast_bounds`, and
    /// finite end/balance sliders inside the recipe domains `-1..=1` (the
    /// validation in `lib.rs` rejects anything outside). Both contrast bounds
    /// keep `0.0` inside `contrast_bounds`; the reason is the source boundary
    /// documented on `auto_tone_uniform_frame_has_zero_contrast` (the
    /// `span <= epsilon` identity path returns 0.0 unclamped).
    #[test]
    fn auto_tone_bounds_and_finite(frame in any_frame_strategy()) {
        for config in [AutoToneConfig::default(), CUSTOM_CONFIG] {
            let result = suggest_auto_tone(&frame, config).unwrap();
            prop_assert!(result.exposure.is_finite(), "exposure must be finite");
            prop_assert!(
                (config.exposure_bounds.0..=config.exposure_bounds.1)
                    .contains(&result.exposure),
                "exposure {} outside {:?}",
                result.exposure,
                config.exposure_bounds,
            );
            prop_assert!(result.contrast.is_finite(), "contrast must be finite");
            prop_assert!(
                (config.contrast_bounds.0..=config.contrast_bounds.1)
                    .contains(&result.contrast),
                "contrast {} outside {:?}",
                result.contrast,
                config.contrast_bounds,
            );
            for (name, value) in [
                ("whites", result.whites),
                ("blacks", result.blacks),
                ("highlights", result.highlights),
                ("shadows", result.shadows),
            ] {
                prop_assert!(value.is_finite(), "{name} must be finite");
                prop_assert!(
                    (-1.0..=1.0).contains(&value),
                    "{name} {value} outside -1..=1"
                );
            }
        }
    }

    /// Invariant (AUTO-TONE-2, kein Anschlag): the soft limiter caps contrast
    /// at `0.9` and the end/balance sliders at `0.8`, so on reachable targets
    /// no tone slider sits on the hard `±1` recipe limit. Exposure is
    /// excluded: the documented black/white fallbacks legitimately produce
    /// the `±10` bounds on degenerate frames.
    #[test]
    fn auto_tone_sliders_never_pin_to_the_hard_limit(frame in any_frame_strategy()) {
        let config = AutoToneConfig::default();
        let result = suggest_auto_tone(&frame, config).unwrap();
        for (name, value) in [
            ("contrast", result.contrast),
            ("whites", result.whites),
            ("blacks", result.blacks),
            ("highlights", result.highlights),
            ("shadows", result.shadows),
        ] {
            prop_assert!(
                value.abs() < 1.0,
                "{name} {value} sits on the hard limit"
            );
        }
    }

    /// Invariant (F-043, Monotonie): a brighter frame never gets a larger
    /// auto exposure than a darker frame for the identical configuration. The
    /// median is monotone in the pixel values, `log2(target/median)` falls
    /// with rising median, and the epsilon/black-white fallbacks only ever
    /// produce the two extreme bounds — so `<=` is robust at the boundaries.
    #[test]
    fn auto_tone_exposure_is_monotonic_in_brightness((darker, brighter) in brighter_pair_strategy()) {
        let config = AutoToneConfig::default();
        let exposure_darker = suggest_auto_tone(&darker, config).unwrap().exposure;
        let exposure_brighter = suggest_auto_tone(&brighter, config).unwrap().exposure;
        prop_assert!(
            exposure_brighter <= exposure_darker,
            "brighter frame exposure {exposure_brighter} > darker frame exposure {exposure_darker}"
        );
    }

    /// Invariant (F-043, Rahmen): for every frame and target in `0..=1`,
    /// `match_total_exposure` returns a finite delta in `-10..=10`.
    #[test]
    fn matching_delta_bounds_and_finite(frame in frame_strategy(), target in 0.0f64..=1.0) {
        let delta = match_total_exposure(&frame, target).unwrap();
        prop_assert!(delta.is_finite(), "delta {} must be finite", delta);
        prop_assert!(
            (-10.0..=10.0).contains(&delta),
            "delta {delta} outside -10..=10"
        );
    }

    /// Invariant (F-043, Monotonie): for a fixed target, a brighter frame
    /// yields a smaller-or-equal delta than a darker frame (same mean-based
    /// `log2(target/mean)` reasoning as the auto-tone monotonicity).
    #[test]
    fn matching_is_monotonic_in_brightness((darker, brighter) in brighter_pair_strategy(), target in 0.0f64..=1.0) {
        let delta_darker = match_total_exposure(&darker, target).unwrap();
        let delta_brighter = match_total_exposure(&brighter, target).unwrap();
        prop_assert!(
            delta_brighter <= delta_darker,
            "brighter frame delta {delta_brighter} > darker frame delta {delta_darker} (target {target})"
        );
    }

    /// Invariant (F-043, Monotonie): for a fixed frame, the delta rises
    /// monotonically with the target (`log2(target/mean)` grows with `target`;
    /// the epsilon paths only pin `-10.0` / `10.0`).
    #[test]
    fn matching_is_monotonic_in_target(frame in frame_strategy(), a in 0.0f64..=1.0, b in 0.0f64..=1.0) {
        let (low, high) = if a <= b { (a, b) } else { (b, a) };
        let delta_low = match_total_exposure(&frame, low).unwrap();
        let delta_high = match_total_exposure(&frame, high).unwrap();
        prop_assert!(
            delta_low <= delta_high,
            "delta({low}) = {delta_low} > delta({high}) = {delta_high}"
        );
    }

    /// Invariant (F-041, F-043): an empty `mask_layers` slice is bit-exactly
    /// identical to `match_total_exposure` (the documented delegation).
    #[test]
    fn masked_empty_slice_equals_plain(frame in frame_strategy(), target in 0.0f64..=1.0) {
        prop_assert_eq!(
            match_total_exposure_masked(&frame, target, &[]).unwrap(),
            match_total_exposure(&frame, target).unwrap(),
        );
    }

    /// Invariant (F-041, F-043): a single fully-white plane (all `u16::MAX`)
    /// is bit-exactly identical to the unmasked measurement — every weight is
    /// exactly `1.0`, so the weighted mean is the plain mean.
    #[test]
    fn masked_all_max_plane_equals_plain(frame in frame_strategy(), target in 0.0f64..=1.0) {
        let plane = MaskPlane::new(
            frame.width,
            frame.height,
            vec![u16::MAX; frame.width as usize * frame.height as usize],
        )
        .unwrap();
        prop_assert_eq!(
            match_total_exposure_masked(&frame, target, &[plane]).unwrap(),
            match_total_exposure(&frame, target).unwrap(),
        );
    }

    /// Invariant (F-041, F-043): the same with two fully-white layers — the
    /// product `1.0 * 1.0` still yields weight exactly `1.0`.
    #[test]
    fn masked_two_all_max_layers_equals_plain(frame in frame_strategy(), target in 0.0f64..=1.0) {
        let plane = MaskPlane::new(
            frame.width,
            frame.height,
            vec![u16::MAX; frame.width as usize * frame.height as usize],
        )
        .unwrap();
        prop_assert_eq!(
            match_total_exposure_masked(&frame, target, &[plane.clone(), plane]).unwrap(),
            match_total_exposure(&frame, target).unwrap(),
        );
    }

    /// Invariant (F-041, F-043): a mask whose pixels are either fully masked
    /// (0) or fully visible (`u16::MAX`) reduces the measurement to the
    /// visible subset — `match_total_exposure_masked` on the full frame equals
    /// `match_total_exposure` on the extracted subframe. Covers one and two
    /// layers (the intersection product).
    ///
    /// TOLERANCE (documented): the comparison is `|delta_masked -
    /// delta_plain| < 1e-9`, not bit-exact. Mathematically both deltas are
    /// `log2(target / mean_visible)`, but the two code paths sum in different
    /// expressions — the plain matcher accumulates the unweighted pixel-order
    /// luminance mean, the masked loop accumulates weighted luminances in
    /// row-major order. f64 addition is not associative, so the results can
    /// differ in the last bit (≈1 ULP), which `log2` amplifies by at most
    /// ~1e-13 in the measurement range of these frames. 1e-9 is two to four
    /// orders of magnitude above that and far below any semantic difference;
    /// the intersection/subset character of the property (visible pixels,
    /// product of layers) is asserted exactly via [`visible_subframe`].
    #[test]
    fn masked_binary_planes_reduce_to_subframe(
        (frame, layers) in frame_with_binary_planes_strategy(),
        target in 0.0f64..=1.0,
    ) {
        let subframe = visible_subframe(&frame, &layers);
        prop_assert!(
            subframe.pixels.len() >= 4,
            "the forced first visible pixel must keep the subframe non-empty"
        );
        let masked = match_total_exposure_masked(&frame, target, &layers).unwrap();
        let plain = match_total_exposure(&subframe, target).unwrap();
        prop_assert!(
            (masked - plain).abs() < 1e-9,
            "masked delta {masked} != plain subframe delta {plain} (target {target}): \
             deviation exceeds the documented 1e-9 f64-rounding tolerance"
        );
    }
}

// Light properties: proptest's default 256 cases each. All of these are
// cheap (single pixels, fingerprints, validation errors), so the default
// case count is fine.
proptest! {
    /// Invariant (F-043, Schwarz-/Weißbild-Pfade): the documented fallbacks
    /// `median <= epsilon -> exposure_bounds.1` (black) and
    /// `median >= 1 - epsilon -> exposure_bounds.0` (white). Values 1..=3
    /// stay on the regular `log2` path because the analysis median is > epsilon
    /// for `v >= 1` (and `< 1 - epsilon` for `v <= 3`). The expected exposure
    /// mirrors the source branch-for-branch, including the final clamp.
    ///
    /// R2-PERF-01: the analysis median is the documented 256-bin class-mark
    /// estimate (`bin(v) = v` for gray levels; bin 0 reports its lower edge
    /// `0.0`, bin 255 its upper edge `1.0`, interior bins their center), so
    /// the expectation uses that exact median — keeping the branch structure
    /// assertions bit-exact.
    #[test]
    fn auto_tone_black_and_white_fallback_paths(value in prop_oneof![
        (0u8..=3).boxed(),
        Just(255u8).boxed(),
    ]) {
        let frame = ImageFrame::new(1, 1, vec![value, value, value, 255]).unwrap();
        let config = AutoToneConfig::default();
        let result = suggest_auto_tone(&frame, config).unwrap();
        let median = match value {
            0 => 0.0,
            255 => 1.0,
            v => (f64::from(v) + 0.5) / 256.0,
        };
        let expected = {
            let raw = if median <= config.epsilon {
                config.exposure_bounds.1
            } else if median >= 1.0 - config.epsilon {
                config.exposure_bounds.0
            } else {
                (config.target_luminance / median).log2()
            };
            raw.clamp(config.exposure_bounds.0, config.exposure_bounds.1)
        };
        prop_assert_eq!(result.exposure, expected, "value {}", value);
        prop_assert_eq!(result.contrast, 0.0, "uniform frame, value {}", value);
        // AUTO-TONE-2: the zero-span gate yields identity for the four
        // end/balance sliders as well (single-pixel frames have no span).
        prop_assert_eq!(result.whites, 0.0, "uniform frame, value {}", value);
        prop_assert_eq!(result.blacks, 0.0, "uniform frame, value {}", value);
        prop_assert_eq!(result.highlights, 0.0, "uniform frame, value {}", value);
        prop_assert_eq!(result.shadows, 0.0, "uniform frame, value {}", value);
    }

    /// Invariant (F-043, Gleichverteilung): a frame with a single pixel value
    /// has span `p99 - p01 == 0 <= epsilon`, so contrast is the identity
    /// `0.0` — R2-PERF-01 preserves this bit-exactly: every sample of a
    /// constant frame lands in one bin, so both quantile brackets estimate the
    /// same class mark and the span stays exactly `0.0`. For `0 < k < 255` the
    /// exposure follows the regular `log2` path; for the universal black/white
    /// frames the documented fallback bounds apply. SOURCE BOUNDARY
    /// (documented, not fixed): the `span <= epsilon` branch returns `0.0`
    /// *unclamped* — for pathological `contrast_bounds` excluding `0.0` the
    /// "contrast in bounds" invariant would not hold. `0.0` is the documented
    /// identity value (pipeline.md: "Leere Bilder liefern 0"), so the
    /// properties use bounds containing `0.0` (default config).
    ///
    /// R2-PERF-01: the expected exposure uses the documented 256-bin
    /// class-mark median (`(k + 0.5)/256` for interior gray levels, exact
    /// `0.0`/`1.0` at the outer bins).
    #[test]
    fn auto_tone_uniform_frame_has_zero_contrast(value in 0u8..=255) {
        let mut pixels = Vec::with_capacity(16);
        for _ in 0..4 {
            pixels.extend_from_slice(&[value, value, value, 255]);
        }
        let frame = ImageFrame::new(2, 2, pixels).unwrap();
        let config = AutoToneConfig::default();
        let result = suggest_auto_tone(&frame, config).unwrap();
        prop_assert_eq!(result.contrast, 0.0, "value {}", value);
        // AUTO-TONE-2: same zero-span identity for the end/balance sliders.
        prop_assert_eq!(result.whites, 0.0, "value {}", value);
        prop_assert_eq!(result.blacks, 0.0, "value {}", value);
        prop_assert_eq!(result.highlights, 0.0, "value {}", value);
        prop_assert_eq!(result.shadows, 0.0, "value {}", value);
        let median = match value {
            0 => 0.0,
            255 => 1.0,
            v => (f64::from(v) + 0.5) / 256.0,
        };
        let expected = {
            let raw = if median <= config.epsilon {
                config.exposure_bounds.1
            } else if median >= 1.0 - config.epsilon {
                config.exposure_bounds.0
            } else {
                (config.target_luminance / median).log2()
            };
            raw.clamp(config.exposure_bounds.0, config.exposure_bounds.1)
        };
        prop_assert!(
            (result.exposure - expected).abs() < 1e-9,
            "value {value}: exposure {} != expected {expected}",
            result.exposure,
        );
    }

    /// Invariant (F-043, Ziel-Luminanz): `AutoToneConfig::validate()` rejects
    /// every target outside `0..=1` (including non-finite values) with
    /// `CoreError::InvalidAutoToneConfig` — the clamp to `epsilon..=1.0`
    /// happens inside `suggest_auto_tone` only *after* validation, so out-of-
    /// range targets are errors, not silently clamped values.
    #[test]
    fn auto_tone_rejects_out_of_range_targets(target in invalid_target_strategy()) {
        let config = AutoToneConfig {
            target_luminance: target,
            ..AutoToneConfig::default()
        };
        let frame = ImageFrame::new(1, 1, vec![128, 128, 128, 255]).unwrap();
        let error = suggest_auto_tone(&frame, config).unwrap_err();
        prop_assert!(
            matches!(error, CoreError::InvalidAutoToneConfig(_)),
            "target {target} must be rejected"
        );
    }

    /// Invariant (F-043): `match_total_exposure` rejects every target outside
    /// `0..=1` with `CoreError::InvalidAdjustment` (no silent clamping).
    #[test]
    fn matching_rejects_out_of_range_targets(target in invalid_target_strategy()) {
        let frame = ImageFrame::new(1, 1, vec![128, 128, 128, 255]).unwrap();
        let error = match_total_exposure(&frame, target).unwrap_err();
        prop_assert!(
            matches!(error, CoreError::InvalidAdjustment { .. }),
            "target {target} must be rejected"
        );
    }

    /// Invariant (F-041, F-043): a fully masked frame (weight sum `0 <= 1e-6`)
    /// returns the documented identity delta `0.0` — for every valid target,
    /// no NaN, no panic.
    #[test]
    fn masked_fully_masked_plane_returns_zero_delta(frame in frame_strategy(), target in 0.0f64..=1.0) {
        let plane = MaskPlane::new(
            frame.width,
            frame.height,
            vec![0; frame.width as usize * frame.height as usize],
        )
        .unwrap();
        prop_assert_eq!(
            match_total_exposure_masked(&frame, target, &[plane]).unwrap(),
            0.0,
        );
    }

    /// Invariant (F-041, F-043): a plane whose width, height or value count
    /// disagrees with the frame is rejected with `CoreError::InvalidMaskPlane`
    /// before any pixel is touched (no silent fallback).
    #[test]
    fn masked_dimension_mismatch_is_rejected(
        (frame, plane) in frame_strategy().prop_flat_map(|frame| {
            let planes = mismatched_plane_strategy(&frame);
            (Just(frame), planes)
        }),
    ) {
        let error =
            match_total_exposure_masked(&frame, 0.5, std::slice::from_ref(&plane)).unwrap_err();
        let is_invalid_plane = matches!(error, CoreError::InvalidMaskPlane { .. });
        prop_assert!(is_invalid_plane, "mismatched plane must be rejected");
    }

    /// Control for the mismatch property: a plane matching the frame
    /// dimensions (with arbitrary values) is always accepted and yields a
    /// finite delta.
    #[test]
    fn masked_valid_plane_is_accepted(frame in frame_strategy(), target in 0.0f64..=1.0) {
        let count = frame.width as usize * frame.height as usize;
        let values: Vec<u16> = (0..count)
            .map(|index| (index as u16).wrapping_mul(7919).wrapping_add(1))
            .collect();
        let plane = MaskPlane::new(frame.width, frame.height, values).unwrap();
        let result = match_total_exposure_masked(&frame, target, &[plane]).unwrap();
        prop_assert!(result.is_finite(), "delta {} must be finite", result);
    }

    /// Invariant (F-043, Determinismus): identical frame + config produce the
    /// identical fingerprint string.
    #[test]
    fn fingerprint_is_deterministic(frame in frame_strategy()) {
        let config = AutoToneConfig::default();
        prop_assert_eq!(
            tone_fingerprint(&frame, config),
            tone_fingerprint(&frame, config),
        );
    }

    /// Weak collision-freedom (F-043): two frames that differ in at least one
    /// byte produce different fingerprints. FNV-1a is not a cryptographic
    /// hash, so this is a statistical property, not a proof — the probe count
    /// stays at the default and the mutation is a single byte.
    #[test]
    fn fingerprint_differs_for_different_frames(frame in frame_strategy()) {
        let config = AutoToneConfig::default();
        let mut other = frame.clone();
        other.pixels[0] ^= 0x01;
        prop_assert_ne!(
            tone_fingerprint(&frame, config),
            tone_fingerprint(&other, config),
        );
    }

    /// Invariant (F-043, Analyse): `analyze_tone` always reports exactly
    /// `width * height` samples with finite values in `0..=1`, monotone
    /// quantiles (`p01 <= median <= p99` — guaranteed by construction since
    /// R2-PERF-01: class marks are ordered by bin index and interpolation is
    /// convex) and `p01 <= p99`.
    #[test]
    fn analyze_tone_invariants(frame in any_frame_strategy()) {
        let analysis = analyze_tone(&frame);
        prop_assert_eq!(
            analysis.sample_count,
            frame.width as usize * frame.height as usize,
        );
        for value in [analysis.mean, analysis.median, analysis.p01, analysis.p99] {
            prop_assert!(value.is_finite(), "analysis value must be finite");
            prop_assert!((0.0..=1.0).contains(&value), "analysis value {} outside 0..=1", value);
        }
        prop_assert!(analysis.p01 <= analysis.median, "p01 {} > median {}", analysis.p01, analysis.median);
        prop_assert!(analysis.median <= analysis.p99, "median {} > p99 {}", analysis.median, analysis.p99);
        prop_assert!(analysis.p01 <= analysis.p99, "p01 {} > p99 {}", analysis.p01, analysis.p99);
    }

    /// Invariant (R2-PERF-01): the 256-bin class-mark statistics track the
    /// exact sorted-sample statistics within the documented universal bound of
    /// one bin width `1/256`. Every class mark lies inside the bin of the
    /// order statistic it estimates; linear interpolation is convex in both
    /// brackets, so the deviation can never exceed one bin width — for ANY
    /// frame, dense or sparse. The mean stays the exact pixel-order sum and
    /// must agree with the exact mean to within f64 rounding.
    #[test]
    fn analyze_tone_tracks_sorted_sample_statistics_within_one_bin(
        frame in any_frame_strategy(),
    ) {
        const BIN_BOUND: f64 = 1.0 / 256.0 + 1e-9;
        let analysis = analyze_tone(&frame);
        let mut values: Vec<f64> = frame
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| {
                (0.2126 * f64::from(pixel[0])
                    + 0.7152 * f64::from(pixel[1])
                    + 0.0722 * f64::from(pixel[2]))
                    / 255.0
            })
            .collect();
        if values.is_empty() {
            prop_assert_eq!(analysis.sample_count, 0);
            return Ok(());
        }
        values.sort_by(f64::total_cmp);
        let percentile = |q: f64| {
            let position = q * (values.len() - 1) as f64;
            let low = position.floor() as usize;
            let high = position.ceil() as usize;
            values[low]
                + (values[high] - values[low]) * (position - low as f64)
        };
        prop_assert!(
            (analysis.mean - values.iter().sum::<f64>() / values.len() as f64).abs() <= 1e-9,
            "mean: pixel-order sum {} vs sorted sum {}",
            analysis.mean,
            values.iter().sum::<f64>() / values.len() as f64
        );
        for (name, actual, expected) in [
            ("p01", analysis.p01, percentile(0.01)),
            ("median", analysis.median, percentile(0.5)),
            ("p99", analysis.p99, percentile(0.99)),
        ] {
            prop_assert!(
                (actual - expected).abs() <= BIN_BOUND,
                "{name}: bin estimate {actual} vs exact {expected}, exceeds {BIN_BOUND}"
            );
        }
    }

    /// Invariant (F-043, Alpha): alpha is deliberately ignored by the tone
    /// domain — zeroing every alpha byte must not change the analysis.
    #[test]
    fn analyze_tone_ignores_alpha(frame in frame_strategy()) {
        let mut no_alpha = frame.clone();
        for pixel in no_alpha.pixels.as_chunks_mut::<4>().0 {
            pixel[3] = 0;
        }
        prop_assert_eq!(analyze_tone(&frame), analyze_tone(&no_alpha));
    }
}

/// Deterministic path of the target clamp (F-043): `validate()` accepts
/// `0.0` (it is inside `0..=1`); inside `suggest_auto_tone` the target is
/// clamped to `epsilon..=1.0`, so every target in `[0, epsilon)` is lifted to
/// `epsilon` and yields a bit-identical result. The upper end of the clamp
/// range is *not* pulled down: target `1.0` stays `1.0`.
///
/// R2-PERF-01: gray level 128 lands in bin 128, whose documented class-mark
/// median is `(128 + 0.5)/256`; the expectations below use that analysis
/// median.
#[test]
fn auto_tone_target_zero_is_clamped_to_epsilon() {
    let config = AutoToneConfig {
        target_luminance: 0.0,
        epsilon: 0.01,
        exposure_bounds: (-30.0, 30.0),
        contrast_bounds: (-1.0, 1.0),
    };
    config.validate().unwrap();
    let frame = ImageFrame::new(1, 1, vec![128, 128, 128, 255]).unwrap();
    let lifted = suggest_auto_tone(&frame, config).unwrap();
    let expected = (0.01f64 / (128.5f64 / 256.0)).log2();
    assert!(
        (lifted.exposure - expected).abs() < 1e-9,
        "exposure {} != lifted formula {expected}",
        lifted.exposure
    );
    // Any target below epsilon lifts to the same value: bit-identical result.
    let also_lifted = suggest_auto_tone(
        &frame,
        AutoToneConfig {
            target_luminance: 0.005,
            ..config
        },
    )
    .unwrap();
    assert_eq!(lifted.exposure, also_lifted.exposure);
    // The upper end is not clamped down: target 1.0 keeps the regular path.
    let upper = suggest_auto_tone(
        &frame,
        AutoToneConfig {
            target_luminance: 1.0,
            ..config
        },
    )
    .unwrap();
    let expected_upper = (1.0f64 / (128.5f64 / 256.0)).log2();
    assert!(
        (upper.exposure - expected_upper).abs() < 1e-9,
        "exposure {} != upper formula {expected_upper}",
        upper.exposure
    );
}

/// Deterministic deltas of `matching_delta` (F-043): target `<= 1e-6`
/// pins `-10.0`, current `<= 1e-6` pins `10.0`, otherwise
/// `log2(target/current)`. Black frame with target 0.5 is `10.0` (pinned, so
/// exact); the `log2`-based expectations are asserted within `1e-9` because
/// `f64::log2` is not exact even for powers of two (`log2(0.5)` evaluates to
/// `-0.9999999999999997` in this build) — the mathematical value is exact
/// (`log2(0.5/1.0) = -1.0`), the rounded f64 value is not.
#[test]
fn matching_black_and_white_are_exact() {
    let black = ImageFrame::new(1, 1, vec![0, 0, 0, 255]).unwrap();
    assert_eq!(match_total_exposure(&black, 0.5).unwrap(), 10.0);
    assert_eq!(match_total_exposure(&black, 0.0).unwrap(), -10.0);
    assert_eq!(match_total_exposure(&black, 1e-6).unwrap(), -10.0);
    let white = ImageFrame::new(1, 1, vec![255, 255, 255, 255]).unwrap();
    assert!((match_total_exposure(&white, 0.5).unwrap() - (-1.0)).abs() < 1e-9); // log2(0.5/1.0)
    assert!((match_total_exposure(&white, 0.25).unwrap() - (-2.0)).abs() < 1e-9); // log2(0.25/1.0)
    assert!((match_total_exposure(&white, 1.0).unwrap() - 0.0).abs() < 1e-9); // log2(1.0/1.0)
}
