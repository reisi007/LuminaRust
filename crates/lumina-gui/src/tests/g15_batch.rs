//! G-15 batch apply/save-load/empty selection tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// G-15 Smart-Katalog E2E (DoD §1): Regel-Stack → Create → Datei →
/// Reload in neuer App → Match; ungültige Dateien/Definitionen laut.
#[test]
fn g15_smart_catalog_create_save_load_match() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let catalog_path = directory.path().join("catalog.json");
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.add_keyword("portrait").unwrap();
    app.set_rating(4).unwrap();

    // Create braucht genau eine kombinierte Regel.
    assert!(app.create_smart_collection("s1", "S1").is_err());
    app.push_smart_rule("keyword", "portrait").unwrap();
    app.push_smart_rule("rating_at_least", "4").unwrap();
    app.combine_smart_stack("and").unwrap();
    assert!(app.push_smart_rule("bogus", "x").is_err());
    app.create_smart_collection("s1", "Portraits 4+").unwrap();
    // Doppelte Id scheitert laut.
    app.push_smart_rule("all", "").unwrap();
    assert!(app.create_smart_collection("s1", "Dup").is_err());
    app.smart_rule_stack.clear();
    // Unbekanntes Löschen scheitert laut.
    assert!(app.delete_smart_collection("ghost").is_err());

    app.save_smart_catalog(&catalog_path.display().to_string())
        .unwrap();
    // CLI-kompatibles Format: gleiche Envelope.
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&catalog_path).unwrap()).unwrap();
    assert_eq!(raw["format"], "lumina-smart-catalog");
    assert_eq!(raw["version"], 1);

    // Reload in neuer App: Katalog + Match über den neu gescannten Eintrag.
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    let count = reopened
        .load_smart_catalog(&catalog_path.display().to_string())
        .unwrap();
    assert_eq!(count, 1);
    reopened.set_directory(directory.path().display().to_string());
    let entry = reopened
        .entries
        .iter()
        .find(|entry| entry.name == "photo.png")
        .unwrap();
    assert!(collection_filter_matches_entry(
        entry,
        &CollectionFilter::Smart {
            id: "s1".to_string()
        },
        &reopened.smart_catalog
    ));
    // Ungültiger Katalog (falsches Format) scheitert laut.
    let bad_path = directory.path().join("bad.json");
    std::fs::write(
        &bad_path,
        r#"{"format":"nope","version":1,"collections":[]}"#,
    )
    .unwrap();
    assert!(reopened
        .load_smart_catalog(&bad_path.display().to_string())
        .is_err());
    // Ungültige Definition (leeres And) scheitert laut beim Laden.
    let bad_rule = directory.path().join("bad-rule.json");
    std::fs::write(
        &bad_rule,
        r#"{"format":"lumina-smart-catalog","version":1,"collections":[{"version":1,"id":"x","name":"X","rule":{"op":"and","rules":[]}}]}"#,
    )
    .unwrap();
    assert!(reopened
        .load_smart_catalog(&bad_rule.display().to_string())
        .is_err());
}

