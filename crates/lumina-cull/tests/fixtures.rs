//! Fixture accuracy: precision/recall of the Stage-1 heuristic on synthetic,
//! labelled frames generated in-process (no original files, no network).
//!
//! Positive class = `keep`. `review` and `reject-kandidat` both count as
//! "not keep" here, because the proposal classifier is deliberately
//! conservative: only a clearly good image is promoted to `keep`.

mod common;

use common::{block_texture, box_blur, checkerboard, gray_frame, noise_frame};

use lumina_core::ImageFrame;
use lumina_cull::{
    analyze_heuristic, CullConfig, CullSourceInput, REASON_EXPOSURE_CLIPPED,
    REASON_EXPOSURE_OVEREXPOSED, REASON_EXPOSURE_UNDEREXPOSED, REASON_MOTION_BLUR_SUSPECT,
    REASON_NOISE_HIGH_ISO, REASON_SHARPNESS_LOW,
};
use lumina_sidecar::CullProposal;

struct Fixture {
    name: &'static str,
    frame: ImageFrame,
    keep: bool,
    expect_reason: Option<&'static str>,
}

const SIZE: u32 = 160;

fn fixtures() -> Vec<Fixture> {
    let checker_8 = checkerboard(SIZE, SIZE, 8, 90, 165);
    let checker_4 = checkerboard(SIZE, SIZE, 4, 80, 170);
    let checker_12 = checkerboard(SIZE, SIZE, 12, 70, 150);
    let texture_a = block_texture(SIZE, SIZE, 8, 1);
    let texture_b = block_texture(SIZE, SIZE, 6, 2);
    let texture_c = block_texture(SIZE, SIZE, 10, 3);

    let overexposed = gray_frame(SIZE, SIZE, |x, y| {
        if y < 112 {
            255
        } else if (x / 8 + y / 8).is_multiple_of(2) {
            40
        } else {
            120
        }
    });
    let underexposed = gray_frame(SIZE, SIZE, |x, y| {
        if y >= 48 {
            0
        } else if (x / 8 + y / 8).is_multiple_of(2) {
            40
        } else {
            120
        }
    });

    vec![
        Fixture {
            name: "checker_8",
            frame: checker_8.clone(),
            keep: true,
            expect_reason: None,
        },
        Fixture {
            name: "checker_4",
            frame: checker_4.clone(),
            keep: true,
            expect_reason: None,
        },
        Fixture {
            name: "checker_12",
            frame: checker_12,
            keep: true,
            expect_reason: None,
        },
        Fixture {
            name: "texture_8",
            frame: texture_a.clone(),
            keep: true,
            expect_reason: None,
        },
        Fixture {
            name: "texture_6",
            frame: texture_b,
            keep: true,
            expect_reason: None,
        },
        Fixture {
            name: "texture_10",
            frame: texture_c,
            keep: true,
            expect_reason: None,
        },
        Fixture {
            name: "defocus_3",
            frame: box_blur(&checker_8, 3, 3),
            keep: false,
            // Isotropic defocus: low structural sharpness without a dominant
            // gradient direction, so the plain `sharpness_low` reason fires.
            expect_reason: Some(REASON_SHARPNESS_LOW),
        },
        Fixture {
            name: "defocus_6",
            frame: box_blur(&checker_8, 6, 6),
            keep: false,
            expect_reason: Some(REASON_SHARPNESS_LOW),
        },
        Fixture {
            name: "defocus_texture",
            frame: box_blur(&texture_a, 4, 4),
            keep: false,
            expect_reason: None,
        },
        Fixture {
            name: "motion_blur",
            frame: box_blur(&checker_8, 16, 0),
            keep: false,
            expect_reason: Some(REASON_MOTION_BLUR_SUSPECT),
        },
        Fixture {
            name: "overexposed_clipped",
            frame: overexposed,
            keep: false,
            expect_reason: Some(REASON_EXPOSURE_CLIPPED),
        },
        Fixture {
            name: "underexposed",
            frame: underexposed,
            keep: false,
            expect_reason: Some(REASON_EXPOSURE_UNDEREXPOSED),
        },
        Fixture {
            name: "noise_60",
            frame: noise_frame(SIZE, SIZE, 128, 60, 10),
            keep: false,
            expect_reason: Some(REASON_NOISE_HIGH_ISO),
        },
        Fixture {
            name: "noise_80",
            frame: noise_frame(SIZE, SIZE, 100, 80, 11),
            keep: false,
            expect_reason: Some(REASON_NOISE_HIGH_ISO),
        },
    ]
}

