//! LRPAR-G14-REDEYE-AUTO-15 (Release 2.0): end-to-end tests for the explicit
//! automatic red-pupil detection.
//!
//! These drive the **real binary** (clap parsing, exit codes included) and
//! verify the contract from `feature/architecture/pipeline.md` § G-14:
//!
//! * `red-eye --detect` is read-only (lists candidates, writes nothing),
//! * `red-eye --detect --detect-apply` persists `auto-re-` regions explicitly,
//! * `--detect-apply` without `--detect` is refused loudly (never implicit),
//! * an image without red pupils produces no regions and no write,
//! * the original image stays byte-identical.

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_sidecar::{load_sidecar, sidecar_path_for};
use std::fs;
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

/// Grey RGBA PNG with solid red rectangles at the given pixel bounds.
fn write_red_pupil_png(
    directory: &tempfile::TempDir,
    name: &str,
    width: u32,
    height: u32,
    pupils: &[(u32, u32, u32, u32)],
) -> std::path::PathBuf {
    let path = directory.path().join(name);
    let mut frame = ImageFrame::new(
        width,
        height,
        [120u8, 120, 120, 255]
            .iter()
            .copied()
            .cycle()
            .take((width * height * 4) as usize)
            .collect(),
    )
    .unwrap();
    for &(x0, y0, x1, y1) in pupils {
        for y in y0..y1 {
            for x in x0..x1 {
                let index = ((y * width + x) as usize) * 4;
                frame.pixels[index..index + 4].copy_from_slice(&[220, 30, 40, 255]);
            }
        }
    }
    fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    path
}

fn import(input: &std::path::Path) {
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

fn run_red_eye(input: &std::path::Path, extra: &[&str]) -> (bool, serde_json::Value, String) {
    let mut args = vec!["red-eye", "--input", input.to_str().unwrap(), "--json"];
    args.extend_from_slice(extra);
    let result = cli().args(&args).output().unwrap();
    let stderr = String::from_utf8_lossy(&result.stderr).to_string();
    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let payload: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or(serde_json::Value::Null);
    (result.status.success(), payload, stdout + &stderr)
}

#[test]
fn red_eye_detect_lists_readonly_and_applies_explicitly() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_red_pupil_png(&directory, "pupil-e2e.png", 64, 64, &[(30, 30, 36, 36)]);
    let original_bytes = fs::read(&input).unwrap();
    import(&input);
    let sidecar_path = sidecar_path_for(&input);

    // `--detect` alone: candidates listed, sidecar byte-identical, stage absent.
    let before = fs::read(&sidecar_path).unwrap();
    let (ok, payload, log) = run_red_eye(&input, &["--detect"]);
    assert!(ok, "{log}");
    let detected = payload["detected"].as_array().expect("detected array");
    assert_eq!(detected.len(), 1, "{payload}");
    assert_eq!(payload["dropped"], 0);
    assert!(detected[0]["id"].as_str().unwrap().starts_with("auto-re-"));
    assert_eq!(fs::read(&sidecar_path).unwrap(), before);
    assert!(load_sidecar(&sidecar_path).unwrap().virtual_copies[0]
        .recipe
        .red_eye
        .is_none());

    // Explicit apply persists exactly the listed candidate.
    let (ok, payload, log) = run_red_eye(&input, &["--detect", "--detect-apply"]);
    assert!(ok, "{log}");
    assert_eq!(payload["count"], 1);
    let document = load_sidecar(&sidecar_path).unwrap();
    let regions = &document.virtual_copies[0]
        .recipe
        .red_eye
        .as_ref()
        .expect("persisted red-eye stage")
        .regions;
    assert_eq!(regions.len(), 1);
    assert!(regions[0].id.starts_with("auto-re-"));
    assert_eq!(regions[0].desaturate, 0.8);
    assert_eq!(regions[0].darken, 0.4);
    assert_eq!(document.virtual_copies[0].history.len(), 1);
    assert_eq!(fs::read(&input).unwrap(), original_bytes);
}

#[test]
fn red_eye_detect_apply_requires_detect_loudly() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_red_pupil_png(&directory, "pupil-e2e.png", 64, 64, &[(30, 30, 36, 36)]);
    import(&input);
    let sidecar_path = sidecar_path_for(&input);
    let before = fs::read(&sidecar_path).unwrap();

    let (ok, _payload, log) = run_red_eye(&input, &["--detect-apply"]);
    assert!(!ok, "detect-apply without detect must fail: {log}");
    assert!(log.contains("requires --detect"), "{log}");
    assert_eq!(fs::read(&sidecar_path).unwrap(), before);
}

#[test]
fn red_eye_detect_without_pupils_writes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_red_pupil_png(&directory, "grey-e2e.png", 64, 64, &[]);
    import(&input);
    let sidecar_path = sidecar_path_for(&input);
    let before = fs::read(&sidecar_path).unwrap();

    let (ok, payload, log) = run_red_eye(&input, &["--detect"]);
    assert!(ok, "{log}");
    assert_eq!(payload["detected"].as_array().unwrap().len(), 0);

    let (ok, _payload, log) = run_red_eye(&input, &["--detect", "--detect-apply"]);
    assert!(ok, "{log}");
    assert_eq!(fs::read(&sidecar_path).unwrap(), before);
    let document = load_sidecar(&sidecar_path).unwrap();
    assert!(document.virtual_copies[0].recipe.red_eye.is_none());
    assert!(document.virtual_copies[0].history.is_empty());
}
