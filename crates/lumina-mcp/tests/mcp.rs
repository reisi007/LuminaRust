//! Integration and error-path tests for the LuminaRust MCP server.
//!
//! These exercise the server through its public JSON-RPC surface (the same
//! path an MCP client uses) plus a few internal helpers that are exposed for
//! testing. No network and no real RAW fixtures are required: a synthetic PNG
//! drives the roundtrip, determinism, persistence and error-path coverage.

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_mcp::tools::edit::validate_adjustments;
use lumina_mcp::util::{detect_format, downscale_bilinear, recipe_hash};
use lumina_mcp::Server;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const TOOL_NAMES: &[&str] = &[
    "lumina_load",
    "lumina_edit",
    "lumina_get_recipe",
    "lumina_save",
    "lumina_preview",
    "lumina_list_virtual_copies",
    "lumina_inspect",
    "lumina_analyze",
];

fn new_server(preview_dir: &Path) -> Server {
    std::env::set_var("LUMINA_MCP_PREVIEW_DIR", preview_dir);
    Server::new()
}

fn call(server: &mut Server, method: &str, params: Value) -> Value {
    let request = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    server.handle_message(request).expect("expected a response")
}

fn call_tool(server: &mut Server, name: &str, args: Value) -> Value {
    call(
        server,
        "tools/call",
        json!({ "name": name, "arguments": args }),
    )
}

fn tool_ok(server: &mut Server, name: &str, args: Value) -> Value {
    let response = call_tool(server, name, args);
    assert!(
        response.get("error").is_none(),
        "unexpected error for `{name}`: {:?}",
        response.get("error")
    );
    response["result"]["structuredContent"].clone()
}

fn tool_error_name(server: &mut Server, name: &str, args: Value) -> String {
    let response = call_tool(server, name, args);
    // Tool execution errors arrive as result objects with `isError: true`
    // (MCP spec); protocol-level errors remain top-level error objects.
    let data = if response.get("error").is_some() {
        response["error"]["data"].clone()
    } else {
        assert_eq!(
            response["result"]["isError"], true,
            "expected a tool execution error for `{name}`"
        );
        response["result"]["structuredContent"].clone()
    };
    data["error"]
        .as_str()
        .expect("error payload carries a stable name")
        .to_string()
}

fn make_png(path: &Path, width: u32, height: u32) {
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let x = (index % width as usize) as u8;
        let y = (index / width as usize) as u8;
        pixel[0] = x;
        pixel[1] = y;
        pixel[2] = 128;
        pixel[3] = 255;
    }
    let frame = ImageFrame::new(width, height, pixels).unwrap();
    let bytes = frame.encode(ImageFileFormat::Png).unwrap();
    fs::write(path, bytes).unwrap();
}

// ---------------------------------------------------------------------------
// Unit-level checks (internal helpers exposed for testing)
// ---------------------------------------------------------------------------

#[test]
fn detect_format_accepts_supported_and_rejects_others() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("a.png");
    fs::write(&png, b"x").unwrap();
    assert_eq!(detect_format(&png).unwrap(), "png");

    let raw = dir.path().join("b.arw");
    fs::write(&raw, b"x").unwrap();
    assert_eq!(detect_format(&raw).unwrap(), "arw");

    let bad = dir.path().join("c.txt");
    fs::write(&bad, b"x").unwrap();
    assert!(matches!(
        detect_format(&bad),
        Err(lumina_mcp::McpError::UnsupportedFormat(_))
    ));
}

#[test]
fn validate_adjustments_rejects_out_of_range() {
    let mut map = BTreeMap::new();
    map.insert("exposure".to_string(), 100.0);
    assert!(matches!(
        validate_adjustments(&map),
        Err(lumina_mcp::McpError::InvalidAdjustment { .. })
    ));

    let mut valid = BTreeMap::new();
    valid.insert("contrast".to_string(), -0.5);
    valid.insert("wb_temperature".to_string(), 5500.0);
    assert!(validate_adjustments(&valid).is_ok());

    let mut unknown = BTreeMap::new();
    unknown.insert("not_a_key".to_string(), 0.0);
    assert!(matches!(
        validate_adjustments(&unknown),
        Err(lumina_mcp::McpError::InvalidAdjustment { .. })
    ));
}

