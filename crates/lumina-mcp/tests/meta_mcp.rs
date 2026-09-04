//! LRPAR-G15-IPTC-S7: path-based metadata tools + read-only draft resource.
//!
//! Exercises the server through its public JSON-RPC surface (the same path an
//! MCP client uses). Fixtures are synthetic PNG/JPEG bytes — no network, no
//! RAW fixtures, no model downloads.

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_mcp::tools::meta_update::write_metadata_update_expected;
use lumina_mcp::Server;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn new_server(preview_dir: &Path) -> Server {
    Server::with_preview_dir(preview_dir.to_path_buf())
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
        "unexpected protocol error for `{name}`: {:?}",
        response.get("error")
    );
    assert_eq!(
        response["result"]["isError"],
        false,
        "`{name}` must succeed: {:?}",
        response["result"]["structuredContent"].get("error")
    );
    response["result"]["structuredContent"].clone()
}

fn tool_error_name(server: &mut Server, name: &str, args: Value) -> String {
    let response = call_tool(server, name, args);
    assert!(
        response.get("error").is_none(),
        "`{name}` must fail as a tool execution error, not a protocol error"
    );
    assert_eq!(
        response["result"]["isError"], true,
        "expected a tool execution error for `{name}`"
    );
    response["result"]["structuredContent"]["error"]
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
    fs::write(path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
}

fn import(server: &mut Server, path: &Path) {
    tool_ok(
        server,
        "lumina_import",
        json!({ "path": path.to_string_lossy() }),
    );
}

fn get_draft(server: &mut Server, path: &Path) -> Value {
    tool_ok(
        server,
        "lumina_get_metadata_draft",
        json!({ "path": path.to_string_lossy() }),
    )
}

fn history_len(server: &mut Server, path: &Path) -> u64 {
    get_draft(server, path)["history_len"].as_u64().unwrap()
}

/// Minimal percent-encoding for the test URIs (mirrors the server rule for
/// the ASCII fixture names used here).
fn draft_uri(path_str: &str) -> String {
    format!(
        "metadata://draft/{}",
        path_str.replace('%', "%25").replace(' ', "%20")
    )
}

// ---------------------------------------------------------------------------
// Tool schemas + initialize capabilities
// ---------------------------------------------------------------------------

#[test]
fn metadata_tools_are_listed_with_valid_schemas() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(dir.path());
    let response = call(&mut server, "tools/list", json!({}));
    let tools = response["result"]["tools"].as_array().unwrap();
    for expected in [
        "lumina_get_metadata_draft",
        "lumina_update_metadata_draft",
        "lumina_apply_meta_preset",
        "lumina_batch_sync_metadata",
        "lumina_trigger_export",
    ] {
        let tool = tools
            .iter()
            .find(|tool| tool["name"] == expected)
            .unwrap_or_else(|| panic!("missing tool `{expected}`"));
        assert_eq!(tool["inputSchema"]["type"], "object", "`{expected}`");
        assert!(
            tool["inputSchema"]["required"].is_array(),
            "`{expected}` has no required array"
        );
        assert!(
            tool["inputSchema"]["properties"].is_object(),
            "`{expected}` has no properties map"
        );
        assert!(tool["description"].is_string(), "`{expected}`");
    }
}

#[test]
fn initialize_declares_resources_capability_without_prompts() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(dir.path());
    let response = call(&mut server, "initialize", json!({}));
    assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(response["result"]["capabilities"]["tools"], json!({}));
    assert_eq!(
        response["result"]["capabilities"]["resources"],
        json!({ "subscribe": false, "listChanged": false })
    );
    assert!(
        response["result"]["capabilities"].get("prompts").is_none(),
        "prompts stay unimplemented"
    );
}

#[test]
fn resources_list_is_empty_and_read_rejects_foreign_uris() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(dir.path());

    let listed = call(&mut server, "resources/list", json!({}));
    assert_eq!(listed["result"], json!({ "resources": [] }));

    // A foreign scheme is a loud protocol-level error (InvalidParams/-32602),
    // not an empty result.
    let response = call(
        &mut server,
        "resources/read",
        json!({ "uri": "metadata://other/x" }),
    );
    assert_eq!(response["error"]["code"], -32602);
    assert!(response["error"]["message"]
        .as_str()
        .unwrap()
        .contains("InvalidParams"));

    // Malformed escapes fail loudly as well.
    let response = call(
        &mut server,
        "resources/read",
        json!({ "uri": "metadata://draft/a%zz" }),
    );
    assert_eq!(response["error"]["code"], -32602);
}

