use super::*;

#[test]
fn exposure_clamps_channels_and_preserves_alpha() {
    let mut frame = ImageFrame::new(1, 1, vec![100, 200, 255, 17]).unwrap();
    frame.apply_recipe(&recipe(&[("exposure", 1.0)])).unwrap();
    assert_eq!(frame.pixels, vec![200, 255, 255, 17]);
}

#[test]
fn contrast_is_linear_around_midpoint() {
    let mut frame = ImageFrame::new(1, 1, vec![64, 128, 192, 9]).unwrap();
    frame.apply_recipe(&recipe(&[("contrast", 1.0)])).unwrap();
    assert_eq!(frame.pixels, vec![0, 128, 255, 9]);
}

#[test]
fn rejects_invalid_and_unknown_adjustments() {
    let mut frame = ImageFrame::new(1, 1, vec![1, 2, 3, 255]).unwrap();
    assert!(matches!(
        frame.apply_recipe(&recipe(&[("contrast", 2.0)])),
        Err(CoreError::InvalidAdjustment { .. })
    ));
    assert!(matches!(
        frame.apply_recipe(&recipe(&[("clarity", 0.5)])),
        Err(CoreError::UnsupportedAdjustment { key }) if key == "clarity"
    ));
    assert!(frame
        .apply_recipe(&recipe(&[("exposure", f64::NAN)]))
        .is_err());
}

#[test]
fn rejects_non_finite_and_out_of_range_values_for_each_adjustment() {
    for (name, values) in [
        (
            "exposure",
            [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -10.1, 10.1],
        ),
        (
            "contrast",
            [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.1, 1.1],
        ),
        (
            "highlights",
            [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.1, 1.1],
        ),
        (
            "shadows",
            [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.1, 1.1],
        ),
    ] {
        for value in values {
            let mut frame = ImageFrame::new(1, 1, vec![1, 2, 3, 255]).unwrap();
            assert!(
                matches!(
                    frame.apply_recipe(&recipe(&[(name, value)])),
                    Err(CoreError::InvalidAdjustment { .. })
                ),
                "{name}={value:?} should be rejected"
            );
        }
    }
}

#[test]
fn accepts_both_adjustment_boundaries() {
    for (name, boundaries) in [
        ("exposure", [-10.0, 10.0]),
        ("contrast", [-1.0, 1.0]),
        ("highlights", [-1.0, 1.0]),
        ("shadows", [-1.0, 1.0]),
    ] {
        for value in boundaries {
            let mut frame = ImageFrame::new(1, 1, vec![64, 128, 192, 255]).unwrap();
            assert!(frame.apply_recipe(&recipe(&[(name, value)])).is_ok());
        }
    }
}

#[test]
fn highlights_and_shadows_adjust_expected_tones_and_preserve_alpha() {
    let mut shadows = ImageFrame::new(1, 1, vec![32, 96, 160, 17]).unwrap();
    shadows.apply_recipe(&recipe(&[("shadows", 1.0)])).unwrap();
    assert_eq!(shadows.pixels, vec![68, 100, 160, 17]);

    let mut highlights = ImageFrame::new(1, 1, vec![96, 160, 224, 23]).unwrap();
    highlights
        .apply_recipe(&recipe(&[("highlights", -1.0)]))
        .unwrap();
    assert_eq!(highlights.pixels, vec![96, 156, 187, 23]);
}

#[test]
fn highlights_and_shadows_leave_midpoint_unchanged_and_clamp_extremes() {
    let mut frame = ImageFrame::new(2, 1, vec![0, 128, 255, 31, 250, 1, 128, 47]).unwrap();
    frame
        .apply_recipe(&recipe(&[("shadows", -1.0), ("highlights", 1.0)]))
        .unwrap();
    assert_eq!(frame.pixels, vec![0, 128, 255, 31, 255, 0, 128, 47]);
}

#[test]
fn shadows_are_applied_before_highlights() {
    let mut frame = ImageFrame::new(1, 1, vec![64, 128, 192, 255]).unwrap();
    frame
        .apply_recipe(&recipe(&[("shadows", 1.0), ("highlights", -1.0)]))
        .unwrap();
    let mut expected = ImageFrame::new(1, 1, vec![64, 128, 192, 255]).unwrap();
    expected.apply_recipe(&recipe(&[("shadows", 1.0)])).unwrap();
    expected
        .apply_recipe(&recipe(&[("highlights", -1.0)]))
        .unwrap();
    assert_eq!(frame, expected);
}
