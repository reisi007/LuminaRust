//! LRPAR-MATRIX-RECIPE (Slice 2): the headless GUI matrix runner.
//!
//! SOLL: `feature/quality/conflicts-and-acceptance.md` § „Rezept-Matrix
//! (LRPAR-MATRIX-RECIPE)" → „GUI-headless-Modus". The runner drives the real
//! [`crate::LuminaApp`] on an egui context (no window, no GPU renderer) over the
//! committed `testdata/matrix` recipe set and compares the app's preview against
//! the **same** committed goldens as the CLI runner, with the documented PSNR
//! tolerance classes. It goes through the app pipeline (`load_bytes` → set
//! recipe → full-resolution matrix render → `preview()`), so there is no second
//! render pipeline and no GPU-only path: the app preview *is* the shared CPU
//! core render.
//!
//! The committed CR3 samples make the full matrix RAW/fixture-dependent and
//! slow, so `real_matrix_headless` is `#[ignore]`d and env-gated
//! (`LUMINA_MATRIX=1`), mirroring the CLI `matrix_e2e`. A hermetic, always-on
//! test pins that the app preview is byte-identical to the core `render_frame`
//! output and exercises the tolerance gate end-to-end in a tempdir.
//!
//! The recipe-set schema mirror is deliberate: `lumina-cli` is a binary crate
//! and cannot be linked, so the small schema + tolerance table is repeated here.
//! The numeric thresholds and the golden contract are the identical documented
//! values from the CLI runner / SOLL (kept in sync by a test that fails loudly
//! if the committed set no longer deserializes).

use eframe::egui;
use lumina_core::{downscale_bilinear, psnr, ImageFrame};
use lumina_sidecar::EditRecipe;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::LuminaApp;
mod render;

/// Recipe-set schema version this runner understands (matches the CLI).
const MATRIX_SCHEMA_VERSION: u32 = 1;
/// Pipeline version the recipe set is authored for (matches the CLI).
const MATRIX_PIPELINE_VERSION: &str = "raster-mvp-1";

/// Committed matrix directory: `LUMINA_MATRIX_DIR`, else
/// `<workspace root>/testdata/matrix` (the GUI manifest lives in
/// `crates/lumina-gui`, so the workspace root is two levels up). A test-side
/// default only — the CLI runner resolves its path without a build-host path.
pub(crate) fn matrix_dir() -> PathBuf {
    std::env::var_os("LUMINA_MATRIX_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/matrix"))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeSet {
    schema_version: u32,
    pipeline_version: String,
    comparison_width: u32,
    samples: Vec<SampleSpec>,
    recipes: Vec<RecipeSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SampleSpec {
    id: String,
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
    /// Declared route (verified by the CLI `--require-gpu`); parsed here so the
    /// set stays forward-compatible and reported for traceability.
    expected_route: ExpectedRoute,
    recipe: serde_json::Value,
}

/// Mirrors the CLI's route declaration (never used to *route*, only reported).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ExpectedRoute {
    Gpu,
    Cpu(String),
}

impl ExpectedRoute {
    fn label(&self) -> String {
        match self {
            ExpectedRoute::Gpu => "gpu".to_string(),
            ExpectedRoute::Cpu(reason) => format!("cpu ({reason})"),
        }
    }
}

/// PSNR tolerance class (same thresholds as the CLI runner / SOLL § tolerance
/// table).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ToleranceClass {
    Exact,
    Strict,
    Standard,
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

/// One (sample × recipe) outcome of the headless run.
pub(crate) struct PairReport {
    pub sample: String,
    pub recipe: String,
    pub status: &'static str,
    pub psnr_db: Option<f64>,
    pub max_abs_diff: Option<u8>,
    pub duration_ms: u128,
    pub expected_route: String,
    pub message: Option<String>,
}

impl PairReport {
    pub fn passed(&self) -> bool {
        self.status == "pass"
    }
}

/// Full report of one headless matrix run (verify mode only).
pub(crate) struct MatrixReport {
    pub pairs: Vec<PairReport>,
    pub total_ms: u128,
}

impl MatrixReport {
    pub fn failed(&self) -> Vec<&PairReport> {
        self.pairs.iter().filter(|pair| !pair.passed()).collect()
    }

    /// Human-readable per-pair + total summary (mirrors the CLI report shape).
    pub fn summary(&self) -> String {
        let mut text = String::new();
        text.push_str(&format!("gui-matrix: {} pair(s)\n", self.pairs.len()));
        for pair in &self.pairs {
            let detail = match (pair.psnr_db, pair.message.as_deref()) {
                (Some(value), _) => format!("psnr={value:.2}"),
                (None, Some(message)) => message.to_string(),
                (None, None) => "-".to_string(),
            };
            text.push_str(&format!(
                "  [{}] {} / {} ({} ms) expected={} {}\n",
                pair.status,
                pair.sample,
                pair.recipe,
                pair.duration_ms,
                pair.expected_route,
                detail
            ));
        }
        text.push_str(&format!("total: {} ms", self.total_ms));
        text
    }
}

