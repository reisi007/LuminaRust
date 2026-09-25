//! GUI-REFACTOR-W2-20 S2.8: recipe preset operations and the history
//! restore, extracted verbatim from `lib.rs`.
//!
//! [`LuminaApp::create_preset`] builds a named [`Preset`] from the active
//! recipe (relative-exposure validation, loud errors),
//! [`LuminaApp::apply_preset`] applies it non-destructively to the active copy
//! and [`LuminaApp::restore_history`] restores a stored history entry without
//! touching the saved sidecar until the next commit. All three stay `pub`
//! (public API / tests).

use super::*;
use log::trace;

impl LuminaApp {
    pub fn create_preset(&self, name: impl Into<String>) -> Result<Preset, GuiError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io(Str::PresetNameEmpty.t().to_string()));
        }
        let mut recipe = EditRecipe::default();
        for (field, selected) in &self.preset_fields {
            if *selected {
                if let Some(value) = self.recipe.adjustments.get(field) {
                    recipe.adjustments.insert(field.clone(), *value);
                }
            }
        }
        if self.preset_relative_exposure {
            if !self.recipe.auto_features.enable_auto_tone {
                return Err(GuiError::Io(
                    Str::RelativeExposureRequiresAutoTone.t().to_string(),
                ));
            }
            recipe
                .options
                .insert("exposure_semantics".into(), "relative".into());
        } else {
            recipe
                .options
                .insert("exposure_semantics".into(), "absolute".into());
        }
        Ok(Preset {
            id: format!("preset-{}", blake3::hash(name.as_bytes()).to_hex()),
            name,
            recipe,
            extras: BTreeMap::new(),
        })
    }

    pub fn apply_preset(&mut self, preset: &Preset) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::ApplyPreset);
        trace!("GUI interaction: apply_preset {}", preset.name);
        if preset
            .recipe
            .options
            .get("exposure_semantics")
            .map(String::as_str)
            == Some("relative")
            && !self.recipe.auto_features.enable_auto_tone
        {
            return Err(GuiError::Io(
                Str::RelativeExposureRequiresAutoTone.t().to_string(),
            ));
        }
        let previous = self.recipe.clone();
        for (key, value) in &preset.recipe.adjustments {
            let value = if key == "exposure"
                && preset
                    .recipe
                    .options
                    .get("exposure_semantics")
                    .map(String::as_str)
                    == Some("relative")
            {
                self.recipe.adjustments.get(key).copied().unwrap_or(0.0) + value
            } else {
                *value
            };
            self.recipe.adjustments.insert(key.clone(), value);
        }
        let changes = history_changes::recipe_changes(&previous, &self.recipe);
        let timestamp = self.history_timestamp();
        if let Some(document) = &mut self.document {
            if let Some(copy) = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == self.virtual_copy_id)
            {
                let id = format!("history-{}", copy.history.len() + 1);
                let mut entry = HistoryEntry {
                    id,
                    recipe: previous,
                    recorded_at: Some(timestamp),
                    extras: BTreeMap::new(),
                };
                entry
                    .set_mask_state(lumina_sidecar::MaskStateSnapshot::new(
                        copy.mask_layers.clone(),
                    ))
                    .map_err(|error| GuiError::Io(error.to_string()))?;
                if let Err(error) = entry.set_changes(changes) {
                    log::error!("preset history changes rejected: {error}");
                }
                copy.history.push(entry);
            }
        }
        self.render()
    }

    /// Lightroom-style non-destructive history restore: copies the stored
    /// recipe state of a history step of the active virtual copy into the
    /// session recipe and re-renders. Nothing is persisted until the user
    /// presses Save Recipe / Sidecar.
    pub fn restore_history(&mut self, entry_id: &str) -> Result<(), GuiError> {
        let document = self
            .document
            .as_ref()
            .ok_or_else(|| GuiError::Io(Str::NoSidecarLoaded.t().to_string()))?;
        let entry = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)
            .and_then(|copy| copy.history.iter().find(|entry| entry.id == entry_id))
            .cloned()
            .ok_or_else(|| GuiError::Io(Str::HistoryEntryMissing.t().to_string()))?;
        let recipe = entry.recipe.clone();
        let mask_state = entry
            .mask_state()
            .map_err(|error| GuiError::Io(error.to_string()))?;
        trace!("GUI interaction: history restore {}", entry_id);
        self.history_selected = Some(entry_id.to_string());
        self.recipe = recipe;
        if let Some(state) = mask_state {
            let copy_id = self.virtual_copy_id.clone();
            let document = self
                .document
                .as_mut()
                .ok_or_else(|| GuiError::Io(Str::NoSidecarLoaded.t().to_string()))?;
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == copy_id)
                .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
            copy.mask_layers = state.layers;
        }
        self.render()
    }
}
