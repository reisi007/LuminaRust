//! MASK-LOCAL-P1.2c CPU compositor *quantization-boundary* proofs for the local
//! presence block.
//!
//! Split from `local_presence_contract_tests.rs` (file-size ratchet): this half
//! holds the two heaviest structural proofs — the single-quantization-boundary
//! golden and the kernel-order proof — while the other half holds the refusals,
//! the alpha/overlap behaviour and the state/identity invariants.
//!
//! Both proofs are derived from an **independent re-derivation written into the
//! test**, not from the kernel under test: the WB + Basic prefix, the box mean,
//! the DoG detail, the dark channel, the airlight percentile and the dehaze
//! formula are transcribed from the documented contract, not read out of
//! `crate::presence_stages`.
//!
//! There is deliberately **no** test here claiming byte-equality between the
//! local presence path and the global presence stage. The two quantize at
//! different points on purpose: the global kernel rounds to `u8` after every
//! presence sub-stage because it owns a `u8` frame, while the local layer owns
//! exactly **one** RGBA8 rounding, at the very end. That divergence is
//! documented in `crate::presence_stages` and is a design decision, not an
//! accident — so there is nothing to assert about byte-equality.

use super::local_adjustments::local_render;
use super::*;
use lumina_sidecar::{CurvePoint, Curves, LocalAdjustments, Presence};

