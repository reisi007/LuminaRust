//! R3-RENDER-SIZE-1 (User-Entscheid 2026-09-20): the preview viewport cap.
//!
//! SOLL: preview renders (draft **and** full) never exceed the viewport
//! resolution × device pixel ratio; full source resolution is reserved for
//! export and the 1:1 loupe. These headless tests pin the cap semantics end to
//! end through the real render entries (no GPU needed):
//!
//! * formula: `cap = pane_points · dpr · PREVIEW_ROI_MARGIN` (device px),
//! * exemptions: export, a 1:1 loupe **and the absolute-frame stages**
//!   (AI-Denoise / generative canvas) render the full source resolution
//!   (R3-RENDER-SIZE-1 B1: `apply_denoise_blend` and `composite_auto_fill`
//!   dimension-check the full frame; the expand canvas is absolutely sized by
//!   the recipe `canvas`),
//! * loud fallback: an unknown viewport keeps the full-resolution render and
//!   warns (`warn!`) exactly once, never silently.

use super::*;
use crate::preview_size::{preview_size_cap, PreviewSizeCap, PREVIEW_MIN_CAP_EDGE};

/// Install a pane + dpr the way one painted frame would (`draw_preview` writes
/// the pane, the eframe loop writes the dpr).
fn set_viewport(app: &mut LuminaApp, pane: (f32, f32), dpr: f32) {
    app.preview_pane_w = pane.0;
    app.preview_pane_h = pane.1;
    app.preview_cap_state.dpr = dpr;
}

fn big_source(w: u32, h: u32) -> Vec<u8> {
    ImageFrame::new(w, h, [140_u8, 120, 100, 255].repeat((w * h) as usize))
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap()
}

/// RGBA source with a transparent 8-px border (triggers the generative
/// auto-fill role) over an opaque centre, large enough that a 100×80 pane cap
/// would shrink it (400×300 → 256×192).
fn transparent_source(w: u32, h: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((w * h) as usize * 4);
    for y in 0..h {
        for x in 0..w {
            let border = x < 8 || y < 8 || x + 8 >= w || y + 8 >= h;
            let alpha = if border { 0_u8 } else { 255 };
            pixels.extend_from_slice(&[140, 120, 100, alpha]);
        }
    }
    ImageFrame::new(w, h, pixels)
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap()
}

/// The cap formula, exercised through the pure helper on the exact pane/dpr a
/// frame would install.
#[test]
fn viewport_cap_formula_matches_pane_times_dpr() {
    let cap = preview_size_cap(800.0, 600.0, 1.0).unwrap();
    assert_eq!((cap.width, cap.height), (1040, 780));
    let retina = preview_size_cap(800.0, 600.0, 2.0).unwrap();
    assert_eq!((retina.width, retina.height), (2080, 1560));
    assert_eq!(
        preview_size_cap(10.0, 10.0, 1.0).unwrap().width,
        PREVIEW_MIN_CAP_EDGE
    );
}

/// A full preview of a large source is capped to the viewport resolution × dpr:
/// the rendered texture is far below the source, and the render source is the
/// capped frame — not the full original.
#[test]
fn full_preview_is_capped_to_the_viewport() {
    let mut app = new_app();
    app.load_bytes(big_source(2000, 1500), "cap.png").unwrap();
    set_viewport(&mut app, (800.0, 600.0), 1.0);
    app.render_full([800, 600], None).unwrap();

    let preview = app.preview().unwrap();
    assert!(
        preview.width <= 1040 && preview.height <= 780,
        "full preview must not exceed the 800×600@dpr1 cap: {}×{}",
        preview.width,
        preview.height
    );
    assert!(
        preview.width < 2000,
        "the 2000-px source must be visibly capped, got {}",
        preview.width
    );
    let src = app.preview_render_src.expect("render source recorded");
    assert!(
        src.0 < 2000 && src.1 < 1500,
        "render source must be capped: {src:?}"
    );
}

