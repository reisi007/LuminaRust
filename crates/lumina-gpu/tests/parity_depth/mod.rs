use super::*;

// ---------------------------------------------------------------------------
// GPU-RENDER-PARITY-1 lens-blur wave: G-05 external depth + VRAM parity
// ---------------------------------------------------------------------------

/// A deterministic per-pixel depth plane (`0` top-left → `1` bottom-right,
/// mixed axes) so both ramps of `focal_weight` are exercised.
fn depth_plane(width: u32, height: u32) -> DepthPlane {
    let mut values = Vec::with_capacity((width * height) as usize);
    let dx = (width.max(2) - 1) as f32;
    let dy = (height.max(2) - 1) as f32;
    for y in 0..height {
        for x in 0..width {
            values.push((x as f32 / dx * 0.7 + y as f32 / dy * 0.3).clamp(0.0, 1.0));
        }
    }
    DepthPlane::new(width, height, values).expect("depth plane")
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// G-05 with a caller-supplied external depth plane: the GPU pass must match
/// the CPU oracle, and every missing/mismatched plane must fail loudly on both
/// backends (never a silent heuristic fallback).
fn lens_blur_external_depth_matches_cpu_oracle() {
    let mut ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped lens-blur depth check");
            return;
        }
    };
    let frame = gradient_frame(48, 48);
    let plane = depth_plane(48, 48);
    let recipe = external_depth_lens_blur_recipe();
    assert!(
        unsupported_gpu_stages(&recipe).is_empty(),
        "external-depth lens blur is GPU-eligible"
    );

    // Missing plane: the CPU oracle aborts loudly, and so must the GPU entry
    // (a silent heuristic render would diverge from the source-of-truth).
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
        "the CPU oracle must abort a missing depth artifact"
    );
    let gpu_err = ctx
        .render_with_gpu(&frame, &recipe)
        .expect_err("GPU must reject a missing depth artifact");
    assert!(
        format!("{gpu_err}").contains("lens_blur.depth_artifact"),
        "unexpected error: {gpu_err}"
    );

    ctx.set_depth_plane(Some(&plane)).expect("bind depth plane");
    let cpu = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: Some(&plane),
        },
    )
    .expect("CPU oracle render")
    .frame;

    if !support::require_adapter(&ctx) {
        // CPU-only context: the bound plane flows into the full reference, so
        // the fallback is still the exact CPU pixels.
        eprintln!("{SKIP_MESSAGE} - CPU fallback depth assertion only");
        let gpu = ctx
            .render_with_gpu(&frame, &recipe)
            .expect("CPU fallback renders the bound depth plane");
        assert_eq!(max_abs_diff(cpu.pixels(), &gpu.pixels), 0);
        return;
    }

    let gpu = ctx
        .render_with_gpu(&frame, &recipe)
        .expect("GPU render with external depth");
    assert_eq!((cpu.width(), cpu.height()), (gpu.width, gpu.height));
    let diff = max_abs_diff(cpu.pixels(), &gpu.pixels);
    let psnr = psnr_db(cpu.pixels(), &gpu.pixels);
    let bias = mean_signed_error(cpu.pixels(), &gpu.pixels);
    eprintln!("lens_blur_external_depth: maxAbsDiff={diff} psnr={psnr:.2} bias={bias:+.4}");
    assert!(
        diff <= 1 && psnr >= MIN_PSNR_DB && bias.abs() <= MAX_ABS_MEAN_SIGNED_ERROR,
        "external-depth lens blur exceeded the declared bound maxAbsDiff <= 1 / \
         PSNR >= {MIN_PSNR_DB} dB / |meanSignedErr| <= {MAX_ABS_MEAN_SIGNED_ERROR}: \
         maxAbsDiff={diff} psnr={psnr:.2} bias={bias:+.4}"
    );

    // A dimension-mismatched plane is rejected loudly, not resampled.
    let bad = depth_plane(8, 8);
    ctx.set_depth_plane(Some(&bad))
        .expect("bind mismatched plane");
    assert!(
        ctx.render_with_gpu(&frame, &recipe).is_err(),
        "a mismatched depth plane must be rejected loudly"
    );

    // Out-of-range / non-finite values are rejected at bind time (no partial
    // state, no silent clamp).
    let mut invalid = depth_plane(8, 8);
    invalid.values[0] = 1.5;
    assert!(
        ctx.set_depth_plane(Some(&invalid)).is_err(),
        "an out-of-range depth value must be rejected"
    );
    invalid.values[0] = f32::NAN;
    assert!(
        ctx.set_depth_plane(Some(&invalid)).is_err(),
        "a NaN depth value must be rejected"
    );
    ctx.set_depth_plane(None).expect("clear depth plane");
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// The readback-free VRAM path renders G-05 lens blur into the resident output
/// (dimension-preserving, source-sized) and matches the CPU oracle — including
/// after a dimension-preserving geometry chain and with a bound external depth
/// plane.
fn vram_path_applies_lens_blur() {
    let mut ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped VRAM lens-blur check");
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
    ctx.ensure_vram(W, H).expect("vram state");

    // `after_mirror_geometry` exercises the geometry→lens-blur ping-pong: the
    // dimension-preserving, default-crop-inactive mirror pass must write a
    // transient, not the resident output the blur then overwrites. (A manual
    // lens without an explicit crop is CPU-routed since GPU-MAXRECT-WELLE.)
    let mut after_mirror_geometry = lens_blur_recipe(0.5, 0.0, 0.05, BokehShape::Round);
    after_mirror_geometry.geometry = Some(Geometry {
        version: 1,
        crop: None,
        rotation_degrees: 0.0,
        mirror_horizontal: true,
        mirror_vertical: false,
    });

    let recipes: [(&str, EditRecipe, Equivalence); 3] = [
        (
            "round",
            lens_blur_recipe(0.6, 0.0, 0.05, BokehShape::Round),
            Equivalence::Bounded(1),
        ),
        (
            "hexagonal",
            lens_blur_recipe(0.6, 0.0, 0.05, BokehShape::Hexagonal),
            Equivalence::Bounded(1),
        ),
        (
            "after_mirror_geometry",
            after_mirror_geometry,
            Equivalence::Bounded(1),
        ),
    ];
    for (name, recipe, equivalence) in recipes {
        assert!(
            unsupported_gpu_stages(&recipe).is_empty(),
            "lens blur must be GPU-eligible"
        );
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
        assert_eq!((cpu.width(), cpu.height()), (gpu.width, gpu.height));
        let diff = max_abs_diff(cpu.pixels(), &gpu.pixels);
        let psnr = psnr_db(cpu.pixels(), &gpu.pixels);
        let bias = mean_signed_error(cpu.pixels(), &gpu.pixels);
        eprintln!("vram_lens_blur[{name}]: maxAbsDiff={diff} psnr={psnr:.2} bias={bias:+.4}");
        match equivalence {
            Equivalence::ByteIdentical => {
                assert_eq!(diff, 0, "vram[{name}] must be byte-identical")
            }
            Equivalence::Bounded(bound) => assert!(
                diff <= bound && psnr >= MIN_PSNR_DB && bias.abs() <= MAX_ABS_MEAN_SIGNED_ERROR,
                "vram[{name}] exceeded declared bound maxAbsDiff <= {bound}: \
                 maxAbsDiff={diff} psnr={psnr:.2} bias={bias:+.4}"
            ),
        }
    }

    // External depth in VRAM: a referenced artifact without a bound plane must
    // refuse before any resident write; with the plane bound the pass matches
    // the oracle.
    let external = external_depth_lens_blur_recipe();
    assert!(
        ctx.render_to_vram(&frame, &external).is_err(),
        "VRAM must refuse a referenced-but-unbound depth artifact"
    );
    let plane = depth_plane(W, H);
    ctx.set_depth_plane(Some(&plane)).expect("bind plane");
    ctx.render_to_vram(&frame, &external)
        .expect("vram render with external depth");
    let gpu = ctx.readback_output_frame().expect("vram readback");
    let cpu = render_frame(
        &frame,
        &RenderContext {
            recipe: &external,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: Some(&plane),
        },
    )
    .expect("CPU oracle render")
    .frame;
    let diff = max_abs_diff(cpu.pixels(), &gpu.pixels);
    eprintln!("vram_lens_blur[external_depth]: maxAbsDiff={diff}");
    assert!(
        diff <= 1,
        "VRAM external-depth lens blur: maxAbsDiff={diff}"
    );
}
