//! Slice A (gain compensation): CLI≡MCP parity with `gain_compensation`,
//! the HDR-only rejection, and the no-flag no-drift anchor (A-T7–A-T9).
//!
//! The harness (`merge_mcp_parity_common`) renders one [`MergeCall`] onto
//! both transports, so the two sides cannot run on different input sets.
//! The fixtures below are new test fixtures (64×48, in-file): no committed
//! golden or testdata set is touched.

// CLI-NO-DEFAULT-TESTBUILD-80: the whole target is CLI<->MCP parity work; the
// shared harness needs `lumina_mcp`, which only exists under the default `mcp`
// feature, so the file is gated on that feature as a whole.
#![cfg(feature = "mcp")]

#[path = "merge_mcp_parity_common/mod.rs"]
mod common;

use common::{fixtures::*, graders::*, job::*};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

/// Gain-fixture geometry: 64×48 (overlap 64×48 = 3072 px ≥ 1024).
const GAIN_W: u32 = 64;
const GAIN_H: u32 = 48;

/// Non-periodic gradient `v = (x * 0.7 + y * 3.1) / 128`, 64×48
/// (the `merge_mcp_parity_common` 32×24 gradient scaled up: its overlap
/// would be 28×24 = 672 px, below the 1024 px gain-statistic minimum).
fn gain_gradient(dir: &Path, name: &str, scale: f32) -> PathBuf {
    let (w, h) = (GAIN_W, GAIN_H);
    let mut pixels = Vec::with_capacity((w * h) as usize * 4);
    for y in 0..h {
        for x in 0..w {
            let v = (x as f32 * 0.7 + y as f32 * 3.1) / 128.0 * scale;
            let b = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            pixels.extend_from_slice(&[b, b, b, 255]);
        }
    }
    let path = dir.join(name);
    fs::create_dir_all(dir).unwrap();
    let png = lumina_core::ImageFrame::new(w, h, pixels)
        .unwrap()
        .encode(lumina_core::ImageFileFormat::Png)
        .unwrap();
    fs::write(&path, png).unwrap();
    path
}

/// The gain pair: reference gradient plus the same gradient at half
/// brightness (the auto-exposure pano case: same viewpoint, different
/// exposure). Full overlap: 64×48 = 3072 px ≥ 1024, true gain 2.0.
///
/// Deliberately unshifted: the alignment SAD matches raw luminance, so a
/// global brightness factor plus a shift cannot reliably separate (the
/// true shift loses against dx=0 on edge costs — a pre-existing estimator
/// characteristic, not Slice-A scope). The shifted-overlap geometry is
/// covered exactly by the unit tests with known matrices (A-T1).
fn gain_pair(dir: &Path) -> Vec<PathBuf> {
    vec![
        gain_gradient(dir, "a.png", 1.0),
        gain_gradient(dir, "b.png", 0.5),
    ]
}

/// The persisted gain compensation of the sidecar at `dng`.
fn gain_of(dng: &Path) -> serde_json::Value {
    sidecar_json(dng)["merge_recipe"]["alignment"]["gain_compensation"].clone()
}

