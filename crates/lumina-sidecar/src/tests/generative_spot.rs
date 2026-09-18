use super::*;

#[test]
fn generative_artifact_link_roundtrips() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.generative_edit = Some(generative_edit_with_link());
    let json = d.to_json().unwrap();
    assert!(json.contains("gen-canvas-1"));
    let decoded = SidecarDocument::from_json(&json).unwrap();
    assert_eq!(
        decoded.virtual_copies[0].recipe.generative_edit,
        Some(generative_edit_with_link())
    );
}

#[test]
fn generative_link_and_spot_removals_absent_keys_are_identity() {
    // Legacy documents without the additive keys read as no link / empty
    // list and require no migration.
    let json = r#"{"format":"lumina-sidecar","schema_version":2,"source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"PNG","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","presets":[],"virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{"generative_edit":{"version":1}},"mask_library":[],"mask_layers":[],"history":[],"export_records":[]}]}"#;
    let doc = SidecarDocument::from_json(json).unwrap();
    let recipe = &doc.virtual_copies[0].recipe;
    assert_eq!(recipe.generative_edit.as_ref().unwrap().artifact, None);
    assert!(recipe.spot_removals.is_empty());
}

#[test]
fn generative_and_spot_unknown_versions_are_rejected() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    let mut bad = generative_edit_with_link();
    bad.version = 99;
    d.virtual_copies[0].recipe.generative_edit = Some(bad);
    assert!(d.validate().is_err());

    let mut d2 = SidecarDocument::new(source(), "pipeline-1");
    d2.virtual_copies[0].recipe.spot_removals = vec![spot_removal(
        SpotRemovalMode::Generative,
        Some(generative_link()),
    )];
    d2.virtual_copies[0].recipe.spot_removals[0].version = 99;
    assert!(d2.validate().is_err());
}

#[test]
fn generative_bad_link_is_rejected() {
    let mut bad_cases = Vec::new();
    let mut empty_id = generative_link();
    empty_id.id.clear();
    bad_cases.push(empty_id);
    let mut absolute = generative_link();
    absolute.relative_path = "/abs/a.zdata".into();
    bad_cases.push(absolute);
    let mut opaque_format = generative_link();
    opaque_format.format = "opaque".into();
    bad_cases.push(opaque_format);
    let mut empty_checksum = generative_link();
    empty_checksum.checksum.clear();
    bad_cases.push(empty_checksum);
    let mut zero_dims = generative_link();
    zero_dims.width = 0;
    bad_cases.push(zero_dims);
    let mut wrong_channels = generative_link();
    wrong_channels.channels = "f32".into();
    bad_cases.push(wrong_channels);
    let mut wrong_data_version = generative_link();
    wrong_data_version.data_version = "2".into();
    bad_cases.push(wrong_data_version);
    for link in bad_cases {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        let mut edit = generative_edit_with_link();
        edit.artifact = Some(link);
        d.virtual_copies[0].recipe.generative_edit = Some(edit);
        assert!(d.validate().is_err());
    }
}

#[test]
fn spot_heuristic_rejects_artifact_generative_roundtrips() {
    // Heuristic + artifact is a loud exclusion violation.
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.spot_removals = vec![spot_removal(
        SpotRemovalMode::Heuristic,
        Some(generative_link()),
    )];
    assert!(d.validate().is_err());

    // Generative with link roundtrips; the load carries the extras mirror of
    // the same key (SPOT-SCHEMA-GEOMETRY) which stays valid (generative
    // needs no geometry, link verified).
    let mut d2 = SidecarDocument::new(source(), "pipeline-1");
    d2.virtual_copies[0].recipe.spot_removals = vec![spot_removal(
        SpotRemovalMode::Generative,
        Some(generative_link()),
    )];
    let json = d2.to_json().unwrap();
    assert!(json.contains("spot_removals"));
    let decoded = SidecarDocument::from_json(&json).unwrap();
    assert_eq!(
        decoded.virtual_copies[0].recipe.spot_removals,
        d2.virtual_copies[0].recipe.spot_removals
    );
    assert_eq!(
        decoded.virtual_copies[0].recipe.extras.get("spot_removals"),
        Some(&serde_json::to_value(&d2.virtual_copies[0].recipe.spot_removals).unwrap())
    );
}

#[test]
fn spot_typed_heuristic_without_geometry_is_loudly_invalid_on_load() {
    // SPOT-SCHEMA-GEOMETRY: params-lose typed heuristic entries (no heal
    // geometry anywhere) serialize without key loss, but loading them is
    // rejected loudly — the mirrored extras entry misses the mandatory
    // geometry. Old entries stay recognizable as missing/invalid instead
    // of rendering as if no spot existed.
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.spot_removals = vec![spot_removal(SpotRemovalMode::Heuristic, None)];
    let json = d.to_json().unwrap();
    assert!(json.contains("spot_removals"));
    let err = SidecarDocument::from_json(&json).unwrap_err();
    assert!(
        err.to_string().contains("spot_removal"),
        "loud geometry error expected, got {err}"
    );
}

#[test]
fn spot_removals_empty_list_is_absent_when_empty() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.spot_removals = vec![];
    let json = d.to_json().unwrap();
    assert!(!json.contains("spot_removals"));
    let decoded = SidecarDocument::from_json(&json).unwrap();
    assert!(decoded.virtual_copies[0].recipe.spot_removals.is_empty());
    assert!(!decoded.virtual_copies[0]
        .recipe
        .extras
        .contains_key("spot_removals"));
}

