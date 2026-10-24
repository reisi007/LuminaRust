use super::*;
use lumina_sidecar::{
    BrushMark, BrushMarkSign, CoordinateSystem, DecodeFingerprint, Extras, GeometryFingerprint,
    MaskPrompt, MaskStatus, ModelIdentity, NormalizedRect, Point2, Preprocessing, PromptTransform,
    Resolution, SourceFingerprint,
};

fn definition(
    id: &str,
    operation: MaskOperation,
    references: Vec<MaskReference>,
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
        status: MaskStatus::Valid,
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
fn reference(id: &str) -> MaskReference {
    MaskReference {
        copy_id: "vc".into(),
        mask_id: id.into(),
        extras: Extras::new(),
    }
}
fn graph(definitions: Vec<MaskDefinition>, sources: &[(&str, Vec<u16>)]) -> MaskGraph<'static> {
    let copy = Box::leak(Box::new(VirtualCopy {
        id: "vc".into(),
        name: "VC".into(),
        is_default: true,
        rating: 0,
        flag: lumina_sidecar::Flag::Unflagged,
        recipe: Default::default(),
        mask_library: definitions,
        mask_layers: vec![],
        history: vec![],
        export_records: vec![],
        extras: Extras::new(),
    }));
    let payloads = sources
        .iter()
        .map(|(id, values)| {
            (
                ("vc".into(), (*id).into()),
                MaskPlane::new(2, 1, values.clone()).unwrap(),
            )
        })
        .collect();
    MaskGraph::new(std::slice::from_ref(copy), payloads)
}

#[test]
fn evaluates_all_operations_and_subtract_rounding() {
    let refs = |ids: &[&str]| ids.iter().map(|id| reference(id)).collect();
    let graph = graph(
        vec![
            definition("a", MaskOperation::Source, vec![]),
            definition("b", MaskOperation::Source, vec![]),
            definition("u", MaskOperation::Union, refs(&["a", "b"])),
            definition("i", MaskOperation::Intersect, refs(&["a", "b"])),
            definition("s", MaskOperation::Subtract, refs(&["a", "b"])),
            definition("n", MaskOperation::Invert, refs(&["a"])),
        ],
        &[("a", vec![0, 32768]), ("b", vec![65535, 16384])],
    );
    assert_eq!(
        graph.evaluate(&reference("u")).unwrap().values,
        vec![65535, 32768]
    );
    assert_eq!(
        graph.evaluate(&reference("i")).unwrap().values,
        vec![0, 16384]
    );
    assert_eq!(
        graph.evaluate(&reference("s")).unwrap().values,
        vec![0, 24576]
    );
    assert_eq!(
        graph.evaluate(&reference("n")).unwrap().values,
        vec![65535, 32767]
    );
}

#[test]
fn resolves_cross_copy_and_reports_failures() {
    let source = definition("source", MaskOperation::Source, vec![]);
    let mut derived = definition(
        "derived",
        MaskOperation::Invert,
        vec![MaskReference {
            copy_id: "other".into(),
            mask_id: "source".into(),
            extras: Extras::new(),
        }],
    );
    derived.id = "derived".into();
    let other = Box::leak(Box::new(VirtualCopy {
        id: "other".into(),
        name: "Other".into(),
        is_default: false,
        rating: 0,
        flag: lumina_sidecar::Flag::Unflagged,
        recipe: Default::default(),
        mask_library: vec![source],
        mask_layers: vec![],
        history: vec![],
        export_records: vec![],
        extras: Extras::new(),
    }));
    let target = Box::leak(Box::new(VirtualCopy {
        id: "vc".into(),
        name: "VC".into(),
        is_default: true,
        rating: 0,
        flag: lumina_sidecar::Flag::Unflagged,
        recipe: Default::default(),
        mask_library: vec![derived],
        mask_layers: vec![],
        history: vec![],
        export_records: vec![],
        extras: Extras::new(),
    }));
    let copies = Box::leak(Box::new(vec![(*target).clone(), (*other).clone()]));
    let graph = MaskGraph::new(
        copies,
        BTreeMap::from([(
            ("other".into(), "source".into()),
            MaskPlane::new(1, 1, vec![7]).unwrap(),
        )]),
    );
    assert_eq!(
        graph.evaluate(&reference("derived")).unwrap().values,
        vec![65528]
    );
    assert!(matches!(
        MaskPlane::new(2, 1, vec![1]),
        Err(MaskError::InvalidPlane { .. })
    ));
    assert!(matches!(
        MaskGraph::new(std::slice::from_ref(target), BTreeMap::new())
            .evaluate(&reference("derived")),
        Err(MaskError::MissingMask { .. })
    ));
}