#[test]
fn recipe_hash_is_idempotent() {
    let mut a = BTreeMap::new();
    a.insert("exposure".to_string(), 1.0);
    a.insert("contrast".to_string(), -0.2);
    let mut b = BTreeMap::new();
    b.insert("contrast".to_string(), -0.2);
    b.insert("exposure".to_string(), 1.0);
    let recipe_a = lumina_sidecar::EditRecipe {
        adjustments: a,
        ..Default::default()
    };
    let recipe_b = lumina_sidecar::EditRecipe {
        adjustments: b,
        ..Default::default()
    };
    assert_eq!(recipe_hash(&recipe_a), recipe_hash(&recipe_b));
}

#[test]
fn downscale_never_upsizes_and_is_deterministic() {
    let frame = ImageFrame::new(64, 48, vec![10u8; 64 * 48 * 4]).unwrap();
    let small = downscale_bilinear(&frame, 1024);
    assert_eq!((small.width, small.height), (64, 48));

    let scaled = downscale_bilinear(&frame, 32);
    assert_eq!(scaled.width, 32);
    assert_eq!(scaled.height, 24);

    let again = downscale_bilinear(&frame, 32);
    assert_eq!(scaled.pixels, again.pixels);
}

// ---------------------------------------------------------------------------
// Protocol compliance
// ---------------------------------------------------------------------------

#[test]
fn initialize_reports_capabilities_and_protocol() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(dir.path());
    let response = call(&mut server, "initialize", json!({}));
    assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(response["result"]["capabilities"]["tools"], json!({}));
    assert_eq!(response["result"]["serverInfo"]["name"], "lumina-mcp");

    // Notifications must not produce a response.
    let note = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    assert!(server.handle_message(note).is_none());
}

#[test]
fn tools_list_returns_all_eight_tools_with_schemas() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(dir.path());
    let response = call(&mut server, "tools/list", json!({}));
    let tools = response["result"]["tools"].as_array().unwrap();
    let names: Vec<String> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect();
    for expected in TOOL_NAMES {
        assert!(
            names.contains(&expected.to_string()),
            "missing tool `{expected}`"
        );
        let tool = tools.iter().find(|t| t["name"] == *expected).unwrap();
        assert!(
            tool["inputSchema"].is_object(),
            "`{expected}` has no schema"
        );
        assert!(tool["description"].is_string());
    }
}

#[test]
fn unknown_tool_is_protocol_level_invalid_params() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(dir.path());
    let response = call_tool(&mut server, "does_not_exist", json!({}));
    // MCP spec: an unknown tool is answered at the protocol layer with
    // -32602 ("Unknown tool"), not as a tool execution error result.
    assert_eq!(response["error"]["code"], -32602);
    assert!(response["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Unknown tool"));
    assert!(response["result"].is_null());
}

// ---------------------------------------------------------------------------
// JSON-RPC code correctness (REVIEW-MCP-N1)
// ---------------------------------------------------------------------------

#[test]
fn parse_error_and_invalid_request_use_distinct_codes() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(dir.path());

    // Broken JSON on the line: -32700 Parse error, id null.
    let response = server.handle_line("{not json").expect("response");
    assert_eq!(response["error"]["code"], -32700);
    assert!(response["error"]["message"]
        .as_str()
        .unwrap()
        .contains("parse error"));
    assert!(response["id"].is_null());

    // Valid JSON that is not a request object: -32600 Invalid Request.
    let response = server.handle_line("[1, 2, 3]").expect("response");
    assert_eq!(response["error"]["code"], -32600);
    assert!(response["id"].is_null());

    // Wrong-typed member with a detectable id: -32600, id echoed.
    let response = server
        .handle_line(r#"{"jsonrpc":"2.0","id":7,"method":42}"#)
        .expect("response");
    assert_eq!(response["error"]["code"], -32600);
    assert_eq!(response["id"], 7);

    // A well-formed message still dispatches normally.
    let response = server
        .handle_line(r#"{"jsonrpc":"2.0","id":8,"method":"ping"}"#)
        .expect("response");
    assert!(response.get("result").is_some());
}

#[test]
fn tool_execution_errors_are_error_results_not_protocol_errors() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(dir.path());
    // No image loaded: an execution failure of a *registered* tool.
    let response = call_tool(&mut server, "lumina_preview", json!({ "image_id": "none" }));
    assert!(
        response.get("error").is_none(),
        "execution errors must stay inside the result"
    );
    assert_eq!(response["result"]["isError"], true);
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("NoImageLoaded"));
    assert_eq!(
        response["result"]["structuredContent"]["error"],
        "NoImageLoaded"
    );
}

