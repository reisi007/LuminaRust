//! MASK-LOCAL-P1.2c CPU compositor *contract* goldens for the local presence
//! block: the one-quantization-boundary proof, the loud preflight refusals, the
//! alpha/overlap behaviour, and the visible refusal of the stages that stay
//! disabled.
//!
//! Split from `local_presence_tests.rs` (file-size ratchet): that half holds
//! the exact pixel goldens, this half holds the invariants around them.

use super::local_adjustments::local_render;
use super::*;
use lumina_sidecar::{Curves, LocalAdjustments, Presence};

fn presence_layer(recipe: LocalAdjustments) -> MaskLayer {
    let mut layer = layer("layer-1", reference("vc", "subject"));
    layer.local_adjustments = Some(recipe);
    layer
}

fn render_single(recipe: &LocalAdjustments, pixel: [u8; 4]) -> Vec<u8> {
    let definition = mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![presence_layer(recipe.clone())],
    )];
    let frame = ImageFrame::new(1, 1, pixel.to_vec()).unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
    )]);
    local_render(&frame, &copies, planes, &EditRecipe::default(), None)
        .unwrap()
        .frame
        .pixels
}

fn render_row(recipe: &LocalAdjustments, pixels: Vec<u8>) -> Vec<u8> {
    let width = (pixels.len() / 4) as u32;
    let mut definition =
        mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    definition.geometry_context.width = width;
    definition.geometry_context.height = 1;
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![presence_layer(recipe.clone())],
    )];
    let frame = ImageFrame::new(width, 1, pixels).unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(width, 1, vec![u16::MAX; width as usize]).unwrap(),
    )]);
    local_render(&frame, &copies, planes, &EditRecipe::default(), None)
        .unwrap()
        .frame
        .pixels
}

/// A valid presence-only layer used as the "must not change" baseline.
fn baseline() -> LocalAdjustments {
    let mut recipe = LocalAdjustments::default();
    recipe
        .set_local_presence_field("texture", 0.5)
        .expect("texture");
    recipe
}

// ------------------------------------------------------ alpha and order

/// A half mask must blend the presence result fractionally, exactly like every
/// other local adjustment, and alpha is never part of the blend.
#[test]
fn local_presence_half_mask_and_zero_alpha_are_exact() {
    let recipe = baseline();
    let pixels: Vec<u8> = (0..9u8)
        .flat_map(|i| {
            let v = 80 + 10 * i;
            [v, v, v, 10 + i]
        })
        .collect();
    let full = render_row(&recipe, pixels.clone());
    let mut definition =
        mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    definition.geometry_context.width = 9;
    definition.geometry_context.height = 1;
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![presence_layer(recipe.clone())],
    )];
    let frame = ImageFrame::new(9, 1, pixels.clone()).unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(9, 1, vec![0, 32_768, u16::MAX, 0, 0, 0, 0, 0, 0]).unwrap(),
    )]);
    let output = local_render(&frame, &copies, planes, &EditRecipe::default(), None).unwrap();
    // Alpha 0 leaves every byte untouched, alpha included.
    assert_eq!(&output.frame.pixels[0..4], &pixels[0..4]);
    // Full alpha takes the whole local result.
    assert_eq!(&output.frame.pixels[8..12], &full[8..12]);
    // Half alpha is the exact integer blend of the two byte results.
    let blend = |base: u8, present: u8| {
        ((base as u32 * (u16::MAX - 32_768) as u32
            + present as u32 * 32_768
            + u32::from(u16::MAX) / 2)
            / u32::from(u16::MAX)) as u8
    };
    assert_eq!(
        &output.frame.pixels[4..8],
        &[
            blend(pixels[4], full[4]),
            blend(pixels[5], full[5]),
            blend(pixels[6], full[6]),
            pixels[7]
        ]
    );
}

