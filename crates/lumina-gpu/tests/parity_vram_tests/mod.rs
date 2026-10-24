use super::*;

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// The interactive VRAM path (`render_to_vram`, which the GUI uses for the
/// readback-free present) must apply the same post-tone chain as
/// `render_with_gpu`. Read the VRAM output back through the diagnostic seam and
/// gate it on the same tolerance.
fn vram_path_applies_post_stages() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped VRAM parity check");
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
    let recipes: [(&str, EditRecipe, Equivalence); 4] = [
        (
            "curves",
            EditRecipe {
                curves: Some(Curves {
                    version: 1,
                    master: curve(&[(0.0, 0.0), (0.5, 0.4), (1.0, 1.0)]),
                    channels: CurveChannels::default(),
                }),
                ..Default::default()
            },
            // Measured 1 on the VRAM gradient frame (same rounding tie as the
            // streaming path).
            Equivalence::Bounded(1),
        ),
        (
            "presence_full",
            EditRecipe {
                presence: Some(Presence {
                    version: 1,
                    texture: 0.5,
                    clarity: 0.3,
                    dehaze: 0.5,
                }),
                ..Default::default()
            },
            Equivalence::Bounded(1),
        ),
        (
            "grading",
            EditRecipe {
                color_grading: Some(ColorGrading {
                    version: 1,
                    shadows: range(200.0, 0.3, 0.0),
                    midtones: ColorGradingRange::neutral(),
                    highlights: range(60.0, 0.2, 0.05),
                    balance: 0.1,
                    blending: 0.5,
                }),
                ..Default::default()
            },
            // Color Grading measured byte-identical.
            Equivalence::ByteIdentical,
        ),
        (
            // GPU-RENDER-PARITY-1 stage 2: the detail chain (Noise Reduction →
            // Sharpening → vignette → grain) runs in the readback-free VRAM path
            // through the same `render_post_stages` seam.
            "detail_stage_stack",
            EditRecipe {
                noise_reduction: Some(NoiseReduction {
                    version: 1,
                    luminance: 0.4,
                    color: 0.3,
                }),
                sharpening: Some(Sharpening {
                    version: 1,
                    amount: 1.0,
                    radius: 1.2,
                    detail: 0.6,
                    masking: 0.4,
                }),
                effects: Some(Effects {
                    vignette: Some(Vignette {
                        version: 1,
                        amount: 0.3,
                        midpoint: 0.5,
                        roundness: 1.0,
                        feather: 0.5,
                    }),
                    grain: Some(Grain {
                        version: 1,
                        amount: 0.25,
                        size: 0.4,
                        roughness: 0.5,
                        seed: 42,
                    }),
                }),
                ..Default::default()
            },
            Equivalence::Bounded(2),
        ),
    ];
    ctx.ensure_vram(W, H).expect("vram state");
    for (name, recipe, equivalence) in recipes {
        ctx.render_to_vram(&frame, &recipe).expect("vram render");
        let gpu = ctx.readback_output_frame().expect("vram readback");
        let cpu = render_frame(
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
        .expect("CPU oracle render")
        .frame;
        let diff = max_abs_diff(cpu.pixels(), &gpu.pixels);
        let psnr = psnr_db(cpu.pixels(), &gpu.pixels);
        let bias = mean_signed_error(cpu.pixels(), &gpu.pixels);
        eprintln!("vram[{name}]: maxAbsDiff={diff} psnr={psnr:.2} dB meanSignedErr={bias:+.4}");
        match equivalence {
            Equivalence::ByteIdentical => assert_eq!(
                diff, 0,
                "vram[{name}] asserted byte-identical (got {diff}, psnr={psnr:.2})"
            ),
            Equivalence::Bounded(bound) => assert!(
                diff <= bound && psnr >= MIN_PSNR_DB && bias.abs() <= MAX_ABS_MEAN_SIGNED_ERROR,
                "vram[{name}] exceeded declared bound maxAbsDiff <= {bound}: \
                 maxAbsDiff={diff} psnr={psnr:.2} meanSignedErr={bias:+.4}"
            ),
        }
    }
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// GPU-RENDER-PARITY-1 stage 3: the readback-free VRAM path must also run the
/// red-eye pass (it shares `render_post_stages` with the streaming path).
fn vram_path_applies_red_eye() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped VRAM red-eye check");
            return;
        }
    };
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    const W: u32 = 32;
    const H: u32 = 32;
    let frame = red_frame(W, H);
    let recipe = red_eye_recipe();
    assert!(unsupported_gpu_stages(&recipe).is_empty());
    ctx.ensure_vram(W, H).expect("vram state");
    ctx.render_to_vram(&frame, &recipe).expect("vram render");
    let gpu = ctx.readback_output_frame().expect("vram readback");
    let cpu = render_frame(
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
    .expect("CPU oracle render")
    .frame;
    let diff = max_abs_diff(cpu.pixels(), &gpu.pixels);
    eprintln!("vram[red_eye]: maxAbsDiff={diff}");
    assert_eq!(diff, 0, "VRAM red-eye pass must match the CPU oracle");
}