/// Runs the committed recipe set headless through [`LuminaApp`] and verifies
/// every rendered preview against `golden_dir` with the documented tolerances.
///
/// `golden_dir == None` uses `<recipe-set-dir>/golden`. `comparison_width`
/// overrides the set's width (same semantics as the CLI flag). `selected`
/// filters the recipes by id (empty = all), mirroring the CLI `--recipe` flag so
/// a time-bounded local run can be proven without touching the full nightly set.
pub(crate) fn run_matrix(
    recipe_set_path: &Path,
    golden_dir: Option<&Path>,
    comparison_width: Option<u32>,
    selected: &[String],
) -> Result<MatrixReport, String> {
    let raw = fs::read_to_string(recipe_set_path)
        .map_err(|error| format!("read recipe set `{}`: {error}", recipe_set_path.display()))?;
    let set: RecipeSet = serde_json::from_str(&raw).map_err(|error| {
        format!(
            "invalid recipe set `{}`: {error}",
            recipe_set_path.display()
        )
    })?;
    if set.schema_version != MATRIX_SCHEMA_VERSION {
        return Err(format!(
            "recipe set `{}` has unsupported schema_version {} (expected {MATRIX_SCHEMA_VERSION})",
            recipe_set_path.display(),
            set.schema_version
        ));
    }
    if set.pipeline_version != MATRIX_PIPELINE_VERSION {
        return Err(format!(
            "recipe set `{}` targets pipeline_version `{}` (expected `{MATRIX_PIPELINE_VERSION}`)",
            recipe_set_path.display(),
            set.pipeline_version
        ));
    }
    if set.samples.is_empty() || set.recipes.is_empty() {
        return Err(format!(
            "recipe set `{}` declares no samples or no recipes",
            recipe_set_path.display()
        ));
    }
    for id in selected {
        if !set.recipes.iter().any(|recipe| &recipe.id == id) {
            return Err(format!(
                "unknown recipe id `{id}` (not in `{}`)",
                recipe_set_path.display()
            ));
        }
    }
    let base_dir = recipe_set_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let golden_dir = golden_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| base_dir.join("golden"));
    let comparison_width = comparison_width.unwrap_or(set.comparison_width);
    if comparison_width == 0 {
        return Err("comparison_width must be > 0".to_string());
    }

    let started = Instant::now();
    let mut pairs = Vec::new();
    for sample in &set.samples {
        let sample_path = if sample.path.is_absolute() {
            sample.path.clone()
        } else {
            base_dir.join(&sample.path)
        };
        let bytes = fs::read(&sample_path)
            .map_err(|error| format!("read sample `{}`: {error}", sample_path.display()))?;
        let name = sample_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("sample")
            .to_string();

        // One app per sample; the recipe changes between pairs, the decoded
        // source stays loaded. `load_bytes` is the synchronous headless decode
        // path used by the GUI tests (no window, no event loop).
        let mut app = LuminaApp::new(egui::Context::default());
        app.load_bytes(bytes, name)
            .map_err(|error| format!("load sample `{}`: {error}", sample_path.display()))?;
        // Golden contract (same as the CLI runner): the committed goldens are
        // generated with the shared pipeline and deliberately **no** Lensfun
        // auto-corrector, so the goldens stay feature-independent. The app
        // would otherwise resolve an EXIF profile; neutralise it here.
        app.loaded_lens_identity = None;
        #[cfg(feature = "lensfun")]
        {
            app.lensfun_cache = None;
        }

        for spec in &set.recipes {
            if !selected.is_empty() && !selected.contains(&spec.id) {
                continue;
            }
            let pair_started = Instant::now();
            let recipe: EditRecipe =
                serde_json::from_value(spec.recipe.clone()).map_err(|error| {
                    format!(
                        "recipe `{}` in `{}` is not a valid EditRecipe: {error}",
                        spec.id,
                        recipe_set_path.display()
                    )
                })?;
            let expected_route = spec.expected_route.label();
            app.recipe = recipe;
            app.render_key = None;

            let mut report = PairReport {
                sample: sample.id.clone(),
                recipe: spec.id.clone(),
                status: "pass",
                psnr_db: None,
                max_abs_diff: None,
                duration_ms: 0,
                expected_route: expected_route.clone(),
                message: None,
            };

            match render::full_resolution(&mut app) {
                Ok(()) => {}
                Err(error) => {
                    report.status = "error";
                    report.message = Some(format!("app render failed: {error}"));
                    report.duration_ms = pair_started.elapsed().as_millis();
                    pairs.push(report);
                    continue;
                }
            }
            let Some(preview) = app.preview() else {
                report.status = "error";
                report.message = Some("app produced no preview".to_string());
                report.duration_ms = pair_started.elapsed().as_millis();
                pairs.push(report);
                continue;
            };
            let comparison = match downscale_bilinear(preview, comparison_width) {
                Ok(comparison) => comparison,
                Err(error) => {
                    report.status = "error";
                    report.message = Some(format!("comparison downscale failed: {error}"));
                    report.duration_ms = pair_started.elapsed().as_millis();
                    pairs.push(report);
                    continue;
                }
            };

            let golden = golden_dir.join(format!("{}__{}.png", sample.id, spec.id));
            compare_against_golden(&mut report, &comparison, &golden, spec.tolerance);
            report.duration_ms = pair_started.elapsed().as_millis();
            pairs.push(report);
        }
    }

    Ok(MatrixReport {
        pairs,
        total_ms: started.elapsed().as_millis(),
    })
}

