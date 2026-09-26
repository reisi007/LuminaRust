//! MCP-PARITY-B: the **write** direction of the byte-identity claim for the five
//! path-based commands.
//!
//! Split out of `library_parity.rs` by MCP-PARITY-B so neither file grows past
//! the 500-line ratchet (a new file may not get a baseline entry). The read
//! direction lives there; this file covers the same five commands in the
//! direction that writes, plus the mutation test that proves the byte compares
//! have teeth.

// Each parity test binary uses a part of the shared harness, so the module is
// declared with `dead_code` allowed: an unused helper here is a helper another
// file in this slice uses, not dead code.
#[allow(dead_code)]
#[path = "library_parity_common/mod.rs"]
mod common;
use common::*;

// --------------------------------------------------------------- write
// direction

#[test]
fn collections_write_is_byte_identical_to_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let (cli_input, _) = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);

    // Write #1: add two memberships in the CLI, one call per membership on the
    // MCP side (the MCP tool takes exactly one membership per call).
    let cli_report = cli_json(&[
        "collections",
        "--input",
        cli_input.to_str().unwrap(),
        "--add-to",
        "holiday=Holiday",
        "--add-to",
        "trip=Trip",
        "--json",
    ]);
    session.ok(
        "collections",
        json!({ "path": session.path(), "op": "add", "membership": "holiday=Holiday" }),
    );
    let mcp_report = session.ok(
        "collections",
        json!({ "path": session.path(), "op": "add", "membership": "trip=Trip" }),
    );

    // No `history` entry is appended, so this is a literal byte compare.
    assert_identical(
        &sidecar_path_for(&cli_input),
        &session.sidecar(),
        "collections add x2",
    );
    assert_same_state(
        &mcp_report,
        &cli_report,
        &cli_dir,
        &mcp_dir,
        "lumina_collections op=add",
    );
    assert_eq!(mcp_report["saved"], json!(true));
    assert_eq!(mcp_report["changed"], json!(true));
    assert_eq!(mcp_report["action"], json!("add-to:trip"));

    // Write #2: remove one membership (idempotent on both sides).
    let cli_report = cli_json(&[
        "collections",
        "--input",
        cli_input.to_str().unwrap(),
        "--remove-from",
        "trip",
        "--json",
    ]);
    let mcp_report = session.ok(
        "collections",
        json!({ "path": session.path(), "op": "remove", "id": "trip" }),
    );
    assert_identical(
        &sidecar_path_for(&cli_input),
        &session.sidecar(),
        "collections remove",
    );
    assert_same_state(
        &mcp_report,
        &cli_report,
        &cli_dir,
        &mcp_dir,
        "lumina_collections op=remove",
    );

    // Write #3: the idempotent no-op. Both sides report `changed: false` and
    // neither rewrites a byte.
    let before = session.sidecar_bytes();
    let cli_report = cli_json(&[
        "collections",
        "--input",
        cli_input.to_str().unwrap(),
        "--add-to",
        "holiday=Holiday",
        "--json",
    ]);
    let mcp_report = session.ok(
        "collections",
        json!({ "path": session.path(), "op": "add", "membership": "holiday=Holiday" }),
    );
    assert_unchanged(
        &before,
        &session.sidecar_bytes(),
        "collections idempotent no-op (mcp)",
    );
    assert_identical(
        &sidecar_path_for(&cli_input),
        &session.sidecar(),
        "collections idempotent no-op",
    );
    assert_eq!(mcp_report["changed"], json!(false));
    assert_eq!(mcp_report["saved"], json!(false));
    assert_same_state(
        &mcp_report,
        &cli_report,
        &cli_dir,
        &mcp_dir,
        "lumina_collections idempotent no-op",
    );
}

