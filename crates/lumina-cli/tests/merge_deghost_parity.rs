//! Slice B (deghosting): CLI≡MCP parity with `deghost`, the pano-only
//! rejection, and the no-flag no-drift anchor (B-T6–B-T8).
//!
//! The harness (`merge_mcp_parity_common`) renders one [`MergeCall`] onto
//! both transports, so the two sides cannot run on different input sets.
//! The fixture below is new (in-file): no committed golden or testdata set is
//! touched.
//!
//! # What the three tests prove
//!
//! - **B-T6** byte-identity across both transports (payload, DNG, sidecar)
//!   *and* that the mask reaches the pixels: the fixture carries a moved
//!   object, so the same pair merged with the flag and without it must not
//!   produce the same DNG, while a threshold above the deviation must.
//! - **B-T7** the pano-only refusal with the identical clause on both
//!   transports, and nothing left behind on either side.
//! - **B-T8** the no-drift anchor: without the flag the recipe carries no
//!   `deghost` key at all (the `skip_serializing_if` mechanism, B.9), which is
//!   what keeps the committed digest/checksum anchors in `merge_parity.rs`
//!   and `merge_mcp_parity*` unchanged. A forced re-run is pixel-identical.
//!
//! The pixel-side decision itself (the mask, the reference fallback, the
//! threshold sensitivity) is unit-tested in `lumina-merge/src/hdr_deghost.rs`
//! (B-T1–B-T4) and the recipe contract in
//! `lumina-sidecar/src/merge_deghost.rs` (B-T5).

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

/// B-T6 fixture geometry: the 32×24 non-periodic gradient of the shared
/// fixtures (the smallest HDR-viable pair), which aligns exactly (shift 0) —
/// the §9.1 64×48 anchor is `level + x % 16`, i.e. invariant along y, so its
/// alignment is free to carry a large vertical shift that could move a
/// painted object out of the frame. A deghost fixture needs a shift that is
/// exactly known to be zero.
const GHOST_W: u32 = GRADIENT_W;
const GHOST_H: u32 = GRADIENT_H;

/// The moved object of the B-T6 fixture: an 8×8 region in the second frame
/// whose pixels are 1.5× brighter than the bracket expects.
const BLOCK: (u32, u32, u32) = (12, 8, 8);

