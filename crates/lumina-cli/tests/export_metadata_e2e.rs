//! LRPAR-G15-IPTC-S6: CLI-E2E-Tests für den Opt-in-Export-Bake-In
//! (`--write-metadata` an `export`/`process`/`batch`).
//!
//! Jede Story ist an Datei-Artefakten verankert (DoD §1): Tags werden aus der
//! exportierten JPEG-Datei zurückgelesen (`lumina-iptc`), Pixel per Decode
//! verglichen, `ExportRecord`-Inhalte per `load_sidecar` aus der Datei
//! geprüft, Exit-Codes per Prozessstatus. Das Original bleibt überall
//! byte-identisch.

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_iptc::extract_metadata;
use lumina_sidecar::{load_sidecar, sidecar_path_for};
use std::fs;
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

fn write_png(directory: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    let path = directory.path().join(name);
    // 8x8 statt 1x1: JPEG-Encode braucht echte Bildfläche.
    let frame = ImageFrame::new(8, 8, vec![140; 8 * 8 * 4]).unwrap();
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

fn draft_set(input: &std::path::Path, fields: &[&str]) -> std::process::Output {
    let mut command = cli();
    command.args(["meta", "draft", "set", input.to_str().unwrap()]);
    for field in fields {
        command.args(["--field", field]);
    }
    command.output().unwrap()
}

fn export(
    input: &std::path::Path,
    output: &std::path::Path,
    format: &str,
    write_metadata: bool,
) -> std::process::Output {
    let mut command = cli();
    command.args([
        "export",
        "--input",
        input.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--format",
        format,
    ]);
    if write_metadata {
        command.arg("--write-metadata");
    }
    command.output().unwrap()
}

fn metadata_written_of(input: &std::path::Path) -> serde_json::Value {
    let document = load_sidecar(&sidecar_path_for(input)).unwrap();
    let copy = &document.virtual_copies[0];
    let record = copy
        .export_records
        .last()
        .expect("expected an export record");
    assert_eq!(record.format, "jpg");
    record.extras["metadata_written"].clone()
}

/// S6-Golden: JPEG-Bake-In schreibt re-lesbare Tags, Pixel bleiben identisch
/// zum Export ohne Option, Determinismus (zwei Läufe byte-identisch),
/// `ExportRecord`-Inhalt, Exit 0, Original byte-identisch.
#[test]
fn export_jpeg_bake_in_roundtrip_pixels_identical_and_deterministic() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);
    let original_bytes = fs::read(&input).unwrap();

    let result = draft_set(
        &input,
        &[
            "title=Startschuss",
            "creator=Fotografin",
            "city=Berlin",
            "date_created=2026-09-04",
            "keywords=Sport",
            "keywords=Outdoor",
        ],
    );
    assert!(
        result.status.success(),
        "draft set failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let with_meta = directory.path().join("with-meta.jpg");
    let result = export(&input, &with_meta, "jpg", true);
    assert!(
        result.status.success(),
        "export failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.status.code(), Some(0));

    // Tags sind aus der Datei re-lesbar (IIM+XMP-Merge).
    let back = extract_metadata(&fs::read(&with_meta).unwrap()).unwrap();
    assert_eq!(back.title.as_deref(), Some("Startschuss"));
    assert_eq!(back.creator.as_deref(), Some("Fotografin"));
    assert_eq!(back.city.as_deref(), Some("Berlin"));
    assert_eq!(back.date_created.as_deref(), Some("2026-09-04"));
    assert_eq!(back.keywords, vec!["Sport", "Outdoor"]);

    // Pixelbytes identisch zum Export ohne Option (Splice ist Post-Encode).
    let plain = directory.path().join("plain.jpg");
    let result = export(&input, &plain, "jpg", false);
    assert!(result.status.success());
    let pixels_meta = ImageFrame::decode(&fs::read(&with_meta).unwrap())
        .unwrap()
        .pixels;
    let pixels_plain = ImageFrame::decode(&fs::read(&plain).unwrap())
        .unwrap()
        .pixels;
    assert_eq!(pixels_meta, pixels_plain);

    // Determinismus: gleiches Rezept + gleiche Drafts → byte-identische Datei.
    let repeat = directory.path().join("repeat.jpg");
    let result = export(&input, &repeat, "jpg", true);
    assert!(result.status.success());
    assert_eq!(fs::read(&with_meta).unwrap(), fs::read(&repeat).unwrap());

    // ExportRecord-Inhalt: additiv-optional `{iim, xmp, status: written}`.
    assert_eq!(
        metadata_written_of(&input),
        serde_json::json!({"iim": true, "xmp": true, "status": "written"})
    );

    // Original byte-identisch.
    assert_eq!(fs::read(&input).unwrap(), original_bytes);
}

/// S6: Ohne Opt-in exakt heutiges Verhalten — keine Metadaten, kein Record.
#[test]
fn export_without_flag_writes_no_metadata_and_no_record() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);
    let result = draft_set(&input, &["title=Startschuss", "keywords=Sport"]);
    assert!(result.status.success());

    let output = directory.path().join("out.jpg");
    let result = export(&input, &output, "jpg", false);
    assert!(result.status.success());

    let back = extract_metadata(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(back, lumina_iptc::IptcMetadata::default());
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(document.virtual_copies[0].export_records.is_empty());
}

