//! Adapter-free unit checks for [`unsupported_gpu_stages`], extracted from
//! `golden.rs` (file-size ratchet, `DoD.md` §8). The child reaches the
//! parent test crate's imports and helpers through `use super::*`.
//!
//! This test is not adapter-dependent; it stays an always-run test.

use super::*;

/// Always-runs unit checks for [`unsupported_gpu_stages`]: supported recipes
/// stay unflagged; every still-unsupported stage produces its reason.
///
/// GPU-RENDER-PARITY-1 moved curves, HSL, Point Color, Presence and
/// vibrance/saturation into the GPU pipeline, so they are no longer flagged —
/// including at non-neutral values. Effects, unbound source actions and the
/// remaining neighborhood stages stay CPU-routed.
#[test]
fn gpu_support_validator_flags_exactly_the_unsupported_stages() {
    // Supported: tone/WB sliders plus the GPU-RENDER-PARITY-1 color/presence
    // stages, at non-neutral values.
    assert!(unsupported_gpu_stages(&EditRecipe::default()).is_empty());
    assert!(unsupported_gpu_stages(&EditRecipe {
        adjustments: BTreeMap::from([
            ("exposure".into(), 1.0),
            ("contrast".into(), -0.2),
            ("highlights".into(), 0.1),
            ("shadows".into(), -0.1),
            ("whites".into(), 0.2),
            ("blacks".into(), -0.2),
            ("wb_temperature".into(), 5500.0),
            ("wb_tint".into(), 0.05),
            ("vibrance".into(), 0.3),
            ("saturation".into(), -0.5),
        ]),
        curves: Some(Curves {
            version: 1,
            master: vec![
                CurvePoint {
                    input: 0.0,
                    output: 0.0
                },
                CurvePoint {
                    input: 0.5,
                    output: 0.4
                },
                CurvePoint {
                    input: 1.0,
                    output: 1.0
                }
            ],
            channels: Default::default(),
        }),
        hsl: Some(HslAdjustments {
            version: 1,
            blue: Some(HslChannel {
                hue: -0.1,
                saturation: 0.2,
                luminance: 0.0,
            }),
            ..Default::default()
        }),
        presence: Some(Presence {
            version: 1,
            texture: 0.1,
            clarity: 0.2,
            dehaze: 0.3,
        }),
        ..Default::default()
    })
    .is_empty());

    // GPU-RENDER-PARITY-1 stage 2: the detail stages are GPU-supported and
    // must not be flagged, including at non-neutral values.
    assert!(unsupported_gpu_stages(&EditRecipe {
        effects: Some(Effects {
            vignette: Some(Vignette {
                version: 1,
                amount: -0.3,
                midpoint: 0.5,
                roundness: 1.0,
                feather: 0.5,
            }),
            grain: None,
        }),
        ..Default::default()
    })
    .is_empty());

    // GPU-RENDER-PARITY-1 lens-blur wave: G-05 lens blur is GPU-rendered
    // (heuristic and external depth), so an active recipe must not be flagged.
    assert!(unsupported_gpu_stages(&EditRecipe {
        lens_blur: Some(lumina_sidecar::LensBlur {
            version: 1,
            enabled: true,
            focus_rect: lumina_sidecar::FocusRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            focal_near: 0.0,
            focal_far: 0.05,
            blur_amount: 0.5,
            bokeh: lumina_sidecar::BokehShape::Round,
            depth_artifact: None,
        }),
        ..Default::default()
    })
    .is_empty());

    // Each still-unsupported stage is flagged with a recognisable reason.
    let cases: Vec<(&str, EditRecipe)> = vec![(
        "source_actions",
        EditRecipe {
            source_actions: vec![SourceActionSpec {
                version: SOURCE_ACTION_VERSION,
                kind: SourceActionKind::DustRemoval,
                artifact: SourceActionArtifactRef {
                    id: "r".into(),
                    relative_path: "b.lumina.zdata".into(),
                    checksum: "c".into(),
                },
            }],
            ..Default::default()
        },
    )];
    for (expected_reason, recipe) in cases {
        let reasons = unsupported_gpu_stages(&recipe);
        assert!(
            reasons.iter().any(|r| r.contains(expected_reason)),
            "expected a reason containing `{expected_reason}`, got {reasons:?}"
        );
    }
}
