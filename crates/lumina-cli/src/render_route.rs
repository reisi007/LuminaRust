//! The one render-routing decision: [`RenderRoute`], the GPU-vs-CPU choice
//! ([`render_best_effort`]) with its loud CPU routing reasons and the
//! `render_standard*` entry points the command slices call.
//!
//! Extracted verbatim from `main.rs` for the file-size ratchet
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100, render-routing slice; the `process_selected`
//! handler has to stay in the crate root because the structural Auto-Tone guard
//! in `tests/auto_tone_writer.rs` pins its shared-writer call to `src/main.rs`).
//! Pure move: bodies are
//! byte-identical, the `#[cfg(feature = "gpu")]` / `#[cfg(not(feature = "gpu"))]`
//! pairs moved verbatim; only the `pub(super)` tokens and this header are new.

use super::*;
/// Lazily creates a per-thread [`GpuContext`] (one adapter/device per worker
/// thread in a batch) and logs the backend selection exactly once per process.
/// Returns `None` when GPU init fails or no adapter is available.
#[cfg(feature = "gpu")]
fn init_render_backend() -> Option<GpuContext> {
    match GpuContext::new() {
        Ok(ctx) => {
            if ctx.is_available() {
                if let Some(info) = ctx.adapter_info() {
                    log_backend(&format!("render backend: gpu ({info})"));
                } else {
                    log_backend("render backend: gpu (unknown adapter)");
                }
            } else {
                log_backend("render backend: cpu");
            }
            Some(ctx)
        }
        Err(error) => {
            log_backend(&format!("render backend: cpu (gpu init failed: {error})"));
            None
        }
    }
}

// Per-thread cache for the [`GpuContext`], so the (potentially expensive)
// adapter/device enumeration happens once per worker thread rather than per
// image in a batch.
//
// SIGTRAP-GPU-TESTS: wgpu `Device`/`Queue` teardown on a Rayon worker thread
// can raise SIGTRAP (Metal autorelease / thread-affine teardown). The context
// is therefore intentionally leaked per worker thread — `ManuallyDrop` prevents
// the thread_local destructor from running wgpu teardown on Rayon thread exit.
// The OS reclaims the leaked allocation at process exit; this is safe because
// `GpuContext` is process-scoped and never needs orderly drop on workers.
#[cfg(feature = "gpu")]
thread_local! {
    static GPU_CTX: std::cell::RefCell<std::mem::ManuallyDrop<Option<GpuContext>>> =
        const { std::cell::RefCell::new(std::mem::ManuallyDrop::new(None)) };
}

/// Which backend the standard render entry actually used, with the loud CPU
/// routing reasons (empty for a GPU render).
///
/// The router computes this decision in exactly one place
/// ([`render_best_effort`]); the LRPAR-MATRIX-RECIPE `--require-gpu` gate
/// consumes the returned value instead of re-deriving or string-matching a log
/// line, so a future routing change can never silently disagree with the gate.
/// The `Gpu` variant only exists in GPU-enabled builds; the accessors below
/// abstract that away so callers never need `#[cfg]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenderRoute {
    #[cfg(feature = "gpu")]
    Gpu,
    Cpu {
        reasons: Vec<String>,
    },
}

impl RenderRoute {
    /// `true` when the render ran on the GPU.
    pub(crate) fn is_gpu(&self) -> bool {
        match self {
            #[cfg(feature = "gpu")]
            RenderRoute::Gpu => true,
            RenderRoute::Cpu { .. } => false,
        }
    }

    /// Short route label for reports (`"gpu"`/`"cpu"`).
    pub(crate) fn label(&self) -> &'static str {
        if self.is_gpu() {
            "gpu"
        } else {
            "cpu"
        }
    }

    /// CPU routing reasons (empty for a GPU render).
    pub(crate) fn reasons(&self) -> &[String] {
        match self {
            #[cfg(feature = "gpu")]
            RenderRoute::Gpu => &[],
            RenderRoute::Cpu { reasons } => reasons,
        }
    }
}