/// Verifies one downscaled preview against its golden with the tolerance class.
/// A missing golden, a dimension mismatch and a PSNR violation are all loud.
fn compare_against_golden(
    report: &mut PairReport,
    comparison: &ImageFrame,
    golden: &Path,
    tolerance: ToleranceClass,
) {
    if !golden.is_file() {
        report.status = "fail";
        report.message = Some(format!("missing golden `{}`", golden.display()));
        return;
    }
    let bytes = match fs::read(golden) {
        Ok(bytes) => bytes,
        Err(error) => {
            report.status = "fail";
            report.message = Some(format!(
                "golden `{}` could not be read: {error}",
                golden.display()
            ));
            return;
        }
    };
    let expected = match ImageFrame::decode(&bytes) {
        Ok(frame) => frame,
        Err(error) => {
            report.status = "fail";
            report.message = Some(format!(
                "golden `{}` could not be decoded: {error}",
                golden.display()
            ));
            return;
        }
    };
    if (expected.width, expected.height) != (comparison.width, comparison.height) {
        report.status = "fail";
        report.message = Some(format!(
            "golden dimension mismatch: golden {}x{} vs comparison {}x{}",
            expected.width, expected.height, comparison.width, comparison.height
        ));
        return;
    }
    let value = psnr(comparison, &expected);
    report.psnr_db = Some(value);
    report.max_abs_diff = Some(max_abs_diff(comparison, &expected));
    let ok = match tolerance.min_psnr_db() {
        None => value.is_infinite(),
        Some(min) => !value.is_nan() && value >= min,
    };
    if !ok {
        report.status = "fail";
        report.message = Some(match tolerance.min_psnr_db() {
            None => format!(
                "tolerance `exact` requires byte-identical output (PSNR = inf), got {:.2} dB",
                value
            ),
            Some(min) => format!(
                "tolerance `{}` requires PSNR >= {min} dB, got {:.2} dB (max abs diff {})",
                tolerance.label(),
                value,
                report.max_abs_diff.unwrap_or(0)
            ),
        });
    }
}

/// Maximum per-channel (RGB) absolute difference (diagnostic, matches the CLI).
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed `testdata/matrix/recipe-set.v1.json` must keep the schema
    /// this runner parses (fails loudly if the shared set drifts).
    #[test]
    fn committed_recipe_set_parses() {
        let path = matrix_dir().join("recipe-set.v1.json");
        let raw = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "committed recipe set `{}` unreadable: {error}",
                path.display()
            )
        });
        let set: RecipeSet = serde_json::from_str(&raw)
            .unwrap_or_else(|error| panic!("committed recipe set is not parseable: {error}"));
        assert_eq!(set.schema_version, MATRIX_SCHEMA_VERSION);
        assert_eq!(set.pipeline_version, MATRIX_PIPELINE_VERSION);
        assert_eq!(set.samples.len(), 2);
        assert_eq!(set.recipes.len(), 11);
        // Every recipe documents the goals + pipeline stages it covers.
        for recipe in &set.recipes {
            assert!(
                !recipe.goals.is_empty(),
                "recipe `{}` declares no goals",
                recipe.id
            );
            assert!(
                !recipe.stages.is_empty(),
                "recipe `{}` declares no stages",
                recipe.id
            );
        }
    }

    /// Full committed matrix through the app pipeline (both RAW samples, same
    /// goldens). RAW/fixture-dependent and slow → `#[ignore]` + env-gated,
    /// mirroring the CLI `real_matrix_against_committed_goldens`.
    ///
    /// `LUMINA_MATRIX_RECIPES` optionally bounds the run to a comma-separated
    /// recipe-id list (time-bounded local proof); unset runs the full set, as
    /// the nightly does.
    #[test]
    #[ignore]
    fn real_matrix_headless() {
        if std::env::var("LUMINA_MATRIX").ok().as_deref() != Some("1") {
            eprintln!("LUMINA_MATRIX=1 not set; skipping the headless GUI matrix");
            return;
        }
        let selected: Vec<String> = std::env::var("LUMINA_MATRIX_RECIPES")
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .map(|entry| entry.trim().to_string())
                    .filter(|entry| !entry.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let dir = matrix_dir();
        let report = run_matrix(&dir.join("recipe-set.v1.json"), None, None, &selected)
            .expect("headless GUI matrix must run");
        eprintln!("{}", report.summary());
        let failed = report.failed();
        assert!(
            failed.is_empty(),
            "{} of {} pair(s) failed:\n{}",
            failed.len(),
            report.pairs.len(),
            report.summary()
        );
    }
}
