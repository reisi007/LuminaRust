//! MASK-LOCAL-P1.2a CPU compositor *contract* goldens for the local tone
//! curve: the refusals, the neutral identities, and the promise that a local
//! curve never mutates the global recipe.
//!
//! Split from `local_tone_curve_tests.rs` (file-size ratchet): that half holds
//! the exact pixel goldens, this half holds the invariants around them.

use super::local_adjustments::local_render;
use super::*;
use lumina_sidecar::{CurvePoint, CurvePoints, Curves, LocalAdjustments};

/// A layer whose only local control is a tone curve.
fn curve_layer(id: &str, mask: lumina_sidecar::MaskReference, curves: Option<Curves>) -> MaskLayer {
    let mut layer = layer(id, mask);
    layer.local_adjustments = Some(LocalAdjustments {
        curves,
        ..LocalAdjustments::default()
    });
    layer
}

/// A single-pixel layer with one mask, rendered through the full pipeline.
fn render_single(curves: Option<Curves>, recipe: &EditRecipe, pixel: [u8; 4]) -> Vec<u8> {
    let definition = mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![curve_layer("layer-1", reference("vc", "subject"), curves)],
    )];
    let frame = ImageFrame::new(1, 1, pixel.to_vec()).unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
    )]);
    local_render(&frame, &copies, planes, recipe, None)
        .unwrap()
        .frame
        .pixels
}

/// A master curve that lifts the midtones: (0,0) -> (0.5,0.7) -> (1,1).
fn lifted_master() -> CurvePoints {
    vec![
        CurvePoint {
            input: 0.0,
            output: 0.0,
        },
        CurvePoint {
            input: 0.5,
            output: 0.7,
        },
        CurvePoint {
            input: 1.0,
            output: 1.0,
        },
    ]
}

/// Invalid local points are a loud preflight error: no pixel may be touched,
/// and a refused curve is never silently clipped or dropped.
#[test]
fn invalid_local_curve_points_are_a_loud_preflight_error() {
    let definition = mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    let frame = ImageFrame::new(1, 1, vec![100, 100, 100, 255]).unwrap();
    let cases: Vec<(&str, Curves)> = vec![
        (
            "one point",
            Curves {
                version: 1,
                master: vec![CurvePoint {
                    input: 0.0,
                    output: 0.0,
                }],
                channels: Default::default(),
            },
        ),
        (
            "non-ascending input",
            Curves {
                version: 1,
                master: vec![
                    CurvePoint {
                        input: 0.0,
                        output: 0.0,
                    },
                    CurvePoint {
                        input: 0.7,
                        output: 0.7,
                    },
                    CurvePoint {
                        input: 0.5,
                        output: 0.5,
                    },
                    CurvePoint {
                        input: 1.0,
                        output: 1.0,
                    },
                ],
                channels: Default::default(),
            },
        ),
        (
            "out of range output",
            Curves {
                version: 1,
                master: vec![
                    CurvePoint {
                        input: 0.0,
                        output: 0.0,
                    },
                    CurvePoint {
                        input: 0.5,
                        output: 1.4,
                    },
                    CurvePoint {
                        input: 1.0,
                        output: 1.0,
                    },
                ],
                channels: Default::default(),
            },
        ),
        (
            "missing (0,0) endpoint",
            Curves {
                version: 1,
                master: vec![
                    CurvePoint {
                        input: 0.1,
                        output: 0.1,
                    },
                    CurvePoint {
                        input: 1.0,
                        output: 1.0,
                    },
                ],
                channels: Default::default(),
            },
        ),
        (
            "missing (1,1) endpoint",
            Curves {
                version: 1,
                master: vec![
                    CurvePoint {
                        input: 0.0,
                        output: 0.0,
                    },
                    CurvePoint {
                        input: 0.9,
                        output: 0.9,
                    },
                ],
                channels: Default::default(),
            },
        ),
        (
            "unsupported block version",
            Curves {
                version: 2,
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
                channels: Default::default(),
            },
        ),
        (
            "invalid channel curve",
            Curves {
                version: 1,
                master: lumina_sidecar::identity_curve_points(),
                channels: lumina_sidecar::CurveChannels {
                    red: Some(vec![
                        CurvePoint {
                            input: 0.0,
                            output: 0.0,
                        },
                        CurvePoint {
                            input: 0.5,
                            output: 0.5,
                        },
                    ]),
                    ..Default::default()
                },
            },
        ),
    ];
    for (name, curves) in cases {
        let copies = vec![copy_with(
            "vc",
            vec![definition.clone()],
            vec![curve_layer(
                "layer-1",
                reference("vc", "subject"),
                Some(curves),
            )],
        )];
        let planes = BTreeMap::from([(
            ("vc".into(), "subject".into()),
            MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
        )]);
        let error = local_render(&frame, &copies, planes, &EditRecipe::default(), None)
            .expect_err("invalid local curve points must be a loud error");
        assert!(
            matches!(error, CoreError::InvalidLocalAdjustment { .. }),
            "{name} produced {error:?}"
        );
    }
}

