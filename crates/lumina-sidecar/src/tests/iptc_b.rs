use super::*;

#[test]
fn iptc_s1_apply_all_or_nothing_and_idempotent() {
    let mut doc = metadata_doc();
    doc.apply_metadata_draft(
        &draft_fields(&[("title", "Start")]),
        "manual",
        "2026-09-04T09:00:00Z",
    )
    .unwrap();
    // One invalid field among valid ones rejects everything.
    let failed = doc.apply_metadata_draft(
        &draft_fields(&[("city", "Berlin"), ("nope", "x")]),
        "manual",
        "2026-09-04T10:00:00Z",
    );
    assert!(failed.is_err());
    assert_eq!(doc.metadata.get("city"), None);
    assert_eq!(doc.metadata.history.len(), 1);
    assert_eq!(doc.metadata.latest_rev(), 1);
    // Idempotent re-application: no change, no history entry.
    assert!(!doc
        .apply_metadata_draft(
            &draft_fields(&[("title", "Start")]),
            "manual",
            "2026-09-04T11:00:00Z",
        )
        .unwrap());
    assert_eq!(doc.metadata.history.len(), 1);
    // Empty call: no change, no entry.
    assert!(!doc
        .apply_metadata_draft(&BTreeMap::new(), "manual", "2026-09-04T11:00:00Z")
        .unwrap());
    assert_eq!(doc.metadata.history.len(), 1);
    // A real change records exactly one entry with the affected IDs.
    assert!(doc
        .apply_metadata_draft(
            &draft_fields(&[("title", "Ziel"), ("city", "Berlin")]),
            "cli",
            "2026-09-04T12:00:00Z",
        )
        .unwrap());
    assert_eq!(doc.metadata.history.len(), 2);
    let entry = &doc.metadata.history[0];
    assert_eq!(entry.rev, 2);
    assert_eq!(entry.origin, "cli");
    assert_eq!(entry.timestamp, "2026-09-04T12:00:00Z");
    assert_eq!(entry.changed, vec!["city".to_string(), "title".to_string()]);
}

#[test]
fn iptc_s1_empty_value_removes_field() {
    let mut doc = metadata_doc();
    doc.apply_metadata_draft(
        &draft_fields(&[("title", "Start"), ("city", "Berlin")]),
        "manual",
        "2026-09-04T09:00:00Z",
    )
    .unwrap();
    // Empty and whitespace-only both remove; unaffected fields are not
    // listed in `changed`.
    assert!(doc
        .apply_metadata_draft(
            &draft_fields(&[("title", ""), ("city", "   ")]),
            "gui",
            "2026-09-04T10:00:00Z",
        )
        .unwrap());
    assert!(doc.metadata.draft.is_empty());
    assert_eq!(
        doc.metadata.history[0].changed,
        vec!["city".to_string(), "title".to_string()]
    );
    // Removing an absent field is an idempotent no-op (no entry).
    assert!(!doc
        .apply_metadata_draft(
            &draft_fields(&[("title", "")]),
            "gui",
            "2026-09-04T11:00:00Z",
        )
        .unwrap());
    assert_eq!(doc.metadata.history.len(), 2);
}

