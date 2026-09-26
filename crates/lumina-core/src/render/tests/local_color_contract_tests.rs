//! MASK-LOCAL-P1.2b CPU compositor *contract* goldens for the local colour
//! block: the loud preflight refusals, the disabled-stage refusals, and the
//! promise that a local colour edit never mutates the global recipe.
//!
//! Split from `local_color_tests.rs` (file-size ratchet): that half holds the
//! exact pixel goldens, this half holds the invariants around them.

use super::local_adjustments::local_render;
use super::*;
use lumina_sidecar::{
    ColorGrading, ColorGradingRange, LocalAdjustments, PointColor, PointColorEntry,
};

/// A layer whose only local control is a colour block.
fn color_layer(recipe: LocalAdjustments) -> MaskLayer {
    let mut layer = layer("layer-1", reference("vc", "subject"));
    layer.local_adjustments = Some(recipe);
    layer
}

/// A single-pixel layer with one mask, rendered through the full pipeline.
fn render_single(recipe: &LocalAdjustments, recipe_global: &EditRecipe, pixel: [u8; 4]) -> Vec<u8> {
    let definition = mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![color_layer(recipe.clone())],
    )];
    let frame = ImageFrame::new(1, 1, pixel.to_vec()).unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
    )]);
    local_render(&frame, &copies, planes, recipe_global, None)
        .unwrap()
        .frame
        .pixels
}

/// A valid colour-only layer used as the "must not change" baseline.
fn baseline() -> LocalAdjustments {
    let mut recipe = LocalAdjustments::default();
    recipe
        .set_local_hsl_band("red", "hue", 0.25)
        .expect("red hue");
    recipe
}

/// Invalid local colour values are a loud preflight error: the render is
/// refused and a hand-constructed invalid object is never silently clipped or
/// dropped.
#[test]
fn invalid_local_color_values_are_a_loud_preflight_error() {
    let mut cases: Vec<(&str, LocalAdjustments)> = Vec::new();

    // Assign the fields directly: the public setters refuse these values, so a
    // hand-constructed object is the only way to reach the validator.
    let mut out_of_range = baseline();
    out_of_range.color_grading = Some(ColorGrading {
        shadows: ColorGradingRange {
            hue_degrees: 400.0,
            saturation: 0.5,
            luminance: 0.0,
        },
        midtones: ColorGradingRange::neutral(),
        highlights: ColorGradingRange::neutral(),
        balance: 0.0,
        blending: 0.5,
        version: 1,
    });
    cases.push(("grading hue out of range", out_of_range));

    let mut wrong_hsl_version = baseline();
    wrong_hsl_version
        .hsl
        .as_mut()
        .expect("baseline has an hsl block")
        .version = 2;
    cases.push(("hsl block version", wrong_hsl_version));

    let mut wrong_point_version = baseline();
    wrong_point_version.point_color = Some(PointColor {
        version: 3,
        entries: Vec::new(),
    });
    cases.push(("point color block version", wrong_point_version));

    let mut duplicate_ids = baseline();
    duplicate_ids.point_color = Some(PointColor {
        version: 1,
        entries: vec![
            PointColorEntry {
                id: "pc-1".into(),
                hue_center: 30.0,
                hue_range: 45.0,
                hue_shift: 0.2,
                saturation_shift: 0.0,
                luminance_shift: 0.0,
            },
            PointColorEntry {
                id: "pc-1".into(),
                hue_center: 200.0,
                hue_range: 45.0,
                hue_shift: 0.2,
                saturation_shift: 0.0,
                luminance_shift: 0.0,
            },
        ],
    });
    cases.push(("duplicate point color ids", duplicate_ids));

    let mut empty_id = baseline();
    empty_id.point_color = Some(PointColor {
        version: 1,
        entries: vec![PointColorEntry {
            id: String::new(),
            hue_center: 30.0,
            hue_range: 45.0,
            hue_shift: 0.2,
            saturation_shift: 0.0,
            luminance_shift: 0.0,
        }],
    });
    cases.push(("empty point color id", empty_id));

    let mut too_many = baseline();
    too_many.point_color = Some(PointColor {
        version: 1,
        entries: (0..9)
            .map(|index| PointColorEntry {
                id: format!("pc-{index}"),
                hue_center: 30.0,
                hue_range: 45.0,
                hue_shift: 0.2,
                saturation_shift: 0.0,
                luminance_shift: 0.0,
            })
            .collect(),
    });
    cases.push(("too many point color entries", too_many));

    let mut nan = baseline();
    nan.vibrance = f64::NAN;
    cases.push(("non-finite local vibrance", nan));

    let mut out_of_range_scalar = baseline();
    out_of_range_scalar.saturation = 1.5;
    cases.push(("local saturation out of range", out_of_range_scalar));

    let mut grading_range = baseline();
    grading_range.color_grading = Some(ColorGrading {
        shadows: ColorGradingRange {
            hue_degrees: 200.0,
            saturation: 1.5,
            luminance: 0.0,
        },
        midtones: ColorGradingRange::neutral(),
        highlights: ColorGradingRange::neutral(),
        balance: 0.0,
        blending: 0.5,
        version: 1,
    });
    cases.push(("grading saturation out of range", grading_range));

    assert!(!cases.is_empty());
    for (name, recipe) in cases {
        let definition =
            mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
        let copies = vec![copy_with(
            "vc",
            vec![definition],
            vec![color_layer(recipe.clone())],
        )];
        let frame = ImageFrame::new(1, 1, vec![100, 100, 100, 255]).unwrap();
        let planes = BTreeMap::from([(
            ("vc".into(), "subject".into()),
            MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
        )]);
        let error = local_render(&frame, &copies, planes, &EditRecipe::default(), None)
            .expect_err("an invalid local colour block must be a loud preflight error");
        let message = error.to_string();
        assert!(message.contains("local"), "{name}: {message}");
    }
}