/// A-T7 — CLI `--gain-compensation overlap-median` ≡ MCP
/// `gain_compensation: "overlap-median"`: identical digest + checksum +
/// DNG bytes, and the recipe carries the estimated gain.
///
/// A stub that ignores the flag (or estimates a constant) fails the
/// gain-value assertions; a transport that drifts fails the byte compares.
#[test]
fn a_t7_gain_compensation_cli_equals_mcp_byte_for_byte() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    let cli_call = MergeCall::new(gain_pair(&cli_dir)).gain_compensation("overlap-median");
    let mcp_call = MergeCall::new(gain_pair(&mcp_dir)).gain_compensation("overlap-median");
    assert_eq!(
        snapshot(&cli_dir),
        snapshot(&mcp_dir),
        "the two input trees must start byte-identical"
    );

    let cli_report = cli_call.cli_json("merge-pano");
    let mcp_payload = mcp_call.mcp_seam(PANO_TOOL);

    // Same payload (the zero-shift residual is below the warning threshold,
    // so neither side carries `warning`).
    assert_payload_field_set("A-T7 CLI --json", &cli_report, false);
    assert_payload_field_set("A-T7 MCP structuredContent", &mcp_payload, false);
    assert_same_payload(
        "A-T7 gain pano",
        &mcp_payload,
        &cli_report,
        &mcp_dir,
        &cli_dir,
    );
    for field in ["digest", "checksum"] {
        assert_eq!(
            cli_report[field], mcp_payload[field],
            "{field} must match across transports"
        );
    }

    // Same DNG bytes, and the checksum is the blake3 of the written DNG.
    let cli_dng = Path::new(cli_report["output"].as_str().unwrap()).to_path_buf();
    let mcp_dng = Path::new(mcp_payload["output"].as_str().unwrap()).to_path_buf();
    assert_identical_files(&cli_dng, &mcp_dng, "A-T7 gain DNG");
    assert_eq!(mcp_payload["checksum"], json!(blake3_file(&mcp_dng)));

    // Same sidecar except the wall-clock stamp.
    let cli_sidecar = Path::new(cli_report["sidecar"].as_str().unwrap()).to_path_buf();
    let mcp_sidecar = Path::new(mcp_payload["sidecar"].as_str().unwrap()).to_path_buf();
    assert_identical_sidecar(&cli_sidecar, &mcp_sidecar, "A-T7 gain sidecar");

    // The recipe carries a real estimate: overlap-median, gains [1.0, ~2.0]
    // (half-brightness frame scaled back up), overlap above the minimum —
    // on both sides identically.
    for (label, dng) in [("CLI", &cli_dng), ("MCP", &mcp_dng)] {
        let gain = gain_of(dng);
        assert_eq!(gain["method"], json!("overlap-median"), "{label}: method");
        let gains = gain["gains"].as_array().unwrap();
        assert_eq!(gains.len(), 2, "{label}: one gain per source");
        assert_eq!(
            gains[0],
            json!(1.0),
            "{label}: reference gain is exactly 1.0"
        );
        let estimated = gains[1].as_f64().unwrap();
        assert!(
            (estimated - 2.0).abs() < 0.06,
            "{label}: half-brightness frame must estimate ~2.0, got {estimated}"
        );
        assert!(
            gain["min_overlap_px"].as_u64().unwrap() >= 1024,
            "{label}: statistic stands on ≥ 1024 px: {gain}"
        );
    }
    assert_eq!(gain_of(&cli_dng), gain_of(&mcp_dng));

    // Canvas geometry: unshifted pair, so the canvas is the frame itself.
    assert_merge_envelope(
        "A-T7 gain pano",
        &mcp_sidecar,
        &mcp_dir,
        "panorama",
        &mcp_payload["checksum"],
        (GAIN_W, GAIN_H),
        &["a.png", "b.png"],
    );

    // Identity: the gain run digests differently from the no-gain run on the
    // same bytes (same sources + different gains = different digest).
    let off_digest = MergeCall::new(gain_pair(&cli_dir))
        .force()
        .cli_json("merge-pano")["digest"]
        .clone();
    assert_ne!(
        cli_report["digest"], off_digest,
        "gain on vs. gain off must digest differently"
    );
}