// ---------------------------------------------------------------------------
// Get / update roundtrip + resource identity
// ---------------------------------------------------------------------------

#[test]
fn metadata_draft_roundtrip_via_tools_and_resource() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(&dir.path().join("previews"));
    // A space in the name exercises the URI percent-encoding roundtrip.
    let source = dir.path().join("my photo.png");
    make_png(&source, 32, 32);
    let path_str = source.to_string_lossy().to_string();
    import(&mut server, &source);

    // Fresh draft: empty overlay, no history, non-JPEG embedded unavailable.
    let fresh = get_draft(&mut server, &source);
    assert_eq!(fresh["path"], path_str);
    assert_eq!(fresh["draft"], json!({}));
    assert_eq!(fresh["keywords"], json!([]));
    assert_eq!(fresh["history_len"], 0);
    assert_eq!(fresh["embedded"]["available"], false);
    assert_eq!(fresh["embedded"]["fields"], json!({}));
    assert_eq!(fresh["status"], "ok");

    // Set two fields → exactly one history entry (rev 1).
    let updated = tool_ok(
        &mut server,
        "lumina_update_metadata_draft",
        json!({ "path": path_str, "fields": { "title": "Startschuss", "city": "Berlin" } }),
    );
    assert_eq!(updated, json!({ "ok": true, "rev": 1 }));
    let after_set = get_draft(&mut server, &source);
    assert_eq!(after_set["draft"]["title"], "Startschuss");
    assert_eq!(after_set["draft"]["city"], "Berlin");
    assert_eq!(after_set["history_len"], 1);

    // Idempotent re-application writes nothing (rev and history unchanged).
    let noop = tool_ok(
        &mut server,
        "lumina_update_metadata_draft",
        json!({ "path": path_str, "fields": { "title": "Startschuss", "city": "Berlin" } }),
    );
    assert_eq!(noop, json!({ "ok": true, "rev": 1 }));
    assert_eq!(history_len(&mut server, &source), 1);

    // Clearing one field removes its key and appends rev 2.
    let cleared = tool_ok(
        &mut server,
        "lumina_update_metadata_draft",
        json!({ "path": path_str, "clear_fields": ["city"] }),
    );
    assert_eq!(cleared, json!({ "ok": true, "rev": 2 }));
    let after_clear = get_draft(&mut server, &source);
    assert!(after_clear["draft"].get("city").is_none());
    assert_eq!(after_clear["draft"]["title"], "Startschuss");
    assert_eq!(after_clear["history_len"], 2);

    // `resources/read` delivers exactly the tool JSON (same canonical value).
    let uri = draft_uri(&path_str);
    let read = call(&mut server, "resources/read", json!({ "uri": uri }));
    assert!(
        read.get("error").is_none(),
        "unexpected resource error: {:?}",
        read.get("error")
    );
    let contents = read["result"]["contents"].as_array().unwrap();
    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0]["uri"], uri);
    assert_eq!(contents[0]["mimeType"], "application/json");
    let text: Value = serde_json::from_str(contents[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text, after_clear);

    // The session is untouched by path-based tools: nothing is loaded.
    assert!(server.session.current.is_none());
}

