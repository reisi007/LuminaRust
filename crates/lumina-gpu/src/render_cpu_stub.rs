//! Full-frame and VRAM API stubs for builds without the `gpu` feature.

use super::{
    render_cpu, render_cpu_with_generative, CpuFallbackReason, EditRecipe, Frame, GpuContext,
    GpuError, ImageFrame, RenderExecutionReport,
};

impl GpuContext {
    /// Create a CPU-only context (the `gpu` feature is disabled, so no adapter
    /// is ever bound). Rendering always uses the CPU fallback.
    pub fn new() -> Result<Self, GpuError> {
        Ok(Self {
            camera_white_balance: std::sync::Mutex::new(None),
        })
    }

    /// Always `false` without the `gpu` feature.
    pub fn is_available(&self) -> bool {
        false
    }

    /// Always `None` without the `gpu` feature (no adapter can be bound).
    pub fn adapter_info(&self) -> Option<String> {
        None
    }

    /// CPU fallback render (the only path when the `gpu` feature is off): runs
    /// the full CPU reference chain (the free `render_cpu`).
    pub fn render_with_gpu(
        &self,
        frame: &ImageFrame,
        recipe: &EditRecipe,
    ) -> Result<Frame, GpuError> {
        self.render_with_gpu_report(frame, recipe)
            .map(|(frame, _report)| frame)
    }

    /// Render one frame and report the actual execution outcome.
    ///
    /// Without the `gpu` feature this reports `GpuFeatureDisabled` after a
    /// successful CPU reference render. A render error remains an error and
    /// carries no report.
    pub fn render_with_gpu_report(
        &self,
        frame: &ImageFrame,
        recipe: &EditRecipe,
    ) -> Result<(Frame, RenderExecutionReport), GpuError> {
        render_cpu(frame, recipe, None, self.camera_white_balance()).map(|frame| {
            (
                frame,
                RenderExecutionReport::CpuFallback {
                    reason: CpuFallbackReason::GpuFeatureDisabled,
                },
            )
        })
    }

    /// GEN-ONNX-1: CPU-only generative render, preserving the caller's artifacts.
    pub fn render_with_gpu_and_generative(
        &self,
        frame: &ImageFrame,
        recipe: &EditRecipe,
        generative: &lumina_core::GenerativeCanvasInput<'_>,
    ) -> Result<Frame, GpuError> {
        self.render_with_gpu_and_generative_report(frame, recipe, generative)
            .map(|(frame, _report)| frame)
    }

    /// Artifact-aware CPU render with its actual execution outcome.
    pub fn render_with_gpu_and_generative_report(
        &self,
        frame: &ImageFrame,
        recipe: &EditRecipe,
        generative: &lumina_core::GenerativeCanvasInput<'_>,
    ) -> Result<(Frame, RenderExecutionReport), GpuError> {
        render_cpu_with_generative(
            frame,
            recipe,
            None,
            self.camera_white_balance(),
            Some(generative),
        )
        .map(|frame| {
            (
                frame,
                RenderExecutionReport::CpuFallback {
                    reason: CpuFallbackReason::GpuFeatureDisabled,
                },
            )
        })
    }

    pub fn perf_log_enabled() -> bool {
        false
    }
    pub fn ensure_vram(&self, _w: u32, _h: u32) -> Result<(), GpuError> {
        Ok(())
    }
    pub fn render_to_vram(&self, _f: &ImageFrame, _r: &EditRecipe) -> Result<(), GpuError> {
        Ok(())
    }
    pub fn upload_mask_tile(
        &self,
        _x: u32,
        _y: u32,
        _w: u32,
        _h: u32,
        _d: &[u8],
    ) -> Result<(), GpuError> {
        Ok(())
    }
    pub fn copy_vram_to_texture(&self, _d: &(), _overlay_color: [u8; 3]) -> Result<(), GpuError> {
        Ok(())
    }
}
