//! AUTO-TONE-CLI-6: end-to-end tests of the `process --auto-tone` surface.
//!
//! These drive the **real binary** (clap parsing, exit codes and the `--json`
//! report included) and pin the contract:
//!
//! * `process --auto-tone` persists all six sliders, all six mirrors and the
//!   analysis fingerprint, and renders the exact golden bytes,
//! * the resulting recipe is **fresh**, so `regenerate` reports
//!   `skipped`/`fresh` and leaves the sidecar byte-identical,
//! * a deliberately broken contract is regenerated again,
//! * the report shape and the exit codes are unchanged.

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_sidecar::{load_sidecar, save_sidecar, sidecar_path_for};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

/// The same documented pixel function as the unit tests
/// (`src/tests/support/auto_tone.rs`): 16x16 RGB,
/// `r = (13x + 5y + 1) % 179`, `g = (7x + 11y + 2) % 137`,
/// `b = (3x + 17y + 3) % 113`, alpha 255.
fn write_fixture(directory: &tempfile::TempDir, name: &str) -> PathBuf {
    let path = directory.path().join(name);
    let mut pixels = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16u32 {
        for x in 0..16u32 {
            pixels.push(((13 * x + 5 * y + 1) % 179) as u8);
            pixels.push(((7 * x + 11 * y + 2) % 137) as u8);
            pixels.push(((3 * x + 17 * y + 3) % 113) as u8);
            pixels.push(255);
        }
    }
    let frame = ImageFrame::new(16, 16, pixels).unwrap();
    fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    path
}

fn import(input: &Path) {
    let result = cli()
        .args(["import", "--input", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "import failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn run(args: &[&str]) -> (bool, String, String) {
    let result = cli().args(args).output().unwrap();
    (
        result.status.success(),
        String::from_utf8_lossy(&result.stdout).to_string(),
        String::from_utf8_lossy(&result.stderr).to_string(),
    )
}

/// `process` has no `--json` flag and prints nothing on success (the
/// machine-readable reports of this slice are `regenerate --json` and
/// `inspect --json`, both pinned below); the empty stdout is pinned here so a
/// report change would be visible.
fn run_process_auto_tone(input: &Path, output: &Path, extra: &[&str]) -> String {
    let mut args = vec![
        "process",
        "--input",
        input.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--auto-tone",
    ];
    args.extend_from_slice(extra);
    let (ok, stdout, stderr) = run(&args);
    assert!(ok, "process --auto-tone failed: {stderr}");
    assert_eq!(
        stdout, "",
        "the plain `process` report must stay unchanged (it is silent today)"
    );
    stdout
}

fn run_regenerate(input: &Path, extra: &[&str]) -> (serde_json::Value, String) {
    let mut args = vec!["regenerate", "--input", input.to_str().unwrap(), "--json"];
    args.extend_from_slice(extra);
    let (ok, stdout, stderr) = run(&args);
    assert!(ok, "regenerate failed: {stderr}");
    (
        serde_json::from_str(&stdout).unwrap_or_else(|error| panic!("invalid --json: {error}")),
        stderr,
    )
}

fn module_entry<'a>(payload: &'a serde_json::Value, module: &str) -> (&'a str, &'a str) {
    let entry = payload["modules"]
        .as_array()
        .expect("the report must carry the module array")
        .iter()
        .find(|entry| entry["module"].as_str() == Some(module))
        .unwrap_or_else(|| panic!("no `{module}` entry in {payload}"));
    (
        entry["action"].as_str().unwrap(),
        entry["reason"].as_str().unwrap(),
    )
}

const SLIDER_KEYS: [&str; 6] = [
    "exposure",
    "contrast",
    "whites",
    "blacks",
    "highlights",
    "shadows",
];

const MIRROR_KEYS: [&str; 6] = [
    "auto_exposure",
    "auto_contrast",
    "auto_whites",
    "auto_blacks",
    "auto_highlights",
    "auto_shadows",
];

/// One `process --auto-tone` run writes all six sliders, all six mirrors and
/// the analysis fingerprint, and the rendered image is byte-exact.
#[test]
fn process_auto_tone_persists_the_full_six_set_and_renders_the_golden() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_fixture(&directory, "at6-e2e.png");
    import(&input);
    let output = directory.path().join("at6-e2e-out.png");

    run_process_auto_tone(&input, &output, &[]);

    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let recipe = &document.virtual_copies[0].recipe;
    for key in SLIDER_KEYS {
        assert!(
            recipe.adjustments.contains_key(key),
            "`process --auto-tone` must persist the `{key}` slider"
        );
    }
    let serialized = document.to_json().unwrap();
    for mirror in MIRROR_KEYS {
        assert!(
            serialized.contains(&format!("\"{mirror}\"")),
            "`process --auto-tone` must persist the `{mirror}` mirror"
        );
    }
    assert!(
        serialized.contains("\"analysis_fingerprint\""),
        "`process --auto-tone` must persist the analysis fingerprint"
    );
    assert!(!fs::read(&output).unwrap().is_empty());

    // Byte-level golden of the render (exact RGBA bytes of the exported file).
    let bytes = fs::read(&output).unwrap();
    let rendered = ImageFrame::decode(&bytes).unwrap().pixels;
    assert_eq!(
        rendered
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        AUTO_TONE_GOLDEN_PIXELS.trim(),
        "the `process --auto-tone` render must stay byte-exact"
    );
}

