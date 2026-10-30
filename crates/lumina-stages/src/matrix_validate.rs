//! MCP-PARITY-C / Impl-A: validation substrate of the shared matrix runner.
//!
//! Moved verbatim out of `crates/lumina-cli/src/matrix.rs`
//! (`load_recipe_set`, `prepare_recipes`, `reject_artifact_backed_stages`,
//! the recipe-set types, `check_expected_route`): every rejection text is
//! load-bearing for the CLI ≡ MCP error-class parity (SOLL §7), so a second
//! copy on either frontend would drift the loud rejections. Both frontends call
//! [`load_recipe_set`], [`prepare_recipes`] and [`check_expected_route`] here.

use super::matrix_port::MatrixRoute;
use crate::error::StageError;
use lumina_sidecar::{EditRecipe, SpotRemovalMode};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Recipe-set schema version the runner understands.
pub(crate) const MATRIX_SCHEMA_VERSION: u32 = 1;
/// Pipeline version the recipe set is authored for; a mismatch is a loud error
/// (reproducibility over convenience).
pub(crate) const MATRIX_PIPELINE_VERSION: &str = "raster-mvp-1";
/// Environment override for the matrix directory (committed recipe set +
/// goldens): `LUMINA_MATRIX_DIR=/path/to/testdata/matrix`.
pub(crate) const MATRIX_DIR_ENV: &str = "LUMINA_MATRIX_DIR";
/// Environment override for an explicit recipe-set file; wins over
/// [`MATRIX_DIR_ENV`].
pub(crate) const MATRIX_RECIPE_SET_ENV: &str = "LUMINA_MATRIX_RECIPE_SET";
/// Committed matrix location, relative to the workspace root. Shared by the CLI
/// runner and the GUI headless matrix; every project folder is
/// resolved relative to the recipe-set file, never as an absolute path in
/// persistent data.
pub(crate) const DEFAULT_MATRIX_DIR: &str = "testdata/matrix";
/// Default recipe-set file name inside the matrix directory.
pub(crate) const DEFAULT_RECIPE_SET: &str = "recipe-set.v1.json";
/// Golden sub-directory name inside the recipe-set directory.
pub(crate) const GOLDEN_DIR: &str = "golden";

/// PSNR tolerance class (documented in the SOLL § „Rezept-Matrix"). The class
/// is stored by name so the recipe set is self-documenting; the numeric
/// thresholds live here, in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ToleranceClass {
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
    pub(crate) fn min_psnr_db(self) -> Option<f64> {
        match self {
            ToleranceClass::Exact => None,
            ToleranceClass::Strict => Some(45.0),
            ToleranceClass::Standard => Some(40.0),
            ToleranceClass::Robust => Some(30.0),
        }
    }

    pub(crate) fn label(self) -> &'static str {
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
pub(crate) struct RecipeSet {
    pub(crate) schema_version: u32,
    pub(crate) pipeline_version: String,
    /// Comparison width in pixels (goldens are stored/compared at this width).
    pub(crate) comparison_width: u32,
    pub(crate) samples: Vec<SampleSpec>,
    pub(crate) recipes: Vec<RecipeSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SampleSpec {
    pub(crate) id: String,
    /// Relative to the recipe-set file's directory (never absolute).
    pub(crate) path: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecipeSpec {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) goals: Vec<String>,
    #[serde(default)]
    pub(crate) stages: Vec<String>,
    pub(crate) tolerance: ToleranceClass,
    /// Backend route this recipe must take. `gpu` or the single documented CPU
    /// exemption `{ "cpu": "<reason>" }`; verified by `--require-gpu`.
    pub(crate) expected_route: ExpectedRoute,
    /// `EditRecipe`-compatible JSON (the same shape a virtual copy stores).
    pub(crate) recipe: serde_json::Value,
}

/// Expected render route declared per recipe. Externally tagged so the recipe
/// set stays self-documenting: `"gpu"` or `{ "cpu": "<reason>" }`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ExpectedRoute {
    /// Must render on the GPU (no CPU fallback).
    Gpu,
    /// Documented, deliberately loud CPU route; the reason must appear in the
    /// reasons the standard render path reported (e.g.
    /// `geometry (default content crop)`).
    Cpu(String),
}

impl ExpectedRoute {
    pub(crate) fn label(&self) -> String {
        match self {
            ExpectedRoute::Gpu => "gpu".to_string(),
            ExpectedRoute::Cpu(reason) => format!("cpu ({reason})"),
        }
    }
}