// ---------------------------------------------------------------------------
// Roundtrip: load -> edit -> preview -> save
// ---------------------------------------------------------------------------

#[test]
fn roundtrip_load_edit_preview_save() {
    let dir = tempfile::tempdir().unwrap();
    let preview_dir = dir.path().join("previews");
    let source = dir.path().join("photo.png");
    let output = dir.path().join("photo_edited.png");
    make_png(&source, 64, 48);

    let mut server = new_server(&preview_dir);
    let loaded = tool_ok(
        &mut server,
        "lumina_load",
        json!({ "path": source.to_string_lossy() }),
    );
    let image_id = loaded["image_id"].as_str().unwrap().to_string();
    assert_eq!(loaded["width"], 64);
    assert_eq!(loaded["height"], 48);
    assert_eq!(loaded["format"], "png");
    assert_eq!(loaded["virtual_copies"].as_array().unwrap().len(), 1);
    assert_eq!(loaded["sidecar_status"], "created");

    let edited = tool_ok(
        &mut server,
        "lumina_edit",
        json!({ "image_id": image_id, "adjustments": { "exposure": 1.0, "contrast": -0.2 } }),
    );
    assert!(edited["ok"].as_bool().unwrap());
    let recipe_hash_after_edit = edited["recipe_hash"].as_str().unwrap().to_string();

    // Preview.
    let preview = tool_ok(
        &mut server,
        "lumina_preview",
        json!({ "image_id": image_id, "max_width": 1024 }),
    );
    assert!(preview["ok"].as_bool().unwrap());
    let preview_path = PathBuf::from(preview["preview_path"].as_str().unwrap());
    assert!(preview_path.exists());
    // 64x48 fits under max_width, so no downscale occurs.
    assert_eq!(preview["width"], 64);
    assert_eq!(preview["height"], 48);

    // Save (full resolution render via the shared entry point).
    tool_ok(
        &mut server,
        "lumina_save",
        json!({ "image_id": image_id, "output_path": output.to_string_lossy(), "format": "png" }),
    );
    assert!(output.exists());
    let saved = fs::read(&output).unwrap();
    let saved_frame = ImageFrame::decode(&saved).unwrap();
    assert_eq!((saved_frame.width, saved_frame.height), (64, 48));

    // Recipe reflects the edits.
    let recipe = tool_ok(
        &mut server,
        "lumina_get_recipe",
        json!({ "image_id": image_id }),
    );
    assert_eq!(recipe["recipe"]["adjustments"]["exposure"], 1.0);
    assert_eq!(recipe["recipe"]["adjustments"]["contrast"], -0.2);
    assert_eq!(recipe["recipe_hash"], recipe_hash_after_edit);

    // Inspect without decoding.
    let inspect = tool_ok(
        &mut server,
        "lumina_inspect",
        json!({ "image_id": image_id }),
    );
    assert_eq!(inspect["virtual_copies"], 1);
    assert_eq!(inspect["ai_masks"].as_array().unwrap().len(), 0);

    // Analyze returns structured statistics.
    let analysis = tool_ok(
        &mut server,
        "lumina_analyze",
        json!({ "image_id": image_id }),
    );
    assert!(analysis["histogram"]["luminance"].as_array().unwrap().len() == 256);
    assert!(analysis["exposure_estimate"]["ev"].is_number());
    assert!(!analysis["dominant_colors"].as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Determinism: two previews of the same edit state are byte-identical
// ---------------------------------------------------------------------------

#[test]
fn preview_is_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let preview_dir = dir.path().join("previews");
    let source = dir.path().join("photo.png");
    make_png(&source, 256, 192);
    let mut server = new_server(&preview_dir);

    let loaded = tool_ok(
        &mut server,
        "lumina_load",
        json!({ "path": source.to_string_lossy() }),
    );
    let image_id = loaded["image_id"].as_str().expect("image_id").to_string();

    tool_ok(
        &mut server,
        "lumina_edit",
        json!({ "image_id": image_id, "adjustments": { "exposure": 0.7 } }),
    );
    let first = tool_ok(
        &mut server,
        "lumina_preview",
        json!({ "image_id": image_id, "max_width": 64 }),
    );
    let preview_path = first["preview_path"].as_str().unwrap().to_string();
    let bytes_first = fs::read(&preview_path).unwrap();

    // Second preview must overwrite with identical bytes.
    let second = tool_ok(
        &mut server,
        "lumina_preview",
        json!({ "image_id": image_id, "max_width": 64 }),
    );
    let bytes_second = fs::read(second["preview_path"].as_str().unwrap()).unwrap();
    assert_eq!(bytes_first, bytes_second);
    assert_eq!(second["width"], 64);
    assert_eq!(second["height"], 48);
}

// ---------------------------------------------------------------------------
// Sidecar persistence across a simulated server restart
// ---------------------------------------------------------------------------

#[test]
fn sidecar_persists_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let preview_dir = dir.path().join("previews");
    let source = dir.path().join("photo.png");
    make_png(&source, 32, 32);

    // First session: load (creates sidecar) and edit.
    let mut first = new_server(&preview_dir);
    let loaded = tool_ok(
        &mut first,
        "lumina_load",
        json!({ "path": source.to_string_lossy() }),
    );
    let image_id = loaded["image_id"].as_str().unwrap().to_string();
    tool_ok(
        &mut first,
        "lumina_edit",
        json!({ "image_id": image_id, "adjustments": { "exposure": 2.0 } }),
    );
    drop(first);

    // Second session: fresh server, same source -> sidecar is read back.
    let mut second = new_server(&preview_dir);
    let reloaded = tool_ok(
        &mut second,
        "lumina_load",
        json!({ "path": source.to_string_lossy() }),
    );
    assert_eq!(reloaded["sidecar_status"], "loaded");
    let recipe = tool_ok(
        &mut second,
        "lumina_get_recipe",
        json!({ "image_id": reloaded["image_id"].as_str().unwrap() }),
    );
    assert_eq!(recipe["recipe"]["adjustments"]["exposure"], 2.0);
}

