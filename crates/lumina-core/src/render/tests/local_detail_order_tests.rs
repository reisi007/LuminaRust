//! MASK-LOCAL-P1.2d: the **within-layer** order proofs.
//!
//! Split out of `local_detail_reference.rs` (file-size ratchet). This half owns
//! the two tests that consume the independently transcribed chains of that file:
//! the within-layer order of the colour block against the two detail stages, and
//! the single quantization boundary of the whole layer.
//!
//! The frame is a 2-D **hue sweep**, not a grey ramp, and that is the whole
//! point: an HSL luminance shift puts neighbouring pixels in different bands, so
//! the bilateral weights, the separable Gaussian and the whole-frame gradient
//! maximum downstream see a genuinely different neighbourhood depending on
//! whether the colour block ran before or after them.

use super::local_detail_reference::{independent_chains, independent_chains_from, l1};
use crate::ImageFrame;
use lumina_sidecar::{
    CurveChannels, CurvePoint, Curves, Detail, HslAdjustments, HslChannel, LocalAdjustments,
    NoiseReduction, PointColor, PointColorEntry, Presence, Sharpening,
};

/// A 2-D frame that sweeps **hue** at mid lightness with a luma ramp down the
/// rows. A hue sweep is what makes the band selection spatially *structured*, and
/// therefore what makes "colour before the neighbourhood stages" observable: an
/// HSL luminance shift puts neighbouring pixels in different bands, so the
/// bilateral weights, the Gaussian and the gradient maximum downstream all see a
/// different neighbourhood than they would before the colour block. Values stay
/// well inside `0..=255` so nothing converges through clamping.
fn frame(width: usize, height: usize) -> Vec<u8> {
    (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                let hue = 360.0 * x as f32 / width as f32;
                let value = 0.42 + 0.06 * y as f32 / height.max(1) as f32;
                let [r, g, b] = hsv_to_rgb(hue, 0.7, value);
                [r, g, b, 200]
            })
        })
        .collect()
}

/// The textbook HSV→RGB conversion, transcribed (the test needs no `hsv`
/// dependency and the frame must be reproducible from this file alone).
fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> [u8; 3] {
    let sector = hue / 60.0;
    let index = (sector.floor() as u32) % 6;
    let f = sector - sector.floor();
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * f);
    let t = value * (1.0 - saturation * (1.0 - f));
    let rgb = match index {
        0 => [value, t, p],
        1 => [q, value, p],
        2 => [p, value, t],
        3 => [p, q, value],
        4 => [t, p, value],
        _ => [value, p, q],
    };
    [
        (rgb[0] * 255.0).round() as u8,
        (rgb[1] * 255.0).round() as u8,
        (rgb[2] * 255.0).round() as u8,
    ]
}

/// Render one layer carrying `recipe`, straight through the public local entry
/// point. No mask, no ROI, no scale: the point is the layer content, not the
/// compositing.
fn render(recipe: &LocalAdjustments, pixels: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut image = ImageFrame::new(width as u32, height as u32, pixels.to_vec()).unwrap();
    image
        .apply_mask_local_recipe_with_scale(recipe, 1.0)
        .expect("a valid local recipe");
    image.pixels
}

/// A lifted master curve, a two-band HSL shift and a colour-grading tint: colour
/// stages that visibly move the luminance, which is exactly what the bilateral
/// weights, the Gaussian and the gradient maximum downstream read.
fn colour_blocks() -> (Option<Curves>, Option<HslAdjustments>, Option<PointColor>) {
    (
        Some(Curves {
            version: 1,
            master: vec![
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
            ],
            channels: CurveChannels::default(),
        }),
        Some(HslAdjustments {
            version: 1,
            red: Some(HslChannel {
                hue: 0.3,
                saturation: 0.8,
                luminance: 0.6,
            }),
            green: Some(HslChannel {
                hue: -0.2,
                saturation: 0.7,
                luminance: -0.5,
            }),
            ..Default::default()
        }),
        Some(PointColor {
            version: 1,
            entries: vec![PointColorEntry {
                id: "blue".into(),
                hue_center: 210.0,
                hue_range: 60.0,
                hue_shift: 0.8,
                saturation_shift: 0.5,
                luminance_shift: 0.4,
            }],
        }),
    )
}