#[test]
fn reports_missing_source_payload() {
    let graph = graph(
        vec![definition("source", MaskOperation::Source, vec![])],
        &[],
    );

    assert_eq!(
        graph.evaluate(&reference("source")),
        Err(MaskError::MissingSourcePayload {
            copy_id: "vc".into(),
            mask_id: "source".into(),
        })
    );
}

#[test]
fn detects_cycle_during_evaluation() {
    let refs = |id: &str| vec![reference(id)];
    let graph = graph(
        vec![
            definition("a", MaskOperation::Invert, refs("b")),
            definition("b", MaskOperation::Invert, refs("a")),
        ],
        &[],
    );

    assert_eq!(
        graph.evaluate(&reference("a")),
        Err(MaskError::Cycle {
            copy_id: "vc".into(),
            mask_id: "a".into(),
        })
    );
}

#[test]
fn reports_dimensions_of_referenced_masks() {
    let mut definitions = vec![
        definition("a", MaskOperation::Source, vec![]),
        definition("b", MaskOperation::Source, vec![]),
        definition(
            "union",
            MaskOperation::Union,
            vec![reference("a"), reference("b")],
        ),
    ];
    let copy = Box::leak(Box::new(VirtualCopy {
        id: "vc".into(),
        name: "VC".into(),
        is_default: true,
        rating: 0,
        flag: lumina_sidecar::Flag::Unflagged,
        recipe: Default::default(),
        mask_library: std::mem::take(&mut definitions),
        mask_layers: vec![],
        history: vec![],
        export_records: vec![],
        extras: Extras::new(),
    }));
    let graph = MaskGraph::new(
        std::slice::from_ref(copy),
        BTreeMap::from([
            (
                ("vc".into(), "a".into()),
                MaskPlane::new(2, 1, vec![1, 2]).unwrap(),
            ),
            (
                ("vc".into(), "b".into()),
                MaskPlane::new(1, 2, vec![3, 4]).unwrap(),
            ),
        ]),
    );

    assert_eq!(
        graph.evaluate(&reference("union")),
        Err(MaskError::DimensionMismatch {
            expected_width: 2,
            expected_height: 1,
            actual_width: 1,
            actual_height: 2,
        })
    );
}

#[test]
fn rejects_invalid_arity_during_evaluation() {
    let graph = graph(
        vec![definition(
            "subtract",
            MaskOperation::Subtract,
            vec![reference("a")],
        )],
        &[],
    );

    assert_eq!(
        graph.evaluate(&reference("subtract")),
        Err(MaskError::InvalidArity {
            copy_id: "vc".into(),
            mask_id: "subtract".into(),
            actual: 1,
        })
    );
}

#[test]
fn subtract_handles_u16_boundaries_without_overflow() {
    let graph = graph(
        vec![
            definition("a", MaskOperation::Source, vec![]),
            definition("b", MaskOperation::Source, vec![]),
            definition(
                "subtract",
                MaskOperation::Subtract,
                vec![reference("a"), reference("b")],
            ),
        ],
        &[("a", vec![0, u16::MAX]), ("b", vec![u16::MAX, 0])],
    );

    assert_eq!(
        graph.evaluate(&reference("subtract")).unwrap().values,
        vec![0, u16::MAX]
    );
}

// =====================================================================
// F-079: deterministic geometric rasterization of prompt sources.
// =====================================================================

#[test]
fn rasterize_box_fills_rectangle_interior() {
    let prompt = MaskPrompt::Box {
        rect: NormalizedRect {
            x: 0.25,
            y: 0.25,
            width: 0.5,
            height: 0.5,
        },
        transformation: PromptTransform::default(),
    };
    let plane = rasterize_prompt(&prompt, 4, 4).unwrap();
    assert_eq!((plane.width, plane.height), (4, 4));
    // Outside corners are zero.
    assert_eq!(plane.values[0], 0);
    assert_eq!(plane.values[15], 0);
    // The central 2x2 block (x,y in {1,2}) is fully on.
    for idx in [5, 6, 9, 10] {
        assert_eq!(plane.values[idx], u16::MAX, "box interior at {idx}");
    }
}

