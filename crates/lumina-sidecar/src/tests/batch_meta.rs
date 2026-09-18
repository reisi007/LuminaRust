use super::*;

#[test]
fn batch_ops_apply_idempotently_and_fail_loudly() {
    let mut d = SidecarDocument::new(source(), "p");
    // Add/remove keyword with changed flags.
    assert!(apply_batch_op(
        &mut d,
        &BatchOp::AddKeyword {
            keyword: "alps".into()
        }
    )
    .unwrap());
    assert!(!apply_batch_op(
        &mut d,
        &BatchOp::AddKeyword {
            keyword: "alps".into()
        }
    )
    .unwrap());
    assert_eq!(d.keywords, vec!["alps".to_string()]);
    assert!(!apply_batch_op(
        &mut d,
        &BatchOp::RemoveKeyword {
            keyword: "sea".into()
        }
    )
    .unwrap());
    assert!(apply_batch_op(
        &mut d,
        &BatchOp::RemoveKeyword {
            keyword: "alps".into()
        }
    )
    .unwrap());
    assert!(d.keywords.is_empty());
    // Collections: add, rename propagation, idempotent add, remove.
    assert!(apply_batch_op(
        &mut d,
        &BatchOp::AddToCollection {
            id: "c1".into(),
            name: "One".into()
        }
    )
    .unwrap());
    assert!(!apply_batch_op(
        &mut d,
        &BatchOp::AddToCollection {
            id: "c1".into(),
            name: "One".into()
        }
    )
    .unwrap());
    assert!(apply_batch_op(
        &mut d,
        &BatchOp::AddToCollection {
            id: "c1".into(),
            name: "Uno".into()
        }
    )
    .unwrap());
    assert_eq!(d.collections[0].name, "Uno");
    assert!(!apply_batch_op(
        &mut d,
        &BatchOp::RemoveFromCollection {
            id: "missing".into()
        }
    )
    .unwrap());
    assert!(apply_batch_op(&mut d, &BatchOp::RemoveFromCollection { id: "c1".into() }).unwrap());
    // Rating/flag per copy with changed flags.
    assert!(apply_batch_op(
        &mut d,
        &BatchOp::SetRating {
            copy_id: "vc-original".into(),
            rating: 5
        }
    )
    .unwrap());
    assert!(!apply_batch_op(
        &mut d,
        &BatchOp::SetRating {
            copy_id: "vc-original".into(),
            rating: 5
        }
    )
    .unwrap());
    assert!(apply_batch_op(
        &mut d,
        &BatchOp::SetFlag {
            copy_id: "vc-original".into(),
            flag: Flag::Pick
        }
    )
    .unwrap());
    assert!(d.validate().is_ok());
    // Loud failures leave the document unchanged.
    let before = d.clone();
    assert!(apply_batch_op(
        &mut d,
        &BatchOp::SetRating {
            copy_id: "vc-original".into(),
            rating: 6
        }
    )
    .is_err());
    assert!(apply_batch_op(
        &mut d,
        &BatchOp::SetRating {
            copy_id: "vc-missing".into(),
            rating: 3
        }
    )
    .is_err());
    assert!(apply_batch_op(
        &mut d,
        &BatchOp::SetFlag {
            copy_id: "vc-missing".into(),
            flag: Flag::Pick
        }
    )
    .is_err());
    assert!(apply_batch_op(
        &mut d,
        &BatchOp::AddKeyword {
            keyword: "  padded".into()
        }
    )
    .is_err());
    assert!(apply_batch_op(
        &mut d,
        &BatchOp::AddToCollection {
            id: "/abs".into(),
            name: "X".into()
        }
    )
    .is_err());
    assert_eq!(d, before);
}

#[test]
fn batch_ops_preserve_unrelated_state_and_roundtrip() {
    let mut d = meta_document();
    d.virtual_copies[0]
        .recipe
        .adjustments
        .insert("exposure".into(), 0.75);
    let recipe_before = d.virtual_copies[0].recipe.clone();
    assert!(apply_batch_op(
        &mut d,
        &BatchOp::AddKeyword {
            keyword: "night".into()
        }
    )
    .unwrap());
    assert!(apply_batch_op(
        &mut d,
        &BatchOp::SetFlag {
            copy_id: "vc-original".into(),
            flag: Flag::Reject
        }
    )
    .unwrap());
    // Recipe, masks and history are untouched by metadata batch ops.
    assert_eq!(d.virtual_copies[0].recipe, recipe_before);
    let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(decoded, d);
    // Batch ops themselves are portable data.
    let op = BatchOp::AddKeyword {
        keyword: "alps".into(),
    };
    let json = serde_json::to_string(&op).unwrap();
    assert_eq!(serde_json::from_str::<BatchOp>(&json).unwrap(), op);
}

