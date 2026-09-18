//! CLI-LENSFUN-GPU-1 / GPU-LENSFUN-PARITY-1 (F7, Release 1.0): routing tests
//! for the strict-corrector GPU binding (`crate::lensfun_gpu`).
//!
//! The pure `classify` cell runs in every `--features gpu` test build (no
//! native Lensfun DB, no adapter) and pins the distortion-without-crop guard.
//! The full end-to-end route cell needs a Metal adapter **and** the system
//! Lensfun DB; without either it prints an explicit SKIP verdict and returns —
//! never a silently green run (same policy as the GUI `gpu_audit`).

#![cfg(feature = "gpu")]

use super::*;
use crate::lensfun_gpu;
use lumina_core::LensfunMap;

/// A 2x2 map with the given distortion flag. Coordinates/gains are inert; only
/// `has_distortion` and the recipe crop feed [`lensfun_gpu::classify`].
fn map(has_distortion: bool) -> LensfunMap {
    LensfunMap::new(
        2,
        2,
        vec![[0.0, 0.0]; 4],
        None,
        None,
        vec![[1.0, 1.0, 1.0]; 4],
        has_distortion,
    )
    .expect("inert 2x2 map validates")
}

/// Explicit full-frame aspect crop: satisfies the `lensfun_map.default_content_crop`
/// guard (an explicit crop wins, no data-dependent maximum rectangle).
fn explicit_crop() -> Geometry {
    Geometry {
        version: 1,
        crop: Some(Crop::Aspect {
            preset: AspectPreset::Original,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    }
}

/// Distortion correction **without** an explicit crop keeps the loud CPU route
/// (the CPU oracle derives the data-dependent content rectangle; the GPU plan
/// refuses it instead of writing divergent pixels).
#[test]
fn classify_distortion_without_crop_routes_loud_cpu() {
    let reason = lensfun_gpu::classify(&map(true), &EditRecipe::default())
        .expect("distortion without an explicit crop must route CPU");
    assert_eq!(reason, lensfun_gpu::DEFAULT_CROP_REASON);
    assert!(
        reason.contains("content default crop"),
        "the reason must name the CPU content default crop, got `{reason}`"
    );
}

/// A distortion corrector **with** an explicit crop and a vignetting-only
/// corrector are both GPU-eligible (the two remaining GPU guards do not apply).
#[test]
fn classify_explicit_crop_and_vignetting_only_are_gpu_eligible() {
    let cropped = EditRecipe {
        geometry: Some(explicit_crop()),
        ..Default::default()
    };
    assert!(lensfun_gpu::classify(&map(true), &cropped).is_none());
    assert!(lensfun_gpu::classify(&map(false), &EditRecipe::default()).is_none());
}

#[cfg(feature = "lensfun")]
fn metadata(make: &str, model: &str, width: u32, height: u32) -> RawMetadata {
    RawMetadata {
        width,
        height,
        orientation: 1,
        camera_make: Some(make.to_string()),
        camera_model: Some(model.to_string()),
        iso: None,
        shutter: None,
        aperture: Some(5.6),
        lens: None,
        focal_length: Some(18.0),
        timestamp: None,
        artist: None,
        description: None,
        camera_matrix: [[0.0; 4]; 3],
        camera_white_balance: [1.0; 4],
        pre_multipliers: [1.0; 4],
        icc_profile: None,
    }
}

/// Deterministic spatial gradient (uniform frames are invariant under lensfun:
/// distortion only remaps positions and the small vignette rounds back).
#[cfg(feature = "lensfun")]
fn gradient_frame(width: u32, height: u32) -> ImageFrame {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let value = ((x / 4 + y / 4) % 256) as u8;
            pixels.extend_from_slice(&[value, value, value, 255]);
        }
    }
    ImageFrame::new(width, height, pixels).expect("gradient frame")
}

/// GPU-LENSFUN-PARITY-1 (CLI wiring): a strictly matched corrector with an
/// explicit crop must present through the GPU route with **no** CPU reason; the
/// same corrector without a crop must keep the loud CPU route. Needs a Metal
/// adapter (SKIP otherwise) and the system Lensfun DB (SKIP otherwise).
#[cfg(feature = "lensfun")]
#[test]
fn corrector_recipe_presents_gpu_without_cpu_reason() {
    let Ok(mut gpu) = GpuContext::new() else {
        eprintln!("GPU adapter unavailable - skipped CLI Lensfun GPU route test");
        return;
    };
    if !gpu.is_available() {
        eprintln!("GPU adapter unavailable - skipped CLI Lensfun GPU route test");
        return;
    }
    let (width, height) = (64u32, 48u32);
    // The same real profile the CLI/`lumina-lensfun` tests use, so a strict,
    // non-identity match is guaranteed when the system DB is installed.
    let Some((_db, corrector)) = build_lensfun_corrector(Some(&metadata(
        "Nikon Corporation",
        "Nikon D40",
        width,
        height,
    ))) else {
        eprintln!("Lensfun system profile unavailable - skipped CLI Lensfun GPU route test");
        return;
    };
    assert!(!corrector.is_identity());
    let frame = gradient_frame(width, height);

    // Positive: explicit crop → map binds → GPU route, no CPU reason.
    let recipe = EditRecipe {
        geometry: Some(explicit_crop()),
        ..Default::default()
    };
    let render_ctx = RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        depth: None,
        lensfun: Some(LensfunCorrectorRef(&corrector)),
    };
    let (_, route) = render_best_effort(
        Some(&mut gpu),
        &frame,
        &recipe,
        &render_ctx,
        GenerativeCanvasInput::default(),
    )
    .expect("corrector + explicit crop renders");
    assert!(
        route.is_gpu(),
        "a bound corrector map must present on GPU, reasons={:?}",
        route.reasons()
    );
    assert!(route.reasons().is_empty());

    // Negative: distortion without an explicit crop stays loudly on the CPU and
    // names the content-default-crop guard (never a silent divergent render).
    let no_crop = EditRecipe::default();
    let render_ctx = RenderContext {
        recipe: &no_crop,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        depth: None,
        lensfun: Some(LensfunCorrectorRef(&corrector)),
    };
    let (_, route) = render_best_effort(
        Some(&mut gpu),
        &frame,
        &no_crop,
        &render_ctx,
        GenerativeCanvasInput::default(),
    )
    .expect("corrector without crop renders on the CPU reference");
    assert!(!route.is_gpu());
    assert!(
        route
            .reasons()
            .iter()
            .any(|reason| reason.contains("default crop")),
        "the negative route must be loud, reasons={:?}",
        route.reasons()
    );
}
