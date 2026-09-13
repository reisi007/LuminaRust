//! GPU-RENDER-PARITY-1 parity harness: the post-tone GPU stages (per-pixel
//! color + Presence) against the CPU oracle (`lumina_core::render_frame`).
//!
//! Each implemented stage gets a recipe that drives it at a non-neutral value;
//! the test asserts
//!
//! 1. [`unsupported_gpu_stages`] is **empty** (the stage is genuinely GPU-
//!    renderable, not silently CPU-routed), and
//! 2. the GPU output matches the CPU oracle at that stage's **strongest
//!    measured property** ([`Equivalence`]): stages measured byte-identical on
//!    both frames (Point Color, Color Grading, neutral vibrance/saturation,
//!    Presence Clarity and positive Dehaze) assert `maxAbsDiff == 0`; the rest
//!    assert their measured bound (≤ 1, or ≤ 2 for the fully stacked recipe)
//!    plus a structural PSNR floor and a **mean-signed-error bias bound** so the
//!    tolerance cannot hide a systematic tilt. The bound is not loosened to fit
//!    the data — the prose in `lib.rs` and here states exactly what is asserted.
//!
//! The residuals come from the oracle evaluating the curves ratio in `f64`
//! (the GPU is `f32`) and from the Metal backend rounding one ulp differently
//! at a `round()` tie.
//!
//! Without a bound adapter the hardware checks are skipped loudly, never
//! failed (matching `golden.rs`); the validator assertions still run.
#![cfg(feature = "gpu")]

use lumina_core::{render_frame, ImageFrame, RenderContext};
use lumina_gpu::{
    unsupported_gpu_stages, unsupported_gpu_stages_for, unsupported_gpu_stages_with_context,
    GpuContext, MAX_SOURCE_ACTIONS,
};
use lumina_sidecar::{
    BokehShape, ColorGrading, ColorGradingRange, CurveChannels, CurvePoint, Curves, EditRecipe,
    Effects, FocusRect, GenerativeCanvas, GenerativeEdit, Geometry, Grain, HslAdjustments,
    HslChannel, LensBlur, LensCorrection, NoiseReduction, Perspective, PointColor, PointColorEntry,
    Presence, RedEyeCorrection, RedEyeRegion, Sharpening, SourceActionArtifactRef,
    SourceActionKind, SourceActionSpec, SpotRemoval, SpotRemovalMode, Vignette,
    SOURCE_ACTION_VERSION,
};
use std::collections::BTreeMap;

const SKIP_MESSAGE: &str = "GPU adapter unavailable - skipped parity check";

/// The strongest CPU↔GPU equivalence property a recipe is asserted at.
#[derive(Clone, Copy, Debug)]
enum Equivalence {
    /// Measured byte-identical on both parity frames (`maxAbsDiff == 0`).
    ByteIdentical,
    /// Measured within this many 8-bit codes on both parity frames.
    Bounded(u8),
}

/// Structural PSNR floor for bounded stages (in addition to `maxAbsDiff`).
const MIN_PSNR_DB: f64 = 48.0;
/// Maximum absolute **mean signed** per-byte error for bounded stages: the
/// residual must be rounding noise, not a systematic brightness/colour tilt.
const MAX_ABS_MEAN_SIGNED_ERROR: f64 = 0.05;

fn gradient_frame(width: u32, height: u32) -> ImageFrame {
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let rx = x as f64 / (width as f64 - 1.0).max(1.0);
            let ry = y as f64 / (height as f64 - 1.0).max(1.0);
            pixels.extend_from_slice(&[
                (rx * 255.0).round() as u8,
                (ry * 255.0).round() as u8,
                (((rx + ry) * 0.5) * 255.0).round() as u8,
                255,
            ]);
        }
    }
    ImageFrame::new(width, height, pixels).expect("synthetic gradient frame")
}

fn noise_frame(width: u32, height: u32, seed: u64) -> ImageFrame {
    let mut state = seed;
    let mut next = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for _ in 0..(width * height) {
        pixels.extend_from_slice(&[
            (next() & 0xFF) as u8,
            (next() & 0xFF) as u8,
            (next() & 0xFF) as u8,
            255,
        ]);
    }
    ImageFrame::new(width, height, pixels).expect("synthetic noise frame")
}

fn max_abs_diff(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

fn psnr_db(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let mut mse = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let e = (*x as f64) - (*y as f64);
        mse += e * e;
    }
    let mse = mse / a.len() as f64;
    if mse == 0.0 {
        f64::INFINITY
    } else {
        10.0 * (255.0f64 * 255.0 / mse).log10()
    }
}

/// Mean signed per-byte error `mean(a - b)`. A tolerance that merely capped the
/// maximum would let a systematic brightness shift through; this metric makes
/// such a bias visible (both signs cancel only for zero-mean rounding noise).
fn mean_signed_error(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let sum: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| (*x as f64) - (*y as f64))
        .sum();
    sum / a.len() as f64
}

fn curve(points: &[(f32, f32)]) -> Vec<CurvePoint> {
    points
        .iter()
        .map(|&(input, output)| CurvePoint { input, output })
        .collect()
}

fn hsl(hue: f32, saturation: f32, luminance: f32) -> HslChannel {
    HslChannel {
        hue,
        saturation,
        luminance,
    }
}

fn entry(id: &str, center: f32, range: f32, hue: f32, sat: f32, lum: f32) -> PointColorEntry {
    PointColorEntry {
        id: id.into(),
        hue_center: center,
        hue_range: range,
        hue_shift: hue,
        saturation_shift: sat,
        luminance_shift: lum,
    }
}

fn range(hue: f32, saturation: f32, luminance: f32) -> ColorGradingRange {
    ColorGradingRange {
        hue_degrees: hue,
        saturation,
        luminance,
    }
}