#[test]
fn heuristic_precision_and_recall_on_synthetic_fixtures() {
    let config = CullConfig::default();
    let mut true_positive = 0u32;
    let mut false_positive = 0u32;
    let mut false_negative = 0u32;
    let mut true_negative = 0u32;
    let (mut saw_keep, mut saw_review, mut saw_reject) = (false, false, false);

    for fixture in fixtures() {
        let input = CullSourceInput {
            frame: &fixture.frame,
            iso: None,
        };
        let analysis = analyze_heuristic(&input, &config)
            .unwrap_or_else(|error| panic!("{} must analyze: {error}", fixture.name));
        let predicted_keep = analysis.proposal() == CullProposal::Keep;
        println!(
            "fixture {:<20} label_keep={} proposal={:?} score={:.3} sharp={:.3} aniso={:.3} \
             noise_sigma={:.4} clip={:.4} mean={:.3} reasons={:?}",
            fixture.name,
            fixture.keep,
            analysis.proposal(),
            analysis.score(),
            analysis.diagnostics.sharpness_score,
            analysis.diagnostics.anisotropy,
            analysis.diagnostics.noise_sigma,
            analysis.diagnostics.clip_fraction,
            analysis.diagnostics.mean_luminance,
            analysis.reasons()
        );

        match (fixture.keep, predicted_keep) {
            (true, true) => true_positive += 1,
            (false, true) => false_positive += 1,
            (true, false) => false_negative += 1,
            (false, false) => true_negative += 1,
        }
        match analysis.proposal() {
            CullProposal::Keep => saw_keep = true,
            CullProposal::Review => saw_review = true,
            CullProposal::RejectCandidate => saw_reject = true,
        }

        if let Some(expected) = fixture.expect_reason {
            assert!(
                analysis.reasons().iter().any(|reason| reason == expected),
                "{} must report reason `{expected}`, got {:?}",
                fixture.name,
                analysis.reasons()
            );
        }
    }

    let precision = f64::from(true_positive) / f64::from(true_positive + false_positive).max(1.0);
    let recall = f64::from(true_positive) / f64::from(true_positive + false_negative).max(1.0);
    println!(
        "precision={precision:.3} recall={recall:.3} \
         (tp={true_positive} fp={false_positive} fn={false_negative} tn={true_negative})"
    );

    assert!(
        false_positive == 0,
        "no defect fixture may be promoted to keep"
    );
    assert!(
        false_negative == 0,
        "every labelled-keep fixture must be promoted to keep"
    );
    // Pinned to the current, fully covered fixture set: a regression that
    // misclassifies any labelled fixture must fail here (not merely drop below
    // a slack bound).
    assert_eq!(
        precision, 1.0,
        "precision must stay at 1.0 on the fixture set"
    );
    assert_eq!(recall, 1.0, "recall must stay at 1.0 on the fixture set");
    assert!(
        saw_keep && saw_review && saw_reject,
        "the fixture set must exercise all three proposal classes \
         (keep={saw_keep} review={saw_review} reject={saw_reject})"
    );
}

#[test]
fn sharp_frames_are_not_mistaken_for_noise() {
    let config = CullConfig::default();
    let frame = checkerboard(SIZE, SIZE, 8, 90, 165);
    let input = CullSourceInput {
        frame: &frame,
        iso: None,
    };
    let analysis = analyze_heuristic(&input, &config).expect("analysis");
    assert!(
        !analysis
            .reasons()
            .iter()
            .any(|reason| reason == REASON_NOISE_HIGH_ISO),
        "sharp structure must not trigger the noise reason: {:?}",
        analysis.reasons()
    );
    assert!(
        analysis.diagnostics.noise_sigma < 0.01,
        "flat-region noise estimate on a clean checkerboard should be ~0, got {}",
        analysis.diagnostics.noise_sigma
    );
}

#[test]
fn near_white_frame_emits_the_overexposed_reason() {
    // A near-white frame with no pixel in the brightest histogram bin, so the
    // mean luminance alone crosses `overexposed_mean_threshold` (0.88). This
    // covers the otherwise untested `exposure_overexposed` branch of
    // `collect_reasons`; the existing `overexposed_clipped` fixture stays below
    // the mean threshold (0.794) and only exercises the clipping branch.
    let config = CullConfig::default();
    let frame = checkerboard(SIZE, SIZE, 8, 235, 245);
    let input = CullSourceInput {
        frame: &frame,
        iso: None,
    };
    let analysis = analyze_heuristic(&input, &config).expect("analysis");

    assert!(
        analysis.diagnostics.mean_luminance > config.overexposed_mean_threshold,
        "fixture must sit above the overexposed mean threshold: mean={} threshold={}",
        analysis.diagnostics.mean_luminance,
        config.overexposed_mean_threshold
    );
    assert!(
        analysis
            .reasons()
            .iter()
            .any(|reason| reason == REASON_EXPOSURE_OVEREXPOSED),
        "near-white frame must emit `{REASON_EXPOSURE_OVEREXPOSED}`, got {:?}",
        analysis.reasons()
    );
    assert!(
        !analysis
            .reasons()
            .iter()
            .any(|reason| reason == REASON_EXPOSURE_CLIPPED
                || reason == REASON_EXPOSURE_UNDEREXPOSED),
        "an unclipped near-white frame must not emit the clipping/underexposed reasons, got {:?}",
        analysis.reasons()
    );
    assert_eq!(analysis.proposal(), CullProposal::RejectCandidate);
}
