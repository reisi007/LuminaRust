use super::*;

#[test]
fn meta_keywords_and_collections_roundtrip() {
    let d = meta_document();
    let json = d.to_json().unwrap();
    assert!(json.contains("landscape"));
    assert!(json.contains("col-best"));
    let decoded = SidecarDocument::from_json(&json).unwrap();
    assert_eq!(decoded, d);
    // Second roundtrip is a fixed point.
    let decoded2 = SidecarDocument::from_json(&decoded.to_json().unwrap()).unwrap();
    assert_eq!(decoded2, d);
}

#[test]
fn meta_legacy_documents_default_to_empty_and_serialize_absent() {
    // Current-schema JSON without the additive keys: absent = empty.
    let json = r#"{"format":"lumina-sidecar","schema_version":2,"source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"PNG","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","presets":[],"virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{},"mask_library":[],"mask_layers":[],"history":[],"export_records":[]}]}"#;
    let doc = SidecarDocument::from_json(json).unwrap();
    assert!(doc.keywords.is_empty());
    assert!(doc.collections.is_empty());
    assert!(doc.validate().is_ok());
    let out = doc.to_json().unwrap();
    assert!(!out.contains("keywords"));
    assert!(!out.contains("\"collections\""));
    // Schema-v1 JSON without the keys behaves identically (no migration
    // needed for additive metadata).
    let v1 = json.replace("\"schema_version\":2", "\"schema_version\":1");
    let doc_v1 = SidecarDocument::from_json(&v1).unwrap();
    assert!(doc_v1.keywords.is_empty());
    assert!(doc_v1.collections.is_empty());
}

#[test]
fn meta_migration_v1_to_v2_preserves_document_without_loss() {
    // The explicit migration path stamps v1 → v2 while every other field
    // (including legacy rating/flag and recipe content) is preserved; the
    // new metadata keys default to empty.
    let mut document = SidecarDocument::new(source(), "pipeline-1");
    document.virtual_copies[0].rating = 3;
    document.virtual_copies[0].flag = Flag::Reject;
    document.virtual_copies[0]
        .recipe
        .adjustments
        .insert("exposure".into(), 1.5);
    let mut legacy: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
    legacy["schema_version"] = Value::from(1);
    let migrated = migrate_json(&serde_json::to_string(&legacy).unwrap()).unwrap();
    let decoded = SidecarDocument::from_json(&migrated).unwrap();
    assert_eq!(decoded.schema_version, SCHEMA_VERSION);
    assert!(decoded.keywords.is_empty());
    assert!(decoded.collections.is_empty());
    assert_eq!(decoded.virtual_copies[0].rating, 3);
    assert_eq!(decoded.virtual_copies[0].flag, Flag::Reject);
    assert_eq!(
        decoded.virtual_copies[0].recipe.adjustments["exposure"],
        1.5
    );
}

#[test]
fn meta_keyword_validation_rejects_loudly() {
    for bad in [
        String::new(),
        "   ".into(),
        " leading".into(),
        "trailing ".into(),
        "with\ttab".into(),
        "with\nnewline".into(),
        "x".repeat(MAX_KEYWORD_CHARS + 1),
    ] {
        let mut d = SidecarDocument::new(source(), "p");
        d.keywords = vec![bad.clone()];
        assert!(
            d.validate().is_err(),
            "keyword `{bad}` must be rejected loudly"
        );
    }
    // Exact duplicates are rejected, not silently deduplicated.
    let mut d = SidecarDocument::new(source(), "p");
    d.keywords = vec!["alps".into(), "alps".into()];
    assert!(d.validate().unwrap_err().to_string().contains("duplicate"));
    // Over-limit list is rejected.
    let mut d = SidecarDocument::new(source(), "p");
    d.keywords = (0..MAX_KEYWORDS_PER_DOCUMENT + 1)
        .map(|i| format!("kw-{i}"))
        .collect();
    assert!(d.validate().is_err());
    // Valid keywords pass.
    assert!(meta_document().validate().is_ok());
}

#[test]
fn meta_collection_validation_rejects_loudly() {
    // Path-like ids must never enter a portable sidecar.
    for bad_id in [
        String::new(),
        " padded ".into(),
        "../outside".into(),
        "/abs/path".into(),
        "a/b".into(),
        r"a\b".into(),
        "c:drive".into(),
    ] {
        let mut d = SidecarDocument::new(source(), "p");
        d.collections = vec![CollectionMembership {
            id: bad_id.clone(),
            name: "Name".into(),
        }];
        assert!(
            d.validate().is_err(),
            "collection id `{bad_id}` must be rejected loudly"
        );
    }
    // Empty/padded names and duplicate ids fail loudly.
    let mut d = SidecarDocument::new(source(), "p");
    d.collections = vec![CollectionMembership {
        id: "a".into(),
        name: String::new(),
    }];
    assert!(d.validate().is_err());
    let mut d = SidecarDocument::new(source(), "p");
    d.collections = vec![
        CollectionMembership {
            id: "a".into(),
            name: "One".into(),
        },
        CollectionMembership {
            id: "a".into(),
            name: "Two".into(),
        },
    ];
    assert!(d.validate().unwrap_err().to_string().contains("duplicate"));
    assert!(meta_document().validate().is_ok());
}