/// Result of the routed standard-render call, including the exact report from
/// `lumina-gpu` when that backend was entered. An absent report means the CLI
/// took a CPU route before calling the GPU renderer.
#[derive(Debug)]
pub(crate) struct RoutedRender {
    pub(crate) output: RenderOutput,
    pub(crate) route: RenderRoute,
    #[cfg(feature = "gpu")]
    pub(crate) gpu_report: Option<lumina_gpu::RenderExecutionReport>,
}

/// Renders `frame` with `recipe`, preferring the GPU when an adapter is bound,
/// otherwise the full platform-neutral CPU pipeline. Returns the actual route
/// alongside the result so callers can assert parity (see [`RenderRoute`]).
///
/// REVIEW-GPU-DIVERGENCE-1 / CAMERA-WB-WELLE: the GPU path implements the full
/// adjustment/geometry chain. Before routing to the GPU, the render is validated
/// against **both** the recipe and the render context ([`gpu_routing_reasons`] —
/// e.g. an **invalid** decoder As-Shot WB context, R2-MCP-01, and
/// touched-but-reset sliders at their neutral value, R2-GPU-05). A valid As-Shot
/// context is carried into the GPU entry via
/// [`GpuContext::set_camera_white_balance`], and a strict Lensfun corrector's
/// map via [`lensfun_gpu::bind`]. Any unsupported stage routes the whole render
/// explicitly to the CPU pipeline with a once-per-reason-set log line, so
/// GPU-enabled builds always produce the same pixels as CPU builds. The GPU is
/// an accelerator, never a semantic change (AGENTS.md: no silent fallbacks).
#[cfg(all(test, feature = "gpu"))]
pub(super) fn render_best_effort(
    ctx: Option<&mut GpuContext>,
    frame: &ImageFrame,
    recipe: &EditRecipe,
    render_ctx: &RenderContext<'_>,
    generative: GenerativeCanvasInput<'_>,
) -> Result<(RenderOutput, RenderRoute), CliError> {
    render_best_effort_with_report(ctx, frame, recipe, render_ctx, generative)
        .map(|rendered| (rendered.output, rendered.route))
}

#[cfg(feature = "gpu")]
pub(super) fn render_best_effort_with_report(
    ctx: Option<&mut GpuContext>,
    frame: &ImageFrame,
    recipe: &EditRecipe,
    render_ctx: &RenderContext<'_>,
    generative: GenerativeCanvasInput<'_>,
) -> Result<RoutedRender, CliError> {
    let mut reasons = gpu_routing_reasons(recipe, render_ctx);

    match ctx {
        Some(ctx) if ctx.is_available() && reasons.is_empty() => {
            // GPU-LENSFUN-PARITY-1 (F7): build/bind the strict corrector's map
            // for this source/dimensions before the GPU entry. An unbound map or
            // a distortion corrector without an explicit crop keeps the exact CPU
            // reference and surfaces the reason — never a silent corrector loss.
            if let Some(reason) = lensfun_gpu::bind(ctx, render_ctx, frame.width(), frame.height())
            {
                reasons.push(reason.to_string());
                lumina_gpu::log_cpu_routing_once(&reasons, "cli render");
                let output = render_frame_with_generative(frame, render_ctx, generative)
                    .map_err(|error| CliError::Message(error.to_string()))?;
                return Ok(RoutedRender {
                    output,
                    route: RenderRoute::Cpu { reasons },
                    gpu_report: None,
                });
            }
            // CAMERA-WB-WELLE (R2-MCP-01): carry the decoder As-Shot context into
            // the GPU entry like the Lensfun corrector / depth plane. The gains
            // are validated there with the oracle's own error but never
            // re-applied (the decoder already multiplied them in), so a valid
            // context renders byte-identically to the CPU reference. The gate
            // already flags invalid gains; this branch is the belt-and-braces
            // entry validation and falls back loudly (never silently).
            if let Err(error) = ctx.set_camera_white_balance(render_ctx.camera_white_balance) {
                let reasons = vec![format!("camera_white_balance ({error})")];
                lumina_gpu::log_cpu_routing_once(&reasons, "cli render");
                let output = render_frame_with_generative(frame, render_ctx, generative)
                    .map_err(|error| CliError::Message(error.to_string()))?;
                return Ok(RoutedRender {
                    output,
                    route: RenderRoute::Cpu { reasons },
                    gpu_report: None,
                });
            }
            // GEN-ONNX-1 Welle 2a: the generative artifact compositing runs on
            // the GPU (mid-geometry `Substitute` steps) — no blanket CPU route.
            let (global, gpu_report) = ctx
                .render_with_gpu_and_generative_report(frame, recipe, &generative)
                .map_err(|error| CliError::Message(error.to_string()))?;
            // GPU-RENDER-MASK-19 (Slice B2): a pass refusal is a loud CPU route.
            let mask_outcome = local_mask_gpu::composite(ctx, render_ctx, frame, &global);
            let route = route_after_mask_stage(&gpu_report, &mask_outcome);
            match mask_outcome {
                Ok(output) => {
                    if !route.reasons().is_empty() {
                        lumina_gpu::log_cpu_routing_once(route.reasons(), "cli render");
                    }
                    Ok(RoutedRender {
                        output,
                        route,
                        gpu_report: Some(gpu_report),
                    })
                }
                Err(_) => {
                    lumina_gpu::log_cpu_routing_once(route.reasons(), "cli render");
                    let output = render_frame_with_generative(frame, render_ctx, generative)
                        .map_err(|error| CliError::Message(error.to_string()))?;
                    Ok(RoutedRender {
                        output,
                        route,
                        gpu_report: Some(gpu_report),
                    })
                }
            }
        }
        _ => {
            if !reasons.is_empty() {
                lumina_gpu::log_cpu_routing_once(&reasons, "cli render");
            }
            let output = render_frame_with_generative(frame, render_ctx, generative)
                .map_err(|error| CliError::Message(error.to_string()))?;
            Ok(RoutedRender {
                output,
                route: RenderRoute::Cpu { reasons },
                gpu_report: None,
            })
        }
    }
}

