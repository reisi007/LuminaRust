//! G-06 Lensfun TCA double-correction guard on the GPU geometry planner.
//!
//! Mirrors the CPU-oracle precedence test
//! `lumina_core::render::tests::manual_ca_skipped_under_tca_corrector_and_applied_without`
//! on the GPU path: a recipe whose manual `lens_correction` carries non-neutral
//! CA, rendered with a bound **TCA-capable** [`LensfunMap`], must skip the
//! manual CA pass (the map already corrected CA per channel) — exactly like the
//! CPU oracle. The companion cases keep the guard honest:
//!
//! * a bound map **without** TCA keeps the manual CA stage active, and
//! * **no** bound map keeps it active on the manual lens path.
//!
//! Skipping CA whenever *any* map is bound would be over-correction; not
//! skipping it under a TCA map would be the double correction G-06 forbids.
//! Each scenario compares the CA recipe against an identical recipe with only
//! the CA fields removed, so the GPU↔GPU delta isolates the manual CA stage.
//!
//! The manual lens intentionally carries a non-neutral radial term
//! (`distortion_k1`): an all-neutral manual lens currently trips a separate
//! 1-ULP boundary divergence in the GPU Lens pass (reported as a finding) that
//! would mask this precedence check. TCA and non-TCA map cases both carry the
//! same radial term, so the comparison stays apples-to-apples.
//!
//! Hardware checks skip loudly without a bound adapter (matching
//! `tests/parity.rs`). The whole harness needs the `lensfun` feature: the
//! corrector-backed map builder (and the corrector the CPU oracle compares
//! against) are only available there.

#![cfg(all(feature = "gpu", feature = "lensfun"))]
mod support;

use lumina_core::{render_frame, ImageFrame, LensfunCorrectorRef, LensfunMap, RenderContext};
use lumina_gpu::GpuContext;
use lumina_lensfun::{Corrector, LensfunDb};
use lumina_sidecar::{Crop, EditRecipe, Geometry, LensCorrection};

const W: u32 = 96;
const H: u32 = 72;
/// F-043 geometry-wave bound (see `tests/parity.rs::equivalence`): the manual
/// CA pass resamples in the same `f32` 0..=255 domain as the oracle and carries
/// at most one rounding-tie code.
const GEOMETRY_TOLERANCE: u8 = 1;
/// Structural PSNR floor for bounded stages (see `tests/parity_support`).
const MIN_PSNR_DB: f64 = 48.0;
/// Maximum absolute mean signed per-byte error for bounded stages.
const MAX_ABS_MEAN_SIGNED_ERROR: f64 = 0.05;
const SKIP_MESSAGE: &str = "GPU adapter unavailable - skipped TCA-skip check";

/// Synthetic colour gradient with per-pixel variation so a channel shift is
/// visible in every region of the frame.
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
    let mse = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let e = f64::from(*x) - f64::from(*y);
            e * e
        })
        .sum::<f64>()
        / a.len() as f64;
    if mse == 0.0 {
        f64::INFINITY
    } else {
        10.0 * (255.0f64 * 255.0 / mse).log10()
    }
}

/// Mean signed per-byte error `mean(a - b)` — catches a systematic tilt that a
/// max-only bound would let through.
fn mean_signed_error(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let sum: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| f64::from(*x) - f64::from(*y))
        .sum();
    sum / a.len() as f64
}

