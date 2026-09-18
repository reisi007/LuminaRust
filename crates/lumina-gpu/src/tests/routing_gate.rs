use super::*;

use lumina_sidecar::Perspective;

fn recipe_with_adjustments(entries: &[(&str, f64)]) -> EditRecipe {
    EditRecipe {
        adjustments: entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), *value))
            .collect(),
        ..Default::default()
    }
}

/// R2-GPU-05 / GPU-RENDER-PARITY-1: vibrance and saturation are rendered by
/// the color pass, so neither a present-but-neutral (`0.0`) value nor a
/// non-neutral one may flag the GPU route anymore.
#[test]
fn vibrance_and_saturation_are_always_supported() {
    let touched_and_reset =
        recipe_with_adjustments(&[("vibrance", 0.0), ("saturation", 0.0), ("exposure", 0.0)]);
    assert!(unsupported_gpu_stages(&touched_and_reset).is_empty());

    assert!(unsupported_gpu_stages(&recipe_with_adjustments(&[
        ("vibrance", 0.2),
        ("saturation", -0.5),
    ]))
    .is_empty());
}

/// Keys outside the recipe schema have no neutral value and stay flagged
/// regardless of their value — the CPU pipeline rejects them outright.
#[test]
fn unknown_keys_have_no_neutral_value() {
    let bogus = recipe_with_adjustments(&[("clarity_v2", 0.0)]);
    let reasons = unsupported_gpu_stages(&bogus);
    assert!(
        reasons.iter().any(|r| r.contains("clarity_v2")),
        "{reasons:?}"
    );
}

/// GPU-RENDER-PARITY-1: Point Color is rendered by the color pass, so no
/// Point Color configuration blocks the GPU route anymore.
#[test]
fn point_color_is_always_supported() {
    use lumina_sidecar::{PointColor, PointColorEntry};
    let entry = |saturation_shift| PointColorEntry {
        id: "pc-1".into(),
        hue_center: 30.0,
        hue_range: 20.0,
        hue_shift: 0.0,
        saturation_shift,
        luminance_shift: 0.0,
    };
    let recipe = |entries: Vec<PointColorEntry>| EditRecipe {
        point_color: Some(PointColor {
            version: 1,
            entries,
        }),
        ..Default::default()
    };
    assert!(unsupported_gpu_stages(&EditRecipe::default()).is_empty());
    assert!(unsupported_gpu_stages(&recipe(vec![])).is_empty());
    assert!(unsupported_gpu_stages(&recipe(vec![entry(0.0)])).is_empty());
    assert!(unsupported_gpu_stages(&recipe(vec![entry(0.5)])).is_empty());
}

/// Documented per-key identity values (R2-GPU-05 follow-up): everything is
/// centered at `0.0` except `wb_temperature`, whose identity point is
/// 6500 K (exactly `[1.0, 1.0, 1.0]` channel gains).
#[test]
fn adjustment_neutral_table() {
    assert_eq!(adjustment_neutral_value("wb_temperature"), Some(6500.0));
    for key in [
        "exposure",
        "contrast",
        "highlights",
        "shadows",
        "whites",
        "blacks",
        "wb_tint",
        "vibrance",
        "saturation",
    ] {
        assert_eq!(adjustment_neutral_value(key), Some(0.0), "{key}");
    }
    assert_eq!(adjustment_neutral_value("not_a_key"), None);
}

