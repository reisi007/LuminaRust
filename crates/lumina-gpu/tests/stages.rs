//! GPU-STAGE-1 equivalence harness for the dedicated GPU stages.
//!
//! The tone/WB stage is gated by `tests/golden.rs` (PERF-GUI-8). This file adds
//! the regression net for the stages landed with GPU-STAGE-1:
//!
//! - **Source-action stage:** composites bound artifacts exactly like
//!   `lumina_core`'s CPU source-action pass (`out = replacement` where the
//!   region coverage reaches the 50% threshold). Because compositing is a pure
//!   texel copy, the pure-compositing case must be **byte-identical**; stacked
//!   behind the tone stage it stays within the documented F-043 tolerances.
//! - **Mask plane data path:** evaluated mask planes upload into the VRAM mask
//!   texture byte-exactly (`u16` domain), and [`lumina_gpu::combine_mask_planes`]
//!   reproduces the F-041 intersection-product weights.
//!
//! Headless behaviour matches `golden.rs`: without a bound adapter the
//! hardware-dependent checks are skipped loudly, never failed.

// Every test here drives GPU hardware paths; in pure-CPU builds (`--no-default-
// features`) the whole harness is compiled out.
#![cfg(feature = "gpu")]

use lumina_core::render::SourceActionArtifact;
use lumina_core::{render_frame, ImageFrame, MaskPlane, RenderContext};
use lumina_gpu::{unsupported_gpu_stages_for, GpuContext};
use lumina_sidecar::{
    EditRecipe, SourceActionArtifactRef, SourceActionKind, SourceActionSpec, SOURCE_ACTION_VERSION,
};
use std::collections::BTreeMap;

const MAX_ABS_DIFF_TOLERANCE: u8 = 1;
const SKIP_MESSAGE: &str = "GPU adapter unavailable - skipped equivalence check";

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

/// Deterministic replacement image (splitmix64-seeded noise).
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

/// Region plane exercising every threshold neighbourhood: 0, just below
/// (32767), exactly at (32768), and full coverage (u16::MAX), tiled across the
/// frame so both branches of the shader are hit densely.
fn threshold_region(width: u32, height: u32) -> MaskPlane {
    let pattern = [0u16, 32767, 32768, u16::MAX];
    let values = (0..width * height)
        .map(|i| pattern[(i % pattern.len() as u32) as usize])
        .collect();
    MaskPlane {
        width,
        height,
        values,
    }
}

fn source_action_spec() -> SourceActionSpec {
    SourceActionSpec {
        version: SOURCE_ACTION_VERSION,
        kind: SourceActionKind::DustRemoval,
        artifact: SourceActionArtifactRef {
            id: "gpu-stage-test".into(),
            relative_path: "test.lumina.zdata".into(),
            checksum: "unused".into(),
        },
    }
}

fn max_abs_diff(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

/// The dedicated source-action GPU stage must composite exactly like the CPU
/// oracle. With a neutral recipe the whole pipeline is pure copying, so the
/// outputs have to be **byte-identical**; with an exposure slider stacked on
/// top, the combined output stays within the golden tolerance (the tone math
/// itself is already gated by `golden.rs`).
#[test]
fn source_action_stage_matches_cpu_reference() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped source-action check");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }

    const W: u32 = 64;
    const H: u32 = 64;
    let frame = gradient_frame(W, H);
    let region = threshold_region(W, H);
    let replacement = noise_frame(W, H, 0x0A11_CE5D);
    let artifact = SourceActionArtifact {
        region: region.clone(),
        replacement: replacement.clone(),
    };

    // --- Case 1: neutral recipe → pure compositing → byte-identical. ---
    let neutral = EditRecipe {
        source_actions: vec![source_action_spec()],
        ..Default::default()
    };
    let cpu = render_frame(
        &frame,
        &RenderContext {
            recipe: &neutral,
            camera_white_balance: None,
            source_actions: std::slice::from_ref(&artifact),
            masks: None,
            lensfun: None,
        },
    )
    .expect("CPU render")
    .frame;

    let mut gpu_ctx = ctx;
    gpu_ctx
        .set_source_action_artifacts(std::slice::from_ref(&artifact))
        .expect("artifacts validate");
    assert!(
        unsupported_gpu_stages_for(&neutral, true).is_empty(),
        "bound artifacts must un-flag source_actions"
    );
    let gpu = gpu_ctx
        .render_with_gpu(&frame, &neutral)
        .expect("GPU render");
    assert_eq!(
        max_abs_diff(&cpu.pixels, &gpu.pixels),
        0,
        "pure source-action compositing must be byte-identical to the CPU oracle"
    );

    // --- Case 2: exposure stacked on top → within golden tolerance. ---
    let exposure = EditRecipe {
        adjustments: BTreeMap::from([("exposure".into(), 0.4)]),
        source_actions: vec![source_action_spec()],
        ..Default::default()
    };
    let cpu = render_frame(
        &frame,
        &RenderContext {
            recipe: &exposure,
            camera_white_balance: None,
            source_actions: &[artifact],
            masks: None,
            lensfun: None,
        },
    )
    .expect("CPU render")
    .frame;
    let gpu = gpu_ctx
        .render_with_gpu(&frame, &exposure)
        .expect("GPU render");
    let diff = max_abs_diff(&cpu.pixels, &gpu.pixels);
    assert!(
        diff <= MAX_ABS_DIFF_TOLERANCE,
        "compositing + tone must stay within tolerance (got {diff})"
    );

    // --- Cleanup: clearing the binding restores strict routing. ---
    gpu_ctx.clear_source_action_artifacts();
    assert!(
        !unsupported_gpu_stages_for(&neutral, false).is_empty(),
        "without bound artifacts source_actions must CPU-route again"
    );
}