#[test]
fn update_rejects_unknown_and_contradictory_input_loudly() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(&dir.path().join("previews"));
    let source = dir.path().join("photo.png");
    make_png(&source, 16, 16);
    let path_str = source.to_string_lossy().to_string();
    import(&mut server, &source);

    // Unknown field id.
    assert_eq!(
        tool_error_name(
            &mut server,
            "lumina_update_metadata_draft",
            json!({ "path": path_str, "fields": { "nope": "x" } }),
        ),
        "InvalidParams"
    );
    // `keywords` is routed elsewhere, never a draft field.
    assert_eq!(
        tool_error_name(
            &mut server,
            "lumina_update_metadata_draft",
            json!({ "path": path_str, "fields": { "keywords": "x" } }),
        ),
        "InvalidParams"
    );
    // Unknown clear id.
    assert_eq!(
        tool_error_name(
            &mut server,
            "lumina_update_metadata_draft",
            json!({ "path": path_str, "clear_fields": ["nope"] }),
        ),
        "InvalidParams"
    );
    // Non-string values and non-string clear entries.
    assert_eq!(
        tool_error_name(
            &mut server,
            "lumina_update_metadata_draft",
            json!({ "path": path_str, "fields": { "title": 42 } }),
        ),
        "InvalidParams"
    );
    assert_eq!(
        tool_error_name(
            &mut server,
            "lumina_update_metadata_draft",
            json!({ "path": path_str, "clear_fields": [42] }),
        ),
        "InvalidParams"
    );
    // The same field in `fields` and `clear_fields` is a loud contradiction.
    assert_eq!(
        tool_error_name(
            &mut server,
            "lumina_update_metadata_draft",
            json!({ "path": path_str, "fields": { "title": "x" }, "clear_fields": ["title"] }),
        ),
        "InvalidParams"
    );
    // All-or-nothing: nothing was written by any rejected call.
    assert_eq!(history_len(&mut server, &source), 0);
    assert_eq!(get_draft(&mut server, &source)["draft"], json!({}));

    // Missing file vs. missing sidecar are distinct loud errors.
    assert_eq!(
        tool_error_name(
            &mut server,
            "lumina_update_metadata_draft",
            json!({ "path": "/no/such/file.png", "fields": { "title": "x" } }),
        ),
        "FileNotFound"
    );
    let bare = dir.path().join("bare.png");
    make_png(&bare, 8, 8);
    let response = call_tool(
        &mut server,
        "lumina_update_metadata_draft",
        json!({ "path": bare.to_string_lossy(), "fields": { "title": "x" } }),
    );
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["structuredContent"]["error"],
        "SidecarError"
    );
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("lumina_import"));
}

// ---------------------------------------------------------------------------
// CAS conflict path (-32010)
// ---------------------------------------------------------------------------

#[test]
fn update_cas_conflict_surfaces_sidecar_conflict_minus_32010() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(&dir.path().join("previews"));
    let source = dir.path().join("photo.png");
    make_png(&source, 16, 16);
    import(&mut server, &source);
    let sidecar_path = lumina_sidecar::sidecar_path_for(&source);

    // Snapshot the document + revision, then let an external writer
    // (CLI/GUI/another agent) modify the sidecar behind our back.
    let stale = lumina_sidecar::load_sidecar(&sidecar_path).unwrap();
    let stale_rev = lumina_sidecar::document_revision(&stale).unwrap();
    let mut external = stale.clone();
    external
        .metadata
        .draft
        .insert("title".into(), "Extern".into());
    lumina_sidecar::save_sidecar(&sidecar_path, &external).unwrap();

    // The stale write must surface a conflict, not clobber the change.
    let mut fields = BTreeMap::new();
    fields.insert("title".to_string(), "Neu".to_string());
    let error = write_metadata_update_expected(&sidecar_path, &stale, &fields, &[], &stale_rev)
        .expect_err("a stale CAS expectation must conflict");
    assert!(
        matches!(error, lumina_mcp::McpError::SidecarConflict(_)),
        "expected SidecarConflict, got `{error:?}`"
    );
    assert_eq!(error.code(), -32010);
    assert_eq!(error.name(), "SidecarConflict");

    // Disk still holds exactly the external edit.
    let on_disk = lumina_sidecar::load_sidecar(&sidecar_path).unwrap();
    assert_eq!(
        on_disk.metadata.draft.get("title").map(String::as_str),
        Some("Extern")
    );

    // A fresh expectation succeeds (the tool path recomputes it per call).
    let fresh = lumina_sidecar::load_sidecar(&sidecar_path).unwrap();
    let fresh_rev = lumina_sidecar::document_revision(&fresh).unwrap();
    let rev = write_metadata_update_expected(&sidecar_path, &fresh, &fields, &[], &fresh_rev)
        .expect("a fresh CAS expectation must succeed");
    assert_eq!(rev, 1);
    drop(server);
}

// ---------------------------------------------------------------------------
// Presets
// ---------------------------------------------------------------------------

fn write_preset(dir: &Path, name: &str, fields: Value, placeholders: Value) -> PathBuf {
    let path = dir.join(format!("{name}.lumina-meta-preset.json"));
    fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "format": "lumina-meta-preset",
            "version": 1,
            "name": name,
            "fields": fields,
            "placeholders": placeholders,
        }))
        .unwrap(),
    )
    .unwrap();
    path
}

