//! LRPAR-G15-IPTC-S5: CLI-E2E-Tests für `meta sync` (feld-selektiver
//! Draft-/Keyword-Transfer).
//!
//! Jede Story ist an der Kette Datei → Reload verankert (DoD §1): Mutationen
//! werden per `load_sidecar` aus der Datei zurückgelesen, nicht aus dem
//! Prozessgedächtnis. Originale bleiben byte-identisch, Exit-Codes sind laut
//! (0 ok / 1 Fehler / 3 partiell, konsistent zu S3/S4).

use lumina_core::{ImageFileFormat, ImageFrame};
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

fn draft_set(input: &std::path::Path, assignments: &[&str]) {
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

fn sidecar_bytes(input: &std::path::Path) -> Vec<u8> {
    fs::read(sidecar_path_for(input)).unwrap()
}

fn sync(
    source: &std::path::Path,
    targets: &[&std::path::Path],
    fields: Option<&str>,
) -> std::process::Output {
    let mut command = cli();
    command.args(["meta", "sync", "--source", source.to_str().unwrap()]);
    for target in targets {
        command.args(["--target", target.to_str().unwrap()]);
    }
    if let Some(ids) = fields {
        command.args(["--fields", ids]);
    }
    command.args(["--json"]);
    command.output().unwrap()
}

/// S5: Sync-Roundtrip über zwei Ziele — Datei → Reload, nur selektierte
/// Felder (+ Keywords) werden gespiegelt, Rest bleibt, je Ziel ein
/// `sync:<quell-dateiname>`-Eintrag, Exit 0, Originale byte-identisch,
/// Rezepte unberührt.
#[test]
fn meta_sync_roundtrip_over_two_targets() {
    let directory = tempfile::tempdir().unwrap();
    let source = write_png(&directory, "quelle.png");
    let a = write_png(&directory, "a.png");
    let b = write_png(&directory, "b.png");
    import(&source);
    import(&a);
    import(&b);
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
    draft_set(&a, &["title=Alt", "creator=Behalten"]);
    draft_set(&b, &["headline=Weg", "creator=Behalten"]);

    // Rezept-Fingerabdruck je Ziel (Sync darf nie Rezepte anfassen).
    let recipe_before_a = load_sidecar(&sidecar_path_for(&a))
        .unwrap()
        .virtual_copies
        .clone();
    let original_a = fs::read(&a).unwrap();
    let original_b = fs::read(&b).unwrap();

    let result = sync(&source, &[&a, &b], Some("title,city,keywords"));
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.status.code(), Some(0));
    let payload: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["command"], "meta-sync");
    assert_eq!(payload["updated"], 2);
    assert_eq!(payload["unchanged"], 0);
    assert_eq!(payload["failed"], 0);
    assert_eq!(payload["origin"], "sync:quelle.png");

    // Reload aus der Datei (DoD §7 E2E): selektierte Felder gespiegelt,
    // unselektierte (`creator`, `headline` auf b) unberührt.
    for target in [&a, &b] {
        let document = load_sidecar(&sidecar_path_for(target)).unwrap();
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
            Some("Behalten"),
            "unselected draft field must survive the sync"
        );
        assert_eq!(
            document.keywords,
            vec!["fest".to_string(), "abend".to_string()]
        );
        // Zwei Einträge: `draft set` (Setup) + Sync; neueste zuerst.
        assert_eq!(document.metadata.history.len(), 2);
        assert_eq!(document.metadata.history[1].origin, "cli");
        assert_eq!(document.metadata.history[0].rev, 2);
        assert_eq!(document.metadata.history[0].origin, "sync:quelle.png");
        assert_eq!(
            document.metadata.history[0].changed,
            vec![
                "city".to_string(),
                "keywords".to_string(),
                "title".to_string()
            ]
        );
    }
    // `headline=Weg` auf b war nicht selektiert und bleibt stehen.
    let document_b = load_sidecar(&sidecar_path_for(&b)).unwrap();
    assert_eq!(
        document_b
            .metadata
            .draft
            .get("headline")
            .map(String::as_str),
        Some("Weg")
    );
    // Rezepte + Originale unberührt.
    assert_eq!(
        load_sidecar(&sidecar_path_for(&a)).unwrap().virtual_copies,
        recipe_before_a
    );
    assert_eq!(fs::read(&a).unwrap(), original_a);
    assert_eq!(fs::read(&b).unwrap(), original_b);
}

/// S5: Mirror-Semantik — selektiertes, in der Quelle fehlendes Feld wird auf
/// dem Ziel entfernt (laute Konvergenz statt stillem Behalten); leere
/// Quell-Keywords leeren die Zielliste.
#[test]
fn meta_sync_mirrors_absent_source_fields_as_removal() {
    let directory = tempfile::tempdir().unwrap();
    let source = write_png(&directory, "quelle.png");
    let target = write_png(&directory, "ziel.png");
    import(&source);
    import(&target);
    draft_set(&source, &["title=Nur Titel"]);
    draft_set(&target, &["title=Alt", "city=Weg", "keywords=alt"]);

    let result = sync(&source, &[&target], Some("title,city,keywords"));
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let document = load_sidecar(&sidecar_path_for(&target)).unwrap();
    assert_eq!(
        document.metadata.draft.get("title").map(String::as_str),
        Some("Nur Titel")
    );
    assert!(
        !document.metadata.draft.contains_key("city"),
        "selected-but-absent field must be removed on the target"
    );
    assert!(
        document.keywords.is_empty(),
        "empty source keywords must clear the target list"
    );
    // `title` war bereits verschieden → geändert; `city`/`keywords` entfernt.
    assert_eq!(
        document.metadata.history[0].changed,
        vec![
            "city".to_string(),
            "keywords".to_string(),
            "title".to_string()
        ]
    );
}

