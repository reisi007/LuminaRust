//! MCP-PARITY-C: the Real-RAW gate (SOLL §8, T26 [G]).
//!
//! One `#[ignore]`d test in its own file, because RAW bytes are the one input
//! class the default fixture set cannot afford: a matrix run over real camera
//! frames decodes through LibRaw and renders full-resolution pairs, so the
//! CLI≡MCP proof is slow by construction. The default gate of this slice
//! stays on generated PNGs (`matrix_mcp_parity.rs`, `matrix_mcp_parity_errors.rs`);
//! the RAW proof is invoked explicitly instead —
//! `LUMINA_MATRIX=1 cargo test -p lumina-cli --test matrix_mcp_parity_gate --
//! --ignored`, the same shape as `matrix_e2e.rs:421`.
//!
//! # What the gate proves
//!
//! The committed set (`testdata/matrix/recipe-set.v1.json`: 2 samples, 11
//! recipes, 10× `gpu` + 1× documented CPU exemption) on a bounded recipe
//! subset (`g01-identity` + `g01-tone-wb`, the `matrix_e2e.rs:432-443`
//! pattern): MCP-`structuredContent` == CLI-`--json` field for field, every
//! pair green, the `expected_route` echo delivered unchanged — and the
//! committed files byte-identical before and after (the gate **never** writes
//! goldens: no `--update-goldens` on either transport).
//!
//! # What is deliberately NOT claimed
//!
//! Full-set parity (the nightly owns the 22-pair run,
//! `.github/workflows/matrix-nightly.yml`) and the GPU-parity verdict (§5:
//! not part of this slice). This gate proves the *path* — real CR3 bytes
//! through LibRaw on both transports with identical reports — on the bounded
//! subset a debug build finishes in reasonable time.

// CLI-NO-DEFAULT-TESTBUILD-80: the whole target is CLI<->MCP parity work; the
// shared harness needs `lumina_mcp`, which only exists under the default `mcp`
// feature, so the file is gated on that feature as a whole.
#![cfg(feature = "mcp")]

#[path = "matrix_mcp_parity_common/mod.rs"]
mod common;
use common::{fixtures::*, graders::*, job::*};
use serde_json::json;
use std::path::{Path, PathBuf};

/// The committed recipe set, resolved from `CARGO_MANIFEST_DIR` so the gate
/// does not depend on the test process' working directory.
fn committed_set() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/matrix/recipe-set.v1.json")
}

/// The committed golden directory the set resolves to by default.
fn committed_golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/matrix/golden")
}

/// T26 — Real-RAW parity on the committed set (bounded subset).
///
/// # Why this is `#[ignore]` and not part of the default run
///
/// Real CR3 bytes through LibRaw plus full-resolution renders are slow by
/// construction; the default suite proves transport parity on the synthetic
/// anchor instead. Invoke on purpose (SOLL §9 T26, §10 (11)–(12)):
/// `LUMINA_MATRIX=1 cargo test -p lumina-cli --test matrix_mcp_parity_gate --
/// --ignored`.
#[test]
#[ignore = "T26 Real-RAW gate: CR3 bytes through LibRaw on both transports - invoke on purpose: LUMINA_MATRIX=1 cargo test -p lumina-cli --test matrix_mcp_parity_gate -- --ignored (SOLL §9 T26)"]
fn t26_real_raw_subset_is_identical_on_both_transports() {
    if std::env::var("LUMINA_MATRIX").ok().as_deref() != Some("1") {
        lumina_testskip::report_env_gate(&lumina_testskip::env_gate(
            "t26_real_raw_subset_is_identical_on_both_transports",
            "LUMINA_MATRIX=1",
            "LUMINA_MATRIX=1 cargo test -p lumina-cli --test matrix_mcp_parity_gate -- --ignored \
             t26_real_raw_subset_is_identical_on_both_transports",
        ));
        return;
    }
    let set = committed_set();
    assert!(
        set.is_file(),
        "the committed set must exist: {}",
        set.display()
    );

    // The committed files, fingerprinted before the runs: the gate reads
    // them and must not move one byte (§4.2, §10 (12)).
    let set_before = bytes(&set);
    let golden_before = snapshot(&committed_golden_dir());

    // One work dir per transport: the exports have to land somewhere, and the
    // default (a fresh tempdir each) cannot agree on an address. Both
    // transports share the committed set and goldens; only the work dirs
    // differ, so the report compare stays a pure state compare.
    let root = tempfile::tempdir().unwrap();
    let (cli_work, mcp_work) = (root.path().join("cli-work"), root.path().join("mcp-work"));
    let cli_call =
        MatrixCall::verify(set.clone(), cli_work.clone()).recipes(&["g01-identity", "g01-tone-wb"]);
    let mcp_call =
        MatrixCall::verify(set.clone(), mcp_work.clone()).recipes(&["g01-tone-wb", "g01-identity"]);

    // NOTE: the recipe order differs on purpose — the CLI spells `--recipe`
    // in the given order while MCP passes one array. The runner selects in
    // set order either way (`prepare_recipes`), so the reports must still
    // agree; a transport that honoured call order would show up here.
    let cli_report = cli_call.cli_json();
    let mcp_payload = mcp_call.mcp_seam();

    // §10 (11): the bounded subset is green on both, pair for pair.
    for (label, payload) in [("CLI", &cli_report), ("MCP", &mcp_payload)] {
        assert_eq!(payload["command"], json!("matrix"), "{label}: command");
        assert_eq!(payload["mode"], json!("verify"), "{label}: mode");
        assert_eq!(
            payload["pairs_total"],
            json!(4),
            "{label}: 2 samples x 2 recipes"
        );
        assert_eq!(payload["passed"], json!(4), "{label}: passed");
        assert_eq!(payload["failed"], json!(0), "{label}: failed");
        for pair in payload["pairs"].as_array().unwrap() {
            assert_eq!(pair["status"], json!("pass"), "{label}: {pair}");
        }
    }
    // The two work dirs are the only addresses that differ (the committed
    // set and goldens are shared), so they are the normalisation roots.
    assert_same_report(
        "T26 committed subset",
        &mcp_payload,
        &cli_report,
        &mcp_work,
        &cli_work,
    );

    // §10 (12): the declared routes are delivered unchanged — and identically.
    let routes = |payload: &serde_json::Value| -> Vec<String> {
        payload["pairs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| pair["expected_route"].to_string())
            .collect()
    };
    assert_eq!(routes(&mcp_payload), routes(&cli_report));
    assert!(
        !routes(&mcp_payload).is_empty(),
        "the echo must be delivered"
    );

    // The committed files are untouched: the set byte-identical, every golden
    // fingerprinted identical. No `--update-goldens` was passed anywhere.
    assert_eq!(bytes(&set), set_before, "the committed set is read-only");
    assert_eq!(
        snapshot(&committed_golden_dir()),
        golden_before,
        "the committed goldens are read-only"
    );
}
