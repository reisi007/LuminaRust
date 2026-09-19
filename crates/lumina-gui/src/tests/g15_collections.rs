//! G-15 keywords/collections/smart-catalog tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

// ---- G-15 META-MVP Slice 3 (GUI): keywords, extended filter,
// collections, smart catalog, batch (DoD §1: Edit→Commit→Datei→Reload) ----

/// G-15 E2E-Anker (DoD §1): keyword vergeben → Sidecar-Datei →
/// Reload → wiederhergestellt; entfernen dito; ungültige Keywords
/// scheitern laut ohne Dateiänderung.
#[test]
fn g15_keyword_add_remove_roundtrip_reload() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    assert!(app.keywords().is_empty());

    assert!(app.add_keyword("portrait").unwrap());
    // Idempotent: zweites Hinzufügen ändert nichts, bleibt Ok.
    assert!(!app.add_keyword("portrait").unwrap());
    let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    assert_eq!(document.keywords, vec!["portrait".to_string()]);

    // Ungültig (leer / führende Whitespaces) scheitert laut; die Datei
    // bleibt unberührt.
    assert!(app.add_keyword("").is_err());
    assert!(app.add_keyword(" leading").is_err());
    let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    assert_eq!(document.keywords, vec!["portrait".to_string()]);

    // Reload-Anker: ein neuer App-Lauf sieht das Keyword.
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert_eq!(reopened.keywords(), vec!["portrait".to_string()]);
    assert!(reopened.remove_keyword("portrait").unwrap());
    assert!(!reopened.remove_keyword("portrait").unwrap());
    let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    assert!(document.keywords.is_empty());

    let mut reread = new_app();
    open_and_decode(&mut reread, source.display().to_string());
    assert!(reread.keywords().is_empty());
}

/// G-15 E2E-Anker (DoD §1): Sammlung beitreten/verlassen →
/// Sidecar-Datei → Reload; `id=name`-Kurzform; ungültige Ids laut.
#[test]
fn g15_collection_join_leave_roundtrip_reload() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    assert!(app.collections().is_empty());

    assert!(app.add_to_collection("best", "Best Of").unwrap());
    // Gleiche Id mit neuem Namen = Umbenennung (changed).
    assert!(app.add_to_collection("best", "Best Of 2026").unwrap());
    let document = lumina_sidecar::load_sidecar(&sidecar).unwrap();
    assert_eq!(
        document.collections,
        vec![CollectionMembership {
            id: "best".to_string(),
            name: "Best Of 2026".to_string(),
        }]
    );
    // Ungültige Id (pfadartig) scheitert laut.
    assert!(app.add_to_collection("a/b", "Bad").is_err());

    // `id=name`-Kurzform des Panels.
    let (id, name) = LuminaApp::split_collection_assignment("sel=Selection").unwrap();
    assert_eq!((id.as_str(), name.as_str()), ("sel", "Selection"));
    assert!(LuminaApp::split_collection_assignment("no-equals").is_err());

    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert_eq!(
        reopened.collections(),
        vec![CollectionMembership {
            id: "best".to_string(),
            name: "Best Of 2026".to_string(),
        }]
    );
    assert!(reopened.remove_from_collection("best").unwrap());
    assert!(!reopened.remove_from_collection("best").unwrap());
    let mut reread = new_app();
    open_and_decode(&mut reread, source.display().to_string());
    assert!(reread.collections().is_empty());
}