/// Retina (dpr 2) doubles the device-pixel budget, so the same source is less
/// aggressively capped — the ratio participates in the formula.
#[test]
fn dpr_scales_the_preview_budget() {
    let mut app = new_app();
    app.load_bytes(big_source(2000, 1500), "dpr.png").unwrap();
    set_viewport(&mut app, (800.0, 600.0), 1.0);
    app.render_full([800, 600], None).unwrap();
    let dpr1 = app.preview().unwrap().width;

    let mut retina = new_app();
    retina
        .load_bytes(big_source(2000, 1500), "dpr.png")
        .unwrap();
    set_viewport(&mut retina, (800.0, 600.0), 2.0);
    retina.render_full([800, 600], None).unwrap();
    let dpr2 = retina.preview().unwrap().width;

    assert!(
        dpr2 > dpr1,
        "dpr 2 must allow more preview pixels than dpr 1: {dpr1} vs {dpr2}"
    );
    assert!(
        dpr2 <= 2080,
        "dpr 2 preview must stay within the 2080-px cap, got {dpr2}"
    );
}

/// Ticking the draft path on a large source renders the same (capped) source
/// size as the capped full render — the whole violation R3-RENDER-SIZE-1 was
/// about (the draft used to be a fixed 1280 px edge).
#[test]
fn draft_is_built_at_the_viewport_cap_not_a_fixed_edge() {
    let mut app = new_app();
    app.load_bytes(big_source(2000, 1500), "draft.png").unwrap();
    set_viewport(&mut app, (800.0, 600.0), 1.0);
    app.refresh_preview_cap(1.0);
    let draft = app.draft_original.as_ref().unwrap();
    assert_eq!(
        (draft.width, draft.height),
        (1040, 780),
        "draft source must be built at the viewport cap"
    );
    app.render_draft([800, 600], None).unwrap();
    assert!(app.preview().unwrap().width <= 1040);
}

/// Export is explicitly exempt: it renders the full source resolution, never
/// the viewport cap.
#[test]
fn export_renders_full_source_resolution() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = new_app();
    app.load_bytes(big_source(2000, 1500), "export.png")
        .unwrap();
    set_viewport(&mut app, (800.0, 600.0), 1.0);
    // A preview render in between must not leak the cap into export.
    app.render_full([800, 600], None).unwrap();
    let out = directory.path().join("out.png");
    app.export_to(out.clone()).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    let exported = ImageFrame::decode(&bytes).unwrap();
    assert_eq!(
        (exported.width, exported.height),
        (2000, 1500),
        "export must stay full source resolution (viewport cap is preview-only)"
    );
}

/// A 1:1 loupe renders full source resolution (the documented exemption): the
/// visible window at 1:1 maps to exactly the pane's device pixels, so the cap
/// is an exact fit and never downscales the source.
#[test]
fn one_to_one_loupe_renders_full_source_resolution() {
    let mut app = new_app();
    app.load_bytes(big_source(1200, 900), "loupe.png").unwrap();
    // A 1:1 loupe on a 200×150 pane: `preview_zoom` is relative-to-fit
    // (1 / fit), so the visible source window is exactly pane-sized (+margin).
    set_viewport(&mut app, (200.0, 150.0), 1.0);
    let fit = (200.0_f32 / 1200.0).min(150.0 / 900.0);
    app.zoom_mode = ZoomMode::OneToOne;
    app.preview_zoom = 1.0 / fit;
    app.preview_pan = egui::Vec2::ZERO;
    app.render_full([200, 150], None).unwrap();
    let src = app.preview_render_src.expect("render source recorded");
    assert_eq!(
        src,
        (1200, 900),
        "1:1 loupe must keep the full source render source, got {src:?}"
    );
    // The loupe texture is the visible window at 1:1 resolution, not a
    // downscaled stand-in: its ROI in full pixels covers exactly the 1:1 window.
    let roi = app.preview_roi.expect("1:1 window ROI");
    assert!(
        roi[2] >= 200 && roi[3] >= 150,
        "1:1 window must keep pane-resolution pixels, got {roi:?}"
    );
}

