use super::*;

// =====================================================================
// F-079: prompt-capable mask sources in the mask DAG data model.
// =====================================================================

#[test]
fn mask_prompt_variants_serde_roundtrip() {
    let cases: Vec<MaskPrompt> = vec![
        MaskPrompt::Box {
            rect: NormalizedRect {
                x: 0.1,
                y: 0.2,
                width: 0.5,
                height: 0.6,
            },
            transformation: PromptTransform::default(),
        },
        MaskPrompt::Brush {
            marks: vec![BrushMark {
                x: 0.5,
                y: 0.5,
                radius: 0.2,
                sign: BrushMarkSign::Positive,
                softness: 0.35,
                flow: 0.7,
            }],
            resolution: (64, 64),
            transformation: PromptTransform {
                method: "brush-to-points".into(),
                parameters: BTreeMap::from([("include_negatives".into(), "true".into())]),
            },
        },
        MaskPrompt::Polygon {
            points: vec![
                Point2 { x: 0.0, y: 0.0 },
                Point2 { x: 1.0, y: 0.0 },
                Point2 { x: 0.5, y: 1.0 },
            ],
            transformation: PromptTransform::default(),
        },
        MaskPrompt::Ellipse {
            center: Point2 { x: 0.5, y: 0.5 },
            radii: Point2 { x: 0.3, y: 0.4 },
            transformation: PromptTransform::default(),
        },
        MaskPrompt::Gradient {
            angle_deg: 45.0,
            start: 0.0,
            end: 1.0,
            transformation: PromptTransform::default(),
        },
    ];
    for original in &cases {
        let json = serde_json::to_string(original).unwrap();
        let decoded: MaskPrompt = serde_json::from_str(&json).unwrap();
        assert_eq!(original, &decoded, "prompt roundtrip failed for {json}");
    }
}

