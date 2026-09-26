//! MCP-PARITY-A: the read/write split and the session contract of the four stage
//! tools.
//!
//! Each tool must have a read op that changes nothing and write ops that change
//! exactly one named thing, and a write must keep the `lumina_edit`
//! compare-and-swap contract: an externally modified sidecar surfaces as
//! `SidecarConflict` instead of being overwritten. A contradictory argument is
//! a loud rejection, not a silently ignored one. (The registry, the schema
//! shape and the unknown-field rejections are proven in `stage_tools.rs`.)

#[path = "stage_tools_common/mod.rs"]
mod common;
use common::*;

// --------------------------------------------------------- read/write split

#[test]
fn every_stage_tool_has_a_read_op_that_changes_nothing() {
    let root = tempfile::tempdir().unwrap();
    for tool in STAGE_TOOLS {
        let mut session = Session::new(&root.path().join(tool));
        let before = session.sidecar_bytes();
        let report = session.call(tool, json!({ "image_id": session.image_id, "op": "list" }));
        assert_eq!(
            report["saved"],
            json!(false),
            "{tool}: a read must not save"
        );
        assert_eq!(report["action"], json!(""), "{tool}: a read has no action");
        assert_eq!(
            report["actions"],
            json!([]),
            "{tool}: a read has no actions"
        );
        assert_eq!(
            session.sidecar_bytes(),
            before,
            "{tool}: a read wrote bytes"
        );
    }
}

#[test]
fn every_stage_tool_has_a_write_op_that_saves_exactly_one_thing() {
    let root = tempfile::tempdir().unwrap();
    // One representative write per tool; each must report `saved: true`, exactly
    // one action, and a changed sidecar.
    let cases: Vec<(&str, Value, &str)> = vec![
        (
            "lumina_spot",
            json!({ "op": "add", "center_x": 0.5, "center_y": 0.5, "radius": 4.0 }),
            "add-heuristic",
        ),
        ("lumina_lens_blur", json!({ "op": "enable" }), "enable"),
        (
            "lumina_geometry",
            json!({ "op": "set_rotation", "value": 5.0 }),
            "rotation:5",
        ),
        (
            "lumina_upright",
            json!({ "op": "analyze" }),
            "upright:analyze,upright:enable",
        ),
    ];
    for (tool, arguments, action) in cases {
        let mut session = Session::new(&root.path().join(tool));
        let before = session.sidecar_bytes();
        let report = session.call(tool, {
            let mut payload = json!({ "image_id": session.image_id });
            for (key, value) in arguments.as_object().unwrap() {
                payload[key.clone()] = value.clone();
            }
            payload
        });
        assert_eq!(report["saved"], json!(true), "{tool}");
        assert_eq!(report["action"], json!(action), "{tool}");
        assert_ne!(session.sidecar_bytes(), before, "{tool} did not write");
    }
}

#[test]
fn an_external_sidecar_change_surfaces_as_sidecar_conflict() {
    // The session tools keep the `lumina_edit` compare-and-swap contract: a
    // sidecar modified on disk after `lumina_load` is never silently
    // overwritten.
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::new(root.path());
    let id = session.image_id.clone();
    let mut document = lumina_sidecar::load_sidecar(&session.sidecar).unwrap();
    document.virtual_copies[0]
        .recipe
        .adjustments
        .insert("exposure".into(), 1.0);
    lumina_sidecar::save_sidecar(&session.sidecar, &document).unwrap();
    let response = session
        .server
        .handle_message(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "lumina_lens_blur",
                "arguments": { "image_id": id, "op": "set_amount", "amount": 0.5 }
            }
        }))
        .unwrap();
    assert_eq!(response["result"]["isError"], json!(true));
    let data = &response["result"]["structuredContent"];
    assert_eq!(data["error"], json!("SidecarConflict"));
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("SidecarConflict"));
    // The external write survived: no lost update.
    let reloaded = lumina_sidecar::load_sidecar(&session.sidecar).unwrap();
    assert_eq!(
        reloaded.virtual_copies[0].recipe.adjustments["exposure"],
        json!(1.0)
    );
    assert!(reloaded.virtual_copies[0].recipe.lens_blur.is_none());
}

// ------------------------------------------------------- loud rejection

#[test]
fn a_contradictory_field_is_refused_rather_than_silently_ignored() {
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::new(&root.path().join("contradiction"));
    let before = session.sidecar_bytes();
    let id = session.image_id.clone();
    // `disable_after_analyze` outside op=analyze would be a silently ignored
    // argument; it is a loud contradiction instead.
    let error = session.err(
        "lumina_upright",
        json!({ "image_id": id, "op": "clear", "disable_after_analyze": true }),
    );
    assert_eq!(error["error"], json!("InvalidParams"), "{error}");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("only applies to op=analyze"),
        "{error}"
    );
    // `field` carrying the CLI's `FIELD:VALUE` separator would silently produce a
    // wrong field name; it is refused.
    let error = session.err(
        "lumina_geometry",
        json!({ "image_id": id, "op": "set_lens_field", "field": "a:b", "value": 1.0 }),
    );
    assert_eq!(error["error"], json!("InvalidParams"), "{error}");
    assert_eq!(session.sidecar_bytes(), before);
}