/// One prepared matrix entry: the source spec plus the parsed recipe.
pub(crate) struct Prepared {
    pub(crate) spec: RecipeSpec,
    pub(crate) recipe: EditRecipe,
}

/// Loads and validates the recipe set; every deviation is loud.
pub(crate) fn load_recipe_set(path: &Path) -> Result<RecipeSet, StageError> {
    let raw = std::fs::read_to_string(path).map_err(|error| StageError::io(path, error))?;
    let set: RecipeSet = serde_json::from_str(&raw).map_err(|error| {
        StageError::Message(format!("invalid recipe set `{}`: {error}", path.display()))
    })?;
    if set.schema_version != MATRIX_SCHEMA_VERSION {
        return Err(StageError::Message(format!(
            "recipe set `{}` has unsupported schema_version {} (expected {MATRIX_SCHEMA_VERSION})",
            path.display(),
            set.schema_version
        )));
    }
    if set.pipeline_version != MATRIX_PIPELINE_VERSION {
        return Err(StageError::Message(format!(
            "recipe set `{}` targets pipeline_version `{}` (expected `{MATRIX_PIPELINE_VERSION}`)",
            path.display(),
            set.pipeline_version
        )));
    }
    if set.samples.is_empty() {
        return Err(StageError::Message(format!(
            "recipe set `{}` declares no samples",
            path.display()
        )));
    }
    if set.recipes.is_empty() {
        return Err(StageError::Message(format!(
            "recipe set `{}` declares no recipes",
            path.display()
        )));
    }
    let mut sample_ids = std::collections::BTreeSet::new();
    for sample in &set.samples {
        if sample.id.trim().is_empty() {
            return Err(StageError::Message(format!(
                "recipe set `{}` has an empty sample id",
                path.display()
            )));
        }
        if sample.path.is_absolute() {
            return Err(StageError::Message(format!(
                "sample `{}` uses the absolute path `{}`; sample paths must be relative to the recipe set",
                sample.id,
                sample.path.display()
            )));
        }
        if !sample_ids.insert(sample.id.clone()) {
            return Err(StageError::Message(format!(
                "recipe set `{}` has the duplicate sample id `{}`",
                path.display(),
                sample.id
            )));
        }
    }
    let mut recipe_ids = std::collections::BTreeSet::new();
    for recipe in &set.recipes {
        if recipe.id.trim().is_empty() {
            return Err(StageError::Message(format!(
                "recipe set `{}` has an empty recipe id",
                path.display()
            )));
        }
        if !recipe_ids.insert(recipe.id.clone()) {
            return Err(StageError::Message(format!(
                "recipe set `{}` has the duplicate recipe id `{}`",
                path.display(),
                recipe.id
            )));
        }
        // A documented CPU exemption must name its reason; an empty one would
        // turn the `--require-gpu` gate into a silent pass.
        if let ExpectedRoute::Cpu(reason) = &recipe.expected_route {
            if reason.trim().is_empty() {
                return Err(StageError::Message(format!(
                    "recipe `{}` in `{}` declares an empty CPU-route reason; a documented CPU exemption must name its reason (e.g. `geometry (default content crop)`)",
                    recipe.id,
                    path.display()
                )));
            }
        }
    }
    Ok(set)
}