/// G-15 Filter-Klasse (DoD §3, vollständig): alle Präfixe plus
/// UND-Verknüpfung, unparsebar→leer, Keyword-Case-Sensitivität.
#[test]
fn g15_extended_filter_predicates_all_prefixes() {
    let entry = filter_entry();
    // Leere Query matcht alles.
    assert!(library_entry_matches(&entry, ""));
    assert!(library_entry_matches(&entry, "   "));
    // Name (alt) + UND-Verknüpfung.
    assert!(library_entry_matches(&entry, "a.png rating:4"));
    assert!(!library_entry_matches(&entry, "a.png rating:5"));
    assert!(!library_entry_matches(&entry, "zzz"));
    // keyword: exakt + case-sensitiv (Slice-1-Semantik).
    assert!(library_entry_matches(&entry, "keyword:portrait"));
    assert!(!library_entry_matches(&entry, "keyword:Portrait"));
    assert!(library_entry_matches(&entry, "keyword:Studio"));
    assert!(!library_entry_matches(&entry, "keyword:landscape"));
    // collection: Id oder Name (case-insensitiv).
    assert!(library_entry_matches(&entry, "collection:best"));
    assert!(library_entry_matches(&entry, "collection:BEST"));
    assert!(library_entry_matches(&entry, "collection:best of"));
    assert!(!library_entry_matches(&entry, "collection:other"));
    assert!(!library_entry_matches(&entry, "collection:"));
    // camera: Teilvergleich, case-insensitiv.
    assert!(library_entry_matches(&entry, "camera:canon"));
    assert!(library_entry_matches(&entry, "camera:EOS R5"));
    assert!(!library_entry_matches(&entry, "camera:nikon"));
    assert!(!library_entry_matches(&entry, "camera:"));
    // iso:/focal: numerisch exakt; unparsebar matcht nichts.
    assert!(library_entry_matches(&entry, "iso:400"));
    assert!(!library_entry_matches(&entry, "iso:800"));
    assert!(!library_entry_matches(&entry, "iso:fast"));
    assert!(library_entry_matches(&entry, "focal:50"));
    assert!(library_entry_matches(&entry, "focal_length:50"));
    assert!(!library_entry_matches(&entry, "focal:85"));
    assert!(!library_entry_matches(&entry, "focal:wide"));
    // UND über erweiterte Prädikate.
    assert!(library_entry_matches(
        &entry,
        "keyword:portrait camera:canon iso:400 focal:50 rating:4 flag:pick label:red collection:best"
    ));
    assert!(!library_entry_matches(
        &entry,
        "keyword:portrait camera:nikon"
    ));
    // Fehlende EXIF-Daten matchen nichts (kein stilles Pass-Through).
    let mut no_exif = filter_entry();
    no_exif.camera = None;
    no_exif.iso = None;
    no_exif.focal_length = None;
    assert!(!library_entry_matches(&no_exif, "camera:canon"));
    assert!(!library_entry_matches(&no_exif, "iso:400"));
    assert!(!library_entry_matches(&no_exif, "focal:50"));
    assert!(library_entry_matches(&no_exif, "keyword:portrait"));
    // Die alte Overload bleibt kompatibel: erweiterte Präfixe ohne
    // Eintragsdaten matchen nichts (statt Datei zu suchen).
    assert!(!library_filter_matches(
        "a.png",
        4,
        Flag::Pick,
        1,
        "keyword:portrait"
    ));
    assert!(library_filter_matches(
        "a.png",
        4,
        Flag::Pick,
        1,
        "a.png rating:4"
    ));
}

/// G-15 Sammlungsfilter-Klasse: statisch per Mitglieds-`id`, smart per
/// Regel über Keywords + Default-Copy-Rating/Flag; unbekannte Ids und
/// invalide Definitionen matchen nichts.
#[test]
fn g15_collection_filter_static_and_smart() {
    let entry = filter_entry();
    assert!(collection_filter_matches_entry(
        &entry,
        &CollectionFilter::Static {
            id: "best".to_string()
        },
        &[]
    ));
    assert!(!collection_filter_matches_entry(
        &entry,
        &CollectionFilter::Static {
            id: "other".to_string()
        },
        &[]
    ));
    // Name filtert nicht — nur die stabile Id.
    assert!(!collection_filter_matches_entry(
        &entry,
        &CollectionFilter::Static {
            id: "Best Of".to_string()
        },
        &[]
    ));
    let catalog = vec![SmartCollectionDef {
        version: SMART_COLLECTION_VERSION,
        id: "smart-best".to_string(),
        name: "Best portraits".to_string(),
        rule: SmartRule::And {
            rules: vec![
                SmartRule::Keyword {
                    keyword: "portrait".to_string(),
                },
                SmartRule::RatingAtLeast { rating: 4 },
            ],
        },
    }];
    assert!(collection_filter_matches_entry(
        &entry,
        &CollectionFilter::Smart {
            id: "smart-best".to_string()
        },
        &catalog
    ));
    // Unbekannte Smart-Id matcht nichts.
    assert!(!collection_filter_matches_entry(
        &entry,
        &CollectionFilter::Smart {
            id: "ghost".to_string()
        },
        &catalog
    ));
    // Invalide Definition (falsche Version) matcht nichts.
    let mut bad = catalog.clone();
    bad[0].version = 99;
    assert!(!collection_filter_matches_entry(
        &entry,
        &CollectionFilter::Smart {
            id: "smart-best".to_string()
        },
        &bad
    ));
    // Aggregation: statische Liste aus dem Scan (Seitenname + Zähler).
    let directory = tempfile::tempdir().unwrap();
    for name in ["a.png", "b.png"] {
        let source = directory.path().join(name);
        save_png(&source);
    }
    let mut app = new_app();
    open_and_decode(
        &mut app,
        directory.path().join("a.png").display().to_string(),
    );
    app.add_to_collection("best", "Best Of").unwrap();
    app.set_directory(directory.path().display().to_string());
    let aggregated = app.static_collections();
    assert_eq!(aggregated.len(), 1);
    assert_eq!(aggregated[0].0, "best");
    assert_eq!(aggregated[0].2, 1);
}

