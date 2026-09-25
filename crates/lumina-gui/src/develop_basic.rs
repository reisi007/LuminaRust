//! GUI-REFACTOR-W2-20 S2.2: the Basic Develop section plus the two helpers
//! shared by every section, extracted verbatim from `lib.rs`.
//!
//! [`LuminaApp::adjustment_slider`] is the flat-recipe slider row (reset/scroll/
//! scale + the G-16 masking-preview mod-keys), [`LuminaApp::draw_section_prev_reset`]
//! is the per-panel Previous/Reset row (LRPAR-G01-BASIC) and
//! [`LuminaApp::draw_basic`] paints WB/tone/presence. No behaviour changes: the
//! G-16 modifier rules, the auto-endpoint routing and the B&W stash bridge are
//! byte-identical. All three are `pub(crate)` because the other section modules
//! and `DEVELOP_SECTIONS` call them.

use super::*;

impl LuminaApp {
    /// One horizontal Lightroom-style adjustment row bound to a flat recipe key.
    pub(crate) fn adjustment_slider(
        &mut self,
        ui: &mut egui::Ui,
        key: &str,
        label: &str,
        spec: SliderSpec,
    ) {
        let mut v = self
            .recipe
            .adjustments
            .get(key)
            .copied()
            .unwrap_or(Self::default_for_adjustment(key));
        match lr_slider(ui, label, &mut v, spec) {
            SliderAction::Changed => {
                self.set_adjustment(key, v);
                // G-16: an `Alt`-held value edit on a Basic tone slider arms
                // the masking preview (additive: label `Alt`-click stays the
                // single-control reset, `Alt`-scroll stays the fine step).
                if ui.ctx().input(|i| i.modifiers.alt) {
                    if masking_preview_for_slider(key) {
                        if let Err(error) = self.set_masking_preview(Some(key)) {
                            self.show_error(error);
                        }
                    }
                } else if self.masking_preview_key().is_some() {
                    let _ = self.set_masking_preview(None);
                }
            }
            SliderAction::ResetRequested => {
                // G-16: `Shift`+double-click on the whites/blacks label
                // applies the auto end point through the shared auto-tone
                // path; every other label (and any reset without `Shift`)
                // keeps the normal single-control reset. An `Alt`-held label
                // click always resets (the Welle-2 `Alt`-reset keeps priority
                // over the `Shift` end point, so the two modifiers never
                // compete for the same click).
                let (shift, alt) = ui.ctx().input(|i| (i.modifiers.shift, i.modifiers.alt));
                match auto_endpoint_for_slider(key, shift && !alt, true) {
                    Some(endpoint) => {
                        if let Err(error) = self.apply_auto_endpoint(endpoint) {
                            self.show_error(error);
                        }
                    }
                    None => self.reset_single_adjustment(key),
                }
                if self.masking_preview_key().is_some() {
                    let _ = self.set_masking_preview(None);
                }
            }
            SliderAction::Nothing => {
                // `Alt` released without a value change: disarm silently.
                if !ui.ctx().input(|i| i.modifiers.alt) && self.masking_preview_key().is_some() {
                    let _ = self.set_masking_preview(None);
                }
            }
        }
    }

    // ---- Develop panel sections (fixed F-100 order) ----

