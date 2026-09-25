//! GPU routing refusal badges tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).
#![cfg(feature = "gpu")]

use super::*;

/// GPU-LENSFUN-PARITY-1: a non-identity Lensfun corrector is **no longer** a
/// recipe-gate reason. The present path binds its `LensfunMap` on the GPU
/// (`lensfun_gpu::bind`), and the honest CPU routes (unbindable map, the
/// `lumina-gpu` map guards) surface through `vram_render_refusal` instead.
/// This pins the gate half; the GPU bind/render half is covered headless by
/// `gpu_audit_lensfun_corrector_presents_gpu_without_badge` and the guard
/// classification by [`Self::lensfun_map_refusal_is_classified_for_the_badge`].
#[test]
#[cfg(all(feature = "gpu", feature = "lensfun"))]
fn active_lensfun_corrector_is_not_a_recipe_gate_reason() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = new_app();
    app.load_bytes(png(), "lensfun-gate.png").unwrap();
    app.render().unwrap();
    assert!(app.render_key.is_some());

    // No corrector yet: the default recipe is fully GPU-eligible and the
    // verdict is memoized against the render key.
    assert!(
        !app.recipe_has_unsupported_gpu_stages(),
        "without a corrector the default recipe must stay GPU-eligible"
    );
    assert!(
        app.gpu_stage_gate.is_some(),
        "the verdict must be memoized against the render key"
    );

    // A genuinely non-identity corrector changes CPU pixels, but must not
    // add a recipe-gate reason: the present path binds its map on the GPU.
    let (corrector, db) = synthetic_lensfun_corrector(directory.path());
    assert!(
        !corrector.is_identity(),
        "fixture profile must be a real (non-identity) correction"
    );
    app.lensfun_cache = Some(CachedLensCorrector {
        corrector,
        _db: db,
        key: (
            Some("Lumina Test Corp".into()),
            Some("Lumina Test Body".into()),
            None,
            640,
            480,
            50.0f32.to_bits(),
            2.8f32.to_bits(),
        ),
        active: true,
        gpu_map: None,
    });
    assert!(
        app.gpu_unsupported_reasons().is_empty(),
        "GPU-LENSFUN-PARITY-1: an active corrector must not block the recipe gate, got {:?}",
        app.gpu_unsupported_reasons()
    );
    app.render().unwrap();
    assert!(
        !app.recipe_has_unsupported_gpu_stages(),
        "GPU-LENSFUN-PARITY-1: a corrector recipe must stay GPU-eligible \
         (no stale recipe-gate reason)"
    );

    // Clearing the corrector keeps the recipe eligible and the memo honest.
    app.lensfun_cache = None;
    app.render().unwrap();
    assert!(
        !app.recipe_has_unsupported_gpu_stages(),
        "clearing the corrector must keep the GPU eligibility"
    );
}

/// GPU-LENSFUN-PARITY-1: the two `lumina-gpu` map guards are classified into
/// a precise badge reason — the core guard refuses, the GUI names it. Pure
/// classification (no adapter), so it runs in the normal suite; it pins the
/// "no silent CPU route" contract for a distortion corrector without an
/// explicit crop and for a dimension-mismatched bound map.
#[test]
#[cfg(all(feature = "gpu", feature = "lensfun"))]
fn lensfun_map_refusal_is_classified_for_the_badge() {
    let default_crop = lumina_gpu::GpuError::Core(lumina_core::CoreError::InvalidAdjustment {
        name: "lensfun_map.default_content_crop".into(),
        value: 0.0,
        minimum: 0.0,
        maximum: 1.0,
    });
    let reason = LuminaApp::classify_vram_refusal(&default_crop)
        .expect("the default-content-crop guard must be classified");
    assert!(reason.contains("Lensfun corrector"), "got {reason:?}");
    assert!(reason.contains("default crop"), "got {reason:?}");

    let mismatch = lumina_gpu::GpuError::Core(lumina_core::CoreError::InvalidAdjustment {
        name: "lensfun_map.dimensions".into(),
        value: 1.0,
        minimum: 1.0,
        maximum: 1.0,
    });
    let reason = LuminaApp::classify_vram_refusal(&mismatch)
        .expect("the map-dimension guard must be classified");
    assert!(reason.contains("map dimensions"), "got {reason:?}");

    // Unrelated failures stay unclassified (their existing loud handling).
    let other = lumina_gpu::GpuError::RenderFailed("device lost".into());
    assert!(
        LuminaApp::classify_vram_refusal(&other).is_none(),
        "an unrelated VRAM failure must not be labelled as a Lensfun route"
    );
}

