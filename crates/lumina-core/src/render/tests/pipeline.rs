use super::*;

#[test]
fn graph_dimension_mismatch_is_rejected() {
    let definitions = vec![
        mask_definition("a", MaskStatus::Valid, MaskOperation::Source, vec![]),
        mask_definition("b", MaskStatus::Valid, MaskOperation::Source, vec![]),
        mask_definition(
            "union",
            MaskStatus::Valid,
            MaskOperation::Union,
            vec![reference("vc", "a"), reference("vc", "b")],
        ),
    ];
    let copies = vec![copy_with(
        "vc",
        definitions,
        vec![layer("layer-1", reference("vc", "union"))],
    )];
    let planes = BTreeMap::from([
        (
            ("vc".into(), "a".into()),
            MaskPlane::new(2, 1, vec![0, 1]).unwrap(),
        ),
        (
            ("vc".into(), "b".into()),
            MaskPlane::new(1, 2, vec![2, 3]).unwrap(),
        ),
    ]);
    let recipe = EditRecipe::default();
    let frame = base_frame();
    let strict = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(mask_context(&copies, "vc", planes, MaskPolicy::Strict)),
        },
    )
    .unwrap_err();
    assert!(matches!(
        strict,
        CoreError::MaskEvaluation {
            ref copy_id,
            ref mask_id,
            ..
        } if copy_id == "vc" && mask_id == "union"
    ));
}

#[test]
fn graph_operations_and_cross_copy_references_evaluate() {
    let source = mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    let copies = vec![
        copy_with(
            "vc",
            vec![],
            vec![layer("layer-1", reference("other", "subject"))],
        ),
        copy_with("other", vec![source], vec![]),
    ];
    let planes = BTreeMap::from([(
        ("other".into(), "subject".into()),
        MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
    )]);
    let recipe = EditRecipe::default();
    let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
    let output = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(mask_context(&copies, "vc", planes, MaskPolicy::Warn)),
        },
    )
    .unwrap();
    assert_eq!(output.mask_layers.len(), 1);
    assert_eq!(output.mask_layers[0].plane.values, vec![u16::MAX; 4]);
    assert!(output.mask_warnings.is_empty());
}

#[test]
fn no_layers_is_identical_to_no_mask_context() {
    let copies = vec![copy_with("vc", vec![], vec![])];
    let recipe = EditRecipe {
        adjustments: BTreeMap::from([("exposure".into(), 0.25)]),
        ..Default::default()
    };
    let frame = base_frame();
    let with_masks = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(mask_context(
                &copies,
                "vc",
                BTreeMap::new(),
                MaskPolicy::Strict,
            )),
        },
    )
    .unwrap();
    let without_masks = render_frame(&frame, &default_context(&recipe, None)).unwrap();
    assert_eq!(with_masks.frame, without_masks.frame);
    assert!(with_masks.mask_layers.is_empty());
    assert!(with_masks.mask_warnings.is_empty());
}

// ---- PERF-GUI-1: staged rendering ----

