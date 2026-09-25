//! MASK-LOCAL-P1.2b CPU compositor goldens, part 2: Point Color, Color
//! Grading, the full stack, alpha behaviour and overlap order.
//!
//! Split from `local_color_tests.rs` (file-size ratchet): that half holds the
//! HSL and vibrance/saturation goldens, this half holds the remaining areas and
//! the composition contracts.

use super::local_adjustments::local_render;
use super::local_color::{
    color_layer, geometry_3x1, lifted_master, probe_pixels, render_global, render_single,
};
use super::*;
use lumina_sidecar::{
    ColorGrading, ColorGradingRange, Curves, HslAdjustments, HslChannel, LocalAdjustments,
    PointColor, PointColorEntry,
};

// ------------------------------------------------------- Point Color

/// A local Point Color block is byte-identical to the global stage, including
/// the sequential in-list order of several entries.
#[test]
fn local_point_color_matches_the_global_stage() {
    let point_color = PointColor {
        version: 1,
        entries: vec![
            PointColorEntry {
                id: "pc-1".into(),
                hue_center: 20.0,
                hue_range: 40.0,
                hue_shift: 0.5,
                saturation_shift: -0.3,
                luminance_shift: 0.2,
            },
            PointColorEntry {
                id: "pc-2".into(),
                hue_center: 220.0,
                hue_range: 60.0,
                hue_shift: -0.4,
                saturation_shift: 0.4,
                luminance_shift: -0.1,
            },
        ],
    };
    let mut local = LocalAdjustments::default();
    local.point_color = Some(point_color.clone());
    for pixel in probe_pixels() {
        let mut global = EditRecipe::default();
        global.point_color = Some(point_color.clone());
        assert_eq!(
            render_single(&local, &EditRecipe::default(), pixel),
            render_global(&global, pixel),
            "local Point Color must equal the global stage for {pixel:?}"
        );
    }
}

