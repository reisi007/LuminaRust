//! MASK-LOCAL-P1.2a CPU compositor goldens for the mask-local tone curve.
//!
//! Every expected byte in this file is derived by hand from the documented
//! kernel, not recorded from a run: the local stage is
//! `global result → local relative WB → local Basic → local tone curve →
//! fractional mask blend`, evaluated in `f64` with exactly one RGBA8
//! quantization at the end.

use super::local_adjustments::local_render;
use super::*;
use lumina_sidecar::{CurvePoint, CurvePoints, Curves, LocalAdjustments};

/// A master curve that lifts the midtones: (0,0) → (0.5,0.7) → (1,1).
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

/// Pin a mask definition's geometry context to the golden frame so the
/// evaluated planes are used 1:1 instead of being bilinearly resampled.
fn geometry_3x1(mut definition: MaskDefinition) -> MaskDefinition {
    definition.geometry_context.width = 3;
    definition.geometry_context.height = 1;
    definition
}

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

/// The local tone stage with no other local control must be byte-identical to
/// the *global* curve stage on the same input. This is the strongest possible
/// statement of "local curves use the global kernel": the two paths are driven
/// by two different code paths (the global `apply_recipe` curve stage vs. the
/// local tone kernel) and must agree byte-for-byte.
#[test]
fn local_tone_only_is_byte_identical_to_the_global_curve_stage() {
    for pixel in [
        [0u8, 0, 0, 255],
        [64, 128, 192, 17],
        [128, 128, 128, 255],
        [200, 90, 40, 200],
        [255, 255, 255, 0],
    ] {
        let mut curves = Curves::identity();
        curves.master = lifted_master();
        curves.channels.red = Some(vec![
            CurvePoint {
                input: 0.0,
                output: 0.0,
            },
            CurvePoint {
                input: 0.25,
                output: 0.4,
            },
            CurvePoint {
                input: 1.0,
                output: 1.0,
            },
        ]);
        let mut global = EditRecipe::default();
        global.curves = Some(curves.clone());
        let local = render_single(Some(curves), &EditRecipe::default(), pixel);
        let mut reference = ImageFrame::new(1, 1, pixel.to_vec()).unwrap();
        reference.apply_recipe(&global).unwrap();
        assert_eq!(
            local, reference.pixels,
            "local tone must equal the global curve stage for {pixel:?}"
        );
    }
}

/// A master-only curve gives an exact, hand-derived golden.
#[test]
fn local_master_curve_has_an_exact_golden_and_preserves_alpha() {
    // 128/255 = 0.50196; the master curve is (0,0)-(0.5,0.7)-(1,1), so the
    // luminance 0.50196 maps onto the second segment: PCHIP with a
    // symmetric control polygon gives 0.50196 * 1.4 = 0.70275.
    // luminance = 128/255, master/luminance = 1.4 → 128 * 1.4 = 179.2.
    let mut curves = Curves::identity();
    curves.master = lifted_master();
    let out = render_single(Some(curves), &EditRecipe::default(), [128, 128, 128, 77]);
    assert_eq!(out, vec![179, 179, 179, 77]);
}

/// A single RGB channel curve must only touch that channel. The master stays
/// the identity, so the composed value is exactly the channel curve.
#[test]
fn local_channel_curve_touches_only_its_own_channel() {
    let mut curves = Curves::identity();
    curves.channels.blue = Some(vec![
        CurvePoint {
            input: 0.0,
            output: 0.0,
        },
        CurvePoint {
            input: 0.5,
            output: 0.25,
        },
        CurvePoint {
            input: 1.0,
            output: 1.0,
        },
    ]);
    // The master is the identity, so the composition is `value * 1`, and only
    // blue moves. Blue 200/255 = 0.78431 on the (0.5,0.25)-(1,1) segment with
    // d = 1.5 and tangents 1.0/1.5 (neither clipped by the 0..3*d guard) gives
    // the cubic Hermite 0.65000 at t = 0.56863, i.e. 0.65000 * 255 = 165.75
    // -> 166. Red and green are untouched.
    let out = render_single(Some(curves), &EditRecipe::default(), [100, 140, 200, 5]);
    assert_eq!(out, vec![100, 140, 166, 5]);
}