#[test]
fn brush_softness_and_flow_are_additive_roundtrip_values() {
    let legacy: BrushMark =
        serde_json::from_str(r#"{"x":0.5,"y":0.5,"radius":0.1,"sign":"positive"}"#)
            .expect("legacy brush mark remains readable");
    assert_eq!(legacy.softness, 0.0);
    assert_eq!(legacy.flow, 1.0);

    let current = BrushMark {
        softness: 0.4,
        flow: 0.65,
        ..legacy
    };
    let json = serde_json::to_string(&current).unwrap();
    assert!(json.contains("\"softness\":0.4"));
    assert!(json.contains("\"flow\":0.65"));
    assert_eq!(serde_json::from_str::<BrushMark>(&json).unwrap(), current);
}

#[test]
fn brush_softness_and_flow_are_loudly_range_validated() {
    for (softness, flow) in [(f32::NAN, 1.0), (-0.01, 1.0), (0.5, 1.01)] {
        let mut document = SidecarDocument::new(source(), "p");
        let mut definition = mask("bad-brush-controls");
        definition.prompt = Some(MaskPrompt::Brush {
            marks: vec![BrushMark {
                x: 0.5,
                y: 0.5,
                radius: 0.1,
                sign: BrushMarkSign::Positive,
                softness,
                flow,
            }],
            resolution: (64, 64),
            transformation: PromptTransform::default(),
        });
        document.virtual_copies[0].mask_library.push(definition);
        assert!(document.validate().is_err());
    }
}

fn brush_sidecar_json(resolution: (u32, u32)) -> String {
    let mut document = SidecarDocument::new(source(), "p");
    let mut definition = mask("brush-resolution");
    definition.prompt = Some(MaskPrompt::Brush {
        marks: vec![BrushMark {
            x: 0.5,
            y: 0.5,
            radius: 0.1,
            sign: BrushMarkSign::Positive,
            softness: 0.0,
            flow: 1.0,
        }],
        resolution: (64, 64),
        transformation: PromptTransform::default(),
    });
    document.virtual_copies[0].mask_library.push(definition);
    let mut json: serde_json::Value = document.to_json().unwrap().parse().unwrap();
    json["virtual_copies"][0]["mask_library"][0]["prompt"]["brush"]["resolution"] =
        serde_json::json!([resolution.0, resolution.1]);
    json.to_string()
}

fn assert_brush_resolution_sidecar_is_invalid(resolution: (u32, u32)) {
    let error = SidecarDocument::from_json(&brush_sidecar_json(resolution)).unwrap_err();
    assert!(
        matches!(&error, SidecarError::Invalid(message) if message.contains("brush resolution")),
        "unexpected error for {resolution:?}: {error}"
    );
}

#[test]
fn brush_zero_width_resolution_rejects_invalid_sidecar() {
    assert_brush_resolution_sidecar_is_invalid((0, 64));
}

#[test]
fn brush_zero_height_resolution_rejects_invalid_sidecar() {
    assert_brush_resolution_sidecar_is_invalid((64, 0));
}

#[test]
fn valid_legacy_brush_sidecar_without_softness_or_flow_remains_readable() {
    let mut legacy: serde_json::Value = serde_json::from_str(&brush_sidecar_json((1, 1))).unwrap();
    let mark = legacy["virtual_copies"][0]["mask_library"][0]["prompt"]["brush"]["marks"][0]
        .as_object_mut()
        .unwrap();
    mark.remove("softness");
    mark.remove("flow");
    assert!(SidecarDocument::from_json(&legacy.to_string()).is_ok());
}

#[test]
fn mask_definition_with_prompt_roundtrips_through_document() {
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    let mut prompt_mask = mask("prompta");
    prompt_mask.prompt = Some(MaskPrompt::Box {
        rect: NormalizedRect {
            x: 0.25,
            y: 0.25,
            width: 0.5,
            height: 0.5,
        },
        transformation: PromptTransform {
            method: "normalize".into(),
            parameters: BTreeMap::from([("scale".into(), "1".into())]),
        },
    });
    d.virtual_copies[0].mask_library.push(prompt_mask);
    let json = d.to_json().unwrap();
    assert!(json.contains("prompt"));
    assert!(json.contains("\"box\""));
    // The prompt is stored as part of the mask identity (next to the node).
    let decoded = SidecarDocument::from_json(&json).unwrap();
    assert_eq!(decoded, d);
}

#[test]
fn mask_prompt_out_of_range_is_rejected() {
    // Box with a coordinate outside [0,1].
    let mut d = SidecarDocument::new(source(), "p");
    let mut m = mask("bad");
    m.prompt = Some(MaskPrompt::Box {
        rect: NormalizedRect {
            x: -0.1,
            y: 0.0,
            width: 0.5,
            height: 0.5,
        },
        transformation: PromptTransform::default(),
    });
    d.virtual_copies[0].mask_library.push(m);
    assert!(d.validate().is_err());

    // Box with a zero width.
    let mut d2 = SidecarDocument::new(source(), "p");
    let mut m2 = mask("bad2");
    m2.prompt = Some(MaskPrompt::Box {
        rect: NormalizedRect {
            x: 0.1,
            y: 0.1,
            width: 0.0,
            height: 0.5,
        },
        transformation: PromptTransform::default(),
    });
    d2.virtual_copies[0].mask_library.push(m2);
    assert!(d2.validate().is_err());

    // Polygon with an out-of-range point.
    let mut d3 = SidecarDocument::new(source(), "p");
    let mut m3 = mask("bad3");
    m3.prompt = Some(MaskPrompt::Polygon {
        points: vec![Point2 { x: 0.0, y: 0.0 }, Point2 { x: 2.0, y: 0.5 }],
        transformation: PromptTransform::default(),
    });
    d3.virtual_copies[0].mask_library.push(m3);
    assert!(d3.validate().is_err());

    // Gradient with a finite-but-out-of-range end value.
    let mut d4 = SidecarDocument::new(source(), "p");
    let mut m4 = mask("bad4");
    m4.prompt = Some(MaskPrompt::Gradient {
        angle_deg: 0.0,
        start: 0.0,
        end: 1.5,
        transformation: PromptTransform::default(),
    });
    d4.virtual_copies[0].mask_library.push(m4);
    assert!(d4.validate().is_err());

    // Brush with empty marks is rejected.
    let mut d5 = SidecarDocument::new(source(), "p");
    let mut m5 = mask("bad5");
    m5.prompt = Some(MaskPrompt::Brush {
        marks: vec![],
        resolution: (512, 512),
        transformation: PromptTransform::default(),
    });
    d5.virtual_copies[0].mask_library.push(m5);
    assert!(d5.validate().is_err());

    // Brush with a non-positive radius is rejected.
    let mut d6 = SidecarDocument::new(source(), "p");
    let mut m6 = mask("bad6");
    m6.prompt = Some(MaskPrompt::Brush {
        marks: vec![BrushMark {
            x: 0.5,
            y: 0.5,
            radius: 0.0,
            sign: BrushMarkSign::Positive,
            softness: 0.0,
            flow: 1.0,
        }],
        resolution: (512, 512),
        transformation: PromptTransform::default(),
    });
    d6.virtual_copies[0].mask_library.push(m6);
    assert!(d6.validate().is_err());

    // Non-finite (NaN) coordinate is rejected.
    let mut d7 = SidecarDocument::new(source(), "p");
    let mut m7 = mask("bad7");
    m7.prompt = Some(MaskPrompt::Box {
        rect: NormalizedRect {
            x: f32::NAN,
            y: 0.0,
            width: 0.5,
            height: 0.5,
        },
        transformation: PromptTransform::default(),
    });
    d7.virtual_copies[0].mask_library.push(m7);
    assert!(d7.validate().is_err());

    // Valid prompt is accepted.
    let mut ok = SidecarDocument::new(source(), "p");
    let mut good = mask("good");
    good.prompt = Some(MaskPrompt::Ellipse {
        center: Point2 { x: 0.5, y: 0.5 },
        radii: Point2 { x: 0.3, y: 0.3 },
        transformation: PromptTransform::default(),
    });
    ok.virtual_copies[0].mask_library.push(good);
    assert!(ok.validate().is_ok());
}

// ----- G-03 Maskierungs-Parität: AiSelect, Range-Prompts, visible -----

#[test]
fn ai_select_kind_parse_roundtrip() {
    for kind in AiSelectKind::all() {
        assert_eq!(AiSelectKind::parse(kind.as_str()), Some(kind));
    }
    // Case-insensitive reads; unknown kinds stay unknown (no guessing).
    assert_eq!(AiSelectKind::parse("Subject"), Some(AiSelectKind::Subject));
    assert_eq!(AiSelectKind::parse("SKY"), Some(AiSelectKind::Sky));
    assert_eq!(AiSelectKind::parse("people"), Some(AiSelectKind::People));
    assert_eq!(AiSelectKind::parse("cat"), None);
    assert_eq!(AiSelectKind::parse(""), None);
}

#[test]
fn ai_select_roundtrip_and_validation() {
    let mut d = SidecarDocument::new(source(), "p");
    let mut m = mask("ai");
    m.ai_select = Some(AiSelect {
        kind: AiSelectKind::People,
        detail: Some("pupil".into()),
        extras: BTreeMap::new(),
    });
    d.virtual_copies[0].mask_library.push(m);
    assert!(d.validate().is_ok());
    let json = d.to_json().unwrap();
    assert!(json.contains("ai_select"));
    assert!(json.contains("\"people\""));
    let decoded = SidecarDocument::from_json(&json).unwrap();
    assert_eq!(decoded, d);

    // Every documented part validates.
    for part in AI_SELECT_KNOWN_PARTS {
        let mut d = SidecarDocument::new(source(), "p");
        let mut m = mask("ai");
        m.ai_select = Some(AiSelect {
            kind: AiSelectKind::Subject,
            detail: Some((*part).into()),
            extras: BTreeMap::new(),
        });
        d.virtual_copies[0].mask_library.push(m);
        assert!(d.validate().is_ok(), "part `{part}` must validate");
    }

    // Untrimmed, empty, overlong and control-char details are rejected.
    for bad in [" face", "face ", "", "a".repeat(65).as_str(), "fa\tce"] {
        let mut d = SidecarDocument::new(source(), "p");
        let mut m = mask("bad");
        m.ai_select = Some(AiSelect {
            kind: AiSelectKind::Sky,
            detail: Some(bad.into()),
            extras: BTreeMap::new(),
        });
        d.virtual_copies[0].mask_library.push(m);
        assert!(d.validate().is_err(), "detail `{bad:?}` must be rejected");
    }
}

#[test]
fn ai_select_on_derived_node_is_rejected() {
    let mut d = SidecarDocument::new(source(), "p");
    d.virtual_copies[0].mask_library.push(mask("a"));
    d.virtual_copies[0].mask_library.push(mask("b"));
    let mut derived = mask("combo");
    derived.operation = MaskOperation::Union;
    derived.references = vec![
        MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "a".into(),
            extras: BTreeMap::new(),
        },
        MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "b".into(),
            extras: BTreeMap::new(),
        },
    ];
    derived.ai_select = Some(AiSelect {
        kind: AiSelectKind::Subject,
        detail: None,
        extras: BTreeMap::new(),
    });
    d.virtual_copies[0].mask_library.push(derived);
    let error = d.validate().unwrap_err().to_string();
    assert!(error.contains("ai_select"), "unexpected error: {error}");
}

