//! MCP-PARITY-C: byte identity between `lumina matrix` (CLI) and
//! `lumina_matrix` (MCP) on the §8 anchor (SOLL §9 phase 3: T15–T18; the
//! argument-shape half T22–T25 lives in `matrix_mcp_parity_options.rs`, the
//! loud rejections in `matrix_mcp_parity_errors.rs`, the Real-RAW gate T26 in
//! `matrix_mcp_parity_gate.rs`).
//!
//! # What is actually compared
//!
//! The real `lumina-cli` process runs on one fixture tree, the MCP tool runs
//! in-process on a *second, byte-identical* tree, and then:
//!
//! * the `--json` / `structuredContent` documents are compared field for
//!   field (T15) under exactly the two §3.1 masks (`duration_ms`, absolute
//!   paths) — so the MCP tool cannot answer with the CLI's values in its own
//!   shape, nor invent a field;
//! * the `expected_route` input is proven inert in verify mode (T16): a
//!   rotated route changes the echo and nothing else, and the set file stays
//!   byte-identical;
//! * the baseline goldens are compared byte for byte (T17) — the actual
//!   stub-killer, because a stub would have to hit real PNG bytes;
//! * the full-resolution exports and the diff images are compared byte for
//!   byte (T18).
//!
//! # The two masks, and why they are the only two
//!
//! 1. **`duration_ms`** (top level and per pair): wall-clock timings, dropped
//!    on both sides.
//! 2. **absolute paths**: both roots rewritten to `<TREE>` across the whole
//!    serialised document.
//!
//! Everything else — the `psnr_db` **strings**, `max_abs_diff`, `status`,
//! `message`, `route`, `route_reasons`, `expected_route` — is compared raw.

#[path = "matrix_mcp_parity_common/mod.rs"]
mod common;
use common::{fixtures::*, graders::*, green::*, job::*};
use serde_json::{json, Value};
use std::path::PathBuf;

/// T15 — Verify parity: `structuredContent` == CLI `--json` field for field.
///
/// §9 T15 / §10 (1)–(4): the ten §3 top-level fields and nothing invented,
/// `command == "matrix"`, `mode == "verify"`, `pairs_total == passed == 2`,
/// every pair `pass` with `psnr_db == "inf"` (baseline golden against the
/// identical verify render), `max_abs_diff == 0`, the `expected_route` echo,
/// `route == "cpu"` with empty reasons in a CPU build, `message == null` —
/// and then the full masked equality, including the `psnr_db` strings.
#[test]
fn t15_verify_report_is_the_cli_json_field_for_field() {
    let green = green_trees();
    let (cli_dir, mcp_dir) = (&green.cli_dir, &green.mcp_dir);
    let (cli_report, mcp_payload) = (&green.cli_report, &green.mcp_payload);

    // §10 (1): the field set — ten top-level, thirteen per pair, no extras.
    assert_report_field_set("T15 CLI --json", cli_report);
    assert_report_field_set("T15 MCP structuredContent", mcp_payload);

    // §10 (2)+(3): the pinned counts and per-pair values.
    for (label, payload) in [("CLI", cli_report), ("MCP", mcp_payload)] {
        assert_eq!(payload["command"], json!("matrix"), "{label}: command");
        assert_eq!(payload["mode"], json!("verify"), "{label}: mode");
        assert_eq!(payload["pairs_total"], json!(2), "{label}: pairs_total");
        assert_eq!(payload["passed"], json!(2), "{label}: passed");
        assert_eq!(payload["failed"], json!(0), "{label}: failed");
        assert_eq!(payload["comparison_width"], json!(48), "{label}: width");
        let pairs = payload["pairs"].as_array().unwrap();
        assert_eq!(pairs.len(), 2, "{label}: two pairs");
        // One sample × two recipes in set order.
        assert_eq!(pairs[0]["recipe"], json!("identity"));
        assert_eq!(pairs[1]["recipe"], json!("tone"));
        // Baseline golden against the identical verify render: infinite PSNR.
        for pair in pairs {
            assert_eq!(pair["status"], json!("pass"), "{label}: {pair}");
            assert_eq!(pair["psnr_db"], json!("inf"), "{label}: {pair}");
            assert_eq!(pair["max_abs_diff"], json!(0), "{label}: {pair}");
            assert_eq!(pair["message"], Value::Null, "{label}: {pair}");
            assert_eq!(pair["route"], json!("cpu"), "{label}: {pair}");
            assert_eq!(pair["route_reasons"], json!([]), "{label}: {pair}");
            assert_eq!(pair["expected_route"], json!("gpu"), "{label}: echo");
        }
        // The tolerance-class mapping survives the transport: exact → null,
        // strict → 45 (SOLL §3).
        assert_eq!(pairs[0]["min_psnr_db"], Value::Null, "{label}: exact");
        assert_eq!(pairs[1]["min_psnr_db"], json!(45.0), "{label}: strict");
    }

    // §10 (4): the whole document, both masks and nothing else.
    assert_same_report("T15 verify", mcp_payload, cli_report, mcp_dir, cli_dir);
}

