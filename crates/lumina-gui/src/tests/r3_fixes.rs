//! R3-Runde-3 routing/denoise fixes (F-103-N6):
//!
//! * **R3-ROUTING-1** — while the interactive crop tool is armed the CPU
//!   preview renders the geometry-free full frame; the GPU present must
//!   evaluate that same recipe, otherwise a committed crop's dimension change
//!   refuses every tick and the display silently splits.
//! * **R3-DENOISE-2** — the recipe-gate CPU route must log exactly once per
//!   reason set (the visible badge previously had no log line).
//! * **R3-DENOISE-1** — the neighbor-preview worker must follow the same
//!   denoise fallback policy as the active render (no hard fail where the
//!   active path falls back).
//!
//! Split into its own file for the file-size ratchet (new logic in a new,
//! coherent test module; `tests/gpu_routing.rs` stays at its committed size).

use super::*;

/// R3-ROUTING-1: with the interactive crop tool armed the preview shows the
/// geometry-free full frame, so the GPU present path must evaluate **exactly**
/// that recipe. Adapter-independent — it pins the recipe selection only; the
/// VRAM-present half is covered by
/// [`Self::crop_mode_draft_tick_presents_gpu_without_badge`].
#[test]
#[cfg(feature = "gpu")]
fn crop_mode_gpu_present_recipe_drops_geometry() {
    let mut app = new_app();
    app.load_bytes(png(), "crop-recipe.png").unwrap();
    app.set_crop_free(0.1, 0.1, 0.5, 0.5).unwrap();
    assert!(
        app.recipe
            .geometry
            .as_ref()
            .and_then(|geometry| geometry.crop.as_ref())
            .is_some(),
        "the fixture must commit a crop"
    );

    app.crop_mode = false;
    assert!(
        app.gpu_present_recipe().geometry.is_some(),
        "with the crop tool disarmed the GPU must evaluate the real (cropped) recipe"
    );

    // R3-LOG-1: the routing decision must be trace-visible.
    let _ = crate::timing::take_timing_log();
    app.crop_mode = true;
    assert!(
        app.gpu_present_recipe().geometry.is_none(),
        "with the crop tool armed the GPU must evaluate the geometry-free display recipe"
    );
    assert!(
        crate::timing::take_timing_log()
            .iter()
            .any(|line| line.contains("recipe=crop-display")),
        "the crop-display routing decision must be logged"
    );
}

/// R3-ROUTING-1 end-to-end: with a real adapter and the crop tool armed, a
/// committed crop no longer makes every draft tick fall back to the CPU — the
/// hot path renders the geometry-free display recipe on VRAM and the routing
/// badge stays absent. Skips (with a loud comment) without a usable adapter,
/// like the other VRAM-path tests.
#[test]
#[cfg(feature = "gpu")]
fn crop_mode_draft_tick_presents_gpu_without_badge() {
    use crate::timing::take_vram_refusal_warns;

    let (png, _frame) = synthetic_8x8_png();
    let mut app = new_app();
    app.load_bytes(png.clone(), "gpu-crop-present.png").unwrap();
    let _dir = app_source_path(&mut app, &png, "gpu-crop-present.png");
    let Some(gpu) = lumina_gpu::GpuContext::new().ok() else {
        eprintln!("no GPU adapter; skipping crop-mode VRAM present test");
        return;
    };
    if !gpu.is_available() {
        eprintln!("GPU unavailable; skipping crop-mode VRAM present test");
        return;
    }
    app.gpu = Some(gpu);
    app.set_crop_free(0.1, 0.1, 0.5, 0.5).unwrap();
    app.toggle_crop_mode();
    assert!(app.crop_mode, "the crop tool must be armed");
    let _ = take_vram_refusal_warns();

    app.render_draft_tick([800, 600]);
    assert!(
        app.vram_fresh,
        "R3-ROUTING-1: the crop-mode draft tick must present from VRAM"
    );
    assert!(
        app.vram_render_refusal.is_none(),
        "no present refusal while the crop tool is armed: {:?}",
        app.vram_render_refusal
    );
    assert_eq!(
        take_vram_refusal_warns(),
        0,
        "no refusal warning may fire while the GPU presents"
    );

    app.update_texture(&egui::Context::default());
    assert!(
        app.gpu_routing_fallback_badge().is_none(),
        "the crop-mode present must not raise a CPU-routing badge, got {:?}",
        app.gpu_routing_fallback_badge()
    );
}

