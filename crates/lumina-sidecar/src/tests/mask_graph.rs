use super::*;

#[test]
fn mask_cycles_and_invalid_targets_are_rejected() {
    let mut d = SidecarDocument::new(source(), "p");
    let mut a = mask("a");
    let mut b = mask("b");
    a.operation = MaskOperation::Invert;
    b.operation = MaskOperation::Invert;
    a.references.push(MaskReference {
        copy_id: "vc-original".into(),
        mask_id: "b".into(),
        extras: Extras::new(),
    });
    b.references.push(MaskReference {
        copy_id: "vc-original".into(),
        mask_id: "a".into(),
        extras: Extras::new(),
    });
    d.virtual_copies[0].mask_library = vec![a, b];
    let error = d.validate().unwrap_err().to_string();
    assert!(error.contains("cycle"));
    d.virtual_copies[0].mask_library[0].references[0].mask_id = "missing".into();
    assert!(d
        .validate()
        .unwrap_err()
        .to_string()
        .contains("unknown mask"));
    d.virtual_copies[0].mask_library[0].references.clear();
    d.virtual_copies[0].mask_library[0].operation = MaskOperation::Source;
    d.virtual_copies[0].mask_layers.push(MaskLayer {
        id: "layer".into(),
        mask: MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "missing".into(),
            extras: Extras::new(),
        },
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        extras: Extras::new(),
        visible: true,
        local_adjustments: None,
    });
    assert!(d.validate().unwrap_err().to_string().contains("mask layer"));
}

#[test]
fn valid_cross_copy_mask_reference_is_accepted() {
    let mut d = SidecarDocument::new(source(), "p");
    d.virtual_copies[0].mask_library.push(mask("source-mask"));
    d.virtual_copies.push(VirtualCopy {
        id: "vc-target".into(),
        name: "Target".into(),
        is_default: false,
        rating: 0,
        flag: Flag::Unflagged,
        recipe: EditRecipe::default(),
        mask_library: vec![MaskDefinition {
            operation: MaskOperation::Invert,
            references: vec![MaskReference {
                copy_id: "vc-original".into(),
                mask_id: "source-mask".into(),
                extras: Extras::new(),
            }],
            ..mask("derived")
        }],
        mask_layers: vec![],
        history: vec![],
        export_records: vec![],
        extras: Extras::new(),
    });
    assert!(d.validate().is_ok());
}

#[test]
fn cross_copy_mask_cycle_is_rejected() {
    let mut d = SidecarDocument::new(source(), "p");
    d.virtual_copies[0].mask_library.push(MaskDefinition {
        operation: MaskOperation::Invert,
        references: vec![MaskReference {
            copy_id: "vc-target".into(),
            mask_id: "target-mask".into(),
            extras: Extras::new(),
        }],
        ..mask("source-mask")
    });
    d.virtual_copies.push(VirtualCopy {
        id: "vc-target".into(),
        name: "Target".into(),
        is_default: false,
        rating: 0,
        flag: Flag::Unflagged,
        recipe: EditRecipe::default(),
        mask_library: vec![MaskDefinition {
            operation: MaskOperation::Invert,
            references: vec![MaskReference {
                copy_id: "vc-original".into(),
                mask_id: "source-mask".into(),
                extras: Extras::new(),
            }],
            ..mask("target-mask")
        }],
        mask_layers: vec![],
        history: vec![],
        export_records: vec![],
        extras: Extras::new(),
    });
    assert!(d.validate().unwrap_err().to_string().contains("cycle"));
}

#[test]
fn direct_mask_self_reference_is_rejected() {
    let mut d = SidecarDocument::new(source(), "p");
    d.virtual_copies[0].mask_library.push(MaskDefinition {
        operation: MaskOperation::Invert,
        references: vec![MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "self".into(),
            extras: Extras::new(),
        }],
        ..mask("self")
    });
    let error = d.validate().unwrap_err().to_string();
    assert!(error.contains("references itself"));
}

