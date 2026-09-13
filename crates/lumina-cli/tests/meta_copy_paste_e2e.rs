//! META-COPYPASTE-1: CLI-E2E-Tests für `meta copy` / `meta paste`
//! (explizite Clipboard-Datei, additiver Paste über den Commit-Pfad).
//!
//! Jede Story ist an der Kette Datei → Sidecar → Reload verankert (DoD §1):
//! Mutationen werden per `load_sidecar` aus der Datei zurückgelesen, nicht aus
//! dem Prozessgedächtnis. Originale bleiben byte-identisch, Exit-Codes sind
//! laut (0 ok / 1 Fehler / 3 partiell, konsistent zu S3–S5).

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_sidecar::{load_sidecar, sidecar_path_for};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

fn write_png(directory: &tempfile::TempDir, name: &str) -> PathBuf {
    let path = directory.path().join(name);
    let frame = ImageFrame::new(1, 1, vec![40, 80, 120, 255]).unwrap();
    fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    path
}

fn import(input: &Path) {
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

fn draft_set(input: &Path, assignments: &[&str]) {
    let mut command = cli();
    command.args(["meta", "draft", "set", input.to_str().unwrap()]);
    for assignment in assignments {
        command.args(["--field", assignment]);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "draft set failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn sidecar_bytes(input: &Path) -> Vec<u8> {
    fs::read(sidecar_path_for(input)).unwrap()
}

fn copy(source: &Path, out: &Path, fields: Option<&str>) -> Output {
    let mut command = cli();
    command.args(["meta", "copy", source.to_str().unwrap()]);
    if let Some(ids) = fields {
        command.args(["--fields", ids]);
    }
    command.args(["--out", out.to_str().unwrap(), "--json"]);
    command.output().unwrap()
}

fn paste(clipboard: &Path, targets: &[&Path], fields: Option<&str>) -> Output {
    let mut command = cli();
    command.args(["meta", "paste", clipboard.to_str().unwrap()]);
    for target in targets {
        command.args(["--target", target.to_str().unwrap()]);
    }
    if let Some(ids) = fields {
        command.args(["--fields", ids]);
    }
    command.args(["--json"]);
    command.output().unwrap()
}

/// Copy→Paste-Roundtrip über eine explizite Clipboard-Datei: Datei → Sidecar →
/// Reload. Der Paste schreibt nur Clipboard-Felder, lässt andere Zielfelder
/// stehen, ersetzt Keywords als Ganzes, nutzt `origin = "cli"` und ist beim
/// zweiten Lauf idempotent (`unchanged`, keine neuen Bytes). Copy selbst
/// mutiert kein Sidecar.
#[test]
fn meta_copy_paste_roundtrip_over_explicit_file() {
    let directory = tempfile::tempdir().unwrap();
    let source = write_png(&directory, "quelle.png");
    let target = write_png(&directory, "ziel.png");
    let clipboard = directory.path().join("clipboard.json");
    import(&source);
    import(&target);
    draft_set(
        &source,
        &[
            "title=Startschuss",
            "city=Berlin",
            "creator=Fotograf",
            "keywords=fest",
            "keywords=abend",
        ],
    );
    draft_set(&target, &["headline=Behalten", "creator=Alt"]);

    let source_sidecar_before = sidecar_bytes(&source);
    let recipe_before = load_sidecar(&sidecar_path_for(&target))
        .unwrap()
        .virtual_copies
        .clone();
    let original = fs::read(&target).unwrap();

    // Copy: schreibt die Datei, mutiert aber kein Sidecar.
    let copied = copy(&source, &clipboard, None);
    assert!(
        copied.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&copied.stderr)
    );
    assert_eq!(copied.status.code(), Some(0));
    assert_eq!(sidecar_bytes(&source), source_sidecar_before);

    let file: serde_json::Value = serde_json::from_slice(&fs::read(&clipboard).unwrap()).unwrap();
    assert_eq!(file["format"], "lumina-meta-clipboard");
    assert_eq!(file["version"], 1);
    assert_eq!(file["source"], "quelle.png");
    assert_eq!(file["fields"]["title"], "Startschuss");
    assert_eq!(file["fields"]["city"], "Berlin");
    assert_eq!(
        file["keywords"],
        serde_json::json!(["fest", "abend"]),
        "`meta copy` without --fields carries non-empty keywords"
    );

    // Paste: nur Clipboard-Felder, `headline` der Ziele bleibt stehen.
    let pasted = paste(&clipboard, &[&target], None);
    assert!(
        pasted.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&pasted.stderr)
    );
    assert_eq!(pasted.status.code(), Some(0));
    let payload: serde_json::Value = serde_json::from_slice(&pasted.stdout).unwrap();
    assert_eq!(payload["command"], "meta-paste");
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["origin"], "cli");
    assert_eq!(payload["updated"], 1);
    assert_eq!(payload["unchanged"], 0);
    assert_eq!(payload["failed"], 0);

    // Reload aus der Datei (DoD §7 E2E).
    let document = load_sidecar(&sidecar_path_for(&target)).unwrap();
    assert_eq!(
        document.metadata.draft.get("title").map(String::as_str),
        Some("Startschuss")
    );
    assert_eq!(
        document.metadata.draft.get("city").map(String::as_str),
        Some("Berlin")
    );
    assert_eq!(
        document.metadata.draft.get("creator").map(String::as_str),
        Some("Fotograf")
    );
    assert_eq!(
        document.metadata.draft.get("headline").map(String::as_str),
        Some("Behalten"),
        "paste must never delete a non-selected target field"
    );
    assert_eq!(
        document.keywords,
        vec!["fest".to_string(), "abend".to_string()]
    );
    // Setup `draft set` + Paste, neueste zuerst.
    assert_eq!(document.metadata.history.len(), 2);
    assert_eq!(document.metadata.history[0].origin, "cli");
    assert_eq!(document.metadata.history[0].rev, 2);
    assert_eq!(
        document.metadata.history[0].changed,
        vec![
            "city".to_string(),
            "creator".to_string(),
            "keywords".to_string(),
            "title".to_string()
        ]
    );
    // Rezept + Original unberührt.
    assert_eq!(
        load_sidecar(&sidecar_path_for(&target))
            .unwrap()
            .virtual_copies,
        recipe_before
    );
    assert_eq!(fs::read(&target).unwrap(), original);

    // Idempotenz: zweiter Paste = `unchanged`, keine neuen Bytes/Einträge.
    let bytes_before = sidecar_bytes(&target);
    let again = paste(&clipboard, &[&target], None);
    assert!(again.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(payload["updated"], 0);
    assert_eq!(payload["unchanged"], 1);
    assert_eq!(payload["failed"], 0);
    assert_eq!(sidecar_bytes(&target), bytes_before);
    assert_eq!(
        load_sidecar(&sidecar_path_for(&target))
            .unwrap()
            .metadata
            .history
            .len(),
        2
    );
}

