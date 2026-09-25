//! MASK-LOCAL-P0 state transfer for cross-image Previous.
//!
//! Sync Settings intentionally remains a recipe-only operation.  Previous is
//! different: its session reference carries the complete active-copy mask
//! state as a typed snapshot.  A target may receive that state only when its
//! mask context can resolve every referenced mask; otherwise the operation is
//! rejected before any sidecar write.

use super::*;
use crate::sidecar_rebase::{save_rebased, save_rebased_unit, MAX_REBASE_ATTEMPTS};

fn validate_previous_mask_context(
    document: &SidecarDocument,
    source_copy_id: &str,
    target_copy_id: &str,
    state: &MaskStateSnapshot,
) -> Result<(), String> {
    state.validate().map_err(|error| error.to_string())?;
    if state.layers.is_empty() {
        return Ok(());
    }
    if source_copy_id != target_copy_id {
        return Err(format!(
            "Previous mask state belongs to copy `{source_copy_id}`, but target copy is \
             `{target_copy_id}`; cross-copy mask transfer is not portable"
        ));
    }
    for layer in &state.layers {
        let Some(copy) = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == layer.mask.copy_id)
        else {
            return Err(format!(
                "Previous mask layer `{}` references missing target copy `{}`",
                layer.id, layer.mask.copy_id
            ));
        };
        if !copy
            .mask_library
            .iter()
            .any(|mask| mask.id == layer.mask.mask_id)
        {
            return Err(format!(
                "Previous mask layer `{}` references missing target mask `{}/{}`",
                layer.id, layer.mask.copy_id, layer.mask.mask_id
            ));
        }
    }
    Ok(())
}

fn add_previous_history(
    copy: &mut VirtualCopy,
    id: &str,
    before_recipe: &EditRecipe,
    after_recipe: &EditRecipe,
    before_layers: Vec<MaskLayer>,
    extras: BTreeMap<String, Value>,
    recorded_at: Option<String>,
) -> Result<(), String> {
    let mut history_id = id.to_string();
    let mut suffix = 1;
    while copy.history.iter().any(|entry| entry.id == history_id) {
        history_id = format!("{id}-{suffix}");
        suffix += 1;
    }
    let mut entry = HistoryEntry {
        id: history_id,
        recipe: before_recipe.clone(),
        recorded_at,
        extras,
    };
    entry
        .set_mask_state(MaskStateSnapshot::new(before_layers))
        .map_err(|error| error.to_string())?;
    entry
        .set_changes(history_changes::recipe_changes(before_recipe, after_recipe))
        .map_err(|error| error.to_string())?;
    copy.history.push(entry);
    Ok(())
}