/// The implemented stages, each represented by a non-neutral recipe.
fn supported_recipes() -> Vec<(&'static str, EditRecipe)> {
    vec![
        (
            "curves_s_curve",
            EditRecipe {
                curves: Some(Curves {
                    version: 1,
                    master: curve(&[(0.0, 0.0), (0.25, 0.18), (0.75, 0.85), (1.0, 1.0)]),
                    channels: CurveChannels {
                        red: Some(curve(&[(0.0, 0.0), (0.5, 0.6), (1.0, 1.0)])),
                        green: None,
                        blue: Some(curve(&[(0.0, 0.0), (0.5, 0.45), (1.0, 1.0)])),
                    },
                }),
                ..Default::default()
            },
        ),
        (
            "hsl_all_channels",
            EditRecipe {
                hsl: Some(HslAdjustments {
                    version: 1,
                    red: Some(hsl(0.2, 0.1, 0.05)),
                    orange: Some(hsl(-0.1, -0.2, 0.0)),
                    yellow: Some(hsl(0.0, 0.3, -0.1)),
                    green: Some(hsl(0.15, 0.0, 0.1)),
                    cyan: Some(hsl(-0.3, 0.2, 0.0)),
                    blue: Some(hsl(0.25, -0.15, 0.05)),
                    violet: Some(hsl(0.0, 0.1, -0.05)),
                    magenta: Some(hsl(-0.2, 0.25, 0.0)),
                }),
                ..Default::default()
            },
        ),
        (
            "point_color_two_entries",
            EditRecipe {
                point_color: Some(PointColor {
                    version: 1,
                    entries: vec![
                        entry("pc-1", 30.0, 40.0, 0.5, -0.3, 0.1),
                        entry("pc-2", 210.0, 90.0, -0.4, 0.2, -0.2),
                    ],
                }),
                ..Default::default()
            },
        ),
        (
            "vibrance_saturation",
            EditRecipe {
                adjustments: BTreeMap::from([
                    ("vibrance".into(), 0.45),
                    ("saturation".into(), -0.25),
                ]),
                ..Default::default()
            },
        ),
        (
            "vibrance_present_zero",
            EditRecipe {
                adjustments: BTreeMap::from([("vibrance".into(), 0.0), ("saturation".into(), 0.0)]),
                ..Default::default()
            },
        ),
        (
            "color_grading_full",
            EditRecipe {
                color_grading: Some(ColorGrading {
                    version: 1,
                    shadows: range(220.0, 0.4, -0.1),
                    midtones: range(40.0, 0.2, 0.05),
                    highlights: range(120.0, 0.3, 0.1),
                    balance: 0.2,
                    blending: 0.6,
                }),
                ..Default::default()
            },
        ),
        (
            "presence_texture",
            EditRecipe {
                presence: Some(Presence {
                    version: 1,
                    texture: 0.7,
                    clarity: 0.0,
                    dehaze: 0.0,
                }),
                ..Default::default()
            },
        ),
        (
            "presence_clarity",
            EditRecipe {
                presence: Some(Presence {
                    version: 1,
                    texture: 0.0,
                    clarity: 0.4,
                    dehaze: 0.0,
                }),
                ..Default::default()
            },
        ),
        (
            "presence_dehaze_positive",
            EditRecipe {
                presence: Some(Presence {
                    version: 1,
                    texture: 0.0,
                    clarity: 0.0,
                    dehaze: 0.6,
                }),
                ..Default::default()
            },
        ),
        (
            "presence_dehaze_negative",
            EditRecipe {
                presence: Some(Presence {
                    version: 1,
                    texture: 0.0,
                    clarity: 0.0,
                    dehaze: -0.4,
                }),
                ..Default::default()
            },
        ),
        (
            "presence_full",
            EditRecipe {
                presence: Some(Presence {
                    version: 1,
                    texture: 0.5,
                    clarity: 0.3,
                    dehaze: 0.5,
                }),
                ..Default::default()
            },
        ),
        (
            "presence_texture_clarity",
            EditRecipe {
                presence: Some(Presence {
                    version: 1,
                    texture: 0.4,
                    clarity: 0.2,
                    dehaze: 0.0,
                }),
                ..Default::default()
            },
        ),
        (
            "presence_texture_clarity_curves",
            EditRecipe {
                presence: Some(Presence {
                    version: 1,
                    texture: 0.4,
                    clarity: 0.2,
                    dehaze: 0.0,
                }),
                curves: Some(Curves {
                    version: 1,
                    master: curve(&[(0.0, 0.0), (0.5, 0.45), (1.0, 1.0)]),
                    channels: CurveChannels::default(),
                }),
                ..Default::default()
            },
        ),
        (
            "tone_curves",
            EditRecipe {
                adjustments: BTreeMap::from([
                    ("exposure".into(), 0.35),
                    ("contrast".into(), 0.15),
                    ("wb_temperature".into(), 5800.0),
                ]),
                curves: Some(Curves {
                    version: 1,
                    master: curve(&[(0.0, 0.0), (0.5, 0.45), (1.0, 1.0)]),
                    channels: CurveChannels::default(),
                }),
                ..Default::default()
            },
        ),
        (
            "combined_tone_color_presence",
            EditRecipe {
                adjustments: BTreeMap::from([
                    ("exposure".into(), 0.35),
                    ("contrast".into(), 0.15),
                    ("wb_temperature".into(), 5800.0),
                    ("vibrance".into(), 0.2),
                ]),
                presence: Some(Presence {
                    version: 1,
                    texture: 0.4,
                    clarity: 0.2,
                    dehaze: 0.0,
                }),
                curves: Some(Curves {
                    version: 1,
                    master: curve(&[(0.0, 0.0), (0.5, 0.45), (1.0, 1.0)]),
                    channels: CurveChannels::default(),
                }),
                hsl: Some(HslAdjustments {
                    version: 1,
                    blue: Some(hsl(0.2, 0.1, 0.0)),
                    ..Default::default()
                }),
                point_color: Some(PointColor {
                    version: 1,
                    entries: vec![entry("pc-1", 120.0, 60.0, 0.3, -0.1, 0.0)],
                }),
                color_grading: Some(ColorGrading {
                    version: 1,
                    shadows: range(200.0, 0.2, 0.0),
                    midtones: ColorGradingRange::neutral(),
                    highlights: range(60.0, 0.2, 0.05),
                    balance: 0.0,
                    blending: 0.5,
                }),
                ..Default::default()
            },
        ),
        // --- GPU-RENDER-PARITY-1 stage 2: detail stages ---
        (
            "effects_vignette",
            EditRecipe {
                effects: Some(Effects {
                    vignette: Some(Vignette {
                        version: 1,
                        amount: -0.5,
                        midpoint: 0.5,
                        roundness: 1.0,
                        feather: 0.5,
                    }),
                    grain: None,
                }),
                ..Default::default()
            },
        ),
        (
            "effects_grain",
            EditRecipe {
                effects: Some(Effects {
                    vignette: None,
                    grain: Some(Grain {
                        version: 1,
                        amount: 0.5,
                        size: 0.3,
                        roughness: 0.5,
                        seed: 0x0BAD_F00D_1234,
                    }),
                }),
                ..Default::default()
            },
        ),
        (
            "effects_vignette_grain",
            EditRecipe {
                effects: Some(Effects {
                    vignette: Some(Vignette {
                        version: 1,
                        amount: 0.4,
                        midpoint: 0.4,
                        roundness: 0.5,
                        feather: 0.7,
                    }),
                    grain: Some(Grain {
                        version: 1,
                        amount: 0.35,
                        size: 0.6,
                        roughness: 0.8,
                        seed: 7,
                    }),
                }),
                ..Default::default()
            },
        ),
        (
            "noise_reduction_luminance",
            EditRecipe {
                noise_reduction: Some(NoiseReduction {
                    version: 1,
                    luminance: 0.6,
                    color: 0.0,
                }),
                ..Default::default()
            },
        ),
        (
            "noise_reduction_color",
            EditRecipe {
                noise_reduction: Some(NoiseReduction {
                    version: 1,
                    luminance: 0.0,
                    color: 0.7,
                }),
                ..Default::default()
            },
        ),
        (
            "noise_reduction_full",
            EditRecipe {
                noise_reduction: Some(NoiseReduction {
                    version: 1,
                    luminance: 0.5,
                    color: 0.5,
                }),
                ..Default::default()
            },
        ),
        (
            "sharpening_unmasked",
            EditRecipe {
                sharpening: Some(Sharpening {
                    version: 1,
                    amount: 1.2,
                    radius: 1.0,
                    detail: 0.5,
                    masking: 0.0,
                }),
                ..Default::default()
            },
        ),
        (
            "sharpening_masked",
            EditRecipe {
                sharpening: Some(Sharpening {
                    version: 1,
                    amount: 1.5,
                    radius: 1.5,
                    detail: 0.7,
                    masking: 0.6,
                }),
                ..Default::default()
            },
        ),
        (
            "detail_stage_stack",
            EditRecipe {
                adjustments: BTreeMap::from([("exposure".into(), 0.2)]),
                noise_reduction: Some(NoiseReduction {
                    version: 1,
                    luminance: 0.4,
                    color: 0.3,
                }),
                sharpening: Some(Sharpening {
                    version: 1,
                    amount: 1.0,
                    radius: 1.2,
                    detail: 0.6,
                    masking: 0.4,
                }),
                effects: Some(Effects {
                    vignette: Some(Vignette {
                        version: 1,
                        amount: 0.3,
                        midpoint: 0.5,
                        roundness: 1.0,
                        feather: 0.5,
                    }),
                    grain: Some(Grain {
                        version: 1,
                        amount: 0.25,
                        size: 0.4,
                        roughness: 0.5,
                        seed: 42,
                    }),
                }),
                ..Default::default()
            },
        ),
        (
            // GPU-RENDER-PARITY-1 stage 3 (G-14): a red-eye correction rendered
            // by the dedicated pass (after Sharpening, before Effects).
            "red_eye_single_region",
            red_eye_recipe(),
        ),
        (
            // Red-eye stacked after Sharpening and before Effects — the exact
            // oracle slot.
            "red_eye_after_detail",
            EditRecipe {
                sharpening: Some(Sharpening {
                    version: 1,
                    amount: 1.0,
                    radius: 1.2,
                    detail: 0.5,
                    masking: 0.3,
                }),
                effects: Some(Effects {
                    vignette: Some(Vignette {
                        version: 1,
                        amount: 0.2,
                        midpoint: 0.5,
                        roundness: 1.0,
                        feather: 0.5,
                    }),
                    grain: None,
                }),
                ..red_eye_recipe()
            },
        ),
        (
            // GPU-RENDER-PARITY-1 stage-2 follow-up: the schema maximum radius
            // (10.0) with an active edge mask (`masking > 0`) exercises the
            // full 91-tap kernel and the readback gradient pass.
            "sharpening_radius_max_masked",
            EditRecipe {
                sharpening: Some(Sharpening {
                    version: 1,
                    amount: 1.0,
                    radius: 10.0,
                    detail: 0.5,
                    masking: 0.6,
                }),
                ..Default::default()
            },
        ),
    ]
}

