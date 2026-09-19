//! VRAM render state, present reuse and GPU context tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

// ---- R2-GUIMOD-01: the debounced full render must retire stale VRAM ----

/// Simulates the drag→release sequence at the state level: during the drag
/// `render_to_vram` marks VRAM fresh while the preview shows a draft; the
/// debounced full render that follows must invalidate that freshness so
/// the gate stops presenting the soft draft and the sharp CPU pixels get
/// uploaded instead (the reported "preview stays blurry forever" bug).
#[test]
#[cfg(feature = "gpu")]
fn full_render_invalidates_stale_vram_but_draft_render_keeps_it() {
    let mut app = new_app();
    app.load_bytes(png(), "vram.png").unwrap();

    // Drag state: VRAM carries a draft-source tone result.
    app.preview_is_draft = true;
    app.vram_fresh = true;
    app.render_draft([800, 600], None).unwrap();
    assert!(
        app.vram_fresh,
        "a draft render must keep the VRAM result presentable — it is what \
         the interactive path just rendered into VRAM"
    );

    // Release: the debounced full-quality render supersedes VRAM.
    app.render_full([800, 600], None).unwrap();
    assert!(
        !app.vram_fresh,
        "the full render must invalidate vram_fresh or the present gate \
         keeps showing the superseded draft"
    );
    assert!(!app.preview_is_draft());
}

/// R2-GUIMOD-01 belt-and-braces: even if a stale freshness flag ever
/// slipped through, the geometry cross-check must refuse to present VRAM
/// content whose dimensions do not describe the current full-quality
/// preview. Draft previews are exempt by design: the interactive path
/// presents exactly the draft-source VRAM render.
#[test]
#[cfg(feature = "gpu")]
fn vram_geometry_gate_rejects_dimension_mismatch_for_full_previews() {
    let mut app = new_app();
    app.load_bytes(png(), "geom.png").unwrap(); // 2×1 source
    app.render().unwrap(); // full-quality preview, not a draft

    assert!(
        !app.preview_is_draft() && app.vram_content_matches_displayed_preview((2, 1)),
        "matching dimensions describe the same pixels"
    );
    assert!(
        !app.vram_content_matches_displayed_preview((1280, 720)),
        "draft-sized VRAM behind a full-quality preview must be rejected"
    );

    // Draft exemption: geometry mismatches are allowed while a draft is
    // displayed (the VRAM tone output *is* the draft render).
    app.preview_is_draft = true;
    assert!(app.vram_content_matches_displayed_preview((1280, 720)));

    // No preview at all → nothing may be presented from VRAM.
    app.preview_is_draft = false;
    app.preview = None;
    assert!(!app.vram_content_matches_displayed_preview((2, 1)));
}

// ---- R2-GUIMOD-02: CPU present uploads only on content changes ----

/// The texture handle must survive repaints without new content (same egui
/// texture id) and be updated **in place** when the preview changes —
/// never re-created per frame (`load_texture` would mint a fresh id every
/// time and pay a full-frame upload even for pure mousemoves).
#[test]
fn cpu_present_reuses_texture_handle_until_content_changes() {
    let mut app = new_app();
    let ctx = egui::Context::default();
    app.load_bytes(png(), "tex.png").unwrap();
    app.render().unwrap();

    app.update_texture(&ctx);
    let first_id = app
        .texture
        .as_ref()
        .expect("texture after first upload")
        .id();
    assert_eq!(
        app.texture_identity.map(|(gen, _, _)| gen),
        Some(app.preview_generation),
        "identity records the generation it was uploaded from"
    );

    // Repaint without any render change (e.g. mousemove over panels):
    // neither the handle nor its pixels may be touched.
    app.update_texture(&ctx);
    assert_eq!(app.texture.as_ref().unwrap().id(), first_id);

    // New preview content: same handle, updated in place, identity bumped.
    app.set_adjustment("exposure", 0.5);
    app.render().unwrap();
    let generation_after_edit = app.preview_generation;
    assert!(
        generation_after_edit > 1,
        "each completed render bumps the preview generation"
    );
    app.update_texture(&ctx);
    assert_eq!(
        app.texture.as_ref().unwrap().id(),
        first_id,
        "the handle must be reused (set), not replaced by load_texture"
    );
    assert_eq!(
        app.texture_identity.map(|(gen, _, _)| gen),
        Some(generation_after_edit)
    );

    // A follow-up repaint with unchanged content stays a no-op again.
    app.update_texture(&ctx);
    assert_eq!(app.texture.as_ref().unwrap().id(), first_id);
}