/// GUI-LENSFUN-GATE-2: the visible routing badge names the precise reason,
/// not just the generic headline — `geometry (default content crop)` and an
/// invalid As-Shot context are VRAM-gate refusals. The Lensfun map guards
/// are covered separately by
/// [`Self::lensfun_map_refusal_is_classified_for_the_badge`]. Pure text
/// check, so it runs without a bound adapter.
#[test]
#[cfg(feature = "gpu")]
fn routing_fallback_badge_names_the_precise_reason() {
    let mut app = new_app();
    app.load_bytes(png(), "routing-reason.png").unwrap();
    app.render().unwrap();

    // A lens correction without an explicit crop activates the CPU oracle's
    // content-based default crop — a recipe-expressible CPU-routing reason.
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
    app.render_key = None; // no key → fresh verdict, never a stale memo hit
    let reasons = app.gpu_unsupported_stage_reasons();
    assert!(
        reasons
            .iter()
            .any(|r| r == "geometry (default content crop)"),
        "got {reasons:?}"
    );
    let badge = LuminaApp::format_routing_fallback_reason(&reasons).expect("badge");
    assert!(badge.contains(Str::CpuFallbackUnsupportedStages.t()));
    assert!(badge.contains("geometry (default content crop)"));

    // An invalid As-Shot white balance is an equally precise reason.
    app.recipe.lens_correction = None;
    app.camera_white_balance = Some([0.0, 1.0, 1.0, 1.0]);
    app.render_key = None;
    let reasons = app.gpu_unsupported_stage_reasons();
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("camera_white_balance (invalid")),
        "got {reasons:?}"
    );
    let badge = LuminaApp::format_routing_fallback_reason(&reasons).expect("badge");
    assert!(badge.contains("camera_white_balance (invalid"));

    // No capability reason → no badge (never a fallback label without cause).
    app.camera_white_balance = None;
    app.render_key = None;
    assert!(app.gpu_unsupported_stage_reasons().is_empty());
    assert!(LuminaApp::format_routing_fallback_reason(&[]).is_none());
}

/// GUI-LENSFUN-GATE-3 (F1): a dimension-changing geometry chain is refused
/// by `render_to_vram` **after** the recipe gate passed, so the gate alone
/// left that CPU route without a badge. The refusal is classified into the
/// documented `lumina-gpu` reason and, with an empty gate, becomes the
/// visible badge — no silent CPU route. Pure classification + merge, so it
/// runs without a bound adapter.
#[test]
#[cfg(feature = "gpu")]
fn dimension_changing_vram_refusal_earns_a_badge_with_empty_gate() {
    // Mirrors the real `render_to_vram` error for a crop/rotation chain.
    let refusal = lumina_gpu::GpuError::RenderFailed(
        "VRAM path cannot present geometry (dimension-changing output; the VRAM \
         present texture is source-sized); render it through the full CPU \
         reference (render_frame) instead"
            .to_string(),
    );
    let reason = LuminaApp::classify_vram_refusal(&refusal)
        .expect("a dimension-changing refusal must be classified");
    assert!(
        reason.contains("dimension-changing output"),
        "got {reason:?}"
    );

    // A non-geometry refusal is not misclassified as a visible badge reason
    // (it keeps its existing gate/warn path).
    let other = lumina_gpu::GpuError::AdapterUnavailable("device lost".into());
    assert!(LuminaApp::classify_vram_refusal(&other).is_none());

    // Gate empty + captured refusal → a badge naming the precise reason.
    let reasons = LuminaApp::combine_routing_reasons(Vec::new(), Some(&reason));
    assert_eq!(reasons, vec![reason.clone()]);
    let badge = LuminaApp::format_routing_fallback_reason(&reasons).expect("badge");
    assert!(badge.contains(Str::CpuFallbackUnsupportedStages.t()));
    assert!(badge.contains("dimension-changing output"));
}