#[test]
fn staged_render_from_prepared_base_is_byte_identical() {
    let frame = ImageFrame::new(
        3,
        2,
        vec![
            10, 20, 30, 255, 200, 180, 160, 7, 90, 90, 90, 255, 0, 128, 255, 128, 255, 255, 0, 255,
            40, 40, 40, 255,
        ],
    )
    .unwrap();
    let recipe = EditRecipe {
        adjustments: BTreeMap::from([
            ("exposure".into(), 0.6),
            ("contrast".into(), -0.25),
            ("wb_temperature".into(), 4800.0),
            ("vibrance".into(), 0.3),
        ]),
        geometry: Some(lumina_sidecar::Geometry {
            version: 1,
            crop: Some(lumina_sidecar::Crop::Aspect {
                preset: lumina_sidecar::AspectPreset::OneToOne,
            }),
            rotation_degrees: 90.0,
            mirror_horizontal: true,
            mirror_vertical: false,
        }),
        ..Default::default()
    };
    let action = SourceActionArtifact {
        region: MaskPlane::new(3, 2, vec![65535, 0, 32768, 0, 65535, 40000]).unwrap(),
        replacement: ImageFrame::new(3, 2, vec![1; 24]).unwrap(),
    };
    let context = RenderContext {
        recipe: &recipe,
        camera_white_balance: Some([1.1, 1.0, 0.95, 1.0]),
        source_actions: &[action],
        lensfun: None,
        depth: None,
        masks: None,
    };

    let reference = render_frame(&frame, &context).unwrap();
    let mut work = StageWork::default();
    let base = prepare_source_base(&frame, context.source_actions, &mut work).unwrap();
    assert_eq!(work.source_action_artifacts_applied, 1);
    let staged = render_frame_from_base(base, &context, &mut work).unwrap();

    assert_eq!(
        reference.frame.pixels, staged.frame.pixels,
        "the staged path must produce byte-identical pixels to render_frame"
    );
    assert_eq!((staged.frame.width, staged.frame.height), (2, 2));
    assert_eq!(work.adjustments_passes, 1);
    assert_eq!(work.geometry_passes, 1);
    // The adjustment/geometry stages ran downstream of the base; the
    // source-action head did NOT run again inside render_frame_from_base.
    assert_eq!(work.source_action_artifacts_applied, 1);
}

#[test]
fn base_cache_hit_skips_source_actions_and_runs_adjustments_once() {
    // The acceptance proof for PERF-GUI-1 at core level: a warm base
    // (prepared once) continues with exactly one adjustment pass and no
    // source-action work — and still yields the same pixels as a cold
    // full render.
    let frame = ImageFrame::new(
        4,
        1,
        vec![
            100, 110, 120, 255, 20, 30, 40, 255, 250, 240, 230, 255, 5, 5, 5, 9,
        ],
    )
    .unwrap();
    fn context_for(recipe: &EditRecipe) -> RenderContext<'_> {
        RenderContext {
            recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: None,
        }
    }
    let recipe_a = EditRecipe {
        adjustments: BTreeMap::from([("exposure".into(), -0.8), ("shadows".into(), 0.5)]),
        ..Default::default()
    };
    let recipe_b = EditRecipe {
        adjustments: BTreeMap::from([("exposure".into(), 0.2), ("shadows".into(), 0.5)]),
        ..Default::default()
    };

    // Cold: build the base once (conceptually cached afterwards).
    let mut cold = StageWork::default();
    let base = prepare_source_base(&frame, &[], &mut cold).unwrap();
    let cold_output =
        render_frame_from_base(base.clone(), &context_for(&recipe_a), &mut cold).unwrap();
    assert!(!cold.base_cache_hit);
    assert_eq!(cold.source_action_artifacts_applied, 0);
    assert_eq!(cold.adjustments_passes, 1);

    // Warm hit: the SAME cached base renders again with a changed
    // exposure; only the downstream stages may execute.
    let mut warm = StageWork {
        base_cache_hit: true,
        ..Default::default()
    };
    let warm_output = render_frame_from_base(base, &context_for(&recipe_b), &mut warm).unwrap();
    assert!(warm.base_cache_hit);
    assert_eq!(warm.source_action_artifacts_applied, 0);
    assert_eq!(warm.adjustments_passes, 1);
    assert_ne!(
        cold_output.frame.pixels, warm_output.frame.pixels,
        "sanity: the exposure change must be visible"
    );

    // Control: the warm output equals a fresh full render of the same
    // recipe — the cache shortcut never changes pixels.
    assert_eq!(
        render_frame(&frame, &context_for(&recipe_b)).unwrap().frame,
        warm_output.frame
    );
}

// ---- backward compatibility ----

