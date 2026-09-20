//! LRPAR-G03-MASKGROUP-03 (GUI panel slice): the collapsible mask-group panel.
//!
//! Extracted from `develop_masking_groups.rs` for the file-size ratchet. The
//! model methods and persistence live there; this module owns only the egui
//! painting and the per-copy visibility helper. Every visible control is a
//! clickable button/checkbox (DoD §5); user actions route through the tested
//! model methods and failures surface via `show_error` (loud).

use super::*;

/// One group row for the panel: id, name, collapse state, member ids and the
/// resolved member display names.
struct MaskGroupRow {
    id: String,
    name: String,
    collapsed: bool,
    members: Vec<(String, String)>,
}

impl LuminaApp {
    /// The mask-group panel (LRPAR-G03-MASKGROUP-03). Painted by
    /// [`Self::draw_masking_g03`] so the headless panel test covers it without
    /// the collapsing-header animation.
    pub(crate) fn draw_masking_groups(&mut self, ui: &mut egui::Ui, document: &SidecarDocument) {
        let library: Vec<(String, String)> = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)
            .map(|copy| {
                copy.mask_library
                    .iter()
                    .map(|mask| (mask.id.clone(), mask.name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let rows: Vec<MaskGroupRow> = match document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)
            .map(lumina_sidecar::mask_groups_of)
            .transpose()
        {
            Ok(Some(groups)) => groups
                .into_iter()
                .map(|group| MaskGroupRow {
                    id: group.id.clone(),
                    name: group.name.clone(),
                    collapsed: group.collapsed,
                    members: group
                        .members
                        .iter()
                        .map(|member| {
                            let name = library
                                .iter()
                                .find(|(id, _)| id == &member.mask_id)
                                .map(|(_, name)| name.clone())
                                .unwrap_or_else(|| member.mask_id.clone());
                            (member.mask_id.clone(), name)
                        })
                        .collect(),
                })
                .collect(),
            Ok(None) => Vec::new(),
            Err(error) => {
                self.show_error(error.to_string());
                Vec::new()
            }
        };

        ui.separator();
        ui.label(Str::MaskGroupsLabel.t());
        for (id, name) in &library {
            let mut checked = self.group_member_selected(id);
            if ui.checkbox(&mut checked, name).changed() {
                self.toggle_group_member_selection(id);
            }
        }
        ui.text_edit_singleline(&mut self.group_name_input);
        // Button-first right-to-left (GUI-VISION-1): the group button stays
        // inside the 320px panel budget.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            let clicked = ui.button(Str::GroupSelected.t()).clicked();
            ui.label(Str::GroupMembersLabel.t());
            if clicked {
                let members: Vec<String> = library
                    .iter()
                    .map(|(id, _)| id.clone())
                    .filter(|id| self.group_member_selected(id))
                    .collect();
                if let Err(error) = self.create_mask_group(self.group_name_input.clone(), &members)
                {
                    self.show_error(error);
                } else {
                    self.group_name_input.clear();
                }
            }
        });

        for row in &rows {
            ui.separator();
            let arrow = if row.collapsed { "▸" } else { "▾" };
            let selected = self.selected_group_id.as_deref() == Some(row.id.as_str());
            if ui
                .selectable_label(selected, format!("{} [{}]", row.name, row.members.len()))
                .clicked()
            {
                if let Err(error) = self.select_mask_group(&row.id) {
                    self.show_error(error);
                }
            }
            ui.horizontal_wrapped(|ui| {
                let mut active = row.members.iter().all(|(id, _)| self.mask_visible(id));
                if ui.checkbox(&mut active, Str::GroupActive.t()).changed() {
                    if let Err(error) = self.set_mask_group_visible(&row.id, active) {
                        self.show_error(error);
                    }
                }
                if ui.button(Str::Ungroup.t()).clicked() {
                    if let Err(error) = self.remove_mask_group(&row.id) {
                        self.show_error(error);
                    }
                }
                if ui.button(arrow).clicked() {
                    if let Err(error) = self.set_mask_group_collapsed(&row.id, !row.collapsed) {
                        self.show_error(error);
                    }
                }
            });
            if !row.collapsed {
                for (mask_id, name) in &row.members {
                    ui.horizontal_wrapped(|ui| {
                        if ui.small_button("↑").clicked() {
                            if let Err(error) = self.move_mask_group_member(&row.id, mask_id, -1) {
                                self.show_error(error);
                            }
                        }
                        if ui.small_button("↓").clicked() {
                            if let Err(error) = self.move_mask_group_member(&row.id, mask_id, 1) {
                                self.show_error(error);
                            }
                        }
                        if ui.button(name).clicked() {
                            if let Err(error) = self.select_mask(mask_id) {
                                self.show_error(error);
                            }
                        }
                        if ui.small_button(Str::DeleteMaskButton.t()).clicked() {
                            if let Err(error) = self.delete_mask(mask_id) {
                                self.show_error(error);
                            }
                        }
                    });
                }
            }
        }
        // Shared parameter offsets for all members of the selected group.
        if let Some(group_id) = self.selected_group_id.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(Str::GroupFeatherOffset.t());
                ui.add(egui::Slider::new(
                    &mut self.group_feather_offset,
                    -1.0..=1.0,
                ));
                ui.label(Str::GroupDensityOffset.t());
                ui.add(egui::Slider::new(
                    &mut self.group_density_offset,
                    -1.0..=1.0,
                ));
                if ui.button(Str::GroupApplyOffsets.t()).clicked() {
                    let feather = self.group_feather_offset;
                    let density = self.group_density_offset;
                    if let Err(error) = self.adjust_mask_group_offsets(&group_id, feather, density)
                    {
                        self.show_error(error);
                    }
                }
            });
        }
    }
}

/// Sets the visibility of every layer referencing `mask_id` of `copy`,
/// creating the referencing layer when none exists (never an invented matte).
pub(crate) fn set_mask_visible_on_copy(
    copy: &mut lumina_sidecar::VirtualCopy,
    mask_id: &str,
    visible: bool,
) -> Result<(), lumina_sidecar::SidecarError> {
    if !copy.mask_library.iter().any(|mask| mask.id == mask_id) {
        return Err(lumina_sidecar::SidecarError::Invalid(format!(
            "mask `{mask_id}` not found"
        )));
    }
    let copy_id = copy.id.clone();
    let mut touched = false;
    for layer in copy
        .mask_layers
        .iter_mut()
        .filter(|layer| layer.mask.copy_id == copy_id && layer.mask.mask_id == mask_id)
    {
        layer.visible = visible;
        touched = true;
    }
    if !touched {
        let mut layer_id = format!("layer-{mask_id}");
        let mut suffix = 2;
        while copy.mask_layers.iter().any(|layer| layer.id == layer_id) {
            layer_id = format!("layer-{mask_id}-{suffix}");
            suffix += 1;
        }
        copy.mask_layers.push(MaskLayer {
            id: layer_id,
            mask: MaskReference {
                copy_id,
                mask_id: mask_id.into(),
                extras: BTreeMap::new(),
            },
            inverted: false,
            feather: 0.0,
            blur: 0.0,
            density: 1.0,
            visible,
            extras: BTreeMap::new(),
        });
    }
    Ok(())
}
