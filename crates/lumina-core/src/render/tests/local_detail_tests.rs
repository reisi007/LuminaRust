//! MASK-LOCAL-P1.2d CPU compositor goldens for the mask-local detail block.
//!
//! Every expected byte in this file is an **exact** literal: there is no
//! tolerance, no PSNR and no "close enough" anywhere. The literals are pinned
//! CPU goldens of the documented chain
//! `global result → local relative WB → local Basic → local Presence →
//! local tone curve → local colour → local Noise Reduction → local Sharpening →
//! fractional mask blend`, evaluated in floating point with exactly **one** RGBA8
//! quantization at the end of the layer. The heaviest structural claims (the
//! single quantization boundary, the full-frame neighbourhood and the global
//! render scale) are proved by independent re-derivations in
//! `local_detail_boundary_tests.rs`.
//!
//! There deliberately is **no** test in this file (or anywhere in the crate)
//! claiming byte-equality between the local detail path and the global detail
//! stages: the two quantize at different points (the global kernel rounds after
//! the noise-reduction stage and again after the sharpening write because it
//! owns a `u8` frame; the local chain rounds once at the end of the layer), so
//! that equality is false by design.

use super::local_adjustments::local_render;
use super::local_detail_goldens::{
    independent_order_chains, GOLDEN_FULL_STACK, GOLDEN_NOISE_BEFORE_SHARPEN, GOLDEN_NOISE_COLOR,
    GOLDEN_NOISE_LUMINANCE, GOLDEN_SHARPENING_AMOUNT, GOLDEN_SHARPENING_DETAIL_ONE,
    GOLDEN_SHARPENING_DETAIL_ZERO, GOLDEN_SHARPENING_MASKING_ONE, GOLDEN_SHARPENING_MASKING_ZERO,
    GOLDEN_SHARPENING_RADIUS_MAX, GOLDEN_SHARPENING_RADIUS_MIN,
};
use super::*;
use lumina_sidecar::{
    CurveChannels, CurvePoint, CurvePoints, Curves, Detail, HslAdjustments, LocalAdjustments,
    NoiseReduction, Presence, Sharpening,
};

pub(super) fn detail_layer(recipe: LocalAdjustments) -> MaskLayer {
    let mut layer = layer("layer-1", reference("vc", "subject"));
    layer.local_adjustments = Some(recipe);
    layer
}

/// Render a single-row frame through the full pipeline with one mask layer
/// carrying `recipe` at mask alpha `mask_alpha`.
pub(super) fn render_row(recipe: &LocalAdjustments, pixels: Vec<u8>, mask_alpha: u16) -> Vec<u8> {
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
    let frame = ImageFrame::new(width, 1, pixels).unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(width, 1, vec![mask_alpha; width as usize]).unwrap(),
    )]);
    local_render(&frame, &copies, planes, &EditRecipe::default(), None)
        .unwrap()
        .frame
        .pixels
}

/// A five-sample row with a hard edge on the left and a smooth ramp to the
/// right, so the bilateral similarity term, the Gaussian support and the
/// whole-frame gradient maximum all have something to work with.
pub(super) fn step_row() -> Vec<u8> {
    vec![
        20, 20, 20, 255, 20, 20, 20, 255, 90, 90, 90, 255, 160, 160, 160, 255, 230, 230, 230, 255,
    ]
}

