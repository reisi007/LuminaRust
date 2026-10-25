use super::*;

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

#[cfg(feature = "gpu")]
fn resource_less_context() -> GpuContext {
    GpuContext {
        resources: None,
        pipeline: std::sync::Mutex::new(None),
        sa_pipeline: std::sync::Mutex::new(None),
        spot_pipeline: std::sync::Mutex::new(None),
        post_pipeline: std::sync::Mutex::new(None),
        geometry_pipeline: std::sync::Mutex::new(None),
        lens_blur_pipeline: std::sync::Mutex::new(None),
        depth_plane: None,
        lensfun_map: None,
        vram: std::sync::Mutex::new(VramPool::new()),
        source_actions: None,
        sa_textures: None,
        recipe: None,
        rwgpu_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        camera_white_balance: std::sync::Mutex::new(None),
    }
}

#[cfg(feature = "gpu")]
#[test]
fn resource_less_context_reports_adapter_fallback_for_both_render_entries() {
    let context = resource_less_context();
    let source = sample_frame();
    let expected_frame = Frame::from_image_frame(source.clone());
    let recipe = EditRecipe::default();

    let (rendered, report) = context
        .render_with_gpu_report(&source, &recipe)
        .expect("resource-less context renders through the CPU reference");
    assert_eq!(rendered, expected_frame);
    assert_eq!(
        report,
        RenderExecutionReport::CpuFallback {
            reason: CpuFallbackReason::AdapterOrDeviceUnavailable,
        }
    );

    let generative = lumina_core::GenerativeCanvasInput::default();
    let (rendered, report) = context
        .render_with_gpu_and_generative_report(&source, &recipe, &generative)
        .expect("resource-less generative render uses the CPU reference");
    assert_eq!(rendered, expected_frame);
    assert_eq!(
        report,
        RenderExecutionReport::CpuFallback {
            reason: CpuFallbackReason::AdapterOrDeviceUnavailable,
        }
    );
}

#[cfg(not(feature = "gpu"))]
#[test]
fn cpu_only_report_names_the_compile_time_fallback() {
    let context = GpuContext::new().expect("CPU-only context initializes");
    let source = sample_frame();
    let (rendered, report) = context
        .render_with_gpu_report(&source, &EditRecipe::default())
        .expect("default CPU render succeeds");

    assert_eq!(
        report,
        RenderExecutionReport::CpuFallback {
            reason: CpuFallbackReason::GpuFeatureDisabled,
        }
    );
    assert_eq!(rendered, Frame::from_image_frame(source));
}

#[test]
fn render_error_is_not_returned_as_an_execution_report() {
    let context = GpuContext::new().expect("context initializes");
    let recipe = EditRecipe {
        adjustments: std::collections::BTreeMap::from([("not_a_real_adjustment".into(), 1.0)]),
        ..EditRecipe::default()
    };

    assert!(context
        .render_with_gpu_report(&sample_frame(), &recipe)
        .is_err());
}

#[cfg(all(feature = "gpu", feature = "gpu-adapter-tests"))]
#[test]
fn device_loss_reports_cpu_fallback_for_that_call() {
    let context = GpuContext::new().expect("GPU context initializes");
    assert!(
        context.is_available(),
        "adapter gate requires a real adapter"
    );
    context
        .resources
        .as_ref()
        .expect("available context has GPU resources")
        .device_lost
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let source = sample_frame();
    let (rendered, report) = context
        .render_with_gpu_report(&source, &EditRecipe::default())
        .expect("device-loss fallback succeeds on the CPU");

    assert_eq!(
        report,
        RenderExecutionReport::CpuFallback {
            reason: CpuFallbackReason::DeviceLost,
        }
    );
    assert_eq!(rendered, Frame::from_image_frame(source));
}