/// S5: Fehlerisolation — ein Ziel ohne Sidecar markiert nur sein Item als
/// `failed` (Exit 3, „zuerst importieren" via `run import first`), das intakte
/// Ziel wird trotzdem aktualisiert, kein stilles Anlegen.
#[test]
fn meta_sync_isolates_target_failures() {
    let directory = tempfile::tempdir().unwrap();
    let source = write_png(&directory, "quelle.png");
    let good = write_png(&directory, "gut.png");
    let missing = write_png(&directory, "fehlend.png");
    import(&source);
    import(&good);
    // `fehlend.png` wird bewusst NICHT importiert (kein Sidecar).
    draft_set(&source, &["title=Startschuss"]);

    let result = sync(&source, &[&good, &missing], Some("title"));
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
    assert_eq!(payload["unchanged"], 0);
    assert_eq!(payload["failed"], 1);
    // Intaktes Ziel wurde trotz des Fehlers aktualisiert (Reload-Beleg);
    // für das fehlende Ziel wurde kein Sidecar angelegt.
    let document = load_sidecar(&sidecar_path_for(&good)).unwrap();
    assert_eq!(
        document.metadata.draft.get("title").map(String::as_str),
        Some("Startschuss")
    );
    assert!(
        !sidecar_path_for(&missing).exists(),
        "sync must never create a sidecar silently"
    );
}

/// S5: Idempotenz — erneuter Sync meldet `unchanged` (Exit 0, kein neuer
/// Historie-Eintrag, Sidecar-Bytes unverändert).
#[test]
fn meta_sync_is_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let source = write_png(&directory, "quelle.png");
    let target = write_png(&directory, "ziel.png");
    import(&source);
    import(&target);
    draft_set(&source, &["title=Startschuss", "keywords=fest"]);

    let first = sync(&source, &[&target], Some("title,keywords"));
    assert!(first.status.success());
    let before = sidecar_bytes(&target);
    let history_before = load_sidecar(&sidecar_path_for(&target))
        .unwrap()
        .metadata
        .history
        .len();

    let result = sync(&source, &[&target], Some("title,keywords"));
    assert!(result.status.success());
    assert_eq!(result.status.code(), Some(0));
    let payload: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(payload["updated"], 0);
    assert_eq!(payload["unchanged"], 1);
    assert_eq!(payload["failed"], 0);
    assert_eq!(sidecar_bytes(&target), before);
    assert_eq!(
        load_sidecar(&sidecar_path_for(&target))
            .unwrap()
            .metadata
            .history
            .len(),
        history_before
    );
}

/// S5: `--fields` ist Pflicht — ohne Angabe kein Still-All (Exit 1, lauter
/// Fehler, nichts geschrieben).
#[test]
fn meta_sync_requires_fields() {
    let directory = tempfile::tempdir().unwrap();
    let source = write_png(&directory, "quelle.png");
    let target = write_png(&directory, "ziel.png");
    import(&source);
    import(&target);
    draft_set(&source, &["title=Startschuss"]);
    let before = sidecar_bytes(&target);

    let result = sync(&source, &[&target], None);
    assert_eq!(result.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("--fields"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(sidecar_bytes(&target), before);
}

/// S5: unbekannte Feld-ID ist ein lauter Fehler (Exit 1, nichts geschrieben,
/// auch nicht die gültigen Felder).
#[test]
fn meta_sync_rejects_unknown_field_id() {
    let directory = tempfile::tempdir().unwrap();
    let source = write_png(&directory, "quelle.png");
    let target = write_png(&directory, "ziel.png");
    import(&source);
    import(&target);
    draft_set(&source, &["title=Startschuss"]);
    let before = sidecar_bytes(&target);

    let result = sync(&source, &[&target], Some("title,nope"));
    assert_eq!(result.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("unknown metadata field `nope`"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(sidecar_bytes(&target), before);
}

/// S5: fehlendes Quell-Sidecar bricht alles laut ab (Exit 1); fehlendes
/// `--target` verweigert clap (Exit 2, kein Still-All ohne Ziel).
#[test]
fn meta_sync_rejects_missing_source_and_missing_targets() {
    let directory = tempfile::tempdir().unwrap();
    let source = write_png(&directory, "quelle.png");
    let target = write_png(&directory, "ziel.png");
    // Quelle bewusst NICHT importiert.
    import(&target);
    let before = sidecar_bytes(&target);

    let result = sync(&source, &[&target], Some("title"));
    assert_eq!(result.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("no sidecar"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(sidecar_bytes(&target), before);

    // Kein `--target`: clap-Usage-Fehler, kein Sync ohne Ziel.
    let output = cli()
        .args([
            "meta",
            "sync",
            "--source",
            target.to_str().unwrap(),
            "--fields",
            "title",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}