/// A colour-only local layer never changes the global recipe, no matter which
/// colour area is edited, and the kernel never reads a global colour field.
#[test]
fn a_local_color_edit_never_touches_the_global_recipe() {
    let pixel = [200, 90, 40, 55];
    // A global recipe that *does* carry colour must not influence the local
    // colour block either: the local stages read only the layer.
    let mut global = EditRecipe::default();
    global.adjustments.insert("saturation".into(), -0.9);
    global.hsl = Some(lumina_sidecar::HslAdjustments {
        version: 1,
        red: Some(lumina_sidecar::HslChannel {
            hue: 0.9,
            saturation: 0.9,
            luminance: -0.5,
        }),
        ..lumina_sidecar::HslAdjustments::default()
    });
    global.color_grading = Some(ColorGrading {
        shadows: ColorGradingRange {
            hue_degrees: 0.0,
            saturation: 0.9,
            luminance: 0.0,
        },
        ..ColorGrading::neutral()
    });

    // A local vibrance that matches the global saturation value is still a
    // *local* edit: it applies on top of the global result, once, in the local
    // chain — and it is not the global key.
    let mut local = baseline();
    local.set_value("vibrance", 0.35).expect("vibrance");
    let combined = render_single(&local, &global, pixel);

    let mut expected_frame = ImageFrame::new(1, 1, pixel.to_vec()).unwrap();
    expected_frame.apply_recipe(&global).unwrap();
    let mut local_only = baseline();
    local_only.set_value("vibrance", 0.35).expect("vibrance");
    expected_frame.apply_mask_local_recipe(&local_only).unwrap();
    assert_eq!(
        combined, expected_frame.pixels,
        "the local colour block must run on the global result, once, in local order"
    );
}

/// The still-disabled stages must stay unreachable from a local layer: no
/// detail, AI-denoise, noise reduction, sharpening or optics field exists, and
/// the local renderer must not fake one.
///
/// Local presence is deliberately **not** in this list any more: MASK-LOCAL-P1.2c
/// added it as a typed block with its own `presence.<field>` setters. What stays
/// true is that the three presence *amounts* are not scalar keys and that the
/// *default* recipe does not serialize the block at all.
#[test]
fn disabled_local_stages_stay_unreachable() {
    let json = serde_json::to_value(LocalAdjustments::default()).expect("serializable");
    let keys: Vec<String> = json
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    for disabled in [
        "detail",
        "noise_reduction",
        "denoise_ai",
        "sharpening",
        "optics",
        "lens_correction",
    ] {
        assert!(
            !keys.iter().any(|key| key == disabled),
            "the local recipe must not carry a `{disabled}` field"
        );
        let mut recipe = LocalAdjustments::default();
        assert!(
            recipe.set_value(disabled, 0.1).is_err(),
            "`{disabled}` must not be a local adjustment key"
        );
    }
    // The presence block is absent by default and is never a scalar key: it is
    // only reachable through the typed `presence.<field>` setters.
    assert!(
        !keys.iter().any(|key| key == "presence"),
        "a never-edited layer must not persist a presence block"
    );
    for amount in ["texture", "clarity", "dehaze", "presence"] {
        let mut recipe = LocalAdjustments::default();
        assert!(
            recipe.set_value(amount, 0.1).is_err(),
            "`{amount}` must not be a scalar local adjustment key"
        );
        assert!(
            recipe.set_local_presence_field(amount, 0.1).is_ok() || amount == "presence",
            "`presence.{amount}` must be a typed presence field"
        );
    }
    // And a colour-only layer is still refused by the stand-in routes, i.e.
    // the routing predicate is driven by `is_neutral`, not by a colour
    // special case.
    let mut recipe = LocalAdjustments::default();
    assert!(recipe.is_neutral());
    recipe.set_value("vibrance", 0.1).expect("vibrance");
    assert!(!recipe.is_neutral());
    recipe.reset_local_color();
    assert!(recipe.is_neutral());
    assert!(!recipe.has_local_color());
}

