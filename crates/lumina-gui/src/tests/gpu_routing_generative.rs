//! Generative GPU-present refusals through the real draft tick.
#![cfg(feature = "gpu")]

use super::*;

/// GEN-ONNX-1 Welle 2b: the artifact-blind VRAM present refusal stays specific
/// when the CPU reference succeeds (opaque auto-fill) and when it fails
/// (expand without an artifact). The failed CPU error remains visible and
/// neither route can mark VRAM fresh.
#[test]
fn generative_vram_refusal_surfaces_as_badge_end_to_end() {
    let (png, _frame) = synthetic_8x8_png();
    let mut app = new_app();
    app.load_bytes(png.clone(), "gpu-generative-badge.png")
        .unwrap();
    let _dir = app_source_path(&mut app, &png, "gpu-generative-badge.png");
    let Some(gpu) = lumina_gpu::GpuContext::new().ok() else {
        eprintln!("no GPU adapter; skipping generative VRAM-badge integration test");
        return;
    };
    if !gpu.is_available() {
        eprintln!("GPU unavailable; skipping generative VRAM-badge integration test");
        return;
    }
    app.gpu = Some(gpu);

    // An opaque source needs no auto-fill artifact, so this is an active
    // generative edit with a successful CPU reference render.
    app.set_auto_fill_transparent(true)
        .expect("opaque auto-fill CPU render succeeds without an artifact");
    assert!(
        app.error().is_none(),
        "successful CPU render must stay clean"
    );
    app.render_draft_tick([800, 600]);
    assert!(
        app.error().is_none(),
        "successful CPU reference remains valid"
    );
    assert!(
        !app.vram_fresh,
        "generative present refusal cannot mark VRAM fresh"
    );
    assert_generative_refusal_badge(&mut app);

    // Expanding without a canvas artifact deterministically fails the CPU
    // reference. The specific present refusal must not replace that error.
    let expected_cpu_error = app
        .set_expand_beyond_image(true)
        .expect_err("expand without an artifact must fail the CPU reference")
        .to_string();
    app.render_draft_tick([800, 600]);
    let visible_cpu_error = app
        .error()
        .expect("failed CPU reference error must remain visible")
        .to_owned();
    assert_eq!(
        visible_cpu_error, expected_cpu_error,
        "badge routing must not replace or rewrite the CPU render error"
    );
    assert!(
        !app.vram_fresh,
        "failed CPU reference must leave the GPU frame stale"
    );
    assert_generative_refusal_badge(&mut app);
}

fn assert_generative_refusal_badge(app: &mut LuminaApp) {
    let refusal = app
        .vram_render_refusal
        .as_deref()
        .expect("the artifact-blind VRAM refusal must be captured");
    assert!(refusal.contains("generative_edit"), "got {refusal:?}");
    app.update_texture(&egui::Context::default());
    let badge = app
        .gpu_routing_fallback_badge()
        .expect("the captured refusal must surface as a routing badge");
    assert!(badge.contains("generative_edit"), "got {badge:?}");
    assert!(
        app.gpu_unsupported_stage_reasons().is_empty(),
        "generative_edit must not be a recipe-gate reason"
    );
}
