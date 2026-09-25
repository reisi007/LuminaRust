//! MASK-LOCAL-P0 CPU compositor and output-space alignment tests.

use super::*;
use lumina_sidecar::{LocalAdjustments, MaskOperation};

fn local_layer(id: &str, mask: lumina_sidecar::MaskReference, exposure: f64) -> MaskLayer {
    let mut layer = layer(id, mask);
    layer.local_adjustments = Some(LocalAdjustments {
        version: lumina_sidecar::LOCAL_ADJUSTMENTS_VERSION,
        exposure,
        ..LocalAdjustments::default()
    });
    layer
}

pub(super) fn local_render(
    frame: &ImageFrame,
    copies: &[VirtualCopy],
    planes: BTreeMap<(String, String), MaskPlane>,
    recipe: &EditRecipe,
    source_roi: Option<[f32; 4]>,
) -> Result<RenderOutput, CoreError> {
    let mut context = mask_context(copies, "vc", planes, MaskPolicy::Strict);
    context.source_roi = source_roi;
    render_frame(
        frame,
        &RenderContext {
            recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(context),
        },
    )
}

#[test]
fn local_compositor_alpha_endpoints_partial_and_alpha_preservation_are_exact() {
    let mut definition =
        mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    definition.geometry_context.height = 2;
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![local_layer("layer-1", reference("vc", "subject"), 1.0)],
    )];
    let frame = ImageFrame::new(
        2,
        2,
        vec![
            50, 11, 12, 13, 100, 21, 22, 23, 150, 31, 32, 33, 200, 41, 42, 43,
        ],
    )
    .unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(2, 2, vec![0, 32_768, u16::MAX, 0]).unwrap(),
    )]);
    let output = local_render(&frame, &copies, planes, &EditRecipe::default(), None).unwrap();
    let mut expected_full = frame.clone();
    expected_full
        .apply_recipe(
            &LocalAdjustments {
                exposure: 1.0,
                ..LocalAdjustments::default()
            }
            .as_recipe(),
        )
        .unwrap();
    assert_eq!(&output.frame.pixels[0..4], &frame.pixels[0..4]);
    let partial_expected = |base: u8, adjusted: u8| {
        ((base as u32 * (u16::MAX - 32_768) as u32
            + adjusted as u32 * 32_768
            + u16::MAX as u32 / 2)
            / u16::MAX as u32) as u8
    };
    assert_eq!(
        output.frame.pixels[4],
        partial_expected(100, expected_full.pixels[4])
    );
    assert_eq!(&output.frame.pixels[8..12], &expected_full.pixels[8..12]);
    assert_eq!(&output.frame.pixels[12..16], &frame.pixels[12..16]);
    assert_eq!(output.frame.pixels[3], frame.pixels[3]);
    assert_eq!(output.frame.pixels[7], frame.pixels[7]);
    assert_eq!(output.frame.pixels[11], frame.pixels[11]);
    assert_eq!(output.frame.pixels[15], frame.pixels[15]);
}

#[test]
fn local_layers_composite_in_persisted_order() {
    let definitions = vec![
        mask_definition("first", MaskStatus::Valid, MaskOperation::Source, vec![]),
        mask_definition("second", MaskStatus::Valid, MaskOperation::Source, vec![]),
    ];
    let first = LocalAdjustments {
        exposure: 1.0,
        ..LocalAdjustments::default()
    };
    let second = LocalAdjustments {
        contrast: -0.5,
        ..LocalAdjustments::default()
    };
    let mut first_layer = local_layer("first-layer", reference("vc", "first"), 1.0);
    first_layer.local_adjustments = Some(first.clone());
    let mut second_layer = local_layer("second-layer", reference("vc", "second"), 0.0);
    second_layer.local_adjustments = Some(second.clone());
    let copies = vec![copy_with(
        "vc",
        definitions,
        vec![first_layer, second_layer],
    )];
    let frame = ImageFrame::new(2, 2, vec![80; 16]).unwrap();
    let planes = BTreeMap::from([
        (
            ("vc".into(), "first".into()),
            MaskPlane::new(2, 2, vec![u16::MAX; 4]).unwrap(),
        ),
        (
            ("vc".into(), "second".into()),
            MaskPlane::new(2, 2, vec![u16::MAX; 4]).unwrap(),
        ),
    ]);
    let output = local_render(&frame, &copies, planes, &EditRecipe::default(), None).unwrap();
    let mut expected = frame;
    expected.apply_recipe(&first.as_recipe()).unwrap();
    expected.apply_recipe(&second.as_recipe()).unwrap();
    assert_eq!(output.frame, expected);
}

#[test]
fn local_neutral_object_is_byte_identity() {
    let mut definition =
        mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    definition.geometry_context.height = 2;
    let mut local = layer("layer-1", reference("vc", "subject"));
    local.local_adjustments = Some(LocalAdjustments::default());
    let copies = vec![copy_with("vc", vec![definition], vec![local])];
    let frame = ImageFrame::new(2, 2, vec![37; 16]).unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(2, 2, vec![1234; 4]).unwrap(),
    )]);
    let output = local_render(&frame, &copies, planes, &EditRecipe::default(), None).unwrap();
    assert_eq!(output.frame, frame);
}

