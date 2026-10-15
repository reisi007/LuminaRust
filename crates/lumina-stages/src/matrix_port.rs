//! MCP-PARITY-C / Impl-A: the render port of the shared matrix runner.
//!
//! The runner needs a render that reports the backend it actually used,
//! because the `--require-gpu` gate is evaluated against that exact decision
//! (never a re-derivation). Each frontend supplies it as a [`MatrixRender`]
//! (the `MatchingRender` precedent in [`crate::regenerate`]): the CLI passes
//! its GPU-aware `render_standard_routed`, the MCP server a CPU-or-GPU choke
//! point of its own. The gate evaluation (`check_expected_route`, in
//! [`crate::matrix_validate`]) lives in the runner, once — both frontends
//! only read the decision the port returned.
//!
//! # The route type
//!
//! [`MatrixRoute`] is the cfg-free port of the CLI's `RenderRoute`
//! (`main.rs`): `Gpu` exists unconditionally so the shared crate carries no
//! `gpu`-feature split — a build without a GPU backend simply never constructs
//! it (see [`MatrixRender::has_gpu_backend`]). The label/reason accessors are
//! the contract the report and the gate read.

use crate::error::StageError;
use lumina_core::ImageFrame;

/// Which backend the render port actually used, with the loud CPU routing
/// reasons (empty for a GPU render).
///
/// The decision is computed by the port ([`MatrixRender`]); the runner and its
/// `--require-gpu` gate only read the returned value, so a future routing
/// change can never silently disagree with the gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatrixRoute {
    /// The render ran on the GPU.
    Gpu,
    /// The render ran on the CPU pipeline, with the loud routing reasons.
    Cpu { reasons: Vec<String> },
}

impl MatrixRoute {
    /// `true` when the render ran on the GPU.
    pub fn is_gpu(&self) -> bool {
        matches!(self, MatrixRoute::Gpu)
    }

    /// Short route label for reports (`"gpu"`/`"cpu"`).
    pub fn label(&self) -> &'static str {
        if self.is_gpu() {
            "gpu"
        } else {
            "cpu"
        }
    }

    /// CPU routing reasons (empty for a GPU render).
    pub fn reasons(&self) -> &[String] {
        match self {
            MatrixRoute::Gpu => &[],
            MatrixRoute::Cpu { reasons } => reasons,
        }
    }
}

/// The render entry the matrix runner needs.
///
/// The shared crate owns the [`lumina_core::RenderContext`] it hands over;
/// only the *route* (caller accelerator vs. CPU oracle) is the caller's
/// choice. Both implementations must be handed the identical context, which is
/// why the context is built in the runner ([`crate::matrix_run::run`]).
pub trait MatrixRender {
    /// Renders `frame` with `context` and returns the output **plus** the
    /// route that produced it. An error aborts the pair loudly; there is no
    /// "render could not happen" fallback.
    fn render(
        &self,
        frame: &ImageFrame,
        recipe: &lumina_sidecar::EditRecipe,
        context: &lumina_core::RenderContext<'_>,
    ) -> Result<(lumina_core::RenderOutput, MatrixRoute), StageError>;

    /// `true` when this port can ever take the GPU route. The runner's
    /// `--require-gpu` pre-check reads it: a port without a GPU backend fails
    /// the flag loudly instead of reporting every recipe as an unexpected CPU
    /// route. The CLI answers with its compile-time `gpu` feature, the CPU
    /// oracle answers `false`.
    fn has_gpu_backend(&self) -> bool {
        false
    }
}

/// The default port: the CPU oracle. `lumina_core::render_frame` is the
/// reference pipeline (and, in a build without the `gpu` feature, literally
/// the same call the CLI makes).
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuMatrixRender;

impl MatrixRender for CpuMatrixRender {
    fn render(
        &self,
        frame: &ImageFrame,
        recipe: &lumina_sidecar::EditRecipe,
        context: &lumina_core::RenderContext<'_>,
    ) -> Result<(lumina_core::RenderOutput, MatrixRoute), StageError> {
        // The CPU oracle only needs the context (which carries the recipe
        // reference); the explicit `recipe` parameter keeps the port uniform
        // with the CLI's GPU-aware entry.
        let _ = recipe;
        Ok((
            lumina_core::render_frame(frame, context)?,
            MatrixRoute::Cpu {
                reasons: Vec::new(),
            },
        ))
    }
}