/// T16 — `expected_route` protection: a rotated route changes the echo and
/// nothing else, and the set file stays byte-identical.
///
/// §9 T16 / §4.2: (a) the set file is byte-identical before and after both
/// runs, (b) MCP == CLI on the same mutation, (c) the green verdict is
/// unchanged — the field is inert in verify mode.
#[test]
fn t16_a_rotated_expected_route_is_inert_in_verify_mode() {
    let green = green_trees();
    let (cli_dir, mcp_dir) = (&green.cli_dir, &green.mcp_dir);

    // The mutation: rotate the identity route `gpu` → the documented CPU
    // exemption object — the same fixture idea as `matrix_e2e.rs:107-112`.
    // Written byte-identically into both trees.
    let rotate = |dir: &std::path::Path| {
        let path = dir.join("recipe-set.json");
        let text = std::fs::read_to_string(&path).unwrap();
        // Rotate only the FIRST route (identity); tone keeps `gpu`, so the
        // echo assertion below can tell the two apart.
        let mutated = text.replacen(
            "\"expected_route\": \"gpu\"",
            "\"expected_route\": { \"cpu\": \"geometry (default content crop)\" }",
            1,
        );
        assert_ne!(mutated, text, "the rotation must change the set");
        std::fs::write(&path, &mutated).unwrap();
    };
    rotate(cli_dir);
    rotate(mcp_dir);
    let (cli_set_before, mcp_set_before) = (
        bytes(&cli_dir.join("recipe-set.json")),
        bytes(&mcp_dir.join("recipe-set.json")),
    );
    assert_eq!(
        cli_set_before, mcp_set_before,
        "the mutated sets must be byte-identical across trees"
    );

    let cli_report =
        MatrixCall::verify(cli_dir.join("recipe-set.json"), cli_dir.join("work")).cli_json();
    let mcp_payload =
        MatrixCall::verify(mcp_dir.join("recipe-set.json"), mcp_dir.join("work")).mcp_seam();

    // (a) Neither transport writes the set.
    assert_eq!(
        bytes(&cli_dir.join("recipe-set.json")),
        cli_set_before,
        "the CLI run must not touch the set file"
    );
    assert_eq!(
        bytes(&mcp_dir.join("recipe-set.json")),
        mcp_set_before,
        "the MCP run must not touch the set file"
    );
    // (b) Same mutation, same report.
    assert_same_report("T16 mutated", &mcp_payload, &cli_report, mcp_dir, cli_dir);
    // (c) The verdict is unchanged: green with identical counts, statuses and
    // PSNR strings — only the echo moved.
    for field in [
        "mode",
        "pairs_total",
        "passed",
        "failed",
        "comparison_width",
    ] {
        assert_eq!(
            mcp_payload[field], green.mcp_payload[field],
            "T16: `{field}` must not depend on the declared route"
        );
    }
    let pairs = mcp_payload["pairs"].as_array().unwrap();
    assert_eq!(pairs[0]["status"], json!("pass"));
    assert_eq!(pairs[0]["psnr_db"], json!("inf"));
    assert_eq!(
        pairs[0]["expected_route"],
        json!("cpu (geometry (default content crop))"),
        "the echo follows the input (as the route label, §4.1)"
    );
    assert_eq!(
        pairs[1]["expected_route"],
        json!("gpu"),
        "tone is unrotated"
    );
}

