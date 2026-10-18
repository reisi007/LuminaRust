//! GPU-RENDER-MASK-19 (Slice B2): CLI wiring of the GPU local-mask pixel pass.
//!
//! `crates/lumina-gpu/src/local_mask_pass.rs` renders the local mask layers on
//! the GPU (Slice A) and `lumina_core::evaluate_local_mask_layers` is the one
//! shared mask evaluation (Slice B1). This module is the CLI's routing and
//! plumbing half — extracted from `main.rs` on purpose (file-size ratchet §8:
//! new logic in a small, coherent file instead of growing the oversized app
//! module), mirroring the committed `lensfun_gpu.rs` split.
//!
//! ## Routing contract (loud, never a silent fallback)
//!
//! [`mask_layer_routing_reasons`] replaces the former blanket
//! `"masks (active copy has layers)"` reason. Only what the GPU pass genuinely
//! cannot render keeps the exact CPU reference: a **visible**, **non-neutral**
//! layer that carries a P1.2 block (local tone curve / colour / presence /
//! detail), plus malformed persisted local state (which the CPU reference
//! validates and rejects loudly — a GPU-only global render would silently
//! diverge from that error). A visible, non-neutral P0/P1.1 layer is
//! GPU-eligible, so the CLI now renders it on the GPU like any other stage.
//!
//! ## Geometry mapping (why `input_* = frame`, `frame_* = global`)
//!
//! The CPU render sets `mask_input_width`/`mask_input_height` from the
//! **decoded source** frame *before* geometry (`crates/lumina-core/src/render.rs:611-612`)
//! and `frame_width`/`frame_height` from the post-geometry frame
//! (`:697-698`) before calling `evaluate_mask_stage`. The CLI's `frame`
//! argument is exactly that decoded source frame and the GPU global render's
//! output is the post-geometry frame, so the pass is driven with
//! `input = frame.width/height` and `frame = global.width/height` — the same
//! pair the CPU oracle uses.
//!
//! ## Failure handling
//!
//! [`composite`] never returns the un-modulated global frame when the pass is
//! needed: any refusal/failure is returned as `Err(reason)`, and the caller
//! (`render_best_effort`) logs it through the shared `log_cpu_routing_once`
//! channel and runs the **exact CPU path** (`RenderRoute::Cpu { reasons }`), so
//! `--require-gpu` fails loudly. A **mask-free** render (`masks: None`) is not a
//! refusal: it evaluates to zero layers and returns the global frame unchanged
//! on the GPU route, so the mask reason only ever appears when an active copy
//! with layers exists.
//!
//! ## Scope
//!
//! CLI only. The GUI present path (`local_adjustment_route_reason`) stays a
//! separate, later slice; P1.2 blocks stay a loud CPU route.

use crate::RenderOutput;
use lumina_core::{evaluate_local_mask_layers, ImageFrame, MaskLayerResult, RenderContext};
use lumina_gpu::local_mask_pass::{unsupported_local_mask_adjustments, LocalMaskLayer};
use lumina_gpu::{Frame, GpuContext};
use lumina_sidecar::VirtualCopy;

/// The active virtual copy of `render_ctx`, if any (mirrors
/// `lumina_core::render::local_adjustments::active_mask_layers`).
fn active_copy<'a>(render_ctx: &'a RenderContext<'_>) -> Option<&'a VirtualCopy> {
    let masks = render_ctx.masks.as_ref()?;
    masks
        .copies
        .iter()
        .find(|copy| copy.id == masks.active_copy_id)
}

/// CPU-routing reasons contributed by the active copy's local mask layers.
///
/// Only a layer the GPU pass cannot render is a reason:
///
/// - a **visible**, **non-neutral** layer whose typed recipe carries a P1.2
///   block (local tone curve / colour / presence / detail) — the plane-free
///   [`unsupported_local_mask_adjustments`] predicate;
/// - malformed persisted local state: the CPU reference validates it and
///   rejects the render loudly, so routing there keeps that loud error instead
///   of a silent global-only GPU frame.
///
/// Invisible and neutral layers cannot change a pixel and are skipped; a layer
/// without a typed local recipe (`Ok(None)`) is skipped by the CPU oracle's
/// `apply_local_adjustments` too.
pub(crate) fn mask_layer_routing_reasons(render_ctx: &RenderContext<'_>) -> Vec<String> {
    let mut reasons = Vec::new();
    let Some(copy) = active_copy(render_ctx) else {
        return reasons;
    };
    for layer in &copy.mask_layers {
        if !layer.visible {
            continue;
        }
        match layer.effective_local_adjustments() {
            Err(error) => reasons.push(format!(
                "masks (local mask layer `{}` has invalid local state: {error})",
                layer.id
            )),
            Ok(None) => {}
            Ok(Some(adjustments)) if adjustments.is_neutral() => {}
            Ok(Some(adjustments)) => {
                if let Some(reason) = unsupported_local_mask_adjustments(&layer.id, &adjustments) {
                    reasons.push(reason);
                }
            }
        }
    }
    reasons
}

