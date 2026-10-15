//! MCP-PARITY-C: the green premise every parity test starts from.
//!
//! Two anchor trees plus a green verify on both transports: baseline on both,
//! then verify on both. Every test in `matrix_mcp_parity*.rs` starts here and
//! then varies one input — so a failure is attributable to that variation,
//! never to the fixture.

#![allow(dead_code)]

use super::fixtures::anchor_tree;
use super::graders::both_trees;
use super::graders::snapshot;
use super::job::MatrixCall;
use serde_json::Value;
use std::path::PathBuf;

/// Two anchor trees plus a green premise: baseline on both transports, then a
/// green verify on both.
pub struct Green {
    /// Keeps the tempdir alive for the whole test.
    pub _root: tempfile::TempDir,
    pub cli_dir: PathBuf,
    pub mcp_dir: PathBuf,
    pub cli_report: Value,
    pub mcp_payload: Value,
}

/// Builds two byte-identical anchor trees, baselines both (CPU-pinned) and
/// verifies both green. The MCP side runs over the real `tools/call` seam —
/// what an agent sees.
pub fn green_trees() -> Green {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = both_trees(root.path());
    anchor_tree(&cli_dir);
    anchor_tree(&mcp_dir);
    assert_eq!(
        snapshot(&cli_dir),
        snapshot(&mcp_dir),
        "the two input trees must start byte-identical"
    );
    let cli_set = cli_dir.join("recipe-set.json");
    let mcp_set = mcp_dir.join("recipe-set.json");

    // Baseline first (CPU-pinned): the goldens the verify compares against.
    let cli_base = MatrixCall::verify(cli_set.clone(), cli_dir.join("work"))
        .baseline()
        .cli_json();
    let mcp_base = MatrixCall::verify(mcp_set.clone(), mcp_dir.join("work"))
        .baseline()
        .mcp_seam();
    assert_eq!(cli_base["mode"], serde_json::json!("baseline"));
    assert_eq!(mcp_base["mode"], serde_json::json!("baseline"));

    let cli_report = MatrixCall::verify(cli_set, cli_dir.join("work")).cli_json();
    let mcp_payload = MatrixCall::verify(mcp_set, mcp_dir.join("work")).mcp_seam();
    Green {
        _root: root,
        cli_dir,
        mcp_dir,
        cli_report,
        mcp_payload,
    }
}
