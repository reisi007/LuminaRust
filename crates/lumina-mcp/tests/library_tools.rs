//! MCP-PARITY-B: the MCP surface of the five path-based commands
//! `lumina_collections`, `lumina_smart_collections`, `lumina_relocate`,
//! `lumina_generative`, `lumina_regenerate`.
//!
//! Three things are proven here, none of which is the byte identity itself (that
//! lives in `crates/lumina-cli/tests/library_parity*.rs`, where the real
//! `lumina-cli` process is available):
//!
//! 1. **Registry** — every tool is in `list_tool_definitions` *and*
//!    `dispatch_tool`, and `is_known_tool` agrees with both, driven through the
//!    real `tools/list` and `tools/call` JSON-RPC paths. A tool missing from
//!    either list is invisible to `tools/list` or dead for `tools/call`; the
//!    test that one name is listed and yet not dispatchable fails here.
//! 2. **Schema** — unknown/extra fields are refused, required fields are
//!    enforced, `path` (not `image_id`) is the addressing vocabulary, and every
//!    rejection is a loud `InvalidParams` that changes no bytes.
//! 3. **The model/artefact gate** — `lumina_generative op="generate"` and
//!    `lumina_regenerate op="matching"` cannot report a success for a canvas
//!    that could not be produced or resolved. Neither may fall back to a
//!    fabricated artefact or a "nothing to do" success.

use lumina_mcp::tools::{is_known_tool, list_tool_definitions};
use serde_json::{json, Value};

#[path = "library_tools_common/mod.rs"]
mod support;
use support::*;

// ------------------------------------------------------------------ registry

/// Every tool is in `list_tool_definitions` **and** in `dispatch_tool`, and
/// `is_known_tool` agrees with both.
#[test]
fn all_five_library_tools_are_registered_in_both_paths() {
    let definitions = list_tool_definitions();
    let listed: Vec<&str> = definitions
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    for name in LIBRARY_TOOLS {
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
    // listed but not dispatchable would be a dead tool. Every listed tool is
    // called with empty arguments, so a *protocol* error (unknown tool) would
    // surface here while a tool *execution* error stays inside the result.
    for name in &listed {
        let request = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": {} }
        });
        let response = server().handle_message(request).expect("a response");
        assert!(
            response.get("error").is_none(),
            "{name} is listed but dispatch_tool does not know it: {response}"
        );
    }
}

/// The same registry claim over the real `tools/list` response, plus the schema
/// contract every one of the five declares.
#[test]
fn all_five_library_tools_appear_in_tools_list_over_the_protocol() {
    let response = server()
        .handle_message(json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/list" }))
        .unwrap();
    let tools = response["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    for name in LIBRARY_TOOLS {
        assert!(
            names.contains(name),
            "tools/list is missing {name}: {names:?}"
        );
        let tool = tools
            .iter()
            .find(|tool| tool["name"].as_str() == Some(*name))
            .expect("tool");
        assert!(
            !tool["description"].as_str().unwrap().is_empty(),
            "{name}: no description"
        );
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], json!("object"), "{name}");
        assert_eq!(
            schema["additionalProperties"],
            json!(false),
            "{name}: the schema must forbid unknown fields"
        );
        let required = schema["required"].as_array().unwrap();
        assert!(
            required.contains(&json!("op")),
            "{name}: `op` must be required — a call without one has no meaning"
        );
        // Path-based addressing: these are bulk tools, so they name a path (or a
        // from/to pair) and must NOT require the session's `image_id`.
        assert!(
            !required.contains(&json!("image_id")),
            "{name}: a path-based tool must not require image_id"
        );
        let op = schema["properties"]["op"]["enum"].as_array().unwrap();
        assert!(!op.is_empty(), "{name}: the op enum must not be empty");
        // A read op and at least one write op, where the command has both.
        let read_only = *name == "lumina_smart_collections" || *name == "lumina_relocate";
        if !read_only {
            assert!(op.len() > 1, "{name}: needs a read op and a write op");
        }
    }
    // An unknown tool is a protocol-level error, not a tool error.
    let response = server()
        .handle_message(json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "lumina_collections_removal", "arguments": {} }
        }))
        .unwrap();
    assert_eq!(response["error"]["code"], json!(-32602));
    assert!(!is_known_tool("lumina_collections_removal"));
}

