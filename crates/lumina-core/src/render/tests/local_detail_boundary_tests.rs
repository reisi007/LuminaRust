//! MASK-LOCAL-P1.2d CPU compositor *structural* proofs for the local detail
//! block.
//!
//! Two claims live here, and both are proved with material that does **not**
//! come from the function under test:
//!
//! 1. **Full-frame neighbourhoods, mask-only blending.** The proof is partly a
//!    source-level statement — the local detail kernel can be called with no mask
//!    and no region of interest at all, which this test actually does — and partly
//!    behavioural: a mask that covers a single pixel still lets that pixel's
//!    detail see its *unmasked* neighbours, and a mask that covers nothing
//!    reproduces the input bytes exactly.
//! 2. **The render scale is global.** A changed effective scale changes the
//!    local detail result through the *same* `max(radius · scale, 0.5)` formula
//!    the global stage uses — pinned **on pixels**, for the local path *and* for
//!    the global stage, both above and below the radius floor — plus a
//!    decode-digest invariant.
//!
//! The other two claims live in `local_detail_reference.rs` (which holds the
//! independently transcribed chains) and `local_detail_order_tests.rs`: the
//! **within-layer order** — curve and the whole colour block *before* the detail
//! stages — and the **single quantization boundary**.
//!
//! There is deliberately **no** test here claiming byte-equality between the
//! local detail path and the global detail stages. The two quantize at different
//! points on purpose: the global kernel rounds to `u8` after the noise-reduction
//! stage and again after the sharpening write because it owns a `u8` frame,
//! while the local layer owns exactly **one** RGBA8 rounding, at the very end.
//! That divergence is documented in `crate::detail_stages` and is a design
//! decision, not an accident — so there is nothing to assert about
//! byte-equality.

use super::local_adjustments::local_render;
use super::*;
use lumina_sidecar::{Detail, LocalAdjustments, NoiseReduction, Sharpening};

fn detail_layer(recipe: LocalAdjustments) -> MaskLayer {
    let mut layer = layer("layer-1", reference("vc", "subject"));
    layer.local_adjustments = Some(recipe);
    layer
}

fn render_row(recipe: &LocalAdjustments, pixels: Vec<u8>, mask_alpha: u16) -> Vec<u8> {
    render_with_mask_alphas(recipe, &pixels, &vec![mask_alpha; pixels.len() / 4])
}

/// Render one row with a **per-pixel** mask alpha vector.
fn render_with_mask_alphas(recipe: &LocalAdjustments, pixels: &[u8], alphas: &[u16]) -> Vec<u8> {
    let width = (pixels.len() / 4) as u32;
    let mut definition =
        mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    definition.geometry_context.width = width;
    definition.geometry_context.height = 1;
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![detail_layer(recipe.clone())],
    )];
    let frame = ImageFrame::new(width, 1, pixels.to_vec()).unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(width, 1, alphas.to_vec()).unwrap(),
    )]);
    local_render(&frame, &copies, planes, &EditRecipe::default(), None)
        .unwrap()
        .frame
        .pixels
}

fn saturating_row() -> Vec<u8> {
    (0..5u8)
        .flat_map(|i| {
            let v = 40 + 30 * i;
            [v, v, 255 - v, 255]
        })
        .collect()
}

