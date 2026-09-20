//! GUI-REFACTOR-W2-20 S2.2: the Masking Develop section, extracted verbatim
//! from `lib.rs`.
//!
//! [`LuminaApp::draw_masking`] paints the mask list, tool selector and local
//! adjustments and hosts the G-03 editor (`develop_masking_g03.rs`). The G-11
//! overlay gates and recalculation offers are unchanged. `pub(crate)` because
//! `DEVELOP_SECTIONS` references it.

use super::*;

impl LuminaApp {
    pub(crate) fn draw_masking(&mut self, ui: &mut egui::Ui) {
        // G-11 solo: see `draw_basic`.
        let section_was_open = self.section_open[SECTION_MASKING];
        let section_header =
            egui::CollapsingHeader::new(Str::Masking.t()).open(Some(section_was_open));
        let section_response = section_header.show(ui, |ui| {
            let Some(document) = self.document.clone() else {
                return;
            };
            self.draw_section_prev_reset(ui, SECTION_MASKING);
            let mask_options: Vec<(String, String)> = document
                .virtual_copies
                .iter()
                .find(|c| c.id == self.virtual_copy_id)
                .map(|c| {
                    c.mask_library
                        .iter()
                        .map(|m| (m.id.clone(), m.name.clone()))
                        .collect()
                })
                .unwrap_or_default();
            let mut selected_mask = self.selected_mask_id.clone().unwrap_or_default();
            egui::ComboBox::from_label(Str::SelectMask.t())
                .selected_text(
                    mask_options
                        .iter()
                        .find(|(id, _)| id == &selected_mask)
                        .map(|(_, name)| name.as_str())
                        .unwrap_or("None"),
                )
                .show_ui(ui, |ui| {
                    for (id, name) in &mask_options {
                        ui.selectable_value(&mut selected_mask, id.clone(), name);
                    }
                });
            if selected_mask != self.selected_mask_id.clone().unwrap_or_default()
                && !selected_mask.is_empty()
            {
                if let Err(e) = self.select_mask(&selected_mask) {
                    self.show_error(e);
                }
            }
            // GUI-VISION-1 (same bug class as the Export Choose row):
            // button-first (right-to-left) so New Mask stays inside the panel.
            let mut new_clicked = false;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                new_clicked = ui.button(Str::NewMask.t()).clicked();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                    ui.text_edit_singleline(&mut self.mask_name_input);
                });
            });
            if new_clicked {
                if let Err(e) = self.create_mask(self.mask_name_input.clone()) {
                    self.show_error(e);
                } else {
                    self.mask_name_input.clear();
                }
            }
            // G-03 masking parity rows (own method so the headless panel
            // test can paint them without the collapsing-header animation).
            self.draw_masking_g03(ui, &document);
            // F-103-N4: interactive mask tools. The tool only picks how a drag on
            // the preview is interpreted; persistence goes through the sidecar.
            ui.separator();
            ui.label(Str::MaskTool.t());
            // R5-TOOLFLOW-1 (User-Entscheid 2026-09-20): the former geometry
            // hard-lock is replaced by the tool switch committing the active
            // crop/straighten draft first. The tools stay selectable; no
            // dead-end banner.
            ui.horizontal_wrapped(|ui| {
                for (tool, label) in [
                    (MaskTool::Brush, Str::MaskToolBrush),
                    (MaskTool::LinearGradient, Str::MaskToolGradient),
                    (MaskTool::Radial, Str::MaskToolRadial),
                ] {
                    if ui
                        .selectable_label(self.mask_tool == tool, label.t())
                        .clicked()
                    {
                        self.commit_outgoing_tool_for_switch(ui.ctx());
                        self.set_mask_tool(tool);
                    }
                }
                if ui
                    .selectable_label(self.mask_tool == MaskTool::None, Str::MaskToolNone.t())
                    .clicked()
                {
                    self.set_mask_tool(MaskTool::None);
                }
            });
            // G-11 overlay/panel comfort: global tool-overlay mode, edit-pin
            // visibility and solo mode. Session-only display state — never
            // recipe or sidecar.
            ui.separator();
            ui.label(Str::OverlayModeLabel.t());
            ui.horizontal_wrapped(|ui| {
                for (mode, name) in [
                    (OverlayMode::Always, Str::OverlayAlways),
                    (OverlayMode::Auto, Str::OverlayAuto),
                    (OverlayMode::Never, Str::OverlayNever),
                ] {
                    if ui
                        .selectable_label(self.overlay_mode == mode, name.t())
                        .clicked()
                    {
                        self.set_overlay_mode(mode);
                    }
                }
            });
            ui.label(Str::PinVisibilityLabel.t());
            ui.horizontal_wrapped(|ui| {
                for (visibility, name) in [
                    (PinVisibility::Always, Str::OverlayAlways),
                    (PinVisibility::Auto, Str::OverlayAuto),
                    (PinVisibility::Never, Str::OverlayNever),
                ] {
                    if ui
                        .selectable_label(self.pin_visibility == visibility, name.t())
                        .clicked()
                    {
                        self.set_pin_visibility(visibility);
                    }
                }
            });
            let mut solo = self.solo_mode;
            if ui.checkbox(&mut solo, Str::SoloMode.t()).changed() {
                self.set_solo_mode(solo);
            }
            if self.mask_tool == MaskTool::Brush {
                let mut radius = self.brush_radius;
                if ui
                    .add(egui::Slider::new(&mut radius, 0.005..=1.0).text(Str::BrushSize.t()))
                    .changed()
                {
                    if let Err(e) = self.set_brush_radius(radius) {
                        self.show_error(e);
                    }
                }
                ui.checkbox(&mut self.brush_eraser, Str::BrushEraser.t());
            }
            ui.label(Str::DrawMaskHint.t());
            if self.selected_mask_id.is_some() {
                let mut inverted = document
                    .virtual_copies
                    .iter()
                    .find(|c| c.id == self.virtual_copy_id)
                    .and_then(|c| c.mask_layers.first())
                    .is_some_and(|layer| layer.inverted);
                if ui.checkbox(&mut inverted, Str::Invert.t()).changed() {
                    if let Err(e) = self.set_mask_inverted(inverted) {
                        self.show_error(e);
                    }
                }
                let mut feather = document
                    .virtual_copies
                    .iter()
                    .find(|c| c.id == self.virtual_copy_id)
                    .and_then(|c| c.mask_layers.first())
                    .map_or(0.0, |layer| layer.feather);
                if ui
                    .add(egui::Slider::new(&mut feather, 0.0..=1.0).text(Str::Feather.t()))
                    .changed()
                {
                    if let Err(e) = self.set_mask_feather(feather) {
                        self.show_error(e);
                    }
                }
                let mut blur = document
                    .virtual_copies
                    .iter()
                    .find(|c| c.id == self.virtual_copy_id)
                    .and_then(|c| c.mask_layers.first())
                    .map_or(0.0, |layer| layer.blur);
                if ui
                    .add(egui::Slider::new(&mut blur, 0.0..=1.0).text(Str::Blur.t()))
                    .changed()
                {
                    if let Err(e) = self.set_mask_blur(blur) {
                        self.show_error(e);
                    }
                }
                let mut density = document
                    .virtual_copies
                    .iter()
                    .find(|c| c.id == self.virtual_copy_id)
                    .and_then(|c| c.mask_layers.first())
                    .map_or(1.0, |layer| layer.density);
                if ui
                    .add(egui::Slider::new(&mut density, 0.0..=1.0).text(Str::Density.t()))
                    .changed()
                {
                    if let Err(e) = self.set_mask_density(density) {
                        self.show_error(e);
                    }
                }
                if ui.button(Str::OfferRecalculation.t()).clicked() {
                    if let Err(e) = self.offer_mask_recalculation().and_then(|offered| {
                        if offered {
                            self.mark_mask_for_recalculation()
                        } else {
                            Ok(())
                        }
                    }) {
                        self.show_error(e);
                    }
                }
                ui.label(Str::LocalAdjustments.t());
                for (key, label) in [("exposure", Str::Exposure), ("contrast", Str::Contrast)] {
                    let stored = document
                        .virtual_copies
                        .iter()
                        .find(|c| c.id == self.virtual_copy_id)
                        .and_then(|c| c.mask_layers.first())
                        .and_then(|layer| layer.extras.get(&format!("adjustment_{key}")))
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);
                    let range = if key == "exposure" {
                        -10.0..=10.0
                    } else {
                        -1.0..=1.0
                    };
                    let mut value = stored;
                    if ui
                        .add(egui::Slider::new(&mut value, range).text(label.t()))
                        .changed()
                    {
                        if let Err(e) = self.set_mask_local_adjustment(key, value) {
                            self.show_error(e);
                        }
                    }
                }
            }
        });
        if section_response.header_response.clicked() {
            self.set_section_open(SECTION_MASKING, !section_was_open);
        }
    }
}