#[cfg(feature = "gpu")]
fn route_after_mask_stage(
    report: &lumina_gpu::RenderExecutionReport,
    masks: &Result<RenderOutput, String>,
) -> RenderRoute {
    let mut reasons = match report {
        lumina_gpu::RenderExecutionReport::GpuPipelineCompleted => Vec::new(),
        lumina_gpu::RenderExecutionReport::CpuFallback { reason } => {
            vec![format!("lumina-gpu CPU fallback: {reason:?}")]
        }
    };
    match masks {
        Ok(_) => match report {
            lumina_gpu::RenderExecutionReport::GpuPipelineCompleted => RenderRoute::Gpu,
            lumina_gpu::RenderExecutionReport::CpuFallback { .. } => RenderRoute::Cpu { reasons },
        },
        Err(mask_reason) => {
            reasons.push(mask_reason.clone());
            RenderRoute::Cpu { reasons }
        }
    }
}

/// Recipe- and context-level reasons that force CPU rendering in
/// [`render_best_effort`]. Pure decision logic so tests can pin the routing
/// contract without a GPU adapter.
///
/// Context-level features the GPU path cannot reproduce here: an **invalid**
/// decoder As-Shot white balance (R2-MCP-01; valid gains are carried into the
/// GPU entry and are pixel-neutral) and source-action artifacts. Local mask
/// layers are no longer a blanket reason: only a visible, non-neutral layer that
/// carries a **P1.2** block is named (`local_mask_gpu::mask_layer_routing_reasons`).
/// The Lensfun corrector is **not** a static reason: [`lensfun_gpu::bind`]
/// builds/binds its map and reports only an unbound map or a distortion
/// corrector without an explicit crop as a loud CPU route.
#[cfg(feature = "gpu")]
pub(super) fn gpu_routing_reasons(
    recipe: &EditRecipe,
    render_ctx: &RenderContext<'_>,
) -> Vec<String> {
    let mut reasons = unsupported_gpu_stages_with_context(
        recipe,
        false,
        render_ctx.camera_white_balance.as_ref(),
    );
    // Invalid adjustments must also CPU-route so the CPU pipeline's strict
    // validation (finite + range) runs and rejects them loudly. The GPU shader
    // has no notion of pipeline ranges and would otherwise silently render
    // `inf`/`nan`/out-of-range values instead of erroring (so
    // `cargo test --features gpu` would greenly ignore contract violations).
    for (key, value) in &recipe.adjustments {
        let (minimum, maximum) = match key.as_str() {
            "exposure" => (-10.0, 10.0),
            "contrast" | "highlights" | "shadows" | "whites" | "blacks" | "wb_tint"
            | "vibrance" | "saturation" => (-1.0, 1.0),
            "wb_temperature" => (1500.0, 12000.0),
            _ => continue, // unknown keys already flagged above
        };
        if !value.is_finite() || *value < minimum || *value > maximum {
            reasons.push(format!("invalid adjustment `{key}`"));
        }
    }
    if !render_ctx.source_actions.is_empty() {
        reasons.push("source_actions (context artifacts)".into());
    }
    // GPU-RENDER-MASK-19 (Slice B2): the blanket `masks (active copy has layers)`
    // route is replaced by the real P1.2 predicate — a P0-only / relative-WB
    // layer set stays GPU-eligible and is composited by `local_mask_gpu`.
    reasons.extend(local_mask_gpu::mask_layer_routing_reasons(render_ctx));
    reasons
}

