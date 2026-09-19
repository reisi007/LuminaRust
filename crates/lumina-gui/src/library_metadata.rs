//! GUI-REFACTOR-W2-20 S2.3: the Library metadata editor sections,
//! extracted verbatim from `lib.rs`.
//!
//! [`LuminaApp::draw_library_metadata_panel`] hosts the draft editor, the
//! read-only embedded values, the metadata history, the preset application and
//! the selection sync; [`LuminaApp::draw_meta_preset_dialog`] is the modal meta
//! preset dialog. Recipe/sidecar writes still route through the existing
//! mutators (loud errors, CAS saves). `pub(crate)` on the entry points called
//! from `lib.rs`; the sub-editors stay private to this module.

use super::*;

impl LuminaApp {
    /// LRPAR-G15-IPTC-S8: right-column Metadata panel of the Library module
    /// (SOLL §10). Draft field editor (all registry fields, `description`
    /// multiline, `date_created` with format validation), keyword chips
    /// ([`Self::draw_keyword_chips`]), read-only embedded values (JPEG),
    /// metadata history, meta presets (with prompt dialog for dynamic
    /// presets) and field-selective "sync to selection". Every mutation
    /// travels the same sidecar path as the CLI (CAS, atomar, loud).
    pub(crate) fn draw_library_metadata_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading(Str::MetadataSection.t());
        self.ensure_meta_buffers();
        self.draw_metadata_draft_editor(ui);
        ui.collapsing(Str::KeywordsSection.t(), |ui| {
            self.draw_keyword_chips(ui);
        });
        self.draw_metadata_embedded(ui);
        self.draw_metadata_history(ui);
        self.draw_metadata_preset(ui);
        self.draw_metadata_sync(ui);
        // LRPAR-G09-CULL-25: assisted-culling section (Library-only); shows
        // the read state and the explicit adopt/clear actions.
        self.draw_culling_section(ui);
        // LRPAR-G13-MERGE-15: HDR/panorama actions, job state and the visible
        // merge-bundle status.
        self.draw_merge_section(ui);
    }

    /// Draft field editor: one row per registry field (label + input +
    /// read-only embedded value when available), Save/Clear below. `date_created`
    /// carries its format hint; invalid values fail loudly on Save (nothing
    /// written, S1 validation).
    fn draw_metadata_draft_editor(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::MetadataDraftSection.t(), |ui| {
            let embedded = self.embedded_cached();
            let embedded_meta = embedded.as_ref().ok().and_then(|meta| meta.clone());
            for id in METADATA_FIELD_IDS {
                let Some(label) = metadata_field_label(id) else {
                    continue;
                };
                ui.horizontal(|ui| {
                    ui.label(label);
                    if let Some(value) = embedded_meta
                        .as_ref()
                        .and_then(|meta| embedded_field_value(meta, id))
                    {
                        ui.label(Str::MetadataEmbeddedValuePattern.format_arg(value));
                    }
                });
                let mut buffered = self.meta_buffers.get(*id).cloned().unwrap_or_default();
                let changed = if *id == "description" {
                    ui.add(
                        egui::TextEdit::multiline(&mut buffered)
                            .desired_rows(3)
                            .hint_text(label),
                    )
                    .changed()
                } else if *id == "date_created" {
                    ui.add(
                        egui::TextEdit::singleline(&mut buffered)
                            .hint_text(Str::MetadataDateHint.t()),
                    )
                    .changed()
                } else {
                    ui.add(egui::TextEdit::singleline(&mut buffered).hint_text(label))
                        .changed()
                };
                if changed {
                    self.meta_buffers.insert((*id).to_string(), buffered);
                    self.meta_buffers_dirty = true;
                }
            }
            // KITTEST-COVERAGE-STATES-2 (d): `horizontal_wrapped` — four
            // buttons on one unwrapped row forced the resizable right panel
            // ~33 px wider than its default (353 px), reflowing the center at
            // 1024 px. Wrapping keeps every button reachable at the 320 px
            // default width instead of growing the panel.
            ui.horizontal_wrapped(|ui| {
                if ui.button(Str::MetadataSaveDraft.t()).clicked() {
                    if let Err(error) = self.commit_metadata_draft() {
                        self.show_error(error);
                    }
                }
                if ui.button(Str::MetadataClearDraft.t()).clicked() {
                    let all: Vec<String> = METADATA_FIELD_IDS
                        .iter()
                        .map(|id| (*id).to_string())
                        .collect();
                    if let Err(error) = self.clear_metadata_fields(&all) {
                        self.show_error(error);
                    }
                }
                // KITTEST-COVERAGE-STATES-1: metadata panel's own clipboard
                // (separate from the Develop settings copy/paste).
                if ui.button(Str::MetadataCopyDraft.t()).clicked() {
                    if let Err(error) = self.copy_metadata_draft() {
                        self.show_error(error);
                    }
                }
                if ui.button(Str::MetadataPasteDraft.t()).clicked() {
                    if let Err(error) = self.paste_metadata_draft() {
                        self.show_error(error);
                    }
                }
            });
        });
    }

    /// Read-only embedded values (JPEG IIM/XMP): per-field draft-vs-embedded
    /// overlay is painted inline in the draft editor; this section shows the
    /// embedded keywords plus the unavailable note for non-JPEG sources.
    /// Broken JPEG segments stay loud (error text, never a silent skip).
    fn draw_metadata_embedded(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::MetadataEmbeddedSection.t(), |ui| {
            match self.embedded_cached() {
                Ok(None) => {
                    ui.label(Str::MetadataEmbeddedUnavailable.t());
                }
                Ok(Some(meta)) => {
                    if meta.keywords.is_empty() {
                        ui.label(Str::MetadataEmbeddedNoKeywords.t());
                    } else {
                        ui.label(
                            Str::MetadataEmbeddedKeywordsPattern
                                .format_arg(&meta.keywords.join(", ")),
                        );
                    }
                }
                Err(message) => {
                    ui.label(Str::MetadataEmbeddedUnreadablePattern.format_arg(&message));
                }
            }
        });
    }

    /// Metadata history (newest first, last entries) + explicit clear.
    /// Provenance context, not undo (SOLL §3).
    fn draw_metadata_history(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::History.t(), |ui| {
            let history = self.metadata_history();
            if history.is_empty() {
                ui.label(Str::NoHistory.t());
            } else {
                for entry in history.iter().take(10) {
                    let line = Str::MetadataHistoryEntryPattern
                        .t()
                        .replacen("{}", &entry.rev.to_string(), 1)
                        .replacen("{}", &entry.timestamp, 1)
                        .replacen("{}", &entry.origin, 1)
                        .replacen("{}", &entry.changed.join(", "), 1);
                    ui.label(line);
                }
            }
            if ui.button(Str::MetadataHistoryClear.t()).clicked() {
                if let Err(error) = self.clear_metadata_history_gui() {
                    self.show_error(error);
                }
            }
        });
    }

    /// Meta preset selector + Apply. Dynamic presets open the prompt dialog
    /// (one required input per placeholder); Cancel discards it. Failed
    /// preset files stay visible with their error text.
    fn draw_metadata_preset(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::MetadataPresetSection.t(), |ui| {
            if self.meta_preset_entries.is_empty() {
                self.refresh_meta_presets();
            }
            let names = self.meta_preset_names();
            let mut selected = self.selected_meta_preset.clone();
            egui::ComboBox::from_id_salt("meta_preset")
                .selected_text(if selected.is_empty() {
                    Str::MetadataPresetChooseHint.t().to_string()
                } else {
                    selected.clone()
                })
                .show_ui(ui, |ui| {
                    for name in &names {
                        ui.selectable_value(&mut selected, name.clone(), name);
                    }
                });
            self.selected_meta_preset = selected.clone();
            if ui.button(Str::MetadataPresetApply.t()).clicked() {
                if selected.trim().is_empty() {
                    self.show_error(Str::MetadataPresetChooseHint.t());
                    return;
                }
                match self.meta_preset_placeholders(&selected) {
                    Ok(placeholders) if placeholders.is_empty() => {
                        if let Err(error) =
                            self.apply_meta_preset_loaded(&selected, &BTreeMap::new())
                        {
                            self.show_error(error);
                        }
                    }
                    Ok(placeholders) => {
                        self.meta_preset_dialog = Some(MetaPresetDialog {
                            spec: selected.clone(),
                            name: selected.clone(),
                            placeholders,
                            vars: BTreeMap::new(),
                            error: None,
                        });
                    }
                    Err(error) => self.show_error(error),
                }
            }
            for entry in &self.meta_preset_entries {
                if let MetaPresetEntry::Failed { path, error } = entry {
                    ui.label(
                        Str::NeighborFailedPattern
                            .format_arg(&format!("{}: {error}", path.display())),
                    );
                }
            }
        });
    }

    /// Field checkboxes (default: all draft fields + keywords) + "sync to
    /// selection". The report surfaces via the status line (`applied` count)
    /// and `show_error` on failures, plus `info!`/`error!` per file (Stapel
    /// §4 pattern).
    fn draw_metadata_sync(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::MetadataSyncSection.t(), |ui| {
            let mut order: Vec<String> = METADATA_FIELD_IDS
                .iter()
                .map(|id| (*id).to_string())
                .collect();
            order.push("keywords".to_string());
            for id in &order {
                let label = metadata_field_label(id)
                    .map(str::to_string)
                    .unwrap_or_else(|| Str::KeywordsSection.t().to_string());
                let mut checked = self.meta_sync_fields.get(id).copied().unwrap_or(false);
                if ui.checkbox(&mut checked, label).changed() {
                    self.meta_sync_fields.insert(id.clone(), checked);
                }
            }
            if ui.button(Str::MetadataSyncButton.t()).clicked() {
                let mut fields: BTreeSet<String> = BTreeSet::new();
                for (id, checked) in &self.meta_sync_fields {
                    if *checked {
                        fields.insert(id.clone());
                    }
                }
                self.sync_metadata_to_selection(&fields);
            }
        });
    }

    /// Prompt dialog for dynamic meta presets (drawn as a floating window
    /// from `update`, like the toast): one required input per placeholder.
    /// Confirm applies (loud errors stay in the dialog); Cancel discards the
    /// dialog without touching any sidecar.
    pub(crate) fn draw_meta_preset_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.meta_preset_dialog.clone() else {
            return;
        };
        let mut close = false;
        let mut confirm = false;
        egui::Window::new(Str::MetadataPresetDialog.t()).show(ctx, |ui| {
            ui.label(&dialog.name);
            for (name, description) in &dialog.placeholders {
                ui.label(name);
                ui.label(description);
                let mut value = dialog.vars.get(name).cloned().unwrap_or_default();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut value)
                            .hint_text(Str::MetadataPresetVarHint.t()),
                    )
                    .changed()
                {
                    dialog.vars.insert(name.clone(), value);
                }
            }
            if let Some(error) = &dialog.error {
                ui.label(Str::NeighborFailedPattern.format_arg(error));
            }
            ui.horizontal(|ui| {
                if ui.button(Str::MetadataPresetConfirm.t()).clicked() {
                    confirm = true;
                }
                if ui.button(Str::Cancel.t()).clicked() {
                    close = true;
                }
            });
        });
        if close {
            self.meta_preset_dialog = None;
            return;
        }
        if confirm {
            let missing = dialog
                .placeholders
                .iter()
                .find(|(name, _)| dialog.vars.get(name).is_none_or(|v| v.trim().is_empty()));
            if let Some((name, _)) = missing {
                dialog.error = Some(Str::MetadataPresetVarRequiredPattern.format_arg(name));
                self.meta_preset_dialog = Some(dialog);
                return;
            }
            match self.apply_meta_preset_loaded(&dialog.spec, &dialog.vars) {
                Ok(_) if self.error().is_none() => {
                    self.meta_preset_dialog = None;
                }
                Ok(_) => {
                    dialog.error = Some(self.error().unwrap_or_default().to_string());
                    self.meta_preset_dialog = Some(dialog);
                }
                Err(error) => {
                    dialog.error = Some(error.to_string());
                    self.meta_preset_dialog = Some(dialog);
                }
            }
        } else {
            self.meta_preset_dialog = Some(dialog);
        }
    }
}
