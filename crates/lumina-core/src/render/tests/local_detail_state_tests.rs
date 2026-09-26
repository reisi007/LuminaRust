//! MASK-LOCAL-P1.2d CPU compositor *state and identity* tests for the local
//! detail block.
//!
//! Split from `local_detail_tests.rs` (file-size ratchet): this half owns the
//! refusals, the resets, the JSON/identity contracts and the alpha endpoints,
//! while the other half owns the pixel goldens. Both halves share the same
//! helpers, which live in the golden half and are re-exported here through
//! `super::local_detail`.
//!
//! There deliberately is **no** test in either half claiming byte-equality
//! between the local detail path and the global detail stages: the two quantize
//! at different points on purpose, so that equality is false by design.

use super::local_adjustments::local_render;
use super::local_detail::{
    detail_layer, lifted_master, noise_only, red_hsl, render_row, sharpening_only, step_row,
};
use super::local_detail_goldens::{
    GOLDEN_DETAIL_AFTER_COLOUR, GOLDEN_HALF_MASK, GOLDEN_OVERLAP_NOISE_FIRST,
    GOLDEN_OVERLAP_SHARPEN_FIRST, GOLDEN_SHARPENING_AMOUNT, GOLDEN_TRANSPARENT_HALF_MASK,
};
use super::*;
use lumina_sidecar::{
    CurveChannels, Curves, Detail, LocalAdjustments, NoiseReduction, Presence, Sharpening,
};

/// A per-area reset removes exactly one sub-block; the whole-block reset removes
/// both; and a sub-block that returns to neutral removes itself.
#[test]
fn detail_resets_work_per_sub_block_and_for_the_whole_block() {
    let mut recipe = LocalAdjustments::default();
    recipe.set_local_sharpening_field("amount", 0.75).unwrap();
    recipe.set_local_sharpening_field("radius", 2.0).unwrap();
    recipe
        .set_local_noise_reduction_field("luminance", 0.4)
        .unwrap();
    assert!(recipe.has_local_sharpening());
    assert!(recipe.has_local_noise_reduction());
    assert_eq!(recipe.detail_summary(), "sharpening+noise_reduction");

    // Writing the amount back to zero drops the sharpening sub-block, and only
    // that one.
    recipe.set_local_sharpening_field("amount", 0.0).unwrap();
    assert!(!recipe.has_local_sharpening());
    assert!(recipe.has_local_noise_reduction());
    assert_eq!(recipe.detail_summary(), "noise_reduction");
    assert!(recipe.detail.as_ref().unwrap().sharpening.is_none());
    assert!(recipe.detail.as_ref().unwrap().noise_reduction.is_some());

    // Re-create the sharpening sub-block and use the per-area reset instead.
    recipe.set_local_sharpening_field("amount", 1.0).unwrap();
    recipe.reset_local_detail_field("noise_reduction").unwrap();
    assert!(recipe.has_local_sharpening());
    assert!(!recipe.has_local_noise_reduction());
    assert_eq!(recipe.detail_summary(), "sharpening");
    assert!(recipe
        .reset_local_detail_field("optics")
        .unwrap_err()
        .contains("unknown local detail field"));
    assert!(recipe
        .set_local_sharpening_field("luminance", 0.5)
        .unwrap_err()
        .contains("unknown local sharpening field"));
    assert!(recipe
        .set_local_noise_reduction_field("amount", 0.5)
        .unwrap_err()
        .contains("unknown local noise reduction field"));

    // The whole-block reset drops everything.
    recipe.reset_local_detail();
    assert!(recipe.detail.is_none());
    assert!(recipe.is_neutral());
    assert_eq!(recipe.detail_summary(), "none");
    assert!(recipe.to_string().contains("detail=none"));
}