impl LuminaApp {
    /// Capture the complete state of the currently loaded copy for a
    /// session-only cross-image Previous reference.
    pub(crate) fn previous_reference_for(&self, path: String) -> PreviousReference {
        let layers = self
            .document
            .as_ref()
            .and_then(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
            })
            .map(|copy| copy.mask_layers.clone())
            .unwrap_or_default();
        PreviousReference {
            path,
            recipe: self.recipe.clone(),
            copy_id: self.virtual_copy_id.clone(),
            mask_state: MaskStateSnapshot::new(layers),
        }
    }

    /// Apply Previous to the loaded image as one checked transaction. The
    /// candidate document is not adopted until the CAS write succeeds, so a
    /// failed save leaves both the live state and the pending retry intact.
    pub(crate) fn apply_previous_state_to_current(
        &mut self,
        reference: &PreviousReference,
        history_id: &str,
        history_extras: BTreeMap<String, Value>,
    ) -> Result<(), String> {
        self.ensure_document_loaded()
            .map_err(|error| error.to_string())?;
        if self.pending_slider_commit.is_some()
            || self.pending_history_step.is_some()
            || self.pending_mask_state_before.is_some()
        {
            return Err(
                "Previous target has an unsaved pending edit; commit or retry it before applying Previous"
                    .into(),
            );
        }
        let target_copy_id = self.virtual_copy_id.clone();
        let document = self
            .document
            .as_ref()
            .ok_or_else(|| "no sidecar document loaded".to_string())?;
        validate_previous_mask_context(
            document,
            &reference.copy_id,
            &target_copy_id,
            &reference.mask_state,
        )?;
        let base = document.clone();
        let mut candidate = base.clone();
        let copy = candidate
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == target_copy_id)
            .ok_or_else(|| "sidecar has no active virtual copy".to_string())?;
        let before_recipe = copy.recipe.clone();
        let before_layers = copy.mask_layers.clone();
        copy.recipe = reference.recipe.clone();
        copy.mask_layers = reference.mask_state.layers.clone();
        add_previous_history(
            copy,
            history_id,
            &before_recipe,
            &reference.recipe,
            before_layers,
            history_extras,
            Some(self.history_timestamp()),
        )?;
        candidate
            .validate()
            .map_err(|error| format!("Previous candidate is invalid: {error}"))?;
        let path = PathBuf::from(self.path.trim());
        let sidecar_path = lumina_sidecar::sidecar_path_for(&path);
        let saved = save_rebased(
            &sidecar_path,
            &base,
            &candidate,
            self.sidecar_revision.as_deref(),
            MAX_REBASE_ATTEMPTS,
        )
        .map_err(|error| error.to_string())?;
        let persisted = saved
            .document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == target_copy_id)
            .ok_or_else(|| "saved Previous state has no active virtual copy".to_string())?;
        if persisted.recipe != reference.recipe
            || persisted.mask_layers != reference.mask_state.layers
        {
            return Err("sidecar rebase did not preserve the complete Previous state".into());
        }
        let persisted_recipe = persisted.recipe.clone();
        self.recipe = persisted_recipe;
        self.finish_sidecar_save(&path, saved);
        self.reset_brush_mask_plane();
        self.render_mask_layers.clear();
        self.mark_dirty();
        self.render()
            .map_err(|error| format!("Previous persisted but preview render failed: {error}"))?;
        Ok(())
    }

    /// Apply a recipe to a file target, optionally carrying Previous's full
    /// mask snapshot. `None` is the documented Sync Settings path and leaves
    /// target mask layers untouched.
    pub(crate) fn apply_recipe_to_path_with_mask_state(
        &self,
        target: &str,
        recipe: &EditRecipe,
        history_id: &str,
        history_extras: BTreeMap<String, Value>,
        previous: Option<(&str, &MaskStateSnapshot)>,
    ) -> Result<(), String> {
        let path = PathBuf::from(target);
        let sidecar_path = lumina_sidecar::sidecar_path_for(&path);
        let (bytes, frame, orientation) = decode_selection_frame(&path)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(target);
        let mut document = if sidecar_path.exists() {
            lumina_sidecar::load_sidecar(&sidecar_path).map_err(|error| error.to_string())?
        } else {
            SidecarDocument::new(
                selection_source_identity(name, &bytes, &frame, orientation, is_raw_name(name)),
                "raster-mvp-1",
            )
        };
        let expected =
            lumina_sidecar::document_revision(&document).map_err(|error| error.to_string())?;
        let expected_revision = if sidecar_path.exists() {
            Some(expected)
        } else {
            None
        };
        let base = document.clone();
        let copy = default_copy_mut(&mut document)
            .ok_or_else(|| "sidecar has no virtual copies".to_string())?;
        let target_copy_id = copy.id.clone();
        if let Some((source_copy_id, state)) = previous {
            validate_previous_mask_context(&base, source_copy_id, &target_copy_id, state)?;
        }
        let before_recipe = copy.recipe.clone();
        let before_layers = copy.mask_layers.clone();
        copy.recipe = recipe.clone();
        if let Some((_, state)) = previous {
            copy.mask_layers = state.layers.clone();
        }
        let mut unique_id = history_id.to_string();
        let mut suffix = 1;
        while copy.history.iter().any(|entry| entry.id == unique_id) {
            unique_id = format!("{history_id}-{suffix}");
            suffix += 1;
        }
        let mut entry = HistoryEntry {
            id: unique_id,
            recipe: before_recipe.clone(),
            recorded_at: Some(self.history_timestamp()),
            extras: history_extras,
        };
        if previous.is_some() {
            entry
                .set_mask_state(MaskStateSnapshot::new(before_layers))
                .map_err(|error| error.to_string())?;
        }
        entry
            .set_changes(history_changes::recipe_changes(&before_recipe, recipe))
            .map_err(|error| error.to_string())?;
        copy.history.push(entry);
        document
            .validate()
            .map_err(|error| format!("Previous candidate is invalid: {error}"))?;
        save_rebased_unit(
            &sidecar_path,
            &base,
            &document,
            expected_revision.as_deref(),
        )
        .map_err(|error| error.to_string())
    }

    /// Switch copies only after pending edits are saved (or explicitly kept in
    /// memory for a bytes-only session). Dirty state is never silently replaced
    /// by the target copy.
    pub fn select_virtual_copy(&mut self, id: &str) -> Result<(), GuiError> {
        let Some(document) = &self.document else {
            return Err(GuiError::Io(Str::NoSidecarLoaded.t().to_string()));
        };
        let copy = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == id)
            .cloned()
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
        if id == self.virtual_copy_id {
            return Ok(());
        }
        let previous_id = self.virtual_copy_id.clone();
        let current = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == previous_id)
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
        let dirty = self
            .recipe_baseline
            .as_ref()
            .is_some_and(|baseline| baseline != &self.recipe)
            || current.recipe != self.recipe
            || self.mask_baseline != current.mask_layers;
        let pending = self.pending_slider_commit.is_some() || self.pending_history_step.is_some();
        let can_persist = !self.path.trim().is_empty() && self.original.is_some();
        let mut preserved_only = false;
        let saved_pending = if pending {
            if !can_persist {
                // Bytes-only sessions have no sidecar target. Keep the complete
                // copy state in memory and disarm only the debounce token.
                self.pending_slider_commit = None;
                self.pending_history_step = None;
                self.pending_mask_state_before = None;
                preserved_only = true;
                true
            } else if !self.commit_pending_slider_save([0, 0]) {
                return Err(GuiError::Io(
                    "virtual-copy switch aborted: pending edits could not be saved".into(),
                ));
            } else {
                true
            }
        } else if dirty {
            // Some older mask/brush paths mutate the document transactionally
            // without arming the debounce token. Flush those states as well;
            // refusing here would strand an otherwise valid edit.
            if !can_persist {
                preserved_only = true;
                true
            } else if let Err(error) = self.save_sidecar_result() {
                self.show_error(error.to_string());
                return Err(GuiError::Io(
                    "virtual-copy switch aborted: unsaved edits could not be saved".into(),
                ));
            } else {
                true
            }
        } else {
            false
        };
        // The flush may have rebased and adopted a newer document. Re-read
        // the target after that write so a concurrent target-side change is
        // never overwritten by this pre-flush clone.
        let copy = self
            .document
            .as_ref()
            .and_then(|document| document.virtual_copies.iter().find(|copy| copy.id == id))
            .cloned()
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
        self.virtual_copy_id = copy.id.clone();
        self.recipe = copy.recipe.clone();
        self.generative_artifacts = GenerativeArtifacts::default();
        self.generative_role_status = [GenerativeRoleStatus::Missing; 2];
        self.generative_memo = None;
        self.spot_detect_threshold = self.recipe.spot_visualize_threshold().unwrap_or(0.5);
        self.selected_mask_id = Self::first_local_mask_id(&copy);
        self.mask_rename_input = self
            .selected_mask_id
            .as_deref()
            .and_then(|id| copy.mask_library.iter().find(|mask| mask.id == id))
            .map(|mask| mask.name.clone())
            .unwrap_or_default();
        self.mask_rename_inputs.clear();
        self.selected_spot_id = None;
        self.history_selected = None;
        self.pending_brush_marks.clear();
        self.drag_start = None;
        self.drag_current = None;
        self.drawing = false;
        self.reset_brush_mask_plane();
        self.capture_section_baselines();
        if saved_pending {
            if preserved_only {
                warn!(
                    "virtual-copy switch from `{previous_id}` to `{}` preserved unsaved edits in memory",
                    self.virtual_copy_id
                );
            } else {
                warn!(
                    "virtual-copy switch from `{previous_id}` to `{}` flushed pending edits first",
                    self.virtual_copy_id
                );
            }
        }
        let outcome = self.render();
        if outcome.is_ok() {
            self.status = if saved_pending {
                if preserved_only {
                    format!(
                        "Switched to copy `{}` — unsaved edits of `{previous_id}` were preserved in memory",
                        self.virtual_copy_id
                    )
                } else {
                    format!(
                        "Switched to copy `{}` — pending edits of `{previous_id}` were saved first",
                        self.virtual_copy_id
                    )
                }
            } else {
                format!("Switched to copy `{}`", self.virtual_copy_id)
            };
        }
        outcome
    }
}