/// Parses the selected recipes and rejects artifact-backed stages the runner
/// cannot synthesize (never a silent skip).
pub(crate) fn prepare_recipes(
    set: &RecipeSet,
    selected: &[String],
    path: &Path,
) -> Result<Vec<Prepared>, StageError> {
    for id in selected {
        if !set.recipes.iter().any(|recipe| &recipe.id == id) {
            return Err(StageError::Message(format!(
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
            StageError::Message(format!(
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
                expected_route: spec.expected_route.clone(),
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
fn reject_artifact_backed_stages(id: &str, recipe: &EditRecipe) -> Result<(), StageError> {
    if !recipe.source_actions.is_empty() {
        return Err(StageError::Message(format!(
            "recipe `{id}` references `source_actions`; the matrix runner cannot synthesize zdata repair-region artifacts (loudly rejected, never skipped)"
        )));
    }
    if recipe.generative_edit.is_some() {
        return Err(StageError::Message(format!(
            "recipe `{id}` references `generative_edit`; the matrix runner cannot synthesize a canvas artifact/model (loudly rejected, never skipped)"
        )));
    }
    if recipe
        .lens_blur
        .as_ref()
        .and_then(|blur| blur.depth_artifact.as_ref())
        .is_some()
    {
        return Err(StageError::Message(format!(
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
        return Err(StageError::Message(format!(
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
                    return Err(StageError::Message(format!(
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
            return Err(StageError::Message(format!(
                "recipe `{id}` carries the unknown top-level key `{key}` (it deserializes into `extras` and would silently render as identity); the matrix rejects unknown keys, only the `spot_removals` geometry mirror is allowed (loudly rejected, never skipped)"
            )));
        }
    }
    Ok(())
}

pub(crate) fn resolve_relative(base_dir: &Path, relative: &Path) -> PathBuf {
    if relative.is_absolute() {
        relative.to_path_buf()
    } else {
        base_dir.join(relative)
    }
}

/// Resolves the committed recipe-set path when no explicit set is given.
///
/// There is no build-host path baked into the binary: `LUMINA_MATRIX_RECIPE_SET`
/// names an explicit file, `LUMINA_MATRIX_DIR` a matrix directory, and the
/// fallback walks up from the current directory to the workspace-root
/// `testdata/matrix/recipe-set.v1.json`. Everything referenced by the set
/// itself stays relative to the set's directory.
pub(crate) fn default_recipe_set_path() -> Result<PathBuf, StageError> {
    if let Some(path) = std::env::var_os(MATRIX_RECIPE_SET_ENV) {
        return Ok(PathBuf::from(path));
    }
    let matrix_dir = match std::env::var_os(MATRIX_DIR_ENV) {
        Some(dir) => PathBuf::from(dir),
        None => resolve_default_matrix_dir()?,
    };
    Ok(matrix_dir.join(DEFAULT_RECIPE_SET))
}

/// Walks up from the current directory to the ancestor that owns the committed
/// `testdata/matrix` recipe set.
fn resolve_default_matrix_dir() -> Result<PathBuf, StageError> {
    let cwd = std::env::current_dir().map_err(|error| {
        StageError::Message(format!(
            "could not determine the current directory to locate the matrix recipe set: {error}"
        ))
    })?;
    for ancestor in cwd.ancestors() {
        let candidate = ancestor.join(DEFAULT_MATRIX_DIR);
        if candidate.join(DEFAULT_RECIPE_SET).is_file() {
            return Ok(candidate);
        }
    }
    Err(StageError::Message(format!(
        "could not locate the committed matrix recipe set `{DEFAULT_MATRIX_DIR}/{DEFAULT_RECIPE_SET}` at or above `{}`; set {MATRIX_DIR_ENV} (or {MATRIX_RECIPE_SET_ENV}) or pass --recipe-set",
        cwd.display()
    )))
}

/// Verifies the declared `expected_route` against the route the render port
/// actually used. Any mismatch is a loud GPU-parity failure
/// (`AGENTS.md`: no silent CPU fallback), including an unavailable adapter or a
/// CPU route for a different reason than the documented exemption.
pub(crate) fn check_expected_route(
    recipe_id: &str,
    expected: &ExpectedRoute,
    actual: &MatrixRoute,
) -> Result<(), String> {
    match expected {
        ExpectedRoute::Gpu if actual.is_gpu() => Ok(()),
        ExpectedRoute::Gpu => Err(format!(
            "recipe `{recipe_id}` expected the GPU route but the standard render path used the CPU{}; a non-exempt CPU route is a GPU-parity failure (no-fallback doctrine)",
            format_route_reasons(actual.reasons())
        )),
        ExpectedRoute::Cpu(reason) if actual.is_gpu() => Err(format!(
            "recipe `{recipe_id}` declares the documented CPU route `{reason}` but the standard render path used the GPU"
        )),
        ExpectedRoute::Cpu(reason) => {
            if actual.reasons().iter().any(|r| r.contains(reason.as_str())) {
                Ok(())
            } else {
                Err(format!(
                    "recipe `{recipe_id}` expected the documented CPU route `{reason}` but rendered on the CPU for a different reason{}",
                    format_route_reasons(actual.reasons())
                ))
            }
        }
    }
}

fn format_route_reasons(reasons: &[String]) -> String {
    if reasons.is_empty() {
        " (no GPU adapter available)".to_string()
    } else {
        format!(" (reasons: {})", reasons.join("; "))
    }
}
