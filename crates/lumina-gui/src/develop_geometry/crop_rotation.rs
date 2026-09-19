//! UX-LOOK-CROP-18b: straighten rotation + auto-level inside the crop tool.
//!
//! The crop tool owns a **session-only rotation draft** alongside the crop
//! rectangle draft (`crop_overlay.rs`). The angle is edited on a crop bar under
//! the preview (the chosen gesture variant: a labeled slider, not an on-canvas
//! drag — documented here and in the feature report). Nothing reaches the recipe
//! until `Enter`:
//!
//! * `Enter` commits the crop and, when the draft differs from the committed
//!   angle, the rotation through the existing [`LuminaApp::set_straighten`]
//!   path (`geometry.rotation_degrees`, no new recipe field). Because all
//!   setters run before the single debounced save, crop + rotation share **one**
//!   geometry history step.
//! * `Esc` (and leaving crop mode) discards everything bit-for-bit.
//!
//! **Auto-Level** runs the deterministic, model-free
//! `lumina_core::upright::analyze_upright` backend (not changed here, only
//! used). At sufficient confidence it derives the straighten angle from the
//! suggestion and stores the analysis — fingerprinted for the loaded source —
//! as a session stash that is persisted (as a *disabled* `upright` stage, so it
//! is evidence only and never double-applies the perspective) together with the
//! rotation on `Enter`. At low confidence it refuses loudly: a visible status
//! plus `warn!`, no draft, no recipe write, no save.
//!
//! The rotation coefficient/degree conversion: the upright module returns the
//! correction in the F-099 domain (`-1..=1` over ±45°, `coefficient =
//! -deviation/45°`), and both `rotate_frame` and the F-099 homography rotate the
//! image by `+coefficient·45°`. `upright_rotation_to_degrees` therefore maps
//! `coefficient → coefficient · 45°`.

use super::*;
use log::{info, warn};

/// Minimum upright confidence for Auto-Level to apply a rotation silently.
/// Below this the tool reports `none` and warns instead (no save). Chosen so a
/// real line signal (grid/structure) passes and flat/noise frames do not.
pub(crate) const AUTO_LEVEL_MIN_CONFIDENCE: f32 = 0.10;

/// Session stash holding the fingerprinted upright analysis produced by
/// Auto-Level, until `Enter` persists it (or `Esc`/leaving the tool drops it).
#[derive(Debug, Clone)]
pub(crate) struct AutoLevelStash {
    pub(crate) analysis: lumina_sidecar::UprightAnalysis,
}

fn rotation_draft_id() -> egui::Id {
    egui::Id::new("lumina.crop_overlay.rotation_draft")
}

fn auto_level_id() -> egui::Id {
    egui::Id::new("lumina.crop_overlay.auto_level")
}

/// The session rotation draft in degrees, if the crop bar changed it.
pub(crate) fn crop_rotation_draft(ctx: &egui::Context) -> Option<f32> {
    ctx.data(|data| data.get_temp(rotation_draft_id()))
}

/// Store (`Some`) or clear (`None`) the session rotation draft. Returns whether
/// a value was present before a clear (the cancel primitive).
pub(crate) fn set_crop_rotation_draft(ctx: &egui::Context, value: Option<f32>) -> bool {
    ctx.data_mut(|data| match value {
        Some(degrees) => {
            data.insert_temp(rotation_draft_id(), degrees);
            true
        }
        None => {
            let had = data.get_temp::<f32>(rotation_draft_id()).is_some();
            data.remove::<f32>(rotation_draft_id());
            had
        }
    })
}

/// The fingerprinted Auto-Level analysis stashed by the crop bar, if any.
pub(crate) fn crop_auto_level_stash(ctx: &egui::Context) -> Option<AutoLevelStash> {
    ctx.data(|data| data.get_temp(auto_level_id()))
}

/// Store (`Some`) or clear (`None`) the Auto-Level analysis stash. Returns
/// whether a value was present before a clear.
pub(crate) fn set_crop_auto_level_stash(
    ctx: &egui::Context,
    value: Option<AutoLevelStash>,
) -> bool {
    ctx.data_mut(|data| match value {
        Some(stash) => {
            data.insert_temp(auto_level_id(), stash);
            true
        }
        None => {
            let had = data.get_temp::<AutoLevelStash>(auto_level_id()).is_some();
            data.remove::<AutoLevelStash>(auto_level_id());
            had
        }
    })
}

/// Upright rotation coefficient (`-1..=1` over ±45°) → straighten degrees for
/// `geometry.rotation_degrees` (see the module docs for the sign reasoning).
pub(crate) fn upright_rotation_to_degrees(rotation: f32) -> f32 {
    rotation * 45.0
}

/// Screen rect of the crop bar: a strip at the bottom of the preview pane.
/// Kept out of the full-frame handle area where possible; the crop gesture
/// additionally ignores presses that start inside it.
pub(crate) fn crop_bar_rect(pane: egui::Rect) -> egui::Rect {
    let height = 36.0_f32.min(pane.height());
    egui::Rect::from_min_max(egui::pos2(pane.left(), pane.bottom() - height), pane.max)
}

impl LuminaApp {
    /// Status text for the current crop-mode flag (`CropModeOn`/`CropModeOff`).
    /// Extracted here so `lib.rs::toggle_crop_mode` (file-size ratchet) stays a
    /// thin wrapper while the armed tool invalidates the crop-mode preview.
    pub(crate) fn set_crop_mode_status(&mut self) {
        self.status = if self.crop_mode {
            Str::CropModeOn.t().into()
        } else {
            Str::CropModeOff.t().into()
        };
    }