#[test]
fn relocate_write_is_byte_identical_to_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let (cli_input, _) = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);
    // Give the fixture a bundle so the companion move is exercised, not just the
    // sidecar: `dust-removal` persists a source action into `.lumina.zdata`.
    persist_bundle(&cli_input, &cli_dir);
    persist_bundle(&session.input, &mcp_dir);
    assert!(lumina_sidecar::zdata_path_for(&cli_input).is_file());

    let cli_target = cli_dir.join("moved.png");
    let mcp_target = mcp_dir.join("moved.png");
    let cli_report = cli_json(&[
        "relocate",
        "--from",
        cli_input.to_str().unwrap(),
        "--to",
        cli_target.to_str().unwrap(),
        "--json",
    ]);
    let mcp_report = session.ok(
        "relocate",
        json!({
            "from": session.path(),
            "to": mcp_target.display().to_string(),
            "op": "move"
        }),
    );

    // The image moved …
    assert!(!cli_input.exists(), "the source image must be gone");
    assert!(!session.input.exists(), "the source image must be gone");
    assert!(cli_target.is_file() && mcp_target.is_file());
    // … with the recipe attached (the point of the command) …
    assert!(
        sidecar_path_for(&cli_target).is_file(),
        "the sidecar must travel with the image"
    );
    assert!(lumina_sidecar::zdata_path_for(&mcp_target).is_file());
    // … and byte-identically.
    assert_identical(&cli_target, &mcp_target, "relocate image");
    assert_identical(
        &sidecar_path_for(&cli_target),
        &sidecar_path_for(&mcp_target),
        "relocate sidecar",
    );
    assert_identical(
        &lumina_sidecar::zdata_path_for(&cli_target),
        &lumina_sidecar::zdata_path_for(&mcp_target),
        "relocate bundle",
    );
    assert_same_state(
        &mcp_report,
        &cli_report,
        &cli_dir,
        &mcp_dir,
        "lumina_relocate op=move",
    );
    assert_eq!(mcp_report["saved"], json!(true));
    assert_eq!(mcp_report["action"], json!("image,sidecar,bundle"));
}

/// Persists a source action so a `.lumina.zdata` bundle exists next to `input`.
fn persist_bundle(input: &std::path::Path, dir: &std::path::Path) {
    let region = repair_region_file(input, dir);
    let args = [
        "dust-removal",
        "--input",
        input.to_str().unwrap(),
        "--repair-region",
        region.to_str().unwrap(),
    ];
    let output = cli().args(args).output().unwrap();
    assert!(
        output.status.success(),
        "dust-removal failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn generative_write_is_byte_identical_to_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let (cli_input, _) = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);

    // The double-role producer: the fixture's transparent corner makes the
    // auto-fill role required, and the expand role adds a canvas.
    let cli_report = cli_json(&[
        "generative",
        "--input",
        cli_input.to_str().unwrap(),
        "--generate",
        "--expand",
        "--auto-fill",
        "--canvas",
        "48x48+8+8",
        "--seed",
        "7",
        "--prompt",
        "parity",
        "--json",
    ]);
    let mcp_report = session.ok(
        "generative",
        json!({
            "path": session.path(),
            "op": "generate",
            "expand": true,
            "auto_fill": true,
            "canvas": "48x48+8+8",
            "seed": 7,
            "prompt": "parity"
        }),
    );

    // No `history` entry, so this is a literal byte compare of the sidecar AND
    // of the produced canvas bundle: the same model, the same record id and the
    // same pixels.
    assert_identical(
        &sidecar_path_for(&cli_input),
        &session.sidecar(),
        "generative generate (sidecar)",
    );
    assert_identical(
        &lumina_sidecar::zdata_path_for(&cli_input),
        &session.zdata(),
        "generative generate (bundle)",
    );
    assert_same_state(
        &mcp_report,
        &cli_report,
        &cli_dir,
        &mcp_dir,
        "lumina_generative op=generate",
    );
    assert_eq!(mcp_report["status"], json!("generated"));
    assert_eq!(mcp_report["saved"], json!(true));
    // The double-role document keeps the per-role list on both sides.
    assert_eq!(mcp_report["roles"].as_array().unwrap().len(), 2);

    // A second write: `remove` unlinks the artefact and keeps the bundle record.
    let (cli_report, cli_code) = cli_json_with_code(&[
        "generative",
        "--input",
        cli_input.to_str().unwrap(),
        "--remove",
        "--json",
    ]);
    assert_eq!(cli_code, 0);
    let mcp_report = session.ok(
        "generative",
        json!({ "path": session.path(), "op": "remove" }),
    );
    assert_identical(
        &sidecar_path_for(&cli_input),
        &session.sidecar(),
        "generative remove (sidecar)",
    );
    assert_identical(
        &lumina_sidecar::zdata_path_for(&cli_input),
        &session.zdata(),
        "generative remove keeps the bundle record",
    );
    // `--remove` prints nothing on `--json`; the MCP envelope still says what
    // happened, so an agent never receives an empty object.
    assert_eq!(mcp_report["saved"], json!(true));
    assert_eq!(mcp_report["action"], json!("remove"));
    assert!(
        cli_report.is_null(),
        "`generative --remove --json` prints no document, as before the extraction"
    );
}

#[test]
fn regenerate_write_is_byte_identical_to_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let (cli_input, _) = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);

    // `auto_tone`: the six sliders, the six mirrors and the analysis fingerprint
    // through the single shared write path. No `history` entry, so literal.
    let cli_report = cli_json(&[
        "regenerate",
        "--input",
        cli_input.to_str().unwrap(),
        "--module",
        "auto-tone",
        "--json",
    ]);
    let mcp_report = session.ok(
        "regenerate",
        json!({ "path": session.path(), "op": "auto_tone" }),
    );
    assert_identical(
        &sidecar_path_for(&cli_input),
        &session.sidecar(),
        "regenerate auto_tone",
    );
    assert_same_state(
        &mcp_report,
        &cli_report,
        &cli_dir,
        &mcp_dir,
        "lumina_regenerate op=auto_tone",
    );
    assert_eq!(mcp_report["saved"], json!(true));
    assert_eq!(mcp_report["status"], json!("updated"));

    // `matching`: `matched_exposure` is re-derived from a real render. The
    // derived float lands in the sidecar, so this byte compare is the strongest
    // statement that both transports ran the same render and the same matching
    // maths.
    let cli_report = cli_json(&[
        "regenerate",
        "--input",
        cli_input.to_str().unwrap(),
        "--module",
        "matching",
        "--json",
    ]);
    let mcp_report = session.ok(
        "regenerate",
        json!({ "path": session.path(), "op": "matching" }),
    );
    assert_identical(
        &sidecar_path_for(&cli_input),
        &session.sidecar(),
        "regenerate matching",
    );
    assert_same_state(
        &mcp_report,
        &cli_report,
        &cli_dir,
        &mcp_dir,
        "lumina_regenerate op=matching",
    );
    let modules = mcp_report["modules"].as_array().unwrap();
    let matched = modules
        .iter()
        .find(|entry| entry["module"] == json!("matching"))
        .expect("the matching module must be reported");
    assert!(
        matched["matched_exposure"].is_number(),
        "the derived exposure must be reported: {matched}"
    );

    // A second collective run: both modules are now fresh, so both sides skip
    // them and neither writes a byte.
    let before = session.sidecar_bytes();
    let (cli_report, _) = cli_json_with_code(&[
        "regenerate",
        "--input",
        cli_input.to_str().unwrap(),
        "--json",
    ]);
    let mcp_report = session.ok("regenerate", json!({ "path": session.path(), "op": "all" }));
    assert_unchanged(
        &before,
        &session.sidecar_bytes(),
        "regenerate all (mcp, fresh)",
    );
    assert_identical(
        &sidecar_path_for(&cli_input),
        &session.sidecar(),
        "regenerate all (fresh)",
    );
    assert_eq!(mcp_report["saved"], json!(false));
    assert_same_state(
        &mcp_report,
        &cli_report,
        &cli_dir,
        &mcp_dir,
        "lumina_regenerate op=all (everything fresh)",
    );
}

