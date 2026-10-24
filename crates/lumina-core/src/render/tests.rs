use super::*;
use crate::tone::{analyze_tone, match_total_exposure, suggest_auto_tone, AutoToneConfig};
use lumina_sidecar::{
    AnalysisFingerprint, AutoFeatures, CoordinateSystem, DecodeFingerprint, Extras,
    GeometryFingerprint, MaskDefinition, MaskLayer, MaskOperation, MaskStatus, ModelIdentity,
    Preprocessing, Resolution, SourceFingerprint,
};

fn mask_definition(
    id: &str,
    status: MaskStatus,
    operation: MaskOperation,
    references: Vec<lumina_sidecar::MaskReference>,
) -> MaskDefinition {
    MaskDefinition {
        id: id.into(),
        name: id.into(),
        source_fingerprint: SourceFingerprint {
            content_hash: "h".into(),
            byte_length: 1,
            extras: Extras::new(),
        },
        decode_context: DecodeFingerprint {
            decoder: "d".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: Extras::new(),
        },
        geometry_context: GeometryFingerprint {
            width: 2,
            height: 1,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: Extras::new(),
        },
        model: ModelIdentity {
            name: "m".into(),
            version: "1".into(),
            hash: "h".into(),
            extras: Extras::new(),
        },
        inference_resolution: Resolution {
            width: 2,
            height: 1,
            extras: Extras::new(),
        },
        preprocessing: Preprocessing {
            name: "p".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: Extras::new(),
        },
        rescaling_method: "none".into(),
        rescaling_parameters: BTreeMap::new(),
        coordinate_system: CoordinateSystem::SourceOriented,
        status,
        created_at: "now".into(),
        generator_version: "g".into(),
        error_text: None,
        artifact: None,
        operation,
        references,
        prompt: None,
        extras: Extras::new(),
        ai_select: None,
    }
}

fn reference(copy_id: &str, mask_id: &str) -> lumina_sidecar::MaskReference {
    lumina_sidecar::MaskReference {
        copy_id: copy_id.into(),
        mask_id: mask_id.into(),
        extras: Extras::new(),
    }
}

fn copy_with(id: &str, definitions: Vec<MaskDefinition>, layers: Vec<MaskLayer>) -> VirtualCopy {
    VirtualCopy {
        id: id.into(),
        name: id.into(),
        is_default: id == "vc-original",
        rating: 0,
        flag: lumina_sidecar::Flag::Unflagged,
        recipe: EditRecipe::default(),
        mask_library: definitions,
        mask_layers: layers,
        history: vec![],
        export_records: vec![],
        extras: Extras::new(),
    }
}

fn layer(id: &str, mask: lumina_sidecar::MaskReference) -> MaskLayer {
    MaskLayer {
        id: id.into(),
        mask,
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        extras: Extras::new(),
        visible: true,
        local_adjustments: None,
    }
}

fn base_frame() -> ImageFrame {
    ImageFrame::new(2, 2, vec![100; 16]).unwrap()
}

fn default_context<'a>(
    recipe: &'a EditRecipe,
    masks: Option<MaskContext<'a>>,
) -> RenderContext<'a> {
    RenderContext {
        recipe,
        camera_white_balance: None,
        source_actions: &[],
        lensfun: None,
        depth: None,
        masks,
    }
}

// ---- Masks stage ----

fn mask_context<'a>(
    copies: &'a [VirtualCopy],
    active_copy_id: &'a str,
    planes: BTreeMap<(String, String), MaskPlane>,
    policy: MaskPolicy,
) -> MaskContext<'a> {
    MaskContext {
        copies,
        active_copy_id,
        planes,
        policy,
        source_roi: None,
    }
}

