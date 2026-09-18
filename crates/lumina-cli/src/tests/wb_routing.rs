use super::*;

/// R2-WB (parity with lumina-gui): non-finite or non-positive As-Shot gains
/// are dropped to `None` so the render degrades instead of aborting; a
/// healthy gain vector is kept verbatim.
#[test]
fn sanitize_camera_white_balance_rejects_non_finite_and_non_positive() {
    assert_eq!(
        sanitize_camera_white_balance([1.9, 1.0, 1.4, 1.0]),
        Some([1.9, 1.0, 1.4, 1.0])
    );
    assert_eq!(sanitize_camera_white_balance([0.0, 1.0, 1.0, 1.0]), None);
    assert_eq!(sanitize_camera_white_balance([-0.5, 1.0, 1.0, 1.0]), None);
    assert_eq!(
        sanitize_camera_white_balance([f32::NAN, 1.0, 1.0, 1.0]),
        None
    );
    assert_eq!(
        sanitize_camera_white_balance([f32::INFINITY, 1.0, 1.0, 1.0]),
        None
    );
    assert_eq!(
        sanitize_camera_white_balance([1.0, f32::NEG_INFINITY, 1.0, 1.0]),
        None
    );
}

/// R2-MCP-01 (CAMERA-WB-WELLE) + R2-GPU-05: the CLI routing decision no
/// longer CPU-routes a **valid** decoder As-Shot WB context — the gains are
/// carried into the GPU entry and validated there (the caller binds them via
/// `set_camera_white_balance`, like the Lensfun corrector) — while an
/// **invalid** context still forces the CPU route. Touched-but-reset sliders
/// at their neutral value stay GPU-allowed.
///
/// Pure reason-level assertions by design: `lumina-core` validates the
/// As-Shot gains without re-applying them to pixels, so a WB divergence is
/// not pixel-observable — the parity itself is pinned in `lumina-gpu`
/// (`as_shot_wb_gains_match_cpu_oracle_across_recipe_wb`).
#[cfg(feature = "gpu")]
#[test]
fn gpu_routing_reasons_carry_valid_wb_flag_invalid_and_respect_neutral_sliders() {
    // A valid context WB is carried, not flagged; absent is trivially clear.
    let recipe = EditRecipe::default();
    let with_wb = RenderContext {
        recipe: &recipe,
        camera_white_balance: Some([1.9, 1.0, 1.4, 1.0]),
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    assert!(
        gpu_routing_reasons(&recipe, &with_wb).is_empty(),
        "a valid As-Shot context must be GPU-carried, not a reason"
    );

    let without_wb = RenderContext {
        camera_white_balance: None,
        ..with_wb.clone()
    };
    assert!(gpu_routing_reasons(&recipe, &without_wb).is_empty());

    // An invalid context stays CPU-forcing so the oracle rejects it loudly.
    let invalid_wb = RenderContext {
        camera_white_balance: Some([0.0, 1.0, 1.0, 1.0]),
        ..with_wb.clone()
    };
    let reasons = gpu_routing_reasons(&recipe, &invalid_wb);
    assert!(
        reasons
            .iter()
            .any(|r| r.starts_with("camera_white_balance")),
        "{reasons:?}"
    );
    assert_eq!(reasons.len(), 1, "{reasons:?}");

    // Touched-but-reset sliders stay GPU-allowed …
    let touched_reset = EditRecipe {
        adjustments: BTreeMap::from([
            ("vibrance".to_string(), 0.0),
            ("saturation".to_string(), 0.0),
        ]),
        ..Default::default()
    };
    let reset_ctx = RenderContext {
        recipe: &touched_reset,
        ..without_wb.clone()
    };
    assert!(gpu_routing_reasons(&touched_reset, &reset_ctx).is_empty());

    // … while a recipe stage the GPU genuinely cannot render keeps forcing
    // the CPU route. Every schema adjustment key is GPU-rendered now
    // (GPU-RENDER-PARITY-1: tone + detail + Red-Eye), so the honest,
    // permanent probe here is the unknown-key class: a key outside the
    // recipe schema has no neutral default, the CPU reference rejects it
    // outright and no GPU stage could ever accept it — unlike
    // geometry/lens_correction/perspective/lens_blur/spot_removals/
    // generative_edit (all queued for GPU parity in GPU-RENDER-PARITY-1,
    // so they would go stale again). The same class is pinned by
    // `cpu_routing_inventory_is_complete` in `lumina-gpu`.
    let unsupported = EditRecipe {
        adjustments: BTreeMap::from([("clarity_v2".to_string(), 0.5)]),
        ..Default::default()
    };
    let unsupported_ctx = RenderContext {
        recipe: &unsupported,
        ..reset_ctx
    };
    let reasons = gpu_routing_reasons(&unsupported, &unsupported_ctx);
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("clarity_v2") && r.contains("not implemented on GPU")),
        "{reasons:?}"
    );

    // An invalid WB context stacks with other context-level reasons instead
    // of replacing them.
    let artifact = SourceActionArtifact {
        region: MaskPlane {
            width: 4,
            height: 4,
            values: vec![u16::MAX; 16],
        },
        replacement: ImageFrame::new(4, 4, vec![0; 4 * 4 * 4]).unwrap(),
    };
    let stacked = RenderContext {
        source_actions: std::slice::from_ref(&artifact),
        ..invalid_wb
    };
    let reasons = gpu_routing_reasons(&recipe, &stacked);
    assert_eq!(reasons.len(), 2, "{reasons:?}");
    assert!(
        reasons
            .iter()
            .any(|r| r.starts_with("camera_white_balance")),
        "{reasons:?}"
    );
    assert!(reasons.iter().any(|r| r.contains("source_actions")));
}
