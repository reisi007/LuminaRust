use super::*;

#[test]
fn auto_tone_mirror_fields_validate_range_and_finiteness() {
    // Jede der vier AUTO-TONE-2-Spiegelfelder einzeln: gültige
    // Randwerte ±1.0 passieren, NaN/∞/Out-of-range wird laut abgelehnt.
    for (index, valid) in [0.8, -0.8, 1.0, -1.0, 0.0].iter().enumerate() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        let features = &mut d.virtual_copies[0].recipe.auto_features;
        match index % 4 {
            0 => features.auto_whites = Some(*valid),
            1 => features.auto_blacks = Some(*valid),
            2 => features.auto_highlights = Some(*valid),
            _ => features.auto_shadows = Some(*valid),
        }
        assert!(d.validate().is_ok(), "valid value {valid} rejected");
    }
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1.5, -1.5] {
        for field in [
            "auto_whites",
            "auto_blacks",
            "auto_highlights",
            "auto_shadows",
        ] {
            let mut d = SidecarDocument::new(source(), "pipeline-1");
            let features = &mut d.virtual_copies[0].recipe.auto_features;
            match field {
                "auto_whites" => features.auto_whites = Some(bad),
                "auto_blacks" => features.auto_blacks = Some(bad),
                "auto_highlights" => features.auto_highlights = Some(bad),
                _ => features.auto_shadows = Some(bad),
            }
            assert!(
                d.validate().is_err(),
                "field {field} accepted invalid value {bad}"
            );
        }
    }
    // `None` (kein Auto-Wert) bleibt gültig.
    assert!(SidecarDocument::new(source(), "pipeline-1")
        .validate()
        .is_ok());
}

#[test]
fn auto_tone_mirror_fields_missing_in_legacy_json_default_to_none() {
    // Additiv, keine Migration nötig: Alt-JSON ohne die vier Felder
    // parst dank `#[serde(default)]` und validiert; `schema_version`
    // bleibt unverändert.
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.auto_features.auto_exposure = Some(1.25);
    d.virtual_copies[0].recipe.auto_features.auto_contrast = Some(-0.2);
    let mut json_value = serde_json::to_value(&d).expect("sidecar serializes");
    for field in [
        "auto_whites",
        "auto_blacks",
        "auto_highlights",
        "auto_shadows",
    ] {
        json_value["virtual_copies"][0]["recipe"]["auto_features"]
            .as_object_mut()
            .expect("auto_features is an object")
            .remove(field);
    }
    let json = serde_json::to_string(&json_value).expect("json serializes");
    assert!(!json.contains("auto_whites"));
    let decoded = SidecarDocument::from_json(&json).expect("legacy json parses");
    let features = &decoded.virtual_copies[0].recipe.auto_features;
    assert_eq!(features.auto_whites, None);
    assert_eq!(features.auto_blacks, None);
    assert_eq!(features.auto_highlights, None);
    assert_eq!(features.auto_shadows, None);
    assert!(decoded.validate().is_ok());
}

#[test]
fn presence_and_geometry_roundtrip_in_recipe() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.presence = Some(Presence {
        version: 1,
        texture: 0.25,
        clarity: -0.5,
        dehaze: 1.0,
    });
    d.virtual_copies[0].recipe.geometry = Some(Geometry {
        version: 1,
        crop: Some(Crop::Aspect {
            preset: AspectPreset::FourToFive,
        }),
        rotation_degrees: -12.5,
        mirror_horizontal: true,
        mirror_vertical: false,
    });
    let json = d.to_json().unwrap();
    assert!(json.contains("\"presence\""));
    assert!(json.contains("\"geometry\""));
    assert_eq!(d, SidecarDocument::from_json(&json).unwrap());
}

#[test]
fn geometry_free_crop_rotation_and_both_mirrors_roundtrip() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.geometry = Some(Geometry {
        version: 1,
        crop: Some(Crop::Free {
            x: 0.125,
            y: 0.25,
            width: 0.5,
            height: 0.375,
        }),
        rotation_degrees: 90.0,
        mirror_horizontal: true,
        mirror_vertical: true,
    });
    let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(
        decoded.virtual_copies[0].recipe.geometry,
        d.virtual_copies[0].recipe.geometry
    );
}

#[test]
fn presence_values_roundtrip_without_loss() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.presence = Some(Presence {
        version: 1,
        texture: -0.75,
        clarity: 0.375,
        dehaze: -1.0,
    });
    let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(
        decoded.virtual_copies[0].recipe.presence,
        d.virtual_copies[0].recipe.presence
    );
}

#[test]
fn presence_and_geometry_validation_rejects_invalid_values() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.presence = Some(Presence {
        version: 2,
        texture: 0.0,
        clarity: 0.0,
        dehaze: 0.0,
    });
    assert!(d.validate().is_err());

    d.virtual_copies[0].recipe.presence = Some(Presence {
        version: 1,
        texture: f32::NAN,
        clarity: 0.0,
        dehaze: 0.0,
    });
    assert!(d.validate().is_err());

    d.virtual_copies[0].recipe.presence = None;
    d.virtual_copies[0].recipe.geometry = Some(Geometry {
        version: 1,
        crop: Some(Crop::Free {
            x: 0.8,
            y: 0.0,
            width: 0.3,
            height: 0.5,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    });
    assert!(d.validate().is_err());
}