/// The whole colour block survives a JSON round trip byte for byte, including
/// the *stored form* of a neutral block (which is neutral but not identical to
/// "absent").
#[test]
fn local_color_json_round_trip_is_byte_stable() {
    let mut recipe = baseline();
    recipe.set_value("vibrance", 0.4).expect("vibrance");
    recipe.set_value("saturation", -0.2).expect("saturation");
    recipe
        .add_local_point_color_entry(120.0, 60.0, 0.1, -0.2, 0.3)
        .expect("point colour");
    recipe
        .set_local_color_grading_field("highlights", "luminance", 0.2)
        .expect("highlight luminance");
    let json = serde_json::to_string(&recipe).expect("serializable");
    let parsed: LocalAdjustments = serde_json::from_str(&json).expect("deserializable");
    assert_eq!(parsed, recipe);
    assert_eq!(serde_json::to_string(&parsed).unwrap(), json);
}

/// The whole local chain owns **exactly one** RGBA8 quantization.
///
/// The test re-derives the WB + Basic prefix from the documented arithmetic in
/// `f64` (independently of the production helper) and then runs the shared
/// per-pixel colour stages on that float. It proves two things at once:
///
/// 1. the render equals the fully-float chain rounded once, and
/// 2. that result *differs* from a chain that rounds between the Basic and the
///    colour stage — so "one boundary" is an observable property, not a comment.
#[test]
fn local_wb_basic_tone_and_color_share_exactly_one_quantization_boundary() {
    // Relative WB gains, derived from the delta alone: warmth = 2000/5500.
    let recipe = LocalAdjustments {
        exposure: 0.5,
        contrast: 0.25,
        shadows: 0.5,
        highlights: -0.5,
        temperature_delta_k: 2000.0,
        tint_delta: -0.2,
        saturation: -0.5,
        ..LocalAdjustments::default()
    };
    let pixel = [100u8, 120, 140, 77];

    // Independent WB + Basic arithmetic, straight from the documented formula.
    let warmth = recipe.temperature_delta_k / 5500.0;
    let gains = [
        1.0 - warmth * 0.35,
        1.0 - recipe.tint_delta * 0.20,
        1.0 + warmth * 0.35,
    ];
    let prefix = |value: f64, channel: usize| {
        let exposure_multiplier = 2.0_f64.powf(recipe.exposure);
        let contrast_factor = 1.0 + recipe.contrast;
        let mut value = value * gains[channel];
        value = (value * exposure_multiplier).clamp(0.0, 255.0);
        value = ((value - 128.0) * contrast_factor + 128.0).clamp(0.0, 255.0);
        let x = value / 255.0;
        let shadow_weight = ((0.5 - x) / 0.5).max(0.0).powi(2);
        value = (x + recipe.shadows * shadow_weight * 0.25).clamp(0.0, 1.0) * 255.0;
        let x = value / 255.0;
        let highlight_weight = ((x - 0.5) / 0.5).max(0.0).powi(2);
        (x + recipe.highlights * highlight_weight * 0.25).clamp(0.0, 1.0) * 255.0
    };

    // (1) One boundary: WB + Basic and the colour stage share one rounding.
    let mut rgb = [0.0_f32; 3];
    for (channel, slot) in rgb.iter_mut().enumerate() {
        *slot = (prefix(f64::from(pixel[channel]), channel) / 255.0) as f32;
    }
    let staged = crate::color_stages::vibrance_saturation_stage(rgb, 0.0, recipe.saturation as f32);
    let mut expected: Vec<u8> = staged
        .iter()
        .map(|value| (f64::from(*value) * 255.0).round().clamp(0.0, 255.0) as u8)
        .collect();
    expected.push(pixel[3]);
    assert_eq!(
        render_single(&recipe, &EditRecipe::default(), pixel),
        expected,
        "the local chain must round exactly once, at the end"
    );

    // (2) A chain that rounds between the Basic and the colour stage is
    // observably different — otherwise the property above would be vacuous.
    let mut rgb = [0.0_f32; 3];
    for (channel, slot) in rgb.iter_mut().enumerate() {
        let rounded = prefix(f64::from(pixel[channel]), channel)
            .round()
            .clamp(0.0, 255.0) as u8;
        *slot = rounded as f32 / 255.0;
    }
    let staged = crate::color_stages::vibrance_saturation_stage(rgb, 0.0, recipe.saturation as f32);
    let early_round_result: Vec<u8> = staged
        .iter()
        .map(|value| (f64::from(*value) * 255.0).round().clamp(0.0, 255.0) as u8)
        .collect();
    assert_ne!(
        early_round_result, expected,
        "an intermediate quantization must be observable, or the single boundary is untested"
    );
}