/// G-15 Stapel E2E (DoD §1): ein BatchOp über die Auswahl → beide
/// Sidecar-Dateien → Reload beider; danach Umbenennung per
/// Remove+Add über die Auswahl.
#[test]
fn g15_batch_applies_to_selection_and_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let sources: Vec<PathBuf> = ["a.png", "b.png"]
        .iter()
        .map(|name| directory.path().join(name))
        .collect();
    for source in &sources {
        save_png(source);
    }
    // Sidecars entstehen über normale Edits in je eigener App-Instanz
    // (frisches `open_and_decode` wartet die Hintergrund-Decodierung
    // verlässlich ab — bei wiederverwendeter App wäre `original` schon
    // gesetzt und der zweite Open ein Race).
    for source in &sources {
        let mut setup = new_app();
        open_and_decode(&mut setup, source.display().to_string());
        setup.add_keyword("seed").unwrap();
    }
    let mut app = new_app();
    open_and_decode(&mut app, sources[0].display().to_string());
    for source in &sources {
        app.filmstrip_selection.insert(source.display().to_string());
    }
    let op = parse_metadata_batch_op("add_keyword", "batch").unwrap();
    let report = app.apply_metadata_batch(&op);
    assert_eq!(report.applied_count(), 2);
    assert_eq!(report.failed_count(), 0);
    for source in &sources {
        let document =
            lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(source)).unwrap();
        assert!(document.keywords.contains(&"batch".to_string()));
        assert!(document.keywords.contains(&"seed".to_string()));
    }
    // Rating-Stapel löst die Default-Copy pro Zieldatei auf.
    let rating_op = parse_metadata_batch_op("set_rating", "5").unwrap();
    let report = app.apply_metadata_batch(&rating_op);
    assert_eq!(report.failed_count(), 0);
    // Reload-Anker beider Dateien.
    for source in &sources {
        let mut reopened = new_app();
        open_and_decode(&mut reopened, source.display().to_string());
        assert!(reopened.keywords().contains(&"batch".to_string()));
        assert_eq!(reopened.active_rating_flag().unwrap().0, 5);
    }
    // Sammlungs-Umbenennung als Stapel: alt entfernen + neu setzen.
    let mut app = new_app();
    open_and_decode(&mut app, sources[0].display().to_string());
    for source in &sources {
        app.filmstrip_selection.insert(source.display().to_string());
    }
    let join = parse_metadata_batch_op("add_to_collection", "old=Old").unwrap();
    app.apply_metadata_batch(&join);
    let leave = parse_metadata_batch_op("remove_from_collection", "old").unwrap();
    let report = app.apply_metadata_batch(&leave);
    assert_eq!(report.failed_count(), 0);
    let join_new = parse_metadata_batch_op("add_to_collection", "new=New").unwrap();
    app.apply_metadata_batch(&join_new);
    for source in &sources {
        let document =
            lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(source)).unwrap();
        assert_eq!(
            document.collections,
            vec![CollectionMembership {
                id: "new".to_string(),
                name: "New".to_string(),
            }]
        );
    }
}

/// G-15 Stapel-Fehlerisolation: ein korruptes Sidecar ist ein lauter
/// Pro-Bild-Eintrag und bricht die übrigen Ziele nie ab.
#[test]
fn g15_batch_reports_per_image_failure_without_aborting_rest() {
    let directory = tempfile::tempdir().unwrap();
    let good = directory.path().join("good.png");
    let bad = directory.path().join("bad.png");
    save_png(&good);
    save_png(&bad);
    for target in [&good, &bad] {
        let mut setup = new_app();
        open_and_decode(&mut setup, target.display().to_string());
        setup.add_keyword("seed").unwrap();
    }
    let mut app = new_app();
    open_and_decode(&mut app, good.display().to_string());
    // Sidecar korrumpieren (Original bleibt unberührt).
    std::fs::write(lumina_sidecar::sidecar_path_for(&bad), b"{broken").unwrap();
    app.filmstrip_selection.insert(good.display().to_string());
    app.filmstrip_selection.insert(bad.display().to_string());
    let op = parse_metadata_batch_op("add_keyword", "batch").unwrap();
    let report = app.apply_metadata_batch(&op);
    assert_eq!(report.applied_count(), 1);
    assert_eq!(report.failed_count(), 1);
    assert_eq!(report.failed[0].0, bad.display().to_string());
    assert!(app.error().is_some(), "failure must stay loud");
    let document = lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&good)).unwrap();
    assert!(document.keywords.contains(&"batch".to_string()));
}

/// G-15 leere Auswahl: Fallback auf das geladene Bild (nie stilles
/// No-Op); ohne jedes Bild lauter Hinweis, kein Report, kein Write.
#[test]
fn g15_batch_empty_selection_falls_back_or_stays_loud() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.add_keyword("seed").unwrap();
    app.filmstrip_selection.clear();
    let op = parse_metadata_batch_op("add_keyword", "solo").unwrap();
    let report = app.apply_metadata_batch(&op);
    assert_eq!(report.applied_count(), 1);
    assert_eq!(report.failed_count(), 0);
    assert!(app.keywords().contains(&"solo".to_string()));

    let mut empty = new_app();
    empty.filmstrip_selection.clear();
    let report = empty.apply_metadata_batch(&op);
    assert_eq!(report.applied_count(), 0);
    assert_eq!(report.failed_count(), 0);
    assert_eq!(empty.status(), "No images selected");
}