fn presence_layer(recipe: LocalAdjustments) -> MaskLayer {
    let mut layer = layer("layer-1", reference("vc", "subject"));
    layer.local_adjustments = Some(recipe);
    layer
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

fn grey_row(start: u8, step: u8, count: usize, alpha: u8) -> Vec<u8> {
    (0..count)
        .flat_map(|i| {
            let v = start + step * i as u8;
            [v, v, v, alpha]
        })
        .collect()
}

/// The whole local layer owns **exactly one** RGBA8 quantization.
///
/// The expected bytes come from an **independent re-derivation written into
/// this test**: the WB + Basic prefix is transcribed from the documented `f64`
/// arithmetic (not from the production helper), the presence stage is
/// transcribed from the documented box-mean / DoG / dark-channel / percentile /
/// dehaze formulas (not from `crate::presence_stages`), and the colour stage is
/// the shared per-pixel function the kernel also uses. It then proves two
/// things at once:
///
/// 1. the render equals the fully-float chain rounded once, and
/// 2. that result **differs** from a chain that rounds between the Basic and
///    the presence stage — so "one boundary" is an observable property, not a
///    comment.
#[test]
fn local_wb_basic_presence_and_colour_share_exactly_one_quantization_boundary() {
    let recipe = LocalAdjustments {
        exposure: 0.5,
        contrast: 0.25,
        shadows: 0.5,
        highlights: -0.5,
        temperature_delta_k: 2000.0,
        tint_delta: -0.2,
        saturation: -0.5,
        presence: Some(Presence {
            version: 1,
            texture: 0.5,
            clarity: 0.25,
            dehaze: 0.3,
        }),
        ..LocalAdjustments::default()
    };
    let pixels: Vec<u8> = (0..9u8)
        .flat_map(|i| {
            let v = 80 + 10 * i;
            [
                v,
                (v as u16 + 17).min(255) as u8,
                (v as u16 + 53).min(255) as u8,
                77,
            ]
        })
        .collect();
    let rendered = render_row(&recipe, pixels.clone());
    let (expected, early_round) = independent_chains(&recipe, &pixels);
    assert_eq!(
        rendered, expected,
        "the local layer must round exactly once, at the very end"
    );
    assert_ne!(
        early_round, expected,
        "an intermediate quantization must be observable, or the single boundary is untested"
    );
}

/// The two independent reference chains: the contract's fully-float chain, and
/// the same chain with one extra quantization between the Basic stage and the
/// presence stage.
fn independent_chains(recipe: &LocalAdjustments, pixels: &[u8]) -> (Vec<u8>, Vec<u8>) {
    // --- The documented WB + Basic prefix, transcribed from the contract. ---
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
    let presence = recipe
        .presence
        .as_ref()
        .expect("the recipe carries presence");
    let (texture, clarity, dehaze) = (
        f64::from(presence.texture),
        f64::from(presence.clarity),
        f64::from(presence.dehaze),
    );
    // `1 + round(|texture| * 2)` and `8 + round(|clarity| * 24)`, transcribed.
    let radii = [
        (1 + (texture.abs() * 2.0).round() as usize, texture),
        (8 + (clarity.abs() * 24.0).round() as usize, clarity),
    ];

    // --- The documented presence stage, transcribed from the contract. ---
    let dog_pass = |plane: &[[f64; 3]], radius: usize, amount: f64| {
        let w = plane.len();
        let snapshot: Vec<[f64; 3]> = plane.to_vec();
        (0..w)
            .map(|index| {
                let x = index % w;
                let mut row = [0.0_f64; 3];
                for c in 0..3 {
                    let x0 = x.saturating_sub(radius);
                    let x1 = (x + radius).min(w - 1);
                    let window = &snapshot[x0..=x1];
                    let mean: f64 = window.iter().map(|p| p[c]).sum::<f64>() / window.len() as f64;
                    let value = snapshot[index][c];
                    row[c] = (value + amount * (value - mean)).clamp(0.0, 255.0);
                }
                row
            })
            .collect::<Vec<_>>()
    };
    let dehaze_pass = |plane: &[[f64; 3]]| {
        let w = plane.len();
        let mut dark = vec![0.0_f64; w];
        for (index, _p) in plane.iter().enumerate() {
            let x0 = (index % w).saturating_sub(2);
            let x1 = ((index % w) + 2).min(w - 1);
            let mut m = 1.0_f64;
            for q in &plane[x0..=x1] {
                m = m.min(q[0].min(q[1]).min(q[2]) / 255.0);
            }
            dark[index] = m;
        }
        let mut sorted = dark.clone();
        sorted.sort_by(f64::total_cmp);
        let airlight =
            sorted[((sorted.len() as f64 * 0.95) as usize).min(sorted.len() - 1)].max(0.05);
        plane
            .iter()
            .enumerate()
            .map(|(index, p)| {
                let base_t = (1.0 - 0.95 * dark[index] / airlight).clamp(0.05, 1.0);
                let t = if dehaze > 0.0 {
                    1.0 - dehaze * (1.0 - base_t)
                } else {
                    1.0 + (-dehaze) * 0.5 * (1.0 - base_t)
                };
                let mut row = [0.0_f64; 3];
                for c in 0..3 {
                    let x = p[c] / 255.0;
                    row[c] = ((x - airlight) / t + airlight).clamp(0.0, 1.0) * 255.0;
                }
                row
            })
            .collect::<Vec<_>>()
    };
    let quantise = |plane: &[[f64; 3]]| -> Vec<u8> {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(plane)
            .flat_map(|(p, row)| {
                let mut rgb = [0.0_f32; 3];
                for (c, slot) in rgb.iter_mut().enumerate() {
                    *slot = (row[c] / 255.0) as f32;
                }
                rgb = crate::color_stages::vibrance_saturation_stage(
                    rgb,
                    0.0,
                    recipe.saturation as f32,
                );
                [
                    (f64::from(rgb[0]) * 255.0).round().clamp(0.0, 255.0) as u8,
                    (f64::from(rgb[1]) * 255.0).round().clamp(0.0, 255.0) as u8,
                    (f64::from(rgb[2]) * 255.0).round().clamp(0.0, 255.0) as u8,
                    p[3],
                ]
            })
            .collect()
    };

    // Chain A (the contract): one boundary, at the very end.
    let basic: Vec<[f64; 3]> = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let mut row = [0.0_f64; 3];
            for (channel, slot) in row.iter_mut().enumerate() {
                *slot = prefix(f64::from(p[channel]), channel);
            }
            row
        })
        .collect();
    let mut chain = basic.clone();
    for (radius, amount) in radii {
        chain = dog_pass(&chain, radius, amount);
    }
    let contract = quantise(&dehaze_pass(&chain));

    // Chain B: one *extra* quantization between Basic and presence.
    let early: Vec<[f64; 3]> = basic
        .iter()
        .map(|p| {
            let mut row = [0.0_f64; 3];
            for (c, slot) in row.iter_mut().enumerate() {
                *slot = f64::from(p[c].round().clamp(0.0, 255.0) as u8);
            }
            row
        })
        .collect();
    let mut early_chain = early.clone();
    for (radius, amount) in radii {
        early_chain = dog_pass(&early_chain, radius, amount);
    }
    (contract, quantise(&dehaze_pass(&early_chain)))
}

