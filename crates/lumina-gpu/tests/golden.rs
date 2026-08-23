//! CPU↔GPU golden-image equivalence harness (PERF-GUI-8).
//!
//! This integration test is the regression net that keeps the GPU render path
//! (`lumina_gpu::GpuContext::render_with_gpu`) byte-close to the CPU oracle
//! (`lumina_core::render_frame`). It is the gating test that proves the two
//! pipelines produce near-identical pixels once the GPU DAG lands.
//!
//! # What it does
//!
//! For a set of small synthetic [`ImageFrame`]s (smooth gradients and
//! high-frequency noise, generated in-test — no decodeable sample fixture is
//! shipped in `sample-data/`, only `.cr3` RAW files which this harness does not
//! decode) and a basket of recipes (default, exposure, contrast, white balance),
//! it:
//!
//! 1. renders the frame once through the CPU oracle (`render_frame`), and
//! 2. renders the same frame+recipe once through the GPU path
//!    (`GpuContext::render_with_gpu`),
//!
//! then gates the two RGBA8 outputs on the tolerance policy defined in
//! `docs/gpu-bootstrap.md` → "Equivalence verification":
//!
//! - **maxAbsDiff per channel ≤ 1** (RGBA8), and
//! - **PSNR ≥ 45 dB** (global, MAX=255).
//!
//! A **blake3** content hash of each output is also computed and *reported*;
//! hash equality is informational only (it MAY differ — e.g. due to FP16
//! rounding in the future GPU path — and is never asserted).
//!
//! # GPU availability / headless CI
//!
//! The GPU path is only exercised when a real adapter is bound
//! (`GpuContext::is_available()`). When no adapter is present (headless CI, or
//! `--no-default-features`) the equivalence check is **skipped**, not failed:
//! the test prints `GPU adapter unavailable - skipped equivalence check` and
//! returns. This keeps the harness green on machines without a GPU while still
//! running the real comparison wherever a GPU exists.
//!
//! Per `Agents.md` (no silent fallback), the bootstrap's CPU fallback is called
//! out loudly: until the GPU DAG is wired, `render_with_gpu` routes through the
//! CPU pipeline, so the harness prints a `[WARN]` noting that the comparison is
//! currently CPU-vs-CPU and will become meaningful once the shader/tiling
//! stages land.

use blake3::Hasher;
use lumina_core::{render_frame, ImageFrame, RenderContext};
use lumina_gpu::{unsupported_gpu_stages, GpuContext};
use lumina_sidecar::{
    CurvePoint, Curves, EditRecipe, Effects, HslAdjustments, HslChannel, Presence,
    SourceActionArtifactRef, SourceActionKind, SourceActionSpec, Vignette, SOURCE_ACTION_VERSION,
};
use std::collections::BTreeMap;

/// Tolerance policy (see `docs/gpu-bootstrap.md`).
///
/// Per-channel maximum absolute difference allowed between CPU and GPU output,
/// measured in 8-bit units. A value of `1` keeps the two paths visually
/// indistinguishable for 8-bit display.
const MAX_ABS_DIFF_TOLERANCE: u8 = 1;

/// Tolerance policy (see `docs/gpu-bootstrap.md`).
///
/// Minimum peak-signal-to-noise ratio (dB) required between the CPU and GPU
/// outputs. `45 dB` corresponds to an RMS error of ~1.4/255 and is the project
/// gate for "near-identical" render equivalence.
const MIN_PSNR_DB: f64 = 45.0;

/// Exact message printed when no GPU adapter is available so the run is visibly
/// skipped (never silently passed).
const SKIP_MESSAGE: &str = "GPU adapter unavailable - skipped equivalence check";

// ---------------------------------------------------------------------------
// Synthetic frame generation
// ---------------------------------------------------------------------------