#[test]
fn spot_removals_extras_heuristic_geometry_survives_roundtrip() {
    // SPOT-SCHEMA-GEOMETRY detector (mirrors the GUI headless test
    // `spot_heal_headless_quick_heal_q_shortcut_and_render` at sidecar
    // level): producer-written extras heal geometry must survive
    // save/load losslessly — the 69dad91 data loss may never return.
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.extras.insert(
        "spot_removals".into(),
        Value::Array(vec![heuristic_spot_extra()]),
    );
    let json = d.to_json().unwrap();
    let decoded = SidecarDocument::from_json(&json).unwrap();
    assert_eq!(
        decoded.virtual_copies[0].recipe.extras.get("spot_removals"),
        d.virtual_copies[0].recipe.extras.get("spot_removals")
    );
    assert_eq!(decoded.virtual_copies[0].recipe.spot_removals.len(), 1);
    assert_eq!(
        decoded.virtual_copies[0].recipe.spot_removals[0].mode,
        SpotRemovalMode::Heuristic
    );
    decoded.validate().unwrap();
    // Second roundtrip is a fixed point (mirror of mirror is identical).
    let decoded2 = SidecarDocument::from_json(&decoded.to_json().unwrap()).unwrap();
    assert_eq!(
        decoded2.virtual_copies[0]
            .recipe
            .extras
            .get("spot_removals"),
        d.virtual_copies[0].recipe.extras.get("spot_removals")
    );
}

#[test]
fn spot_removals_extras_generative_roundtrips_without_geometry() {
    // Generative spots need no heal geometry; the extras view roundtrips
    // with mode intact and the typed view carries version/mode.
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.extras.insert(
        "spot_removals".into(),
        serde_json::json!([{"id": "g1", "version": 1, "mode": "generative"}]),
    );
    let json = d.to_json().unwrap();
    let decoded = SidecarDocument::from_json(&json).unwrap();
    assert_eq!(
        decoded.virtual_copies[0].recipe.extras.get("spot_removals"),
        d.virtual_copies[0].recipe.extras.get("spot_removals")
    );
    assert_eq!(decoded.virtual_copies[0].recipe.spot_removals.len(), 1);
    assert_eq!(
        decoded.virtual_copies[0].recipe.spot_removals[0].mode,
        SpotRemovalMode::Generative
    );
    decoded.validate().unwrap();
}

#[test]
fn spot_removals_extras_generative_bad_link_is_rejected() {
    // A generative extras entry with a corrupt artifact link fails loudly.
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    let mut bad_link = generative_link();
    bad_link.checksum.clear();
    d.virtual_copies[0].recipe.extras.insert(
        "spot_removals".into(),
        serde_json::json!([{
            "id": "g1", "version": 1, "mode": "generative",
            "artifact": serde_json::to_value(&bad_link).unwrap()
        }]),
    );
    assert!(d.validate().is_err());
}

#[test]
fn spot_removals_extras_validation_rejects_loudly() {
    // Every malformed extras entry fails loudly — no silent fallback, no
    // silent reinterpretation. Each case is a full entry array element.
    let mut cases: Vec<(&str, Value)> = Vec::new();
    // Unknown version.
    let mut v = heuristic_spot_extra();
    v["version"] = serde_json::json!(99);
    cases.push(("version", v));
    // Missing version.
    let mut v = heuristic_spot_extra();
    v.as_object_mut().unwrap().remove("version");
    cases.push(("missing-version", v));
    // Unknown mode.
    let mut v = heuristic_spot_extra();
    v["mode"] = serde_json::json!("clone");
    cases.push(("mode", v));
    // Missing geometry (params-lose shape).
    for field in [
        "id",
        "center_x",
        "center_y",
        "radius",
        "offset_dx",
        "offset_dy",
    ] {
        let mut v = heuristic_spot_extra();
        v.as_object_mut().unwrap().remove(field);
        cases.push(("missing-geometry", v));
    }
    // Out-of-range geometry.
    let mut v = heuristic_spot_extra();
    v["radius"] = serde_json::json!(0.0);
    cases.push(("radius-range", v));
    let mut v = heuristic_spot_extra();
    v["center_x"] = serde_json::json!(1.5);
    cases.push(("center-range", v));
    let mut v = heuristic_spot_extra();
    v["opacity"] = serde_json::json!(2.0);
    cases.push(("opacity-range", v));
    // Wrong-typed geometry (JSON has no non-finite numbers; a string must
    // fail loudly instead of being coerced or skipped).
    let mut v = heuristic_spot_extra();
    v["radius"] = serde_json::json!("wide");
    cases.push(("radius-type", v));
    // Heuristic must not carry an artifact (mirrors the typed exclusion rule).
    let mut v = heuristic_spot_extra();
    v["artifact"] = serde_json::to_value(generative_link()).unwrap();
    cases.push(("heuristic-artifact", v));
    // Non-object entry and non-array key.
    cases.push(("non-object", serde_json::json!("spot-1")));
    for (name, entry) in cases {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0]
            .recipe
            .extras
            .insert("spot_removals".into(), Value::Array(vec![entry]));
        assert!(
            d.validate().is_err(),
            "extras entry `{name}` must be rejected loudly"
        );
    }
    // Non-array key shape.
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.extras.insert(
        "spot_removals".into(),
        serde_json::json!({"mode": "heuristic"}),
    );
    assert!(d.validate().is_err());
}