/// CAMERA-WB-WELLE (R2-MCP-01): a **valid** decoder As-Shot WB context is
/// GPU-eligible (the caller binds it via `set_camera_white_balance`); an
/// **invalid** context still produces exactly one CPU-routing reason. The
/// reason stacks with recipe-level reasons instead of replacing them, and
/// the legacy predicates delegate with no context.
#[test]
fn context_wb_valid_is_gpu_eligible_invalid_routes_to_cpu() {
    let wb: [f32; 4] = [1.8999, 1.0, 1.3953, 1.0];
    // Valid gains are pixel-neutral on both backends and the GPU carries
    // them explicitly → no reason.
    assert!(
        unsupported_gpu_stages_with_context(&EditRecipe::default(), false, Some(&wb)).is_empty()
    );
    // Source-action binding does not change the WB verdict.
    assert!(
        unsupported_gpu_stages_with_context(&EditRecipe::default(), true, Some(&wb)).is_empty()
    );
    // Absence keeps the gate empty for a supported recipe.
    assert!(unsupported_gpu_stages_with_context(&EditRecipe::default(), false, None).is_empty());

    // Invalid gains (non-finite / non-positive) stay flagged so an unbound
    // caller still reaches the oracle's loud rejection.
    for bad in [
        [0.0f32, 1.0, 1.0, 1.0],
        [1.0, f32::NAN, 1.0, 1.0],
        [1.0, 1.0, f32::INFINITY, 1.0],
    ] {
        let reasons =
            unsupported_gpu_stages_with_context(&EditRecipe::default(), false, Some(&bad));
        assert_eq!(
            reasons,
            vec!["camera_white_balance (invalid As-Shot gains)".to_string()],
            "{bad:?}"
        );
    }

    // An invalid WB stacks with recipe reasons instead of replacing them.
    // (A key outside the schema has no neutral value and stays CPU-routed.)
    let mixed = unsupported_gpu_stages_with_context(
        &recipe_with_adjustments(&[("unknown_stage", 0.3)]),
        false,
        Some(&[0.0, 1.0, 1.0, 1.0]),
    );
    assert_eq!(mixed.len(), 2, "{mixed:?}");
    assert!(
        mixed.iter().any(|r| r.contains("unknown_stage")),
        "{mixed:?}"
    );
    assert!(
        mixed.iter().any(|r| r.starts_with("camera_white_balance")),
        "{mixed:?}"
    );

    // The legacy predicates delegate with `None`: identical to passing no
    // context explicitly.
    assert_eq!(
        unsupported_gpu_stages(&EditRecipe::default()),
        unsupported_gpu_stages_with_context(&EditRecipe::default(), false, None)
    );
    let vibrance_recipe = recipe_with_adjustments(&[("vibrance", 0.3)]);
    assert_eq!(
        unsupported_gpu_stages_for(&vibrance_recipe, true),
        unsupported_gpu_stages_with_context(&vibrance_recipe, true, None)
    );
}

/// CAMERA-WB-WELLE: the explicit As-Shot bind validates with the oracle's
/// exact error and leaves the previous binding untouched on rejection.
#[test]
fn set_camera_white_balance_validates_like_the_oracle() {
    let ctx = GpuContext::new().expect("context creation never fails hard");
    assert_eq!(ctx.camera_white_balance(), None);

    ctx.set_camera_white_balance(Some([1.9, 1.0, 1.4, 1.0]))
        .expect("valid gains bind");
    assert_eq!(ctx.camera_white_balance(), Some([1.9, 1.0, 1.4, 1.0]));

    for bad in [
        [0.0f32, 1.0, 1.0, 1.0],
        [-1.0, 1.0, 1.0, 1.0],
        [f32::NAN, 1.0, 1.0, 1.0],
        [f32::INFINITY, 1.0, 1.0, 1.0],
    ] {
        let error = ctx
            .set_camera_white_balance(Some(bad))
            .expect_err("invalid gains must be rejected loudly");
        assert!(
            matches!(
                error,
                GpuError::Core(lumina_core::CoreError::InvalidAdjustment { ref name, .. })
                    if name == "camera_white_balance"
            ),
            "{bad:?}: {error:?}"
        );
        // Rejection changes nothing: the previous valid binding survives.
        assert_eq!(
            ctx.camera_white_balance(),
            Some([1.9, 1.0, 1.4, 1.0]),
            "{bad:?}"
        );
    }

    ctx.set_camera_white_balance(None)
        .expect("clearing always succeeds");
    assert_eq!(ctx.camera_white_balance(), None);
}