/// The local detail block is part of the local state identity.
#[test]
fn the_detail_block_is_part_of_the_local_render_identity() {
    let base = LocalAdjustments::default();
    let mut sharpened = LocalAdjustments::default();
    sharpened.set_local_sharpening_field("amount", 1.0).unwrap();
    let mut noised = LocalAdjustments::default();
    noised
        .set_local_noise_reduction_field("luminance", 0.3)
        .unwrap();
    assert_ne!(base.digest(), sharpened.digest());
    assert_ne!(base.digest(), noised.digest());
    assert_ne!(sharpened.digest(), noised.digest());
}

/// Two overlapping local detail layers use the persisted list order.
#[test]
fn two_overlapping_local_detail_layers_use_persisted_order() {
    let definitions: Vec<MaskDefinition> = ["subject", "sky"]
        .iter()
        .map(|name| {
            let mut definition =
                mask_definition(name, MaskStatus::Valid, MaskOperation::Source, vec![]);
            definition.geometry_context.width = 5;
            definition.geometry_context.height = 1;
            definition
        })
        .collect();
    let layer_of = |id: &str, name: &str, recipe: LocalAdjustments| {
        let mut layer = layer(id, reference("vc", name));
        layer.local_adjustments = Some(recipe);
        layer
    };
    let sharpening = sharpening_only(1.0, 2.0, 1.0, 0.0);
    let noise = noise_only(0.5, 0.0);
    let row = vec![
        20, 20, 20, 255, 60, 60, 60, 255, 120, 120, 120, 255, 200, 200, 200, 255, 240, 240, 240,
        255,
    ];
    let frame = ImageFrame::new(5, 1, row).unwrap();
    let render = |layers: Vec<MaskLayer>| {
        let copy = copy_with("vc", definitions.clone(), layers);
        let planes = BTreeMap::from([
            (
                ("vc".into(), "subject".into()),
                MaskPlane::new(5, 1, vec![u16::MAX; 5]).unwrap(),
            ),
            (
                ("vc".into(), "sky".into()),
                MaskPlane::new(5, 1, vec![u16::MAX; 5]).unwrap(),
            ),
        ]);
        local_render(&frame, &[copy], planes, &EditRecipe::default(), None)
            .unwrap()
            .frame
            .pixels
    };
    let sharpen_first = render(vec![
        layer_of("layer-1", "subject", sharpening.clone()),
        layer_of("layer-2", "sky", noise.clone()),
    ]);
    let noise_first = render(vec![
        layer_of("layer-1", "subject", noise),
        layer_of("layer-2", "sky", sharpening),
    ]);
    assert_eq!(sharpen_first, GOLDEN_OVERLAP_SHARPEN_FIRST);
    assert_eq!(noise_first, GOLDEN_OVERLAP_NOISE_FIRST);
    assert_ne!(
        sharpen_first, noise_first,
        "reordering overlapping local layers must change the render"
    );
}