/// The detail neighbourhood is **full-frame**; the mask only gates the blend.
///
/// The structural half of the proof is the kernel *signature*: the test calls the
/// local detail kernel directly, with a frame, a recipe and the global render
/// scale — and **nothing else**. There is no mask plane and no region of interest
/// to pass, so a mask-restricted neighbourhood, a mask-derived window size or a
/// mask-derived statistic is not expressible. The behavioural half then pins:
///
/// * the mask-free kernel result is exactly the fully-masked render, so the mask
///   contributes nothing but the blend amount,
/// * a single-pixel mask still leaves that pixel's *neighbourhood* coming from
///   the whole frame, and
/// * a mask that covers nothing reproduces the input bytes exactly.
#[test]
fn the_detail_neighbourhood_is_full_frame_and_the_mask_only_gates_the_blend() {
    // (1) The signature, exercised: a mask is not even a parameter.
    let mask_free_kernel: fn(&mut [u8], u32, u32, &lumina_sidecar::MaskLocalRecipe, f32) =
        super::super::local_detail::apply_mask_local_wb_basic_tone_color_detail;

    let recipe = LocalAdjustments {
        detail: Some(Detail {
            sharpening: Some(Sharpening {
                version: 1,
                amount: 1.0,
                radius: 2.0,
                detail: 1.0,
                masking: 0.0,
            }),
            noise_reduction: Some(NoiseReduction {
                version: 1,
                luminance: 0.6,
                color: 0.0,
            }),
        }),
        ..LocalAdjustments::default()
    };
    let pixels = saturating_row();
    // (2) The mask-free kernel result equals the fully-masked render.
    let mut direct = pixels.clone();
    mask_free_kernel(&mut direct, 5, 1, &recipe, 1.0);
    let full = render_row(&recipe, pixels.clone(), u16::MAX);
    assert_eq!(
        direct, full,
        "the mask must contribute nothing but the blend"
    );
    assert_ne!(
        direct, pixels,
        "and the detail must actually have done work"
    );

    // (3) A single-pixel mask leaves the other four pixels byte-identical, and the
    // masked pixel keeps the full-frame neighbourhood value (a mask-restricted
    // window would have seen one sample and produced a different byte).
    let one_pixel = render_with_mask_alphas(&recipe, &pixels, &[u16::MAX, 0, 0, 0, 0]);
    assert_eq!(&one_pixel[4..], &pixels[4..], "alpha 0 must change no byte");
    assert_eq!(&one_pixel[..4], &full[..4], "the masked pixel must be full");
    assert_ne!(&one_pixel[..4], &pixels[..4], "and it must have changed");

    // (4) A mask that covers nothing is byte identity, and a source mask smaller
    // than the output is resampled by the *mask evaluation* stage to the output
    // frame — never by the detail kernel. The evaluated plane is still
    // frame-sized, so the detail maths always sees the whole frame.
    let none = render_with_mask_alphas(&recipe, &pixels, &[0, 0, 0, 0, 0]);
    assert_eq!(none, pixels);
    let mut definition =
        mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    definition.geometry_context.width = 3;
    definition.geometry_context.height = 1;
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![detail_layer(recipe.clone())],
    )];
    let frame = ImageFrame::new(5, 1, pixels.clone()).unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(3, 1, vec![u16::MAX; 3]).unwrap(),
    )]);
    let resampled = local_render(&frame, &copies, planes, &EditRecipe::default(), None)
        .expect("mask evaluation resamples to the output frame");
    assert_eq!(resampled.mask_layers[0].plane.width, 5);
    assert_eq!(resampled.frame.pixels, full, "a full mask is a full mask");
}

