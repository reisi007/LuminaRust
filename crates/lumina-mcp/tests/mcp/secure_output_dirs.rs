use super::*;
use std::os::unix::fs::symlink;

#[test]
fn batch_and_preview_reject_symlink_output_directories_and_accept_normal_ones() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input");
    fs::create_dir(&input).unwrap();
    let source = input.join("photo.png");
    make_png(&source, 8, 8);

    let attacker = dir.path().join("attacker");
    fs::create_dir(&attacker).unwrap();
    let marker = attacker.join("marker");
    fs::write(&marker, b"unchanged").unwrap();
    let batch_link = dir.path().join("batch-link");
    symlink(&attacker, &batch_link).unwrap();
    let mut server = new_server(&dir.path().join("unused-preview"));
    let result = call_tool(
        &mut server,
        "lumina_batch",
        json!({"input": input.to_string_lossy(), "output": batch_link.to_string_lossy()}),
    );
    assert_eq!(result["result"]["isError"], true);
    assert_eq!(fs::read(&marker).unwrap(), b"unchanged");
    assert_eq!(fs::read_dir(&attacker).unwrap().count(), 1);

    let loaded = tool_ok(
        &mut server,
        "lumina_load",
        json!({"path": source.to_string_lossy()}),
    );
    let image_id = loaded["image_id"].as_str().unwrap();
    let preview_link = dir.path().join("preview-link");
    symlink(&attacker, &preview_link).unwrap();
    server.preview_dir = preview_link;
    let result = call_tool(&mut server, "lumina_preview", json!({"image_id": image_id}));
    assert_eq!(result["result"]["isError"], true);
    assert_eq!(fs::read(&marker).unwrap(), b"unchanged");
    assert_eq!(fs::read_dir(&attacker).unwrap().count(), 1);

    let batch_dir = dir.path().join("batch-output");
    let batch = tool_ok(
        &mut server,
        "lumina_batch",
        json!({"input": input.to_string_lossy(), "output": batch_dir.to_string_lossy()}),
    );
    assert_eq!(batch["status"], "ok");
    assert_eq!(
        batch["results"][0]["output"],
        batch_dir.join("photo.png").to_string_lossy().as_ref()
    );
    assert!(batch_dir.join("photo.png").is_file());

    let preview_dir = dir.path().join("preview-output");
    server.preview_dir = preview_dir.clone();
    let preview = tool_ok(&mut server, "lumina_preview", json!({"image_id": image_id}));
    let expected_preview = preview_dir.join(format!("{image_id}.png"));
    assert_eq!(
        preview["preview_path"],
        expected_preview.to_string_lossy().as_ref()
    );
    assert!(expected_preview.is_file());
}