#[cfg(not(feature = "gpu"))]
pub(super) fn render_best_effort_with_report(
    _ctx: Option<()>,
    frame: &ImageFrame,
    _recipe: &EditRecipe,
    render_ctx: &RenderContext<'_>,
    generative: GenerativeCanvasInput<'_>,
) -> Result<RoutedRender, CliError> {
    let output = render_frame_with_generative(frame, render_ctx, generative)
        .map_err(|error| CliError::Message(error.to_string()))?;
    Ok(RoutedRender {
        output,
        route: RenderRoute::Cpu {
            reasons: Vec::new(),
        },
    })
}

/// GEN-ONNX-1: [`render_standard_routed`] with caller-supplied generative
/// canvas artifacts (artifact compositing on the CPU oracle path).
pub(super) fn render_standard_with_generative(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    render_ctx: &RenderContext<'_>,
    generative: GenerativeCanvasInput<'_>,
) -> Result<RenderOutput, CliError> {
    render_standard_with_generative_report(frame, recipe, render_ctx, generative)
        .map(|rendered| rendered.output)
}

/// Same standard-render production path as the output-only entry, retaining
/// the per-call GPU report for diagnostics and routing verification.
pub(crate) fn render_standard_with_generative_report(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    render_ctx: &RenderContext<'_>,
    generative: GenerativeCanvasInput<'_>,
) -> Result<RoutedRender, CliError> {
    #[cfg(feature = "gpu")]
    {
        GPU_CTX.with(|cell| {
            let mut gpu = cell.borrow_mut();
            if gpu.is_none() {
                **gpu = init_render_backend();
            }
            match gpu.as_mut() {
                Some(ctx) => {
                    render_best_effort_with_report(Some(ctx), frame, recipe, render_ctx, generative)
                }
                None => render_best_effort_with_report(None, frame, recipe, render_ctx, generative),
            }
        })
    }
    #[cfg(not(feature = "gpu"))]
    {
        log_backend("render backend: cpu");
        render_best_effort_with_report(None, frame, recipe, render_ctx, generative)
    }
}

/// [`render_standard`] returning the actual [`RenderRoute`] as well. The
/// matrix's `--require-gpu` gate uses it to prove GPU parity from the **same**
/// decision that produced the pixels (never a re-derived or log-scraped guess).
pub(crate) fn render_standard_routed(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    render_ctx: &RenderContext<'_>,
) -> Result<(RenderOutput, RenderRoute), CliError> {
    render_standard_routed_with_generative(
        frame,
        recipe,
        render_ctx,
        GenerativeCanvasInput::default(),
    )
}

/// GEN-ONNX-1: [`render_standard_routed`] with generative canvas artifacts.
pub(crate) fn render_standard_routed_with_generative(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    render_ctx: &RenderContext<'_>,
    generative: GenerativeCanvasInput<'_>,
) -> Result<(RenderOutput, RenderRoute), CliError> {
    render_standard_with_generative_report(frame, recipe, render_ctx, generative)
        .map(|rendered| (rendered.output, rendered.route))
}

#[cfg(all(test, feature = "gpu"))]
#[path = "render_route_execution_report_tests.rs"]
mod execution_report_tests;
