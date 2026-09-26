//! MCP-PARITY-B: the loud rejections of the artefact commands `generative` and
//! `regenerate`, including the model/artefact gate.
//!
//! Split out of `library_parity_errors.rs` by MCP-PARITY-B so neither file grows
//! past the 500-line ratchet. The library-shaped commands (`collections`,
//! `smart-collections`, `relocate`) and the shared harness stay there.

// Each parity test binary uses a part of the shared harness, so the module is
// declared with `dead_code` allowed: an unused helper here is a helper another
// file in this slice uses, not dead code.
#[allow(dead_code)]
#[path = "library_parity_common/mod.rs"]
mod common;
use common::*;
use std::fs;

// ---------------------------------------------------------------- generative

/// The model/artefact gate: a `generate` that cannot produce a canvas is a
/// refusal, and a `status` on a role whose artefact is gone is a refusal too.
/// Neither may report a success, and neither may leave a record or a link.
#[test]
fn generative_cannot_succeed_without_a_producible_canvas() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let (cli_input, _) = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);

    // 1. `generate` with no active role: a model call that would produce nothing.
    let before = snapshot(&mcp_dir);
    let mcp = session.call(
        "generative",
        json!({ "path": session.path(), "op": "generate" }),
    );
    assert_both_refuse(
        "generative generate with no active role",
        &mcp_dir,
        &before,
        cli_fails(&[
            "generative",
            "--input",
            cli_input.to_str().unwrap(),
            "--generate",
            "--json",
        ]),
        mcp,
    );

    // 2. `expand` without a canvas: the role cannot be produced at all.
    let mcp = session.call(
        "generative",
        json!({ "path": session.path(), "op": "generate", "expand": true }),
    );
    assert!(
        mcp.unwrap_err().message().contains("requires `canvas`"),
        "an expand without a canvas must be refused before the model call"
    );

    // 3. After a real production, removing the link makes the role `missing`, so
    //    `status` must FAIL (the CLI exits 1 with "no silent fallback") instead
    //    of reporting an available artefact.
    session.ok(
        "generative",
        json!({
            "path": session.path(), "op": "generate", "expand": true,
            "canvas": "48x48+8+8", "seed": 7
        }),
    );
    session.ok(
        "generative",
        json!({ "path": session.path(), "op": "remove" }),
    );
    let before = snapshot(&mcp_dir);
    let mcp = session.call(
        "generative",
        json!({ "path": session.path(), "op": "status" }),
    );
    let error = mcp.expect_err("a missing artefact must not be a successful status");
    assert!(
        error.message().contains("no silent fallback"),
        "the gate must be named in the error: {}",
        error.message()
    );
    // The CLI says the same and exits non-zero.
    cli_json(&[
        "generative",
        "--input",
        cli_input.to_str().unwrap(),
        "--generate",
        "--expand",
        "--canvas",
        "48x48+8+8",
        "--seed",
        "7",
        "--json",
    ]);
    cli_json(&[
        "generative",
        "--input",
        cli_input.to_str().unwrap(),
        "--remove",
        "--json",
    ]);
    let (stderr, code) = cli_fails(&[
        "generative",
        "--input",
        cli_input.to_str().unwrap(),
        "--status",
        "--json",
    ]);
    assert_ne!(code, 0, "the CLI must fail on a missing artefact");
    assert!(stderr.contains("no silent fallback"), "{stderr}");
    assert_eq!(
        before,
        snapshot(&mcp_dir),
        "a refused status writes nothing"
    );
}