/// Before/After swaps which frame is displayed without touching the
/// preview generation — the identity must catch the flag change so the
/// toggle still swaps the visible pixels exactly once per flip.
#[test]
fn cpu_present_uploads_again_when_before_after_flips() {
    let mut app = new_app();
    let ctx = egui::Context::default();
    app.load_bytes(png(), "ba.png").unwrap();
    app.render().unwrap();
    app.update_texture(&ctx);
    assert!(!app.texture_identity.unwrap().1, "preview shown initially");

    app.before_after = true;
    app.update_texture(&ctx);
    assert!(app.texture_identity.unwrap().1, "original shown after flip");

    // Flipping back re-uploads the preview once more.
    app.before_after = false;
    app.update_texture(&ctx);
    assert!(!app.texture_identity.unwrap().1);
}

// ---- R2-GUIMOD-05: unsupported-stage verdict memoized per render key ----

#[test]
#[cfg(feature = "gpu")]
fn unsupported_gpu_stage_verdict_is_memoized_per_render_key() {
    let mut app = new_app();
    app.load_bytes(png(), "stages.png").unwrap();
    app.render().unwrap();
    assert!(app.render_key.is_some());

    let verdict = app.recipe_has_unsupported_gpu_stages();
    assert!(
        !verdict,
        "plain exposure-only recipe is fully GPU-supported"
    );
    assert!(
        app.gpu_stage_gate.is_some(),
        "a keyed verdict must be stored once a render key exists"
    );
    // Repeat queries hit the memo and stay consistent.
    assert_eq!(app.recipe_has_unsupported_gpu_stages(), verdict);

    // Editing nulls the render key: the next query must NOT trust (nor
    // store) a memo entry keyed by nothing — the recipe can drift across
    // edits before the next render produces a new key.
    app.set_adjustment("exposure", 1.0);
    assert!(app.render_key.is_none());
    let _ = app.recipe_has_unsupported_gpu_stages();
    assert!(
        app.gpu_stage_gate.is_none(),
        "no memo entry may be cached without a render key"
    );
}

// ---- R2-GUIMOD-09: GPU context construction is deferred to attach ----

#[test]
#[cfg(feature = "gpu")]
fn gpu_context_is_not_created_eagerly_in_new() {
    let app = LuminaApp::new(egui::Context::default());
    assert!(
        app.gpu.is_none(),
        "LuminaApp::new must not perform a blocking adapter/device request; \
         attach_wgpu_render_state owns the single GPU init (R2-GUIMOD-09)"
    );
    assert!(app.wgpu_render_state.is_none());
    assert!(!app.vram_fresh);
}

// ---- CAMERA-WB-WELLE: a valid As-Shot context is GPU-carried, invalid still flags ----

#[test]
#[cfg(feature = "gpu")]
fn camera_white_balance_is_carried_not_a_fallback() {
    // Pure function: a valid context is not flagged (the GPU carries it);
    // an invalid one still is.
    let recipe = EditRecipe::default();
    let wb: [f32; 4] = [1.7, 1.0, 1.3, 1.0];
    assert!(
        lumina_gpu::unsupported_gpu_stages_with_context(&recipe, false, Some(&wb)).is_empty(),
        "CAMERA-WB-WELLE: a valid As-Shot context must be GPU-eligible"
    );
    assert!(
        lumina_gpu::unsupported_gpu_stages_with_context(
            &recipe,
            false,
            Some(&[0.0, 1.0, 1.0, 1.0])
        )
        .iter()
        .any(|r| r.contains("camera_white_balance")),
        "CAMERA-WB-WELLE: invalid As-Shot gains must stay flagged"
    );
    let reasons_without = lumina_gpu::unsupported_gpu_stages_with_context(&recipe, false, None);
    assert!(
        !reasons_without
            .iter()
            .any(|r| r.contains("camera_white_balance")),
        "absent WB must not flag camera_white_balance"
    );

    // App-level memoized gate: a valid WB context stays GPU-eligible.
    let mut app = new_app();
    app.load_bytes(png(), "wb.png").unwrap();
    app.render().unwrap();
    app.camera_white_balance = None;
    app.render().unwrap();
    assert!(app.render_key.is_some());
    assert!(
        !app.recipe_has_unsupported_gpu_stages(),
        "default recipe without WB must be GPU-eligible"
    );
    app.camera_white_balance = Some(wb);
    assert!(
        !app.recipe_has_unsupported_gpu_stages(),
        "a valid WB context must stay GPU-eligible (carried, not a fallback)"
    );
    assert!(
        app.routing_fallback_reason().is_none(),
        "no fallback when a valid WB context is carried"
    );

    // An invalid context (not producible from the decode path, which
    // sanitizes) is still a routing reason.
    app.camera_white_balance = Some([0.0, 1.0, 1.0, 1.0]);
    assert!(
        app.recipe_has_unsupported_gpu_stages(),
        "invalid WB gains must flag the CPU route"
    );

    // Clearing WB restores the trivially eligible state.
    app.camera_white_balance = None;
    assert!(
        !app.recipe_has_unsupported_gpu_stages(),
        "clearing WB must keep GPU eligibility"
    );
    assert!(app.routing_fallback_reason().is_none());
}

