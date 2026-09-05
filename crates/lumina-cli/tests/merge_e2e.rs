//! LRPAR-G13-MERGE-15 / MERGE-CLI-1: end-to-end tests for `merge-hdr` and
//! `merge-pano` through the built CLI binary.
//!
//! SOLL: `feature/decisions/LRPAR-G13-MERGE-15.md` (§Abnahme CLI) and
//! `feature/platform/cli-gui-wasm.md` (§ „HDR-/Panorama-Merge").
//! Fixtures are generated PNGs (no user photos, no network); every run
//! asserts exit codes, the DNG + merge-sidecar bundle, loud
//! `missing`/`stale`/`unsupported` failures and byte-identical originals.

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_sidecar::{load_sidecar, sidecar_path_for};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

const W: u32 = 32;
const H: u32 = 24;

/// Non-periodic gradient fixture (unique alignment minimum, no wrap-around
/// ambiguity): `v = (x * 0.7 + y * 3.1) / 128`, optionally scaled and
/// shifted right by `shift_x` (vacated columns stay black).
fn gradient_png(dir: &tempfile::TempDir, name: &str, scale: f32, shift_x: u32) -> PathBuf {
    let mut rgba = Vec::with_capacity(W as usize * H as usize * 4);
    for y in 0..H {
        for x in 0..W {
            let sx = x.saturating_sub(shift_x);
            let v = if x < shift_x && shift_x > 0 {
                0.0
            } else {
                (sx as f32 * 0.7 + y as f32 * 3.1) / 128.0 * scale
            };
            let b = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            rgba.extend_from_slice(&[b, b, b, 255]);
        }
    }
    let path = dir.path().join(name);
    let frame = ImageFrame::new(W, H, rgba).unwrap();
    fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    path
}

fn solid_png(dir: &tempfile::TempDir, name: &str, w: u32, h: u32, v: u8) -> PathBuf {
    let path = dir.path().join(name);
    let frame = ImageFrame::new(w, h, [v, v, v, 255].repeat(w as usize * h as usize)).unwrap();
    fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    path
}

fn run_merge(dir: &tempfile::TempDir, command: &str, extra: &[&str]) -> std::process::Output {
    let mut inputs: Vec<PathBuf> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("png"))
        .collect();
    // Deterministic order: exposure lists map positionally onto `--input`.
    inputs.sort();
    let mut cmd = cli();
    cmd.arg(command);
    for input in &inputs {
        cmd.arg("--input").arg(input);
    }
    for arg in extra {
        cmd.arg(arg);
    }
    cmd.output().unwrap()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn blake3_file(path: &Path) -> String {
    format!("blake3:{}", blake3::hash(&fs::read(path).unwrap()).to_hex())
}

/// The sidecar JSON must carry no absolute paths: the bundle stays valid
/// when moved as a whole.
fn assert_no_absolute_paths(sidecar_json: &str, dir: &Path) {
    assert!(
        !sidecar_json.contains(&dir.to_string_lossy().into_owned()),
        "sidecar must not contain the tempdir path"
    );
    let value: serde_json::Value = serde_json::from_str(sidecar_json).unwrap();
    let mut paths = Vec::new();
    collect_path_fields(&value, &mut paths);
    assert!(!paths.is_empty(), "expected path fields in merge sidecar");
    for path in paths {
        assert!(
            !path.starts_with('/') && !path.contains('\\') && !path.contains(':'),
            "absolute path in merge sidecar: `{path}`"
        );
    }
}

fn collect_path_fields(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                if key == "path"
                    || key == "file"
                    || key == "relative_path"
                    || key == "relative_name"
                {
                    if let Some(text) = val.as_str() {
                        out.push(text.to_string());
                    }
                } else {
                    collect_path_fields(val, out);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_path_fields(item, out);
            }
        }
        _ => {}
    }
}