#[test]
fn meta_file_roundtrip_preserves_metadata_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let document = meta_document();
    save_sidecar(&path, &document).unwrap();
    assert_eq!(load_sidecar(&path).unwrap(), document);
}

/// LRPAR-G01-BASIC: treatment/profile roundtrip — defaults read as
/// `color`/`default`, `bw` stashes and restores, JSON roundtrips.
#[test]
fn g01_treatment_profile_roundtrip() {
    let mut recipe = EditRecipe::default();
    assert_eq!(recipe.treatment(), TREATMENT_COLOR);
    assert_eq!(recipe.develop_profile(), DEFAULT_DEVELOP_PROFILE);
    // Enabling B&W from defaults stashes absence and sets -1.
    assert!(recipe.apply_treatment(TREATMENT_BW).unwrap());
    assert_eq!(recipe.treatment(), TREATMENT_BW);
    assert_eq!(recipe.adjustments.get("saturation"), Some(&-1.0));
    assert_eq!(recipe.adjustments.get("vibrance"), Some(&-1.0));
    // Idempotent re-apply reports no change.
    assert!(!recipe.apply_treatment(TREATMENT_BW).unwrap());
    // A pre-existing saturation survives the roundtrip via the stash.
    let mut recipe2 = EditRecipe::default();
    recipe2.adjustments.insert("saturation".into(), 0.3);
    assert!(recipe2.apply_treatment(TREATMENT_BW).unwrap());
    assert!(recipe2.apply_treatment(TREATMENT_COLOR).unwrap());
    assert_eq!(recipe2.treatment(), TREATMENT_COLOR);
    assert_eq!(recipe2.adjustments.get("saturation"), Some(&0.3));
    assert!(!recipe2.adjustments.contains_key("vibrance"));
    // Absent keys are removed again, never left at -1.
    assert!(recipe.apply_treatment(TREATMENT_COLOR).unwrap());
    assert!(!recipe.adjustments.contains_key("saturation"));
    assert!(!recipe.adjustments.contains_key("vibrance"));
    assert!(!recipe.apply_treatment(TREATMENT_COLOR).unwrap());
    // Profile: default removes the key, others persist.
    assert!(!recipe
        .apply_develop_profile(DEFAULT_DEVELOP_PROFILE)
        .unwrap());
    assert!(recipe.apply_develop_profile("vivid").unwrap());
    assert_eq!(recipe.develop_profile(), "vivid");
    assert!(!recipe.apply_develop_profile("vivid").unwrap());
    assert!(recipe
        .apply_develop_profile(DEFAULT_DEVELOP_PROFILE)
        .unwrap());
    assert!(!recipe.options.contains_key(DEVELOP_PROFILE_KEY));
    // Full JSON roundtrip + validation of the bw state.
    recipe.apply_treatment(TREATMENT_BW).unwrap();
    recipe.apply_develop_profile("portrait").unwrap();
    let json = serde_json::to_value(&recipe).unwrap();
    let decoded: EditRecipe = serde_json::from_value(json).unwrap();
    assert_eq!(decoded, recipe);
    validate_adjustments(&decoded).unwrap();
}

/// LRPAR-G01-BASIC: unknown/empty/corrupt treatment/profile values fail
/// loudly — never a silent normalisation to a default.
#[test]
fn g01_treatment_profile_rejects_invalid() {
    let mut recipe = EditRecipe::default();
    assert!(recipe.apply_treatment("sepia").is_err());
    assert!(recipe.apply_treatment("").is_err());
    assert!(recipe.apply_develop_profile("adobe-color").is_err());
    assert!(recipe.apply_develop_profile("").is_err());
    assert!(recipe.apply_develop_profile("/abs/path").is_err());
    // Crafted invalid states fail validation.
    recipe
        .extras
        .insert(TREATMENT_KEY.into(), Value::String("sepia".into()));
    assert!(validate_adjustments(&recipe).is_err());
    recipe.extras.remove(TREATMENT_KEY);
    recipe
        .extras
        .insert(BW_STASH_KEY.into(), serde_json::json!({"saturation": 99.0}));
    assert!(validate_adjustments(&recipe).is_err());
    recipe
        .extras
        .insert(BW_STASH_KEY.into(), Value::String("corrupt".into()));
    assert!(validate_adjustments(&recipe).is_err());
    recipe.extras.remove(BW_STASH_KEY);
    recipe
        .options
        .insert(DEVELOP_PROFILE_KEY.into(), "unknown".into());
    assert!(validate_adjustments(&recipe).is_err());
    recipe.options.remove(DEVELOP_PROFILE_KEY);
    validate_adjustments(&recipe).unwrap();
}