    /// Committed straighten angle (`geometry.rotation_degrees`), 0 when absent.
    pub(crate) fn committed_rotation_degrees(&self) -> f64 {
        self.recipe
            .geometry
            .as_ref()
            .map(|geometry| f64::from(geometry.rotation_degrees))
            .unwrap_or(0.0)
    }

    /// Crop-bar Auto-Level button: run the deterministic upright analysis and
    /// set the session rotation draft. Loud low-confidence refusal (status +
    /// `warn!`, no save); at sufficient confidence the analysis is stashed with
    /// its source fingerprint and only persisted on `Enter`.
    pub(crate) fn apply_auto_level(&mut self, ctx: &egui::Context) {
        instrument_gui_action!(self, GuiAction::AnalyzeUpright);
        let Some(frame) = self.original.clone() else {
            self.status = Str::NoImageLoaded.t().into();
            return;
        };
        let suggestion = analyze_upright(&frame);
        if suggestion.line_count == 0 || suggestion.confidence < AUTO_LEVEL_MIN_CONFIDENCE {
            set_crop_auto_level_stash(ctx, None);
            set_crop_rotation_draft(ctx, None);
            warn!(
                "auto level: low confidence (lines={} confidence={:.3}) — \
                 no rotation applied, nothing saved",
                suggestion.line_count, suggestion.confidence
            );
            self.status = Str::UprightStatusPattern.format_arg(Str::UprightNone.t());
            return;
        }
        let fingerprint = upright_input_fingerprint(
            &self.resolved_source_hash(),
            frame.width,
            frame.height,
            self.raw_orientation,
        );
        let analysis = upright_analysis(suggestion, fingerprint);
        let degrees = upright_rotation_to_degrees(suggestion.rotation);
        set_crop_rotation_draft(ctx, Some(degrees));
        set_crop_auto_level_stash(ctx, Some(AutoLevelStash { analysis }));
        self.status = Str::UprightStatusPattern.format_arg(Str::UprightFresh.t());
        info!(
            "GUI interaction: auto_level -> lines={} confidence={:.3} rotation={degrees:.3}deg",
            suggestion.line_count, suggestion.confidence
        );
    }

    /// Persist the stashed Auto-Level analysis on `Enter` as a **disabled**
    /// `upright` stage (fingerprint evidence, exactly like the existing
    /// `Analyze` button persists it, but inert so it never double-applies next
    /// to the committed `geometry.rotation_degrees`). An already present upright
    /// stage is left untouched and reported loudly — Auto-Level never silently
    /// overwrites a persisted analysis. No-op without a stash.
    pub(crate) fn persist_auto_level_analysis(&mut self, stash: &AutoLevelStash) {
        if self.recipe.upright.is_some() {
            warn!(
                "auto level: an upright stage is already persisted — analysis not \
                 overwritten; the rotation is committed via geometry only"
            );
            return;
        }
        let confidence = stash.analysis.confidence;
        self.recipe.upright = Some(Upright {
            version: 1,
            enabled: false,
            analysis: Some(stash.analysis.clone()),
        });
        self.mark_recipe_dirty("upright.auto_level", f64::from(confidence));
        self.pending_history_step = Some("upright.auto_level".into());
        info!(
            "GUI interaction: auto_level analysis persisted (fingerprint, disabled) \
             confidence={confidence:.3}"
        );
    }

    /// Paint the crop bar (straighten slider + Auto-Level button) and react to
    /// its controls. All edits stay in the session drafts; the recipe is only
    /// written by `Enter`.
    pub(crate) fn draw_crop_bar(
        &mut self,
        ui: &mut egui::Ui,
        bar: egui::Rect,
        ctx: &egui::Context,
    ) {
        if bar.width() < 8.0 || bar.height() < 8.0 {
            return;
        }
        ui.painter()
            .rect_filled(bar, 0.0, egui::Color32::from_black_alpha(190));
        let inner = bar.shrink2(egui::vec2(8.0, 4.0));
        // Reserve a fixed strip for the Auto-Level button on the right so it is
        // never clipped by a narrow preview pane (the slider takes the rest).
        let button_w = 56.0_f32.min(inner.width() * 0.4);
        let slider_rect = egui::Rect::from_min_max(
            inner.min,
            egui::pos2((inner.max.x - button_w - 6.0).max(inner.min.x), inner.max.y),
        );
        let button_rect = egui::Rect::from_min_max(
            egui::pos2((inner.max.x - button_w).max(inner.min.x), inner.min.y),
            inner.max,
        );
        let mut slider_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(slider_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let committed = self.committed_rotation_degrees() as f32;
        let mut degrees = crop_rotation_draft(ctx).unwrap_or(committed);
        if matches!(
            lr_slider(
                &mut slider_ui,
                Str::Straighten.t(),
                &mut degrees,
                identity_spec(-180.0..=180.0, 0.0, 1.0)
            ),
            SliderAction::Changed | SliderAction::ResetRequested
        ) {
            set_crop_rotation_draft(ctx, Some(degrees));
        }
        let mut button_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(button_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        if button_ui.button(Str::Auto.t()).clicked() {
            self.apply_auto_level(ctx);
        }
    }
}