#[test]
fn preset_apply_reports_per_path_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(&dir.path().join("previews"));
    let first = dir.path().join("a.png");
    let second = dir.path().join("b.png");
    let no_sidecar = dir.path().join("c.png");
    for path in [&first, &second, &no_sidecar] {
        make_png(path, 16, 16);
    }
    import(&mut server, &first);
    import(&mut server, &second);
    let preset = write_preset(
        dir.path(),
        "Veranstaltung",
        json!({ "title": "Startschuss", "city": "Berlin" }),
        json!([]),
    );
    let preset_str = preset.to_string_lossy().to_string();

    let report = tool_ok(
        &mut server,
        "lumina_apply_meta_preset",
        json!({
            "paths": [first.to_string_lossy(), second.to_string_lossy(), no_sidecar.to_string_lossy()],
            "preset": preset_str,
        }),
    );
    assert_eq!(report["status"], "partial");
    assert_eq!(report["updated"], 2);
    assert_eq!(report["unchanged"], 0);
    assert_eq!(report["failed"], 1);
    assert_eq!(report["items"][0]["status"], "updated");
    assert_eq!(report["items"][2]["status"], "failed");
    assert!(
        report["items"][2]["error"]
            .as_str()
            .unwrap()
            .contains("lumina_import"),
        "missing sidecar names the remedy"
    );
    assert_eq!(
        get_draft(&mut server, &first)["draft"]["title"],
        "Startschuss"
    );

    // Idempotent re-application reports `unchanged` (no new history entries).
    let again = tool_ok(
        &mut server,
        "lumina_apply_meta_preset",
        json!({
            "paths": [first.to_string_lossy(), second.to_string_lossy()],
            "preset": preset_str,
        }),
    );
    assert_eq!(again["status"], "ok");
    assert_eq!(again["updated"], 0);
    assert_eq!(again["unchanged"], 2);
    assert_eq!(again["failed"], 0);
    assert_eq!(history_len(&mut server, &first), 1);

    // Unknown preset spec aborts the whole call loudly, nothing written.
    assert_eq!(
        tool_error_name(
            &mut server,
            "lumina_apply_meta_preset",
            json!({ "paths": [first.to_string_lossy()], "preset": "/no/such/preset.json" }),
        ),
        "InvalidParams"
    );
}

#[test]
fn preset_apply_renders_dynamic_vars_loudly() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(&dir.path().join("previews"));
    let source = dir.path().join("photo.png");
    make_png(&source, 16, 16);
    let path_str = source.to_string_lossy().to_string();
    import(&mut server, &source);
    let preset = write_preset(
        dir.path(),
        "Dynamisch",
        json!({ "title": "{event_name} in {ort}" }),
        json!([
            { "name": "event_name", "description": "Name" },
            { "name": "ort", "description": "Stadt" },
        ]),
    );
    let preset_str = preset.to_string_lossy().to_string();

    // A missing placeholder variable aborts everything (nothing written).
    assert_eq!(
        tool_error_name(
            &mut server,
            "lumina_apply_meta_preset",
            json!({ "paths": [path_str], "preset": preset_str, "vars": { "event_name": "Fest" } }),
        ),
        "InvalidParams"
    );
    // So does an unknown variable.
    assert_eq!(
        tool_error_name(
            &mut server,
            "lumina_apply_meta_preset",
            json!({ "paths": [path_str], "preset": preset_str,
                    "vars": { "event_name": "Fest", "ort": "Berlin", "extra": "x" } }),
        ),
        "InvalidParams"
    );
    assert_eq!(history_len(&mut server, &source), 0);

    // Complete vars render and apply.
    let report = tool_ok(
        &mut server,
        "lumina_apply_meta_preset",
        json!({ "paths": [path_str], "preset": preset_str,
                "vars": { "event_name": "Fest", "ort": "Berlin" } }),
    );
    assert_eq!(report["updated"], 1);
    assert_eq!(
        get_draft(&mut server, &source)["draft"]["title"],
        "Fest in Berlin"
    );
}

// ---------------------------------------------------------------------------
// Sync
// ---------------------------------------------------------------------------