/// The central behavioural pin through the real binary: after
/// `process --auto-tone` the recipe is fresh, so the collective `regenerate`
/// reports `skipped`/`fresh`, `status: "unchanged"` and the sidecar bytes are
/// untouched.
#[test]
fn regenerate_is_skipped_as_fresh_after_process_auto_tone() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_fixture(&directory, "at6-fresh.png");
    import(&input);
    let output = directory.path().join("at6-fresh-out.png");
    run_process_auto_tone(&input, &output, &[]);
    let sidecar = sidecar_path_for(&input);
    let before = fs::read(&sidecar).unwrap();

    let (payload, _stderr) = run_regenerate(&input, &[]);
    assert_eq!(payload["command"], "regenerate");
    assert_eq!(payload["status"], "unchanged");
    assert_eq!(module_entry(&payload, "auto-tone"), ("skipped", "fresh"));
    assert_eq!(
        fs::read(&sidecar).unwrap(),
        before,
        "a fresh Auto-Tone recipe must not be rewritten"
    );

    // An explicit `--module auto-tone` still regenerates (unchanged report
    // shape, `generated`/`explicit`).
    let (payload, _stderr) = run_regenerate(&input, &["--module", "auto-tone"]);
    assert_eq!(payload["status"], "updated");
    assert_eq!(
        module_entry(&payload, "auto-tone"),
        ("generated", "explicit")
    );
}

/// The other direction: a deliberately removed slider makes the recipe stale
/// and the collective `regenerate` regenerates it (`stale-or-missing`).
#[test]
fn a_broken_contract_is_regenerated_by_the_collective_run() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_fixture(&directory, "at6-stale.png");
    import(&input);
    let output = directory.path().join("at6-stale-out.png");
    run_process_auto_tone(&input, &output, &[]);
    let sidecar = sidecar_path_for(&input);
    let mut document = load_sidecar(&sidecar).unwrap();
    document.virtual_copies[0].recipe.auto_features.auto_shadows = None;
    save_sidecar(&sidecar, &document).unwrap();

    let (payload, _stderr) = run_regenerate(&input, &[]);
    assert_eq!(payload["status"], "updated");
    assert_eq!(
        module_entry(&payload, "auto-tone"),
        ("generated", "stale-or-missing")
    );
    let after = load_sidecar(&sidecar).unwrap();
    assert!(
        after.virtual_copies[0]
            .recipe
            .auto_features
            .auto_shadows
            .is_some(),
        "the missing mirror must be regenerated"
    );
}