/// The local detail block sits **after** the colour block. Two overlapping
/// layers — the outer one carrying only colour, the inner one only detail — put
/// the two stages in a fixed relative order, and swapping the persisted layer
/// order swaps the stage order. The documented order is pinned by a golden, and
/// the fact that the two orders differ proves the stages do not commute.
#[test]
fn local_detail_sits_after_the_colour_block() {
    let definitions: Vec<MaskDefinition> = ["subject", "sky"]
        .iter()
        .map(|name| {
            let mut definition =
                mask_definition(name, MaskStatus::Valid, MaskOperation::Source, vec![]);
            definition.geometry_context.width = 5;
            definition.geometry_context.height = 1;
            definition
        })
        .collect();
    let layer_of = |id: &str, name: &str, recipe: LocalAdjustments| {
        let mut layer = layer(id, reference("vc", name));
        layer.local_adjustments = Some(recipe);
        layer
    };
    let colour = LocalAdjustments {
        hsl: Some(red_hsl()),
        ..LocalAdjustments::default()
    };
    let detail = sharpening_only(0.75, 2.0, 0.5, 0.0);
    let row = step_row();
    let frame = ImageFrame::new(5, 1, row).unwrap();
    let render = |layers: Vec<MaskLayer>| {
        let copy = copy_with("vc", definitions.clone(), layers);
        let planes = BTreeMap::from([
            (
                ("vc".into(), "subject".into()),
                MaskPlane::new(5, 1, vec![u16::MAX; 5]).unwrap(),
            ),
            (
                ("vc".into(), "sky".into()),
                MaskPlane::new(5, 1, vec![u16::MAX; 5]).unwrap(),
            ),
        ]);
        local_render(&frame, &[copy], planes, &EditRecipe::default(), None)
            .unwrap()
            .frame
            .pixels
    };
    let colour_then_detail = render(vec![
        layer_of("layer-1", "subject", colour.clone()),
        layer_of("layer-2", "sky", detail.clone()),
    ]);
    let detail_then_colour = render(vec![
        layer_of("layer-1", "subject", detail.clone()),
        layer_of("layer-2", "sky", colour.clone()),
    ]);
    assert_eq!(colour_then_detail, GOLDEN_DETAIL_AFTER_COLOUR);
    assert_ne!(
        colour_then_detail, detail_then_colour,
        "the colour and detail stages must not commute"
    );
    // Neither is a no-op on its own.
    assert_ne!(render_row(&colour, step_row(), u16::MAX), step_row());
    assert_ne!(render_row(&detail, step_row(), u16::MAX), step_row());
}

/// Mask alpha is honoured exactly: alpha 0 leaves every byte alone, alpha
/// `u16::MAX` takes the full local recipe, and a partial alpha blends
/// fractionally. The detail stages themselves never see the mask.
#[test]
fn local_detail_alpha_endpoints_and_partial_alpha_are_exact() {
    let row = step_row();
    let full = render_row(&sharpening_only(0.75, 2.0, 0.5, 0.0), row.clone(), u16::MAX);
    assert_eq!(full, GOLDEN_SHARPENING_AMOUNT);
    let none = render_row(&sharpening_only(0.75, 2.0, 0.5, 0.0), row.clone(), 0);
    assert_eq!(none, row, "alpha 0 must be byte-identical to the input");
    let half = render_row(&sharpening_only(0.75, 2.0, 0.5, 0.0), row.clone(), 32768);
    assert_eq!(half, GOLDEN_HALF_MASK);
    // The half-mask result is the documented fractional blend of the two.
    for ((before, after), blended) in row
        .as_chunks::<4>()
        .0
        .iter()
        .zip(full.as_chunks::<4>().0)
        .zip(half.as_chunks::<4>().0)
    {
        for channel in 0..3 {
            let expected = (u32::from(before[channel]) * u32::from(u16::MAX - 32768)
                + u32::from(after[channel]) * 32768
                + u32::from(u16::MAX / 2))
                / u32::from(u16::MAX);
            assert_eq!(
                u32::from(blended[channel]),
                expected,
                "channel {channel} must be the documented fractional blend"
            );
        }
        assert_eq!(blended[3], before[3]);
    }
    // The alpha plane is the *mask*, not the image: a zero-alpha mask over a
    // transparent image pixel still leaves every byte alone.
    let mut transparent = row.clone();
    for pixel in transparent.as_chunks_mut::<4>().0 {
        pixel[3] = 0;
    }
    let transparent_half = render_row(
        &sharpening_only(0.75, 2.0, 0.5, 0.0),
        transparent.clone(),
        32768,
    );
    assert_eq!(transparent_half, GOLDEN_TRANSPARENT_HALF_MASK);
    let transparent_none = render_row(
        &sharpening_only(0.75, 2.0, 0.5, 0.0),
        transparent.clone(),
        0,
    );
    assert_eq!(transparent_none, transparent);
}

