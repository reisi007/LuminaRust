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

use lumina_core::{
    render_frame, render_frame_with_generative, DepthPlane, GenerativeCanvasArtifact,
    GenerativeCanvasInput, GenerativeRole as CoreGenerativeRole, ImageFrame, MaskPlane,
    RenderContext, SourceActionArtifact,
};
use lumina_gpu::{
    unsupported_gpu_stages, unsupported_gpu_stages_for, unsupported_gpu_stages_with_context,
    GpuContext, MAX_SOURCE_ACTIONS,
};
use lumina_sidecar::{
    AspectPreset, BokehShape, ColorGrading, ColorGradingRange, Crop, CurveChannels, CurvePoint,
    Curves, DepthArtifactRef, EditRecipe, Effects, FocusRect, GenerativeCanvas, GenerativeEdit,
    Geometry, Grain, HslAdjustments, HslChannel, LensBlur, LensCorrection, NoiseReduction,
    Perspective, PointColor, PointColorEntry, Presence, RedEyeCorrection, RedEyeRegion, Sharpening,
    SourceActionArtifactRef, SourceActionKind, SourceActionSpec, SpotRemoval, SpotRemovalMode,
    Vignette, SOURCE_ACTION_VERSION,
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

/// A flat RGBA8 frame (used for generative artifacts where any divergence from
/// the substituted canvas is obvious).
fn solid_frame(width: u32, height: u32, rgba: [u8; 4]) -> ImageFrame {
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for _ in 0..width * height {
        pixels.extend_from_slice(&rgba);
    }
    ImageFrame::new(width, height, pixels).expect("synthetic solid frame")
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

/// A G-05 lens-blur stage spec (enabled), heuristic unless `depth_artifact` is
/// set.
fn lens_blur_spec(
    amount: f32,
    near: f32,
    far: f32,
    bokeh: BokehShape,
    depth_artifact: Option<DepthArtifactRef>,
) -> LensBlur {
    LensBlur {
        version: 1,
        enabled: true,
        focus_rect: FocusRect {
            x: 0.2,
            y: 0.25,
            width: 0.5,
            height: 0.4,
        },
        focal_near: near,
        focal_far: far,
        blur_amount: amount,
        bokeh,
        depth_artifact,
    }
}

/// A G-05 lens-blur recipe on the focus-rect heuristic (no external depth
/// artifact).
fn lens_blur_recipe(amount: f32, near: f32, far: f32, bokeh: BokehShape) -> EditRecipe {
    EditRecipe {
        lens_blur: Some(lens_blur_spec(amount, near, far, bokeh, None)),
        ..Default::default()
    }
}

/// A G-05 lens-blur recipe referencing an external depth artifact.
fn external_depth_lens_blur_recipe() -> EditRecipe {
    EditRecipe {
        lens_blur: Some(lens_blur_spec(
            0.5,
            0.2,
            0.7,
            BokehShape::Round,
            Some(DepthArtifactRef {
                relative_path: "depth/map.bin".into(),
                sha256: "unused".into(),
            }),
        )),
        ..Default::default()
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
        (
            // GPU-RENDER-PARITY-1 follow-up end-state probe: every GPU-eligible
            // stage class at once — tone + curves/HSL/Point Color/Color Grading
            // + Presence + Noise Reduction/Sharpening + Red-Eye + Effects +
            // legacy spot-heal geometry. The gate must stay empty; the stacked
            // pixel parity is gated below.
            "fully_stacked_eligible",
            fully_stacked_eligible_recipe(),
        ),
        // --- GPU-RENDER-PARITY-1 geometry wave: lens/perspective/crop ---
        // (A lens/perspective correction **without** an explicit crop is
        // CPU-routed by GPU-MAXRECT-WELLE; see `default_content_crop_recipes`.)
        (
            "geometry_crop_free",
            EditRecipe {
                geometry: Some(Geometry {
                    version: 1,
                    crop: Some(Crop::Free {
                        x: 0.1,
                        y: 0.15,
                        width: 0.7,
                        height: 0.6,
                    }),
                    rotation_degrees: 0.0,
                    mirror_horizontal: false,
                    mirror_vertical: false,
                }),
                ..Default::default()
            },
        ),
        (
            "geometry_crop_aspect",
            EditRecipe {
                geometry: Some(Geometry {
                    version: 1,
                    crop: Some(Crop::Aspect {
                        preset: AspectPreset::OneToOne,
                    }),
                    rotation_degrees: 0.0,
                    mirror_horizontal: false,
                    mirror_vertical: false,
                }),
                ..Default::default()
            },
        ),
        (
            "geometry_rotate_90",
            EditRecipe {
                geometry: Some(Geometry {
                    version: 1,
                    crop: None,
                    rotation_degrees: 90.0,
                    mirror_horizontal: false,
                    mirror_vertical: false,
                }),
                ..Default::default()
            },
        ),
        (
            "geometry_rotate_arbitrary",
            EditRecipe {
                geometry: Some(Geometry {
                    version: 1,
                    crop: None,
                    rotation_degrees: 17.5,
                    mirror_horizontal: false,
                    mirror_vertical: false,
                }),
                ..Default::default()
            },
        ),
        (
            "geometry_mirror_both",
            EditRecipe {
                geometry: Some(Geometry {
                    version: 1,
                    crop: Some(Crop::Free {
                        x: 0.05,
                        y: 0.1,
                        width: 0.8,
                        height: 0.75,
                    }),
                    rotation_degrees: 0.0,
                    mirror_horizontal: true,
                    mirror_vertical: true,
                }),
                ..Default::default()
            },
        ),
        (
            "geometry_full_stack",
            EditRecipe {
                adjustments: BTreeMap::from([("exposure".into(), 0.2)]),
                lens_correction: Some(LensCorrection {
                    version: 1,
                    profile: None,
                    distortion_k1: Some(0.08),
                    distortion_k2: None,
                    distortion_k3: None,
                    vignette_c0: Some(0.95),
                    vignette_c1: None,
                    vignette_c2: None,
                    ca_red: Some(0.004),
                    ca_blue: Some(-0.004),
                }),
                perspective: Some(Perspective {
                    version: 1,
                    vertical: 0.15,
                    horizontal: 0.0,
                    rotation: 0.0,
                    scale: 1.0,
                    aspect_ratio: 1.0,
                    shift_x: 0.0,
                    shift_y: 0.0,
                }),
                geometry: Some(Geometry {
                    version: 1,
                    crop: Some(Crop::Free {
                        x: 0.1,
                        y: 0.1,
                        width: 0.7,
                        height: 0.7,
                    }),
                    rotation_degrees: 8.0,
                    mirror_horizontal: true,
                    mirror_vertical: false,
                }),
                ..Default::default()
            },
        ),
        (
            // Geometry must run *after* the post-tone adjustment chain (the
            // oracle order is adjustments → lens/perspective/crop). This recipe
            // exercises the `needs_post && geometry` wiring in both render
            // paths with a non-neutral curve + effects stack.
            "geometry_after_post_stages",
            EditRecipe {
                adjustments: BTreeMap::from([("exposure".into(), 0.15)]),
                curves: Some(Curves {
                    version: 1,
                    master: curve(&[(0.0, 0.0), (0.5, 0.42), (1.0, 1.0)]),
                    channels: CurveChannels {
                        red: Some(curve(&[(0.0, 0.0), (0.5, 0.55), (1.0, 1.0)])),
                        green: None,
                        blue: None,
                    },
                }),
                effects: Some(Effects {
                    vignette: Some(Vignette {
                        version: 1,
                        amount: 0.3,
                        midpoint: 0.5,
                        roundness: 1.0,
                        feather: 0.5,
                    }),
                    grain: None,
                }),
                geometry: Some(Geometry {
                    version: 1,
                    crop: Some(Crop::Free {
                        x: 0.15,
                        y: 0.1,
                        width: 0.6,
                        height: 0.7,
                    }),
                    rotation_degrees: 11.0,
                    mirror_horizontal: true,
                    mirror_vertical: true,
                }),
                ..Default::default()
            },
        ),
        // --- GPU-RENDER-PARITY-1 lens-blur wave: G-05 depth bokeh ---
        (
            "lens_blur_round",
            lens_blur_recipe(0.5, 0.0, 0.05, BokehShape::Round),
        ),
        (
            "lens_blur_elliptical",
            lens_blur_recipe(0.6, 0.0, 0.05, BokehShape::Elliptical),
        ),
        (
            "lens_blur_hexagonal",
            lens_blur_recipe(0.6, 0.0, 0.05, BokehShape::Hexagonal),
        ),
        (
            // A far focus band: the near/far ramps exercise both sides of
            // `focal_weight` (depth 0 and depth 1 both blur).
            "lens_blur_focus_far",
            lens_blur_recipe(0.5, 0.3, 0.6, BokehShape::Round),
        ),
        (
            // Schema-maximum radius (16): the widest tap set the oracle runs.
            "lens_blur_max_radius",
            lens_blur_recipe(1.0, 0.0, 0.05, BokehShape::Round),
        ),
        (
            // Identity short circuit (disabled): the gate must stay empty and
            // the pixels must be byte-identical to the source.
            "lens_blur_disabled",
            EditRecipe {
                lens_blur: Some(LensBlur {
                    enabled: false,
                    ..lens_blur_spec(0.5, 0.0, 0.05, BokehShape::Round, None)
                }),
                ..Default::default()
            },
        ),
        (
            // Lens blur runs *after* the post-tone color chain (oracle order).
            "lens_blur_after_post",
            EditRecipe {
                curves: Some(Curves {
                    version: 1,
                    master: curve(&[(0.0, 0.0), (0.5, 0.4), (1.0, 1.0)]),
                    channels: CurveChannels::default(),
                }),
                ..lens_blur_recipe(0.5, 0.0, 0.05, BokehShape::Round)
            },
        ),
        (
            // Lens blur is a sub-stage of Crop: it runs after the geometry
            // chain, so the output dimensions are the cropped ones and the
            // blur's focus rect is normalized to that canvas.
            "lens_blur_after_geometry",
            EditRecipe {
                geometry: Some(Geometry {
                    version: 1,
                    crop: Some(Crop::Free {
                        x: 0.1,
                        y: 0.1,
                        width: 0.7,
                        height: 0.7,
                    }),
                    rotation_degrees: 0.0,
                    mirror_horizontal: false,
                    mirror_vertical: false,
                }),
                ..lens_blur_recipe(0.5, 0.0, 0.05, BokehShape::Round)
            },
        ),
    ]
}

/// A manual lens-correction recipe (distortion + vignette + CA) driving all
/// three sub-stages with non-neutral coefficients.
fn manual_lens_recipe() -> EditRecipe {
    EditRecipe {
        lens_correction: Some(LensCorrection {
            version: 1,
            profile: None,
            distortion_k1: Some(0.12),
            distortion_k2: Some(-0.04),
            distortion_k3: None,
            vignette_c0: Some(0.9),
            vignette_c1: Some(-0.2),
            vignette_c2: None,
            ca_red: Some(0.006),
            ca_blue: Some(-0.006),
        }),
        ..Default::default()
    }
}

/// A manual lens correction whose radial map pushes the sampled source
/// coordinate *outward* (`k1 < 0`, pincushion), so the resampled frame keeps
/// transparent edges — the canonical "lens wedge" that makes the CPU oracle's
/// content-based default crop non-identity (CROP-MAXRECT-1).
fn lens_wedge_recipe() -> EditRecipe {
    EditRecipe {
        lens_correction: Some(LensCorrection {
            version: 1,
            profile: None,
            distortion_k1: Some(-0.08),
            distortion_k2: None,
            distortion_k3: None,
            vignette_c0: None,
            vignette_c1: None,
            vignette_c2: None,
            ca_red: None,
            ca_blue: None,
        }),
        ..Default::default()
    }
}

/// GPU-MAXRECT-WELLE: recipes that activate the CPU oracle's content-based
/// default crop (a lens/perspective correction without an explicit
/// `geometry.crop`). The resulting rectangle is derived from the resampled
/// alpha channel, so the recipe-only GPU geometry plan cannot predict its
/// (possibly smaller) output dimensions. These recipes are therefore routed to
/// the CPU loudly (gate reason `geometry (default content crop)`) instead of
/// rendering an uncropped, differently-sized frame.
///
/// Note the two non-wedge lens recipes: their default crop is the identity, but
/// the recipe-only gate cannot prove that without the resampled alpha, so it
/// conservatively routes them too. `lens_wedge`/`perspective_*` make the
/// divergence real and keep this test non-vacuous.
fn default_content_crop_recipes() -> Vec<(&'static str, EditRecipe)> {
    vec![
        ("lens_correction_manual", manual_lens_recipe()),
        (
            "lens_correction_wide_light",
            EditRecipe {
                lens_correction: Some(LensCorrection {
                    version: 1,
                    profile: Some("wide-light".into()),
                    distortion_k1: None,
                    distortion_k2: None,
                    distortion_k3: None,
                    vignette_c0: None,
                    vignette_c1: None,
                    vignette_c2: None,
                    ca_red: None,
                    ca_blue: None,
                }),
                ..Default::default()
            },
        ),
        ("lens_wedge", lens_wedge_recipe()),
        (
            "perspective_vertical",
            EditRecipe {
                perspective: Some(Perspective {
                    version: 1,
                    vertical: 0.25,
                    horizontal: 0.0,
                    rotation: 0.0,
                    scale: 1.0,
                    aspect_ratio: 1.0,
                    shift_x: 0.0,
                    shift_y: 0.0,
                }),
                ..Default::default()
            },
        ),
        (
            "perspective_full",
            EditRecipe {
                perspective: Some(Perspective {
                    version: 1,
                    vertical: 0.2,
                    horizontal: -0.15,
                    rotation: 0.1,
                    scale: 1.15,
                    aspect_ratio: 1.1,
                    shift_x: 0.05,
                    shift_y: -0.03,
                }),
                ..Default::default()
            },
        ),
    ]
}

/// GPU-RENDER-PARITY-1 follow-up end-state probe: a single recipe that sets
/// every GPU-eligible stage class. Used to assert the routing gate is empty and
/// that the accumulated parity stays within the documented tolerance.
fn fully_stacked_eligible_recipe() -> EditRecipe {
    let mut recipe = EditRecipe {
        adjustments: BTreeMap::from([
            ("exposure".into(), 0.3),
            ("contrast".into(), 0.1),
            ("highlights".into(), -0.2),
            ("shadows".into(), 0.15),
            ("whites".into(), 0.1),
            ("blacks".into(), -0.1),
            ("wb_temperature".into(), 5600.0),
            ("wb_tint".into(), 0.05),
            ("vibrance".into(), 0.3),
            ("saturation".into(), -0.1),
        ]),
        curves: Some(Curves {
            version: 1,
            master: curve(&[(0.0, 0.0), (0.4, 0.35), (0.7, 0.78), (1.0, 1.0)]),
            channels: CurveChannels {
                red: Some(curve(&[(0.0, 0.0), (0.5, 0.55), (1.0, 1.0)])),
                green: None,
                blue: Some(curve(&[(0.0, 0.0), (0.5, 0.48), (1.0, 1.0)])),
            },
        }),
        hsl: Some(HslAdjustments {
            version: 1,
            blue: Some(hsl(0.15, 0.1, 0.0)),
            orange: Some(hsl(-0.1, 0.05, 0.0)),
            ..Default::default()
        }),
        point_color: Some(PointColor {
            version: 1,
            entries: vec![entry("pc-1", 200.0, 70.0, 0.2, -0.1, 0.05)],
        }),
        color_grading: Some(ColorGrading {
            version: 1,
            shadows: range(210.0, 0.2, -0.05),
            midtones: ColorGradingRange::neutral(),
            highlights: range(50.0, 0.15, 0.05),
            balance: 0.1,
            blending: 0.55,
        }),
        presence: Some(Presence {
            version: 1,
            texture: 0.3,
            clarity: 0.2,
            dehaze: 0.2,
        }),
        noise_reduction: Some(NoiseReduction {
            version: 1,
            luminance: 0.3,
            color: 0.2,
        }),
        sharpening: Some(Sharpening {
            version: 1,
            amount: 0.8,
            radius: 1.2,
            detail: 0.5,
            masking: 0.3,
        }),
        red_eye: Some(RedEyeCorrection {
            version: 1,
            regions: vec![RedEyeRegion {
                id: "re-1".into(),
                x: 0.45,
                y: 0.5,
                radius: 0.2,
                desaturate: 0.5,
                darken: 0.2,
            }],
        }),
        effects: Some(Effects {
            vignette: Some(Vignette {
                version: 1,
                amount: 0.25,
                midpoint: 0.5,
                roundness: 1.0,
                feather: 0.5,
            }),
            grain: Some(Grain {
                version: 1,
                amount: 0.2,
                size: 0.4,
                roughness: 0.5,
                seed: 2026,
            }),
        }),
        ..Default::default()
    };
    recipe.extras.insert(
        "spot_removals".into(),
        serde_json::json!([{
            "id": "spot-1",
            "version": 1,
            "mode": "heuristic",
            "center_x": 0.4,
            "center_y": 0.45,
            "radius": 7.0,
            "feather": 0.5,
            "offset_dx": 0.25,
            "offset_dy": 0.1,
            "opacity": 0.9,
            "status": "valid"
        }]),
    );
    recipe
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
        // GPU-RENDER-PARITY-1 follow-up end-state: every eligible stage class
        // stacked; the per-stage FMA/rounding residuals accumulate on the noise
        // frame. Bound set from the measured MAX frames run (see test output).
        "fully_stacked_eligible" => Equivalence::Bounded(4),
        // --- GPU-RENDER-PARITY-1 geometry wave ---
        // Crop and mirror are exact integer sub-rect copies / flips; a
        // quarter-turn rotation is an exact integer remap. Asserted
        // byte-identical. Arbitrary rotation, lens and perspective resample in
        // the same `f32` 0..=255 domain as the oracle and carry at most one
        // rounding-tie code; the full chain accumulates that per stage.
        "geometry_crop_free"
        | "geometry_crop_aspect"
        | "geometry_rotate_90"
        | "geometry_mirror_both" => Equivalence::ByteIdentical,
        "geometry_rotate_arbitrary" | "geometry_full_stack" | "geometry_after_post_stages" => {
            Equivalence::Bounded(1)
        }
        // --- GPU-RENDER-PARITY-1 lens-blur wave ---
        // The convolution itself is exact integer accumulation; only the final
        // `f32` lerp differs from the oracle's `f64` (the heuristic weight math
        // matches operation-for-operation in `f32`), so each recipe carries at
        // most one rounding-tie code. `geometry` before it is an exact integer
        // sub-rect copy.
        "lens_blur_round"
        | "lens_blur_elliptical"
        | "lens_blur_hexagonal"
        | "lens_blur_focus_far"
        | "lens_blur_max_radius"
        | "lens_blur_after_post"
        | "lens_blur_after_geometry" => Equivalence::Bounded(1),
        // Disabled blur leaves the source bytes untouched.
        "lens_blur_disabled" => Equivalence::ByteIdentical,
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
            // GPU-RENDER-PARITY-1 geometry wave: a dimension-changing geometry
            // chain must reproduce the oracle's canvas exactly — a silently
            // different output size must fail here, not be masked by a pixel
            // compare.
            assert_eq!(
                (cpu.width, cpu.height),
                (gpu.width, gpu.height),
                "{frame_name}/{recipe_name}: GPU output dimensions must equal the CPU oracle"
            );
            let diff = max_abs_diff(&cpu.pixels, &gpu.pixels);
            let psnr = psnr_db(&cpu.pixels, &gpu.pixels);
            let bias = mean_signed_error(&cpu.pixels, &gpu.pixels);
            eprintln!(
                "parity[{frame_name}/{recipe_name}] {}x{}: maxAbsDiff={diff} psnr={psnr:.2} dB \
                 meanSignedErr={bias:+.4}",
                cpu.width, cpu.height
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

/// GPU-MAXRECT-WELLE (CROP-MAXRECT-1): a lens/perspective correction without an
/// explicit crop activates the CPU oracle's content-based default crop. The
/// recipe-only GPU plan cannot predict the (resampled-alpha-dependent)
/// rectangle, so the gate flags the recipe (`geometry (default content crop)`)
/// and `render_with_gpu` routes it to the CPU — the output dimensions and pixels
/// must be identical to the oracle. `lens_wedge` and the two `perspective_*`
/// recipes prove the default crop really fired (the oracle frame is smaller than
/// the uncropped geometry dimensions); the non-wedge lens recipes document the
/// conservative route (identity crop, still CPU-routed rather than risk a
/// mispredicted rectangle).
#[test]
fn default_content_crop_routes_to_cpu_with_parity() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped default-crop check");
            return;
        }
    };
    let frames: [(&str, ImageFrame); 2] = [
        ("gradient_64x64", gradient_frame(64, 64)),
        ("noise_64x64", noise_frame(64, 64, 0x0BAD_F00D_1234_5678)),
    ];
    // Recipes whose default crop is genuinely non-identity on these frames.
    let shrinks = ["lens_wedge", "perspective_vertical", "perspective_full"];

    for (name, recipe) in default_content_crop_recipes() {
        let reasons = unsupported_gpu_stages(&recipe);
        assert!(
            reasons
                .iter()
                .any(|reason| reason.contains("geometry (default content crop)")),
            "{name} must be flagged with the default-crop reason, got {reasons:?}"
        );
        for (frame_name, frame) in &frames {
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
            // The uncropped geometry dimensions (these recipes carry no explicit
            // crop and no rotation): when the default crop fires, the oracle
            // frame is strictly smaller than this.
            let uncropped = frame
                .measurement_domain_with_perspective(
                    recipe.geometry.as_ref(),
                    recipe.lens_correction.as_ref(),
                    recipe.perspective.as_ref(),
                )
                .expect("uncropped measurement domain");
            let cpu_area = cpu.width * cpu.height;
            let uncropped_area = uncropped.output_width * uncropped.output_height;
            if shrinks.contains(&name) {
                assert!(
                    cpu_area < uncropped_area,
                    "{frame_name}/{name}: the default content crop must shrink the \
                     oracle frame ({cpu_area} !< {uncropped_area})"
                );
            }
            if !ctx.is_available() {
                continue;
            }
            let gpu = ctx
                .render_with_gpu(frame, &recipe)
                .unwrap_or_else(|error| panic!("{frame_name}/{name}: GPU render: {error}"));
            assert_eq!(
                (cpu.width, cpu.height),
                (gpu.width, gpu.height),
                "{frame_name}/{name}: the CPU-routed default crop must match the oracle",
            );
            let diff = max_abs_diff(&cpu.pixels, &gpu.pixels);
            eprintln!(
                "default_crop[{frame_name}/{name}]: {}x{} (uncropped {}x{}) maxAbsDiff={diff}",
                cpu.width, cpu.height, uncropped.output_width, uncropped.output_height
            );
            assert_eq!(
                diff, 0,
                "{frame_name}/{name}: a default-content-crop render must be byte-identical"
            );
        }
    }
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

/// CAMERA-WB-WELLE (R2-MCP-01): a valid decoder As-Shot WB context is now an
/// explicit GPU input the caller binds via `GpuContext::set_camera_white_balance`;
/// the gains are validated with the oracle's own error but never re-applied
/// (matching `lumina-core`), so the GPU render stays byte-identical to the CPU
/// oracle for a tone-only recipe. Invalid gains are rejected at the bind and
/// still flagged by the routing gate so unbound callers reach the oracle's
/// loud rejection.
#[test]
fn as_shot_wb_context_is_carried_and_validated() {
    let recipe = EditRecipe {
        adjustments: BTreeMap::from([("exposure".into(), 0.3)]),
        ..Default::default()
    };
    let wb = [1.9f32, 1.0, 1.4, 1.0];

    // A valid context is no longer a routing reason …
    assert!(
        unsupported_gpu_stages_with_context(&recipe, false, Some(&wb)).is_empty(),
        "a valid As-Shot context must be GPU-eligible"
    );
    // … an invalid one still is.
    let invalid_reasons =
        unsupported_gpu_stages_with_context(&recipe, false, Some(&[0.0, 1.0, 1.0, 1.0]));
    assert!(
        invalid_reasons
            .iter()
            .any(|r| r.contains("camera_white_balance")),
        "{invalid_reasons:?}"
    );

    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped WB parity check");
            return;
        }
    };
    // The bind validates exactly like the oracle and never stores bad gains.
    assert!(
        ctx.set_camera_white_balance(Some([0.0, 1.0, 1.0, 1.0]))
            .is_err(),
        "invalid As-Shot gains must be rejected at the GPU entry"
    );
    assert_eq!(ctx.camera_white_balance(), None);
    ctx.set_camera_white_balance(Some(wb))
        .expect("valid As-Shot gains bind");

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
        "a carried (validated, not re-applied) As-Shot context must match the CPU reference"
    );
}