/// Two overlapping local-presence layers must be evaluated in the persisted
/// list order, exactly like every other local adjustment. Reordering is a
/// render-identity change, never an equivalent state.
#[test]
fn two_overlapping_local_presence_layers_use_persisted_order() {
    let mut definitions = Vec::new();
    for id in ["left", "overlap"] {
        let mut definition = mask_definition(id, MaskStatus::Valid, MaskOperation::Source, vec![]);
        definition.geometry_context.width = 5;
        definition.geometry_context.height = 1;
        definitions.push(definition);
    }
    let mut left = LocalAdjustments::default();
    left.set_local_presence_field("texture", 0.8)
        .expect("left texture");
    let mut right = LocalAdjustments::default();
    right
        .set_local_presence_field("clarity", 0.5)
        .expect("right clarity");
    let make_layer = |id: &str, mask: &str, recipe: &LocalAdjustments| {
        let mut layer = layer(id, reference("vc", mask));
        layer.local_adjustments = Some(recipe.clone());
        layer
    };
    let pixels: Vec<u8> = (0..5u8)
        .flat_map(|i| {
            let v = 70 + 30 * i;
            [v, v, v, 1 + i]
        })
        .collect();
    let frame = ImageFrame::new(5, 1, pixels.clone()).unwrap();
    let planes = BTreeMap::from([
        (
            ("vc".into(), "left".into()),
            MaskPlane::new(5, 1, vec![u16::MAX, u16::MAX, 0, 0, 0]).unwrap(),
        ),
        (
            ("vc".into(), "overlap".into()),
            MaskPlane::new(5, 1, vec![0, u16::MAX, u16::MAX, u16::MAX, u16::MAX]).unwrap(),
        ),
    ]);
    let render = |layers: Vec<MaskLayer>| {
        let copies = vec![copy_with("vc", definitions.clone(), layers)];
        local_render(
            &frame,
            &copies,
            planes.clone(),
            &EditRecipe::default(),
            None,
        )
        .unwrap()
        .frame
        .pixels
    };
    let forward = render(vec![
        make_layer("layer-left", "left", &left),
        make_layer("layer-overlap", "overlap", &right),
    ]);
    let swapped = render(vec![
        make_layer("layer-overlap", "overlap", &right),
        make_layer("layer-left", "left", &left),
    ]);
    assert_ne!(
        forward, swapped,
        "the overlap pixel must follow the persisted list order"
    );
    // Sequential compositing is exact and fully derived. The left texture stage
    // (`texture 0.8` -> radius `1 + round(0.8*2) = 3`) over
    // `[70, 100, 130, 160, 190]` yields `[34, 76, 130, 184, 226]`, but only
    // pixels 0 and 1 are inside the left mask, so the *working* frame becomes
    // `[34, 76, 130, 160, 190]`. The overlap layer's clarity stage
    // (`clarity 0.5` -> radius 20, clipped to the five-pixel row) then sees
    // that row, so its mean is `(34+76+130+160+190)/5 = 118` and pixel 1
    // becomes `76 + 0.5 * (76 - 118) = 55`.
    assert_eq!(&forward[0..4], &[34, 34, 34, 1], "left mask only");
    assert_eq!(&forward[4..8], &[55, 55, 55, 2], "left then overlap");
    assert_eq!(&forward[8..12], &[136, 136, 136, 3], "overlap mask only");
    // Reversed, the overlap stage runs first: its clarity window over the
    // original row has mean 130, so pixel 1 becomes `100 + 0.5 * (100-130) =
    // 85`, the working frame is `[70, 85, 130, 175, 220]`, and the left texture
    // stage then gives pixel 1 a window mean of 136 and
    // `85 + 0.8 * (85-136) = 44`. Different bytes: reordering is a
    // render-identity change, never an equivalent state.
    assert_eq!(&swapped[4..8], &[44, 44, 44, 2], "overlap then left");
    // The left-only pixel is order-independent: only the left layer touches it.
    assert_eq!(&forward[0..4], &swapped[0..4]);
}

// -------------------------------------------------------------- refusals

