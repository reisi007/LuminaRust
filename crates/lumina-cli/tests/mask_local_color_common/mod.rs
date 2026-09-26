//! Shared helpers for the MASK-LOCAL-P1.2b CLI end-to-end tests.
//!
//! Lives in a subdirectory on purpose: Cargo only auto-discovers top-level
//! `tests/*.rs` as test targets, so this file is compiled into both
//! `mask_local_color_e2e` and `mask_local_color_history_e2e` via an explicit
//! `#[path]` module instead of becoming a third, empty test binary.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_sidecar::{load_sidecar, sidecar_path_for};

/// The CLI never gained a second, near-identical flag pair: the local colour
/// block rides the existing generic `--set-local-adjustment KEY=VALUE` /
/// `--reset-local-adjustment KEY` channel in the `hsl.`, `point_color.`,
/// `color_grading.` namespaces plus the `vibrance`/`saturation` scalars.
pub(crate) const HSL_RED_HUE: &str = "hsl.red.hue=-0.25";
pub(crate) const POINT_COLOR_ADD: &str = "point_color.add=30,45,0.2,0.1,-0.1";
pub(crate) const GRADING_SHADOWS: &str = "color_grading.shadows.saturation=0.4";

pub(crate) fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

/// A 2×2 PNG whose pixels are deliberately colourful, so a local colour edit is
/// observable.
pub(crate) fn write_png(directory: &tempfile::TempDir, name: &str) -> PathBuf {
    let path = directory.path().join(name);
    let frame = ImageFrame::new(
        2,
        2,
        vec![
            200, 90, 40, 255, 30, 90, 200, 255, 200, 40, 40, 255, 250, 250, 250, 255,
        ],
    )
    .unwrap();
    fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    path
}

pub(crate) fn assert_success(output: &Output, action: &str) {
    assert!(
        output.status.success(),
        "{action} failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(crate) fn import_image(path: &Path) {
    let output = cli()
        .args(["import", "--input", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert_success(&output, "import");
}

pub(crate) fn add_and_attach_layer(path: &Path) -> String {
    let add = cli()
        .args([
            "mask",
            "--input",
            path.to_str().unwrap(),
            "--add-ai-select",
            "subject",
            "--name",
            "Subject",
        ])
        .output()
        .unwrap();
    assert_success(&add, "add mask");
    let mask_id = load_sidecar(&sidecar_path_for(path))
        .unwrap()
        .virtual_copies[0]
        .mask_library[0]
        .id
        .clone();
    let attach = cli()
        .args([
            "mask",
            "--input",
            path.to_str().unwrap(),
            "--attach-layer",
            &mask_id,
        ])
        .output()
        .unwrap();
    assert_success(&attach, "attach mask layer");
    load_sidecar(&sidecar_path_for(path))
        .unwrap()
        .virtual_copies[0]
        .mask_layers[0]
        .id
        .clone()
}

pub(crate) fn set_local(path: &Path, layer_id: &str, specification: &str) -> Output {
    cli()
        .args([
            "mask",
            "--input",
            path.to_str().unwrap(),
            "--local-layer",
            layer_id,
            "--set-local-adjustment",
            specification,
        ])
        .output()
        .unwrap()
}

pub(crate) fn reset_local(path: &Path, layer_id: &str, key: &str) -> Output {
    cli()
        .args([
            "mask",
            "--input",
            path.to_str().unwrap(),
            "--local-layer",
            layer_id,
            "--reset-local-adjustment",
            key,
        ])
        .output()
        .unwrap()
}

pub(crate) fn mask_list_json(path: &Path) -> serde_json::Value {
    let output = cli()
        .args([
            "mask",
            "--input",
            path.to_str().unwrap(),
            "--list",
            "--json",
        ])
        .output()
        .unwrap();
    assert_success(&output, "list");
    serde_json::from_slice(&output.stdout).expect("json list output")
}

pub(crate) fn imported_local_image(directory: &tempfile::TempDir, name: &str) -> (PathBuf, String) {
    let path = write_png(directory, name);
    import_image(&path);
    let layer_id = add_and_attach_layer(&path);
    (path, layer_id)
}

pub(crate) fn stored_local(path: &Path) -> lumina_sidecar::LocalAdjustments {
    load_sidecar(&sidecar_path_for(path))
        .unwrap()
        .virtual_copies[0]
        .mask_layers[0]
        .local_adjustments
        .clone()
        .expect("typed local recipe")
}

/// Attach a deterministic, model-free luminance-range mask to the image.
///
/// A range mask is computed straight from the frame (no ONNX model, no
/// persisted tile), so the CLI really evaluates the matte and the local colour
/// block instead of rendering a stand-in with no mask at all.
pub(crate) fn add_range_mask(path: &Path) {
    let output = cli()
        .args([
            "mask",
            "--input",
            path.to_str().unwrap(),
            "--add-luminance-range",
            "--range-min",
            "0",
            "--range-max",
            "1",
            "--name",
            "Range",
        ])
        .output()
        .unwrap();
    assert_success(&output, "add luminance range mask");
    let mask_id = load_sidecar(&sidecar_path_for(path))
        .unwrap()
        .virtual_copies[0]
        .mask_library[0]
        .id
        .clone();
    assert_success(
        &cli()
            .args([
                "mask",
                "--input",
                path.to_str().unwrap(),
                "--attach-layer",
                &mask_id,
            ])
            .output()
            .unwrap(),
        "attach range mask layer",
    );
}

/// Render the imported image through the CLI so the local colour block is
/// proven to reach pixels, not only the file.
pub(crate) fn render_png(path: &Path, out: &Path) -> Vec<u8> {
    let output = cli()
        .args([
            "export",
            "--input",
            path.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_success(&output, "export");
    ImageFrame::decode(&fs::read(out).unwrap())
        .expect("decodable png")
        .pixels
}