/// The strongest CPU↔GPU equivalence **measured** for each recipe on this
/// backend (MAX-frames run: gradient 64² + noise 64²).
///
/// Every recipe must declare a bound — there is no catch-all, so adding a
/// recipe forces an explicit, reviewed decision instead of silently inheriting
/// a loose tolerance. The byte-identical group is asserted at `diff == 0`; the
/// bounded groups additionally require the PSNR floor and the mean-signed-error
/// bias bound.
fn equivalence_for(name: &str) -> Equivalence {
    match name {
        // Measured 0 on both frames.
        "point_color_two_entries"
        | "vibrance_present_zero"
        | "color_grading_full"
        | "presence_clarity"
        | "presence_dehaze_positive"
        | "presence_texture_clarity" => Equivalence::ByteIdentical,
        // Measured ≤ 1 on both frames (one rounding-tie code).
        "curves_s_curve"
        | "hsl_all_channels"
        | "vibrance_saturation"
        | "presence_texture"
        | "presence_dehaze_negative"
        | "presence_full"
        | "presence_texture_clarity_curves"
        | "tone_curves" => Equivalence::Bounded(1),
        // Stacks tone + Presence + every color stage; two ±1 codes coincide on
        // one pixel of the gradient frame.
        "combined_tone_color_presence" => Equivalence::Bounded(2),
        // --- GPU-RENDER-PARITY-1 stage 2: detail stages ---
        // Vignette and grain are `f32`-exact ports of the oracle and asserted
        // byte-identical on their own; stacking both can flip one `round()` tie
        // in the grain delta (GPU FMA vs. the oracle's separate multiply/add),
        // so the combined recipe is bounded at one code. Noise Reduction /
        // Sharpening carry the same `exp`/FMA residual.
        "effects_vignette" | "effects_grain" => Equivalence::ByteIdentical,
        "effects_vignette_grain" => Equivalence::Bounded(1),
        "noise_reduction_luminance"
        | "noise_reduction_color"
        | "noise_reduction_full"
        | "sharpening_unmasked"
        | "sharpening_masked" => Equivalence::Bounded(1),
        "detail_stage_stack" => Equivalence::Bounded(2),
        // GPU-RENDER-PARITY-1 stage 3 (G-14): the red-eye pass is a direct port
        // (per-pixel + region-local); `hypot`-vs-`sqrt` rounding at the disc
        // edge can flip a single code. The stacked recipe adds the detail
        // residual.
        "red_eye_single_region" => Equivalence::Bounded(1),
        "red_eye_after_detail" => Equivalence::Bounded(2),
        // Stage-2 follow-up: maximum-radius sharpening is the same kernel as
        // `sharpening_masked`, just wider (still one rounding-tie code).
        "sharpening_radius_max_masked" => Equivalence::Bounded(1),
        other => panic!("no measured equivalence bound declared for recipe `{other}`"),
    }
}

#[test]
fn tone_only_is_byte_identical() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let frame = gradient_frame(64, 64);
    for (name, recipe) in [
        ("default", EditRecipe::default()),
        (
            "exposure",
            EditRecipe {
                adjustments: BTreeMap::from([("exposure".into(), 0.5)]),
                ..Default::default()
            },
        ),
    ] {
        let cpu = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: None,
                depth: None,
            },
        )
        .expect("CPU oracle render")
        .frame;
        let gpu = ctx.render_with_gpu(&frame, &recipe).expect("GPU render");
        let diff = max_abs_diff(&cpu.pixels, &gpu.pixels);
        eprintln!("tone[{name}]: maxAbsDiff={diff}");
        assert_eq!(
            diff, 0,
            "tone-only render must stay byte-identical ({name})"
        );
    }
}