#[test]
fn sync_mirrors_selected_fields_and_keywords() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(&dir.path().join("previews"));
    let source = dir.path().join("src.png");
    let target = dir.path().join("tgt.png");
    make_png(&source, 16, 16);
    make_png(&target, 16, 16);
    import(&mut server, &source);
    import(&mut server, &target);
    let source_str = source.to_string_lossy().to_string();
    let target_str = target.to_string_lossy().to_string();

    tool_ok(
        &mut server,
        "lumina_update_metadata_draft",
        json!({ "path": source_str, "fields": { "title": "S", "city": "SCity" } }),
    );
    tool_ok(
        &mut server,
        "lumina_update_metadata_draft",
        json!({ "path": target_str, "fields": { "title": "T", "headline": "H" } }),
    );
    // Keywords travel outside the draft tool: seed them directly (valid values).
    for (path, keywords) in [
        (&source, vec!["k1".to_string()]),
        (&target, vec!["alt".to_string()]),
    ] {
        let sidecar = lumina_sidecar::sidecar_path_for(path);
        let mut document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
        document.keywords = keywords;
        lumina_sidecar::save_sidecar(&sidecar, &document).unwrap();
    }

    // Selected fields mirror; unselected ones stay untouched.
    let report = tool_ok(
        &mut server,
        "lumina_batch_sync_metadata",
        json!({ "source": source_str, "targets": [target_str], "fields": ["title", "keywords"] }),
    );
    assert_eq!(report["status"], "ok");
    assert_eq!(report["updated"], 1);
    let synced = get_draft(&mut server, &target);
    assert_eq!(synced["draft"]["title"], "S");
    assert!(
        synced["draft"].get("city").is_none(),
        "unselected stays absent"
    );
    assert_eq!(synced["draft"]["headline"], "H");
    assert_eq!(synced["keywords"], json!(["k1"]));

    // Mirror removal: a selected field absent in the source is removed.
    tool_ok(
        &mut server,
        "lumina_update_metadata_draft",
        json!({ "path": source_str, "clear_fields": ["city"] }),
    );
    // Give the target a city first so the removal is observable.
    tool_ok(
        &mut server,
        "lumina_update_metadata_draft",
        json!({ "path": target_str, "fields": { "city": "TCity" } }),
    );
    let removal = tool_ok(
        &mut server,
        "lumina_batch_sync_metadata",
        json!({ "source": source_str, "targets": [target_str], "fields": ["city"] }),
    );
    assert_eq!(removal["updated"], 1);
    assert!(get_draft(&mut server, &target)["draft"]
        .get("city")
        .is_none());

    // Idempotent re-sync reports `unchanged`.
    let same = tool_ok(
        &mut server,
        "lumina_batch_sync_metadata",
        json!({ "source": source_str, "targets": [target_str], "fields": ["city"] }),
    );
    assert_eq!(same["unchanged"], 1);
    assert_eq!(same["updated"], 0);
}

#[test]
fn sync_isolates_per_target_failures_and_rejects_empty_fields() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(&dir.path().join("previews"));
    let source = dir.path().join("src.png");
    let good = dir.path().join("good.png");
    let bare = dir.path().join("bare.png");
    for path in [&source, &good, &bare] {
        make_png(path, 16, 16);
    }
    import(&mut server, &source);
    import(&mut server, &good);
    let source_str = source.to_string_lossy().to_string();
    let before = history_len(&mut server, &good);

    // A target without a sidecar fails only its own item.
    let report = tool_ok(
        &mut server,
        "lumina_batch_sync_metadata",
        json!({
            "source": source_str,
            "targets": [good.to_string_lossy(), bare.to_string_lossy()],
            "fields": ["title"],
        }),
    );
    assert_eq!(report["status"], "partial");
    assert_eq!(
        report["updated"], 0,
        "empty source draft mirrors nothing new"
    );
    assert_eq!(report["unchanged"], 1);
    assert_eq!(report["failed"], 1);
    assert!(report["items"][1]["error"]
        .as_str()
        .unwrap()
        .contains("lumina_import"));

    // `fields` is required: empty, unknown, or missing aborts the whole call
    // loudly and writes nothing.
    for fields in [json!([]), json!(["nope"]), json!([""])] {
        assert_eq!(
            tool_error_name(
                &mut server,
                "lumina_batch_sync_metadata",
                json!({ "source": source_str, "targets": [good.to_string_lossy()], "fields": fields }),
            ),
            "InvalidParams"
        );
    }
    assert_eq!(history_len(&mut server, &good), before);

    // A missing source sidecar aborts the whole call loudly.
    assert_eq!(
        tool_error_name(
            &mut server,
            "lumina_batch_sync_metadata",
            json!({ "source": bare.to_string_lossy(), "targets": [good.to_string_lossy()], "fields": ["title"] }),
        ),
        "SidecarError"
    );
}