#[test]
fn rasterize_ellipse_fills_interior() {
    let prompt = MaskPrompt::Ellipse {
        center: Point2 { x: 0.5, y: 0.5 },
        radii: Point2 { x: 0.4, y: 0.4 },
        transformation: PromptTransform::default(),
    };
    let plane = rasterize_prompt(&prompt, 16, 16).unwrap();
    // Centre pixel inside the ellipse.
    assert_eq!(plane.values[8 * 16 + 8], u16::MAX);
    // Far corner outside the ellipse.
    assert_eq!(plane.values[0], 0);
}

#[test]
fn rasterize_polygon_fills_interior() {
    let prompt = MaskPrompt::Polygon {
        points: vec![
            Point2 { x: 0.0, y: 1.0 },
            Point2 { x: 1.0, y: 1.0 },
            Point2 { x: 0.5, y: 0.0 },
        ],
        transformation: PromptTransform::default(),
    };
    let plane = rasterize_prompt(&prompt, 10, 10).unwrap();
    // Bottom-left corner sits inside the upward triangle.
    assert_eq!(plane.values[9 * 10], u16::MAX);
    // Top-centre near the apex is outside.
    assert_eq!(plane.values[5], 0);
}

#[test]
fn rasterize_gradient_is_monotonic_and_maps_endpoints() {
    let prompt = MaskPrompt::Gradient {
        angle_deg: 0.0,
        start: 0.0,
        end: 1.0,
        transformation: PromptTransform::default(),
    };
    let width = 8u32;
    let plane = rasterize_prompt(&prompt, width, 1).unwrap();
    // Leftmost/rightmost columns map to the endpoints.
    assert_eq!(plane.values[0], 0);
    assert_eq!(plane.values[(width - 1) as usize], u16::MAX);
    // Monotonic non-decreasing left to right.
    for i in 1..width as usize {
        assert!(
            plane.values[i] >= plane.values[i - 1],
            "gradient not monotonic at {i}: {} < {}",
            plane.values[i],
            plane.values[i - 1]
        );
    }
    // Reversed gradient is monotonic non-increasing.
    let reversed = MaskPrompt::Gradient {
        angle_deg: 0.0,
        start: 1.0,
        end: 0.0,
        transformation: PromptTransform::default(),
    };
    let plane = rasterize_prompt(&reversed, width, 1).unwrap();
    assert_eq!(plane.values[0], u16::MAX);
    assert_eq!(plane.values[(width - 1) as usize], 0);
    for i in 1..width as usize {
        assert!(plane.values[i] <= plane.values[i - 1]);
    }
}

#[test]
fn rasterize_brush_paints_positive_and_erases_negative() {
    let prompt = MaskPrompt::Brush {
        marks: vec![
            BrushMark {
                x: 0.5,
                y: 0.5,
                radius: 0.4,
                sign: BrushMarkSign::Positive,
                softness: 0.0,
                flow: 1.0,
            },
            BrushMark {
                x: 0.2,
                y: 0.5,
                radius: 0.2,
                sign: BrushMarkSign::Negative,
                softness: 0.0,
                flow: 1.0,
            },
        ],
        resolution: (32, 32),
        transformation: PromptTransform::default(),
    };
    let plane = rasterize_prompt(&prompt, 32, 32).unwrap();
    // Centre is covered by the positive mark -> max.
    assert_eq!(plane.values[16 * 32 + 16], u16::MAX);
    // Far corner is untouched -> zero.
    assert_eq!(plane.values[0], 0);
    // A pixel near the negative mark (left side) is erased to zero even
    // though it would be inside the large positive disk.
    let negative_pixel = (16usize) * 32 + (6usize); // normalized x ~0.2
    assert_eq!(plane.values[negative_pixel], 0);
}

#[test]
fn rasterize_prompt_is_deterministic() {
    let prompt = MaskPrompt::Gradient {
        angle_deg: 30.0,
        start: 0.1,
        end: 0.9,
        transformation: PromptTransform::default(),
    };
    let a = rasterize_prompt(&prompt, 12, 7).unwrap();
    let b = rasterize_prompt(&prompt, 12, 7).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.values, b.values);
}

#[test]
fn source_with_prompt_evaluates_geometric_matte_without_loaded_plane() {
    // A Source node carrying a Box prompt must evaluate to the deterministic
    // geometric matte even when no inferred (model) plane is loaded.
    let mut def = definition("box-source", MaskOperation::Source, vec![]);
    def.prompt = Some(MaskPrompt::Box {
        rect: NormalizedRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
        transformation: PromptTransform::default(),
    });
    // geometry_context in the test helper is 2x1, so the full-image box
    // rasterizes to an all-max 2x1 plane.
    let graph = graph(vec![def], &[]);
    let plane = graph.evaluate(&reference("box-source")).unwrap();
    assert_eq!(plane.values, vec![u16::MAX, u16::MAX]);
}

