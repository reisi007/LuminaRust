//! generative status, roles and failure paths tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// GEN-ONNX-1 Welle 2b (F8): the panel status distinguishes
/// `valid`/`stale`/`missing`/`corrupt` from the last resolver outcome, not
/// just ready/missing.
#[test]
fn generative_status_text_covers_all_states() {
    let (png, frame) = synthetic_8x8_png();
    let mut app = new_app();
    app.load_bytes(png.clone(), "status-test.png").unwrap();
    let dir = app_source_path(&mut app, &png, "status-test.png");
    // Inactive → no text.
    assert!(app.generative_status_text().is_empty());
    // Armed expand without a canvas artifact: the loud render records
    // `missing` for the role.
    app.set_expand_beyond_image(true).unwrap_err();
    let text = app.generative_status_text();
    assert!(
        text.contains("Expand: missing"),
        "an armed expand without an artifact must read missing, got {text:?}"
    );
    // Generated → valid.
    app.generate_generative_canvas().unwrap();
    let text = app.generative_status_text();
    assert!(
        text.contains("Expand: valid"),
        "a resolved canvas must read valid, got {text:?}"
    );
    // A recipe identity drift with a persisted (now stale) link reports
    // stale: change the seed and resolve again.
    let zdata = zdata_path_for(&dir.path().join("status-test.png"));
    assert!(zdata.exists());
    app.generative_artifacts = GenerativeArtifacts::default();
    app.generative_memo = None;
    app.recipe.generative_edit.as_mut().unwrap().seed = Some(999);
    // The persisted link is for seed 7, so the current identity is stale.
    let error = app.resolve_generative_artifacts(&frame).unwrap_err();
    assert!(
        error.to_string().to_lowercase().contains("stale"),
        "got {error}"
    );
    assert!(
        app.generative_status_text().contains("Expand: stale"),
        "an identity drift must read stale, got {:?}",
        app.generative_status_text()
    );
    // Corrupt: point the link at a bundle whose record checksum fails by
    // flipping the stored identity record via a fresh mismatch is already
    // covered by Stale; a corrupt bundle is the `Corrupt` branch.
    std::fs::write(&zdata, b"not a LUMZDATA container").unwrap();
    app.generative_artifacts = GenerativeArtifacts::default();
    app.generative_memo = None;
    app.recipe.generative_edit.as_mut().unwrap().seed = Some(7);
    let error = app.resolve_generative_artifacts(&frame).unwrap_err();
    assert!(
        error.to_string().to_lowercase().contains("corrupt"),
        "a damaged bundle must read corrupt, got {error}"
    );
    assert!(
        app.generative_status_text().contains("Expand: corrupt"),
        "a damaged bundle must read corrupt, got {:?}",
        app.generative_status_text()
    );
}

