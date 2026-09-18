use super::*;

#[test]
fn color_grading_accepts_cyclic_hue_and_preserves_alpha() {
    let range = |hue_degrees| lumina_sidecar::ColorGradingRange {
        hue_degrees,
        saturation: 0.7,
        luminance: 0.0,
    };
    let grading = lumina_sidecar::ColorGrading {
        version: 1,
        shadows: range(360.0),
        midtones: range(120.0),
        highlights: range(240.0),
        balance: 0.0,
        blending: 0.5,
    };
    let mut a = ImageFrame::new(1, 1, vec![30, 40, 50, 13]).unwrap();
    let mut b = a.clone();
    let mut zero = grading.clone();
    zero.shadows.hue_degrees = 0.0;
    a.apply_recipe(&EditRecipe {
        color_grading: Some(grading),
        ..Default::default()
    })
    .unwrap();
    b.apply_recipe(&EditRecipe {
        color_grading: Some(zero),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(a.pixels, b.pixels);
    assert_eq!(a.pixels[3], 13);
}

#[test]
fn color_grading_rejects_invalid_fields() {
    let base = lumina_sidecar::ColorGradingRange {
        hue_degrees: 0.0,
        saturation: 0.0,
        luminance: 0.0,
    };
    for grading in [
        lumina_sidecar::ColorGrading {
            version: 2,
            shadows: base,
            midtones: base,
            highlights: base,
            balance: 0.0,
            blending: 0.5,
        },
        lumina_sidecar::ColorGrading {
            version: 1,
            shadows: lumina_sidecar::ColorGradingRange {
                hue_degrees: 361.0,
                ..base
            },
            midtones: base,
            highlights: base,
            balance: 0.0,
            blending: 0.5,
        },
        lumina_sidecar::ColorGrading {
            version: 1,
            shadows: lumina_sidecar::ColorGradingRange {
                saturation: 1.1,
                ..base
            },
            midtones: base,
            highlights: base,
            balance: 0.0,
            blending: 0.5,
        },
        lumina_sidecar::ColorGrading {
            version: 1,
            shadows: base,
            midtones: base,
            highlights: base,
            balance: 1.1,
            blending: 0.5,
        },
        lumina_sidecar::ColorGrading {
            version: 1,
            shadows: base,
            midtones: base,
            highlights: base,
            balance: 0.0,
            blending: 1.1,
        },
        lumina_sidecar::ColorGrading {
            version: 1,
            shadows: lumina_sidecar::ColorGradingRange {
                luminance: -1.1,
                ..base
            },
            midtones: base,
            highlights: base,
            balance: 0.0,
            blending: 0.5,
        },
    ] {
        let mut frame = ImageFrame::new(1, 1, vec![10, 20, 30, 255]).unwrap();
        assert!(matches!(
            frame.apply_recipe(&EditRecipe {
                color_grading: Some(grading),
                ..Default::default()
            }),
            Err(CoreError::InvalidAdjustment { .. })
        ));
    }
}

/// G-02 Feinschliff: `luminance = 0` + `blending = 0.5` ist Identität bei
/// neutralen Tönungen, Luminance wirkt je Bereich, Blending ändert die
/// Gewichte deterministisch.
#[test]
fn color_grading_luminance_and_blending_behave() {
    let range = |saturation, luminance| lumina_sidecar::ColorGradingRange {
        hue_degrees: 0.0,
        saturation,
        luminance,
    };
    let grading = |saturation, luminance, blending| lumina_sidecar::ColorGrading {
        version: 1,
        shadows: range(saturation, luminance),
        midtones: range(saturation, luminance),
        highlights: range(saturation, luminance),
        balance: 0.0,
        blending,
    };
    let original = ImageFrame::new(2, 1, vec![30, 40, 50, 9, 200, 190, 180, 9]).unwrap();
    // Neutral grading (no saturation, no luminance, legacy blending) is identity.
    let mut identity = original.clone();
    identity
        .apply_recipe(&EditRecipe {
            color_grading: Some(grading(0.0, 0.0, 0.5)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(identity, original);
    // Positive shadows luminance lightens the dark pixel's lightness.
    let mut lightened = original.clone();
    lightened
        .apply_recipe(&EditRecipe {
            color_grading: Some(lumina_sidecar::ColorGrading {
                shadows: range(0.0, 1.0),
                midtones: range(0.0, 0.0),
                highlights: range(0.0, 0.0),
                version: 1,
                balance: 0.0,
                blending: 0.5,
            }),
            ..Default::default()
        })
        .unwrap();
    let before = 0.2126 * 30.0 + 0.7152 * 40.0 + 0.0722 * 50.0;
    let after = 0.2126 * f64::from(lightened.pixels[0])
        + 0.7152 * f64::from(lightened.pixels[1])
        + 0.0722 * f64::from(lightened.pixels[2]);
    assert!(
        after > before,
        "shadows luminance +1 lightens, {before} -> {after}"
    );
    assert_eq!(&lightened.pixels[3..4], &[9]);
    assert_eq!(&lightened.pixels[7..8], &[9]);
    // Blending changes the range weights deterministically.
    let render = |blending| {
        let mut frame = original.clone();
        frame
            .apply_recipe(&EditRecipe {
                color_grading: Some(grading(0.6, 0.0, blending)),
                ..Default::default()
            })
            .unwrap();
        frame.pixels.clone()
    };
    let narrow = render(0.0);
    let legacy = render(0.5);
    let wide = render(1.0);
    assert_ne!(narrow, legacy);
    assert_ne!(legacy, wide);
    assert_eq!(render(0.5), legacy, "deterministic re-render");
}

/// G-02 Point Color: Identität bei Null-Shifts/leerer Liste, Selektivität
/// (ferne Farben unverändert), zyklische Auswahl über 0°, Determinismus
/// und Alpha-Erhalt.
#[test]
fn point_color_identity_selectivity_and_cyclic_wrap() {
    use lumina_sidecar::{PointColor, PointColorEntry};
    let recipe_with = |entries: Vec<PointColorEntry>| EditRecipe {
        point_color: Some(PointColor {
            version: 1,
            entries,
        }),
        ..Default::default()
    };
    // Pure red + pure blue pixels (alpha 7/19).
    let original = ImageFrame::new(2, 1, vec![255, 0, 0, 7, 0, 0, 255, 19]).unwrap();
    // Empty list is identity.
    let mut frame = original.clone();
    frame.apply_recipe(&recipe_with(vec![])).unwrap();
    assert_eq!(frame, original);
    // All-zero shifts are identity (untouched pixels see no HSL drift).
    let mut frame = original.clone();
    frame
        .apply_recipe(&recipe_with(vec![PointColorEntry {
            id: "pc-1".into(),
            hue_center: 0.0,
            hue_range: 30.0,
            hue_shift: 0.0,
            saturation_shift: 0.0,
            luminance_shift: 0.0,
        }]))
        .unwrap();
    assert_eq!(frame, original);
    // Desaturate reds: red pixel changes, blue pixel is untouched.
    let red_entry = PointColorEntry {
        id: "pc-1".into(),
        hue_center: 0.0,
        hue_range: 30.0,
        hue_shift: 0.0,
        saturation_shift: -1.0,
        luminance_shift: 0.0,
    };
    let mut frame = original.clone();
    frame
        .apply_recipe(&recipe_with(vec![red_entry.clone()]))
        .unwrap();
    assert_ne!(&frame.pixels[..3], &original.pixels[..3]);
    assert_eq!(&frame.pixels[4..7], &original.pixels[4..7]);
    assert_eq!(&frame.pixels[3..4], &[7]);
    assert_eq!(&frame.pixels[7..8], &[19]);
    // Cyclic selection wraps over 0°: center 360° behaves exactly like
    // center 0° (pure red, hue 0°, gets full weight in both cases).
    let mut wrapped = original.clone();
    wrapped
        .apply_recipe(&recipe_with(vec![PointColorEntry {
            hue_center: 360.0,
            hue_range: 30.0,
            ..red_entry.clone()
        }]))
        .unwrap();
    assert_eq!(wrapped.pixels, frame.pixels);
    // Determinism: two runs are byte-identical.
    let mut again = original.clone();
    again.apply_recipe(&recipe_with(vec![red_entry])).unwrap();
    assert_eq!(again.pixels, frame.pixels);
}

#[test]
fn point_color_rejects_invalid_fields() {
    use lumina_sidecar::{PointColor, PointColorEntry};
    let entry = PointColorEntry {
        id: "pc-1".into(),
        hue_center: 30.0,
        hue_range: 20.0,
        hue_shift: 0.0,
        saturation_shift: 0.0,
        luminance_shift: 0.0,
    };
    let recipes = vec![
        EditRecipe {
            point_color: Some(PointColor {
                version: 2,
                entries: vec![entry.clone()],
            }),
            ..Default::default()
        },
        EditRecipe {
            point_color: Some(PointColor {
                version: 1,
                entries: vec![PointColorEntry {
                    hue_center: 400.0,
                    ..entry.clone()
                }],
            }),
            ..Default::default()
        },
        EditRecipe {
            point_color: Some(PointColor {
                version: 1,
                entries: vec![entry.clone(), entry.clone()],
            }),
            ..Default::default()
        },
        EditRecipe {
            point_color: Some(PointColor {
                version: 1,
                entries: vec![entry; 9],
            }),
            ..Default::default()
        },
    ];
    for recipe in recipes {
        let mut frame = ImageFrame::new(1, 1, vec![10, 20, 30, 255]).unwrap();
        assert!(frame.apply_recipe(&recipe).is_err());
    }
}

/// G-02 Kurve je Kanal: eine rote Punktkurve wirkt nur auf den R-Kanal.
#[test]
fn per_channel_point_curves_are_independent() {
    use lumina_sidecar::{CurveChannels, CurvePoint, Curves};
    let lift_red = Curves {
        version: 1,
        master: vec![
            CurvePoint {
                input: 0.0,
                output: 0.0,
            },
            CurvePoint {
                input: 1.0,
                output: 1.0,
            },
        ],
        channels: CurveChannels {
            red: Some(vec![
                CurvePoint {
                    input: 0.0,
                    output: 0.0,
                },
                CurvePoint {
                    input: 0.5,
                    output: 0.75,
                },
                CurvePoint {
                    input: 1.0,
                    output: 1.0,
                },
            ]),
            green: None,
            blue: None,
        },
    };
    let original = ImageFrame::new(1, 1, vec![128, 128, 128, 255]).unwrap();
    let mut frame = original.clone();
    frame
        .apply_recipe(&EditRecipe {
            curves: Some(lift_red),
            ..Default::default()
        })
        .unwrap();
    assert!(frame.pixels[0] > original.pixels[0], "red lifted");
    assert_eq!(frame.pixels[1], original.pixels[1], "green untouched");
    assert_eq!(frame.pixels[2], original.pixels[2], "blue untouched");
    assert_eq!(frame.pixels[3], 255);
}

/// G-02 Render-Golden (CPU-Referenz, Toleranz 0): alle neuen/berührten
/// Stufen (Kanal-Kurve + HSL + Point Color + Vibrance + Grading mit
/// Luminance/Blending) rendern auf einem fixen 8x8-Verlauf deterministisch
/// byte-identisch; die Änderung gegenüber dem unbearbeiteten Frame ist
/// echt (L1 > 0) und beschränkt (L1 pro Pixel <= 3*255, Alpha unberührt).
/// CPU-Stufen sind reine Funktionen der Eingabe-Bytes — daher ist die
/// Re-Render-Toleranz exakt 0 (kein PSNR-Schlupf nötig); die GPU-Route
/// ist für diese Stufen gegated (s. `lumina-gpu`).
#[test]
fn g02_color_stages_render_byte_identical_golden() {
    use lumina_sidecar::{
        ColorGrading, ColorGradingRange, CurveChannels, CurvePoint, Curves, HslAdjustments,
        HslChannel, PointColor, PointColorEntry,
    };
    let mut pixels = Vec::new();
    for y in 0..8u32 {
        for x in 0..8u32 {
            pixels.extend_from_slice(&[(x * 32) as u8, (y * 32) as u8, ((x + y) * 16) as u8, 255]);
        }
    }
    let original = ImageFrame::new(8, 8, pixels).unwrap();
    let recipe = EditRecipe {
        curves: Some(Curves {
            version: 1,
            master: vec![
                CurvePoint {
                    input: 0.0,
                    output: 0.0,
                },
                CurvePoint {
                    input: 1.0,
                    output: 1.0,
                },
            ],
            channels: CurveChannels {
                red: Some(vec![
                    CurvePoint {
                        input: 0.0,
                        output: 0.0,
                    },
                    CurvePoint {
                        input: 0.5,
                        output: 0.6,
                    },
                    CurvePoint {
                        input: 1.0,
                        output: 1.0,
                    },
                ]),
                green: None,
                blue: None,
            },
        }),
        hsl: Some(HslAdjustments {
            version: 1,
            red: Some(HslChannel {
                hue: 0.2,
                saturation: 0.1,
                luminance: -0.1,
            }),
            ..Default::default()
        }),
        point_color: Some(PointColor {
            version: 1,
            entries: vec![PointColorEntry {
                id: "pc-1".into(),
                hue_center: 30.0,
                hue_range: 40.0,
                hue_shift: 0.3,
                saturation_shift: -0.2,
                luminance_shift: 0.1,
            }],
        }),
        ..Default::default()
    };
    let mut recipe = recipe;
    recipe.adjustments.insert("vibrance".into(), 0.3);
    recipe.color_grading = Some(ColorGrading {
        version: 1,
        shadows: ColorGradingRange {
            hue_degrees: 200.0,
            saturation: 0.4,
            luminance: 0.2,
        },
        midtones: ColorGradingRange {
            hue_degrees: 40.0,
            saturation: 0.3,
            luminance: 0.0,
        },
        highlights: ColorGradingRange {
            hue_degrees: 0.0,
            saturation: 0.0,
            luminance: -0.2,
        },
        balance: 0.2,
        blending: 0.7,
    });
    let render = |recipe: &EditRecipe| {
        let mut frame = original.clone();
        frame.apply_recipe(recipe).unwrap();
        frame.pixels.clone()
    };
    let first = render(&recipe);
    let second = render(&recipe);
    assert_eq!(first, second, "re-render is byte-identical (tolerance 0)");
    assert_ne!(first, original.pixels, "stages visibly change the frame");
    let l1: u64 = first
        .iter()
        .zip(original.pixels.iter())
        .map(|(a, b)| a.abs_diff(*b) as u64)
        .sum();
    let pixels_n = (8 * 8) as u64;
    assert!(
        l1 > 0 && l1 <= pixels_n * 3 * 255,
        "bounded change, L1={l1}"
    );
    for (index, chunk) in first.as_chunks::<4>().0.iter().enumerate() {
        assert_eq!(chunk[3], 255, "alpha untouched at pixel {index}");
    }
}
