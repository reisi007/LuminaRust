// SPLICE-GPU-PARITY-92 (Slice 8): reiner Move des Source-Action-Blocks aus `parity.rs`.
use super::*;

// ---------------------------------------------------------------------------
// "Nicht-GPU-taugliche Rezepte" inventory (GPU-RENDER-PARITY-1 follow-up)
// ---------------------------------------------------------------------------

fn source_action(id: &str) -> SourceActionSpec {
    SourceActionSpec {
        version: SOURCE_ACTION_VERSION,
        kind: SourceActionKind::DustRemoval,
        artifact: SourceActionArtifactRef {
            id: id.into(),
            relative_path: format!("{id}{ZDATA_FILE_SUFFIX}"),
            checksum: "unused".into(),
        },
    }
}

fn source_actions(count: usize) -> EditRecipe {
    EditRecipe {
        source_actions: (0..count)
            .map(|i| source_action(&format!("sa-{i}")))
            .collect(),
        ..Default::default()
    }
}

/// Nails down the complete set of recipe configurations that still CPU-route
/// after GPU-RENDER-PARITY-1. Every branch of
/// [`unsupported_gpu_stages_with_context`] must be represented by at least one
/// minimal example whose expected reason substring is asserted, so a future
/// change that silently drops a gate branch fails this test.
///
/// Stage 2 additionally **proves the implemented detail classes are
/// GPU-eligible**: non-neutral Effects (vignette + grain), Noise Reduction and
/// Sharpening must all yield an empty reason list (the pixel parity itself is
/// asserted by `implemented_stages_match_cpu_oracle`). Stage 3 does the same for
/// a valid red-eye correction.
///
/// Reason classes and their minimal trigger (the reason string is the one the
/// gate emits):
/// - `source_actions` — non-empty actions and `source_actions_bound = false`.
/// - `camera_white_balance (invalid As-Shot gains)` — an invalid context gain
///   (`inf`/`nan`/non-positive); a valid context is now GPU-carried and
///   therefore *not* a reason.
/// - `red_eye` — an **invalid** `recipe.red_eye` (out-of-range/NaN); a valid
///   correction is GPU-rendered.
///   (`generative_edit` is **no longer** a reason since GEN-ONNX-1 Welle 2a: the
///   artifact-aware entry `render_with_gpu_and_generative` injects the composited
///   canvas mid-geometry; an artifact-blind render fails loudly in the plan.)
/// - `adjustment \`clarity_v2\` not implemented on GPU` — a key outside
///   [`GPU_SUPPORTED_ADJUSTMENT_KEYS`] with no neutral value (all schema keys
///   are now GPU-supported, so this is the unknown-key class).
/// - `geometry (default content crop)` — a lens/perspective correction is
///   present and `geometry.crop` is `None`, so the CPU oracle applies its
///   content-based maximum-content-rect crop before rotation/mirroring. That
///   rectangle depends on the resampled alpha, so the recipe-only GPU plan
///   cannot reproduce its (possibly smaller) output dimensions
///   (GPU-MAXRECT-WELLE / CROP-MAXRECT-1).
///
/// GPU-RENDER-PARITY-1 geometry wave / GPU-MAXRECT-WELLE: geometry
/// (crop/rotation/mirror), the manual lens correction and perspective are
/// GPU-eligible for valid recipes **with an explicit crop** (their
/// pixel/dimension parity is asserted by `implemented_stages_match_cpu_oracle`),
/// so they are not gate reasons. The one geometry case that still flags is a
/// lens/perspective correction **without** an explicit crop: it activates the
/// content-based default crop (see `default_content_crop_recipes` and
/// `default_content_crop_routes_to_cpu_with_parity`).
///
/// GPU-RENDER-PARITY-1 lens-blur wave: G-05 lens blur is GPU-eligible for valid
/// recipes (heuristic **and** external depth, the latter requiring a bound
/// plane), so it is **not** a gate reason either (parity asserted by
/// `implemented_stages_match_cpu_oracle` and
/// `lens_blur_external_depth_matches_cpu_oracle`).
///
/// GPU-RENDER-PARITY-1 follow-up (items 5 + 7): legacy spot geometry, a typed
/// geometry-free mirror shadow, and **any number** of bound source actions
/// (batched in groups of [`MAX_SOURCE_ACTIONS`]) are GPU-eligible; their parity
/// is asserted by the oracle harness and `source_action_batch` tests, not by a
/// gate reason.
#[test]
fn cpu_routing_inventory_is_complete() {
    // GPU-RENDER-PARITY-1 stage 2 detail stages: implemented, so these must NOT
    // be flagged (byte-exactness/parity is proven by the oracle test).
    let effects = EditRecipe {
        effects: Some(Effects {
            vignette: Some(Vignette {
                version: 1,
                amount: -0.4,
                midpoint: 0.6,
                roundness: 1.0,
                feather: 0.5,
            }),
            grain: Some(Grain {
                version: 1,
                amount: 0.3,
                size: 0.4,
                roughness: 0.5,
                seed: 42,
            }),
        }),
        ..Default::default()
    };
    let noise = EditRecipe {
        noise_reduction: Some(NoiseReduction {
            version: 1,
            luminance: 0.5,
            color: 0.3,
        }),
        ..Default::default()
    };
    let sharpening = EditRecipe {
        sharpening: Some(Sharpening {
            version: 1,
            amount: 1.0,
            radius: 1.0,
            detail: 0.5,
            masking: 0.4,
        }),
        ..Default::default()
    };
    for (name, recipe) in [
        ("effects", &effects),
        ("noise_reduction", &noise),
        ("sharpening", &sharpening),
    ] {
        let reasons = unsupported_gpu_stages(recipe);
        assert!(
            reasons.is_empty(),
            "implemented stage `{name}` must be GPU-eligible, got {reasons:?}"
        );
    }

    // Stage 3: a valid red-eye correction is GPU-eligible; an invalid one must
    // stay flagged. (CAMERA-WB-WELLE: a valid As-Shot WB context is now
    // GPU-carried too; only invalid gains stay flagged — see the inventory
    // cases below.)
    assert!(
        unsupported_gpu_stages(&red_eye_recipe()).is_empty(),
        "valid red-eye must be GPU-eligible"
    );

    let geometry = EditRecipe {
        geometry: Some(Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        }),
        ..Default::default()
    };
    let lens = EditRecipe {
        lens_correction: Some(LensCorrection {
            version: 1,
            profile: None,
            distortion_k1: Some(0.1),
            distortion_k2: None,
            distortion_k3: None,
            vignette_c0: None,
            vignette_c1: None,
            vignette_c2: None,
            ca_red: None,
            ca_blue: None,
        }),
        ..Default::default()
    };
    let perspective = EditRecipe {
        perspective: Some(Perspective {
            version: 1,
            vertical: 0.1,
            horizontal: 0.0,
            rotation: 0.0,
            scale: 1.0,
            aspect_ratio: 1.0,
            shift_x: 0.0,
            shift_y: 0.0,
        }),
        ..Default::default()
    };
    let lens_blur = EditRecipe {
        lens_blur: Some(LensBlur {
            version: 1,
            enabled: true,
            focus_rect: FocusRect {
                x: 0.25,
                y: 0.25,
                width: 0.5,
                height: 0.5,
            },
            focal_near: 0.0,
            focal_far: 1.0,
            blur_amount: 0.5,
            bokeh: BokehShape::Round,
            depth_artifact: None,
        }),
        ..Default::default()
    };
    let unknown_key = EditRecipe {
        adjustments: BTreeMap::from([("clarity_v2".into(), 0.5)]),
        ..Default::default()
    };

    let wb = [0.0f32, 1.0, 1.0, 1.0];

    let cases: Vec<(&str, Vec<String>)> = vec![
        (
            "source_actions",
            unsupported_gpu_stages_for(&source_actions(1), false),
        ),
        (
            "camera_white_balance (invalid As-Shot gains)",
            unsupported_gpu_stages_with_context(&EditRecipe::default(), false, Some(&wb)),
        ),
        ("red_eye", unsupported_gpu_stages(&invalid_red_eye_recipe())),
        (
            // LRPAR-G14-DENOISE-IMPL-20: the additive KI-Denoise stage is not
            // GPU-wired; an active stage must route to the CPU loudly.
            "denoise_ai (not GPU-wired)",
            unsupported_gpu_stages(&denoise_ai_recipe(0.5)),
        ),
        (
            "adjustment `clarity_v2` not implemented on GPU",
            unsupported_gpu_stages(&unknown_key),
        ),
        // GPU-MAXRECT-WELLE / CROP-MAXRECT-1: a lens/perspective correction
        // without an explicit crop activates the content-based default crop.
        (
            "geometry (default content crop)",
            unsupported_gpu_stages(&lens),
        ),
        (
            "geometry (default content crop)",
            unsupported_gpu_stages(&perspective),
        ),
    ];

    for (expected, reasons) in &cases {
        assert!(
            reasons.iter().any(|r| r.contains(expected)),
            "inventory entry `{expected}` is no longer emitted by the gate: {reasons:?}"
        );
    }

    // GEN-ONNX-1 Welle 2a: `generative_edit` is no longer a routing reason. The
    // artifact-aware GPU entry injects the composited canvas; a recipe-only
    // render fails loudly instead of a blanket CPU route.
    assert!(
        unsupported_gpu_stages(&generative_expand_recipe()).is_empty(),
        "generative_edit must be GPU-eligible now (artifact compositing)"
    );

    // GPU-RENDER-PARITY-1 geometry wave / GPU-MAXRECT-WELLE: geometry
    // (crop/rotation/mirror) without a correction is rendered by the GPU
    // `geometry` passes and must stay GPU-eligible; a correction **with** an
    // explicit crop is authoritative and likewise stays eligible. A correction
    // **without** an explicit crop is the flagged default-crop class asserted in
    // the inventory cases above. The pixel/dimension parity is asserted by
    // `implemented_stages_match_cpu_oracle` (explicit crop) and
    // `default_content_crop_routes_to_cpu_with_parity` (loud CPU route).
    let geometry_reasons = unsupported_gpu_stages(&geometry);
    assert!(
        geometry_reasons.is_empty(),
        "geometry without a correction must be GPU-eligible, got {geometry_reasons:?}"
    );
    let mut lens_cropped = lens.clone();
    lens_cropped.geometry = Some(Geometry {
        version: 1,
        crop: Some(Crop::Free {
            x: 0.1,
            y: 0.1,
            width: 0.8,
            height: 0.8,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    });
    assert!(
        unsupported_gpu_stages(&lens_cropped).is_empty(),
        "a lens correction with an explicit crop must stay GPU-eligible"
    );

    // GPU-RENDER-PARITY-1 lens-blur wave: G-05 is GPU-rendered (heuristic and
    // external depth — the latter requires a bound plane, which the recipe-only
    // gate cannot see), so an active recipe must no longer be flagged.
    let lens_blur_reasons = unsupported_gpu_stages(&lens_blur);
    assert!(
        lens_blur_reasons.is_empty(),
        "G-05 lens blur must be GPU-eligible, got {lens_blur_reasons:?}"
    );
    assert!(
        unsupported_gpu_stages(&external_depth_lens_blur_recipe()).is_empty(),
        "an external-depth lens-blur recipe must be GPU-eligible (caller binds the plane)"
    );

    // GPU-RENDER-PARITY-1 follow-up: every class that used to be flagged here is
    // now GPU-eligible for valid recipes — legacy spot geometry, a typed
    // geometry-free mirror shadow, and any number of bound source actions.
    assert!(
        unsupported_gpu_stages(&legacy_spot_recipe()).is_empty(),
        "legacy spot geometry is GPU-rendered"
    );
    let mut shadow = legacy_spot_recipe();
    shadow.spot_removals = typed_spot_recipe().spot_removals;
    assert!(
        unsupported_gpu_stages(&shadow).is_empty(),
        "a typed shadow with extras geometry is GPU-eligible"
    );
    assert!(
        unsupported_gpu_stages_for(&source_actions(MAX_SOURCE_ACTIONS + 3), true).is_empty(),
        "source actions above the per-pass slot count are batched on the GPU"
    );

    // Neutral/disabled configurations must stay GPU-eligible (R2-GPU-05-style
    // neutrality preserved) — these are the examples the inventory excludes.
    let disabled_lens_blur = EditRecipe {
        lens_blur: Some(LensBlur {
            version: 1,
            enabled: false,
            focus_rect: FocusRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            focal_near: 0.0,
            focal_far: 1.0,
            blur_amount: 0.0,
            bokeh: BokehShape::Round,
            depth_artifact: None,
        }),
        ..Default::default()
    };
    assert!(unsupported_gpu_stages(&disabled_lens_blur).is_empty());
    assert!(unsupported_gpu_stages(&EditRecipe::default()).is_empty());
    assert!(unsupported_gpu_stages_for(&source_actions(1), true).is_empty());
    assert!(unsupported_gpu_stages_with_context(&EditRecipe::default(), false, None).is_empty());
    // LRPAR-G14-DENOISE-IMPL-20: an identity KI-Denoise stage (`enabled:false`
    // or `strength:0`) is pixel-neutral on both backends and must stay
    // GPU-eligible, exactly like a neutral adjustment key.
    let mut disabled_denoise = denoise_ai_recipe(0.5);
    disabled_denoise.denoise_ai.as_mut().unwrap().enabled = false;
    assert!(
        unsupported_gpu_stages(&disabled_denoise).is_empty(),
        "a disabled denoise_ai is pixel-identity and must be GPU-eligible"
    );
    assert!(
        unsupported_gpu_stages(&denoise_ai_recipe(0.0)).is_empty(),
        "strength:0 denoise_ai is pixel-identity and must be GPU-eligible"
    );
    // CAMERA-WB-WELLE: a *valid* As-Shot context is GPU-carried, so it must not
    // be flagged (its parity is asserted by `as_shot_wb_gains_match_cpu_oracle_across_recipe_wb`).
    assert!(unsupported_gpu_stages_with_context(
        &EditRecipe::default(),
        false,
        Some(&[1.9, 1.0, 1.4, 1.0])
    )
    .is_empty());
}

// ---------------------------------------------------------------------------
// GPU-RENDER-PARITY-1 follow-up: source-action batching (item 7) and the
// legacy spot-heal pass (item 5).
// ---------------------------------------------------------------------------

/// Build `count` full-frame source-action artifacts. Each artifact replaces a
/// distinct overlapping rectangle (region coverage `u16::MAX`) with a solid
/// colour; the overlap makes the batch order observable.
fn source_action_artifacts(width: u32, height: u32, count: usize) -> Vec<SourceActionArtifact> {
    (0..count)
        .map(|index| {
            let mut values = vec![0u16; (width * height) as usize];
            let x0 = (index as u32 * 2) % width.max(1);
            let y0 = (index as u32 * 3) % height.max(1);
            for y in y0..(y0 + height / 2).min(height) {
                for x in x0..(x0 + width / 2).min(width) {
                    values[(y * width + x) as usize] = u16::MAX;
                }
            }
            let replacement = [
                (index as u8).wrapping_mul(37),
                (index as u8).wrapping_mul(53),
                (index as u8).wrapping_mul(97),
                255u8,
            ]
            .repeat((width * height) as usize);
            SourceActionArtifact {
                region: MaskPlane {
                    width,
                    height,
                    values,
                },
                replacement: ImageFrame::new(width, height, replacement).expect("replacement"),
            }
        })
        .collect()
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// Item 7: more artifacts than the per-pass slot count must composite on the
/// GPU in batches and match the CPU oracle byte-for-byte (the previous gate
/// CPU-routed these).
fn source_action_batch_matches_oracle() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped source-action batch check");
            return;
        }
    };
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    const W: u32 = 32;
    const H: u32 = 32;
    let frame = gradient_frame(W, H);
    let count = MAX_SOURCE_ACTIONS + 3;
    let artifacts = source_action_artifacts(W, H, count);
    let recipe = source_actions(count);
    assert!(
        unsupported_gpu_stages_for(&recipe, true).is_empty(),
        "batched source actions must be GPU-eligible"
    );

    let mut ctx = ctx;
    ctx.set_source_action_artifacts(&artifacts)
        .expect("bind artifacts");

    let cpu = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &artifacts,
            masks: None,
            lensfun: None,
            depth: None,
        },
    )
    .expect("CPU oracle render")
    .frame;
    let gpu = ctx
        .render_with_gpu(&frame, &recipe)
        .expect("GPU render with batched source actions");
    let diff = max_abs_diff(cpu.pixels(), &gpu.pixels);
    eprintln!("source_action_batch[{count}]: maxAbsDiff={diff}");
    assert_eq!(
        diff, 0,
        "batched source-action compositing must match the CPU oracle"
    );
}