pub(super) fn lifted_master() -> CurvePoints {
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

pub(super) fn red_hsl() -> HslAdjustments {
    HslAdjustments {
        version: 1,
        red: Some(lumina_sidecar::HslChannel {
            hue: 0.3,
            saturation: 0.5,
            luminance: 0.0,
        }),
        orange: None,
        yellow: None,
        green: None,
        cyan: None,
        blue: None,
        violet: None,
        magenta: None,
    }
}

pub(super) fn sharpening_only(
    amount: f32,
    radius: f32,
    detail: f32,
    masking: f32,
) -> LocalAdjustments {
    LocalAdjustments {
        detail: Some(Detail {
            sharpening: Some(Sharpening {
                version: 1,
                amount,
                radius,
                detail,
                masking,
            }),
            noise_reduction: None,
        }),
        ..LocalAdjustments::default()
    }
}

pub(super) fn noise_only(luminance: f32, color: f32) -> LocalAdjustments {
    LocalAdjustments {
        detail: Some(Detail {
            sharpening: None,
            noise_reduction: Some(NoiseReduction {
                version: 1,
                luminance,
                color,
            }),
        }),
        ..LocalAdjustments::default()
    }
}

/// A sharpening block is pixel-neutral **exactly** iff `amount == 0`, and that is
/// observed on pixels in both directions.
///
/// * `amount == 0` is the global F-095 early return, so `radius`, `detail` and
///   `masking` can be anything at all and the block still drops out of the
///   content-based path selection: the render is byte-identical to the input.
/// * `amount != 0` is never neutral. The sigma floor is `0.5` and the first
///   off-centre tap of the documented kernel still carries
///   `exp(-1/(2·0.5²)) = exp(-2) ≈ 0.135`, so no legal `radius`/`detail`/`masking`
///   can make the stage a no-op — including `masking = 0`, which is the
///   *strongest* setting (`1.0` for every pixel, flat areas included), never a
///   "no flat-area suppression means no sharpening" switch.
#[test]
fn a_sharpening_block_is_pixel_neutral_exactly_when_the_amount_is_zero() {
    let row = step_row();
    // (a) amount = 0 is neutral for every legal radius/detail/masking.
    for (radius, detail, masking) in [
        (0.1f32, 0.0f32, 0.0f32),
        (0.1, 1.0, 1.0),
        (2.0, 0.5, 0.5),
        (10.0, 1.0, 0.0),
    ] {
        let neutral = sharpening_only(0.0, radius, detail, masking);
        assert!(!neutral.has_local_sharpening(), "{neutral}");
        assert!(!neutral.has_local_detail(), "{neutral}");
        assert!(neutral.is_neutral(), "{neutral}");
        assert_eq!(
            render_row(&neutral, row.clone(), u16::MAX),
            row,
            "amount 0 must be a byte-identity no-op at radius {radius}"
        );
    }
    // (b) amount != 0 is never neutral, at any legal radius/detail/masking.
    for (radius, detail, masking) in [
        (0.1f32, 0.0f32, 0.0f32),
        (0.1, 0.0, 1.0),
        (0.5, 1.0, 1.0),
        (2.0, 0.5, 0.0),
        (2.0, 0.5, 1.0),
        (10.0, 0.0, 1.0),
        (10.0, 1.0, 0.0),
    ] {
        let live = sharpening_only(1.25, radius, detail, masking);
        assert!(live.has_local_sharpening(), "{live}");
        assert!(live.has_local_detail(), "{live}");
        assert!(!live.is_neutral(), "{live}");
        let rendered = render_row(&live, row.clone(), u16::MAX);
        assert_ne!(
            rendered, row,
            "amount 1.25 must change bytes at radius {radius}, detail {detail}, masking {masking}"
        );
    }
    // (c) `masking = 0` is the strongest setting, not a no-op: with a non-zero
    // amount it must move at least as much as any other masking value, and it
    // must move the flat interior that `masking = 1` protects. (A *completely*
    // uniform frame is a separate, mathematical identity — the detail term
    // `lum − blur` is zero there — so the fixture is flat *with one outlier*:
    // the flat interior is then a real flat area next to a real edge.)
    let mut flat_row = vec![128_u8; 40];
    flat_row[3 * 4..3 * 4 + 3].copy_from_slice(&[200, 200, 200]);
    let strong = render_row(
        &sharpening_only(2.0, 2.0, 1.0, 0.0),
        flat_row.clone(),
        u16::MAX,
    );
    let guarded = render_row(
        &sharpening_only(2.0, 2.0, 1.0, 1.0),
        flat_row.clone(),
        u16::MAX,
    );
    assert_ne!(&strong[..3], &[128, 128, 128], "{strong:?}");
    assert_eq!(&guarded[..3], &[128, 128, 128], "{guarded:?}");
}

/// The sharpening **amount** has an exact golden, and the image alpha byte
/// survives untouched.
#[test]
fn local_sharpening_amount_has_an_exact_golden_and_preserves_alpha() {
    let row = step_row();
    let mut translucent = row.clone();
    for pixel in translucent.as_chunks_mut::<4>().0 {
        pixel[3] = 77;
    }
    let opaque = render_row(&sharpening_only(0.75, 2.0, 0.5, 0.0), row, u16::MAX);
    assert_eq!(opaque, GOLDEN_SHARPENING_AMOUNT);
    let rendered = render_row(&sharpening_only(0.75, 2.0, 0.5, 0.0), translucent, u16::MAX);
    // Only the alpha byte differs; the RGB bytes are the same golden.
    for (plain, seen) in opaque
        .as_chunks::<4>()
        .0
        .iter()
        .zip(rendered.as_chunks::<4>().0)
    {
        assert_eq!(&plain[..3], &seen[..3]);
        assert_eq!(seen[3], 77);
    }
}

/// The sharpening **radius** has exact goldens at both legal boundaries
/// (`0.1` and `10.0`), and the two differ.
#[test]
fn local_sharpening_radius_has_exact_goldens_at_both_legal_boundaries() {
    let row = step_row();
    let narrow = render_row(&sharpening_only(0.75, 0.1, 0.5, 0.0), row.clone(), u16::MAX);
    let wide = render_row(&sharpening_only(0.75, 10.0, 0.5, 0.0), row, u16::MAX);
    assert_eq!(narrow, GOLDEN_SHARPENING_RADIUS_MIN);
    assert_eq!(wide, GOLDEN_SHARPENING_RADIUS_MAX);
    assert_ne!(narrow, wide, "the radius must reach the kernel support");
}

/// The sharpening **detail** mix has exact goldens for both ends of the
/// documented `detail·d_fine + (1−detail)·d_coarse` mix.
#[test]
fn local_sharpening_detail_has_exact_goldens_for_both_ends_of_the_mix() {
    let row = step_row();
    let fine_only = render_row(&sharpening_only(0.75, 2.0, 1.0, 0.0), row.clone(), u16::MAX);
    let coarse_only = render_row(&sharpening_only(0.75, 2.0, 0.0, 0.0), row, u16::MAX);
    assert_eq!(fine_only, GOLDEN_SHARPENING_DETAIL_ONE);
    assert_eq!(coarse_only, GOLDEN_SHARPENING_DETAIL_ZERO);
    assert_ne!(fine_only, coarse_only);
}

/// The flat-area **masking** has exact goldens: with `masking = 1` the flat
/// sample keeps its value, with `masking = 0` the whole frame is sharpened
/// (flat areas included).
#[test]
fn local_sharpening_masking_has_an_exact_golden_and_suppresses_the_flat_area() {
    // A flat row with a single outlier: the outlier is an edge, everything else
    // is a genuinely flat area.
    let mut row = vec![128_u8; 40];
    row[3 * 4..3 * 4 + 3].copy_from_slice(&[200, 200, 200]);
    let masked = render_row(&sharpening_only(2.0, 2.0, 1.0, 1.0), row.clone(), u16::MAX);
    let unmasked = render_row(&sharpening_only(2.0, 2.0, 1.0, 0.0), row, u16::MAX);
    assert_eq!(masked, GOLDEN_SHARPENING_MASKING_ONE);
    assert_eq!(unmasked, GOLDEN_SHARPENING_MASKING_ZERO);
    // The flat leftmost sample is untouched by the masked run and moved by the
    // unmasked one — the whole point of the flat-area suppression.
    assert_eq!(&masked[0..3], &[128, 128, 128], "{masked:?}");
    assert_ne!(&unmasked[0..3], &[128, 128, 128], "{unmasked:?}");
}

/// `masking = 0` is the **strongest** setting, so a sharpening block with
/// `masking = 0` and a differing `amount`/`radius`/`detail` must never be a
/// silent no-op. This is the content-based kernel-path selection.
#[test]
fn a_masking_zero_sharpening_block_is_not_a_silent_no_op() {
    let row = step_row();
    let mut zero_masking = LocalAdjustments::default();
    zero_masking
        .set_local_sharpening_field("amount", 0.75)
        .unwrap();
    zero_masking
        .set_local_sharpening_field("radius", 2.0)
        .unwrap();
    zero_masking
        .set_local_sharpening_field("detail", 0.5)
        .unwrap();
    assert_eq!(
        zero_masking
            .detail
            .as_ref()
            .unwrap()
            .sharpening
            .unwrap()
            .masking,
        0.0
    );
    assert!(zero_masking.has_local_detail());
    assert!(zero_masking.has_local_sharpening());
    let rendered = render_row(&zero_masking, row.clone(), u16::MAX);
    assert_eq!(rendered, GOLDEN_SHARPENING_AMOUNT);
    assert_ne!(rendered, row, "a masking=0 block must change the bytes");
    // A genuinely neutral block (amount 0) is the only sharpening state the path
    // selection drops.
    let mut neutral = LocalAdjustments::default();
    neutral.set_local_sharpening_field("amount", 0.0).unwrap();
    assert!(!neutral.has_local_detail());
    assert!(neutral.is_neutral());
    assert_eq!(render_row(&neutral, row.clone(), u16::MAX), row);
}

/// The noise-reduction **luminance** has an exact golden and preserves alpha.
#[test]
fn local_noise_reduction_luminance_has_an_exact_golden_and_preserves_alpha() {
    let row = vec![
        20, 20, 20, 33, 30, 30, 30, 33, 60, 60, 60, 33, 120, 120, 120, 33, 200, 200, 200, 33,
    ];
    let rendered = render_row(&noise_only(0.6, 0.0), row, u16::MAX);
    assert_eq!(rendered, GOLDEN_NOISE_LUMINANCE);
    assert!(rendered
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| pixel[3] == 33));
}

