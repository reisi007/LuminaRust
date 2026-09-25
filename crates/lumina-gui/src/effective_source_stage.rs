//! Lifecycle of the captured `effective_source_stage`: the post-global,
//! post-geometry frame the mask-local white-balance picker samples.
//!
//! The local WB picker must never guess pixels. It samples the stage that the
//! core pipeline actually composited the local mask WB on top of, so the
//! capture is taken inside the core render hub
//! ([`LuminaApp::render_preview_frame`], which the shared GUI hub calls) before
//! the local mask composite runs, and is stored together with the [`RenderKey`]
//! digest that produced it.
//!
//! The capture is **opt-in** and lazy: retaining the stage costs a full-frame
//! copy, so it is only requested for renders that are part of a local-WB
//! sampling session (see
//! [`LuminaApp::wants_effective_source_stage`](super::LuminaApp::wants_effective_source_stage)).
//! CLI, export, and every global-only render keep the core field `None` and
//! copy nothing; a `None` stage and an absent digest are simply "no session",
//! never a stale sample.
//!
//! Two invariants live here, and both fail closed rather than degrading:
//!
//! * the captured stage and its digest are cleared together on every recipe,
//!   source or geometry edit ([`LuminaApp::clear_effective_source_stage`]), so
//!   a stale capture can never be sampled after the identity moved on; and
//! * the digest is recorded from the same `render_key` that the frame came
//!   from, so a mismatched pair is detectable instead of silently accepted.

use super::*;

impl LuminaApp {
    /// Whether the *next* render must retain the post-global/post-geometry
    /// stage.
    ///
    /// True while the local-WB picker is armed, and sticky for the rest of the
    /// session's mask-local WB work: a pick is a multi-step interaction
    /// (arm → sample → set delta), and arming alone does not re-render, so the
    /// first render after the arm has no stage yet. Keeping the request on
    /// while the selected layer still carries a relative-WB delta (or a stage
    /// is already held) means the picker always has a fresh stage for the live
    /// identity without paying the copy on unrelated renders.
    pub(crate) fn wants_effective_source_stage(&self) -> bool {
        self.local_wb_pick_mode
            || self.effective_source_stage.is_some()
            || self
                .selected_mask_local_wb_delta()
                .is_ok_and(|(temperature, tint)| temperature != 0.0 || tint != 0.0)
    }

    /// Run the shared core render for the preview, retaining the pre-local
    /// stage only when a local-WB pick session needs it.
    ///
    /// Both branches call the identical core stage sequence; the only
    /// difference is the opt-in full-frame copy in
    /// [`RenderOutput::effective_source_stage`]. Keeping the choice here makes
    /// the lazy contract a single, testable seam instead of a per-call-site
    /// decision.
    pub(crate) fn render_preview_frame(
        &self,
        base_frame: ImageFrame,
        context: &RenderContext<'_>,
        work: &mut StageWork,
        generative: GenerativeCanvasInput<'_>,
        denoise: &lumina_core::DenoiseStageInput<'_>,
    ) -> Result<lumina_core::RenderOutput, lumina_core::CoreError> {
        if self.wants_effective_source_stage() {
            render_frame_from_base_with_source_stage(base_frame, context, work, generative, denoise)
        } else {
            render_frame_from_base_with_generative_and_denoise(
                base_frame, context, work, generative, denoise,
            )
        }
    }

    /// Stores the post-global/post-geometry frame for the local WB picker and
    /// pins it to the render identity that produced it.
    ///
    /// The digest is derived from `render_key` at store time rather than being
    /// passed in, which makes it impossible to record a stage against an
    /// identity the caller did not actually render. The pair is updated
    /// together: a render that did not retain a stage clears both fields, so
    /// the app never holds a digest without a stage (or the reverse).
    pub(crate) fn set_effective_source_stage(&mut self, stage: Option<ImageFrame>) {
        match stage {
            Some(stage) => {
                self.effective_source_stage = Some(stage);
                self.effective_source_stage_digest =
                    self.render_key.as_ref().map(RenderKey::digest);
            }
            None => self.clear_effective_source_stage(),
        }
    }

    /// Drops the captured stage and its identity together.
    ///
    /// Called from the single invalidation entry point so a local WB pick after
    /// any edit demands a fresh render instead of sampling the previous
    /// identity.
    pub(crate) fn clear_effective_source_stage(&mut self) {
        self.effective_source_stage = None;
        self.effective_source_stage_digest = None;
    }
}
