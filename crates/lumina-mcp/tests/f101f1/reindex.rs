//! Shared helpers are inherited from the F-101-F1 integration-test parent.
use super::*;

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
