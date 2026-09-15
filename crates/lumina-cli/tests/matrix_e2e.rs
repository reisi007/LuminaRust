//! LRPAR-MATRIX-RECIPE (Slice 1): hermetic end-to-end tests for `lumina matrix`.
//!
//! SOLL: `feature/quality/conflicts-and-acceptance.md` § „Rezept-Matrix".
//! These tests drive the built binary against synthetic PNG samples in a
//! tempdir (no network, no absolute fixture paths, no RAW dependency) and pin
//! the runner's exit codes:
//!
//! - baseline writes goldens and exits 0,
//! - a green verify run exits 0,
//! - a missing golden, a PSNR/tolerance violation and an `exact` mismatch are
//!   loud (non-zero + reason),
//! - an artifact-backed recipe and an unknown recipe id are rejected loudly.
//!
//! The real CR3 matrix (both committed samples + committed goldens) is
//! env-gated (`LUMINA_MATRIX=1`) and `#[ignore]`d, mirroring the
//! `LUMINA_RAW_FIXTURE` convention: it is RAW/fixture-dependent and does not
//! run in the fast standard `cargo test`.

use lumina_core::{ImageFileFormat, ImageFrame};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

/// A small deterministic gradient sample (width 64, height 48) so tone and
/// geometry stages have something to change.
fn write_sample_png(directory: &tempfile::TempDir) -> PathBuf {
    let path = directory.path().join("sample.png");
    let (width, height) = (64u32, 48u32);
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            pixels.push((x * 4) as u8);
            pixels.push((y * 5) as u8);
            pixels.push(((x + y) * 2) as u8);
            pixels.push(255);
        }
    }
    let frame = ImageFrame::new(width, height, pixels).unwrap();
    fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    path
}

/// Writes a minimal single-sample recipe set. `recipes` is the raw JSON array
/// of recipe entries (so each test can vary tolerance/recipe content).
fn write_recipe_set(directory: &tempfile::TempDir, recipes: &str) -> PathBuf {
    let path = directory.path().join("recipe-set.json");
    let json = format!(
        r#"{{
  "schema_version": 1,
  "pipeline_version": "raster-mvp-1",
  "comparison_width": 48,
  "samples": [{{ "id": "sample", "path": "sample.png" }}],
  "recipes": {recipes}
}}"#
    );
    fs::write(&path, json).unwrap();
    path
}

fn golden_path(directory: &tempfile::TempDir, recipe: &str) -> PathBuf {
    directory
        .path()
        .join("golden")
        .join(format!("sample__{recipe}.png"))
}

fn run_matrix(recipe_set: &Path, extra: &[&str]) -> Output {
    let mut command = cli();
    command.arg("matrix").arg("--recipe-set").arg(recipe_set);
    command.args(extra);
    command.output().unwrap()
}

const IDENTITY_RECIPE: &str = r#"[
  {
    "id": "identity",
    "goals": ["G-01"],
    "stages": ["decode", "output"],
    "tolerance": "exact",
    "recipe": { "adjustments": {} }
  }
]"#;

const TONE_RECIPE: &str = r#"[
  {
    "id": "tone",
    "goals": ["G-01"],
    "stages": ["exposure", "contrast", "highlights", "shadows"],
    "tolerance": "strict",
    "recipe": {
      "adjustments": {
        "exposure": 0.4,
        "contrast": 0.2,
        "highlights": -0.2,
        "shadows": 0.25
      }
    }
  }
]"#;

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn baseline_writes_goldens_and_verify_is_green() {
    let directory = tempfile::tempdir().unwrap();
    write_sample_png(&directory);
    let recipe_set = write_recipe_set(&directory, IDENTITY_RECIPE);

    let baseline = run_matrix(&recipe_set, &["--update-goldens", "--json"]);
    assert!(
        baseline.status.success(),
        "baseline stderr: {}",
        stderr(&baseline)
    );
    assert!(golden_path(&directory, "identity").is_file());
    assert!(stdout(&baseline).contains("\"mode\":\"baseline\""));

    let verify = run_matrix(&recipe_set, &[]);
    assert!(
        verify.status.success(),
        "verify stderr: {}",
        stderr(&verify)
    );
    assert!(stdout(&verify).contains("1 passed, 0 failed"));
}

#[test]
fn missing_golden_is_loud_and_non_zero() {
    let directory = tempfile::tempdir().unwrap();
    write_sample_png(&directory);
    let recipe_set = write_recipe_set(&directory, TONE_RECIPE);

    let verify = run_matrix(&recipe_set, &[]);
    assert!(!verify.status.success());
    assert!(
        stderr(&verify).contains("missing golden"),
        "stderr: {}",
        stderr(&verify)
    );
}

