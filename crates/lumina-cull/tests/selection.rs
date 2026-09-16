//! Duplicate/series grouping is confined to the explicit selection passed to
//! `analyze_selection`; the crate never scans folders or expands a selection.

mod common;

use common::{block_texture, checkerboard, gray_frame};

use lumina_cull::{
    analyze_heuristic, analyze_selection, CullConfig, CullSourceInput, SimilarityKind,
    REASON_DUPLICATE_GROUP, REASON_SERIES_GROUP,
};
use lumina_sidecar::CullProposal;

#[test]
fn exact_duplicates_in_the_selection_are_grouped_and_penalized() {
    let base = checkerboard(120, 120, 8, 90, 165);
    let copy = base.clone();
    let distinct = block_texture(120, 120, 8, 99);
    let frames = [&base, &copy, &distinct];
    let inputs: Vec<CullSourceInput<'_>> = frames
        .iter()
        .map(|frame| CullSourceInput { frame, iso: None })
        .collect();

    let result = analyze_selection(&inputs, &CullConfig::default()).expect("selection");
    assert_eq!(result.groups.len(), 1, "exactly one duplicate group");
    let group = &result.groups[0];
    assert_eq!(group.members, vec![0, 1]);
    assert_eq!(group.kind, SimilarityKind::Duplicate);

    for member in [0, 1] {
        assert!(
            result.images[member]
                .reasons()
                .iter()
                .any(|reason| reason == REASON_DUPLICATE_GROUP),
            "duplicate member must carry the reason"
        );
    }
    assert!(
        !result.images[2]
            .reasons()
            .iter()
            .any(|reason| reason == REASON_DUPLICATE_GROUP),
        "distinct image must not be grouped"
    );

    let best = group.best;
    let redundant = if best == 0 { 1 } else { 0 };
    assert!(!result.images[best].similar_redundant);
    assert!(result.images[redundant].similar_redundant);
    assert!(
        result.images[redundant].score() < result.images[best].score(),
        "the redundant member must be scored below the group representative"
    );
}

#[test]
fn a_single_image_selection_never_emits_a_group() {
    let frame = checkerboard(96, 96, 8, 90, 165);
    let inputs = [CullSourceInput {
        frame: &frame,
        iso: None,
    }];
    let result = analyze_selection(&inputs, &CullConfig::default()).expect("selection");
    assert!(result.groups.is_empty());
    assert!(!result.images[0]
        .reasons()
        .iter()
        .any(|reason| reason == REASON_DUPLICATE_GROUP));
}

#[test]
fn per_image_analysis_never_reports_duplicates() {
    // Two identical frames analyzed *separately* are not compared: grouping is
    // an explicit-selection operation, never a global folder scan.
    let frame = checkerboard(96, 96, 8, 90, 165);
    let config = CullConfig::default();
    let input = CullSourceInput {
        frame: &frame,
        iso: None,
    };
    let first = analyze_heuristic(&input, &config).expect("first");
    let second = analyze_heuristic(&input, &config).expect("second");
    assert_eq!(first, second);
    assert!(!first
        .reasons()
        .iter()
        .any(|reason| reason == REASON_DUPLICATE_GROUP));
}

#[test]
fn a_burst_exposure_drift_in_the_selection_emits_the_series_reason() {
    // Two frames of the same block texture with a small brightness drift: the
    // dHash is unchanged (identical ordering, hash distance 0) while the
    // luminance histogram has moved past the duplicate bound (0.10) but stays
    // within the series bound (0.25). That is a burst/exposure-drift series,
    // *not* a near-duplicate, so it exercises the `SimilarityKind::Series`
    // branch of `analyze_selection` (the `Duplicate` branch is covered by the
    // exact-copy tests above).
    let base = block_texture(160, 160, 8, 7);
    let drifted = gray_frame(160, 160, |x, y| {
        base.pixels[((y * 160 + x) * 4) as usize].saturating_sub(6)
    });
    let inputs = [
        CullSourceInput {
            frame: &base,
            iso: None,
        },
        CullSourceInput {
            frame: &drifted,
            iso: None,
        },
    ];

    let result = analyze_selection(&inputs, &CullConfig::default()).expect("selection");
    assert_eq!(result.groups.len(), 1, "exactly one series group");
    let group = &result.groups[0];
    assert_eq!(group.members, vec![0, 1]);
    assert_eq!(
        group.kind,
        SimilarityKind::Series,
        "an exposure drift inside the series bounds must not be a near-duplicate"
    );

    for (index, member) in group.members.iter().enumerate() {
        let image = &result.images[*member];
        assert_eq!(
            image.similar_group,
            Some(0),
            "series member {index} must record its group membership"
        );
        assert!(
            image
                .reasons()
                .iter()
                .any(|reason| reason == REASON_SERIES_GROUP),
            "series member {index} must emit `{REASON_SERIES_GROUP}`, got {:?}",
            image.reasons()
        );
        assert!(
            !image
                .reasons()
                .iter()
                .any(|reason| reason == REASON_DUPLICATE_GROUP),
            "a series member must not claim a near-duplicate: {:?}",
            image.reasons()
        );
    }

    let best = group.best;
    let redundant = if best == 0 { 1 } else { 0 };
    assert!(!result.images[best].similar_redundant);
    assert!(result.images[redundant].similar_redundant);
    assert!(
        result.images[redundant].score() < result.images[best].score(),
        "the redundant series member must be scored below the group representative"
    );
}

#[test]
fn duplicate_group_penalty_can_demote_a_redundant_candidate() {
    // A strong frame plus its exact copy: the copy loses the penalty and must
    // no longer outrank the representative.
    let base = checkerboard(120, 120, 8, 90, 165);
    let frames = [&base, &base];
    let inputs: Vec<CullSourceInput<'_>> = frames
        .iter()
        .map(|frame| CullSourceInput { frame, iso: None })
        .collect();
    let result = analyze_selection(&inputs, &CullConfig::default()).expect("selection");
    assert_eq!(result.groups.len(), 1);
    assert!(!result.images[0].similar_redundant);
    assert!(result.images[1].similar_redundant);
    assert!(result.images[1].score() < result.images[0].score());
    // The representative keeps the strong recommendation; the copy is demoted
    // but still a valid explicit proposal (no silent drop).
    assert!(matches!(
        result.images[0].proposal(),
        CullProposal::Keep | CullProposal::Review
    ));
}
