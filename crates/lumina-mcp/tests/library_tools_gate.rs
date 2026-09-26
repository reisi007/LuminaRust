//! MCP-PARITY-B: the model/artefact gate and the read/write split of the five
//! path-based MCP tools.
//!
//! Split out of `library_tools.rs` by MCP-PARITY-B so neither file grows past the
//! 500-line ratchet. The registry and schema contracts live there; this file
//! proves that neither `lumina_generative` nor `lumina_regenerate` can report a
//! success for a canvas that could not be produced or resolved.

use serde_json::json;

#[path = "library_tools_common/mod.rs"]
mod support;
use support::*;

// ---------------------------------------------------- the model/artefact gate

/// `lumina_generative op="generate"` cannot report a success when no canvas can
/// be produced: no active role, and an `expand` without a canvas. Both are
/// refusals, and neither leaves a record, a link or a sidecar byte.
#[test]
fn generative_cannot_report_success_without_a_producible_canvas() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("t");
    let input = write_fixture(&dir);
    let path = input.display().to_string();
    let before = snapshot(&dir);

    // No active role at all.
    let payload = err(
        "lumina_generative",
        json!({ "path": path, "op": "generate" }),
    );
    assert_eq!(error_name(&payload), "InvalidParams");
    assert!(
        payload["message"]
            .as_str()
            .unwrap()
            .contains("expand or auto_fill"),
        "{}",
        payload["message"]
    );

    // An active role that cannot produce anything: `expand` without a canvas.
    let payload = err(
        "lumina_generative",
        json!({ "path": path, "op": "generate", "expand": true }),
    );
    assert!(
        payload["message"]
            .as_str()
            .unwrap()
            .contains("requires `canvas`"),
        "{}",
        payload["message"]
    );

    assert_eq!(before, snapshot(&dir), "a refused generate wrote bytes");
    assert!(
        !lumina_sidecar::zdata_path_for(&input).exists(),
        "no canvas bundle may exist after a refused generate"
    );
}

/// `lumina_generative op="status"` reports a role whose artefact is not
/// available as a **refusal**, never as an empty success.
#[test]
fn generative_status_refuses_an_unavailable_artefact() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("t");
    let input = write_fixture(&dir);
    let path = input.display().to_string();

    // Produce, then unlink: the role stays active with no artefact.
    ok(
        "lumina_generative",
        json!({ "path": path, "op": "generate", "expand": true, "canvas": "48x48+8+8", "seed": 7 }),
    );
    ok("lumina_generative", json!({ "path": path, "op": "remove" }));
    let before = snapshot(&dir);

    let payload = err("lumina_generative", json!({ "path": path, "op": "status" }));
    let message = payload["message"].as_str().unwrap();
    assert!(
        message.contains("no silent fallback"),
        "the gate must be named in the error: {message}"
    );
    assert_eq!(before, snapshot(&dir), "a refused status wrote bytes");
}

/// `lumina_regenerate op="matching"` cannot report a derived exposure while a
/// generative role is active: the module's render is impossible, so it aborts
/// instead of deriving from a frame the renderer refused.
#[test]
fn regenerate_matching_cannot_succeed_with_an_active_generative_role() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("t");
    let input = write_fixture(&dir);
    let path = input.display().to_string();

    // Without a generative role the module runs …
    let report = ok(
        "lumina_regenerate",
        json!({ "path": path, "op": "matching" }),
    );
    assert_eq!(report["saved"], json!(true));

    // … with the role active it must not.
    ok(
        "lumina_generative",
        json!({ "path": path, "op": "generate", "expand": true, "canvas": "48x48+8+8", "seed": 7 }),
    );
    let before = snapshot(&dir);
    let payload = err(
        "lumina_regenerate",
        json!({ "path": path, "op": "matching" }),
    );
    // The core stage reports the refusal as an invalid adjustment whose `name`
    // IS the stage; that structured name is what an agent can branch on.
    assert_eq!(error_name(&payload), "InvalidAdjustment", "{payload}");
    assert!(
        payload["name"]
            .as_str()
            .unwrap_or_default()
            .starts_with("generative_artifact."),
        "the failing stage must be named: {payload}"
    );
    assert_eq!(before, snapshot(&dir), "an aborted module wrote bytes");
}

// ------------------------------------------------------------ read/write split

/// Each command's read path never writes and each write path changes exactly
/// one named thing.
#[test]
fn read_ops_never_write_and_write_ops_change_one_thing() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("t");
    let input = write_fixture(&dir);
    let path = input.display().to_string();
    let catalog = dir.join("catalog.json");
    std::fs::write(
        &catalog,
        serde_json::json!({
            "format": "lumina-smart-catalog",
            "version": 1,
            "collections": [{ "id": "all", "name": "All", "version": 1, "rule": { "op": "all" } }],
        })
        .to_string(),
    )
    .unwrap();

    // Reads.
    let before = snapshot(&dir);
    let report = ok("lumina_collections", json!({ "path": path, "op": "list" }));
    assert_eq!(report["saved"], json!(false));
    assert_eq!(report["action"], json!(""));
    let report = ok(
        "lumina_smart_collections",
        json!({ "path": path, "op": "evaluate", "catalog": catalog.display().to_string() }),
    );
    assert_eq!(report["sidecars"], json!(1));
    let report = ok("lumina_generative", json!({ "path": path, "op": "status" }));
    assert_eq!(report["status"], json!("inactive"));
    let report = ok("lumina_regenerate", json!({ "path": path, "op": "all" }));
    assert_eq!(report["saved"], json!(false));
    assert_eq!(before, snapshot(&dir), "a read op wrote bytes");

    // Writes: exactly one membership per call, one module per call.
    let report = ok(
        "lumina_collections",
        json!({ "path": path, "op": "add", "membership": "one=One" }),
    );
    assert_eq!(report["changed"], json!(true));
    assert_eq!(report["action"], json!("add-to:one"));
    assert_eq!(report["collections"].as_array().unwrap().len(), 1);
    let report = ok(
        "lumina_collections",
        json!({ "path": path, "op": "add", "membership": "two=Two" }),
    );
    assert_eq!(report["collections"].as_array().unwrap().len(), 2);
    let report = ok(
        "lumina_collections",
        json!({ "path": path, "op": "remove", "id": "one" }),
    );
    assert_eq!(report["collections"].as_array().unwrap().len(), 1);

    // `smart-collections` has no write path at all: an op other than `evaluate`
    // is refused, so an agent cannot mistake it for a mutating tool.
    assert_eq!(
        error_name(&err(
            "lumina_smart_collections",
            json!({ "path": path, "op": "apply", "catalog": catalog.display().to_string() })
        )),
        "InvalidParams"
    );
}
