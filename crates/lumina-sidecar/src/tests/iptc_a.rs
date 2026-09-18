use super::*;

#[test]
fn iptc_s1_absent_metadata_reads_as_empty_and_serializes_absent() {
    // Legacy JSON without the additive key: absent = empty draft.
    let json = r#"{"format":"lumina-sidecar","schema_version":2,"source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"PNG","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","presets":[],"virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{},"mask_library":[],"mask_layers":[],"history":[],"export_records":[]}]}"#;
    let decoded = SidecarDocument::from_json(json).unwrap();
    assert!(decoded.metadata.is_empty());
    assert_eq!(decoded.metadata.version, METADATA_DRAFT_VERSION);
    assert_eq!(decoded.metadata.latest_rev(), 0);
    // Empty drafts serialize back absent: legacy documents stay byte-stable.
    let reserialized = decoded.to_json().unwrap();
    assert!(
        !reserialized.contains("\"metadata\""),
        "empty draft must serialize absent, got {reserialized}"
    );
    // A fresh document behaves the same.
    assert!(metadata_doc().metadata.is_empty());
    assert!(!metadata_doc().to_json().unwrap().contains("\"metadata\""));
}

#[test]
fn iptc_s1_draft_roundtrip_file() {
    let mut doc = metadata_doc();
    doc.apply_metadata_draft(
        &draft_fields(&[
            ("title", "Startschuss"),
            ("description", "Mehrzeilige … Beschreibung"),
            ("city", "Berlin"),
            ("date_created", "2026-09-04"),
        ]),
        "manual",
        "2026-09-04T09:30:00Z",
    )
    .unwrap();
    doc.apply_metadata_draft(
        &draft_fields(&[("title", "Zieleinlauf"), ("creator", "Fotografin Ü")]),
        "preset:veranstaltung",
        "2026-09-04T10:00:00Z",
    )
    .unwrap();
    // JSON roundtrip is a fixed point, newest entry first.
    let json = doc.to_json().unwrap();
    assert!(json.contains("\"metadata\""));
    let decoded = SidecarDocument::from_json(&json).unwrap();
    assert_eq!(decoded, doc);
    assert_eq!(decoded.metadata.history.len(), 2);
    assert_eq!(decoded.metadata.history[0].rev, 2);
    assert_eq!(decoded.metadata.history[1].rev, 1);
    assert_eq!(decoded.metadata.get("title"), Some("Zieleinlauf"));
    // File roundtrip through the atomic + CAS write path.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("IMG_0001.ARW.lumina.json");
    save_sidecar(&path, &doc).unwrap();
    let reloaded = load_sidecar(&path).unwrap();
    assert_eq!(reloaded, doc);
    let revision = document_revision(&doc).unwrap();
    let saved_revision = save_sidecar_if_unchanged(&path, &reloaded, Some(&revision)).unwrap();
    assert_eq!(saved_revision, revision);
}

#[test]
fn iptc_s1_validation_matrix() {
    // Per-field limits: exactly at the limit passes, one char over fails.
    for (field, limit) in [
        ("title", MAX_METADATA_TITLE_CHARS),
        ("headline", MAX_METADATA_HEADLINE_CHARS),
        ("description", MAX_METADATA_DESCRIPTION_CHARS),
        ("copyright_notice", MAX_METADATA_COPYRIGHT_NOTICE_CHARS),
        ("creator", MAX_METADATA_CREATOR_CHARS),
        ("credit", MAX_METADATA_CREDIT_CHARS),
        ("source", MAX_METADATA_SOURCE_CHARS),
        ("city", MAX_METADATA_CITY_CHARS),
        ("state_province", MAX_METADATA_STATE_PROVINCE_CHARS),
        ("country", MAX_METADATA_COUNTRY_CHARS),
    ] {
        assert_eq!(metadata_field_limit(field), Some(limit));
        validate_metadata_field_value(field, &"x".repeat(limit)).unwrap();
        assert!(
            validate_metadata_field_value(field, &"x".repeat(limit + 1)).is_err(),
            "field `{field}` must reject {} chars",
            limit + 1
        );
        // Multi-byte chars count as chars, not bytes.
        validate_metadata_field_value(field, &"ü".repeat(limit)).unwrap();
        assert!(
            validate_metadata_field_value(field, &"ü".repeat(limit + 1)).is_err(),
            "field `{field}` must count chars, not bytes"
        );
    }
    assert_eq!(metadata_field_limit("date_created"), None);
    // `date_created`: valid dates pass, everything else fails loudly.
    for valid in ["2026-09-04", "2024-02-29", "2000-02-29", "1999-12-31"] {
        validate_metadata_field_value("date_created", valid).unwrap();
    }
    for invalid_date in [
        "2026-9-4",
        "04.09.2026",
        "2026/09/04",
        "2026-09-04T10:00:00Z",
        "2026-13-01",
        "2026-00-10",
        "2026-02-30",
        "2023-02-29",
        "1900-02-29",
        "2026-04-31",
        "not-a-date",
    ] {
        assert!(
            validate_metadata_field_value("date_created", invalid_date).is_err(),
            "`{invalid_date}` must be rejected"
        );
    }
    // Unknown IDs, control characters and untrimmed values fail loudly.
    assert!(validate_metadata_field_value("byline_title", "x").is_err());
    assert!(validate_metadata_field_value("Title", "x").is_err());
    assert!(validate_metadata_field_value("title", "a\tb").is_err());
    assert!(validate_metadata_field_value("title", "a\nb").is_err());
    assert!(validate_metadata_field_value("title", " padded").is_err());
    assert!(validate_metadata_field_value("title", "padded ").is_err());
    // Empty / whitespace-only means "remove" at mutation time.
    validate_metadata_field_value("title", "").unwrap();
    validate_metadata_field_value("title", "   ").unwrap();
    // ...but a *stored* empty/untrimmed value is a loud schema violation.
    let mut doc = metadata_doc();
    doc.metadata.draft.insert("title".into(), "".into());
    assert!(doc.validate().is_err());
    doc.metadata.draft.insert("title".into(), " padded ".into());
    assert!(doc.validate().is_err());
    doc.metadata.draft.insert("mystery".into(), "x".into());
    assert!(doc.validate().is_err());
    // Unsupported metadata version fails loudly, never silently accepted.
    let mut versioned = metadata_doc();
    versioned.metadata.version = METADATA_DRAFT_VERSION + 1;
    assert!(versioned.validate().is_err());
}

