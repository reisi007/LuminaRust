//! F-101-F1 tests: full CLI coverage tools (`lumina_import`, `lumina_batch`,
//! `lumina_reindex`, `lumina_dust_removal`) plus the session-isolation
//! guarantee of the path-based bulk tools.
//!
//! Success and error paths run through the public JSON-RPC surface, exactly
//! like an MCP client. No network, no real RAW fixtures.

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_mcp::Server;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

fn new_server(preview_dir: &Path) -> Server {
    // Race-free injection (CI-MCP-PREVIEW-FLAKY-1): never mutate the
    // process-global `LUMINA_MCP_PREVIEW_DIR` from parallel test threads.
    Server::with_preview_dir(preview_dir.to_path_buf())
}

fn call_tool(server: &mut Server, name: &str, args: Value) -> Value {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": name, "arguments": args },
    });
    server.handle_message(request).expect("expected a response")
}

fn tool_ok(server: &mut Server, name: &str, args: Value) -> Value {
    let response = call_tool(server, name, args);
    assert!(
        response.get("error").is_none(),
        "unexpected error for `{name}`: {:?}",
        response.get("error")
    );
    assert_eq!(
        response["result"]["isError"], false,
        "`{name}` must succeed"
    );
    response["result"]["structuredContent"].clone()
}

/// Returns the stable error name plus the full message of a tool execution
/// failure.
fn tool_error(server: &mut Server, name: &str, args: Value) -> (String, String) {
    let response = call_tool(server, name, args);
    let data = if response.get("error").is_some() {
        (
            response["error"]["data"]["error"].clone(),
            response["error"]["message"].clone(),
        )
    } else {
        assert_eq!(
            response["result"]["isError"], true,
            "expected a tool execution error for `{name}`"
        );
        (
            response["result"]["structuredContent"]["error"].clone(),
            response["result"]["content"][0]["text"].clone(),
        )
    };
    (
        data.0.as_str().unwrap_or_default().to_string(),
        data.1.as_str().unwrap_or_default().to_string(),
    )
}

fn make_png(path: &Path, width: u32, height: u32, base: u8) {
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let x = (index % width as usize) as u8;
        let y = (index / width as usize) as u8;
        pixel[0] = x.wrapping_add(base);
        pixel[1] = y;
        pixel[2] = 128;
        pixel[3] = 255;
    }
    let frame = ImageFrame::new(width, height, pixels).unwrap();
    fs::write(path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
}

fn sidecar_json(source: &Path) -> PathBuf {
    lumina_sidecar::sidecar_path_for(source)
}

// ---------------------------------------------------------------------------
// lumina_import
// ---------------------------------------------------------------------------

#[test]
fn import_creates_then_validates_sidecar_without_session_change() {
    let dir = tempfile::tempdir().unwrap();
    let source_a = dir.path().join("a.png");
    let source_b = dir.path().join("b.png");
    make_png(&source_a, 8, 8, 0);
    make_png(&source_b, 8, 8, 40);
    let mut server = new_server(&dir.path().join("previews"));

    // Keep an image in the session to prove import never touches it.
    let loaded = tool_ok(
        &mut server,
        "lumina_load",
        json!({ "path": source_a.to_string_lossy() }),
    );
    let image_id = loaded["image_id"].as_str().unwrap().to_string();

    // First import creates the sidecar.
    let first = tool_ok(
        &mut server,
        "lumina_import",
        json!({ "path": source_b.to_string_lossy() }),
    );
    assert_eq!(first["status"], "created");
    assert_eq!(first["ok"], true);
    assert!(sidecar_json(&source_b).is_file());

    // Second import validates the existing document.
    let second = tool_ok(
        &mut server,
        "lumina_import",
        json!({ "path": source_b.to_string_lossy() }),
    );
    assert_eq!(second["status"], "validated");

    // The session image is still usable after both imports.
    let recipe = tool_ok(
        &mut server,
        "lumina_get_recipe",
        json!({ "image_id": image_id }),
    );
    assert!(recipe["recipe_hash"].is_string());
}

