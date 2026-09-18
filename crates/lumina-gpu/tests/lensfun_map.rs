//! GPU-LENSFUN-PARITY-1 rework tests (verification findings F1–F4).
//!
//! The core parity/VRAM coverage lives in `tests/parity.rs`; this file pins the
//! loud-guard contracts around the caller-bound [`lumina_core::LensfunMap`]:
//!
//! * **F1** — a map whose dimensions do not match the frame is rejected by the
//!   geometry planner (loud `lensfun_map.dimensions`), on both the readback and
//!   the VRAM entry point.
//! * **F2** — a **vignetting-only** corrector (no distortion, no explicit crop)
//!   renders on the GPU with CPU-oracle parity: the CPU default-content crop is
//!   the identity for a distortion-free correction, so the GPU map step (no
//!   crop) matches byte-for-byte.
//! * **F3** — with a map bound, a recipe that would route to the internal CPU
//!   fallback (`render_cpu`) is **refused loudly** instead of silently dropping
//!   the correction.
//! * **F4** — the GPU binder validates a malformed map (core `validate` path)
//!   and changes no state.
//!
//! F1/F3/F4 use a hand-built map (no Lensfun needed); F2 builds a hermetic
//! corrector and therefore requires the `lensfun` feature. Hardware checks skip
//! loudly without an adapter, matching `tests/parity.rs`.

#![cfg(feature = "gpu")]

use lumina_core::{CoreError, ImageFrame, LensfunMap};
use lumina_gpu::{GpuContext, GpuError};
use lumina_sidecar::{Crop, EditRecipe, Geometry, LensCorrection};

const W: u32 = 24;
const H: u32 = 18;

fn gradient_frame(width: u32, height: u32) -> ImageFrame {
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            pixels.extend_from_slice(&[
                (x * 255 / width.max(1)) as u8,
                (y * 255 / height.max(1)) as u8,
                ((x ^ y) & 0xff) as u8,
                255,
            ]);
        }
    }
    ImageFrame::new(width, height, pixels).expect("synthetic frame")
}

/// A well-formed identity-warp map (`has_distortion` as requested).
fn manual_map(width: u32, height: u32, has_distortion: bool) -> LensfunMap {
    let green: Vec<[f32; 2]> = (0..height)
        .flat_map(|y| (0..width).map(move |x| [x as f32, y as f32]))
        .collect();
    let gain = vec![[1.0f32; 3]; (width * height) as usize];
    LensfunMap::new(width, height, green, None, None, gain, has_distortion).expect("valid map")
}

/// A schema-valid recipe with an explicit full-frame crop (isolates the map
/// guards from the default-content-crop class).
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

/// F1: a map dimension mismatch is a loud planner error on both entry points.
#[test]
fn f1_mismatched_map_dimensions_are_refused_loudly() {
    let mut ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("F1 skipped (no GPU context: {err})");
            return;
        }
    };
    let frame = gradient_frame(W, H);
    // Internally valid map, but one pixel wider than the frame.
    let map = manual_map(W + 1, H, true);
    ctx.set_lensfun_map(Some(&map))
        .expect("bind valid-shaped map");

    // VRAM entry: the planner runs before the adapter check.
    match ctx.render_to_vram(&frame, &cropped_recipe()) {
        Err(GpuError::Core(CoreError::InvalidAdjustment { name, .. })) => {
            assert!(
                name.contains("lensfun_map.dimensions"),
                "VRAM must reject with the dimensions guard, got {name}"
            );
        }
        other => panic!("VRAM must refuse a mismatched map loudly, got {other:?}"),
    }

    // Readback entry: without an adapter the map-bound CPU fallback refuses
    // first, so the planner path needs a real adapter.
    if ctx.is_available() {
        match ctx.render_with_gpu(&frame, &cropped_recipe()) {
            Err(GpuError::Core(CoreError::InvalidAdjustment { name, .. })) => {
                assert!(
                    name.contains("lensfun_map.dimensions"),
                    "readback must reject with the dimensions guard, got {name}"
                );
            }
            other => panic!("readback must refuse a mismatched map loudly, got {other:?}"),
        }
    } else {
        eprintln!("F1 readback planner assertion skipped (no GPU adapter)");
    }
    ctx.set_lensfun_map(None).expect("clear map");
}