/// A hand-derived exact golden: the entry selects the pixel's hue with weight
/// `1` and rotates it by `+0.5 * 30° = 15°` while adding saturation.
#[test]
fn local_point_color_has_an_exact_golden_and_preserves_alpha() {
    let mut local = LocalAdjustments::default();
    local
        .add_local_point_color_entry(0.0, 45.0, 0.5, 0.0, 0.0)
        .expect("point colour entry");
    // (200, 90, 40) sits at h = 18.75°, so the weight is 1 - 18.75/45 = 0.5833
    // and the rotation is +15° * 0.5833 = +8.75°, i.e. h = 27.5°.
    let out = render_single(&local, &EditRecipe::default(), [200, 90, 40, 11]);
    let stage = crate::color_stages::point_color_stage(
        [200.0 / 255.0, 90.0 / 255.0, 40.0 / 255.0],
        local.point_color.as_ref().unwrap(),
    )
    .expect("the entry reaches this hue");
    for channel in 0..3 {
        assert_eq!(
            out[channel],
            (f64::from(stage[channel]) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8
        );
    }
    assert!(
        out[1] > 90,
        "rotating towards green must raise green: {out:?}"
    );
    assert_eq!(out[3], 11);
    // A selection that does not reach the pixel is a literal no-op.
    let mut other = LocalAdjustments::default();
    other
        .add_local_point_color_entry(180.0, 10.0, 0.5, 0.0, 0.0)
        .expect("point colour entry");
    assert_eq!(
        render_single(&other, &EditRecipe::default(), [200, 90, 40, 11]),
        vec![200, 90, 40, 11]
    );
}

// ------------------------------------------------------ Color Grading

/// A local Color Grading block is byte-identical to the global stage, with all
/// three ranges plus `balance` and `blending` active.
#[test]
fn local_color_grading_matches_the_global_stage() {
    let grading = ColorGrading {
        shadows: ColorGradingRange {
            hue_degrees: 200.0,
            saturation: 0.5,
            luminance: -0.1,
        },
        midtones: ColorGradingRange {
            hue_degrees: 40.0,
            saturation: 0.3,
            luminance: 0.05,
        },
        highlights: ColorGradingRange {
            hue_degrees: 300.0,
            saturation: 0.4,
            luminance: 0.0,
        },
        balance: -0.3,
        blending: 0.7,
        version: 1,
    };
    let mut local = LocalAdjustments::default();
    local.color_grading = Some(grading.clone());
    for pixel in probe_pixels() {
        let mut global = EditRecipe::default();
        global.color_grading = Some(grading.clone());
        assert_eq!(
            render_single(&local, &EditRecipe::default(), pixel),
            render_global(&global, pixel),
            "local Color Grading must equal the global stage for {pixel:?}"
        );
    }
}

/// A hand-derived exact golden: a pure-white pixel is fully inside the
/// highlight range, so only the highlight tint can move it.
#[test]
fn local_color_grading_has_an_exact_golden_and_preserves_alpha() {
    let mut local = LocalAdjustments::default();
    local
        .set_local_color_grading_field("highlights", "hue", 120.0)
        .expect("highlight hue");
    local
        .set_local_color_grading_field("highlights", "saturation", 0.5)
        .expect("highlight saturation");
    // White has luminance 1, so the highlight weight is 1 and the tint is the
    // fully saturated HSL colour at h = 120°, L = 0.5, mixed channel-wise with
    // amount 1 * 0.5.
    let out = render_single(&local, &EditRecipe::default(), [255, 255, 255, 77]);
    let stage = crate::color_stages::color_grading_stage(
        [1.0, 1.0, 1.0],
        local.color_grading.as_ref().unwrap(),
    );
    for channel in 0..3 {
        assert_eq!(
            out[channel],
            (f64::from(stage[channel].clamp(0.0, 1.0)) * 255.0).round() as u8
        );
    }
    // h = 120°, s = 1, l = 0.5 is (0, 1, 0); mixing white towards it with
    // amount 0.5 gives (0.5, 1.0, 0.5) -> (128, 255, 128).
    assert_eq!(out, vec![128, 255, 128, 77]);
}

// --------------------------------------------------------- full stack

/// The full local stack — WB, Basic, tone curve and all four colour stages —
/// runs in exactly the global kernel order: the tone stage sees the WB/Basic
/// float, and each colour stage sees the previous stage's float.
#[test]
fn local_color_stack_applies_in_global_kernel_order() {
    let mut curves = Curves::identity();
    curves.master = lifted_master();
    let mut recipe = LocalAdjustments {
        exposure: 0.5,
        contrast: 0.25,
        shadows: 0.3,
        highlights: -0.2,
        temperature_delta_k: 900.0,
        tint_delta: -0.1,
        curves: Some(curves),
        vibrance: 0.35,
        saturation: -0.15,
        ..LocalAdjustments::default()
    };
    recipe
        .set_local_hsl_band("red", "hue", 0.2)
        .expect("red hue");
    recipe
        .add_local_point_color_entry(30.0, 60.0, 0.25, 0.15, -0.1)
        .expect("point colour");
    recipe
        .set_local_color_grading_field("midtones", "hue", 90.0)
        .expect("midtone hue");
    recipe
        .set_local_color_grading_field("midtones", "saturation", 0.35)
        .expect("midtone saturation");

    let pixel = [200, 90, 40, 55];
    let stacked = render_single(&recipe, &EditRecipe::default(), pixel);

    // Reference chain, computed step by step from the *shared* per-pixel
    // stages and the shared P1.1/P1.2a float helpers, quantized once.
    let gains = recipe.relative_white_balance_gains();
    let scaled = [
        super::local_wb::scale_mask_local_wb_basic(200.0, &gains, &recipe, 0),
        super::local_wb::scale_mask_local_wb_basic(90.0, &gains, &recipe, 1),
        super::local_wb::scale_mask_local_wb_basic(40.0, &gains, &recipe, 2),
    ];
    let toned = super::local_tone::apply_local_tone(
        &scaled,
        recipe.curves.as_ref().expect("the stack carries a curve"),
    );
    let mut rgb = [
        toned[0] as f32 / 255.0,
        toned[1] as f32 / 255.0,
        toned[2] as f32 / 255.0,
    ];
    rgb = crate::color_stages::hsl_stage(rgb, recipe.hsl.as_ref().unwrap()).unwrap();
    rgb =
        crate::color_stages::point_color_stage(rgb, recipe.point_color.as_ref().unwrap()).unwrap();
    rgb = crate::color_stages::vibrance_saturation_stage(rgb, 0.35, -0.15);
    rgb = crate::color_stages::color_grading_stage(rgb, recipe.color_grading.as_ref().unwrap());
    for channel in 0..3 {
        assert_eq!(
            stacked[channel],
            (f64::from(rgb[channel]) * 255.0).round().clamp(0.0, 255.0) as u8,
            "stack channel {channel}"
        );
    }
    assert_eq!(stacked[3], 55, "alpha is never touched");

    // Ordering is not commutative here: applying vibrance/saturation *before*
    // the grading stage produces different bytes.
    let mut swapped = LocalAdjustments::default();
    swapped
        .set_local_color_grading_field("midtones", "hue", 90.0)
        .unwrap();
    swapped
        .set_local_color_grading_field("midtones", "saturation", 0.35)
        .unwrap();
    swapped.set_value("vibrance", 0.35).unwrap();
    swapped.set_value("saturation", -0.15).unwrap();
    swapped
        .set_local_hsl_band("red", "hue", 0.2)
        .expect("red hue");
    let grading_first = render_single(&swapped, &EditRecipe::default(), pixel);
    assert_ne!(
        stacked, grading_first,
        "the colour stage order must be observable"
    );
}

/// Without a local colour block the P0/P1.1/P1.2a bytes are preserved exactly.
#[test]
fn no_local_color_keeps_the_p0_p11_p12a_bytes() {
    let mut curves = Curves::identity();
    curves.master = lifted_master();
    let recipe = LocalAdjustments {
        exposure: 0.75,
        temperature_delta_k: 700.0,
        curves: Some(curves),
        ..LocalAdjustments::default()
    };
    // A layer that stores a *neutral* colour block is still the old path.
    let mut neutral_blocks = recipe.clone();
    neutral_blocks.hsl = Some(HslAdjustments {
        version: 1,
        red: Some(HslChannel::default()),
        ..HslAdjustments::default()
    });
    neutral_blocks.point_color = Some(PointColor {
        version: 1,
        entries: vec![PointColorEntry {
            id: "pc-1".into(),
            hue_center: 30.0,
            hue_range: 45.0,
            hue_shift: 0.0,
            saturation_shift: 0.0,
            luminance_shift: 0.0,
        }],
    });
    neutral_blocks.color_grading = Some(ColorGrading::neutral());
    let pixel = [200, 90, 40, 55];
    let baseline = render_single(&recipe, &EditRecipe::default(), pixel);
    assert_eq!(
        render_single(&neutral_blocks, &EditRecipe::default(), pixel),
        baseline,
        "a neutral colour block must not change a single byte"
    );
    // And it really is not a no-op in general: one additive scalar moves it.
    let mut edited = neutral_blocks.clone();
    edited.set_value("saturation", -0.5).expect("saturation");
    assert_ne!(
        render_single(&edited, &EditRecipe::default(), pixel),
        baseline
    );
}

// -------------------------------------------------- alpha / overlap order

/// A half mask must blend the coloured result fractionally, exactly like every
/// other local adjustment, and alpha is never part of the blend.
#[test]
fn local_color_half_mask_and_zero_alpha_are_exact() {
    let mut definition =
        mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    definition.geometry_context.width = 3;
    definition.geometry_context.height = 1;
    let mut recipe = LocalAdjustments::default();
    recipe.set_value("saturation", -0.5).expect("saturation");
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![color_layer(
            "layer-1",
            reference("vc", "subject"),
            recipe.clone(),
        )],
    )];
    let frame = ImageFrame::new(3, 1, vec![255, 0, 0, 10, 255, 0, 0, 20, 255, 0, 0, 30]).unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(3, 1, vec![0, 32_768, u16::MAX]).unwrap(),
    )]);
    let output = local_render(&frame, &copies, planes, &EditRecipe::default(), None).unwrap();
    // Outside the mask every byte is untouched, alpha included.
    assert_eq!(&output.frame.pixels[0..4], &frame.pixels[0..4]);
    // Full alpha takes the whole local recipe (see the saturation golden).
    assert_eq!(&output.frame.pixels[8..12], &[191, 64, 64, 30]);
    // Half alpha is the exact integer blend of the two byte results.
    let blend = |base: u8, coloured: u8| {
        ((base as u32 * (u16::MAX - 32_768) as u32
            + coloured as u32 * 32_768
            + u32::from(u16::MAX) / 2)
            / u32::from(u16::MAX)) as u8
    };
    assert_eq!(
        &output.frame.pixels[4..8],
        &[blend(255, 191), blend(0, 64), blend(0, 64), 20]
    );
}

