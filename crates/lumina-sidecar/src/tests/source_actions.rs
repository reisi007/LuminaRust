use super::*;

#[test]
fn source_actions_empty_list_roundtrips_and_is_absent_when_empty() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.source_actions = vec![];
    let json = d.to_json().unwrap();
    assert!(!json.contains("source_actions"));
    let decoded = SidecarDocument::from_json(&json).unwrap();
    assert!(decoded.virtual_copies[0].recipe.source_actions.is_empty());
}

#[test]
fn source_actions_non_empty_roundtrip_preserves_fields() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.source_actions = vec![source_action_spec(
        SOURCE_ACTION_VERSION,
        SourceActionKind::DustRemoval,
    )];
    let json = d.to_json().unwrap();
    assert!(json.contains("source_actions"));
    let decoded = SidecarDocument::from_json(&json).unwrap();
    assert_eq!(
        decoded.virtual_copies[0].recipe.source_actions,
        vec![source_action_spec(
            SOURCE_ACTION_VERSION,
            SourceActionKind::DustRemoval
        )]
    );
}

#[test]
fn source_actions_absent_key_is_empty_list() {
    let json = r#"{"format":"lumina-sidecar","schema_version":2,"source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"PNG","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","presets":[],"virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{},"mask_library":[],"mask_layers":[],"history":[],"export_records":[]}]}"#;
    let doc = SidecarDocument::from_json(json).unwrap();
    assert!(doc.virtual_copies[0].recipe.source_actions.is_empty());
}

#[test]
fn source_actions_unknown_kind_is_rejected() {
    let json = r#"{"version":1,"kind":"explode","artifact":{"id":"r","relative_path":"a.zdata","checksum":"c"}}"#;
    assert!(serde_json::from_str::<SourceActionSpec>(json).is_err());
}

#[test]
fn source_actions_bad_version_is_rejected() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.source_actions =
        vec![source_action_spec(99, SourceActionKind::DustRemoval)];
    assert!(d.validate().is_err());
}

#[test]
fn source_actions_bad_artifact_ref_is_rejected() {
    // empty id
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.source_actions = vec![SourceActionSpec {
        version: SOURCE_ACTION_VERSION,
        kind: SourceActionKind::AiReplacement,
        artifact: SourceActionArtifactRef {
            id: String::new(),
            relative_path: "a.zdata".into(),
            checksum: "c".into(),
        },
    }];
    assert!(d.validate().is_err());

    // absolute relative_path
    let mut d2 = SidecarDocument::new(source(), "pipeline-1");
    d2.virtual_copies[0].recipe.source_actions = vec![SourceActionSpec {
        version: SOURCE_ACTION_VERSION,
        kind: SourceActionKind::DustRemoval,
        artifact: SourceActionArtifactRef {
            id: "r".into(),
            relative_path: "/abs/a.zdata".into(),
            checksum: "c".into(),
        },
    }];
    assert!(d2.validate().is_err());

    // empty checksum
    let mut d3 = SidecarDocument::new(source(), "pipeline-1");
    d3.virtual_copies[0].recipe.source_actions = vec![SourceActionSpec {
        version: SOURCE_ACTION_VERSION,
        kind: SourceActionKind::DustRemoval,
        artifact: SourceActionArtifactRef {
            id: "r".into(),
            relative_path: "a.zdata".into(),
            checksum: String::new(),
        },
    }];
    assert!(d3.validate().is_err());
}