#[test]
fn source_with_prompt_and_loaded_plane_prefers_loaded_plane() {
    // When an inferred (loaded) plane is present for a prompt node, it takes
    // precedence: the matte can be recomputed and the user's prompt stays.
    let mut def = definition("box-source", MaskOperation::Source, vec![]);
    def.prompt = Some(MaskPrompt::Box {
        rect: NormalizedRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
        transformation: PromptTransform::default(),
    });
    let graph = graph(vec![def], &[("box-source", vec![7, 11])]);
    assert_eq!(
        graph.evaluate(&reference("box-source")).unwrap().values,
        vec![7, 11]
    );
}

#[test]
fn source_with_prompt_falls_back_when_no_loaded_plane() {
    let mut def = definition("ellipse-source", MaskOperation::Source, vec![]);
    def.prompt = Some(MaskPrompt::Ellipse {
        center: Point2 { x: 0.5, y: 0.5 },
        radii: Point2 { x: 0.5, y: 0.5 },
        transformation: PromptTransform::default(),
    });
    let graph = graph(vec![def], &[]);
    let plane = graph.evaluate(&reference("ellipse-source")).unwrap();
    // Full-image ellipse -> all max.
    assert_eq!(plane.values, vec![u16::MAX, u16::MAX]);
}

// =====================================================================
// REVIEW-MASK-ZERO-1: zero-dimension targets must be MaskErrors.
// =====================================================================

#[test]
fn rasterize_prompt_rejects_zero_dimensions_with_mask_error() {
    let prompt = MaskPrompt::Box {
        rect: NormalizedRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
        transformation: PromptTransform::default(),
    };
    for (width, height) in [(0u32, 5u32), (5, 0), (0, 0)] {
        assert_eq!(
            rasterize_prompt(&prompt, width, height),
            Err(MaskError::InvalidPlane {
                width,
                height,
                length: 0
            }),
            "width={width} height={height}"
        );
    }
    // The guard runs before the per-prompt match: every prompt kind is
    // covered by the same rejection.
    let ellipse = MaskPrompt::Brush {
        marks: vec![],
        resolution: (4, 4),
        transformation: PromptTransform::default(),
    };
    assert!(matches!(
        rasterize_prompt(&ellipse, 0, 4),
        Err(MaskError::InvalidPlane { .. })
    ));
}

#[test]
fn graph_evaluates_zero_dimension_prompt_source_as_error_not_panic() {
    let mut def = definition("zero-source", MaskOperation::Source, vec![]);
    def.prompt = Some(MaskPrompt::Box {
        rect: NormalizedRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
        transformation: PromptTransform::default(),
    });
    def.geometry_context.width = 0;
    def.geometry_context.height = 0;
    let graph = graph(vec![def], &[]);
    assert!(matches!(
        graph.evaluate(&reference("zero-source")),
        Err(MaskError::InvalidPlane { .. })
    ));
}

// =====================================================================
// REVIEW-MASK-N1: per-call memoization of shared subtrees.
// =====================================================================

#[test]
fn memoized_dag_executes_each_shared_subtree_once() {
    // Diamond with a doubly-shared leaf:
    //   d = union(a, b); a = invert(s); b = invert(s)
    // Without memoization the leaf `s` executes twice (once under `a`,
    // once under `b`); with memoization every node body runs exactly
    // once and the result is unchanged.
    let refs = |ids: &[&str]| ids.iter().map(|id| reference(id)).collect();
    let graph = graph(
        vec![
            definition("s", MaskOperation::Source, vec![]),
            definition("a", MaskOperation::Invert, refs(&["s"])),
            definition("b", MaskOperation::Invert, refs(&["s"])),
            definition("d", MaskOperation::Union, refs(&["a", "b"])),
        ],
        &[("s", vec![100, 65_000])],
    );
    let plane = graph.evaluate(&reference("d")).unwrap();
    assert_eq!(
        plane.values,
        vec![u16::MAX - 100, u16::MAX - 65_000],
        "union of two identical inversions must equal one inversion"
    );
    assert_eq!(
        graph.last_evaluated_node_count(),
        4,
        "distinct nodes s/a/b/d must each execute exactly once"
    );
    // A second evaluation on the same graph is independent (per-call
    // memo) and executes the same number of node bodies again.
    let again = graph.evaluate(&reference("d")).unwrap();
    assert_eq!(again.values, plane.values);
    assert_eq!(graph.last_evaluated_node_count(), 4);
}

