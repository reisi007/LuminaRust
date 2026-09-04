//! LRPAR-G15-IPTC-S3: CLI-E2E-Tests für `meta inspect` / `meta draft set` /
//! `meta draft clear` / `meta history show|clear`.
//!
//! Jede Story ist an der Kette Edit → Sidecar-Datei → Reload verankert
//! (DoD §1): Mutationen werden per `load_sidecar` aus der Datei
//! zurückgelesen, nicht aus dem Prozessgedächtnis.

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_iptc::{embed_metadata, IptcMetadata};
use lumina_sidecar::{load_sidecar, sidecar_path_for};
use std::fs;
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

fn write_png(directory: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    let path = directory.path().join(name);
    let frame = ImageFrame::new(1, 1, vec![40, 80, 120, 255]).unwrap();
    fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    path
}

fn write_jpeg_with_iptc(
    directory: &tempfile::TempDir,
    name: &str,
    meta: &IptcMetadata,
) -> std::path::PathBuf {
    let path = directory.path().join(name);
    let frame = ImageFrame::new(8, 8, vec![128; 8 * 8 * 4]).unwrap();
    let plain = frame.encode(ImageFileFormat::Jpeg).unwrap();
    fs::write(&path, embed_metadata(&plain, meta).unwrap()).unwrap();
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

fn sidecar_bytes(input: &std::path::Path) -> Vec<u8> {
    fs::read(sidecar_path_for(input)).unwrap()
}

/// S3: set → inspect → clear-Roundtrip mit Reload aus der Datei
/// (Edit→Datei→Reload), Exit 0, Original byte-identisch.
#[test]
fn meta_draft_set_inspect_clear_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);
    let original_bytes = fs::read(&input).unwrap();

    let result = draft_set(
        &input,
        &[
            "title=Startschuss",
            "city=Berlin",
            "date_created=2026-09-04",
            "keywords=Sport",
            "keywords=Outdoor",
        ],
    );
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.status.code(), Some(0));

    // Reload aus der Datei: Entwurf, Keywords und genau ein History-Eintrag.
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(
        document.metadata.draft.get("title").map(String::as_str),
        Some("Startschuss")
    );
    assert_eq!(
        document.metadata.draft.get("city").map(String::as_str),
        Some("Berlin")
    );
    assert_eq!(
        document
            .metadata
            .draft
            .get("date_created")
            .map(String::as_str),
        Some("2026-09-04")
    );
    assert_eq!(document.keywords, vec!["Sport", "Outdoor"]);
    assert_eq!(document.metadata.history.len(), 1);
    let entry = &document.metadata.history[0];
    assert_eq!(entry.rev, 1);
    assert_eq!(entry.origin, "cli");
    assert_eq!(
        entry.changed,
        vec!["city", "date_created", "keywords", "title"]
    );

    // Inspect zeigt das Draft-Overlay, Keywords, Historie-Länge — und meldet
    // für PNG laut "nicht verfügbar" (Exit bleibt 0).
    let result = cli()
        .args(["meta", "inspect", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(result.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["draft"]["title"], "Startschuss");
    assert_eq!(payload["draft"]["city"], "Berlin");
    assert_eq!(payload["embedded_available"], false);
    assert_eq!(payload["keywords"], serde_json::json!(["Sport", "Outdoor"]));
    assert_eq!(payload["history_len"], 1);
    assert_eq!(payload["history_latest_rev"], 1);

    let result = cli()
        .args(["meta", "inspect", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(result.status.success());
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("nicht verfügbar"), "stdout: {stdout}");
    assert!(stdout.contains("title: draft=\"Startschuss\""));

    // Clear eines Feldes: Entwurf schrumpft, Historie wächst (Eintrag 2).
    let result = cli()
        .args([
            "meta",
            "draft",
            "clear",
            input.to_str().unwrap(),
            "--field",
            "title",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(!document.metadata.draft.contains_key("title"));
    assert_eq!(
        document.metadata.draft.get("city").map(String::as_str),
        Some("Berlin")
    );
    assert_eq!(document.metadata.history.len(), 2);
    assert_eq!(document.metadata.history[0].rev, 2);
    assert_eq!(document.metadata.history[0].changed, vec!["title"]);

    assert_eq!(fs::read(&input).unwrap(), original_bytes);
}

/// S3: idempotentes Set schreibt nichts (Sidecar-Bytes identisch, keine
/// neue History-Revision).
#[test]
fn meta_draft_set_idempotent_writes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);

    assert!(draft_set(&input, &["title=Startschuss"]).status.success());
    let before = sidecar_bytes(&input);
    let result = draft_set(&input, &["title=Startschuss"]);
    assert!(result.status.success());
    assert_eq!(sidecar_bytes(&input), before);
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(document.metadata.history.len(), 1);
}

/// S3: `--field a,b` löscht selektiv, `--all` leert den Entwurf, die
/// Historie bleibt in beiden Fällen erhalten (Keywords bleiben bei `--all`).
#[test]
fn meta_draft_clear_field_and_all_keep_history() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);

    assert!(draft_set(
        &input,
        &["title=T", "city=C", "description=D", "keywords=Sport",]
    )
    .status
    .success());

    let result = cli()
        .args([
            "meta",
            "draft",
            "clear",
            input.to_str().unwrap(),
            "--field",
            "title,city",
        ])
        .output()
        .unwrap();
    assert!(result.status.success());
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(document.metadata.draft.len(), 1);
    assert!(document.metadata.draft.contains_key("description"));
    assert_eq!(document.metadata.history.len(), 2);
    assert_eq!(document.metadata.history[0].changed, vec!["city", "title"]);

    let result = cli()
        .args(["meta", "draft", "clear", input.to_str().unwrap(), "--all"])
        .output()
        .unwrap();
    assert!(result.status.success());
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(document.metadata.draft.is_empty());
    // `--all` leert nur den Entwurf: Keywords und Historie bleiben.
    assert_eq!(document.keywords, vec!["Sport"]);
    assert_eq!(document.metadata.history.len(), 3);
    assert_eq!(document.metadata.history[0].rev, 3);

    // Keywords lassen sich gezielt leeren.
    let result = cli()
        .args([
            "meta",
            "draft",
            "clear",
            input.to_str().unwrap(),
            "--field",
            "keywords",
        ])
        .output()
        .unwrap();
    assert!(result.status.success());
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(document.keywords.is_empty());
    assert_eq!(document.metadata.history[0].changed, vec!["keywords"]);
}

