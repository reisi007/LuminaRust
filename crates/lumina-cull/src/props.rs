//! Real `proptest` properties for the Stage-1 analyzer (score range,
//! monotonicity, clipping, determinism).

use lumina_core::ImageFrame;
use lumina_sidecar::{CullProposal, DecodeFingerprint, GeometryFingerprint, SourceFingerprint};
use proptest::prelude::*;
use std::collections::BTreeMap;

use crate::analyze::{analyze_heuristic, CullSourceInput};
use crate::config::CullConfig;
use crate::score::{exposure_component, noise_score, propose, sharpness_score};
use crate::signals::ExposureMetrics;

fn frame_strategy() -> impl Strategy<Value = ImageFrame> {
    (1u32..=20, 1u32..=20).prop_flat_map(|(width, height)| {
        prop::collection::vec(any::<u8>(), (width * height * 4) as usize)
            .prop_map(move |pixels| ImageFrame::new(width, height, pixels).expect("exact buffer"))
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn analysis_score_stays_in_unit_range(frame in frame_strategy()) {
        let config = CullConfig::default();
        let input = CullSourceInput { frame: &frame, iso: None };
        let analysis = analyze_heuristic(&input, &config).expect("analysis");
        prop_assert!(analysis.score().is_finite());
        prop_assert!((0.0..=1.0).contains(&analysis.score()));
        let expected = propose(f64::from(analysis.score()), &config);
        prop_assert_eq!(analysis.proposal(), expected);
        for reason in analysis.reasons() {
            prop_assert!(reason.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'));
        }
    }

    #[test]
    fn sharpness_score_is_monotone(first in 0f64..1000.0, delta in 0f64..1000.0) {
        let scale = 0.02;
        prop_assert!(sharpness_score(first + delta, scale) >= sharpness_score(first, scale));
    }

    #[test]
    fn noise_score_is_monotone(first in 0f64..10.0, delta in 0f64..10.0) {
        let scale = 0.05;
        prop_assert!(noise_score(first + delta, scale) >= noise_score(first, scale));
    }

    #[test]
    fn lower_score_never_raises_recommendation(first in 0f64..1.0, delta in 0f64..1.0) {
        let config = CullConfig::default();
        let low = propose(first, &config);
        let high = propose(first + delta, &config);
        let rank = |value: CullProposal| match value {
            CullProposal::RejectCandidate => 0,
            CullProposal::Review => 1,
            CullProposal::Keep => 2,
        };
        prop_assert!(rank(high) >= rank(low));
    }

    #[test]
    fn exposure_component_is_monotone_in_clipping(
        low in 0f64..0.5,
        delta in 0f64..0.5,
    ) {
        let config = CullConfig::default();
        let make = |clip: f64| ExposureMetrics {
            mean: 0.5,
            median: 0.5,
            p01: 0.2,
            p99: 0.8,
            shadow_clip_fraction: clip / 2.0,
            highlight_clip_fraction: clip / 2.0,
            clip_fraction: clip,
            sample_count: 100,
        };
        prop_assert!(
            exposure_component(&make(low + delta), &config)
                <= exposure_component(&make(low), &config)
        );
    }

    #[test]
    fn analysis_is_deterministic_and_byte_identical(frame in frame_strategy()) {
        let config = CullConfig::default();
        let input = CullSourceInput { frame: &frame, iso: Some(800) };
        let first = analyze_heuristic(&input, &config).expect("first");
        let second = analyze_heuristic(&input, &config).expect("second");
        prop_assert_eq!(&first.core, &second.core);
        prop_assert_eq!(first.diagnostics, second.diagnostics);

        let source = SourceFingerprint {
            content_hash: "blake3:prop".into(),
            byte_length: 1,
            extras: Default::default(),
        };
        let decode = DecodeFingerprint {
            decoder: "test".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: Default::default(),
        };
        let geometry = GeometryFingerprint {
            width: frame.width,
            height: frame.height,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: Default::default(),
        };
        let a = first
            .core
            .to_section(source.clone(), decode.clone(), geometry.clone(), "2026-09-16T00:00:00Z")
            .expect("section a");
        let b = second
            .core
            .to_section(source, decode, geometry, "2026-09-16T00:00:00Z")
            .expect("section b");
        prop_assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
    }
}