/// GEN-ONNX-1 Welle 2b (point 3): the readback-free VRAM present path is
/// artifact-blind. An active generative recipe is refused by `lumina-gpu`
/// with `generative_artifact.missing`; the GUI classifies that refusal into
/// a visible badge (never a swallowed `warn!`-only CPU route), while the
/// artifact-aware CPU preview still renders the supplied canvas. Pure
/// classification, so it runs without a bound adapter.
#[test]
#[cfg(feature = "gpu")]
fn artifact_blind_vram_refusal_earns_a_badge() {
    let refusal = lumina_gpu::GpuError::Core(lumina_core::CoreError::InvalidAdjustment {
        name: "generative_artifact.missing (GPU entry without artifacts)".into(),
        value: 0.0,
        minimum: 0.0,
        maximum: 1.0,
    });
    let reason = LuminaApp::classify_vram_refusal(&refusal)
        .expect("an artifact-blind generative refusal must be classified");
    assert!(reason.contains("generative_edit"), "got {reason:?}");
    let reasons = LuminaApp::combine_routing_reasons(Vec::new(), Some(&reason));
    let badge = LuminaApp::format_routing_fallback_reason(&reasons).expect("badge");
    assert!(badge.contains(Str::CpuFallbackUnsupportedStages.t()));
    assert!(badge.contains("generative_edit"));
}

/// GEN-ONNX-1 Welle 2b (F4): end-to-end — an active generative recipe is
/// driven through the real drag tick (`render_draft_tick` →
/// `render_to_vram`), the artifact-blind VRAM refusal is captured and
/// classified, and `gpu_routing_fallback_badge()` surfaces it as a visible
/// badge instead of a swallowed `warn!`. The recipe gate itself carries no
/// generative reason. Skips loudly-commented without a usable adapter.
#[test]
#[cfg(feature = "gpu")]
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
    // Arm expand without an artifact: the render is loud and the VRAM
    // present path is artifact-blind.
    app.set_expand_beyond_image(true).unwrap_err();
    app.render_draft_tick([800, 600]);
    assert!(
        app.error().is_some(),
        "the armed generative recipe must stay loud on the CPU path"
    );
    let refusal = app
        .vram_render_refusal
        .as_deref()
        .expect("the artifact-blind VRAM refusal must be captured");
    assert!(refusal.contains("generative_edit"), "got {refusal:?}");
    // `update_texture` is the real badge producer (via the present gate).
    app.update_texture(&egui::Context::default());
    let badge = app
        .gpu_routing_fallback_badge()
        .expect("the captured refusal must surface as a routing badge")
        .to_owned();
    assert!(badge.contains("generative_edit"), "got {badge:?}");
    assert!(
        app.gpu_unsupported_stage_reasons().is_empty(),
        "generative_edit must not be a recipe-gate reason"
    );
}