/// The noise-reduction **colour** has an exact golden.
#[test]
fn local_noise_reduction_color_has_an_exact_golden() {
    let row = vec![
        40, 90, 200, 255, 60, 120, 180, 255, 200, 40, 90, 255, 240, 200, 30, 255,
    ];
    let rendered = render_row(&noise_only(0.0, 0.75), row, u16::MAX);
    assert_eq!(rendered, GOLDEN_NOISE_COLOR);
}

/// Noise reduction runs **before** sharpening inside a local layer, exactly as
/// the global kernel runs it. The expected bytes come from an **independent**
/// transcription of both orders written into this test.
#[test]
fn local_noise_reduction_runs_before_local_sharpening() {
    let row = vec![
        20, 20, 20, 255, 25, 25, 25, 255, 20, 20, 20, 255, 235, 235, 235, 255, 240, 240, 240, 255,
    ];
    let noise = NoiseReduction {
        version: 1,
        luminance: 0.5,
        color: 0.0,
    };
    let sharp = Sharpening {
        version: 1,
        amount: 2.0,
        radius: 2.0,
        detail: 1.0,
        masking: 0.0,
    };
    let combined = LocalAdjustments {
        detail: Some(Detail {
            sharpening: Some(sharp),
            noise_reduction: Some(noise),
        }),
        ..LocalAdjustments::default()
    };
    let specified = render_row(&combined, row.clone(), u16::MAX);
    assert_eq!(specified, GOLDEN_NOISE_BEFORE_SHARPEN);
    let (right, wrong) = independent_order_chains(row.as_slice(), &noise, &sharp);
    assert_eq!(specified, right, "noise reduction must run first");
    assert_ne!(
        right, wrong,
        "the documented order must be observable, or this test is vacuous"
    );
}