/// CAMERA-WB-WELLE: the GUI binds the decode-path As-Shot context on the
/// GPU context (when one exists) through the same single-source funnel as a
/// loaded RAW. Without a bound adapter (headless) the bind is a no-op on
/// pixels but still must not panic, and an invalid value must be rejected
/// loudly rather than stored.
#[test]
#[cfg(feature = "gpu")]
fn decoded_as_shot_context_is_bound_on_the_gpu_context() {
    let mut app = new_app();
    // No GPU context in the headless harness: load must still succeed.
    app.load_bytes(png(), "wb-bind.png").unwrap();
    assert_eq!(app.camera_white_balance, None);

    // The entry-level validation contract the GUI relies on.
    let ctx = lumina_gpu::GpuContext::new().ok();
    if let Some(ctx) = ctx {
        assert!(ctx
            .set_camera_white_balance(Some([1.7, 1.0, 1.3, 1.0]))
            .is_ok());
        assert_eq!(ctx.camera_white_balance(), Some([1.7, 1.0, 1.3, 1.0]));
        assert!(ctx
            .set_camera_white_balance(Some([0.0, 1.0, 1.0, 1.0]))
            .is_err());
        // The rejected bind must not overwrite the previous valid context.
        assert_eq!(ctx.camera_white_balance(), Some([1.7, 1.0, 1.3, 1.0]));
    }
}

// ---- R2-JANK-1 F3: no redundant CPU upload while GPU present is active ----

/// F3: while the GPU present path is active the per-tick CPU upload is skipped,
/// but the CPU handle (Navigator overview + fallback) is kept; the first frame
/// without a handle still creates it, and the first CPU frame afterwards
/// re-uploads once.
#[test]
fn gpu_present_skips_cpu_upload_but_keeps_the_navigator_texture() {
    let mut app = new_app();
    let ctx = egui::Context::default();
    app.load_bytes(png(), "skip-upload.png").unwrap();
    app.render().unwrap();

    // First GPU-presented frame: no CPU handle yet → created once so the
    // Navigator (`navigator_viewport` clones `self.texture`) stays valid.
    app.update_cpu_texture(&ctx, true);
    let handle_id = app
        .texture
        .as_ref()
        .expect("navigator/fallback texture is created once")
        .id();
    let identity = app.texture_identity;
    assert!(identity.is_some());

    // New content while GPU present: upload skipped, identity left stale.
    app.set_adjustment("exposure", 0.5);
    app.render().unwrap();
    app.update_cpu_texture(&ctx, true);
    assert_eq!(
        app.texture.as_ref().unwrap().id(),
        handle_id,
        "the navigator/fallback texture survives the skipped upload"
    );
    assert_eq!(
        app.texture_identity, identity,
        "the upload was skipped, so the identity is left stale for the next CPU frame"
    );

    // GPU path ends: the current pixels are uploaded once, handle reused.
    app.update_cpu_texture(&ctx, false);
    assert_ne!(app.texture_identity, identity, "CPU fallback re-uploads");
    assert_eq!(
        app.texture.as_ref().unwrap().id(),
        handle_id,
        "the CPU handle is updated in place, never re-created"
    );
}