/// Non-periodic gradient `v = (x * 0.7 + y * 3.1) / 128 * scale`, the shared
/// fixture formula, with an optional 1.5×-bright block (the moved object).
fn ghost_frame(dir: &Path, name: &str, scale: f32, block: bool) -> PathBuf {
    let (w, h) = (GHOST_W, GHOST_H);
    let mut pixels = Vec::with_capacity((w * h) as usize * 4);
    for y in 0..h {
        for x in 0..w {
            let mut v = (x as f32 * 0.7 + y as f32 * 3.1) / 128.0 * scale;
            if block
                && (BLOCK.0..BLOCK.0 + BLOCK.2).contains(&x)
                && (BLOCK.1..BLOCK.1 + BLOCK.2).contains(&y)
            {
                // 1.5x too bright for this frame's exposure: a 50 % relative
                // radiance deviation — above the 0.25 threshold, below the
                // 2.0 maximum.
                v *= 1.5;
            }
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

/// The ghost pair, in merge order (reference first): the gradient at 1× and
/// 0.5 brightness under a 2×/1× bracket (`radiance = pixel / rel`, so the
/// half-brightness frame carries the SAME radiance), with the moved object in
/// the second frame.
fn ghost_pair(dir: &Path) -> Vec<PathBuf> {
    vec![
        ghost_frame(dir, "a.png", 1.0, false),
        ghost_frame(dir, "b.png", 0.5, true),
    ]
}

/// The 2×/1× bracket that makes the 1.0/0.5 brightness pair radiance-consistent.
fn bracket() -> (&'static [f64], &'static [u32], &'static [f64]) {
    (&[0.02, 0.01][..], &[100, 100][..], &[8.0, 8.0][..])
}

/// The persisted deghosting of the sidecar at `dng`.
fn deghost_of(dng: &Path) -> serde_json::Value {
    sidecar_json(dng)["merge_recipe"]["alignment"]["deghost"].clone()
}

/// B-T6 — CLI `--deghost 0.25` ≡ MCP `deghost: 0.25`: identical digest,
/// checksum, DNG bytes and sidecar, and the recipe carries the threshold on
/// both transports.
///
/// The fixture carries a real moved object, so this also proves the mask
/// reaches the pixels through the whole orchestration: the deghost run's DNG
/// bytes differ from the same pair merged without the flag. (Honest scope:
/// 8-bit quantisation of the 1.0/0.5 pair also pushes the two very dark
/// pixels (1,0)/(2,0) above 0.25, so the byte difference is not exclusively
/// carried by the painted block — the block itself deviates a genuine 50 %,
/// which is what keeps `2.0 ≡ plain` a valid companion claim.)
#[test]
fn b_t6_deghost_cli_equals_mcp_byte_for_byte() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    let (times, isos, f_numbers) = bracket();
    let cli_call = MergeCall::new(ghost_pair(&cli_dir))
        .exposure(times, isos, f_numbers)
        .deghost("0.25");
    let mcp_call = MergeCall::new(ghost_pair(&mcp_dir))
        .exposure(times, isos, f_numbers)
        .deghost("0.25");
    assert_eq!(
        snapshot(&cli_dir),
        snapshot(&mcp_dir),
        "the two input trees must start byte-identical"
    );

    let cli_report = cli_call.cli_json("merge-hdr");
    let mcp_payload = mcp_call.mcp_seam(HDR_TOOL);

    // Same payload: the non-periodic gradient pair aligns exactly (residual
    // 0), so neither transport carries the MCP-only `warning` field.
    assert_payload_field_set("B-T6 CLI --json", &cli_report, false);
    assert_payload_field_set("B-T6 MCP structuredContent", &mcp_payload, false);
    assert_same_payload(
        "B-T6 deghost hdr",
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

    let cli_dng = Path::new(cli_report["output"].as_str().unwrap()).to_path_buf();
    let mcp_dng = Path::new(mcp_payload["output"].as_str().unwrap()).to_path_buf();
    assert_identical_files(&cli_dng, &mcp_dng, "B-T6 deghost DNG");
    assert_eq!(mcp_payload["checksum"], json!(blake3_file(&mcp_dng)));

    let cli_sidecar = Path::new(cli_report["sidecar"].as_str().unwrap()).to_path_buf();
    let mcp_sidecar = Path::new(mcp_payload["sidecar"].as_str().unwrap()).to_path_buf();
    assert_identical_sidecar(&cli_sidecar, &mcp_sidecar, "B-T6 deghost sidecar");
    // Captured before the comparison runs below: they rewrite the same target
    // in the same tree.
    let deghost_bytes = bytes(&cli_dng);

    // The recipe carries the requested threshold (and the fixed fallback
    // policy) on both sides, identically.
    for (label, dng) in [("CLI", &cli_dng), ("MCP", &mcp_dng)] {
        assert_eq!(
            deghost_of(dng),
            json!({ "threshold": 0.25, "reference_fallback": true }),
            "{label}: the recipe must persist the threshold"
        );
    }

    // Envelope + identity: the deghost run digests differently from the same
    // pair without the flag (same sources, different threshold = different
    // identity), and the mask actually changed pixels.
    let off_report = MergeCall::new(ghost_pair(&cli_dir))
        .exposure(times, isos, f_numbers)
        .force()
        .cli_json("merge-hdr");
    let off_digest = off_report["digest"].clone();
    assert_ne!(
        cli_report["digest"], off_digest,
        "deghost on vs. off must digest differently"
    );
    let off_bytes = bytes(Path::new(off_report["output"].as_str().unwrap()));
    assert_ne!(
        deghost_bytes, off_bytes,
        "the mask must reach the pixels (same pair, threshold 0.25 vs off)"
    );

    // A threshold above the deviation leaves the pixels untouched: the DNG
    // is byte-identical to the no-flag run, only the recipe digest moves.
    let tolerant = MergeCall::new(ghost_pair(&cli_dir))
        .exposure(times, isos, f_numbers)
        .deghost("2.0")
        .force()
        .cli_json("merge-hdr");
    assert_eq!(
        tolerant["checksum"], off_report["checksum"],
        "a 50 % deviation is below 2.0: no pixel may move"
    );
    assert_ne!(
        tolerant["digest"], off_report["digest"],
        "but the threshold is part of the identity"
    );
}

/// B-T7 — `--deghost 0.25` on `merge-pano` is `merge failed:` (HDR only) on
/// both transports, and the refusal leaves nothing behind.
#[test]
fn b_t7_deghost_on_pano_fails_loudly_and_writes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    let cli_call = MergeCall::new(pano_pair(&cli_dir)).deghost("0.25");
    let mcp_call = MergeCall::new(pano_pair(&mcp_dir)).deghost("0.25");

    // CLI: loud refusal with the HDR-only clause …
    let before = snapshot(&cli_dir);
    let output = cli_call.cli_failure("merge-pano");
    let cause = cli_cause(&output);
    assert!(
        cause.contains("merge failed:"),
        "pano + deghost must be `merge failed:`, got: {cause}"
    );
    assert!(
        cause.contains("HDR only"),
        "the refusal must name the HDR-only rule, got: {cause}"
    );
    assert_eq!(
        snapshot(&cli_dir),
        before,
        "a refused merge-pano must leave nothing behind"
    );

    // … and the MCP twin maps the identical class and clause (D.2).
    let before = snapshot(&mcp_dir);
    let error = mcp_call
        .mcp(PANO_TOOL)
        .expect_err("MCP merge-pano + deghost must refuse");
    assert_eq!(error.name(), "MergeFailed", "got: {}", error.message());
    assert!(
        merge_clause(&mcp_cause(&error)).contains("merge failed:"),
        "got: {}",
        error.message()
    );
    assert!(
        mcp_cause(&error).contains("HDR only"),
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

/// B-T8 — the no-drift anchor: without the flag both transports agree byte
/// for byte, the recipe carries NO deghost key (the mechanism that keeps
/// pre-deghost digests stable), and a forced re-run is pixel-identical.
#[test]
fn b_t8_no_flag_runs_agree_and_carry_no_deghost_key() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    let (times, isos, f_numbers) = bracket();
    let cli_call = MergeCall::new(ghost_pair(&cli_dir)).exposure(times, isos, f_numbers);
    let mcp_call = MergeCall::new(ghost_pair(&mcp_dir)).exposure(times, isos, f_numbers);
    assert_eq!(
        snapshot(&cli_dir),
        snapshot(&mcp_dir),
        "the two input trees must start byte-identical"
    );

    let cli_report = cli_call.cli_json("merge-hdr");
    let mcp_payload = mcp_call.mcp_seam(HDR_TOOL);
    assert_payload_field_set("B-T8 CLI --json", &cli_report, false);
    assert_payload_field_set("B-T8 MCP structuredContent", &mcp_payload, false);
    assert_same_payload(
        "B-T8 default hdr",
        &mcp_payload,
        &cli_report,
        &mcp_dir,
        &cli_dir,
    );

    let cli_dng = Path::new(cli_report["output"].as_str().unwrap()).to_path_buf();
    let mcp_dng = Path::new(mcp_payload["output"].as_str().unwrap()).to_path_buf();
    assert_identical_files(&cli_dng, &mcp_dng, "B-T8 default DNG");
    let cli_sidecar = Path::new(cli_report["sidecar"].as_str().unwrap()).to_path_buf();
    let mcp_sidecar = Path::new(mcp_payload["sidecar"].as_str().unwrap()).to_path_buf();
    assert_identical_sidecar(&cli_sidecar, &mcp_sidecar, "B-T8 default sidecar");

    // No deghost key without the flag — on either side. `None` serialises to
    // absent (not `null`), so the digest input is exactly the pre-deghost
    // one: existing bundles and committed goldens do not go stale from this
    // slice.
    for (label, dng) in [("CLI", &cli_dng), ("MCP", &mcp_dng)] {
        let alignment = &sidecar_json(dng)["merge_recipe"]["alignment"];
        assert!(
            alignment.get("deghost").is_none(),
            "{label}: no deghost key without the flag: {alignment}"
        );
    }

    // Pixel identity: a forced re-run reproduces the DNG byte for byte.
    let before = bytes(&cli_dng);
    let again = MergeCall::new(ghost_pair(&cli_dir))
        .exposure(times, isos, f_numbers)
        .force()
        .cli_json("merge-hdr");
    assert_eq!(again["digest"], cli_report["digest"]);
    assert_eq!(
        bytes(&cli_dng),
        before,
        "no-flag re-run must be pixel-identical"
    );
}