/// The six explicit slider flags exist on `process` (clap surface) and reach
/// the recipe; an out-of-range value is still a loud usage error with no
/// output.
#[test]
fn the_six_explicit_slider_flags_are_accepted_and_validated() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_fixture(&directory, "at6-flags.png");
    import(&input);
    let output = directory.path().join("at6-flags-out.png");
    run_process_auto_tone(
        &input,
        &output,
        &[
            "--exposure",
            "1.0",
            "--contrast",
            "0.1",
            "--whites",
            "0.2",
            "--blacks",
            "0.3",
            "--highlights",
            "0.4",
            "--shadows",
            "0.5",
        ],
    );

    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    let recipe = &document.virtual_copies[0].recipe;
    for (key, expected) in [
        ("exposure", 1.0),
        ("contrast", 0.1),
        ("whites", 0.2),
        ("blacks", 0.3),
        ("highlights", 0.4),
        ("shadows", 0.5),
    ] {
        assert_eq!(
            recipe.adjustments.get(key).copied(),
            Some(expected),
            "`--{key}` must win over the auto value"
        );
    }
    // The mirrors still document the auto values, not the explicit ones.
    let serialized = document.to_json().unwrap();
    for mirror in MIRROR_KEYS {
        assert!(
            serialized.contains(&format!("\"{mirror}\"")),
            "mirror `{mirror}` must stay present after explicit overrides"
        );
    }
    assert!(
        !serialized.contains("\"auto_whites\":0.2"),
        "an explicit value must never be adopted as the auto value"
    );

    // Out of range is a loud error, no output file, exit code 1.
    let bad = directory.path().join("at6-bad.png");
    let (ok, stdout, stderr) = run(&[
        "process",
        "--input",
        input.to_str().unwrap(),
        "--output",
        bad.to_str().unwrap(),
        "--auto-tone",
        "--whites",
        "1.5",
    ]);
    assert!(!ok, "an out-of-range slider must fail: {stdout}");
    assert!(
        stderr.contains("invalid whites"),
        "stderr must name the invalid slider: {stderr}"
    );
    assert!(
        !bad.exists(),
        "a rejected run must not write an output file"
    );
}

/// The exit-code contract is unchanged: an unknown flag is clap's usage error
/// (2), an invalid value is a runtime error (1).
#[test]
fn the_exit_codes_are_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_fixture(&directory, "at6-exit.png");
    import(&input);
    let output = directory.path().join("at6-exit-out.png");
    run_process_auto_tone(&input, &output, &[]);

    let result = cli()
        .args([
            "process",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--nonsense",
        ])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2), "clap usage errors exit 2");

    let result = cli()
        .args([
            "process",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--target-luminance",
            "2.0",
            "--auto-tone",
        ])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1), "runtime errors exit 1");
}

/// `render` and `export` pass `auto_tone: false` and must be untouched by this
/// slice: the exported image is the unchanged HEAD render of the same fixture
/// (pinned pixel-for-pixel in `src/tests/auto_tone_process.rs`) and its file
/// bytes are byte-identical to the `render` output.
#[test]
fn render_and_export_are_byte_identical_without_auto_tone() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_fixture(&directory, "at6-plain.png");
    import(&input);
    let rendered = directory.path().join("at6-plain-render.png");
    let exported = directory.path().join("at6-plain-export.png");
    let (ok, _stdout, stderr) = run(&[
        "render",
        "--input",
        input.to_str().unwrap(),
        "--output",
        rendered.to_str().unwrap(),
    ]);
    assert!(ok, "render failed: {stderr}");
    let (ok, _stdout, stderr) = run(&[
        "export",
        "--input",
        input.to_str().unwrap(),
        "--output",
        exported.to_str().unwrap(),
        "--format",
        "png",
    ]);
    assert!(ok, "export failed: {stderr}");

    assert_eq!(
        fs::read(&exported).unwrap(),
        fs::read(&rendered).unwrap(),
        "export and render of the same recipe must produce the same file bytes"
    );
    let pixels = ImageFrame::decode(&fs::read(&exported).unwrap())
        .unwrap()
        .pixels;
    assert_eq!(
        pixels
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        include_str!("../src/tests/plain_render_golden.txt").trim(),
        "the exported image must be the unchanged HEAD render"
    );
    // Neither run wrote a single `auto_features` mirror.
    let serialized = load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .to_json()
        .unwrap();
    for mirror in MIRROR_KEYS {
        assert!(
            !serialized.contains(&format!(r#""{mirror}""#)),
            "`export` must not write the `{mirror}` mirror without `--auto-tone`"
        );
    }
    assert!(!serialized.contains(r#""analysis_fingerprint""#));
}

/// The `--auto-tone` help text advertises the six slider flags.
#[test]
fn the_process_help_lists_the_six_slider_flags() {
    let (ok, stdout, _stderr) = run(&["process", "--help"]);
    assert!(ok);
    for flag in [
        "--exposure",
        "--contrast",
        "--whites",
        "--blacks",
        "--highlights",
        "--shadows",
        "--auto-tone",
        "--match-total-exposure",
    ] {
        assert!(stdout.contains(flag), "help must list `{flag}`: {stdout}");
    }
}

/// Byte-level golden of the `process --auto-tone` render of the documented
/// fixture. Computed from the real pipeline; see the AUTO-TONE-CLI-6 report.
const AUTO_TONE_GOLDEN_PIXELS: &str = include_str!("../src/tests/auto_tone_render_golden.txt");