#[test]
fn mask_identity_fields_roundtrip() {
    let mut d = SidecarDocument::new(source(), "p");
    let mut definition = mask("identity");
    definition.rescaling_method = "lanczos".into();
    definition
        .rescaling_parameters
        .insert("filter_radius".into(), "3".into());
    definition.generator_version = "segmenter-2.4".into();
    d.virtual_copies[0].mask_library.push(definition);
    let json = d.to_json().unwrap();
    assert!(json.contains("rescaling_method"));
    assert!(json.contains("generator_version"));
    assert_eq!(d, SidecarDocument::from_json(&json).unwrap());
}

#[test]
fn collection_ids_must_be_nonempty_and_unique() {
    let mut d = SidecarDocument::new(source(), "p");
    d.presets = vec![
        Preset {
            id: "preset".into(),
            name: "One".into(),
            recipe: EditRecipe::default(),
            extras: Extras::new(),
        },
        Preset {
            id: "preset".into(),
            name: "Two".into(),
            recipe: EditRecipe::default(),
            extras: Extras::new(),
        },
    ];
    assert!(d
        .validate()
        .unwrap_err()
        .to_string()
        .contains("duplicate preset id"));

    d.presets.clear();
    d.virtual_copies[0].mask_library.push(mask("layer-mask"));
    let layer = MaskLayer {
        id: "layer".into(),
        mask: MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "layer-mask".into(),
            extras: Extras::new(),
        },
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        extras: Extras::new(),
        visible: true,
        local_adjustments: None,
    };
    d.virtual_copies[0].mask_layers = vec![layer.clone(), layer];
    assert!(d
        .validate()
        .unwrap_err()
        .to_string()
        .contains("duplicate mask layer id"));

    d.virtual_copies[0].mask_layers.clear();
    let history = HistoryEntry {
        id: "history".into(),
        recipe: EditRecipe::default(),
        recorded_at: None,
        extras: Extras::new(),
    };
    d.virtual_copies[0].history = vec![history.clone(), history];
    assert!(d
        .validate()
        .unwrap_err()
        .to_string()
        .contains("duplicate history entry id"));

    d.virtual_copies[0].history.clear();
    let export = ExportRecord {
        id: "export".into(),
        relative_path: "exports/out.jpg".into(),
        format: "jpeg".into(),
        exported_at: None,
        extras: Extras::new(),
    };
    d.virtual_copies[0].export_records = vec![export.clone(), export];
    assert!(d
        .validate()
        .unwrap_err()
        .to_string()
        .contains("duplicate export record id"));
}

#[test]
fn ids_must_be_nonempty() {
    let mut d = SidecarDocument::new(source(), "p");
    d.virtual_copies[0].id.clear();
    assert!(d
        .validate()
        .unwrap_err()
        .to_string()
        .contains("virtual copy id"));

    let mut d = SidecarDocument::new(source(), "p");
    d.virtual_copies[0].mask_library.push(mask("mask"));
    d.virtual_copies[0].mask_library[0].id.clear();
    assert!(d.validate().unwrap_err().to_string().contains("mask id"));

    let mut d = SidecarDocument::new(source(), "p");
    let mut referenced_mask = mask("referenced");
    referenced_mask.operation = MaskOperation::Invert;
    referenced_mask.references.push(MaskReference {
        copy_id: "vc-original".into(),
        mask_id: "mask".into(),
        extras: Extras::new(),
    });
    referenced_mask.references[0].copy_id.clear();
    d.virtual_copies[0].mask_library.push(referenced_mask);
    assert!(d
        .validate()
        .unwrap_err()
        .to_string()
        .contains("mask reference copy_id"));

    let mut d = SidecarDocument::new(source(), "p");
    let mut referenced_mask = mask("referenced");
    referenced_mask.operation = MaskOperation::Invert;
    referenced_mask.references.push(MaskReference {
        copy_id: "vc-original".into(),
        mask_id: "mask".into(),
        extras: Extras::new(),
    });
    referenced_mask.references[0].mask_id.clear();
    d.virtual_copies[0].mask_library.push(referenced_mask);
    assert!(d
        .validate()
        .unwrap_err()
        .to_string()
        .contains("mask reference mask_id"));

    let mut d = SidecarDocument::new(source(), "p");
    d.virtual_copies[0].mask_layers.push(MaskLayer {
        id: String::new(),
        mask: MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "mask".into(),
            extras: Extras::new(),
        },
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        extras: Extras::new(),
        visible: true,
        local_adjustments: None,
    });
    assert!(d
        .validate()
        .unwrap_err()
        .to_string()
        .contains("mask layer id"));

    let mut d = SidecarDocument::new(source(), "p");
    d.virtual_copies[0].history.push(HistoryEntry {
        id: String::new(),
        recipe: EditRecipe::default(),
        recorded_at: None,
        extras: Extras::new(),
    });
    assert!(d
        .validate()
        .unwrap_err()
        .to_string()
        .contains("history entry id"));

    let mut d = SidecarDocument::new(source(), "p");
    d.virtual_copies[0].export_records.push(ExportRecord {
        id: String::new(),
        relative_path: "exports/out.jpg".into(),
        format: "jpeg".into(),
        exported_at: None,
        extras: Extras::new(),
    });
    assert!(d.validate().unwrap_err().to_string().contains("export id"));

    let mut d = SidecarDocument::new(source(), "p");
    d.presets.push(Preset {
        id: String::new(),
        name: "Preset".into(),
        recipe: EditRecipe::default(),
        extras: Extras::new(),
    });
    assert!(d.validate().unwrap_err().to_string().contains("preset id"));
}