#[test]
fn import_reports_changed_source_loudly_and_rejects_bad_paths() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("photo.png");
    make_png(&source, 8, 8, 0);
    let mut server = new_server(&dir.path().join("previews"));

    tool_ok(
        &mut server,
        "lumina_import",
        json!({ "path": source.to_string_lossy() }),
    );

    // Swap the file contents behind the existing sidecar.
    make_png(&source, 8, 8, 90);
    let (error_name, message) = tool_error(
        &mut server,
        "lumina_import",
        json!({ "path": source.to_string_lossy() }),
    );
    assert_eq!(error_name, "SidecarError");
    assert!(message.contains("source changed since sidecar was written"));

    // Missing file and unsupported format keep their documented error names.
    let (missing, _) = tool_error(
        &mut server,
        "lumina_import",
        json!({ "path": "/no/such/file.png" }),
    );
    assert_eq!(missing, "FileNotFound");

    let note = dir.path().join("note.txt");
    fs::write(&note, b"hello").unwrap();
    let (unsupported, _) = tool_error(
        &mut server,
        "lumina_import",
        json!({ "path": note.to_string_lossy() }),
    );
    assert_eq!(unsupported, "UnsupportedFormat");
}

// ---------------------------------------------------------------------------
// lumina_batch
// ---------------------------------------------------------------------------

#[test]
fn batch_renders_directory_with_sidecar_recipe_and_writes_nothing_extra() {
    let dir = tempfile::tempdir().unwrap();
    let input_dir = dir.path().join("in");
    let nested = input_dir.join("nested");
    let output_dir = dir.path().join("out");
    fs::create_dir_all(&nested).unwrap();

    let plain = input_dir.join("plain.png");
    make_png(&plain, 8, 8, 10);
    let edited = nested.join("edited.png");
    make_png(&edited, 8, 8, 10);

    // Give `edited.png` a sidecar with a strong exposure push via the normal
    // load/edit path; `plain.png` stays sidecar-less on purpose.
    let mut server = new_server(&dir.path().join("previews"));
    let loaded = tool_ok(
        &mut server,
        "lumina_load",
        json!({ "path": edited.to_string_lossy() }),
    );
    let image_id = loaded["image_id"].as_str().unwrap().to_string();
    tool_ok(
        &mut server,
        "lumina_edit",
        json!({ "image_id": image_id, "adjustments": { "exposure": 5.0 } }),
    );

    let before = count_files_with_extension(&input_dir, ".lumina.json");
    let report = tool_ok(
        &mut server,
        "lumina_batch",
        json!({
            "input": input_dir.to_string_lossy(),
            "output": output_dir.to_string_lossy(),
            "format": "png",
        }),
    );

    assert_eq!(report["status"], "ok", "report: {report}");
    assert_eq!(report["succeeded"], 2);
    assert_eq!(report["failed"], 0);

    // Both outputs exist, decode at source resolution, and the recipe edit is
    // visible: exposure +5 EV pushes pixels away from the source values.
    let out_plain = ImageFrame::decode(&fs::read(output_dir.join("plain.png")).unwrap()).unwrap();
    let out_edited = ImageFrame::decode(&fs::read(output_dir.join("edited.png")).unwrap()).unwrap();
    assert_eq!((out_plain.width, out_plain.height), (8, 8));
    assert_eq!((out_edited.width, out_edited.height), (8, 8));
    let source_pixels = ImageFrame::decode(&fs::read(&edited).unwrap()).unwrap();
    assert_ne!(
        out_edited.pixels[..4],
        source_pixels.pixels[..4],
        "sidecar recipe must be applied"
    );

    // Documented limit: batch never materializes sidecars.
    let after = count_files_with_extension(&input_dir, ".lumina.json");
    assert_eq!(before, 1, "only the edited sidecar exists beforehand");
    assert_eq!(after, 1, "batch must not write sidecars");
}

#[test]
fn batch_reports_per_item_failures_without_losing_good_outputs() {
    let dir = tempfile::tempdir().unwrap();
    let input_dir = dir.path().join("in");
    let output_dir = dir.path().join("out");
    fs::create_dir_all(&input_dir).unwrap();
    make_png(&input_dir.join("good.png"), 8, 8, 0);
    fs::write(input_dir.join("broken.png"), b"not a png").unwrap();

    let mut server = new_server(&dir.path().join("previews"));
    let report = tool_ok(
        &mut server,
        "lumina_batch",
        json!({
            "input": input_dir.to_string_lossy(),
            "output": output_dir.to_string_lossy(),
        }),
    );

    // Bulk semantics: per-item failures are reported loudly inside an ok
    // transport result — never silent, never losing the successful items.
    assert_eq!(report["status"], "failed");
    assert_eq!(report["succeeded"], 1);
    assert_eq!(report["failed"], 1);
    let results = report["results"].as_array().unwrap();
    let broken = results
        .iter()
        .find(|r| r["status"] == "failed")
        .expect("broken item reported");
    assert_eq!(broken["error_name"], "DecodeError");
    assert!(output_dir.join("good.png").is_file());
    assert!(!output_dir.join("broken.png").exists());
}