/// GEN-ONNX-1 Welle 2b: a single record carrying both roles
/// (`auto_fill_transparent` + `expand_beyond_image`) renders with the
/// documented order `Lens → auto-fill → Perspective → expand → Crop`. The
/// expand canvas embeds the auto-filled pixels, both canvases persist, and
/// a fresh app resolves both from the bundle alone.
#[test]
fn generative_double_role_record_renders() {
    let mut pixels = Vec::with_capacity(8 * 8 * 4);
    for y in 0..8 {
        for x in 0..8 {
            let alpha = if x == 0 && y == 0 { 0 } else { 255 };
            pixels.extend_from_slice(&[80, 90, 100, alpha]);
        }
    }
    let frame = ImageFrame::new(8, 8, pixels).unwrap();
    let png = frame.encode(ImageFileFormat::Png).unwrap();
    let mut app = new_app();
    app.load_bytes(png.clone(), "double-role.png").unwrap();
    let dir = app_source_path(&mut app, &png, "double-role.png");
    app.recipe.generative_edit = Some(GenerativeEdit {
        version: 1,
        canvas: Some(GenerativeCanvas {
            output_width: 12,
            output_height: 12,
            source_offset_x: 2,
            source_offset_y: 2,
            extras: Default::default(),
        }),
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: Some(true),
        expand_beyond_image: Some(true),
        seed: Some(7),
        prompt: Some("expand the sky".into()),
        extras: Default::default(),
    });
    app.generate_generative_canvas().unwrap();
    assert!(
        app.generative_artifacts.auto_fill.is_some(),
        "the double role must produce the auto-fill canvas"
    );
    assert!(
        app.generative_artifacts.expand.is_some(),
        "the double role must produce the expand canvas"
    );
    let preview = app.preview().unwrap().clone();
    assert_eq!((preview.width, preview.height), (12, 12));
    assert!(
        preview
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|px| px[3] == 255),
        "auto-fill + expand must leave no transparent pixels"
    );
    // The expand canvas is authoritative and embeds the auto-filled pixel:
    // the source pixel at the canvas offset is opaque (the composited frame
    // can never be served as the original transparent pixel).
    let offset_idx = (2 * 12 + 2) * 4;
    assert_eq!(preview.pixels[offset_idx + 3], 255);
    // Byte-identical to a direct core render with both resolved artifacts.
    let g = app.generative_artifacts.clone();
    let ctx = RenderContext {
        recipe: app.recipe(),
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    let direct = lumina_core::render_frame_with_generative(&frame, &ctx, g.input())
        .unwrap()
        .frame;
    assert_eq!(preview.pixels, direct.pixels);
    // Both canvases persist; the recipe links the canvas-defining expand
    // record.
    assert!(zdata_path_for(&dir.path().join("double-role.png")).exists());
    assert!(app
        .recipe()
        .generative_edit
        .as_ref()
        .unwrap()
        .artifact
        .is_some());
    // F5: a fresh app must reload the persisted state through the real
    // sidecar (`open_and_decode` → `finish_decode` → `load_sidecar`), not a
    // manually injected recipe. Both roles resolve from the bundle (the
    // auto-fill record is addressed by its deterministic identity id).
    let reloaded_path = dir.path().join("double-role.png");
    let mut reloaded = new_app();
    open_and_decode(&mut reloaded, reloaded_path.display().to_string());
    assert!(
        reloaded.original.is_some(),
        "the reloaded source must decode"
    );
    let reloaded_edit = reloaded
        .recipe()
        .generative_edit
        .clone()
        .expect("the generative edit must be restored from the sidecar");
    assert!(
        reloaded_edit.artifact.is_some(),
        "the sidecar must carry the persisted artifact link"
    );
    assert_eq!(
        reloaded_edit.expand_beyond_image,
        Some(true),
        "the persisted expand flag must be restored"
    );
    assert_eq!(
        reloaded_edit.auto_fill_transparent,
        Some(true),
        "the persisted auto-fill flag must be restored"
    );
    let resolved = reloaded.resolve_generative_artifacts(&frame).unwrap();
    assert!(
        resolved.auto_fill.is_some(),
        "auto-fill must resolve from the bundle"
    );
    assert!(
        resolved.expand.is_some(),
        "expand must resolve from the bundle"
    );
}

