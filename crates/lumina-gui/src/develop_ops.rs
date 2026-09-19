//! GUI-REFACTOR-W2-20 S2.8: the Develop Presets/History/Rating sections,
//! extracted verbatim from `lib.rs`.
//!
//! [`LuminaApp::draw_presets_section`] hosts the file-preset list and the
//! save-as-preset action ([`LuminaApp::draw_preset_file_list`],
//! [`LuminaApp::reload_preset_entries`],
//! [`LuminaApp::save_current_selection_as_preset_file`]);
//! [`LuminaApp::draw_history_section`] the human-readable history/snapshot
//! list and [`LuminaApp::draw_rating_section`] the rating/flag/label row. The
//! recipe mutations behind them are unchanged (paths relative, loud errors,
//! CAS saves). The three sections are `pub(crate)` because
//! `develop_scroll_content` and the headless tests call them; the private
//! preset-file helpers stay in this module.

use super::*;
use log::{info, trace};

impl LuminaApp {
    /// Lightroom-style Presets section (F-009): the file-backed preset list
    /// from the user-global presets directory (`<name>.lumina-preset.json`,
    /// click to apply, failing files stay visible with their error text), the
    /// save-to-file action, and the in-memory create/apply flow for the
    /// current field selection.
    pub(crate) fn draw_presets_section(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::PresetsSection.t(), |ui| {
            {
                self.draw_preset_file_list(ui);
                ui.separator();
            }
            ui.text_edit_singleline(&mut self.preset_name);
            for field in ["exposure", "contrast", "highlights", "shadows"] {
                let selected = self.preset_fields.entry(field.into()).or_insert(false);
                ui.checkbox(selected, field);
            }
            ui.checkbox(
                &mut self.preset_relative_exposure,
                Str::ExposureRelative.t(),
            );
            if ui.button(Str::ApplyPreset.t()).clicked() {
                match self
                    .create_preset(self.preset_name.clone())
                    .and_then(|preset| self.apply_preset(&preset))
                {
                    Ok(()) => self.status = "Preset applied, new history step".into(),
                    Err(error) => self.show_error(error),
                }
            }
            if ui.button(Str::SavePresetFile.t()).clicked() {
                match self.save_current_selection_as_preset_file() {
                    Ok(path) => {
                        trace!("GUI interaction: saved preset file {}", path.display());
                        self.status = Str::PresetSaved.format_arg(&path.display().to_string());
                    }
                    Err(error) => self.show_error(error),
                }
            }
        });
    }

    /// F-009: renders the file-backed preset list of `self.preset_entries`.
    /// The folder is shown so the storage location stays visible; every entry
    /// that failed validation is rendered with its error instead of being
    /// skipped silently. Entries are cloned first so clicking can borrow
    /// `self` mutably for [`Self::apply_preset`].
    fn draw_preset_file_list(&mut self, ui: &mut egui::Ui) {
        let Some(directory) = self.presets_dir.clone() else {
            ui.label(Str::PresetsUnavailable.t());
            return;
        };
        ui.horizontal(|ui| {
            ui.label(Str::PresetsFolder.t());
            ui.monospace(directory.display().to_string());
        });
        if ui.button(Str::Refresh.t()).clicked() {
            self.reload_preset_entries();
        }
        if self.preset_entries.is_empty() {
            ui.label(Str::NoPresets.t());
            return;
        }
        let entries = self.preset_entries.clone();
        for entry in &entries {
            match entry {
                presets::PresetEntry::Available { preset, .. } => {
                    if ui.selectable_label(false, &preset.name).clicked() {
                        trace!("GUI interaction: apply file preset {}", preset.name);
                        match self.apply_preset(preset) {
                            Ok(()) => self.status = Str::PresetApplied.format_arg(&preset.name),
                            Err(error) => self.show_error(error),
                        }
                    }
                }
                presets::PresetEntry::Failed { path, error } => {
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("{name}: {error}"));
                }
            }
        }
    }

    /// F-009: rescans the user presets directory. Scan problems surface as
    /// failed entries inside the list, never as silent drops.
    pub(crate) fn reload_preset_entries(&mut self) {
        instrument_gui_action!(self, GuiAction::ReloadPresetEntries);
        if let Some(directory) = self.presets_dir.as_deref() {
            self.preset_entries = presets::scan_presets_dir(directory);
        }
        info!(
            "GUI interaction: reload_preset_entries -> {} entries",
            self.preset_entries.len()
        );
    }

    /// F-009: persists the currently selected preset fields as
    /// `<name>.lumina-preset.json` in the user presets directory and refreshes
    /// the list. Overwriting an existing name is the documented update
    /// semantics (the display name is the identity and the list above shows
    /// the names before replacement); validation failures are loud errors.
    pub(crate) fn save_current_selection_as_preset_file(
        &mut self,
    ) -> Result<std::path::PathBuf, GuiError> {
        instrument_gui_action!(self, GuiAction::SavePresetFile);
        let directory = self
            .presets_dir
            .clone()
            .ok_or_else(|| GuiError::Io(Str::PresetsUnavailable.t().to_string()))?;
        let preset = self.create_preset(self.preset_name.clone())?;
        let path = presets::save_preset_file(&directory, &preset, true)
            .map_err(|error| GuiError::Io(error.to_string()))?;
        self.reload_preset_entries();
        Ok(path)
    }

    /// Lightroom-style History section: reverse-chronological entries of the
    /// active virtual copy; clicking an entry restores its stored recipe into
    /// the session recipe (non-destructive until Save Recipe / Sidecar).
    pub(crate) fn draw_history_section(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::History.t(), |ui| {
            // F-100 Klickbarkeit (GUI-CLICK-ALL-17): the copy/history actions
            // that used to be keyboard-only get clickable buttons here. Every
            // button routes through the same method as its shortcut.
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(Str::DuplicateCopy.t())
                    .on_hover_text(Str::ShortcutHint.format_arg("Cmd/Ctrl+'"))
                    .clicked()
                {
                    if let Err(error) = self.duplicate_active_copy() {
                        self.show_error(error);
                    }
                }
                if ui
                    .button(Str::CopySettings.t())
                    .on_hover_text(Str::ShortcutHint.format_arg("Cmd/Ctrl+Shift+C"))
                    .clicked()
                {
                    if let Err(error) = self.copy_settings() {
                        self.show_error(error);
                    }
                }
                if ui
                    .button(Str::PasteSettings.t())
                    .on_hover_text(Str::ShortcutHint.format_arg("Cmd/Ctrl+Shift+V"))
                    .clicked()
                {
                    if let Err(error) = self.paste_settings() {
                        self.show_error(error);
                    }
                }
                if ui
                    .button(Str::SnapshotButton.t())
                    .on_hover_text(Str::ShortcutHint.format_arg("Cmd/Ctrl+Alt+S"))
                    .clicked()
                {
                    self.create_snapshot_auto();
                }
                let stacked = self.stack_group_id().is_some();
                let stack_label = if stacked {
                    Str::StackUngroup
                } else {
                    Str::StackGroup
                };
                if ui
                    .selectable_label(stacked, stack_label.t())
                    .on_hover_text(Str::ShortcutHint.format_arg("Cmd/Ctrl+G"))
                    .clicked()
                {
                    if let Err(error) = self.toggle_stack_group() {
                        self.show_error(error);
                    }
                }
            });
            ui.separator();
            let Some(document) = self.document.clone() else {
                ui.label(Str::NoSidecarLoaded.t());
                return;
            };
            let Some(copy) = document
                .virtual_copies
                .iter()
                .find(|copy| copy.id == self.virtual_copy_id)
            else {
                ui.label(Str::VirtualCopyNotFound.t());
                return;
            };
            if copy.history.is_empty() {
                ui.label(Str::NoHistory.t());
                return;
            }
            let mut restore_target: Option<String> = None;
            for (index, entry) in copy.history.iter().enumerate().rev() {
                let mut label = format!("{}. {}", index + 1, entry.id);
                if let Some(recorded_at) = &entry.recorded_at {
                    label.push_str(&format!(" ({})", recorded_at));
                }
                let selected = self.history_selected.as_deref() == Some(entry.id.as_str());
                if ui.selectable_label(selected, label).clicked() {
                    restore_target = Some(entry.id.clone());
                }
            }
            if let Some(id) = restore_target {
                if let Err(error) = self.restore_history(&id) {
                    self.show_error(error);
                }
            }
        });
    }

    /// Lightroom-style Rating section (LR-01 + Welle 2 color label): star
    /// buttons `1`–`5` plus clear (`0` = unrated), Pick/Reject/Unflag buttons
    /// and color-label buttons (`6`–`9` select `1`–`4`, `0` clears) for the
    /// active virtual copy. Every button routes through
    /// [`Self::set_rating`]/[`Self::set_flag`]/[`Self::set_color_label`] —
    /// the same paths the `1-5`/`6-9`/`P`/`X`/`U` shortcuts use — so panel
    /// and keyboard can never diverge.
    pub(crate) fn draw_rating_section(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::Rating.t(), |ui| {
            let Some((rating, flag)) = self.active_rating_flag() else {
                ui.label(Str::NoSidecarLoaded.t());
                return;
            };
            let label = self.color_label().unwrap_or(0);
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{} {} ●{}",
                    stars_for_rating(rating),
                    flag_label(flag),
                    color_label_name(label)
                ));
            });
            ui.horizontal(|ui| {
                for candidate in 0..=5u8 {
                    let label = if candidate == 0 {
                        Str::UnsetPattern.format_arg("0")
                    } else {
                        candidate.to_string()
                    };
                    if ui
                        .selectable_label(rating == candidate, label)
                        .on_hover_text(format!("{candidate}"))
                        .clicked()
                    {
                        if let Err(error) = self.set_rating(candidate) {
                            self.show_error(error);
                        }
                    }
                }
            });
            ui.horizontal(|ui| {
                for candidate in [Flag::Pick, Flag::Reject, Flag::Unflagged] {
                    if ui
                        .selectable_label(flag == candidate, flag_label(candidate))
                        .clicked()
                    {
                        if let Err(error) = self.set_flag(candidate) {
                            self.show_error(error);
                        }
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(Str::ColorLabel.t());
                for candidate in 0..=4u8 {
                    let name = if candidate == 0 {
                        Str::UnsetPattern.format_arg("0")
                    } else {
                        format!("{candidate} {}", color_label_name(candidate))
                    };
                    if ui
                        .selectable_label(label == candidate, name)
                        .on_hover_text(format!("{candidate}"))
                        .clicked()
                    {
                        if let Err(error) = self.set_color_label(candidate) {
                            self.show_error(error);
                        }
                    }
                }
            });
        });
    }
}
