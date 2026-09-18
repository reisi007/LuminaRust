use super::*;

#[test]
fn default_crop_after_perspective_excludes_transparent_wedges() {
    let mut frame = maxrect_source(40, 30);
    frame
        .apply_perspective_stage(
            None,
            Some(&wedge_perspective()),
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    let canvas = (frame.width, frame.height);
    assert!(
        has_transparent_alpha(&frame),
        "perspective must introduce transparent wedges"
    );
    let uncropped = frame.clone();

    frame.apply_crop_stage(None, true).unwrap();
    assert!(
        !has_transparent_alpha(&frame),
        "the default crop must contain only content"
    );
    assert!(
        (frame.width, frame.height) != canvas,
        "a wedge must shrink the frame"
    );
    assert!(frame.width <= canvas.0 && frame.height <= canvas.1);

    // Exact rect: the default crop is byte-identical to cropping the
    // uncropped geometry result at the computed maximum-content rectangle.
    let (x, y, w, h) = maximum_content_rect(&uncropped).expect("content rect");
    let expected = crop_frame(&uncropped, x, y, w, h).unwrap();
    assert_eq!(
        (frame.width, frame.height, frame.pixels),
        (expected.width, expected.height, expected.pixels)
    );
}

#[test]
fn explicit_crop_is_untouched_by_default_maxrect() {
    let mut frame = maxrect_source(40, 30);
    frame
        .apply_perspective_stage(
            None,
            Some(&wedge_perspective()),
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    // A free crop that deliberately keeps the full (transparent) canvas.
    let geometry = lumina_sidecar::Geometry {
        version: 1,
        crop: Some(lumina_sidecar::Crop::Free {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    };
    let mut with_default = frame.clone();
    with_default
        .apply_crop_stage(Some(&geometry), true)
        .unwrap();
    let mut without_default = frame.clone();
    without_default
        .apply_crop_stage(Some(&geometry), false)
        .unwrap();
    assert_eq!(with_default.pixels, without_default.pixels);
    assert_eq!(
        (with_default.width, with_default.height),
        (without_default.width, without_default.height)
    );
    // The authored crop wins: it keeps the transparent wedge.
    assert!(has_transparent_alpha(&with_default));
}

#[test]
fn default_crop_without_correction_is_identity() {
    // Fully opaque source, no lens/perspective: the render is byte-identical
    // and keeps its dimensions.
    let source = maxrect_source(24, 16);
    let recipe = EditRecipe::default();
    let context = RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        depth: None,
        lensfun: None,
    };
    let out = render_frame(&source, &context).unwrap().frame;
    assert_eq!((out.width, out.height), (24, 16));
    assert_eq!(out.pixels, source.pixels);

    // A source whose *own* transparent border is not a geometry wedge is
    // never cropped without a correction: no crop without a reason.
    let mut bordered = maxrect_source(24, 16);
    for x in 0..24 {
        bordered.pixels[x * 4 + 3] = 0;
    }
    let out2 = render_frame(&bordered, &context).unwrap().frame;
    assert_eq!((out2.width, out2.height), (24, 16));
    assert_eq!(out2.pixels, bordered.pixels);
    assert!(has_transparent_alpha(&out2));
}

#[test]
fn render_default_crop_after_lens_is_content_only() {
    // k1 < 0 maps the output corners outside the source (transparent
    // wedge); the default crop must then keep only content. k1 > 0 leaves
    // the corners inside the source — no wedge, so the default stays the
    // identity full frame.
    let source = maxrect_source(64, 48);
    let lens = |k1: f32| lumina_sidecar::LensCorrection {
        version: 1,
        profile: None,
        distortion_k1: Some(k1),
        distortion_k2: Some(0.0),
        distortion_k3: Some(0.0),
        vignette_c0: None,
        vignette_c1: None,
        vignette_c2: None,
        ca_red: None,
        ca_blue: None,
    };
    let full_crop = lumina_sidecar::Geometry {
        version: 1,
        crop: Some(lumina_sidecar::Crop::Free {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    };

    // Wedge case: the default crop keeps only content and matches the
    // explicit maximum-content rectangle byte-for-byte.
    let wedge_recipe = EditRecipe {
        lens_correction: Some(lens(-0.5)),
        ..Default::default()
    };
    let context = RenderContext {
        recipe: &wedge_recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        depth: None,
        lensfun: None,
    };
    let cropped = render_frame(&source, &context).unwrap().frame;
    assert!(
        !has_transparent_alpha(&cropped),
        "the shared render entry point must exclude lens wedges by default"
    );
    assert!(
        cropped.width < 64 && cropped.height < 48,
        "a negative-k1 lens wedge must shrink the frame"
    );
    let uncropped_recipe = EditRecipe {
        lens_correction: Some(lens(-0.5)),
        geometry: Some(full_crop),
        ..Default::default()
    };
    let uncropped_context = RenderContext {
        recipe: &uncropped_recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        depth: None,
        lensfun: None,
    };
    let uncropped = render_frame(&source, &uncropped_context).unwrap().frame;
    assert_eq!((uncropped.width, uncropped.height), (64, 48));
    assert!(has_transparent_alpha(&uncropped));
    let (x, y, w, h) = maximum_content_rect(&uncropped).expect("content rect");
    let expected = crop_frame(&uncropped, x, y, w, h).unwrap();
    assert_eq!(
        (cropped.width, cropped.height, cropped.pixels),
        (expected.width, expected.height, expected.pixels)
    );

    // No-wedge lens: a correction that introduces no transparent edge is
    // not cropped (no crop without a reason).
    let flat_recipe = EditRecipe {
        lens_correction: Some(lens(0.5)),
        ..Default::default()
    };
    let flat_context = RenderContext {
        recipe: &flat_recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        depth: None,
        lensfun: None,
    };
    let flat = render_frame(&source, &flat_context).unwrap().frame;
    assert_eq!((flat.width, flat.height), (64, 48));
}

#[test]
fn auto_filled_lens_render_keeps_full_frame_without_silent_crop() {
    // Auto-fill removes the wedges; because the default crop is
    // content-based it must then keep the full (filled) frame instead of
    // cropping away freshly generated content.
    let source = maxrect_source(64, 48);
    let lens = lumina_sidecar::LensCorrection {
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
    let recipe = |auto_fill: bool| EditRecipe {
        lens_correction: Some(lens.clone()),
        generative_edit: Some(lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas: None,
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: Some(auto_fill),
            expand_beyond_image: None,
            seed: Some(7),
            prompt: None,
            extras: Default::default(),
        }),
        ..Default::default()
    };

    // Control: without auto-fill the transparent wedge is cropped.
    let plain_recipe = recipe(false);
    let plain_context = RenderContext {
        recipe: &plain_recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        depth: None,
        lensfun: None,
    };
    let plain = render_frame(&source, &plain_context).unwrap().frame;
    assert!(
        plain.width < 64 && plain.height < 48,
        "control without auto-fill must crop the lens wedge"
    );

    // Auto-filled: no transparent pixel remains, so nothing is cropped.
    // GEN-ONNX-1: the render consumes a caller-supplied composited canvas;
    // build it from the post-lens frame (deterministic producer).
    let filled_recipe = recipe(true);
    let filled_context = RenderContext {
        recipe: &filled_recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        depth: None,
        lensfun: None,
    };
    let mut lensed = source.clone();
    #[cfg(feature = "lensfun")]
    lensed
        .apply_lens_stage(filled_recipe.lens_correction.as_ref(), None)
        .unwrap();
    #[cfg(not(feature = "lensfun"))]
    lensed
        .apply_lens_stage(filled_recipe.lens_correction.as_ref())
        .unwrap();
    let mut canvas = lensed.clone();
    crate::generative::fill_transparent_heuristic(&mut canvas, 7);
    let artifact = crate::generative::GenerativeCanvasArtifact::new(
        crate::generative::GenerativeRole::AutoFillTransparent,
        canvas,
    );
    let filled = crate::render_frame_with_generative(
        &source,
        &filled_context,
        crate::generative::GenerativeCanvasInput {
            auto_fill: Some(&artifact),
            expand: None,
        },
    )
    .unwrap()
    .frame;
    assert!(!has_transparent_alpha(&filled));
    assert_eq!(
        (filled.width, filled.height),
        (64, 48),
        "a fully filled frame must not be silently cropped"
    );
}

#[test]
fn invalid_generative_edit_is_rejected_not_silently_ignored() {
    fn recipe_with(ge: lumina_sidecar::GenerativeEdit) -> lumina_sidecar::EditRecipe {
        lumina_sidecar::EditRecipe {
            generative_edit: Some(ge),
            ..Default::default()
        }
    }
    fn edit(
        canvas: Option<lumina_sidecar::GenerativeCanvas>,
        expand: Option<bool>,
    ) -> lumina_sidecar::GenerativeEdit {
        lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas,
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: None,
            expand_beyond_image: expand,
            seed: None,
            prompt: None,
            extras: Default::default(),
        }
    }
    let canvas = lumina_sidecar::GenerativeCanvas {
        output_width: 12,
        output_height: 12,
        source_offset_x: 2,
        source_offset_y: 2,
        extras: Default::default(),
    };
    let frame = ImageFrame::new(8, 8, vec![5u8; 8 * 8 * 4]).unwrap();
    // expand=true without canvas, canvas without expand=true, bad
    // version, and zero-size canvas are all hard errors.
    for bad in [
        edit(None, Some(true)),
        edit(Some(canvas.clone()), Some(false)),
        edit(Some(canvas.clone()), None),
        lumina_sidecar::GenerativeEdit {
            version: 2,
            ..edit(Some(canvas.clone()), Some(true))
        },
        edit(
            Some(lumina_sidecar::GenerativeCanvas {
                output_width: 0,
                output_height: 12,
                source_offset_x: 0,
                source_offset_y: 0,
                extras: Default::default(),
            }),
            Some(true),
        ),
    ] {
        assert!(
            frame.clone().apply_recipe(&recipe_with(bad)).is_err(),
            "invalid generative_edit must be rejected"
        );
    }
    // Valid combinations apply without error (identity without expand).
    assert!(frame
        .clone()
        .apply_recipe(&recipe_with(edit(None, None)))
        .is_ok());
    assert!(frame
        .clone()
        .apply_recipe(&recipe_with(edit(None, Some(false))))
        .is_ok());
}