#[test]
fn range_prompts_roundtrip_and_validation() {
    let mut d = SidecarDocument::new(source(), "p");
    let mut lum = mask("lum");
    lum.prompt = Some(MaskPrompt::LuminanceRange {
        min: 0.2,
        max: 0.8,
        feather: 0.5,
        transformation: PromptTransform::default(),
    });
    let mut col = mask("col");
    col.prompt = Some(MaskPrompt::ColorRange {
        hue_center: 120.0,
        hue_width: 60.0,
        sat_min: 0.1,
        sat_max: 0.9,
        lum_min: 0.0,
        lum_max: 1.0,
        feather: 0.25,
        transformation: PromptTransform::default(),
    });
    d.virtual_copies[0].mask_library.push(lum);
    d.virtual_copies[0].mask_library.push(col);
    assert!(d.validate().is_ok());
    let json = d.to_json().unwrap();
    assert!(json.contains("luminancerange") || json.contains("luminance_range"));
    let decoded = SidecarDocument::from_json(&json).unwrap();
    assert_eq!(decoded, d);

    // min > max is rejected.
    let mut bad = SidecarDocument::new(source(), "p");
    let mut m = mask("bad");
    m.prompt = Some(MaskPrompt::LuminanceRange {
        min: 0.9,
        max: 0.1,
        feather: 0.0,
        transformation: PromptTransform::default(),
    });
    bad.virtual_copies[0].mask_library.push(m);
    assert!(bad.validate().is_err());

    // hue out of degrees is rejected.
    let mut bad2 = SidecarDocument::new(source(), "p");
    let mut m2 = mask("bad2");
    m2.prompt = Some(MaskPrompt::ColorRange {
        hue_center: 400.0,
        hue_width: 60.0,
        sat_min: 0.0,
        sat_max: 1.0,
        lum_min: 0.0,
        lum_max: 1.0,
        feather: 0.0,
        transformation: PromptTransform::default(),
    });
    bad2.virtual_copies[0].mask_library.push(m2);
    assert!(bad2.validate().is_err());

    // A range prompt on a derived node is rejected.
    let mut bad3 = SidecarDocument::new(source(), "p");
    bad3.virtual_copies[0].mask_library.push(mask("a"));
    let mut inv = mask("inv");
    inv.operation = MaskOperation::Invert;
    inv.references = vec![MaskReference {
        copy_id: "vc-original".into(),
        mask_id: "a".into(),
        extras: BTreeMap::new(),
    }];
    inv.prompt = Some(MaskPrompt::LuminanceRange {
        min: 0.0,
        max: 1.0,
        feather: 0.0,
        transformation: PromptTransform::default(),
    });
    bad3.virtual_copies[0].mask_library.push(inv);
    let error = bad3.validate().unwrap_err().to_string();
    assert!(error.contains("range prompt"), "unexpected error: {error}");
}

#[test]
fn mask_layer_visible_defaults_true_and_roundtrips() {
    // Legacy JSON without `visible` reads as visible (identity).
    let json = serde_json::json!({
        "id": "layer",
        "mask": {"copy_id": "vc-original", "mask_id": "a"},
        "inverted": false,
        "feather": 0.0,
        "blur": 0.0,
        "density": 1.0
    });
    let layer: MaskLayer = serde_json::from_value(json).unwrap();
    assert!(layer.visible);

    // Explicit false survives a full document roundtrip.
    let mut d = SidecarDocument::new(source(), "p");
    d.virtual_copies[0].mask_library.push(mask("a"));
    d.virtual_copies[0].mask_layers.push(MaskLayer {
        id: "layer".into(),
        mask: MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "a".into(),
            extras: BTreeMap::new(),
        },
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        visible: false,
        local_adjustments: None,
        extras: BTreeMap::new(),
    });
    assert!(d.validate().is_ok());
    let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(decoded, d);
    assert!(!decoded.virtual_copies[0].mask_layers[0].visible);
}