/// CAMERA-WB-WELLE oracle parity: decoder As-Shot gains (identity and
/// non-identity) crossed with every recipe white-balance combination. The GPU
/// entry carries the gains, so each render must match the CPU oracle within the
/// standard tone bound, and binding the gains must not change the GPU pixels at
/// all (they are validation state — `lumina-core` never re-applies them, and
/// neither may the shader).
#[test]
fn as_shot_wb_gains_match_cpu_oracle_across_recipe_wb() {
    let recipes: Vec<(&str, EditRecipe)> = vec![
        ("default", EditRecipe::default()),
        (
            "exposure",
            EditRecipe {
                adjustments: BTreeMap::from([("exposure".into(), 0.4)]),
                ..Default::default()
            },
        ),
        (
            "wb_temperature",
            EditRecipe {
                adjustments: BTreeMap::from([("wb_temperature".into(), 5600.0)]),
                ..Default::default()
            },
        ),
        (
            "wb_tint",
            EditRecipe {
                adjustments: BTreeMap::from([("wb_tint".into(), -0.3)]),
                ..Default::default()
            },
        ),
        (
            "wb_both",
            EditRecipe {
                adjustments: BTreeMap::from([
                    ("wb_temperature".into(), 7200.0),
                    ("wb_tint".into(), 0.2),
                ]),
                ..Default::default()
            },
        ),
        (
            "wb_exposure_contrast",
            EditRecipe {
                adjustments: BTreeMap::from([
                    ("wb_temperature".into(), 5800.0),
                    ("wb_tint".into(), 0.05),
                    ("exposure".into(), -0.3),
                    ("contrast".into(), 0.2),
                ]),
                ..Default::default()
            },
        ),
    ];
    let gains_cases: [(&str, [f32; 4]); 3] = [
        ("identity", [1.0, 1.0, 1.0, 1.0]),
        ("as_shot", [1.9, 1.0, 1.4, 1.0]),
        ("daylight", [0.8, 1.0, 1.25, 1.0]),
    ];

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

    let frame = gradient_frame(64, 64);
    let mut failures: Vec<String> = Vec::new();
    for (recipe_name, recipe) in &recipes {
        assert!(
            unsupported_gpu_stages(recipe).is_empty(),
            "{recipe_name} must stay GPU-eligible"
        );
        for (gains_name, gains) in gains_cases {
            ctx.set_camera_white_balance(Some(gains))
                .expect("identity/non-identity As-Shot gains are valid");
            let cpu = render_frame(
                &frame,
                &RenderContext {
                    recipe,
                    camera_white_balance: Some(gains),
                    source_actions: &[],
                    masks: None,
                    lensfun: None,
                    depth: None,
                },
            )
            .expect("CPU oracle render")
            .frame;
            let gpu = ctx
                .render_with_gpu(&frame, recipe)
                .unwrap_or_else(|error| panic!("{recipe_name}/{gains_name}: GPU render: {error}"));
            let diff = max_abs_diff(&cpu.pixels, &gpu.pixels);
            let psnr = psnr_db(&cpu.pixels, &gpu.pixels);
            let bias = mean_signed_error(&cpu.pixels, &gpu.pixels);
            eprintln!(
                "as_shot_wb[{recipe_name}/{gains_name}]: maxAbsDiff={diff} psnr={psnr:.2} \
                 bias={bias:+.4}"
            );
            if diff > 1 || psnr < MIN_PSNR_DB || bias.abs() > MAX_ABS_MEAN_SIGNED_ERROR {
                failures.push(format!(
                    "{recipe_name}/{gains_name}: exceeded the standard tone bound \
                     maxAbsDiff <= 1 / PSNR >= {MIN_PSNR_DB} dB / \
                     |meanSignedErr| <= {MAX_ABS_MEAN_SIGNED_ERROR}: got \
                     maxAbsDiff={diff} psnr={psnr:.2} bias={bias:+.4}"
                ));
            }

            // The gains are validation state: binding them must not move a
            // single GPU pixel (no double white balance on the decoder's work).
            ctx.set_camera_white_balance(None)
                .expect("clearing As-Shot context");
            let gpu_unbound = ctx
                .render_with_gpu(&frame, recipe)
                .unwrap_or_else(|error| panic!("{recipe_name}/{gains_name}: GPU render: {error}"));
            if gpu.pixels != gpu_unbound.pixels {
                failures.push(format!(
                    "{recipe_name}/{gains_name}: binding As-Shot gains changed GPU pixels \
                     (they must be validated, never re-applied)"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "As-Shot WB parity: {failures:?}");
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
        format!("{err}").contains("sharpening.radius"),
        "unexpected error: {err}"
    );
    ctx.ensure_vram(16, 16).expect("vram state");
    let err = ctx
        .render_to_vram(&frame, &recipe)
        .expect_err("VRAM path must reject the out-of-schema radius");
    assert!(
        format!("{err}").contains("sharpening.radius"),
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
/// GEN-ONNX-1 Welle 2a: `generative_edit` is no longer a CPU-routing reason, but
/// an **artifact-blind** render (recipe-only GPU entry / CPU `render_frame`) must
/// still refuse loudly instead of rendering unexpanded.
#[test]
fn artifact_blind_generative_render_is_loud_not_routed() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped routing check");
            return;
        }
    };
    let frame = gradient_frame(48, 48);
    let recipe = generative_expand_recipe();
    // Welle 2a: no blanket CPU route anymore — the stage is GPU-eligible.
    assert!(
        unsupported_gpu_stages(&recipe).is_empty(),
        "generative_edit must not be a routing reason: {:?}",
        unsupported_gpu_stages(&recipe)
    );
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE} - validator-only assertion");
        return;
    }
    // Without an artifact both the CPU oracle and the artifact-blind GPU entry
    // reject loudly (no silent unexpanded render, no third state).
    let context = RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    assert!(
        render_frame(&frame, &context).is_err(),
        "the CPU oracle must refuse an expand without a composited canvas"
    );
    assert!(
        ctx.render_with_gpu(&frame, &recipe).is_err(),
        "the artifact-blind GPU entry must refuse an expand without a canvas"
    );
    // With the artifact the artifact-aware GPU entry renders successfully.
    let canvas = lumina_core::generative::apply_generative_expand(&frame, &recipe)
        .expect("deterministic producer");
    let artifact = GenerativeCanvasArtifact::new(CoreGenerativeRole::Expand, canvas);
    let result = ctx.render_with_gpu_and_generative(
        &frame,
        &recipe,
        &GenerativeCanvasInput {
            auto_fill: None,
            expand: Some(&artifact),
        },
    );
    assert!(
        result.is_ok(),
        "the artifact-aware GPU entry must render an expand with a canvas: {:?}",
        result.err()
    );
}

