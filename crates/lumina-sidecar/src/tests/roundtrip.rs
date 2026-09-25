use super::*;

#[test]
fn complete_roundtrip() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.analysis_fingerprint = Some(AnalysisFingerprint {
        algorithm: "scene-analysis".into(),
        version: "2.1".into(),
        input_fingerprint: "sha256:analysis-input".into(),
        extras: Extras::from([("analysis_extra".into(), Value::from(true))]),
    });
    d.extras
        .insert("future_root".into(), Value::from("preserved"));

    let mut source_mask = mask("a");
    source_mask
        .source_fingerprint
        .extras
        .insert("future_source_fingerprint".into(), Value::from(7));
    source_mask
        .decode_context
        .parameters
        .insert("quality".into(), "high".into());
    source_mask.geometry_context.pixel_aspect_ratio = 1.25;
    source_mask
        .model
        .extras
        .insert("future_model".into(), Value::from("kept"));
    source_mask.inference_resolution.width = 512;
    source_mask
        .preprocessing
        .parameters
        .insert("mean".into(), "0.5".into());
    source_mask.rescaling_method = "lanczos".into();
    source_mask
        .rescaling_parameters
        .insert("radius".into(), "3".into());
    source_mask.coordinate_system = CoordinateSystem::Normalized;
    source_mask.status = MaskStatus::Corrupt;
    source_mask.created_at = "2026-02-03T04:05:06Z".into();
    source_mask.generator_version = "segmenter-2.4".into();
    source_mask.error_text = Some("model output checksum mismatch".into());
    source_mask.artifact = Some(ArtifactReference {
        relative_path: "masks/a.zdata".into(),
        format: "zdata-mask".into(),
        checksum: "sha256:artifact".into(),
        width: 512,
        height: 256,
        channels: "f32".into(),
        data_version: "1".into(),
        extras: Extras::new(),
    });
    source_mask
        .extras
        .insert("future_mask".into(), Value::from("kept"));
    d.virtual_copies[0].mask_library.push(source_mask);
    d.virtual_copies.push(VirtualCopy {
        id: "vc-bw".into(),
        name: "B&W".into(),
        is_default: false,
        rating: 0,
        flag: Flag::Unflagged,
        recipe: EditRecipe {
            recipe_version: "1".into(),
            adjustments: BTreeMap::from([("exposure".into(), 1.25)]),
            curves: None,
            hsl: None,
            point_color: None,
            color_grading: None,
            presence: None,
            noise_reduction: None,
            denoise_ai: None,
            sharpening: None,
            red_eye: None,
            geometry: None,
            lens_correction: None,
            perspective: None,
            upright: None,
            effects: None,
            lens_blur: None,
            generative_edit: None,
            source_actions: Vec::new(),
            spot_removals: Vec::new(),
            options: BTreeMap::from([("profile".into(), "neutral".into())]),
            auto_features: AutoFeatures::default(),
            extras: Extras::from([("future_recipe".into(), Value::from(42))]),
        },
        mask_library: vec![],
        mask_layers: vec![MaskLayer {
            id: "layer".into(),
            mask: MaskReference {
                copy_id: "vc-original".into(),
                mask_id: "a".into(),
                extras: Extras::new(),
            },
            inverted: false,
            feather: 0.0,
            blur: 0.0,
            density: 1.0,
            extras: Extras::new(),
            visible: true,
            local_adjustments: None,
        }],
        history: vec![HistoryEntry {
            id: "h".into(),
            recipe: EditRecipe {
                recipe_version: "1".into(),
                adjustments: BTreeMap::from([("contrast".into(), -0.4)]),
                curves: None,
                hsl: None,
                point_color: None,
                color_grading: None,
                presence: None,
                noise_reduction: None,
                denoise_ai: None,
                sharpening: None,
                red_eye: None,
                geometry: None,
                lens_correction: None,
                perspective: None,
                upright: None,
                effects: None,
                lens_blur: None,
                generative_edit: None,
                source_actions: Vec::new(),
                spot_removals: Vec::new(),
                options: BTreeMap::from([("source".into(), "preset".into())]),
                auto_features: AutoFeatures::default(),
                extras: Extras::new(),
            },
            recorded_at: Some("2026-02-03T04:05:06Z".into()),
            extras: Extras::from([("future_history".into(), Value::from(true))]),
        }],
        export_records: vec![ExportRecord {
            id: "e".into(),
            relative_path: "exports/out.jpg".into(),
            format: "jpeg".into(),
            exported_at: Some("2026-02-03T04:06:06Z".into()),
            extras: Extras::from([("future_export".into(), Value::from("kept"))]),
        }],
        extras: Extras::from([("future_copy".into(), Value::from(true))]),
    });
    d.presets.push(Preset {
        id: "preset-1".into(),
        name: "Monochrome Contrast".into(),
        recipe: EditRecipe {
            recipe_version: "1".into(),
            adjustments: BTreeMap::from([("highlights".into(), -0.75)]),
            curves: None,
            hsl: None,
            point_color: None,
            color_grading: None,
            presence: None,
            noise_reduction: None,
            denoise_ai: None,
            sharpening: None,
            red_eye: None,
            geometry: None,
            lens_correction: None,
            perspective: None,
            upright: None,
            effects: None,
            lens_blur: None,
            generative_edit: None,
            source_actions: Vec::new(),
            spot_removals: Vec::new(),
            options: BTreeMap::from([("curve".into(), "film".into())]),
            auto_features: AutoFeatures::default(),
            extras: Extras::new(),
        },
        extras: Extras::new(),
    });
    let json = d.to_json().unwrap();
    assert_eq!(d, SidecarDocument::from_json(&json).unwrap());
}

#[test]
fn empty_sidecar_roundtrip() {
    let d = SidecarDocument::new(source(), "pipeline-1");
    let json = d.to_json().unwrap();
    assert_eq!(d, SidecarDocument::from_json(&json).unwrap());
}

#[test]
fn auto_features_roundtrip_with_result_and_fingerprint() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    let features = &mut d.virtual_copies[0].recipe.auto_features;
    features.enable_auto_tone = true;
    features.match_total_exposure = true;
    features.target_luminance = 0.42;
    features.auto_exposure = Some(1.25);
    features.auto_contrast = Some(-0.2);
    features.auto_whites = Some(0.35);
    features.auto_blacks = Some(-0.45);
    features.auto_highlights = Some(0.15);
    features.auto_shadows = Some(-0.25);
    features.matched_exposure = Some(0.5);
    features.analysis_fingerprint = Some(AnalysisFingerprint {
        algorithm: "tone-rgba8-rec709".into(),
        version: "1".into(),
        input_fingerprint: "tone-rgba8-rec709:abc".into(),
        extras: Extras::new(),
    });
    let json = d.to_json().unwrap();
    assert!(json.contains("auto_exposure"));
    assert!(json.contains("auto_whites"));
    assert!(json.contains("auto_blacks"));
    assert!(json.contains("auto_highlights"));
    assert!(json.contains("auto_shadows"));
    assert!(json.contains("tone-rgba8-rec709:abc"));
    assert_eq!(d, SidecarDocument::from_json(&json).unwrap());
    assert!(d.validate().is_ok());
}
