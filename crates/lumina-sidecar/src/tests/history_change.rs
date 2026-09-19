//! UX-LOOK-HISTORY-18: structured history changes — roundtrip, additive
//! legacy behavior and loud rejection of malformed structured data.

use super::*;
use serde_json::json;

fn entry_with_changes(id: &str) -> HistoryEntry {
    let mut entry = HistoryEntry {
        id: id.into(),
        recipe: EditRecipe::default(),
        recorded_at: Some("2026-09-19T10:00:00Z".into()),
        extras: Extras::new(),
    };
    entry
        .set_changes(vec![
            HistoryChange {
                parameter: "exposure".into(),
                from: "0".into(),
                to: "0.5".into(),
            },
            HistoryChange {
                parameter: "contrast".into(),
                from: "0".into(),
                to: "-0.25".into(),
            },
        ])
        .unwrap();
    entry
}

fn document_with_history(entry: HistoryEntry) -> SidecarDocument {
    let mut document = SidecarDocument::new(source(), "pipeline-1");
    document.virtual_copies[0].history.push(entry);
    document
}

/// Builds a raw sidecar string whose history entry carries `changes` verbatim,
/// bypassing `to_json`'s validation (so the loader's rejection can be tested).
fn raw_document_with_changes(id: &str, changes: Value) -> String {
    let mut entry = HistoryEntry {
        id: id.into(),
        recipe: EditRecipe::default(),
        recorded_at: None,
        extras: Extras::new(),
    };
    entry.extras.insert(HISTORY_CHANGES_KEY.into(), changes);
    serde_json::to_string(&document_with_history(entry)).unwrap()
}

#[test]
fn structured_history_changes_roundtrip_through_the_sidecar_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("IMG_0001.ARW.lumina.json");
    let document = document_with_history(entry_with_changes("history-1"));
    save_sidecar(&path, &document).unwrap();

    let loaded = load_sidecar(&path).unwrap();
    assert_eq!(loaded, document);
    let entry = &loaded.virtual_copies[0].history[0];
    assert_eq!(
        entry.changes().unwrap(),
        entry_with_changes("history-1").changes().unwrap()
    );

    // The JSON carries the structured object (name + old→new), not a machine id.
    let json = fs::read_to_string(&path).unwrap();
    assert!(json.contains("\"parameter\": \"exposure\""));
    assert!(json.contains("\"from\": \"0\""));
    assert!(json.contains("\"to\": \"0.5\""));
    assert!(json.contains(&format!("\"{HISTORY_CHANGES_KEY}\"")));
}

#[test]
fn legacy_history_entry_without_changes_loads_and_serializes_absent() {
    let document = document_with_history(HistoryEntry {
        id: "history-legacy".into(),
        recipe: EditRecipe::default(),
        recorded_at: None,
        extras: Extras::new(),
    });
    let json = document.to_json().unwrap();
    assert!(
        !json.contains(&format!("\"{HISTORY_CHANGES_KEY}\"")),
        "a legacy entry must not serialize an empty changes array"
    );
    let decoded = SidecarDocument::from_json(&json).unwrap();
    assert!(decoded.virtual_copies[0].history[0]
        .changes()
        .unwrap()
        .is_empty());
    assert_eq!(decoded, document);
}

#[test]
fn malformed_structured_changes_are_rejected_loudly() {
    // Wrong JSON type (the pre-structured shape must never be interpreted).
    let error =
        SidecarDocument::from_json(&raw_document_with_changes("history-1", json!("exposure")))
            .unwrap_err();
    assert!(matches!(error, SidecarError::Invalid(_)));
    assert!(
        error.to_string().contains(HISTORY_CHANGES_KEY),
        "the loud error must name the offending key, got: {error}"
    );

    // Missing mandatory field.
    assert!(SidecarDocument::from_json(&raw_document_with_changes(
        "history-1",
        json!([{ "parameter": "exposure" }])
    ))
    .is_err());

    // Unknown additional field.
    assert!(SidecarDocument::from_json(&raw_document_with_changes(
        "history-1",
        json!([{ "parameter": "exposure", "from": "0", "to": "1", "extra": true }])
    ))
    .is_err());

    // Empty parameter and control characters.
    assert!(SidecarDocument::from_json(&raw_document_with_changes(
        "history-1",
        json!([{ "parameter": "   ", "from": "0", "to": "1" }])
    ))
    .is_err());
    assert!(SidecarDocument::from_json(&raw_document_with_changes(
        "history-1",
        json!([{ "parameter": "exposure\n", "from": "0", "to": "1" }])
    ))
    .is_err());
}

#[test]
fn set_changes_validates_and_empty_removes_the_key() {
    let mut entry = HistoryEntry {
        id: "history-1".into(),
        recipe: EditRecipe::default(),
        recorded_at: None,
        extras: Extras::new(),
    };
    assert!(entry
        .set_changes(vec![HistoryChange {
            parameter: "  ".into(),
            from: String::new(),
            to: String::new(),
        }])
        .is_err());
    assert!(entry
        .set_changes(vec![HistoryChange {
            parameter: "exposure".into(),
            from: "0".into(),
            to: "1".into(),
        }])
        .is_ok());
    assert!(entry.extras.contains_key(HISTORY_CHANGES_KEY));
    entry.set_changes(Vec::new()).unwrap();
    assert!(!entry.extras.contains_key(HISTORY_CHANGES_KEY));
}

#[test]
fn to_json_revalidates_injected_malformed_changes() {
    // A caller that bypasses `set_changes` and injects a raw value cannot save:
    // `to_json`/`save_sidecar` validate the entry and refuse loudly.
    let mut entry = HistoryEntry {
        id: "history-1".into(),
        recipe: EditRecipe::default(),
        recorded_at: None,
        extras: Extras::new(),
    };
    entry
        .extras
        .insert(HISTORY_CHANGES_KEY.into(), json!("not-a-list"));
    let document = document_with_history(entry);
    assert!(document.to_json().is_err());
}

#[test]
fn unsupported_schema_versions_stay_loudly_rejected() {
    let document = SidecarDocument::new(source(), "pipeline-1");
    let mut value: Value = serde_json::to_value(&document).unwrap();
    value["schema_version"] = Value::from(0);
    let v0 = serde_json::to_string(&value).unwrap();
    let error = SidecarDocument::from_json(&v0).unwrap_err();
    assert!(error.to_string().contains("migration"), "got: {error}");

    value["schema_version"] = Value::from(999);
    let future = serde_json::to_string(&value).unwrap();
    assert!(SidecarDocument::from_json(&future).is_err());

    // The explicit migration path still lifts the historical v1 shape.
    value["schema_version"] = Value::from(1);
    let v1 = serde_json::to_string(&value).unwrap();
    let migrated = migrate_json(&v1).unwrap();
    assert!(migrated.contains("\"schema_version\": 2"));
}