/// The non-neutral detail block every chain below shares.
fn detail_block() -> Detail {
    Detail {
        sharpening: Some(Sharpening {
            version: 1,
            amount: 2.0,
            radius: 1.0,
            detail: 1.0,
            masking: 0.0,
        }),
        noise_reduction: Some(NoiseReduction {
            version: 1,
            luminance: 1.0,
            color: 0.5,
        }),
    }
}

/// The scalar prefix every chain below shares: non-zero, so the WB and Basic
/// stages do real work and neither order can agree by accident.
fn scalars() -> LocalAdjustments {
    LocalAdjustments {
        exposure: 0.0,
        contrast: 0.15,
        shadows: 0.2,
        highlights: 0.1,
        temperature_delta_k: 800.0,
        tint_delta: -0.1,
        vibrance: 0.3,
        saturation: 0.2,
        ..LocalAdjustments::default()
    }
}

/// The **within-layer** order of the local detail block: the curve and the whole
/// colour block run **before** noise reduction and sharpening, in **one** layer
/// that carries both blocks at once.
///
/// Two separate layers — the P0 layer order — can never see this: that is what
/// `local_detail_sits_after_the_colour_block` pins, and it is a different claim.
/// So this test uses the verifier's method:
///
/// 1. transcribe the documented chain twice, once in the documented order and
///    once with the colour block swapped behind the detail stages,
/// 2. validate **each transcription in isolation** against the real kernel — the
///    colour half against a layer that carries *only* colour, the detail half
///    against a layer that carries *only* detail — so neither half can drift
///    silently and still be used as evidence, and
/// 3. assert that the one layer carrying both blocks renders the **documented**
///    chain and differs from the swapped one by a non-trivial margin, which is
///    what makes this test fail if the within-layer order is flipped.
#[test]
fn the_within_layer_order_runs_the_colour_block_before_the_detail_stages() {
    let (width, height) = (24usize, 3usize);
    let pixels = frame(width, height);
    let (curves, hsl, point_color) = colour_blocks();
    let mut colour_recipe = scalars();
    colour_recipe.curves = curves.clone();
    colour_recipe.hsl = hsl.clone();
    colour_recipe.point_color = point_color.clone();
    let mut detail_recipe = scalars();
    detail_recipe.detail = Some(detail_block());
    let mut both_recipe = detail_recipe.clone();
    both_recipe.curves = curves;
    both_recipe.hsl = hsl;
    both_recipe.point_color = point_color;

    // (1) The colour half of the transcription, validated in isolation.
    let colour_chains = independent_chains(&colour_recipe, &pixels, width, height, 1.0);
    assert_eq!(
        render(&colour_recipe, &pixels, width, height),
        colour_chains.colour_only,
        "the transcribed colour half must match the real kernel on its own"
    );
    assert_ne!(
        colour_chains.colour_only, pixels,
        "the colour half must actually change the pixels"
    );
    // (2) The detail half of the transcription, validated in isolation.
    let detail_chains = independent_chains(&detail_recipe, &pixels, width, height, 1.0);
    assert_eq!(
        render(&detail_recipe, &pixels, width, height),
        detail_chains.documented,
        "the transcribed detail half must match the real kernel on its own"
    );
    assert_ne!(
        render(&detail_recipe, &pixels, width, height),
        pixels,
        "the detail half must actually change the pixels"
    );

    // (3) One layer, both blocks: the documented order wins and the swapped one
    // loses by a wide margin.
    let rendered = render(&both_recipe, &pixels, width, height);
    let chains = independent_chains(&both_recipe, &pixels, width, height, 1.0);
    assert_eq!(
        rendered, chains.documented,
        "the within-layer order must be curve/colour BEFORE noise reduction and sharpening"
    );
    let margin = l1(&chains.documented, &chains.swapped);
    assert!(
        margin > 1024,
        "the two orders must be observably different, otherwise this test cannot fail when the \
         within-layer order is flipped (L1 distance was only {margin})"
    );
    assert_ne!(
        rendered, chains.swapped,
        "the render must not be the swapped order"
    );
    // And in that same order the layer still owns a single quantization
    // boundary: a second, forbidden one between the colour block and the noise
    // reduction is visible.
    assert_ne!(
        chains.early, chains.documented,
        "an intermediate quantization must be observable, or the single boundary is untested"
    );
}