#[test]
fn implemented_stages_are_not_flagged() {
    for (name, recipe) in supported_recipes() {
        let reasons = unsupported_gpu_stages(&recipe);
        assert!(
            reasons.is_empty(),
            "{name} must be fully GPU-supported, got {reasons:?}"
        );
    }
}

#[test]
fn implemented_stages_match_cpu_oracle() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped parity check");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }

    let frames: Vec<(&'static str, ImageFrame)> = vec![
        ("gradient_64x64", gradient_frame(64, 64)),
        ("noise_64x64", noise_frame(64, 64, 0x0BAD_F00D_1234_5678)),
    ];

    let mut failures: Vec<String> = Vec::new();
    for (frame_name, frame) in &frames {
        for (recipe_name, recipe) in supported_recipes() {
            let cpu = render_frame(
                frame,
                &RenderContext {
                    recipe: &recipe,
                    camera_white_balance: None,
                    source_actions: &[],
                    masks: None,
                    lensfun: None,
                    depth: None,
                },
            )
            .expect("CPU oracle render")
            .frame;
            let gpu = ctx
                .render_with_gpu(frame, &recipe)
                .unwrap_or_else(|error| panic!("{frame_name}/{recipe_name}: GPU render: {error}"));
            let diff = max_abs_diff(&cpu.pixels, &gpu.pixels);
            let psnr = psnr_db(&cpu.pixels, &gpu.pixels);
            let bias = mean_signed_error(&cpu.pixels, &gpu.pixels);
            eprintln!(
                "parity[{frame_name}/{recipe_name}]: maxAbsDiff={diff} psnr={psnr:.2} dB \
                 meanSignedErr={bias:+.4}"
            );
            match equivalence_for(recipe_name) {
                Equivalence::ByteIdentical => {
                    if diff != 0 {
                        failures.push(format!(
                            "{frame_name}/{recipe_name}: asserted byte-identical, got \
                             maxAbsDiff={diff} psnr={psnr:.2} meanSignedErr={bias:+.4}"
                        ));
                    }
                }
                Equivalence::Bounded(bound) => {
                    if diff > bound || psnr < MIN_PSNR_DB || bias.abs() > MAX_ABS_MEAN_SIGNED_ERROR
                    {
                        failures.push(format!(
                            "{frame_name}/{recipe_name}: exceeded declared bound \
                             maxAbsDiff <= {bound} / PSNR >= {MIN_PSNR_DB} dB / \
                             |meanSignedErr| <= {MAX_ABS_MEAN_SIGNED_ERROR}: got \
                             maxAbsDiff={diff} psnr={psnr:.2} meanSignedErr={bias:+.4}"
                        ));
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "post-tone GPU stages exceeded their per-stage declared equivalence: {failures:?}"
    );
}

/// GPU-RENDER-PARITY-1 stage 3 (G-14): the red-eye pass must be pixel-effective
/// and match the CPU oracle on a frame whose redness actually triggers the
/// correction (the gradient/noise frames barely exercise it).
#[test]
fn implemented_red_eye_matches_cpu_oracle() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped red-eye parity check");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }

    let recipes: [(&str, EditRecipe); 2] = [
        ("single_region", red_eye_recipe()),
        (
            "two_regions_overlap",
            EditRecipe {
                red_eye: Some(RedEyeCorrection {
                    version: 1,
                    regions: vec![
                        RedEyeRegion {
                            id: "re-1".into(),
                            x: 0.4,
                            y: 0.5,
                            radius: 0.4,
                            desaturate: 0.9,
                            darken: 0.5,
                        },
                        RedEyeRegion {
                            id: "re-2".into(),
                            x: 0.6,
                            y: 0.5,
                            radius: 0.4,
                            desaturate: 0.3,
                            darken: 0.8,
                        },
                    ],
                }),
                ..Default::default()
            },
        ),
    ];
    let frames: [(&str, ImageFrame); 2] = [
        ("red_solid_64x64", red_frame(64, 64)),
        ("red_gradient_48x64", {
            let mut pixels = Vec::with_capacity(48 * 64 * 4);
            for y in 0..64u32 {
                for x in 0..48u32 {
                    let r = (120 + x * 120 / 47) as u8;
                    let g = (y * 80 / 63) as u8;
                    let b = 30;
                    pixels.extend_from_slice(&[r, g, b, 255]);
                }
            }
            ImageFrame::new(48, 64, pixels).expect("red gradient")
        }),
    ];

    let mut failures = Vec::new();
    for (name, recipe) in &recipes {
        assert!(
            unsupported_gpu_stages(recipe).is_empty(),
            "red-eye `{name}` must be GPU-eligible"
        );
        for (frame_name, frame) in &frames {
            let cpu = render_frame(
                frame,
                &RenderContext {
                    recipe,
                    camera_white_balance: None,
                    source_actions: &[],
                    masks: None,
                    lensfun: None,
                    depth: None,
                },
            )
            .expect("CPU oracle render")
            .frame;
            let gpu = ctx
                .render_with_gpu(frame, recipe)
                .unwrap_or_else(|error| panic!("{frame_name}/{name}: GPU render: {error}"));
            let diff = max_abs_diff(&cpu.pixels, &gpu.pixels);
            let psnr = psnr_db(&cpu.pixels, &gpu.pixels);
            let bias = mean_signed_error(&cpu.pixels, &gpu.pixels);
            eprintln!(
                "red_eye[{frame_name}/{name}]: maxAbsDiff={diff} psnr={psnr:.2} bias={bias:+.4}"
            );
            // Measured: byte-identical on the solid/gradient single-region
            // frames and one ±1 code on the overlapping-region gradient
            // (`hypot` vs. `sqrt` rounding at the disc edge). Asserted at the
            // documented stage bound.
            if diff > 1 || psnr < MIN_PSNR_DB || bias.abs() > MAX_ABS_MEAN_SIGNED_ERROR {
                failures.push(format!(
                    "{frame_name}/{name}: red-eye exceeded the declared bound \
                     maxAbsDiff <= 1 / PSNR >= {MIN_PSNR_DB} dB / \
                     |meanSignedErr| <= {MAX_ABS_MEAN_SIGNED_ERROR}: got \
                     maxAbsDiff={diff} psnr={psnr:.2} bias={bias:+.4}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "red-eye parity: {failures:?}");
}

/// R2-MCP-01: a decoder As-Shot WB context keeps CPU-routing, and the CPU-routed
/// GPU render must be byte-identical to the CPU oracle (both validate the gains
/// without re-applying them, so the context is pixel-neutral today). Stage-3
/// GPU-eligibility for valid contexts is a reported SOLL conflict because it
/// changes the CLI/MCP routing contract (out of this crate's write scope).
#[test]
fn as_shot_wb_context_routes_to_cpu_identity() {
    let recipe = EditRecipe {
        adjustments: BTreeMap::from([("exposure".into(), 0.3)]),
        ..Default::default()
    };
    let wb = [1.9f32, 1.0, 1.4, 1.0];

    assert!(
        unsupported_gpu_stages_with_context(&recipe, false, Some(&wb))
            .iter()
            .any(|r| r.contains("camera_white_balance")),
        "an As-Shot WB context must keep CPU-routing"
    );

    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped WB parity check");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let frame = gradient_frame(48, 48);
    let cpu = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: Some(wb),
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
    )
    .expect("CPU oracle render")
    .frame;
    let gpu = ctx.render_with_gpu(&frame, &recipe).expect("GPU render");
    let diff = max_abs_diff(&cpu.pixels, &gpu.pixels);
    eprintln!("wb_context: maxAbsDiff={diff}");
    assert_eq!(
        diff, 0,
        "a CPU-routed As-Shot WB context must match the full CPU reference"
    );
}

/// GPU-RENDER-PARITY-1 stage-2 follow-up: a schema-invalid sharpening radius
/// (> 10.0) must be **rejected**, not silently clamped to the 91-tap kernel.
/// The gate deliberately does *not* classify it as a stage gap: the GPU entry
/// rejects it directly (`validate_gpu_recipe`), mirroring the CPU oracle's
/// `InvalidAdjustment`.
#[test]
fn sharpening_radius_out_of_schema_is_rejected_not_clamped() {
    let recipe = EditRecipe {
        sharpening: Some(Sharpening {
            version: 1,
            amount: 1.0,
            radius: 12.0,
            detail: 0.5,
            masking: 0.0,
        }),
        ..Default::default()
    };

    // The CPU oracle rejects it loudly.
    assert!(
        render_frame(
            &gradient_frame(16, 16),
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: None,
                depth: None,
            },
        )
        .is_err(),
        "out-of-schema radius must be a hard CPU error"
    );
    // It is a rejection, not an unimplemented stage: no routing reason.
    assert!(
        unsupported_gpu_stages(&recipe).is_empty(),
        "an out-of-schema radius is not a GPU stage gap"
    );

    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped radius rejection check");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let frame = gradient_frame(16, 16);
    let err = ctx
        .render_with_gpu(&frame, &recipe)
        .expect_err("GPU must reject the out-of-schema radius");
    assert!(
        format!("{err}").contains("outside the schema range"),
        "unexpected error: {err}"
    );
    ctx.ensure_vram(16, 16).expect("vram state");
    let err = ctx
        .render_to_vram(&frame, &recipe)
        .expect_err("VRAM path must reject the out-of-schema radius");
    assert!(
        format!("{err}").contains("outside the schema range"),
        "unexpected error: {err}"
    );
}

/// Schema-foreign adjustment keys have no GPU meaning and no neutral value:
/// the GPU entry must surface the CPU oracle's `UnsupportedAdjustment` (a clean
/// rejection), not silently drop the key.
#[test]
fn schema_foreign_adjustment_key_is_rejected() {
    let recipe = EditRecipe {
        adjustments: BTreeMap::from([("definitely_not_a_key".into(), 0.5)]),
        ..Default::default()
    };
    assert!(
        unsupported_gpu_stages(&recipe)
            .iter()
            .any(|r| r.contains("definitely_not_a_key")),
        "unknown key must be reported"
    );

    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped unknown-key check");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let err = ctx
        .render_with_gpu(&gradient_frame(16, 16), &recipe)
        .expect_err("unknown key must be rejected, not dropped");
    assert!(
        format!("{err}").contains("UnsupportedAdjustment")
            || format!("{err}").contains("not supported")
            || format!("{err}").contains("unsupported"),
        "unexpected error: {err}"
    );
}

/// The interactive VRAM path (`render_to_vram`, which the GUI uses for the
/// readback-free present) must apply the same post-tone chain as
/// `render_with_gpu`. Read the VRAM output back through the diagnostic seam and
/// gate it on the same tolerance.
#[test]
fn vram_path_applies_post_stages() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped VRAM parity check");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    const W: u32 = 32;
    const H: u32 = 32;
    let frame = gradient_frame(W, H);
    let recipes: [(&str, EditRecipe, Equivalence); 4] = [
        (
            "curves",
            EditRecipe {
                curves: Some(Curves {
                    version: 1,
                    master: curve(&[(0.0, 0.0), (0.5, 0.4), (1.0, 1.0)]),
                    channels: CurveChannels::default(),
                }),
                ..Default::default()
            },
            // Measured 1 on the VRAM gradient frame (same rounding tie as the
            // streaming path).
            Equivalence::Bounded(1),
        ),
        (
            "presence_full",
            EditRecipe {
                presence: Some(Presence {
                    version: 1,
                    texture: 0.5,
                    clarity: 0.3,
                    dehaze: 0.5,
                }),
                ..Default::default()
            },
            Equivalence::Bounded(1),
        ),
        (
            "grading",
            EditRecipe {
                color_grading: Some(ColorGrading {
                    version: 1,
                    shadows: range(200.0, 0.3, 0.0),
                    midtones: ColorGradingRange::neutral(),
                    highlights: range(60.0, 0.2, 0.05),
                    balance: 0.1,
                    blending: 0.5,
                }),
                ..Default::default()
            },
            // Color Grading measured byte-identical.
            Equivalence::ByteIdentical,
        ),
        (
            // GPU-RENDER-PARITY-1 stage 2: the detail chain (Noise Reduction →
            // Sharpening → vignette → grain) runs in the readback-free VRAM path
            // through the same `render_post_stages` seam.
            "detail_stage_stack",
            EditRecipe {
                noise_reduction: Some(NoiseReduction {
                    version: 1,
                    luminance: 0.4,
                    color: 0.3,
                }),
                sharpening: Some(Sharpening {
                    version: 1,
                    amount: 1.0,
                    radius: 1.2,
                    detail: 0.6,
                    masking: 0.4,
                }),
                effects: Some(Effects {
                    vignette: Some(Vignette {
                        version: 1,
                        amount: 0.3,
                        midpoint: 0.5,
                        roundness: 1.0,
                        feather: 0.5,
                    }),
                    grain: Some(Grain {
                        version: 1,
                        amount: 0.25,
                        size: 0.4,
                        roughness: 0.5,
                        seed: 42,
                    }),
                }),
                ..Default::default()
            },
            Equivalence::Bounded(2),
        ),
    ];
    ctx.ensure_vram(W, H).expect("vram state");
    for (name, recipe, equivalence) in recipes {
        ctx.render_to_vram(&frame, &recipe).expect("vram render");
        let gpu = ctx.readback_output_frame().expect("vram readback");
        let cpu = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: None,
                depth: None,
            },
        )
        .expect("CPU oracle render")
        .frame;
        let diff = max_abs_diff(&cpu.pixels, &gpu.pixels);
        let psnr = psnr_db(&cpu.pixels, &gpu.pixels);
        let bias = mean_signed_error(&cpu.pixels, &gpu.pixels);
        eprintln!("vram[{name}]: maxAbsDiff={diff} psnr={psnr:.2} dB meanSignedErr={bias:+.4}");
        match equivalence {
            Equivalence::ByteIdentical => assert_eq!(
                diff, 0,
                "vram[{name}] asserted byte-identical (got {diff}, psnr={psnr:.2})"
            ),
            Equivalence::Bounded(bound) => assert!(
                diff <= bound && psnr >= MIN_PSNR_DB && bias.abs() <= MAX_ABS_MEAN_SIGNED_ERROR,
                "vram[{name}] exceeded declared bound maxAbsDiff <= {bound}: \
                 maxAbsDiff={diff} psnr={psnr:.2} meanSignedErr={bias:+.4}"
            ),
        }
    }
}

/// GPU-RENDER-PARITY-1 stage 3: the readback-free VRAM path must also run the
/// red-eye pass (it shares `render_post_stages` with the streaming path).
#[test]
fn vram_path_applies_red_eye() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped VRAM red-eye check");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    const W: u32 = 32;
    const H: u32 = 32;
    let frame = red_frame(W, H);
    let recipe = red_eye_recipe();
    assert!(unsupported_gpu_stages(&recipe).is_empty());
    ctx.ensure_vram(W, H).expect("vram state");
    ctx.render_to_vram(&frame, &recipe).expect("vram render");
    let gpu = ctx.readback_output_frame().expect("vram readback");
    let cpu = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
    )
    .expect("CPU oracle render")
    .frame;
    let diff = max_abs_diff(&cpu.pixels, &gpu.pixels);
    eprintln!("vram[red_eye]: maxAbsDiff={diff}");
    assert_eq!(diff, 0, "VRAM red-eye pass must match the CPU oracle");
}

