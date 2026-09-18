//! GUI-REFACTOR-W1-20 S1.1: the interactive draft render and the coalesced
//! pointer-drag tick (Jank hot path), extracted verbatim from `lib.rs`.
//!
//! `render_draft` renders the cached downscaled draft source at viewport
//! resolution while a slider is dragged; `render_draft_tick` is the coalesced
//! per-tick hot path (VRAM tone stage first, then the CPU draft render) and
//! records the R2-GUIMOD-04a timings. [`DragTickTimings`] and the read-only
//! `last_drag_tick` accessor move together with them. No behaviour changes:
//! the renders, flags and error paths are byte-identical to the inlined
//! sequence this extraction replaces.
//!
//! `render_draft` stays `pub` (external integration tests call it);
//! `render_draft_tick` is `pub(crate)` only because the app root and the
//! headless tests drive the real hot path.

use super::*;
use log::trace;
// `warn!` is only reached from the `#[cfg(feature = "gpu")]` VRAM block.
#[cfg(feature = "gpu")]
use log::warn;

/// R2-GUIMOD-04a: per-tick timings of one coalesced pointer-drag render tick
/// (measurement only — never read for logic, feeds F-103-N6).
///
/// * `cpu_draft_ms` — wall time of the CPU draft render (`render_draft`,
///   including the analysis pass below).
/// * `gpu_ms` — wall time of the VRAM tone stage (`render_to_vram`, 0 when
///   the GPU path is off or unavailable).
/// * `analyse_ms` — wall time of the shared `analyze_tone_with_histogram`
///   pass inside that draft render.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragTickTimings {
    pub cpu_draft_ms: f64,
    pub gpu_ms: f64,
    pub analyse_ms: f64,
}

impl LuminaApp {
    /// R2-GUIMOD-04a: timings of the last instrumented drag tick, if any.
    pub fn last_drag_tick(&self) -> Option<DragTickTimings> {
        self.last_drag_tick
    }

    /// Cheap preview render used while a slider is being dragged: renders the
    /// cached downscaled draft source (`draft_original`) at viewport resolution
    /// instead of re-processing the full 45 MP original on every pointer tick.
    /// Mask planes are skipped because they are full-resolution and would not
    /// align with the downscaled source. Falls back to the full original when no
    /// draft is cached yet. `viewport`/`roi` mirror [`Self::render_full`].
    pub fn render_draft(
        &mut self,
        _viewport: [u32; 2],
        roi: Option<[u32; 4]>,
    ) -> Result<(), GuiError> {
        // GEN-ONNX-1 Welle 2b: a generative canvas is absolute geometry on the
        // full-resolution source. A downscaled draft cannot host it (the
        // auto-fill canvas is source-sized, the expand canvas canvas-sized), so
        // the draft is upgraded to a full render instead of silently skipping
        // the generative stage or compositing mismatched dimensions.
        // LRPAR-G14-DENOISE-IMPL-20: the `denoise_rgb` artifact is full-frame
        // (dimension-checked against the input frame by the core blend), so an
        // active denoise stage is upgraded to a full render the same way.
        if self.generative_stage_active() || self.denoise_stage_active() {
            trace!("GUI render: absolute-frame stage active — draft upgraded to full render");
            return self.render_full(_viewport, roi);
        }
        // Take the pre-allocated draft source so `render_from` borrows a local
        // value rather than `self` — zero allocation while dragging. Fall back to
        // a clone of the full original only when no draft is cached yet.
        let mut took_draft = true;
        let source = if let Some(d) = self.draft_original.take() {
            d
        } else {
            match &self.original {
                Some(o) => {
                    took_draft = false;
                    o.clone()
                }
                None => {
                    self.status = Str::NoImageLoaded.t().into();
                    return Ok(());
                }
            }
        };
        let roi = roi.or_else(|| {
            Self::roi_from_zoom(
                source.width,
                source.height,
                self.preview_zoom,
                self.preview_pan,
                self.preview_pane_w,
                self.preview_pane_h,
            )
        });
        self.preview_is_draft = true;
        // No generative stage is active here (checked above), so the hook gets
        // an empty artifact set — the core render then cannot hit the
        // generative stage at all.
        let result = self.render_from(&source, false, roi, GenerativeArtifacts::default());
        if took_draft {
            self.draft_original = Some(source);
        }
        result
    }

