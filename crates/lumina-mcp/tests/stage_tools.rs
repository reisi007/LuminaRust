//! MCP-PARITY-A: the MCP surface of the four session-based recipe stage editors
//! `lumina_spot`, `lumina_lens_blur`, `lumina_geometry`, `lumina_upright`.
//!
//! Three things are proven here, none of which is the byte identity itself (that
//! lives in `crates/lumina-cli/tests/stage_parity.rs`, where the real
//! `lumina-cli` process is available):
//!
//! 1. **Registry** — every tool is in `list_tool_definitions` *and*
//!    `dispatch_tool`, and `is_known_tool` agrees with both. A tool missing from
//!    either list is invisible to `tools/list` or dead for `tools/call`; this
//!    test drives the actual `tools/list` and `tools/call` JSON-RPC paths, not
//!    just the helper functions.
//! 2. **Schema** — unknown/extra fields are refused, required fields are
//!    enforced, and every rejection is a loud `InvalidParams` that changes no
//!    bytes. The existing tools' error style (`McpError::InvalidParams`, a
//!    `code()` of `-32602`, an `isError: true` result) is reused verbatim.
//! 3. **Read/write separation** — each tool has a read op that leaves the
//!    sidecar untouched and write ops that change exactly one named thing; there
//!    is no op that sets a whole stage. That, the read-op contract and the
//!    compare-and-swap contract are proven in `stage_tools_session.rs`, not
//!    here.
//!
//! The four tool modules are also reached directly (not only through the
//! protocol) to prove each one exposes a name, a description and a closed
//! object schema.

use lumina_mcp::tools::{is_known_tool, list_tool_definitions};

#[path = "stage_tools_common/mod.rs"]
mod common;
use common::*;

// --------------------------------------------------- tool-level unit reach

#[test]
fn every_stage_tool_module_exposes_name_description_and_schema() {
    for (name, description, schema) in [
        (spot::NAME, spot::DESCRIPTION, spot::schema()),
        (lens_blur::NAME, lens_blur::DESCRIPTION, lens_blur::schema()),
        (geometry::NAME, geometry::DESCRIPTION, geometry::schema()),
        (upright::NAME, upright::DESCRIPTION, upright::schema()),
    ] {
        assert!(STAGE_TOOLS.contains(&name), "{name} is not a stage tool");
        assert!(!description.is_empty(), "{name}");
        assert_eq!(schema["type"], json!("object"), "{name}");
        assert_eq!(schema["additionalProperties"], json!(false), "{name}");
    }
}

#[test]
fn nested_object_members_are_required_and_finite() {
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::new(&root.path().join("nested"));
    let before = session.sidecar_bytes();
    let id = session.image_id.clone();
    // A missing member of `focus_rect` is loud, not silently zero-filled.
    let error = session.err(
        "lumina_lens_blur",
        json!({
            "image_id": id,
            "op": "set_focus_rect",
            "focus_rect": { "x": 0.1, "y": 0.1, "width": 0.5 }
        }),
    );
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("`focus_rect` requires `height`"),
        "{error}"
    );
    // A non-finite / non-numeric member is loud too.
    let error = session.err(
        "lumina_lens_blur",
        json!({
            "image_id": id,
            "op": "set_focus_rect",
            "focus_rect": { "x": "0.1", "y": 0.1, "width": 0.5, "height": 0.4 }
        }),
    );
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("`focus_rect.x` must be a number"),
        "{error}"
    );
    // A missing member of `depth_artifact` is loud.
    let error = session.err(
        "lumina_lens_blur",
        json!({
            "image_id": id,
            "op": "set_depth_artifact",
            "depth_artifact": { "relative_path": "depth/a.bin" }
        }),
    );
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("`depth_artifact.sha256` must be a string"),
        "{error}"
    );
    // `rect` in the geometry tool follows the same rule.
    let error = session.err(
        "lumina_geometry",
        json!({
            "image_id": id,
            "op": "set_crop_free",
            "rect": { "x": 0.1, "y": 0.1, "width": 0.5 }
        }),
    );
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("`rect` requires `height`"),
        "{error}"
    );
    assert_eq!(session.sidecar_bytes(), before);
}