/// A recipe that still uses an unimplemented stage stays CPU-routed and yields
/// CPU-identical pixels; this guards the "no third state" rule.
#[test]
fn unimplemented_stages_still_route_to_cpu() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped routing check");
            return;
        }
    };
    let frame = gradient_frame(48, 48);
    let recipe = EditRecipe {
        geometry: Some(Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        }),
        ..Default::default()
    };
    let reasons = unsupported_gpu_stages(&recipe);
    assert!(
        reasons.iter().any(|r| r.contains("geometry")),
        "{reasons:?}"
    );
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE} - validator-only assertion");
        return;
    }
    let cpu = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
    )
    .expect("CPU oracle render")
    .frame;
    let gpu = ctx.render_with_gpu(&frame, &recipe).expect("GPU render");
    assert_eq!(max_abs_diff(&cpu.pixels, &gpu.pixels), 0);
}

/// A Red-Eye recipe with a real, pixel-effective region on a red frame.
fn red_eye_recipe() -> EditRecipe {
    EditRecipe {
        red_eye: Some(RedEyeCorrection {
            version: 1,
            regions: vec![RedEyeRegion {
                id: "re-1".into(),
                x: 0.5,
                y: 0.5,
                radius: 0.3,
                desaturate: 0.8,
                darken: 0.3,
            }],
        }),
        ..Default::default()
    }
}

