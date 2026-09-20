//! GUI-REFACTOR-W2-20 S2.2: the Detail Develop section, extracted verbatim
//! from `lib.rs`.
//!
//! [`LuminaApp::draw_detail`] paints the sharpening/noise-reduction sliders and
//! the extracted denoise panel (`denoise_panel.rs`). `pub(crate)` because
//! `DEVELOP_SECTIONS` references it.

use super::*;

impl LuminaApp {
    pub(crate) fn draw_detail(&mut self, ui: &mut egui::Ui) {
        // G-11 solo: see `draw_basic`.
        let section_was_open = self.section_open[SECTION_DETAIL];
        let section_header =
            egui::CollapsingHeader::new(Str::Detail.t()).open(Some(section_was_open));
        let section_response = section_header.show(ui, |ui| {
            self.draw_section_prev_reset(ui, SECTION_DETAIL);
            ui.label(Str::Sharpening.t());
            // GUI-SLIDER-SAVE-1: sharpening sliders commit through
            // `set_sharpening_value` (save at debounce); `sh` is only a
            // slider binding buffer.
            let mut sh = self.recipe.sharpening.unwrap_or(Sharpening {
                version: 1,
                amount: 0.0,
                radius: 0.5,
                detail: 0.0,
                masking: 0.0,
            });
            let mut amount = sh.amount;
            if matches!(
                lr_slider(
                    ui,
                    Str::Amount.t(),
                    &mut amount,
                    identity_spec(0.0..=3.0, 0.0, 0.01)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                self.set_sharpening_value("amount", f64::from(amount));
            }
            let mut radius = sh.radius;
            if matches!(
                lr_slider(
                    ui,
                    Str::Radius.t(),
                    &mut radius,
                    identity_spec(0.1..=10.0, 0.5, 0.1)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                self.set_sharpening_value("radius", f64::from(radius));
            }
            let mut detail = sh.detail;
            if matches!(
                lr_slider(
                    ui,
                    Str::Detail.t(),
                    &mut detail,
                    identity_spec(0.0..=1.0, 0.0, 0.01)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                self.set_sharpening_value("detail", f64::from(detail));
            }
            let mut masking = sh.masking;
            if matches!(
                lr_slider(
                    ui,
                    Str::Masking.t(),
                    &mut masking,
                    identity_spec(0.0..=1.0, 0.0, 0.01)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                self.set_sharpening_value("masking", f64::from(masking));
            }
            ui.label(Str::NoiseReduction.t());
            // GUI-SLIDER-SAVE-1: same commit pattern via
            // `set_noise_reduction_value`.
            let mut nr = self.recipe.noise_reduction.unwrap_or(NoiseReduction {
                version: 1,
                luminance: 0.0,
                color: 0.0,
            });
            let mut lum = nr.luminance;
            if matches!(
                lr_slider(
                    ui,
                    Str::Luminance.t(),
                    &mut lum,
                    identity_spec(0.0..=1.0, 0.0, 0.01)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                self.set_noise_reduction_value("luminance", f64::from(lum));
            }
            let mut col = nr.color;
            if matches!(
                lr_slider(
                    ui,
                    Str::Color.t(),
                    &mut col,
                    identity_spec(0.0..=1.0, 0.0, 0.01)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                self.set_noise_reduction_value("color", f64::from(col));
            }
            // LRPAR-G14-DENOISE-IMPL-20: KI-Denoise stage controls + status
            // badge (the stage runs in the shared core pipeline).
            self.draw_denoise_section(ui);
            // G-14 Rote Augen (LRPAR-G14-REDEYE-15): explicit region marking.
            // The picker marks a pupil on the preview; every region is persisted
            // by a stable id with its own strength. No automatic detection in
            // this release (documented Folgearbeit).
            ui.separator();
            ui.label(Str::RedEye.t()).on_hover_text(Str::RedEyeHint.t());
            let mut pick = self.red_eye_pick_mode;
            if ui
                .toggle_value(&mut pick, Str::RedEyePickMode.t())
                .changed()
            {
                // R5-TOOLFLOW-1: a tool switch commits the active tool first.
                self.commit_outgoing_tool_for_switch(ui.ctx());
                self.set_red_eye_pick_mode(pick);
            }
            // LRPAR-G14-REDEYE-AUTO-15: detection is explicit only. "Detect
            // pupils" lists candidates; "Apply detected" persists them. No
            // automatic run ever happens while loading or rendering.
            ui.horizontal(|ui| {
                if ui.button(Str::RedEyeDetect.t()).clicked() {
                    if let Err(error) = self.detect_red_eye_candidates().map(|_| ()) {
                        self.show_error(error);
                    }
                }
                if ui.button(Str::RedEyeApplyDetected.t()).clicked() {
                    if let Err(error) = self.apply_detected_red_eye_objects().map(|_| ()) {
                        self.show_error(error);
                    }
                }
            });
            if !self.red_eye_detect_status.is_empty() {
                ui.label(&self.red_eye_detect_status);
            }
            let regions = self
                .recipe
                .red_eye
                .as_ref()
                .map(|correction| correction.regions.clone())
                .unwrap_or_default();
            if !regions.is_empty() {
                ui.label(Str::RedEyeCountPattern.format_arg(&regions.len().to_string()));
            }
            let mut pending_value: Option<(String, &'static str, f64)> = None;
            let mut pending_remove: Option<String> = None;
            for region in &regions {
                let id = region.id.clone();
                ui.push_id(&id, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(Str::RedEyeRegion.format_arg(&id));
                        if ui.button(Str::RedEyeRemove.t()).clicked() {
                            pending_remove = Some(id.clone());
                        }
                    });
                    let mut radius = region.radius;
                    if matches!(
                        lr_slider(
                            ui,
                            Str::RedEyeRadius.t(),
                            &mut radius,
                            identity_spec(0.001..=1.0, 0.05, 0.001)
                        ),
                        SliderAction::Changed | SliderAction::ResetRequested
                    ) {
                        pending_value = Some((id.clone(), "radius", f64::from(radius)));
                    }
                    let mut desaturate = region.desaturate;
                    if matches!(
                        lr_slider(
                            ui,
                            Str::RedEyeDesaturate.t(),
                            &mut desaturate,
                            identity_spec(0.0..=1.0, 0.8, 0.01)
                        ),
                        SliderAction::Changed | SliderAction::ResetRequested
                    ) {
                        pending_value = Some((id.clone(), "desaturate", f64::from(desaturate)));
                    }
                    let mut darken = region.darken;
                    if matches!(
                        lr_slider(
                            ui,
                            Str::RedEyeDarken.t(),
                            &mut darken,
                            identity_spec(0.0..=1.0, 0.4, 0.01)
                        ),
                        SliderAction::Changed | SliderAction::ResetRequested
                    ) {
                        pending_value = Some((id.clone(), "darken", f64::from(darken)));
                    }
                });
            }
            if let Some((id, field, value)) = pending_value {
                self.set_red_eye_region_value(&id, field, value);
            }
            if let Some(id) = pending_remove {
                self.remove_red_eye_region(&id);
            }
            if !regions.is_empty() && ui.button(Str::RedEyeClear.t()).clicked() {
                self.clear_red_eye();
            }
        });
        if section_response.header_response.clicked() {
            self.set_section_open(SECTION_DETAIL, !section_was_open);
        }
    }
}
