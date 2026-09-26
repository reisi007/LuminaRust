//! R2-MODSWITCH-1 F7: the per-frame render scheduler (extracted from the
//! `update` loop, file-size ratchet) plus the module-switch deferral.
//!
//! The scheduler runs the interactive draft tick while a pointer drag is in
//! progress and the debounced full-quality commit once the drag settles — the
//! pre-existing F1/F4 throttle and 150 ms debounce paths, unchanged.
//!
//! F7 adds one rule: on the first scheduler run after the active module
//! changed, a full render that would otherwise fire *in the switch frame* is
//! deferred by one frame. The module paints first (preview/"Stale"), and the
//! render lands on the next repaint. `pending_full_render` stays set and
//! `render_key` stays invalid in between, so the visible "Stale" badge keeps
//! advertising the lag — no silent state.
//!
//! `last_scheduled_module` is compared instead of hooking `set_module` so the
//! deferral also covers the module-bar click (which runs *after* the scheduler
//! in the same frame) and every direct `active_module` assignment.

use super::*;
use log::trace;

impl LuminaApp {
    /// GUI-SLIDER-SAVE-1: whether a debounced commit is armed — the same pair
    /// [`Self::commit_pending_slider_save`] and [`Self::flush_pending_edit`]
    /// treat as "there is something to save": a slider/presence value token, or
    /// a mask/history transaction. Pure view state (zoom/pan) arms neither, so
    /// a view edit never reaches the commit and never writes a sidecar.
    fn debounced_commit_armed(&self) -> bool {
        self.pending_slider_commit.is_some() || self.pending_history_step.is_some()
    }

    /// Run this frame's render scheduling (draft tick / debounced full render).
    pub(crate) fn schedule_render(&mut self, ctx: &egui::Context) {
        // PERF-GUI-3/4: draft render while a pointer drag is in progress
        // (coalesced: latest params overwrite, intermediate frames are dropped,
        // a repaint is requested); a debounced full-quality render fires on
        // mouse-up / idle (150 ms) so the final frame is computed once.
        // GUI-60FPS-1: the slider/mask hot path prefers the VRAM-resident GPU
        // tone stage (`render_to_vram`, no `map_async` CPU readback). The CPU
        // fallback remains fully functional when no adapter is bound or the
        // `gpu` feature is off.
        let pointer_down = ctx.input(|i| i.pointer.any_down());
        let now = ctx.input(|i| i.time);
        // R2-MODSWITCH-1 F7: first scheduler run after a module change.
        let module_switched = self.last_scheduled_module != Some(self.active_module);
        self.last_scheduled_module = Some(self.active_module);
        // SIDECAR-SAVE-STRAND-39: a full render does **not** discharge an owed
        // save. A render that does not go through `commit_pending_slider_save` —
        // the Develop footer's `Render / Apply` (`render_action` →
        // `LuminaApp::render`) and the other explicit `render()` callers —
        // clears `pending_full_render` in its own frame, and that flag is the
        // gate of the only commit path below. An armed token would then be
        // unreachable for the rest of the session: the edited value stayed in
        // memory, the sidecar kept the old one, and nothing said so. Re-arm the
        // flag so the unchanged debounce path below still saves the edit.
        //
        // A running drag clock (`last_edit_time > 0.0`) is what makes this
        // exactly one-shot rather than a retry loop: the clock is `0.0` for an
        // edit *without* a drag (that one commits immediately, so there is no
        // window to cancel) and after every commit attempt, successful or not.
        // So a successful save is never repeated, and a failed one keeps
        // today's behaviour — loud, and retried by the next user action.
        if !pointer_down
            && !self.pending_full_render
            && self.last_edit_time > 0.0
            && self.debounced_commit_armed()
        {
            trace!("GUI render: debounced save re-armed after a non-committing render");
            self.pending_full_render = true;
        }
        if pointer_down
            && self.pending_full_render
            && self.original.is_some()
            && self.render_key.is_none()
        {
            trace!("GUI render: draft render during pointer drag");
            let screen = ctx.input(|i| i.viewport_rect());
            let viewport = [screen.width() as u32, screen.height() as u32];
            // R2-JANK-1 F1: frame-budget-gated (at most one CPU draft per
            // 16 ms); a throttled tick leaves `render_key` invalid so the
            // "Stale" badge keeps the lag visible, and the unconditional
            // repaint below retries once the budget elapses.
            self.render_draft_tick_at(viewport, now);
            self.last_edit_time = now;
            ctx.request_repaint();
        } else if !pointer_down && self.pending_full_render {
            // 150 ms debounce after the last edit before committing the full
            // render. `last_edit_time == 0` (no drag recorded) routes to an
            // immediate full render so non-drag edits are never stranded.
            match full_render_debounce_remaining(self.last_edit_time, now) {
                Some(remaining_seconds) => {
                    // REVIEW-GUI-DEBOUNCE-1: while still inside the wait window
                    // neither a render happens nor did anything schedule a
                    // repaint — egui would sleep indefinitely and the draft
                    // preview stayed until the next unrelated input. The waiting
                    // branch requests a timed repaint exactly when the debounce
                    // elapses.
                    trace!(
                        "GUI render: debounce wait, repaint in {:.1} ms",
                        remaining_seconds * 1000.0
                    );
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                        remaining_seconds,
                    ));
                }
                None => {
                    if module_switched {
                        // R2-MODSWITCH-1 F7: do not block the frame that paints
                        // the newly selected module with a full render. The
                        // pending edit stays armed and visible as "Stale"; the
                        // un-deferred next frame commits it.
                        trace!("GUI render: full render deferred past the module-switch frame");
                        #[cfg(test)]
                        MODULE_SWITCH_DEFERRALS.with(|count| count.set(count.get() + 1));
                        ctx.request_repaint();
                    } else {
                        trace!("GUI render: debounced full render after interaction");
                        let screen = ctx.input(|i| i.viewport_rect());
                        let viewport = [screen.width() as u32, screen.height() as u32];
                        // GUI-SLIDER-SAVE-1: the settled render commits pending
                        // slider edits to the sidecar (CAS, loud conflicts) with
                        // an INFO log; pure view edits (zoom/pan) only re-render.
                        self.commit_pending_slider_save(viewport);
                        self.last_edit_time = 0.0;
                    }
                }
            }
        }
    }
}

// R3-LOG-1 test seam: count the F7 deferral firings so a headless test can
// prove the scheduler path fired (the production line stays `trace!`).
#[cfg(test)]
thread_local! {
    static MODULE_SWITCH_DEFERRALS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// R3-LOG-1: drains the F7 module-switch deferral counter.
#[cfg(test)]
pub(crate) fn take_module_switch_deferrals() -> u32 {
    MODULE_SWITCH_DEFERRALS.with(|count| count.replace(0))
}
