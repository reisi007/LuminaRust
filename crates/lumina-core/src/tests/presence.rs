use super::*;

#[test]
fn presence_is_deterministic_clipped_and_preserves_alpha() {
    let original = ImageFrame::new(
        3,
        1,
        vec![20, 30, 40, 7, 128, 96, 64, 19, 240, 220, 200, 31],
    )
    .unwrap();
    let recipe = EditRecipe {
        presence: Some(lumina_sidecar::Presence {
            version: 1,
            texture: 1.0,
            clarity: 1.0,
            dehaze: 1.0,
        }),
        ..Default::default()
    };
    let mut first = original.clone();
    let mut second = original.clone();
    first.apply_recipe(&recipe).unwrap();
    second.apply_recipe(&recipe).unwrap();
    assert_eq!(first, second);
    assert_ne!(&first.pixels[..9], &original.pixels[..9]);
    assert_eq!(
        [first.pixels[3], first.pixels[7], first.pixels[11]],
        [7, 19, 31]
    );
    assert!(first.pixels[..]
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| &pixel[..3])
        .any(|channel| *channel == 0 || *channel == 255));
}

#[test]
fn presence_validation_rejects_invalid_nested_values() {
    for presence in [
        lumina_sidecar::Presence {
            version: 2,
            texture: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
        },
        lumina_sidecar::Presence {
            version: 1,
            texture: f32::NAN,
            clarity: 0.0,
            dehaze: 0.0,
        },
        lumina_sidecar::Presence {
            version: 1,
            texture: 0.0,
            clarity: 1.01,
            dehaze: 0.0,
        },
        lumina_sidecar::Presence {
            version: 1,
            texture: 0.0,
            clarity: 0.0,
            dehaze: -1.01,
        },
    ] {
        let mut frame = ImageFrame::new(1, 1, vec![10, 20, 30, 255]).unwrap();
        assert!(matches!(
            frame.apply_recipe(&EditRecipe {
                presence: Some(presence),
                ..Default::default()
            }),
            Err(CoreError::InvalidAdjustment { .. })
        ));
    }
}

#[test]
fn presence_is_identity_for_zero_and_texture_is_edge_directional() {
    let original =
        ImageFrame::new(3, 1, vec![20, 20, 20, 9, 200, 200, 200, 9, 20, 20, 20, 9]).unwrap();
    let mut identity = original.clone();
    identity
        .apply_recipe(&EditRecipe {
            presence: Some(lumina_sidecar::Presence {
                version: 1,
                texture: 0.0,
                clarity: 0.0,
                dehaze: 0.0,
            }),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(identity, original);
    let mut texture = original.clone();
    texture
        .apply_recipe(&EditRecipe {
            presence: Some(lumina_sidecar::Presence {
                version: 1,
                texture: 1.0,
                clarity: 0.0,
                dehaze: 0.0,
            }),
            ..Default::default()
        })
        .unwrap();
    assert!(texture.pixels[0] < original.pixels[0] && texture.pixels[4] > original.pixels[4]);
}

#[test]
fn clarity_has_broader_edge_influence_than_texture() {
    let pixels: Vec<u8> = (0..40)
        .flat_map(|x| {
            let v = if x < 20 { 20 } else { 200 };
            [v, v, v, 255]
        })
        .collect();
    let mut texture = ImageFrame::new(40, 1, pixels.clone()).unwrap();
    let mut clarity = ImageFrame::new(40, 1, pixels).unwrap();
    let p = |texture, clarity| EditRecipe {
        presence: Some(lumina_sidecar::Presence {
            version: 1,
            texture,
            clarity,
            dehaze: 0.0,
        }),
        ..Default::default()
    };
    texture.apply_recipe(&p(1.0, 0.0)).unwrap();
    clarity.apply_recipe(&p(0.0, 1.0)).unwrap();
    let texture_changed = texture
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[0] != 20 && px[0] != 200)
        .count();
    let clarity_changed = clarity
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[0] != 20 && px[0] != 200)
        .count();
    assert!(clarity_changed > texture_changed);
}

#[test]
fn dehaze_positive_direction_and_negative_direction_are_bounded() {
    let source = ImageFrame::new(
        3,
        1,
        vec![40, 40, 40, 255, 120, 120, 120, 255, 220, 220, 220, 255],
    )
    .unwrap();
    let make = |amount| EditRecipe {
        presence: Some(lumina_sidecar::Presence {
            version: 1,
            texture: 0.0,
            clarity: 0.0,
            dehaze: amount,
        }),
        ..Default::default()
    };
    let mut positive = source.clone();
    positive.apply_recipe(&make(1.0)).unwrap();
    let mut negative = source.clone();
    negative.apply_recipe(&make(-1.0)).unwrap();
    assert!(positive.pixels[8] > source.pixels[8]);
    assert!(negative.pixels[4] < source.pixels[4] && negative.pixels[8] < source.pixels[8]);
    assert!(
        negative
            .pixels
            .iter()
            .zip(source.pixels.iter())
            .map(|(a, b)| (*a as i16 - *b as i16).abs())
            .sum::<i16>()
            < 300
    );
}

#[test]
fn presence_is_applied_before_curves() {
    let curve = lumina_sidecar::Curves {
        version: 1,
        master: vec![
            lumina_sidecar::CurvePoint {
                input: 0.0,
                output: 0.0,
            },
            lumina_sidecar::CurvePoint {
                input: 0.5,
                output: 1.0,
            },
            lumina_sidecar::CurvePoint {
                input: 1.0,
                output: 1.0,
            },
        ],
        channels: Default::default(),
    };
    let presence = lumina_sidecar::Presence {
        version: 1,
        texture: 1.0,
        clarity: 0.0,
        dehaze: 0.0,
    };
    let input = vec![
        60, 60, 60, 255, 80, 80, 80, 255, 100, 100, 100, 255, 120, 120, 120, 255, 140, 140, 140,
        255,
    ];
    let mut combined = ImageFrame::new(5, 1, input.clone()).unwrap();
    combined
        .apply_recipe(&EditRecipe {
            presence: Some(presence),
            curves: Some(curve.clone()),
            ..Default::default()
        })
        .unwrap();
    let mut reverse = ImageFrame::new(5, 1, input).unwrap();
    reverse
        .apply_recipe(&EditRecipe {
            curves: Some(curve),
            ..Default::default()
        })
        .unwrap();
    reverse
        .apply_recipe(&EditRecipe {
            presence: Some(presence),
            ..Default::default()
        })
        .unwrap();
    assert_ne!(combined.pixels, reverse.pixels);
}
