use super::*;

#[test]
fn migration_unknown_fields_and_incompatible_version() {
    let d = SidecarDocument::new(source(), "pipeline-1");
    let mut value: Value = serde_json::from_str(&d.to_json().unwrap()).unwrap();
    value["schema_version"] = Value::from(0);
    value["virtual_copies"][0]["recipe"]
        .as_object_mut()
        .unwrap()
        .remove("recipe_version");
    let migrated = migrate_json(&serde_json::to_string(&value).unwrap()).unwrap();
    let decoded = SidecarDocument::from_json(&migrated).unwrap();
    assert_eq!(decoded.virtual_copies[0].recipe.recipe_version, "1");
    value["schema_version"] = Value::from(99);
    assert_eq!(
        migrate_json(&serde_json::to_string(&value).unwrap()).unwrap_err(),
        SidecarError::Invalid(
            "unsupported schema_version 99; explicit migration is required".into()
        )
    );
}

#[test]
fn explicit_v1_to_v2_migration_keeps_flat_adjustments() {
    let mut document = SidecarDocument::new(source(), "pipeline-1");
    document.virtual_copies[0].recipe.adjustments.extend([
        (String::from("exposure"), 1.5),
        (String::from("contrast"), -0.25),
    ]);
    let mut legacy: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
    legacy["schema_version"] = Value::from(1);

    let migrated: Value =
        serde_json::from_str(&migrate_json(&serde_json::to_string(&legacy).unwrap()).unwrap())
            .unwrap();

    assert_eq!(migrated["schema_version"], Value::from(2));
    assert_eq!(
        migrated["virtual_copies"][0]["recipe"]["adjustments"]["exposure"],
        Value::from(1.5)
    );
    assert_eq!(
        migrated["virtual_copies"][0]["recipe"]["adjustments"]["contrast"],
        Value::from(-0.25)
    );
    assert_eq!(
        SidecarDocument::from_json(&serde_json::to_string(&migrated).unwrap())
            .unwrap()
            .schema_version,
        2
    );
}

#[test]
fn curve_and_hsl_validation_reject_invalid_values() {
    let valid_sidecar = || {
        let document = SidecarDocument::new(source(), "pipeline-1");
        serde_json::to_value(document).unwrap()
    };
    let curve = |points: Vec<(f32, f32)>| {
        serde_json::json!({
            "version": 1,
            "master": points.into_iter().map(|(input, output)| {
                serde_json::json!({"input": input, "output": output})
            }).collect::<Vec<_>>(),
            "channels": {}
        })
    };

    let invalid_curves = [
        // Fewer than two points.
        curve(vec![(0.0, 0.0)]),
        // Inputs are not strictly ascending.
        curve(vec![(0.0, 0.0), (0.5, 0.5), (0.5, 0.75), (1.0, 1.0)]),
        // Both required endpoints are absent.
        curve(vec![(0.25, 0.25), (0.75, 0.75)]),
    ];
    for invalid_curve in invalid_curves {
        let mut sidecar = valid_sidecar();
        sidecar["virtual_copies"][0]["recipe"]["adjustments"]["curves"] = invalid_curve;
        assert!(SidecarDocument::from_json(&serde_json::to_string(&sidecar).unwrap()).is_err());
    }

    let mut sidecar = valid_sidecar();
    sidecar["virtual_copies"][0]["recipe"]["adjustments"]["hsl"] = serde_json::json!({
        "version": 1,
        "red": {"hue": 1.1, "saturation": 0.0, "luminance": 0.0}
    });
    assert!(SidecarDocument::from_json(&serde_json::to_string(&sidecar).unwrap()).is_err());
}

#[test]
fn legacy_flat_adjustments_api_and_json_remain_compatible() {
    let json = r#"{
        "recipe_version":"1",
        "adjustments":{"exposure":1.5,"contrast":-0.25},
        "options":{}, "auto_features":{}, "future_recipe":{"kept":true}
    }"#;
    let recipe: EditRecipe = serde_json::from_str(json).unwrap();
    assert_eq!(recipe.adjustments["exposure"], 1.5);
    assert_eq!(recipe.adjustments["contrast"], -0.25);
    assert!(recipe.curves.is_none() && recipe.hsl.is_none());
    assert!(recipe.extras.contains_key("future_recipe"));
    let encoded = serde_json::to_value(&recipe).unwrap();
    assert_eq!(encoded["adjustments"]["exposure"], 1.5);
    assert!(encoded["adjustments"].get("curves").is_none());
}

