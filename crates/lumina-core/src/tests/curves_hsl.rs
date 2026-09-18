use super::*;

#[test]
fn identity_curve_and_hsl_preserve_rgb_and_alpha() {
    let identity = vec![
        lumina_sidecar::CurvePoint {
            input: 0.0,
            output: 0.0,
        },
        lumina_sidecar::CurvePoint {
            input: 1.0,
            output: 1.0,
        },
    ];
    let hsl = lumina_sidecar::HslAdjustments {
        version: 1,
        red: None,
        orange: None,
        yellow: None,
        green: None,
        cyan: None,
        blue: None,
        violet: None,
        magenta: None,
    };
    let mut frame = ImageFrame::new(1, 1, vec![80, 140, 210, 7]).unwrap();
    let recipe = EditRecipe {
        curves: Some(lumina_sidecar::Curves {
            version: 1,
            master: identity,
            channels: Default::default(),
        }),
        hsl: Some(hsl),
        ..Default::default()
    };
    frame.apply_recipe(&recipe).unwrap();
    assert_eq!(frame.pixels, vec![80, 140, 210, 7]);
}

#[test]
fn apply_recipe_rejects_invalid_nested_curves() {
    let point = |input, output| lumina_sidecar::CurvePoint { input, output };
    let invalid_curves = [
        lumina_sidecar::Curves {
            version: 2,
            master: vec![point(0.0, 0.0), point(1.0, 1.0)],
            channels: Default::default(),
        },
        lumina_sidecar::Curves {
            version: 1,
            master: vec![point(0.0, 0.0)],
            channels: Default::default(),
        },
        lumina_sidecar::Curves {
            version: 1,
            master: vec![point(0.0, 0.0), point(0.5, 0.5), point(0.4, 1.0)],
            channels: Default::default(),
        },
        lumina_sidecar::Curves {
            version: 1,
            master: vec![point(0.0, 0.1), point(1.0, 1.0)],
            channels: Default::default(),
        },
        lumina_sidecar::Curves {
            version: 1,
            master: vec![point(0.0, 0.0), point(1.0, 0.9)],
            channels: lumina_sidecar::CurveChannels {
                red: Some(vec![point(0.0, 0.0), point(1.0, f32::NAN)]),
                ..Default::default()
            },
        },
    ];

    for curves in invalid_curves {
        let mut frame = ImageFrame::new(1, 1, vec![10, 20, 30, 255]).unwrap();
        let recipe = EditRecipe {
            curves: Some(curves),
            ..Default::default()
        };
        assert!(matches!(
            frame.apply_recipe(&recipe),
            Err(CoreError::InvalidAdjustment { .. })
        ));
    }
}

#[test]
fn apply_recipe_rejects_invalid_nested_hsl() {
    for (version, channel) in [
        (2, None),
        (
            1,
            Some(lumina_sidecar::HslChannel {
                hue: f32::INFINITY,
                ..Default::default()
            }),
        ),
        (
            1,
            Some(lumina_sidecar::HslChannel {
                saturation: -1.01,
                ..Default::default()
            }),
        ),
        (
            1,
            Some(lumina_sidecar::HslChannel {
                luminance: f32::NAN,
                ..Default::default()
            }),
        ),
    ] {
        let mut frame = ImageFrame::new(1, 1, vec![10, 20, 30, 255]).unwrap();
        let recipe = EditRecipe {
            hsl: Some(lumina_sidecar::HslAdjustments {
                version,
                red: channel,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(matches!(
            frame.apply_recipe(&recipe),
            Err(CoreError::InvalidAdjustment { .. })
        ));
    }
}

#[test]
fn vibrance_and_saturation_preserve_alpha_and_have_identity() {
    let original = ImageFrame::new(2, 1, vec![80, 140, 210, 7, 220, 80, 80, 19]).unwrap();
    let mut identity = original.clone();
    identity.apply_recipe(&recipe(&[])).unwrap();
    assert_eq!(identity, original);
    let mut adjusted = original.clone();
    adjusted
        .apply_recipe(&recipe(&[("vibrance", 0.5), ("saturation", 0.25)]))
        .unwrap();
    assert_eq!(&adjusted.pixels[3..4], &[7]);
    assert_eq!(&adjusted.pixels[7..8], &[19]);
    assert_ne!(&adjusted.pixels[..3], &original.pixels[..3]);
}