/// The byte compares above are raw (no mask). This proves they have teeth: a
/// difference in ANY byte must fail, which is what makes "byte-identical, with
/// no mask" a claim rather than a hope.
#[test]
fn mutation_a_non_stamp_byte_still_fails() {
    let root = tempfile::tempdir().unwrap();
    let (a_dir, b_dir) = (root.path().join("a"), root.path().join("b"));
    let (a_input, _) = fixture(&a_dir);
    let mut session = Session::new(&b_dir);
    session.ok(
        "collections",
        json!({ "path": session.path(), "op": "add", "membership": "holiday=Holiday" }),
    );
    cli_json(&[
        "collections",
        "--input",
        a_input.to_str().unwrap(),
        "--add-to",
        "holiday=Holiday",
        "--json",
    ]);
    // The unmutated pair really is identical …
    assert_identical(
        &sidecar_path_for(&a_input),
        &session.sidecar(),
        "unmutated pair",
    );

    // … and a single flipped byte in one of them must break the comparison.
    let mut mutated = session.sidecar_bytes();
    let marker = br#"recipe_version": "1""#;
    let at = mutated
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("the recipe_version marker");
    let digit = mutated[at + marker.len() - 1];
    mutated[at + marker.len() - 1] = if digit == b'1' { b'9' } else { b'1' };
    assert_ne!(
        mutated,
        session.sidecar_bytes(),
        "the mutation must actually change a byte"
    );
    let mutated_path = b_dir.join("mutated.lumina.json");
    std::fs::write(&mutated_path, &mutated).unwrap();
    let comparison = std::panic::catch_unwind(|| {
        assert_identical(&sidecar_path_for(&a_input), &mutated_path, "mutated pair")
    });
    assert!(
        comparison.is_err(),
        "a difference in a non-stamp byte MUST fail the byte compare; a comparison that \
         tolerates it does not prove byte identity"
    );
}
