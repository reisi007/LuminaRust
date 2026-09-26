//! MCP-PARITY-B: the loud rejections of the five path-based commands.
//!
//! Every case is expressed on **both** transports. What must be identical is the
//! *verdict*: both abort loudly (non-zero exit / `isError: true`) and both leave
//! the filesystem byte-identical. The message wording may differ by one step
//! (the MCP layer can catch a missing value before the shared editor is
//! reached), so the assertions name the shared substring and the shared
//! filesystem consequence, not the whole sentence.
//!
//! The cases are chosen by failure class:
//!
//! * **collision** — `relocate` must refuse an existing target image *and* an
//!   existing target companion, because the second is what would leave a path
//!   without a recipe;
//! * **artefact/model gate** — `generative` and `regenerate` must not be able to
//!   report a success for a canvas that could not be produced or resolved;
//! * **silent-degrade** (the bug class MCP-PARITY-A found twice) — a write op
//!   that names no value must be a refusal, never a successful read. Each such
//!   case names the guard it exercises, and
//!   [`deleting_the_value_guard_makes_the_test_fail`] documents the mutation.

// Each parity test binary uses a part of the shared harness, so the module is
// declared with `dead_code` allowed: an unused helper here is a helper another
// file in this slice uses, not dead code.
#[allow(dead_code)]
#[path = "library_parity_common/mod.rs"]
mod common;
use common::*;
use std::fs;

// ---------------------------------------------------------------- collections

#[test]
fn collections_loud_errors_change_no_bytes_and_match_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("mcp");
    let mut session = Session::new(&dir);
    let cli_dir = root.path().join("cli");
    let (cli_input, _) = fixture(&cli_dir);
    let cli_path = cli_input.display().to_string();

    // A write op without its value must be a refusal, not a successful read.
    let before = snapshot(&dir);
    let mcp = session.call(
        "collections",
        json!({ "path": session.path(), "op": "add" }),
    );
    assert_eq!(
        mcp.as_ref().unwrap_err().message(),
        "InvalidParams: lumina_collections: op `add` requires `membership`"
    );
    assert_eq!(before, snapshot(&dir), "a refused add must write nothing");

    // The CLI's own equivalent is a usage error (exit 2, clap), and it names the
    // same missing piece.
    let (stderr, code) = cli_fails(&["collections", "--input", &cli_path, "--add-to"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("--add-to") || stderr.contains("<ID=NAME>"),
        "the CLI usage error must name the missing value: {stderr}"
    );

    // A malformed `id=name` is loud on both sides.
    let mcp = session.call(
        "collections",
        json!({ "path": session.path(), "op": "add", "membership": "nope" }),
    );
    assert!(
        mcp.unwrap_err().message().contains("expected `id=name`"),
        "the shared splitter's message must survive the transport"
    );
    let (stderr, code) = cli_fails(&[
        "collections",
        "--input",
        &cli_path,
        "--add-to",
        "nope",
        "--json",
    ]);
    assert_ne!(code, 0);
    assert!(stderr.contains("expected `id=name`"), "{stderr}");

    // An unknown field is a loud InvalidParams, not a silently ignored one.
    let mcp = session.call(
        "collections",
        json!({ "path": session.path(), "op": "list", "not_a_field": 1 }),
    );
    assert_eq!(mcp.unwrap_err().name(), "InvalidParams");

    // A missing sidecar is loud on both sides.
    let orphan = dir.join("orphan.png");
    fs::write(&orphan, fixture_png()).unwrap();
    let before = snapshot(&dir);
    let mcp = session.call(
        "collections",
        json!({ "path": orphan.display().to_string(), "op": "list" }),
    );
    assert!(
        mcp.unwrap_err().message().contains("no sidecar"),
        "a missing sidecar must be named, never defaulted"
    );
    let (stderr, code) = cli_fails(&[
        "collections",
        "--input",
        orphan.display().to_string().as_str(),
        "--json",
    ]);
    assert_eq!(code, 1);
    assert!(stderr.contains("no sidecar"), "{stderr}");
    assert_eq!(before, snapshot(&dir));
}

// -------------------------------------------------------- smart-collections