/// The whole local stack — WB + Basic + Presence + curve + colour + NR +
/// sharpening — has an exact golden.
#[test]
fn the_full_local_detail_stack_has_an_exact_golden() {
    let mut recipe = LocalAdjustments {
        exposure: 0.5,
        contrast: 0.25,
        shadows: 0.5,
        highlights: -0.5,
        temperature_delta_k: 2000.0,
        tint_delta: -0.2,
        vibrance: 0.3,
        saturation: -0.5,
        presence: Some(Presence {
            version: 1,
            texture: 0.5,
            clarity: 0.0,
            dehaze: 0.0,
        }),
        curves: Some(Curves {
            version: 1,
            master: lifted_master(),
            channels: CurveChannels::default(),
        }),
        hsl: Some(red_hsl()),
        detail: Some(Detail {
            sharpening: Some(Sharpening {
                version: 1,
                amount: 1.2,
                radius: 2.5,
                detail: 0.75,
                masking: 0.5,
            }),
            noise_reduction: Some(NoiseReduction {
                version: 1,
                luminance: 0.4,
                color: 0.3,
            }),
        }),
        ..LocalAdjustments::default()
    };
    assert!(recipe.has_local_detail());
    let row: Vec<u8> = (0..9u8)
        .flat_map(|i| {
            let v = 80 + 10 * i;
            [
                v,
                (v as u16 + 17).min(255) as u8,
                (v as u16 + 53).min(255) as u8,
                200,
            ]
        })
        .collect();
    let rendered = render_row(&recipe, row, u16::MAX);
    assert_eq!(rendered, GOLDEN_FULL_STACK);
    recipe.reset_local_detail();
    assert!(!recipe.has_local_detail());
    assert!(recipe.has_local_presence());
}