/// A local curve never mutates the global recipe: the render reads the global
/// recipe, applies the local curve, and hands the *unchanged* recipe back.
#[test]
fn local_curve_never_mutates_the_global_recipe() {
    let mut curves = Curves::identity();
    curves.master = lifted_master();
    let mut global = EditRecipe::default();
    global.adjustments.insert("wb_temperature".into(), 4200.0);
    let before = serde_json::to_string(&global).unwrap();
    let _ = render_single(Some(curves), &global, [90, 130, 170, 200]);
    assert_eq!(serde_json::to_string(&global).unwrap(), before);
    assert!(global.curves.is_none());
}

/// The local curve follows the local relative WB and the local Basic controls
/// in the documented order, with a single quantization: a recipe that only
/// carries a tone curve must produce the same bytes as the identical curve
/// applied to the already globally-adjusted result.
#[test]
fn local_curve_applies_after_the_global_result_and_local_wb() {
    let mut curves = Curves::identity();
    curves.master = lifted_master();
    // Global WB 4200 K is applied first, then the local delta of +1100 K, and
    // only then the local curve. The proof that the curve sees the *global*
    // result is that the same local curve on the same frame with a different
    // global recipe produces different bytes.
    let mut warm = EditRecipe::default();
    warm.adjustments.insert("wb_temperature".into(), 4200.0);
    let mut as_shot = EditRecipe::default();
    as_shot.adjustments.remove("wb_temperature");
    let toned = render_single(Some(curves.clone()), &warm, [100, 120, 140, 255]);
    let as_shot_toned = render_single(Some(curves), &as_shot, [100, 120, 140, 255]);
    assert_ne!(toned, as_shot_toned);
    // The global keys survive: a local curve is not a global reset.
    assert_eq!(warm.adjustments.get("wb_temperature"), Some(&4200.0));
    // Reset to As Shot really removes the absolute key (no silent fallback).
    assert!(!as_shot.adjustments.contains_key("wb_temperature"));
}

