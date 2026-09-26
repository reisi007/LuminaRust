//! The mask-independent `SourceActions` *stage* tests of `render_frame`.
//!
//! Extracted verbatim from `render.rs` (file-size ratchet, the same
//! Gegenextraktion the P1.2b/P1.2c lines of the baseline describe) so the
//! wiring growth of the MASK-LOCAL-P1.2d test split can be paid for by a real
//! move instead of a raised baseline. Pure code motion: the four tests below
//! are byte-for-byte the ones that were in `render.rs`, and their sibling
//! `source_action_contract_tests.rs` keeps the F-085 schema/policy contracts.

use super::*;
use std::collections::BTreeMap;

// ---- SourceActions stage ----
#[test]
fn source_action_composites_above_threshold_and_keeps_alpha() {
    let frame = ImageFrame::new(2, 1, vec![100, 100, 100, 255, 200, 200, 200, 40]).unwrap();
    let action = SourceActionArtifact {
        region: MaskPlane::new(2, 1, vec![32768, 32767]).unwrap(),
        replacement: ImageFrame::new(2, 1, vec![10, 20, 30, 128, 1, 2, 3, 9]).unwrap(),
    };
    let recipe = EditRecipe::default();
    let output = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[action],
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap();
    // Pixel 0: region 32768 >= threshold -> replacement incl. its alpha.
    assert_eq!(&output.frame.pixels[0..4], &[10, 20, 30, 128]);
    // Pixel 1: region 32767 < threshold -> source incl. its alpha.
    assert_eq!(&output.frame.pixels[4..8], &[200, 200, 200, 40]);
}
#[test]
fn empty_source_actions_are_byte_identical_to_apply_recipe() {
    let frame = ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 7]).unwrap();
    let recipe = EditRecipe {
        adjustments: BTreeMap::from([("exposure".into(), 0.5), ("contrast".into(), -0.2)]),
        ..Default::default()
    };
    let mut expected = frame.clone();
    expected
        .apply_recipe_with_white_balance(&recipe, Some([1.0, 1.0, 1.0, 1.0]))
        .unwrap();
    let output = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: Some([1.0, 1.0, 1.0, 1.0]),
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap();
    assert_eq!(output.frame, expected);
    assert!(output.mask_layers.is_empty());
    assert!(output.mask_warnings.is_empty());
}
#[test]
fn source_actions_run_before_adjustments() {
    // Pixel value 100. Exposure +1 doubles whatever the source-actions
    // stage left in the frame. With the action applied BEFORE adjustments
    // the replaced value 10 becomes 20 (not 10 = action after adjustments,
    // not 200 = no action at all).
    let frame = ImageFrame::new(1, 1, vec![100, 100, 100, 255]).unwrap();
    let recipe = EditRecipe {
        adjustments: BTreeMap::from([("exposure".into(), 1.0)]),
        ..Default::default()
    };
    let action = SourceActionArtifact {
        region: MaskPlane::new(1, 1, vec![65535]).unwrap(),
        replacement: ImageFrame::new(1, 1, vec![10, 10, 10, 255]).unwrap(),
    };
    let output = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[action],
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap();
    assert_eq!(output.frame.pixels, vec![20, 20, 20, 255]);
    // Control: no action -> 100 * 2 = 200.
    let control = render_frame(&frame, &default_context(&recipe, None)).unwrap();
    assert_eq!(control.frame.pixels, vec![200, 200, 200, 255]);
}
#[test]
fn source_action_rejects_mismatched_artifacts() {
    let frame = base_frame();
    let recipe = EditRecipe::default();
    let mismatched_dims = SourceActionArtifact {
        region: MaskPlane::new(2, 2, vec![0; 4]).unwrap(),
        replacement: ImageFrame::new(1, 4, vec![0; 16]).unwrap(),
    };
    let error = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[mismatched_dims],
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap_err();
    assert!(matches!(error, CoreError::InvalidSourceAction(_)));
    let wrong_frame_dims = SourceActionArtifact {
        region: MaskPlane::new(1, 1, vec![0]).unwrap(),
        replacement: ImageFrame::new(1, 1, vec![0; 4]).unwrap(),
    };
    assert!(matches!(
        render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[wrong_frame_dims],
                lensfun: None,
                depth: None,
                masks: None,
            },
        ),
        Err(CoreError::InvalidSourceAction(_))
    ));
}