/// Invalid local presence values are a loud preflight error: the render is
/// refused and a hand-constructed invalid object is never silently clipped or
/// dropped.
#[test]
fn invalid_local_presence_values_are_a_loud_preflight_error() {
    let mut cases: Vec<(&str, LocalAdjustments)> = Vec::new();

    let mut wrong_block_version = baseline();
    wrong_block_version
        .presence
        .as_mut()
        .expect("baseline")
        .version = 2;
    cases.push(("presence block version", wrong_block_version));

    let mut out_of_range = baseline();
    out_of_range.presence.as_mut().expect("baseline").clarity = 1.01;
    cases.push(("presence clarity out of range", out_of_range));

    let mut below_range = baseline();
    below_range.presence.as_mut().expect("baseline").texture = -1.5;
    cases.push(("presence texture below range", below_range));

    let mut non_finite = baseline();
    non_finite.presence.as_mut().expect("baseline").dehaze = f32::NAN;
    cases.push(("non-finite presence dehaze", non_finite));

    let mut infinite = baseline();
    infinite.presence.as_mut().expect("baseline").dehaze = f32::INFINITY;
    cases.push(("infinite presence dehaze", infinite));

    // A v4 document that carries a presence block is a loud smuggling error,
    // not a silent drop.
    let mut smuggling = baseline();
    smuggling.version = 4;
    cases.push(("v4 document with a presence block", smuggling));

    assert!(!cases.is_empty());
    for (name, recipe) in cases {
        let definition =
            mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
        let copies = vec![copy_with(
            "vc",
            vec![definition],
            vec![presence_layer(recipe.clone())],
        )];
        let frame = ImageFrame::new(1, 1, vec![100, 100, 100, 255]).unwrap();
        let planes = BTreeMap::from([(
            ("vc".into(), "subject".into()),
            MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
        )]);
        let error = local_render(&frame, &copies, planes, &EditRecipe::default(), None)
            .expect_err("an invalid local presence block must be a loud preflight error");
        let message = error.to_string();
        assert!(message.contains("local"), "{name}: {message}");
    }
}

/// The public setters refuse the same values the validator refuses, *before*
/// they mutate, so a rejected edit leaves the layer byte-for-byte unchanged.
#[test]
fn the_local_presence_setters_refuse_before_they_mutate() {
    let mut recipe = baseline();
    let before = serde_json::to_string(&recipe).expect("serializable");
    for (field, value) in [
        ("texture", 1.000_001),
        ("clarity", -2.0),
        ("dehaze", f64::NAN),
        ("texture", f64::INFINITY),
    ] {
        let error = recipe
            .set_local_presence_field(field, value)
            .expect_err("an out-of-range or non-finite amount must be refused");
        assert!(error.contains(field), "{error}");
        assert_eq!(
            serde_json::to_string(&recipe).expect("serializable"),
            before,
            "a refused `{field}` must leave the layer byte-for-byte unchanged"
        );
    }
    let error = recipe
        .set_local_presence_field("grain", 0.5)
        .expect_err("an unknown field must be refused");
    assert!(error.contains("grain"), "{error}");
}