// ---------------------------------------------------------------------------
// Virtual copies
// ---------------------------------------------------------------------------

#[test]
fn lists_at_least_one_virtual_copy() {
    let dir = tempfile::tempdir().unwrap();
    let preview_dir = dir.path().join("previews");
    let source = dir.path().join("photo.png");
    make_png(&source, 16, 16);
    let mut server = new_server(&preview_dir);
    let loaded = tool_ok(
        &mut server,
        "lumina_load",
        json!({ "path": source.to_string_lossy() }),
    );
    let image_id = loaded["image_id"].as_str().unwrap().to_string();
    let copies = tool_ok(
        &mut server,
        "lumina_list_virtual_copies",
        json!({ "image_id": image_id }),
    );
    let list = copies["copies"].as_array().unwrap();
    assert!(!list.is_empty());
    assert!(list[0]["id"].is_string());
    assert!(list[0]["recipe_hash"].is_string());
}

// ---------------------------------------------------------------------------
// Error paths
// ---------------------------------------------------------------------------

#[test]
fn load_missing_file_is_file_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(&dir.path().join("previews"));
    let name = tool_error_name(
        &mut server,
        "lumina_load",
        json!({ "path": "/no/such/file.png" }),
    );
    assert_eq!(name, "FileNotFound");
}

#[test]
fn load_unsupported_format_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("note.txt");
    fs::write(&bad, b"hello").unwrap();
    let mut server = new_server(&dir.path().join("previews"));
    let name = tool_error_name(
        &mut server,
        "lumina_load",
        json!({ "path": bad.to_string_lossy() }),
    );
    assert_eq!(name, "UnsupportedFormat");
}

#[test]
fn edit_without_load_is_no_image_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(&dir.path().join("previews"));
    let name = tool_error_name(
        &mut server,
        "lumina_edit",
        json!({ "image_id": "whatever", "adjustments": { "exposure": 1.0 } }),
    );
    assert_eq!(name, "NoImageLoaded");
}

#[test]
fn edit_invalid_adjustment_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let preview_dir = dir.path().join("previews");
    let source = dir.path().join("photo.png");
    make_png(&source, 16, 16);
    let mut server = new_server(&preview_dir);
    let loaded = tool_ok(
        &mut server,
        "lumina_load",
        json!({ "path": source.to_string_lossy() }),
    );
    let image_id = loaded["image_id"].as_str().unwrap().to_string();
    let name = tool_error_name(
        &mut server,
        "lumina_edit",
        json!({ "image_id": image_id, "adjustments": { "exposure": 100.0 } }),
    );
    assert_eq!(name, "InvalidAdjustment");
}

