use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_sidecar::{load_sidecar, sidecar_path_for};
use std::fs;
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

fn write_png(directory: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    write_png_in(directory.path(), name)
}

fn write_png_in(directory: &std::path::Path, name: &str) -> std::path::PathBuf {
    let path = directory.join(name);
    let frame = ImageFrame::new(1, 1, vec![40, 80, 120, 255]).unwrap();
    fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    path
}

fn import(input: &std::path::Path) {
    let output = cli()
        .args(["import", "--input", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "import failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// G-15 Slice 2: Keywords setzen → Reload (Edit→Datei→Reload), Exit 0, kein
/// Datenverlust, Original byte-identisch.
#[test]
fn keywords_set_persists_and_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);
    let original_bytes = fs::read(&input).unwrap();

    let result = cli()
        .args([
            "keywords",
            "--input",
            input.to_str().unwrap(),
            "--add",
            "portrait",
            "--add",
            "outdoor",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.status.code(), Some(0));
    let payload: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["changed"], true);

    // Reload from disk: values must be restored (DoD §7 E2E chain).
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(document.keywords, vec!["portrait", "outdoor"]);

    // Remove one keyword → reload shows the removal.
    let result = cli()
        .args([
            "keywords",
            "--input",
            input.to_str().unwrap(),
            "--remove",
            "outdoor",
        ])
        .output()
        .unwrap();
    assert!(result.status.success());
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(document.keywords, vec!["portrait"]);

    // Idempotent re-add → unchanged, exit 0, sidecar bytes untouched.
    let before = fs::read(sidecar_path_for(&input)).unwrap();
    let result = cli()
        .args([
            "keywords",
            "--input",
            input.to_str().unwrap(),
            "--add",
            "portrait",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(result.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(payload["changed"], false);
    assert_eq!(fs::read(sidecar_path_for(&input)).unwrap(), before);

    // Invalid keyword (leading whitespace) fails loudly, exit != 0.
    let result = cli()
        .args([
            "keywords",
            "--input",
            input.to_str().unwrap(),
            "--add",
            "  bad",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("rejected"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    // Rejected op left the persisted document untouched.
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(document.keywords, vec!["portrait"]);

    assert_eq!(fs::read(&input).unwrap(), original_bytes);
}

/// G-15 Slice 2: Sammlungs-Mitgliedschaft setzen → Reload, Rename-Semantik.
#[test]
fn collections_membership_persists_and_renames() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);

    let result = cli()
        .args([
            "collections",
            "--input",
            input.to_str().unwrap(),
            "--add-to",
            "c1=Best Of",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(document.collections.len(), 1);
    assert_eq!(document.collections[0].id, "c1");
    assert_eq!(document.collections[0].name, "Best Of");

    // Rename via same id propagates the new display name.
    let result = cli()
        .args([
            "collections",
            "--input",
            input.to_str().unwrap(),
            "--add-to",
            "c1=Favoriten",
        ])
        .output()
        .unwrap();
    assert!(result.status.success());
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(document.collections.len(), 1);
    assert_eq!(document.collections[0].name, "Favoriten");

    // Malformed assignment fails loudly.
    let result = cli()
        .args([
            "collections",
            "--input",
            input.to_str().unwrap(),
            "--add-to",
            "no-separator",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("id=name"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

/// G-15 Slice 2: Stapelvergabe über N Sidecars — 1 defektes Sidecar → Rest ok,
/// Fehler laut (stderr), Exit-Code 3, kein stiller Skip.
#[test]
fn batch_meta_partial_failure_keeps_rest_and_exits_3() {
    let directory = tempfile::tempdir().unwrap();
    let src = directory.path().join("src");
    fs::create_dir_all(&src).unwrap();
    let a = write_png_in(&src, "a.png");
    let b = write_png_in(&src, "b.png");
    let c = write_png_in(&src, "c.png");
    import(&a);
    import(&b);
    import(&c);
    // Break exactly one sidecar.
    fs::write(sidecar_path_for(&b), "{ truncated").unwrap();

    let result = cli()
        .args([
            "batch-meta",
            "--input",
            src.to_str().unwrap(),
            "--op",
            r#"{"op":"add_keyword","keyword":"batch"}"#,
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(3),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("batch-meta"),
        "per-file failure must be loud, stderr: {stderr}"
    );
    let payload: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(payload["changed"], 2);
    assert_eq!(payload["failed"], 1);
    assert_eq!(payload["status"], "partial");

    // Rest ok: both intact sidecars carry the keyword after reload.
    for input in [&a, &c] {
        let document = load_sidecar(&sidecar_path_for(input)).unwrap();
        assert!(document.keywords.contains(&"batch".to_string()));
    }
    // batch-meta requires exactly one of --op/--op-file.
    let result = cli()
        .args(["batch-meta", "--input", src.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("--op"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

/// Writes a portable smart-catalog file and returns its path.
fn write_catalog(
    directory: &std::path::Path,
    name: &str,
    collections: &serde_json::Value,
) -> std::path::PathBuf {
    let path = directory.join(name);
    let catalog = serde_json::json!({
        "format": "lumina-smart-catalog",
        "version": 1,
        "collections": collections,
    });
    fs::write(&path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
    path
}

/// G-15 Slice 2: Smart-Filter-Ergebnis deterministisch + Katalog-Roundtrip.
/// Zwei Läufe liefern byte-identische stdout; nur das passende Bild matcht.
#[test]
fn smart_collections_filter_is_deterministic() {
    let directory = tempfile::tempdir().unwrap();
    let src = directory.path().join("src");
    fs::create_dir_all(&src).unwrap();
    let portrait = write_png_in(&src, "portrait.png");
    let landscape = write_png_in(&src, "landscape.png");
    import(&portrait);
    import(&landscape);
    for (input, keyword, rating) in [(&portrait, "portrait", "4"), (&landscape, "landscape", "1")] {
        assert!(cli()
            .args([
                "keywords",
                "--input",
                input.to_str().unwrap(),
                "--add",
                keyword,
            ])
            .output()
            .unwrap()
            .status
            .success());
        assert!(cli()
            .args([
                "batch-meta",
                "--input",
                input.to_str().unwrap(),
                "--op",
                &format!(r#"{{"op":"set_rating","copy_id":"vc-original","rating":{rating}}}"#),
            ])
            .output()
            .unwrap()
            .status
            .success());
    }

    let catalog = write_catalog(
        directory.path(),
        "catalog.json",
        &serde_json::json!([
            {"version": 1, "id": "s1", "name": "Portraits",
             "rule": {"op": "and", "rules": [
                 {"op": "keyword", "keyword": "portrait"},
                 {"op": "rating_at_least", "rating": 3}]}},
            {"version": 1, "id": "s2", "name": "Rejects",
             "rule": {"op": "flag", "flag": "reject"}}
        ]),
    );
    // Catalog roundtrip: file parses and validates (version 1, known ids).
    let raw = fs::read_to_string(&catalog).unwrap();
    assert!(raw.contains("lumina-smart-catalog"));
    assert!(!raw.contains(directory.path().to_str().unwrap()));

    let run = || {
        cli()
            .args([
                "smart-collections",
                "--input",
                src.to_str().unwrap(),
                "--catalog",
                catalog.to_str().unwrap(),
                "--json",
            ])
            .output()
            .unwrap()
    };
    let first = run();
    let second = run();
    assert!(first.status.success());
    assert!(second.status.success());
    assert_eq!(
        first.stdout, second.stdout,
        "smart evaluation must be deterministic"
    );
    let payload: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["matched_files"], 1);
    let items = payload["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let entry_for = |name: &str| {
        items
            .iter()
            .find(|item| item["sidecar"].as_str().unwrap().contains(name))
            .unwrap()
    };
    assert_eq!(entry_for("portrait")["matches"], serde_json::json!(["s1"]));
    assert_eq!(
        entry_for("landscape")["matches"],
        serde_json::json!(Vec::<String>::new())
    );

    // Invalid catalog version fails loudly, exit != 0, no silent fallback.
    let bad = write_catalog(
        directory.path(),
        "bad.json",
        &serde_json::json!([
            {"version": 99, "id": "x", "name": "X", "rule": {"op": "all"}}
        ]),
    );
    let raw_bad = fs::read_to_string(&bad)
        .unwrap()
        .replace("\"version\": 1,", "\"version\": 2,");
    fs::write(&bad, raw_bad).unwrap();
    let result = cli()
        .args([
            "smart-collections",
            "--input",
            src.to_str().unwrap(),
            "--catalog",
            bad.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("version"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}