/// Feldselektion: Copy mit `--fields title` zeichnet nur `title` auf; der
/// Paste schreibt nur die ausgewählte Teilmenge, alle anderen Zielfelder
/// (inkl. Keywords) bleiben unangetastet. Ein danach gewähltes, nicht im
/// Clipboard enthaltenes Feld (`city`) ist ein lauter Fehler.
#[test]
fn meta_copy_paste_field_selection_never_deletes_others() {
    let directory = tempfile::tempdir().unwrap();
    let source = write_png(&directory, "quelle.png");
    let target = write_png(&directory, "ziel.png");
    let clipboard = directory.path().join("clipboard.json");
    import(&source);
    import(&target);
    draft_set(
        &source,
        &["title=Startschuss", "city=Berlin", "keywords=fest"],
    );
    draft_set(
        &target,
        &["title=Alt", "city=Behalten", "country=DE", "keywords=alt"],
    );

    let copied = copy(&source, &clipboard, Some("title"));
    assert!(
        copied.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&copied.stderr)
    );
    let file: serde_json::Value = serde_json::from_slice(&fs::read(&clipboard).unwrap()).unwrap();
    assert_eq!(file["fields"]["title"], "Startschuss");
    assert!(
        file["fields"].get("city").is_none(),
        "unselected field must not be captured: {file}"
    );
    assert!(
        file.get("keywords").is_none(),
        "unselected keywords must not be captured: {file}"
    );

    let pasted = paste(&clipboard, &[&target], Some("title"));
    assert!(
        pasted.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&pasted.stderr)
    );
    let document = load_sidecar(&sidecar_path_for(&target)).unwrap();
    assert_eq!(
        document.metadata.draft.get("title").map(String::as_str),
        Some("Startschuss")
    );
    assert_eq!(
        document.metadata.draft.get("city").map(String::as_str),
        Some("Behalten"),
        "unselected target field must survive a paste"
    );
    assert_eq!(
        document.metadata.draft.get("country").map(String::as_str),
        Some("DE")
    );
    assert_eq!(
        document.keywords,
        vec!["alt".to_string()],
        "a paste that does not select keywords must leave them alone"
    );
    assert_eq!(
        document.metadata.history[0].changed,
        vec!["title".to_string()]
    );

    // `--fields city` ist nicht im Clipboard → lauter Fehler, nichts geschrieben.
    let bytes_before = sidecar_bytes(&target);
    let rejected = paste(&clipboard, &[&target], Some("city"));
    assert_eq!(
        rejected.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("not present in the clipboard"),
        "stderr: {}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert_eq!(sidecar_bytes(&target), bytes_before);
}