/// Smooth horizontal+diagonal gradient. Every channel ramps across the frame so
/// the tonal operators (exposure/contrast/WB) have a full tonal range to act on.
fn gradient_frame(width: u32, height: u32) -> ImageFrame {
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let rx = x as f64 / (width as f64 - 1.0).max(1.0);
            let ry = y as f64 / (height as f64 - 1.0).max(1.0);
            let r = (rx * 255.0).round() as u8;
            let g = (ry * 255.0).round() as u8;
            let b = (((rx + ry) * 0.5) * 255.0).round() as u8;
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    ImageFrame::new(width, height, pixels).expect("synthetic gradient frame")
}

/// Deterministic high-frequency noise frame (splitmix64 seeded). High-frequency
/// content stresses the per-pixel LUT fusion and any future GPU kernel rounding.
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
        let r = (next() & 0xFF) as u8;
        let g = (next() & 0xFF) as u8;
        let b = (next() & 0xFF) as u8;
        pixels.extend_from_slice(&[r, g, b, 255]);
    }
    ImageFrame::new(width, height, pixels).expect("synthetic noise frame")
}

// ---------------------------------------------------------------------------
// Recipes
// ---------------------------------------------------------------------------

/// The recipe basket exercised by the harness.
///
/// REVIEW-GPU-DIVERGENCE-1: the basket deliberately includes recipes whose
/// stages the GPU tone stage does NOT implement (vibrance/saturation, curves,
/// HSL, presence, effects, source actions). For those, `render_with_gpu` must
/// route to the CPU pipeline instead of silently dropping the stage — the
/// routing test below pins this byte-exactly.
fn recipes() -> Vec<(&'static str, EditRecipe)> {
    vec![
        ("default", EditRecipe::default()),
        (
            "exposure_0_5",
            EditRecipe {
                adjustments: BTreeMap::from([("exposure".into(), 0.5)]),
                ..Default::default()
            },
        ),
        (
            "contrast_neg_0_2",
            EditRecipe {
                adjustments: BTreeMap::from([("contrast".into(), -0.2)]),
                ..Default::default()
            },
        ),
        (
            "wb_5800_tint_0_05",
            EditRecipe {
                adjustments: BTreeMap::from([
                    ("wb_temperature".into(), 5800.0),
                    ("wb_tint".into(), 0.05),
                ]),
                ..Default::default()
            },
        ),
        // --- unsupported by the GPU tone stage (must CPU-route) ---
        (
            "vibrance_saturation_unsupported",
            EditRecipe {
                adjustments: BTreeMap::from([
                    ("vibrance".into(), 0.3),
                    ("saturation".into(), -0.1),
                ]),
                ..Default::default()
            },
        ),
        (
            "curves_s_master_unsupported",
            EditRecipe {
                curves: Some(Curves {
                    version: 1,
                    master: vec![
                        CurvePoint {
                            input: 0.0,
                            output: 0.0,
                        },
                        CurvePoint {
                            input: 0.5,
                            output: 0.4,
                        },
                        CurvePoint {
                            input: 1.0,
                            output: 1.0,
                        },
                    ],
                    channels: Default::default(),
                }),
                ..Default::default()
            },
        ),
        (
            "hsl_red_shift_unsupported",
            EditRecipe {
                hsl: Some(HslAdjustments {
                    version: 1,
                    red: Some(HslChannel {
                        hue: 0.1,
                        saturation: 0.05,
                        luminance: 0.0,
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ),
        (
            "presence_clarity_unsupported",
            EditRecipe {
                presence: Some(Presence {
                    version: 1,
                    texture: 0.0,
                    clarity: 0.25,
                    dehaze: 0.0,
                }),
                ..Default::default()
            },
        ),
        (
            "vignette_effects_unsupported",
            EditRecipe {
                effects: Some(Effects {
                    vignette: Some(Vignette {
                        version: 1,
                        amount: -0.4,
                        midpoint: 0.6,
                        roundness: 1.0,
                        feather: 0.5,
                    }),
                    grain: None,
                }),
                ..Default::default()
            },
        ),
        (
            "source_actions_unsupported",
            EditRecipe {
                source_actions: vec![SourceActionSpec {
                    version: SOURCE_ACTION_VERSION,
                    kind: SourceActionKind::DustRemoval,
                    artifact: SourceActionArtifactRef {
                        id: "repair-1".into(),
                        relative_path: "test.lumina.zdata".into(),
                        checksum: "unused".into(),
                    },
                }],
                ..Default::default()
            },
        ),
    ]
}

/// The subset of [`recipes`] the GPU tone stage cannot render (validator must
/// flag each of them; the default/tone/WB recipes must stay unflagged).
fn unsupported_recipe_names() -> &'static [&'static str] {
    &[
        "vibrance_saturation_unsupported",
        "curves_s_master_unsupported",
        "hsl_red_shift_unsupported",
        "presence_clarity_unsupported",
        "vignette_effects_unsupported",
        "source_actions_unsupported",
    ]
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

/// Per-channel + global comparison of two RGBA8 buffers of identical length.
struct DiffReport {
    /// Maximum absolute difference per channel `[R, G, B, A]`.
    max_abs_diff: [u8; 4],
    /// Peak signal-to-noise ratio in decibels (global, MAX=255). `f64::INFINITY`
    /// when the buffers are byte-identical.
    psnr_db: f64,
    /// blake3 content hash of the CPU output (hex).
    cpu_hash: String,
    /// blake3 content hash of the GPU output (hex).
    gpu_hash: String,
    /// Whether the two content hashes match (informational only).
    hashes_equal: bool,
}

/// Compare two RGBA8 pixel buffers (CPU oracle vs GPU output).
///
/// Panics if the buffers differ in length — that is a structural failure, not a
/// tolerated numerical drift.
fn compare(cpu_pixels: &[u8], gpu_pixels: &[u8]) -> DiffReport {
    assert_eq!(
        cpu_pixels.len(),
        gpu_pixels.len(),
        "CPU and GPU output buffers have different lengths"
    );

    let mut max_abs = [0u8; 4];
    let mut squared_error: f64 = 0.0;
    for (i, (pc, pg)) in cpu_pixels.iter().zip(gpu_pixels.iter()).enumerate() {
        let channel = i % 4;
        let diff = pc.abs_diff(*pg);
        if diff > max_abs[channel] {
            max_abs[channel] = diff;
        }
        let e = (i64::from(*pc) - i64::from(*pg)) as f64;
        squared_error += e * e;
    }

    let mse = squared_error / cpu_pixels.len() as f64;
    let psnr_db = if mse == 0.0 {
        f64::INFINITY
    } else {
        10.0 * (255.0f64 * 255.0 / mse).log10()
    };

    let cpu_hash = blake3_hex(cpu_pixels);
    let gpu_hash = blake3_hex(gpu_pixels);
    let hashes_equal = cpu_hash == gpu_hash;

    DiffReport {
        max_abs_diff: max_abs,
        psnr_db,
        cpu_hash,
        gpu_hash,
        hashes_equal,
    }
}

/// Simple, dependency-free PSNR over all RGBA8 bytes (MAX=255). Kept as a named
/// helper so the metric is inspectable and unit-testable in isolation.
fn psnr_db(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len(), "PSNR inputs must have equal length");
    let mut mse: f64 = 0.0;
    for (x, y) in a.iter().zip(b.iter()) {
        let e = (i64::from(*x) - i64::from(*y)) as f64;
        mse += e * e;
    }
    let mse = mse / a.len() as f64;
    if mse == 0.0 {
        f64::INFINITY
    } else {
        10.0 * (255.0f64 * 255.0 / mse).log10()
    }
}

/// Per-channel maximum absolute difference (RGBA8). Mirrors the per-channel
/// field of [`DiffReport`] but callable on arbitrary buffers.
fn max_abs_diff_per_channel(a: &[u8], b: &[u8]) -> [u8; 4] {
    assert_eq!(a.len(), b.len(), "inputs must have equal length");
    let mut max = [0u8; 4];
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        let d = x.abs_diff(*y);
        if d > max[i % 4] {
            max[i % 4] = d;
        }
    }
    max
}

/// blake3 content hash of a byte buffer, returned as a hex string.
fn blake3_hex(data: &[u8]) -> String {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize().to_hex().to_string()
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

#[test]
fn cpu_gpu_golden_equivalence() {
    // Probe the GPU once. `GpuContext::new` is graceful: it returns `Ok` even
    // when no adapter is bound (the context then degrades to the CPU fallback).
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped equivalence check");
            return;
        }
    };

    // No adapter → skip the equivalence check (do not fail headless CI).
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }

    // Bootstrap note: since REVIEW-GPU-DIVERGENCE-1 the GPU path validates the
    // recipe and CPU-routes anything its tone stage cannot render, so every
    // pair below is pixel-safe by construction; the tolerance gates still catch
    // real GPU/CPU divergence on the supported (tone/WB) recipes.
    eprintln!(
        "[INFO] lumina-gpu routes GPU-unsupported recipes to the CPU pipeline \
         (REVIEW-GPU-DIVERGENCE-1); tone/WB-only recipes exercise the real GPU \
         stage and are gated on maxAbsDiff<=1 / PSNR>=45dB below."
    );

    let frames: Vec<(&'static str, ImageFrame)> = vec![
        ("gradient_64x64", gradient_frame(64, 64)),
        ("noise_a_64x64", noise_frame(64, 64, 0x1234_5678_9ABC_DEF0)),
        ("noise_b_64x64", noise_frame(64, 64, 0xFEDC_BA98_7654_3210)),
    ];

    let mut all_passed = true;

    for (frame_name, frame) in &frames {
        for (recipe_name, recipe) in recipes() {
            // 1) CPU oracle.
            let cpu = render_frame(
                frame,
                &RenderContext {
                    recipe: &recipe,
                    camera_white_balance: None,
                    source_actions: &[],
                    masks: None,
                    lensfun: None,
                },
            )
            .expect("CPU render_frame must succeed")
            .frame;

            // 2) GPU path.
            let gpu = ctx
                .render_with_gpu(frame, &recipe)
                .expect("GPU render_with_gpu must succeed");

            // 3) Compare + gate on tolerances.
            let report = compare(&cpu.pixels, &gpu.pixels);
            eprintln!(
                "equivalence[{frame_name}/{recipe_name}]: maxAbsDiff={:?} psnr={:.2} (dB) \
                 hashes_equal={}",
                report.max_abs_diff, report.psnr_db, report.hashes_equal,
            );

            let max_violation = report
                .max_abs_diff
                .iter()
                .any(|&d| d > MAX_ABS_DIFF_TOLERANCE);
            let psnr_violation = report.psnr_db < MIN_PSNR_DB;

            if max_violation || psnr_violation {
                all_passed = false;
                eprintln!(
                    "  [FAIL] {frame_name}/{recipe_name}: \
                     maxAbsDiff tolerance (<= {MAX_ABS_DIFF_TOLERANCE}) violated = {max_violation}, \
                     PSNR tolerance (>= {MIN_PSNR_DB} dB) violated = {psnr_violation} \
                     (cpu_hash={}, gpu_hash={})",
                    report.cpu_hash, report.gpu_hash,
                );
            }
        }
    }

    assert!(
        all_passed,
        "CPU↔GPU golden equivalence failed one or more frame/recipe pairs \
         (see per-pair report above; tolerances: maxAbsDiff<= {MAX_ABS_DIFF_TOLERANCE}, \
         PSNR>= {MIN_PSNR_DB} dB)"
    );
}

/// REVIEW-GPU-DIVERGENCE-1: recipes containing stages the GPU tone stage does
/// not implement must never be rendered by the shader. `render_with_gpu` has
/// to route them to the CPU pipeline so GPU-enabled builds stay pixel-safe.
///
/// This test runs even without a GPU adapter: with an adapter it proves the
/// routing produces byte-identical output to the CPU oracle; without one the
/// CPU fallback trivially matches (and the validator assertions still hold).
#[test]
fn unsupported_recipes_route_to_cpu_byte_identically() {
    let frame = gradient_frame(64, 64);
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped routing check");
            return;
        }
    };

    for name in unsupported_recipe_names() {
        let recipe = recipes()
            .into_iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, r)| r)
            .unwrap_or_else(|| panic!("recipe basket must contain {name}"));

        // The validator must flag every unsupported recipe…
        let reasons = unsupported_gpu_stages(&recipe);
        assert!(
            !reasons.is_empty(),
            "{name} must be flagged as GPU-unsupported"
        );

        // …and the render must CPU-route to byte-identical pixels. A real
        // divergence (shader dropping the stage) would show up here.
        if !ctx.is_available() {
            eprintln!("{SKIP_MESSAGE} - validator-only assertions for {name}");
            continue;
        }
        let cpu = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: None,
            },
        )
        .unwrap_or_else(|error| panic!("CPU oracle render failed for {name}: {error}"))
        .frame;
        let gpu = ctx
            .render_with_gpu(&frame, &recipe)
            .unwrap_or_else(|error| panic!("GPU render_with_gpu failed for {name}: {error}"));
        let report = compare(&cpu.pixels, &gpu.pixels);
        assert_eq!(
            report.max_abs_diff,
            [0, 0, 0, 0],
            "{name}: CPU-routed GPU render must be byte-identical to the CPU \
             oracle (got maxAbsDiff={:?}; reasons={reasons:?})",
            report.max_abs_diff
        );
    }
}