/// GPU-RENDER-PARITY-1 follow-up (gate completeness) + GEN-ONNX-1 Welle 2a: an
/// **invalid** red-eye correction is the remaining recipe stage that is
/// validated by the CPU reference but not implemented on the GPU, so it must be
/// reported as CPU-only. A valid red-eye correction, the legacy
/// `extras["spot_removals"]` heal geometry and `generative_edit` (mid-geometry
/// artifact compositing) are GPU-rendered and therefore must NOT be flagged; a
/// typed geometry-free mirror shadow is tolerated exactly like the CPU oracle
/// when the extras geometry is present, and an isolated typed entry is a
/// **hard error on both backends** (asserted by
/// `typed_spot_without_extras_is_a_hard_error_on_both_backends`) instead of a
/// routing reason.
#[test]
fn cpu_only_recipe_stages_are_gated() {
    let invalid_red_eye = invalid_red_eye_recipe();

    let cases: Vec<(&str, &EditRecipe)> = vec![("red_eye", &invalid_red_eye)];

    for (expected_reason, recipe) in &cases {
        let reasons = unsupported_gpu_stages(recipe);
        assert!(
            reasons.iter().any(|r| r.contains(expected_reason)),
            "`{expected_reason}` must be reported as CPU-only, got {reasons:?}"
        );
    }

    // GPU-eligible now: a valid red-eye correction, legacy spot geometry, a
    // geometry-free typed mirror shadow paired with that extras geometry, and
    // the generative edit (artifact-aware GPU entry).
    assert!(
        unsupported_gpu_stages(&red_eye_recipe()).is_empty(),
        "a valid red-eye correction must be GPU-eligible"
    );
    assert!(
        unsupported_gpu_stages(&legacy_spot_recipe()).is_empty(),
        "legacy spot geometry must be GPU-eligible"
    );
    let mut shadow = legacy_spot_recipe();
    shadow.spot_removals = typed_spot_recipe().spot_removals;
    assert!(
        unsupported_gpu_stages(&shadow).is_empty(),
        "a geometry-free typed shadow with extras geometry must be GPU-eligible"
    );
    assert!(
        unsupported_gpu_stages(&generative_expand_recipe()).is_empty(),
        "generative_edit must be GPU-eligible via artifact compositing (Welle 2a)"
    );

    // The compound recipe (invalid red-eye + generative edit) reports only the
    // genuinely unsupported invalid red-eye now.
    let all = EditRecipe {
        red_eye: invalid_red_eye.red_eye.clone(),
        generative_edit: generative_expand_recipe().generative_edit.clone(),
        ..Default::default()
    };
    let reasons = unsupported_gpu_stages(&all);
    assert!(
        reasons.iter().any(|r| r.contains("red_eye")),
        "compound recipe must keep `red_eye`: {reasons:?}"
    );
    assert!(
        !reasons.iter().any(|r| r.contains("generative_edit")),
        "compound recipe must not flag the GPU-capable generative stage: {reasons:?}"
    );
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// GPU-RENDER-PARITY-1 follow-up: a typed `spot_removals` entry without the
/// extras heal geometry is unrenderable on both backends. The GPU entry must
/// reject it loudly (not silently drop the spot, and not route to the CPU to
/// hide the divergence), and it must not appear as a GPU stage gap.
fn typed_spot_without_extras_is_a_hard_error_on_both_backends() {
    let recipe = typed_spot_recipe();
    let frame = gradient_frame(16, 16);
    // Not a routing reason: the GPU validates and errors itself.
    assert!(
        unsupported_gpu_stages(&recipe).is_empty(),
        "an isolated typed spot is a validation error, not a stage gap"
    );
    // The CPU reference rejects it loudly.
    assert!(
        render_frame(
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
        .is_err(),
        "isolated typed spot must be a hard CPU error"
    );

    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped typed-spot error check");
            return;
        }
    };
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let err = ctx
        .render_with_gpu(&frame, &recipe)
        .expect_err("GPU must reject an isolated typed spot");
    assert!(
        format!("{err}").contains("spot_heal"),
        "unexpected error: {err}"
    );
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// Blocker-1 resolution: `render_with_gpu`'s fallback is the **full**
/// `lumina_core::render_frame` chain. The (role-inactive) generative recipe is
/// CPU-routed as a fallback example and its pixels must be byte-identical to
/// `render_frame`; an *active* generative edit has no artifact on the CPU
/// fallback and must be rejected loudly by both backends (GEN-ONNX-1 Welle 2a —
/// no blanket route, no silent unexpanded render). The legacy spot
/// geometry is GPU-rendered and must match the oracle byte-for-byte through
/// the GPU spot pass. For a recipe the reference rejects (isolated typed spot),
/// the GPU entry must reject it too — never silently drop it.
fn cpu_only_recipe_stages_render_through_full_cpu_reference() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped full-reference check");
            return;
        }
    };
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }

    let frame = gradient_frame(48, 48);

    // Renderable recipes: legacy spots go through the GPU spot pass, the
    // (role-inactive) generative recipe through the full CPU fallback — both
    // must equal the full `render_frame` oracle (dimensions and bytes).
    let renderable: Vec<(&str, &ImageFrame, EditRecipe)> = vec![
        ("legacy_spot", &frame, legacy_spot_recipe()),
        ("generative_identity", &frame, generative_identity_recipe()),
    ];
    for (name, source, recipe) in renderable {
        let oracle = render_frame(
            source,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: None,
                depth: None,
            },
        )
        .unwrap_or_else(|e| panic!("{name}: CPU reference failed: {e}"))
        .frame;
        let gpu = ctx
            .render_with_gpu(source, &recipe)
            .unwrap_or_else(|e| panic!("{name}: fallback render failed: {e}"));
        assert_eq!(
            (gpu.width, gpu.height),
            (oracle.width(), oracle.height()),
            "{name}: fallback must match the reference frame dimensions"
        );
        assert_eq!(
            max_abs_diff(oracle.pixels(), &gpu.pixels),
            0,
            "{name}: fallback must be byte-identical to the full CPU reference"
        );
    }

    // GEN-ONNX-1: an *active* expand without a composited canvas artifact is
    // refused loudly by both backends (no silent unexpanded render).
    let active = generative_expand_recipe();
    assert!(
        render_frame(
            &frame,
            &RenderContext {
                recipe: &active,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: None,
                depth: None,
            },
        )
        .is_err(),
        "an active expand without an artifact must be a hard CPU error"
    );
    assert!(
        ctx.render_with_gpu(&frame, &active).is_err(),
        "the GPU fallback must refuse an active expand without an artifact"
    );

    // A recipe the reference rejects must be rejected loudly, not dropped.
    let typed = typed_spot_recipe();
    assert!(
        render_frame(
            &frame,
            &RenderContext {
                recipe: &typed,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: None,
                depth: None,
            },
        )
        .is_err(),
        "isolated typed spot must be a hard CPU error"
    );
    assert!(
        ctx.render_with_gpu(&frame, &typed).is_err(),
        "fallback must propagate the CPU hard error instead of dropping the stage"
    );

    // An invalid red-eye correction stays CPU-routed so the oracle's loud
    // rejection is preserved (the GPU pass never silently renders it).
    let invalid_red_eye = invalid_red_eye_recipe();
    assert!(
        render_frame(
            &frame,
            &RenderContext {
                recipe: &invalid_red_eye,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: None,
                depth: None,
            },
        )
        .is_err(),
        "out-of-range red-eye must be a hard CPU error"
    );
    assert!(
        ctx.render_with_gpu(&frame, &invalid_red_eye).is_err(),
        "fallback must propagate the invalid red-eye rejection"
    );
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// `render_to_vram` cannot CPU-route without a readback, so it must refuse a
/// recipe it cannot render (no divergent pixels in the VRAM output). GEN-ONNX-1
/// Welle 2a: an active generative edit is no longer a routing *reason*, but the
/// artifact-blind VRAM path still refuses it loudly (no artifact injection).
fn vram_path_refuses_unsupported_recipes() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped VRAM refusal check");
            return;
        }
    };
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let frame = gradient_frame(16, 16);
    ctx.ensure_vram(16, 16).expect("vram state");
    let recipe = generative_expand_recipe();
    assert!(
        unsupported_gpu_stages(&recipe).is_empty(),
        "generative_edit must be GPU-eligible (no routing reason) in Welle 2a"
    );
    assert!(
        ctx.render_to_vram(&frame, &recipe).is_err(),
        "the artifact-blind VRAM path must refuse an active generative recipe instead of \
         writing divergent (unexpanded) pixels"
    );
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// GPU-RENDER-PARITY-1 geometry wave / GPU-MAXRECT-WELLE: the readback-free
/// VRAM present texture is source-sized, so a geometry chain that **changes the
/// output dimensions** (crop/rotation/perspective) must be refused loudly — the
/// caller then uses the exact CPU present path. The same applies to a
/// lens/perspective correction without an explicit crop, whose content-based
/// default crop is derived from the resampled alpha and cannot be planned
/// readback-free (gate reason `geometry (default content crop)`).
/// Dimension-preserving, default-crop-inactive geometry (mirroring) is rendered
/// into the resident output and must match the CPU oracle.
fn vram_geometry_dimension_change_is_refused_loudly() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped VRAM geometry check");
            return;
        }
    };
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    const W: u32 = 24;
    const H: u32 = 24;
    let frame = gradient_frame(W, H);
    ctx.ensure_vram(W, H).expect("vram state");

    // A crop is GPU-eligible as a recipe, but its output dimensions differ from
    // the source, so the VRAM present path must refuse it (no silent write).
    let crop = EditRecipe {
        geometry: Some(Geometry {
            version: 1,
            crop: Some(Crop::Free {
                x: 0.1,
                y: 0.1,
                width: 0.5,
                height: 0.5,
            }),
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        }),
        ..Default::default()
    };
    assert!(
        unsupported_gpu_stages(&crop).is_empty(),
        "a crop recipe is GPU-eligible on the full render path"
    );
    assert!(
        ctx.render_to_vram(&frame, &crop).is_err(),
        "VRAM must refuse dimension-changing geometry loudly"
    );

    // GPU-MAXRECT-WELLE: a lens correction without an explicit crop activates
    // the content-based default crop. The VRAM path cannot plan it readback-free,
    // so the gate flags the recipe and the present path refuses (the GUI then
    // falls back to the exact CPU present path) — no divergent pixels are ever
    // written into the source-sized resident output.
    let lens = manual_lens_recipe();
    assert!(
        unsupported_gpu_stages(&lens)
            .iter()
            .any(|reason| reason.contains("geometry (default content crop)")),
        "a lens correction without an explicit crop must be flagged"
    );
    assert!(
        ctx.render_to_vram(&frame, &lens).is_err(),
        "VRAM must refuse a recipe with an unpredictable default content crop"
    );

    // Dimension-preserving, default-crop-inactive geometry (mirror) renders into
    // the resident output and matches the oracle byte-for-byte.
    let mirror = EditRecipe {
        geometry: Some(Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: true,
            mirror_vertical: false,
        }),
        ..Default::default()
    };
    assert!(unsupported_gpu_stages(&mirror).is_empty());
    ctx.render_to_vram(&frame, &mirror)
        .expect("dimension-preserving geometry renders in VRAM");
    let gpu = ctx.readback_output_frame().expect("vram readback");
    let cpu = render_frame(
        &frame,
        &RenderContext {
            recipe: &mirror,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
    )
    .expect("CPU oracle render")
    .frame;
    assert_eq!(
        (cpu.width(), cpu.height()),
        (gpu.width, gpu.height),
        "VRAM geometry must keep the source dimensions"
    );
    let diff = max_abs_diff(cpu.pixels(), &gpu.pixels);
    eprintln!("vram_geometry[mirror]: maxAbsDiff={diff}");
    assert_eq!(
        diff, 0,
        "VRAM mirror pass must match the CPU oracle byte-for-byte, got {diff}"
    );

    // Dimension-preserving geometry stacked *after* post-tone stages exercises
    // the VRAM `needs_post && geometry` wiring (post lands in a transient,
    // geometry then writes the resident output).
    let mut mirror_post = mirror.clone();
    mirror_post.curves = Some(Curves {
        version: 1,
        master: curve(&[(0.0, 0.0), (0.5, 0.42), (1.0, 1.0)]),
        channels: CurveChannels::default(),
    });
    assert!(unsupported_gpu_stages(&mirror_post).is_empty());
    ctx.render_to_vram(&frame, &mirror_post)
        .expect("post + dimension-preserving geometry renders in VRAM");
    let gpu = ctx.readback_output_frame().expect("vram readback");
    let cpu = render_frame(
        &frame,
        &RenderContext {
            recipe: &mirror_post,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
    )
    .expect("CPU oracle render")
    .frame;
    assert_eq!((cpu.width(), cpu.height()), (gpu.width, gpu.height));
    let diff = max_abs_diff(cpu.pixels(), &gpu.pixels);
    eprintln!("vram_geometry[mirror+curves]: maxAbsDiff={diff}");
    assert!(
        diff <= 1,
        "VRAM post + mirror chain must match the CPU oracle within one code, got {diff}"
    );
}

/// An *empty* legacy `extras["spot_removals"]` array is an explicit no-op in
/// core (`reject_unsupported_spot_modes_extras` iterates it, `spots_from_recipe`
/// yields none); it must not flag the GPU route.
#[test]
fn empty_legacy_spot_removals_do_not_flag() {
    let mut recipe = EditRecipe::default();
    recipe
        .extras
        .insert("spot_removals".into(), serde_json::json!([]));
    let reasons = unsupported_gpu_stages(&recipe);
    assert!(
        reasons.is_empty(),
        "an empty legacy spot list is identity, got {reasons:?}"
    );
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// Item 5: legacy spot-heal geometry rendered by the GPU spot pass must match
/// `apply_spot_heals` byte-for-byte, including feathered and overlapping spots.
fn legacy_spot_heal_matches_cpu_oracle() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped spot-heal check");
            return;
        }
    };
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let mut recipe = EditRecipe::default();
    recipe.extras.insert(
        "spot_removals".into(),
        serde_json::json!([
            {
                "id": "spot-1",
                "version": 1,
                "mode": "heuristic",
                "center_x": 0.3,
                "center_y": 0.4,
                "radius": 6.0,
                "feather": 0.5,
                "offset_dx": 0.3,
                "offset_dy": 0.1,
                "opacity": 1.0,
                "status": "valid"
            },
            {
                "id": "spot-2",
                "version": 1,
                "mode": "heuristic",
                "center_x": 0.35,
                "center_y": 0.45,
                "radius": 9.0,
                "feather": 0.0,
                "offset_dx": -0.2,
                "offset_dy": 0.25,
                "opacity": 0.6,
                "status": "valid"
            }
        ]),
    );
    assert!(unsupported_gpu_stages(&recipe).is_empty());

    let frames: Vec<(&str, ImageFrame)> = vec![
        ("gradient_48x48", gradient_frame(48, 48)),
        ("noise_48x48", noise_frame(48, 48, 0x51C0_FFEE)),
    ];
    let mut failures = Vec::new();
    for (name, frame) in &frames {
        let cpu = render_frame(
            frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: None,
                depth: None,
            },
        )
        .expect("CPU oracle render")
        .frame;
        let gpu = ctx
            .render_with_gpu(frame, &recipe)
            .unwrap_or_else(|error| panic!("{name}: GPU spot render: {error}"));
        let diff = max_abs_diff(cpu.pixels(), &gpu.pixels);
        eprintln!("spot_heal[{name}]: maxAbsDiff={diff}");
        if diff != 0 {
            failures.push(format!("{name}: maxAbsDiff={diff}"));
        }
    }

    // The readback-free VRAM path shares the spot-heal pass; assert it there too.
    let frame = gradient_frame(48, 48);
    ctx.ensure_vram(48, 48).expect("vram state");
    ctx.render_to_vram(&frame, &recipe).expect("vram render");
    let gpu = ctx.readback_output_frame().expect("vram readback");
    let cpu = render_frame(
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
    .expect("CPU oracle render")
    .frame;
    let diff = max_abs_diff(cpu.pixels(), &gpu.pixels);
    eprintln!("spot_heal[vram_48x48]: maxAbsDiff={diff}");
    assert_eq!(diff, 0, "VRAM spot-heal pass must match the CPU oracle");

    assert!(
        failures.is_empty(),
        "spot-heal GPU pass diverged from the CPU oracle: {failures:?}"
    );
}