#[test]
fn save_unsupported_format_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let preview_dir = dir.path().join("previews");
    let source = dir.path().join("photo.png");
    make_png(&source, 16, 16);
    let mut server = new_server(&preview_dir);
    let loaded = tool_ok(
        &mut server,
        "lumina_load",
        json!({ "path": source.to_string_lossy() }),
    );
    let image_id = loaded["image_id"].as_str().unwrap().to_string();
    let name = tool_error_name(
        &mut server,
        "lumina_save",
        json!({ "image_id": image_id, "output_path": "out.bmp", "format": "bmp" }),
    );
    assert_eq!(name, "UnsupportedFormat");
}

#[test]
fn preview_without_load_is_no_image_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(&dir.path().join("previews"));
    let name = tool_error_name(&mut server, "lumina_preview", json!({ "image_id": "nope" }));
    assert_eq!(name, "NoImageLoaded");
}

// ---------------------------------------------------------------------------
// REVIEW-MCP-QUALITY-1: strict integer bounds, no truncation
// ---------------------------------------------------------------------------

#[test]
fn save_quality_out_of_range_is_rejected_without_output() {
    let dir = tempfile::tempdir().unwrap();
    let preview_dir = dir.path().join("previews");
    let source = dir.path().join("photo.png");
    make_png(&source, 16, 16);
    let mut server = new_server(&preview_dir);
    let loaded = tool_ok(
        &mut server,
        "lumina_load",
        json!({ "path": source.to_string_lossy() }),
    );
    let image_id = loaded["image_id"].as_str().unwrap().to_string();

    // Out-of-range, negative, fractional and u8-truncating values must fail
    // as JSON-RPC InvalidParams — and must never write an artifact.
    for quality in [json!(0), json!(101), json!(256), json!(-5), json!(90.5)] {
        let output = dir.path().join(format!("rejected-{quality}.jpg"));
        let response = call_tool(
            &mut server,
            "lumina_save",
            json!({
                "image_id": image_id,
                "output_path": output.to_string_lossy(),
                "format": "jpeg",
                "quality": quality,
            }),
        );
        assert_eq!(
            response["result"]["isError"], true,
            "quality {quality} must be rejected"
        );
        assert_eq!(
            response["result"]["structuredContent"]["error"], "InvalidParams",
            "quality {quality} must fail as InvalidParams"
        );
        assert!(!output.exists(), "no artifact on rejected quality");
    }

    // The documented boundary values are accepted.
    for quality in [1u64, 100] {
        let output = dir.path().join(format!("boundary-{quality}.jpg"));
        tool_ok(
            &mut server,
            "lumina_save",
            json!({
                "image_id": image_id,
                "output_path": output.to_string_lossy(),
                "format": "jpeg",
                "quality": quality,
            }),
        );
        assert!(output.exists(), "quality {quality} is valid");
    }
}

#[test]
fn preview_max_width_must_be_positive_and_fit_u32() {
    let dir = tempfile::tempdir().unwrap();
    let preview_dir = dir.path().join("previews");
    let source = dir.path().join("photo.png");
    make_png(&source, 16, 16);
    let mut server = new_server(&preview_dir);
    let loaded = tool_ok(
        &mut server,
        "lumina_load",
        json!({ "path": source.to_string_lossy() }),
    );
    let image_id = loaded["image_id"].as_str().unwrap().to_string();

    // 0, fractional and >u32::MAX values must fail instead of truncating
    // (2^32 + n silently becoming a tiny width).
    for width in [json!(0), json!(1.5), json!(4_294_967_296i64)] {
        let response = call_tool(
            &mut server,
            "lumina_preview",
            json!({ "image_id": image_id, "max_width": width }),
        );
        assert_eq!(
            response["result"]["isError"], true,
            "max_width {width} must be rejected"
        );
        assert_eq!(
            response["result"]["structuredContent"]["error"],
            "InvalidParams"
        );
    }

    // Boundary value 1 is valid.
    let ok = tool_ok(
        &mut server,
        "lumina_preview",
        json!({ "image_id": image_id, "max_width": 1 }),
    );
    assert_eq!(ok["width"], 1);
}

