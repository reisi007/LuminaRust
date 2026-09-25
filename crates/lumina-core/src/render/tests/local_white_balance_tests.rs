//! MASK-LOCAL-P1.1 CPU compositor goldens.

use super::local_adjustments::local_render;
use super::*;
use lumina_sidecar::LocalAdjustments;

fn local_wb_layer(
    id: &str,
    mask: lumina_sidecar::MaskReference,
    temperature_delta_k: f64,
    tint_delta: f64,
    exposure: f64,
) -> MaskLayer {
    let mut layer = layer(id, mask);
    layer.local_adjustments = Some(LocalAdjustments {
        version: lumina_sidecar::LOCAL_ADJUSTMENTS_VERSION,
        exposure,
        temperature_delta_k,
        tint_delta,
        ..LocalAdjustments::default()
    });
    layer
}

/// Pin a mask definition's geometry context to the 3×1 golden frame so the
/// evaluated planes are used 1:1 instead of being bilinearly resampled.
fn geometry_3x1(mut definition: MaskDefinition) -> MaskDefinition {
    definition.geometry_context.width = 3;
    definition.geometry_context.height = 1;
    definition
}

/// `local_render` through the opt-in stage-capturing core entry point.
fn local_render_with_source_stage(
    frame: &ImageFrame,
    copies: &[VirtualCopy],
    planes: BTreeMap<(String, String), MaskPlane>,
    recipe: &EditRecipe,
    source_roi: Option<[f32; 4]>,
) -> Result<RenderOutput, CoreError> {
    let mut context = mask_context(copies, "vc", planes, MaskPolicy::Strict);
    context.source_roi = source_roi;
    let mut work = StageWork::default();
    let base = prepare_source_base(frame, &[], &mut work)?;
    super::super::render_frame_from_base_with_source_stage(
        base,
        &RenderContext {
            recipe,
            camera_white_balance: None,
            source_actions: &[],
            lensfun: None,
            depth: None,
            masks: Some(context),
        },
        &mut work,
        crate::generative::GenerativeCanvasInput::default(),
        &crate::DenoiseStageInput::inactive(),
    )
}

#[test]
fn local_relative_wb_delta_only_has_exact_float_gain_golden_and_preserves_alpha() {
    let definition = mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![local_wb_layer(
            "layer-1",
            reference("vc", "subject"),
            1100.0,
            0.0,
            0.0,
        )],
    )];
    let frame = ImageFrame::new(1, 1, vec![100, 120, 140, 77]).unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
    )]);
    let output = local_render(&frame, &copies, planes, &EditRecipe::default(), None).unwrap();
    // gains = (0.93, 1.0, 1.07); 100*0.93=93, 140*1.07=149.8
    // rounds to 150 exactly once. Alpha is never part of the WB kernel.
    assert_eq!(output.frame.pixels, vec![93, 120, 150, 77]);
    // The default (no-picker) render path never retains the pre-local stage.
    assert_eq!(output.effective_source_stage, None);
}

#[test]
fn local_relative_wb_tint_only_and_temperature_only_keep_the_other_delta_zero() {
    let definition = mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    let copies = vec![copy_with(
        "vc",
        vec![definition.clone()],
        vec![local_wb_layer(
            "tint",
            reference("vc", "subject"),
            0.0,
            -0.5,
            0.0,
        )],
    )];
    let frame = ImageFrame::new(1, 1, vec![100, 120, 140, 200]).unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
    )]);
    let tint = local_render(
        &frame,
        &copies,
        planes.clone(),
        &EditRecipe::default(),
        None,
    )
    .unwrap();
    assert_eq!(tint.frame.pixels, vec![100, 132, 140, 200]);

    let temperature_copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![local_wb_layer(
            "temperature",
            reference("vc", "subject"),
            1100.0,
            0.0,
            0.0,
        )],
    )];
    let temperature = local_render(
        &frame,
        &temperature_copies,
        planes,
        &EditRecipe::default(),
        None,
    )
    .unwrap();
    assert_eq!(temperature.frame.pixels, vec![93, 120, 150, 200]);
}

#[test]
fn local_relative_wb_is_before_local_basic_and_has_one_quantization_boundary() {
    let definition = mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![local_wb_layer(
            "layer-1",
            reference("vc", "subject"),
            2750.0,
            0.0,
            1.0,
        )],
    )];
    let frame = ImageFrame::new(1, 1, vec![100, 100, 100, 9]).unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
    )]);
    let output = local_render(&frame, &copies, planes, &EditRecipe::default(), None).unwrap();
    // WB first gives 82.5/117.5 in float; local exposure doubles those values
    // before the one final byte quantization: [165,200,235]. Rounding WB first
    // would incorrectly produce [166,200,236].
    assert_eq!(output.frame.pixels, vec![165, 200, 235, 9]);
}

