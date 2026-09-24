//! Transactional persistence for mask-library management operations.
//!
//! A mask definition, its materialized layer, and the session selection are one
//! user-visible operation.  Keeping the transaction here prevents a constructor
//! from persisting the definition and then discovering a second write in
//! `select_mask`, which used to leave a half-applied operation after a CAS/IO
//! failure.

use super::*;
use std::collections::BTreeMap;

/// The mask-library state that must be restored when a management transaction
/// cannot be validated or persisted.  The rendered preview is intentionally not
/// snapshotted: `mark_dirty` runs only after the checked save succeeds, and a
/// failed transaction drops the live-plane cache so no stale pixels survive.
pub(crate) struct MaskMutationSnapshot {
    document: Option<SidecarDocument>,
    selected_mask_id: Option<String>,
    selected_group_id: Option<String>,
    mask_rename_input: String,
    mask_rename_inputs: BTreeMap<String, String>,
    pending_history_step: Option<String>,
}

impl LuminaApp {
    pub(crate) fn mask_mutation_snapshot(&self) -> MaskMutationSnapshot {
        MaskMutationSnapshot {
            document: self.document.clone(),
            selected_mask_id: self.selected_mask_id.clone(),
            selected_group_id: self.selected_group_id.clone(),
            mask_rename_input: self.mask_rename_input.clone(),
            mask_rename_inputs: self.mask_rename_inputs.clone(),
            pending_history_step: self.pending_history_step.clone(),
        }
    }

    pub(crate) fn restore_mask_mutation(&mut self, snapshot: MaskMutationSnapshot) {
        self.document = snapshot.document;
        self.selected_mask_id = snapshot.selected_mask_id;
        self.selected_group_id = snapshot.selected_group_id;
        self.mask_rename_input = snapshot.mask_rename_input;
        self.mask_rename_inputs = snapshot.mask_rename_inputs;
        self.pending_history_step = snapshot.pending_history_step;
        self.reset_brush_mask_plane();
    }

    /// Select a mask and materialize its own layer, without persisting yet.
    /// Returns whether a layer was added.  The caller decides whether this
    /// selection-only state change needs a sidecar write.
    pub(crate) fn select_mask_in_memory(&mut self, mask_id: &str) -> Result<bool, GuiError> {
        let copy_id = self.virtual_copy_id.clone();
        let (name, materialized) = {
            let document = self
                .document
                .as_mut()
                .ok_or_else(|| GuiError::Io(Str::NoSidecarLoaded.t().to_string()))?;
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == copy_id)
                .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
            let name = copy
                .mask_library
                .iter()
                .find(|mask| mask.id == mask_id)
                .map(|mask| mask.name.clone())
                .ok_or_else(|| GuiError::Io(Str::MaskNotFound.t().to_string()))?;
            let already_materialized = copy
                .mask_layers
                .iter()
                .any(|layer| layer.mask.copy_id == copy.id && layer.mask.mask_id == mask_id);
            if !already_materialized {
                let digest = blake3::hash(mask_id.as_bytes()).to_hex().to_string();
                let mut layer_id = format!("layer-mask-{}", &digest[..16]);
                let mut suffix = 2;
                while copy.mask_layers.iter().any(|layer| layer.id == layer_id) {
                    layer_id = format!("layer-mask-{}-{suffix}", &digest[..16]);
                    suffix += 1;
                }
                copy.mask_layers.push(MaskLayer {
                    id: layer_id,
                    mask: MaskReference {
                        copy_id: copy.id.clone(),
                        mask_id: mask_id.into(),
                        extras: BTreeMap::new(),
                    },
                    inverted: false,
                    feather: 0.0,
                    blur: 0.0,
                    density: 1.0,
                    extras: BTreeMap::new(),
                    visible: true,
                });
            }
            (name, !already_materialized)
        };
        self.selected_mask_id = Some(mask_id.into());
        self.mask_rename_inputs.insert(mask_id.into(), name.clone());
        self.mask_rename_input = name;
        // Selecting one mask leaves group mode.
        self.selected_group_id = None;
        self.reset_brush_mask_plane();
        Ok(materialized)
    }

    /// Validate and optionally persist a complete mask mutation as one
    /// transaction.  No caller may perform a second definition/selection save
    /// after this returns successfully.
    pub(crate) fn transact_mask_mutation<T>(
        &mut self,
        persist: bool,
        mutate: impl FnOnce(&mut Self) -> Result<T, GuiError>,
    ) -> Result<T, GuiError> {
        let snapshot = self.mask_mutation_snapshot();
        let value = match mutate(self) {
            Ok(value) => value,
            Err(error) => {
                self.restore_mask_mutation(snapshot);
                return Err(error);
            }
        };
        if let Some(document) = self.document.as_ref() {
            if let Err(error) = document.validate() {
                self.restore_mask_mutation(snapshot);
                return Err(GuiError::Io(error.to_string()));
            }
        }
        if persist {
            if let Err(error) = self.save_sidecar_checked() {
                self.restore_mask_mutation(snapshot);
                return Err(error);
            }
        }
        self.mark_dirty();
        Ok(value)
    }
}
