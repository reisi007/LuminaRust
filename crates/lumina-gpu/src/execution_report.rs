/// Execution outcome for one successful `GpuContext` full-frame render.
///
/// This report describes only the renderer call that returned it. It does not
/// claim that every application-level stage was GPU-parity-tested, or identify
/// why a separate caller observed different pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderExecutionReport {
    /// The GPU tone/pipeline path submitted its work and the final readback
    /// completed successfully.
    ///
    /// This variant is only constructed after the output frame has been read
    /// back. It is execution evidence, not a CPU-parity or root-cause claim.
    GpuPipelineCompleted,
    /// This call produced its frame through the complete CPU reference instead
    /// of the internal GPU render path.
    CpuFallback {
        /// The exact internal fallback branch taken by this call.
        reason: CpuFallbackReason,
    },
}

/// Why one full-frame `lumina-gpu` render used its CPU reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CpuFallbackReason {
    /// This crate was built without its `gpu` feature.
    GpuFeatureDisabled,
    /// No usable GPU resources (adapter/device/queue) were bound to the context.
    AdapterOrDeviceUnavailable,
    /// The context's existing device-loss callback marked the GPU unusable.
    DeviceLost,
    /// The recipe contains stages that this GPU renderer cannot execute.
    UnsupportedStages(Vec<String>),
    /// Pipeline initialization completed without a render pipeline being
    /// available, so the existing renderer chose its CPU reference.
    PipelineUnavailable,
}
