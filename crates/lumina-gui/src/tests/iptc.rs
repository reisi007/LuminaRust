//! IPTC GUI draft/preset/sync/history tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

#[test]
fn iptc_gui_draft_edit_commit_reload() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let original_bytes = std::fs::read(&source).unwrap();
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_metadata_buffer("title", "Startschuss".into())
        .unwrap();
    app.set_metadata_buffer("city", "Berlin".into()).unwrap();
    app.set_metadata_buffer("description", "Erste Zeile, zweite folgt".into())
        .unwrap();
    app.set_metadata_buffer("date_created", "2026-09-04".into())
        .unwrap();
    assert!(app.commit_metadata_draft().unwrap());
    // Datei: Werte + Historie (origin gui, rev 1, changed sortiert).
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert_eq!(document.metadata.get("title"), Some("Startschuss"));
    assert_eq!(document.metadata.get("city"), Some("Berlin"));
    assert_eq!(document.metadata.get("date_created"), Some("2026-09-04"));
    assert_eq!(document.metadata.history.len(), 1);
    let entry = &document.metadata.history[0];
    assert_eq!(entry.rev, 1);
    assert_eq!(entry.origin, "gui");
    assert_eq!(
        entry.changed,
        vec![
            "city".to_string(),
            "date_created".to_string(),
            "description".to_string(),
            "title".to_string()
        ]
    );
    // Original byte-identisch.
    assert_eq!(std::fs::read(&source).unwrap(), original_bytes);
    // Reload: Entwurf + idempotenter No-Op ohne Historie-Eintrag.
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert_eq!(
        reopened.metadata_draft().get("title").map(String::as_str),
        Some("Startschuss")
    );
    assert!(!reopened.commit_metadata_draft().unwrap());
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert_eq!(document.metadata.history.len(), 1);
}

/// KITTEST-COVERAGE-STATES-1: the metadata panel's own copy/paste system —
/// copy the draft of one image, paste it onto another, and prove the
/// result persisted through the sidecar (Edit → Commit → Datei → Reload).
#[test]
fn iptc_gui_metadata_copy_paste_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.png");
    let target = directory.path().join("target.png");
    save_png(&source);
    save_png(&target);
    // Source: enter + commit a draft, then copy it.
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_metadata_buffer("title", "Startschuss".into())
        .unwrap();
    app.set_metadata_buffer("city", "Berlin".into()).unwrap();
    assert!(app.commit_metadata_draft().unwrap());
    assert_eq!(app.copy_metadata_draft().unwrap(), 2);
    assert!(app.status().contains("Metadata copied"));
    // Paste onto the target (open switches the loaded path).
    open_and_decode_switch(&mut app, &target.display().to_string());
    assert!(app.paste_metadata_draft().unwrap());
    // Datei: target's sidecar carries the pasted draft + a gui history entry.
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&target)).unwrap();
    assert_eq!(document.metadata.get("title"), Some("Startschuss"));
    assert_eq!(document.metadata.get("city"), Some("Berlin"));
    assert_eq!(document.metadata.history.len(), 1);
    assert_eq!(document.metadata.history[0].origin, "gui");
    assert_eq!(
        document.metadata.history[0].changed,
        vec!["city".to_string(), "title".to_string()]
    );
    // Reload: the pasted draft is restored. The clipboard is session-only
    // (never persisted), so the reopened app has none — paste is loud.
    let mut reopened = new_app();
    open_and_decode(&mut reopened, target.display().to_string());
    assert_eq!(
        reopened.metadata_draft().get("city").map(String::as_str),
        Some("Berlin")
    );
    assert!(reopened
        .paste_metadata_draft()
        .unwrap_err()
        .to_string()
        .contains("copy metadata"));
}

/// KITTEST-COVERAGE-STATES-1: paste without a prior copy is loud (never a
/// silent no-op) and copies the current image's unsaved buffer edits.
#[test]
fn iptc_gui_metadata_copy_paste_loud_without_clipboard() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let error = app.paste_metadata_draft().unwrap_err().to_string();
    assert!(error.contains("copy metadata"), "loud paste error: {error}");
    // Unsaved buffer edits are what Copy captures (panel shows them).
    app.set_metadata_buffer("headline", "Vor Ort".into())
        .unwrap();
    assert_eq!(app.copy_metadata_draft().unwrap(), 1);
}