#[test]
fn local_relative_wb_full_half_and_zero_masks_preserve_fractional_bytes() {
    let mut definition =
        mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    definition.geometry_context.width = 3;
    definition.geometry_context.height = 1;
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![local_wb_layer(
            "layer-1",
            reference("vc", "subject"),
            1100.0,
            0.0,
            0.0,
        )],
    )];
    let frame = ImageFrame::new(
        3,
        1,
        vec![100, 100, 100, 10, 100, 100, 100, 20, 100, 100, 100, 30],
    )
    .unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(3, 1, vec![0, 32_768, u16::MAX]).unwrap(),
    )]);
    let output = local_render(&frame, &copies, planes, &EditRecipe::default(), None).unwrap();
    assert_eq!(&output.frame.pixels[0..4], &frame.pixels[0..4]);
    let expected_red = ((100 * (u16::MAX - 32_768) as u32 + 93 * 32_768 + u16::MAX as u32 / 2)
        / u16::MAX as u32) as u8;
    assert_eq!(output.frame.pixels[4..8], [expected_red, 100, 104, 20]);
    assert_eq!(&output.frame.pixels[8..12], &[93, 100, 107, 30]);
    assert_eq!(output.frame.pixels[3], 10);
}

#[test]
fn local_relative_wb_global_result_and_explicit_as_shot_reset_are_independent() {
    let definition = mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![local_wb_layer(
            "layer-1",
            reference("vc", "subject"),
            1100.0,
            0.0,
            0.0,
        )],
    )];
    let frame = ImageFrame::new(1, 1, vec![100, 120, 140, 255]).unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
    )]);
    let mut global = EditRecipe::default();
    global.adjustments.insert("wb_temperature".into(), 3000.0);
    let with_global = local_render(&frame, &copies, planes.clone(), &global, None).unwrap();
    // The local delta is applied to the already globally-adjusted result; it
    // does not replace or reset the absolute global keys. Global 3000 K first
    // gives [122,120,109]; the +1100 K local delta then gives [113,120,117].
    assert_eq!(with_global.frame.pixels, vec![113, 120, 117, 255]);
    assert!(global.adjustments.contains_key("wb_temperature"));
    global.adjustments.remove("wb_temperature");
    let as_shot = local_render(&frame, &copies, planes, &global, None).unwrap();
    assert_eq!(as_shot.effective_source_stage, None);
    assert_eq!(as_shot.frame.pixels, vec![93, 120, 150, 255]);
}

#[test]
fn invalid_local_wb_is_a_loud_preflight_error_not_a_partial_render() {
    let definition = mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    let mut invalid = local_wb_layer("layer-1", reference("vc", "subject"), 5000.1, 0.0, 1.0);
    invalid.local_adjustments.as_mut().unwrap().version = 2;
    let copies = vec![copy_with("vc", vec![definition], vec![invalid])];
    let frame = ImageFrame::new(1, 1, vec![100; 4]).unwrap();
    let planes = BTreeMap::from([(
        ("vc".into(), "subject".into()),
        MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
    )]);
    let error = local_render(&frame, &copies, planes, &EditRecipe::default(), None).unwrap_err();
    assert!(matches!(error, CoreError::InvalidLocalAdjustment { .. }));
}

/// The same frame must render byte-identically with and without the optional
/// stage capture. This is the structural guard for the lazy source stage: the
/// opt-in entry point adds a clone and nothing else — no ordering, rounding or
/// composition change — so the picker's provenance can never depend on whether
/// the stage was retained.
#[test]
fn source_stage_capture_is_opt_in_and_never_changes_pixels() {
    let definition = mask_definition("subject", MaskStatus::Valid, MaskOperation::Source, vec![]);
    let copies = vec![copy_with(
        "vc",
        vec![definition],
        vec![local_wb_layer(
            "layer-1",
            reference("vc", "subject"),
            1100.0,
            -0.25,
            0.5,
        )],
    )];
    let frame = ImageFrame::new(1, 1, vec![100, 120, 140, 77]).unwrap();
    let planes = || {
        BTreeMap::from([(
            ("vc".into(), "subject".into()),
            MaskPlane::new(1, 1, vec![u16::MAX]).unwrap(),
        )])
    };
    let without = local_render(&frame, &copies, planes(), &EditRecipe::default(), None).unwrap();
    let with =
        local_render_with_source_stage(&frame, &copies, planes(), &EditRecipe::default(), None)
            .unwrap();
    assert_eq!(without.effective_source_stage, None);
    assert_eq!(with.effective_source_stage.as_ref(), Some(&frame));
    assert_eq!(with.frame, without.frame);
}