/// G-15 reine Builder (DoD §3): alle Op-Varianten + alle Regelarten +
/// Kombinatoren, Fehlerfälle laut.
#[test]
fn g15_batch_op_and_rule_builders_cover_all_variants() {
    // Alle sechs BatchOp-Varianten parsen.
    assert_eq!(
        parse_metadata_batch_op("add_keyword", "portrait").unwrap(),
        BatchOp::AddKeyword {
            keyword: "portrait".to_string()
        }
    );
    assert_eq!(
        parse_metadata_batch_op("remove_keyword", "portrait").unwrap(),
        BatchOp::RemoveKeyword {
            keyword: "portrait".to_string()
        }
    );
    assert_eq!(
        parse_metadata_batch_op("add_to_collection", "best=Best Of").unwrap(),
        BatchOp::AddToCollection {
            id: "best".to_string(),
            name: "Best Of".to_string(),
        }
    );
    assert_eq!(
        parse_metadata_batch_op("remove_from_collection", "best").unwrap(),
        BatchOp::RemoveFromCollection {
            id: "best".to_string()
        }
    );
    // set_rating/set_flag tragen leere copy_id (Auflösung pro Zieldatei
    // in `apply_metadata_batch`).
    assert!(matches!(
        parse_metadata_batch_op("set_rating", "4").unwrap(),
        BatchOp::SetRating { rating: 4, .. }
    ));
    assert!(matches!(
        parse_metadata_batch_op("set_flag", "pick").unwrap(),
        BatchOp::SetFlag {
            flag: Flag::Pick,
            ..
        }
    ));
    // Fehlerfälle: laut, nie still.
    assert!(parse_metadata_batch_op("bogus", "x").is_err());
    assert!(parse_metadata_batch_op("add_to_collection", "no-equals").is_err());
    assert!(parse_metadata_batch_op("set_rating", "6").is_err());
    assert!(parse_metadata_batch_op("set_rating", "high").is_err());
    assert!(parse_metadata_batch_op("set_flag", "maybe").is_err());
    // Alle Regelarten bauen.
    assert_eq!(build_smart_rule("all", "").unwrap(), SmartRule::All);
    assert_eq!(build_smart_rule("none", "").unwrap(), SmartRule::None);
    assert_eq!(
        build_smart_rule("keyword", "portrait").unwrap(),
        SmartRule::Keyword {
            keyword: "portrait".to_string()
        }
    );
    assert_eq!(
        build_smart_rule("rating_at_least", "3").unwrap(),
        SmartRule::RatingAtLeast { rating: 3 }
    );
    assert_eq!(
        build_smart_rule("rating_equals", "5").unwrap(),
        SmartRule::RatingEquals { rating: 5 }
    );
    assert_eq!(
        build_smart_rule("flag", "reject").unwrap(),
        SmartRule::Flag { flag: Flag::Reject }
    );
    assert!(build_smart_rule("bogus", "x").is_err());
    assert!(build_smart_rule("rating_at_least", "9").is_err());
    assert!(build_smart_rule("flag", "maybe").is_err());
    // Kombinatoren: and/or brauchen ≥2, not ≥1.
    let mut stack = vec![SmartRule::All, SmartRule::None];
    combine_smart_rules(&mut stack, "and").unwrap();
    assert_eq!(
        stack,
        vec![SmartRule::And {
            rules: vec![SmartRule::All, SmartRule::None]
        }]
    );
    combine_smart_rules(&mut stack, "not").unwrap();
    assert!(matches!(stack.as_slice(), [SmartRule::Not { .. }]));
    let mut short = vec![SmartRule::All];
    assert!(combine_smart_rules(&mut short, "or").is_err());
    let mut empty: Vec<SmartRule> = Vec::new();
    assert!(combine_smart_rules(&mut empty, "not").is_err());
    assert!(combine_smart_rules(&mut empty, "xor").is_err());
}