#[test]
fn iptc_gui_draft_validation_is_loud() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    // Ungültiges Datum: Commit scheitert laut, nichts geschrieben.
    app.set_metadata_buffer("date_created", "2026/09/04".into())
        .unwrap();
    assert!(app.commit_metadata_draft().is_err());
    assert!(!lumina_sidecar::sidecar_path_for(&source).is_file());
    // Unbekannte ID und keywords als Draft-Feld: schon der Puffer laut.
    assert!(app.set_metadata_buffer("nope", "x".into()).is_err());
    let keywords_error = app.set_metadata_buffer("keywords", "x".into()).unwrap_err();
    assert!(keywords_error.to_string().contains("keywords"));
    // Überlanges Feld: Commit scheitert, gültige Felder landen nicht partiell.
    app.set_metadata_buffer("date_created", "2026-09-04".into())
        .unwrap();
    app.set_metadata_buffer("title", "x".repeat(257)).unwrap();
    assert!(app.commit_metadata_draft().is_err());
    assert!(!lumina_sidecar::sidecar_path_for(&source).is_file());
}

#[test]
fn iptc_gui_empty_buffer_removes_field() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_metadata_buffer("title", "Startschuss".into())
        .unwrap();
    app.set_metadata_buffer("city", "Berlin".into()).unwrap();
    assert!(app.commit_metadata_draft().unwrap());
    // Leerer Puffer entfernt das Feld (S1-Draft-Semantik), Historie +1.
    app.set_metadata_buffer("title", String::new()).unwrap();
    assert!(app.commit_metadata_draft().unwrap());
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert_eq!(document.metadata.get("title"), None);
    assert_eq!(document.metadata.get("city"), Some("Berlin"));
    assert_eq!(document.metadata.history.len(), 2);
    assert_eq!(
        document.metadata.history[0].changed,
        vec!["title".to_string()]
    );
}

#[test]
fn iptc_gui_clear_fields_and_all() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_metadata_buffer("title", "Startschuss".into())
        .unwrap();
    app.set_metadata_buffer("city", "Berlin".into()).unwrap();
    app.commit_metadata_draft().unwrap();
    // Gezieltes Leeren (inkl. keywords-Routing).
    app.add_keyword("temp").unwrap();
    assert!(app
        .clear_metadata_fields(&["title".to_string(), "keywords".to_string()])
        .unwrap());
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert_eq!(document.metadata.get("title"), None);
    assert!(document.keywords.is_empty());
    // --all leert den Entwurf, Historie bleibt.
    let history_before = document.metadata.history.len();
    assert!(app.clear_metadata_draft_all().unwrap());
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert!(document.metadata.draft.is_empty());
    assert_eq!(document.metadata.history.len(), history_before + 1);
    assert!(!app.clear_metadata_draft_all().unwrap());
    // Unbekannte ID scheitert laut ohne Mutation.
    assert!(app.clear_metadata_fields(&["nope".to_string()]).is_err());
}

#[test]
fn iptc_gui_static_preset_apply() {
    let directory = tempfile::tempdir().unwrap();
    let presets = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    write_meta_preset(
        presets.path(),
        "Fest.lumina-meta-preset.json",
        "Fest",
        &[("title", "Startschuss"), ("city", "Berlin")],
        &[],
    );
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_meta_presets_dir(Some(presets.path().to_path_buf()));
    app.refresh_meta_presets();
    assert_eq!(app.meta_preset_names(), vec!["Fest".to_string()]);
    assert!(app.meta_preset_placeholders("Fest").unwrap().is_empty());
    assert!(app
        .apply_meta_preset_loaded("Fest", &BTreeMap::new())
        .unwrap());
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert_eq!(document.metadata.get("title"), Some("Startschuss"));
    assert_eq!(document.metadata.history[0].origin, "preset:Fest");
    // Idempotente Wiederanwendung: kein Fehler, kein Historie-Eintrag.
    assert!(!app
        .apply_meta_preset_loaded("Fest", &BTreeMap::new())
        .unwrap());
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert_eq!(document.metadata.history.len(), 1);
}

#[test]
fn iptc_gui_dynamic_preset_requires_vars() {
    let directory = tempfile::tempdir().unwrap();
    let presets = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    write_meta_preset(
        presets.path(),
        "Dyn.lumina-meta-preset.json",
        "Dyn",
        &[("title", "{event_name} in {ort}"), ("city", "{ort}")],
        &[("event_name", "Name"), ("ort", "Stadt")],
    );
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_meta_presets_dir(Some(presets.path().to_path_buf()));
    app.refresh_meta_presets();
    let placeholders = app.meta_preset_placeholders("Dyn").unwrap();
    assert_eq!(placeholders.len(), 2);
    // Fehlende Variablen: laut, nichts geschrieben (Prompt-Abbruch-Semantik).
    assert!(app
        .apply_meta_preset_loaded("Dyn", &BTreeMap::new())
        .is_err());
    assert!(!lumina_sidecar::sidecar_path_for(&source).is_file());
    // Unbekannte Variable: laut, nichts geschrieben.
    let mut vars = BTreeMap::new();
    vars.insert("event_name".to_string(), "Fest".to_string());
    vars.insert("ort".to_string(), "Berlin".to_string());
    vars.insert("extra".to_string(), "x".to_string());
    assert!(app.apply_meta_preset_loaded("Dyn", &vars).is_err());
    assert!(!lumina_sidecar::sidecar_path_for(&source).is_file());
    // Vollständige Variablen: aufgelöst geschrieben.
    vars.remove("extra");
    assert!(app.apply_meta_preset_loaded("Dyn", &vars).unwrap());
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert_eq!(document.metadata.get("title"), Some("Fest in Berlin"));
    assert_eq!(document.metadata.get("city"), Some("Berlin"));
    // Dialog-Abbruch ändert nichts: verworfener Dialog ohne Apply.
    app.meta_preset_dialog = Some(MetaPresetDialog {
        spec: "Dyn".to_string(),
        name: "Dyn".to_string(),
        placeholders: vec![("event_name".to_string(), "Name".to_string())],
        vars: BTreeMap::new(),
        error: None,
    });
    app.meta_preset_dialog = None;
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert_eq!(document.metadata.history.len(), 1);
}

