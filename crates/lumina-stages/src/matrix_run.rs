//! MCP-PARITY-C / Impl-A: the shared, clap-free matrix runner.
//!
//! `lumina matrix` (CLI) and the later `lumina_matrix` MCP tool run **this**
//! function ([`run`]) over a [`MatrixRequest`] — the clap-free mirror of the
//! CLI's `MatrixArgs` (field for field = SOLL §2, mirroring the
//! `RegenerateRequest` precedent in [`crate::regenerate`]). There is no second
//! matrix logic, no second render path, no second golden writer and no second
//! report builder: the CLI is a thin wrapper (args → request → [`run`], port
//! wiring in `crates/lumina-cli/src/matrix.rs`), the MCP tool will be the
//! same over JSON.
//!
//! The render port ([`MatrixRender`]/[`MatrixRoute`], in
//! [`crate::matrix_port`]), the validation substrate (in
//! [`crate::matrix_validate`]) and the report substrate (in
//! [`crate::matrix_report`]) are separate modules so every file stays under
//! the 500-line ratchet.

use super::matrix_port::{MatrixRender, MatrixRoute};
use super::matrix_report::{self, encode_and_stage, max_abs_diff, write_diff, PairOutcome};
use super::matrix_validate::{
    check_expected_route, default_recipe_set_path, load_recipe_set, prepare_recipes,
    resolve_relative, GOLDEN_DIR,
};
use crate::error::StageError;
use lumina_core::{downscale_bilinear, psnr, ExportOptions, ImageFileFormat, ImageFrame};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Transport-neutral matrix request. Every field maps 1:1 onto one CLI flag
/// (`MatrixArgs`); `--json` is deliberately absent — it is a pure CLI
/// transport flag, the shared report always carries the full payload.
#[derive(Debug, Clone, Default)]
pub struct MatrixRequest {
    /// Versioned recipe set. `None` = the env chain
    /// `LUMINA_MATRIX_RECIPE_SET` → `LUMINA_MATRIX_DIR` → ancestor walk.
    pub recipe_set: Option<PathBuf>,
    /// Golden directory. `None` = `<recipe-set-dir>/golden`.
    pub golden_dir: Option<PathBuf>,
    /// Directory for the full-resolution exports and diff images. `None` = a
    /// fresh temporary directory that is removed afterwards.
    pub work_dir: Option<PathBuf>,
    /// Only run the named recipes. Empty = all recipes of the set, in
    /// set order; an unknown id aborts loudly before any render.
    pub recipes: Vec<String>,
    /// Override the comparison width from the recipe set. `None` = the set
    /// value; `Some(0)` is a loud domain error, never a clamp.
    pub comparison_width: Option<u32>,
    /// Write/refresh the goldens instead of verifying (baseline mode:
    /// CPU-pinned, platform-neutral).
    pub update_goldens: bool,
    /// GPU-parity proof: fail every recipe whose declared `expected_route`
    /// does not match the route the render port actually used.
    pub require_gpu: bool,
}

/// What one matrix call produced: the machine-readable report plus the exact
/// human-readable lines, so the two transports cannot format the same state
/// differently.
///
/// A run whose pairs are not all green is still an `Ok` value carrying
/// `summary_error`: the CLI emits the payload first and fails afterwards, and
/// the MCP tool will carry the same payload in its error `data` (SOLL §7.1).
/// Only aborts *before* any report exists (invalid set, missing sample, …)
/// are `Err`.
#[derive(Debug, Clone)]
pub struct MatrixReport {
    /// The `--json` document (SOLL §3), always complete, even on failure.
    pub payload: serde_json::Value,
    /// The human-readable report including the one-line summary (without a
    /// trailing newline; the transport trims/prints it verbatim).
    pub text: String,
    /// The `matrix failure: <sample>/<recipe>: <reason>` lines, in pair order
    /// (empty on green). The transport prints them to stderr verbatim.
    pub failure_lines: Vec<String>,
    /// The `matrix failed: N of M pair(s) not green …` summary (`None` on
    /// green). The transport turns it into its own error type verbatim.
    pub summary_error: Option<String>,
}