/// Hermetic `version_1` fixture: one camera + one lens with distortion (PTLens)
/// and vignetting (PA), optionally TCA (poly3). Returns the loaded database
/// alongside the corrector (the db must outlive it).
fn corrector(tag: &str, with_tca: bool) -> (Corrector, LensfunDb) {
    let tca = if with_tca {
        r#"<tca model="poly3" focal="50" vr="1.006" vb="0.994"/>"#
    } else {
        "<!-- no TCA calibration -->"
    };
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<lensdatabase>
    <camera><maker>Lumina GPU Corp</maker><model>Lumina GPU Body</model>
        <mount>LuminaGpuMount</mount><cropfactor>1.5</cropfactor></camera>
    <lens><maker>Lumina GPU Corp</maker><model>Lumina GPU 50mm f/2.8</model>
        <mount>LuminaGpuMount</mount><cropfactor>1.5</cropfactor>
        <calibration>
            <distortion model="ptlens" focal="50" a="0.06" b="-0.08" c="0.015"/>
            <vignetting model="pa" focal="50" aperture="2.8" distance="10" k1="-0.08" k2="-0.03" k3="-0.01"/>
            {tca}
        </calibration></lens>
</lensdatabase>
"#
    );
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "lumina-gpu-tca-skip-{tag}-{}-{seq}.xml",
        std::process::id()
    ));
    std::fs::write(&path, xml).expect("write fixture database");
    let db = LensfunDb::load_file(&path).expect("fixture database must load");
    let _ = std::fs::remove_file(&path);
    let corrector = Corrector::for_camera(
        &db,
        "Lumina GPU Corp",
        "Lumina GPU Body",
        None,
        W,
        H,
        50.0,
        2.8,
        10.0,
    )
    .expect("fixture corrector must be built");
    (corrector, db)
}

/// The baseline recipe: an explicit full-frame crop (required by the planner
/// whenever a bound map carries distortion) and no correction at all.
fn cropped_recipe() -> EditRecipe {
    EditRecipe {
        geometry: Some(Geometry {
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
        }),
        ..Default::default()
    }
}

/// A manual lens correction with a non-neutral radial term and, when
/// `with_ca`, non-neutral CA. The radial term is identical in both variants, so
/// the delta between them is exactly the manual CA stage.
fn manual_lens_recipe(with_ca: bool) -> EditRecipe {
    EditRecipe {
        lens_correction: Some(LensCorrection {
            version: 1,
            profile: None,
            distortion_k1: Some(0.12),
            distortion_k2: None,
            distortion_k3: None,
            vignette_c0: None,
            vignette_c1: None,
            vignette_c2: None,
            ca_red: with_ca.then_some(0.02),
            ca_blue: with_ca.then_some(-0.02),
        }),
        ..cropped_recipe()
    }
}

/// CPU oracle render with an optional Lensfun corrector.
fn cpu_render(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    corrector: Option<&Corrector>,
) -> ImageFrame {
    render_frame(
        frame,
        &RenderContext {
            recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: corrector.map(LensfunCorrectorRef),
            depth: None,
        },
    )
    .expect("CPU oracle render")
    .frame
}

/// Assert a bounded (F-043) GPU↔CPU match, logging the measured metrics.
fn assert_bounded_match(label: &str, cpu: &[u8], gpu: &[u8], dims: (u32, u32)) {
    let (cw, ch) = dims;
    assert_eq!(cpu.len(), gpu.len(), "{label}: pixel buffer length");
    let diff = max_abs_diff(cpu, gpu);
    let psnr = psnr_db(cpu, gpu);
    let bias = mean_signed_error(cpu, gpu);
    eprintln!(
        "lensfun-tca-skip[{label}]: dims={cw}x{ch} maxAbsDiff={diff} psnr={psnr:.2} dB bias={bias:+.4}"
    );
    assert!(
        diff <= GEOMETRY_TOLERANCE
            && psnr >= MIN_PSNR_DB
            && bias.abs() <= MAX_ABS_MEAN_SIGNED_ERROR,
        "{label}: exceeded F-043 bounds (maxAbsDiff={diff} psnr={psnr:.2} bias={bias:+.4})"
    );
}

