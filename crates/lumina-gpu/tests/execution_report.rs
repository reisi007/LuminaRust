//! Per-call execution evidence for the full-frame GPU renderer.

mod support;

use lumina_core::ImageFrame;
use lumina_gpu::{CpuFallbackReason, GpuContext, RenderExecutionReport};
use lumina_sidecar::{
    EditRecipe, SourceActionArtifactRef, SourceActionKind, SourceActionSpec, SOURCE_ACTION_VERSION,
};

fn sample_frame() -> ImageFrame {
    ImageFrame::new(
        2,
        2,
        vec![
            12, 30, 90, 255, 70, 100, 140, 255, 180, 30, 50, 255, 240, 200, 10, 255,
        ],
    )
    .expect("valid sample frame")
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
fn adapter_render_reports_gpu_completion_only_after_readback() {
    let context = GpuContext::new().expect("GPU context initializes");
    assert!(support::require_adapter(&context));

    let (frame, report) = context
        .render_with_gpu_report(&sample_frame(), &EditRecipe::default())
        .expect("GPU render and final readback succeed");

    assert_eq!(report, RenderExecutionReport::GpuPipelineCompleted);
    assert_eq!((frame.width, frame.height), (2, 2));
    assert_eq!(frame.pixels.len(), 2 * 2 * 4);
}

#[test]
#[cfg_attr(not(feature = "gpu-adapter-tests"), ignore = "requires a GPU adapter")]
fn unsupported_recipe_report_names_the_real_internal_cpu_route() {
    let context = GpuContext::new().expect("GPU context initializes");
    assert!(support::require_adapter(&context));
    let recipe = EditRecipe {
        source_actions: vec![SourceActionSpec {
            version: SOURCE_ACTION_VERSION,
            kind: SourceActionKind::DustRemoval,
            artifact: SourceActionArtifactRef {
                id: "repair-1".into(),
                relative_path: "test.lumina.zdata".into(),
                checksum: "unused".into(),
            },
        }],
        ..EditRecipe::default()
    };

    let (_, report) = context
        .render_with_gpu_report(&sample_frame(), &recipe)
        .expect("unsupported stage is rendered by the CPU reference");

    assert_eq!(
        report,
        RenderExecutionReport::CpuFallback {
            reason: CpuFallbackReason::UnsupportedStages(vec!["source_actions".into()]),
        }
    );
}
