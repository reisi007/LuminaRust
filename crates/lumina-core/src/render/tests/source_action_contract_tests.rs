//! MASK-LOCAL-P1.2c ratchet extraction: the F-085 source-action goldens.
//!
//! This is a **pure move** of the `source actions x auto-WB / auto-tone /
//! exposure matching` block out of `render.rs`, so the P1.2c wiring growth fits
//! under the file-size ratchet without ever raising a baseline. The tests, their
//! names, their assertions and their helper are byte-for-byte the ones that were
//! inline in `render.rs`; only the location changed. The block keeps the
//! `super::*` import so it resolves exactly the same helpers it did before.

use super::*;

// ---- F-085: source actions × auto-WB / auto-tone / exposure matching ----

fn action(region: MaskPlane, replacement: ImageFrame) -> SourceActionArtifact {
    SourceActionArtifact {
        region,
        replacement,
    }
}

#[test]
fn source_action_runs_before_white_balance() {
    // Both pixels start at (100,100,100). The action replaces pixel 0 with
    // (10,20,30); the WB recipe (wb_temperature 3000 -> warmth -0.63636,
    // gains [1.22273, 1.0, 0.77727]) is applied afterwards. Order proof:
    //   - action first + WB:  replaced -> (12,20,23), source -> (122,100,78)
    //   - action only:        replaced -> (10,20,30) (WB not applied)
    //   - WB only:            every pixel -> (122,100,78)
    let frame = ImageFrame::new(2, 1, vec![100, 100, 100, 255, 100, 100, 100, 255]).unwrap();
    let recipe = EditRecipe {
        adjustments: BTreeMap::from([("wb_temperature".into(), 3000.0)]),
        ..Default::default()
    };
    let actions = [action(
        MaskPlane::new(2, 1, vec![65535, 0]).unwrap(),
        ImageFrame::new(2, 1, vec![10, 20, 30, 255, 0, 0, 0, 0]).unwrap(),
    )];
    let with_action = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &actions,
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap();
    let wb_only = render_frame(&frame, &default_context(&recipe, None)).unwrap();
    let action_only = render_frame(
        &frame,
        &RenderContext {
            recipe: &EditRecipe::default(),
            camera_white_balance: None,
            source_actions: &actions,
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap();

    // Exact values: the replaced pixel receives the WB gain on the
    // replacement's values, the non-replaced pixel on the source's values.
    assert_eq!(&with_action.frame.pixels[0..4], &[12, 20, 23, 255]);
    assert_eq!(&with_action.frame.pixels[4..8], &[122, 100, 78, 255]);
    // Differential proofs of the order:
    // WB changed the action output (10,20,30) -> (12,20,23) ...
    assert_ne!(
        &with_action.frame.pixels[0..4],
        &action_only.frame.pixels[0..4]
    );
    // ... and the action changed the WB input (100,100,100) -> (10,20,30).
    assert_ne!(&with_action.frame.pixels[0..4], &wb_only.frame.pixels[0..4]);
    // The non-replaced pixel is identical to WB-only (same source value).
    assert_eq!(&with_action.frame.pixels[4..8], &wb_only.frame.pixels[4..8]);
}

#[test]
fn source_action_changes_auto_tone_and_measurement_is_post_action() {
    // 1x4 frame; the action replaces the bright pixel 255 with 20. The
    // post-action median (80/255 ~= 0.314) differs clearly from the
    // pre-action median (114/255 ~= 0.447), so suggest_auto_tone must
    // produce different exposure/contrast.
    let frame = ImageFrame::new(
        4,
        1,
        vec![
            255, 255, 255, 255, 128, 128, 128, 255, 100, 100, 100, 255, 60, 60, 60, 255,
        ],
    )
    .unwrap();
    let actions = [action(
        MaskPlane::new(4, 1, vec![65535, 0, 0, 0]).unwrap(),
        ImageFrame::new(
            4,
            1,
            vec![20, 20, 20, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        )
        .unwrap(),
    )];
    let config = AutoToneConfig::default();

    let pre = suggest_auto_tone(&frame, config).unwrap();
    let post_frame = render_frame(
        &frame,
        &RenderContext {
            recipe: &EditRecipe::default(),
            camera_white_balance: None,
            source_actions: &actions,
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap()
    .frame;
    let post = suggest_auto_tone(&post_frame, config).unwrap();

    assert_eq!(post.analysis.sample_count, 4);
    assert!(
        (post.exposure - pre.exposure).abs() > 0.1,
        "auto exposure must differ between pre-action ({}) and post-action ({}) frames",
        pre.exposure,
        post.exposure
    );
    assert!(
        (post.contrast - pre.contrast).abs() > 0.1,
        "auto contrast must differ between pre-action ({}) and post-action ({}) frames",
        pre.contrast,
        post.contrast
    );

    // Caller semantics (CLI/GUI): auto-tone measures the post-action
    // frame, the result is written into the recipe, then the full recipe
    // is rendered with the same source actions. The median of the result
    // must hit the documented target (0.5) within tolerance ±0.02.
    let recipe = EditRecipe {
        adjustments: BTreeMap::from([
            ("exposure".into(), post.exposure),
            ("contrast".into(), post.contrast),
        ]),
        ..Default::default()
    };
    let rendered = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &actions,
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap();
    let median = analyze_tone(&rendered.frame).median;
    assert!(
        (median - 0.5).abs() <= 0.02,
        "median {median} not within 0.02 of the 0.5 auto-tone target"
    );
    // Applying the recipe to the post-action frame directly is equivalent
    // to rendering the original with action + recipe.
    let direct = render_frame(
        &post_frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap();
    assert_eq!(direct.frame, rendered.frame);
}

#[test]
fn source_action_changes_matching_delta_and_application_reaches_target() {
    // Same frame as above: the matching delta measured on the post-action
    // frame (mean 77/255 ~= 0.302) differs clearly from the delta on the
    // pre-action frame (mean 135.75/255 ~= 0.532).
    let frame = ImageFrame::new(
        4,
        1,
        vec![
            255, 255, 255, 255, 128, 128, 128, 255, 100, 100, 100, 255, 60, 60, 60, 255,
        ],
    )
    .unwrap();
    let actions = [action(
        MaskPlane::new(4, 1, vec![65535, 0, 0, 0]).unwrap(),
        ImageFrame::new(
            4,
            1,
            vec![20, 20, 20, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        )
        .unwrap(),
    )];
    let post_frame = render_frame(
        &frame,
        &RenderContext {
            recipe: &EditRecipe::default(),
            camera_white_balance: None,
            source_actions: &actions,
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap()
    .frame;
    let delta_post = match_total_exposure(&post_frame, 0.5).unwrap();
    let delta_pre = match_total_exposure(&frame, 0.5).unwrap();
    assert!(
            (delta_post - delta_pre).abs() > 0.2,
            "matching delta must differ between pre-action ({delta_pre}) and post-action ({delta_post}) frames"
        );

    // CLI semantics: matching measures the rendered (post-action) frame
    // and applies the exposure delta to that same frame. The result must
    // reach the target luminance within tolerance ±0.02.
    let mut matched = post_frame.clone();
    matched
        .apply_recipe_with_white_balance(
            &EditRecipe {
                adjustments: BTreeMap::from([("exposure".into(), delta_post)]),
                ..Default::default()
            },
            None,
        )
        .unwrap();
    let mean = analyze_tone(&matched).mean;
    assert!(
        (mean - 0.5).abs() <= 0.02,
        "mean {mean} not within 0.02 of the 0.5 matching target"
    );
}

#[test]
fn render_with_source_actions_does_not_mutate_inputs() {
    let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
    let actions = [action(
        MaskPlane::new(2, 2, vec![0, 32768, 65535, 32767]).unwrap(),
        ImageFrame::new(
            2,
            2,
            vec![
                10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160,
            ],
        )
        .unwrap(),
    )];
    let frame_before = frame.clone();
    let region_before = actions[0].region.clone();
    let replacement_before = actions[0].replacement.clone();
    let recipe = EditRecipe {
        adjustments: BTreeMap::from([("exposure".into(), 0.5)]),
        ..Default::default()
    };
    render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &actions,
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap();
    // Byte-identical comparisons: neither the input frame nor the artifact
    // (region/replacement) may be mutated by the render.
    assert_eq!(frame, frame_before);
    assert_eq!(actions[0].region, region_before);
    assert_eq!(actions[0].replacement, replacement_before);
}

#[test]
fn source_action_threshold_boundaries_are_exact() {
    let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
    let actions = [action(
        MaskPlane::new(2, 2, vec![32768, 32767, 0, 65535]).unwrap(),
        ImageFrame::new(
            2,
            2,
            vec![10, 20, 30, 128, 1, 2, 3, 9, 4, 5, 6, 7, 8, 9, 10, 11],
        )
        .unwrap(),
    )];
    let rendered = render_frame(
        &frame,
        &RenderContext {
            recipe: &EditRecipe::default(),
            camera_white_balance: None,
            source_actions: &actions,
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap();
    // 32768 (exact threshold) -> replaced incl. replacement alpha;
    // 32767 -> source; 0 -> source; u16::MAX -> replaced.
    assert_eq!(&rendered.frame.pixels[0..4], &[10, 20, 30, 128]);
    assert_eq!(&rendered.frame.pixels[4..8], &[100, 100, 100, 100]);
    assert_eq!(&rendered.frame.pixels[8..12], &[100, 100, 100, 100]);
    assert_eq!(&rendered.frame.pixels[12..16], &[8, 9, 10, 11]);
}

#[test]
fn zero_source_action_region_is_byte_identical_to_no_action() {
    let frame = ImageFrame::new(
        2,
        2,
        vec![
            7, 13, 29, 255, 200, 100, 50, 3, 1, 2, 3, 4, 250, 251, 252, 253,
        ],
    )
    .unwrap();
    let actions = [action(
        MaskPlane::new(2, 2, vec![0; 4]).unwrap(),
        ImageFrame::new(2, 2, vec![9; 16]).unwrap(),
    )];
    let with = render_frame(
        &frame,
        &RenderContext {
            recipe: &EditRecipe::default(),
            camera_white_balance: None,
            source_actions: &actions,
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap();
    let without = render_frame(&frame, &default_context(&EditRecipe::default(), None)).unwrap();
    assert_eq!(with.frame, without.frame);
    assert_eq!(with.frame, frame);
}

#[test]
fn full_source_action_region_replaces_every_pixel() {
    let frame = ImageFrame::new(1, 3, vec![100; 12]).unwrap();
    let replacement = ImageFrame::new(1, 3, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]).unwrap();
    let actions = [action(
        MaskPlane::new(1, 3, vec![65535; 3]).unwrap(),
        replacement.clone(),
    )];
    let rendered = render_frame(
        &frame,
        &RenderContext {
            recipe: &EditRecipe::default(),
            camera_white_balance: None,
            source_actions: &actions,
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap();
    assert_eq!(rendered.frame, replacement);
}

#[test]
fn render_frame_is_deterministic_with_source_actions_and_masks() {
    let frame = ImageFrame::new(
        2,
        2,
        vec![
            90, 91, 92, 255, 40, 41, 42, 128, 200, 201, 202, 7, 30, 31, 32, 255,
        ],
    )
    .unwrap();
    let actions = [action(
        MaskPlane::new(2, 2, vec![0, 32768, 65535, 32767]).unwrap(),
        ImageFrame::new(
            2,
            2,
            vec![
                10, 20, 30, 255, 11, 21, 31, 255, 12, 22, 32, 255, 13, 23, 33, 255,
            ],
        )
        .unwrap(),
    )];
    let recipe = EditRecipe {
        adjustments: BTreeMap::from([("wb_temperature".into(), 5200.0), ("exposure".into(), 0.25)]),
        ..Default::default()
    };
    // A valid mask layer so the full order actions -> adjustments -> masks
    // is exercised twice.
    let definitions = vec![mask_definition(
        "subject",
        MaskStatus::Valid,
        MaskOperation::Source,
        vec![],
    )];
    let copies = vec![copy_with(
        "vc",
        definitions,
        vec![layer("layer-1", reference("vc", "subject"))],
    )];
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(2, 2, vec![0, 1, 32768, 65535]).unwrap(),
    )]);
    let context = RenderContext {
        recipe: &recipe,
        camera_white_balance: Some([1.2, 1.0, 0.9, 1.0]),
        source_actions: &actions,
        lensfun: None,
        depth: None,
        masks: Some(mask_context(&copies, "vc", planes, MaskPolicy::Warn)),
    };
    let first = render_frame(&frame, &context).unwrap();
    let second = render_frame(&frame, &context).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.frame, second.frame);
    assert!(first.mask_warnings.is_empty());
}
