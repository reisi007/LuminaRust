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
use lumina_gpu::GpuContext;
use lumina_sidecar::EditRecipe;
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

    // Bootstrap note: until the GPU DAG is wired, `render_with_gpu` routes
    // through the CPU pipeline. Flag this loudly rather than hiding it — the
    // comparison below is currently CPU-vs-CPU and becomes meaningful once the
    // shader/tiling stages land.
    eprintln!(
        "[WARN] lumina-gpu render_with_gpu still uses the CPU fallback in this \
         bootstrap; equivalence is CPU-vs-CPU until the GPU DAG lands. A real \
         GPU/CPU divergence would be caught by the maxAbsDiff<=1 / PSNR>=45dB \
         gates below."
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