/// R3-ROUTING-1 (NIEDRIG-1): the real `render_draft_tick` path drives the
/// classified `render_to_vram` refusal through two identical ticks — exactly
/// one `warn!` (the first state change), the repeat only `trace!`d. Skips
/// loudly-commented without a usable adapter.
#[test]
#[cfg(feature = "gpu")]
fn repeated_vram_refusal_warns_once_through_the_real_draft_tick() {
    use crate::timing::take_vram_refusal_warns;

    let (png, _frame) = synthetic_8x8_png();
    let mut app = new_app();
    app.load_bytes(png.clone(), "gpu-refusal-throttle.png")
        .unwrap();
    let _dir = app_source_path(&mut app, &png, "gpu-refusal-throttle.png");
    let Some(gpu) = lumina_gpu::GpuContext::new().ok() else {
        eprintln!("no GPU adapter; skipping VRAM refusal throttle test");
        return;
    };
    if !gpu.is_available() {
        eprintln!("GPU unavailable; skipping VRAM refusal throttle test");
        return;
    }
    app.gpu = Some(gpu);
    // A free crop changes the output dimensions: the recipe gate stays empty
    // (an explicit crop is set), but `render_to_vram` refuses it post-gate.
    app.set_crop_free(0.1, 0.1, 0.5, 0.5).unwrap();
    let _ = take_vram_refusal_warns();

    app.render_draft_tick([800, 600]);
    assert_eq!(
        take_vram_refusal_warns(),
        1,
        "the first classified refusal must warn exactly once"
    );
    assert!(
        app.vram_render_refusal
            .as_deref()
            .is_some_and(|reason| reason.contains("geometry")),
        "the geometry refusal must be captured: {:?}",
        app.vram_render_refusal
    );

    app.render_draft_tick([800, 600]);
    assert_eq!(
        take_vram_refusal_warns(),
        0,
        "a repeated identical refusal must not warn again (trace only)"
    );
}

/// GUI-LENSFUN-GATE-3 (F1): the captured present refusal only fills an
/// *empty* gate — a recipe-gate reason already explains the CPU route, and
/// the dimension-changing refusal is a post-gate condition that never
/// coincides with one. Guards against a duplicated badge tail.
#[test]
#[cfg(feature = "gpu")]
fn present_refusal_only_fills_an_empty_gate() {
    let gate = vec!["lens_correction (Lensfun corrector)".to_string()];
    let combined = LuminaApp::combine_routing_reasons(
        gate.clone(),
        Some("geometry (dimension-changing output)"),
    );
    assert_eq!(combined, gate, "a gate reason already explains the route");
    assert!(LuminaApp::combine_routing_reasons(Vec::new(), None).is_empty());
    assert!(LuminaApp::combine_routing_reasons(Vec::new(), Some("")).is_empty());
}

/// GUI-LENSFUN-GATE-3 (F2): a long routing badge truncates with an ellipsis
/// inside its panel clip instead of clipping its tail at the panel edge.
/// Headless layout check (`run_ui`, no GPU adapter needed — the badge text
/// is injected directly). The badge is drawn into a narrow (200 px)
/// allocation simulating the panel edge: it must stay single-line within
/// that width (ellipsis, no wrap-overflow) and inside its clip.
#[test]
#[cfg(feature = "gpu")]
fn routing_badge_truncates_instead_of_clipping() {
    let mut app = new_app();
    app.gpu_route_fallback = Some(format!(
        "{} [geometry (dimension-changing output; the VRAM present texture is \
         source-sized); lens_correction (Lensfun corrector); camera_white_balance \
         (invalid As-Shot gains)]",
        Str::CpuFallbackUnsupportedStages.t()
    ));

    let shapes = headless_shapes(&mut app, |app, ui| {
        ui.allocate_ui(egui::vec2(200.0, 20.0), |ui| {
            app.draw_routing_fallback_badge(ui);
        });
    });

    let headline = Str::CpuFallbackUnsupportedStages.t();
    let mut painted = false;
    for clipped in &shapes {
        if let egui::Shape::Text(text) = &clipped.shape {
            if text.galley.text().starts_with(headline) {
                painted = true;
                let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                assert!(
                    rect.width() <= 201.0,
                    "badge must respect the narrow allocation (ellipsis, no overflow), got width {}",
                    rect.width()
                );
                assert!(
                    rect.height() <= 22.0,
                    "badge must stay single-line (ellipsis, no wrap), got height {}",
                    rect.height()
                );
                assert!(
                    clipped.clip_rect.expand(1.0).contains_rect(rect),
                    "badge {:?} at {rect:?} must be truncated inside its clip {:?}",
                    text.galley.text(),
                    clipped.clip_rect
                );
            }
        }
    }
    assert!(painted, "the routing badge must be painted");
}