#[test]
fn generative_loud_errors_change_no_bytes_and_match_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let (cli_input, _) = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);

    // Unknown virtual copy.
    let before = snapshot(&mcp_dir);
    let mcp = session.call(
        "generative",
        json!({ "path": session.path(), "op": "status", "virtual_copy": "nope" }),
    );
    assert_both_refuse(
        "generative with an unknown copy",
        &mcp_dir,
        &before,
        cli_fails(&[
            "generative",
            "--input",
            cli_input.to_str().unwrap(),
            "--status",
            "--virtual-copy",
            "nope",
            "--json",
        ]),
        mcp,
    );

    // A malformed canvas spec is loud on both sides.
    let mcp = session.call(
        "generative",
        json!({ "path": session.path(), "op": "generate", "expand": true, "canvas": "bogus" }),
    );
    assert!(
        mcp.unwrap_err().message().contains("expected WxH+X+Y"),
        "a malformed canvas must be named"
    );
    let (stderr, code) = cli_fails(&[
        "generative",
        "--input",
        cli_input.to_str().unwrap(),
        "--generate",
        "--expand",
        "--canvas",
        "bogus",
        "--json",
    ]);
    assert_ne!(code, 0);
    assert!(stderr.contains("expected WxH+X+Y"), "{stderr}");

    // A source that changed since the sidecar was written: the recipe is bound
    // to a content hash, so both sides must refuse.
    let pixels = mutated_png();
    fs::write(&session.input, &pixels).unwrap();
    fs::write(&cli_input, &pixels).unwrap();
    let mcp = session.call(
        "generative",
        json!({ "path": session.path(), "op": "generate", "expand": true, "canvas": "48x48+8+8" }),
    );
    assert!(
        mcp.expect_err("a changed source must be refused")
            .message()
            .contains("source changed"),
        "a changed source must be refused"
    );
    let (stderr, code) = cli_fails(&[
        "generative",
        "--input",
        cli_input.to_str().unwrap(),
        "--generate",
        "--expand",
        "--canvas",
        "48x48+8+8",
        "--json",
    ]);
    assert_ne!(code, 0);
    assert!(stderr.contains("source changed"), "{stderr}");
}

// ---------------------------------------------------------------- regenerate

/// The artefact gate of `regenerate --module matching`.
///
/// **Measured, pre-existing CLI behaviour:** the module re-derives
/// `matched_exposure` from a real render, and it renders with
/// `GenerativeCanvasInput::default()` — exactly as `lumina regenerate` always
/// has. A copy with an **active generative role** therefore cannot be rendered
/// at all: the core stage reports `generative_artifact.expand.missing` (or
/// `…auto_fill.missing`) instead of silently rendering "as if not generated".
/// So the gate is closed for the whole `matching` module on such a copy, whether
/// the artefact is linked or not — and the module must abort loudly and write
/// no sidecar byte. This test pins that verdict on both transports, and pins
/// that a copy **without** a generative role does run, so the refusal cannot be
/// mistaken for "the module is broken".
#[test]
fn regenerate_matching_cannot_succeed_with_an_active_generative_role() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let (cli_input, _) = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);

    // No generative role yet: the module runs and derives a value.
    let report = session.ok(
        "regenerate",
        json!({ "path": session.path(), "op": "matching" }),
    );
    assert_eq!(report["saved"], json!(true), "the module must run");
    let derived = report["modules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["module"] == json!("matching"))
        .and_then(|entry| entry["matched_exposure"].as_f64())
        .expect("a derived exposure must be reported");
    assert!(derived.is_finite(), "{derived}");

    // Now activate the expand role: the module's render is impossible, so it
    // must abort loudly and write nothing.
    session.ok(
        "generative",
        json!({
            "path": session.path(), "op": "generate", "expand": true,
            "canvas": "48x48+8+8", "seed": 7
        }),
    );
    let before = snapshot(&mcp_dir);
    let mcp = session.call(
        "regenerate",
        json!({ "path": session.path(), "op": "matching" }),
    );
    let error = mcp.expect_err("matching must not succeed with an active generative role");
    assert!(
        error.message().contains("generative_artifact"),
        "the failing stage must be named: {}",
        error.message()
    );
    assert_eq!(
        before,
        snapshot(&mcp_dir),
        "an aborted module must not write a single byte"
    );

    // The CLI reaches the same verdict on the same state.
    cli_json(&[
        "generative",
        "--input",
        cli_input.to_str().unwrap(),
        "--generate",
        "--expand",
        "--canvas",
        "48x48+8+8",
        "--seed",
        "7",
        "--json",
    ]);
    let (stderr, code) = cli_fails(&[
        "regenerate",
        "--input",
        cli_input.to_str().unwrap(),
        "--module",
        "matching",
        "--json",
    ]);
    assert_ne!(code, 0, "the CLI must fail on the same state");
    assert!(stderr.contains("generative_artifact"), "{stderr}");

    // Unlinking the artefact does NOT reopen the gate: the role is still active,
    // so the module still refuses. A `missing` artefact is never re-adopted
    // silently, and the render is never "as if not generated".
    session.ok(
        "generative",
        json!({ "path": session.path(), "op": "remove" }),
    );
    let before = snapshot(&mcp_dir);
    let mcp = session.call(
        "regenerate",
        json!({ "path": session.path(), "op": "matching" }),
    );
    assert!(
        mcp.expect_err("an unlinked artefact must not reopen the gate")
            .message()
            .contains("generative_artifact"),
        "the failing stage must still be named"
    );
    assert_eq!(before, snapshot(&mcp_dir));
}