/// S3: unbekannte ID / ungültiger Wert / fehlendes `=` brechen laut ab —
/// all-or-nothing: auch gültige Felder desselben Aufrufs werden nicht
/// geschrieben (Sidecar-Bytes identisch).
#[test]
fn meta_draft_set_rejects_loudly_all_or_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);

    for fields in [
        vec!["nope=Wert"],
        vec!["title=Ok", "nope=Wert"],
        vec![&format!("title={}", "x".repeat(257))],
        vec!["title=Ok", &format!("city={}", "y".repeat(129))],
        vec!["date_created=04.09.2026"],
        vec!["title=Ok", "date_created=gestern"],
        vec!["title-ohne-gleich"],
    ] {
        let before = sidecar_bytes(&input);
        let result = draft_set(&input, &fields);
        assert!(!result.status.success(), "fields {fields:?} must fail");
        assert_ne!(result.status.code(), Some(0));
        assert_eq!(
            sidecar_bytes(&input),
            before,
            "rejected call must write nothing (fields {fields:?})"
        );
        assert!(
            !String::from_utf8_lossy(&result.stderr).is_empty(),
            "failure must be loud on stderr"
        );
    }
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(document.metadata.draft.is_empty());
    assert!(document.metadata.history.is_empty());
}

/// S3: fehlendes Sidecar = lauter Fehler (Exit ≠ 0) für alle fünf Befehle.
#[test]
fn meta_without_sidecar_fails_loudly() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    // Kein `import`: kein Sidecar.

    let cases: Vec<Vec<&str>> = vec![
        vec!["meta", "inspect", input.to_str().unwrap()],
        vec![
            "meta",
            "draft",
            "set",
            input.to_str().unwrap(),
            "--field",
            "title=T",
        ],
        vec!["meta", "draft", "clear", input.to_str().unwrap(), "--all"],
        vec!["meta", "history", "show", input.to_str().unwrap()],
        vec!["meta", "history", "clear", input.to_str().unwrap()],
    ];
    for args in cases {
        let result = cli().args(&args).output().unwrap();
        assert!(
            !result.status.success(),
            "args {args:?} must fail without a sidecar"
        );
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(
            stderr.contains("sidecar") || stderr.contains("import"),
            "args {args:?} must name the missing sidecar, stderr: {stderr}"
        );
    }
    assert!(!sidecar_path_for(&input).exists());
}