// ---------------------------------------------------------------------------
// REVIEW-MCP-SAVE-1: extension validation + atomic export writes
// ---------------------------------------------------------------------------

#[test]
fn save_rejects_format_extension_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let preview_dir = dir.path().join("previews");
    let source = dir.path().join("photo.png");
    make_png(&source, 16, 16);
    let mut server = new_server(&preview_dir);
    let loaded = tool_ok(
        &mut server,
        "lumina_load",
        json!({ "path": source.to_string_lossy() }),
    );
    let image_id = loaded["image_id"].as_str().unwrap().to_string();

    // JPEG bytes must not land in a `.png` file.
    let wrong = dir.path().join("wrong.png");
    let response = call_tool(
        &mut server,
        "lumina_save",
        json!({
            "image_id": image_id,
            "output_path": wrong.to_string_lossy(),
            "format": "jpeg",
        }),
    );
    assert_eq!(
        response["result"]["structuredContent"]["error"],
        "InvalidParams"
    );
    assert!(!wrong.exists());

    // Unknown extensions are rejected loudly (same rule as the CLI).
    let bmp = dir.path().join("out.bmp");
    let response = call_tool(
        &mut server,
        "lumina_save",
        json!({
            "image_id": image_id,
            "output_path": bmp.to_string_lossy(),
            "format": "png",
        }),
    );
    assert_eq!(
        response["result"]["structuredContent"]["error"],
        "UnsupportedFormat"
    );
    assert!(!bmp.exists());
}

#[test]
fn save_refuses_to_overwrite_the_source_image() {
    let dir = tempfile::tempdir().unwrap();
    let preview_dir = dir.path().join("previews");
    let source = dir.path().join("photo.png");
    make_png(&source, 16, 16);
    let before = fs::read(&source).unwrap();
    let mut server = new_server(&preview_dir);
    let loaded = tool_ok(
        &mut server,
        "lumina_load",
        json!({ "path": source.to_string_lossy() }),
    );
    let image_id = loaded["image_id"].as_str().unwrap().to_string();

    let response = call_tool(
        &mut server,
        "lumina_save",
        json!({
            "image_id": image_id,
            "output_path": source.to_string_lossy(),
            "format": "png",
        }),
    );
    assert_eq!(
        response["result"]["structuredContent"]["error"],
        "EncodeError"
    );
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("refusing to overwrite the original"));
    // Non-destructive guarantee: the original is byte-identical afterwards.
    assert_eq!(fs::read(&source).unwrap(), before);
}

#[test]
fn save_overwrites_existing_output_and_leaves_no_temporaries() {
    let dir = tempfile::tempdir().unwrap();
    let preview_dir = dir.path().join("previews");
    let source = dir.path().join("photo.png");
    make_png(&source, 24, 24);
    let mut server = new_server(&preview_dir);
    let loaded = tool_ok(
        &mut server,
        "lumina_load",
        json!({ "path": source.to_string_lossy() }),
    );
    let image_id = loaded["image_id"].as_str().unwrap().to_string();
    let args = || {
        json!({
            "image_id": image_id,
            "output_path": dir.path().join("out.png").to_string_lossy(),
            "format": "png",
        })
    };

    tool_ok(&mut server, "lumina_save", args());
    tool_ok(&mut server, "lumina_save", args());

    let saved = ImageFrame::decode(&fs::read(dir.path().join("out.png")).unwrap()).unwrap();
    assert_eq!((saved.width, saved.height), (24, 24));
    let temporaries: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".out.png.tmp-")
        })
        .collect();
    assert!(temporaries.is_empty(), "atomic write leaves no temporaries");
}

// ---------------------------------------------------------------------------
// REVIEW-MCP-SESSION-1: compare-and-swap sidecar persistence
// ---------------------------------------------------------------------------