/// Missing viewport information is loud: the render keeps full resolution (no
/// silent cap fallback) and warns exactly once (dedup), captured via the seam.
#[test]
fn missing_viewport_keeps_full_resolution_and_warns_once() {
    let _ = crate::render_entry::take_preview_cap_missing_viewport_warns();
    let mut app = new_app();
    app.load_bytes(big_source(2000, 1500), "noviewport.png")
        .unwrap();
    set_viewport(&mut app, (0.0, 0.0), 1.0);
    app.preview_cap_state.warned = false;
    app.render_full([0, 0], None).unwrap();
    assert!(app.preview_cap().is_none(), "a zero pane must yield no cap");
    assert!(
        app.preview_cap_state.warned,
        "the missing-viewport warning must be armed (loud, not silent)"
    );
    // The render is the (correct) full-resolution source, not a silent shrink.
    assert_eq!(app.preview_render_src, Some((2000, 1500)));

    // A second render with the still-degenerate pane must not warn again
    // (the flag is the dedup): unhide/re-run and see the seam count.
    app.render_full([0, 0], None).unwrap();
    // Exactly one warning across both renders.
    assert_eq!(
        crate::render_entry::take_preview_cap_missing_viewport_warns(),
        1,
        "the loud path must fire exactly once per source, never per frame"
    );
}

/// A source smaller than the cap is never downscaled (existing small-fixture
/// renders stay byte-identical); the cap only ever shrinks.
#[test]
fn small_sources_are_left_untouched_by_the_cap() {
    let mut app = new_app();
    app.load_bytes(png(), "small.png").unwrap(); // 2×1
    set_viewport(&mut app, (800.0, 600.0), 1.0);
    app.refresh_preview_cap(1.0);
    app.render_full([800, 600], None).unwrap();
    assert_eq!(
        app.preview_render_src,
        Some((2, 1)),
        "a source below the cap must stay at its own resolution"
    );
    assert_eq!(
        (app.preview().unwrap().width, app.preview().unwrap().height),
        (2, 1)
    );
}

/// The cap is a pure size rule: a `PreviewSizeCap` with the pane's device
/// pixels leaves an exactly-fitting window unchecked.
#[test]
fn cap_scale_boundary_is_exact() {
    let cap = PreviewSizeCap {
        width: 1040,
        height: 780,
    };
    assert_eq!(
        cap.scale_for(1040, 780),
        None,
        "exact fit is not downscaled"
    );
    assert!(cap.scale_for(1041, 780).is_some(), "one pixel over caps");
}

// ---------------------------------------------------------------------------
// R3-RENDER-SIZE-1 (B1): absolute-frame stages suspend the cap.
//
// AI-Denoise (`apply_denoise_blend`: "frame dimensions must match artifact
// exactly") and the generative **auto-fill** canvas (`composite_auto_fill`
// dimension-checks `artifact.frame` against the current frame) are
// absolute-frame stages: their artifacts are defined at full source geometry,
// so a capped source would make the blend/fill fail loudly. The generative
// **expand** canvas is absolute too, but its artifact is sized by the recipe
// `canvas` (`composite_expand` only bounds-checks that the source fits that
// canvas, so a capped source would not be caught there); it is suspended
// through the same `generative_stage_active` gate. The cap is therefore
// suspended for the whole generative/denoise stage — the documented exception
// alongside export and the 1:1 loupe. These tests use a source **larger than
// the 256-px cap floor** so an accidental cap would actually shrink it (the
// pre-existing denoise/generative tests all use small sources below the floor,
// which is exactly the B1 coverage gap).
// ---------------------------------------------------------------------------