/// Always-runs unit checks for [`unsupported_gpu_stages`]: supported recipes
/// stay unflagged; neutral/identity nested objects do not trigger false
/// positives; every unsupported stage produces its reason.
#[test]
fn gpu_support_validator_flags_exactly_the_unsupported_stages() {
    // Supported: default + tone/WB sliders only.
    assert!(unsupported_gpu_stages(&EditRecipe::default()).is_empty());
    assert!(unsupported_gpu_stages(&EditRecipe {
        adjustments: BTreeMap::from([
            ("exposure".into(), 1.0),
            ("contrast".into(), -0.2),
            ("highlights".into(), 0.1),
            ("shadows".into(), -0.1),
            ("whites".into(), 0.2),
            ("blacks".into(), -0.2),
            ("wb_temperature".into(), 5500.0),
            ("wb_tint".into(), 0.05),
        ]),
        ..Default::default()
    })
    .is_empty());

    // Neutral nested objects are NOT unsupported (identity semantics).
    assert!(unsupported_gpu_stages(&EditRecipe {
        curves: Some(Curves {
            version: 1,
            master: vec![
                CurvePoint {
                    input: 0.0,
                    output: 0.0
                },
                CurvePoint {
                    input: 1.0,
                    output: 1.0
                }
            ],
            channels: Default::default(),
        }),
        hsl: Some(HslAdjustments::default()),
        presence: Some(Presence {
            version: 1,
            texture: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
        }),
        ..Default::default()
    })
    .is_empty());

    // Each unsupported stage is flagged with a recognisable reason.
    let cases: Vec<(&str, EditRecipe)> = vec![
        (
            "vibrance",
            EditRecipe {
                adjustments: BTreeMap::from([("vibrance".into(), 0.2)]),
                ..Default::default()
            },
        ),
        (
            "saturation",
            EditRecipe {
                adjustments: BTreeMap::from([("saturation".into(), -0.5)]),
                ..Default::default()
            },
        ),
        (
            "curves",
            EditRecipe {
                curves: Some(Curves {
                    version: 1,
                    master: vec![CurvePoint {
                        input: 0.5,
                        output: 0.4,
                    }],
                    channels: Default::default(),
                }),
                ..Default::default()
            },
        ),
        (
            "hsl",
            EditRecipe {
                hsl: Some(HslAdjustments {
                    version: 1,
                    blue: Some(HslChannel {
                        hue: -0.1,
                        saturation: 0.0,
                        luminance: 0.0,
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ),
        (
            "presence",
            EditRecipe {
                presence: Some(Presence {
                    version: 1,
                    texture: 0.1,
                    clarity: 0.0,
                    dehaze: 0.0,
                }),
                ..Default::default()
            },
        ),
        (
            "effects",
            EditRecipe {
                effects: Some(Effects::default()),
                ..Default::default()
            },
        ),
        (
            "source_actions",
            EditRecipe {
                source_actions: vec![SourceActionSpec {
                    version: SOURCE_ACTION_VERSION,
                    kind: SourceActionKind::DustRemoval,
                    artifact: SourceActionArtifactRef {
                        id: "r".into(),
                        relative_path: "b.lumina.zdata".into(),
                        checksum: "c".into(),
                    },
                }],
                ..Default::default()
            },
        ),
    ];
    for (expected_reason, recipe) in cases {
        let reasons = unsupported_gpu_stages(&recipe);
        assert!(
            reasons.iter().any(|r| r.contains(expected_reason)),
            "expected a reason containing `{expected_reason}`, got {reasons:?}"
        );
    }
}

/// Always-runs sanity check: the CPU oracle must be deterministic across two
/// renders of the same frame+recipe. This gives the harness value even on
/// machines where the GPU equivalence check is skipped.
#[test]
fn cpu_render_oracle_is_deterministic() {
    let frame = gradient_frame(64, 64);
    let recipe = recipes()
        .into_iter()
        .find(|(name, _)| *name == "wb_5800_tint_0_05")
        .map(|(_, r)| r)
        .unwrap();

    let a = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
        },
    )
    .expect("first CPU render")
    .frame;
    let b = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
        },
    )
    .expect("second CPU render")
    .frame;

    assert_eq!(a, b, "CPU oracle must be deterministic");
}

// ---------------------------------------------------------------------------
// Metric unit tests — pin down the helpers so a regression in the gate math is
// caught independently of the GPU path.
// ---------------------------------------------------------------------------

#[test]
fn metrics_identical_buffers() {
    let buf = vec![10u8, 20, 30, 255, 200, 100, 50, 7];
    let report = compare(&buf, &buf);
    assert_eq!(report.max_abs_diff, [0, 0, 0, 0]);
    assert!(report.psnr_db.is_infinite());
    assert!(report.hashes_equal);
    assert_eq!(psnr_db(&buf, &buf), f64::INFINITY);
    assert_eq!(max_abs_diff_per_channel(&buf, &buf), [0, 0, 0, 0]);
}

#[test]
fn metrics_known_diff() {
    // Black (0) vs white (255) over 4 bytes: MSE = 255^2, PSNR = 0 dB.
    let black = vec![0u8; 4];
    let white = vec![255u8; 4];
    let report = compare(&black, &white);
    assert_eq!(report.max_abs_diff, [255, 255, 255, 255]);
    assert!((report.psnr_db - 0.0).abs() < 1e-9);
    assert!(!report.hashes_equal);
    assert_eq!(psnr_db(&black, &white), 0.0);
}

#[test]
fn metrics_within_tolerance() {
    // A single-step per-channel difference stays under the gate.
    let a = vec![100u8, 100, 100, 255];
    let b = vec![101u8, 99, 101, 255];
    let report = compare(&a, &b);
    assert!(report
        .max_abs_diff
        .iter()
        .all(|&d| d <= MAX_ABS_DIFF_TOLERANCE));
    assert!(report.psnr_db >= MIN_PSNR_DB);
}