/// Without a non-neutral detail block the P0/P1.1/P1.2a/P1.2b/P1.2c bytes stay
/// **exactly** unchanged — an absent block, a persisted all-neutral block and a
/// persisted block with only neutral sub-blocks all take the identical old
/// kernel path.
#[test]
fn no_local_detail_keeps_the_p0_p11_p12a_p12b_p12c_bytes() {
    let row: Vec<u8> = (0..7u8)
        .flat_map(|i| {
            let v = 40 + 20 * i;
            [v, v / 2, 255 - v, 90]
        })
        .collect();
    let mut recipe = LocalAdjustments {
        exposure: 0.4,
        temperature_delta_k: -1200.0,
        vibrance: 0.2,
        presence: Some(Presence {
            version: 1,
            texture: 0.4,
            clarity: 0.2,
            dehaze: 0.1,
        }),
        curves: Some(Curves {
            version: 1,
            master: lifted_master(),
            channels: CurveChannels::default(),
        }),
        hsl: Some(red_hsl()),
        ..LocalAdjustments::default()
    };
    // (a) an absent block,
    assert!(recipe.detail.is_none());
    let absent = render_row(&recipe, row.clone(), u16::MAX);
    // (b) a persisted block with both sub-blocks neutral, and
    recipe.detail = Some(lumina_sidecar::neutral_local_detail());
    assert!(!recipe.has_local_detail());
    assert!(!recipe.has_local_sharpening());
    assert!(!recipe.has_local_noise_reduction());
    let both_neutral = render_row(&recipe, row.clone(), u16::MAX);
    // (c) a persisted block with only non-default but still neutral sub-blocks.
    recipe.detail = Some(Detail {
        sharpening: Some(Sharpening {
            version: 1,
            amount: 0.0,
            radius: 3.0,
            detail: 1.0,
            masking: 1.0,
        }),
        noise_reduction: Some(NoiseReduction {
            version: 1,
            luminance: 0.0,
            color: 0.0,
        }),
    });
    assert!(!recipe.has_local_detail());
    let partly_neutral = render_row(&recipe, row.clone(), u16::MAX);
    assert_eq!(
        absent, both_neutral,
        "a neutral detail block must change no byte"
    );
    assert_eq!(absent, partly_neutral);
    assert_ne!(absent, row, "the P1.2b/P1.2c path must still do its work");
    // And the same holds without any other local block: a *detail-only* neutral
    // layer is byte-identical to a layer with no local state at all.
    let untouched = render_row(&LocalAdjustments::default(), row.clone(), u16::MAX);
    assert_eq!(untouched, row);
}

