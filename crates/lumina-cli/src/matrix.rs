//! LRPAR-MATRIX-RECIPE (Slice 1): `lumina matrix` — the multi-recipe matrix
//! runner.
//!
//! SOLL: `feature/quality/conflicts-and-acceptance.md` § „Rezept-Matrix
//! (LRPAR-MATRIX-RECIPE)". The runner applies the versioned recipe set
//! (`matrix/recipe-set.v1.json`) to the two committed RAW samples, exports
//! through the shared render entry point ([`super::render_standard`]) and
//! verifies the rendered frame against committed goldens with documented PSNR
//! tolerances. `--update-goldens` writes the goldens explicitly.
//!
//! This module is orchestration only: the pixels come from
//! `lumina-core`/`render_standard` (GPU-default, CPU reference otherwise) and
//! the encode uses the shared `ExportOptions` path — there is no second
//! image-processing implementation and no second render pipeline. The only
//! extra operation is the documented, deterministic comparison downscale
//! ([`lumina_core::downscale_bilinear`]) and a diagnostic diff image.
//!
//! Failure policy (Agents.md, no silent fallback): a missing golden, a PSNR
//! tolerance violation, an invalid recipe set and any render/export/IO error
//! are loud. Recipes that reference artifact-backed stages the runner cannot
//! synthesize (source actions, generative edit, external depth, generative
//! spots) are rejected by the recipe-set validation instead of being silently
//! skipped.

use clap::Args;
use lumina_core::{
    downscale_bilinear, psnr, ExportOptions, ImageFileFormat, ImageFrame, RenderContext,
};
use lumina_sidecar::{EditRecipe, SpotRemovalMode};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::{
    decode_input, emit, io_error, render_standard, sanitize_camera_white_balance, CliError,
    StagedArtifact,
};

/// Recipe-set schema version this runner understands.
const MATRIX_SCHEMA_VERSION: u32 = 1;
/// Pipeline version the recipe set is authored for; a mismatch is a loud error
/// (reproducibility over convenience).
const MATRIX_PIPELINE_VERSION: &str = "raster-mvp-1";
/// Default recipe set, relative to the crate manifest (dev/CI convenience);
/// every project folder in the repo is resolved relative to the recipe-set
/// file, never as an absolute path in persistent data.
const DEFAULT_RECIPE_SET: &str = "matrix/recipe-set.v1.json";
/// Golden sub-directory name inside the recipe-set directory.
const GOLDEN_DIR: &str = "golden";

#[derive(Debug, Args)]
pub struct MatrixArgs {
    /// Versioned recipe set (schema_version 1). Default:
    /// `<crate>/matrix/recipe-set.v1.json`.
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
    /// Machine-readable JSON report on stdout (logs stay on stderr).
    #[arg(long)]
    pub json: bool,
}

/// PSNR tolerance class (documented in the SOLL § „Rezept-Matrix"). The class
/// is stored by name so the recipe set is self-documenting; the numeric
/// thresholds live here, in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ToleranceClass {
    /// Byte-identical (PSNR must be infinite).
    Exact,
    /// Per-pixel stages without resampling.
    Strict,
    /// Stages with resampling / spatial filters.
    Standard,
    /// High-variance spatial stages.
    Robust,
}

impl ToleranceClass {
    fn min_psnr_db(self) -> Option<f64> {
        match self {
            ToleranceClass::Exact => None,
            ToleranceClass::Strict => Some(45.0),
            ToleranceClass::Standard => Some(40.0),
            ToleranceClass::Robust => Some(30.0),
        }
    }

