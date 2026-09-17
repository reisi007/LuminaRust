//! LRPAR-G13-MERGE-15 / MERGE-IMPL-15 (F6): CLI ↔ GUI parity anchor.
//!
//! SOLL: `feature/decisions/LRPAR-G13-MERGE-15.md` (§Capability-Matrix
//! „Kein stiller Fallback zwischen CLI und GUI") and the F6 follow-up: after
//! the orchestration dedup both frontends call
//! [`lumina_merge::bundle::run_merge`]. The GUI headless suite
//! (`lumina-gui/src/merge_gui.rs`) pins the GUI-side decode contract for a
//! raster source (`decoder = "image"`, `decode_version = CARGO_PKG_VERSION`,
//! `orientation = 1`). This test runs the CLI binary and then the same shared
//! orchestrator through a decode adapter that deliberately follows the GUI
//! contract on identical bytes: both must land on the same merge digest and
//! the same DNG checksum. That is the hash anchor the dedup promises (same
//! path, same input → same artifact).
//!
//! Only the decode adapter differs between the two calls; alignment, blending,
//! recipe, digest, gate and DNG writer are the shared implementation.

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_merge::bundle::{
    linear_from_rgba, merge_checksum, run_merge, MergeExif, MergeRunError, MergeRunOptions,
    MergeSourceFrame,
};
use lumina_sidecar::{MergeDecodeContext, MergeExposure, MergeMode};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

/// The exact fixture shape the GUI headless golden uses (64×48 gradient
/// `level + x % 16`); identical bytes on both paths are the parity premise.
fn write_png(dir: &Path, name: &str, level: u8) -> PathBuf {
    let pixels: Vec<u8> = (0..64 * 48)
        .flat_map(|i| {
            let x = (i % 64) as u8;
            let value = level.saturating_add(x % 16);
            [value, value, value, 255]
        })
        .collect();
    let png = ImageFrame::new(64, 48, pixels)
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap();
    let path = dir.join(name);
    fs::write(&path, png).unwrap();
    path
}

/// GUI-contract decode adapter (raster): `ImageFrame::decode` + the GUI's
/// decode context for a non-RAW source.
fn decode_gui_contract(path: &Path) -> Result<MergeSourceFrame, MergeRunError> {
    let bytes = fs::read(path).map_err(|error| {
        MergeRunError::Missing(format!("cannot read source `{}`: {error}", path.display()))
    })?;
    let frame = ImageFrame::decode(&bytes)
        .map_err(|error| MergeRunError::Unsupported(error.to_string()))?;
    let linear = linear_from_rgba(&frame)
        .ok_or_else(|| MergeRunError::Unsupported("malformed RGBA8 frame".to_string()))?;
    Ok(MergeSourceFrame {
        path: path.to_path_buf(),
        content_hash: merge_checksum(&bytes),
        frame: linear,
        decode_context: MergeDecodeContext {
            decoder: "image".into(),
            decode_version: env!("CARGO_PKG_VERSION").into(),
            orientation: 1,
        },
        exif: MergeExif::default(),
    })
}

#[test]
fn cli_and_gui_contract_paths_share_the_same_anchor() {
    // Reference run: the real CLI binary on the fixture pair.
    let cli_dir = tempfile::tempdir().unwrap();
    let cli_a = write_png(cli_dir.path(), "a.png", 40);
    let cli_b = write_png(cli_dir.path(), "b.png", 160);
    let output = cli()
        .arg("merge-hdr")
        .arg("--input")
        .arg(&cli_a)
        .arg("--input")
        .arg(&cli_b)
        .arg("--exposure-times")
        .arg("0.004,0.016")
        .arg("--isos")
        .arg("100,100")
        .arg("--f-numbers")
        .arg("8,8")
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let cli_digest = report["digest"].as_str().unwrap().to_string();
    let cli_checksum = report["checksum"].as_str().unwrap().to_string();

    // Parity run: identical bytes in a fresh bundle directory, driven through
    // the shared orchestrator with the GUI decode contract.
    let gui_dir = tempfile::tempdir().unwrap();
    let gui_a = write_png(gui_dir.path(), "a.png", 40);
    let gui_b = write_png(gui_dir.path(), "b.png", 160);
    let inputs = vec![gui_a, gui_b];
    let exposures = [
        MergeExposure {
            exposure_time_s: 0.004,
            iso: 100,
            f_number: 8.0,
        },
        MergeExposure {
            exposure_time_s: 0.016,
            iso: 100,
            f_number: 8.0,
        },
    ];
    let resolve =
        |_mode: MergeMode, index: usize, _source: &MergeSourceFrame| Ok(exposures[index].clone());
    let options = MergeRunOptions {
        output: None,
        max_shift_px: 16,
        blend_width_px: 64,
        force: false,
        dng_decode_version: lumina_raw::libraw_decode_version(),
    };
    let outcome = run_merge(
        MergeMode::Hdr,
        &inputs,
        &options,
        decode_gui_contract,
        resolve,
    )
    .unwrap();

    assert_eq!(
        outcome.digest, cli_digest,
        "CLI and GUI-contract runs must share the merge digest"
    );
    assert_eq!(
        outcome.checksum, cli_checksum,
        "CLI and GUI-contract runs must share the DNG checksum anchor"
    );
}
