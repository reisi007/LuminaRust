//! GUI-REFACTOR-W1-20 S1.4b: GPU present routing — adapter override, readback
//! and prime parity hooks, the VRAM-fresh/stage gate, the Lensfun map bind and
//! the CPU-routing refusal classification/formatting, extracted verbatim from
//! `lib.rs`.
//!
//! This module owns only routing/observability: which present path is taken,
//! how a GPU refusal is classified into a visible badge, and the diagnostic
//! adapter/readback/prime hooks the parity tests drive. The pixels are
//! unchanged — every routing decision that would diverge silently instead
//! keeps the exact CPU present route (Agents.md: kein stiller Fallback).
//!
//! Methods called from other GUI modules (the present path, the draft tick) are
//! `pub(crate)`; the diagnostic parity hooks stay `pub` for the integration
//! tests.

use super::*;
use log::warn;

impl LuminaApp {
    /// GPU-LENSFUN-PARITY-1 (GUI-Wiring): refresh the auto-corrector for exactly
    /// `width × height` and bind its `LensfunMap` on the GPU context. Returns
    /// `None` when the VRAM present may proceed (no non-identity corrector, or
    /// its map is bound) and `Some(reason)` when an active corrector's map could
    /// not be bound — the caller must then keep the exact CPU present route and
    /// surface the reason (never a silent manual-model render).
    #[cfg(all(feature = "gpu", feature = "lensfun"))]
    pub(crate) fn bind_lensfun_map(&mut self, width: u32, height: u32) -> Option<&'static str> {
        self.ensure_lensfun_cache(width, height);
        let gpu = self.gpu.as_mut()?;
        lensfun_gpu::bind(gpu, self.lensfun_cache.as_mut(), width, height)
    }

    /// Non-Lensfun build: no corrector can exist, so the VRAM route is never
    /// blocked by one.
    #[cfg(all(feature = "gpu", not(feature = "lensfun")))]
    pub(crate) fn bind_lensfun_map(&mut self, _width: u32, _height: u32) -> Option<&'static str> {
        None
    }

    /// KITTEST-PARITY-PATHS-1: whether a usable GPU adapter is bound. The
    /// path-parity framework reports an explicit SKIP verdict when this is
    /// `false` — a missing adapter never counts as a silently green parity
    /// check (same policy as the `lumina-gpu` oracle tests).
    ///
    /// PARITY-PATHS-2: honours the diagnostic override
    /// ([`Self::set_gpu_adapter_override`]) so the adapterless SKIP branch is
    /// executable on an adapter-equipped machine.
    #[cfg(feature = "gpu")]
    #[must_use]
    pub fn gpu_adapter_available(&self) -> bool {
        if let Some(forced) = self.gpu_adapter_override {
            return forced;
        }
        self.gpu
            .as_ref()
            .is_some_and(lumina_gpu::GpuContext::is_available)
    }

    /// PARITY-PATHS-2 test hook: force the verdict of
    /// [`Self::gpu_adapter_available`] so the parity matrix's adapterless SKIP
    /// branch is exercised for real on a Metal machine (the verification gap
    /// noted in the task: the branch could previously only be code-reviewed).
    /// `Some(false)` simulates a machine without a usable adapter, `Some(true)`
    /// simulates one with, and `None` restores the real GPU-context probe.
    /// Diagnostic only — no presentation or render logic reads this field.
    #[cfg(feature = "gpu")]
    #[doc(hidden)]
    pub fn set_gpu_adapter_override(&mut self, available: Option<bool>) {
        self.gpu_adapter_override = available;
    }

    /// PARITY-PATHS-2 test hook: bind a caller-owned Lensfun corrector as if
    /// the EXIF auto-profile cache ([`Self::ensure_lensfun_cache`]) had resolved
    /// it, so the parity matrix can exercise the active-corrector GPU route
    /// headlessly without a real EXIF/DB match. The corrector must be built for
    /// the frame geometry the next render uses (the GUI builds its own
    /// corrector at exactly the base-frame dimensions). Diagnostic only: like
    /// the real cache this keeps the DB handle alive for the modifier and marks
    /// the corrector `active` when it is non-identity, so the present path binds
    /// a `LensfunMap` for it (`lensfun_gpu::bind`).
    #[cfg(all(feature = "gpu", feature = "lensfun"))]
    #[doc(hidden)]
    pub fn bind_test_lensfun_corrector(
        &mut self,
        corrector: lumina_lensfun::Corrector,
        db: lumina_lensfun::LensfunDb,
    ) {
        let active = !corrector.is_identity();
        self.lensfun_cache = Some(CachedLensCorrector {
            corrector,
            _db: db,
            key: (None, None, None, 0, 0, 0, 0),
            active,
            gpu_map: None,
        });
    }

    /// KITTEST-PARITY-PATHS-1: render the current draft (or full) source through
    /// the interactive VRAM path and read the result back as an [`ImageFrame`]
    /// — the GPU-path counterpart of the CPU [`Self::preview`] frame for the
    /// CPU↔GPU parity assertion. Diagnostics only: the preview texture, the
    /// recipe and the sidecar are untouched. `Ok(None)` when no usable adapter
    /// is bound (the caller then prints the SKIP verdict — never a substituted
    /// CPU frame passed off as a GPU result).
    ///
    /// GPU-LENSFUN-PARITY-1: binds the corrector's `LensfunMap` for the source
    /// dimensions first; an active corrector whose map cannot be bound is a loud
    /// error (never a readback of the uncorrected manual-model frame).
    #[cfg(feature = "gpu")]
    pub fn render_gpu_readback_frame(&mut self) -> Result<Option<ImageFrame>, GuiError> {
        let Some((width, height)) = self
            .draft_original
            .as_ref()
            .or(self.original.as_ref())
            .map(|source| (source.width, source.height))
        else {
            return Ok(None);
        };
        if !self.gpu.as_ref().is_some_and(|gpu| gpu.is_available()) {
            return Ok(None);
        }
        if let Some(reason) = self.bind_lensfun_map(width, height) {
            return Err(GuiError::Io(format!(
                "gpu parity readback kept on the CPU reference: {reason}"
            )));
        }
        let Some(gpu) = self.gpu.as_ref() else {
            return Ok(None);
        };
        let Some(source) = self.draft_original.as_ref().or(self.original.as_ref()) else {
            return Ok(None);
        };
        gpu.render_to_vram(source, self.gpu_present_recipe().as_ref())
            .map_err(|error| GuiError::Io(format!("gpu parity readback: {error}")))?;
        let frame = gpu
            .readback_output_frame()
            .map_err(|error| GuiError::Io(format!("gpu parity readback: {error}")))?;
        Ok(Some(frame.to_image_frame()))
    }

    /// KITTEST-PARITY-PATHS-1: pixel size of the GPU-presented preview for the
    /// frame painted last, or `None` when the CPU present path was used. The
    /// parity framework asserts the GPU matrix cell really presented from VRAM
    /// (`Some`) and the CPU cell really did not (`None`) — so a silent GPU→CPU
    /// fallback can never be mislabelled as a passing GPU golden.
    #[cfg(feature = "gpu")]
    #[must_use]
    pub fn gpu_present_frame_size(&self) -> Option<[usize; 2]> {
        self.gpu_present_frame.map(|(_, size)| size)
    }

    /// KITTEST-PARITY-PATHS-1: run the interactive VRAM tone/detail pass on the
    /// current draft (or full) source and mark the VRAM result fresh, so the
    /// next frame's present path draws the GPU pixels exactly like the
    /// pointer-drag hot path (`render_draft_tick`). Returns `true` when a usable
    /// adapter rendered the VRAM result; `false` (with `vram_fresh` left
    /// `false`) when no adapter is bound or an active Lensfun corrector's map
    /// could not be bound — never a silent CPU substitution.
    #[cfg(feature = "gpu")]
    pub fn prime_gpu_present(&mut self) -> bool {
        if !self.gpu.as_ref().is_some_and(|gpu| gpu.is_available()) {
            return false;
        }
        let Some((width, height)) = self
            .draft_original
            .as_ref()
            .or(self.original.as_ref())
            .map(|source| (source.width, source.height))
        else {
            return false;
        };
        if let Some(reason) = self.bind_lensfun_map(width, height) {
            warn!("gpu parity prime kept on the CPU reference: {reason}");
            self.vram_fresh = false;
            return false;
        }
        let rendered = {
            let Some(source) = self.draft_original.as_ref().or(self.original.as_ref()) else {
                return false;
            };
            let recipe = self.gpu_present_recipe();
            self.gpu
                .as_ref()
                .expect("availability checked above")
                .render_to_vram(source, recipe.as_ref())
        };
        match rendered {
            Ok(()) => {
                self.vram_fresh = true;
                true
            }
            Err(error) => {
                warn!("gpu parity prime failed: {error}");
                self.vram_fresh = false;
                false
            }
        }
    }

    /// R2-GUIMOD-01: does the VRAM content describe the pixels currently
    /// displayed? For a **full-quality** preview only exact dimension equality
    /// proves that preview and VRAM tone result show the same crop of the same
    /// source; any mismatch keeps the (exact) CPU present path. A displayed
    /// *draft* is exempt by design: the interactive drag path presents exactly
    /// the draft-source VRAM render it just produced.
    #[cfg(feature = "gpu")]
    pub(crate) fn vram_content_matches_displayed_preview(&self, dims: (u32, u32)) -> bool {
        if self.preview_is_draft {
            return true;
        }
        match self.preview.as_ref() {
            Some(preview) => preview.width == dims.0 && preview.height == dims.1,
            None => false,
        }
    }

    /// R2-GUIMOD-05: `!lumina_gpu::unsupported_gpu_stages(&self.recipe)
    /// .is_empty()`, memoized against the current render key. The verdict can
    /// only change when the recipe/source/copy identity changes — exactly what
    /// replaces the render key — so the per-frame `Vec<String>` rebuild with
    /// its `format!` allocations collapses to one call per render.
    ///
    /// CAMERA-WB-WELLE: the memo key additionally carries the caller-owned
    /// As-Shot WB context — a verdict computed with one context must never be
    /// served for another. GPU-LENSFUN-PARITY-1 removed the former Lensfun
    /// corrector key component: the corrector is no longer a recipe-gate reason
    /// (its map is bound on the GPU present path; an unbindable map and the
    /// `lumina-gpu` map guards surface through [`Self::vram_render_refusal`]
    /// instead).
    ///
    /// While no render key exists (dirty preview) the memo is deliberately
    /// bypassed *and* not populated: the recipe may drift between edits
    /// without ever producing an intermediate key, so a `None`-keyed entry
    /// could serve a verdict for a long-gone recipe.
    #[cfg(feature = "gpu")]
    pub(crate) fn recipe_has_unsupported_gpu_stages(&mut self) -> bool {
        if self.sidecar_resolution_pending() {
            return true;
        }
        if self.refresh_gpu_stage_gate() {
            self.gpu_stage_gate
                .as_ref()
                .is_some_and(|(_, reasons)| !reasons.is_empty())
        } else {
            !self.gpu_unsupported_reasons().is_empty()
        }
    }

    /// GUI-LENSFUN-GATE-2: the precise reason list behind
    /// [`Self::recipe_has_unsupported_gpu_stages`], memoized identically. The
    /// visible routing badge consumes this so the user sees *why* the CPU route
    /// was taken (`geometry (default content crop)`, an invalid As-Shot context,
    /// …) instead of only the generic headline — never a silent fallback.
    #[cfg(feature = "gpu")]
    pub(crate) fn gpu_unsupported_stage_reasons(&mut self) -> Vec<String> {
        if self.sidecar_resolution_pending() {
            return vec!["source sidecar unavailable".to_string()];
        }
        if self.refresh_gpu_stage_gate() {
            self.gpu_stage_gate
                .as_ref()
                .map(|(_, reasons)| reasons.clone())
                .unwrap_or_default()
        } else {
            self.gpu_unsupported_reasons()
        }
    }

    /// Populate the memoized gate verdict for the current render key and
    /// As-Shot WB context when it is stale. Returns whether a valid keyed entry
    /// now exists — `false` (with the memo cleared) while no render key exists,
    /// so a drifting recipe can never be served a `None`-keyed verdict (see the
    /// doc above).
    #[cfg(feature = "gpu")]
    fn refresh_gpu_stage_gate(&mut self) -> bool {
        let wb = self.camera_white_balance;
        let fresh = self
            .gpu_stage_gate
            .as_ref()
            .is_some_and(|((key, cached_wb), _)| {
                Some(key) == self.render_key.as_ref() && *cached_wb == wb
            });
        if fresh {
            return true;
        }
        let Some(key) = self.render_key.clone() else {
            self.gpu_stage_gate = None;
            return false;
        };
        let reasons = self.gpu_unsupported_reasons();
        // R3-DENOISE-2: the recipe-gate CPU route needs a log line, not only the
        // post-gate present refusals. The verdict memo holds the *previous*
        // key's reason set, so comparing against it emits exactly one `warn!`
        // per distinct reason set: a slider drag (new render key per tick, same
        // gate verdict) warns once, a changed set warns again, and an empty
        // verdict re-arms the next occurrence. Never a silent CPU route.
        let unchanged = self
            .gpu_stage_gate
            .as_ref()
            .is_some_and(|(_, previous)| previous == &reasons);
        if !reasons.is_empty() && !unchanged {
            warn!(
                "gpu present refused by recipe gate, keeping CPU route: {}",
                reasons.join("; ")
            );
            #[cfg(test)]
            crate::timing::note_gpu_gate_route_warn();
        }
        self.gpu_stage_gate = Some(((key, wb), reasons));
        true
    }

    /// GUI-LENSFUN-GATE-1: the GPU routing reason list for the current
    /// recipe/context, mirroring the CLI `gpu_routing_reasons`. Split out so
    /// the list is directly testable without a bound GPU adapter.
    ///
    /// R3-ROUTING-1 (B1): the gate must evaluate the **same recipe the VRAM
    /// present path actually renders** ([`Self::gpu_present_recipe`]) — while
    /// the crop tool is armed that is the geometry-free display recipe. Judging
    /// the committed recipe instead left the crop-tool + lens/perspective +
    /// committed-crop combination with an empty badge while `render_to_vram`
    /// refused the display recipe's `default content crop` (silent CPU route).
    ///
    /// GPU-LENSFUN-PARITY-1: a non-identity Lensfun corrector is **no longer** a
    /// reason here. The present path binds its `LensfunMap` on the GPU
    /// (`lensfun_gpu::bind`); only an unbindable map keeps the exact CPU route,
    /// and that surfaces through [`Self::vram_render_refusal`] (bind failure) or
    /// the `lumina-gpu` map guard (`lensfun_map.dimensions` /
    /// `lensfun_map.default_content_crop`), classified in
    /// [`Self::classify_vram_refusal`].
    #[cfg(feature = "gpu")]
    pub(crate) fn gpu_unsupported_reasons(&self) -> Vec<String> {
        let recipe = self.gpu_present_recipe();
        lumina_gpu::unsupported_gpu_stages_with_context(
            recipe.as_ref(),
            false,
            self.camera_white_balance.as_ref(),
        )
    }

    /// R3-ROUTING-1: the recipe the readback-free VRAM present path must
    /// evaluate for the frame currently displayed.
    ///
    /// While the interactive crop tool is armed the preview shows the
    /// geometry-free full frame ([`Self::crop_mode_display_recipe`]); the GPU
    /// must evaluate **exactly** that recipe. With the committed crop still in
    /// the recipe, `render_to_vram` refuses the dimension-changing output on
    /// every tick and the display silently splits between the CPU render
    /// (geometry-free) and a badge-less/refused GPU row (the R3-ROUTING-1
    /// finding: 38 refusals, GPU time wasted). The normal path borrows the real
    /// recipe without a clone; the routing decision is trace-visible (R3-LOG-1).
    #[cfg(feature = "gpu")]
    pub(crate) fn gpu_present_recipe(&self) -> std::borrow::Cow<'_, EditRecipe> {
        if self.crop_mode {
            crate::timing::emit(|| {
                "GUI routing: gpu present recipe=crop-display (geometry removed)".to_string()
            });
            std::borrow::Cow::Owned(self.crop_mode_display_recipe())
        } else {
            std::borrow::Cow::Borrowed(&self.recipe)
        }
    }

    /// R2-GUIMOD-06: classify *why* the GPU present path was not taken this
    /// frame, so the silent GPU→CPU routing can be shown as a visible badge.
    ///
    /// Returns `Some(reason)` only when a GPU context exists **and** is usable
    /// (`is_available`) **and** the recipe cannot be fully evaluated on the VRAM
    /// tone path — i.e. the preview is being computed on the CPU for a
    /// capability reason rather than because of an editorial state (stale VRAM,
    /// Before/After toggle, zoomed ROI, or a missing present target). In all
    /// other cases there is no "fallback" to report and `None` is returned.
    ///
    /// GUI-LENSFUN-GATE-2: the returned badge names every precise reason from
    /// [`Self::gpu_unsupported_stage_reasons`] — not just the generic headline —
    /// so the user can see *why* the CPU route was taken (Agents.md: kein
    /// stiller Fallback). Reuses the memoized verdict, so calling it every frame
    /// is cheap once the render key is stable.
    ///
    /// GUI-LENSFUN-GATE-3 (F1): a dimension-changing geometry chain is refused
    /// by [`lumina_gpu::GpuContext::render_to_vram`] *after* the recipe gate
    /// passed, so the gate alone left that CPU route badge-less. When the gate
    /// is empty, the captured present refusal
    /// ([`Self::vram_render_refusal`]) is used as the reason. Routing and
    /// pixels are unchanged — this is observability only.
    #[cfg(feature = "gpu")]
    pub(crate) fn routing_fallback_reason(&mut self) -> Option<String> {
        if !self.gpu.as_ref().is_some_and(|gpu| gpu.is_available()) {
            return None;
        }
        let reasons = self.gpu_unsupported_stage_reasons();
        let reasons = Self::combine_routing_reasons(reasons, self.vram_render_refusal.as_deref());
        Self::format_routing_fallback_reason(&reasons)
    }

    /// GUI-LENSFUN-GATE-3 (F1): merge the recipe-gate reasons with a captured
    /// present refusal. The refusal is only appended while the gate itself has
    /// no reason — a gate reason already explains the CPU route, and a
    /// dimension-changing refusal is a *post-gate* condition that never
    /// coincides with a gate reason. Pure so the merge is testable without a
    /// bound adapter.
    #[cfg(feature = "gpu")]
    pub(crate) fn combine_routing_reasons(
        mut reasons: Vec<String>,
        present_refusal: Option<&str>,
    ) -> Vec<String> {
        if reasons.is_empty() {
            if let Some(refusal) = present_refusal.filter(|reason| !reason.is_empty()) {
                reasons.push(refusal.to_string());
            }
        }
        reasons
    }

    /// GEN-ONNX-1 Welle 2b: the single, stable refusal reason for an active
    /// generative stage on the readback-free VRAM present path. Shared by
    /// [`Self::classify_vram_refusal`] (real `render_to_vram` error) and the
    /// proactive skip in [`Self::render_draft_tick`] so the badge text and the
    /// no-spam fast path cannot drift.
    #[cfg(feature = "gpu")]
    pub(crate) const GENERATIVE_VRAM_REFUSAL_REASON: &'static str =
        "generative_edit (artifact-blind VRAM present stage; CPU renders the caller-supplied canvas)";

    /// GUI-LENSFUN-GATE-3 (F1): classify a failed `render_to_vram` into a
    /// user-facing present-refusal reason, or `None` for failures already
    /// covered by the recipe gate / other visible paths. Two post-gate refusals
    /// are classified here — both are documented `lumina-gpu` limitations the
    /// recipe-only gate cannot express:
    ///
    /// * a **dimension-changing** geometry chain (source-sized present
    ///   texture); and
    /// * GEN-ONNX-1 Welle 2a/2b an **active generative stage**: the
    ///   readback-free VRAM present path is artifact-blind and refuses it
    ///   loudly (`generative_artifact.missing`) because there is no VRAM
    ///   generative injection point without a readback.
    ///
    /// Other failures (device lost, no adapter, invalid recipe) keep their
    /// existing loud `warn!`/gate handling.
    #[cfg(feature = "gpu")]
    pub(crate) fn classify_vram_refusal(error: &lumina_gpu::GpuError) -> Option<String> {
        let message = error.to_string();
        // Mirrors `lumina-gpu`'s documented `warn_vram_dimension_change_once`
        // reason string; a stable cross-crate contract for this limitation.
        if message.contains("dimension-changing output") {
            return Some(
                "geometry (dimension-changing output; the VRAM present texture is source-sized)"
                    .into(),
            );
        }
        // GEN-ONNX-1 Welle 2a/2b: the readback-free VRAM present path is
        // artifact-blind and refuses an active generative stage loudly
        // (`generative_artifact.missing`) because there is no VRAM generative
        // injection point without a readback. The CPU preview renders it through
        // the artifact-aware hook; the badge makes the refusal visible instead
        // of swallowing it into a plain `warn!` line.
        if message.contains("generative_artifact") {
            return Some(Self::GENERATIVE_VRAM_REFUSAL_REASON.into());
        }
        // GPU-LENSFUN-PARITY-1: the `lumina-gpu` map guard refuses a bound map
        // whose dimensions do not match the frame, or a distortion corrector
        // without an explicit crop (the CPU oracle would apply its content-based
        // default crop, whose rectangle is not plannable). Both are loud core
        // guards; classify them so the CPU route is never badge-less.
        #[cfg(feature = "lensfun")]
        if let Some(reason) = lensfun_gpu::classify_refusal(error) {
            return Some(reason);
        }
        None
    }

    /// The visible badge text for a CPU routing decision: the generic
    /// [`Str::CpuFallbackUnsupportedStages`] headline plus the precise,
    /// semicolon-separated reasons (`geometry (default content crop)`,
    /// `lens_correction (Lensfun corrector; …)`, …). `None` when there is no
    /// capability reason — never a badge without a cause. Pure formatting so the
    /// precise text is testable without a bound GPU adapter.
    #[cfg(feature = "gpu")]
    pub(crate) fn format_routing_fallback_reason(reasons: &[String]) -> Option<String> {
        if reasons.is_empty() {
            return None;
        }
        Some(format!(
            "{} [{}]",
            Str::CpuFallbackUnsupportedStages.t(),
            reasons.join("; ")
        ))
    }

    /// GUI-LENSFUN-GATE-2: the visible CPU-routing fallback badge text for the
    /// frame painted last — `None` when the GPU present path was taken, no GPU
    /// context is bound, or only an editorial (non-capability) cause applies.
    /// Diagnostics only: mirrors the on-screen badge and never affects pixels.
    #[cfg(feature = "gpu")]
    #[must_use]
    pub fn gpu_routing_fallback_badge(&self) -> Option<&str> {
        self.gpu_route_fallback.as_deref()
    }
}
