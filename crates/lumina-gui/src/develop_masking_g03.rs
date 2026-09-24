//! GUI-REFACTOR-W2-20 S2.2: the G-03 AI/range mask editor, extracted verbatim
//! from `lib.rs`.
//!
//! [`LuminaApp::draw_masking_g03`] paints the AI-select/range/combinator controls
//! inside the Masking section. `pub(crate)` because `draw_masking` (sibling
//! module) and the headless G-03 tests call it.

use super::*;

pub(crate) type MaskLibraryRow = (String, String, String, Option<String>, Option<String>);

fn available_mask_name(rows: &[MaskLibraryRow], stem: &str, kind: &str) -> String {
    let mut candidate = format!("{stem} {kind}");
    let mut suffix = 2;
    while rows.iter().any(|(_, name, ..)| name == &candidate) {
        candidate = format!("{stem} {kind} {suffix}");
        suffix += 1;
    }
    candidate
}

impl LuminaApp {
    /// Paint the mask-library rows and their complete management surface.
    ///
    /// This is shared by the production G-03 editor and the R5-BRUSH-24
    /// structural/native snapshot tests. The returned rows are the immutable
    /// list snapshot used by the combine controls below, so actions dispatched
    /// after painting cannot borrow a stale document.
    pub(crate) fn draw_mask_library(
        &mut self,
        ui: &mut egui::Ui,
        document: &SidecarDocument,
    ) -> Vec<MaskLibraryRow> {
        ui.separator();
        let library: Vec<MaskLibraryRow> = document
            .virtual_copies
            .iter()
            .find(|c| c.id == self.virtual_copy_id)
            .map(|c| {
                let mut pin_number = 0usize;
                c.mask_library
                    .iter()
                    .map(|m| {
                        let pin = m
                            .prompt
                            .as_ref()
                            .filter(|_| self.mask_visible(&m.id))
                            .and_then(pin_anchor_for_prompt)
                            .map(|_| {
                                pin_number += 1;
                                pin_number.to_string()
                            });
                        (
                            m.id.clone(),
                            m.name.clone(),
                            format!("{:?}", m.status).to_lowercase(),
                            m.error_text.clone(),
                            pin,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        for (index, (id, name, status, _error, pin)) in library.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                if index == 0 {
                    ui.label(Str::MaskPin.t());
                }
                let mut visible = self.mask_visible(id);
                if ui.checkbox(&mut visible, Str::MaskEye.t()).changed() {
                    if let Err(e) = self.set_mask_visible(id, visible) {
                        self.show_error(e);
                    }
                }
                let selected = self.selected_mask_id.as_deref() == Some(id.as_str());
                let label = pin.as_ref().map_or_else(
                    || format!("{name} [{status}]"),
                    |pin| format!("{pin} · {name} [{status}]"),
                );
                if ui.selectable_label(selected, label).clicked() {
                    if let Err(e) = self.select_mask(id) {
                        self.show_error(e);
                    }
                }
            });

            // Every row owns its complete management surface. Actions are
            // collected while painting and dispatched after the closure so a
            // row can be reordered/deleted without borrowing a stale document.
            let mut move_up = false;
            let mut move_down = false;
            let mut rename = false;
            let mut delete = false;
            let mut copy = false;
            let mut duplicate = false;
            let mut draft = self
                .mask_rename_inputs
                .get(id)
                .cloned()
                .unwrap_or_else(|| name.clone());
            ui.horizontal_wrapped(|ui| {
                if ui.small_button(Str::MoveMaskUp.t()).clicked() {
                    move_up = true;
                }
                if ui.small_button(Str::MoveMaskDown.t()).clicked() {
                    move_down = true;
                }
                ui.add(
                    egui::TextEdit::singleline(&mut draft)
                        .desired_width(72.0)
                        .hint_text(Str::RenameMask.t()),
                );
                if ui.small_button(Str::RenameMask.t()).clicked() {
                    rename = true;
                }
                if ui.small_button(Str::DeleteMaskButton.t()).clicked() {
                    delete = true;
                }
                if ui.small_button(Str::DuplicateMask.t()).clicked() {
                    copy = true;
                }
                if ui.small_button(Str::DuplicateGroup.t()).clicked() {
                    duplicate = true;
                }
            });
            self.mask_rename_inputs.insert(id.clone(), draft.clone());
            if move_up {
                if let Err(e) = self.move_mask(id, -1) {
                    self.show_error(e);
                }
            } else if move_down {
                if let Err(e) = self.move_mask(id, 1) {
                    self.show_error(e);
                }
            } else if rename {
                if let Err(e) = self.rename_mask(id, draft) {
                    self.show_error(e);
                }
            } else if delete {
                if let Err(e) = self.delete_mask(id) {
                    self.show_error(e);
                }
            } else if copy {
                let name = available_mask_name(&library, name, "copy");
                if let Err(e) = self.duplicate_mask(id, name) {
                    self.show_error(e);
                }
            } else if duplicate {
                let name = available_mask_name(&library, name, "duplicate");
                if let Err(e) = self.group_duplicate_mask(id, name) {
                    self.show_error(e);
                }
            }
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
        library
    }

    /// G-03 masking-parity editor (mask list with eye, status line, Show +
    /// color overlay, AI-select and range adds, Add/Subtract/Invert/Duplicate
    /// combinators). The list is delegated to [`Self::draw_mask_library`] so
    /// the same painted rows are covered by structural and native tests.
    pub(crate) fn draw_masking_g03(&mut self, ui: &mut egui::Ui, document: &SidecarDocument) {
        let library = self.draw_mask_library(ui, document);
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
            .filter(|(id, _, _, _, _)| self.selected_mask_id.as_deref() != Some(id.as_str()))
            .map(|(id, name, _, _, _)| (id.clone(), name.clone()))
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