/// A ready AI-Denoise stage on a large source keeps full resolution: the
/// full-resolution `denoise_rgb` artifact blends without a dimension error, and
/// the preview stays at source resolution (cap suspended), not the viewport cap.
#[test]
fn absolute_stage_denoise_suspends_the_cap() {
    use lumina_core::DenoisePolicy;
    use lumina_sidecar::{
        save_denoise_rgb, zdata_path_for, DenoiseRgbArtifact as SidecarDenoiseArtifact,
    };
    use std::path::Path;

    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("denoise.png");
    // 400×300 is above the 256-px cap floor; a 100×80 pane yields the 256-px
    // floor cap (256×256) and would shrink the long edge to 256 (→ 256×192).
    let png = big_source(400, 300);
    std::fs::write(&source_path, &png).unwrap();

    let mut app = new_app();
    app.load_bytes(png, "denoise.png").unwrap();
    app.path = source_path.display().to_string();
    set_viewport(&mut app, (100.0, 80.0), 1.0);
    app.set_denoise_enabled(true).unwrap();

    // Full-resolution artifact (exactly the source dimensions) + pinned model
    // identity + producer provenance → status `ready`.
    let (width, height) = (400u32, 300u32);
    let data = vec![0u8; width as usize * height as usize * 3];
    let artifact = SidecarDenoiseArtifact {
        id: app.virtual_copy_id.clone(),
        width,
        height,
        pixels: data,
    };
    artifact.validate().unwrap();
    let checksum = artifact.checksum();
    save_denoise_rgb(&zdata_path_for(Path::new(&app.path)), artifact, false).unwrap();
    let model = lumina_sidecar::DenoiseModelIdentity {
        name: crate::denoise_gui::GUI_DENOISE_MODEL_NAME.into(),
        version: crate::denoise_gui::GUI_DENOISE_MODEL_VERSION.into(),
        model_hash: format!("sha256:{}", "11".repeat(32)),
        extras: Default::default(),
    };
    app.set_denoise_live_model(Some(model.clone()));
    let mut denoise = app.recipe.denoise_ai.clone().unwrap();
    denoise.model = model;
    denoise.artifact = Some(lumina_sidecar::DenoiseArtifactRef {
        kind: lumina_sidecar::DenoiseArtifactKind::DenoiseRgb,
        relative_path: "denoise.png.lumina.zdata".into(),
        format: "lumina-zdata".into(),
        checksum,
        width,
        height,
        channels: "rgb8".into(),
        data_version: "1".into(),
        extras: Default::default(),
    });
    app.recipe.denoise_ai = Some(denoise.clone());
    let loaded = app.load_denoise_artifact();
    let resolved = app.resolve_denoise_state(loaded.as_ref());
    let current = resolved.current.unwrap();
    lumina_core::set_denoise_producer_provenance(&mut denoise, &current);
    app.recipe.denoise_ai = Some(denoise);
    app.set_denoise_policy(DenoisePolicy::Strict);

    // Cap would shrink 400×300 → 256×192; the active absolute stage must
    // suspend it so the full-res artifact blends.
    app.refresh_preview_cap(1.0);
    app.render_full([100, 80], None)
        .expect("denoise render must succeed (no dimension mismatch)");
    assert!(
        app.error().is_none(),
        "active denoise render must not error: {:?}",
        app.error()
    );
    assert_eq!(
        app.preview_render_src,
        Some((400, 300)),
        "the cap must be suspended for an absolute (denoise) stage"
    );
    assert_eq!(
        (app.preview().unwrap().width, app.preview().unwrap().height),
        (400, 300),
        "the denoise preview must stay at full source resolution"
    );
}

