//! GUI-REFACTOR-W1-20 S1.3: recipe invalidation (the Dirty-Key) and the
//! single-adjustment default/reset path, extracted verbatim from `lib.rs`.
//!
//! [`LuminaApp::mark_dirty`] is the single invalidation entry point that drops
//! the final-render identity (`render_key` / `tone_analysis` / `error`) while
//! deliberately keeping the recipe-blind base-stage cache; [`mark_recipe_dirty`]
//! additionally arms the debounced slider-save commit. The invalidation
//! invariant (what is dropped vs. kept) is unchanged — this extraction only
//! moves the methods. `default_for_adjustment` / `reset_single_adjustment`
//! carry the documented flat-adjustment defaults.
//!
//! `mark_dirty` is `pub(crate)` because callers across the GUI modules and the
//! headless tests use it; `default_for_adjustment` / `reset_single_adjustment`
//! stay `pub` (public API).

use super::*;
use log::trace;

impl LuminaApp {
    /// Record a struct-backed recipe edit for the debounced slider-save commit
    /// (GUI-SLIDER-SAVE-1) and arm the re-render. Every recipe mutation routes
    /// through here (or `set_adjustment`/`set_presence`); pure view state
    /// (zoom/pan) uses bare `mark_dirty` and is therefore never saved.
    pub(crate) fn mark_recipe_dirty(&mut self, key: &str, value: f64) {
        // GUI-JANKLOG-19: hand the Dirty-Key to the active jank record
        // (behaviour-neutral observation; the invalidation invariant and the
        // `set_adjustment` duplicate below are untouched).
        #[cfg(all(feature = "janklog", debug_assertions))]
        jank_log::note_recipe_key(key);
        self.pending_slider_commit = Some((key.to_string(), value));
        self.mark_dirty();
    }

    pub(crate) fn mark_dirty(&mut self) {
        // PERF-GUI-1 stepwise invalidation: like `set_adjustment`, this drops
        // only the final-render identity and its derived panel state. The
        // base-stage cache survives — its keys cover source/decode/ROI/source-
        // action identity and are recipe-blind, so geometry/optics/mask edits
        // reuse the cached base as well (they run downstream of it in the
        // documented pipeline order). A new SOURCE clears the cache in
        // `apply_decoded_frame`; nothing here can ever serve stale pixels.
        self.render_key = None;
        self.tone_analysis = None;
        self.error = None;
        // An edit occurred: a full-quality render will be needed (debounced on
        // pointer release / idle, PERF-GUI-3/4).
        self.pending_full_render = true;
        // GUI-WGPU-PRESENT-1: the VRAM tone result no longer matches the
        // recipe — never present it until the drag path re-renders it.
        #[cfg(feature = "gpu")]
        {
            self.vram_fresh = false;
            self.vram_mask_is_evaluated = false;
            // GUI-LENSFUN-GATE-3 (F1): the recipe changed, so a present refusal
            // captured for the previous recipe is no longer known to apply.
            self.vram_render_refusal = None;
        }
    }

    /// Documented default for a flat adjustment key (identity is 0; WB Kelvin 6500).
    pub fn default_for_adjustment(key: &str) -> f64 {
        match key {
            "wb_temperature" => 6500.0,
            _ => 0.0,
        }
    }

    /// Reset exactly one flat adjustment to its documented default. Never resets
    /// the whole recipe (that is [`Self::reset`]).
    pub fn reset_single_adjustment(&mut self, key: &str) {
        trace!("GUI interaction: reset_single_adjustment {}", key);
        let default = Self::default_for_adjustment(key);
        self.recipe.adjustments.insert(key.to_owned(), default);
        // GUI-SLIDER-SAVE-1: a single-slider reset is a commit like a drag.
        self.pending_slider_commit = Some((key.to_owned(), default));
        self.mark_dirty();
        self.status = format!("Reset {key}");
    }
}