#[test]
fn iptc_gui_preset_keywords_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let presets = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    write_meta_preset(
        presets.path(),
        "Bad.lumina-meta-preset.json",
        "Bad",
        &[("keywords", "x")],
        &[],
    );
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_meta_presets_dir(Some(presets.path().to_path_buf()));
    app.refresh_meta_presets();
    // Kaputte Datei bleibt als Failed-Eintrag sichtbar (S4-Semantik).
    assert!(app.meta_preset_names().is_empty());
    assert_eq!(app.meta_preset_entries.len(), 1);
    assert!(app
        .apply_meta_preset_loaded("Bad", &BTreeMap::new())
        .is_err());
    assert!(!lumina_sidecar::sidecar_path_for(&source).is_file());
}

#[test]
fn iptc_gui_sync_mirror() {
    let directory = tempfile::tempdir().unwrap();
    let a = directory.path().join("a.png");
    let b = directory.path().join("b.png");
    save_png(&a);
    save_png(&b);
    seed_sidecar(&a);
    seed_sidecar(&b);
    // Quelle A: title + city + Keywords.
    let mut setup = new_app();
    open_and_decode(&mut setup, a.display().to_string());
    setup.set_metadata_buffer("title", "Fest".into()).unwrap();
    setup.set_metadata_buffer("city", "Berlin".into()).unwrap();
    setup.commit_metadata_draft().unwrap();
    setup.add_keyword("quelle").unwrap();
    // Ziel B: abweichender title, eigenes headline, eigene Keywords.
    let mut setup = new_app();
    open_and_decode(&mut setup, b.display().to_string());
    setup.set_metadata_buffer("title", "Alt".into()).unwrap();
    setup
        .set_metadata_buffer("headline", "Bleibt".into())
        .unwrap();
    setup.commit_metadata_draft().unwrap();
    setup.add_keyword("ziel").unwrap();
    // Sync {title, keywords} von A auf beide (A selbst: No-Op).
    let mut app = new_app();
    open_and_decode(&mut app, a.display().to_string());
    app.filmstrip_selection.insert(a.display().to_string());
    app.filmstrip_selection.insert(b.display().to_string());
    let fields: BTreeSet<String> = ["title".to_string(), "keywords".to_string()]
        .into_iter()
        .collect();
    let report = app.sync_metadata_to_selection(&fields);
    assert_eq!(report.applied_count(), 2);
    assert_eq!(report.failed_count(), 0);
    let document = lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&b)).unwrap();
    assert_eq!(document.metadata.get("title"), Some("Fest"));
    assert_eq!(document.metadata.get("headline"), Some("Bleibt"));
    assert!(document.keywords.contains(&"quelle".to_string()));
    assert!(!document.keywords.contains(&"ziel".to_string()));
    let entry = &document.metadata.history[0];
    assert_eq!(entry.origin, "sync:a.png");
    assert_eq!(
        entry.changed,
        vec!["keywords".to_string(), "title".to_string()]
    );
    // Mirror-Entfernung: city auf A leeren, {city} syncen → B verliert city.
    let mut setup = new_app();
    open_and_decode(&mut setup, b.display().to_string());
    setup.set_metadata_buffer("city", "Hamburg".into()).unwrap();
    setup.commit_metadata_draft().unwrap();
    let mut app = new_app();
    open_and_decode(&mut app, a.display().to_string());
    app.set_metadata_buffer("city", String::new()).unwrap();
    app.commit_metadata_draft().unwrap();
    app.filmstrip_selection.insert(a.display().to_string());
    app.filmstrip_selection.insert(b.display().to_string());
    let fields: BTreeSet<String> = ["city".to_string()].into_iter().collect();
    let report = app.sync_metadata_to_selection(&fields);
    assert_eq!(report.failed_count(), 0);
    let document = lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&b)).unwrap();
    assert_eq!(document.metadata.get("city"), None);
    // Nicht-selektierte Felder bleiben unberührt.
    assert_eq!(document.metadata.get("title"), Some("Fest"));
}