/// F3: a bound map turns a would-be CPU fallback into a loud refusal.
#[test]
fn f3_bound_map_refuses_internal_cpu_fallback() {
    let mut ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("F3 skipped (no GPU context: {err})");
            return;
        }
    };
    ctx.set_lensfun_map(Some(&manual_map(W, H, true)))
        .expect("bind map");
    // Manual lens without an explicit crop → the recipe-only gate routes to the
    // CPU (default content crop). With a map bound that fallback must refuse.
    let recipe = EditRecipe {
        lens_correction: Some(LensCorrection {
            version: 1,
            profile: None,
            distortion_k1: Some(0.05),
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
    let frame = gradient_frame(W, H);
    match ctx.render_with_gpu(&frame, &recipe) {
        Err(GpuError::RenderFailed(message)) => assert!(
            message.contains("Lensfun map"),
            "the refusal must name the bound map, got {message}"
        ),
        other => panic!("a bound map must refuse the internal CPU fallback, got {other:?}"),
    }
    match ctx.render_to_vram(&frame, &recipe) {
        Err(GpuError::RenderFailed(message)) => assert!(
            message.contains("GPU-unsupported stage"),
            "the VRAM path must refuse the unsupported recipe loudly, got {message}"
        ),
        other => panic!("VRAM must refuse the unsupported recipe, got {other:?}"),
    }
    ctx.set_lensfun_map(None).expect("clear map");
}

/// F4: the GPU binder validates a malformed map and stores no state.
#[test]
fn f4_malformed_map_is_rejected_on_bind() {
    let mut ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("F4 skipped (no GPU context: {err})");
            return;
        }
    };
    // Public fields allow an internally inconsistent literal the constructor
    // would reject: one green coordinate short of width*height.
    let malformed = LensfunMap {
        width: W,
        height: H,
        green: vec![[0.0, 0.0]; (W * H) as usize - 1],
        red: None,
        blue: None,
        gain: vec![[1.0; 3]; (W * H) as usize],
        has_distortion: false,
    };
    match ctx.set_lensfun_map(Some(&malformed)) {
        Err(GpuError::Core(CoreError::InvalidMaskPlane { .. })) => {}
        other => panic!("the binder must reject a malformed map, got {other:?}"),
    }
    // No state was stored: an empty recipe renders through the plain tone path
    // (byte-neutral) instead of the missing map.
    let frame = gradient_frame(8, 6);
    let out = ctx
        .render_with_gpu(&frame, &EditRecipe::default())
        .expect("no map must remain bound after the rejected bind");
    assert_eq!(out.width, frame.width);
    assert_eq!(out.height, frame.height);
}

/// F2: a vignetting-only corrector (no distortion, no explicit crop) renders on
/// the GPU with CPU-oracle parity.
#[cfg(feature = "lensfun")]
#[test]
fn f2_vignetting_only_corrector_matches_cpu_oracle() {
    use lumina_core::{render_frame, LensfunCorrectorRef, RenderContext};
    use lumina_lensfun::{Corrector, LensfunDb};

    let mut ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("F2 skipped (no GPU context: {err})");
            return;
        }
    };
    if !ctx.is_available() {
        eprintln!("F2 skipped (no GPU adapter)");
        return;
    }
    let (w, h) = (W, H);
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<lensdatabase>
    <camera><maker>Lumina Vign Corp</maker><model>Lumina Vign Body</model>
        <mount>LuminaVignMount</mount><cropfactor>1.5</cropfactor></camera>
    <lens><maker>Lumina Vign Corp</maker><model>Lumina Vignetting 50mm f/2.8</model>
        <mount>LuminaVignMount</mount><cropfactor>1.5</cropfactor>
        <calibration>
            <vignetting model="pa" focal="50" aperture="2.8" distance="10" k1="-0.08" k2="-0.03" k3="-0.01"/>
        </calibration></lens>
</lensdatabase>
"#;
    let path = std::env::temp_dir().join(format!("lumina-gpu-vign-{}.xml", std::process::id()));
    std::fs::write(&path, xml).expect("write fixture database");
    let db = LensfunDb::load_file(&path).expect("fixture database must load");
    let _ = std::fs::remove_file(&path);
    let corrector = Corrector::for_camera(
        &db,
        "Lumina Vign Corp",
        "Lumina Vign Body",
        None,
        w,
        h,
        50.0,
        2.8,
        10.0,
    )
    .expect("vignetting-only corrector must build");
    assert!(
        !corrector.has_distortion(),
        "fixture must have no distortion"
    );

    let map = LensfunMap::from_corrector(&corrector, w, h).expect("map builds");
    assert!(
        !map.has_distortion,
        "a vignetting-only map must not carry the default-crop class"
    );
    ctx.set_lensfun_map(Some(&map)).expect("bind map");

    // No explicit crop: the CPU oracle's default crop is the identity for a
    // distortion-free correction, so the GPU map step must match it exactly.
    let recipe = EditRecipe::default();
    for (frame_name, frame) in [
        ("gradient", gradient_frame(w, h)),
        ("noise", {
            let mut pixels = Vec::with_capacity((w * h * 4) as usize);
            let mut state = 0x1234_5678_9abc_def0u64;
            for _ in 0..w * h {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                pixels.extend_from_slice(&[(state >> 33) as u8, (state >> 41) as u8, 90, 255]);
            }
            ImageFrame::new(w, h, pixels).unwrap()
        }),
    ] {
        let cpu = render_frame(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: Some(LensfunCorrectorRef(&corrector)),
                depth: None,
            },
        )
        .expect("CPU oracle render")
        .frame;
        let gpu = ctx
            .render_with_gpu(&frame, &recipe)
            .unwrap_or_else(|e| panic!("{frame_name}: GPU render: {e}"));
        assert_eq!(
            (cpu.width, cpu.height),
            (gpu.width, gpu.height),
            "{frame_name}: dimensions"
        );
        let diff = cpu
            .pixels
            .iter()
            .zip(gpu.pixels.iter())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        eprintln!("lensfun-vign[{frame_name}]: maxAbsDiff={diff}");
        assert_eq!(
            diff, 0,
            "{frame_name}: vignetting-only map must match the CPU oracle"
        );
    }
    ctx.set_lensfun_map(None).expect("clear map");
}