support::gated_test!(manual_ca_skipped_under_tca_map_and_applied_without, {
    let mut ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("TCA-skip check skipped (no GPU context: {err})");
            return;
        }
    };
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let frame = gradient_frame(W, H);
    let ca_recipe = manual_lens_recipe(true);
    let no_ca_recipe = manual_lens_recipe(false);

    // --- TCA map: the manual CA stage must be skipped (no double correction) ---
    let (tca, _tca_db) = corrector("tca", true);
    let tca_map = LensfunMap::from_corrector(&tca, W, H).expect("TCA map builds");
    assert!(tca_map.has_tca(), "fixture must carry TCA coordinates");
    ctx.set_lensfun_map(Some(&tca_map)).expect("bind TCA map");

    let gpu_ca = ctx
        .render_with_gpu(&frame, &ca_recipe)
        .expect("GPU render with TCA map + manual CA");
    let gpu_no_ca = ctx
        .render_with_gpu(&frame, &no_ca_recipe)
        .expect("GPU render with TCA map, no manual CA");
    assert_eq!(
        gpu_ca.pixels, gpu_no_ca.pixels,
        "manual CA must be skipped under a TCA map (no double correction)"
    );
    let cpu_ca = cpu_render(&frame, &ca_recipe, Some(&tca));
    assert_eq!(
        max_abs_diff(&cpu_ca.pixels, &gpu_ca.pixels),
        0,
        "TCA map + manual CA must be byte-identical to the CPU oracle that skips CA too"
    );
    eprintln!("lensfun-tca-skip[tca_map/manual_ca]: maxAbsDiff=0 (CA skipped)");

    // The interactive VRAM present path (the GUI route) must skip CA identically.
    ctx.render_to_vram(&frame, &ca_recipe)
        .expect("VRAM render with TCA map + manual CA");
    let vram_ca = ctx.readback_output_frame().expect("vram readback (ca)");
    ctx.render_to_vram(&frame, &no_ca_recipe)
        .expect("VRAM render with TCA map, no manual CA");
    let vram_no_ca = ctx.readback_output_frame().expect("vram readback (no_ca)");
    assert_eq!(
        vram_ca.pixels, vram_no_ca.pixels,
        "the VRAM present path must skip manual CA under a TCA map as well"
    );
    ctx.set_lensfun_map(None).expect("clear TCA map");

    // --- Non-TCA map: the manual CA stage must stay active ---
    let (plain, _plain_db) = corrector("plain", false);
    let plain_map = LensfunMap::from_corrector(&plain, W, H).expect("non-TCA map builds");
    assert!(
        !plain_map.has_tca(),
        "the non-TCA fixture must not carry TCA coordinates"
    );
    ctx.set_lensfun_map(Some(&plain_map))
        .expect("bind non-TCA map");
    let gpu_ca = ctx
        .render_with_gpu(&frame, &ca_recipe)
        .expect("GPU render with non-TCA map + manual CA");
    let gpu_no_ca = ctx
        .render_with_gpu(&frame, &no_ca_recipe)
        .expect("GPU render with non-TCA map, no manual CA");
    assert_ne!(
        gpu_ca.pixels, gpu_no_ca.pixels,
        "manual CA must still apply when the bound map carries no TCA"
    );
    let cpu_ca = cpu_render(&frame, &ca_recipe, Some(&plain));
    assert_eq!((cpu_ca.width, cpu_ca.height), (gpu_ca.width, gpu_ca.height));
    assert_bounded_match(
        "non_tca_map/manual_ca",
        &cpu_ca.pixels,
        &gpu_ca.pixels,
        (gpu_ca.width, gpu_ca.height),
    );
    ctx.set_lensfun_map(None).expect("clear non-TCA map");

    // --- No map at all: the manual lens path must keep CA active ---
    let gpu_ca = ctx
        .render_with_gpu(&frame, &ca_recipe)
        .expect("GPU render without map + manual CA");
    let gpu_no_ca = ctx
        .render_with_gpu(&frame, &no_ca_recipe)
        .expect("GPU render without map, no manual CA");
    assert_ne!(
        gpu_ca.pixels, gpu_no_ca.pixels,
        "manual CA must still apply on the manual lens path without a map"
    );
    let cpu_ca = cpu_render(&frame, &ca_recipe, None);
    let cpu_no_ca = cpu_render(&frame, &no_ca_recipe, None);
    assert_ne!(
        cpu_ca.pixels, cpu_no_ca.pixels,
        "the CPU oracle must apply manual CA without a corrector"
    );
    assert_eq!((cpu_ca.width, cpu_ca.height), (gpu_ca.width, gpu_ca.height));
    assert_bounded_match(
        "no_map/manual_ca",
        &cpu_ca.pixels,
        &gpu_ca.pixels,
        (gpu_ca.width, gpu_ca.height),
    );
});