#[test]
fn valid_layer_is_resampled_to_frame_dimensions() {
    let definitions = vec![mask_definition(
        "subject",
        MaskStatus::Valid,
        MaskOperation::Source,
        vec![],
    )];
    let copies = vec![copy_with(
        "vc",
        definitions,
        vec![layer("layer-1", reference("vc", "subject"))],
    )];
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(1, 1, vec![32768]).unwrap(),
    )]);
    let recipe = EditRecipe::default();
    let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
    let output = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(mask_context(&copies, "vc", planes, MaskPolicy::Strict)),
        },
    )
    .unwrap();
    assert_eq!(output.mask_layers.len(), 1);
    let result = &output.mask_layers[0];
    assert_eq!(result.layer_id, "layer-1");
    assert_eq!((result.plane.width, result.plane.height), (2, 2));
    assert_eq!(result.plane.values, vec![32768; 4]);
    assert!(output.mask_warnings.is_empty());
}

#[path = "tests/local_adjustments_tests.rs"]
mod local_adjustments;

#[path = "tests/local_white_balance_tests.rs"]
mod local_white_balance;

#[path = "tests/local_tone_curve_tests.rs"]
mod local_tone_curve;

#[path = "tests/local_tone_curve_contract_tests.rs"]
mod local_tone_curve_contract;

#[path = "tests/local_color_tests.rs"]
mod local_color;

#[path = "tests/local_point_color_grading_tests.rs"]
mod local_point_color_grading;

#[path = "tests/local_color_contract_tests.rs"]
mod local_color_contract;

#[path = "tests/local_presence_tests.rs"]
mod local_presence;

#[path = "tests/local_presence_contract_tests.rs"]
mod local_presence_contract;

#[path = "tests/local_presence_boundary_tests.rs"]
mod local_presence_boundary;

#[path = "tests/local_detail_goldens.rs"]
mod local_detail_goldens;

#[path = "tests/local_detail_tests.rs"]
mod local_detail;

#[path = "tests/local_detail_state_tests.rs"]
mod local_detail_state;

#[path = "tests/local_detail_boundary_tests.rs"]
mod local_detail_boundary;

#[path = "tests/local_detail_reference.rs"]
mod local_detail_reference;

#[path = "tests/local_detail_order_tests.rs"]
mod local_detail_order;

#[path = "tests/local_detail_recipe_tests.rs"]
mod local_detail_recipe;

#[path = "tests/tca_isolation_tests.rs"]
mod tca_isolation;

#[path = "tests/source_action_apply_tests.rs"]
mod source_action_apply;

#[path = "tests/source_action_contract_tests.rs"]
mod source_action_contract;

#[path = "tests/lensfun_corrector_tests.rs"]
mod lensfun_corrector;

#[test]
fn invisible_layer_is_skipped_silently() {
    let definitions = vec![
        mask_definition("shown", MaskStatus::Valid, MaskOperation::Source, vec![]),
        mask_definition("hidden", MaskStatus::Valid, MaskOperation::Source, vec![]),
    ];
    let mut hidden_layer = layer("layer-hidden", reference("vc", "hidden"));
    hidden_layer.visible = false;
    let copies = vec![copy_with(
        "vc",
        definitions,
        vec![layer("layer-shown", reference("vc", "shown")), hidden_layer],
    )];
    let planes = BTreeMap::from([
        (
            ("vc".into(), "shown".into()),
            MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
        ),
        (
            ("vc".into(), "hidden".into()),
            MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
        ),
    ]);
    let recipe = EditRecipe::default();
    let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
    let output = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(mask_context(&copies, "vc", planes, MaskPolicy::Strict)),
        },
    )
    .unwrap();
    // The eye is an explicit user choice: the hidden layer vanishes
    // without warnings (Strict still passes) and only the shown plane
    // reaches the output.
    assert_eq!(output.mask_layers.len(), 1);
    assert_eq!(output.mask_layers[0].layer_id, "layer-shown");
    assert!(output.mask_warnings.is_empty());
}