/// Composite the active copy's local mask layers onto an **already globally
/// rendered** GPU frame.
///
/// `frame` is the decoded source frame (the CPU render's `mask_input_*`);
/// `global` is the output of [`GpuContext::render_with_gpu_and_generative`] for
/// the same recipe (the post-geometry frame, the CPU render's `frame_*`). The
/// layers are evaluated with the **same** `evaluate_mask_stage` the CPU render
/// uses, so `mask_layers`/`mask_warnings` match the CPU oracle exactly (the CLI
/// derives the `match_total_exposure` masked domain from them — an empty
/// `mask_layers` here would silently drop that domain).
///
/// The GPU pixel pass runs only when at least one visible layer carries a
/// non-neutral local recipe; a mask-only / neutral set is a pure no-op (the CPU
/// oracle's `apply_local_adjustments` skips the same layers) and returns the
/// global frame unchanged. Each evaluated layer is paired by id with its
/// persisted recipe (a missing pairing is a loud error, never a skip).
///
/// Any refusal or evaluation failure is returned as `Err(reason)`; the caller
/// turns that into the exact CPU path (see the module docs), so the
/// un-modulated global frame is never accepted silently.
pub(crate) fn composite(
    ctx: &GpuContext,
    render_ctx: &RenderContext<'_>,
    frame: &ImageFrame,
    global: &Frame,
) -> Result<RenderOutput, String> {
    // The same evaluation the CPU render runs (one `evaluate_mask_stage`). An
    // error here is the identical error the CPU reference would produce, so
    // routing there keeps the message and the exit code unchanged.
    let (evaluated, mask_warnings) = evaluate_local_mask_layers(
        render_ctx,
        frame.width(),
        frame.height(),
        global.width,
        global.height,
    )
    .map_err(|error| format!("masks ({error})"))?;
    // A **mask-free** render (`masks: None` — the normal CLI/matrix render
    // context, e.g. `testdata/matrix/recipe-set.v1.json`) evaluates to zero
    // layers and has no active copy to pair against. Short-circuit before
    // `pair_layers`, so its missing-active-copy error stays reserved for the
    // genuine divergence: `evaluate_mask_stage` only ever returns a **non-empty**
    // `evaluated` after it found the active copy, so an empty `evaluated` here
    // means there was nothing to pair — not a vanished copy. A non-empty
    // `evaluated` without its persisted layers must still fail loudly below
    // (never a silent global-only frame).
    let layers = if evaluated.is_empty() {
        Vec::new()
    } else {
        pair_layers(render_ctx, &evaluated)?
    };
    if layers.is_empty() {
        return Ok(gpu_output(global.clone(), evaluated, mask_warnings));
    }
    let frame = ctx
        .apply_gpu_local_masks(global, &layers)
        .map_err(|error| format!("masks (GPU local-mask pass refused: {error})"))?;
    Ok(gpu_output(frame, evaluated, mask_warnings))
}

/// The GPU [`RenderOutput`] for an already-modulated frame. `mask_layers` /
/// `mask_warnings` are the evaluated layers — the CPU oracle reports exactly
/// the same values.
fn gpu_output(
    frame: Frame,
    mask_layers: Vec<MaskLayerResult>,
    mask_warnings: Vec<String>,
) -> RenderOutput {
    RenderOutput {
        // MASK-LOCAL-P1.1: the CLI has no local-WB picker, so it never asks
        // for (and never pays for) the pre-local stage.
        effective_source_stage: None,
        frame: frame.to_image_frame(),
        mask_layers,
        mask_warnings,
    }
}

/// Pair each evaluated layer with the persisted local recipe of the same id, in
/// persisted order.
///
/// Only called with a **non-empty** `evaluated` (see [`composite`]): a non-empty
/// evaluation means `evaluate_mask_stage` already found the active copy, so a
/// missing active copy here is a real divergence, not the mask-free no-op case.
/// A missing pairing is a loud error (never a silent skip): evaluation and the
/// persisted list come from the same active copy. A layer without a typed local
/// recipe, or with a neutral one, cannot modulate a pixel and is skipped — the
/// CPU oracle's `apply_local_adjustments` skips exactly those.
fn pair_layers(
    render_ctx: &RenderContext<'_>,
    evaluated: &[MaskLayerResult],
) -> Result<Vec<LocalMaskLayer>, String> {
    let Some(copy) = active_copy(render_ctx) else {
        return Err(
            "masks (GPU local-mask pass: the active copy vanished before layer pairing)".into(),
        );
    };
    let mut layers = Vec::with_capacity(evaluated.len());
    for result in evaluated {
        let layer = copy
            .mask_layers
            .iter()
            .find(|layer| layer.id == result.layer_id)
            .ok_or_else(|| {
                format!(
                    "masks (GPU local-mask pass: evaluated mask layer `{}` has no persisted layer)",
                    result.layer_id
                )
            })?;
        let Some(adjustments) = layer
            .effective_local_adjustments()
            .map_err(|error| format!("masks ({error})"))?
        else {
            continue;
        };
        if adjustments.is_neutral() {
            continue;
        }
        layers.push(LocalMaskLayer {
            layer_id: result.layer_id.clone(),
            plane: result.plane.clone(),
            adjustments,
        });
    }
    Ok(layers)
}
