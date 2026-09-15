//! LRPAR-MATRIX-RECIPE (Slice 2): hermetic tests for the CLI recipe-set path
//! resolution chain.
//!
//! SOLL: `feature/quality/conflicts-and-acceptance.md` § „Rezept-Matrix" →
//! „Pfadauflösung (Slice 2)": `--recipe-set` > `LUMINA_MATRIX_RECIPE_SET` >
//! `LUMINA_MATRIX_DIR/recipe-set.v1.json` > ancestor walk from the current
//! directory to the workspace-root `testdata/matrix/recipe-set.v1.json`.
//!
//! Environment/parallelism hygiene: the child process environment is set with
//! [`Command::env`]/[`Command::env_remove`] (never `std::env::set_var`), so the
//! tests mutate no process-global state and cannot pollute each other or other
//! test binaries. The resolved path is observed through the loud
//! `unknown recipe id` error the runner emits right after loading the set — no
//! render, no RAW fixture, no network, and no dependence on the host's ambient
//! `LUMINA_MATRIX_*` variables.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

/// A minimal, valid recipe set. It is never rendered: every run below aborts at
/// the recipe filter. `sample.path` may point at a non-existent relative file —
/// the set loader validates the shape, not the sample's existence.
fn write_set(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(
        path,
        r#"{
  "schema_version": 1,
  "pipeline_version": "raster-mvp-1",
  "comparison_width": 48,
  "samples": [{ "id": "sample", "path": "sample.png" }],
  "recipes": [
    {
      "id": "only",
      "goals": ["G-01"],
      "stages": ["decode"],
      "tolerance": "standard",
      "expected_route": "gpu",
      "recipe": { "adjustments": {} }
    }
  ]
}"#,
    )
    .unwrap();
}

/// Runs `lumina matrix` **without** `--recipe-set` and with a guaranteed-unknown
/// recipe filter: the runner resolves the recipe-set path, loads it, and then
/// fails loudly with the resolved path in the message. The child env/cwd is
/// fully controlled (`set` overrides ambient values, `remove` clears the rest).
fn resolved_path_error(envs: &[(&str, &Path)], remove: &[&str], cwd: Option<&Path>) -> Output {
    let mut command = cli();
    command.arg("matrix").arg("--recipe").arg("__unknown__");
    for (key, value) in envs {
        command.env(key, value);
    }
    for key in remove {
        command.env_remove(key);
    }
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    command.output().unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

/// `LUMINA_MATRIX_RECIPE_SET` names an explicit file and wins over
/// `LUMINA_MATRIX_DIR` (a valid set exists in both locations, so only the
/// priority can decide).
#[test]
fn recipe_set_env_wins_over_matrix_dir() {
    let directory = tempfile::tempdir().unwrap();
    let explicit = directory.path().join("explicit-set.json");
    let matrix_dir = directory.path().join("matrix-dir");
    write_set(&explicit);
    write_set(&matrix_dir.join("recipe-set.v1.json"));

    let output = resolved_path_error(
        &[
            ("LUMINA_MATRIX_RECIPE_SET", explicit.as_path()),
            ("LUMINA_MATRIX_DIR", matrix_dir.as_path()),
        ],
        &[],
        None,
    );
    assert!(!output.status.success());
    let message = stderr(&output);
    assert!(
        message.contains(&explicit.display().to_string()),
        "explicit recipe-set env must win, stderr: {message}"
    );
    assert!(
        !message.contains(&matrix_dir.display().to_string()),
        "LUMINA_MATRIX_DIR must be ignored when LUMINA_MATRIX_RECIPE_SET is set, stderr: {message}"
    );
}

/// `LUMINA_MATRIX_DIR` resolves `<dir>/recipe-set.v1.json` (the documented
/// default file name).
#[test]
fn matrix_dir_env_uses_default_recipe_set_name() {
    let directory = tempfile::tempdir().unwrap();
    let matrix_dir = directory.path().join("matrix-dir");
    let expected = matrix_dir.join("recipe-set.v1.json");
    write_set(&expected);

    let output = resolved_path_error(
        &[("LUMINA_MATRIX_DIR", matrix_dir.as_path())],
        &["LUMINA_MATRIX_RECIPE_SET"],
        None,
    );
    assert!(!output.status.success());
    let message = stderr(&output);
    assert!(
        message.contains(&expected.display().to_string()),
        "stderr: {message}"
    );
}

/// With neither env variable set, the runner walks up from the current
/// directory to the workspace root and finds the committed
/// `testdata/matrix/recipe-set.v1.json`. The crate directory is nested below
/// the workspace root, so the walk (not the crate path) must be the hit.
#[test]
fn ancestor_walk_finds_workspace_testdata_matrix() {
    let output = resolved_path_error(
        &[],
        &["LUMINA_MATRIX_RECIPE_SET", "LUMINA_MATRIX_DIR"],
        Some(Path::new(env!("CARGO_MANIFEST_DIR"))),
    );
    assert!(!output.status.success(), "stderr: {}", stderr(&output));
    let message = stderr(&output);
    assert!(
        message.contains("testdata/matrix/recipe-set.v1.json"),
        "stderr: {message}"
    );
    assert!(
        !message.contains("crates/lumina-cli/testdata"),
        "the walk must resolve to the workspace root, not the crate dir, stderr: {message}"
    );
}