#[test]
fn iptc_s1_clear_semantics() {
    let mut doc = metadata_doc();
    // Clearing an empty draft touches neither draft nor history.
    assert!(!doc
        .clear_metadata_draft("manual", "2026-09-04T09:00:00Z")
        .unwrap());
    assert!(doc.metadata.history.is_empty());
    assert!(!doc
        .clear_metadata_fields(&["title"], "manual", "2026-09-04T09:00:00Z")
        .unwrap());
    assert!(doc.metadata.history.is_empty());
    doc.apply_metadata_draft(
        &draft_fields(&[("title", "Start"), ("city", "Berlin")]),
        "manual",
        "2026-09-04T09:00:00Z",
    )
    .unwrap();
    // Selective clear: one entry, history kept.
    assert!(doc
        .clear_metadata_fields(&["title", "headline"], "mcp", "2026-09-04T10:00:00Z")
        .unwrap());
    assert_eq!(doc.metadata.get("title"), None);
    assert_eq!(doc.metadata.get("city"), Some("Berlin"));
    assert_eq!(doc.metadata.history[0].changed, vec!["title".to_string()]);
    assert_eq!(doc.metadata.history[0].origin, "mcp");
    // `--all`: draft emptied, history kept and extended.
    assert!(doc
        .clear_metadata_draft("cli", "2026-09-04T11:00:00Z")
        .unwrap());
    assert!(doc.metadata.draft.is_empty());
    assert_eq!(doc.metadata.history.len(), 3);
    assert_eq!(doc.metadata.history[0].changed, vec!["city".to_string()]);
    // History clear is explicit and total; draft values survive it.
    doc.apply_metadata_draft(
        &draft_fields(&[("title", "Neu")]),
        "manual",
        "2026-09-04T12:00:00Z",
    )
    .unwrap();
    doc.clear_metadata_history();
    assert!(doc.metadata.history.is_empty());
    assert_eq!(doc.metadata.get("title"), Some("Neu"));
    doc.clear_metadata_history();
    assert!(doc.metadata.history.is_empty());
    // Unknown IDs fail loudly on the clear paths too.
    assert!(doc
        .clear_metadata_fields(&["mystery"], "manual", "2026-09-04T13:00:00Z")
        .is_err());
    assert!(doc.metadata.history.is_empty());
}

#[test]
fn iptc_s1_history_cap_and_rev_monotonic() {
    let mut doc = metadata_doc();
    for index in 1..=(MAX_METADATA_HISTORY_ENTRIES + 5) {
        let value = format!("Titel {index}");
        assert!(doc
            .apply_metadata_draft(
                &draft_fields(&[("title", value.as_str())]),
                "manual",
                "2026-09-04T10:00:00Z",
            )
            .unwrap());
    }
    // Cap is enforced FIFO-deterministically: oldest fall off the end.
    assert_eq!(doc.metadata.history.len(), MAX_METADATA_HISTORY_ENTRIES);
    assert_eq!(
        doc.metadata.history[0].rev as usize,
        MAX_METADATA_HISTORY_ENTRIES + 5
    );
    assert_eq!(doc.metadata.history[0].changed, vec!["title".to_string()]);
    assert_eq!(
        doc.metadata.latest_rev() as usize,
        MAX_METADATA_HISTORY_ENTRIES + 5
    );
    let last = doc.metadata.history.last().unwrap();
    assert_eq!(last.rev, 6);
    let mut previous = u64::MAX;
    for entry in &doc.metadata.history {
        assert!(entry.rev < previous, "revs must strictly decrease");
        previous = entry.rev;
    }
    assert_eq!(
        doc.metadata.get("title"),
        Some(format!("Titel {}", MAX_METADATA_HISTORY_ENTRIES + 5).as_str())
    );
    doc.validate().unwrap();
    // Hand-crafted violations fail loudly instead of being normalized.
    let mut ascending = metadata_doc();
    ascending.metadata.history = vec![
        MetadataHistoryEntry {
            rev: 1,
            timestamp: "2026-09-04T09:00:00Z".into(),
            origin: "manual".into(),
            changed: vec!["title".into()],
        },
        MetadataHistoryEntry {
            rev: 2,
            timestamp: "2026-09-04T10:00:00Z".into(),
            origin: "manual".into(),
            changed: vec!["title".into()],
        },
    ];
    assert!(ascending.validate().is_err());
    let mut overlong = metadata_doc();
    overlong.metadata.history = (1..=(MAX_METADATA_HISTORY_ENTRIES + 1) as u64)
        .rev()
        .map(|rev| MetadataHistoryEntry {
            rev,
            timestamp: "2026-09-04T10:00:00Z".into(),
            origin: "manual".into(),
            changed: vec!["title".into()],
        })
        .collect();
    assert!(overlong.validate().is_err());
    let mut bad_changed = metadata_doc();
    bad_changed.metadata.history = vec![MetadataHistoryEntry {
        rev: 1,
        timestamp: "2026-09-04T10:00:00Z".into(),
        origin: "manual".into(),
        changed: vec!["mystery".into()],
    }];
    assert!(bad_changed.validate().is_err());
    bad_changed.metadata.history[0].changed = vec![];
    assert!(bad_changed.validate().is_err());
    bad_changed.metadata.history[0].changed = vec!["title".into(), "title".into()];
    assert!(bad_changed.validate().is_err());
    // `keywords` is a legal `changed` ID (sync carries it); rev 0 is not.
    bad_changed.metadata.history[0].changed = vec!["keywords".into(), "title".into()];
    bad_changed.validate().unwrap();
    bad_changed.metadata.history[0].rev = 0;
    assert!(bad_changed.validate().is_err());
}