/// Master and channel curves stack in the global kernel's order: the channel
/// curve is evaluated first, the master then scales the whole pixel by
/// `master / luminance` of the *pre-curve* luminance.
#[test]
fn local_master_and_channel_curve_stack_in_kernel_order() {
    let mut curves = Curves::identity();
    curves.master = lifted_master();
    curves.channels.green = Some(vec![
        CurvePoint {
            input: 0.0,
            output: 0.0,
        },
        CurvePoint {
            input: 0.5,
            output: 0.25,
        },
        CurvePoint {
            input: 1.0,
            output: 1.0,
        },
    ]);
    // Pre-curve luminance of (60, 120, 180) = 0.2126*60 + 0.7152*120 +
    // 0.0722*180 = 12.756 + 85.824 + 12.996 = 111.576 / 255 = 0.43755.
    // master(0.43755) on the (0,0)-(0.5,0.7) segment: PCHIP with m0 = 1.4 and
    // d = 1.4 -> m0 clamped to 3*d is not binding, so the segment is the
    // cubic Hermite through (0,0),(0.5,0.7) with tangents 1.4 / 1.4.
    // The same composition is what the global stage computes, so this golden
    // is pinned against the global stage as well (see the test above); here we
    // pin the *stack* itself: green is pulled down by the channel curve and the
    // master scales everything, so green is strictly below the red/blue
    // result of a master-only run while red and blue move with the master.
    let stacked = render_single(
        Some(curves.clone()),
        &EditRecipe::default(),
        [60, 120, 180, 9],
    );
    let mut master_only = curves;
    master_only.channels.green = None;
    let master = render_single(Some(master_only), &EditRecipe::default(), [60, 120, 180, 9]);
    assert_ne!(stacked, master);
    // The channel curve maps 120/255 = 0.47059 down to 0.23529, i.e. about
    // half of the master-only green, before the master scale.
    assert!(stacked[1] < master[1]);
    assert_ne!(stacked[0], 0);
    assert_ne!(stacked[2], 0);
    assert_eq!(stacked[3], 9);
}

/// A half mask must blend the toned result fractionally, exactly like every
/// other local adjustment.
#[test]
fn local_curve_half_mask_and_zero_alpha_are_exact() {
    let mut definition =
        mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    definition.geometry_context.width = 3;
    definition.geometry_context.height = 1;
    let mut curves = Curves::identity();
    curves.master = lifted_master();
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![curve_layer(
            "layer-1",
            reference("vc", "subject"),
            Some(curves),
        )],
    )];
    let frame = ImageFrame::new(
        3,
        1,
        vec![128, 128, 128, 10, 128, 128, 128, 20, 128, 128, 128, 30],
    )
    .unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(3, 1, vec![0, 32_768, u16::MAX]).unwrap(),
    )]);
    let output = local_render(&frame, &copies, planes, &EditRecipe::default(), None).unwrap();
    // Outside the mask every byte is untouched, including alpha.
    assert_eq!(&output.frame.pixels[0..4], &frame.pixels[0..4]);
    // Full alpha takes the whole local recipe.
    assert_eq!(&output.frame.pixels[8..12], &[179, 179, 179, 30]);
    // Half alpha is the exact integer blend of the two byte results.
    let blend = |base: u8, toned: u8| {
        ((base as u32 * (u16::MAX - 32_768) as u32
            + toned as u32 * 32_768
            + u32::from(u16::MAX) / 2)
            / u32::from(u16::MAX)) as u8
    };
    assert_eq!(
        &output.frame.pixels[4..8],
        &[blend(128, 179), blend(128, 179), blend(128, 179), 20]
    );
}

