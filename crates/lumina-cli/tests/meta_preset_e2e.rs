//! LRPAR-G15-IPTC-S4: CLI-E2E-Tests für `meta preset list|show|apply`
//! (statische + dynamische Meta-Presets).
//!
//! Jede Story ist an der Kette Datei → Reload verankert (DoD §1): Mutationen
//! werden per `load_sidecar` aus der Datei zurückgelesen, nicht aus dem
//! Prozessgedächtnis. Originale bleiben byte-identisch, Exit-Codes sind laut.

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

fn write_preset(
    dir: &std::path::Path,
    name: &str,
    payload: &serde_json::Value,
) -> std::path::PathBuf {
    let path = dir.join(format!("{name}.lumina-meta-preset.json"));
    fs::write(&path, serde_json::to_vec_pretty(payload).unwrap()).unwrap();
    path
}

fn static_payload() -> serde_json::Value {
    serde_json::json!({
        "format": "lumina-meta-preset",
        "version": 1,
        "name": "Veranstaltung",
        "fields": { "title": "Startschuss", "city": "Berlin" },
        "placeholders": [],
    })
}

fn dynamic_payload() -> serde_json::Value {
    serde_json::json!({
        "format": "lumina-meta-preset",
        "version": 1,
        "name": "Dynamik",
        "fields": { "title": "{event_name} in {ort}", "city": "{ort}" },
        "placeholders": [
            { "name": "event_name", "description": "Name der Veranstaltung" },
            { "name": "ort", "description": "Stadt" },
        ],
    })
}

fn sidecar_bytes(input: &std::path::Path) -> Vec<u8> {
    fs::read(sidecar_path_for(input)).unwrap()
}