#[test]
fn edit_rejects_concurrently_modified_sidecar_instead_of_lost_update() {
    let dir = tempfile::tempdir().unwrap();
    let preview_dir = dir.path().join("previews");
    let source = dir.path().join("photo.png");
    let sidecar_path = lumina_sidecar::sidecar_path_for(&source);
    make_png(&source, 32, 32);

    let mut server = new_server(&preview_dir);
    let loaded = tool_ok(
        &mut server,
        "lumina_load",
        json!({ "path": source.to_string_lossy() }),
    );
    let image_id = loaded["image_id"].as_str().unwrap().to_string();

    // An external writer (CLI/GUI/another agent) modifies the sidecar behind
    // the session's back.
    let mut external = lumina_sidecar::load_sidecar(&sidecar_path).unwrap();
    external.virtual_copies[0]
        .recipe
        .adjustments
        .insert("contrast".into(), 0.5);
    lumina_sidecar::save_sidecar(&sidecar_path, &external).unwrap();

    // The session's edit must surface a conflict, not clobber the change.
    let response = call_tool(
        &mut server,
        "lumina_edit",
        json!({ "image_id": image_id, "adjustments": { "exposure": 1.0 } }),
    );
    assert_eq!(
        response["result"]["structuredContent"]["error"],
        "SidecarConflict"
    );

    // Disk still holds exactly the external edit.
    let on_disk = lumina_sidecar::load_sidecar(&sidecar_path).unwrap();
    assert_eq!(
        on_disk.virtual_copies[0].recipe.adjustments.get("contrast"),
        Some(&0.5)
    );
    assert_eq!(
        on_disk.virtual_copies[0].recipe.adjustments.get("exposure"),
        None
    );

    // After re-loading, the same edit succeeds and preserves the external value.
    let reloaded = tool_ok(
        &mut server,
        "lumina_load",
        json!({ "path": source.to_string_lossy() }),
    );
    let id_after_reload = reloaded["image_id"].as_str().unwrap();
    tool_ok(
        &mut server,
        "lumina_edit",
        json!({ "image_id": id_after_reload, "adjustments": { "exposure": 1.0 } }),
    );
    let merged = lumina_sidecar::load_sidecar(&sidecar_path).unwrap();
    assert_eq!(
        merged.virtual_copies[0].recipe.adjustments.get("exposure"),
        Some(&1.0)
    );
    assert_eq!(
        merged.virtual_copies[0].recipe.adjustments.get("contrast"),
        Some(&0.5)
    );
}

#[test]
fn load_rejects_sidecar_whose_recorded_identity_no_longer_matches() {
    let dir = tempfile::tempdir().unwrap();
    let preview_dir = dir.path().join("previews");
    let source = dir.path().join("photo.png");
    let sidecar_path = lumina_sidecar::sidecar_path_for(&source);
    make_png(&source, 16, 16);

    let mut server = new_server(&preview_dir);
    tool_ok(
        &mut server,
        "lumina_load",
        json!({ "path": source.to_string_lossy() }),
    );

    // Tamper the sidecar's recorded content hash (simulates a swapped or
    // modified source). The stale document must be reported, not used.
    let mut tampered = lumina_sidecar::load_sidecar(&sidecar_path).unwrap();
    tampered.source.content_hash = "blake3:tampered".into();
    lumina_sidecar::save_sidecar(&sidecar_path, &tampered).unwrap();

    let response = call_tool(
        &mut server,
        "lumina_load",
        json!({ "path": source.to_string_lossy() }),
    );
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["structuredContent"]["error"],
        "SidecarError"
    );
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("source changed since sidecar was written"));
}

#[test]
fn consecutive_edits_succeed_with_compare_and_swap() {
    // Two sequential session edits must both pass the CAS check because each
    // successful write updates the session's expected revision.
    let dir = tempfile::tempdir().unwrap();
    let preview_dir = dir.path().join("previews");
    let source = dir.path().join("photo.png");
    let sidecar_path = lumina_sidecar::sidecar_path_for(&source);
    make_png(&source, 16, 16);

    let mut server = new_server(&preview_dir);
    let loaded = tool_ok(
        &mut server,
        "lumina_load",
        json!({ "path": source.to_string_lossy() }),
    );
    let image_id = loaded["image_id"].as_str().unwrap().to_string();

    tool_ok(
        &mut server,
        "lumina_edit",
        json!({ "image_id": image_id, "adjustments": { "exposure": 0.5 } }),
    );
    tool_ok(
        &mut server,
        "lumina_edit",
        json!({ "image_id": image_id, "adjustments": { "exposure": 1.5 } }),
    );

    let doc = lumina_sidecar::load_sidecar(&sidecar_path).unwrap();
    assert_eq!(
        doc.virtual_copies[0].recipe.adjustments.get("exposure"),
        Some(&1.5)
    );
}
