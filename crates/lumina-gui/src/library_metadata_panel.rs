//! GUI-REFACTOR-W2-20 S2.3: the Library right-hand metadata panel,
//! extracted verbatim from `lib.rs`.
//!
//! [`LuminaApp::draw_library_metadata`] renders copy selection, keyword chips
//! and the metadata editor sections for the active entry. `pub(crate)` because
//! `draw_library_grid` (sibling module) calls it.

use super::*;

impl LuminaApp {
    pub(crate) fn draw_library_metadata(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::KeywordsSection.t(), |ui| {
            self.draw_keyword_chips(ui);
        });
        ui.collapsing(Str::CollectionsSection.t(), |ui| {
            let memberships = self.collections();
            let mut leave: Option<String> = None;
            for membership in &memberships {
                ui.horizontal(|ui| {
                    ui.label(format!("{} ({})", membership.name, membership.id));
                    if ui.button("✕").clicked() {
                        leave = Some(membership.id.clone());
                    }
                });
            }
            if let Some(id) = leave {
                if let Err(error) = self.remove_from_collection(&id) {
                    self.show_error(error);
                }
            }
            ui.horizontal(|ui| {
                let mut id = self.collection_id_input.clone();
                let mut name = self.collection_name_input.clone();
                let mut changed = false;
                changed |= ui
                    .add(egui::TextEdit::singleline(&mut id).hint_text(Str::CollectionIdHint.t()))
                    .changed();
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut name)
                            .hint_text(Str::CollectionNameHint.t()),
                    )
                    .changed();
                if changed {
                    self.collection_id_input = id.clone();
                    self.collection_name_input = name.clone();
                }
                if ui.button(Str::AddToCollection.t()).clicked() {
                    // `id=name` shorthand in the id field (CLI-compat).
                    let assignment = if name.trim().is_empty() && id.contains('=') {
                        Self::split_collection_assignment(id.trim())
                    } else {
                        Ok((id.trim().to_string(), name.trim().to_string()))
                    };
                    match assignment {
                        Ok((final_id, final_name)) => {
                            match self.add_to_collection(&final_id, &final_name) {
                                Ok(_) => {
                                    self.collection_id_input.clear();
                                    self.collection_name_input.clear();
                                }
                                Err(error) => self.show_error(error),
                            }
                        }
                        Err(error) => self.show_error(error),
                    }
                }
            });
            ui.label(Str::StaticCollections.t());
            if ui
                .selectable_label(self.active_collection.is_none(), Str::AllImages.t())
                .clicked()
            {
                self.set_active_collection(None);
            }
            for (id, name, count) in self.static_collections() {
                let selected =
                    self.active_collection == Some(CollectionFilter::Static { id: id.clone() });
                if ui
                    .selectable_label(selected, format!("{name} ({count})"))
                    .clicked()
                {
                    self.set_active_collection(Some(CollectionFilter::Static { id }));
                }
            }
        });
        ui.collapsing(Str::SmartSection.t(), |ui| {
            ui.horizontal(|ui| {
                let mut path = self.smart_catalog_path.clone();
                if ui
                    .add(egui::TextEdit::singleline(&mut path).hint_text(Str::CatalogPathHint.t()))
                    .changed()
                {
                    self.smart_catalog_path = path.clone();
                }
                if ui.button(Str::LoadCatalog.t()).clicked() {
                    if let Err(error) = self.load_smart_catalog(&path.clone()) {
                        self.show_error(error);
                    }
                }
                if ui.button(Str::SaveCatalog.t()).clicked() {
                    if let Err(error) = self.save_smart_catalog(&path.clone()) {
                        self.show_error(error);
                    }
                }
            });
            ui.horizontal(|ui| {
                let mut kind = self.smart_rule_kind.clone();
                egui::ComboBox::from_id_salt("smart_rule_kind")
                    .selected_text(kind.clone())
                    .show_ui(ui, |ui| {
                        for option in [
                            "all",
                            "none",
                            "keyword",
                            "rating_at_least",
                            "rating_equals",
                            "flag",
                        ] {
                            ui.selectable_value(&mut kind, option.to_string(), option);
                        }
                    });
                self.smart_rule_kind = kind.clone();
                let mut value = self.smart_rule_value.clone();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut value)
                            .hint_text(Str::SmartRuleValueHint.t()),
                    )
                    .changed()
                {
                    self.smart_rule_value = value.clone();
                }
                if ui.button(Str::PushRule.t()).clicked() {
                    if let Err(error) = self.push_smart_rule(&kind, &value) {
                        self.show_error(error);
                    } else {
                        self.smart_rule_value.clear();
                    }
                }
            });
            ui.horizontal(|ui| {
                for (op, label) in [
                    ("and", Str::CombineAnd.t()),
                    ("or", Str::CombineOr.t()),
                    ("not", Str::CombineNot.t()),
                ] {
                    if ui.button(label).clicked() {
                        if let Err(error) = self.combine_smart_stack(op) {
                            self.show_error(error);
                        }
                    }
                }
                if ui.button(Str::ClearStack.t()).clicked() {
                    self.smart_rule_stack.clear();
                }
                ui.label(
                    Str::RuleStackPattern.format_arg(&self.smart_rule_stack.len().to_string()),
                );
            });
            ui.horizontal(|ui| {
                let mut id = self.smart_id_input.clone();
                let mut name = self.smart_name_input.clone();
                let mut changed = false;
                changed |= ui
                    .add(egui::TextEdit::singleline(&mut id).hint_text(Str::SmartIdHint.t()))
                    .changed();
                changed |= ui
                    .add(egui::TextEdit::singleline(&mut name).hint_text(Str::SmartNameHint.t()))
                    .changed();
                if changed {
                    self.smart_id_input = id.clone();
                    self.smart_name_input = name.clone();
                }
                if ui.button(Str::CreateSmart.t()).clicked() {
                    if let Err(error) = self.create_smart_collection(id.trim(), name.trim()) {
                        self.show_error(error);
                    } else {
                        self.smart_id_input.clear();
                        self.smart_name_input.clear();
                    }
                }
            });
            let defs: Vec<(String, String)> = self
                .smart_catalog
                .iter()
                .map(|def| (def.id.clone(), def.name.clone()))
                .collect();
            for (id, name) in &defs {
                ui.horizontal(|ui| {
                    let selected =
                        self.active_collection == Some(CollectionFilter::Smart { id: id.clone() });
                    if ui.selectable_label(selected, name).clicked() {
                        if selected {
                            self.set_active_collection(None);
                        } else {
                            self.set_active_collection(Some(CollectionFilter::Smart {
                                id: id.clone(),
                            }));
                        }
                    }
                    if ui.button("✕").clicked() {
                        if let Err(error) = self.delete_smart_collection(id) {
                            self.show_error(error);
                        }
                    }
                });
            }
        });
        ui.collapsing(Str::BatchSection.t(), |ui| {
            ui.horizontal(|ui| {
                let mut kind = self.batch_kind.clone();
                egui::ComboBox::from_id_salt("batch_kind")
                    .selected_text(kind.clone())
                    .show_ui(ui, |ui| {
                        for option in [
                            "add_keyword",
                            "remove_keyword",
                            "add_to_collection",
                            "remove_from_collection",
                            "set_rating",
                            "set_flag",
                        ] {
                            ui.selectable_value(&mut kind, option.to_string(), option);
                        }
                    });
                self.batch_kind = kind.clone();
                let mut value = self.batch_value.clone();
                if ui
                    .add(egui::TextEdit::singleline(&mut value).hint_text(Str::BatchValueHint.t()))
                    .changed()
                {
                    self.batch_value = value.clone();
                }
                if ui.button(Str::BatchApply.t()).clicked() {
                    match parse_metadata_batch_op(&kind, value.trim()) {
                        Ok(op) => {
                            self.apply_metadata_batch(&op);
                        }
                        Err(message) => {
                            self.show_error(format!("invalid batch operation: {message}"));
                        }
                    }
                }
            });
            ui.label(format!("{} selected", self.filmstrip_selection.len()));
        });
        // LRPAR-G15-STACK-15: image stacks (Grid/Filmstrip unit). Buttons are
        // the primary, always-clickable path; `info!` logs each mutation and
        // errors stay visible via `show_error` (no silent failure).
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .button(Str::StackGroup.t())
                .on_hover_text("Group the selected images of this folder into a stack")
                .clicked()
            {
                if let Err(message) = self.create_stack_from_selection() {
                    self.show_error(message);
                }
            }
            if ui
                .button(Str::StackUngroup.t())
                .on_hover_text("Dissolve the stack(s) of the selection")
                .clicked()
            {
                if let Err(message) = self.unstack_selection() {
                    self.show_error(message);
                }
            }
            if let Some(collapsed) = self.active_stack_collapsed() {
                let toggle = if collapsed { "⊞" } else { "⊟" };
                if ui
                    .button(toggle)
                    .on_hover_text(if collapsed {
                        "Expand this stack"
                    } else {
                        "Collapse this stack"
                    })
                    .clicked()
                {
                    if let Err(message) = self.toggle_stack_collapse() {
                        self.show_error(message);
                    }
                }
            }
            ui.label(self.stack_status_label());
        });
    }
}