#[test]
fn merge_hdr_produces_dng_bundle_and_reimportable_dng() {
    let dir = tempfile::tempdir().unwrap();
    let a = gradient_png(&dir, "a.png", 1.0, 0);
    let b = gradient_png(&dir, "b.png", 0.5, 0);
    let hash_a = blake3_file(&a);
    let hash_b = blake3_file(&b);

    let output = run_merge(
        &dir,
        "merge-hdr",
        &[
            "--exposure-times",
            "0.01,0.02",
            "--isos",
            "100,100",
            "--f-numbers",
            "8,8",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let dng = dir.path().join("a-HDR.dng");
    assert!(dng.is_file(), "deterministic default DNG name");
    let sidecar = sidecar_path_for(&dng);
    assert!(sidecar.is_file(), "merge sidecar bundle");

    // Originals byte-identical.
    assert_eq!(blake3_file(&a), hash_a);
    assert_eq!(blake3_file(&b), hash_b);

    // Envelope + recipe + artifact.
    let document = load_sidecar(&sidecar).expect("merge sidecar loads");
    assert_eq!(
        document.extras.get("type"),
        Some(&serde_json::Value::from("merge"))
    );
    let recipe_value = document.extras.get("merge_recipe").expect("merge_recipe");
    assert_eq!(recipe_value["mode"], serde_json::Value::from("hdr"));
    assert_eq!(recipe_value["merge_version"], serde_json::Value::from(1));
    assert_eq!(recipe_value["status"], serde_json::Value::from("ok"));
    let sources = recipe_value["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 2);
    let mut names: Vec<&str> = sources
        .iter()
        .map(|s| s["path"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(names, vec!["a.png", "b.png"]);
    for source in sources {
        assert!(source["content_hash"]
            .as_str()
            .unwrap()
            .starts_with("blake3:"));
    }
    assert_eq!(
        recipe_value["output"]["file"],
        serde_json::Value::from("a-HDR.dng")
    );
    let artifact = document
        .extras
        .get("merge_artifact")
        .expect("merge_artifact");
    assert_eq!(artifact["format"], serde_json::Value::from("dng"));
    assert_eq!(artifact["channels"], serde_json::Value::from("rgb16"));
    assert_eq!(artifact["width"], serde_json::Value::from(W));
    assert_eq!(artifact["height"], serde_json::Value::from(H));
    assert_eq!(
        artifact["checksum"],
        serde_json::Value::from(blake3_file(&dng))
    );

    // Standard copy with its own recipe (Agents.md virtual-copy rule).
    assert!(document
        .virtual_copies
        .iter()
        .any(|copy| copy.id == "vc-original" && copy.is_default));

    assert_no_absolute_paths(&fs::read_to_string(&sidecar).unwrap(), dir.path());

    // Re-import: the DNG decodes via lumina-raw with merged geometry.
    let decoded = lumina_raw::decode_bytes(&fs::read(&dng).unwrap(), "a-HDR.dng").unwrap();
    assert_eq!((decoded.frame.width, decoded.frame.height), (W, H));
}

#[test]
fn merge_pano_produces_wider_dng() {
    let dir = tempfile::tempdir().unwrap();
    gradient_png(&dir, "a.png", 1.0, 0);
    gradient_png(&dir, "b.png", 1.0, 4);

    let output = run_merge(&dir, "merge-pano", &["--blend-width-px", "4"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let dng = dir.path().join("a-Pano.dng");
    assert!(dng.is_file());
    let decoded = lumina_raw::decode_bytes(&fs::read(&dng).unwrap(), "a-Pano.dng").unwrap();
    let document = load_sidecar(&sidecar_path_for(&dng)).unwrap();
    assert_eq!(
        document.extras["merge_recipe"]["mode"],
        serde_json::Value::from("panorama")
    );
    // End-to-end wiring pin: the DNG canvas equals the single-frame width
    // plus the estimated offset stored in the recipe transform (the
    // cylindrical estimator may report a smaller shift than the linear
    // pixel shift; what matters is that sidecar and DNG agree).
    let matrix = document.extras["merge_recipe"]["alignment"]["transforms"][0]["matrix_3x3"]
        .as_array()
        .unwrap();
    let dx = matrix[2].as_f64().unwrap().round() as i32;
    assert!(dx.abs() > 0, "an offset was estimated, got {matrix:?}");
    assert_eq!(
        (decoded.frame.width, decoded.frame.height),
        (W + dx.unsigned_abs(), H),
        "side-by-side canvas grows by the estimated offset"
    );
    assert_no_absolute_paths(
        &fs::read_to_string(sidecar_path_for(&dng)).unwrap(),
        dir.path(),
    );
}

#[test]
fn merge_is_idempotent_without_force() {
    let dir = tempfile::tempdir().unwrap();
    gradient_png(&dir, "a.png", 1.0, 0);
    gradient_png(&dir, "b.png", 0.5, 0);
    let args = [
        "--exposure-times",
        "0.01,0.02",
        "--isos",
        "100,100",
        "--f-numbers",
        "8,8",
    ];
    assert!(run_merge(&dir, "merge-hdr", &args).status.success());
    let dng = dir.path().join("a-HDR.dng");
    let before = fs::read(&dng).unwrap();

    let second = run_merge(&dir, "merge-hdr", &args);
    assert!(second.status.success(), "stderr: {}", stderr(&second));
    assert!(
        stderr(&second).contains("already current"),
        "idempotent run says so, got: {}",
        stderr(&second)
    );
    assert_eq!(fs::read(&dng).unwrap(), before, "no rewrite when current");
}

#[test]
fn merge_stale_after_source_change_and_force_regenerates() {
    let dir = tempfile::tempdir().unwrap();
    gradient_png(&dir, "a.png", 1.0, 0);
    let b = gradient_png(&dir, "b.png", 0.5, 0);
    let args = [
        "--exposure-times",
        "0.01,0.02",
        "--isos",
        "100,100",
        "--f-numbers",
        "8,8",
    ];
    assert!(run_merge(&dir, "merge-hdr", &args).status.success());

    // Change one source: digest mismatch must be loud, never silent.
    gradient_png(&dir, "b.png", 0.75, 0);
    assert_ne!(fs::read(&b).unwrap().len(), 0);
    let stale = run_merge(&dir, "merge-hdr", &args);
    assert!(!stale.status.success());
    assert!(
        stderr(&stale).contains("merge stale"),
        "got: {}",
        stderr(&stale)
    );

    let forced = cli()
        .arg("merge-hdr")
        .arg("--input")
        .arg(dir.path().join("a.png"))
        .arg("--input")
        .arg(dir.path().join("b.png"))
        .args(args)
        .arg("--force")
        .output()
        .unwrap();
    assert!(forced.status.success(), "stderr: {}", stderr(&forced));
}

#[test]
fn merge_missing_dng_reports_missing_and_force_regenerates() {
    let dir = tempfile::tempdir().unwrap();
    gradient_png(&dir, "a.png", 1.0, 0);
    gradient_png(&dir, "b.png", 1.0, 4);
    assert!(run_merge(&dir, "merge-pano", &[]).status.success());

    fs::remove_file(dir.path().join("a-Pano.dng")).unwrap();
    let missing = run_merge(&dir, "merge-pano", &[]);
    assert!(!missing.status.success());
    assert!(
        stderr(&missing).contains("merge missing"),
        "got: {}",
        stderr(&missing)
    );

    let forced = cli()
        .arg("merge-pano")
        .arg("--input")
        .arg(dir.path().join("a.png"))
        .arg("--input")
        .arg(dir.path().join("b.png"))
        .arg("--force")
        .output()
        .unwrap();
    assert!(forced.status.success(), "stderr: {}", stderr(&forced));
}

#[test]
fn merge_missing_source_is_loud() {
    let dir = tempfile::tempdir().unwrap();
    gradient_png(&dir, "a.png", 1.0, 0);
    let output = cli()
        .arg("merge-hdr")
        .arg("--input")
        .arg(dir.path().join("a.png"))
        .arg("--input")
        .arg(dir.path().join("gone.png"))
        .arg("--exposure-times")
        .arg("0.01,0.02")
        .arg("--isos")
        .arg("100,100")
        .arg("--f-numbers")
        .arg("8,8")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("merge missing"),
        "got: {}",
        stderr(&output)
    );
    assert!(!dir.path().join("a-HDR.dng").exists());
}

#[test]
fn merge_hdr_without_exposure_is_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    gradient_png(&dir, "a.png", 1.0, 0);
    gradient_png(&dir, "b.png", 0.5, 0);
    let output = run_merge(&dir, "merge-hdr", &[]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("merge unsupported"),
        "raster sources have no EXIF exposure, got: {}",
        stderr(&output)
    );
    assert!(!dir.path().join("a-HDR.dng").exists());
}

#[test]
fn merge_rejects_single_source_loudly() {
    let dir = tempfile::tempdir().unwrap();
    gradient_png(&dir, "a.png", 1.0, 0);
    let output = cli()
        .arg("merge-hdr")
        .arg("--input")
        .arg(dir.path().join("a.png"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("at least 2"),
        "got: {}",
        stderr(&output)
    );
}

#[test]
fn merge_dimension_mismatch_is_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    solid_png(&dir, "a.png", 32, 24, 120);
    solid_png(&dir, "b.png", 40, 32, 120);
    let output = run_merge(
        &dir,
        "merge-hdr",
        &[
            "--exposure-times",
            "0.01,0.02",
            "--isos",
            "100,100",
            "--f-numbers",
            "8,8",
        ],
    );
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("merge unsupported"),
        "got: {}",
        stderr(&output)
    );
}

#[test]
fn merge_undersized_frames_are_unsupported() {
    // Below the LibRaw 22px re-import minimum: the writer refuses loudly.
    let dir = tempfile::tempdir().unwrap();
    solid_png(&dir, "a.png", 8, 8, 120);
    solid_png(&dir, "b.png", 8, 8, 200);
    let output = run_merge(
        &dir,
        "merge-hdr",
        &[
            "--exposure-times",
            "0.01,0.02",
            "--isos",
            "100,100",
            "--f-numbers",
            "8,8",
        ],
    );
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("merge unsupported"),
        "got: {}",
        stderr(&output)
    );
    assert!(!dir.path().join("a-HDR.dng").exists());
}

#[test]
fn merge_exposure_arity_mismatch_is_loud() {
    let dir = tempfile::tempdir().unwrap();
    gradient_png(&dir, "a.png", 1.0, 0);
    gradient_png(&dir, "b.png", 0.5, 0);
    let output = run_merge(&dir, "merge-hdr", &["--exposure-times", "0.01"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("one per --input"),
        "got: {}",
        stderr(&output)
    );
}

#[test]
fn merge_leaves_source_sidecars_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let a = gradient_png(&dir, "a.png", 1.0, 0);
    let b = gradient_png(&dir, "b.png", 0.5, 0);
    // Give both sources their own sidecars first.
    for source in [&a, &b] {
        let imported = cli()
            .arg("import")
            .arg("--input")
            .arg(source)
            .output()
            .unwrap();
        assert!(imported.status.success(), "stderr: {}", stderr(&imported));
    }
    let sidecar_a = sidecar_path_for(&a);
    let sidecar_b = sidecar_path_for(&b);
    let bytes_a = fs::read(&sidecar_a).unwrap();
    let bytes_b = fs::read(&sidecar_b).unwrap();

    let output = run_merge(
        &dir,
        "merge-hdr",
        &[
            "--exposure-times",
            "0.01,0.02",
            "--isos",
            "100,100",
            "--f-numbers",
            "8,8",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(fs::read(&sidecar_a).unwrap(), bytes_a);
    assert_eq!(fs::read(&sidecar_b).unwrap(), bytes_b);
}

#[test]
fn merge_source_outside_bundle_is_unsupported() {
    // `--output` in a sub-bundle while a source stays outside: relative
    // references cannot leave the bundle, so this fails loudly.
    let dir = tempfile::tempdir().unwrap();
    gradient_png(&dir, "a.png", 1.0, 0);
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    gradient_png(&dir, "sub/b.png", 1.0, 4);
    let output = cli()
        .arg("merge-pano")
        .arg("--input")
        .arg(dir.path().join("a.png"))
        .arg("--input")
        .arg(sub.join("b.png"))
        .arg("--output")
        .arg(sub.join("a-Pano.dng"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("merge unsupported") && stderr(&output).contains("outside"),
        "got: {}",
        stderr(&output)
    );
}

#[test]
fn merge_hdr_json_report_parses() {
    let dir = tempfile::tempdir().unwrap();
    gradient_png(&dir, "a.png", 1.0, 0);
    gradient_png(&dir, "b.png", 0.5, 0);
    let output = run_merge(
        &dir,
        "merge-hdr",
        &[
            "--exposure-times",
            "0.01,0.02",
            "--isos",
            "100,100",
            "--f-numbers",
            "8,8",
            "--json",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["command"], serde_json::Value::from("merge-hdr"));
    assert_eq!(report["status"], serde_json::Value::from("ok"));
    assert!(report["digest"].as_str().unwrap().starts_with("blake3:"));
    assert!(report["checksum"].as_str().unwrap().starts_with("blake3:"));
}