#[test]
fn batch_rejects_output_collisions_before_any_write() {
    let dir = tempfile::tempdir().unwrap();
    let input_dir = dir.path().join("in");
    let output_dir = dir.path().join("out");
    fs::create_dir_all(input_dir.join("a")).unwrap();
    fs::create_dir_all(input_dir.join("b")).unwrap();
    make_png(&input_dir.join("a/x.png"), 4, 4, 0);
    make_png(&input_dir.join("b/x.png"), 4, 4, 60);

    let mut server = new_server(&dir.path().join("previews"));
    let (error_name, message) = tool_error(
        &mut server,
        "lumina_batch",
        json!({
            "input": input_dir.to_string_lossy(),
            "output": output_dir.to_string_lossy(),
        }),
    );
    assert_eq!(error_name, "InvalidParams");
    assert!(message.contains("collision"), "{message}");
    // Refusal happens BEFORE the output directory is created.
    assert!(!output_dir.exists());
}

#[test]
fn batch_validates_arguments_before_touching_the_filesystem() {
    let dir = tempfile::tempdir().unwrap();
    let input_dir = dir.path().join("in");
    let output_dir = dir.path().join("out");
    fs::create_dir_all(&input_dir).unwrap();
    make_png(&input_dir.join("a.png"), 4, 4, 0);
    let mut server = new_server(&dir.path().join("previews"));

    let (not_dir, _) = tool_error(
        &mut server,
        "lumina_batch",
        json!({
            "input": dir.path().join("missing-dir").to_string_lossy(),
            "output": output_dir.to_string_lossy(),
        }),
    );
    assert_eq!(not_dir, "FileNotFound");

    let (bad_format, _) = tool_error(
        &mut server,
        "lumina_batch",
        json!({
            "input": input_dir.to_string_lossy(),
            "output": output_dir.to_string_lossy(),
            "format": "bmp",
        }),
    );
    assert_eq!(bad_format, "UnsupportedFormat");

    let (bad_quality, _) = tool_error(
        &mut server,
        "lumina_batch",
        json!({
            "input": input_dir.to_string_lossy(),
            "output": output_dir.to_string_lossy(),
            "format": "jpeg",
            "quality": 101,
        }),
    );
    assert_eq!(bad_quality, "InvalidParams");
    assert!(!output_dir.exists(), "no output on rejected arguments");
}

/// R2-CLI-02 MCP drift guard: BOTH predicates must accept every RAW extension
/// exported by the single-source `lumina_raw::RAW_EXTENSIONS` — batch
/// collection and decode routing previously disagreed via a private 9-extension
/// copy that silently skipped RAF/ORF/etc. in `lumina_batch`.
#[test]
fn batch_collection_and_decode_routing_agree_on_every_raw_extension() {
    use lumina_mcp::util::{has_batch_image_extension, is_raw_path};

    for extension in lumina_raw::RAW_EXTENSIONS {
        let path_string = format!("photo.{extension}");
        let path = Path::new(&path_string);
        assert!(is_raw_path(path), "`is_raw_path` must accept `{extension}`");
        assert!(
            has_batch_image_extension(path),
            "`has_batch_image_extension` (batch collection) must accept `{extension}`"
        );
    }
    // Non-image names stay out of both paths.
    for foreign in ["notes.txt", "archive.zip", "x.lumina.json", "noext"] {
        assert!(!has_batch_image_extension(Path::new(foreign)), "{foreign}");
        assert!(!is_raw_path(Path::new(foreign)), "{foreign}");
    }
}

/// R2-CLI-02 end-to-end: synthetic (non-decodable) files for ALL RAW formats
/// must be COLLECTED by `lumina_batch` — every one attempted exactly once and,
/// on decode failure, reported loudly per item. The decodable PNG still
/// renders; the run itself reports `failed` instead of silently dropping the
/// skipped formats.
#[test]
fn batch_collection_finds_every_raw_format() {
    let dir = tempfile::tempdir().unwrap();
    let input_dir = dir.path().join("in");
    let output_dir = dir.path().join("out");
    fs::create_dir_all(&input_dir).unwrap();

    // Distinct stems: all targets normalize onto `.png`, so identical stems
    // would trip the up-front collision guard instead of testing collection.
    for (index, extension) in lumina_raw::RAW_EXTENSIONS.iter().enumerate() {
        fs::write(
            input_dir.join(format!("camera_{index:02}.{extension}")),
            b"synthetic non-image payload",
        )
        .unwrap();
    }
    make_png(&input_dir.join("good.png"), 4, 4, 0);

    let mut server = new_server(&dir.path().join("previews"));
    let report = tool_ok(
        &mut server,
        "lumina_batch",
        json!({
            "input": input_dir.to_string_lossy(),
            "output": output_dir.to_string_lossy(),
        }),
    );

    let raw_count = lumina_raw::RAW_EXTENSIONS.len();
    let results = report["results"].as_array().unwrap();
    assert_eq!(
        results.len(),
        raw_count + 1,
        "every RAW format plus the PNG must be collected"
    );
    let collected_raw = results
        .iter()
        .filter(|r| r["input"].as_str().unwrap_or_default().contains("camera_"))
        .count();
    assert_eq!(collected_raw, raw_count, "report: {report}");
    // The synthetic payloads cannot decode: loud per-item failures, and the
    // one real image still renders.
    assert_eq!(report["failed"], raw_count);
    assert_eq!(report["succeeded"], 1);
    assert!(output_dir.join("good.png").is_file());
}