#[test]
fn deep_sharing_stays_linear_under_memoization() {
    // A chain where every level references the SAME base mask twice:
    //   m0 = source; m{k} = union(m{k-1}, m{k-1})
    // Without memoization this is exponential in k; with memoization the
    // executed-node count is exactly k + 1 (m0..mk).
    let levels = 12;
    let mut definitions = vec![definition("m0", MaskOperation::Source, vec![])];
    for k in 1..=levels {
        definitions.push(definition(
            &format!("m{k}"),
            MaskOperation::Union,
            vec![
                reference(&format!("m{}", k - 1)),
                reference(&format!("m{}", k - 1)),
            ],
        ));
    }
    let payloads = [("m0", vec![1u16, 65_000])];
    let graph = graph(definitions, &payloads);
    let plane = graph.evaluate(&reference(&format!("m{levels}"))).unwrap();
    assert_eq!(plane.values, payloads[0].1);
    assert_eq!(
        graph.last_evaluated_node_count(),
        levels as usize + 1,
        "memoization must keep the doubly-referencing chain linear"
    );
}

// ----- G-03: AI-select never rasterizes, range needs pixels -----

#[test]
fn ai_select_without_plane_is_loud_missing_never_geometry() {
    use lumina_sidecar::{AiSelect, AiSelectKind};
    let mut ai = definition("ai", MaskOperation::Source, vec![]);
    ai.ai_select = Some(AiSelect {
        kind: AiSelectKind::Sky,
        detail: None,
        extras: Extras::new(),
    });
    // A persisted box prompt stays as model input but must not become a
    // geometric fallback matte while the model plane is missing.
    ai.prompt = Some(MaskPrompt::Box {
        rect: NormalizedRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
        transformation: PromptTransform::default(),
    });
    let graph = graph(vec![ai], &[]);
    let error = graph.evaluate(&reference("ai")).unwrap_err();
    assert_eq!(
        error,
        MaskError::MissingSourcePayload {
            copy_id: "vc".into(),
            mask_id: "ai".into(),
        }
    );
}

#[test]
fn ai_select_with_loaded_plane_uses_plane() {
    use lumina_sidecar::{AiSelect, AiSelectKind};
    let mut ai = definition("ai", MaskOperation::Source, vec![]);
    ai.ai_select = Some(AiSelect {
        kind: AiSelectKind::Subject,
        detail: Some("face".into()),
        extras: Extras::new(),
    });
    let graph = graph(vec![ai], &[("ai", vec![7u16, 9])]);
    let plane = graph.evaluate(&reference("ai")).unwrap();
    assert_eq!(plane.values, vec![7u16, 9]);
}

#[test]
fn range_prompt_without_pixels_is_loud() {
    let mut range = definition("range", MaskOperation::Source, vec![]);
    range.prompt = Some(MaskPrompt::LuminanceRange {
        min: 0.0,
        max: 1.0,
        feather: 0.0,
        transformation: PromptTransform::default(),
    });
    let graph = graph(vec![range], &[]);
    assert_eq!(
        graph.evaluate(&reference("range")).unwrap_err(),
        MaskError::RangeNeedsPixels
    );
    assert_eq!(
        rasterize_prompt(
            &MaskPrompt::ColorRange {
                hue_center: 0.0,
                hue_width: 360.0,
                sat_min: 0.0,
                sat_max: 1.0,
                lum_min: 0.0,
                lum_max: 1.0,
                feather: 0.0,
                transformation: PromptTransform::default(),
            },
            2,
            1,
        )
        .unwrap_err(),
        MaskError::RangeNeedsPixels
    );
}

#[test]
fn range_prompt_with_loaded_plane_prefers_plane() {
    // A refined/inferred matte wins over recomputation, like geometry.
    let mut range = definition("range", MaskOperation::Source, vec![]);
    range.prompt = Some(MaskPrompt::LuminanceRange {
        min: 0.0,
        max: 1.0,
        feather: 0.0,
        transformation: PromptTransform::default(),
    });
    let graph = graph(vec![range], &[("range", vec![3u16, 4])]);
    let plane = graph.evaluate(&reference("range")).unwrap();
    assert_eq!(plane.values, vec![3u16, 4]);
}