/// GEN-ONNX-1 Welle 2a GPU parity of the mid-geometry expand compositing: the
/// chain is `Substitute(expand) → Crop` (an exact integer crop after the
/// substitution). The compositing itself is a texture swap, so parity is
/// byte-identical (`maxAbsDiff == 0`), matching the other exact stages.
#[test]
fn generative_canvas_compositing_is_gpu_parity() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped compositing parity");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let frame = gradient_frame(48, 48);
    let mut recipe = generative_expand_recipe();
    // Force a render pass *after* the substitution so the mid-chain insertion is
    // exercised (an exact 0.5 crop; the crop pass is a pure integer copy).
    recipe.geometry = Some(Geometry {
        version: 1,
        crop: Some(Crop::Free {
            x: 0.0,
            y: 0.0,
            width: 0.5,
            height: 0.5,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    });
    // Deterministic producer: the exact canvas the render will adopt.
    let canvas_frame = lumina_core::generative::apply_generative_expand(&frame, &recipe)
        .expect("deterministic producer");
    let artifact = GenerativeCanvasArtifact::new(CoreGenerativeRole::Expand, canvas_frame);

    let input = GenerativeCanvasInput {
        auto_fill: None,
        expand: Some(&artifact),
    };
    let cpu = render_frame_with_generative(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
        input,
    )
    .expect("CPU oracle compositing render")
    .frame;

    let gpu = ctx
        .render_with_gpu_and_generative(&frame, &recipe, &input)
        .expect("GPU generative render");

    assert_eq!(
        (cpu.width, cpu.height),
        (gpu.width, gpu.height),
        "composited dimensions must match on both backends"
    );
    assert_eq!(
        max_abs_diff(&cpu.pixels, &gpu.pixels),
        0,
        "the expand compositing + downstream crop must be GPU-parity"
    );
}

/// GEN-ONNX-1 Welle 2a GPU parity of the auto-fill compositing: the plan is a
/// single `Substitute(auto-fill)` (no other geometry), which exercises the
/// "only substitutions → copy into the final texture" path. Byte-identical.
#[test]
fn generative_auto_fill_compositing_is_gpu_parity() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped auto-fill parity");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    // 32x32 frame with a transparent border wedge.
    let mut pixels = vec![0u8; 32 * 32 * 4];
    for y in 0..32u32 {
        for x in 0..32u32 {
            let idx = ((y * 32 + x) * 4) as usize;
            if x < 4 || y < 4 || x >= 28 || y >= 28 {
                pixels[idx + 3] = 0;
            } else {
                let v = if (x + y) % 2 == 0 { 20 } else { 230 };
                pixels[idx] = v;
                pixels[idx + 1] = v;
                pixels[idx + 2] = v;
                pixels[idx + 3] = 255;
            }
        }
    }
    let frame = ImageFrame::new(32, 32, pixels).unwrap();
    let recipe = EditRecipe {
        generative_edit: Some(GenerativeEdit {
            version: 1,
            canvas: None,
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: Some(true),
            expand_beyond_image: None,
            seed: Some(11),
            prompt: None,
            extras: BTreeMap::new(),
        }),
        ..Default::default()
    };
    // Deterministic producer over the post-lens frame (no lens here).
    let mut canvas = frame.clone();
    lumina_core::generative::fill_transparent_heuristic(&mut canvas, 11);
    let artifact = GenerativeCanvasArtifact::new(CoreGenerativeRole::AutoFillTransparent, canvas);
    let input = GenerativeCanvasInput {
        auto_fill: Some(&artifact),
        expand: None,
    };

    let cpu = render_frame_with_generative(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
        input,
    )
    .expect("CPU oracle auto-fill render")
    .frame;
    let gpu = ctx
        .render_with_gpu_and_generative(&frame, &recipe, &input)
        .expect("GPU auto-fill render");
    assert_eq!((cpu.width, cpu.height), (32, 32));
    assert_eq!(
        max_abs_diff(&cpu.pixels, &gpu.pixels),
        0,
        "the auto-fill compositing must be GPU-parity"
    );
}

