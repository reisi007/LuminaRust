use super::*;

/// META-COPYPASTE-1: the clipboard file serializes with the documented
/// format marker/version, and `load_meta_clipboard` rejects structural
/// deviations loudly instead of falling back to an empty clipboard.
#[test]
fn meta_clipboard_file_roundtrip_and_validation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("clipboard.json");
    let clipboard = MetaClipboardFile {
        format: META_CLIPBOARD_FORMAT.to_string(),
        version: META_CLIPBOARD_VERSION,
        source: Some("quelle.png".to_string()),
        fields: BTreeMap::from([("title".to_string(), "Startschuss".to_string())]),
        keywords: vec!["fest".to_string()],
    };
    fs::write(&path, serde_json::to_string_pretty(&clipboard).unwrap()).unwrap();
    let loaded = load_meta_clipboard(&path).unwrap();
    assert_eq!(loaded.format, META_CLIPBOARD_FORMAT);
    assert_eq!(loaded.version, META_CLIPBOARD_VERSION);
    assert_eq!(loaded.source.as_deref(), Some("quelle.png"));
    assert_eq!(
        loaded.fields.get("title").map(String::as_str),
        Some("Startschuss")
    );
    assert_eq!(loaded.keywords, vec!["fest".to_string()]);
    assert_eq!(
        loaded.stored_ids(),
        BTreeSet::from(["keywords".to_string(), "title".to_string()])
    );

    // Wrong format marker → loud.
    fs::write(&path, r#"{"format":"nope","version":1}"#).unwrap();
    assert!(load_meta_clipboard(&path).is_err());

    // Empty value would mean "delete" on paste → loud.
    fs::write(
        &path,
        r#"{"format":"lumina-meta-clipboard","version":1,"fields":{"title":""}}"#,
    )
    .unwrap();
    assert!(load_meta_clipboard(&path).is_err());

    // `keywords` must not hide inside `fields`.
    fs::write(
        &path,
        r#"{"format":"lumina-meta-clipboard","version":1,"fields":{"keywords":"x"}}"#,
    )
    .unwrap();
    assert!(load_meta_clipboard(&path).is_err());
}

/// META-COPYPASTE-2: die defensiven Validierungszweige von
/// `load_meta_clipboard` (truncated JSON, fremde Version, Keyword-Whitespace/
/// -Leere/-Überlänge/-Anzahl) und `parse_meta_fields` (leere IDs) sind
/// laut — kein stiller Fallback, keine Normalisierung.
#[test]
fn meta_clipboard_rejects_defensive_violations() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("clipboard.json");
    let write = |raw: &str| fs::write(&path, raw).unwrap();
    let assert_rejects = |needle: &str| {
        let error = load_meta_clipboard(&path).unwrap_err().to_string();
        assert!(error.contains(needle), "expected `{needle}` in `{error}`");
    };

    // Truncated JSON (abgebrochener Write des geteilten Formats).
    write(r#"{"format":"lumina-meta-clipboard","version":1,"fields":{"title":"Start"#);
    assert_rejects("invalid metadata clipboard");

    // Fremde Version statt stillem Fallback.
    write(r#"{"format":"lumina-meta-clipboard","version":2,"fields":{"title":"X"}}"#);
    assert_rejects("unsupported metadata clipboard version 2");

    // Unbekannte Feld-ID im Clipboard.
    write(r#"{"format":"lumina-meta-clipboard","version":1,"fields":{"nope":"X"}}"#);
    assert_rejects("unknown metadata field `nope`");

    // Keyword mit führendem Whitespace.
    write(r#"{"format":"lumina-meta-clipboard","version":1,"keywords":[" fest"]}"#);
    assert_rejects("leading/trailing whitespace");

    // Leeres Keyword.
    write(r#"{"format":"lumina-meta-clipboard","version":1,"keywords":[""]}"#);
    assert_rejects("leading/trailing whitespace");

    // Keyword-Überlänge.
    let long = "x".repeat(MAX_KEYWORD_CHARS + 1);
    write(&format!(
        r#"{{"format":"lumina-meta-clipboard","version":1,"keywords":["{long}"]}}"#
    ));
    assert_rejects("exceeds limit");

    // Keyword-Anzahl.
    let many = serde_json::json!({
        "format": "lumina-meta-clipboard",
        "version": 1,
        "keywords": vec!["fest"; MAX_KEYWORDS_PER_DOCUMENT + 1]
    });
    write(&serde_json::to_string(&many).unwrap());
    assert_rejects("keyword list exceeds limit");

    // Leere `--fields`-Elemente werden vor jedem Lesezugriff abgelehnt.
    let empty = vec!["".to_string()];
    assert!(parse_meta_fields(&empty, "meta copy").is_err());
    let mixed = vec!["title".to_string(), String::new()];
    assert!(parse_meta_fields(&mixed, "meta paste").is_err());
    // Nichtleere, bekannte IDs bleiben gültig und werden dedupliziert.
    let ok = parse_meta_fields(
        &[
            "title".to_string(),
            "title".to_string(),
            "keywords".to_string(),
        ],
        "meta copy",
    )
    .unwrap();
    assert_eq!(
        ok,
        BTreeSet::from(["keywords".to_string(), "title".to_string()])
    );
}

/// META-COPYPASTE-1: the default clipboard lives in the OS temp directory
/// (explicit/ephemeral, never a CWD dotfile) and keeps a stable name.
#[test]
fn default_meta_clipboard_path_is_in_os_temp() {
    let path = default_meta_clipboard_path();
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("lumina-meta-clipboard.json")
    );
    assert!(path.starts_with(std::env::temp_dir()));
}