#[test]
fn local_alignment_covers_roi_crop_aspect_quarter_turn_and_mirror() {
    let mut definition =
        mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    definition.geometry_context.width = 4;
    definition.geometry_context.height = 2;
    let local = local_layer("layer-1", reference("vc", "subject"), 1.0);
    let copies = vec![copy_with("vc", vec![definition], vec![local])];
    let source_plane = MaskPlane::new(
        4,
        2,
        vec![0, u16::MAX, 0, u16::MAX, 0, u16::MAX, 0, u16::MAX],
    )
    .unwrap();
    let planes = || BTreeMap::from([(("vc".into(), "subject".into()), source_plane.clone())]);
    let frame = ImageFrame::new(4, 2, vec![100; 32]).unwrap();

    let roi_frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
    let roi = local_render(
        &roi_frame,
        &copies,
        planes(),
        &EditRecipe::default(),
        Some([0.25, 0.0, 0.5, 1.0]),
    )
    .unwrap();
    assert_eq!(
        (
            roi.mask_layers[0].plane.width,
            roi.mask_layers[0].plane.height
        ),
        (2, 2)
    );
    assert_eq!(
        roi.mask_layers[0].plane.values,
        vec![u16::MAX, 0, u16::MAX, 0]
    );

    let mut crop_recipe = EditRecipe::default();
    crop_recipe.geometry = Some(lumina_sidecar::Geometry {
        version: 1,
        crop: Some(lumina_sidecar::Crop::Free {
            x: 0.25,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    });
    let cropped = local_render(&frame, &copies, planes(), &crop_recipe, None).unwrap();
    assert_eq!(
        cropped.mask_layers[0].plane.values,
        vec![u16::MAX, 0, u16::MAX, 0]
    );

    let mut aspect_recipe = EditRecipe::default();
    aspect_recipe.geometry = Some(lumina_sidecar::Geometry {
        version: 1,
        crop: Some(lumina_sidecar::Crop::Aspect {
            preset: lumina_sidecar::AspectPreset::OneToOne,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    });
    let aspect = local_render(&frame, &copies, planes(), &aspect_recipe, None).unwrap();
    assert_eq!(
        (
            aspect.mask_layers[0].plane.width,
            aspect.mask_layers[0].plane.height
        ),
        (2, 2)
    );
    // Aspect 1:1 on a 4x2 frame is the centered 2x2 rectangle (columns 1..3):
    // the matte is cropped, never rescaled onto the new size.
    assert_eq!(
        aspect.mask_layers[0].plane.values,
        vec![u16::MAX, 0, u16::MAX, 0]
    );

    let mut rotated_recipe = EditRecipe::default();
    rotated_recipe.geometry = Some(lumina_sidecar::Geometry {
        version: 1,
        crop: None,
        rotation_degrees: 90.0,
        mirror_horizontal: true,
        mirror_vertical: false,
    });
    let rotated = local_render(&frame, &copies, planes(), &rotated_recipe, None).unwrap();
    assert_eq!(
        (
            rotated.mask_layers[0].plane.width,
            rotated.mask_layers[0].plane.height
        ),
        (2, 4)
    );
    // The 90-degree turn halves the width: the matte was transformed, not
    // blindly resized back onto the source aspect.
    assert_ne!(rotated.mask_layers[0].plane.values, source_plane.values);
}

/// A binary matte travels through an RGBA frame so the *core's own* rotate /
/// flip helpers can produce the reference output. `0` and `u16::MAX` survive
/// the `>> 8` / `* 257` round trip bit-exactly, so the comparison stays exact
/// while the reference is literally the geometry the rendered frame receives.
fn plane_as_frame(plane: &MaskPlane) -> ImageFrame {
    let pixels = plane
        .values
        .iter()
        .flat_map(|value| {
            let byte = (value >> 8) as u8;
            [byte, byte, byte, u8::MAX]
        })
        .collect();
    ImageFrame::new(plane.width, plane.height, pixels).expect("plane frame")
}

fn frame_as_plane(frame: &ImageFrame) -> Vec<u16> {
    let (pixels, _) = frame.pixels.as_chunks::<4>();
    pixels
        .iter()
        .map(|pixel| u16::from(pixel[0]) * 257)
        .collect()
}

fn geometry_recipe(degrees: f32, mirror_horizontal: bool, mirror_vertical: bool) -> EditRecipe {
    let mut recipe = EditRecipe::default();
    recipe.geometry = Some(lumina_sidecar::Geometry {
        version: 1,
        crop: None,
        rotation_degrees: degrees,
        mirror_horizontal,
        mirror_vertical,
    });
    recipe
}

/// MASK-LOCAL-P0 alignment golden: for every quarter turn the core accepts
/// (including the negative angles the straighten slider can produce) and for
/// every mirror combination, the aligned matte is *identical* to the matte the
/// frame's own geometry produces — never a blindly resized copy.
#[test]
fn local_alignment_matches_the_core_geometry_for_every_quarter_turn_and_mirror() {
    let mut definition =
        mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    definition.geometry_context.width = 4;
    definition.geometry_context.height = 2;
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![local_layer("layer-1", reference("vc", "subject"), 1.0)],
    )];
    // Deliberately asymmetric, so neither a mirrored nor a rotated variant can
    // accidentally reproduce the identity pattern and hide a wrong transform.
    let source_plane = MaskPlane::new(
        4,
        2,
        vec![0, 0, u16::MAX, 0, u16::MAX, 0, u16::MAX, u16::MAX],
    )
    .unwrap();
    let planes = || BTreeMap::from([(("vc".into(), "subject".into()), source_plane.clone())]);
    let frame = ImageFrame::new(4, 2, vec![100; 32]).unwrap();

    let mut rendered: Vec<(f32, bool, bool, Vec<u16>)> = Vec::new();
    for (degrees, mirror_horizontal, mirror_vertical) in [
        (0.0, false, false),
        (90.0, false, false),
        (180.0, false, false),
        (-90.0, false, false),
        (-180.0, false, false),
        (90.0, true, false),
        (180.0, true, true),
        (0.0, false, true),
    ] {
        let recipe = geometry_recipe(degrees, mirror_horizontal, mirror_vertical);
        let output = local_render(&frame, &copies, planes(), &recipe, None).unwrap();
        let mut reference = plane_as_frame(&source_plane);
        reference = crate::rotate_frame(&reference, degrees);
        if mirror_horizontal {
            crate::flip_horizontal(&mut reference);
        }
        if mirror_vertical {
            crate::flip_vertical(&mut reference);
        }
        assert_eq!(
            (
                output.mask_layers[0].plane.width,
                output.mask_layers[0].plane.height
            ),
            (reference.width, reference.height),
            "plane size for {degrees} deg, mirror_h={mirror_horizontal}, mirror_v={mirror_vertical}"
        );
        assert_eq!(
            output.mask_layers[0].plane.values,
            frame_as_plane(&reference),
            "aligned plane for {degrees} deg, mirror_h={mirror_horizontal}, mirror_v={mirror_vertical}"
        );
        rendered.push((
            degrees,
            mirror_horizontal,
            mirror_vertical,
            output.mask_layers[0].plane.values.clone(),
        ));
    }

    let value_of = |degrees: f32, mirror_h: bool, mirror_v: bool| {
        rendered
            .iter()
            .find(|(d, h, v, _)| *d == degrees && *h == mirror_h && *v == mirror_v)
            .map(|(_, _, _, values)| values.clone())
            .expect("covered quarter-turn case")
    };
    // -180 is the same transform as 180, and -90 must be the *opposite* turn
    // from +90. Both would pass with a sign-blind or angle-dropping aligner.
    assert_eq!(
        value_of(-180.0, false, false),
        value_of(180.0, false, false)
    );
    assert_ne!(value_of(-90.0, false, false), value_of(90.0, false, false));
    // The mirrors are real transforms on this pattern, not no-ops.
    assert_ne!(value_of(0.0, false, true), value_of(0.0, false, false));
    assert_ne!(value_of(90.0, true, false), value_of(90.0, false, false));
}

/// A 45-degree rotation has no defined pixel-domain mapping for a local matte
/// yet, so it is refused loudly *before* any global pixel is touched. The
/// original (pre-existing) 45-degree render is a bilinear rotation, so this
/// also pins that the local path never takes that approximation silently.
#[test]
fn non_quarter_turn_rotation_is_refused_loudly() {
    let mut definition =
        mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    definition.geometry_context.height = 2;
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![local_layer("layer-1", reference("vc", "subject"), 1.0)],
    )];
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(2, 2, vec![u16::MAX; 4]).unwrap(),
    )]);
    let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
    let error = local_render(
        &frame,
        &copies,
        planes,
        &geometry_recipe(45.0, false, false),
        None,
    )
    .unwrap_err();
    assert!(
        matches!(error, CoreError::LocalAdjustmentUnsupported { .. }),
        "unexpected error: {error}"
    );
}

#[test]
fn unsupported_local_geometry_is_refused_before_global_mutation() {
    let definition = mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![local_layer("layer-1", reference("vc", "subject"), 1.0)],
    )];
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(2, 1, vec![u16::MAX; 2]).unwrap(),
    )]);
    let mut recipe = EditRecipe::default();
    recipe.lens_correction = Some(lumina_sidecar::LensCorrection {
        version: 1,
        profile: None,
        distortion_k1: None,
        distortion_k2: None,
        distortion_k3: None,
        vignette_c0: None,
        vignette_c1: None,
        vignette_c2: None,
        ca_red: None,
        ca_blue: None,
    });
    let frame = ImageFrame::new(2, 1, vec![100; 8]).unwrap();
    let error = local_render(&frame, &copies, planes, &recipe, None).unwrap_err();
    assert!(matches!(
        error,
        CoreError::LocalAdjustmentUnsupported { .. }
    ));
}