/// Runs one matrix call: the pre-extraction `matrix::matrix` body, minus the
/// clap transport and the stdout/stderr effects.
///
/// Side effects are unchanged: reads the samples (never writes them), writes
/// full-resolution exports and diff images into `work_dir`, and writes golden
/// PNGs into `golden_dir` only in baseline mode. With `work_dir: None` the
/// exports land in a fresh temporary directory that is removed when this
/// function returns — exactly as the CLI behaved.
pub fn run<R: MatrixRender + ?Sized>(
    request: &MatrixRequest,
    renderer: &R,
) -> Result<MatrixReport, StageError> {
    let recipe_set_path = match &request.recipe_set {
        Some(path) => path.clone(),
        None => default_recipe_set_path()?,
    };

    if request.require_gpu && request.update_goldens {
        return Err(StageError::Message(
            "--require-gpu cannot be combined with --update-goldens: baseline goldens are deliberately CPU-pinned (platform-neutral), so no GPU route can be proven in baseline mode"
                .into(),
        ));
    }
    // A port without a GPU backend cannot prove parity and must fail loudly
    // instead of reporting every recipe as an "unexpected CPU route".
    if request.require_gpu && !renderer.has_gpu_backend() {
        return Err(StageError::Message(
            "--require-gpu needs a GPU-enabled build (`--features gpu`); this binary has no GPU backend, so no route can be proven"
                .into(),
        ));
    }

    let set = load_recipe_set(&recipe_set_path)?;
    let base_dir = recipe_set_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    let comparison_width = request.comparison_width.unwrap_or(set.comparison_width);
    if comparison_width == 0 {
        return Err(StageError::Message(format!(
            "invalid comparison_width 0 in `{}`; must be > 0",
            recipe_set_path.display()
        )));
    }

    let golden_dir = request
        .golden_dir
        .clone()
        .unwrap_or_else(|| base_dir.join(GOLDEN_DIR));
    if request.update_goldens {
        std::fs::create_dir_all(&golden_dir).map_err(|error| StageError::io(&golden_dir, error))?;
    }

    // Rendered exports and diff images must never pollute the repository: the
    // default is a temporary working directory that is removed afterwards.
    let temp_guard = match &request.work_dir {
        Some(_) => None,
        None => Some(tempfile::tempdir().map_err(|error| StageError::io(Path::new("."), error))?),
    };
    let work_dir = match (&request.work_dir, &temp_guard) {
        (Some(dir), _) => {
            std::fs::create_dir_all(dir).map_err(|error| StageError::io(dir, error))?;
            dir.clone()
        }
        (None, Some(temp)) => temp.path().to_path_buf(),
        (None, None) => unreachable!("temporary work dir must exist when none was given"),
    };

    let prepared = prepare_recipes(&set, &request.recipes, &recipe_set_path)?;
    if prepared.is_empty() {
        return Err(StageError::Message(format!(
            "recipe set `{}` selected no recipes",
            recipe_set_path.display()
        )));
    }

    let mode = if request.update_goldens {
        "baseline"
    } else {
        "verify"
    };
    let started = Instant::now();
    let mut outcomes = Vec::with_capacity(set.samples.len() * prepared.len());

    for sample in &set.samples {
        let sample_path = resolve_relative(&base_dir, &sample.path);
        if !sample_path.is_file() {
            return Err(StageError::Message(format!(
                "matrix sample `{}` not found at `{}`",
                sample.id,
                sample_path.display()
            )));
        }
        let bytes =
            std::fs::read(&sample_path).map_err(|error| StageError::io(&sample_path, error))?;
        let (frame, raw_metadata) = crate::decode::decode_input(&sample_path, &bytes)?;
        let camera_white_balance = raw_metadata.as_ref().and_then(|metadata| {
            let sanitized =
                crate::pipeline::sanitize_camera_white_balance(metadata.camera_white_balance);
            if sanitized.is_none() {
                eprintln!(
                    "lumina: warning: As-Shot white balance invalid {:?} for `{}` — dropping to None (recipe WB remains, image renders)",
                    metadata.camera_white_balance,
                    sample_path.display()
                );
            }
            sanitized
        });

        for entry in &prepared {
            let pair_started = Instant::now();
            let name = format!("{}__{}", sample.id, entry.spec.id);
            let output = work_dir.join(format!("{name}.png"));
            let golden = golden_dir.join(format!("{name}.png"));

            let mut outcome = PairOutcome {
                sample: sample.id.clone(),
                recipe: entry.spec.id.clone(),
                status: "pass",
                psnr_db: None,
                min_psnr_db: entry.spec.tolerance.min_psnr_db(),
                max_abs_diff: None,
                duration_ms: 0,
                golden: golden.clone(),
                output: output.clone(),
                expected_route: entry.spec.expected_route.label(),
                route: None,
                route_reasons: Vec::new(),
                route_error: None,
                message: None,
            };

            let render_ctx = lumina_core::RenderContext {
                recipe: &entry.recipe,
                camera_white_balance,
                // The matrix runner cannot synthesize zdata artifacts / models;
                // artifact-backed recipes are rejected in `prepare_recipes`.
                source_actions: &[],
                masks: None,
                // Deliberately no Lensfun corrector so goldens stay
                // feature-independent (the manual model stays effective).
                lensfun: None,
                depth: None,
            };
            // F4: `--update-goldens` is pinned to the CPU reference so a golden
            // baseline is platform-neutral, reproducible and independent of
            // whether the machine happens to have a GPU adapter. Verification
            // uses the port's path (GPU-default where the caller routes there)
            // and compares within the documented tolerances.
            let rendered = if request.update_goldens {
                lumina_core::render_frame(&frame, &render_ctx)
                    .map(|output| {
                        (
                            output,
                            MatrixRoute::Cpu {
                                reasons: Vec::new(),
                            },
                        )
                    })
                    .map_err(|error| StageError::Message(error.to_string()))
            } else {
                renderer.render(&frame, &entry.recipe, &render_ctx)
            };
            let (render_output, route) = match rendered {
                Ok(pair) => pair,
                Err(error) => {
                    outcome.status = "error";
                    outcome.message = Some(format!("render failed: {error}"));
                    outcome.duration_ms = pair_started.elapsed().as_millis();
                    outcomes.push(outcome);
                    continue;
                }
            };

            outcome.route = Some(route.label());
            outcome.route_reasons = route.reasons().to_vec();
            if request.require_gpu {
                // The route gate is evaluated against the exact decision that
                // produced these pixels (no re-derivation, no log scraping).
                if let Err(message) =
                    check_expected_route(&entry.spec.id, &entry.spec.expected_route, &route)
                {
                    outcome.route_error = Some(message);
                }
            }

            // Export the full-resolution render through the shared encode path
            // (the matrix SOLL says "exportieren"); the golden comparison uses
            // the deterministic downscaled in-memory frame.
            let export = ExportOptions {
                format: ImageFileFormat::Png,
                quality: 90,
                dither: false,
                ..Default::default()
            };
            match render_output.frame.encode_with_options(export) {
                Ok(bytes) => {
                    if let Err(error) = std::fs::write(&output, &bytes) {
                        outcome.status = "error";
                        outcome.message = Some(format!(
                            "could not write export `{}`: {error}",
                            output.display()
                        ));
                        outcome.duration_ms = pair_started.elapsed().as_millis();
                        outcomes.push(outcome);
                        continue;
                    }
                }
                Err(error) => {
                    outcome.status = "error";
                    outcome.message = Some(format!("export encode failed: {error}"));
                    outcome.duration_ms = pair_started.elapsed().as_millis();
                    outcomes.push(outcome);
                    continue;
                }
            }

            let comparison = match downscale_bilinear(&render_output.frame, comparison_width) {
                Ok(comparison) => comparison,
                Err(error) => {
                    outcome.status = "error";
                    outcome.message = Some(format!("comparison downscale failed: {error}"));
                    outcome.duration_ms = pair_started.elapsed().as_millis();
                    outcomes.push(outcome);
                    continue;
                }
            };

            if request.update_goldens {
                let existed = golden.is_file();
                match encode_and_stage(&comparison, &golden) {
                    Ok(()) => {
                        outcome.status = if existed {
                            "baseline-updated"
                        } else {
                            "baseline-created"
                        };
                    }
                    Err(error) => {
                        outcome.status = "error";
                        outcome.message = Some(format!("could not write golden: {error}"));
                    }
                }
            } else if !golden.is_file() {
                outcome.status = "fail";
                outcome.message = Some(format!(
                    "missing golden `{}` (run `lumina matrix --update-goldens` to create the baselines)",
                    golden.display()
                ));
            } else {
                match std::fs::read(&golden)
                    .map_err(|error| StageError::Io {
                        path: golden.display().to_string(),
                        message: error.to_string(),
                    })
                    .and_then(|bytes| {
                        ImageFrame::decode(&bytes).map_err(|error| {
                            StageError::Message(format!(
                                "golden `{}` could not be decoded: {error}",
                                golden.display()
                            ))
                        })
                    }) {
                    Ok(expected) => {
                        if (expected.width(), expected.height())
                            != (comparison.width(), comparison.height())
                        {
                            outcome.status = "fail";
                            outcome.message = Some(format!(
                                "golden dimension mismatch: golden {}x{} vs comparison {}x{} (re-baseline)",
                                expected.width(), expected.height(), comparison.width(), comparison.height()
                            ));
                        } else {
                            let value = psnr(&comparison, &expected);
                            outcome.psnr_db = Some(value);
                            outcome.max_abs_diff = Some(max_abs_diff(&comparison, &expected));
                            let ok = match entry.spec.tolerance.min_psnr_db() {
                                None => value.is_infinite(),
                                Some(min) => !value.is_nan() && value >= min,
                            };
                            if !ok {
                                outcome.status = "fail";
                                outcome.message = Some(match entry.spec.tolerance.min_psnr_db() {
                                    None => format!(
                                        "tolerance `exact` requires byte-identical output (PSNR = inf), got {}",
                                        matrix_report::format_psnr(value)
                                    ),
                                    Some(min) => format!(
                                        "tolerance `{}` requires PSNR >= {min} dB, got {} (max abs diff {})",
                                        entry.spec.tolerance.label(),
                                        matrix_report::format_psnr(value),
                                        outcome.max_abs_diff.unwrap_or(0)
                                    ),
                                });
                                if let Err(error) =
                                    write_diff(&work_dir, &name, &comparison, &expected)
                                {
                                    eprintln!("warning: could not write diff image: {error}");
                                }
                            }
                        }
                    }
                    Err(error) => {
                        outcome.status = "fail";
                        outcome.message = Some(format!("golden could not be read: {error}"));
                    }
                }
            }

            // The route-gate verdict is folded in last so a simultaneous PSNR
            // failure stays visible instead of being overwritten.
            if let Some(route_error) = outcome.route_error.take() {
                if outcome.status == "pass" {
                    outcome.status = "route-fail";
                    outcome.message = Some(route_error);
                } else {
                    let existing = outcome
                        .message
                        .take()
                        .unwrap_or_else(|| "failed".to_string());
                    outcome.message = Some(format!("{existing}; route: {route_error}"));
                }
            }

            outcome.duration_ms = pair_started.elapsed().as_millis();
            outcomes.push(outcome);
        }
    }

    let total_ms = started.elapsed().as_millis();
    let failed: Vec<&PairOutcome> = outcomes.iter().filter(|pair| !pair.passed()).collect();
    let passed = outcomes.len() - failed.len();

    // The machine-readable report is always complete before the exit decision
    // so scripts (and the MCP error `data`) see the full picture even on
    // failure; the transport emits it and fails afterwards.
    let payload = matrix_report::report_json(
        mode,
        &recipe_set_path,
        &golden_dir,
        comparison_width,
        &outcomes,
        total_ms,
    );
    let mut text = matrix_report::report_text(mode, &outcomes, total_ms);
    text.push_str(&format!(
        "\nmatrix: {passed} passed, {} failed, {} total ({total_ms} ms)\n",
        failed.len(),
        outcomes.len()
    ));

    if failed.is_empty() {
        return Ok(MatrixReport {
            payload,
            text,
            failure_lines: Vec::new(),
            summary_error: None,
        });
    }

    let failure_lines = failed
        .iter()
        .map(|pair| {
            format!(
                "matrix failure: {}/{}: {}",
                pair.sample,
                pair.recipe,
                pair.message.as_deref().unwrap_or("failed")
            )
        })
        .collect();
    let summary_error = Some(format!(
        "matrix failed: {} of {} pair(s) not green ({} ms)",
        failed.len(),
        outcomes.len(),
        total_ms
    ));
    Ok(MatrixReport {
        payload,
        text,
        failure_lines,
        summary_error,
    })
}
