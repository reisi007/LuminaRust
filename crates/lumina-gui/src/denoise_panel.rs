//! LRPAR-G14-DENOISE-IMPL-20 — Detail-section KI-Denoise panel (GUI slice).
//!
//! Extracted verbatim from `denoise_gui.rs` (GUI-INSTRDBG-17c-Rework F-A) so the
//! state/status module stays inside the file-size ratchet while the
//! "Enable AI Denoise" checkbox gains its `GuiAction` instrumentation. This
//! module owns **no** image logic: it only paints the marshalled recipe state,
//! the two shared-slider bindings, the model identity, the status badge and the
//! `Strict`/`Warn` policy switch (decision §5/§6) — the stage itself runs in the
//! shared core pipeline.

use super::*;
use crate::denoise_gui::{denoise_badge_color, denoise_model_label};
use lumina_core::DenoisePolicy;

impl LuminaApp {
    /// Detail-section panel for the KI-Denoise stage (decision §5/§6). Paints
    /// the enable switch, the two bounded sliders, the readable model identity,
    /// the status badge and the Strict/Warn policy switch.
    pub(crate) fn draw_denoise_section(&mut self, ui: &mut egui::Ui) {
        if self.denoise_gui_dirty {
            self.refresh_denoise_gui();
        }
        ui.separator();
        ui.label(Str::DenoiseAi.t());
        let mut enabled = self.denoise_ai().is_some_and(|denoise| denoise.enabled);
        if ui.checkbox(&mut enabled, Str::DenoiseEnable.t()).changed() {
            if let Err(error) = self.set_denoise_enabled(enabled) {
                self.show_error(error);
            }
        }
        let Some(denoise) = self.recipe.denoise_ai.clone() else {
            return;
        };
        let mut strength = denoise.strength;
        if matches!(
            crate::slider::lr_slider(
                ui,
                Str::DenoiseStrength.t(),
                &mut strength,
                crate::slider::identity_spec(0.0..=1.0, 0.5, 0.01)
            ),
            crate::slider::SliderAction::Changed | crate::slider::SliderAction::ResetRequested
        ) {
            if let Err(error) = self.set_denoise_strength(f64::from(strength)) {
                self.show_error(error);
            }
        }
        let mut preserve_detail = denoise.preserve_detail;
        if matches!(
            crate::slider::lr_slider(
                ui,
                Str::DenoisePreserveDetail.t(),
                &mut preserve_detail,
                crate::slider::identity_spec(0.0..=1.0, 0.5, 0.01)
            ),
            crate::slider::SliderAction::Changed | crate::slider::SliderAction::ResetRequested
        ) {
            if let Err(error) = self.set_denoise_preserve_detail(f64::from(preserve_detail)) {
                self.show_error(error);
            }
        }
        ui.label(denoise_model_label(&denoise))
            .on_hover_text(Str::DenoiseModelHint.t());
        // Status badge: every state is painted (never a silent fallback).
        let status = self.denoise_state().status;
        let color = denoise_badge_color(status);
        let reason = self.denoise_state().reason.clone();
        ui.colored_label(color, self.denoise_status_text())
            .on_hover_text(Str::DenoiseReasonPattern.format_arg(if reason.is_empty() {
                Str::DenoiseNoReason.t()
            } else {
                &reason
            }));
        let mut policy = self.denoise_policy();
        ui.horizontal(|ui| {
            ui.label(Str::DenoisePolicy.t());
            if ui
                .radio_value(&mut policy, DenoisePolicy::Warn, Str::DenoisePolicyWarn.t())
                .changed()
            {
                self.set_denoise_policy(DenoisePolicy::Warn);
            }
            if ui
                .radio_value(
                    &mut policy,
                    DenoisePolicy::Strict,
                    Str::DenoisePolicyStrict.t(),
                )
                .changed()
            {
                self.set_denoise_policy(DenoisePolicy::Strict);
            }
        });
        if self.denoise_stage_active() && !self.denoise_gui.is_ready() {
            ui.colored_label(crate::theme::ACCENT, Str::DenoiseNotReadyWarning.t());
        }
    }
}
