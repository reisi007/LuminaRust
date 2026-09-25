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
use log::{error, trace, warn};
/// Idle-debounce window (seconds) before the pending full-quality render is
/// committed after the last edit (PERF-GUI-3/4). Moved here from the crate root
/// with [`full_render_debounce_remaining`] (cohesive with the tick hot path).
const FULL_RENDER_DEBOUNCE_SECONDS: f64 = 0.150;
/// REVIEW-GUI-DEBOUNCE-1: pure decision helper for the debounced full render.
///
/// Returns `Some(remaining_seconds)` while the wait window is still open (the
/// caller must schedule a repaint for exactly that long), and `None` when the
/// debounce has elapsed or no drag time was recorded (`last_edit_time == 0.0`
/// → immediate render). Kept pure so the stranding fix is unit-testable
/// without an event loop.
pub(crate) fn full_render_debounce_remaining(last_edit_time: f64, now: f64) -> Option<f64> {
    if last_edit_time <= 0.0 {
        return None;
    }
    let remaining = FULL_RENDER_DEBOUNCE_SECONDS - (now - last_edit_time);
    (remaining > 0.0).then_some(remaining)
}

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
        // GUI-SRCACC-1: repair regions likewise require exact source geometry.
        if self.generative_stage_active()
            || self.denoise_stage_active()
            || !self.recipe.source_actions.is_empty()
            || self.sidecar_resolution_pending()
            || self.has_visible_local_adjustments()
        {
            trace!(
                "GUI render: absolute-frame or local-mask stage active — draft upgraded to full render"
            );
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
        // R3-RENDER-SIZE-1: the draft source IS the viewport cap (it is built at
        // `draft_max_dim`, maintained by `refresh_preview_cap`). When no draft
        // is cached yet (first tick on an uncapped small source, or a missing
        // viewport) the full source is used — also capped by `render_full` in
        // the absolute-frame branch, and by `refresh_preview_cap` otherwise.
        self.preview_is_draft = true;
        // No generative stage is active here (checked above), so the hook gets
        // an empty artifact set — the core render then cannot hit the
        // generative stage at all.
        let result = self.render_from(
            &source,
            false,
            roi,
            None,
            GenerativeArtifacts::default(),
            ResolvedSourceActions::default(),
        );
        if took_draft {
            self.draft_original = Some(source);
        }
        result
    }

    /// R2-JANK-1 F1: frame-budget-gated entry point for the coalesced drag tick.
    /// `now` is the egui frame time (seconds), injected so the throttle is
    /// headless-drivable (DoD §2).
    ///
    /// At most one draft render per `DRAFT_RENDER_BUDGET_SECONDS`: inside the
    /// budget the tick returns without touching the draft, and the caller's
    /// scheduled repaint retries once the budget elapses. No silent state — the
    /// pending edit keeps the render key invalid, so the existing "Stale" badge
    /// stays visible until the next draft render lands. The tick also arms the
    /// F4 analysis cadence (`DraftThrottle::prepare_analysis`); a skipped
    /// analysis pass is marked visibly via `DRAFT_ANALYSIS_PENDING_LABEL`.
    ///
    /// F2 (moving the draft render off the UI thread) is deliberately **not**
    /// implemented here: `render_draft`/`render_from` mutate the whole app
    /// state in place (stage caches, GPU bindings, analysis slots) and borrow
    /// the draft source from `self`, so a worker split requires a shared
    /// pipeline context plus a result-application state machine — an
    /// architecture change, not a small fix. The frame-budget throttle removes
    /// the repeated same-budget renders without it (R2-JANK-1 F2 finding).
    pub(crate) fn render_draft_tick_at(&mut self, viewport: [u32; 2], now: f64) {
        self.draft_throttle.observe(now);
        if !self.draft_throttle.render_due() {
            trace!("GUI render: draft tick throttled (frame budget)");
            #[cfg(test)]
            DRAFT_TICK_THROTTLES.with(|count| count.set(count.get() + 1));
            // The edit dropped the live analysis; restore the retained one
            // (visible pending marker) so the panel never blanks mid-drag.
            if let Some((analysis, histogram)) = self.draft_throttle.retained() {
                self.tone_analysis = Some(analysis);
                self.preview_histogram = Some(histogram);
                self.draft_throttle.note_analysis_pending();
            }
            return;
        }
        self.draft_throttle.note_render();
        self.draft_throttle.prepare_analysis();
        self.render_draft_tick(viewport);
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
        // GUI-JANKLOG-19: a render tick outside an action scope is its own
        // outermost jank scope (`kind=render`); inside an action it nests and
        // fills that action's record instead of emitting a second line.
        #[cfg(all(feature = "janklog", debug_assertions))]
        let _jank = {
            let (route, badge) = self.jank_route_and_badge();
            let key = self
                .pending_slider_commit
                .as_ref()
                .map(|(key, _)| key.as_str());
            jank_log::JankScope::enter(jank_log::JankKind::Render, None, route, badge, key)
        };
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
                    self.note_vram_refusal(Self::GENERATIVE_VRAM_REFUSAL_REASON);
                } else if self.recipe_has_unsupported_gpu_stages() {
                    // R3-ROUTING-1 (B1): the gate now evaluates the recipe the
                    // VRAM present path actually renders (`gpu_present_recipe`).
                    // A gate-unsupported present recipe (e.g. the crop tool's
                    // geometry-free display recipe with a lens/perspective
                    // correction and no explicit crop → `default content crop`)
                    // would make `render_to_vram` refuse with the generic
                    // unsupported-stage text, which the refusal classifier
                    // cannot name — the GUI then warned per tick with no badge.
                    // Record the precise gate reason and skip the doomed attempt:
                    // 1× warn + `trace!`/tick, badge set, no wasted GPU time.
                    let reason = self.gpu_unsupported_stage_reasons().join("; ");
                    self.note_vram_refusal(&reason);
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
                        self.note_vram_refusal(reason);
                    } else {
                        // R2-GUIMOD-03: borrow instead of clone. The fallback
                        // branch (`draft_original` absent on the first tick
                        // after a full render) used to memcpy the entire
                        // full-resolution original (~180 MB worst case) into
                        // a temporary that was dropped immediately after the
                        // call — `render_to_vram` only needs `&ImageFrame`.
                        let rendered = {
                            // R3-ROUTING-1: while the crop tool is armed the
                            // preview is the geometry-free full frame; render
                            // exactly that recipe on the GPU instead of letting
                            // the committed crop's dimension change refuse every
                            // tick.
                            let recipe = self.gpu_present_recipe();
                            self.draft_original
                                .as_ref()
                                .or(self.original.as_ref())
                                .map(|src| {
                                    self.gpu
                                        .as_ref()
                                        .expect("availability checked above")
                                        .render_to_vram(src, recipe.as_ref())
                                })
                        };
                        match rendered {
                            Some(Ok(())) => {
                                // GUI-WGPU-PRESENT-1: the VRAM output now
                                // matches the current recipe/source — the
                                // present path may use it this frame.
                                self.vram_fresh = true;
                                // GUI-LENSFUN-GATE-3 (F1): a successful VRAM
                                // render clears any earlier present refusal.
                                self.vram_render_refusal = None;
                                // R4-WARN-1: a reason that reappears after the
                                // GPU path worked again is a new occurrence.
                                self.clear_present_refusal_warn();
                            }
                            Some(Err(err)) => {
                                // R3-ROUTING-1: a classified post-gate refusal
                                // (dimension-changing geometry, generative,
                                // Lensfun map) warns once per state change and
                                // traces per tick; an unclassified failure stays
                                // loud every tick.
                                match Self::classify_vram_refusal(&err) {
                                    Some(reason) => {
                                        self.note_vram_refusal(&reason);
                                    }
                                    None => {
                                        warn!("gpu render_to_vram failed: {err}");
                                        self.vram_fresh = false;
                                        self.vram_render_refusal = None;
                                    }
                                }
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
        // Crash-Fix Runde 2 (F5): a persistent draft failure must not re-emit
        // `error!` and re-arm the dialog on every drag tick; a success clears
        // the dedup memo so a later recurrence stays loud.
        match result {
            Ok(()) => self.clear_draft_error_dedup(),
            Err(e) => self.show_draft_error(e),
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
        // GUI-JANKLOG-19: link the partial durations with the action/scope in
        // the same record (no separate trace-only attribution).
        #[cfg(all(feature = "janklog", debug_assertions))]
        jank_log::note_render_timings(gpu_ms, cpu_draft_ms, analyse_ms);
    }

    /// Crash-Fix Runde 2 (F5): surface a draft-render failure without spamming
    /// the log or re-arming the dialog once per frame. The first occurrence of a
    /// message is loud (`error!` + "Error" status + dialog); a repeated
    /// identical message is downgraded to a single one-time `warn!` and leaves
    /// status, message and dialog state untouched — so a dialog the user closed
    /// stays closed and a persistent failure does not flood the log. Nothing is
    /// lost silently: the first message and the status stay visible.
    pub(crate) fn show_draft_error(&mut self, error: impl ToString) {
        let message = error.to_string();
        if self.draft_error_dedup.as_deref() == Some(message.as_str()) {
            if !self.draft_error_repeat_warned {
                warn!("draft render failed again (same message, not repeated): {message}");
                self.draft_error_repeat_warned = true;
                #[cfg(test)]
                DRAFT_ERROR_REPEAT_WARNS.with(|warns| warns.set(warns.get() + 1));
            }
            return;
        }
        error!("{message}");
        #[cfg(test)]
        DRAFT_ERRORS.with(|log| log.borrow_mut().push(message.clone()));
        self.draft_error_dedup = Some(message.clone());
        self.draft_error_repeat_warned = false;
        self.status = Str::Error.t().into();
        self.error = Some(message);
        self.error_dialog = true;
    }

    /// Crash-Fix Runde 2 (F5): a successful draft tick forgets the last
    /// failure, so the same failure recurring after a recovery is surfaced
    /// loudly again (no permanent suppression).
    pub(crate) fn clear_draft_error_dedup(&mut self) {
        self.draft_error_dedup = None;
        self.draft_error_repeat_warned = false;
    }
}

// Crash-Fix Runde 2 (F5): thread-local capture of the `error!` lines emitted by
// [`LuminaApp::show_draft_error`], so headless tests can prove the per-frame
// dedup (same capture seam pattern as `jank_log`).
#[cfg(test)]
thread_local! {
    static DRAFT_ERRORS: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static DRAFT_ERROR_REPEAT_WARNS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Drains and returns the captured draft-error lines of the current test thread.
#[cfg(test)]
pub(crate) fn take_draft_error_log() -> Vec<String> {
    DRAFT_ERRORS.with(|log| std::mem::take(&mut *log.borrow_mut()))
}

/// Drains and returns the number of one-time repeat `warn!` emissions of the
/// current test thread.
#[cfg(test)]
pub(crate) fn take_draft_error_repeat_warns() -> u32 {
    DRAFT_ERROR_REPEAT_WARNS.with(|warns| warns.replace(0))
}

// R3-LOG-1 test seam: count F1 frame-budget throttles so a headless test can
// prove the throttle path fired (the production line stays `trace!`).
#[cfg(test)]
thread_local! {
    static DRAFT_TICK_THROTTLES: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// R3-LOG-1: drains the F1 draft-tick throttle counter.
#[cfg(test)]
pub(crate) fn take_draft_tick_throttles() -> u32 {
    DRAFT_TICK_THROTTLES.with(|count| count.replace(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap()
    }

    fn app() -> LuminaApp {
        let mut app = LuminaApp::new(egui::Context::default());
        app.load_bytes(png(), "draft-error-test.png").unwrap();
        app
    }

    /// F5: three identical draft errors produce exactly one `error!` line.
    #[test]
    fn repeated_identical_draft_errors_emit_one_error_line() {
        let mut app = app();
        app.show_draft_error("draft failed");
        app.show_draft_error("draft failed");
        app.show_draft_error("draft failed");
        assert_eq!(take_draft_error_log(), vec!["draft failed"]);
        assert_eq!(
            take_draft_error_repeat_warns(),
            1,
            "a persistent failure must warn about the repeat exactly once, not per tick"
        );
        assert_eq!(app.error(), Some("draft failed"));
        assert!(app.error_dialog_open());
    }

    /// F5: a repeat does not re-arm a dialog the user closed; a new message is
    /// loud again.
    #[test]
    fn repeated_draft_error_does_not_rearm_a_closed_dialog() {
        let mut app = app();
        app.show_draft_error("draft failed");
        assert!(app.error_dialog_open());
        let _ = take_draft_error_log();
        app.close_error_dialog();
        app.show_draft_error("draft failed");
        assert!(
            !app.error_dialog_open(),
            "a repeated message must not re-open the dialog"
        );
        assert!(
            take_draft_error_log().is_empty(),
            "a repeated message must not emit a second error! line"
        );
        app.show_draft_error("other failure");
        assert!(app.error_dialog_open(), "a new message must be surfaced");
        assert_eq!(take_draft_error_log(), vec!["other failure"]);
    }

    /// F5 end-to-end: driving the real draft tick with a recipe that every
    /// render rejects (positive shadows on the (0,0) endpoint) logs once.
    #[test]
    fn failing_draft_ticks_dedup_the_error() {
        let mut app = app();
        app.render().unwrap();
        app.recipe.curves = Some(Curves {
            version: 1,
            master: vec![
                CurvePoint {
                    input: 0.0,
                    output: 0.5,
                },
                CurvePoint {
                    input: 1.0,
                    output: 1.0,
                },
            ],
            channels: CurveChannels::default(),
        });
        for _ in 0..3 {
            app.render_draft_tick([64, 48]);
        }
        assert_eq!(
            take_draft_error_log().len(),
            1,
            "three failing draft ticks must log the error exactly once"
        );
        assert!(app.error().is_some(), "the failure stays loud");
    }

    /// F5: clearing the memo after a recovery re-arms the loud path.
    #[test]
    fn clear_draft_error_dedup_rearms_the_loud_path() {
        let mut app = app();
        app.show_draft_error("boom");
        let _ = take_draft_error_log();
        app.clear_draft_error_dedup();
        app.show_draft_error("boom");
        assert_eq!(take_draft_error_log(), vec!["boom"]);
    }
}