/// An active generative **auto-fill** canvas suspends the cap. Unlike the
/// expand branch, auto-fill is genuinely frame-dimensioned: `composite_auto_fill`
/// requires the source-sized artifact to match the render frame exactly, so
/// removing the suspension makes the render fail loudly (256×192 frame vs the
/// 400×300 artifact) instead of merely shrinking the source.
#[test]
fn absolute_stage_generative_auto_fill_suspends_the_cap() {
    // 400×300 with a transparent border: above the 256-px cap floor, so without
    // the suspension the frame would be capped to 256×192 and the dimension
    // check would reject the full-resolution auto-fill artifact.
    let png = transparent_source(400, 300);
    let mut app = new_app();
    app.load_bytes(png.clone(), "autofill.png").unwrap();
    let _dir = app_source_path(&mut app, &png, "autofill.png");
    app.recipe.generative_edit = Some(GenerativeEdit {
        version: 1,
        canvas: None,
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: Some(true),
        expand_beyond_image: None,
        seed: None,
        prompt: None,
        extras: Default::default(),
    });
    // Produces + persists the source-sized 400×300 auto-fill canvas and renders
    // it once at the default (uncapped) viewport.
    app.generate_generative_canvas().unwrap();

    set_viewport(&mut app, (100.0, 80.0), 1.0);
    app.refresh_preview_cap(1.0);
    app.render_full([100, 80], None)
        .expect("generative auto-fill render must succeed (no dimension mismatch)");
    assert!(
        app.error().is_none(),
        "generative auto-fill render must not error: {:?}",
        app.error()
    );
    assert_eq!(
        app.preview_render_src,
        Some((400, 300)),
        "the cap must be suspended for an absolute (auto-fill) stage"
    );
    assert_eq!(
        (app.preview().unwrap().width, app.preview().unwrap().height),
        (400, 300),
        "the auto-fill preview must stay at full source resolution"
    );
}

/// An active generative **expand** canvas suspends the cap: the expanded canvas
/// is adopted at full geometry (and is not silently downscaled to the
/// viewport). The exact render source pins the suspension: with the cap applied
/// the source would be 256×192, not 400×300.
#[test]
fn absolute_stage_generative_expand_suspends_the_cap() {
    // 12×12 source below the cap floor would prove nothing; use 400×300 and a
    // canvas that grows it further, well above the 256×192 viewport cap.
    let png = big_source(400, 300);
    let mut app = new_app();
    app.load_bytes(png.clone(), "expand.png").unwrap();
    let _dir = app_source_path(&mut app, &png, "expand.png");
    set_viewport(&mut app, (100.0, 80.0), 1.0);

    // Expand beyond the image: canvas = source + a 40-px border per side.
    app.recipe.generative_edit = Some(GenerativeEdit {
        version: 1,
        canvas: Some(GenerativeCanvas {
            output_width: 480,
            output_height: 380,
            source_offset_x: 40,
            source_offset_y: 40,
            extras: Default::default(),
        }),
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: None,
        expand_beyond_image: Some(true),
        seed: None,
        prompt: None,
        extras: Default::default(),
    });
    app.generate_generative_canvas().unwrap();

    app.refresh_preview_cap(1.0);
    app.render_full([100, 80], None)
        .expect("generative expand render must succeed");
    assert!(
        app.error().is_none(),
        "generative expand render must not error: {:?}",
        app.error()
    );
    let preview = app.preview().unwrap();
    assert_eq!(
        (preview.width, preview.height),
        (480, 380),
        "the expanded canvas must be adopted at full geometry, not capped"
    );
    assert_eq!(
        app.preview_render_src,
        Some((400, 300)),
        "the cap must be suspended while a generative canvas is active \
         (without the suspension the source would be capped to 256×192)"
    );
}

/// Control: the same large source + small viewport **without** an absolute
/// stage is capped — so the suspension above is caused by the stage, not by the
/// viewport geometry.
#[test]
fn no_absolute_stage_is_capped_at_the_same_viewport() {
    let mut app = new_app();
    app.load_bytes(big_source(400, 300), "plain.png").unwrap();
    set_viewport(&mut app, (100.0, 80.0), 1.0);
    app.refresh_preview_cap(1.0);
    app.render_full([100, 80], None).unwrap();
    let src = app.preview_render_src.expect("render source recorded");
    assert!(
        src.0 < 400 && src.1 < 300,
        "without an absolute stage the source must be capped, got {src:?}"
    );
}