/// Out-of-range local detail values are a loud preflight error that changes no
/// byte, including at the two radius boundaries and for non-finite values.
#[test]
fn invalid_local_detail_values_are_a_loud_preflight_error() {
    let mut cases: Vec<(&str, LocalAdjustments)> = Vec::new();
    for (name, recipe) in [
        ("amount above range", sharpening_only(3.5, 2.0, 0.5, 0.0)),
        ("amount below range", sharpening_only(-0.5, 2.0, 0.5, 0.0)),
        (
            "radius below the 0.1 boundary",
            sharpening_only(0.75, 0.05, 0.5, 0.0),
        ),
        (
            "radius above the 10.0 boundary",
            sharpening_only(0.75, 10.5, 0.5, 0.0),
        ),
        ("detail above range", sharpening_only(0.75, 2.0, 1.5, 0.0)),
        ("masking above range", sharpening_only(0.75, 2.0, 0.5, 1.5)),
        (
            "non-finite amount",
            sharpening_only(f32::NAN, 2.0, 0.5, 0.0),
        ),
        (
            "infinite radius",
            sharpening_only(0.75, f32::INFINITY, 0.5, 0.0),
        ),
        ("luminance above range", noise_only(1.5, 0.0)),
        ("colour below range", noise_only(0.0, -0.5)),
        ("non-finite luminance", noise_only(f32::NAN, 0.0)),
        ("infinite colour", noise_only(0.0, f32::INFINITY)),
        (
            "sharpening block version",
            LocalAdjustments {
                detail: Some(Detail {
                    sharpening: Some(Sharpening {
                        version: 2,
                        amount: 1.5,
                        radius: 2.0,
                        detail: 0.5,
                        masking: 0.0,
                    }),
                    noise_reduction: None,
                }),
                ..LocalAdjustments::default()
            },
        ),
        (
            "noise reduction block version",
            LocalAdjustments {
                detail: Some(Detail {
                    sharpening: None,
                    noise_reduction: Some(NoiseReduction {
                        version: 3,
                        luminance: 0.5,
                        color: 0.0,
                    }),
                }),
                ..LocalAdjustments::default()
            },
        ),
    ] {
        assert!(recipe.validate().is_err(), "{name} must be refused");
        cases.push((name, recipe));
    }
    assert!(!cases.is_empty());
    for (name, recipe) in cases {
        let mut definition =
            mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
        definition.geometry_context.width = 5;
        definition.geometry_context.height = 1;
        let copies = vec![copy_with(
            "vc",
            vec![definition],
            vec![detail_layer(recipe)],
        )];
        let before = step_row();
        let frame = ImageFrame::new(5, 1, before.clone()).unwrap();
        let planes = BTreeMap::from([(
            ("vc".into(), "subject".into()),
            MaskPlane::new(5, 1, vec![u16::MAX; 5]).unwrap(),
        )]);
        let error = local_render(&frame, &copies, planes, &EditRecipe::default(), None)
            .expect_err("an invalid local detail block must be a loud preflight error");
        let message = error.to_string();
        assert!(message.contains("local"), "{name}: {message}");
        // The input frame the caller owns is untouched by the refusal.
        assert_eq!(frame.pixels, before, "{name} must change no byte");
    }
    // The public setters refuse the same values, before they mutate.
    let mut recipe = LocalAdjustments::default();
    let untouched = serde_json::to_string(&recipe).expect("serializable");
    for (field, value) in [
        ("amount", 3.5),
        ("radius", 0.05),
        ("radius", 10.5),
        ("detail", 1.5),
        ("masking", -0.5),
        ("amount", f64::NAN),
    ] {
        let error = recipe
            .set_local_sharpening_field(field, value)
            .expect_err("an out-of-range or non-finite value must be refused");
        assert!(error.contains(field), "{error}");
        assert_eq!(
            serde_json::to_string(&recipe).expect("serializable"),
            untouched,
            "a refused sharpening edit must not mutate the layer"
        );
    }
    for (field, value) in [
        ("luminance", 1.5),
        ("color", -0.5),
        ("luminance", f64::INFINITY),
    ] {
        let error = recipe
            .set_local_noise_reduction_field(field, value)
            .expect_err("an out-of-range or non-finite value must be refused");
        assert!(error.contains(field), "{error}");
        assert_eq!(
            serde_json::to_string(&recipe).expect("serializable"),
            untouched,
            "a refused noise-reduction edit must not mutate the layer"
        );
    }
    // The two radius boundaries are legal, and the values just outside them are
    // not — pinned explicitly so the range can never silently widen or narrow.
    assert!(sharpening_only(0.75, 0.1, 0.5, 0.0).validate().is_ok());
    assert!(sharpening_only(0.75, 10.0, 0.5, 0.0).validate().is_ok());
    assert_eq!(lumina_sidecar::sharpening_radius_range(), (0.1, 10.0));
    assert_eq!(lumina_sidecar::SHARPENING_FIELDS.len(), 4);
    assert_eq!(lumina_sidecar::NOISE_REDUCTION_FIELDS.len(), 2);
    assert_eq!(
        lumina_sidecar::sharpening_field_range("amount"),
        Some((0.0, 3.0))
    );
    assert_eq!(
        lumina_sidecar::noise_reduction_field_range("luminance"),
        Some((0.0, 1.0))
    );
    assert_eq!(lumina_sidecar::sharpening_field_range("luminance"), None);
}