/// A typed `spot_removals` entry with no `extras` geometry: the CPU reference
/// rejects it loudly (`reject_unsupported_spot_modes_typed`), never heals it.
fn typed_spot_recipe() -> EditRecipe {
    EditRecipe {
        spot_removals: vec![SpotRemoval {
            version: 1,
            mode: SpotRemovalMode::Heuristic,
            artifact: None,
        }],
        ..Default::default()
    }
}

/// A legacy `extras["spot_removals"]` entry with valid, pixel-effective heal
/// geometry (`apply_spot_heals` copies an offset source patch).
fn legacy_spot_recipe() -> EditRecipe {
    let mut recipe = EditRecipe::default();
    recipe.extras.insert(
        "spot_removals".into(),
        // Valid heuristic geometry (id + center + radius + offset), matching
        // `validate_spot_removal_extra_entry`.
        serde_json::json!([{
            "id": "spot-1",
            "version": 1,
            "mode": "heuristic",
            "center_x": 0.5,
            "center_y": 0.5,
            "radius": 8.0,
            "feather": 0.5,
            "offset_dx": 0.25,
            "offset_dy": 0.0,
            "opacity": 1.0,
            "status": "valid"
        }]),
    );
    recipe
}

/// A generative edit with a valid, larger canvas and `expand_beyond_image` on:
/// the CPU reference expands the frame to the canvas size.
fn generative_expand_recipe() -> EditRecipe {
    EditRecipe {
        generative_edit: Some(GenerativeEdit {
            version: 1,
            canvas: Some(GenerativeCanvas {
                output_width: 128,
                output_height: 96,
                source_offset_x: 0,
                source_offset_y: 0,
                extras: BTreeMap::new(),
            }),
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: Some(false),
            expand_beyond_image: Some(true),
            seed: Some(7),
            prompt: None,
            extras: BTreeMap::new(),
        }),
        ..Default::default()
    }
}

/// GPU-RENDER-PARITY-1 stage 3 (gate completeness): typed `spot_removals`,
/// legacy `extras["spot_removals"]`, `generative_edit` and an **invalid**
/// red-eye correction are applied/validated by the CPU reference but not
/// implemented (or not safely implementable) on the GPU, so every one must be
/// reported as CPU-only. A valid red-eye correction is now GPU-rendered and
/// therefore must NOT be flagged (asserted below and by the parity harness);
/// this test asserts the gate verdict only, and
/// `cpu_only_recipe_stages_render_through_full_cpu_reference` proves the
/// fallback pixels.
#[test]
fn cpu_only_recipe_stages_are_gated() {
    let typed_spot = typed_spot_recipe();
    let legacy_spot = legacy_spot_recipe();
    let generative = generative_expand_recipe();
    let invalid_red_eye = invalid_red_eye_recipe();

    let cases: Vec<(&str, &EditRecipe)> = vec![
        ("red_eye", &invalid_red_eye),
        ("spot_removals", &typed_spot),
        ("spot_removals (legacy extras)", &legacy_spot),
        ("generative_edit", &generative),
    ];

    for (expected_reason, recipe) in &cases {
        let reasons = unsupported_gpu_stages(recipe);
        assert!(
            reasons.iter().any(|r| r.contains(expected_reason)),
            "`{expected_reason}` must be reported as CPU-only, got {reasons:?}"
        );
    }

    // A valid red-eye correction is renderable on the GPU.
    assert!(
        unsupported_gpu_stages(&red_eye_recipe()).is_empty(),
        "a valid red-eye correction must be GPU-eligible"
    );

    // The full recipe (every CPU-only stage at once) still reports all of them.
    let all = EditRecipe {
        red_eye: invalid_red_eye.red_eye.clone(),
        spot_removals: typed_spot.spot_removals.clone(),
        generative_edit: generative.generative_edit.clone(),
        extras: legacy_spot.extras.clone(),
        ..Default::default()
    };
    let reasons = unsupported_gpu_stages(&all);
    for expected in [
        "red_eye",
        "spot_removals",
        "spot_removals (legacy extras)",
        "generative_edit",
    ] {
        assert!(
            reasons.iter().any(|r| r.contains(expected)),
            "compound recipe must keep `{expected}`: {reasons:?}"
        );
    }
}