#[test]
fn color_grading_roundtrips_as_nested_adjustment() {
    let recipe = EditRecipe {
        color_grading: Some(ColorGrading {
            version: 1,
            shadows: ColorGradingRange {
                hue_degrees: 360.0,
                saturation: 0.5,
                luminance: 0.0,
            },
            midtones: ColorGradingRange {
                hue_degrees: 120.0,
                saturation: 0.25,
                luminance: 0.0,
            },
            highlights: ColorGradingRange {
                hue_degrees: 240.0,
                saturation: 0.75,
                luminance: 0.0,
            },
            balance: -0.2,
            blending: 0.5,
        }),
        ..Default::default()
    };
    let value = serde_json::to_value(&recipe).unwrap();
    assert!(value["adjustments"]["color_grading"].is_object());
    assert_eq!(recipe, serde_json::from_value(value).unwrap());
}

#[test]
fn color_grading_legacy_fields_default_without_migration() {
    // Altdateien ohne `luminance`/`blending` lesen sich als
    // `luminance = 0` / `blending = 0.5` (identisches Renderverhalten).
    let legacy = serde_json::json!({
        "version": 1,
        "shadows": {"hue_degrees": 0.0, "saturation": 0.0},
        "midtones": {"hue_degrees": 0.0, "saturation": 0.0},
        "highlights": {"hue_degrees": 0.0, "saturation": 0.0},
        "balance": 0.0
    });
    let grading: ColorGrading = serde_json::from_value(legacy).unwrap();
    assert_eq!(grading.blending, 0.5);
    assert_eq!(grading.shadows.luminance, 0.0);
}

#[test]
fn point_color_roundtrips_as_nested_adjustment_with_stable_ids() {
    let recipe = EditRecipe {
        point_color: Some(PointColor {
            version: 1,
            entries: vec![PointColorEntry {
                id: "pc-1".into(),
                hue_center: 30.0,
                hue_range: 20.0,
                hue_shift: 0.5,
                saturation_shift: -0.25,
                luminance_shift: 0.1,
            }],
        }),
        ..Default::default()
    };
    let value = serde_json::to_value(&recipe).unwrap();
    assert!(value["adjustments"]["point_color"].is_object());
    let roundtrip: EditRecipe = serde_json::from_value(value).unwrap();
    assert_eq!(recipe, roundtrip);
    assert_eq!(
        PointColorEntry::next_id(&roundtrip.point_color.expect("point color").entries),
        "pc-2"
    );
}

#[test]
fn point_color_validation_rejects_bad_entries_loudly() {
    let bad = |entries: Vec<PointColorEntry>| {
        let recipe = EditRecipe {
            point_color: Some(PointColor {
                version: 1,
                entries,
            }),
            ..Default::default()
        };
        validate_adjustments(&recipe).is_err()
    };
    let entry = || PointColorEntry {
        id: "pc-1".into(),
        hue_center: 30.0,
        hue_range: 20.0,
        hue_shift: 0.0,
        saturation_shift: 0.0,
        luminance_shift: 0.0,
    };
    let mut out_of_range = entry();
    out_of_range.hue_center = 400.0;
    assert!(bad(vec![out_of_range]));
    let mut dup = entry();
    assert!(bad(vec![entry(), dup.clone()]));
    dup.id = String::new();
    assert!(bad(vec![dup]));
    assert!(bad(vec![entry(); 9]));
    // Gültiger Eintrag passiert die Validierung.
    assert!(!bad(vec![entry()]));
}

#[test]
fn curves_use_curve_points_lists_and_hsl_channels_are_optional() {
    let recipe = EditRecipe {
        curves: Some(Curves {
            version: 1,
            master: vec![
                CurvePoint {
                    input: 0.0,
                    output: 0.0,
                },
                CurvePoint {
                    input: 1.0,
                    output: 1.0,
                },
            ],
            channels: CurveChannels::default(),
        }),
        hsl: Some(HslAdjustments {
            version: 1,
            ..Default::default()
        }),
        ..Default::default()
    };
    let value = serde_json::to_value(&recipe).unwrap();
    assert!(value["adjustments"]["curves"]["master"].is_array());
    assert!(value["adjustments"]["curves"]["master"]
        .get("points")
        .is_none());
    let roundtrip: EditRecipe = serde_json::from_value(value).unwrap();
    assert_eq!(roundtrip, recipe);
}