// ---------------------------------------------------------------------------
// lumina_reindex
// ---------------------------------------------------------------------------

#[test]
fn reindex_scans_counts_and_reports_invalid_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let tree = dir.path().join("tree");
    let deep = tree.join("deep");
    fs::create_dir_all(&deep).unwrap();
    let a = tree.join("a.png");
    let b = deep.join("b.png");
    make_png(&a, 4, 4, 0);
    make_png(&b, 4, 4, 20);
    let mut server = new_server(&dir.path().join("previews"));

    // Empty tree scans fine.
    let empty = tool_ok(
        &mut server,
        "lumina_reindex",
        json!({ "input": tree.to_string_lossy() }),
    );
    assert_eq!(empty["status"], "ok");
    assert_eq!(empty["sidecars"], 0);

    // Two valid sidecars found recursively (deterministic sorted walk).
    tool_ok(
        &mut server,
        "lumina_import",
        json!({ "path": b.to_string_lossy() }),
    );
    tool_ok(
        &mut server,
        "lumina_import",
        json!({ "path": a.to_string_lossy() }),
    );
    let report = tool_ok(
        &mut server,
        "lumina_reindex",
        json!({ "input": tree.to_string_lossy() }),
    );
    assert_eq!(report["status"], "ok");
    assert_eq!(report["sidecars"], 2);
    assert_eq!(report["invalid"], 0);

    // A corrupt sidecar is reported individually — never ignored silently.
    let corrupt = deep.join("corrupt.lumina.json");
    fs::write(&corrupt, "{ not json").unwrap();
    let report = tool_ok(
        &mut server,
        "lumina_reindex",
        json!({ "input": tree.to_string_lossy() }),
    );
    assert_eq!(report["status"], "invalid-sidecars");
    assert_eq!(report["invalid"], 1);
    let errors = report["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].as_str().unwrap().contains("corrupt.lumina.json"),
        "error lists the offending path: {errors:?}"
    );

    // Not-a-directory keeps the documented error name.
    let (not_dir, _) = tool_error(
        &mut server,
        "lumina_reindex",
        json!({ "input": a.to_string_lossy() }),
    );
    assert_eq!(not_dir, "FileNotFound");
}

// ---------------------------------------------------------------------------
// lumina_dust_removal
// ---------------------------------------------------------------------------

#[test]
fn dust_removal_persists_action_and_verifies_via_render_out() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("photo.png");
    let replacement = dir.path().join("replacement.png");
    let render_out = dir.path().join("verified.png");
    // Source: mid-gray; replacement: SOLID red plate (built directly, since
    // make_png gradients wrap at 255) — every region pixel >= 32768 so ALL
    // four pixels must be replaced in the verification render.
    make_png(&source, 2, 2, 128);
    let red = ImageFrame::new(
        2,
        2,
        vec![
            255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
        ],
    )
    .unwrap();
    fs::write(&replacement, red.encode(ImageFileFormat::Png).unwrap()).unwrap();
    let mut server = new_server(&dir.path().join("previews"));

    tool_ok(
        &mut server,
        "lumina_import",
        json!({ "path": source.to_string_lossy() }),
    );

    let result = tool_ok(
        &mut server,
        "lumina_dust_removal",
        json!({
            "input": source.to_string_lossy(),
            "repair_region": {
                "id": "spot-1",
                "kind": "dust-removal",
                "region_width": 2,
                "region_height": 2,
                "region_values": [65535, 65535, 65535, 65535],
                "replacement_path": replacement.to_string_lossy(),
            },
            "render_out": render_out.to_string_lossy(),
        }),
    );
    assert_eq!(result["ok"], true);
    assert_eq!(result["artifact_id"], "spot-1");
    assert_eq!(result["virtual_copy"], "vc-original");
    let bundle = result["bundle"].as_str().unwrap().to_string();
    assert!(bundle.ends_with(".lumina.zdata"));
    assert!(dir.path().join(&bundle).is_file());

    // Recipe carries exactly one source action with the reported checksum.
    let sidecar = lumina_sidecar::load_sidecar(&sidecar_json(&source)).unwrap();
    let actions = &sidecar.virtual_copies[0].recipe.source_actions;
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].artifact.id, "spot-1");
    assert_eq!(actions[0].artifact.checksum, result["checksum"]);
    // Relative reference only (portability rule).
    assert!(!actions[0].artifact.relative_path.contains('/'));

    // Verification render replaced every masked pixel with the red plate.
    let verified = ImageFrame::decode(&fs::read(&render_out).unwrap()).unwrap();
    assert_eq!((verified.width, verified.height), (2, 2));
    for pixel in verified.pixels.as_chunks::<4>().0 {
        assert_eq!(&pixel[..3], &[255, 0, 0], "region pixel must be replaced");
    }

    // Original untouched.
    let original = ImageFrame::decode(&fs::read(&source).unwrap()).unwrap();
    assert_eq!(&original.pixels.as_chunks::<4>().0[0][..3], &[128, 0, 128]);
}

