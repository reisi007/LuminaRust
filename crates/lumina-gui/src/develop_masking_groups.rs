//! LRPAR-G03-MASKGROUP-03 (GUI slice): Copy vs. Duplicate, mask groups and the
//! collapsible group panel.
//!
//! The document operations live in `lumina-sidecar` (`mask_group_ops`); this
//! module owns the session state, the loud error surfacing, the persistence and
//! the panel. Every visible control is a clickable button/checkbox; shortcuts
//! would only be aliases. User actions log at `info!` (DoD §4).
//!
//! `duplicate_mask` (the deep `Copy`) was moved here from `lib.rs` for the
//! file-size ratchet; its behaviour is unchanged.

use super::*;
use lumina_sidecar::MaskGroup;

impl LuminaApp {
    /// Deep, independent `Copy` (LRPAR-G03-MASKGROUP-03): fresh stable id, own
    /// definition/prompt; the binary payload stays deduplicated by content hash
    /// in `.zdata`. Source changes do not propagate. Derived nodes are rebuilt
    /// with [`Self::combine_masks`] instead of aliased silently.
    pub fn duplicate_mask(
        &mut self,
        mask_id: &str,
        name: impl Into<String>,
    ) -> Result<String, GuiError> {
        instrument_gui_action!(self, GuiAction::DuplicateMask);
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io(Str::MaskNameEmpty.t().to_string()));
        }
        self.ensure_document_loaded()?;
        let copy_id = self.virtual_copy_id.clone();
        let template = {
            let document = self.document.as_ref().expect("document was ensured");
            let copy = document
                .virtual_copies
                .iter()
                .find(|copy| copy.id == copy_id)
                .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
            copy.mask_library
                .iter()
                .find(|mask| mask.id == mask_id)
                .cloned()
                .ok_or_else(|| GuiError::Io(Str::MaskNotFound.t().to_string()))?
        };
        if template.operation != MaskOperation::Source {
            return Err(GuiError::Io(Str::DuplicateSourceOnly.t().to_string()));
        }
        let id = format!(
            "mask-{}",
            blake3::hash(format!("duplicate\0{copy_id}\0{mask_id}\0{name}").as_bytes()).to_hex()
        );
        let mut duplicated = template;
        duplicated.id = id;
        duplicated.name = name;
        duplicated.created_at = "pending".into();
        duplicated.generator_version = env!("CARGO_PKG_VERSION").into();
        let id = self.push_mask_definition(duplicated)?;
        info!("GUI interaction: duplicate_mask (Copy) {mask_id} -> {id}");
        self.status = Str::MaskCreated.t().into();
        Ok(id)
    }

    /// `Duplicate` (LRPAR-G03-MASKGROUP-03): a group with a pointer member on
    /// the source node. Editing the source is visible to the member — no silent
    /// decoupling.
    pub fn group_duplicate_mask(
        &mut self,
        mask_id: &str,
        name: impl Into<String>,
    ) -> Result<String, GuiError> {
        instrument_gui_action!(self, GuiAction::GroupDuplicateMask);
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io(Str::MaskNameEmpty.t().to_string()));
        }
        self.ensure_document_loaded()?;
        let copy_id = self.virtual_copy_id.clone();
        let member = MaskReference {
            copy_id: copy_id.clone(),
            mask_id: mask_id.into(),
            extras: BTreeMap::new(),
        };
        let group_id = MaskGroup::group_id_for_members(&copy_id, std::slice::from_ref(&member));
        let member_id = member.mask_id.clone();
        self.transact_mask_mutation(true, |app| {
            let active_copy_id = app.virtual_copy_id.clone();
            {
                let copy = app
                    .document
                    .as_mut()
                    .ok_or_else(|| GuiError::Io(Str::NoSidecarLoaded.t().to_string()))?
                    .virtual_copies
                    .iter_mut()
                    .find(|copy| copy.id == active_copy_id)
                    .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
                lumina_sidecar::group_masks(
                    copy,
                    group_id.clone(),
                    name.clone(),
                    std::slice::from_ref(&member_id),
                )
                .map_err(|error| GuiError::Io(error.to_string()))?;
            }
            app.select_mask_in_memory(mask_id)?;
            app.selected_group_id = Some(group_id.clone());
            Ok(())
        })?;
        info!("GUI interaction: group_duplicate_mask {mask_id} -> {group_id}");
        self.status = Str::MaskGroupedPattern.format_arg(&group_id);
        Ok(group_id)
    }

    /// Late grouping of one or more existing masks under a new group.
    pub fn create_mask_group(
        &mut self,
        name: impl Into<String>,
        member_ids: &[String],
    ) -> Result<String, GuiError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io(Str::MaskNameEmpty.t().to_string()));
        }
        if member_ids.is_empty() {
            return Err(GuiError::Io(Str::SelectMasksToGroup.t().to_string()));
        }
        let copy_id = self.virtual_copy_id.clone();
        let members: Vec<MaskReference> = member_ids
            .iter()
            .map(|mask_id| MaskReference {
                copy_id: copy_id.clone(),
                mask_id: mask_id.clone(),
                extras: BTreeMap::new(),
            })
            .collect();
        let group_id = MaskGroup::group_id_for_members(&copy_id, &members);
        self.mutate_active_copy(|copy| {
            lumina_sidecar::group_masks(copy, group_id.clone(), name.clone(), member_ids)
        })?;
        self.group_member_selection.clear();
        self.selected_group_id = Some(group_id.clone());
        info!(
            "GUI interaction: create_mask_group {group_id} ({} members)",
            member_ids.len()
        );
        self.status = Str::MaskGroupedPattern.format_arg(&group_id);
        Ok(group_id)
    }

    /// Selects a group as a unit: the first member becomes the active mask and
    /// the group is marked selected.
    pub fn select_mask_group(&mut self, group_id: &str) -> Result<(), GuiError> {
        let groups = self.mask_groups()?;
        let group = groups
            .iter()
            .find(|group| group.id == group_id)
            .ok_or_else(|| GuiError::Io(Str::MaskGroupNotFound.t().to_string()))?;
        let first = group
            .members
            .first()
            .map(|member| member.mask_id.clone())
            .ok_or_else(|| GuiError::Io(Str::MaskGroupNotFound.t().to_string()))?;
        self.select_mask(&first)?;
        self.selected_group_id = Some(group_id.into());
        info!("GUI interaction: select_mask_group {group_id}");
        Ok(())
    }

    /// Activate/deactivate the whole group: sets the visibility of every member
    /// mask (creating the referencing layer when absent).
    pub fn set_mask_group_visible(
        &mut self,
        group_id: &str,
        visible: bool,
    ) -> Result<(), GuiError> {
        self.mutate_active_copy(|copy| {
            let groups = lumina_sidecar::mask_groups_of(copy)?;
            let group = groups
                .iter()
                .find(|group| group.id == group_id)
                .ok_or_else(|| {
                    lumina_sidecar::SidecarError::Invalid(format!(
                        "mask group `{group_id}` not found"
                    ))
                })?;
            let member_ids: Vec<String> = group
                .members
                .iter()
                .map(|member| member.mask_id.clone())
                .collect();
            for mask_id in member_ids {
                super::develop_masking_group_panel::set_mask_visible_on_copy(
                    copy, &mask_id, visible,
                )?;
            }
            Ok(())
        })?;
        info!("GUI interaction: set_mask_group_visible {group_id} -> {visible}");
        Ok(())
    }

    /// Dissolves a group container. Member masks are kept (no silent deletion).
    pub fn remove_mask_group(&mut self, group_id: &str) -> Result<(), GuiError> {
        self.mutate_active_copy(|copy| lumina_sidecar::dissolve_group(copy, group_id))?;
        if self.selected_group_id.as_deref() == Some(group_id) {
            self.selected_group_id = None;
        }
        info!("GUI interaction: remove_mask_group {group_id} (members kept)");
        self.status = Str::MaskGroupDissolved.t().into();
        Ok(())
    }

    /// Persists the collapse state of one group (panel is collapsible).
    pub fn set_mask_group_collapsed(
        &mut self,
        group_id: &str,
        collapsed: bool,
    ) -> Result<(), GuiError> {
        self.mutate_active_copy(|copy| {
            lumina_sidecar::set_group_collapsed(copy, group_id, collapsed)
        })?;
        info!("GUI interaction: set_mask_group_collapsed {group_id} -> {collapsed}");
        Ok(())
    }

    /// Moves a group member by `delta` positions (clamped) within the group.
    pub fn move_mask_group_member(
        &mut self,
        group_id: &str,
        mask_id: &str,
        delta: isize,
    ) -> Result<(), GuiError> {
        self.mutate_active_copy(|copy| {
            lumina_sidecar::move_group_member(copy, group_id, mask_id, delta)
        })?;
        info!("GUI interaction: move_mask_group_member {group_id}/{mask_id} {delta:+}");
        Ok(())
    }

    /// Applies shared feather/density offsets to all member layers as one unit.
    pub fn adjust_mask_group_offsets(
        &mut self,
        group_id: &str,
        feather_delta: f32,
        density_delta: f32,
    ) -> Result<usize, GuiError> {
        let touched = self.mutate_active_copy(|copy| {
            lumina_sidecar::apply_group_parameter_offsets(
                copy,
                group_id,
                feather_delta,
                density_delta,
            )
        })?;
        info!(
            "GUI interaction: adjust_mask_group_offsets {group_id} feather{feather_delta:+} density{density_delta:+} -> {touched} layers"
        );
        self.status = Str::GroupOffsetsApplied.format_arg(&touched.to_string());
        Ok(touched)
    }

    /// Move one mask by exactly one position in the persisted list. Identity
    /// remains the stable mask id; the operation never wraps at an end.
    pub fn move_mask(&mut self, mask_id: &str, delta: isize) -> Result<bool, GuiError> {
        instrument_gui_action!(self, GuiAction::MoveMask);
        if !matches!(delta, -1 | 1) {
            return Err(GuiError::Io(Str::MaskOrderInvalid.t().to_string()));
        }
        let index = {
            let copy = self.active_copy_ref()?;
            copy.mask_library
                .iter()
                .position(|mask| mask.id == mask_id)
                .ok_or_else(|| GuiError::Io(Str::MaskNotFound.t().to_string()))?
        };
        let target = index.checked_add_signed(delta);
        let mask_library_len = self.active_copy_ref()?.mask_library.len();
        let Some(target) = target.filter(|target| *target < mask_library_len) else {
            return Ok(false);
        };
        self.mutate_active_copy(|copy| {
            let mask = copy.mask_library.remove(index);
            copy.mask_library.insert(target, mask);
            Ok(())
        })?;
        info!("GUI interaction: move_mask {mask_id} {index} -> {target}");
        Ok(true)
    }

    /// Deletes one mask node. References (derived inputs, group memberships,
    /// layers) are materialized as one frozen copy first — loud, with a history
    /// entry — never a dangling reference and never a silent member deletion.
    pub fn delete_mask(&mut self, mask_id: &str) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::DeleteMask);
        self.ensure_document_loaded()?;
        let copy_id = self.virtual_copy_id.clone();
        let timestamp = self.history_timestamp();
        let outcome = self.transact_mask_mutation(true, |app| {
            let outcome = {
                let document = app.document.as_mut().expect("document was ensured");
                let copy = document
                    .virtual_copies
                    .iter_mut()
                    .find(|copy| copy.id == copy_id)
                    .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
                lumina_sidecar::delete_mask_node(copy, mask_id)
                    .map_err(|error| GuiError::Io(error.to_string()))?
            };
            if !outcome.frozen_copies.is_empty() {
                let copy = app
                    .document
                    .as_mut()
                    .expect("document was ensured")
                    .virtual_copies
                    .iter_mut()
                    .find(|copy| copy.id == copy_id)
                    .expect("active copy exists");
                let mut counter = copy.history.len() + 1;
                while copy
                    .history
                    .iter()
                    .any(|entry| entry.id == format!("mask-delete-{counter}"))
                {
                    counter += 1;
                }
                let mut extras = BTreeMap::new();
                extras.insert(
                    "action".into(),
                    Value::String("mask.group.materialize".into()),
                );
                extras.insert(
                    "frozen".into(),
                    Value::String(outcome.frozen_copies.join(",")),
                );
                copy.history.push(HistoryEntry {
                    id: format!("mask-delete-{counter}"),
                    recipe: copy.recipe.clone(),
                    recorded_at: Some(timestamp),
                    extras,
                });
            }
            if app.selected_mask_id.as_deref() == Some(mask_id) {
                app.selected_mask_id = None;
                app.mask_rename_input.clear();
            }
            app.mask_rename_inputs.remove(mask_id);
            Ok(outcome)
        })?;
        info!(
            "GUI interaction: delete_mask {mask_id} frozen={:?} repointed={} removed={}",
            outcome.frozen_copies, outcome.layers_repointed, outcome.layers_removed
        );
        self.status = Str::MaskDeletedPattern.format_arg(&outcome.frozen_copies.len().to_string());
        Ok(())
    }

    /// Session group selection (never recipe/sidecar): the group marked selected
    /// in the panel.
    pub fn selected_group_id(&self) -> Option<&str> {
        self.selected_group_id.as_deref()
    }

    /// Reads the active copy's groups (loud on a malformed extras value).
    pub fn mask_groups(&self) -> Result<Vec<MaskGroup>, GuiError> {
        let copy = self.active_copy_ref()?;
        lumina_sidecar::mask_groups_of(copy).map_err(|error| GuiError::Io(error.to_string()))
    }

    /// Toggles a mask in the session group-member selection.
    pub fn toggle_group_member_selection(&mut self, mask_id: &str) {
        if !self.group_member_selection.remove(mask_id) {
            self.group_member_selection.insert(mask_id.into());
        }
    }

    /// Whether `mask_id` is currently marked for grouping.
    pub fn group_member_selected(&self, mask_id: &str) -> bool {
        self.group_member_selection.contains(mask_id)
    }

    fn active_copy_ref(&self) -> Result<&lumina_sidecar::VirtualCopy, GuiError> {
        self.document
            .as_ref()
            .and_then(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
            })
            .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))
    }

    /// Mutates the active copy and persists it through the same full-document
    /// rollback transaction as mask constructors. Group operations therefore
    /// cannot leave a saved layer/order change with a failed selection update.
    fn mutate_active_copy<T>(
        &mut self,
        mutate: impl FnOnce(&mut lumina_sidecar::VirtualCopy) -> Result<T, lumina_sidecar::SidecarError>,
    ) -> Result<T, GuiError> {
        self.ensure_document_loaded()?;
        self.transact_mask_mutation(true, |app| {
            let copy_id = app.virtual_copy_id.clone();
            let copy = app
                .document
                .as_mut()
                .ok_or_else(|| GuiError::Io(Str::NoSidecarLoaded.t().to_string()))?
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == copy_id)
                .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
            mutate(copy).map_err(|error| GuiError::Io(error.to_string()))
        })
    }
}
