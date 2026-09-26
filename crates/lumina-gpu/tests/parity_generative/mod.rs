//! Generative-expand GPU parity tests, extracted from `parity.rs` (file-size
//! ratchet, `DoD.md` §8). Each artifact is produced deterministically by
//! `lumina_core::generative` and compared byte-for-byte against the CPU oracle.
//!
//! The child reaches the parent test crate's imports and helpers through
//! `use super::*`; only the four generative-only `lumina_core` names need an
//! explicit import here.

use super::*;
use lumina_core::{
    render_frame_with_generative, GenerativeCanvasArtifact, GenerativeCanvasInput,
    GenerativeRole as CoreGenerativeRole,
};

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// A recipe that still uses an unimplemented stage stays CPU-routed and yields
/// GEN-ONNX-1 Welle 2a: `generative_edit` is no longer a CPU-routing reason, but
/// an **artifact-blind** render (recipe-only GPU entry / CPU `render_frame`) must
/// still refuse loudly instead of rendering unexpanded.
fn artifact_blind_generative_render_is_loud_not_routed() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped routing check");
            return;
        }
    };
    let frame = gradient_frame(48, 48);
    let recipe = generative_expand_recipe();
    // Welle 2a: no blanket CPU route anymore — the stage is GPU-eligible.
    assert!(
        unsupported_gpu_stages(&recipe).is_empty(),
        "generative_edit must not be a routing reason: {:?}",
        unsupported_gpu_stages(&recipe)
    );
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE} - validator-only assertion");
        return;
    }
    // Without an artifact both the CPU oracle and the artifact-blind GPU entry
    // reject loudly (no silent unexpanded render, no third state).
    let context = RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    assert!(
        render_frame(&frame, &context).is_err(),
        "the CPU oracle must refuse an expand without a composited canvas"
    );
    assert!(
        ctx.render_with_gpu(&frame, &recipe).is_err(),
        "the artifact-blind GPU entry must refuse an expand without a canvas"
    );
    // With the artifact the artifact-aware GPU entry renders successfully.
    let canvas = lumina_core::generative::apply_generative_expand(&frame, &recipe)
        .expect("deterministic producer");
    let artifact = GenerativeCanvasArtifact::new(CoreGenerativeRole::Expand, canvas);
    let result = ctx.render_with_gpu_and_generative(
        &frame,
        &recipe,
        &GenerativeCanvasInput {
            auto_fill: None,
            expand: Some(&artifact),
        },
    );
    assert!(
        result.is_ok(),
        "the artifact-aware GPU entry must render an expand with a canvas: {:?}",
        result.err()
    );
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// GEN-ONNX-1 Welle 2a GPU parity of the mid-geometry expand compositing: the
/// chain is `Substitute(expand) → Crop` (an exact integer crop after the
/// substitution). The compositing itself is a texture swap, so parity is
/// byte-identical (`maxAbsDiff == 0`), matching the other exact stages.
fn generative_canvas_compositing_is_gpu_parity() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped compositing parity");
            return;
        }
    };
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let frame = gradient_frame(48, 48);
    let mut recipe = generative_expand_recipe();
    // Force a render pass *after* the substitution so the mid-chain insertion is
    // exercised (an exact 0.5 crop; the crop pass is a pure integer copy).
    recipe.geometry = Some(Geometry {
        version: 1,
        crop: Some(Crop::Free {
            x: 0.0,
            y: 0.0,
            width: 0.5,
            height: 0.5,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    });
    // Deterministic producer: the exact canvas the render will adopt.
    let canvas_frame = lumina_core::generative::apply_generative_expand(&frame, &recipe)
        .expect("deterministic producer");
    let artifact = GenerativeCanvasArtifact::new(CoreGenerativeRole::Expand, canvas_frame);

    let input = GenerativeCanvasInput {
        auto_fill: None,
        expand: Some(&artifact),
    };
    let cpu = render_frame_with_generative(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
        input,
    )
    .expect("CPU oracle compositing render")
    .frame;

    let gpu = ctx
        .render_with_gpu_and_generative(&frame, &recipe, &input)
        .expect("GPU generative render");

    assert_eq!(
        (cpu.width, cpu.height),
        (gpu.width, gpu.height),
        "composited dimensions must match on both backends"
    );
    assert_eq!(
        max_abs_diff(&cpu.pixels, &gpu.pixels),
        0,
        "the expand compositing + downstream crop must be GPU-parity"
    );
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// GEN-ONNX-1 Welle 2a GPU parity of the auto-fill compositing: the plan is a
/// single `Substitute(auto-fill)` (no other geometry), which exercises the
/// "only substitutions → copy into the final texture" path. Byte-identical.
fn generative_auto_fill_compositing_is_gpu_parity() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped auto-fill parity");
            return;
        }
    };
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    // 32x32 frame with a transparent border wedge.
    let mut pixels = vec![0u8; 32 * 32 * 4];
    for y in 0..32u32 {
        for x in 0..32u32 {
            let idx = ((y * 32 + x) * 4) as usize;
            if x < 4 || y < 4 || x >= 28 || y >= 28 {
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
    let recipe = EditRecipe {
        generative_edit: Some(GenerativeEdit {
            version: 1,
            canvas: None,
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: Some(true),
            expand_beyond_image: None,
            seed: Some(11),
            prompt: None,
            extras: BTreeMap::new(),
        }),
        ..Default::default()
    };
    // Deterministic producer over the post-lens frame (no lens here).
    let mut canvas = frame.clone();
    lumina_core::generative::fill_transparent_heuristic(&mut canvas, 11);
    let artifact = GenerativeCanvasArtifact::new(CoreGenerativeRole::AutoFillTransparent, canvas);
    let input = GenerativeCanvasInput {
        auto_fill: Some(&artifact),
        expand: None,
    };

    let cpu = render_frame_with_generative(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
        input,
    )
    .expect("CPU oracle auto-fill render")
    .frame;
    let gpu = ctx
        .render_with_gpu_and_generative(&frame, &recipe, &input)
        .expect("GPU auto-fill render");
    assert_eq!((cpu.width, cpu.height), (32, 32));
    assert_eq!(
        max_abs_diff(&cpu.pixels, &gpu.pixels),
        0,
        "the auto-fill compositing must be GPU-parity"
    );
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
/// GEN-ONNX-1 Welle 2a BLOCKER fix: a real render pass **before** a trailing
/// `Substitute` must not discard the artifact. Recipe: a manual lens pass (real
/// geometry) plus an explicit full-frame crop, then expand (trailing substitute,
/// no crop step). The CPU oracle discards the lens output (`composite_expand`
/// replaces the frame); the GPU must return the artifact, not the lens output.
/// Before the fix this silently returned the lens texture (`Ok`, 4022/4096
/// bytes divergent).
fn generative_trailing_substitute_after_render_pass_is_gpu_parity() {
    let ctx = match GpuContext::new() {
        Ok(ctx) => ctx,
        Err(err) => {
            eprintln!("GPU context init failed ({err}) - skipped trailing-substitute parity");
            return;
        }
    };
    if !support::require_adapter(&ctx) {
        eprintln!("{SKIP_MESSAGE}");
        return;
    }
    let frame = gradient_frame(48, 48);
    let mut recipe = generative_expand_recipe();
    // A real geometry render pass before the trailing substitute…
    recipe.lens_correction = Some(LensCorrection {
        version: 1,
        profile: None,
        distortion_k1: Some(0.2),
        distortion_k2: None,
        distortion_k3: None,
        vignette_c0: None,
        vignette_c1: None,
        vignette_c2: None,
        ca_red: None,
        ca_blue: None,
    });
    // …and an explicit (authoritative) full-frame crop so no default
    // content-crop reason applies; the crop is the identity and adds no step.
    recipe.geometry = Some(Geometry {
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
    });
    // A flat artifact makes any divergence (lens output vs artifact) obvious.
    let artifact = GenerativeCanvasArtifact::new(
        CoreGenerativeRole::Expand,
        solid_frame(128, 96, [10, 20, 30, 255]),
    );
    let input = GenerativeCanvasInput {
        auto_fill: None,
        expand: Some(&artifact),
    };

    let cpu = render_frame_with_generative(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        },
        input,
    )
    .expect("CPU oracle trailing-substitute render")
    .frame;
    let gpu = ctx
        .render_with_gpu_and_generative(&frame, &recipe, &input)
        .expect("GPU trailing-substitute render");

    assert_eq!(
        (cpu.width, cpu.height),
        (128, 96),
        "the CPU oracle result is the expand canvas"
    );
    assert_eq!(
        (gpu.width, gpu.height),
        (128, 96),
        "the GPU must return the trailing artifact, not the lens pass output"
    );
    assert_eq!(
        max_abs_diff(&cpu.pixels, &gpu.pixels),
        0,
        "a render pass before a trailing substitute must not be served as the result"
    );
    assert_eq!(
        &gpu.pixels[..4],
        &[10, 20, 30, 255],
        "the GPU result is the substituted artifact"
    );
}