/// A-T8 — `--gain-compensation overlap-median` on `merge-hdr` is
/// `merge failed:` (panorama only) on both transports, and the refusal
/// leaves nothing behind.
#[test]
fn a_t8_gain_compensation_on_hdr_fails_loudly_and_writes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    let exposure = (&[0.01, 0.02][..], &[100, 100][..], &[8.0, 8.0][..]);
    let cli_call = MergeCall::new(hdr_pair(&cli_dir))
        .exposure(exposure.0, exposure.1, exposure.2)
        .gain_compensation("overlap-median");
    let mcp_call = MergeCall::new(hdr_pair(&mcp_dir))
        .exposure(exposure.0, exposure.1, exposure.2)
        .gain_compensation("overlap-median");

    // CLI: loud refusal with the panorama-only clause …
    let before = snapshot(&cli_dir);
    let output = cli_call.cli_failure("merge-hdr");
    let cause = cli_cause(&output);
    assert!(
        cause.contains("merge failed:"),
        "HDR + gain must be `merge failed:`, got: {cause}"
    );
    assert!(
        cause.contains("panorama only"),
        "the refusal must name the panorama-only rule, got: {cause}"
    );
    assert_eq!(
        snapshot(&cli_dir),
        before,
        "a refused merge-hdr must leave nothing behind"
    );

    // … and the MCP twin maps the identical class and clause (D.2).
    let before = snapshot(&mcp_dir);
    let error = mcp_call
        .mcp(HDR_TOOL)
        .expect_err("MCP merge-hdr + gain must refuse");
    assert_eq!(error.name(), "MergeFailed", "got: {}", error.message());
    assert!(
        merge_clause(&mcp_cause(&error)).contains("merge failed:"),
        "got: {}",
        error.message()
    );
    assert!(
        mcp_cause(&error).contains("panorama only"),
        "got: {}",
        error.message()
    );
    assert_eq!(
        snapshot(&mcp_dir),
        before,
        "a refused MCP call must leave nothing behind"
    );
    assert_eq!(
        merge_clause_of(&cause, [&cli_dir, &mcp_dir]),
        merge_clause_of(&mcp_cause(&error), [&cli_dir, &mcp_dir]),
        "CLI and MCP must refuse with the same clause"
    );
}

/// A-T9 — default parity and the no-drift anchor: without the flag both
/// transports agree byte for byte, the recipe carries no gain key (the
/// mechanism that keeps pre-gain digests stable), and a forced re-run is
/// pixel-identical.
#[test]
fn a_t9_no_flag_runs_agree_and_carry_no_gain_key() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    let cli_call = MergeCall::new(gain_pair(&cli_dir));
    let mcp_call = MergeCall::new(gain_pair(&mcp_dir));
    assert_eq!(
        snapshot(&cli_dir),
        snapshot(&mcp_dir),
        "the two input trees must start byte-identical"
    );

    let cli_report = cli_call.cli_json("merge-pano");
    let mcp_payload = mcp_call.mcp_seam(PANO_TOOL);
    assert_payload_field_set("A-T9 CLI --json", &cli_report, false);
    assert_payload_field_set("A-T9 MCP structuredContent", &mcp_payload, false);
    assert_same_payload(
        "A-T9 default pano",
        &mcp_payload,
        &cli_report,
        &mcp_dir,
        &cli_dir,
    );

    let cli_dng = Path::new(cli_report["output"].as_str().unwrap()).to_path_buf();
    let mcp_dng = Path::new(mcp_payload["output"].as_str().unwrap()).to_path_buf();
    assert_identical_files(&cli_dng, &mcp_dng, "A-T9 default DNG");
    let cli_sidecar = Path::new(cli_report["sidecar"].as_str().unwrap()).to_path_buf();
    let mcp_sidecar = Path::new(mcp_payload["sidecar"].as_str().unwrap()).to_path_buf();
    assert_identical_sidecar(&cli_sidecar, &mcp_sidecar, "A-T9 default sidecar");

    // No gain key without the flag — on either side. `None` serialises to
    // absent (not `null`), so the digest input is exactly the pre-gain one:
    // existing bundles and committed goldens do not go stale from this slice.
    for (label, dng) in [("CLI", &cli_dng), ("MCP", &mcp_dng)] {
        let alignment = &sidecar_json(dng)["merge_recipe"]["alignment"];
        assert!(
            alignment.get("gain_compensation").is_none(),
            "{label}: no gain key without the flag: {alignment}"
        );
    }

    // Pixel identity: a forced re-run reproduces the DNG byte for byte.
    let before = bytes(&cli_dng);
    let again = MergeCall::new(gain_pair(&cli_dir))
        .force()
        .cli_json("merge-pano");
    assert_eq!(again["digest"], cli_report["digest"]);
    assert_eq!(
        bytes(&cli_dng),
        before,
        "no-flag re-run must be pixel-identical"
    );
}