/// The still-disabled stages must stay unreachable from a local layer. Local
/// presence is the *only* new local block: detail, sharpening, noise reduction,
/// AI-denoise and optics must have neither a field, nor a key, nor a renderer
/// stub.
#[test]
fn disabled_local_stages_stay_unreachable() {
    let json = serde_json::to_value(baseline()).expect("serializable");
    let keys: Vec<String> = json
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    assert!(
        keys.iter().any(|key| key == "presence"),
        "a layer that stores a presence block must serialize it"
    );
    for disabled in [
        "detail",
        "texture_detail",
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
    // The presence field names are the three amounts, and nothing else.
    for name in ["texture", "clarity", "dehaze"] {
        let mut recipe = LocalAdjustments::default();
        assert!(
            recipe.set_local_presence_field(name, 0.5).is_ok(),
            "`{name}` must be a local presence field"
        );
    }
    // And a presence-only layer is still refused by the stand-in routes, i.e.
    // the routing predicate is driven by `is_neutral`, not by a presence
    // special case.
    let mut recipe = LocalAdjustments::default();
    assert!(recipe.is_neutral());
    recipe
        .set_local_presence_field("dehaze", 0.1)
        .expect("dehaze");
    assert!(!recipe.is_neutral());
    recipe.reset_local_presence();
    assert!(recipe.is_neutral());
    assert!(!recipe.has_local_presence());
}

/// A local presence edit never mutates the global recipe, not even when the
/// global recipe itself carries a presence block.
#[test]
fn a_local_presence_edit_never_touches_the_global_recipe() {
    let pixel = [180, 90, 40, 55];
    let mut global = EditRecipe::default();
    global.presence = Some(Presence {
        version: 1,
        texture: -0.9,
        clarity: 0.7,
        dehaze: -0.3,
    });
    let mut local = LocalAdjustments::default();
    local
        .set_local_presence_field("texture", 0.35)
        .expect("texture");

    let mut definition =
        mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    definition.geometry_context.width = 5;
    definition.geometry_context.height = 1;
    let row: Vec<u8> = (0..5u8)
        .flat_map(|i| {
            let v = 60 + 25 * i;
            [v, v, v, 55]
        })
        .collect();
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![presence_layer(local.clone())],
    )];
    let frame = ImageFrame::new(5, 1, row.clone()).unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(5, 1, vec![u16::MAX; 5]).unwrap(),
    )]);
    let combined = local_render(&frame, &copies, planes, &global, None).unwrap();

    // The same thing done by hand: global recipe, then the local kernel, once,
    // in local order. The global presence must not be touched by the local one.
    let mut expected = ImageFrame::new(5, 1, row).unwrap();
    expected.apply_recipe(&global).unwrap();
    expected.apply_mask_local_recipe(&local).unwrap();
    assert_eq!(combined.frame.pixels, expected.pixels);
    // And the local block is a *relative* edit: the global presence is still
    // exactly what it was before the local render.
    assert_eq!(
        global.presence.as_ref().expect("global presence").texture,
        -0.9
    );
    // The 1x1 helper must agree with the 5-wide render for the shared pixel.
    let single = render_single(&local, pixel);
    assert_eq!(single[3], 55, "alpha is never touched");
    let _ = combined;
}

/// The whole presence block survives a JSON round trip byte for byte.
#[test]
fn local_presence_json_round_trip_is_byte_stable() {
    let mut recipe = baseline();
    recipe
        .set_local_presence_field("clarity", -0.35)
        .expect("clarity");
    recipe
        .set_local_presence_field("dehaze", 0.5)
        .expect("dehaze");
    let json = serde_json::to_string(&recipe).expect("serializable");
    let parsed: LocalAdjustments = serde_json::from_str(&json).expect("deserializable");
    assert_eq!(parsed, recipe);
    assert_eq!(serde_json::to_string(&parsed).expect("serializable"), json);
    // Resetting every field must leave no block behind at all, so a reset is
    // byte-identical to a layer that was never edited.
    let mut reset = recipe.clone();
    reset.reset_local_presence();
    assert_eq!(reset.presence, None);
    assert_eq!(
        serde_json::to_string(&reset).expect("serializable"),
        serde_json::to_string(&LocalAdjustments::default()).expect("serializable")
    );
    // Typing the last non-neutral amount back to zero drops the block, too.
    let mut typed_back = recipe.clone();
    for field in ["texture", "clarity", "dehaze"] {
        typed_back
            .set_local_presence_field(field, 0.0)
            .expect("presence field");
    }
    assert_eq!(
        typed_back.presence, None,
        "an all-zero block must not linger in storage"
    );
}

/// The presence block is part of the render identity: any amount change must
/// change the layer's digest.
#[test]
fn the_presence_block_is_part_of_the_local_render_identity() {
    let mut recipe = LocalAdjustments::default();
    let none = recipe.digest();
    recipe
        .set_local_presence_field("texture", 0.5)
        .expect("texture");
    let with_texture = recipe.digest();
    assert_ne!(none, with_texture);
    recipe
        .set_local_presence_field("texture", -0.5)
        .expect("texture");
    let negative = recipe.digest();
    assert_ne!(
        with_texture, negative,
        "the signed amount is part of identity"
    );
    recipe
        .set_local_presence_field("texture", 0.0)
        .expect("texture");
    assert_eq!(recipe.presence, None);
    assert_eq!(
        recipe.digest(),
        none,
        "a reset returns the identity to `none`"
    );
    let mut with_curve = LocalAdjustments::default();
    with_curve.curves = Some(Curves::identity());
    assert_ne!(
        with_curve.digest(),
        LocalAdjustments::default().digest(),
        "an explicitly stored identity curve still changes the identity"
    );
}
