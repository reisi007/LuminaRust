//! generative render/golden/export paths tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

#[test]
fn generative_expand_invalid_canvas_output_not_larger_rejected() {
    let canvas = GenerativeCanvas {
        output_width: 8,
        output_height: 8,
        source_offset_x: 0,
        source_offset_y: 0,
        extras: Default::default(),
    };
    assert!(canvas.validate_with_source(8, 8).is_err());
    let ok = GenerativeCanvas {
        output_width: 12,
        output_height: 8,
        source_offset_x: 2,
        source_offset_y: 0,
        extras: Default::default(),
    };
    assert!(ok.validate_with_source(8, 8).is_ok());
}

#[test]
fn generative_expand_golden_preview_headless() {
    // GEN-ONNX-1 Welle 2b: golden assertions run against the composited
    // fixture canvas; the source reappears verbatim at the canvas offset and
    // the generated border is opaque and deterministic.
    let frame = ImageFrame::new(
        4,
        4,
        vec![
            10u8, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255, 10, 20, 30,
            255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255, 10, 20, 30, 255, 40, 50, 60,
            255, 70, 80, 90, 255, 100, 110, 120, 255, 10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90,
            255, 100, 110, 120, 255,
        ],
    )
    .unwrap();
    let mut recipe = EditRecipe::default();
    recipe.generative_edit = Some(GenerativeEdit {
        version: 1,
        canvas: Some(GenerativeCanvas {
            output_width: 6,
            output_height: 6,
            source_offset_x: 1,
            source_offset_y: 1,
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
    let ctx = RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    let artifact = fixture_canvas_artifact(
        &frame,
        recipe.generative_edit.as_ref().unwrap(),
        lumina_onnx::GenerativeRole::Expand,
    );
    let g = GenerativeCanvasInput {
        auto_fill: None,
        expand: Some(&artifact),
    };
    let expanded = lumina_core::render_frame_with_generative(&frame, &ctx, g)
        .unwrap()
        .frame;
    assert_eq!((expanded.width, expanded.height), (6, 6));
    let src_origin_idx = (1 * 6 + 1) * 4;
    let src_idx = 0;
    assert_eq!(
        &expanded.pixels[src_origin_idx..src_origin_idx + 4],
        &frame.pixels[src_idx..src_idx + 4]
    );
    let center_idx = (2 * 6 + 2) * 4;
    let src_1_1_idx = (1 * 4 + 1) * 4;
    assert_eq!(
        &expanded.pixels[center_idx..center_idx + 4],
        &frame.pixels[src_1_1_idx..src_1_1_idx + 4]
    );
    assert!(
        expanded
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|px| px[3] == 255),
        "composited expand canvas must leave no transparent pixels"
    );
    // Determinism: identical inputs give the byte-identical canvas.
    let again = lumina_core::render_frame_with_generative(&frame, &ctx, g)
        .unwrap()
        .frame;
    assert_eq!(expanded.pixels, again.pixels);
    // Visual analysis: the composited canvas differs from the source
    // (generated border) and identical inputs are byte-identical.
    let psnr_val = lumina_core::psnr(&expanded, &again);
    assert!(
        psnr_val.is_infinite(),
        "identical composited canvases must be byte-identical, got PSNR {psnr_val}"
    );
    let hist_src = LuminanceHistogram::new(&frame);
    let hist_expanded = LuminanceHistogram::new(&expanded);
    assert_ne!(
        hist_src.digest(),
        hist_expanded.digest(),
        "the generated border must change the histogram"
    );
    // A canvas that does not expand (`output == source`) is rejected loudly.
    let mut bad_recipe = recipe.clone();
    bad_recipe.generative_edit.as_mut().unwrap().canvas = Some(GenerativeCanvas {
        output_width: 4,
        output_height: 4,
        source_offset_x: 0,
        source_offset_y: 0,
        extras: Default::default(),
    });
    let bad_ctx = RenderContext {
        recipe: &bad_recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    assert!(lumina_core::render_frame_with_generative(&frame, &bad_ctx, g).is_err());
}

#[test]
fn generative_expand_preview_uses_single_core_expand() {
    // GEN-ONNX-1 Welle 2b: the GUI preview injects the resolved canvas
    // through the core hook; the composited frame is rendered exactly once
    // (a second expand would fail `validate_with_source`).
    let (png, frame) = synthetic_8x8_png();
    let mut app = new_app();
    app.load_bytes(png.clone(), "expand-preview-test.png")
        .unwrap();
    let _dir = app_source_path(&mut app, &png, "expand-preview-test.png");
    let plain_key = app.render_key().cloned().unwrap().digest();
    let gen_before = app.preview_generation();
    app.set_expand_beyond_image(true).unwrap_err();
    app.generate_generative_canvas().unwrap();
    let preview = app.preview().unwrap().clone();
    assert_eq!((preview.width, preview.height), (12, 12));
    for y in 0..8 {
        for x in 0..8 {
            let src_idx = (y * 8 + x) * 4;
            let dst_idx = ((y + 2) * 12 + (x + 2)) * 4;
            assert_eq!(
                &preview.pixels[dst_idx..dst_idx + 4],
                &frame.pixels[src_idx..src_idx + 4]
            );
        }
    }
    assert!(
        app.preview_generation() > gen_before,
        "preview_generation must bump on generation"
    );
    assert_ne!(
        app.render_key().cloned().unwrap().digest(),
        plain_key,
        "the generative stage must change the render key"
    );
    assert!(
        app.error().is_none(),
        "composited expand must not leave an error, got {:?}",
        app.error()
    );
    // The preview equals one direct core render with the same resolved
    // artifacts — applying the expand a second time would fail
    // `validate_with_source`, so equality proves the GUI did not re-expand.
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
    assert_eq!(
        preview.pixels, direct.pixels,
        "app preview must equal a single core render with the resolved canvas"
    );
}

#[test]
fn generative_expand_export_is_single_core_expand() {
    // GEN-ONNX-1 Welle 2b: the export hook injects the same resolved canvas
    // as the preview and stays byte-identical to the shared artifact-aware
    // `export_image_with_generative` path.
    let directory = tempfile::tempdir().unwrap();
    let (png, frame) = synthetic_8x8_png();
    let mut app = new_app();
    app.load_bytes(png.clone(), "expand-export-test.png")
        .unwrap();
    let _dir = app_source_path(&mut app, &png, "expand-export-test.png");
    app.set_expand_beyond_image(true).unwrap_err();
    app.generate_generative_canvas().unwrap();
    let preview = app.preview().unwrap().clone();
    assert_eq!((preview.width, preview.height), (12, 12));
    app.export_format = ImageFileFormat::Png;
    let out = directory.path().join("expand_export.png");
    app.export_to(out.clone()).unwrap();
    let gui_bytes = std::fs::read(&out).unwrap();
    let decoded = ImageFrame::decode(&gui_bytes).unwrap();
    assert_eq!((decoded.width, decoded.height), (12, 12));
    assert_eq!(
        decoded.pixels, preview.pixels,
        "export must match the app preview"
    );
    let g = app.generative_artifacts.clone();
    let context = RenderContext {
        recipe: app.recipe(),
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    let options = ExportOptions {
        format: ImageFileFormat::Png,
        quality: 90,
        dither: false,
        ..Default::default()
    };
    let cli_bytes =
        lumina_core::export_image_with_generative(&frame, &context, options, g.input()).unwrap();
    assert_eq!(
        cli_bytes, gui_bytes,
        "GUI export with the resolved canvas must be byte-identical to the shared path"
    );
}

#[test]
fn generative_expand_preview_reuses_resolved_canvas_without_bfs() {
    // GEN-ONNX-1 Welle 2b: the former heuristic BFS (and its `bfs_runs`
    // counter) is gone. A second identical render reuses the already
    // resolved session canvas: removing the persisted bundle before the
    // second render proves no disk re-read and no re-generation.
    let (png, _frame) = synthetic_8x8_png();
    let mut app = new_app();
    app.load_bytes(png.clone(), "expand-cache-test.png")
        .unwrap();
    let dir = app_source_path(&mut app, &png, "expand-cache-test.png");
    app.set_expand_beyond_image(true).unwrap_err();
    app.generate_generative_canvas().unwrap();
    let first_preview = app.preview().unwrap().clone();
    let identity = app
        .generative_artifacts
        .expand
        .as_ref()
        .unwrap()
        .identity
        .clone();
    let memo = app.generative_memo.clone();
    assert!(memo.is_some(), "generation must seed the resolve memo");
    let zdata = zdata_path_for(&dir.path().join("expand-cache-test.png"));
    assert!(zdata.exists(), "the bundle must be persisted");
    std::fs::remove_file(&zdata).unwrap();
    app.render_full([800, 600], None).unwrap();
    assert_eq!(
        app.preview().unwrap().pixels,
        first_preview.pixels,
        "reused canvas is byte-identical to the first render"
    );
    assert_eq!(
        app.generative_artifacts.expand.as_ref().unwrap().identity,
        identity,
        "the resolved identity must be stable across renders"
    );
    assert_eq!(
        app.generative_memo, memo,
        "an unchanged source/recipe must not recompute the generative inputs"
    );
}

/// GEN-ONNX-1 Welle 2b (F6): an active generative recipe turns a draft
/// request into exactly one full-quality render (absolute canvas geometry;
/// no draft compositing), so slider drags on a generative recipe never show
/// a mismatched or blank preview.
#[test]
fn generative_draft_render_upgrades_to_full_render() {
    let (png, _frame) = synthetic_8x8_png();
    let mut app = new_app();
    app.load_bytes(png.clone(), "expand-draft-test.png")
        .unwrap();
    let _dir = app_source_path(&mut app, &png, "expand-draft-test.png");
    app.set_expand_beyond_image(true).unwrap_err();
    app.generate_generative_canvas().unwrap();
    let gen_before = app.preview_generation();
    // A draft request must upgrade to a full render: the preview is the
    // composited 12x12 canvas and `preview_is_draft` stays false.
    app.render_draft([800, 600], None).unwrap();
    assert!(
        !app.preview_is_draft(),
        "a generative recipe must upgrade a draft to a full render"
    );
    assert_eq!(
        app.preview_generation(),
        gen_before + 1,
        "the upgrade must run exactly one full render"
    );
    assert_eq!(app.preview().unwrap().width, 12);
    assert!(app.error().is_none(), "got {:?}", app.error());
}