#[test]
fn a_non_finite_or_wrongly_typed_value_is_refused_without_writing() {
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::new(&root.path().join("finite"));
    let before = session.sidecar_bytes();
    let id = session.image_id.clone();
    for arguments in [
        json!({ "image_id": id, "op": "set_amount", "amount": "0.5" }),
        json!({ "image_id": id, "op": "set_focal_near", "focal_near": true }),
        json!({ "image_id": id, "op": "set_bokeh", "bokeh": 7 }),
        json!({ "image_id": id, "op": "set_visualize", "threshold": [] }),
    ] {
        let error = session.err("lumina_lens_blur", arguments.clone());
        assert_eq!(
            error["error"],
            json!("InvalidParams"),
            "{arguments} must be refused: {error}"
        );
    }
    // detect_max must be a sane integer.
    let error = session.err(
        "lumina_spot",
        json!({ "image_id": id, "op": "detect", "detect_max": -1 }),
    );
    assert_eq!(error["error"], json!("InvalidParams"), "{error}");
    assert_eq!(session.sidecar_bytes(), before);
}

// ---------------------------------------------------------------- registry

#[test]
fn all_four_stage_tools_are_registered_in_both_paths() {
    let definitions = list_tool_definitions();
    let listed: Vec<&str> = definitions
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    for name in STAGE_TOOLS {
        assert!(
            listed.contains(name),
            "{name} is missing from list_tool_definitions (tools/list): {listed:?}"
        );
        assert!(
            is_known_tool(name),
            "{name} is not accepted by is_known_tool"
        );
    }
    // `dispatch_tool` must agree with `list_tool_definitions`: a name that is
    // listed but not dispatchable would be a dead tool.
    for name in &listed {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": name, "arguments": {} }
        });
        let mut server = Server::with_preview_dir(std::env::temp_dir());
        let response = server.handle_message(request).expect("a response");
        assert!(
            response.get("error").is_none(),
            "{name} is listed but dispatch_tool does not know it: {response}"
        );
    }
}

#[test]
fn all_four_stage_tools_appear_in_tools_list_over_the_protocol() {
    let mut server = Server::with_preview_dir(std::env::temp_dir());
    let response = server
        .handle_message(json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/list" }))
        .unwrap();
    let tools = response["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    for name in STAGE_TOOLS {
        assert!(
            names.contains(name),
            "tools/list is missing {name}: {names:?}"
        );
        let tool = tools
            .iter()
            .find(|tool| tool["name"].as_str() == Some(*name))
            .expect("tool");
        // A complete definition: name, description and a closed input schema.
        assert!(!tool["description"].as_str().unwrap().is_empty());
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], json!("object"));
        assert_eq!(
            schema["additionalProperties"],
            json!(false),
            "{name}: the schema must forbid unknown fields"
        );
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("image_id")), "{name}");
        assert!(required.contains(&json!("op")), "{name}");
        // The op enum must contain a read op and at least one write op.
        let op = schema["properties"]["op"]["enum"].as_array().unwrap();
        assert!(op.contains(&json!("list")), "{name}: needs a read op");
        assert!(op.len() > 1, "{name}: needs at least one write op");
    }
}

#[test]
fn an_unknown_stage_tool_is_a_protocol_error_not_a_tool_error() {
    let mut server = Server::with_preview_dir(std::env::temp_dir());
    let response = server
        .handle_message(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": "lumina_spot_removal", "arguments": {} }
        }))
        .unwrap();
    assert_eq!(response["error"]["code"], json!(-32602));
    assert!(response["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Unknown tool"));
    assert!(!is_known_tool("lumina_spot_removal"));
}

// ------------------------------------------------------------------ schema