#[test]
fn range_mask_plane_matches_deterministic_reference() {
    use lumina_sidecar::{MaskPrompt, PromptTransform};
    let mut definition = mask_definition("lum", MaskStatus::Valid, MaskOperation::Source, vec![]);
    let prompt = MaskPrompt::LuminanceRange {
        min: 0.5,
        max: 1.0,
        feather: 0.0,
        transformation: PromptTransform::default(),
    };
    definition.prompt = Some(prompt.clone());
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![layer("layer-lum", reference("vc", "lum"))],
    )];
    // Black/white checker frame: the loader-computed plane is the input
    // here; the render stage must pass it through byte-identical
    // (PSNR-equivalent: zero deviation, no silent resample artefacts).
    let frame = ImageFrame::new(2, 1, vec![0, 0, 0, 255, 255, 255, 255, 255]).unwrap();
    let expected = crate::range_masks::evaluate_range_prompt(&frame, &prompt).unwrap();
    assert_eq!(expected.values, vec![0, u16::MAX]);
    let planes = BTreeMap::from([(("vc".into(), "lum".into()), expected.clone())]);
    let recipe = EditRecipe::default();
    let output = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(mask_context(&copies, "vc", planes, MaskPolicy::Strict)),
        },
    )
    .unwrap();
    assert_eq!(output.mask_layers.len(), 1);
    assert_eq!(output.mask_layers[0].plane.values, expected.values);
    assert!(output.mask_warnings.is_empty());
}

#[test]
fn missing_plane_is_strict_error_and_warn_ok() {
    let definitions = vec![mask_definition(
        "subject",
        MaskStatus::Valid,
        MaskOperation::Source,
        vec![],
    )];
    let copies = vec![copy_with(
        "vc",
        definitions,
        vec![layer("layer-1", reference("vc", "subject"))],
    )];
    let recipe = EditRecipe::default();
    let frame = base_frame();
    let strict = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(mask_context(
                &copies,
                "vc",
                BTreeMap::new(),
                MaskPolicy::Strict,
            )),
        },
    )
    .unwrap_err();
    assert!(matches!(
        strict,
        CoreError::MaskEvaluation {
            ref copy_id,
            ref mask_id,
            ..
        } if copy_id == "vc" && mask_id == "subject"
    ));

    let warn = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(mask_context(
                &copies,
                "vc",
                BTreeMap::new(),
                MaskPolicy::Warn,
            )),
        },
    )
    .unwrap();
    assert!(warn.mask_layers.is_empty());
    assert_eq!(warn.mask_warnings.len(), 1);
    assert!(warn.mask_warnings[0].contains("layer-1"));
    assert_eq!(warn.frame, frame);
}

#[test]
fn non_valid_definition_is_strict_error_and_warn_ok() {
    let definitions = vec![mask_definition(
        "subject",
        MaskStatus::Missing,
        MaskOperation::Source,
        vec![],
    )];
    let copies = vec![copy_with(
        "vc",
        definitions,
        vec![layer("layer-1", reference("vc", "subject"))],
    )];
    let recipe = EditRecipe::default();
    let frame = base_frame();
    let strict = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(mask_context(
                &copies,
                "vc",
                BTreeMap::new(),
                MaskPolicy::Strict,
            )),
        },
    )
    .unwrap_err();
    assert!(matches!(
        strict,
        CoreError::MaskUnavailable {
            ref copy_id,
            ref mask_id,
            ref status,
        } if copy_id == "vc" && mask_id == "subject" && status == "Missing"
    ));

    let warn = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(mask_context(
                &copies,
                "vc",
                BTreeMap::new(),
                MaskPolicy::Warn,
            )),
        },
    )
    .unwrap();
    assert!(warn.mask_layers.is_empty());
    assert_eq!(warn.mask_warnings.len(), 1);
    assert!(warn.mask_warnings[0].contains("unavailable"));
}