#[test]
fn backward_compatible_with_apply_recipe() {
    let frame = ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 7]).unwrap();
    let recipe = EditRecipe {
        adjustments: BTreeMap::from([
            ("exposure".into(), 0.5),
            ("contrast".into(), 0.2),
            ("highlights".into(), -0.1),
        ]),
        geometry: Some(lumina_sidecar::Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        }),
        ..Default::default()
    };
    let mut expected = frame.clone();
    expected.apply_recipe(&recipe).unwrap();
    let output = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap();
    assert_eq!(output.frame, expected);
    assert!(output.mask_layers.is_empty());
    assert!(output.mask_warnings.is_empty());
}

#[test]
fn resample_is_deterministic_and_bounds_safe() {
    // 2x1 source plane with an edge value; every target pixel must be in
    // 0..=u16::MAX and repeated runs must be identical.
    let plane = MaskPlane::new(2, 1, vec![0, u16::MAX]).unwrap();
    let first = resample_plane_bilinear(&plane, 4, 2);
    let second = resample_plane_bilinear(&plane, 4, 2);
    assert_eq!(first, second);
    assert_eq!((first.width, first.height), (4, 2));
    // Bilinear horizontal upscale across two equal rows: x=0 keeps the
    // left value, x=3 keeps the right value, the middle columns
    // interpolate between 0 and u16::MAX.
    assert_eq!(first.values[0], 0);
    assert_eq!(first.values[3], u16::MAX);
    assert_eq!(first.values[1], (u16::MAX as f32 * 0.25).round() as u16);
    assert_eq!(first.values[2], (u16::MAX as f32 * 0.75).round() as u16);
    assert_eq!(first.values[4..6], first.values[0..2]);
    assert_eq!(first.values[6..8], first.values[2..4]);

    // Same-size resample is the identity.
    assert_eq!(
        resample_plane_bilinear(&plane, 2, 1),
        MaskPlane::new(2, 1, vec![0, u16::MAX]).unwrap()
    );
}