#[test]
fn psnr_tolerance_violation_is_loud() {
    let directory = tempfile::tempdir().unwrap();
    write_sample_png(&directory);
    let recipe_set = write_recipe_set(&directory, TONE_RECIPE);

    let baseline = run_matrix(&recipe_set, &["--update-goldens"]);
    assert!(baseline.status.success(), "stderr: {}", stderr(&baseline));

    // Replace the golden with a visibly different image (inverted golden):
    // the tolerance gate must fail, not silently pass.
    let golden = golden_path(&directory, "tone");
    let bytes = fs::read(&golden).unwrap();
    let mut frame = ImageFrame::decode(&bytes).unwrap();
    for (index, byte) in frame.pixels.iter_mut().enumerate() {
        if index % 4 != 3 {
            *byte = 255 - *byte;
        }
    }
    fs::write(&golden, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();

    let verify = run_matrix(&recipe_set, &[]);
    assert!(!verify.status.success());
    let message = stderr(&verify);
    assert!(
        message.contains("requires PSNR"),
        "expected a PSNR tolerance failure, stderr: {message}"
    );
}

#[test]
fn exact_tolerance_requires_byte_identity() {
    let directory = tempfile::tempdir().unwrap();
    write_sample_png(&directory);
    let recipe_set = write_recipe_set(&directory, IDENTITY_RECIPE);

    let baseline = run_matrix(&recipe_set, &["--update-goldens"]);
    assert!(baseline.status.success(), "stderr: {}", stderr(&baseline));

    // One LSB in a single pixel is still a finite PSNR and must fail `exact`.
    let golden = golden_path(&directory, "identity");
    let bytes = fs::read(&golden).unwrap();
    let mut frame = ImageFrame::decode(&bytes).unwrap();
    frame.pixels[0] = frame.pixels[0].wrapping_add(1);
    fs::write(&golden, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();

    let verify = run_matrix(&recipe_set, &[]);
    assert!(!verify.status.success());
    assert!(
        stderr(&verify).contains("exact"),
        "stderr: {}",
        stderr(&verify)
    );
}

#[test]
fn artifact_backed_recipe_is_rejected_loudly() {
    let directory = tempfile::tempdir().unwrap();
    write_sample_png(&directory);
    let recipe_set = write_recipe_set(
        &directory,
        r#"[
  {
    "id": "with-depth",
    "goals": ["G-05"],
    "stages": ["lens_blur"],
    "tolerance": "robust",
    "recipe": {
      "lens_blur": {
        "version": 1,
        "enabled": true,
        "focus_rect": { "x": 0.2, "y": 0.2, "width": 0.4, "height": 0.4 },
        "focal_near": 0.0,
        "focal_far": 0.2,
        "blur_amount": 0.5,
        "bokeh": "round",
        "depth_artifact": { "relative_path": "depth.bin", "sha256": "abc" }
      }
    }
  }
]"#,
    );

    let run = run_matrix(&recipe_set, &["--update-goldens"]);
    assert!(!run.status.success());
    assert!(
        stderr(&run).contains("depth_artifact"),
        "stderr: {}",
        stderr(&run)
    );
}

#[test]
fn unknown_recipe_filter_is_loud() {
    let directory = tempfile::tempdir().unwrap();
    write_sample_png(&directory);
    let recipe_set = write_recipe_set(&directory, IDENTITY_RECIPE);

    let run = run_matrix(&recipe_set, &["--recipe", "does-not-exist"]);
    assert!(!run.status.success());
    assert!(
        stderr(&run).contains("unknown recipe id"),
        "stderr: {}",
        stderr(&run)
    );
}

/// F2 (typo protection): an unknown top-level recipe key deserializes into
/// `extras` and would silently render as identity; the matrix rejects it.
#[test]
fn unknown_recipe_key_is_rejected_loudly() {
    let directory = tempfile::tempdir().unwrap();
    write_sample_png(&directory);
    let recipe_set = write_recipe_set(
        &directory,
        r#"[
  {
    "id": "typo",
    "goals": ["G-06"],
    "stages": ["geometry"],
    "tolerance": "standard",
    "recipe": {
      "geometri": { "version": 1, "rotation_degrees": 1.0 }
    }
  }
]"#,
    );

    let run = run_matrix(&recipe_set, &["--update-goldens"]);
    assert!(!run.status.success());
    assert!(
        stderr(&run).contains("unknown top-level key"),
        "stderr: {}",
        stderr(&run)
    );
}

/// The real CR3 matrix against the committed recipe set and goldens. Env-gated
/// (`LUMINA_MATRIX=1`) and ignored by default: RAW decode + full-resolution
/// renders are expensive and fixture-dependent. Runs a bounded, fast recipe
/// subset (the full set is run via the release binary / the nightly); see
/// `LUMINA_MATRIX=1 cargo test -p lumina-cli --test matrix_e2e -- --ignored`.
#[test]
#[ignore]
fn real_matrix_against_committed_goldens() {
    if std::env::var("LUMINA_MATRIX").ok().as_deref() != Some("1") {
        eprintln!("LUMINA_MATRIX=1 not set; skipping real CR3 matrix test");
        return;
    }
    let run = cli()
        .args([
            "matrix",
            // Bounded subset: no heavy spatial stages (Debugs builds of the
            // 24 MP Dehaze/Detail/Lens-Blur pairs are minutes long).
            "--recipe",
            "g01-identity",
            "--recipe",
            "g01-tone-wb",
        ])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "real matrix failed\nstdout: {}\nstderr: {}",
        stdout(&run),
        stderr(&run)
    );
    assert!(stdout(&run).contains("0 failed"));
}
