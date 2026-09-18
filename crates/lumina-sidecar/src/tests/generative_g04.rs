use super::*;

// ----- LRPAR-G04-REMOVE: visualize, distraction, variant controls -----
#[test]
fn g04_visualize_threshold_roundtrip_and_validation() {
    let mut recipe = EditRecipe::default();
    assert_eq!(recipe.spot_visualize_threshold(), None);
    recipe.set_spot_visualize_threshold(Some(0.35)).unwrap();
    assert_eq!(recipe.spot_visualize_threshold(), Some(0.35));
    assert!(recipe.set_spot_visualize_threshold(Some(1.5)).is_err());
    assert!(recipe.set_spot_visualize_threshold(Some(f32::NAN)).is_err());
    assert_eq!(recipe.spot_visualize_threshold(), Some(0.35));
    recipe.set_spot_visualize_threshold(None).unwrap();
    assert_eq!(recipe.spot_visualize_threshold(), None);
    assert!(!recipe.extras.contains_key(SPOT_VISUALIZE_KEY));
    // Persisted value survives a document roundtrip and validates.
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0]
        .recipe
        .set_spot_visualize_threshold(Some(0.2))
        .unwrap();
    assert!(d.validate().is_ok());
    let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(
        decoded.virtual_copies[0].recipe.spot_visualize_threshold(),
        Some(0.2)
    );
    assert!(decoded.validate().is_ok());
    // A hand-edited out-of-range value fails loudly.
    let mut bad = SidecarDocument::new(source(), "pipeline-1");
    bad.virtual_copies[0]
        .recipe
        .extras
        .insert(SPOT_VISUALIZE_KEY.into(), serde_json::json!(2.0));
    assert!(bad.validate().is_err());
    let mut bad_type = SidecarDocument::new(source(), "pipeline-1");
    bad_type.virtual_copies[0]
        .recipe
        .extras
        .insert(SPOT_VISUALIZE_KEY.into(), serde_json::json!("low"));
    assert!(bad_type.validate().is_err());
}

#[test]
fn g04_distraction_switches_roundtrip_and_default_off() {
    let recipe = EditRecipe::default();
    assert_eq!(recipe.spot_distraction(), SpotDistraction::default());
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    assert!(d.validate().is_ok());
    let setting = SpotDistraction {
        dust: true,
        auto_mode: true,
        ..Default::default()
    };
    d.virtual_copies[0].recipe.set_spot_distraction(setting);
    assert_eq!(d.virtual_copies[0].recipe.spot_distraction(), setting);
    assert!(d.validate().is_ok());
    let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(decoded.virtual_copies[0].recipe.spot_distraction(), setting);
    // Resetting to all-off removes the key (legacy byte-stability).
    d.virtual_copies[0]
        .recipe
        .set_spot_distraction(SpotDistraction::default());
    assert!(!d.virtual_copies[0]
        .recipe
        .extras
        .contains_key(SPOT_DISTRACTION_KEY));
    // A non-object value fails loudly.
    let mut bad = SidecarDocument::new(source(), "pipeline-1");
    bad.virtual_copies[0]
        .recipe
        .extras
        .insert(SPOT_DISTRACTION_KEY.into(), serde_json::json!("dust"));
    assert!(bad.validate().is_err());
}

#[test]
fn g04_generative_variant_controls_roundtrip_and_reject_loudly() {
    // seed/variant/base_seed/prompt ride the generative extras entry and
    // validate (G04-FOLLOWUP-1: `base_seed` is the regenerate provenance).
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.extras.insert(
        "spot_removals".into(),
        serde_json::json!([{
            "id": "g1", "version": 1, "mode": "generative",
            "prompt": "remove dust", "seed": 7, "variant": 2, "base_seed": 7
        }]),
    );
    assert!(d.validate().is_ok());
    let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(
        decoded.virtual_copies[0].recipe.extras.get("spot_removals"),
        d.virtual_copies[0].recipe.extras.get("spot_removals")
    );
    for (field, value) in [
        ("seed", serde_json::json!("seven")),
        ("variant", serde_json::json!(-1)),
        ("base_seed", serde_json::json!("seven")),
        ("base_seed", serde_json::json!(-1)),
        ("prompt", serde_json::json!(42)),
    ] {
        let mut bad = SidecarDocument::new(source(), "pipeline-1");
        let mut entry = serde_json::json!({"id": "g1", "version": 1, "mode": "generative"});
        entry[field] = value;
        bad.virtual_copies[0]
            .recipe
            .extras
            .insert("spot_removals".into(), Value::Array(vec![entry]));
        assert!(
            bad.validate().is_err(),
            "generative `{field}` must be rejected loudly"
        );
    }
}

#[test]
fn save_load_atomic_roundtrip_preserves_generative_links() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("image.lumina.json");
    let mut document = SidecarDocument::new(source(), "pipeline-1");
    document.virtual_copies[0].recipe.generative_edit = Some(generative_edit_with_link());
    document.virtual_copies[0].recipe.spot_removals = vec![spot_removal(
        SpotRemovalMode::Generative,
        Some(generative_link()),
    )];
    save_sidecar(&path, &document).unwrap();
    let loaded = load_sidecar(&path).unwrap();
    assert_eq!(
        loaded.virtual_copies[0].recipe.spot_removals,
        document.virtual_copies[0].recipe.spot_removals
    );
    // SPOT-SCHEMA-GEOMETRY: the load carries the extras mirror of the
    // typed key, so full-document equality no longer holds by design —
    // assert the mirror instead (same raw value the typed view parsed).
    assert_eq!(
        loaded.virtual_copies[0].recipe.extras.get("spot_removals"),
        Some(&serde_json::to_value(&document.virtual_copies[0].recipe.spot_removals).unwrap())
    );
    // No partial atomic-write temporary may linger.
    for entry in std::fs::read_dir(directory.path()).unwrap() {
        let name = entry.unwrap().file_name();
        assert!(
            !name.to_string_lossy().starts_with(".image.lumina.json.tmp"),
            "orphaned temporary: {name:?}"
        );
    }
}
