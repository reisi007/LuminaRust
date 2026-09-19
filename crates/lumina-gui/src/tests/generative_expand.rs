//! generative expand canvas lifecycle tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

#[test]
fn generative_expand_synthetic_8x8_expand_true_creates_larger_canvas() {
    // GEN-ONNX-1 Welle 2b: the expand is artifact compositing. The caller
    // supplies the fixture-produced canvas; core adopts it byte-wise and
    // places the source at the documented offset. Without the artifact the
    // same render is a loud error (kein stiller Fallback).
    let mut pixels = Vec::with_capacity(8 * 8 * 4);
    for _ in 0..8 * 8 {
        pixels.extend_from_slice(&[42, 42, 42, 255]);
    }
    let frame = ImageFrame::new(8, 8, pixels).unwrap();
    let mut recipe = EditRecipe::default();
    recipe.generative_edit = Some(GenerativeEdit {
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
    // Active expand without an artifact is loud — never an unexpanded render.
    assert!(
        lumina_core::render_frame_with_generative(&frame, &ctx, GenerativeCanvasInput::default())
            .is_err(),
        "active expand without a canvas artifact must fail loudly"
    );
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
    assert_eq!(expanded.width, 12);
    assert_eq!(expanded.height, 12);
    for y in 0..8 {
        for x in 0..8 {
            let src_idx = (y * 8 + x) * 4;
            let dst_idx = ((y + 2) * 12 + (x + 2)) * 4;
            assert_eq!(
                &expanded.pixels[dst_idx..dst_idx + 4],
                &frame.pixels[src_idx..src_idx + 4]
            );
        }
    }
    assert!(
        expanded
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|px| px[3] == 255),
        "composited expand canvas must leave no transparent pixels"
    );
}

#[test]
fn generative_expand_false_is_cropped_to_image() {
    // Expand off (Default „auf Bild beschneiden"): the shared core render
    // leaves the frame untouched — no canvas, no second pass.
    let frame = ImageFrame::new(8, 8, vec![10u8; 8 * 8 * 4]).unwrap();
    let mut recipe = EditRecipe::default();
    recipe.generative_edit = Some(GenerativeEdit {
        version: 1,
        canvas: None,
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: None,
        expand_beyond_image: Some(false),
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
    let out = lumina_core::render_frame(&frame, &ctx).unwrap().frame;
    assert_eq!(out.width, 8);
    assert_eq!(out.height, 8);
    assert_eq!(out.pixels, frame.pixels);
}

#[test]
fn generative_expand_sidecar_roundtrip_and_recipe_hash() {
    let mut doc = lumina_sidecar::SidecarDocument::new(
        lumina_sidecar::SourceIdentity {
            relative_name: "IMG_0001.ARW".into(),
            content_hash: "blake3:x".into(),
            byte_length: 42,
            modified_at: None,
            raw_format: "ARW".into(),
            orientation: 1,
            decode_fingerprint: lumina_sidecar::DecodeFingerprint {
                decoder: "test".into(),
                version: "1".into(),
                parameters: Default::default(),
                extras: Default::default(),
            },
            geometry_fingerprint: lumina_sidecar::GeometryFingerprint {
                width: 8,
                height: 8,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: Default::default(),
            },
            extras: Default::default(),
        },
        "pipeline-1",
    );
    let canvas = GenerativeCanvas {
        output_width: 12,
        output_height: 12,
        source_offset_x: 2,
        source_offset_y: 2,
        extras: Default::default(),
    };
    doc.virtual_copies[0].recipe.generative_edit = Some(GenerativeEdit {
        version: 1,
        canvas: Some(canvas.clone()),
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: None,
        expand_beyond_image: Some(true),
        seed: None,
        prompt: None,
        extras: Default::default(),
    });
    let json = doc.to_json().unwrap();
    assert!(json.contains("expand_beyond_image"));
    assert!(json.contains("output_width"));
    let decoded = lumina_sidecar::SidecarDocument::from_json(&json).unwrap();
    assert_eq!(decoded, doc);
    let mut doc2 = doc.clone();
    doc2.virtual_copies[0]
        .recipe
        .generative_edit
        .as_mut()
        .unwrap()
        .expand_beyond_image = Some(false);
    doc2.virtual_copies[0]
        .recipe
        .generative_edit
        .as_mut()
        .unwrap()
        .canvas = None;
    let h1 = blake3::hash(
        serde_json::to_vec(&doc.virtual_copies[0].recipe)
            .unwrap()
            .as_slice(),
    )
    .to_hex()
    .to_string();
    let h2 = blake3::hash(
        serde_json::to_vec(&doc2.virtual_copies[0].recipe)
            .unwrap()
            .as_slice(),
    )
    .to_hex()
    .to_string();
    assert_ne!(h1, h2, "expand flag must be part of recipe_hash");
    let mut bad = doc.clone();
    bad.virtual_copies[0]
        .recipe
        .generative_edit
        .as_mut()
        .unwrap()
        .canvas = None;
    assert!(bad.validate().is_err());
    let mut bad2 = doc.clone();
    bad2.virtual_copies[0]
        .recipe
        .generative_edit
        .as_mut()
        .unwrap()
        .expand_beyond_image = Some(false);
    assert!(bad2.validate().is_err());
}

#[test]
fn generative_expand_preview_generation_bumps_and_persists() {
    let (png, _) = synthetic_8x8_png();
    let mut app = new_app();
    app.load_bytes(png.clone(), "sample.png").unwrap();
    let dir = app_source_path(&mut app, &png, "sample.png");
    let before = app.preview_generation();
    // Enabling the expand toggle without a canvas artifact is loud: the
    // recipe stays active and the render refuses — no silent unexpanded
    // preview.
    let err = app.set_expand_beyond_image(true).unwrap_err();
    assert!(
        err.to_string().contains("generative"),
        "expand without an artifact must fail loudly, got {err}"
    );
    assert!(app
        .recipe()
        .generative_edit
        .as_ref()
        .unwrap()
        .effective_expand());
    // Explicit generation produces + persists the fixture canvas and renders.
    app.generate_generative_canvas().unwrap();
    assert!(
        app.preview_generation() > before,
        "preview_generation must bump on generation"
    );
    assert!(
        app.error().is_none(),
        "generation must succeed, got {:?}",
        app.error()
    );
    assert!(
        app.recipe()
            .generative_edit
            .as_ref()
            .unwrap()
            .artifact
            .is_some(),
        "the recipe must link the persisted generative canvas"
    );
    assert!(
        zdata_path_for(&dir.path().join("sample.png")).exists(),
        "the generative canvas must be persisted in the sidecar bundle"
    );
    assert!(
        sidecar_path_for(&dir.path().join("sample.png")).exists(),
        "the recipe link must be persisted in the sidecar"
    );
    // Toggling off clears the canvas (identity, no expand).
    app.set_expand_beyond_image(false).unwrap();
    assert!(!app
        .recipe()
        .generative_edit
        .as_ref()
        .unwrap()
        .effective_expand());
    assert!(app
        .recipe()
        .generative_edit
        .as_ref()
        .unwrap()
        .canvas
        .is_none());
}
