//! MASK-LOCAL-P1.2c CPU compositor goldens for the mask-local presence block.
//!
//! Every expected byte in this file was produced by an **independent
//! transcription of the documented kernel** — a separate re-derivation of the
//! §P1.2c contract arithmetic — not recorded from a run of the kernel and not
//! copied out of the function under test. The local chain is
//! `global result → local relative WB → local Basic → local Presence →
//! local tone curve → local colour → fractional mask blend`, evaluated in
//! floating point with exactly **one** RGBA8 quantization at the end.
//!
//! There deliberately is **no** test in this file claiming byte-equality
//! between the local presence path and the global presence stage: the two
//! quantize at different points (the global kernel rounds after every presence
//! sub-stage because it owns a `u8` frame; the local chain rounds once at the
//! end of the layer), so that equality is false by design.

use super::local_adjustments::local_render;
use super::*;
use lumina_sidecar::{CurvePoint, CurvePoints, Curves, LocalAdjustments, Presence};

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

fn presence_layer(
    id: &str,
    mask: lumina_sidecar::MaskReference,
    recipe: LocalAdjustments,
) -> MaskLayer {
    let mut layer = layer(id, mask);
    layer.local_adjustments = Some(recipe);
    layer
}

/// A single-pixel layer with one mask, rendered through the full pipeline.
fn render_single(recipe: &LocalAdjustments, pixel: [u8; 4]) -> Vec<u8> {
    let definition = mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![presence_layer(
            "layer-1",
            reference("vc", "subject"),
            recipe.clone(),
        )],
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

/// A full-mask row of `pixels.len() / 4` pixels, so the DoG neighbourhood and
/// the dark channel have real neighbours to work with.
fn render_row(recipe: &LocalAdjustments, pixels: Vec<u8>) -> Vec<u8> {
    let width = (pixels.len() / 4) as u32;
    let mut definition =
        mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    definition.geometry_context.width = width;
    definition.geometry_context.height = 1;
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![presence_layer(
            "layer-1",
            reference("vc", "subject"),
            recipe.clone(),
        )],
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

/// Build the recipe under test through the *public* setter, so the test also
/// pins the accessor/validator contract rather than poking the field.
fn recipe_with_presence(texture: f64, clarity: f64, dehaze: f64) -> LocalAdjustments {
    let mut recipe = LocalAdjustments::default();
    for (field, value) in [
        ("texture", texture),
        ("clarity", clarity),
        ("dehaze", dehaze),
    ] {
        if value != 0.0 {
            recipe
                .set_local_presence_field(field, value)
                .expect("local presence field");
        }
    }
    recipe
}

/// Nine greys `start, start+step, …` at a fixed alpha, as RGBA8 bytes.
fn grey_row(start: u8, step: u8, count: usize, alpha: u8) -> Vec<u8> {
    (0..count)
        .flat_map(|i| {
            let v = start + step * i as u8;
            [v, v, v, alpha]
        })
        .collect()
}

// ----------------------------------------------------------------- texture

/// Hand-derived exact golden for a local texture step.
///
/// Five greys `[90, 120, 150, 180, 210]` with `texture = 0.5`, so the radius is
/// `1 + round(|0.5| * 2) = 2` and the window is `x-2 ..= x+2` clipped to the
/// five-pixel row (edge replicates):
///
/// | x | window          | mean | detail | `v + 0.5 * detail` | byte |
/// |---|-----------------|------|--------|--------------------|------|
/// | 0 | 90, 120, 150    | 120  | -30    | 75                 | 75   |
/// | 1 | 90, 120, 150, 180 | 135 | -15   | 112.5              | 113  |
/// | 2 | all five        | 150  | 0      | 150                | 150  |
/// | 3 | 120, 150, 180, 210 | 165 | 15  | 187.5              | 188  |
/// | 4 | 150, 180, 210   | 180  | 30     | 225                | 225  |
#[test]
fn local_texture_has_an_exact_golden_and_preserves_alpha() {
    let recipe = recipe_with_presence(0.5, 0.0, 0.0);
    let out = render_row(
        &recipe,
        vec![
            90, 90, 90, 11, 120, 120, 120, 11, 150, 150, 150, 11, 180, 180, 180, 11, 210, 210, 210,
            11,
        ],
    );
    assert_eq!(
        out,
        vec![
            75, 75, 75, 11, 113, 113, 113, 11, 150, 150, 150, 11, 187, 187, 187, 11, 225, 225, 225,
            11,
        ]
    );
}

/// A full `texture = 1.0` (radius 3) on a coloured ramp: the three channels are
/// treated independently and both edge-replicated ends move.
#[test]
fn local_texture_full_amount_has_an_exact_golden() {
    let recipe = recipe_with_presence(1.0, 0.0, 0.0);
    let out = render_row(
        &recipe,
        vec![
            80, 80, 80, 255, 100, 100, 100, 255, 120, 120, 120, 255, 140, 120, 160, 255, 160, 160,
            160, 255, 180, 180, 180, 255, 200, 200, 200, 255,
        ],
    );
    assert_eq!(
        out,
        vec![
            50, 55, 45, 255, 80, 84, 76, 255, 110, 113, 107, 255, 140, 103, 177, 255, 170, 173,
            167, 255, 200, 204, 196, 255, 230, 235, 225, 255,
        ]
    );
}

/// A negative texture amount must run the very same maths with a negative
/// `amount`: the DoG window is identical, only the sign of the mix changes.
#[test]
fn local_texture_negative_is_the_same_maths_with_the_opposite_sign() {
    let row: Vec<u8> = (0..7u8)
        .flat_map(|i| {
            let v = 80 + 20 * i;
            [v, v, v, 3]
        })
        .collect();
    let positive = render_row(&recipe_with_presence(0.5, 0.0, 0.0), row.clone());
    let negative = render_row(&recipe_with_presence(-0.5, 0.0, 0.0), row.clone());
    let plain = render_row(&LocalAdjustments::default(), row);
    for index in 0..7 {
        let offset = index * 4;
        let up = i32::from(positive[offset]) - i32::from(plain[offset]);
        let down = i32::from(negative[offset]) - i32::from(plain[offset]);
        assert_eq!(up, -down, "pixel {index}: the DoG detail is antisymmetric");
    }
    assert_ne!(positive, negative, "a signed amount is not a no-op");
}

// ----------------------------------------------------------------- clarity

/// Hand-derived exact golden for a local clarity ramp.
///
/// Nine greys `[70 … 150]` with `clarity = 0.25`, so the radius is
/// `8 + round(|0.25| * 24) = 14` — far wider than the nine-pixel row, so every
/// window is clipped to the whole row and the mean is the row average
/// `(70 + 150) / 2 = 110`. The result is `v + 0.25 * (v - 110)`:
///
/// `70 → 60`, `80 → 72.5`, `90 → 85`, `100 → 97.5`, `110 → 110`,
/// `120 → 122.5`, `130 → 135`, `140 → 147.5`, `150 → 160`.
///
/// That is the "clarity is a much broader neighbourhood than texture" property
/// made visible: the whole row participates in every pixel's detail, while the
/// texture radii only ever see 3..7 neighbours.
#[test]
fn local_clarity_has_an_exact_golden_and_preserves_alpha() {
    let recipe = recipe_with_presence(0.0, 0.25, 0.0);
    let out = render_row(&recipe, grey_row(70, 10, 9, 7));
    assert_eq!(
        out,
        vec![
            60, 60, 60, 7, 73, 73, 73, 7, 85, 85, 85, 7, 98, 98, 98, 7, 110, 110, 110, 7, 123, 123,
            123, 7, 135, 135, 135, 7, 147, 147, 147, 7, 160, 160, 160, 7,
        ]
    );
}

/// The texture and clarity radii are the documented `1 + round(|t|*2)` and
/// `8 + round(|c|*24)`, and they come from the *persisted amount alone* — never
/// from the mask, the frame size or any ROI.
#[test]
fn the_dog_radii_are_exactly_the_documented_formulas() {
    use crate::presence_stages::{clarity_radius, texture_radius};
    assert_eq!(texture_radius(0.0), 1);
    assert_eq!(texture_radius(0.25), 1 + 1);
    assert_eq!(texture_radius(0.5), 1 + 1);
    assert_eq!(texture_radius(1.0), 1 + 2);
    assert_eq!(texture_radius(-1.0), 1 + 2);
    assert_eq!(texture_radius(-0.6), 1 + 1);
    assert_eq!(clarity_radius(0.0), 8);
    assert_eq!(clarity_radius(0.25), 8 + 6);
    assert_eq!(clarity_radius(0.5), 8 + 12);
    assert_eq!(clarity_radius(1.0), 8 + 24);
    assert_eq!(clarity_radius(-1.0), 8 + 24);
    assert_eq!(clarity_radius(0.01), 8);
}

// ------------------------------------------------------------------ dehaze

/// Hand-derived exact golden for a positive local dehaze.
///
/// Nine greys `[80 … 160]` with `dehaze = 0.4`. The dark channel is
/// `min(R,G,B)` over a radius-2 window (a 5-wide window on this row), so
/// `dark = [80, 80, 80, 90, 100, 110, 120, 130, 140] / 255`. The airlight `A` is
/// the 95th percentile of that whole-frame channel, i.e. index
/// `floor(9 * 0.95) = 8` → `140/255 = 0.549020`, above the `0.05` floor. With
/// `dehaze > 0`, `t = 1 - 0.4 * (1 - base_t)` and `out = (x - A) / t + A`.
#[test]
fn local_dehaze_positive_has_an_exact_golden() {
    let recipe = recipe_with_presence(0.0, 0.0, 0.4);
    let out = render_row(&recipe, grey_row(80, 10, 9, 255));
    assert_eq!(
        out,
        vec![
            63, 63, 63, 255, 76, 76, 76, 255, 89, 89, 89, 255, 100, 100, 100, 255, 113, 113, 113,
            255, 126, 126, 126, 255, 140, 140, 140, 255, 155, 155, 155, 255, 172, 172, 172, 255,
        ]
    );
}

/// The negative direction adds haze at half strength (`t = 1 + |amount| * 0.5 *
/// (1 - base_t)`) and is bounded: the first pixel still moves *towards* the
/// airlight, never past the frame.
#[test]
fn local_dehaze_negative_has_an_exact_golden() {
    let recipe = recipe_with_presence(0.0, 0.0, -0.6);
    let out = render_row(&recipe, grey_row(80, 10, 9, 255));
    assert_eq!(
        out,
        vec![
            88, 88, 88, 255, 97, 97, 97, 255, 106, 106, 106, 255, 115, 115, 115, 255, 123, 123,
            123, 255, 132, 132, 132, 255, 140, 140, 140, 255, 148, 148, 148, 255, 156, 156, 156,
            255,
        ]
    );
}

// --------------------------------------------------------------- full stack

/// The exact full-stack golden: local relative WB, local Basic, local presence,
/// the local tone curve and the local colour stages, all on one un-quantized
/// chain with a single RGBA8 write at the end.
///
/// Layer: `exposure 0.25`, `contrast 0.2`, `shadows 0.3`, `highlights -0.3`,
/// `temperature_delta_k 1500`, `tint_delta -0.2`, `texture 0.35`,
/// `clarity 0.2`, `dehaze 0.3`, the lifted master curve, `vibrance 0.3` and
/// `saturation -0.2`. None of the five probe pixels clips.
#[test]
fn the_full_local_presence_stack_has_an_exact_golden() {
    let mut curves = Curves::identity();
    curves.master = lifted_master();
    let mut recipe = LocalAdjustments {
        exposure: 0.25,
        contrast: 0.2,
        shadows: 0.3,
        highlights: -0.3,
        temperature_delta_k: 1500.0,
        tint_delta: -0.2,
        curves: Some(curves),
        vibrance: 0.3,
        saturation: -0.2,
        ..LocalAdjustments::default()
    };
    recipe
        .set_local_presence_field("texture", 0.35)
        .expect("texture");
    recipe
        .set_local_presence_field("clarity", 0.2)
        .expect("clarity");
    recipe
        .set_local_presence_field("dehaze", 0.3)
        .expect("dehaze");
    let out = render_row(
        &recipe,
        vec![
            100, 120, 140, 77, 180, 160, 90, 33, 70, 90, 200, 200, 128, 128, 128, 5, 200, 60, 160,
            128,
        ],
    );
    assert_eq!(
        out,
        vec![
            145, 232, 243, 77, 239, 239, 114, 33, 53, 165, 233, 200, 191, 248, 232, 5, 236, 80,
            236, 128,
        ]
    );
}

/// The same layer **without** the presence block keeps its exact P1.2b bytes,
/// and a persisted all-zero presence block is byte-identical to no block at
/// all. This is the content-based path selection in one test.
#[test]
fn no_local_presence_keeps_the_p0_p11_p12a_p12b_bytes() {
    let mut curves = Curves::identity();
    curves.master = lifted_master();
    let mut without = LocalAdjustments {
        exposure: 0.25,
        contrast: 0.2,
        shadows: 0.3,
        highlights: -0.3,
        temperature_delta_k: 1500.0,
        tint_delta: -0.2,
        curves: Some(curves),
        vibrance: 0.3,
        saturation: -0.2,
        ..LocalAdjustments::default()
    };
    let pixels = vec![
        100u8, 120, 140, 77, 180, 160, 90, 33, 70, 90, 200, 200, 128, 128, 128, 5, 200, 60, 160,
        128,
    ];
    let baseline = render_row(&without, pixels.clone());
    assert_eq!(
        baseline,
        vec![
            151, 202, 241, 77, 221, 225, 141, 33, 116, 161, 240, 200, 179, 210, 222, 5, 239, 114,
            239, 128,
        ],
        "the presence-free layer must keep its exact P1.2b bytes"
    );
    // A persisted all-zero presence block is byte-identical to no block at all.
    let mut zero_block = without.clone();
    zero_block.presence = Some(Presence {
        version: 1,
        texture: 0.0,
        clarity: 0.0,
        dehaze: 0.0,
    });
    assert_eq!(
        render_row(&zero_block, pixels.clone()),
        baseline,
        "a persisted all-zero presence block must take the pre-P1.2c path"
    );
    // And one non-neutral field really does take the new kernel.
    without
        .set_local_presence_field("texture", 0.35)
        .expect("texture");
    assert_ne!(
        render_row(&without, pixels),
        baseline,
        "a non-neutral presence block must not be a silent no-op"
    );
}

/// A single-pixel frame is still a legal input: the DoG window is clipped to
/// the single pixel, so texture and clarity are their documented identity, and
/// the dehaze airlight is the pixel's own dark channel.
#[test]
fn a_single_pixel_frame_sees_the_documented_clipped_neighbourhood() {
    let texture_only = recipe_with_presence(1.0, 0.0, 0.0);
    assert_eq!(
        render_single(&texture_only, [137, 61, 203, 44]),
        vec![137, 61, 203, 44],
        "a 1x1 window has zero detail, so texture is identity"
    );
    let clarity_only = recipe_with_presence(0.0, 1.0, 0.0);
    assert_eq!(
        render_single(&clarity_only, [137, 61, 203, 44]),
        vec![137, 61, 203, 44],
        "the clarity window is clipped to the same single pixel"
    );
    // Dehaze on a 1x1 frame: `A` is the pixel's own dark channel, so `x - A`
    // is zero and the transmission term cannot move the value.
    let dehaze = recipe_with_presence(0.0, 0.0, 1.0);
    assert_eq!(
        render_single(&dehaze, [100, 100, 100, 44]),
        vec![100, 100, 100, 44]
    );
}