/// GPU-LENSFUN-PARITY-1: the memoized GPU-stage verdict is keyed by render
/// identity + As-Shot WB only (the former Lensfun-corrector key component is
/// gone). A source switch must still clear the memo so a verdict computed
/// for the previous image can never be served for the new one.
#[test]
#[cfg(feature = "gpu")]
fn source_switch_resets_gpu_stage_gate_memo() {
    let mut app = new_app();
    app.load_bytes(png(), "memo-first.png").unwrap();
    app.render().unwrap();
    // Populate the memo (a load alone does not query it).
    let _ = app.recipe_has_unsupported_gpu_stages();
    assert!(
        app.gpu_stage_gate.is_some(),
        "the explicit verdict query must populate the memo"
    );
    app.load_bytes(png(), "memo-second.png").unwrap();
    assert!(
        app.gpu_stage_gate.is_none(),
        "a source switch must drop the previous GPU-stage verdict"
    );
}

/// GUI-LENSFUN-GATE-4: adopting a cached neighbor frame clears any captured
/// present refusal. The refusal describes the *previous* frame/recipe; the
/// neighbor pipeline never runs `render_to_vram`, so it can neither be
/// validated nor replaced here. Without the reset, the next
/// `update_texture` would surface a transient, stale CPU-routing badge for
/// the adopted stand-in.
#[test]
#[cfg(feature = "gpu")]
fn neighbor_adopt_resets_vram_render_refusal() {
    let mut app = new_app();
    app.load_bytes(png(), "lensfun-adopt-refusal.png").unwrap();
    app.vram_render_refusal = Some("geometry (dimension-changing output)".into());
    let frame = ImageFrame::new(2, 2, vec![0u8; 2 * 2 * 4]).expect("2x2 frame");
    app.adopt_neighbor_preview_frame(frame);
    assert!(
        app.vram_render_refusal.is_none(),
        "an adopted neighbor frame must not carry the previous present refusal"
    );
}

/// GUI-LENSFUN-GATE-4: `vram_render_refusal` is invalidated by every state
/// change that makes the captured refusal no longer known to apply. Pins
/// all three reset sites of the class (recipe edit via `set_adjustment`,
/// generic edit via `mark_dirty`, source switch via `apply_decoded_frame`)
/// instead of sampling one.
#[test]
#[cfg(feature = "gpu")]
fn vram_render_refusal_invalidated_on_recipe_and_source_change() {
    let mut app = new_app();
    app.load_bytes(png(), "lensfun-invalidate.png").unwrap();

    app.vram_render_refusal = Some("geometry (dimension-changing output)".into());
    app.set_adjustment("exposure", 0.5);
    assert!(
        app.vram_render_refusal.is_none(),
        "set_adjustment must invalidate a stale present refusal"
    );

    app.vram_render_refusal = Some("geometry (dimension-changing output)".into());
    app.mark_dirty();
    assert!(
        app.vram_render_refusal.is_none(),
        "mark_dirty must invalidate a stale present refusal"
    );

    app.vram_render_refusal = Some("geometry (dimension-changing output)".into());
    app.load_bytes(png(), "lensfun-invalidate-second.png")
        .unwrap();
    assert!(
        app.vram_render_refusal.is_none(),
        "a source switch must invalidate a stale present refusal"
    );
}

#[test]
fn diagnostic_gpu_routes_refuse_local_mask_pixels_before_gpu_work() {
    let mut app = new_app();
    app.load_bytes(png(), "diagnostic-local-mask.png").unwrap();
    app.create_mask("Diagnostic").unwrap();
    app.set_mask_local_adjustment("exposure", 1.0).unwrap();
    app.set_mask_local_adjustment("temperature_delta_k", 1100.0)
        .unwrap();

    let error = app
        .render_gpu_readback_frame()
        .expect_err("readback must not return global-only pixels");
    assert!(error.to_string().contains("local mask adjustments"));
    app.vram_fresh = true;
    assert!(!app.prime_gpu_present());
    assert!(!app.vram_fresh);
    assert!(app
        .gpu_unsupported_reasons()
        .iter()
        .any(|reason| reason.contains("local mask adjustments")));
}