/// S6: PNG mit Option = lauter Fehler (Exit ≠ 0, keine Ausgabedatei).
#[test]
fn export_png_with_flag_fails_loudly() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);
    let original_bytes = fs::read(&input).unwrap();
    assert!(draft_set(&input, &["title=Startschuss"]).status.success());

    let output = directory.path().join("out.png");
    let result = export(&input, &output, "png", true);
    assert!(!result.status.success());
    assert_ne!(result.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("--write-metadata"),
        "lauter Grund erwartet, stderr: {stderr}"
    );
    assert!(
        stderr.contains("JPEG"),
        "JPEG-only-Hinweis erwartet, stderr: {stderr}"
    );
    assert!(!output.exists());
    assert_eq!(fs::read(&input).unwrap(), original_bytes);
}

/// S6: WebP mit Option = lauter Fehler (Exit ≠ 0).
#[test]
fn export_webp_with_flag_fails_loudly() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);
    assert!(draft_set(&input, &["title=Startschuss"]).status.success());

    let output = directory.path().join("out.webp");
    let result = export(&input, &output, "webp", true);
    assert!(!result.status.success());
    assert_ne!(result.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("--write-metadata"),
        "lauter Grund erwartet, stderr: {stderr}"
    );
    assert!(!output.exists());
}

/// S6: Sidecar-gültiger Wert über dem IIM-Oktettlimit = lauter per-Datei-Fehler
/// (Feld + Limit genannt, kein Still-Kürzen, keine Ausgabedatei).
#[test]
fn export_iim_octet_limit_fails_loudly_without_truncation() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);
    // 100 Zeichen (Sidecar-Limit 128 ok), aber 400 Oktette (IIM-Limit 128).
    let long_city = "🎉".repeat(100);
    let result = draft_set(&input, &[&format!("city={long_city}")]);
    assert!(
        result.status.success(),
        "Sidecar-gültiger Wert muss annehmbar sein: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let output = directory.path().join("out.jpg");
    let result = export(&input, &output, "jpg", true);
    assert!(!result.status.success());
    assert_ne!(result.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("city"),
        "Feldname erwartet, stderr: {stderr}"
    );
    assert!(stderr.contains("128"), "Limit erwartet, stderr: {stderr}");
    assert!(!output.exists());
}

/// S6: Komplett leerer Entwurf + keine Keywords → Export erfolgt mit lauter
/// Warnung und `metadata_written: "empty"` (kein stiller No-Op).
#[test]
fn export_empty_draft_warns_and_records_empty() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);

    let output = directory.path().join("out.jpg");
    let result = export(&input, &output, "jpg", true);
    assert!(
        result.status.success(),
        "leerer Entwurf darf den Export nicht blockieren: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("metadata_written: empty"),
        "laute Warnung erwartet, stderr: {stderr}"
    );
    let back = extract_metadata(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(back, lumina_iptc::IptcMetadata::default());
    assert_eq!(
        metadata_written_of(&input),
        serde_json::json!({"iim": false, "xmp": false, "status": "empty"})
    );
}