/// GEN-ONNX-1 Welle 2b: the auto-fill hook and the normative caller
/// convention. With transparent pixels after lens the GUI injects the
/// generated canvas (loud when absent); without transparency the caller
/// passes `auto_fill = None` (identity) and no artifact is required.
#[test]
fn generative_auto_fill_hook_and_caller_convention() {
    let mut pixels = Vec::with_capacity(8 * 8 * 4);
    for y in 0..8 {
        for x in 0..8 {
            let alpha = if x == 0 && y == 0 { 0 } else { 255 };
            pixels.extend_from_slice(&[50, 60, 70, alpha]);
        }
    }
    let frame = ImageFrame::new(8, 8, pixels).unwrap();
    let png = frame.encode(ImageFileFormat::Png).unwrap();
    let mut app = new_app();
    app.load_bytes(png.clone(), "auto-fill.png").unwrap();
    let _dir = app_source_path(&mut app, &png, "auto-fill.png");
    app.recipe.generative_edit = Some(GenerativeEdit {
        version: 1,
        canvas: None,
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: Some(true),
        expand_beyond_image: None,
        seed: Some(9),
        prompt: None,
        extras: Default::default(),
    });
    // Active auto-fill with transparent pixels but no canvas → loud.
    assert!(
        app.render().is_err(),
        "auto-fill without a canvas artifact must fail loudly"
    );
    app.generate_generative_canvas().unwrap();
    let preview = app.preview().unwrap().clone();
    assert_eq!((preview.width, preview.height), (8, 8));
    assert!(
        preview
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|px| px[3] == 255),
        "the auto-fill hook must make every pixel opaque"
    );
    let g = app.generative_artifacts.clone();
    assert!(g.auto_fill.is_some());
    let ctx = RenderContext {
        recipe: app.recipe(),
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    let direct = lumina_core::render_frame_with_generative(&frame, &ctx, g.input())
        .unwrap()
        .frame;
    assert_eq!(preview.pixels, direct.pixels);

    // Caller convention (normative): `auto_fill_transparent` active on an
    // opaque frame → `auto_fill = None` (identity); the render succeeds
    // with no artifact and no silent synthetic fill.
    let mut opaque_pixels = Vec::with_capacity(4 * 4 * 4);
    for _ in 0..4 * 4 {
        opaque_pixels.extend_from_slice(&[100, 100, 100, 255]);
    }
    let opaque = ImageFrame::new(4, 4, opaque_pixels).unwrap();
    let mut recipe = EditRecipe::default();
    recipe.generative_edit = Some(GenerativeEdit {
        version: 1,
        canvas: None,
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: Some(true),
        expand_beyond_image: None,
        seed: Some(1),
        prompt: None,
        extras: Default::default(),
    });
    let ctx = RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    let out =
        lumina_core::render_frame_with_generative(&opaque, &ctx, GenerativeCanvasInput::default())
            .unwrap()
            .frame;
    assert_eq!(out.pixels, opaque.pixels);
    // Defense in depth: a transparent frame with auto_fill and no artifact
    // is rejected by core itself (the GPU trusts the caller signal, the CPU
    // re-checks transparency — never a silent unfilled render).
    let mut transparent = opaque.clone();
    transparent.pixels[3] = 0;
    assert!(lumina_core::render_frame_with_generative(
        &transparent,
        &ctx,
        GenerativeCanvasInput::default()
    )
    .is_err());
}

#[test]
fn generative_expand_without_canvas_fails_loudly() {
    // Expand without a canvas is a hard error — never a silent unexpanded
    // render (kein stiller Fallback).
    let (png, _) = synthetic_8x8_png();
    let mut app = new_app();
    app.load_bytes(png, "expand-error-test.png").unwrap();
    app.recipe.generative_edit = Some(GenerativeEdit {
        version: 1,
        canvas: None,
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: None,
        expand_beyond_image: Some(true),
        seed: None,
        prompt: None,
        extras: Default::default(),
    });
    let err = app.render().unwrap_err();
    // Loud failure from the shared path (sidecar validation names it
    // `generative_edit.canvas`, the core stage `generative_expand.canvas`).
    assert!(
        err.to_string().contains("generative"),
        "expand without canvas must fail loudly, got {err}"
    );
}

#[test]
fn generative_expand_without_canvas_export_fails_loudly() {
    // Same loud failure on the export path: no file, no silent fallback.
    let directory = tempfile::tempdir().unwrap();
    let (png, _) = synthetic_8x8_png();
    let mut app = new_app();
    app.load_bytes(png, "expand-error-export-test.png").unwrap();
    app.recipe.generative_edit = Some(GenerativeEdit {
        version: 1,
        canvas: None,
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: None,
        expand_beyond_image: Some(true),
        seed: None,
        prompt: None,
        extras: Default::default(),
    });
    app.export_format = ImageFileFormat::Png;
    let out = directory.path().join("expand_error.png");
    let err = app.export_to(out.clone()).unwrap_err();
    // Loud failure from the shared path (sidecar validation names it
    // `generative_edit.canvas`, the core stage `generative_expand.canvas`).
    assert!(
        err.to_string().contains("generative"),
        "expand export without canvas must fail loudly, got {err}"
    );
    assert!(!out.exists(), "failed export must not leave a file");
}