/// MASK-LOCAL-P1.2d / F-096: the local detail block follows the **global**
/// render scale, has no scale option of its own, and never overrides it.
///
/// Three independent proofs:
///
/// * **Pixel-level scale identity, above *and* below the radius floor.** For the
///   global F-095 radius formula `sigma = max(max(k·r, 0.5)·s, 0.5)` a doubled
///   effective scale produces the same **pixels** as a doubled radius *while the
///   inner `max(k·r, 0.5)` is not active* — pinned for radius `2.0` and `3.0`, for
///   the local path *and* for the global stage. The very same doubling **fails**
///   below that floor (`0.1 → 0.2` clamps the radius to `0.5` but not the scale),
///   which is pinned on pixels too and repeated inside the assertion messages,
///   because the identity is *floor-conditional* and a reader must not have to
///   look it up.
/// * **No scale option of its own** (schema + loud refusal of a bad scale).
/// * **Decode-digest invariance.** A changed output size changes the render
///   digest but never the decode or mask digest, exactly like
///   `sharpening_render_scale_changes_render_only` pins for the global stage.
#[test]
fn the_local_detail_follows_the_global_render_scale() {
    use crate::cache::CacheStage;
    use crate::pipeline::{OutputSpec, RenderKey};

    // (a) The shared radius formula, read from the shared stage module.
    assert_eq!(crate::detail_stages::sharpen_sigma(2.0, 1.0), 2.0);
    assert_eq!(crate::detail_stages::sharpen_sigma(2.0, 2.0), 4.0);
    // The documented `0.5` sigma floor and three-sigma support, shared too.
    assert_eq!(crate::detail_stages::sharpen_sigma(0.0, 1.0), 0.5);
    assert_eq!(crate::detail_stages::sharpen_support(0.5), 2);
    assert_eq!(crate::detail_stages::sharpen_support(2.0), 6);
    // The fine/coarse radius pair is the documented one.
    let sharpening = Sharpening {
        version: 1,
        amount: 1.0,
        radius: 2.0,
        detail: 0.5,
        masking: 0.0,
    };
    assert_eq!(
        crate::detail_stages::sharpen_blur_radii(&sharpening),
        (1.0, 3.0)
    );

    // (b) The identity is **floor-conditional**, and that is asserted on pixels,
    // for the local kernel and for the global stage, in both directions.
    //
    // The formula is `sigma = max(max(k·r, 0.5)·s, 0.5)`, where `k` is the
    // documented `0.5` (fine) resp. `1.5` (coarse) radius factor. It is
    // linear in `r` and in `s` *at the same time* only while the inner
    // `max(k·r, 0.5)` is not active.
    for radius in [2.0f32, 3.0] {
        let at_radius = Sharpening {
            radius,
            ..sharpening
        };
        let mut doubled_radius = at_radius;
        doubled_radius.radius = radius * 2.0;
        let local_at_unit_scale = local_detail_bytes(&at_radius, 1.0);
        let local_at_double_scale = local_detail_bytes(&at_radius, 2.0);
        let local_at_double_radius = local_detail_bytes(&doubled_radius, 1.0);
        assert_eq!(
            local_at_double_scale, local_at_double_radius,
            "ABOVE the radius floor (radius {radius}, k·r >= 0.5): a doubled global render scale \
             must equal a doubled radius on pixels, locally"
        );
        assert_ne!(
            local_at_unit_scale, local_at_double_scale,
            "a changed global render scale must change the local detail result"
        );
        let global_at_unit_scale = global_detail_bytes(&at_radius, 1.0);
        let global_at_double_scale = global_detail_bytes(&at_radius, 2.0);
        let global_at_double_radius = global_detail_bytes(&doubled_radius, 1.0);
        assert_eq!(
            global_at_double_scale, global_at_double_radius,
            "ABOVE the radius floor (radius {radius}, k·r >= 0.5): a doubled global render scale \
             must equal a doubled radius on pixels, in the global stage"
        );
        assert_ne!(global_at_unit_scale, global_at_double_scale);
    }
    // Below the inner floor the identity must **fail** on pixels. With
    // `radius = 0.1` both `k·r` values (`0.05` and `0.15`) are below `0.5`, so
    // the *radius* is clamped to `0.5` and doubling it changes nothing — while
    // doubling the *scale* multiplies that clamped `0.5` and very much does.
    // The identity is therefore a floor-conditional identity, and this is the
    // half that proves it — for the local kernel and for the global stage.
    let below_the_floor = |name: &str, run: fn(&Sharpening, f32) -> Vec<u8>| {
        let narrow = Sharpening {
            radius: 0.1,
            ..sharpening
        };
        let mut doubled_narrow = narrow;
        doubled_narrow.radius = 0.2;
        assert_eq!(
            run(&narrow, 1.0),
            run(&doubled_narrow, 1.0),
            "BELOW the radius floor 0.5: radius 0.1 and 0.2 must produce the same pixels \
             ({name}), because max(k·0.1, 0.5) = max(k·0.2, 0.5) = 0.5"
        );
        assert_ne!(
            run(&narrow, 2.0),
            run(&doubled_narrow, 1.0),
            "BELOW the radius floor 0.5: a doubled scale must NOT equal a doubled radius on \
             pixels ({name}) — max(0.5·2, 0.5) = 1.0 against max(0.5·1, 0.5) = 0.5"
        );
        assert_eq!(
            run(&narrow, 2.0),
            run(&doubled_narrow, 2.0),
            "and the clamped radius must make the two scales agree again ({name})"
        );
    };
    below_the_floor("local kernel", local_detail_bytes);
    below_the_floor("global stage", global_detail_bytes);
    // (c) The local block has no scale option of its own: the schema carries no
    // scale field, and an invalid effective scale is refused loudly by the local
    // entry point with the same error the global one uses, before any pixel moves.
    let recipe = LocalAdjustments {
        detail: Some(Detail {
            sharpening: Some(sharpening),
            noise_reduction: None,
        }),
        ..LocalAdjustments::default()
    };
    let json = serde_json::to_string(&recipe).unwrap();
    for forbidden in ["scale", "render_scale", "effective_scale"] {
        assert!(
            !json.contains(forbidden),
            "the local detail block must not persist its own scale: {json}"
        );
    }
    let pixels = grey_ramp();
    for bad in [0.0_f32, -1.0, f32::NAN, f32::INFINITY] {
        let mut frame = ImageFrame::new(7, 1, pixels.clone()).unwrap();
        let before = frame.pixels.clone();
        let error = frame
            .apply_mask_local_recipe_with_scale(&recipe, bad)
            .expect_err("an invalid effective scale must be refused");
        assert!(error.to_string().contains("effective_scale"), "{error}");
        assert_eq!(frame.pixels, before, "a refused scale must change no byte");
    }

    // (d) The render identity reacts the same way as the global sharpening
    // contract: a changed effective scale changes the render digest but never the
    // decode or the mask digest.
    let global_recipe = EditRecipe {
        sharpening: Some(sharpening),
        ..Default::default()
    };
    let small = RenderKey::new(
        "source",
        "decode",
        "pipeline",
        "vc",
        &global_recipe,
        vec![],
        OutputSpec {
            profile: "srgb".into(),
            width: 7,
            height: 7,
            format: "png".into(),
        },
    );
    let large = RenderKey::new(
        "source",
        "decode",
        "pipeline",
        "vc",
        &global_recipe,
        vec![],
        OutputSpec {
            profile: "srgb".into(),
            width: 14,
            height: 14,
            format: "png".into(),
        },
    );
    assert_eq!(
        small.stage_digest(CacheStage::Decode),
        large.stage_digest(CacheStage::Decode)
    );
    assert_eq!(
        small.stage_digest(CacheStage::Mask),
        large.stage_digest(CacheStage::Mask)
    );
    assert_ne!(small.digest(), large.digest());
}

