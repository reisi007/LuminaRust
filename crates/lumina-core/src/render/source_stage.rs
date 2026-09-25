//! MASK-LOCAL-P1.1 opt-in capture of the effective source stage.
//!
//! The post-global/post-geometry frame (the stage local mask recipes are
//! composited on top of) is what the GUI mask-local white-balance picker
//! samples. Retaining it costs a full-frame copy, so it is **not** part of
//! every render: the shared hub takes an [`EffectiveSourceStage`] and this
//! module owns both public entry points that select it.
//!
//! * [`Skip`] is the default for every CLI, export, global-only and stand-in
//!   render — the field stays `None` and no pixels are copied.
//! * [`Capture`] exists for the one identified consumer that must never guess
//!   pixels. It adds a clone and changes nothing else about the output.

use super::{
    render_frame_from_base_impl, CoreError, ImageFrame, RenderContext, RenderOutput, StageWork,
};

/// Whether the shared render hub must clone the pre-local frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EffectiveSourceStage {
    /// Do not retain the stage; [`RenderOutput::effective_source_stage`] stays
    /// `None` and no full-frame copy is made.
    Skip,
    /// Retain the stage for an explicit, identified sampler.
    Capture,
}

/// MASK-LOCAL-P1.1: the render entry that additionally retains the
/// post-global/post-geometry stage in [`RenderOutput::effective_source_stage`].
///
/// This is the only entry point that pays the full-frame clone. A caller that
/// does not sample from the stage must use
/// [`super::render_frame_from_base_with_generative_and_denoise`] instead; the
/// rendered bytes are identical either way.
pub fn render_frame_from_base_with_source_stage(
    base: ImageFrame,
    context: &RenderContext<'_>,
    work: &mut StageWork,
    generative: crate::generative::GenerativeCanvasInput<'_>,
    denoise: &crate::DenoiseStageInput<'_>,
) -> Result<RenderOutput, CoreError> {
    render_frame_from_base_impl(
        base,
        context,
        work,
        generative,
        denoise,
        EffectiveSourceStage::Capture,
    )
}