#[test]
fn smart_rule_evaluation_matrix_is_deterministic() {
    let keywords = vec!["alps".to_string(), "night".to_string()];
    // Leaf rules.
    assert!(SmartRule::All.matches(&keywords, 0, Flag::Unflagged));
    assert!(!SmartRule::None.matches(&keywords, 5, Flag::Pick));
    assert!(SmartRule::Keyword {
        keyword: "alps".into()
    }
    .matches(&keywords, 0, Flag::Unflagged));
    assert!(!SmartRule::Keyword {
        keyword: "Alps".into()
    }
    .matches(&keywords, 0, Flag::Unflagged));
    assert!(!SmartRule::Keyword {
        keyword: "sea".into()
    }
    .matches(&keywords, 0, Flag::Unflagged));
    assert!(SmartRule::RatingAtLeast { rating: 3 }.matches(&keywords, 4, Flag::Unflagged));
    assert!(!SmartRule::RatingAtLeast { rating: 5 }.matches(&keywords, 4, Flag::Unflagged));
    assert!(SmartRule::RatingEquals { rating: 4 }.matches(&keywords, 4, Flag::Unflagged));
    assert!(!SmartRule::RatingEquals { rating: 3 }.matches(&keywords, 4, Flag::Unflagged));
    assert!(SmartRule::Flag { flag: Flag::Pick }.matches(&keywords, 0, Flag::Pick));
    assert!(!SmartRule::Flag { flag: Flag::Pick }.matches(&keywords, 0, Flag::Reject));
    // Combinators: "rated picks from the alps, but no rejects".
    let rule = SmartRule::And {
        rules: vec![
            SmartRule::Keyword {
                keyword: "alps".into(),
            },
            SmartRule::Or {
                rules: vec![
                    SmartRule::Flag { flag: Flag::Pick },
                    SmartRule::RatingAtLeast { rating: 4 },
                ],
            },
            SmartRule::Not {
                rule: Box::new(SmartRule::Flag { flag: Flag::Reject }),
            },
        ],
    };
    assert!(rule.matches(&keywords, 4, Flag::Pick));
    assert!(!rule.matches(&keywords, 4, Flag::Reject));
    assert!(!rule.matches(&keywords, 2, Flag::Unflagged));
    assert!(!rule.matches(&["sea".to_string()], 5, Flag::Pick));
}

#[test]
fn smart_collection_matches_copies_and_reports_unknown_ids() {
    let mut d = meta_document();
    d.duplicate_virtual_copy("vc-original", "vc-second", "Second")
        .unwrap();
    d.virtual_copies[1].rating = 1;
    d.virtual_copies[1].flag = Flag::Reject;
    let def = smart_def(SmartRule::And {
        rules: vec![
            SmartRule::Keyword {
                keyword: "landscape".into(),
            },
            SmartRule::RatingAtLeast { rating: 4 },
        ],
    });
    assert!(def.matches_copy(&d, "vc-original").unwrap());
    assert!(!def.matches_copy(&d, "vc-second").unwrap());
    assert!(def.matches_any_copy(&d).unwrap());
    let none = smart_def(SmartRule::Flag { flag: Flag::Pick });
    d.virtual_copies[0].flag = Flag::Unflagged;
    assert!(!none.matches_any_copy(&d).unwrap());
    // Unknown copy ids are loud errors, never silent non-matches.
    assert!(def.matches_copy(&d, "vc-missing").is_err());
}

#[test]
fn smart_collection_definitions_roundtrip_and_validate_loudly() {
    let def = smart_def(SmartRule::Or {
        rules: vec![
            SmartRule::Keyword {
                keyword: "alps".into(),
            },
            SmartRule::Not {
                rule: Box::new(SmartRule::Flag { flag: Flag::Reject }),
            },
        ],
    });
    let json = serde_json::to_string(&def).unwrap();
    let decoded: SmartCollectionDef = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, def);
    assert!(validate_smart_collection_def(&def).is_ok());
    // Bad version.
    let mut bad = def.clone();
    bad.version = 99;
    assert!(validate_smart_collection_def(&bad).is_err());
    // Rating out of range.
    assert!(
        validate_smart_collection_def(&smart_def(SmartRule::RatingAtLeast { rating: 6 })).is_err()
    );
    // Empty And/Or are schema violations, not vacuous truths.
    assert!(validate_smart_collection_def(&smart_def(SmartRule::And { rules: vec![] })).is_err());
    assert!(validate_smart_collection_def(&smart_def(SmartRule::Or { rules: vec![] })).is_err());
    // Empty keyword inside a rule.
    assert!(
        validate_smart_collection_def(&smart_def(SmartRule::Keyword {
            keyword: String::new()
        }))
        .is_err()
    );
    // Excessive nesting is rejected (stack-safe bound).
    let mut deep = SmartRule::All;
    for _ in 0..MAX_SMART_RULE_DEPTH + 2 {
        deep = SmartRule::Not {
            rule: Box::new(deep),
        };
    }
    assert!(validate_smart_collection_def(&smart_def(deep)).is_err());
    // Unknown rule operators fail at parse time, never as silent `None`.
    assert!(serde_json::from_str::<SmartRule>(r#"{"op":"fuzzy"}"#).is_err());
}