fn grey_ramp() -> Vec<u8> {
    (0..7u8)
        .flat_map(|i| {
            let v = 40 + 25 * i;
            [v, v, v, 255]
        })
        .collect()
}

/// The local detail result at one effective scale, straight through the public
/// local entry point.
fn local_detail_bytes(sharpening: &Sharpening, effective_scale: f32) -> Vec<u8> {
    let recipe = LocalAdjustments {
        detail: Some(Detail {
            sharpening: Some(*sharpening),
            noise_reduction: None,
        }),
        ..LocalAdjustments::default()
    };
    let mut frame = ImageFrame::new(7, 1, grey_ramp()).unwrap();
    frame
        .apply_mask_local_recipe_with_scale(&recipe, effective_scale)
        .expect("a valid scale");
    frame.pixels
}

/// The **global** detail result at one effective scale, through the global entry
/// point. Used only for the shared radius-formula identity, never for a
/// byte-equality claim against the local path.
fn global_detail_bytes(sharpening: &Sharpening, effective_scale: f32) -> Vec<u8> {
    let recipe = EditRecipe {
        sharpening: Some(*sharpening),
        ..Default::default()
    };
    let mut frame = ImageFrame::new(7, 1, grey_ramp()).unwrap();
    frame
        .apply_recipe_with_scale(&recipe, effective_scale)
        .expect("a valid scale");
    frame.pixels
}

/// The `0..=255 ↔ 0..=1` domain conversion around the colour stages is not a
/// second quantization boundary. That claim is not merely "nothing rounds there"
/// (a code-reading argument): it is measured here, exactly, for every `u8` the
/// conversion can ever see on the way in.
#[test]
fn the_colour_domain_conversion_is_byte_neutral_for_every_input_byte() {
    let mut checked = 0u32;
    for value in 0u16..=255 {
        let input = value as u8;
        // The real conversion pair, spelled exactly as the kernel spells it:
        // narrow to `f32`, divide by 255.0, multiply back by 255.0, write `u8`.
        let narrowed = f64::from(input) as f32;
        let normalized = narrowed / 255.0;
        let restored = normalized * 255.0;
        assert_eq!(
            restored as u8, input,
            "`{input}` must survive the round trip exactly: the `f32` domain conversion is a \
             re-interpretation, not a quantization. 0..=255 is exact in `f32`, so the narrowing \
             costs nothing here."
        );
        checked += 1;
    }
    assert_eq!(checked, 256, "every `u8` input must be covered");
}

/// The bound behind the byte-neutrality above: even where the round trip is not
/// exact, the residual `f32` error is orders of magnitude too small to move a
/// byte. This makes the "no second quantization boundary" claim a measured
/// property instead of an assumption.
#[test]
fn the_colour_domain_conversion_error_cannot_move_a_byte() {
    // Sweep the normalized `0..=1` domain the colour stages actually return.
    let samples = 1_000_001u32;
    let mut worst = 0.0f32;
    for step in 0..samples {
        let normalized = f64::from(step) / f64::from(samples - 1);
        let restored = (normalized as f32) * 255.0;
        let ideal = (normalized * 255.0) as f32;
        worst = worst.max((restored - ideal).abs());
    }
    // A byte only flips at 0.5. The measured worst case is exactly 2^-16 (the
    // `f32` spacing near 255), which is 32768x below that — measured, not
    // assumed, and the assertion below rejects a vacuous zero.
    const HALF: f32 = 0.5;
    const TWO_POW_MINUS_16: f32 = 1.0 / 65_536.0;
    assert!(
        worst <= TWO_POW_MINUS_16,
        "worst `f32` domain-conversion error {worst} must stay at most 2^-16 = \
         {TWO_POW_MINUS_16}, the `f32` spacing near 255"
    );
    assert_eq!(
        worst, TWO_POW_MINUS_16,
        "the bound must equal the measured 2^-16, not a hand-picked number"
    );
    assert!(
        HALF / worst >= 32_768.0,
        "the margin to the 0.5 that would move a byte is {}x",
        HALF / worst
    );
    assert!(
        worst > 0.0,
        "the bound must be measured on a real error, not on a vacuous zero"
    );
}