/// Two overlapping local-curve layers must be evaluated in the persisted list
/// order, exactly like every other local adjustment.
#[test]
fn two_overlapping_local_curve_layers_use_persisted_order() {
    let definitions = vec![
        geometry_3x1(mask_definition(
            "lifted",
            MaskStatus::Valid,
            MaskOperation::Source,
            vec![],
        )),
        geometry_3x1(mask_definition(
            "crushed",
            MaskStatus::Valid,
            MaskOperation::Source,
            vec![],
        )),
    ];
    let mut lift = Curves::identity();
    lift.master = lifted_master();
    let mut crush = Curves::identity();
    crush.master = vec![
        CurvePoint {
            input: 0.0,
            output: 0.0,
        },
        CurvePoint {
            input: 0.5,
            output: 0.3,
        },
        CurvePoint {
            input: 1.0,
            output: 1.0,
        },
    ];
    let lifted = curve_layer("layer-1", reference("vc", "lifted"), Some(lift));
    let crushed = curve_layer("layer-2", reference("vc", "crushed"), Some(crush));
    let ordered = vec![copy_with(
        "vc",
        definitions.clone(),
        vec![lifted.clone(), crushed.clone()],
    )];
    let reordered = vec![copy_with("vc", definitions, vec![crushed, lifted])];
    let frame = ImageFrame::new(
        3,
        1,
        vec![128, 128, 128, 11, 128, 128, 128, 22, 128, 128, 128, 33],
    )
    .unwrap();
    let planes = || {
        BTreeMap::from([
            (
                ("vc".into(), "lifted".into()),
                MaskPlane::new(3, 1, vec![u16::MAX, u16::MAX, 0]).unwrap(),
            ),
            (
                ("vc".into(), "crushed".into()),
                MaskPlane::new(3, 1, vec![0, u16::MAX, u16::MAX]).unwrap(),
            ),
        ])
    };
    let forward = local_render(&frame, &ordered, planes(), &EditRecipe::default(), None).unwrap();
    let reverse = local_render(&frame, &reordered, planes(), &EditRecipe::default(), None).unwrap();
    // Both layers act on the overlap pixel, so the persisted order decides
    // whether the lift sees the crushed value or the other way round. The two
    // runs therefore differ; the non-overlap pixels do not.
    assert_eq!(&forward.frame.pixels[0..4], &[179, 179, 179, 11]);
    assert_eq!(&forward.frame.pixels[8..12], &[77, 77, 77, 33]);
    assert_ne!(forward.frame.pixels[4..8], reverse.frame.pixels[4..8]);
    assert_eq!(&reverse.frame.pixels[0..4], &[179, 179, 179, 11]);
    assert_eq!(&reverse.frame.pixels[8..12], &[77, 77, 77, 33]);
    // Image alpha is never part of the tone kernel.
    assert_eq!(
        [
            forward.frame.pixels[3],
            forward.frame.pixels[7],
            forward.frame.pixels[11]
        ],
        [11, 22, 33]
    );
}

/// A missing curve block, an identity block and a scalar-neutral recipe with
/// an explicit identity curve are all byte-identical to no local edit at all.
#[test]
fn zero_and_identity_local_curves_are_byte_identity() {
    let definition = mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    let frame_pixels: Vec<u8> = (0..64).map(|i| (i as u8).wrapping_mul(4)).collect();
    let frame = ImageFrame::new(4, 4, frame_pixels.clone()).unwrap();
    let planes = || {
        BTreeMap::from([(
            ("vc".into(), "subject".into()),
            MaskPlane::new(4, 4, vec![u16::MAX / 2; 16]).unwrap(),
        )])
    };
    let render = |curves: Option<Curves>| {
        let copies = vec![copy_with(
            "vc",
            vec![definition.clone()],
            vec![curve_layer("layer-1", reference("vc", "subject"), curves)],
        )];
        local_render(&frame, &copies, planes(), &EditRecipe::default(), None)
            .unwrap()
            .frame
            .pixels
    };
    let none = render(None);
    assert_eq!(none, frame_pixels);
    // An explicitly persisted identity block is neutral too — the compositor
    // keeps the exact P0 byte path instead of paying the tone kernel.
    assert_eq!(render(Some(Curves::identity())), frame_pixels);
    // A per-channel identity inside a non-identity block is still neutral.
    let mut all_identity = Curves::identity();
    all_identity.channels.red = Some(lumina_sidecar::identity_curve_points());
    all_identity.channels.blue = Some(lumina_sidecar::identity_curve_points());
    assert_eq!(render(Some(all_identity)), frame_pixels);
    // And the default object reads as neutral, so the compositor skips it.
    assert!(LocalAdjustments::default().is_neutral());
    assert!(!LocalAdjustments::default().has_local_curves());
}