#[test]
fn virtual_copy_lifecycle_preserves_independent_recipe() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0]
        .recipe
        .adjustments
        .insert("exposure".into(), 1.0);
    d.duplicate_virtual_copy("vc-original", "vc-copy", "Copy")
        .unwrap();
    d.rename_virtual_copy("vc-copy", "Renamed").unwrap();
    d.virtual_copies.swap(0, 1);
    d.delete_virtual_copy("vc-copy").unwrap();
    assert_eq!(d.virtual_copies.len(), 1);
    d.restore_virtual_copy("vc-copy").unwrap();
    assert_eq!(d.virtual_copies[1].name, "Renamed");
    d.virtual_copies[1]
        .recipe
        .adjustments
        .insert("exposure".into(), -1.0);
    assert_ne!(
        d.virtual_copies[0].recipe.adjustments["exposure"],
        d.virtual_copies[1].recipe.adjustments["exposure"]
    );
    assert_eq!(
        d,
        SidecarDocument::from_json(&d.to_json().unwrap()).unwrap()
    );
}

#[test]
fn rating_and_flag_roundtrip_per_copy() {
    // LR-01: rating (0..=5) and flag are per-copy metadata with a JSON
    // roundtrip; legacy documents without the fields read as unrated.
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].rating = 4;
    d.virtual_copies[0].flag = Flag::Pick;
    d.duplicate_virtual_copy("vc-original", "vc-copy", "Copy")
        .unwrap();
    // The duplicate inherits the source rating/flag as starting values.
    assert_eq!(d.virtual_copies[1].rating, 4);
    assert_eq!(d.virtual_copies[1].flag, Flag::Pick);
    d.virtual_copies[1].rating = 2;
    d.virtual_copies[1].flag = Flag::Reject;
    let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(decoded.virtual_copies[0].rating, 4);
    assert_eq!(decoded.virtual_copies[0].flag, Flag::Pick);
    assert_eq!(decoded.virtual_copies[1].rating, 2);
    assert_eq!(decoded.virtual_copies[1].flag, Flag::Reject);
    // Legacy JSON without the additive fields still loads as unrated.
    let mut legacy: Value = serde_json::from_str(&d.to_json().unwrap()).unwrap();
    for copy in legacy["virtual_copies"].as_array_mut().unwrap() {
        copy.as_object_mut().unwrap().remove("rating");
        copy.as_object_mut().unwrap().remove("flag");
    }
    let decoded = SidecarDocument::from_json(&serde_json::to_string(&legacy).unwrap()).unwrap();
    assert_eq!(decoded.virtual_copies[0].rating, 0);
    assert_eq!(decoded.virtual_copies[0].flag, Flag::Unflagged);
}

#[test]
fn rating_above_five_is_rejected_loudly() {
    // LR-01: no silent clamping — 6 stars is a schema violation.
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].rating = 6;
    assert!(matches!(d.validate(), Err(SidecarError::Invalid(_))));
    assert!(matches!(d.to_json(), Err(SidecarError::Invalid(_))));
}