/// The typed block round-trips through JSON byte-stably and stores the *global*
/// types at their global ranges.
#[test]
fn local_detail_json_round_trip_is_byte_stable() {
    let mut recipe = LocalAdjustments::default();
    recipe.set_local_sharpening_field("amount", 1.25).unwrap();
    recipe.set_local_sharpening_field("radius", 3.5).unwrap();
    recipe.set_local_sharpening_field("detail", 0.25).unwrap();
    recipe.set_local_sharpening_field("masking", 0.75).unwrap();
    recipe
        .set_local_noise_reduction_field("luminance", 0.5)
        .unwrap();
    recipe
        .set_local_noise_reduction_field("color", 0.25)
        .unwrap();
    let first = serde_json::to_string(&recipe).unwrap();
    let second =
        serde_json::to_string(&serde_json::from_str::<LocalAdjustments>(&first).unwrap()).unwrap();
    assert_eq!(first, second);
    let loaded: LocalAdjustments = serde_json::from_str(&first).unwrap();
    assert_eq!(loaded, recipe);
    assert_eq!(loaded.detail, recipe.detail);
    for key in ["\"detail\"", "\"sharpening\"", "\"noise_reduction\""] {
        assert!(first.contains(key), "{key} must be persisted: {first}");
    }
    let detail = loaded.detail.unwrap();
    let sharpening = detail.sharpening.unwrap();
    assert_eq!(sharpening.version, 1);
    assert!((sharpening.amount - 1.25).abs() < 1e-6);
    assert!((sharpening.radius - 3.5).abs() < 1e-6);
    assert!((sharpening.detail - 0.25).abs() < 1e-6);
    assert!((sharpening.masking - 0.75).abs() < 1e-6);
    let noise = detail.noise_reduction.unwrap();
    assert_eq!(noise.version, 1);
    assert!((noise.luminance - 0.5).abs() < 1e-6);
    assert!((noise.color - 0.25).abs() < 1e-6);
    assert_eq!(recipe.detail_summary(), "sharpening+noise_reduction");
    assert!(recipe
        .to_string()
        .contains("detail=sharpening+noise_reduction"));
    // A whole-block reset returns the layer to the never-edited identity.
    let mut full = recipe.clone();
    full.reset_local_detail();
    assert!(full.detail.is_none());
    assert_eq!(full.digest(), LocalAdjustments::default().digest());
}