#[test]
fn dust_removal_error_paths_are_loud_and_leave_no_partial_state() {
    let dir = tempfile::tempdir().unwrap();
    let orphan = dir.path().join("orphan.png");
    make_png(&orphan, 2, 2, 0);
    let source = dir.path().join("photo.png");
    let replacement = dir.path().join("replacement.png");
    make_png(&source, 2, 2, 0);
    make_png(&replacement, 2, 2, 255);
    let mut server = new_server(&dir.path().join("previews"));

    tool_ok(
        &mut server,
        "lumina_import",
        json!({ "path": source.to_string_lossy() }),
    );

    // Missing sidecar points at lumina_import instead of failing cryptically.
    let (name, message) = tool_error(
        &mut server,
        "lumina_dust_removal",
        json!({
            "input": orphan.to_string_lossy(),
            "repair_region": {
                "id": "s",
                "region_width": 2,
                "region_height": 2,
                "region_values": [65535, 65535, 65535, 65535],
                "replacement_path": replacement.to_string_lossy(),
            },
        }),
    );
    assert_eq!(name, "SidecarError");
    assert!(message.contains("run lumina_import first"));

    // Region/source dimension mismatch fails BEFORE the bundle is touched:
    // no zdata file may appear (REVIEW-CLI-N2 ordering parity).
    let zdata = lumina_sidecar::zdata_path_for(&source);
    let (dims, _) = tool_error(
        &mut server,
        "lumina_dust_removal",
        json!({
            "input": source.to_string_lossy(),
            "repair_region": {
                "id": "s",
                "region_width": 3,
                "region_height": 3,
                "region_values": [65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535],
                "replacement_path": replacement.to_string_lossy(),
            },
        }),
    );
    assert_eq!(dims, "InvalidParams");
    assert!(!zdata.exists(), "rejected call must leave no bundle bytes");

    // Unknown virtual copy.
    let (unknown_copy, _) = tool_error(
        &mut server,
        "lumina_dust_removal",
        json!({
            "input": source.to_string_lossy(),
            "virtual_copy": "does-not-exist",
            "repair_region": {
                "id": "s",
                "region_width": 2,
                "region_height": 2,
                "region_values": [65535, 65535, 65535, 65535],
                "replacement_path": replacement.to_string_lossy(),
            },
        }),
    );
    assert_eq!(unknown_copy, "UnknownCopy");

    // render_out refusing to overwrite the source (non-destructive guard).
    let (protected, message) = tool_error(
        &mut server,
        "lumina_dust_removal",
        json!({
            "input": source.to_string_lossy(),
            "repair_region": {
                "id": "s",
                "region_width": 2,
                "region_height": 2,
                "region_values": [65535, 65535, 65535, 65535],
                "replacement_path": replacement.to_string_lossy(),
            },
            "render_out": source.to_string_lossy(),
        }),
    );
    assert_eq!(protected, "EncodeError");
    assert!(message.contains("refusing"), "{message}");
    assert!(!zdata.exists(), "guard fires before any mutation");
}

// ---------------------------------------------------------------------------

fn count_files_with_extension(root: &Path, extension: &str) -> usize {
    let mut count = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.to_string_lossy().ends_with(extension) {
                count += 1;
            }
        }
    }
    count
}