#[test]
fn unknown_and_extra_fields_are_refused_without_writing() {
    let root = tempfile::tempdir().unwrap();
    for tool in STAGE_TOOLS {
        let mut session = Session::new(&root.path().join(tool));
        let before = session.sidecar_bytes();
        let error = session.err(
            tool,
            json!({ "image_id": session.image_id, "op": "list", "not_a_field": 1 }),
        );
        assert_eq!(
            error["error"],
            json!("InvalidParams"),
            "{tool}: {error}; an unknown field must be InvalidParams"
        );
        let message = error["message"].as_str().unwrap();
        assert!(
            message.contains("unknown field `not_a_field`"),
            "{tool}: {message}"
        );
        assert!(
            message.contains(&format!("for `{tool}`")),
            "{tool}: the message must name the tool: {message}"
        );
        assert_eq!(session.sidecar_bytes(), before, "{tool} wrote bytes");
    }
}

#[test]
fn required_fields_are_enforced_for_every_stage_tool() {
    let root = tempfile::tempdir().unwrap();
    for tool in STAGE_TOOLS {
        let mut session = Session::new(&root.path().join(tool));
        // missing `image_id`
        let error = session.err(tool, json!({ "op": "list" }));
        assert_eq!(
            error["error"],
            json!("InvalidParams"),
            "{tool} without image_id: {error}"
        );
        assert!(
            error["message"].as_str().unwrap().contains("image_id"),
            "{tool}: {}",
            error["message"]
        );
        // missing `op`
        let error = session.err(tool, json!({ "image_id": session.image_id }));
        assert_eq!(
            error["error"],
            json!("InvalidParams"),
            "{tool} without op: {error}"
        );
        assert!(
            error["message"].as_str().unwrap().contains("missing `op`"),
            "{tool}: {}",
            error["message"]
        );
        // wrong type for `image_id`
        let error = session.err(tool, json!({ "image_id": 7, "op": "list" }));
        assert_eq!(error["error"], json!("InvalidParams"), "{tool}: {error}");
        // unknown op
        let error = session.err(
            tool,
            json!({ "image_id": session.image_id, "op": "set_everything" }),
        );
        assert_eq!(error["error"], json!("InvalidParams"), "{tool}: {error}");
        assert!(
            error["message"].as_str().unwrap().contains("unknown op"),
            "{tool}: {}",
            error["message"]
        );
    }
}

#[test]
fn a_write_op_without_its_value_is_refused_rather_than_a_no_op() {
    let root = tempfile::tempdir().unwrap();
    // `op` names a field but the value is missing: that must be a loud
    // rejection, not a successful call that changed nothing.
    let cases: Vec<(&str, Value, &str)> = vec![
        (
            "lumina_lens_blur",
            json!({ "op": "set_amount" }),
            "op `set_amount` requires `amount`",
        ),
        (
            "lumina_geometry",
            json!({ "op": "set_rotation" }),
            "op `set_rotation` requires `value`",
        ),
        (
            "lumina_geometry",
            json!({ "op": "set_lens_field", "value": 0.1 }),
            "op `set_lens_field` requires `field`",
        ),
        (
            "lumina_spot",
            json!({ "op": "add", "center_x": 0.5, "center_y": 0.5 }),
            "op `add` requires `radius`",
        ),
    ];
    for (tool, arguments, fragment) in cases {
        let mut session = Session::new(&root.path().join(format!("{tool}-{}", fragment.len())));
        let before = session.sidecar_bytes();
        let mut payload = json!({ "image_id": session.image_id });
        for (key, value) in arguments.as_object().unwrap() {
            payload[key.clone()] = value.clone();
        }
        let error = session.err(tool, payload);
        assert_eq!(
            error["error"],
            json!("InvalidParams"),
            "{tool} {arguments}: {error}"
        );
        assert!(
            error["message"].as_str().unwrap().contains(fragment),
            "{tool} {arguments}: `{}` does not mention `{fragment}`",
            error["message"]
        );
        assert_eq!(session.sidecar_bytes(), before, "{tool} wrote bytes");
    }
}
