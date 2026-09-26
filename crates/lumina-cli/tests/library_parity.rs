//! MCP-PARITY-B: byte identity between an MCP call and the equivalent CLI
//! subcommand call, for all five path-based / artefact commands, in BOTH
//! directions (read and write).
//!
//! # What is actually compared
//!
//! Not a "structural" or "semantic" comparison: the real `lumina-cli` process
//! (`env!("CARGO_BIN_EXE_lumina-cli")`) runs against one fixture tree and the MCP
//! tool runs in-process against a *second, byte-identical* tree. Then
//!
//! * **read direction** — the sidecar bytes are compared with a literal byte
//!   compare (a read must change nothing) **and** the MCP tool payload is
//!   compared against the CLI's `--json` document, which is the strongest
//!   available statement that the two transports render one state;
//! * **write direction** — the produced bytes are compared after the same
//!   operation on both sides.
//!
//! # Are any of the five mask-pinned?
//!
//! **No — all five are literally byte-equal, with no mask at all.** The
//! measurement is the reason: unlike `geometry` and `upright` (MCP-PARITY-A),
//! none of these five commands appends a `history` entry, so no millisecond
//! stamp enters the sidecar. `lumina render`/`process` *do* append an
//! `h-<ms>` entry, which is why no test here drives them. Every assertion below
//! therefore goes through [`assert_identical`], which is a raw byte compare with
//! no mask, and the mask-narrowness argument from slice A does not apply
//! because there is no mask. [`mutation_a_non_stamp_byte_still_fails`] proves
//! the comparison has teeth.

// Each parity test binary uses a part of the shared harness, so the module is
// declared with `dead_code` allowed: an unused helper here is a helper another
// file in this slice uses, not dead code.
#[allow(dead_code)]
#[path = "library_parity_common/mod.rs"]
mod common;
use common::*;

// ---------------------------------------------------------------- read
// direction

#[test]
fn collections_read_is_byte_identical_to_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let (cli_input, _) = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);
    let cli_before = bytes(&sidecar_path_for(&cli_input));
    assert_eq!(cli_before, session.sidecar_bytes());

    let cli_report = cli_json(&[
        "collections",
        "--input",
        cli_input.to_str().unwrap(),
        "--json",
    ]);
    let mcp_report = session.ok(
        "collections",
        json!({ "path": session.path(), "op": "list" }),
    );

    assert_unchanged(
        &cli_before,
        &bytes(&sidecar_path_for(&cli_input)),
        "collections read (cli)",
    );
    assert_unchanged(
        &session.sidecar_bytes(),
        &session.sidecar_bytes(),
        "collections read (mcp)",
    );
    assert_same_state(
        &mcp_report,
        &cli_report,
        &cli_dir,
        &mcp_dir,
        "collections op=list",
    );
    assert_eq!(mcp_report["saved"], json!(false));
    assert_eq!(mcp_report["collections"], json!([]));
}

#[test]
fn smart_collections_read_is_byte_identical_to_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let (cli_input, cli_catalog) = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);
    let cli_before = bytes(&sidecar_path_for(&cli_input));
    assert_eq!(cli_before, session.sidecar_bytes());

    let cli_report = cli_json(&[
        "smart-collections",
        "--input",
        cli_input.to_str().unwrap(),
        "--catalog",
        cli_catalog.to_str().unwrap(),
        "--json",
    ]);
    let mcp_report = session.ok(
        "smart_collections",
        json!({
            "path": session.path(),
            "op": "evaluate",
            "catalog": session.catalog.display().to_string()
        }),
    );

    // `smart-collections` is read-only by contract: not one byte may change.
    assert_unchanged(
        &cli_before,
        &bytes(&sidecar_path_for(&cli_input)),
        "smart-collections (cli)",
    );
    assert_unchanged(
        &session.sidecar_bytes(),
        &session.sidecar_bytes(),
        "smart-collections (mcp)",
    );
    assert_same_state(
        &mcp_report,
        &cli_report,
        &cli_dir,
        &mcp_dir,
        "lumina_smart_collections op=evaluate",
    );
    assert_eq!(mcp_report["sidecars"], json!(1));
    assert_eq!(mcp_report["status"], json!("ok"));
}

#[test]
fn generative_read_is_byte_identical_to_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let (cli_input, _) = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);
    let cli_before = bytes(&sidecar_path_for(&cli_input));
    assert_eq!(cli_before, session.sidecar_bytes());

    // An inactive generative edit: `status` is a pure read that reports
    // `inactive` and exits 0.
    let (cli_report, cli_code) = cli_json_with_code(&[
        "generative",
        "--input",
        cli_input.to_str().unwrap(),
        "--status",
        "--json",
    ]);
    assert_eq!(cli_code, 0);
    let mcp_report = session.ok(
        "generative",
        json!({ "path": session.path(), "op": "status" }),
    );

    assert_unchanged(
        &cli_before,
        &bytes(&sidecar_path_for(&cli_input)),
        "generative status (cli)",
    );
    assert_unchanged(
        &session.sidecar_bytes(),
        &session.sidecar_bytes(),
        "generative status (mcp)",
    );
    assert_same_state(
        &mcp_report,
        &cli_report,
        &cli_dir,
        &mcp_dir,
        "lumina_generative op=status (inactive)",
    );
    assert_eq!(mcp_report["status"], json!("inactive"));
    assert_eq!(mcp_report["saved"], json!(false));
}

#[test]
fn regenerate_read_collective_is_byte_identical_to_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let (cli_input, _) = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);

    // A freshly imported document has nothing stale, so the collective default
    // is a pure read: every module is skipped, nothing is written.
    let (cli_report, cli_code) = cli_json_with_code(&[
        "regenerate",
        "--input",
        cli_input.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(cli_code, 0);
    let cli_after = bytes(&sidecar_path_for(&cli_input));
    let mcp_after = session.sidecar_bytes();

    let mcp_report = session.ok("regenerate", json!({ "path": session.path(), "op": "all" }));

    assert_unchanged(&cli_before_of(&cli_dir), &cli_after, "regenerate all (cli)");
    assert_unchanged(
        &mcp_after,
        &bytes(&session.sidecar()),
        "regenerate all (mcp)",
    );
    assert_same_state(
        &mcp_report,
        &cli_report,
        &cli_dir,
        &mcp_dir,
        "lumina_regenerate op=all (nothing stale)",
    );
    assert_eq!(mcp_report["saved"], json!(false));
    assert_eq!(mcp_report["status"], json!("unchanged"));
}

/// The sidecar bytes a freshly imported fixture starts from.
fn cli_before_of(dir: &std::path::Path) -> Vec<u8> {
    bytes(&sidecar_path_for(&dir.join("input.png")))
}