    /// LRPAR-G01-BASIC: per-panel Previous/Reset row (panel-local undo to
    /// the last saved state / reset to documented defaults). Shared by all
    /// eight Develop sections so the behaviour is identical per panel;
    /// failures stay visible via `show_error`, never silent.
    ///
    /// UX-LOOK-LAYOUT-18: the pair is anchored at the right edge of the
    /// section body (Lightroom Classic), so the adjustment controls below keep
    /// the left edge. `right_to_left` places Reset at the far right and
    /// Previous to its left — the visual order stays `Previous | Reset`.
    pub(crate) fn draw_section_prev_reset(&mut self, ui: &mut egui::Ui, section: usize) {
        // `allocate_ui_with_layout` (not `with_layout`) bounds the row's
        // `max_rect` to one control row: a bare `with_layout` child inherits the
        // full remaining panel height and pushes the entire section body below
        // the fold.
        let row_size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(
            row_size,
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                if ui.button(Str::Reset.t()).clicked() {
                    if let Err(error) = self.reset_section(section) {
                        self.show_error(error);
                    }
                }
                if ui.button(Str::Previous.t()).clicked() {
                    if let Err(error) = self.restore_section_previous(section) {
                        self.show_error(error);
                    }
                }
            },
        );
    }

    pub(crate) fn draw_basic(&mut self, ui: &mut egui::Ui) {
        // G-11 solo: the explicit `section_open` state drives the header (not
        // egui-implicit memory), so solo mode stays headless-testable.
        let section_was_open = self.section_open[SECTION_BASIC];
        let section_header =
            egui::CollapsingHeader::new(Str::Basic.t()).open(Some(section_was_open));
        let section_response = section_header.show(ui, |ui| {
            // LRPAR-G01-BASIC: Treatment + Profile headline (Lightroom Basic
            // order) — same `apply_treatment`/whitelist paths as `V` and the
            // CLI, persisted through the normal save/render commit.
            ui.horizontal(|ui| {
                ui.label(Str::Treatment.t());
                let bw = self.bw_active();
                if ui.selectable_label(!bw, Str::TreatmentColor.t()).clicked() {
                    if let Err(error) = self.set_treatment(TREATMENT_COLOR) {
                        self.show_error(error);
                    }
                }
                if ui
                    .selectable_label(bw, Str::TreatmentBlackWhite.t())
                    .clicked()
                {
                    if let Err(error) = self.set_treatment(TREATMENT_BW) {
                        self.show_error(error);
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(Str::Profile.t());
                let mut profile = self.recipe.develop_profile().to_string();
                egui::ComboBox::from_id_salt("develop_profile")
                    .selected_text(profile.clone())
                    .show_ui(ui, |ui| {
                        for candidate in DEVELOP_PROFILES {
                            ui.selectable_value(&mut profile, (*candidate).to_string(), *candidate);
                        }
                    });
                if profile != self.recipe.develop_profile() {
                    if let Err(error) = self.set_profile(&profile) {
                        self.show_error(error);
                    }
                }
            });
            self.draw_section_prev_reset(ui, SECTION_BASIC);
            ui.separator();
            ui.label(Str::WhiteBalance.t());
            self.adjustment_slider(
                ui,
                "wb_temperature",
                Str::Temperature.t(),
                identity_spec(1500.0..=12000.0, 6500.0, 50.0).unit(" K"),
            );
            self.adjustment_slider(ui, "wb_tint", Str::Tint.t(), percent_spec(-1.0..=1.0, 0.0));
            if ui.button(Str::ResetAsShot.t()).clicked() {
                if let Err(error) = self.reset_white_balance_to_as_shot() {
                    self.show_error(error);
                }
            }
            if self.wb_pick_mode {
                ui.horizontal(|ui| {
                    if ui.button(Str::WbEyedropperActive.t()).clicked() {
                        self.wb_pick_mode = false;
                    }
                    if ui.button(Str::Cancel.t()).clicked() {
                        self.wb_pick_mode = false;
                    }
                });
                ui.label(Str::PickWhiteBalanceHint.t());
            } else if ui.button(Str::WbEyedropper.t()).clicked() {
                // R5-TOOLFLOW-1 (User-Entscheid 2026-09-20): the former geometry
                // hard-lock is replaced by committing the active crop/straighten
                // draft on the tool switch (no dead-end banner).
                self.commit_outgoing_tool_for_switch(ui.ctx());
                self.arm_wb_picker();
            }
            ui.separator();
            self.adjustment_slider(
                ui,
                "exposure",
                Str::Exposure.t(),
                identity_spec(-10.0..=10.0, 0.0, 0.1),
            );
            self.adjustment_slider(
                ui,
                "contrast",
                Str::Contrast.t(),
                percent_spec(-1.0..=1.0, 0.0),
            );
            self.adjustment_slider(
                ui,
                "highlights",
                Str::Highlights.t(),
                percent_spec(-1.0..=1.0, 0.0),
            );
            self.adjustment_slider(
                ui,
                "shadows",
                Str::Shadows.t(),
                percent_spec(-1.0..=1.0, 0.0),
            );
            self.adjustment_slider(ui, "whites", Str::Whites.t(), percent_spec(-1.0..=1.0, 0.0));
            self.adjustment_slider(ui, "blacks", Str::Blacks.t(), percent_spec(-1.0..=1.0, 0.0));
            if ui.button(Str::Auto.t()).clicked() {
                if let Err(e) = self.auto_tone() {
                    self.show_error(e);
                }
            }
        });
        if section_response.header_response.clicked() {
            self.set_section_open(SECTION_BASIC, !section_was_open);
        }
    }
}
