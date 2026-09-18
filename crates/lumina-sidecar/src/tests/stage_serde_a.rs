use super::*;

#[test]
fn noise_and_sharpening_roundtrip_and_validate_ranges() {
    let recipe = EditRecipe {
        noise_reduction: Some(NoiseReduction {
            version: 1,
            luminance: 0.4,
            color: 0.8,
        }),
        sharpening: Some(Sharpening {
            version: 1,
            amount: 2.0,
            radius: 3.0,
            detail: 0.5,
            masking: 0.7,
        }),
        ..Default::default()
    };
    let value = serde_json::to_value(&recipe).unwrap();
    assert!(value["adjustments"]["noise_reduction"].is_object());
    assert_eq!(recipe, serde_json::from_value(value).unwrap());
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.noise_reduction = Some(NoiseReduction {
        version: 2,
        luminance: 0.0,
        color: 0.0,
    });
    assert!(d.validate().is_err());
    d.virtual_copies[0].recipe.noise_reduction = Some(NoiseReduction {
        version: 1,
        luminance: f32::NAN,
        color: 0.0,
    });
    assert!(d.validate().is_err());
}

#[test]
fn red_eye_roundtrip_and_validate_ranges() {
    let recipe = EditRecipe {
        red_eye: Some(RedEyeCorrection {
            version: 1,
            regions: vec![red_eye_region("re-1"), red_eye_region("re-2")],
        }),
        ..Default::default()
    };
    let value = serde_json::to_value(&recipe).unwrap();
    // `red_eye` lives inside `adjustments` (like `noise_reduction`).
    assert!(value["adjustments"]["red_eye"].is_object());
    assert_eq!(
        value["adjustments"]["red_eye"]["regions"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(recipe, serde_json::from_value(value).unwrap());

    // Absent key stays absent (additive, legacy identity, no migration).
    let legacy = serde_json::json!({
        "recipe_version": "1",
        "adjustments": {},
        "options": {},
        "auto_features": {"enable_auto_tone": false, "match_total_exposure": false, "target_luminance": 0.5},
    });
    let decoded: EditRecipe = serde_json::from_value(legacy).unwrap();
    assert!(decoded.red_eye.is_none());
    assert!(validate_adjustments(&decoded).is_ok());

    // Empty region list roundtrips and validates (identity).
    let empty = EditRecipe {
        red_eye: Some(RedEyeCorrection {
            version: 1,
            regions: Vec::new(),
        }),
        ..Default::default()
    };
    let value = serde_json::to_value(&empty).unwrap();
    assert_eq!(empty, serde_json::from_value(value).unwrap());
    assert!(validate_adjustments(&empty).is_ok());

    // Unknown version is rejected loudly.
    let mut bad = SidecarDocument::new(source(), "pipeline-1");
    bad.virtual_copies[0].recipe.red_eye = Some(RedEyeCorrection {
        version: 2,
        regions: vec![red_eye_region("re-1")],
    });
    assert!(bad.validate().is_err());
}

#[test]
fn red_eye_rejects_out_of_range_and_nan() {
    let recipe = EditRecipe {
        red_eye: Some(RedEyeCorrection {
            version: 1,
            regions: vec![red_eye_region("re-1")],
        }),
        ..Default::default()
    };
    // Each invalid mutation fails loudly instead of being clipped.
    for mutate in [
        |r: &mut RedEyeRegion| r.x = 1.5,
        |r: &mut RedEyeRegion| r.y = -0.1,
        |r: &mut RedEyeRegion| r.x = f32::NAN,
        |r: &mut RedEyeRegion| r.radius = 0.0,
        |r: &mut RedEyeRegion| r.radius = 1.5,
        |r: &mut RedEyeRegion| r.radius = f32::INFINITY,
        |r: &mut RedEyeRegion| r.desaturate = -0.1,
        |r: &mut RedEyeRegion| r.desaturate = 1.1,
        |r: &mut RedEyeRegion| r.desaturate = f32::NAN,
        |r: &mut RedEyeRegion| r.darken = 2.0,
        |r: &mut RedEyeRegion| r.darken = f32::NAN,
    ] {
        let mut candidate = recipe.clone();
        mutate(&mut candidate.red_eye.as_mut().unwrap().regions[0]);
        assert!(validate_adjustments(&candidate).is_err());
    }
    // Empty and duplicate ids are rejected.
    let mut candidate = recipe.clone();
    candidate.red_eye.as_mut().unwrap().regions[0].id.clear();
    assert!(validate_adjustments(&candidate).is_err());
    let mut candidate = recipe.clone();
    candidate
        .red_eye
        .as_mut()
        .unwrap()
        .regions
        .push(red_eye_region("re-1"));
    assert!(validate_adjustments(&candidate).is_err());
    // The valid recipe passes.
    assert!(validate_adjustments(&recipe).is_ok());
}

#[test]
fn upright_roundtrip_and_validate_ranges() {
    // Disabled analysis (persisted suggestion, not applied) roundtrips at
    // the recipe root like `perspective`.
    let recipe = EditRecipe {
        upright: Some(Upright {
            version: 1,
            enabled: false,
            analysis: Some(upright_analysis()),
        }),
        ..Default::default()
    };
    let value = serde_json::to_value(&recipe).unwrap();
    assert!(value["upright"].is_object(), "upright lives at the root");
    assert_eq!(value["upright"]["enabled"], serde_json::Value::Bool(false));
    assert_eq!(recipe, serde_json::from_value(value).unwrap());
    assert!(validate_adjustments(&recipe).is_ok());
    // Disabled → the manual perspective stays authoritative.
    assert!(recipe.effective_perspective().is_none());

    // Enabled → the analysis supplies the effective perspective.
    let enabled = EditRecipe {
        upright: Some(Upright {
            version: 1,
            enabled: true,
            analysis: Some(upright_analysis()),
        }),
        perspective: Some(Perspective {
            version: 1,
            vertical: 0.9,
            horizontal: 0.9,
            rotation: 0.9,
            scale: 2.0,
            aspect_ratio: 1.0,
            shift_x: 0.0,
            shift_y: 0.0,
        }),
        ..Default::default()
    };
    let effective = enabled
        .effective_perspective()
        .expect("enabled upright supplies a perspective");
    assert_eq!(effective.vertical, 0.2);
    assert_eq!(effective.horizontal, -0.1);
    assert_eq!(effective.rotation, 0.05);
    assert_eq!(effective.scale, 1.0);

    // Disabling restores the manual perspective unchanged.
    let mut disabled = enabled.clone();
    disabled.upright.as_mut().unwrap().enabled = false;
    assert_eq!(
        disabled.effective_perspective().unwrap().vertical,
        0.9,
        "manual perspective returns when upright is off"
    );

    // Absent key stays absent (additive, legacy identity, no migration).
    let legacy = serde_json::json!({
        "recipe_version": "1",
        "adjustments": {},
        "options": {},
        "auto_features": {"enable_auto_tone": false, "match_total_exposure": false, "target_luminance": 0.5},
    });
    let decoded: EditRecipe = serde_json::from_value(legacy).unwrap();
    assert!(decoded.upright.is_none());
    assert!(validate_adjustments(&decoded).is_ok());
}

#[test]
fn upright_enabled_without_analysis_is_rejected_loudly() {
    let recipe = EditRecipe {
        upright: Some(Upright {
            version: 1,
            enabled: true,
            analysis: None,
        }),
        ..Default::default()
    };
    assert!(validate_adjustments(&recipe).is_err());
    // Disabled without analysis is a valid no-op state.
    let disabled = EditRecipe {
        upright: Some(Upright {
            version: 1,
            enabled: false,
            analysis: None,
        }),
        ..Default::default()
    };
    assert!(validate_adjustments(&disabled).is_ok());
    // Foreign version is rejected.
    let mut bad = SidecarDocument::new(source(), "pipeline-1");
    bad.virtual_copies[0].recipe.upright = Some(Upright {
        version: 2,
        enabled: false,
        analysis: None,
    });
    assert!(bad.validate().is_err());
}

#[test]
fn upright_rejects_out_of_range_and_nan() {
    let base = EditRecipe {
        upright: Some(Upright {
            version: 1,
            enabled: true,
            analysis: Some(upright_analysis()),
        }),
        ..Default::default()
    };
    for mutate in [
        |a: &mut UprightAnalysis| a.vertical = 1.5,
        |a: &mut UprightAnalysis| a.horizontal = -1.5,
        |a: &mut UprightAnalysis| a.rotation = f32::NAN,
        |a: &mut UprightAnalysis| a.confidence = 1.1,
        |a: &mut UprightAnalysis| a.confidence = f32::NAN,
        |a: &mut UprightAnalysis| a.line_count = UPRIGHT_MAX_LINE_COUNT + 1,
        |a: &mut UprightAnalysis| a.fingerprint.algorithm.clear(),
        |a: &mut UprightAnalysis| a.fingerprint.version.clear(),
        |a: &mut UprightAnalysis| a.fingerprint.input_fingerprint.clear(),
    ] {
        let mut candidate = base.clone();
        mutate(
            candidate
                .upright
                .as_mut()
                .unwrap()
                .analysis
                .as_mut()
                .unwrap(),
        );
        assert!(
            validate_adjustments(&candidate).is_err(),
            "invalid upright contract must be rejected loudly"
        );
    }
    assert!(validate_adjustments(&base).is_ok());
}