/// An invalid red-eye correction (out-of-range desaturation) that the CPU
/// oracle rejects loudly: the GPU gate must keep it CPU-routed.
fn invalid_red_eye_recipe() -> EditRecipe {
    EditRecipe {
        red_eye: Some(RedEyeCorrection {
            version: 1,
            regions: vec![RedEyeRegion {
                id: "re-1".into(),
                x: 0.5,
                y: 0.5,
                radius: 0.3,
                desaturate: 1.5,
                darken: 0.3,
            }],
        }),
        ..Default::default()
    }
}

/// A solid red frame so the Red-Eye correction is unambiguously pixel-effective.
fn red_frame(width: u32, height: u32) -> ImageFrame {
    ImageFrame::new(
        width,
        height,
        [200u8, 40, 40, 255].repeat((width * height) as usize),
    )
    .expect("red frame")
}

/// Blocker-1 resolution: `render_with_gpu`'s fallback is the **full**
/// `lumina_core::render_frame` chain. For CPU-routed, reference-renderable
/// recipes the fallback pixels must be byte-identical to `render_frame`
/// (including a generative canvas that *changes the frame dimensions*); for a
/// recipe the reference rejects (isolated typed spot), the fallback must reject
/// it too — never silently drop it.
#[test]
fn cpu_only_recipe_stages_render_through_full_cpu_reference() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped full-reference check");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }

    let frame = gradient_frame(48, 48);

    // Renderable CPU-routed recipes: fallback == full render_frame oracle.
    let renderable: Vec<(&str, &ImageFrame, EditRecipe)> = vec![
        ("legacy_spot", &frame, legacy_spot_recipe()),
        ("generative_expand", &frame, generative_expand_recipe()),
    ];
    for (name, source, recipe) in renderable {
        let oracle = render_frame(
            source,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: None,
                depth: None,
            },
        )
        .unwrap_or_else(|e| panic!("{name}: CPU reference failed: {e}"))
        .frame;
        let gpu = ctx
            .render_with_gpu(source, &recipe)
            .unwrap_or_else(|e| panic!("{name}: fallback render failed: {e}"));
        assert_eq!(
            (gpu.width, gpu.height),
            (oracle.width, oracle.height),
            "{name}: fallback must match the reference frame dimensions"
        );
        assert_eq!(
            max_abs_diff(&oracle.pixels, &gpu.pixels),
            0,
            "{name}: fallback must be byte-identical to the full CPU reference"
        );
    }

    // A recipe the reference rejects must be rejected loudly, not dropped.
    let typed = typed_spot_recipe();
    assert!(
        render_frame(
            &frame,
            &RenderContext {
                recipe: &typed,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: None,
                depth: None,
            },
        )
        .is_err(),
        "isolated typed spot must be a hard CPU error"
    );
    assert!(
        ctx.render_with_gpu(&frame, &typed).is_err(),
        "fallback must propagate the CPU hard error instead of dropping the stage"
    );

    // An invalid red-eye correction stays CPU-routed so the oracle's loud
    // rejection is preserved (the GPU pass never silently renders it).
    let invalid_red_eye = invalid_red_eye_recipe();
    assert!(
        render_frame(
            &frame,
            &RenderContext {
                recipe: &invalid_red_eye,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: None,
                depth: None,
            },
        )
        .is_err(),
        "out-of-range red-eye must be a hard CPU error"
    );
    assert!(
        ctx.render_with_gpu(&frame, &invalid_red_eye).is_err(),
        "fallback must propagate the invalid red-eye rejection"
    );
}

/// `render_to_vram` cannot CPU-route without a readback, so it must refuse a
/// recipe with unsupported stages (no divergent pixels in the VRAM output).
#[test]
fn vram_path_refuses_unsupported_recipes() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped VRAM refusal check");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let frame = gradient_frame(16, 16);
    ctx.ensure_vram(16, 16).expect("vram state");
    let recipe = EditRecipe {
        geometry: Some(Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        }),
        ..Default::default()
    };
    assert!(
        unsupported_gpu_stages(&recipe)
            .iter()
            .any(|r| r.contains("geometry")),
        "geometry must be reported"
    );
    assert!(
        ctx.render_to_vram(&frame, &recipe).is_err(),
        "VRAM path must refuse an unsupported recipe instead of writing divergent pixels"
    );
}

/// An *empty* legacy `extras["spot_removals"]` array is an explicit no-op in
/// core (`reject_unsupported_spot_modes_extras` iterates it, `spots_from_recipe`
/// yields none); it must not flag the GPU route.
#[test]
fn empty_legacy_spot_removals_do_not_flag() {
    let mut recipe = EditRecipe::default();
    recipe
        .extras
        .insert("spot_removals".into(), serde_json::json!([]));
    let reasons = unsupported_gpu_stages(&recipe);
    assert!(
        reasons.is_empty(),
        "an empty legacy spot list is identity, got {reasons:?}"
    );
}

// ---------------------------------------------------------------------------
// "Nicht-GPU-taugliche Rezepte" inventory (GPU-RENDER-PARITY-1 follow-up)
// ---------------------------------------------------------------------------

fn source_action(id: &str) -> SourceActionSpec {
    SourceActionSpec {
        version: SOURCE_ACTION_VERSION,
        kind: SourceActionKind::DustRemoval,
        artifact: SourceActionArtifactRef {
            id: id.into(),
            relative_path: format!("{id}.lumina.zdata"),
            checksum: "unused".into(),
        },
    }
}

fn source_actions(count: usize) -> EditRecipe {
    EditRecipe {
        source_actions: (0..count)
            .map(|i| source_action(&format!("sa-{i}")))
            .collect(),
        ..Default::default()
    }
}

