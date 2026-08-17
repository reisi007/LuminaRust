use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_sidecar::sidecar_path_for;
use std::fs;
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

fn write_png(directory: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    let path = directory.path().join(name);
    let frame = ImageFrame::new(1, 1, vec![40, 80, 120, 255]).unwrap();
    fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    path
}

#[test]
fn help_is_available_from_built_binary() {
    let output = cli().arg("--help").output().unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Non-destructive raster image MVP"));
}

#[test]
fn process_writes_jpeg_output_and_sidecar() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    let output = directory.path().join("output.jpg");
    let result = cli()
        .args([
            "process",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--exposure",
            "0.5",
        ])
        .output()
        .unwrap();

    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.is_file());
    assert!(sidecar_path_for(&input).is_file());
    let decoded = ImageFrame::decode(&fs::read(output).unwrap()).unwrap();
    assert_eq!((decoded.width, decoded.height), (1, 1));
}

#[test]
fn invalid_adjustment_exits_non_zero_with_error() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    let output = directory.path().join("output.png");
    let result = cli()
        .args([
            "process",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--contrast",
            "2",
        ])
        .output()
        .unwrap();

    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("invalid contrast"));
    assert!(!output.exists());
}

#[test]
fn unknown_adjustment_exits_non_zero_with_error() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    let output = directory.path().join("output.png");
    let preset = directory.path().join("preset.json");
    fs::write(
        &preset,
        r#"{"id":"unknown","name":"Unknown","recipe":{"adjustments":{"clarity":0.5},"options":{}}}"#,
    )
    .unwrap();
    let result = cli()
        .args([
            "process",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--preset",
            preset.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("unsupported adjustment `clarity`"));
}

#[test]
fn raw_extension_exits_non_zero_with_unsupported_raw_error() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.arw");
    let output = directory.path().join("output.png");
    let result = cli()
        .args([
            "process",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("UnsupportedRaw"));
}