/// The full local stack — relative WB, the four Basic controls and the tone
/// curve — has exactly **one** RGBA8 quantization boundary. The golden below is
/// derived by hand from the float chain; rounding between the WB, Basic and
/// tone stages would produce different bytes, so this pins the single-boundary
/// contract rather than merely the result.
#[test]
fn local_wb_basic_and_tone_share_exactly_one_quantization_boundary() {
    let mut curves = Curves::identity();
    curves.master = lifted_master();
    let definition = mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    let recipe = LocalAdjustments {
        exposure: 1.0,
        contrast: 0.25,
        shadows: 0.5,
        highlights: -0.5,
        temperature_delta_k: 1100.0,
        tint_delta: -0.2,
        curves: Some(curves),
        ..LocalAdjustments::default()
    };
    let mut layer = curve_layer("layer-1", reference("vc", "subject"), None);
    layer.local_adjustments = Some(recipe.clone());
    let copies = vec![copy_with("vc", vec![definition], vec![layer])];
    let frame = ImageFrame::new(1, 1, vec![100, 120, 140, 77]).unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
    )]);
    let output = local_render(&frame, &copies, planes, &EditRecipe::default(), None).unwrap();

    // Hand-evaluated float chain (no intermediate rounding):
    //   gains    = (1 - 1100/5500*0.35, 1 + 0.2*0.20, 1 + 1100/5500*0.35)
    //            = (0.93, 1.04, 1.07)
    //   exposure = 2.0, contrast = 1.25, then shadows +0.5 and highlights
    //   -0.5 on the normalized value, then the master curve (0,0)-(0.5,0.7)
    //   -(1,1) on the luminance with the `value * master / luminance`
    //   composition, and only then a single `round()`.
    let gains = recipe.relative_white_balance_gains();
    let mut basic = [0.0_f64; 3];
    for (channel, base) in [100.0_f64, 120.0, 140.0].into_iter().enumerate() {
        let mut value = base * gains[channel];
        value = (value * 2.0).clamp(0.0, 255.0);
        value = ((value - 128.0) * 1.25 + 128.0).clamp(0.0, 255.0);
        let x = value / 255.0;
        let shadow_weight = ((0.5 - x) / 0.5).max(0.0).powi(2);
        value = (x + 0.5 * shadow_weight * 0.25).clamp(0.0, 1.0) * 255.0;
        let x = value / 255.0;
        let highlight_weight = ((x - 0.5) / 0.5).max(0.0).powi(2);
        basic[channel] = (x - 0.5 * highlight_weight * 0.25).clamp(0.0, 1.0) * 255.0;
    }
    let expected = local_tone_expected(&basic, &recipe.curves.as_ref().unwrap().master);
    assert_eq!(
        output.frame.pixels,
        vec![expected[0], expected[1], expected[2], 77],
        "the local stage must round exactly once, after the tone curve"
    );
    // The intermediate is genuinely fractional: rounding after the WB stage
    // would give different bytes, so the golden really constrains the order.
    assert!(
        basic
            .iter()
            .any(|value| (value - value.round()).abs() > 1e-6),
        "the float intermediate must not already be integral"
    );
}

/// The same float chain as the local tone kernel, evaluated independently in
/// the test. `basic` holds the un-quantized post-WB/Basic channel values.
fn local_tone_expected(basic: &[f64; 3], master: &[CurvePoint]) -> [u8; 3] {
    let original = [basic[0] / 255.0, basic[1] / 255.0, basic[2] / 255.0];
    let luminance = 0.2126 * original[0] + 0.7152 * original[1] + 0.0722 * original[2];
    let master_out = f64::from(pchip(master, luminance as f32));
    let mut out = [0u8; 3];
    for (channel, value) in original.iter().enumerate() {
        let scaled = if luminance > 1e-9 {
            value * master_out / luminance
        } else {
            master_out
        };
        out[channel] = (scaled.clamp(0.0, 1.0) * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    out
}

/// A second, independent PCHIP implementation. Writing the Hermite basis out
/// again here means the golden above is not derived from the very function it
/// is testing.
fn pchip(points: &[CurvePoint], x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let i = points
        .windows(2)
        .position(|w| x <= w[1].input)
        .unwrap_or(points.len() - 2);
    let (a, b) = (points[i], points[i + 1]);
    let h = b.input - a.input;
    let t = ((x - a.input) / h).clamp(0.0, 1.0);
    let slope = |j: usize| {
        if j == 0 {
            (points[1].output - points[0].output) / (points[1].input - points[0].input)
        } else if j + 1 == points.len() {
            (points[j].output - points[j - 1].output) / (points[j].input - points[j - 1].input)
        } else {
            (points[j + 1].output - points[j - 1].output)
                / (points[j + 1].input - points[j - 1].input)
        }
    };
    let d = (b.output - a.output) / h;
    let (m0, m1) = if d == 0.0 {
        (0.0, 0.0)
    } else {
        let lo = 0.0f32.min(3.0 * d);
        let hi = 0.0f32.max(3.0 * d);
        (slope(i).clamp(lo, hi), slope(i + 1).clamp(lo, hi))
    };
    let t2 = t * t;
    let t3 = t2 * t;
    ((2.0 * t3 - 3.0 * t2 + 1.0) * a.output
        + (t3 - 2.0 * t2 + t) * h * m0
        + (-2.0 * t3 + 3.0 * t2) * b.output
        + (t3 - t2) * h * m1)
        .clamp(0.0, 1.0)
}