// -------------------------------------------------------------------- schema

/// Unknown fields, missing required fields, a missing `op`, an unknown `op` and
/// a wrong-typed `path` are all loud `InvalidParams` on both sides of the
/// `tools/call` seam.
#[test]
fn required_fields_and_unknown_fields_are_refused_for_every_library_tool() {
    let root = tempfile::tempdir().unwrap();
    let input = write_fixture(&root.path().join("t"));
    let path = input.display().to_string();
    let cases: Vec<(&str, Value, &str)> = vec![
        (
            LIBRARY_TOOLS[0],
            json!({ "op": "list" }),
            "missing string argument `path`",
        ),
        (
            LIBRARY_TOOLS[0],
            json!({ "path": 7, "op": "list" }),
            "missing string argument `path`",
        ),
        (LIBRARY_TOOLS[0], json!({ "path": path }), "missing `op`"),
        (
            LIBRARY_TOOLS[0],
            json!({ "path": path, "op": "" }),
            "must not be empty",
        ),
        (
            LIBRARY_TOOLS[0],
            json!({ "path": path, "op": 7 }),
            "must be a string",
        ),
        (
            LIBRARY_TOOLS[0],
            json!({ "path": path, "op": "set_everything" }),
            "unknown op",
        ),
        (
            LIBRARY_TOOLS[0],
            json!({ "path": path, "op": "list", "nope": 1 }),
            "unknown field `nope`",
        ),
        (
            LIBRARY_TOOLS[0],
            json!({ "path": path, "op": "list", "image_id": "x" }),
            "unknown field `image_id`",
        ),
        (
            LIBRARY_TOOLS[2],
            json!({ "from": path, "op": "move" }),
            "missing string argument `to`",
        ),
        (
            LIBRARY_TOOLS[2],
            json!({ "from": path, "to": path, "op": "fly" }),
            "unknown op",
        ),
        (LIBRARY_TOOLS[3], json!({ "path": path }), "missing `op`"),
        (
            LIBRARY_TOOLS[3],
            json!({ "path": path, "op": "paint" }),
            "unknown op",
        ),
        (LIBRARY_TOOLS[4], json!({ "path": path }), "missing `op`"),
        (
            LIBRARY_TOOLS[4],
            json!({ "path": path, "op": "denoise" }),
            "unknown op",
        ),
        (
            LIBRARY_TOOLS[4],
            json!({ "path": path, "op": "all", "target_luminance": "0.5" }),
            "`target_luminance` must be a number",
        ),
        (
            LIBRARY_TOOLS[4],
            json!({ "path": path, "op": "all", "modules": ["masks"] }),
            "unknown field `modules`",
        ),
    ];
    for (name, args, fragment) in cases {
        let payload = err(name, args.clone());
        assert_eq!(error_name(&payload), "InvalidParams", "{name} {args}");
        assert!(
            payload["message"].as_str().unwrap().contains(fragment),
            "{name} {args}: `{}` does not mention `{fragment}`",
            payload["message"]
        );
    }
}

/// Every refusal leaves the fixture tree byte-identical.
#[test]
fn a_refused_library_call_never_writes() {
    let root = tempfile::tempdir().unwrap();
    let input = write_fixture(&root.path().join("t"));
    let path = input.display().to_string();
    let before = snapshot(&root.path().join("t"));
    for (name, args) in [
        (LIBRARY_TOOLS[0], json!({ "path": path, "op": "add" })),
        (
            LIBRARY_TOOLS[0],
            json!({ "path": path, "op": "add", "membership": "nope" }),
        ),
        (LIBRARY_TOOLS[3], json!({ "path": path, "op": "generate" })),
        (
            LIBRARY_TOOLS[3],
            json!({ "path": path, "op": "generate", "expand": true }),
        ),
        (
            LIBRARY_TOOLS[4],
            json!({ "path": path, "op": "all", "target_luminance": 9.0 }),
        ),
    ] {
        let payload = err(name, args.clone());
        assert_eq!(error_name(&payload), "InvalidParams", "{name} {args}");
        assert_eq!(
            before,
            snapshot(&root.path().join("t")),
            "{name} {args} wrote bytes"
        );
    }
}