/// R3-DENOISE-2: the recipe-gate CPU route warns exactly once per reason set —
/// never per frame/tick — and re-arms when the set changes or clears.
/// Adapter-independent (the verdict memo + log are adapter-free).
#[test]
#[cfg(feature = "gpu")]
fn gate_route_warns_once_per_reason_set() {
    use crate::timing::take_gpu_gate_route_warns;

    let mut app = new_app();
    app.load_bytes(png(), "gate-route.png").unwrap();
    app.render().unwrap();
    let _ = take_gpu_gate_route_warns();

    // Activate a recipe-gate reason (`denoise_ai (not GPU-wired)`).
    app.set_denoise_enabled(true).unwrap();
    app.render().unwrap();
    let reasons = app.gpu_unsupported_stage_reasons();
    assert!(
        reasons.iter().any(|reason| reason.contains("denoise_ai")),
        "the fixture must carry a recipe-gate reason, got {reasons:?}"
    );
    assert_eq!(
        take_gpu_gate_route_warns(),
        1,
        "the first gate verdict of a reason set must warn exactly once"
    );

    // A new render key with the *same* reason set must not warn again: change
    // a non-stage adjustment (exposure) so the render key changes while the
    // gate verdict stays identical. This exercises the dedup against the
    // previous verdict (a mutation dropping it would warn here).
    app.set_adjustment("exposure", 0.5);
    app.render().unwrap();
    let _ = app.gpu_unsupported_stage_reasons();
    assert_eq!(
        take_gpu_gate_route_warns(),
        0,
        "a new render key with an unchanged reason set must not re-warn"
    );

    // A changed reason set warns again.
    app.recipe.lens_correction = Some(lumina_sidecar::LensCorrection {
        version: 1,
        profile: None,
        distortion_k1: None,
        distortion_k2: None,
        distortion_k3: None,
        vignette_c0: None,
        vignette_c1: None,
        vignette_c2: None,
        ca_red: None,
        ca_blue: None,
    });
    app.render().unwrap();
    let _ = app.gpu_unsupported_stage_reasons();
    assert_eq!(
        take_gpu_gate_route_warns(),
        1,
        "a changed reason set must warn again"
    );

    // An empty verdict re-arms the next occurrence of the same set.
    app.recipe.lens_correction = None;
    app.set_denoise_enabled(false).unwrap();
    app.render().unwrap();
    assert!(
        app.gpu_unsupported_stage_reasons().is_empty(),
        "the cleared recipe must produce an empty gate verdict"
    );
    app.set_denoise_enabled(true).unwrap();
    app.render().unwrap();
    let _ = app.gpu_unsupported_stage_reasons();
    assert_eq!(
        take_gpu_gate_route_warns(),
        1,
        "an empty verdict must re-arm the next occurrence"
    );
}

/// R3-DENOISE-2 end-to-end: the real badge producer
/// ([`LuminaApp::routing_fallback_reason`] behind `update_texture`) logs the
/// recipe-gate CPU route exactly once across repeated frames. Skips loudly
/// without a usable adapter (the badge requires a bound GPU context).
#[test]
#[cfg(feature = "gpu")]
fn gate_route_is_logged_once_through_update_texture() {
    use crate::timing::take_gpu_gate_route_warns;

    let ctx = egui::Context::default();
    let mut app = new_app();
    attach_wgpu_render_state(&mut app, None);
    if !app.gpu_adapter_available() {
        eprintln!("GPU adapter unavailable; skipping gate-route logging test");
        return;
    }
    app.load_bytes(png(), "gate-route-e2e.png").unwrap();
    app.set_denoise_enabled(true).unwrap();
    app.render().unwrap();
    let _ = take_gpu_gate_route_warns();

    app.update_texture(&ctx);
    app.update_texture(&ctx);
    app.update_texture(&ctx);
    assert_eq!(
        take_gpu_gate_route_warns(),
        1,
        "the gate CPU route must warn exactly once across frames"
    );
    let badge = app.gpu_routing_fallback_badge().map(str::to_owned);
    assert!(
        badge
            .as_deref()
            .is_some_and(|badge| badge.contains("denoise_ai")),
        "the visible badge must name the gate reason, got {badge:?}"
    );
}