/// GEN-ONNX-1 Welle 2a BLOCKER fix: a real render pass **before** a trailing
/// `Substitute` must not discard the artifact. Recipe: a manual lens pass (real
/// geometry) plus an explicit full-frame crop, then expand (trailing substitute,
/// no crop step). The CPU oracle discards the lens output (`composite_expand`
/// replaces the frame); the GPU must return the artifact, not the lens output.
/// Before the fix this silently returned the lens texture (`Ok`, 4022/4096
/// bytes divergent).
#[test]
fn generative_trailing_substitute_after_render_pass_is_gpu_parity() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped trailing-substitute parity");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let frame = gradient_frame(48, 48);
    let mut recipe = generative_expand_recipe();
    // A real geometry render pass before the trailing substitute…
    recipe.lens_correction = Some(LensCorrection {
        version: 1,
        profile: None,
        distortion_k1: Some(0.2),
        distortion_k2: None,
        distortion_k3: None,
        vignette_c0: None,
        vignette_c1: None,
        vignette_c2: None,
        ca_red: None,
        ca_blue: None,
    });
    // …and an explicit (authoritative) full-frame crop so no default
    // content-crop reason applies; the crop is the identity and adds no step.
    recipe.geometry = Some(Geometry {
        version: 1,
        crop: Some(Crop::Free {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    });
    // A flat artifact makes any divergence (lens output vs artifact) obvious.
    let artifact = GenerativeCanvasArtifact::new(
        CoreGenerativeRole::Expand,
        solid_frame(128, 96, [10, 20, 30, 255]),
    );
    let input = GenerativeCanvasInput {
        auto_fill: None,
        expand: Some(&artifact),
    };

    let cpu = render_frame_with_generative(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
        input,
    )
    .expect("CPU oracle trailing-substitute render")
    .frame;
    let gpu = ctx
        .render_with_gpu_and_generative(&frame, &recipe, &input)
        .expect("GPU trailing-substitute render");

    assert_eq!(
        (cpu.width, cpu.height),
        (128, 96),
        "the CPU oracle result is the expand canvas"
    );
    assert_eq!(
        (gpu.width, gpu.height),
        (128, 96),
        "the GPU must return the trailing artifact, not the lens pass output"
    );
    assert_eq!(
        max_abs_diff(&cpu.pixels, &gpu.pixels),
        0,
        "a render pass before a trailing substitute must not be served as the result"
    );
    assert_eq!(
        &gpu.pixels[..4],
        &[10, 20, 30, 255],
        "the GPU result is the substituted artifact"
    );
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

/// GEN-ONNX-1: a generative edit with **no active role** is the identity and
/// needs no artifact, while still being CPU-only-gated as a recipe class.
fn generative_identity_recipe() -> EditRecipe {
    let mut recipe = generative_expand_recipe();
    if let Some(edit) = recipe.generative_edit.as_mut() {
        edit.canvas = None;
        edit.expand_beyond_image = None;
        edit.auto_fill_transparent = None;
    }
    recipe
}

/// GPU-RENDER-PARITY-1 follow-up (gate completeness) + GEN-ONNX-1 Welle 2a: an
/// **invalid** red-eye correction is the remaining recipe stage that is
/// validated by the CPU reference but not implemented on the GPU, so it must be
/// reported as CPU-only. A valid red-eye correction, the legacy
/// `extras["spot_removals"]` heal geometry and `generative_edit` (mid-geometry
/// artifact compositing) are GPU-rendered and therefore must NOT be flagged; a
/// typed geometry-free mirror shadow is tolerated exactly like the CPU oracle
/// when the extras geometry is present, and an isolated typed entry is a
/// **hard error on both backends** (asserted by
/// `typed_spot_without_extras_is_a_hard_error_on_both_backends`) instead of a
/// routing reason.
#[test]
fn cpu_only_recipe_stages_are_gated() {
    let invalid_red_eye = invalid_red_eye_recipe();

    let cases: Vec<(&str, &EditRecipe)> = vec![("red_eye", &invalid_red_eye)];

    for (expected_reason, recipe) in &cases {
        let reasons = unsupported_gpu_stages(recipe);
        assert!(
            reasons.iter().any(|r| r.contains(expected_reason)),
            "`{expected_reason}` must be reported as CPU-only, got {reasons:?}"
        );
    }

    // GPU-eligible now: a valid red-eye correction, legacy spot geometry, a
    // geometry-free typed mirror shadow paired with that extras geometry, and
    // the generative edit (artifact-aware GPU entry).
    assert!(
        unsupported_gpu_stages(&red_eye_recipe()).is_empty(),
        "a valid red-eye correction must be GPU-eligible"
    );
    assert!(
        unsupported_gpu_stages(&legacy_spot_recipe()).is_empty(),
        "legacy spot geometry must be GPU-eligible"
    );
    let mut shadow = legacy_spot_recipe();
    shadow.spot_removals = typed_spot_recipe().spot_removals;
    assert!(
        unsupported_gpu_stages(&shadow).is_empty(),
        "a geometry-free typed shadow with extras geometry must be GPU-eligible"
    );
    assert!(
        unsupported_gpu_stages(&generative_expand_recipe()).is_empty(),
        "generative_edit must be GPU-eligible via artifact compositing (Welle 2a)"
    );

    // The compound recipe (invalid red-eye + generative edit) reports only the
    // genuinely unsupported invalid red-eye now.
    let all = EditRecipe {
        red_eye: invalid_red_eye.red_eye.clone(),
        generative_edit: generative_expand_recipe().generative_edit.clone(),
        ..Default::default()
    };
    let reasons = unsupported_gpu_stages(&all);
    assert!(
        reasons.iter().any(|r| r.contains("red_eye")),
        "compound recipe must keep `red_eye`: {reasons:?}"
    );
    assert!(
        !reasons.iter().any(|r| r.contains("generative_edit")),
        "compound recipe must not flag the GPU-capable generative stage: {reasons:?}"
    );
}

/// GPU-RENDER-PARITY-1 follow-up: a typed `spot_removals` entry without the
/// extras heal geometry is unrenderable on both backends. The GPU entry must
/// reject it loudly (not silently drop the spot, and not route to the CPU to
/// hide the divergence), and it must not appear as a GPU stage gap.
#[test]
fn typed_spot_without_extras_is_a_hard_error_on_both_backends() {
    let recipe = typed_spot_recipe();
    let frame = gradient_frame(16, 16);
    // Not a routing reason: the GPU validates and errors itself.
    assert!(
        unsupported_gpu_stages(&recipe).is_empty(),
        "an isolated typed spot is a validation error, not a stage gap"
    );
    // The CPU reference rejects it loudly.
    assert!(
        render_frame(
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
        .is_err(),
        "isolated typed spot must be a hard CPU error"
    );

    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped typed-spot error check");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let err = ctx
        .render_with_gpu(&frame, &recipe)
        .expect_err("GPU must reject an isolated typed spot");
    assert!(
        format!("{err}").contains("spot_heal"),
        "unexpected error: {err}"
    );
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
/// `lumina_core::render_frame` chain. The (role-inactive) generative recipe is
/// CPU-routed as a fallback example and its pixels must be byte-identical to
/// `render_frame`; an *active* generative edit has no artifact on the CPU
/// fallback and must be rejected loudly by both backends (GEN-ONNX-1 Welle 2a —
/// no blanket route, no silent unexpanded render). The legacy spot
/// geometry is GPU-rendered and must match the oracle byte-for-byte through
/// the GPU spot pass. For a recipe the reference rejects (isolated typed spot),
/// the GPU entry must reject it too — never silently drop it.
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

    // Renderable recipes: legacy spots go through the GPU spot pass, the
    // (role-inactive) generative recipe through the full CPU fallback — both
    // must equal the full `render_frame` oracle (dimensions and bytes).
    let renderable: Vec<(&str, &ImageFrame, EditRecipe)> = vec![
        ("legacy_spot", &frame, legacy_spot_recipe()),
        ("generative_identity", &frame, generative_identity_recipe()),
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

    // GEN-ONNX-1: an *active* expand without a composited canvas artifact is
    // refused loudly by both backends (no silent unexpanded render).
    let active = generative_expand_recipe();
    assert!(
        render_frame(
            &frame,
            &RenderContext {
                recipe: &active,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: None,
                depth: None,
            },
        )
        .is_err(),
        "an active expand without an artifact must be a hard CPU error"
    );
    assert!(
        ctx.render_with_gpu(&frame, &active).is_err(),
        "the GPU fallback must refuse an active expand without an artifact"
    );

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
/// recipe it cannot render (no divergent pixels in the VRAM output). GEN-ONNX-1
/// Welle 2a: an active generative edit is no longer a routing *reason*, but the
/// artifact-blind VRAM path still refuses it loudly (no artifact injection).
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
    let recipe = generative_expand_recipe();
    assert!(
        unsupported_gpu_stages(&recipe).is_empty(),
        "generative_edit must be GPU-eligible (no routing reason) in Welle 2a"
    );
    assert!(
        ctx.render_to_vram(&frame, &recipe).is_err(),
        "the artifact-blind VRAM path must refuse an active generative recipe instead of \
         writing divergent (unexpanded) pixels"
    );
}

/// GPU-RENDER-PARITY-1 geometry wave / GPU-MAXRECT-WELLE: the readback-free
/// VRAM present texture is source-sized, so a geometry chain that **changes the
/// output dimensions** (crop/rotation/perspective) must be refused loudly — the
/// caller then uses the exact CPU present path. The same applies to a
/// lens/perspective correction without an explicit crop, whose content-based
/// default crop is derived from the resampled alpha and cannot be planned
/// readback-free (gate reason `geometry (default content crop)`).
/// Dimension-preserving, default-crop-inactive geometry (mirroring) is rendered
/// into the resident output and must match the CPU oracle.
#[test]
fn vram_geometry_dimension_change_is_refused_loudly() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped VRAM geometry check");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    const W: u32 = 24;
    const H: u32 = 24;
    let frame = gradient_frame(W, H);
    ctx.ensure_vram(W, H).expect("vram state");

    // A crop is GPU-eligible as a recipe, but its output dimensions differ from
    // the source, so the VRAM present path must refuse it (no silent write).
    let crop = EditRecipe {
        geometry: Some(Geometry {
            version: 1,
            crop: Some(Crop::Free {
                x: 0.1,
                y: 0.1,
                width: 0.5,
                height: 0.5,
            }),
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        }),
        ..Default::default()
    };
    assert!(
        unsupported_gpu_stages(&crop).is_empty(),
        "a crop recipe is GPU-eligible on the full render path"
    );
    assert!(
        ctx.render_to_vram(&frame, &crop).is_err(),
        "VRAM must refuse dimension-changing geometry loudly"
    );

    // GPU-MAXRECT-WELLE: a lens correction without an explicit crop activates
    // the content-based default crop. The VRAM path cannot plan it readback-free,
    // so the gate flags the recipe and the present path refuses (the GUI then
    // falls back to the exact CPU present path) — no divergent pixels are ever
    // written into the source-sized resident output.
    let lens = manual_lens_recipe();
    assert!(
        unsupported_gpu_stages(&lens)
            .iter()
            .any(|reason| reason.contains("geometry (default content crop)")),
        "a lens correction without an explicit crop must be flagged"
    );
    assert!(
        ctx.render_to_vram(&frame, &lens).is_err(),
        "VRAM must refuse a recipe with an unpredictable default content crop"
    );

    // Dimension-preserving, default-crop-inactive geometry (mirror) renders into
    // the resident output and matches the oracle byte-for-byte.
    let mirror = EditRecipe {
        geometry: Some(Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: true,
            mirror_vertical: false,
        }),
        ..Default::default()
    };
    assert!(unsupported_gpu_stages(&mirror).is_empty());
    ctx.render_to_vram(&frame, &mirror)
        .expect("dimension-preserving geometry renders in VRAM");
    let gpu = ctx.readback_output_frame().expect("vram readback");
    let cpu = render_frame(
        &frame,
        &RenderContext {
            recipe: &mirror,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
    )
    .expect("CPU oracle render")
    .frame;
    assert_eq!(
        (cpu.width, cpu.height),
        (gpu.width, gpu.height),
        "VRAM geometry must keep the source dimensions"
    );
    let diff = max_abs_diff(&cpu.pixels, &gpu.pixels);
    eprintln!("vram_geometry[mirror]: maxAbsDiff={diff}");
    assert_eq!(
        diff, 0,
        "VRAM mirror pass must match the CPU oracle byte-for-byte, got {diff}"
    );

    // Dimension-preserving geometry stacked *after* post-tone stages exercises
    // the VRAM `needs_post && geometry` wiring (post lands in a transient,
    // geometry then writes the resident output).
    let mut mirror_post = mirror.clone();
    mirror_post.curves = Some(Curves {
        version: 1,
        master: curve(&[(0.0, 0.0), (0.5, 0.42), (1.0, 1.0)]),
        channels: CurveChannels::default(),
    });
    assert!(unsupported_gpu_stages(&mirror_post).is_empty());
    ctx.render_to_vram(&frame, &mirror_post)
        .expect("post + dimension-preserving geometry renders in VRAM");
    let gpu = ctx.readback_output_frame().expect("vram readback");
    let cpu = render_frame(
        &frame,
        &RenderContext {
            recipe: &mirror_post,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
    )
    .expect("CPU oracle render")
    .frame;
    assert_eq!((cpu.width, cpu.height), (gpu.width, gpu.height));
    let diff = max_abs_diff(&cpu.pixels, &gpu.pixels);
    eprintln!("vram_geometry[mirror+curves]: maxAbsDiff={diff}");
    assert!(
        diff <= 1,
        "VRAM post + mirror chain must match the CPU oracle within one code, got {diff}"
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
// GPU-RENDER-PARITY-1 lens-blur wave: G-05 external depth + VRAM parity
// ---------------------------------------------------------------------------

/// A deterministic per-pixel depth plane (`0` top-left → `1` bottom-right,
/// mixed axes) so both ramps of `focal_weight` are exercised.
fn depth_plane(width: u32, height: u32) -> DepthPlane {
    let mut values = Vec::with_capacity((width * height) as usize);
    let dx = (width.max(2) - 1) as f32;
    let dy = (height.max(2) - 1) as f32;
    for y in 0..height {
        for x in 0..width {
            values.push((x as f32 / dx * 0.7 + y as f32 / dy * 0.3).clamp(0.0, 1.0));
        }
    }
    DepthPlane::new(width, height, values).expect("depth plane")
}

/// G-05 with a caller-supplied external depth plane: the GPU pass must match
/// the CPU oracle, and every missing/mismatched plane must fail loudly on both
/// backends (never a silent heuristic fallback).
#[test]
fn lens_blur_external_depth_matches_cpu_oracle() {
    let mut ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped lens-blur depth check");
            return;
        }
    };
    let frame = gradient_frame(48, 48);
    let plane = depth_plane(48, 48);
    let recipe = external_depth_lens_blur_recipe();
    assert!(
        unsupported_gpu_stages(&recipe).is_empty(),
        "external-depth lens blur is GPU-eligible"
    );

    // Missing plane: the CPU oracle aborts loudly, and so must the GPU entry
    // (a silent heuristic render would diverge from the source-of-truth).
    assert!(
        render_frame(
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
        .is_err(),
        "the CPU oracle must abort a missing depth artifact"
    );
    let gpu_err = ctx
        .render_with_gpu(&frame, &recipe)
        .expect_err("GPU must reject a missing depth artifact");
    assert!(
        format!("{gpu_err}").contains("lens_blur.depth_artifact"),
        "unexpected error: {gpu_err}"
    );

    ctx.set_depth_plane(Some(&plane)).expect("bind depth plane");
    let cpu = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: Some(&plane),
        },
    )
    .expect("CPU oracle render")
    .frame;

    if !ctx.is_available() {
        // CPU-only context: the bound plane flows into the full reference, so
        // the fallback is still the exact CPU pixels.
        eprintln!("{SKIP_MESSAGE} - CPU fallback depth assertion only");
        let gpu = ctx
            .render_with_gpu(&frame, &recipe)
            .expect("CPU fallback renders the bound depth plane");
        assert_eq!(max_abs_diff(&cpu.pixels, &gpu.pixels), 0);
        return;
    }

    let gpu = ctx
        .render_with_gpu(&frame, &recipe)
        .expect("GPU render with external depth");
    assert_eq!((cpu.width, cpu.height), (gpu.width, gpu.height));
    let diff = max_abs_diff(&cpu.pixels, &gpu.pixels);
    let psnr = psnr_db(&cpu.pixels, &gpu.pixels);
    let bias = mean_signed_error(&cpu.pixels, &gpu.pixels);
    eprintln!("lens_blur_external_depth: maxAbsDiff={diff} psnr={psnr:.2} bias={bias:+.4}");
    assert!(
        diff <= 1 && psnr >= MIN_PSNR_DB && bias.abs() <= MAX_ABS_MEAN_SIGNED_ERROR,
        "external-depth lens blur exceeded the declared bound maxAbsDiff <= 1 / \
         PSNR >= {MIN_PSNR_DB} dB / |meanSignedErr| <= {MAX_ABS_MEAN_SIGNED_ERROR}: \
         maxAbsDiff={diff} psnr={psnr:.2} bias={bias:+.4}"
    );

    // A dimension-mismatched plane is rejected loudly, not resampled.
    let bad = depth_plane(8, 8);
    ctx.set_depth_plane(Some(&bad))
        .expect("bind mismatched plane");
    assert!(
        ctx.render_with_gpu(&frame, &recipe).is_err(),
        "a mismatched depth plane must be rejected loudly"
    );

    // Out-of-range / non-finite values are rejected at bind time (no partial
    // state, no silent clamp).
    let mut invalid = depth_plane(8, 8);
    invalid.values[0] = 1.5;
    assert!(
        ctx.set_depth_plane(Some(&invalid)).is_err(),
        "an out-of-range depth value must be rejected"
    );
    invalid.values[0] = f32::NAN;
    assert!(
        ctx.set_depth_plane(Some(&invalid)).is_err(),
        "a NaN depth value must be rejected"
    );
    ctx.set_depth_plane(None).expect("clear depth plane");
}

/// The readback-free VRAM path renders G-05 lens blur into the resident output
/// (dimension-preserving, source-sized) and matches the CPU oracle — including
/// after a dimension-preserving geometry chain and with a bound external depth
/// plane.
#[test]
fn vram_path_applies_lens_blur() {
    let mut ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped VRAM lens-blur check");
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
    ctx.ensure_vram(W, H).expect("vram state");

    // `after_mirror_geometry` exercises the geometry→lens-blur ping-pong: the
    // dimension-preserving, default-crop-inactive mirror pass must write a
    // transient, not the resident output the blur then overwrites. (A manual
    // lens without an explicit crop is CPU-routed since GPU-MAXRECT-WELLE.)
    let mut after_mirror_geometry = lens_blur_recipe(0.5, 0.0, 0.05, BokehShape::Round);
    after_mirror_geometry.geometry = Some(Geometry {
        version: 1,
        crop: None,
        rotation_degrees: 0.0,
        mirror_horizontal: true,
        mirror_vertical: false,
    });

    let recipes: [(&str, EditRecipe, Equivalence); 3] = [
        (
            "round",
            lens_blur_recipe(0.6, 0.0, 0.05, BokehShape::Round),
            Equivalence::Bounded(1),
        ),
        (
            "hexagonal",
            lens_blur_recipe(0.6, 0.0, 0.05, BokehShape::Hexagonal),
            Equivalence::Bounded(1),
        ),
        (
            "after_mirror_geometry",
            after_mirror_geometry,
            Equivalence::Bounded(1),
        ),
    ];
    for (name, recipe, equivalence) in recipes {
        assert!(
            unsupported_gpu_stages(&recipe).is_empty(),
            "lens blur must be GPU-eligible"
        );
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
        assert_eq!((cpu.width, cpu.height), (gpu.width, gpu.height));
        let diff = max_abs_diff(&cpu.pixels, &gpu.pixels);
        let psnr = psnr_db(&cpu.pixels, &gpu.pixels);
        let bias = mean_signed_error(&cpu.pixels, &gpu.pixels);
        eprintln!("vram_lens_blur[{name}]: maxAbsDiff={diff} psnr={psnr:.2} bias={bias:+.4}");
        match equivalence {
            Equivalence::ByteIdentical => {
                assert_eq!(diff, 0, "vram[{name}] must be byte-identical")
            }
            Equivalence::Bounded(bound) => assert!(
                diff <= bound && psnr >= MIN_PSNR_DB && bias.abs() <= MAX_ABS_MEAN_SIGNED_ERROR,
                "vram[{name}] exceeded declared bound maxAbsDiff <= {bound}: \
                 maxAbsDiff={diff} psnr={psnr:.2} bias={bias:+.4}"
            ),
        }
    }

    // External depth in VRAM: a referenced artifact without a bound plane must
    // refuse before any resident write; with the plane bound the pass matches
    // the oracle.
    let external = external_depth_lens_blur_recipe();
    assert!(
        ctx.render_to_vram(&frame, &external).is_err(),
        "VRAM must refuse a referenced-but-unbound depth artifact"
    );
    let plane = depth_plane(W, H);
    ctx.set_depth_plane(Some(&plane)).expect("bind plane");
    ctx.render_to_vram(&frame, &external)
        .expect("vram render with external depth");
    let gpu = ctx.readback_output_frame().expect("vram readback");
    let cpu = render_frame(
        &frame,
        &RenderContext {
            recipe: &external,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: Some(&plane),
        },
    )
    .expect("CPU oracle render")
    .frame;
    let diff = max_abs_diff(&cpu.pixels, &gpu.pixels);
    eprintln!("vram_lens_blur[external_depth]: maxAbsDiff={diff}");
    assert!(
        diff <= 1,
        "VRAM external-depth lens blur: maxAbsDiff={diff}"
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
/// - `source_actions` — non-empty actions and `source_actions_bound = false`.
/// - `camera_white_balance (invalid As-Shot gains)` — an invalid context gain
///   (`inf`/`nan`/non-positive); a valid context is now GPU-carried and
///   therefore *not* a reason.
/// - `red_eye` — an **invalid** `recipe.red_eye` (out-of-range/NaN); a valid
///   correction is GPU-rendered.
///   (`generative_edit` is **no longer** a reason since GEN-ONNX-1 Welle 2a: the
///   artifact-aware entry `render_with_gpu_and_generative` injects the composited
///   canvas mid-geometry; an artifact-blind render fails loudly in the plan.)
/// - `adjustment \`clarity_v2\` not implemented on GPU` — a key outside
///   [`GPU_SUPPORTED_ADJUSTMENT_KEYS`] with no neutral value (all schema keys
///   are now GPU-supported, so this is the unknown-key class).
/// - `geometry (default content crop)` — a lens/perspective correction is
///   present and `geometry.crop` is `None`, so the CPU oracle applies its
///   content-based maximum-content-rect crop before rotation/mirroring. That
///   rectangle depends on the resampled alpha, so the recipe-only GPU plan
///   cannot reproduce its (possibly smaller) output dimensions
///   (GPU-MAXRECT-WELLE / CROP-MAXRECT-1).
///
/// GPU-RENDER-PARITY-1 geometry wave / GPU-MAXRECT-WELLE: geometry
/// (crop/rotation/mirror), the manual lens correction and perspective are
/// GPU-eligible for valid recipes **with an explicit crop** (their
/// pixel/dimension parity is asserted by `implemented_stages_match_cpu_oracle`),
/// so they are not gate reasons. The one geometry case that still flags is a
/// lens/perspective correction **without** an explicit crop: it activates the
/// content-based default crop (see `default_content_crop_recipes` and
/// `default_content_crop_routes_to_cpu_with_parity`).
///
/// GPU-RENDER-PARITY-1 lens-blur wave: G-05 lens blur is GPU-eligible for valid
/// recipes (heuristic **and** external depth, the latter requiring a bound
/// plane), so it is **not** a gate reason either (parity asserted by
/// `implemented_stages_match_cpu_oracle` and
/// `lens_blur_external_depth_matches_cpu_oracle`).
///
/// GPU-RENDER-PARITY-1 follow-up (items 5 + 7): legacy spot geometry, a typed
/// geometry-free mirror shadow, and **any number** of bound source actions
/// (batched in groups of [`MAX_SOURCE_ACTIONS`]) are GPU-eligible; their parity
/// is asserted by the oracle harness and `source_action_batch` tests, not by a
/// gate reason.
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
    // stay flagged. (CAMERA-WB-WELLE: a valid As-Shot WB context is now
    // GPU-carried too; only invalid gains stay flagged — see the inventory
    // cases below.)
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

    let wb = [0.0f32, 1.0, 1.0, 1.0];

    let cases: Vec<(&str, Vec<String>)> = vec![
        (
            "source_actions",
            unsupported_gpu_stages_for(&source_actions(1), false),
        ),
        (
            "camera_white_balance (invalid As-Shot gains)",
            unsupported_gpu_stages_with_context(&EditRecipe::default(), false, Some(&wb)),
        ),
        ("red_eye", unsupported_gpu_stages(&invalid_red_eye_recipe())),
        (
            "adjustment `clarity_v2` not implemented on GPU",
            unsupported_gpu_stages(&unknown_key),
        ),
        // GPU-MAXRECT-WELLE / CROP-MAXRECT-1: a lens/perspective correction
        // without an explicit crop activates the content-based default crop.
        (
            "geometry (default content crop)",
            unsupported_gpu_stages(&lens),
        ),
        (
            "geometry (default content crop)",
            unsupported_gpu_stages(&perspective),
        ),
    ];

    for (expected, reasons) in &cases {
        assert!(
            reasons.iter().any(|r| r.contains(expected)),
            "inventory entry `{expected}` is no longer emitted by the gate: {reasons:?}"
        );
    }

    // GEN-ONNX-1 Welle 2a: `generative_edit` is no longer a routing reason. The
    // artifact-aware GPU entry injects the composited canvas; a recipe-only
    // render fails loudly instead of a blanket CPU route.
    assert!(
        unsupported_gpu_stages(&generative_expand_recipe()).is_empty(),
        "generative_edit must be GPU-eligible now (artifact compositing)"
    );

    // GPU-RENDER-PARITY-1 geometry wave / GPU-MAXRECT-WELLE: geometry
    // (crop/rotation/mirror) without a correction is rendered by the GPU
    // `geometry` passes and must stay GPU-eligible; a correction **with** an
    // explicit crop is authoritative and likewise stays eligible. A correction
    // **without** an explicit crop is the flagged default-crop class asserted in
    // the inventory cases above. The pixel/dimension parity is asserted by
    // `implemented_stages_match_cpu_oracle` (explicit crop) and
    // `default_content_crop_routes_to_cpu_with_parity` (loud CPU route).
    let geometry_reasons = unsupported_gpu_stages(&geometry);
    assert!(
        geometry_reasons.is_empty(),
        "geometry without a correction must be GPU-eligible, got {geometry_reasons:?}"
    );
    let mut lens_cropped = lens.clone();
    lens_cropped.geometry = Some(Geometry {
        version: 1,
        crop: Some(Crop::Free {
            x: 0.1,
            y: 0.1,
            width: 0.8,
            height: 0.8,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    });
    assert!(
        unsupported_gpu_stages(&lens_cropped).is_empty(),
        "a lens correction with an explicit crop must stay GPU-eligible"
    );

    // GPU-RENDER-PARITY-1 lens-blur wave: G-05 is GPU-rendered (heuristic and
    // external depth — the latter requires a bound plane, which the recipe-only
    // gate cannot see), so an active recipe must no longer be flagged.
    let lens_blur_reasons = unsupported_gpu_stages(&lens_blur);
    assert!(
        lens_blur_reasons.is_empty(),
        "G-05 lens blur must be GPU-eligible, got {lens_blur_reasons:?}"
    );
    assert!(
        unsupported_gpu_stages(&external_depth_lens_blur_recipe()).is_empty(),
        "an external-depth lens-blur recipe must be GPU-eligible (caller binds the plane)"
    );

    // GPU-RENDER-PARITY-1 follow-up: every class that used to be flagged here is
    // now GPU-eligible for valid recipes — legacy spot geometry, a typed
    // geometry-free mirror shadow, and any number of bound source actions.
    assert!(
        unsupported_gpu_stages(&legacy_spot_recipe()).is_empty(),
        "legacy spot geometry is GPU-rendered"
    );
    let mut shadow = legacy_spot_recipe();
    shadow.spot_removals = typed_spot_recipe().spot_removals;
    assert!(
        unsupported_gpu_stages(&shadow).is_empty(),
        "a typed shadow with extras geometry is GPU-eligible"
    );
    assert!(
        unsupported_gpu_stages_for(&source_actions(MAX_SOURCE_ACTIONS + 3), true).is_empty(),
        "source actions above the per-pass slot count are batched on the GPU"
    );

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
    // CAMERA-WB-WELLE: a *valid* As-Shot context is GPU-carried, so it must not
    // be flagged (its parity is asserted by `as_shot_wb_gains_match_cpu_oracle_across_recipe_wb`).
    assert!(unsupported_gpu_stages_with_context(
        &EditRecipe::default(),
        false,
        Some(&[1.9, 1.0, 1.4, 1.0])
    )
    .is_empty());
}

// ---------------------------------------------------------------------------
// GPU-RENDER-PARITY-1 follow-up: source-action batching (item 7) and the
// legacy spot-heal pass (item 5).
// ---------------------------------------------------------------------------

/// Build `count` full-frame source-action artifacts. Each artifact replaces a
/// distinct overlapping rectangle (region coverage `u16::MAX`) with a solid
/// colour; the overlap makes the batch order observable.
fn source_action_artifacts(width: u32, height: u32, count: usize) -> Vec<SourceActionArtifact> {
    (0..count)
        .map(|index| {
            let mut values = vec![0u16; (width * height) as usize];
            let x0 = (index as u32 * 2) % width.max(1);
            let y0 = (index as u32 * 3) % height.max(1);
            for y in y0..(y0 + height / 2).min(height) {
                for x in x0..(x0 + width / 2).min(width) {
                    values[(y * width + x) as usize] = u16::MAX;
                }
            }
            let replacement = [
                (index as u8).wrapping_mul(37),
                (index as u8).wrapping_mul(53),
                (index as u8).wrapping_mul(97),
                255u8,
            ]
            .repeat((width * height) as usize);
            SourceActionArtifact {
                region: MaskPlane {
                    width,
                    height,
                    values,
                },
                replacement: ImageFrame::new(width, height, replacement).expect("replacement"),
            }
        })
        .collect()
}

/// Item 7: more artifacts than the per-pass slot count must composite on the
/// GPU in batches and match the CPU oracle byte-for-byte (the previous gate
/// CPU-routed these).
#[test]
fn source_action_batch_matches_oracle() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped source-action batch check");
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
    let count = MAX_SOURCE_ACTIONS + 3;
    let artifacts = source_action_artifacts(W, H, count);
    let recipe = source_actions(count);
    assert!(
        unsupported_gpu_stages_for(&recipe, true).is_empty(),
        "batched source actions must be GPU-eligible"
    );

    let mut ctx = ctx;
    ctx.set_source_action_artifacts(&artifacts)
        .expect("bind artifacts");

    let cpu = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &artifacts,
            masks: None,
            lensfun: None,
            depth: None,
        },
    )
    .expect("CPU oracle render")
    .frame;
    let gpu = ctx
        .render_with_gpu(&frame, &recipe)
        .expect("GPU render with batched source actions");
    let diff = max_abs_diff(&cpu.pixels, &gpu.pixels);
    eprintln!("source_action_batch[{count}]: maxAbsDiff={diff}");
    assert_eq!(
        diff, 0,
        "batched source-action compositing must match the CPU oracle"
    );
}

/// Item 5: legacy spot-heal geometry rendered by the GPU spot pass must match
/// `apply_spot_heals` byte-for-byte, including feathered and overlapping spots.
#[test]
fn legacy_spot_heal_matches_cpu_oracle() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped spot-heal check");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let mut recipe = EditRecipe::default();
    recipe.extras.insert(
        "spot_removals".into(),
        serde_json::json!([
            {
                "id": "spot-1",
                "version": 1,
                "mode": "heuristic",
                "center_x": 0.3,
                "center_y": 0.4,
                "radius": 6.0,
                "feather": 0.5,
                "offset_dx": 0.3,
                "offset_dy": 0.1,
                "opacity": 1.0,
                "status": "valid"
            },
            {
                "id": "spot-2",
                "version": 1,
                "mode": "heuristic",
                "center_x": 0.35,
                "center_y": 0.45,
                "radius": 9.0,
                "feather": 0.0,
                "offset_dx": -0.2,
                "offset_dy": 0.25,
                "opacity": 0.6,
                "status": "valid"
            }
        ]),
    );
    assert!(unsupported_gpu_stages(&recipe).is_empty());

    let frames: Vec<(&str, ImageFrame)> = vec![
        ("gradient_48x48", gradient_frame(48, 48)),
        ("noise_48x48", noise_frame(48, 48, 0x51C0_FFEE)),
    ];
    let mut failures = Vec::new();
    for (name, frame) in &frames {
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
            .unwrap_or_else(|error| panic!("{name}: GPU spot render: {error}"));
        let diff = max_abs_diff(&cpu.pixels, &gpu.pixels);
        eprintln!("spot_heal[{name}]: maxAbsDiff={diff}");
        if diff != 0 {
            failures.push(format!("{name}: maxAbsDiff={diff}"));
        }
    }

    // The readback-free VRAM path shares the spot-heal pass; assert it there too.
    let frame = gradient_frame(48, 48);
    ctx.ensure_vram(48, 48).expect("vram state");
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
    eprintln!("spot_heal[vram_48x48]: maxAbsDiff={diff}");
    assert_eq!(diff, 0, "VRAM spot-heal pass must match the CPU oracle");

    assert!(
        failures.is_empty(),
        "spot-heal GPU pass diverged from the CPU oracle: {failures:?}"
    );
}
