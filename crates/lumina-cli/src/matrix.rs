//! LRPAR-MATRIX-RECIPE (Slice 1): `lumina matrix` — the multi-recipe matrix
//! runner.
//!
//! SOLL: `feature/quality/conflicts-and-acceptance.md` § „Rezept-Matrix
//! (LRPAR-MATRIX-RECIPE)".
//!
//! MCP-PARITY-C / Impl-A: this module is a thin wrapper only. The runner
//! (request validation, render orchestration, report building, golden/diff/
//! export writing, `expected_route` handling) lives in `lumina-stages`
//! (`matrix_run`/`matrix_validate`/`matrix_report`) so `lumina matrix` and the
//! later `lumina_matrix` MCP tool share one implementation. This module holds
//! only the `clap` argument struct, the `*Args -> *Request` mapping, the CLI's
//! GPU-aware render port and the output emission; the CLI behaviour — report
//! JSON, golden bytes, exports, diff images, error texts, exit codes — is
//! unchanged.

use clap::Args;
use lumina_core::{ImageFrame, RenderContext, RenderOutput};
use lumina_sidecar::EditRecipe;
use lumina_stages::matrix_run::{self, MatrixRequest};
use lumina_stages::StageError;
use lumina_stages::{MatrixRender, MatrixRoute};
use std::path::PathBuf;

use super::{emit, render_standard_routed, CliError};

#[derive(Debug, Args)]
pub struct MatrixArgs {
    /// Versioned recipe set (schema_version 1). Default: env
    /// `LUMINA_MATRIX_RECIPE_SET`/`LUMINA_MATRIX_DIR`, else the workspace-root
    /// `testdata/matrix/recipe-set.v1.json`.
    #[arg(long)]
    pub recipe_set: Option<PathBuf>,
    /// Golden directory (default: `<recipe-set-dir>/golden`).
    #[arg(long)]
    pub golden_dir: Option<PathBuf>,
    /// Directory for the full-resolution exports and diff images (default: a
    /// fresh temporary directory that is removed afterwards).
    #[arg(long)]
    pub work_dir: Option<PathBuf>,
    /// Only run the named recipe(s); repeatable. An unknown id aborts loudly.
    #[arg(long)]
    pub recipe: Vec<String>,
    /// Override the comparison width from the recipe set (goldens are stored
    /// and compared at this width).
    #[arg(long)]
    pub comparison_width: Option<u32>,
    /// Write/refresh the goldens instead of verifying (baseline mode).
    #[arg(long)]
    pub update_goldens: bool,
    /// GPU-parity proof: fail every recipe whose declared `expected_route` in
    /// the recipe set does not match the backend the standard render path
    /// actually used. The one documented CPU route
    /// (`geometry (default content crop)`) is the only accepted exemption.
    /// Needs a GPU-enabled build; the baseline mode is CPU-pinned and rejects
    /// this flag.
    #[arg(long)]
    pub require_gpu: bool,
    /// Machine-readable JSON report on stdout (logs stay on stderr).
    #[arg(long)]
    pub json: bool,
}

/// The CLI's render entry for the matrix runner. It is the pre-extraction
/// call, unchanged: `render_standard_routed`, so the CLI keeps its GPU routing
/// and the `--require-gpu` gate keeps proving parity from the same decision
/// that produced the pixels. In a build without the `gpu` feature this is the
/// CPU pipeline, which is what the shared CPU oracle calls — the
/// configuration the byte-identity tests measure.
struct CliMatrixRender;

impl MatrixRender for CliMatrixRender {
    fn render(
        &self,
        frame: &ImageFrame,
        recipe: &EditRecipe,
        context: &RenderContext<'_>,
    ) -> Result<(RenderOutput, MatrixRoute), StageError> {
        render_standard_routed(frame, recipe, context)
            .map(|(output, route)| {
                // The port reads the one routing decision (`label`/`reasons`),
                // it never re-derives it.
                (
                    output,
                    match route.label() {
                        "gpu" => MatrixRoute::Gpu,
                        _ => MatrixRoute::Cpu {
                            reasons: route.reasons().to_vec(),
                        },
                    },
                )
            })
            .map_err(|error| StageError::Message(error.to_string()))
    }

    fn has_gpu_backend(&self) -> bool {
        cfg!(feature = "gpu")
    }
}

pub fn matrix(args: MatrixArgs) -> Result<(), CliError> {
    let request = MatrixRequest {
        recipe_set: args.recipe_set.clone(),
        golden_dir: args.golden_dir.clone(),
        work_dir: args.work_dir.clone(),
        recipes: args.recipe.clone(),
        comparison_width: args.comparison_width,
        update_goldens: args.update_goldens,
        require_gpu: args.require_gpu,
    };
    // `StageError` renders exactly the text the pre-extraction `CliError`
    // rendered (`transparent`), so the report, the stderr lines and the exit
    // codes below are unchanged.
    let report = matrix_run::run(&request, &CliMatrixRender)?;

    // The machine-readable report is always emitted before the exit decision so
    // scripts see the full picture even on failure.
    emit(
        args.json,
        report.payload,
        report.text.trim_end_matches('\n'),
    )?;

    if let Some(summary) = report.summary_error {
        for line in &report.failure_lines {
            eprintln!("{line}");
        }
        return Err(CliError::Message(summary));
    }
    Ok(())
}