#[test]
fn iptc_s1_keywords_routing_rejected() {
    // `keywords` stays the existing source-level field: using it as a
    // draft ID fails loudly with a routing hint, on both paths.
    let error = validate_metadata_field_value("keywords", "festival").unwrap_err();
    assert!(
        matches!(&error, SidecarError::Invalid(message) if message.contains("keywords")),
        "unexpected error: {error}"
    );
    let mut doc = metadata_doc();
    let failed = doc.apply_metadata_draft(
        &draft_fields(&[("keywords", "festival")]),
        "manual",
        "2026-09-04T10:00:00Z",
    );
    assert!(failed.is_err());
    assert!(doc.metadata.is_empty());
    assert!(doc.metadata.history.is_empty());
}

#[test]
fn iptc_s1_origin_timestamp_validation() {
    for valid in [
        "manual",
        "cli",
        "gui",
        "mcp",
        "preset:veranstaltung",
        "preset:a b_c-9",
        "sync:vc-original",
        "sync:copy-42",
    ] {
        validate_metadata_origin(valid).unwrap();
    }
    for bad in [
        "",
        "Manual",
        "MANUAL",
        " manual",
        "manual ",
        "preset:",
        "sync:",
        "preset: name",
        "preset:name ",
        "sync:/abs/path",
        "preset:/abs",
        "mail",
        "user:fred",
        "preset:a\tb",
    ] {
        assert!(
            validate_metadata_origin(bad).is_err(),
            "origin `{bad}` must be rejected"
        );
    }
    for valid in [
        "2026-09-04T10:00:00Z",
        "2026-09-04T10:00:00.123Z",
        "2024-02-29T23:59:59Z",
    ] {
        validate_metadata_timestamp(valid).unwrap();
    }
    for bad in [
        "",
        "yesterday",
        "2026-09-04",
        "2026-09-04T10:00:00",
        "2026-09-04T10:00:00+02:00",
        "2026-09-04 10:00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-09-04T24:00:00Z",
        "2026-09-04T10:00:00.Z",
        "2026-02-30T00:00:00Z",
    ] {
        assert!(
            validate_metadata_timestamp(bad).is_err(),
            "timestamp `{bad}` must be rejected"
        );
    }
    // The std-only constructor always produces valid UTC timestamps.
    let now = now_rfc3339_utc();
    validate_metadata_timestamp(&now).unwrap();
    assert!(now.ends_with('Z'));
    // Bad origin/timestamp reject the whole mutation, all-or-nothing.
    let mut doc = metadata_doc();
    assert!(doc
        .apply_metadata_draft(
            &draft_fields(&[("title", "x")]),
            "carrier-pigeon",
            "2026-09-04T10:00:00Z",
        )
        .is_err());
    assert!(doc
        .apply_metadata_draft(&draft_fields(&[("title", "x")]), "manual", "sometime",)
        .is_err());
    assert!(doc.metadata.is_empty());
}
