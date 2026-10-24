use super::*;

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