/// S6: Ziel-Guard gegen Quelle (mit Option): laut, Original byte-identisch.
/// (`process` schreibt die Extension nicht um, daher läuft der Guard hier
/// direkt gegen den übergebenen Pfad.)
#[test]
fn export_guard_rejects_source_as_target_with_flag() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);
    let original_bytes = fs::read(&input).unwrap();
    assert!(draft_set(&input, &["title=Startschuss"]).status.success());

    let result = cli()
        .args([
            "process",
            "--input",
            input.to_str().unwrap(),
            "--output",
            input.to_str().unwrap(),
            "--write-metadata",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_ne!(result.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("refusing"),
        "lauter Guard erwartet, stderr: {stderr}"
    );
    assert_eq!(fs::read(&input).unwrap(), original_bytes);
}

/// S6: Ziel-Guard gegen das Sidecar-Bundle (mit Option): laut, Bundle
/// unverändert.
#[test]
fn export_guard_rejects_sidecar_bundle_as_target_with_flag() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);
    let sidecar = sidecar_path_for(&input);
    let sidecar_before = fs::read(&sidecar).unwrap();

    let result = cli()
        .args([
            "process",
            "--input",
            input.to_str().unwrap(),
            "--output",
            sidecar.to_str().unwrap(),
            "--write-metadata",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_ne!(result.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("refusing"),
        "lauter Guard erwartet, stderr: {stderr}"
    );
    assert_eq!(fs::read(&sidecar).unwrap(), sidecar_before);
}

/// S6: `process --write-metadata` schreibt Tags + Record (Exit 0).
#[test]
fn process_writes_metadata_with_flag() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);
    assert!(draft_set(&input, &["title=Prozess", "keywords=Labor"])
        .status
        .success());

    let output = directory.path().join("out.jpg");
    let result = cli()
        .args([
            "process",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--write-metadata",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "process failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let back = extract_metadata(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(back.title.as_deref(), Some("Prozess"));
    assert_eq!(back.keywords, vec!["Labor"]);
    assert_eq!(
        metadata_written_of(&input),
        serde_json::json!({"iim": true, "xmp": true, "status": "written"})
    );
}

/// S6: Batch-JPEG mit Flag schreibt Tags pro Datei; Batch-PNG mit Flag meldet
/// pro Item `failed` mit Grund (Exit 3).
#[test]
fn batch_jpeg_bake_in_and_png_item_failed() {
    let directory = tempfile::tempdir().unwrap();
    let src = directory.path().join("src");
    fs::create_dir_all(&src).unwrap();
    // Zwei Eingaben mit unterschiedlichen Namen (keine Batch-Kollision).
    for (name, title) in [("a.png", "Bild A"), ("b.png", "Bild B")] {
        let frame = ImageFrame::new(8, 8, vec![100; 8 * 8 * 4]).unwrap();
        let path = src.join(name);
        fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
        import(&path);
        assert!(draft_set(&path, &[&format!("title={title}")])
            .status
            .success());
    }

    // JPEG-Batch mit Flag: Exit 0, Tags pro Datei, Record pro Sidecar.
    let out_jpg = directory.path().join("out-jpg");
    let result = cli()
        .args([
            "batch",
            "--input",
            src.to_str().unwrap(),
            "--output",
            out_jpg.to_str().unwrap(),
            "--format",
            "jpg",
            "--write-metadata",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "batch jpg failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    for (name, title) in [("a.jpg", "Bild A"), ("b.jpg", "Bild B")] {
        let bytes = fs::read(out_jpg.join(name)).unwrap();
        let back = extract_metadata(&bytes).unwrap();
        assert_eq!(back.title.as_deref(), Some(title));
    }
    for name in ["a.png", "b.png"] {
        assert_eq!(
            metadata_written_of(&src.join(name)),
            serde_json::json!({"iim": true, "xmp": true, "status": "written"})
        );
    }

    // PNG-Batch mit Flag: jedes Item laut `failed` mit Grund, Exit 3.
    let out_png = directory.path().join("out-png");
    let result = cli()
        .args([
            "batch",
            "--input",
            src.to_str().unwrap(),
            "--output",
            out_png.to_str().unwrap(),
            "--format",
            "png",
            "--write-metadata",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&result.stdout);
    let items: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap();
    assert_eq!(items.len(), 2);
    for item in &items {
        assert_eq!(item["status"], "failed");
        let reason = item["error"].as_str().unwrap_or("");
        assert!(
            reason.contains("--write-metadata"),
            "Grund mit Flag erwartet: {reason}"
        );
    }
}

/// S6: Batch-IIM-Limit schlägt pro Datei laut fehl (Exit 3, Grund nennt Feld).
#[test]
fn batch_iim_octet_limit_fails_item_loudly() {
    let directory = tempfile::tempdir().unwrap();
    let src = directory.path().join("src");
    fs::create_dir_all(&src).unwrap();
    let path = src.join("a.png");
    let frame = ImageFrame::new(8, 8, vec![100; 8 * 8 * 4]).unwrap();
    fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    import(&path);
    let long_city = "🎉".repeat(100);
    assert!(draft_set(&path, &[&format!("city={long_city}")])
        .status
        .success());

    let out = directory.path().join("out");
    let result = cli()
        .args([
            "batch",
            "--input",
            src.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
            "--format",
            "jpg",
            "--write-metadata",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(3));
    let stdout = String::from_utf8_lossy(&result.stdout);
    let items: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["status"], "failed");
    let reason = items[0]["error"].as_str().unwrap_or("");
    assert!(reason.contains("city"), "Feldname erwartet: {reason}");
}
