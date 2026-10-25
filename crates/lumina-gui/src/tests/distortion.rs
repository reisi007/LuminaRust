//! synthetic lens-distortion and crop-maxrect previews tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

#[test]
fn auto_fill_transparent_headless_synthetic_8x8_lens_distortion() {
    use lumina_core::{has_transparent_pixels, psnr, ImageFrame as CoreFrame, LuminanceHistogram};
    let mut pixels = Vec::with_capacity(8 * 8 * 4);
    for y in 0..8 {
        for x in 0..8 {
            let v = if (x + y) % 2 == 0 { 20 } else { 230 };
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let frame = CoreFrame::new(8, 8, pixels).unwrap();
    let png_bytes = frame.encode(lumina_core::ImageFileFormat::Png).unwrap();
    let mut app = new_app();
    app.load_bytes(png_bytes.clone(), "synthetic-8x8.png")
        .unwrap();
    // MUTATION-ANCHOR: a negative k1 maps border samples outside the 8×8
    // source, so the lens stage really produces a transparent border
    // (measured: 16 of 64 pixels). The former `+0.5` sampled strictly inside
    // the source and produced **zero** transparent pixels — the old
    // `|| !has_transparent_pixels(...)` escape and the always-true digest
    // comparison could never notice that no fill happened.
    let lens = LensCorrection {
        version: 1,
        profile: None,
        distortion_k1: Some(-0.5),
        distortion_k2: Some(0.0),
        distortion_k3: Some(0.0),
        vignette_c0: None,
        vignette_c1: None,
        vignette_c2: None,
        ca_red: None,
        ca_blue: None,
    };
    let generative = |auto_fill: bool, seed: Option<u64>| GenerativeEdit {
        version: 1,
        canvas: None,
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: Some(auto_fill),
        expand_beyond_image: None,
        seed,
        prompt: None,
        extras: Default::default(),
    };
    // CROP-MAXRECT-1: the alpha-based maximum-content default would crop away
    // exactly the border under test, so an explicit full-frame crop keeps it
    // observable (the fill stage itself runs before the crop).
    let geometry = Geometry {
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
    };
    let recipe_without = EditRecipe {
        lens_correction: Some(lens.clone()),
        generative_edit: Some(generative(false, None)),
        geometry: Some(geometry.clone()),
        ..Default::default()
    };
    // Anker 1: the frame the GUI producer feeds the auto-fill role —
    // `generate_generative_canvas` uses this very helper, and it is the stage
    // right after Lens in the render.
    let (after_lens, _) =
        generative_input_frames(&frame, &recipe_without, None, &[], None).unwrap();
    let transparent_before = after_lens
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[3] < 255)
        .count();
    assert!(
        transparent_before > 0 && has_transparent_pixels(&after_lens),
        "lens distortion (k1=-0.5) must create alpha-transparent border pixels"
    );
    let out_without_core = lumina_core::render_frame(
        &frame,
        &lumina_core::RenderContext {
            recipe: &recipe_without,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap()
    .frame;
    assert_eq!(
        out_without_core.pixels(),
        after_lens.pixels(),
        "without auto-fill the render must be the unfilled lens frame"
    );
    assert!(has_transparent_pixels(&out_without_core));

    // The real GUI path: auto-fill active, canvas produced by the app, then
    // the app's own render.
    app.recipe = EditRecipe {
        lens_correction: Some(lens),
        generative_edit: Some(generative(true, Some(42))),
        geometry: Some(geometry),
        ..Default::default()
    };
    let _dir = app_source_path(&mut app, &png_bytes, "synthetic-8x8.png");
    app.generate_generative_canvas().unwrap();
    assert!(
        app.generative_artifacts.auto_fill.is_some(),
        "the GUI must produce the auto-fill canvas for a transparent post-lens frame"
    );
    let with = app.preview().unwrap().clone();
    // Anker 2: transparent before, fully opaque after, and the pixels changed.
    let transparent_after = with
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[3] < 255)
        .count();
    assert_eq!(
        transparent_after, 0,
        "auto-fill must remove every alpha-transparent pixel (before: {transparent_before})"
    );
    assert!(
        !has_transparent_pixels(&with),
        "auto-fill must leave no transparent/black pixel in the app preview"
    );
    assert_ne!(
        out_without_core.pixels(),
        with.pixels(),
        "auto-fill must change the filled border pixels"
    );
    assert_eq!(
        (with.width(), with.height()),
        (frame.width(), frame.height())
    );
    // Seed-pinned determinism through a second app render.
    app.render().unwrap();
    assert_eq!(
        app.preview().unwrap().pixels(),
        with.pixels(),
        "seed-pinned auto_fill must be byte-identical"
    );
    // The difference is real, not a no-op: an unfilled frame would be
    // byte-identical (infinite PSNR), so no `||` escape remains.
    let psnr_val = psnr(&out_without_core, &with);
    assert!(
        psnr_val.is_finite() && psnr_val > 5.0,
        "PSNR {psnr_val} must be finite and above 5 dB"
    );
    let h1 = LuminanceHistogram::new(&out_without_core);
    let h2 = LuminanceHistogram::new(&with);
    assert_ne!(
        h1.digest(),
        h2.digest(),
        "auto-fill must move the luminance histogram"
    );
    let gen_before = app.preview_generation();
    app.recipe
        .generative_edit
        .as_mut()
        .unwrap()
        .auto_fill_transparent = Some(false);
    app.render().unwrap();
    app.recipe
        .generative_edit
        .as_mut()
        .unwrap()
        .auto_fill_transparent = Some(true);
    app.render().unwrap();
    assert!(
        app.preview_generation() > gen_before,
        "preview_generation must bump on auto_fill toggle"
    );
    let recipe_without2 = EditRecipe {
        generative_edit: Some(generative(false, None)),
        ..Default::default()
    };
    let mut recipe_with2 = recipe_without2.clone();
    recipe_with2
        .generative_edit
        .as_mut()
        .unwrap()
        .auto_fill_transparent = Some(true);
    let json_without = serde_json::to_vec(&recipe_without2).unwrap();
    let json_with = serde_json::to_vec(&recipe_with2).unwrap();
    assert_ne!(
        json_without, json_with,
        "recipe JSON must change with auto_fill flag"
    );
}

// ---- CROP-MAXRECT-1: default maximum-content crop through the GUI path ----

/// CROP-MAXRECT-1: the GUI preview uses the shared core render entry point,
/// so the maximum-content default crop applies without a second GUI rule.
/// Without a correction the preview is the identity full frame; after a
/// perspective correction it contains only content and matches the core
/// render byte-for-byte.
#[test]
fn crop_maxrect_default_applies_through_gui_preview() {
    use lumina_core::ImageFrame as CoreFrame;
    let (w, h) = (32u32, 24u32);
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let v = if (x + y) % 2 == 0 { 40 } else { 210 };
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let frame = CoreFrame::new(w, h, pixels).unwrap();
    let png = frame.encode(lumina_core::ImageFileFormat::Png).unwrap();
    let mut app = new_app();
    app.load_bytes(png, "maxrect-32x24.png").unwrap();

    // No correction: identity full frame (no crop without a reason).
    app.render().unwrap();
    let identity = app.preview().unwrap().clone();
    assert_eq!((identity.width(), identity.height()), (w, h));
    assert!(!identity
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .any(|px| px[3] < 255));

    // A perspective keystone introduces transparent wedges. The GUI setter
    // arms the same recipe field the render reads.
    app.set_perspective_value("vertical", 0.6);
    app.render().unwrap();
    let preview = app.preview().unwrap().clone();
    assert!(
        !preview
            .pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|px| px[3] < 255),
        "GUI preview must contain only content after perspective correction"
    );
    assert!(
        preview.width() <= w && preview.height() <= h,
        "default crop must stay constrained to the image"
    );

    // One code path: the preview is byte-identical to the core render.
    let core = lumina_core::render_frame(
        &frame,
        &lumina_core::RenderContext {
            recipe: &app.recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
    )
    .unwrap()
    .frame;
    assert_eq!(
        (preview.width(), preview.height()),
        (core.width(), core.height())
    );
    assert_eq!(preview.pixels(), core.pixels());
}