/// S4: statisches Preset über 2 Ziele — Apply → Datei → Reload, `origin =
/// "preset:<name>"`, je Ziel ein Historie-Eintrag, Exit 0, Originale
/// byte-identisch.
#[test]
fn meta_preset_apply_static_roundtrip_over_two_targets() {
    let directory = tempfile::tempdir().unwrap();
    let presets = directory.path().join("presets");
    fs::create_dir_all(&presets).unwrap();
    let preset = write_preset(&presets, "Veranstaltung", &static_payload());
    let a = write_png(&directory, "a.png");
    let b = write_png(&directory, "b.png");
    import(&a);
    import(&b);
    let original_a = fs::read(&a).unwrap();
    let original_b = fs::read(&b).unwrap();

    let result = cli()
        .args([
            "meta",
            "preset",
            "apply",
            preset.to_str().unwrap(),
            "--target",
            a.to_str().unwrap(),
            "--target",
            b.to_str().unwrap(),
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
    assert_eq!(payload["updated"], 2);
    assert_eq!(payload["unchanged"], 0);
    assert_eq!(payload["failed"], 0);

    // Reload aus der Datei: beide Ziele tragen Draft + Historie (DoD §7 E2E).
    for input in [&a, &b] {
        let document = load_sidecar(&sidecar_path_for(input)).unwrap();
        assert_eq!(
            document.metadata.draft.get("title").map(String::as_str),
            Some("Startschuss")
        );
        assert_eq!(
            document.metadata.draft.get("city").map(String::as_str),
            Some("Berlin")
        );
        assert_eq!(document.metadata.history.len(), 1);
        assert_eq!(document.metadata.history[0].rev, 1);
        assert_eq!(document.metadata.history[0].origin, "preset:Veranstaltung");
        assert_eq!(
            document.metadata.history[0].changed,
            vec!["city".to_string(), "title".to_string()]
        );
    }
    assert_eq!(fs::read(&a).unwrap(), original_a);
    assert_eq!(fs::read(&b).unwrap(), original_b);
}

/// S4: Idempotenz — erneutes Anwenden meldet `unchanged` (Exit 0, kein neuer
/// Historie-Eintrag, Sidecar-Bytes unverändert).
#[test]
fn meta_preset_apply_is_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let presets = directory.path().join("presets");
    fs::create_dir_all(&presets).unwrap();
    let preset = write_preset(&presets, "Veranstaltung", &static_payload());
    let a = write_png(&directory, "a.png");
    import(&a);
    let apply = || {
        cli()
            .args([
                "meta",
                "preset",
                "apply",
                preset.to_str().unwrap(),
                "--target",
                a.to_str().unwrap(),
                "--json",
            ])
            .output()
            .unwrap()
    };
    assert!(apply().status.success());
    let before = sidecar_bytes(&a);
    let history_before = load_sidecar(&sidecar_path_for(&a))
        .unwrap()
        .metadata
        .history
        .len();

    let result = apply();
    assert!(result.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(payload["updated"], 0);
    assert_eq!(payload["unchanged"], 1);
    assert_eq!(payload["failed"], 0);
    assert_eq!(sidecar_bytes(&a), before);
    assert_eq!(
        load_sidecar(&sidecar_path_for(&a))
            .unwrap()
            .metadata
            .history
            .len(),
        history_before
    );
}

/// S4: dynamisches Preset — Substitution mit `--var`, Datei → Reload;
/// fehlende/unbekannte Variablen und Limitverletzungen sind laute Fehler
/// (Exit ≠ 0, nichts geschrieben).
#[test]
fn meta_preset_apply_dynamic_substitution_and_errors() {
    let directory = tempfile::tempdir().unwrap();
    let presets = directory.path().join("presets");
    fs::create_dir_all(&presets).unwrap();
    let preset = write_preset(&presets, "Dynamik", &dynamic_payload());
    let a = write_png(&directory, "a.png");
    import(&a);

    let result = cli()
        .args([
            "meta",
            "preset",
            "apply",
            preset.to_str().unwrap(),
            "--target",
            a.to_str().unwrap(),
            "--var",
            "event_name=Fest",
            "--var",
            "ort=Berlin",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let document = load_sidecar(&sidecar_path_for(&a)).unwrap();
    assert_eq!(
        document.metadata.draft.get("title").map(String::as_str),
        Some("Fest in Berlin")
    );
    assert_eq!(
        document.metadata.draft.get("city").map(String::as_str),
        Some("Berlin")
    );
    assert_eq!(document.metadata.history[0].origin, "preset:Dynamik");

    // Fehlende Variable: lauter Fehler, nichts geschrieben.
    let before = sidecar_bytes(&a);
    let result = cli()
        .args([
            "meta",
            "preset",
            "apply",
            preset.to_str().unwrap(),
            "--target",
            a.to_str().unwrap(),
            "--var",
            "event_name=Fest",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(result.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("missing value for placeholder"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(sidecar_bytes(&a), before);

    // Unbekannte Variable: lauter Fehler, nichts geschrieben.
    let result = cli()
        .args([
            "meta",
            "preset",
            "apply",
            preset.to_str().unwrap(),
            "--target",
            a.to_str().unwrap(),
            "--var",
            "event_name=Fest",
            "--var",
            "ort=Berlin",
            "--var",
            "extra=x",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("unknown variable"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(sidecar_bytes(&a), before);

    // Limitverletzung (Titel > 256 Zeichen): lauter Fehler, nichts geschrieben.
    let long = "x".repeat(257);
    let result = cli()
        .args([
            "meta",
            "preset",
            "apply",
            preset.to_str().unwrap(),
            "--target",
            a.to_str().unwrap(),
            "--var",
            &format!("event_name={long}"),
            "--var",
            "ort=Berlin",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("exceeds limit"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(sidecar_bytes(&a), before);

    // Doppeltes `--var` und fehlendes `=`: laute Fehler.
    for vars in [
        vec!["event_name=Fest", "event_name=Feier", "ort=Berlin"],
        vec!["event_nameFest", "ort=Berlin"],
    ] {
        let mut command = cli();
        command.args([
            "meta",
            "preset",
            "apply",
            preset.to_str().unwrap(),
            "--target",
            a.to_str().unwrap(),
        ]);
        for var in vars {
            command.args(["--var", var]);
        }
        let result = command.output().unwrap();
        assert!(!result.status.success());
        assert_eq!(sidecar_bytes(&a), before);
    }
}

/// S4: Fehlerisolation je Ziel — ein Ziel ohne Sidecar markiert nur sein Item
/// als `failed` (Exit 3), das intakte Ziel wird trotzdem aktualisiert.
#[test]
fn meta_preset_apply_isolates_target_failures() {
    let directory = tempfile::tempdir().unwrap();
    let presets = directory.path().join("presets");
    fs::create_dir_all(&presets).unwrap();
    let preset = write_preset(&presets, "Veranstaltung", &static_payload());
    let a = write_png(&directory, "a.png");
    let missing = write_png(&directory, "missing.png");
    import(&a);
    // `missing.png` wird bewusst NICHT importiert (kein Sidecar).

    let result = cli()
        .args([
            "meta",
            "preset",
            "apply",
            preset.to_str().unwrap(),
            "--target",
            a.to_str().unwrap(),
            "--target",
            missing.to_str().unwrap(),
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
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("no sidecar"),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let payload: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(payload["status"], "partial");
    assert_eq!(payload["updated"], 1);
    assert_eq!(payload["failed"], 1);
    // Intaktes Ziel wurde trotz des Fehlers aktualisiert (Reload-Beleg).
    let document = load_sidecar(&sidecar_path_for(&a)).unwrap();
    assert_eq!(
        document.metadata.draft.get("title").map(String::as_str),
        Some("Startschuss")
    );
}

/// S4: Format-Validierung — falsche Envelope/version und kaputte
/// Platzhalter-Syntax werden laut abgelehnt (Exit ≠ 0); `list` meldet die
/// Datei als failed-Eintrag (Exit 0); `show` scheitert laut.
#[test]
fn meta_preset_format_validation_is_loud() {
    let directory = tempfile::tempdir().unwrap();
    let presets = directory.path().join("presets");
    fs::create_dir_all(&presets).unwrap();
    let a = write_png(&directory, "a.png");
    import(&a);
    let before = sidecar_bytes(&a);

    let bad_cases: &[(&str, serde_json::Value)] = &[
        (
            "FalschesFormat",
            serde_json::json!({
                "format": "lumina-preset", "version": 1, "name": "FalschesFormat",
                "fields": { "title": "x" }, "placeholders": [],
            }),
        ),
        (
            "FremdeVersion",
            serde_json::json!({
                "format": "lumina-meta-preset", "version": 2, "name": "FremdeVersion",
                "fields": { "title": "x" }, "placeholders": [],
            }),
        ),
        (
            "KlammerFehler",
            serde_json::json!({
                "format": "lumina-meta-preset", "version": 1, "name": "KlammerFehler",
                "fields": { "title": "lone { brace" }, "placeholders": [],
            }),
        ),
        (
            "UnbekanntesFeld",
            serde_json::json!({
                "format": "lumina-meta-preset", "version": 1, "name": "UnbekanntesFeld",
                "fields": { "nope": "x" }, "placeholders": [],
            }),
        ),
    ];
    for (name, payload) in bad_cases {
        let path = write_preset(&presets, name, payload);
        let result = cli()
            .args([
                "meta",
                "preset",
                "apply",
                path.to_str().unwrap(),
                "--target",
                a.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(!result.status.success(), "preset {name} must be rejected");
        assert_eq!(result.status.code(), Some(1));
        assert!(
            !String::from_utf8_lossy(&result.stderr).is_empty(),
            "preset {name} needs a loud stderr"
        );
        let result = cli()
            .args(["meta", "preset", "show", path.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(!result.status.success(), "show {name} must be rejected");
    }
    assert_eq!(sidecar_bytes(&a), before);

    // `list` über das Verzeichnis: alle vier Dateien als failed (Exit 0).
    let result = cli()
        .args([
            "meta",
            "preset",
            "list",
            presets.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(result.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(payload["available"], 0);
    assert_eq!(payload["failed"], 4);
}

/// S4: `list` / `show` — Exit 0, Datei → Anzeige, Escapes werden aufgelöst.
#[test]
fn meta_preset_list_and_show() {
    let directory = tempfile::tempdir().unwrap();
    let presets = directory.path().join("presets");
    fs::create_dir_all(&presets).unwrap();
    write_preset(&presets, "Veranstaltung", &static_payload());
    fs::write(presets.join("Kaputt.lumina-meta-preset.json"), b"{ broken").unwrap();

    let result = cli()
        .args([
            "meta",
            "preset",
            "list",
            presets.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let payload: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(payload["available"], 1);
    assert_eq!(payload["failed"], 1);
    // Sortiert nach Dateiname: "Kaputt…" (failed) vor "Veranstaltung…" (available).
    let items = payload["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let available = items
        .iter()
        .find(|item| item["status"] == "available")
        .unwrap();
    assert_eq!(available["name"], "Veranstaltung");
    assert!(
        items.iter().any(|item| item["status"] == "failed"),
        "failed-Eintrag muss sichtbar sein"
    );

    // `show` per Pfad: Felder + Platzhalter sichtbar (Exit 0).
    let dynamic = write_preset(&presets, "Dynamik", &dynamic_payload());
    let result = cli()
        .args([
            "meta",
            "preset",
            "show",
            dynamic.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(result.status.success());
    let payload: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(payload["name"], "Dynamik");
    assert_eq!(payload["fields"]["city"], "{ort}");
    assert_eq!(payload["placeholders"][0]["name"], "event_name");

    // Namensauflösung gegen ein explizites Verzeichnis gibt es nicht für
    // show/apply (nur global oder Pfad) — ein unbekannter Name scheitert laut.
    let result = cli()
        .args(["meta", "preset", "show", "gibt-es-nicht"])
        .output()
        .unwrap();
    assert!(!result.status.success());
}