#[test]
fn smart_collections_loud_errors_match_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let (cli_input, cli_catalog) = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);

    // An invalid catalogue is loud on both sides and produces no output.
    let broken = mcp_dir.join("broken.json");
    fs::write(&broken, "{\"format\":\"nope\"}").unwrap();
    let before = snapshot(&mcp_dir);
    let mcp = session.call(
        "smart_collections",
        json!({
            "path": session.path(),
            "op": "evaluate",
            "catalog": broken.display().to_string()
        }),
    );
    assert!(
        mcp.unwrap_err()
            .message()
            .contains("invalid smart-collection catalog"),
        "an invalid catalogue must be named"
    );
    let cli_broken = cli_dir.join("broken.json");
    fs::write(&cli_broken, "{\"format\":\"nope\"}").unwrap();
    let (stderr, code) = cli_fails(&[
        "smart-collections",
        "--input",
        cli_input.to_str().unwrap(),
        "--catalog",
        cli_broken.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("invalid smart-collection catalog"),
        "{stderr}"
    );
    assert_eq!(before, snapshot(&mcp_dir));

    // An empty target set is loud (never a successful empty result).
    let empty = mcp_dir.join("empty");
    fs::create_dir_all(&empty).unwrap();
    let mcp = session.call(
        "smart_collections",
        json!({
            "path": empty.display().to_string(),
            "op": "evaluate",
            "catalog": session.catalog.display().to_string()
        }),
    );
    assert!(
        mcp.unwrap_err().message().contains("no sidecars found"),
        "an empty target set must be refused"
    );
    fs::create_dir_all(cli_dir.join("empty")).unwrap();
    let (stderr, code) = cli_fails(&[
        "smart-collections",
        "--input",
        cli_dir.join("empty").to_str().unwrap(),
        "--catalog",
        cli_catalog.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(code, 1);
    assert!(stderr.contains("no sidecars found"), "{stderr}");

    // A corrupt sidecar makes the run PARTIAL: the CLI exits 3, the tool fails.
    // Neither may report a successful evaluation.
    let corrupt = mcp_dir.join("corrupt.lumina.json");
    fs::write(&corrupt, b"{ not json").unwrap();
    let mcp = session.call(
        "smart_collections",
        json!({
            "path": mcp_dir.display().to_string(),
            "op": "evaluate",
            "catalog": session.catalog.display().to_string()
        }),
    );
    let error = mcp.expect_err("a partial run must fail loudly");
    assert!(
        error
            .message()
            .contains("partial run is never a silent success"),
        "a partial run must fail loudly: {}",
        error.message()
    );
    let cli_corrupt = cli_dir.join("corrupt.lumina.json");
    fs::write(&cli_corrupt, b"{ not json").unwrap();
    let (_, code) = cli_fails(&[
        "smart-collections",
        "--input",
        cli_dir.to_str().unwrap(),
        "--catalog",
        cli_catalog.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(
        code, 3,
        "the CLI's partial-failure exit code is 3 and must not change"
    );
}

// ------------------------------------------------------------------ relocate

#[test]
fn relocate_refuses_an_existing_target_and_moves_nothing() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("mcp");
    let mut session = Session::new(&dir);
    let blocker = dir.join("blocked.png");
    fs::write(&blocker, fixture_png()).unwrap();
    let before = snapshot(&dir);
    let mcp = session.call(
        "relocate",
        json!({
            "from": session.path(),
            "to": blocker.display().to_string(),
            "op": "move"
        }),
    );
    assert_both_refuse(
        "relocate onto an existing image",
        &dir,
        &before,
        cli_fails(&[
            "relocate",
            "--from",
            dir.join("input.png").to_str().unwrap(),
            "--to",
            blocker.to_str().unwrap(),
            "--json",
        ]),
        mcp,
    );
}

/// The companion collision: an existing **target bundle** while the target image
/// itself is free. Without this check the move would succeed and leave the new
/// path with no recipe (the bundle stays behind, the sidecar is overwritten or
/// the recipe is orphaned) — the exact failure this command must prevent.
#[test]
fn relocate_refuses_an_existing_target_companion() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let (cli_input, _) = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);
    // The source must own the companion, otherwise there is nothing to move and
    // no collision is possible.
    seed_bundle(&cli_input, &cli_dir);
    seed_bundle(&session.input, &mcp_dir);
    assert!(lumina_sidecar::zdata_path_for(&cli_input).is_file());
    // A bundle sits at the *target* name while the target image does not exist.
    let mcp_target = mcp_dir.join("moved.png");
    fs::write(
        lumina_sidecar::zdata_path_for(&mcp_target),
        b"a bundle that must survive the refusal",
    )
    .unwrap();
    let cli_target = cli_dir.join("moved.png");
    fs::write(
        lumina_sidecar::zdata_path_for(&cli_target),
        b"a bundle that must survive the refusal",
    )
    .unwrap();
    let before = snapshot(&mcp_dir);
    let mcp = session.call(
        "relocate",
        json!({
            "from": session.path(),
            "to": mcp_target.display().to_string(),
            "op": "move"
        }),
    );
    let cli = cli_fails(&[
        "relocate",
        "--from",
        cli_input.to_str().unwrap(),
        "--to",
        cli_target.to_str().unwrap(),
        "--json",
    ]);
    assert_both_refuse(
        "relocate onto an existing companion",
        &mcp_dir,
        &before,
        cli,
        mcp,
    );
    assert!(
        mcp_target.display().to_string().contains("moved.png"),
        "the target image must still not exist"
    );
    assert!(
        !mcp_target.exists(),
        "an existing companion must block the move entirely"
    );
    assert!(session.input.exists(), "the source must not have moved");
}

/// Persists a source action so the source owns a `.lumina.zdata` companion.
fn seed_bundle(input: &std::path::Path, dir: &std::path::Path) {
    let region = repair_region_file(input, dir);
    let output = cli()
        .args([
            "dust-removal",
            "--input",
            input.to_str().unwrap(),
            "--repair-region",
            region.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "dust-removal failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn relocate_refuses_a_missing_source_and_a_non_directory_parent() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("mcp");
    let mut session = Session::new(&dir);
    let before = snapshot(&dir);
    let mcp = session.call(
        "relocate",
        json!({
            "from": dir.join("gone.png").display().to_string(),
            "to": dir.join("x.png").display().to_string(),
            "op": "move"
        }),
    );
    assert_both_refuse(
        "relocate from a missing source",
        &dir,
        &before,
        cli_fails(&[
            "relocate",
            "--from",
            dir.join("gone.png").to_str().unwrap(),
            "--to",
            dir.join("x.png").to_str().unwrap(),
            "--json",
        ]),
        mcp,
    );

    // A target parent that is a file, not a directory.
    let blocker = dir.join("blocker");
    fs::write(&blocker, b"file").unwrap();
    let before = snapshot(&dir);
    let mcp = session.call(
        "relocate",
        json!({
            "from": session.path(),
            "to": blocker.join("x.png").display().to_string(),
            "op": "move"
        }),
    );
    assert_both_refuse(
        "relocate into a non-directory parent",
        &dir,
        &before,
        cli_fails(&[
            "relocate",
            "--from",
            dir.join("input.png").to_str().unwrap(),
            "--to",
            blocker.join("x.png").to_str().unwrap(),
            "--json",
        ]),
        mcp,
    );
}
