use super::*;

#[test]
fn pre_routed_cpu_render_has_no_fabricated_gpu_report() {
    let recipe = EditRecipe::default();
    let frame = ImageFrame::new(1, 1, vec![0, 0, 0, 255]).unwrap();
    let source_actions = [];
    let render_ctx = RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &source_actions,
        masks: None,
        lensfun: None,
        depth: None,
    };

    let rendered = render_best_effort_with_report(
        None,
        &frame,
        &recipe,
        &render_ctx,
        GenerativeCanvasInput::default(),
    )
    .unwrap();

    assert!(!rendered.route.is_gpu());
    assert!(rendered.gpu_report.is_none());
}

fn successful_mask_outcome() -> Result<RenderOutput, String> {
    Ok(RenderOutput {
        frame: ImageFrame::new(1, 1, vec![0, 0, 0, 255]).unwrap(),
        effective_source_stage: None,
        mask_layers: Vec::new(),
        mask_warnings: Vec::new(),
    })
}

#[test]
fn completed_gpu_report_with_successful_mask_stage_routes_gpu_without_reasons() {
    let report = lumina_gpu::RenderExecutionReport::GpuPipelineCompleted;
    let masks = successful_mask_outcome();

    let route = route_after_mask_stage(&report, &masks);

    assert!(route.is_gpu());
    assert_eq!(route.reasons(), &Vec::<String>::new());
}

#[test]
fn completed_gpu_report_with_failed_mask_stage_uses_exact_mask_reason() {
    let report = lumina_gpu::RenderExecutionReport::GpuPipelineCompleted;
    let masks = Err("local mask rejected".to_owned());

    let route = route_after_mask_stage(&report, &masks);

    assert!(!route.is_gpu());
    assert_eq!(route.reasons(), &["local mask rejected"]);
}

#[test]
fn internal_cpu_fallback_with_successful_mask_stage_keeps_exact_internal_reason() {
    let report = lumina_gpu::RenderExecutionReport::CpuFallback {
        reason: lumina_gpu::CpuFallbackReason::DeviceLost,
    };
    let masks = successful_mask_outcome();

    let route = route_after_mask_stage(&report, &masks);

    assert!(!route.is_gpu());
    assert_eq!(
        route.reasons(),
        &["lumina-gpu CPU fallback: DeviceLost".to_owned()]
    );
}

#[test]
fn internal_cpu_fallback_with_failed_mask_stage_keeps_both_reasons_in_order() {
    let report = lumina_gpu::RenderExecutionReport::CpuFallback {
        reason: lumina_gpu::CpuFallbackReason::PipelineUnavailable,
    };
    let masks = Err("local mask rejected".to_owned());

    let route = route_after_mask_stage(&report, &masks);

    assert!(!route.is_gpu());
    assert_eq!(
        route.reasons(),
        &[
            "lumina-gpu CPU fallback: PipelineUnavailable".to_owned(),
            "local mask rejected".to_owned(),
        ]
    );
}
