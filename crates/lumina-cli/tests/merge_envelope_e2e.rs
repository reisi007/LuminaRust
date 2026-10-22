//! MERGE-DNG-1: envelope rejection end-to-end (moved out of `merge_e2e.rs`
//! to satisfy the file-size ratchet; assertions unchanged).

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_sidecar::sidecar_path_for;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

const W: u32 = 32;
const H: u32 = 24;

fn gradient_png(dir: &tempfile::TempDir, name: &str, scale: f32, shift_x: u32) -> PathBuf {
    gradient_png_sized(dir, name, W, H, scale, shift_x)
}

fn gradient_png_sized(
    dir: &tempfile::TempDir,
    name: &str,
    w: u32,
    h: u32,
    scale: f32,
    shift_x: u32,
) -> PathBuf {
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        for x in 0..w {
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
    let frame = ImageFrame::new(w, h, rgba).unwrap();
    fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    path
}

fn run_merge(dir: &tempfile::TempDir, command: &str, extra: &[&str]) -> std::process::Output {
    let mut inputs: Vec<PathBuf> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("png"))
        .collect();
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

#[test]
fn merge_standard_sidecar_is_rejected_loudly() {
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
    let sidecar = sidecar_path_for(&dng);
    let mut value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&sidecar).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("type");
    fs::write(&sidecar, serde_json::to_string_pretty(&value).unwrap()).unwrap();

    let dng_before = fs::read(&dng).unwrap();
    let sidecar_before = fs::read(&sidecar).unwrap();

    let rejected = run_merge(&dir, "merge-hdr", &args);
    assert!(!rejected.status.success(), "got: {}", stderr(&rejected));
    assert!(
        stderr(&rejected).contains("standard sidecar"),
        "got: {}",
        stderr(&rejected)
    );
    assert_eq!(
        fs::read(&dng).unwrap(),
        dng_before,
        "DNG changed after rejection"
    );
    assert_eq!(
        fs::read(&sidecar).unwrap(),
        sidecar_before,
        "sidecar changed after rejection"
    );

    let mut forced_args = args.to_vec();
    forced_args.push("--force");
    let forced = run_merge(&dir, "merge-hdr", &forced_args);
    assert!(!forced.status.success(), "got: {}", stderr(&forced));
    assert!(
        stderr(&forced).contains("standard sidecar"),
        "got: {}",
        stderr(&forced)
    );
    assert_eq!(
        fs::read(&dng).unwrap(),
        dng_before,
        "DNG changed after --force rejection"
    );
    assert_eq!(
        fs::read(&sidecar).unwrap(),
        sidecar_before,
        "sidecar changed after --force rejection"
    );
}

#[test]
fn merge_unknown_envelope_type_is_rejected_loudly() {
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
    let sidecar = sidecar_path_for(&dng);
    let mut value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&sidecar).unwrap()).unwrap();
    value["type"] = serde_json::Value::from("merge-hdr");
    fs::write(&sidecar, serde_json::to_string_pretty(&value).unwrap()).unwrap();

    let rejected = run_merge(&dir, "merge-hdr", &args);
    assert!(!rejected.status.success(), "got: {}", stderr(&rejected));
    assert!(
        stderr(&rejected).contains("unknown document type")
            && stderr(&rejected).contains("merge-hdr"),
        "got: {}",
        stderr(&rejected)
    );
}