/// R3-DENOISE-1: a neighbor whose recipe carries an active `denoise_ai` must
/// follow the *same* fallback policy as the active render. `Warn` (the GUI
/// default) falls back visibly — the downscaled stand-in still renders —
/// instead of the former core `Strict` default that hard-failed every such
/// neighbor while the active image fell back. `Strict` still aborts loudly,
/// exactly like the active render.
#[test]
fn neighbor_denoise_stage_follows_the_app_policy() {
    use lumina_core::preview_cache::PreviewKind;
    use lumina_core::{DenoisePolicy, ImageFileFormat};
    use lumina_sidecar::{
        save_sidecar, sidecar_path_for, DecodeFingerprint, GeometryFingerprint, SidecarDocument,
        SourceIdentity,
    };

    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("denoise-neighbor.png");
    let png = crate::preview_ctrl::make_frame(8, 8, 90)
        .encode(ImageFileFormat::Png)
        .unwrap();
    std::fs::write(&source, &png).unwrap();

    // A sidecar whose active copy carries the documented pending-integration
    // denoise stage (non-ready by design, no weights).
    let identity = SourceIdentity {
        relative_name: "denoise-neighbor.png".into(),
        content_hash: format!("blake3:{}", blake3::hash(&png).to_hex()),
        byte_length: png.len() as u64,
        modified_at: None,
        raw_format: "PNG".into(),
        orientation: 1,
        decode_fingerprint: DecodeFingerprint {
            decoder: "native".into(),
            version: "1".into(),
            parameters: Default::default(),
            extras: Default::default(),
        },
        geometry_fingerprint: GeometryFingerprint {
            width: 8,
            height: 8,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: Default::default(),
        },
        extras: Default::default(),
    };
    let mut document = SidecarDocument::new(identity, "raster-mvp-1");
    document.virtual_copies[0].recipe.denoise_ai = Some(crate::denoise_gui::default_denoise_ai());
    save_sidecar(&sidecar_path_for(&source), &document).unwrap();

    let job = |policy| crate::preview_ctrl::PreviewJob {
        probe_id: "denoise-neighbor".into(),
        source: source.clone(),
        name: "denoise-neighbor.png".into(),
        virtual_copy: "vc-original".into(),
        target: (8, 8),
        kind: PreviewKind::Screen,
        priority: 0,
        denoise_policy: policy,
    };

    assert!(
        crate::preview_jobs::worker_preview(job(DenoisePolicy::Warn)).is_ok(),
        "R3-DENOISE-1: a non-ready denoise stage must fall back under Warn, not fail"
    );

    let error = match crate::preview_jobs::worker_preview(job(DenoisePolicy::Strict)) {
        Ok(_) => panic!("Strict must abort the neighbor render loudly"),
        Err(error) => error,
    };
    assert!(
        error.contains("unavailable"),
        "the strict abort must name the status: {error}"
    );
}