/// T17 — Golden byte identity: a CPU-pinned baseline on both transports
/// writes the same bytes.
///
/// §9 T17 / §10 (5): `mode == "baseline"`, every pair
/// `status == "baseline-created"`, and
/// `blake3(golden_cli) == blake3(golden_mcp)` per recipe.
#[test]
fn t17_baseline_goldens_are_byte_identical() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    anchor_tree(&cli_dir);
    anchor_tree(&mcp_dir);

    let cli_report = MatrixCall::verify(cli_dir.join("recipe-set.json"), cli_dir.join("work"))
        .baseline()
        .cli_json();
    let mcp_payload = MatrixCall::verify(mcp_dir.join("recipe-set.json"), mcp_dir.join("work"))
        .baseline()
        .mcp_seam();

    for (label, payload) in [("CLI", &cli_report), ("MCP", &mcp_payload)] {
        assert_eq!(payload["command"], json!("matrix"), "{label}: command");
        assert_eq!(payload["mode"], json!("baseline"), "{label}: mode");
        for pair in payload["pairs"].as_array().unwrap() {
            assert_eq!(pair["status"], json!("baseline-created"), "{label}: {pair}");
        }
    }
    assert_same_report(
        "T17 baseline",
        &mcp_payload,
        &cli_report,
        &mcp_dir,
        &cli_dir,
    );

    for recipe in ["identity", "tone"] {
        let (cli_golden, mcp_golden) = (
            golden_of(&cli_report, recipe),
            golden_of(&mcp_payload, recipe),
        );
        assert!(
            cli_golden.is_file() && mcp_golden.is_file(),
            "goldens exist"
        );
        assert_identical_files(&cli_golden, &mcp_golden, &format!("T17 golden {recipe}"));
        assert_eq!(blake3_file(&cli_golden), blake3_file(&mcp_golden));
    }
}

/// T18 — Export/diff byte identity: on the tolerance-violation path both
/// transports write the same export and the same diff image.
///
/// §9 T18: explicit `work_dir`, inverted tone golden in both trees, red runs
/// on both sides — the two red reports agree field for field, the exports
/// agree byte for byte, and exactly one diff image (`sample__tone.diff.png`)
/// exists per tree, byte-identical across.
#[test]
fn t18_exports_and_diff_images_are_byte_identical() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    anchor_tree(&cli_dir);
    anchor_tree(&mcp_dir);
    let (cli_work, mcp_work) = (cli_dir.join("work"), mcp_dir.join("work"));

    MatrixCall::verify(cli_dir.join("recipe-set.json"), cli_work.clone())
        .baseline()
        .cli_json();
    MatrixCall::verify(mcp_dir.join("recipe-set.json"), mcp_work.clone())
        .baseline()
        .mcp_ok();
    // The identical violation in both trees.
    invert_golden(&golden_path(&cli_dir.join("golden"), "tone"));
    invert_golden(&golden_path(&mcp_dir.join("golden"), "tone"));

    let cli_call = MatrixCall::verify(cli_dir.join("recipe-set.json"), cli_work.clone());
    let mcp_call = MatrixCall::verify(mcp_dir.join("recipe-set.json"), mcp_work.clone());
    let (cli_report, _) = cli_call.cli_red_json();
    let error = mcp_call.mcp().unwrap_err();
    assert_eq!(error.name(), "MatrixError", "{error:?}");
    assert_eq!(error.code(), -32014, "{error:?}");
    let mcp_report = error.data()["report"].clone();
    assert!(
        mcp_report.is_object(),
        "a red run carries its report: {error:?}"
    );

    // The two red reports agree — including `status == "fail"` and the
    // `requires PSNR` message on the tone pair.
    assert_same_report("T18 red", &mcp_report, &cli_report, &mcp_dir, &cli_dir);
    let tone = mcp_report["pairs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pair| pair["recipe"] == json!("tone"))
        .unwrap();
    assert_eq!(tone["status"], json!("fail"));
    assert!(
        tone["message"].as_str().unwrap().contains("requires PSNR"),
        "unexpected tone message: {}",
        tone["message"]
    );

    // Exports byte-identical per recipe, on both pairs (identity still
    // passes, tone still exports the render that failed the gate).
    for recipe in ["identity", "tone"] {
        assert_identical_files(
            &export_of(&cli_report, recipe),
            &export_of(&mcp_report, recipe),
            &format!("T18 export {recipe}"),
        );
    }
    // Exactly one diff image per tree, byte-identical across.
    let diffs = |work: &std::path::Path| -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = std::fs::read_dir(work)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "png"))
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .ends_with(".diff.png")
            })
            .collect();
        out.sort();
        out
    };
    let (cli_diffs, mcp_diffs) = (diffs(&cli_work), diffs(&mcp_work));
    assert_eq!(cli_diffs.len(), 1, "one diff image (CLI): {cli_diffs:?}");
    assert_eq!(mcp_diffs.len(), 1, "one diff image (MCP): {mcp_diffs:?}");
    assert_eq!(
        cli_diffs[0].file_name(),
        mcp_diffs[0].file_name(),
        "the same diff name on both transports"
    );
    assert_identical_files(&cli_diffs[0], &mcp_diffs[0], "T18 diff image");
}
