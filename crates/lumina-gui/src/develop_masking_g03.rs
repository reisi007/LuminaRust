//! GUI-REFACTOR-W2-20 S2.2: the G-03 AI/range mask editor, extracted verbatim
//! from `lib.rs`.
//!
//! [`LuminaApp::draw_masking_g03`] paints the AI-select/range/combinator controls
//! inside the Masking section. `pub(crate)` because `draw_masking` (sibling
//! module) and the headless G-03 tests call it.

use super::*;

impl LuminaApp {
    /// G-03 masking-parity rows of the Masking section (list with eye,
    /// status line, Show + color overlay, AI-select and range adds,
    /// Add/Subtract/Invert/Duplicate combinators). Own method so the
    /// headless panel test paints exactly this block without the
    /// collapsing-header open animation. Every button routes through the
    /// tested model methods; failures surface via `show_error` (loud).
    pub(crate) fn draw_masking_g03(&mut self, ui: &mut egui::Ui, document: &SidecarDocument) {
        // G-03 masking parity: mask list with visibility eye, AI-select
        // and range adds, Add/Subtract/Invert/Duplicate combinators, Show
        // + color overlay. Every button routes through the tested model
        // methods below; failures surface via `show_error` (loud).
        ui.separator();
        let library: Vec<(String, String, String, Option<String>)> = document
            .virtual_copies
            .iter()
            .find(|c| c.id == self.virtual_copy_id)
            .map(|c| {
                c.mask_library
                    .iter()
                    .map(|m| {
                        (
                            m.id.clone(),
                            m.name.clone(),
                            format!("{:?}", m.status).to_lowercase(),
                            m.error_text.clone(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        for (id, name, status, _error) in &library {
            ui.horizontal(|ui| {
                let mut eye = self.mask_visible(id);
                if ui.checkbox(&mut eye, Str::MaskEye.t()).changed() {
                    if let Err(e) = self.set_mask_visible(id, eye) {
                        self.show_error(e);
                    }
                }
                let selected = self.selected_mask_id.as_deref() == Some(id.as_str());
                if ui
                    .selectable_label(selected, format!("{name} [{status}]"))
                    .clicked()
                {
                    if let Err(e) = self.select_mask(id) {
                        self.show_error(e);
                    }
                }
            });
        }
        if let Some((status, error)) = self.selected_mask_status() {
            let mut line = format!(
                "{}: {}",
                Str::MaskStatusLabel.t(),
                format!("{status:?}").to_lowercase()
            );
            if let Some(error) = error {
                line.push_str(&format!(" — {error}"));
            }
            ui.label(line);
        }
        // Show master switch + overlay color (session display state).
        ui.horizontal(|ui| {
            let mut shown = self.show_mask_overlay;
            if ui.checkbox(&mut shown, Str::ShowOverlay.t()).changed() {
                self.set_show_mask_overlay(shown);
            }
            ui.label(Str::OverlayColor.t());
            let mut color = self.overlay_color;
            if ui.color_edit_button_srgb(&mut color).changed() {
                self.set_overlay_color(color);
            }
        });
        // AI-select add row.
        ui.separator();
        ui.label(Str::AiSelectLabel.t());
        egui::ComboBox::from_id_salt("g03_ai_kind")
            .selected_text(ai_select_kind_name(self.ai_select_kind))
            .show_ui(ui, |ui| {
                for kind in AiSelectKind::all() {
                    ui.selectable_value(&mut self.ai_select_kind, kind, ai_select_kind_name(kind));
                }
            });
        ui.label(Str::DetailLabel.t());
        ui.text_edit_singleline(&mut self.ai_detail_input);
        // Button-first right-to-left (GUI-VISION-1): a text field first would
        // push the row past the 320px panel budget. Nested exactly like the
        // New Mask row above.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            let add_clicked = ui.button(Str::AddAiMask.t()).clicked();
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                ui.text_edit_singleline(&mut self.ai_name_input);
            });
            if add_clicked {
                let detail = if self.ai_detail_input.trim().is_empty() {
                    None
                } else {
                    Some(self.ai_detail_input.trim().to_string())
                };
                if let Err(e) =
                    self.create_ai_mask(self.ai_select_kind, detail, self.ai_name_input.clone())
                {
                    self.show_error(e);
                } else {
                    self.ai_name_input.clear();
                    self.ai_detail_input.clear();
                }
            }
        });
        // Luminance-range add row.
        ui.separator();
        ui.label(Str::LuminanceRange.t());
        ui.add(egui::Slider::new(&mut self.lum_min, 0.0..=1.0).text("Min"));
        ui.add(egui::Slider::new(&mut self.lum_max, 0.0..=1.0).text("Max"));
        ui.add(egui::Slider::new(&mut self.lum_feather, 0.0..=1.0).text(Str::Feather.t()));
        // Button-first right-to-left (GUI-VISION-1): see the AI add row.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            if ui.button(Str::AddRange.t()).clicked() {
                if let Err(e) = self.create_luminance_range_mask(
                    self.lum_min,
                    self.lum_max,
                    self.lum_feather,
                    self.lum_name_input.clone(),
                ) {
                    self.show_error(e);
                } else {
                    self.lum_name_input.clear();
                }
            }
            ui.text_edit_singleline(&mut self.lum_name_input);
        });
        // Color-range add row.
        ui.separator();
        ui.label(Str::ColorRange.t());
        ui.add(egui::Slider::new(&mut self.col_hue_center, 0.0..=360.0).text("Hue"));
        ui.add(egui::Slider::new(&mut self.col_hue_width, 0.0..=360.0).text("Width"));
        ui.add(egui::Slider::new(&mut self.col_sat_min, 0.0..=1.0).text("Sat min"));
        ui.add(egui::Slider::new(&mut self.col_sat_max, 0.0..=1.0).text("Sat max"));
        ui.add(egui::Slider::new(&mut self.col_lum_min, 0.0..=1.0).text("Lum min"));
        ui.add(egui::Slider::new(&mut self.col_lum_max, 0.0..=1.0).text("Lum max"));
        ui.add(egui::Slider::new(&mut self.col_feather, 0.0..=1.0).text(Str::Feather.t()));
        // Button-first right-to-left (GUI-VISION-1): see the AI add row.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            if ui.button(Str::AddRange.t()).clicked() {
                if let Err(e) = self.create_color_range_mask(
                    self.col_hue_center,
                    self.col_hue_width,
                    self.col_sat_min,
                    self.col_sat_max,
                    self.col_lum_min,
                    self.col_lum_max,
                    self.col_feather,
                    self.col_name_input.clone(),
                ) {
                    self.show_error(e);
                } else {
                    self.col_name_input.clear();
                }
            }
            ui.text_edit_singleline(&mut self.col_name_input);
        });
        // Combine (selected + other) + invert + duplicate rows.
        ui.separator();
        ui.label(Str::CombineLabel.t());
        let others: Vec<(String, String)> = library
            .iter()
            .filter(|(id, _, _, _)| self.selected_mask_id.as_deref() != Some(id.as_str()))
            .map(|(id, name, _, _)| (id.clone(), name.clone()))
            .collect();
        egui::ComboBox::from_id_salt("g03_combine_other")
            .selected_text(
                others
                    .iter()
                    .find(|(id, _)| id == &self.combine_other_id)
                    .map(|(_, name)| name.as_str())
                    .unwrap_or("-"),
            )
            .show_ui(ui, |ui| {
                for (id, name) in &others {
                    ui.selectable_value(&mut self.combine_other_id, id.clone(), name);
                }
            });
        // Combine name (shared by Add/Subtract/Invert) on its own line so no
        // button row can push the panel past its budget.
        ui.text_edit_singleline(&mut self.combine_name_input);
        // Wrapped buttons (GUI-VISION-1): wrap instead of growing the panel.
        ui.horizontal_wrapped(|ui| {
            if ui.button(Str::CombineAdd.t()).clicked() {
                if let Err(e) = self.combine_masks(
                    MaskOperation::Union,
                    &self.combine_other_id.clone(),
                    self.combine_name_input.clone(),
                ) {
                    self.show_error(e);
                } else {
                    self.combine_name_input.clear();
                }
            }
            if ui.button(Str::CombineSubtract.t()).clicked() {
                if let Err(e) = self.combine_masks(
                    MaskOperation::Subtract,
                    &self.combine_other_id.clone(),
                    self.combine_name_input.clone(),
                ) {
                    self.show_error(e);
                } else {
                    self.combine_name_input.clear();
                }
            }
            if ui.button(Str::Invert.t()).clicked() {
                if let Err(e) =
                    self.combine_masks(MaskOperation::Invert, "", self.combine_name_input.clone())
                {
                    self.show_error(e);
                } else {
                    self.combine_name_input.clear();
                }
            }
        });
        // Copy (deep, independent) vs. Duplicate (group with a pointer member).
        // The name input owns its own line (GUI-VISION-1) so the wrapped button
        // row cannot push the panel past its budget.
        ui.text_edit_singleline(&mut self.duplicate_name_input);
        ui.horizontal_wrapped(|ui| {
            if ui.button(Str::DuplicateMask.t()).clicked() {
                let selected = self.selected_mask_id.clone().unwrap_or_default();
                if let Err(e) = self.duplicate_mask(&selected, self.duplicate_name_input.clone()) {
                    self.show_error(e);
                } else {
                    self.duplicate_name_input.clear();
                }
            }
            if ui.button(Str::DuplicateGroup.t()).clicked() {
                let selected = self.selected_mask_id.clone().unwrap_or_default();
                if let Err(e) =
                    self.group_duplicate_mask(&selected, self.duplicate_name_input.clone())
                {
                    self.show_error(e);
                } else {
                    self.duplicate_name_input.clear();
                }
            }
        });
        // LRPAR-G03-MASKGROUP-03: the collapsible group panel.
        self.draw_masking_groups(ui, document);
    }
}