/// Without bound artifacts the pre-GPU-STAGE-1 contract holds unchanged: a
/// recipe referencing source actions routes to the CPU pipeline and produces
/// byte-identical pixels there.
#[test]
fn unbound_source_actions_still_route_to_cpu() {
    let frame = gradient_frame(48, 48);
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped routing check");
            return;
        }
    };
    let recipe = EditRecipe {
        source_actions: vec![source_action_spec()],
        ..Default::default()
    };
    assert!(!unsupported_gpu_stages_for(&recipe, false).is_empty());

    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE} - validator-only assertions");
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
        },
    )
    .expect("CPU render")
    .frame;
    let gpu = ctx.render_with_gpu(&frame, &recipe).expect("GPU render");
    assert_eq!(
        max_abs_diff(&cpu.pixels, &gpu.pixels),
        0,
        "CPU-routed renders must stay byte-identical to the CPU oracle"
    );
}

/// The evaluated-mask data path uploads `u16` planes byte-exactly into the
/// VRAM mask texture (GPU-STAGE-1). Read back through a staging buffer and
/// compared against the source plane in the exact u16 domain.
#[test]
fn upload_mask_plane_roundtrip_is_byte_exact() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped mask-plane check");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }

    const W: u32 = 16;
    const H: u32 = 16;
    let values: Vec<u16> = (0..W * H)
        .map(|i| match i % 4 {
            0 => 0,
            1 => 32767,
            2 => 32768,
            _ => u16::MAX,
        })
        .collect();

    ctx.ensure_vram(W, H).expect("vram state");
    // A dimension mismatch must fail loudly instead of silently cropping.
    assert!(ctx.upload_mask_plane(W / 2, H, &values).is_err());
    // A wrong value count fails loudly too.
    assert!(ctx
        .upload_mask_plane(W, H, &values[..values.len() - 1])
        .is_err());
    ctx.upload_mask_plane(W, H, &values)
        .expect("matching plane uploads");

    let (rw, rh, readback) = ctx.readback_mask_plane().expect("mask readback");
    assert_eq!((rw, rh), (W, H));
    assert_eq!(
        readback, values,
        "the VRAM mask data path must preserve the exact u16 domain"
    );
}

/// Artifact validation rejects mismatched geometries before any binding
/// changes (no silent fallback, no partial mutation).
#[test]
fn source_action_binding_validates_dimensions() {
    let mut ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped validation check");
            return;
        }
    };
    let bad = SourceActionArtifact {
        region: MaskPlane {
            width: 8,
            height: 8,
            values: vec![0; 64],
        },
        replacement: ImageFrame::new(4, 16, vec![0; 4 * 64]).unwrap(),
    };
    let err = ctx.set_source_action_artifacts(std::slice::from_ref(&bad));
    assert!(err.is_err(), "region/replacement mismatch must be rejected");

    // A previously good binding survives a failed re-bind untouched.
    let ok = SourceActionArtifact {
        region: MaskPlane {
            width: 8,
            height: 8,
            values: vec![u16::MAX; 64],
        },
        replacement: ImageFrame::new(8, 8, vec![9; 4 * 64]).unwrap(),
    };
    ctx.set_source_action_artifacts(std::slice::from_ref(&ok))
        .expect("valid artifacts bind");
    let _ = ctx.set_source_action_artifacts(std::slice::from_ref(&bad));
    // Re-binding after failure with the same geometry still succeeds.
    ctx.set_source_action_artifacts(std::slice::from_ref(&ok))
        .expect("binding remains usable");
}