/// The no-local path pays nothing and still produces a render. A global-only
/// recipe with no mask layers must keep `effective_source_stage` at `None` on
/// the default entry point and must not be routed through the local compositor.
#[test]
fn global_only_render_without_masks_never_captures_a_source_stage() {
    let frame = ImageFrame::new(2, 1, vec![10, 20, 30, 255, 40, 50, 60, 128]).unwrap();
    let context = RenderContext {
        recipe: &EditRecipe::default(),
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    let mut work = StageWork::default();
    let default = render_frame_from_base_with_generative_and_denoise(
        frame.clone(),
        &context,
        &mut work,
        crate::generative::GenerativeCanvasInput::default(),
        &crate::DenoiseStageInput::inactive(),
    )
    .unwrap();
    assert_eq!(default.effective_source_stage, None);
    assert_eq!(default.frame, frame);
    assert!(default.mask_layers.is_empty());
    assert_eq!(work.mask_layers_evaluated, 0);

    // Only an explicit capture request pays the copy, and the pixels are equal.
    let mut capture_work = StageWork::default();
    let captured = render_frame_from_base_with_source_stage(
        frame.clone(),
        &context,
        &mut capture_work,
        crate::generative::GenerativeCanvasInput::default(),
        &crate::DenoiseStageInput::inactive(),
    )
    .unwrap();
    assert_eq!(captured.effective_source_stage.as_ref(), Some(&frame));
    assert_eq!(captured.frame, default.frame);
}

/// Two *overlapping* local layers, each carrying both a temperature and a tint
/// delta, must be evaluated in the persisted list order. The middle pixel is
/// the overlap (alpha 1 on both masks); its bytes differ from the reordered
/// run, which is what makes the persisted order observable.
#[test]
fn two_overlapping_local_wb_layers_use_persisted_order_with_both_delta_axes() {
    let definitions = vec![
        geometry_3x1(mask_definition(
            "graded",
            MaskStatus::Valid,
            MaskOperation::Source,
            vec![],
        )),
        geometry_3x1(mask_definition(
            "matted",
            MaskStatus::Valid,
            MaskOperation::Source,
            vec![],
        )),
    ];
    // Layer 1 `graded`: warmth 2750/5500 = 0.5 and tint -0.5
    //   -> gains (0.825, 1.1, 1.175)
    // Layer 2 `matted`: warmth -0.5 and tint +0.25
    //   -> gains (1.175, 0.95, 0.825)
    // Both axes are non-zero on both layers, so the overlap pixel exercises the
    // temperature *and* the tint channel of both layers in persisted order.
    let graded = local_wb_layer("layer-1", reference("vc", "graded"), 2750.0, -0.5, 0.0);
    let matted = local_wb_layer("layer-2", reference("vc", "matted"), -2750.0, 0.25, 0.0);
    let ordered = vec![copy_with(
        "vc",
        definitions.clone(),
        vec![graded.clone(), matted.clone()],
    )];
    let reordered = vec![copy_with("vc", definitions, vec![matted, graded])];
    let frame = ImageFrame::new(
        3,
        1,
        vec![
            100, 100, 100, 11, // graded only
            100, 100, 100, 22, // both layers overlap
            100, 100, 100, 33, // matted only
        ],
    )
    .unwrap();
    let planes = || {
        BTreeMap::from([
            (
                ("vc".into(), "graded".into()),
                MaskPlane::new(3, 1, vec![u16::MAX, u16::MAX, 0]).unwrap(),
            ),
            (
                ("vc".into(), "matted".into()),
                MaskPlane::new(3, 1, vec![0, u16::MAX, u16::MAX]).unwrap(),
            ),
        ])
    };
    let forward = local_render(&frame, &ordered, planes(), &EditRecipe::default(), None).unwrap();
    let reverse = local_render(&frame, &reordered, planes(), &EditRecipe::default(), None).unwrap();

    // Pixel 0 (graded only): 100 * (0.825, 1.1, 1.175) -> (82.5, 110, 117.5)
    //   -> round once -> [83, 110, 118]
    // Pixel 1 (overlap, persisted order): graded first -> [83, 110, 118], then
    //   matted -> (97.525, 104.5, 97.35) -> round -> [98, 105, 97]
    // Pixel 2 (matted only): 100 * (1.175, 0.95, 0.825) -> (117.5, 95, 82.5)
    //   -> round -> [118, 95, 83]
    // Image alpha is never part of the WB kernel.
    assert_eq!(
        forward.frame.pixels,
        vec![
            83, 110, 118, 11, // graded only
            98, 105, 97, 22, // overlap: graded then matted
            118, 95, 83, 33, // matted only
        ]
    );
    assert_eq!(forward.effective_source_stage, None);
    // The same two layers in the opposite persisted order give the overlap
    // pixel [97, 105, 98] (matted -> [118, 95, 83] first), so the persisted
    // order is observable and not an equivalent reordering.
    assert_eq!(reverse.frame.pixels[0..4], [83, 110, 118, 11]);
    assert_eq!(reverse.frame.pixels[4..8], [97, 105, 98, 22]);
    assert_eq!(reverse.frame.pixels[8..12], [118, 95, 83, 33]);
    assert_ne!(forward.frame.pixels, reverse.frame.pixels);
}