#[test]
fn missing_definition_is_strict_error_and_warn_ok() {
    // The layer references a mask that exists in no copy at all.
    let copies = vec![copy_with(
        "vc",
        vec![],
        vec![layer("layer-1", reference("vc", "ghost"))],
    )];
    let recipe = EditRecipe::default();
    let frame = base_frame();
    let strict = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(mask_context(
                &copies,
                "vc",
                BTreeMap::new(),
                MaskPolicy::Strict,
            )),
        },
    )
    .unwrap_err();
    assert!(matches!(
        strict,
        CoreError::MaskUnavailable { ref mask_id, .. } if mask_id == "ghost"
    ));

    let warn = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(mask_context(
                &copies,
                "vc",
                BTreeMap::new(),
                MaskPolicy::Warn,
            )),
        },
    )
    .unwrap();
    assert_eq!(warn.mask_warnings.len(), 1);
}

#[test]
fn invalid_density_is_warn_skip_and_strict_error() {
    // REVIEW-MASK-N2: an out-of-range layer density must never silently
    // erase (density < 0) or ignore (density > 1) the mask. Warn skips
    // the layer with a recorded message; Strict aborts the render.
    let definitions = vec![mask_definition(
        "subject",
        MaskStatus::Valid,
        MaskOperation::Source,
        vec![],
    )];
    let copies = vec![copy_with(
        "vc",
        definitions,
        vec![MaskLayer {
            density: -0.5,
            ..layer("layer-1", reference("vc", "subject"))
        }],
    )];
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
    )]);
    let recipe = EditRecipe::default();
    let frame = base_frame();

    let warn = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(mask_context(
                &copies,
                "vc",
                planes.clone(),
                MaskPolicy::Warn,
            )),
        },
    )
    .unwrap();
    assert!(warn.mask_layers.is_empty());
    assert_eq!(warn.mask_warnings.len(), 1);
    assert!(warn.mask_warnings[0].contains("modulated"));
    assert!(warn.mask_warnings[0].contains("density"));
    assert_eq!(warn.frame, frame);

    let strict = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(mask_context(&copies, "vc", planes, MaskPolicy::Strict)),
        },
    )
    .unwrap_err();
    assert!(matches!(
        strict,
        CoreError::MaskEvaluation { ref reason, .. } if reason.contains("density")
    ));
}

#[test]
fn strict_policy_fails_the_whole_render_not_partial_layers() {
    // REVIEW-MASK-STRICT-1 (core semantics): under Strict a render with
    // ANY unavailable layer is an error — no partial `mask_layers` result
    // is produced and no warning list is emitted. The valid first layer
    // must not leak into the error output as if the render succeeded.
    let definitions = vec![
        mask_definition("good", MaskStatus::Valid, MaskOperation::Source, vec![]),
        mask_definition("ghost", MaskStatus::Missing, MaskOperation::Source, vec![]),
    ];
    let copies = vec![copy_with(
        "vc",
        definitions,
        vec![
            layer("layer-good", reference("vc", "good")),
            layer("layer-bad", reference("vc", "ghost")),
        ],
    )];
    let planes = BTreeMap::from([(
        ("vc".into(), "good".into()),
        MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
    )]);
    let recipe = EditRecipe::default();
    let strict = render_frame(
        &base_frame(),
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(mask_context(&copies, "vc", planes, MaskPolicy::Strict)),
        },
    );
    match strict {
        Err(CoreError::MaskUnavailable {
            ref mask_id,
            ref status,
            ..
        }) => {
            assert_eq!(mask_id, "ghost");
            assert_eq!(status, "Missing");
        }
        other => panic!("expected strict MaskUnavailable error, got {other:?}"),
    }

    // Control: the same context under Warn keeps the good layer and
    // records exactly one warning for the bad one.
    let warn_planes = BTreeMap::from([(
        ("vc".into(), "good".into()),
        MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
    )]);
    let warn = render_frame(
        &base_frame(),
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(mask_context(&copies, "vc", warn_planes, MaskPolicy::Warn)),
        },
    )
    .unwrap();
    assert_eq!(warn.mask_layers.len(), 1);
    assert_eq!(warn.mask_layers[0].layer_id, "layer-good");
    assert_eq!(warn.mask_warnings.len(), 1);
}

mod pipeline;