/// The whole local layer owns **exactly one** RGBA8 quantization, in the
/// documented order.
///
/// The expected bytes come from the independent whole-plane re-derivation in
/// `local_detail_reference.rs`: the WB + Basic prefix transcribed in `f64`, the
/// F-096 bilateral kernel and the F-095 separable Gaussian/detail mix/flat-area
/// factor transcribed from the documented formulas (never from
/// `crate::detail_stages`), and the shared per-pixel colour stage. It proves two
/// things at once:
///
/// 1. the render equals the fully-float chain rounded once, and
/// 2. that result **differs** from a chain that rounds between the colour block
///    and the detail stages — so "one boundary" is an observable property, not a
///    comment.
#[test]
fn the_local_detail_layer_owns_exactly_one_quantization_boundary() {
    let (width, height) = (24usize, 3usize);
    let pixels = frame(width, height);
    let mut recipe = scalars();
    recipe.saturation = -0.5;
    recipe.detail = Some(Detail {
        sharpening: Some(Sharpening {
            version: 1,
            amount: 1.0,
            radius: 2.0,
            detail: 0.5,
            masking: 0.5,
        }),
        noise_reduction: Some(NoiseReduction {
            version: 1,
            luminance: 0.4,
            color: 0.3,
        }),
    });
    let rendered = render(&recipe, &pixels, width, height);
    let chains = independent_chains(&recipe, &pixels, width, height, 1.0);
    assert_eq!(
        rendered, chains.documented,
        "the local layer must round exactly once, at the very end"
    );
    assert_ne!(
        chains.early, chains.documented,
        "an intermediate quantization must be observable, or the single boundary is untested"
    );
    // The image alpha byte is the input's, throughout.
    assert!(rendered
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| pixel[3] == 200));
}

/// The whole P1.2d stack — WB + Basic + **Presence** + curve + colour + noise
/// reduction + sharpening — equals the independently transcribed chain, which is
/// what validates the exact bytes of `GOLDEN_FULL_STACK`.
///
/// The *presence* stage is the one link this file does not re-transcribe: it is
/// the shared global stage (`crate::presence_stages`), already pinned by the
/// P1.2c goldens, so the test hands the reference the **shared** prefix plane
/// instead of writing a third copy of that arithmetic. Everything downstream of
/// it — the curve, the four colour stages, the bilateral kernel, the Gaussian,
/// the detail mix, the flat-area factor and the single final round — is the
/// independent transcription, so the assertion pins the *composition order* of
/// the full stack, which is exactly what the P1.2d order fix is about.
#[test]
fn the_full_stack_matches_the_independently_transcribed_chain() {
    let (width, height) = (24usize, 3usize);
    let pixels = frame(width, height);
    let presence = Presence {
        version: 1,
        texture: 0.5,
        clarity: 0.2,
        dehaze: 0.1,
    };
    let (curves, hsl, point_color) = colour_blocks();
    let mut recipe = scalars();
    recipe.presence = Some(presence);
    recipe.curves = curves;
    recipe.hsl = hsl;
    recipe.point_color = point_color;
    recipe.detail = Some(detail_block());
    // The shared prefix: WB + Basic in float, then the shared presence stage.
    let mut prefix = super::super::local_presence::presence_plane(&pixels, &recipe);
    super::super::local_presence::apply_local_presence(&mut prefix, width, height, &presence);
    let chains = independent_chains_from(&recipe, &pixels, width, height, 1.0, prefix);
    let rendered = render(&recipe, &pixels, width, height);
    assert_eq!(
        rendered, chains.documented,
        "the full stack must be the documented chain on the shared presence prefix"
    );
    assert_ne!(
        chains.swapped, chains.documented,
        "and the swapped order must differ, or this composition is untested"
    );
}