/// Paste ohne (vorhandenes) Clipboard ist ein lauter Fehler (Exit 1), die
/// Ziele bleiben unangetastet.
#[test]
fn meta_paste_without_clipboard_fails_loudly() {
    let directory = tempfile::tempdir().unwrap();
    let target = write_png(&directory, "ziel.png");
    let missing = directory.path().join("kein-clipboard.json");
    import(&target);
    draft_set(&target, &["title=Behalten"]);
    let before = sidecar_bytes(&target);

    let result = paste(&missing, &[&target], None);
    assert_eq!(result.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("kein-clipboard.json"),
        "failure must name the missing clipboard, stderr: {stderr}"
    );
    assert_eq!(sidecar_bytes(&target), before);
    assert_eq!(
        load_sidecar(&sidecar_path_for(&target))
            .unwrap()
            .metadata
            .draft
            .get("title")
            .map(String::as_str),
        Some("Behalten")
    );
}

/// Leeres oder strukturell ungültiges Clipboard = lauter Fehler (Exit 1), nie
/// ein stiller No-Op.
#[test]
fn meta_paste_rejects_empty_and_invalid_clipboard() {
    let directory = tempfile::tempdir().unwrap();
    let target = write_png(&directory, "ziel.png");
    import(&target);
    let before = sidecar_bytes(&target);

    // Leeres, aber korrekt markiertes Clipboard (keine Felder).
    let empty = directory.path().join("empty.json");
    fs::write(
        &empty,
        r#"{"format":"lumina-meta-clipboard","version":1,"source":"quelle.png"}"#,
    )
    .unwrap();
    let result = paste(&empty, &[&target], None);
    assert_eq!(result.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("is empty"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    // Falscher Format-Marker.
    let wrong = directory.path().join("wrong.json");
    fs::write(
        &wrong,
        r#"{"format":"lumina-nope","version":1,"fields":{"title":"X"}}"#,
    )
    .unwrap();
    let result = paste(&wrong, &[&target], None);
    assert_eq!(result.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("expected format"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    // Unbekannte Feld-ID im Clipboard.
    let unknown = directory.path().join("unknown.json");
    fs::write(
        &unknown,
        r#"{"format":"lumina-meta-clipboard","version":1,"fields":{"nope":"X"}}"#,
    )
    .unwrap();
    let result = paste(&unknown, &[&target], None);
    assert_eq!(result.status.code(), Some(1));

    // Leerer Wert würde als Löschen gedeutet — wird laut abgelehnt.
    let blank = directory.path().join("blank.json");
    fs::write(
        &blank,
        r#"{"format":"lumina-meta-clipboard","version":1,"fields":{"title":""}}"#,
    )
    .unwrap();
    let result = paste(&blank, &[&target], None);
    assert_eq!(result.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("empty"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    assert_eq!(sidecar_bytes(&target), before);
}

/// Copy mit unbekannter Feld-ID ist ein lauter Fehler (Exit 1); die
/// Clipboard-Datei wird nicht angelegt.
#[test]
fn meta_copy_rejects_unknown_field_id() {
    let directory = tempfile::tempdir().unwrap();
    let source = write_png(&directory, "quelle.png");
    let clipboard = directory.path().join("clipboard.json");
    import(&source);
    draft_set(&source, &["title=Startschuss"]);
    let sidecar_before = sidecar_bytes(&source);

    let result = copy(&source, &clipboard, Some("title,nope"));
    assert_eq!(result.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("unknown metadata field `nope`"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        !clipboard.exists(),
        "no clipboard must be written on rejection"
    );
    assert_eq!(sidecar_bytes(&source), sidecar_before);
}

/// Copy einer Quelle ohne nichtleere Metadaten schreibt ein `empty`-Clipboard
/// (Exit 0, laut gemeldet); der anschließende Paste ist laut (Exit 1).
#[test]
fn meta_copy_empty_source_reports_empty_and_paste_is_loud() {
    let directory = tempfile::tempdir().unwrap();
    let source = write_png(&directory, "leer.png");
    let target = write_png(&directory, "ziel.png");
    let clipboard = directory.path().join("clipboard.json");
    import(&source);
    import(&target);

    let copied = copy(&source, &clipboard, None);
    assert_eq!(copied.status.code(), Some(0));
    let payload: serde_json::Value = serde_json::from_slice(&copied.stdout).unwrap();
    assert_eq!(payload["status"], "empty");
    assert!(clipboard.exists());

    let result = paste(&clipboard, &[&target], None);
    assert_eq!(result.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("is empty"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

/// Fehlerisolation: ein Ziel ohne Sidecar markiert nur sein Item `failed`
/// (Exit 3, „zuerst importieren"), das intakte Ziel wird aktualisiert, kein
/// stilles Anlegen.
#[test]
fn meta_paste_isolates_target_failures() {
    let directory = tempfile::tempdir().unwrap();
    let source = write_png(&directory, "quelle.png");
    let good = write_png(&directory, "gut.png");
    let missing = write_png(&directory, "fehlend.png");
    let clipboard = directory.path().join("clipboard.json");
    import(&source);
    import(&good);
    // `fehlend.png` bewusst NICHT importiert.
    draft_set(&source, &["title=Startschuss"]);

    assert!(copy(&source, &clipboard, None).status.success());
    let result = paste(&clipboard, &[&good, &missing], None);
    assert_eq!(
        result.status.code(),
        Some(3),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("no sidecar"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let payload: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(payload["status"], "partial");
    assert_eq!(payload["updated"], 1);
    assert_eq!(payload["failed"], 1);
    assert_eq!(
        load_sidecar(&sidecar_path_for(&good))
            .unwrap()
            .metadata
            .draft
            .get("title")
            .map(String::as_str),
        Some("Startschuss")
    );
    assert!(!sidecar_path_for(&missing).exists());
}

/// Default-Clipboard-Pfad (OS-Temp): Copy ohne `--out` und Paste ohne
/// Positionsargument finden sich über `TMPDIR` — der Handoff ist explizit und
/// ephemer, kein CWD-Dotfile.
#[test]
fn meta_copy_paste_default_path_uses_os_temp() {
    let directory = tempfile::tempdir().unwrap();
    let fake_temp = directory.path().join("os-temp");
    fs::create_dir_all(&fake_temp).unwrap();
    let source = write_png(&directory, "quelle.png");
    let target = write_png(&directory, "ziel.png");
    import(&source);
    import(&target);
    draft_set(&source, &["title=Startschuss", "keywords=fest"]);

    let copied = cli()
        .args(["meta", "copy", source.to_str().unwrap(), "--json"])
        .env("TMPDIR", &fake_temp)
        .output()
        .unwrap();
    assert!(
        copied.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&copied.stderr)
    );
    let default_clipboard = fake_temp.join("lumina-meta-clipboard.json");
    assert!(
        default_clipboard.exists(),
        "`meta copy` must write the default clipboard into the OS temp dir"
    );

    // Paste ohne Positionsargument liest denselben Default-Pfad.
    let pasted = cli()
        .args([
            "meta",
            "paste",
            "--target",
            target.to_str().unwrap(),
            "--json",
        ])
        .env("TMPDIR", &fake_temp)
        .output()
        .unwrap();
    assert!(
        pasted.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&pasted.stderr)
    );
    let document = load_sidecar(&sidecar_path_for(&target)).unwrap();
    assert_eq!(
        document.metadata.draft.get("title").map(String::as_str),
        Some("Startschuss")
    );
    assert_eq!(document.keywords, vec!["fest".to_string()]);
}

/// Fehlendes `--target` verweigert clap (Exit 2, kein Paste ohne Ziel); ein
/// fehlendes Quell-Sidecar verhindert bereits den Copy (Exit 1).
#[test]
fn meta_copy_paste_reject_missing_source_and_missing_targets() {
    let directory = tempfile::tempdir().unwrap();
    let source = write_png(&directory, "quelle.png");
    let clipboard = directory.path().join("clipboard.json");
    // Quelle bewusst NICHT importiert.
    let result = copy(&source, &clipboard, None);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("no sidecar"));
    assert!(!clipboard.exists());

    // Kein `--target`: clap-Usage-Fehler.
    fs::write(
        &clipboard,
        r#"{"format":"lumina-meta-clipboard","version":1,"fields":{"title":"X"}}"#,
    )
    .unwrap();
    let output = cli()
        .args(["meta", "paste", clipboard.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}