// ---------------------------------------------------------------------------
// Trigger export (JPEG-only gate + bake-in)
// ---------------------------------------------------------------------------

#[test]
fn trigger_export_honors_jpeg_only_gate_and_bakes_in() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(&dir.path().join("previews"));
    let source = dir.path().join("photo.png");
    make_png(&source, 32, 32);
    let path_str = source.to_string_lossy().to_string();
    import(&mut server, &source);
    tool_ok(
        &mut server,
        "lumina_update_metadata_draft",
        json!({ "path": path_str, "fields": { "title": "Export-Test" } }),
    );

    // Plain PNG export: no metadata involved, no `metadata_written` key.
    let plain_png = dir.path().join("plain.png");
    let exported = tool_ok(
        &mut server,
        "lumina_trigger_export",
        json!({ "path": path_str, "output_path": plain_png.to_string_lossy(), "format": "png" }),
    );
    assert_eq!(exported["ok"], true);
    assert!(exported.get("metadata_written").is_none());
    assert!(plain_png.exists());

    // `write_metadata` + PNG is a loud InvalidParams — and writes nothing.
    let rejected = dir.path().join("rejected.png");
    assert_eq!(
        tool_error_name(
            &mut server,
            "lumina_trigger_export",
            json!({
                "path": path_str,
                "output_path": rejected.to_string_lossy(),
                "format": "png",
                "write_metadata": true,
            }),
        ),
        "InvalidParams"
    );
    assert!(!rejected.exists(), "no artifact on the rejected gate");

    // JPEG + flag: splice + `written` record; the title re-parses from bytes.
    let baked = dir.path().join("baked.jpg");
    let with_meta = tool_ok(
        &mut server,
        "lumina_trigger_export",
        json!({
            "path": path_str,
            "output_path": baked.to_string_lossy(),
            "format": "jpeg",
            "write_metadata": true,
        }),
    );
    assert_eq!(with_meta["ok"], true);
    assert_eq!(with_meta["metadata_written"]["status"], "written");
    assert_eq!(with_meta["metadata_written"]["iim"], true);
    assert_eq!(with_meta["metadata_written"]["xmp"], true);
    let meta = lumina_iptc::extract_metadata(&fs::read(&baked).unwrap()).unwrap();
    assert_eq!(meta.title.as_deref(), Some("Export-Test"));

    // JPEG without the flag: today's behavior (no metadata, no record).
    let plain_jpg = dir.path().join("plain.jpg");
    let without_flag = tool_ok(
        &mut server,
        "lumina_trigger_export",
        json!({ "path": path_str, "output_path": plain_jpg.to_string_lossy(), "format": "jpeg" }),
    );
    assert!(without_flag.get("metadata_written").is_none());
    let bare = lumina_iptc::extract_metadata(&fs::read(&plain_jpg).unwrap()).unwrap();
    assert!(bare.is_empty());

    // Unknown virtual copies fail loudly.
    assert_eq!(
        tool_error_name(
            &mut server,
            "lumina_trigger_export",
            json!({
                "path": path_str,
                "output_path": dir.path().join("x.jpg").to_string_lossy(),
                "format": "jpeg",
                "virtual_copy": "nope",
            }),
        ),
        "UnknownCopy"
    );
}

#[test]
fn trigger_export_reports_empty_draft_without_failing() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = new_server(&dir.path().join("previews"));
    let source = dir.path().join("photo.png");
    make_png(&source, 16, 16);
    let path_str = source.to_string_lossy().to_string();
    import(&mut server, &source);

    // An empty draft exports plainly with a loud `empty` record (no silent no-op).
    let output = dir.path().join("empty.jpg");
    let exported = tool_ok(
        &mut server,
        "lumina_trigger_export",
        json!({
            "path": path_str,
            "output_path": output.to_string_lossy(),
            "format": "jpeg",
            "write_metadata": true,
        }),
    );
    assert_eq!(exported["metadata_written"]["status"], "empty");
    assert!(output.exists());
}