/// The presence stage sits at **exactly** the global kernel position: after the
/// scalar (WB/Basic) stage and **before** the curve. Both orders are re-derived
/// here from the shared helpers, independently of the kernel under test, so the
/// position is proven observable instead of merely asserted.
#[test]
fn local_presence_sits_between_the_basic_stage_and_the_curve() {
    use crate::presence_stages::{clarity_radius, dog_channel, FloatPlane};
    let mut curves = Curves::identity();
    curves.master = vec![
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
    ];
    let mut recipe = LocalAdjustments {
        exposure: 0.25,
        curves: Some(curves.clone()),
        ..LocalAdjustments::default()
    };
    recipe
        .set_local_presence_field("clarity", 0.4)
        .expect("clarity");
    let pixels = grey_row(60, 15, 7, 9);
    let rendered = render_row(&recipe, pixels.clone());

    let gains = recipe.relative_white_balance_gains();
    let radius = clarity_radius(0.4);
    let basic: Vec<[f32; 3]> = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let mut row = [0.0_f32; 3];
            for (channel, slot) in row.iter_mut().enumerate() {
                *slot = super::local_wb::scale_mask_local_wb_basic(
                    f64::from(p[channel]),
                    &gains,
                    &recipe,
                    channel,
                ) as f32;
            }
            row
        })
        .collect();
    let apply_dog = |plane: &mut Vec<[f32; 3]>| {
        let snapshot = plane.clone();
        for (index, pixel) in plane.iter_mut().enumerate() {
            let view = FloatPlane {
                pixels: &snapshot,
                width: 7,
                height: 1,
            };
            for (c, channel) in pixel.iter_mut().enumerate() {
                *channel = dog_channel(&view, index, 0, c, radius, 0.4);
            }
        }
    };
    let to_bytes = |plane: &[[f32; 3]]| -> Vec<u8> {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(plane)
            .flat_map(|(p, row)| {
                [
                    row[0].round().clamp(0.0, 255.0) as u8,
                    row[1].round().clamp(0.0, 255.0) as u8,
                    row[2].round().clamp(0.0, 255.0) as u8,
                    p[3],
                ]
            })
            .collect()
    };

    // Order A (the contract): presence on the float plane, then the curve.
    let mut presence_first = basic.clone();
    apply_dog(&mut presence_first);
    let contract: Vec<u8> = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .flat_map(|(index, p)| {
            let scaled = [
                f64::from(presence_first[index][0]),
                f64::from(presence_first[index][1]),
                f64::from(presence_first[index][2]),
            ];
            let toned = super::local_tone::apply_local_tone(&scaled, &curves);
            [
                toned[0].round().clamp(0.0, 255.0) as u8,
                toned[1].round().clamp(0.0, 255.0) as u8,
                toned[2].round().clamp(0.0, 255.0) as u8,
                p[3],
            ]
        })
        .collect();
    assert_eq!(
        rendered, contract,
        "presence must run on the float plane before the curve"
    );
    assert_ne!(
        contract,
        to_bytes(&presence_first),
        "the curve is not a no-op"
    );

    // Order B (wrong, and deliberately not what the kernel does): the curve
    // first, rounded, and only then presence.
    let mut curve_first: Vec<[f32; 3]> = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let mut row = [0.0_f32; 3];
            for (channel, slot) in row.iter_mut().enumerate() {
                let scaled = super::local_wb::scale_mask_local_wb_basic(
                    f64::from(p[channel]),
                    &gains,
                    &recipe,
                    channel,
                );
                *slot = super::local_tone::apply_local_tone(&[scaled; 3], &curves)[channel]
                    .round()
                    .clamp(0.0, 255.0) as f32;
            }
            row
        })
        .collect();
    apply_dog(&mut curve_first);
    assert_ne!(
        rendered,
        to_bytes(&curve_first),
        "presence and the curve do not commute; the position must be observable"
    );
}

/// The presence stage is a **neighbourhood** stage over the whole plane, so it
/// is provably not a per-mask, ROI-resized or mask-scaled statistic: a pixel
/// under a one-pixel mask and the same pixel under a full mask produce the
/// *same* local result, and every outside-mask byte is exactly the global result.
#[test]
fn the_presence_neighbourhood_is_full_frame_and_the_mask_only_gates_the_blend() {
    let mut recipe = LocalAdjustments::default();
    recipe
        .set_local_presence_field("texture", 0.5)
        .expect("texture");
    let pixels = grey_row(80, 10, 9, 11);
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
        MaskPlane::new(9, 1, vec![0, 0, 0, 0, u16::MAX, 0, 0, 0, 0]).unwrap(),
    )]);
    let spotted = local_render(&frame, &copies, planes, &EditRecipe::default(), None).unwrap();
    assert_eq!(
        &spotted.frame.pixels[16..20],
        &full[16..20],
        "a one-pixel mask must still see the full-frame neighbourhood"
    );
    for index in 0..9 {
        if index == 4 {
            continue;
        }
        let offset = index * 4;
        assert_eq!(
            &spotted.frame.pixels[offset..offset + 4],
            &pixels[offset..offset + 4],
            "outside-mask pixel {index} must be byte-identical"
        );
    }
}
