use super::*;

#[test]
fn white_balance_and_edge_tones_have_identity_and_effect() {
    let original = ImageFrame::new(1, 1, vec![80, 100, 120, 9]).unwrap();
    let mut identity = original.clone();
    identity.apply_recipe(&recipe(&[])).unwrap();
    assert_eq!(identity, original);
    let mut warm = original.clone();
    warm.apply_recipe(&recipe(&[("wb_temperature", 3000.0)]))
        .unwrap();
    assert!(warm.pixels[0] > original.pixels[0]);
    assert!(warm.pixels[2] < original.pixels[2]);
    let mut edges = ImageFrame::new(2, 1, vec![20, 20, 20, 255, 230, 230, 230, 255]).unwrap();
    edges
        .apply_recipe(&recipe(&[("whites", 1.0), ("blacks", 1.0)]))
        .unwrap();
    assert!(edges.pixels[0] < 20 && edges.pixels[4] > 230);
}

#[test]
fn apply_recipe_with_white_balance_none_matches_apply_recipe() {
    let pixel_data = vec![80, 100, 120, 9, 200, 60, 30, 200, 10, 250, 128, 77];
    for r in [
        recipe(&[("exposure", 0.5), ("contrast", 0.25)]),
        recipe(&[
            ("wb_temperature", 3000.0),
            ("wb_tint", -0.5),
            ("exposure", 0.5),
        ]),
    ] {
        let mut with_context = ImageFrame::new(3, 1, pixel_data.clone()).unwrap();
        let mut without = ImageFrame::new(3, 1, pixel_data.clone()).unwrap();
        with_context
            .apply_recipe_with_white_balance(&r, None)
            .unwrap();
        without.apply_recipe(&r).unwrap();
        assert_eq!(with_context, without);
    }
}

#[test]
fn as_shot_context_is_identity_without_wb_keys() {
    let pixel_data = vec![80, 100, 120, 9, 200, 60, 30, 200, 10, 250, 128, 77];
    let r = recipe(&[("exposure", 0.5), ("contrast", 0.25)]);
    for gains in [[1.0, 1.0, 1.0, 1.0], [2.0, 1.0, 0.5, 1.0]] {
        let mut with_context = ImageFrame::new(3, 1, pixel_data.clone()).unwrap();
        with_context
            .apply_recipe_with_white_balance(&r, Some(gains))
            .unwrap();
        let mut without = ImageFrame::new(3, 1, pixel_data.clone()).unwrap();
        without.apply_recipe(&r).unwrap();
        assert_eq!(
            with_context, without,
            "As-Shot gains must not be re-applied (decoder already applied them)"
        );
    }
}

#[test]
fn manual_wb_anchors_unchanged_with_as_shot_context() {
    let pixel_data = vec![80, 100, 120, 9, 200, 60, 30, 200, 10, 250, 128, 77];
    let r = recipe(&[
        ("wb_temperature", 3000.0),
        ("wb_tint", -0.5),
        ("exposure", 0.5),
    ]);
    let mut with_context = ImageFrame::new(3, 1, pixel_data.clone()).unwrap();
    with_context
        .apply_recipe_with_white_balance(&r, Some([2.0, 1.0, 0.5, 1.0]))
        .unwrap();
    let mut without = ImageFrame::new(3, 1, pixel_data.clone()).unwrap();
    without.apply_recipe(&r).unwrap();
    assert_eq!(
        with_context, without,
        "manual wb keys keep the deterministic sRGB approximation"
    );
}

#[test]
fn invalid_camera_white_balance_rejected_without_mutation() {
    let original = ImageFrame::new(2, 1, vec![80, 100, 120, 9, 200, 60, 30, 200]).unwrap();
    let r = recipe(&[("wb_temperature", 3000.0)]);
    for gains in [
        [0.0, 1.0, 1.0, 1.0],
        [-1.0, 1.0, 1.0, 1.0],
        [f32::NAN, 1.0, 1.0, 1.0],
        [f32::INFINITY, 1.0, 1.0, 1.0],
    ] {
        let mut frame = original.clone();
        let result = frame.apply_recipe_with_white_balance(&r, Some(gains));
        assert!(
            matches!(
                result,
                Err(CoreError::InvalidAdjustment { name, .. }) if name == "camera_white_balance"
            ),
            "expected InvalidAdjustment for gains {gains:?}"
        );
        assert_eq!(
            frame, original,
            "frame must stay byte-identical when gains {gains:?} are invalid"
        );
    }
}

#[test]
fn wb_application_preserves_alpha_with_context() {
    let mut frame = ImageFrame::new(1, 1, vec![80, 100, 120, 77]).unwrap();
    frame
        .apply_recipe_with_white_balance(
            &recipe(&[("wb_temperature", 3000.0)]),
            Some([2.0, 1.0, 0.5, 1.0]),
        )
        .unwrap();
    assert_eq!(frame.pixels[3], 77);
    assert_ne!(frame.pixels[0], 80);
}

#[test]
fn export_options_reject_mvp_unsupported_values() {
    let frame = ImageFrame::new(1, 1, vec![1, 2, 3, 255]).unwrap();
    assert!(frame
        .encode_with_options(ExportOptions {
            quality: 0,
            ..Default::default()
        })
        .is_err());
    assert!(frame
        .encode_with_options(ExportOptions {
            bit_depth: BitDepth::Sixteen,
            ..Default::default()
        })
        .is_err());
}