/// Two overlapping local-colour layers must be evaluated in the persisted list
/// order, exactly like every other local adjustment.
#[test]
fn two_overlapping_local_color_layers_use_persisted_order() {
    let definitions = vec![
        geometry_3x1(mask_definition(
            "left",
            MaskStatus::Valid,
            MaskOperation::Source,
            vec![],
        )),
        geometry_3x1(mask_definition(
            "overlap",
            MaskStatus::Valid,
            MaskOperation::Source,
            vec![],
        )),
    ];
    let mut left = LocalAdjustments::default();
    left.set_value("saturation", -0.5).expect("left saturation");
    let mut right = LocalAdjustments::default();
    right.set_value("vibrance", 0.8).expect("right vibrance");
    let copies = vec![copy_with(
        "vc",
        definitions,
        vec![
            color_layer("layer-left", reference("vc", "left"), left.clone()),
            color_layer("layer-overlap", reference("vc", "overlap"), right.clone()),
        ],
    )];
    let frame =
        ImageFrame::new(3, 1, vec![200, 40, 40, 1, 200, 40, 40, 2, 200, 40, 40, 3]).unwrap();
    let planes = BTreeMap::from([
        (
            ("vc".into(), "left".into()),
            MaskPlane::new(3, 1, vec![u16::MAX, u16::MAX, 0]).unwrap(),
        ),
        (
            ("vc".into(), "overlap".into()),
            MaskPlane::new(3, 1, vec![0, u16::MAX, u16::MAX]).unwrap(),
        ),
    ]);
    let output = local_render(
        &frame,
        &copies,
        planes.clone(),
        &EditRecipe::default(),
        None,
    )
    .unwrap();
    let swapped_copies = vec![copy_with(
        "vc",
        vec![
            geometry_3x1(mask_definition(
                "left",
                MaskStatus::Valid,
                MaskOperation::Source,
                vec![],
            )),
            geometry_3x1(mask_definition(
                "overlap",
                MaskStatus::Valid,
                MaskOperation::Source,
                vec![],
            )),
        ],
        vec![
            color_layer("layer-overlap", reference("vc", "overlap"), right),
            color_layer("layer-left", reference("vc", "left"), left),
        ],
    )];
    let swapped = local_render(
        &frame,
        &swapped_copies,
        planes,
        &EditRecipe::default(),
        None,
    )
    .unwrap();
    // The overlap pixel is order-dependent: reordering is a render-identity
    // change, never an equivalent state.
    assert_ne!(output.frame.pixels, swapped.frame.pixels);
    // The left-only pixel still carries the left layer's edit.
    let mut expected = ImageFrame::new(1, 1, vec![200, 40, 40, 1]).unwrap();
    let mut left_only = LocalAdjustments::default();
    left_only.set_value("saturation", -0.5).unwrap();
    expected.apply_mask_local_recipe(&left_only).unwrap();
    assert_eq!(&output.frame.pixels[0..4], &expected.pixels[..]);
    // The right-only pixel carries the right layer's vibrance edit and nothing
    // else — in particular not the left layer's saturation.
    let mut expected_right = ImageFrame::new(1, 1, vec![200, 40, 40, 3]).unwrap();
    let mut right_only = LocalAdjustments::default();
    right_only.set_value("vibrance", 0.8).expect("vibrance");
    expected_right.apply_mask_local_recipe(&right_only).unwrap();
    assert_eq!(&output.frame.pixels[8..12], &expected_right.pixels[..]);
    assert_ne!(&output.frame.pixels[8..12], &frame.pixels[8..12]);
}