#[test]
fn regenerate_loud_errors_change_no_bytes_and_match_the_cli() {
    let root = tempfile::tempdir().unwrap();
    let (cli_dir, mcp_dir) = (root.path().join("cli"), root.path().join("mcp"));
    let (cli_input, _) = fixture(&cli_dir);
    let mut session = Session::new(&mcp_dir);

    // A non-finite / out-of-range target luminance.
    let before = snapshot(&mcp_dir);
    let mcp = session.call(
        "regenerate",
        json!({ "path": session.path(), "op": "all", "target_luminance": 2.0 }),
    );
    assert_both_refuse(
        "regenerate with an out-of-range target luminance",
        &mcp_dir,
        &before,
        cli_fails(&[
            "regenerate",
            "--input",
            cli_input.to_str().unwrap(),
            "--target-luminance",
            "2",
            "--json",
        ]),
        mcp,
    );

    // An unknown virtual copy.
    let mcp = session.call(
        "regenerate",
        json!({ "path": session.path(), "op": "all", "virtual_copy": "nope" }),
    );
    assert!(
        mcp.unwrap_err().message().contains("unknown virtual copy"),
        "an unknown copy must be named"
    );

    // An unknown op and an unknown field are both loud InvalidParams.
    assert_eq!(
        session
            .call(
                "regenerate",
                json!({ "path": session.path(), "op": "set_everything" })
            )
            .unwrap_err()
            .name(),
        "InvalidParams"
    );
    assert_eq!(
        session
            .call(
                "regenerate",
                json!({ "path": session.path(), "op": "all", "nope": 1 })
            )
            .unwrap_err()
            .name(),
        "InvalidParams"
    );

    // A wrong type for `target_luminance` is refused rather than coerced.
    assert_eq!(
        session
            .call(
                "regenerate",
                json!({ "path": session.path(), "op": "all", "target_luminance": "0.5" })
            )
            .unwrap_err()
            .name(),
        "InvalidParams"
    );
}

// ------------------------------------------------------- silent-degrade guards

/// The bug class MCP-PARITY-A found twice: an op that silently degrades into a
/// successful read when a required value is missing. Every such guard is
/// exercised above; this test states the contract in one place and fails if a
/// guard is ever removed, because a removed guard turns the call into a
/// successful read — which the assertions below detect explicitly.
#[test]
fn a_write_op_without_its_value_is_refused_rather_than_a_no_op() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("mcp");
    let mut session = Session::new(&dir);
    let path = session.path();
    let cases: Vec<(&str, Value, &str)> = vec![
        (
            "collections",
            json!({ "path": path, "op": "add" }),
            "op `add` requires `membership`",
        ),
        (
            "collections",
            json!({ "path": path, "op": "remove" }),
            "op `remove` requires `id`",
        ),
        (
            "generative",
            json!({ "path": path, "op": "generate" }),
            "op `generate` requires `expand or auto_fill`",
        ),
        (
            "generative",
            json!({ "path": path, "op": "generate", "expand": true }),
            "op `generate` requires `canvas`",
        ),
    ];
    for (tool, arguments, fragment) in cases {
        let before = snapshot(&dir);
        let error = session
            .call(tool, arguments.clone())
            .expect_err(&format!("{tool} {arguments} must be refused"));
        assert_eq!(error.name(), "InvalidParams", "{tool} {arguments}");
        assert!(
            error.message().contains(fragment),
            "{tool} {arguments}: `{}` does not mention `{fragment}`",
            error.message()
        );
        assert_eq!(
            before,
            snapshot(&dir),
            "{tool} {arguments} must not write bytes"
        );
    }
}

/// `relocate` has no optional value, so the only way it can be a silent no-op
/// would be to accept a call without a destination. Both are required, and the
/// guard is named.
#[test]
fn relocate_requires_both_paths() {
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::new(&root.path().join("mcp"));
    for arguments in [
        json!({ "from": session.path(), "op": "move" }),
        json!({ "to": "x", "op": "move" }),
    ] {
        let error = session
            .call("relocate", arguments.clone())
            .expect_err("a missing path must be refused");
        assert_eq!(error.name(), "InvalidParams", "{arguments}");
        assert!(
            error.message().contains("missing string argument"),
            "{arguments}: {}",
            error.message()
        );
    }
}