    fn label(self) -> &'static str {
        match self {
            ToleranceClass::Exact => "exact",
            ToleranceClass::Strict => "strict",
            ToleranceClass::Standard => "standard",
            ToleranceClass::Robust => "robust",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeSet {
    schema_version: u32,
    pipeline_version: String,
    /// Comparison width in pixels (goldens are stored/compared at this width).
    comparison_width: u32,
    samples: Vec<SampleSpec>,
    recipes: Vec<RecipeSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SampleSpec {
    id: String,
    /// Relative to the recipe-set file's directory (never absolute).
    path: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeSpec {
    id: String,
    #[serde(default)]
    goals: Vec<String>,
    #[serde(default)]
    stages: Vec<String>,
    tolerance: ToleranceClass,
    /// `EditRecipe`-compatible JSON (the same shape a virtual copy stores).
    recipe: serde_json::Value,
}

/// One prepared matrix entry: the source spec plus the parsed recipe.
struct Prepared {
    spec: RecipeSpec,
    recipe: EditRecipe,
}

/// Outcome of one (sample × recipe) pair.
struct PairOutcome {
    sample: String,
    recipe: String,
    status: &'static str,
    /// Golden-relative diagnostic; `None` when no PSNR was computed.
    psnr_db: Option<f64>,
    /// `None` for the `exact` class (PSNR must be infinite).
    min_psnr_db: Option<f64>,
    max_abs_diff: Option<u8>,
    duration_ms: u128,
    golden: PathBuf,
    output: PathBuf,
    /// Failure reason for the text/JSON report.
    message: Option<String>,
}

impl PairOutcome {
    fn passed(&self) -> bool {
        matches!(
            self.status,
            "pass" | "baseline-created" | "baseline-updated"
        )
    }
}

pub fn matrix(args: MatrixArgs) -> Result<(), CliError> {
    let recipe_set_path = match &args.recipe_set {
        Some(path) => path.clone(),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join(DEFAULT_RECIPE_SET),
    };
    let set = load_recipe_set(&recipe_set_path)?;
    let base_dir = recipe_set_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    let comparison_width = args.comparison_width.unwrap_or(set.comparison_width);
    if comparison_width == 0 {
        return Err(CliError::Message(format!(
            "invalid comparison_width 0 in `{}`; must be > 0",
            recipe_set_path.display()
        )));
    }

    let golden_dir = args
        .golden_dir
        .clone()
        .unwrap_or_else(|| base_dir.join(GOLDEN_DIR));
    if args.update_goldens {
        fs::create_dir_all(&golden_dir).map_err(|error| io_error(&golden_dir, error))?;
    }

    // Rendered exports and diff images must never pollute the repository: the
    // default is a temporary working directory that is removed afterwards.
    let temp_guard = match &args.work_dir {
        Some(_) => None,
        None => Some(tempfile::tempdir().map_err(|error| io_error(Path::new("."), error))?),
    };
    let work_dir = match (&args.work_dir, &temp_guard) {
        (Some(dir), _) => {
            fs::create_dir_all(dir).map_err(|error| io_error(dir, error))?;
            dir.clone()
        }
        (None, Some(temp)) => temp.path().to_path_buf(),
        (None, None) => unreachable!("temporary work dir must exist when none was given"),
    };

    let prepared = prepare_recipes(&set, &args.recipe, &recipe_set_path)?;
    if prepared.is_empty() {
        return Err(CliError::Message(format!(
            "recipe set `{}` selected no recipes",
            recipe_set_path.display()
        )));
    }

    let mode = if args.update_goldens {
        "baseline"
    } else {
        "verify"
    };
    let started = Instant::now();
    let mut outcomes = Vec::with_capacity(set.samples.len() * prepared.len());

    for sample in &set.samples {
        let sample_path = resolve_relative(&base_dir, &sample.path);
        if !sample_path.is_file() {
            return Err(CliError::Message(format!(
                "matrix sample `{}` not found at `{}`",
                sample.id,
                sample_path.display()
            )));
        }
        let bytes = fs::read(&sample_path).map_err(|error| io_error(&sample_path, error))?;
        let (frame, raw_metadata) = decode_input(&sample_path, &bytes)?;
        let camera_white_balance = raw_metadata.as_ref().and_then(|metadata| {
            let sanitized = sanitize_camera_white_balance(metadata.camera_white_balance);
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
                message: None,
            };

            let render_ctx = RenderContext {
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
            // uses the standard path (GPU-default where available) and compares
            // within the documented tolerances.
            let rendered = if args.update_goldens {
                lumina_core::render_frame(&frame, &render_ctx)
                    .map_err(|error| CliError::Message(error.to_string()))
            } else {
                render_standard(&frame, &entry.recipe, &render_ctx)
            };
            let render_output = match rendered {
                Ok(output) => output,
                Err(error) => {
                    outcome.status = "error";
                    outcome.message = Some(format!("render failed: {error}"));
                    outcome.duration_ms = pair_started.elapsed().as_millis();
                    outcomes.push(outcome);
                    continue;
                }
            };

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
                    if let Err(error) = fs::write(&output, &bytes) {
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

            if args.update_goldens {
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
                match fs::read(&golden)
                    .map_err(|error| CliError::Io {
                        path: golden.display().to_string(),
                        message: error.to_string(),
                    })
                    .and_then(|bytes| {
                        ImageFrame::decode(&bytes).map_err(|error| {
                            CliError::Message(format!(
                                "golden `{}` could not be decoded: {error}",
                                golden.display()
                            ))
                        })
                    }) {
                    Ok(expected) => {
                        if (expected.width, expected.height)
                            != (comparison.width, comparison.height)
                        {
                            outcome.status = "fail";
                            outcome.message = Some(format!(
                                "golden dimension mismatch: golden {}x{} vs comparison {}x{} (re-baseline)",
                                expected.width, expected.height, comparison.width, comparison.height
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
                                        format_psnr(value)
                                    ),
                                    Some(min) => format!(
                                        "tolerance `{}` requires PSNR >= {min} dB, got {} (max abs diff {})",
                                        entry.spec.tolerance.label(),
                                        format_psnr(value),
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

            outcome.duration_ms = pair_started.elapsed().as_millis();
            outcomes.push(outcome);
        }
    }

    let total_ms = started.elapsed().as_millis();
    let failed: Vec<&PairOutcome> = outcomes.iter().filter(|pair| !pair.passed()).collect();
    let passed = outcomes.len() - failed.len();

    // The machine-readable report is always emitted before the exit decision so
    // scripts see the full picture even on failure.
    let payload = report_json(
        mode,
        &recipe_set_path,
        &golden_dir,
        comparison_width,
        &outcomes,
        total_ms,
    );
    let mut text = report_text(mode, &outcomes, total_ms);
    text.push_str(&format!(
        "\nmatrix: {passed} passed, {} failed, {} total ({total_ms} ms)\n",
        failed.len(),
        outcomes.len()
    ));
    emit(args.json, payload, text.trim_end_matches('\n'))?;

    if failed.is_empty() {
        return Ok(());
    }

    for pair in &failed {
        eprintln!(
            "matrix failure: {}/{}: {}",
            pair.sample,
            pair.recipe,
            pair.message.as_deref().unwrap_or("failed")
        );
    }
    Err(CliError::Message(format!(
        "matrix failed: {} of {} pair(s) not green ({} ms)",
        failed.len(),
        outcomes.len(),
        total_ms
    )))
}

/// Loads and validates the recipe set; every deviation is loud.
fn load_recipe_set(path: &Path) -> Result<RecipeSet, CliError> {
    let raw = fs::read_to_string(path).map_err(|error| io_error(path, error))?;
    let set: RecipeSet = serde_json::from_str(&raw).map_err(|error| {
        CliError::Message(format!("invalid recipe set `{}`: {error}", path.display()))
    })?;
    if set.schema_version != MATRIX_SCHEMA_VERSION {
        return Err(CliError::Message(format!(
            "recipe set `{}` has unsupported schema_version {} (expected {MATRIX_SCHEMA_VERSION})",
            path.display(),
            set.schema_version
        )));
    }
    if set.pipeline_version != MATRIX_PIPELINE_VERSION {
        return Err(CliError::Message(format!(
            "recipe set `{}` targets pipeline_version `{}` (expected `{MATRIX_PIPELINE_VERSION}`)",
            path.display(),
            set.pipeline_version
        )));
    }
    if set.samples.is_empty() {
        return Err(CliError::Message(format!(
            "recipe set `{}` declares no samples",
            path.display()
        )));
    }
    if set.recipes.is_empty() {
        return Err(CliError::Message(format!(
            "recipe set `{}` declares no recipes",
            path.display()
        )));
    }
    let mut sample_ids = std::collections::BTreeSet::new();
    for sample in &set.samples {
        if sample.id.trim().is_empty() {
            return Err(CliError::Message(format!(
                "recipe set `{}` has an empty sample id",
                path.display()
            )));
        }
        if sample.path.is_absolute() {
            return Err(CliError::Message(format!(
                "sample `{}` uses the absolute path `{}`; sample paths must be relative to the recipe set",
                sample.id,
                sample.path.display()
            )));
        }
        if !sample_ids.insert(sample.id.clone()) {
            return Err(CliError::Message(format!(
                "recipe set `{}` has the duplicate sample id `{}`",
                path.display(),
                sample.id
            )));
        }
    }
    let mut recipe_ids = std::collections::BTreeSet::new();
    for recipe in &set.recipes {
        if recipe.id.trim().is_empty() {
            return Err(CliError::Message(format!(
                "recipe set `{}` has an empty recipe id",
                path.display()
            )));
        }
        if !recipe_ids.insert(recipe.id.clone()) {
            return Err(CliError::Message(format!(
                "recipe set `{}` has the duplicate recipe id `{}`",
                path.display(),
                recipe.id
            )));
        }
    }
    Ok(set)
}

/// Parses the selected recipes and rejects artifact-backed stages the runner
/// cannot synthesize (never a silent skip).
fn prepare_recipes(
    set: &RecipeSet,
    selected: &[String],
    path: &Path,
) -> Result<Vec<Prepared>, CliError> {
    for id in selected {
        if !set.recipes.iter().any(|recipe| &recipe.id == id) {
            return Err(CliError::Message(format!(
                "unknown recipe id `{id}` (not in `{}`)",
                path.display()
            )));
        }
    }
    let mut prepared = Vec::new();
    for spec in &set.recipes {
        if !selected.is_empty() && !selected.contains(&spec.id) {
            continue;
        }
        let recipe: EditRecipe = serde_json::from_value(spec.recipe.clone()).map_err(|error| {
            CliError::Message(format!(
                "recipe `{}` in `{}` is not a valid EditRecipe: {error}",
                spec.id,
                path.display()
            ))
        })?;
        reject_artifact_backed_stages(&spec.id, &recipe)?;
        prepared.push(Prepared {
            spec: RecipeSpec {
                id: spec.id.clone(),
                goals: spec.goals.clone(),
                stages: spec.stages.clone(),
                tolerance: spec.tolerance,
                recipe: spec.recipe.clone(),
            },
            recipe,
        });
    }
    Ok(prepared)
}

/// Rejects recipe stages that need persisted artifacts or models the matrix
/// runner deliberately cannot synthesize. Silent skipping would hide the stage
/// and produce a misleading green "covered" result.
fn reject_artifact_backed_stages(id: &str, recipe: &EditRecipe) -> Result<(), CliError> {
    if !recipe.source_actions.is_empty() {
        return Err(CliError::Message(format!(
            "recipe `{id}` references `source_actions`; the matrix runner cannot synthesize zdata repair-region artifacts (loudly rejected, never skipped)"
        )));
    }
    if recipe.generative_edit.is_some() {
        return Err(CliError::Message(format!(
            "recipe `{id}` references `generative_edit`; the matrix runner cannot synthesize a canvas artifact/model (loudly rejected, never skipped)"
        )));
    }
    if recipe
        .lens_blur
        .as_ref()
        .and_then(|blur| blur.depth_artifact.as_ref())
        .is_some()
    {
        return Err(CliError::Message(format!(
            "recipe `{id}` references an external `lens_blur.depth_artifact`; the matrix runner cannot bind a depth plane (loudly rejected, never skipped)"
        )));
    }
    // Typed spot entries are mirror shadows for the geometry-carrying extras
    // view; `generative` entries need a model + artifact.
    if recipe
        .spot_removals
        .iter()
        .any(|spot| spot.mode == SpotRemovalMode::Generative)
    {
        return Err(CliError::Message(format!(
            "recipe `{id}` references a generative spot removal; the matrix runner cannot synthesize the model/artifact (loudly rejected, never skipped)"
        )));
    }
    if let Some(value) = recipe.extras.get("spot_removals") {
        if let Some(entries) = value.as_array() {
            for entry in entries {
                let mode = entry
                    .get("mode")
                    .and_then(|mode| mode.as_str())
                    .unwrap_or("heuristic");
                if mode != "heuristic" {
                    return Err(CliError::Message(format!(
                        "recipe `{id}` references a `{mode}` spot removal; the matrix runner only renders the deterministic heuristic path (loudly rejected, never skipped)"
                    )));
                }
            }
        }
    }
    // F2 (typo protection): `EditRecipe` routes every unknown top-level key into
    // `extras`, where it is a silent no-op — a typo (`geometri`, `lens_corection`)
    // would render as identity and pass the matrix without testing anything. The
    // matrix therefore rejects every extras key except the documented
    // `spot_removals` geometry mirror (GEN-ZDATA-LINK-1 / SPOT-SCHEMA-GEOMETRY).
    for key in recipe.extras.keys() {
        if key != "spot_removals" {
            return Err(CliError::Message(format!(
                "recipe `{id}` carries the unknown top-level key `{key}` (it deserializes into `extras` and would silently render as identity); the matrix rejects unknown keys, only the `spot_removals` geometry mirror is allowed (loudly rejected, never skipped)"
            )));
        }
    }
    Ok(())
}

fn resolve_relative(base_dir: &Path, relative: &Path) -> PathBuf {
    if relative.is_absolute() {
        relative.to_path_buf()
    } else {
        base_dir.join(relative)
    }
}

/// Encodes `frame` as a lossless PNG and atomically publishes it at `target`.
fn encode_and_stage(frame: &ImageFrame, target: &Path) -> Result<(), CliError> {
    let bytes = frame.encode(ImageFileFormat::Png)?;
    StagedArtifact::stage(target, &bytes)?.commit()
}

/// Writes a diagnostic diff image (`|actual - golden|` per RGB channel, alpha
/// opaque) next to the exports so a failing pair can be inspected visually.
fn write_diff(
    work_dir: &Path,
    name: &str,
    actual: &ImageFrame,
    golden: &ImageFrame,
) -> Result<(), CliError> {
    let target = work_dir.join(format!("{name}.diff.png"));
    let mut pixels = Vec::with_capacity(actual.pixels.len());
    for (index, (a, b)) in actual.pixels.iter().zip(&golden.pixels).enumerate() {
        if index % 4 == 3 {
            pixels.push(255);
        } else {
            pixels.push(a.abs_diff(*b));
        }
    }
    let diff = ImageFrame::new(actual.width, actual.height, pixels)?;
    encode_and_stage(&diff, &target)
}

/// Maximum per-channel (RGB) absolute difference between two same-size frames.
fn max_abs_diff(a: &ImageFrame, b: &ImageFrame) -> u8 {
    a.pixels
        .iter()
        .zip(&b.pixels)
        .enumerate()
        .filter(|(index, _)| index % 4 != 3)
        .map(|(_, (x, y))| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

fn format_psnr(value: f64) -> String {
    if value.is_infinite() {
        "inf".to_string()
    } else {
        format!("{value:.2} dB")
    }
}

fn report_text(mode: &str, outcomes: &[PairOutcome], total_ms: u128) -> String {
    let mut text = String::new();
    text.push_str(&format!("matrix ({mode}): {} pair(s)\n", outcomes.len()));
    for pair in outcomes {
        let detail = match (pair.psnr_db, pair.message.as_deref()) {
            (Some(value), _) => format!(
                "psnr={} min={}",
                format_psnr(value),
                pair.min_psnr_db
                    .map(|min| format!("{min} dB"))
                    .unwrap_or_else(|| "inf (exact)".to_string())
            ),
            (None, Some(message)) => message.to_string(),
            (None, None) => "-".to_string(),
        };
        text.push_str(&format!(
            "  [{}] {} / {} ({} ms) {} -> {}\n",
            pair.status,
            pair.sample,
            pair.recipe,
            pair.duration_ms,
            detail,
            pair.output.display()
        ));
    }
    text.push_str(&format!("total: {total_ms} ms"));
    text
}

fn report_json(
    mode: &str,
    recipe_set: &Path,
    golden_dir: &Path,
    comparison_width: u32,
    outcomes: &[PairOutcome],
    total_ms: u128,
) -> serde_json::Value {
    let pairs: Vec<serde_json::Value> = outcomes
        .iter()
        .map(|pair| {
            serde_json::json!({
                "sample": pair.sample,
                "recipe": pair.recipe,
                "status": pair.status,
                "psnr_db": pair.psnr_db.map(|value| {
                    if value.is_infinite() { "inf".to_string() } else { format!("{value:.4}") }
                }),
                "min_psnr_db": pair.min_psnr_db,
                "max_abs_diff": pair.max_abs_diff,
                "duration_ms": pair.duration_ms,
                "golden": pair.golden,
                "output": pair.output,
                "message": pair.message,
            })
        })
        .collect();
    let passed = outcomes.iter().filter(|pair| pair.passed()).count();
    serde_json::json!({
        "command": "matrix",
        "mode": mode,
        "recipe_set": recipe_set,
        "golden_dir": golden_dir,
        "comparison_width": comparison_width,
        "pairs_total": outcomes.len(),
        "passed": passed,
        "failed": outcomes.len() - passed,
        "duration_ms": total_ms,
        "pairs": pairs,
    })
}