    /// One coalesced pointer-drag tick (PERF-GUI-3/4 hot path): VRAM tone
    /// stage first (readback-free present), then the CPU draft render.
    ///
    /// R2-GUIMOD-04a: the tick is instrumented — GPU wall time, CPU draft
    /// wall time and the analysis pass inside the draft are recorded in
    /// [`Self::last_drag_tick`] and logged via `trace!`. Measurement only:
    /// the renders, flags and error paths are identical to the inlined
    /// sequence this method replaces.
    pub(crate) fn render_draft_tick(&mut self, viewport: [u32; 2]) {
        let gpu_t0 = std::time::Instant::now();
        // GEN-ONNX-1 Welle 2b (F6): evaluated before the GPU borrow so the
        // proactive skip below can write the present-refusal fields.
        #[cfg(feature = "gpu")]
        let generative_active = self.generative_stage_active();
        #[cfg(feature = "gpu")]
        {
            if self.gpu.as_ref().is_some_and(|gpu| gpu.is_available()) {
                if generative_active {
                    // The readback-free VRAM present path is artifact-blind
                    // and can never render a generative recipe (documented
                    // `lumina-gpu` limitation, no VRAM injection point
                    // without a readback). Skip the doomed `render_to_vram`
                    // attempt — no per-tick `warn!` spam — and record the
                    // known refusal for the badge; the CPU artifact-aware
                    // render below is the authoritative preview.
                    self.vram_fresh = false;
                    self.vram_render_refusal =
                        Some(Self::GENERATIVE_VRAM_REFUSAL_REASON.to_owned());
                } else if let Some((width, height)) = self
                    .draft_original
                    .as_ref()
                    .or(self.original.as_ref())
                    .map(|source| (source.width, source.height))
                {
                    // GPU-LENSFUN-PARITY-1: bind the corrector's precomputed
                    // warp/gain map before the VRAM render. An active corrector
                    // whose map cannot be built/bound is kept on the exact CPU
                    // present route (loud) — the recipe-only VRAM path would
                    // otherwise apply the manual model and diverge silently.
                    if let Some(reason) = self.bind_lensfun_map(width, height) {
                        warn!("gpu present kept on CPU: {reason}");
                        self.vram_fresh = false;
                        self.vram_render_refusal = Some(reason.to_owned());
                    } else {
                        // R2-GUIMOD-03: borrow instead of clone. The fallback
                        // branch (`draft_original` absent on the first tick
                        // after a full render) used to memcpy the entire
                        // full-resolution original (~180 MB worst case) into
                        // a temporary that was dropped immediately after the
                        // call — `render_to_vram` only needs `&ImageFrame`.
                        let rendered =
                            self.draft_original
                                .as_ref()
                                .or(self.original.as_ref())
                                .map(|src| {
                                    self.gpu
                                        .as_ref()
                                        .expect("availability checked above")
                                        .render_to_vram(src, &self.recipe)
                                });
                        match rendered {
                            Some(Ok(())) => {
                                // GUI-WGPU-PRESENT-1: the VRAM output now
                                // matches the current recipe/source — the
                                // present path may use it this frame.
                                self.vram_fresh = true;
                                // GUI-LENSFUN-GATE-3 (F1): a successful VRAM
                                // render clears any earlier present refusal.
                                self.vram_render_refusal = None;
                            }
                            Some(Err(err)) => {
                                warn!("gpu render_to_vram failed: {err}");
                                self.vram_fresh = false;
                                // GUI-LENSFUN-GATE-3 (F1): classify the present
                                // refusal so it can surface as a badge even when
                                // the recipe gate is empty (dimension-changing
                                // geometry is refused *after* the gate).
                                self.vram_render_refusal = Self::classify_vram_refusal(&err);
                            }
                            None => {}
                        }
                    }
                }
            }
        }
        let gpu_ms = gpu_t0.elapsed().as_secs_f64() * 1000.0;
        let cpu_t0 = std::time::Instant::now();
        let result = self.render_draft(viewport, None);
        let cpu_draft_ms = cpu_t0.elapsed().as_secs_f64() * 1000.0;
        if let Err(e) = result {
            self.show_error(e);
        }
        let analyse_ms = self.last_analysis_ms;
        self.last_drag_tick = Some(DragTickTimings {
            cpu_draft_ms,
            gpu_ms,
            analyse_ms,
        });
        trace!(
            "GUI drag tick: cpu_draft_ms={cpu_draft_ms:.2} gpu_ms={gpu_ms:.2} analyse_ms={analyse_ms:.2}"
        );
    }
}