/// Nails down the complete set of recipe configurations that still CPU-route
/// after GPU-RENDER-PARITY-1. Every branch of
/// [`unsupported_gpu_stages_with_context`] must be represented by at least one
/// minimal example whose expected reason substring is asserted, so a future
/// change that silently drops a gate branch fails this test.
///
/// Stage 2 additionally **proves the implemented detail classes are
/// GPU-eligible**: non-neutral Effects (vignette + grain), Noise Reduction and
/// Sharpening must all yield an empty reason list (the pixel parity itself is
/// asserted by `implemented_stages_match_cpu_oracle`). Stage 3 does the same for
/// a valid red-eye correction.
///
/// Reason classes and their minimal trigger (the reason string is the one the
/// gate emits):
/// - `geometry` — `recipe.geometry = Some(..)`.
/// - `lens_correction` — `recipe.lens_correction = Some(..)`.
/// - `perspective` — `recipe.perspective = Some(..)`.
/// - `lens_blur` — `lens_blur.enabled && blur_amount != 0`.
/// - `source_actions` — non-empty actions and `source_actions_bound = false`.
/// - `exceed the GPU stage slot limit` — bound actions with `len > MAX_SOURCE_ACTIONS`.
/// - `camera_white_balance (As-Shot context)` — context WB `Some`.
/// - `red_eye` — an **invalid** `recipe.red_eye` (out-of-range/NaN); a valid
///   correction is GPU-rendered.
/// - `spot_removals` — typed `recipe.spot_removals` non-empty.
/// - `spot_removals (legacy extras)` — non-empty `extras["spot_removals"]`.
/// - `generative_edit` — `recipe.generative_edit = Some(..)`.
/// - `adjustment \`clarity_v2\` not implemented on GPU` — a key outside
///   [`GPU_SUPPORTED_ADJUSTMENT_KEYS`] with no neutral value (all schema keys
///   are now GPU-supported, so this is the unknown-key class).
#[test]
fn cpu_routing_inventory_is_complete() {
    // GPU-RENDER-PARITY-1 stage 2 detail stages: implemented, so these must NOT
    // be flagged (byte-exactness/parity is proven by the oracle test).
    let effects = EditRecipe {
        effects: Some(Effects {
            vignette: Some(Vignette {
                version: 1,
                amount: -0.4,
                midpoint: 0.6,
                roundness: 1.0,
                feather: 0.5,
            }),
            grain: Some(Grain {
                version: 1,
                amount: 0.3,
                size: 0.4,
                roughness: 0.5,
                seed: 42,
            }),
        }),
        ..Default::default()
    };
    let noise = EditRecipe {
        noise_reduction: Some(NoiseReduction {
            version: 1,
            luminance: 0.5,
            color: 0.3,
        }),
        ..Default::default()
    };
    let sharpening = EditRecipe {
        sharpening: Some(Sharpening {
            version: 1,
            amount: 1.0,
            radius: 1.0,
            detail: 0.5,
            masking: 0.4,
        }),
        ..Default::default()
    };
    for (name, recipe) in [
        ("effects", &effects),
        ("noise_reduction", &noise),
        ("sharpening", &sharpening),
    ] {
        let reasons = unsupported_gpu_stages(recipe);
        assert!(
            reasons.is_empty(),
            "implemented stage `{name}` must be GPU-eligible, got {reasons:?}"
        );
    }

    // Stage 3: a valid red-eye correction is GPU-eligible; an invalid one must
    // stay flagged. (A decoder As-Shot WB context still CPU-routes — see the
    // SOLL conflict reported with GPU-RENDER-PARITY-1 stage 3.)
    assert!(
        unsupported_gpu_stages(&red_eye_recipe()).is_empty(),
        "valid red-eye must be GPU-eligible"
    );

    let geometry = EditRecipe {
        geometry: Some(Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        }),
        ..Default::default()
    };
    let lens = EditRecipe {
        lens_correction: Some(LensCorrection {
            version: 1,
            profile: None,
            distortion_k1: Some(0.1),
            distortion_k2: None,
            distortion_k3: None,
            vignette_c0: None,
            vignette_c1: None,
            vignette_c2: None,
            ca_red: None,
            ca_blue: None,
        }),
        ..Default::default()
    };
    let perspective = EditRecipe {
        perspective: Some(Perspective {
            version: 1,
            vertical: 0.1,
            horizontal: 0.0,
            rotation: 0.0,
            scale: 1.0,
            aspect_ratio: 1.0,
            shift_x: 0.0,
            shift_y: 0.0,
        }),
        ..Default::default()
    };
    let lens_blur = EditRecipe {
        lens_blur: Some(LensBlur {
            version: 1,
            enabled: true,
            focus_rect: FocusRect {
                x: 0.25,
                y: 0.25,
                width: 0.5,
                height: 0.5,
            },
            focal_near: 0.0,
            focal_far: 1.0,
            blur_amount: 0.5,
            bokeh: BokehShape::Round,
            depth_artifact: None,
        }),
        ..Default::default()
    };
    let unknown_key = EditRecipe {
        adjustments: BTreeMap::from([("clarity_v2".into(), 0.5)]),
        ..Default::default()
    };

    let wb = [1.9f32, 1.0, 1.4, 1.0];

    let cases: Vec<(&str, Vec<String>)> = vec![
        ("geometry", unsupported_gpu_stages(&geometry)),
        ("lens_correction", unsupported_gpu_stages(&lens)),
        ("perspective", unsupported_gpu_stages(&perspective)),
        ("lens_blur", unsupported_gpu_stages(&lens_blur)),
        (
            "source_actions",
            unsupported_gpu_stages_for(&source_actions(1), false),
        ),
        (
            "exceed the GPU stage slot limit",
            unsupported_gpu_stages_for(&source_actions(MAX_SOURCE_ACTIONS + 1), true),
        ),
        (
            "camera_white_balance (As-Shot context)",
            unsupported_gpu_stages_with_context(&EditRecipe::default(), false, Some(&wb)),
        ),
        ("red_eye", unsupported_gpu_stages(&invalid_red_eye_recipe())),
        (
            "spot_removals",
            unsupported_gpu_stages(&typed_spot_recipe()),
        ),
        (
            "spot_removals (legacy extras)",
            unsupported_gpu_stages(&legacy_spot_recipe()),
        ),
        (
            "generative_edit",
            unsupported_gpu_stages(&generative_expand_recipe()),
        ),
        (
            "adjustment `clarity_v2` not implemented on GPU",
            unsupported_gpu_stages(&unknown_key),
        ),
    ];

    for (expected, reasons) in &cases {
        assert!(
            reasons.iter().any(|r| r.contains(expected)),
            "inventory entry `{expected}` is no longer emitted by the gate: {reasons:?}"
        );
    }

    // Neutral/disabled configurations must stay GPU-eligible (R2-GPU-05-style
    // neutrality preserved) — these are the examples the inventory excludes.
    let disabled_lens_blur = EditRecipe {
        lens_blur: Some(LensBlur {
            version: 1,
            enabled: false,
            focus_rect: FocusRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            focal_near: 0.0,
            focal_far: 1.0,
            blur_amount: 0.0,
            bokeh: BokehShape::Round,
            depth_artifact: None,
        }),
        ..Default::default()
    };
    assert!(unsupported_gpu_stages(&disabled_lens_blur).is_empty());
    assert!(unsupported_gpu_stages(&EditRecipe::default()).is_empty());
    assert!(unsupported_gpu_stages_for(&source_actions(1), true).is_empty());
    assert!(unsupported_gpu_stages_with_context(&EditRecipe::default(), false, None).is_empty());
}