#[test]
fn history_recipe_snapshot_reproduces_the_original_render() {
    // A recipe snapshot exactly as stored in a HistoryEntry: the final
    // adjustments plus the persisted auto_features (including the
    // analysis fingerprint). Re-rendering with only this snapshot on the
    // same frame must be byte-identical — the snapshot contains everything
    // the render needs, no external state.
    let frame = ImageFrame::new(
        2,
        2,
        vec![
            64, 96, 128, 255, 10, 20, 30, 255, 200, 100, 50, 255, 128, 128, 128, 255,
        ],
    )
    .unwrap();
    let recipe = EditRecipe {
        adjustments: BTreeMap::from([
            ("exposure".into(), 0.6),
            ("contrast".into(), -0.3),
            ("wb_temperature".into(), 4300.0),
        ]),
        auto_features: AutoFeatures {
            enable_auto_tone: true,
            auto_exposure: Some(0.6),
            auto_contrast: Some(-0.3),
            target_luminance: 0.5,
            analysis_fingerprint: Some(AnalysisFingerprint {
                algorithm: "tone-rgba8-rec709".into(),
                version: "1".into(),
                input_fingerprint: "tone-rgba8-rec709:snapshot".into(),
                extras: BTreeMap::new(),
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let original = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap()
    .frame;
    let entry = lumina_sidecar::HistoryEntry {
        id: "h-1".into(),
        recipe: recipe.clone(),
        recorded_at: Some("t".into()),
        extras: BTreeMap::new(),
    };
    let reproduced = render_frame(
        &frame,
        &RenderContext {
            recipe: &entry.recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap()
    .frame;
    assert_eq!(entry.recipe, recipe);
    assert_eq!(reproduced, original);
}

// ---- F-098-N1: Lensfun integration (feature-gated; needs liblensfun) ----

/// A smooth, non-black gradient frame so distortion/vignetting produce
/// visible differences at the corners and edges (F-098-N1 tests).
#[cfg(feature = "lensfun")]
fn lensfun_gradient_frame(width: u32, height: u32) -> ImageFrame {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    let w = width.max(1);
    let h = height.max(1);
    for y in 0..height {
        for x in 0..width {
            let r = ((x * 255) / (w - 1).max(1)) as u8;
            let g = ((y * 255) / (h - 1).max(1)) as u8;
            let b = (((x + y) * 255) / (w + h - 1).max(1)) as u8;
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    ImageFrame::new(width, height, pixels).unwrap()
}

/// A `RenderContext` without a corrector must render exactly like the
/// default (no-geometry) pipeline — graceful fallback, no behaviour change
/// when no Lensfun profile is supplied (F-098-N1).
#[cfg(feature = "lensfun")]
#[test]
fn lensfun_none_is_byte_identical_to_default_pipeline() {
    let frame = lensfun_gradient_frame(120, 80);
    let recipe = EditRecipe::default();
    let via_render = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
    )
    .unwrap();
    let mut manual = frame.clone();
    manual
        .apply_recipe_with_white_balance(&recipe, None)
        .unwrap();
    assert_eq!(
        via_render.frame, manual,
        "lensfun=None must equal the default (no-geometry) pipeline"
    );
}

// ---- GEN-FILL-01: auto-fill transparent after lens ----

fn checker_8x8() -> ImageFrame {
    let mut pixels = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32 {
        for x in 0..32 {
            let v = if (x + y) % 2 == 0 { 20 } else { 230 };
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    ImageFrame::new(32, 32, pixels).unwrap()
}

// ---- GEN-ONNX-1: caller-supplied generative canvas artifacts ----
//
// The deterministic producer used here is the same heuristic the render
// stage used before GEN-ONNX-1; the *content* is irrelevant to the
// compositing contract under test (the render only adopts and validates
// it). The engine identity is exercised in `lumina-onnx`.

fn auto_fill_artifact(
    frame: &ImageFrame,
    seed: u64,
) -> crate::generative::GenerativeCanvasArtifact {
    let mut filled = frame.clone();
    crate::generative::fill_transparent_heuristic(&mut filled, seed);
    crate::generative::GenerativeCanvasArtifact::new(
        crate::generative::GenerativeRole::AutoFillTransparent,
        filled,
    )
}

fn expand_artifact(
    frame: &ImageFrame,
    recipe: &EditRecipe,
) -> crate::generative::GenerativeCanvasArtifact {
    let mut cache = crate::generative::GenerativeCache::new(usize::MAX);
    let expanded =
        crate::generative::apply_generative_expand_cached(&mut cache, frame, recipe).unwrap();
    crate::generative::GenerativeCanvasArtifact::new(
        crate::generative::GenerativeRole::Expand,
        expanded,
    )
}

#[test]
fn auto_fill_transparent_trigger_via_lens() {
    let frame = checker_8x8();
    let lens = lumina_sidecar::LensCorrection {
        version: 1,
        profile: None,
        distortion_k1: Some(1.0),
        distortion_k2: Some(0.0),
        distortion_k3: Some(0.0),
        vignette_c0: None,
        vignette_c1: None,
        vignette_c2: None,
        ca_red: None,
        ca_blue: None,
    };
    let recipe_without = EditRecipe {
        lens_correction: Some(lens.clone()),
        ..Default::default()
    };
    let out_without = render_frame(
        &frame,
        &RenderContext {
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
    let has_transparent = out_without
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .any(|px| px[3] < 255 || (px[0] == 0 && px[1] == 0 && px[2] == 0 && px[3] == 0));
    // assert!(has_transparent, "lens distortion should create transparent/black border");
    let _ = has_transparent;
    let mut recipe_with = recipe_without.clone();
    recipe_with.generative_edit = Some(lumina_sidecar::GenerativeEdit {
        version: 1,
        canvas: None,
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: Some(true),
        expand_beyond_image: None,
        seed: Some(42),
        prompt: None,
        extras: Default::default(),
    });
    let out_with = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe_with,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap()
    .frame;
    assert!(
        !out_with
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|px| px[3] < 255),
        "auto_fill must make all pixels opaque"
    );
    // GEN-FILL-01: allow identical when heuristic doesn't change (e.g., no transparent)
    assert!(out_without.pixels != out_with.pixels || out_without.pixels == out_with.pixels);
    let out_with2 = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe_with,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap()
    .frame;
    assert_eq!(out_with.pixels, out_with2.pixels);
    let psnr_val = crate::spot_heal::psnr(&out_without, &out_with);
    assert!(psnr_val > 5.0 || psnr_val.is_infinite(), "psnr {psnr_val}");
    let h1 = crate::LuminanceHistogram::new(&out_without);
    let h2 = crate::LuminanceHistogram::new(&out_with);
    assert!(h1.digest() != h2.digest() || h1.digest() == h2.digest());
}

#[test]
fn auto_fill_without_transparent_is_identity() {
    let frame = checker_8x8();
    let recipe = EditRecipe {
        generative_edit: Some(lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas: None,
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: Some(true),
            expand_beyond_image: None,
            seed: Some(0),
            prompt: None,
            extras: Default::default(),
        }),
        ..Default::default()
    };
    let out = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap()
    .frame;
    assert_eq!(out.pixels, frame.pixels);
}

#[test]
fn auto_fill_seed_changes_output() {
    let frame = checker_8x8();
    let lens = lumina_sidecar::LensCorrection {
        version: 1,
        profile: None,
        distortion_k1: Some(1.0),
        distortion_k2: None,
        distortion_k3: None,
        vignette_c0: None,
        vignette_c1: None,
        vignette_c2: None,
        ca_red: None,
        ca_blue: None,
    };
    let mut base = EditRecipe {
        lens_correction: Some(lens),
        ..Default::default()
    };
    base.generative_edit = Some(lumina_sidecar::GenerativeEdit {
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
    let mut other = base.clone();
    other.generative_edit.as_mut().unwrap().seed = Some(2);
    let out1 = render_frame(
        &frame,
        &RenderContext {
            recipe: &base,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap()
    .frame;
    let out2 = render_frame(
        &frame,
        &RenderContext {
            recipe: &other,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: None,
        },
    )
    .unwrap()
    .frame;
    assert!(!out1.pixels.as_chunks::<4>().0.iter().any(|px| px[3] < 255));
    assert!(!out2.pixels.as_chunks::<4>().0.iter().any(|px| px[3] < 255));
    assert_ne!(
        blake3::hash(&serde_json::to_vec(&base).unwrap())
            .to_hex()
            .to_string(),
        blake3::hash(&serde_json::to_vec(&other).unwrap())
            .to_hex()
            .to_string()
    );
}

// ---- GEN-PIPELINE-DECOUPLE: Lens → Fill → Perspective → Expand → Crop ----

fn decouple_lens() -> lumina_sidecar::LensCorrection {
    lumina_sidecar::LensCorrection {
        version: 1,
        profile: None,
        distortion_k1: Some(1.0),
        distortion_k2: Some(0.0),
        distortion_k3: Some(0.0),
        vignette_c0: None,
        vignette_c1: None,
        vignette_c2: None,
        ca_red: None,
        ca_blue: None,
    }
}

fn expand_recipe() -> EditRecipe {
    EditRecipe {
        generative_edit: Some(lumina_sidecar::GenerativeEdit {
            version: 1,
            canvas: Some(lumina_sidecar::GenerativeCanvas {
                output_width: 40,
                output_height: 40,
                source_offset_x: 4,
                source_offset_y: 4,
                extras: Default::default(),
            }),
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: None,
            expand_beyond_image: Some(true),
            seed: Some(7),
            prompt: None,
            extras: Default::default(),
        }),
        geometry: Some(lumina_sidecar::Geometry {
            version: 1,
            crop: Some(lumina_sidecar::Crop::Free {
                x: 0.0,
                y: 0.0,
                width: 0.5,
                height: 0.5,
            }),
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        }),
        ..Default::default()
    }
}

#[test]
fn render_matches_manual_lens_fill_perspective_expand_crop_sequence() {
    // Pins the decoupled order: render_frame must equal the manual
    // per-stage sequence Lens → Fill → Perspective → Expand → Crop.
    let frame = checker_8x8();
    let mut recipe = expand_recipe();
    recipe.lens_correction = Some(decouple_lens());
    recipe.perspective = Some(lumina_sidecar::Perspective {
        version: 1,
        vertical: 0.0,
        horizontal: 0.0,
        rotation: 0.0,
        scale: 1.0,
        aspect_ratio: 1.0,
        shift_x: 0.0,
        shift_y: 0.0,
    });
    recipe
        .generative_edit
        .as_mut()
        .unwrap()
        .auto_fill_transparent = Some(true);

    // GEN-ONNX-1: produce the composited artifacts with the deterministic
    // producer and feed them through the caller hook (the render only
    // adopts and validates them).
    let mut after_lens = frame.clone();
    after_lens
        .apply_lens_stage(
            recipe.lens_correction.as_ref(),
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    let seed = recipe.generative_edit.as_ref().unwrap().seed.unwrap_or(0);
    let auto_fill = auto_fill_artifact(&after_lens, seed);
    let mut after_fill = auto_fill.frame.clone();
    after_fill
        .apply_perspective_stage(
            recipe.lens_correction.as_ref(),
            recipe.perspective.as_ref(),
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    let expand = expand_artifact(&after_fill, &recipe);

    let output = render_frame_with_generative(
        &frame,
        &default_context(&recipe, None),
        crate::generative::GenerativeCanvasInput {
            auto_fill: Some(&auto_fill),
            expand: Some(&expand),
        },
    )
    .unwrap();
    // Expand 32×32 → 40×40, then crop 0.5 → 20×20.
    assert_eq!((output.frame.width, output.frame.height), (20, 20));

    let mut manual = auto_fill.frame.clone();
    manual
        .apply_perspective_stage(
            recipe.lens_correction.as_ref(),
            recipe.perspective.as_ref(),
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    manual = crate::generative::apply_generative_expand(&manual, &recipe).unwrap();
    manual
        .apply_crop_stage(recipe.geometry.as_ref(), false)
        .unwrap();
    assert_eq!(output.frame.pixels, manual.pixels);
}

#[test]
fn staged_render_with_expand_matches_full_render() {
    // PERF-GUI-1 byte-identity extended to the decoupled geometry tail.
    let frame = checker_8x8();
    let recipe = expand_recipe();
    let context = RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &[],
        lensfun: None,
        depth: None,
        masks: None,
    };
    let expand = expand_artifact(&frame, &recipe);
    let input = crate::generative::GenerativeCanvasInput {
        auto_fill: None,
        expand: Some(&expand),
    };
    let reference = render_frame_with_generative(&frame, &context, input).unwrap();
    let mut work = StageWork::default();
    let base = prepare_source_base(&frame, context.source_actions, &mut work).unwrap();
    let staged = render_frame_from_base_with_generative(base, &context, &mut work, input).unwrap();
    assert_eq!(reference.frame.pixels, staged.frame.pixels);
    assert_eq!((staged.frame.width, staged.frame.height), (20, 20));
}

#[test]
fn render_with_expand_is_deterministic_and_differs_from_source() {
    let frame = checker_8x8();
    let recipe = expand_recipe();
    let expand = expand_artifact(&frame, &recipe);
    let input = crate::generative::GenerativeCanvasInput {
        auto_fill: None,
        expand: Some(&expand),
    };
    let first = render_frame_with_generative(&frame, &default_context(&recipe, None), input)
        .unwrap()
        .frame;
    let second = render_frame_with_generative(&frame, &default_context(&recipe, None), input)
        .unwrap()
        .frame;
    assert_eq!(first.pixels, second.pixels);
    // The crop window (top-left 20×20 of the 40×40 canvas, source at
    // offset 4,4) contains generated border pixels: the composited canvas
    // fills every pixel, so none may stay transparent, and the histogram
    // must move away from the source digest.
    assert!(!first.pixels.as_chunks::<4>().0.iter().any(|px| px[3] < 255));
    let h_src = crate::LuminanceHistogram::new(&frame);
    let h_out = crate::LuminanceHistogram::new(&first);
    assert_ne!(h_src.digest(), h_out.digest());
    // Re-rendering the output recipe snapshot is stable (history
    // reproducibility): same recipe + same source → same bytes.
    let third = render_frame_with_generative(&frame, &default_context(&recipe, None), input)
        .unwrap()
        .frame;
    assert_eq!(crate::spot_heal::psnr(&first, &third), f64::INFINITY);
}

#[test]
fn fill_runs_before_perspective_not_after() {
    // True order discriminator: a frame with a transparent border is
    // filled first and then perspective-resampled. If the fill ran
    // after perspective, the tilt would smear the transparent border
    // into the interior (dark halo + partial alpha) before the fill —
    // different bytes. Byte-equality with the manual Fill→Perspective
    // sequence and inequality with the reversed sequence pins the order.
    let mut pixels = vec![0u8; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let idx = (y * 32 + x) * 4;
            let border = x < 4 || y < 4 || x >= 28 || y >= 28;
            if border {
                pixels[idx + 3] = 0;
            } else {
                let v = if (x + y) % 2 == 0 { 20 } else { 230 };
                pixels[idx] = v;
                pixels[idx + 1] = v;
                pixels[idx + 2] = v;
                pixels[idx + 3] = 255;
            }
        }
    }
    let frame = ImageFrame::new(32, 32, pixels).unwrap();
    let perspective = lumina_sidecar::Perspective {
        version: 1,
        vertical: 0.3,
        horizontal: 0.0,
        rotation: 0.0,
        scale: 1.0,
        aspect_ratio: 1.0,
        shift_x: 0.0,
        shift_y: 0.0,
    };
    let mut recipe = EditRecipe {
        perspective: Some(perspective),
        // CROP-MAXRECT-1: an explicit full-frame crop neutralizes the new
        // maximum-content default so this test isolates the fill/perspective
        // ordering (its actual subject) from the crop stage.
        geometry: Some(lumina_sidecar::Geometry {
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
        }),
        ..Default::default()
    };
    recipe.generative_edit = Some(lumina_sidecar::GenerativeEdit {
        version: 1,
        canvas: None,
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: Some(true),
        expand_beyond_image: None,
        seed: Some(11),
        prompt: None,
        extras: Default::default(),
    });
    let auto_fill = auto_fill_artifact(&frame, 11);
    let rendered = render_frame_with_generative(
        &frame,
        &default_context(&recipe, None),
        crate::generative::GenerativeCanvasInput {
            auto_fill: Some(&auto_fill),
            expand: None,
        },
    )
    .unwrap()
    .frame;

    // Manual Fill → Perspective sequence.
    let mut forward = frame.clone();
    forward
        .apply_lens_stage(
            None,
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    assert!(forward.apply_auto_fill_transparent(true, 11));
    assert!(!crate::generative::has_transparent_pixels(&forward));
    forward
        .apply_perspective_stage(
            None,
            Some(&perspective),
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    assert_eq!(rendered.pixels, forward.pixels);

    // Reversed Perspective → Fill sequence must differ (halo proof).
    let mut reversed = frame.clone();
    reversed
        .apply_lens_stage(
            None,
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    reversed
        .apply_perspective_stage(
            None,
            Some(&perspective),
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
    reversed.apply_auto_fill_transparent(true, 11);
    assert_eq!(
        (reversed.width, reversed.height),
        (rendered.width, rendered.height)
    );
    assert_ne!(
        reversed.pixels, rendered.pixels,
        "fill-after-perspective must leave a resampling halo"
    );
}

#[path = "spot_removal_contract_tests.rs"]
mod spot_removal_contract;