/// B1 (R3-ROUTING-1 regression): the recipe gate must evaluate the **present**
/// recipe. With the crop tool armed, a committed crop **and** a lens/perspective
/// correction, the display recipe has no explicit crop → its
/// `geometry (default content crop)` reason must reach the gate (badge), and the
/// doomed `render_to_vram` attempt must be skipped (exactly one warning, no
/// per-tick spam). Adapter-independent gate half + Metal hot-path half.
#[test]
#[cfg(feature = "gpu")]
fn crop_mode_lens_default_content_crop_is_loud_not_silent() {
    use crate::timing::{take_gpu_gate_route_warns, take_vram_refusal_warns};

    let (png, _frame) = synthetic_8x8_png();
    let mut app = new_app();
    app.load_bytes(png.clone(), "gpu-crop-lens.png").unwrap();
    let _dir = app_source_path(&mut app, &png, "gpu-crop-lens.png");
    // A lens correction counts as active even when neutral; with the crop tool
    // armed the display recipe drops the explicit crop → default content crop.
    app.recipe.lens_correction = Some(lumina_sidecar::LensCorrection {
        version: 1,
        profile: None,
        distortion_k1: None,
        distortion_k2: None,
        distortion_k3: None,
        vignette_c0: None,
        vignette_c1: None,
        vignette_c2: None,
        ca_red: None,
        ca_blue: None,
    });
    app.set_crop_free(0.1, 0.1, 0.5, 0.5).unwrap();
    app.toggle_crop_mode();
    assert!(app.crop_mode, "the crop tool must be armed");

    // Adapter-independent: the gate judges the present (geometry-free) recipe.
    let reasons = app.gpu_unsupported_stage_reasons();
    assert!(
        reasons
            .iter()
            .any(|reason| reason == "geometry (default content crop)"),
        "the gate must name the display recipe's default content crop, got {reasons:?}"
    );

    let Some(gpu) = lumina_gpu::GpuContext::new().ok() else {
        eprintln!("no GPU adapter; skipping crop/lens default-content-crop hot-path test");
        return;
    };
    if !gpu.is_available() {
        eprintln!("GPU unavailable; skipping crop/lens default-content-crop hot-path test");
        return;
    }
    app.gpu = Some(gpu);
    let _ = take_vram_refusal_warns();
    let _ = take_gpu_gate_route_warns();

    for _ in 0..3 {
        app.render_draft_tick([800, 600]);
    }
    assert!(
        app.vram_render_refusal
            .as_deref()
            .is_some_and(|reason| reason.contains("default content crop")),
        "the refusal must be recorded for the badge, got {:?}",
        app.vram_render_refusal
    );
    assert_eq!(
        take_vram_refusal_warns(),
        1,
        "the gate refusal must warn exactly once across three ticks, not per tick"
    );
    assert_eq!(
        take_gpu_gate_route_warns(),
        1,
        "the gate route must warn exactly once across three ticks"
    );

    app.update_texture(&egui::Context::default());
    let badge = app.gpu_routing_fallback_badge().map(str::to_owned);
    assert!(
        badge
            .as_deref()
            .is_some_and(|badge| badge.contains("default content crop")),
        "no silent CPU route: the badge must name the reason, got {badge:?}"
    );
}

/// B4 (R3-DENOISE-1): the navigator overview must follow the session denoise
/// policy like the active preview / neighbor worker — `Warn` falls back
/// visibly, `Strict` aborts (no silent `.ok()?` path split).
#[test]
fn navigator_overview_follows_the_session_denoise_policy() {
    use lumina_core::DenoisePolicy;

    let mut app = new_app();
    app.load_bytes(png(), "nav-denoise.png").unwrap();
    app.set_denoise_enabled(true).unwrap();
    assert_eq!(app.denoise_policy(), DenoisePolicy::Warn);
    assert!(
        app.navigator_zoomed_overview().is_some(),
        "R3-DENOISE-1 (B4): Warn must fall back, the navigator must not silently vanish"
    );

    // Strict aborts; a recipe change forces a fresh render key.
    app.set_denoise_policy(DenoisePolicy::Strict);
    app.set_denoise_strength(0.4).unwrap();
    assert!(
        app.navigator_zoomed_overview().is_none(),
        "Strict must not produce a navigator stand-in"
    );
    // The failure is remembered under the key: a second call neither re-renders
    // nor re-logs.
    assert!(
        app.navigator_zoomed_overview().is_none(),
        "a remembered failure must stay silent on the next frame"
    );
}
