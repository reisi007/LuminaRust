//! GPU-RENDER-MASK-19 (Slice B2, Runde 2): the mask-free no-op and the loud
//! refusal contract of the CLI local-mask wiring (`crate::local_mask_gpu`).
//!
//! The routing cells in `tests/local_mask_gpu.rs` pin the *predicate*. This file
//! pins the two contracts the first verification round found unpinned:
//!
//! - **F1 (high):** a mask-free render (`masks: None`, the normal CLI/matrix
//!   render context) must composite as a pure no-op and stay on the GPU route.
//!   `composite` used to call `pair_layers` unconditionally, which failed with
//!   `"the active copy vanished before layer pairing"` whenever there was
//!   nothing to pair — breaking `--require-gpu` for every mask-free recipe.
//! - **F2 (medium):** the `Err` branch of `render_best_effort` must never turn a
//!   needed modulation into a silent un-modulated GPU frame. A visible,
//!   non-neutral P0 layer whose referenced mask is unavailable is the reachable
//!   case: the pure-recipe routing predicate sees no P1.2 block (so no static
//!   reason) and lets the render enter the GPU branch, where the mask
//!   evaluation refuses; the contract is a loud failure, never `RenderRoute::Gpu`
//!   carrying the global frame.
//!
//! Cells that need a Metal adapter print an explicit SKIP verdict and return
//! without a silently green run (the same policy as `tests/lensfun_gpu.rs`); the
//! mask-free `composite` cell needs no adapter (the empty-layer short-circuit
//! runs before any adapter work).

#![cfg(feature = "gpu")]

use super::local_mask_gpu::{context, copy_with, gradient_frame, layer, planes};
use super::*;
use lumina_gpu::{Frame, GpuContext};
use lumina_sidecar::LocalAdjustments;

/// A `RenderContext` with no mask library at all — the CLI/matrix case for a
/// recipe that carries no sidecar mask state (`masks: None`).
fn mask_free_context<'a>(recipe: &'a EditRecipe) -> RenderContext<'a> {
    RenderContext {
        recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    }
}

/// F1 regression: `composite` must be a pure no-op for a mask-free render. This
/// is the production function `render_best_effort` calls; before the fix it
/// returned `Err("... the active copy vanished before layer pairing")` here,
/// because `pair_layers` ran unconditionally and found no active copy. No
/// adapter is needed: the empty `evaluated` short-circuit returns before the
/// adapter is ever consulted.
#[test]
fn mask_free_composite_is_a_no_op_not_a_refusal() {
    let Ok(ctx) = GpuContext::new() else {
        eprintln!("GPU context init failed - skipped mask-free composite test");
        return;
    };
    let frame = gradient_frame();
    let global = Frame::from_image_frame(gradient_frame());
    let recipe = EditRecipe::default();
    let render_ctx = mask_free_context(&recipe);

    let output = crate::local_mask_gpu::composite(&ctx, &render_ctx, &frame, &global)
        .expect("a mask-free render must composite as a pure no-op, never a refusal");
    assert!(
        output.mask_layers.is_empty(),
        "a mask-free render evaluates to zero layers"
    );
    assert!(output.mask_warnings.is_empty());
    assert_eq!(
        output.frame.pixels(),
        global.to_image_frame().pixels(),
        "the no-op must return the global frame unchanged"
    );
}

/// F1 regression, end-to-end through the route decision: a mask-free recipe must
/// reach `RenderRoute::Gpu` with **empty** reasons (the exact A/B the first
/// verification measured with `matrix --require-gpu --recipe g01-identity`).
/// Needs an adapter — the GPU branch is only taken when one is bound.
#[test]
fn mask_free_recipe_stays_on_the_gpu_route() {
    let Ok(mut gpu) = GpuContext::new() else {
        eprintln!("GPU context init failed - skipped mask-free route test");
        return;
    };
    if !gpu.is_available() {
        eprintln!("GPU adapter unavailable - skipped mask-free route test");
        return;
    }
    let frame = gradient_frame();
    let recipe = EditRecipe::default();
    let render_ctx = mask_free_context(&recipe);

    let (output, route) = render_best_effort(
        Some(&mut gpu),
        &frame,
        &recipe,
        &render_ctx,
        GenerativeCanvasInput::default(),
    )
    .expect("a mask-free recipe renders on the GPU");
    assert!(
        route.is_gpu(),
        "a mask-free recipe must take the GPU route, reasons={:?}",
        route.reasons()
    );
    assert!(
        route.reasons().is_empty(),
        "a mask-free recipe must contribute no CPU reason: {:?}",
        route.reasons()
    );
    assert!(output.mask_layers.is_empty());
}

/// F2: the `Err` branch must fail loudly instead of returning the un-modulated
/// global frame as `RenderRoute::Gpu`.
///
/// The reachable trigger is a visible, non-neutral **P0** layer that references
/// an unavailable mask: the plane-free routing predicate is pure recipe state
/// (no P1.2 block → no static reason), so the render enters the GPU branch; the
/// shared `evaluate_mask_stage` then rejects the unavailable mask under
/// `MaskPolicy::Strict`, and the contract is to route to the exact CPU path — or,
/// as here, propagate the loud error when the CPU reference rejects it too.
/// Mutating the `Err` branch to return the global frame as `Gpu` makes this cell
/// red. Needs an adapter (the GPU branch is only taken when one is bound).
#[test]
fn unavailable_mask_fails_loudly_not_with_a_silent_gpu_frame() {
    let Ok(mut gpu) = GpuContext::new() else {
        eprintln!("GPU context init failed - skipped unavailable-mask refusal test");
        return;
    };
    if !gpu.is_available() {
        eprintln!("GPU adapter unavailable - skipped unavailable-mask refusal test");
        return;
    }
    let frame = gradient_frame();
    // A visible, non-neutral P0 layer over a mask id that is not in the copy's
    // mask library (`copy_with` only registers `subject`).
    let mut orphan = layer(
        "orphan",
        Some(LocalAdjustments {
            exposure: 1.0,
            ..LocalAdjustments::default()
        }),
        true,
    );
    orphan.mask.mask_id = "missing".into();
    let copies = vec![copy_with(vec![orphan])];
    let masks = context(&copies, &planes());
    let recipe = EditRecipe::default();
    let render_ctx = RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: Some(masks),
        lensfun: None,
        depth: None,
    };

    // The pure-recipe predicate names no static reason, so the render really
    // enters the GPU branch (where the failure must be handled loudly).
    assert!(
        gpu_routing_reasons(&recipe, &render_ctx).is_empty(),
        "the GPU branch must be entered to exercise its Err handling"
    );

    let result = render_best_effort(
        Some(&mut gpu),
        &frame,
        &recipe,
        &render_ctx,
        GenerativeCanvasInput::default(),
    );
    let error = result.expect_err(
        "an unavailable mask must fail loudly, never return an Ok un-modulated GPU frame",
    );
    let message = error.to_string();
    assert!(
        message.to_lowercase().contains("mask") || message.contains("missing"),
        "the loud failure must name the mask: {message}"
    );
}