/// S3: `draft clear` verlangt genau einen von `--field` / `--all`.
#[test]
fn meta_draft_clear_requires_exactly_one_selector() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);

    let neither = cli()
        .args(["meta", "draft", "clear", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!neither.status.success());

    let both = cli()
        .args([
            "meta",
            "draft",
            "clear",
            input.to_str().unwrap(),
            "--field",
            "title",
            "--all",
        ])
        .output()
        .unwrap();
    assert!(!both.status.success());
}

/// S3: History anzeigen (neueste zuerst, `--limit`) und ausdrücklich
/// leeren — der Entwurf bleibt dabei erhalten.
#[test]
fn meta_history_show_limit_and_clear() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);

    assert!(draft_set(&input, &["title=Eins"]).status.success());
    assert!(draft_set(&input, &["city=Zwei"]).status.success());

    let result = cli()
        .args(["meta", "history", "show", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(result.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(payload["total"], 2);
    assert_eq!(payload["shown"], 2);
    assert_eq!(payload["history"][0]["rev"], 2);
    assert_eq!(payload["history"][1]["rev"], 1);
    assert_eq!(payload["history"][0]["origin"], "cli");

    let result = cli()
        .args([
            "meta",
            "history",
            "show",
            input.to_str().unwrap(),
            "--limit",
            "1",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(result.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(payload["shown"], 1);
    assert_eq!(payload["total"], 2);
    assert_eq!(payload["history"][0]["rev"], 2);

    let result = cli()
        .args(["meta", "history", "clear", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(result.status.success());
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert!(document.metadata.history.is_empty());
    // Entwurf bleibt erhalten.
    assert_eq!(
        document.metadata.draft.get("title").map(String::as_str),
        Some("Eins")
    );
    assert_eq!(
        document.metadata.draft.get("city").map(String::as_str),
        Some("Zwei")
    );

    // Erneutes Leeren: idempotent, Exit 0.
    let result = cli()
        .args(["meta", "history", "clear", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(result.status.success());
}

/// S3: JPEG mit eingebetteter IPTC — inspect zeigt Embedded-Werte, das
/// Draft-Overlay stellt Draft gegen Embedded. Original byte-identisch.
#[test]
fn meta_inspect_jpeg_shows_embedded_overlay() {
    let directory = tempfile::tempdir().unwrap();
    let embedded = IptcMetadata {
        title: Some("Eingebettet".into()),
        city: Some("Hamburg".into()),
        keywords: vec!["JPEG".into()],
        ..IptcMetadata::default()
    };
    let input = write_jpeg_with_iptc(&directory, "photo.jpg", &embedded);
    import(&input);
    let original_bytes = fs::read(&input).unwrap();

    let result = cli()
        .args(["meta", "inspect", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let payload: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(payload["embedded_available"], true);
    assert_eq!(payload["embedded"]["title"], "Eingebettet");
    assert_eq!(payload["embedded"]["city"], "Hamburg");
    assert_eq!(payload["keywords_embedded"], serde_json::json!(["JPEG"]));
    assert!(payload["draft"].as_object().unwrap().is_empty());

    // Draft danebenlegen: beide Werte sichtbar, Draft gewinnt nicht still —
    // inspect zeigt beide Spalten.
    assert!(draft_set(&input, &["title=Entwurf"]).status.success());
    let result = cli()
        .args(["meta", "inspect", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(result.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(payload["draft"]["title"], "Entwurf");
    assert_eq!(payload["embedded"]["title"], "Eingebettet");

    let result = cli()
        .args(["meta", "inspect", input.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("embedded: verfügbar"), "stdout: {stdout}");
    assert!(
        stdout.contains("title: draft=\"Entwurf\" embedded=\"Eingebettet\""),
        "stdout: {stdout}"
    );

    assert_eq!(fs::read(&input).unwrap(), original_bytes);
}

/// S3 (B2a): `draft clear --field <unbekannte-ID>` bricht laut ab —
/// all-or-nothing: auch bekannte Felder desselben Aufrufs bleiben gesetzt,
/// die Historie wächst nicht.
#[test]
fn meta_draft_clear_unknown_field_writes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);
    assert!(draft_set(&input, &["title=Bleibt", "city=Bleibt"])
        .status
        .success());
    let before = sidecar_bytes(&input);
    let history_len = load_sidecar(&sidecar_path_for(&input))
        .unwrap()
        .metadata
        .history
        .len();

    for fields in [vec!["nope"], vec!["title", "nope"]] {
        let mut command = cli();
        command.args(["meta", "draft", "clear", input.to_str().unwrap()]);
        for field in &fields {
            command.args(["--field", field]);
        }
        let result = command.output().unwrap();
        assert!(!result.status.success(), "fields {fields:?} must fail");
        assert_ne!(result.status.code(), Some(0));
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(
            stderr.contains("unknown metadata field"),
            "failure must name the unknown ID, stderr: {stderr}"
        );
        assert_eq!(
            sidecar_bytes(&input),
            before,
            "rejected clear must write nothing (fields {fields:?})"
        );
    }

    // Reload aus der Datei: Entwurf und Historie unverändert.
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(
        document.metadata.draft.get("title").map(String::as_str),
        Some("Bleibt")
    );
    assert_eq!(document.metadata.history.len(), history_len);
}

/// S3 (B2b): History-Cap-100 auf CLI-Ebene — 105 Draft-Sets via CLI sind
/// über `history show` sichtbar als genau 100 Einträge mit streng monoton
/// fallenden Revisionen (neueste zuerst: rev 105 … 6).
#[test]
fn meta_history_cap_100_visible_via_cli() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_png(&directory, "input.png");
    import(&input);

    for i in 1..=105 {
        let result = draft_set(&input, &[&format!("title=Wert{i}")]);
        assert!(
            result.status.success(),
            "set {i} failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    let result = cli()
        .args(["meta", "history", "show", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(result.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(payload["total"], 100);
    assert_eq!(payload["shown"], 100);
    let history = payload["history"].as_array().unwrap();
    assert_eq!(history[0]["rev"], 105);
    assert_eq!(history[99]["rev"], 6);
    let mut previous = u64::MAX;
    for entry in history {
        let rev = entry["rev"].as_u64().unwrap();
        assert!(rev < previous, "revs must decrease strictly (newest first)");
        assert_eq!(entry["origin"], "cli");
        previous = rev;
    }

    // Reload aus der Datei: Cap + neuester Entwurf bestätigt.
    let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
    assert_eq!(document.metadata.history.len(), 100);
    assert_eq!(document.metadata.latest_rev(), 105);
    assert_eq!(
        document.metadata.draft.get("title").map(String::as_str),
        Some("Wert105")
    );
}