#[test]
fn iptc_gui_sync_failure_isolation() {
    let directory = tempfile::tempdir().unwrap();
    let good = directory.path().join("good.png");
    let bad = directory.path().join("bad.png");
    save_png(&good);
    save_png(&bad);
    seed_sidecar(&good);
    seed_sidecar(&bad);
    let mut setup = new_app();
    open_and_decode(&mut setup, good.display().to_string());
    setup.set_metadata_buffer("title", "Fest".into()).unwrap();
    setup.commit_metadata_draft().unwrap();
    // Ziel-Sidecar korrumpieren (Original bleibt unberührt).
    std::fs::write(lumina_sidecar::sidecar_path_for(&bad), b"{broken").unwrap();
    let mut app = new_app();
    open_and_decode(&mut app, good.display().to_string());
    app.filmstrip_selection.insert(good.display().to_string());
    app.filmstrip_selection.insert(bad.display().to_string());
    let fields: BTreeSet<String> = ["title".to_string()].into_iter().collect();
    let report = app.sync_metadata_to_selection(&fields);
    assert_eq!(report.applied_count(), 1);
    assert_eq!(report.failed_count(), 1);
    assert_eq!(report.failed[0].0, bad.display().to_string());
    assert!(app.error().is_some(), "failure must stay loud");
    // Ziel ohne Sidecar: lauter Pro-Bild-Fehler, kein stilles Anlegen.
    let noside = directory.path().join("noside.png");
    save_png(&noside);
    app.filmstrip_selection.clear();
    app.filmstrip_selection.insert(noside.display().to_string());
    let report = app.sync_metadata_to_selection(&fields);
    assert_eq!(report.applied_count(), 0);
    assert_eq!(report.failed_count(), 1);
    assert!(!lumina_sidecar::sidecar_path_for(&noside).is_file());
    // Leere Feldwahl: kein Still-All.
    let report = app.sync_metadata_to_selection(&BTreeSet::new());
    assert_eq!(report.applied_count(), 0);
    assert!(app.error().is_some());
    // Default-Auswahl: alle Draft-Felder + Keywords, alle an.
    let defaults = default_meta_sync_fields();
    assert_eq!(defaults.len(), METADATA_FIELD_IDS.len() + 1);
    assert!(defaults.values().all(|checked| *checked));
}

#[test]
fn iptc_gui_history_clear_and_embedded() {
    let directory = tempfile::tempdir().unwrap();
    let png = directory.path().join("photo.png");
    save_png(&png);
    let mut app = new_app();
    open_and_decode(&mut app, png.display().to_string());
    // PNG: kein Embedded ("nicht verfügbar").
    assert!(app.embedded_metadata().unwrap().is_none());
    app.set_metadata_buffer("title", "Fest".into()).unwrap();
    app.commit_metadata_draft().unwrap();
    assert_eq!(app.metadata_history().len(), 1);
    // Explizites Leeren: Historie weg, Entwurf bleibt.
    assert_eq!(app.clear_metadata_history_gui().unwrap(), 1);
    assert_eq!(app.metadata_history().len(), 0);
    let document = lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&png)).unwrap();
    assert!(document.metadata.history.is_empty());
    assert_eq!(document.metadata.get("title"), Some("Fest"));
    assert_eq!(app.clear_metadata_history_gui().unwrap(), 0);
    // JPEG mit eingebranntem IPTC: lesend verfügbar (read-only).
    let plain_jpeg = directory.path().join("plain.jpg");
    save_jpeg(&plain_jpeg);
    let mut app = new_app();
    open_and_decode(&mut app, plain_jpeg.display().to_string());
    let read = app.embedded_metadata().unwrap();
    assert!(
        read.is_none_or(|meta| meta.title.is_none()),
        "fresh JPEG without IPTC carries no title"
    );
    let jpeg_path = directory.path().join("photo.jpg");
    let meta = lumina_iptc::IptcMetadata {
        title: Some("Eingebettet".to_string()),
        keywords: vec!["k1".to_string()],
        ..lumina_iptc::IptcMetadata::default()
    };
    let embedded = lumina_iptc::embed_metadata(&jpeg(), &meta).unwrap();
    std::fs::write(&jpeg_path, embedded).unwrap();
    let mut app = new_app();
    open_and_decode(&mut app, jpeg_path.display().to_string());
    let read = app.embedded_metadata().unwrap().expect("JPEG carries IPTC");
    assert_eq!(read.title.as_deref(), Some("Eingebettet"));
    assert_eq!(read.keywords, vec!["k1".to_string()]);
}
