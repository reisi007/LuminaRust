use super::*;

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
fn tone_only_is_byte_identical() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped");
            return;
        }
    };
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let frame = gradient_frame(64, 64);
    for (name, recipe) in [
        ("default", EditRecipe::default()),
        (
            "exposure",
            EditRecipe {
                adjustments: BTreeMap::from([("exposure".into(), 0.5)]),
                ..Default::default()
            },
        ),
    ] {
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
        let gpu = ctx.render_with_gpu(&frame, &recipe).expect("GPU render");
        let diff = max_abs_diff(cpu.pixels(), &gpu.pixels);
        eprintln!("tone[{name}]: maxAbsDiff={diff}");
        assert_eq!(
            diff, 0,
            "tone-only render must stay byte-identical ({name})"
        );
    }
}

#[test]
fn implemented_stages_are_not_flagged() {
    for (name, recipe) in supported_recipes() {
        let reasons = unsupported_gpu_stages(&recipe);
        assert!(
            reasons.is_empty(),
            "{name} must be fully GPU-supported, got {reasons:?}"
        );
    }
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
fn implemented_stages_match_cpu_oracle() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped parity check");
            return;
        }
    };
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }

    let frames: Vec<(&'static str, ImageFrame)> = vec![
        ("gradient_64x64", gradient_frame(64, 64)),
        ("noise_64x64", noise_frame(64, 64, 0x0BAD_F00D_1234_5678)),
    ];

    let mut failures: Vec<String> = Vec::new();
    for (frame_name, frame) in &frames {
        for (recipe_name, recipe) in supported_recipes() {
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
                .unwrap_or_else(|error| panic!("{frame_name}/{recipe_name}: GPU render: {error}"));
            // GPU-RENDER-PARITY-1 geometry wave: a dimension-changing geometry
            // chain must reproduce the oracle's canvas exactly — a silently
            // different output size must fail here, not be masked by a pixel
            // compare.
            assert_eq!(
                (cpu.width(), cpu.height()),
                (gpu.width, gpu.height),
                "{frame_name}/{recipe_name}: GPU output dimensions must equal the CPU oracle"
            );
            let diff = max_abs_diff(cpu.pixels(), &gpu.pixels);
            let psnr = psnr_db(cpu.pixels(), &gpu.pixels);
            let bias = mean_signed_error(cpu.pixels(), &gpu.pixels);
            eprintln!(
                "parity[{frame_name}/{recipe_name}] {}x{}: maxAbsDiff={diff} psnr={psnr:.2} dB \
                 meanSignedErr={bias:+.4}",
                cpu.width(),
                cpu.height()
            );
            match equivalence_for(recipe_name) {
                Equivalence::ByteIdentical => {
                    if diff != 0 {
                        failures.push(format!(
                            "{frame_name}/{recipe_name}: asserted byte-identical, got \
                             maxAbsDiff={diff} psnr={psnr:.2} meanSignedErr={bias:+.4}"
                        ));
                    }
                }
                Equivalence::Bounded(bound) => {
                    if diff > bound || psnr < MIN_PSNR_DB || bias.abs() > MAX_ABS_MEAN_SIGNED_ERROR
                    {
                        failures.push(format!(
                            "{frame_name}/{recipe_name}: exceeded declared bound \
                             maxAbsDiff <= {bound} / PSNR >= {MIN_PSNR_DB} dB / \
                             |meanSignedErr| <= {MAX_ABS_MEAN_SIGNED_ERROR}: got \
                             maxAbsDiff={diff} psnr={psnr:.2} meanSignedErr={bias:+.4}"
                        ));
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "post-tone GPU stages exceeded their per-stage declared equivalence: {failures:?}"
    );
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// GPU-MAXRECT-WELLE (CROP-MAXRECT-1): a lens/perspective correction without an
/// explicit crop activates the CPU oracle's content-based default crop. The
/// recipe-only GPU plan cannot predict the (resampled-alpha-dependent)
/// rectangle, so the gate flags the recipe (`geometry (default content crop)`)
/// and `render_with_gpu` routes it to the CPU — the output dimensions and pixels
/// must be identical to the oracle. `lens_wedge` and the two `perspective_*`
/// recipes prove the default crop really fired (the oracle frame is smaller than
/// the uncropped geometry dimensions); the non-wedge lens recipes document the
/// conservative route (identity crop, still CPU-routed rather than risk a
/// mispredicted rectangle).
fn default_content_crop_routes_to_cpu_with_parity() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped default-crop check");
            return;
        }
    };
    let frames: [(&str, ImageFrame); 2] = [
        ("gradient_64x64", gradient_frame(64, 64)),
        ("noise_64x64", noise_frame(64, 64, 0x0BAD_F00D_1234_5678)),
    ];
    // Recipes whose default crop is genuinely non-identity on these frames.
    let shrinks = ["lens_wedge", "perspective_vertical", "perspective_full"];

    for (name, recipe) in default_content_crop_recipes() {
        let reasons = unsupported_gpu_stages(&recipe);
        assert!(
            reasons
                .iter()
                .any(|reason| reason.contains("geometry (default content crop)")),
            "{name} must be flagged with the default-crop reason, got {reasons:?}"
        );
        for (frame_name, frame) in &frames {
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
            // The uncropped geometry dimensions (these recipes carry no explicit
            // crop and no rotation): when the default crop fires, the oracle
            // frame is strictly smaller than this.
            let uncropped = frame
                .measurement_domain_with_perspective(
                    recipe.geometry.as_ref(),
                    recipe.lens_correction.as_ref(),
                    recipe.perspective.as_ref(),
                )
                .expect("uncropped measurement domain");
            let cpu_area = cpu.width() * cpu.height();
            let uncropped_area = uncropped.output_width * uncropped.output_height;
            if shrinks.contains(&name) {
                assert!(
                    cpu_area < uncropped_area,
                    "{frame_name}/{name}: the default content crop must shrink the \
                     oracle frame ({cpu_area} !< {uncropped_area})"
                );
            }
            if !support::require_adapter(&ctx) {
                continue;
            }
            let gpu = ctx
                .render_with_gpu(frame, &recipe)
                .unwrap_or_else(|error| panic!("{frame_name}/{name}: GPU render: {error}"));
            assert_eq!(
                (cpu.width(), cpu.height()),
                (gpu.width, gpu.height),
                "{frame_name}/{name}: the CPU-routed default crop must match the oracle",
            );
            let diff = max_abs_diff(cpu.pixels(), &gpu.pixels);
            eprintln!(
                "default_crop[{frame_name}/{name}]: {}x{} (uncropped {}x{}) maxAbsDiff={diff}",
                cpu.width(),
                cpu.height(),
                uncropped.output_width,
                uncropped.output_height
            );
            assert_eq!(
                diff, 0,
                "{frame_name}/{name}: a default-content-crop render must be byte-identical"
            );
        }
    }
}

/// LRPAR-G14-DENOISE-IMPL-20: an active `denoise_ai` stage must never be
/// silently dropped by the GPU path. The routing gate flags it, so
/// `render_with_gpu` routes to the CPU oracle, which rejects the active stage
/// loudly under the default `Strict` policy (`CoreError::Denoise`). The GPU
/// path must propagate that abort instead of returning pixel-equal-to-source
/// output (no silent fallback, no GPU parity break).
#[test]
fn denoise_ai_routes_to_cpu_and_aborts_loudly() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped denoise routing check");
            return;
        }
    };
    let frame = gradient_frame(32, 32);
    let recipe = denoise_ai_recipe(0.5);
    let reasons = unsupported_gpu_stages(&recipe);
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("denoise_ai (not GPU-wired)")),
        "an active denoise_ai must be flagged for CPU routing, got {reasons:?}"
    );
    // The CPU oracle rejects the active stage loudly (default Strict policy).
    let cpu_error = render_frame(
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
    .expect_err("the CPU oracle must reject an unresolved active denoise_ai");
    assert!(
        matches!(cpu_error, CoreError::Denoise { .. }),
        "unexpected CPU error: {cpu_error}"
    );
    // The GPU entry routes to that same CPU path and propagates the abort; it
    // must not return pixels that silently omit the stage.
    match ctx.render_with_gpu(&frame, &recipe) {
        Err(GpuError::Core(CoreError::Denoise { .. })) => {}
        Err(other) => panic!("unexpected GPU error (expected the routed CPU abort): {other}"),
        Ok(_) => panic!("an active denoise_ai must not render on the GPU path silently"),
    }
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// GPU-RENDER-PARITY-1 stage 3 (G-14): the red-eye pass must be pixel-effective
/// and match the CPU oracle on a frame whose redness actually triggers the
/// correction (the gradient/noise frames barely exercise it).
fn implemented_red_eye_matches_cpu_oracle() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped red-eye parity check");
            return;
        }
    };
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }

    let recipes: [(&str, EditRecipe); 2] = [
        ("single_region", red_eye_recipe()),
        (
            "two_regions_overlap",
            EditRecipe {
                red_eye: Some(RedEyeCorrection {
                    version: 1,
                    regions: vec![
                        RedEyeRegion {
                            id: "re-1".into(),
                            x: 0.4,
                            y: 0.5,
                            radius: 0.4,
                            desaturate: 0.9,
                            darken: 0.5,
                        },
                        RedEyeRegion {
                            id: "re-2".into(),
                            x: 0.6,
                            y: 0.5,
                            radius: 0.4,
                            desaturate: 0.3,
                            darken: 0.8,
                        },
                    ],
                }),
                ..Default::default()
            },
        ),
    ];
    let frames: [(&str, ImageFrame); 2] = [
        ("red_solid_64x64", red_frame(64, 64)),
        ("red_gradient_48x64", {
            let mut pixels = Vec::with_capacity(48 * 64 * 4);
            for y in 0..64u32 {
                for x in 0..48u32 {
                    let r = (120 + x * 120 / 47) as u8;
                    let g = (y * 80 / 63) as u8;
                    let b = 30;
                    pixels.extend_from_slice(&[r, g, b, 255]);
                }
            }
            ImageFrame::new(48, 64, pixels).expect("red gradient")
        }),
    ];

    let mut failures = Vec::new();
    for (name, recipe) in &recipes {
        assert!(
            unsupported_gpu_stages(recipe).is_empty(),
            "red-eye `{name}` must be GPU-eligible"
        );
        for (frame_name, frame) in &frames {
            let cpu = render_frame(
                frame,
                &RenderContext {
                    recipe,
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
                .render_with_gpu(frame, recipe)
                .unwrap_or_else(|error| panic!("{frame_name}/{name}: GPU render: {error}"));
            let diff = max_abs_diff(cpu.pixels(), &gpu.pixels);
            let psnr = psnr_db(cpu.pixels(), &gpu.pixels);
            let bias = mean_signed_error(cpu.pixels(), &gpu.pixels);
            eprintln!(
                "red_eye[{frame_name}/{name}]: maxAbsDiff={diff} psnr={psnr:.2} bias={bias:+.4}"
            );
            // Measured: byte-identical on the solid/gradient single-region
            // frames and one ±1 code on the overlapping-region gradient
            // (`hypot` vs. `sqrt` rounding at the disc edge). Asserted at the
            // documented stage bound.
            if diff > 1 || psnr < MIN_PSNR_DB || bias.abs() > MAX_ABS_MEAN_SIGNED_ERROR {
                failures.push(format!(
                    "{frame_name}/{name}: red-eye exceeded the declared bound \
                     maxAbsDiff <= 1 / PSNR >= {MIN_PSNR_DB} dB / \
                     |meanSignedErr| <= {MAX_ABS_MEAN_SIGNED_ERROR}: got \
                     maxAbsDiff={diff} psnr={psnr:.2} bias={bias:+.4}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "red-eye parity: {failures:?}");
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// CAMERA-WB-WELLE (R2-MCP-01): a valid decoder As-Shot WB context is now an
/// explicit GPU input the caller binds via `GpuContext::set_camera_white_balance`;
/// the gains are validated with the oracle's own error but never re-applied
/// (matching `lumina-core`), so the GPU render stays byte-identical to the CPU
/// oracle for a tone-only recipe. Invalid gains are rejected at the bind and
/// still flagged by the routing gate so unbound callers reach the oracle's
/// loud rejection.
fn as_shot_wb_context_is_carried_and_validated() {
    let recipe = EditRecipe {
        adjustments: BTreeMap::from([("exposure".into(), 0.3)]),
        ..Default::default()
    };
    let wb = [1.9f32, 1.0, 1.4, 1.0];

    // A valid context is no longer a routing reason …
    assert!(
        unsupported_gpu_stages_with_context(&recipe, false, Some(&wb)).is_empty(),
        "a valid As-Shot context must be GPU-eligible"
    );
    // … an invalid one still is.
    let invalid_reasons =
        unsupported_gpu_stages_with_context(&recipe, false, Some(&[0.0, 1.0, 1.0, 1.0]));
    assert!(
        invalid_reasons
            .iter()
            .any(|r| r.contains("camera_white_balance")),
        "{invalid_reasons:?}"
    );

    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped WB parity check");
            return;
        }
    };
    // The bind validates exactly like the oracle and never stores bad gains.
    assert!(
        ctx.set_camera_white_balance(Some([0.0, 1.0, 1.0, 1.0]))
            .is_err(),
        "invalid As-Shot gains must be rejected at the GPU entry"
    );
    assert_eq!(ctx.camera_white_balance(), None);
    ctx.set_camera_white_balance(Some(wb))
        .expect("valid As-Shot gains bind");

    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let frame = gradient_frame(48, 48);
    let cpu = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: Some(wb),
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
    )
    .expect("CPU oracle render")
    .frame;
    let gpu = ctx.render_with_gpu(&frame, &recipe).expect("GPU render");
    let diff = max_abs_diff(cpu.pixels(), &gpu.pixels);
    eprintln!("wb_context: maxAbsDiff={diff}");
    assert_eq!(
        diff, 0,
        "a carried (validated, not re-applied) As-Shot context must match the CPU reference"
    );
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// CAMERA-WB-WELLE oracle parity: decoder As-Shot gains (identity and
/// non-identity) crossed with every recipe white-balance combination. The GPU
/// entry carries the gains, so each render must match the CPU oracle within the
/// standard tone bound, and binding the gains must not change the GPU pixels at
/// all (they are validation state — `lumina-core` never re-applies them, and
/// neither may the shader).
fn as_shot_wb_gains_match_cpu_oracle_across_recipe_wb() {
    let recipes: Vec<(&str, EditRecipe)> = vec![
        ("default", EditRecipe::default()),
        (
            "exposure",
            EditRecipe {
                adjustments: BTreeMap::from([("exposure".into(), 0.4)]),
                ..Default::default()
            },
        ),
        (
            "wb_temperature",
            EditRecipe {
                adjustments: BTreeMap::from([("wb_temperature".into(), 5600.0)]),
                ..Default::default()
            },
        ),
        (
            "wb_tint",
            EditRecipe {
                adjustments: BTreeMap::from([("wb_tint".into(), -0.3)]),
                ..Default::default()
            },
        ),
        (
            "wb_both",
            EditRecipe {
                adjustments: BTreeMap::from([
                    ("wb_temperature".into(), 7200.0),
                    ("wb_tint".into(), 0.2),
                ]),
                ..Default::default()
            },
        ),
        (
            "wb_exposure_contrast",
            EditRecipe {
                adjustments: BTreeMap::from([
                    ("wb_temperature".into(), 5800.0),
                    ("wb_tint".into(), 0.05),
                    ("exposure".into(), -0.3),
                    ("contrast".into(), 0.2),
                ]),
                ..Default::default()
            },
        ),
    ];
    let gains_cases: [(&str, [f32; 4]); 3] = [
        ("identity", [1.0, 1.0, 1.0, 1.0]),
        ("as_shot", [1.9, 1.0, 1.4, 1.0]),
        ("daylight", [0.8, 1.0, 1.25, 1.0]),
    ];

    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped WB parity check");
            return;
        }
    };
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }

    let frame = gradient_frame(64, 64);
    let mut failures: Vec<String> = Vec::new();
    for (recipe_name, recipe) in &recipes {
        assert!(
            unsupported_gpu_stages(recipe).is_empty(),
            "{recipe_name} must stay GPU-eligible"
        );
        for (gains_name, gains) in gains_cases {
            ctx.set_camera_white_balance(Some(gains))
                .expect("identity/non-identity As-Shot gains are valid");
            let cpu = render_frame(
                &frame,
                &RenderContext {
                    recipe,
                    camera_white_balance: Some(gains),
                    source_actions: &[],
                    masks: None,
                    lensfun: None,
                    depth: None,
                },
            )
            .expect("CPU oracle render")
            .frame;
            let gpu = ctx
                .render_with_gpu(&frame, recipe)
                .unwrap_or_else(|error| panic!("{recipe_name}/{gains_name}: GPU render: {error}"));
            let diff = max_abs_diff(cpu.pixels(), &gpu.pixels);
            let psnr = psnr_db(cpu.pixels(), &gpu.pixels);
            let bias = mean_signed_error(cpu.pixels(), &gpu.pixels);
            eprintln!(
                "as_shot_wb[{recipe_name}/{gains_name}]: maxAbsDiff={diff} psnr={psnr:.2} \
                 bias={bias:+.4}"
            );
            if diff > 1 || psnr < MIN_PSNR_DB || bias.abs() > MAX_ABS_MEAN_SIGNED_ERROR {
                failures.push(format!(
                    "{recipe_name}/{gains_name}: exceeded the standard tone bound \
                     maxAbsDiff <= 1 / PSNR >= {MIN_PSNR_DB} dB / \
                     |meanSignedErr| <= {MAX_ABS_MEAN_SIGNED_ERROR}: got \
                     maxAbsDiff={diff} psnr={psnr:.2} bias={bias:+.4}"
                ));
            }

            // The gains are validation state: binding them must not move a
            // single GPU pixel (no double white balance on the decoder's work).
            ctx.set_camera_white_balance(None)
                .expect("clearing As-Shot context");
            let gpu_unbound = ctx
                .render_with_gpu(&frame, recipe)
                .unwrap_or_else(|error| panic!("{recipe_name}/{gains_name}: GPU render: {error}"));
            if gpu.pixels != gpu_unbound.pixels {
                failures.push(format!(
                    "{recipe_name}/{gains_name}: binding As-Shot gains changed GPU pixels \
                     (they must be validated, never re-applied)"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "As-Shot WB parity: {failures:?}");
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// GPU-RENDER-PARITY-1 stage-2 follow-up: a schema-invalid sharpening radius
/// (> 10.0) must be **rejected**, not silently clamped to the 91-tap kernel.
/// The gate deliberately does *not* classify it as a stage gap: the GPU entry
/// rejects it directly (`validate_gpu_recipe`), mirroring the CPU oracle's
/// `InvalidAdjustment`.
fn sharpening_radius_out_of_schema_is_rejected_not_clamped() {
    let recipe = EditRecipe {
        sharpening: Some(Sharpening {
            version: 1,
            amount: 1.0,
            radius: 12.0,
            detail: 0.5,
            masking: 0.0,
        }),
        ..Default::default()
    };

    // The CPU oracle rejects it loudly.
    assert!(
        render_frame(
            &gradient_frame(16, 16),
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
        "out-of-schema radius must be a hard CPU error"
    );
    // It is a rejection, not an unimplemented stage: no routing reason.
    assert!(
        unsupported_gpu_stages(&recipe).is_empty(),
        "an out-of-schema radius is not a GPU stage gap"
    );

    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped radius rejection check");
            return;
        }
    };
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let frame = gradient_frame(16, 16);
    let err = ctx
        .render_with_gpu(&frame, &recipe)
        .expect_err("GPU must reject the out-of-schema radius");
    assert!(
        format!("{err}").contains("sharpening.radius"),
        "unexpected error: {err}"
    );
    ctx.ensure_vram(16, 16).expect("vram state");
    let err = ctx
        .render_to_vram(&frame, &recipe)
        .expect_err("VRAM path must reject the out-of-schema radius");
    assert!(
        format!("{err}").contains("sharpening.radius"),
        "unexpected error: {err}"
    );
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// Schema-foreign adjustment keys have no GPU meaning and no neutral value:
/// the GPU entry must surface the CPU oracle's `UnsupportedAdjustment` (a clean
/// rejection), not silently drop the key.
fn schema_foreign_adjustment_key_is_rejected() {
    let recipe = EditRecipe {
        adjustments: BTreeMap::from([("definitely_not_a_key".into(), 0.5)]),
        ..Default::default()
    };
    assert!(
        unsupported_gpu_stages(&recipe)
            .iter()
            .any(|r| r.contains("definitely_not_a_key")),
        "unknown key must be reported"
    );

    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped unknown-key check");
            return;
        }
    };
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let err = ctx
        .render_with_gpu(&gradient_frame(16, 16), &recipe)
        .expect_err("unknown key must be rejected, not dropped");
    assert!(
        format!("{err}").contains("UnsupportedAdjustment")
            || format!("{err}").contains("not supported")
            || format!("{err}").contains("unsupported"),
        "unexpected error: {err}"
    );
}
