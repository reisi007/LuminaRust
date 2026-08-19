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
    response["error"]["data"]["error"]
        .as_str()
        .expect("error.data.error")
        .to_string()
}

fn make_png(path: &Path, width: u32, height: u32) {
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for (index, pixel) in pixels.chunks_exact_mut(4).enumerate() {
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
fn unknown_tool_returns_method_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(dir.path());
    let name = tool_error_name(&mut server, "does_not_exist", json!({}));
    assert_eq!(name, "MethodNotFound");
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