/// Invariant: the value-neutrality change (R2-GPU-05) must not weaken any
/// other stage check — nested objects and flat stage markers still route.
/// GPU-RENDER-PARITY-1 geometry and lens-blur waves moved geometry/
/// perspective/manual lens correction and G-05 lens blur into the GPU
/// pipeline **with an explicit crop**; GPU-MAXRECT-WELLE keeps an
/// uncropped lens/perspective correction CPU-routed (content default crop),
/// so that case is asserted to flag here. The still-unsupported nested
/// stages are covered by `tests/parity.rs`'s routing inventory.
#[test]
fn non_adjustment_stage_checks_unchanged() {
    let lens_blur = EditRecipe {
        lens_blur: Some(lumina_sidecar::LensBlur {
            version: 1,
            enabled: true,
            focus_rect: lumina_sidecar::FocusRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            focal_near: 0.0,
            focal_far: 1.0,
            blur_amount: 0.5,
            bokeh: lumina_sidecar::BokehShape::Round,
            depth_artifact: None,
        }),
        ..Default::default()
    };
    assert!(
        unsupported_gpu_stages(&lens_blur).is_empty(),
        "G-05 lens blur is GPU-rendered since the lens-blur wave"
    );

    // Geometry is GPU-rendered now (the geometry wave). It stays eligible
    // without a lens/perspective correction.
    let geometry = EditRecipe {
        geometry: Some(lumina_sidecar::Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        }),
        ..Default::default()
    };
    assert!(
        unsupported_gpu_stages(&geometry).is_empty(),
        "geometry without a correction is GPU-rendered"
    );

    // A perspective correction **with an explicit crop** is GPU-rendered
    // (the crop is authoritative).
    let perspective_cropped = EditRecipe {
        geometry: Some(lumina_sidecar::Geometry {
            version: 1,
            crop: Some(lumina_sidecar::Crop::Free {
                x: 0.1,
                y: 0.1,
                width: 0.8,
                height: 0.8,
            }),
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        }),
        perspective: Some(Perspective {
            version: 1,
            vertical: 0.2,
            horizontal: 0.0,
            rotation: 0.0,
            scale: 1.0,
            aspect_ratio: 1.0,
            shift_x: 0.0,
            shift_y: 0.0,
        }),
        ..Default::default()
    };
    assert!(
        unsupported_gpu_stages(&perspective_cropped).is_empty(),
        "perspective with an explicit crop is GPU-rendered"
    );

    // A lens/perspective correction **without** a crop activates the
    // content-based default crop, whose dimensions depend on the resampled
    // alpha — it is CPU-routed loudly (GPU-MAXRECT-WELLE).
    let perspective_uncropped = EditRecipe {
        perspective: Some(Perspective {
            version: 1,
            vertical: 0.2,
            horizontal: 0.0,
            rotation: 0.0,
            scale: 1.0,
            aspect_ratio: 1.0,
            shift_x: 0.0,
            shift_y: 0.0,
        }),
        ..Default::default()
    };
    let reasons = unsupported_gpu_stages(&perspective_uncropped);
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("geometry (default content crop)")),
        "an uncropped perspective must flag the default content crop: {reasons:?}"
    );

    // LRPAR-G06-UPRIGHT-15: an enabled upright analysis resolves to the
    // effective perspective. It behaves exactly like the equivalent manual
    // perspective — GPU-eligible with an explicit crop, and the content
    // default crop reason without one (never a silent CPU or identity path).
    let upright_analysis = lumina_sidecar::UprightAnalysis {
        fingerprint: lumina_sidecar::AnalysisFingerprint {
            algorithm: lumina_core::UPRIGHT_ALGORITHM.into(),
            version: lumina_core::UPRIGHT_ALGORITHM_VERSION.into(),
            input_fingerprint: "blake3:test".into(),
            extras: Default::default(),
        },
        vertical: 0.2,
        horizontal: 0.0,
        rotation: 0.0,
        line_count: 10,
        confidence: 0.5,
    };
    let upright_cropped = EditRecipe {
        geometry: Some(lumina_sidecar::Geometry {
            version: 1,
            crop: Some(lumina_sidecar::Crop::Free {
                x: 0.1,
                y: 0.1,
                width: 0.8,
                height: 0.8,
            }),
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        }),
        upright: Some(lumina_sidecar::Upright {
            version: 1,
            enabled: true,
            analysis: Some(upright_analysis.clone()),
        }),
        ..Default::default()
    };
    assert!(
        unsupported_gpu_stages(&upright_cropped).is_empty(),
        "an enabled upright with an explicit crop is GPU-rendered"
    );
    let upright_uncropped = EditRecipe {
        upright: Some(lumina_sidecar::Upright {
            version: 1,
            enabled: true,
            analysis: Some(upright_analysis),
        }),
        ..Default::default()
    };
    let reasons = unsupported_gpu_stages(&upright_uncropped);
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("geometry (default content crop)")),
        "an uncropped upright must flag the default content crop: {reasons:?}"
    );
    // A disabled upright is neutral: it neither routes nor changes the
    // effective perspective.
    let upright_disabled = EditRecipe {
        upright: Some(lumina_sidecar::Upright {
            version: 1,
            enabled: false,
            analysis: None,
        }),
        ..Default::default()
    };
    assert!(unsupported_gpu_stages(&upright_disabled).is_empty());
    assert!(upright_disabled.effective_perspective().is_none());

    // Effects (vignette/grain) is fully GPU-supported now.
    let effects = EditRecipe {
        effects: Some(lumina_sidecar::Effects::default()),
        ..Default::default()
    };
    assert!(unsupported_gpu_stages(&effects).is_empty());
}