#[test]
fn iptc_s1_cas_conflict_on_concurrent_metadata_edit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("IMG_0001.ARW.lumina.json");
    let mut doc = metadata_doc();
    doc.apply_metadata_draft(
        &draft_fields(&[("title", "Start")]),
        "manual",
        "2026-09-04T09:00:00Z",
    )
    .unwrap();
    save_sidecar(&path, &doc).unwrap();
    let stale_revision = document_revision(&load_sidecar(&path).unwrap()).unwrap();
    // A concurrent writer moves the file forward...
    let mut concurrent = load_sidecar(&path).unwrap();
    concurrent
        .apply_metadata_draft(
            &draft_fields(&[("city", "Berlin")]),
            "sync:other",
            "2026-09-04T10:00:00Z",
        )
        .unwrap();
    save_sidecar(&path, &concurrent).unwrap();
    // ...so the stale revision conflicts loudly instead of last-write-wins.
    let mut stale = load_sidecar(&path).unwrap();
    stale
        .apply_metadata_draft(
            &draft_fields(&[("title", "Stale")]),
            "manual",
            "2026-09-04T11:00:00Z",
        )
        .unwrap();
    let conflict = save_sidecar_if_unchanged(&path, &stale, Some(&stale_revision));
    assert!(
        matches!(&conflict, Err(SidecarError::Conflict(_))),
        "stale CAS save must conflict, got {conflict:?}"
    );
    // The concurrent edit survived; the stale one was not persisted.
    let current = load_sidecar(&path).unwrap();
    assert_eq!(current.metadata.get("city"), Some("Berlin"));
    assert_eq!(current.metadata.get("title"), Some("Start"));
}

#[test]
fn iptc_s1_mutations_leave_recipes_masks_and_copy_history_untouched() {
    let mut doc = metadata_doc();
    doc.keywords = vec!["alps".into()];
    let recipe_before = serde_json::to_value(&doc.virtual_copies[0].recipe).unwrap();
    let history_before = doc.virtual_copies[0].history.clone();
    doc.apply_metadata_draft(
        &draft_fields(&[("title", "Start"), ("date_created", "2026-09-04")]),
        "preset:veranstaltung",
        "2026-09-04T10:00:00Z",
    )
    .unwrap();
    doc.clear_metadata_fields(&["title"], "sync:copy-7", "2026-09-04T11:00:00Z")
        .unwrap();
    doc.clear_metadata_draft("gui", "2026-09-04T12:00:00Z")
        .unwrap();
    doc.clear_metadata_history();
    assert_eq!(
        serde_json::to_value(&doc.virtual_copies[0].recipe).unwrap(),
        recipe_before
    );
    assert_eq!(doc.virtual_copies[0].history, history_before);
    assert!(doc.virtual_copies[0].mask_layers.is_empty());
    assert!(doc.virtual_copies[0].mask_library.is_empty());
    assert_eq!(doc.keywords, vec!["alps".to_string()]);
    doc.validate().unwrap();
}
