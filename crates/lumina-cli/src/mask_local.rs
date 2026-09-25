//! MASK-LOCAL-P0/P1.1 CLI mutation helpers.
//!
//! The `mask` command keeps orchestration in `main`; parsing, target
//! resolution and the typed local-state transaction live here so validation
//! happens before the sidecar write. Cross-image `previous` stays a
//! recipe-only transfer and refuses non-neutral local mask state.

use super::CliError;
use lumina_sidecar::{
    document_revision, load_sidecar, sidecar_path_for, EditRecipe, HistoryChange, HistoryEntry,
    MaskLayer, MaskStateSnapshot, SidecarDocument, VirtualCopy,
};
use std::collections::BTreeMap;
use std::path::Path;

pub(crate) fn require_mask_name(name: Option<&str>) -> Result<&str, CliError> {
    match name {
        Some(name) if !name.trim().is_empty() => Ok(name),
        _ => Err(CliError::Message(
            "this mask operation requires --name <NAME>".into(),
        )),
    }
}

pub(crate) fn mask_copy_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut VirtualCopy, CliError> {
    document
        .virtual_copies
        .iter_mut()
        .find(|copy| copy.id == copy_id)
        .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{copy_id}`")))
}

pub(crate) fn resolve_mask_copy(
    document: &SidecarDocument,
    requested: Option<&str>,
) -> Result<String, CliError> {
    if let Some(id) = requested {
        if document.virtual_copies.iter().any(|copy| copy.id == id) {
            return Ok(id.into());
        }
        return Err(CliError::Message(format!("unknown virtual copy `{id}`")));
    }
    if let Some(default) = document.virtual_copies.iter().find(|copy| copy.is_default) {
        return Ok(default.id.clone());
    }
    document
        .virtual_copies
        .first()
        .map(|copy| copy.id.clone())
        .ok_or_else(|| CliError::Message("sidecar has no virtual copies".into()))
}

/// Apply the repeatable local P0/P1.1 flags as one prevalidated transaction.
pub(crate) fn apply_local_adjustment_flags(
    document: &mut SidecarDocument,
    copy_id: &str,
    requested_layer: Option<&str>,
    set_specs: &[String],
    reset_keys: &[String],
    actions: &mut Vec<String>,
) -> Result<(), CliError> {
    if set_specs.is_empty() && reset_keys.is_empty() {
        if requested_layer.is_some() {
            return Err(CliError::Message(
                "--local-layer requires --set-local-adjustment or --reset-local-adjustment".into(),
            ));
        }
        return Ok(());
    }
    let layer_id = resolve_local_layer(document, copy_id, requested_layer)?;
    let mut layer = document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == copy_id)
        .and_then(|copy| copy.mask_layers.iter().find(|layer| layer.id == layer_id))
        .cloned()
        .ok_or_else(|| CliError::Message(format!("unknown mask layer `{layer_id}`")))?;
    layer
        .normalize_local_adjustments()
        .map_err(|error| CliError::Message(error.to_string()))?;
    let mut adjustments = layer.local_adjustments.unwrap_or_default();
    let mut staged_actions = Vec::new();
    for spec in set_specs {
        let (key, value) = parse_local_adjustment_spec(spec)?;
        adjustments
            .set_value(key, value)
            .map_err(CliError::Message)?;
        staged_actions.push(format!("local:{key}={value}"));
    }
    for key in reset_keys {
        adjustments.set_value(key, 0.0).map_err(CliError::Message)?;
        staged_actions.push(format!("local-reset:{key}"));
    }
    layer.local_adjustments = Some(adjustments);
    let target = document
        .virtual_copies
        .iter_mut()
        .find(|copy| copy.id == copy_id)
        .and_then(|copy| {
            copy.mask_layers
                .iter_mut()
                .find(|layer| layer.id == layer_id)
        })
        .ok_or_else(|| CliError::Message(format!("unknown mask layer `{layer_id}`")))?;
    *target = layer;
    actions.extend(staged_actions);
    Ok(())
}

/// Immutable pre-edit data needed to make a local adjustment one retryable
/// history/persistence transaction.
pub(crate) struct LocalAdjustmentTransaction {
    expected_revision: String,
    recipe: EditRecipe,
    layers: Vec<MaskLayer>,
    layer_id: String,
}

impl LocalAdjustmentTransaction {
    /// Capture the document revision before any mask command mutates the copy.
    pub(crate) fn capture(
        document: &SidecarDocument,
        copy_id: &str,
        requested_layer: Option<&str>,
    ) -> Result<Self, CliError> {
        let expected_revision = document_revision(document)?;
        let copy = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == copy_id)
            .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{copy_id}`")))?;
        Ok(Self {
            expected_revision,
            recipe: copy.recipe.clone(),
            layers: copy.mask_layers.clone(),
            layer_id: resolve_local_layer(document, copy_id, requested_layer)?,
        })
    }

    pub(crate) fn expected_revision(&self) -> &str {
        &self.expected_revision
    }

    /// Append one typed pre-edit snapshot after validation of all requested
    /// values and before the CAS save. A failed save never publishes this
    /// candidate, so the same command remains safe to retry from disk.
    pub(crate) fn append_history_entry(
        &self,
        document: &mut SidecarDocument,
        copy_id: &str,
        actions: &[String],
    ) -> Result<(), CliError> {
        let copy = document
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == copy_id)
            .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{copy_id}`")))?;
        let mut counter = 1usize;
        let id = loop {
            let candidate = format!("mask-local-{counter}");
            if !copy.history.iter().any(|entry| entry.id == candidate) {
                break candidate;
            }
            counter += 1;
        };
        let mut extras = BTreeMap::new();
        extras.insert(
            "step".into(),
            serde_json::Value::String("mask.local".into()),
        );
        extras.insert(
            "action".into(),
            serde_json::Value::String(actions.join(",")),
        );
        let mut entry = HistoryEntry {
            id,
            recipe: self.recipe.clone(),
            recorded_at: Some(lumina_sidecar::now_rfc3339_utc()),
            extras,
        };
        entry.set_mask_state(MaskStateSnapshot::new(self.layers.clone()))?;
        let before = self
            .layers
            .iter()
            .find(|layer| layer.id == self.layer_id)
            .and_then(|layer| layer.local_adjustments)
            .map(|adjustments| adjustments.to_string())
            .unwrap_or_else(|| "none".into());
        let after = copy
            .mask_layers
            .iter()
            .find(|layer| layer.id == self.layer_id)
            .and_then(|layer| layer.local_adjustments)
            .map(|adjustments| adjustments.to_string())
            .unwrap_or_else(|| "none".into());
        entry.set_changes(vec![HistoryChange {
            parameter: "mask.local".into(),
            from: before,
            to: after,
        }])?;
        copy.history.push(entry);
        Ok(())
    }
}

/// Return every layer carrying a non-neutral typed/legacy local recipe.
///
/// Hidden layers are included: they can become visible later, so dropping
/// their state in a cross-image recipe transfer would be silent data loss.
pub(crate) fn non_neutral_local_layer_ids(copy: &VirtualCopy) -> Result<Vec<String>, CliError> {
    let mut layer_ids = Vec::new();
    for layer in &copy.mask_layers {
        let adjustments = layer
            .effective_local_adjustments()
            .map_err(|error| CliError::Message(error.to_string()))?;
        if adjustments.is_some_and(|adjustments| !adjustments.is_neutral()) {
            layer_ids.push(layer.id.clone());
        }
    }
    Ok(layer_ids)
}

/// Write one Previous recipe with a checked target transaction. A target with
/// non-neutral local mask state is rejected before its recipe/history changes,
/// so the target bytes stay byte-identical.
///
/// MASK-LOCAL-P0/P1.1: this stays a recipe-only transfer. It never copies the
/// source's mask layers or their P0/P1.1 deltas onto the target; an explicit
/// full-look/mask copy is a separate, later action.
pub(crate) fn apply_previous_to_target(
    target: &Path,
    copy_id: &str,
    reference: &EditRecipe,
    from: &Path,
) -> Result<(), CliError> {
    let sidecar = sidecar_path_for(target);
    let mut document = load_sidecar(&sidecar)?;
    let expected_revision = document_revision(&document)?;
    let copy = document
        .virtual_copies
        .iter_mut()
        .find(|copy| copy.id == copy_id)
        .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{copy_id}`")))?;
    let local_layers = non_neutral_local_layer_ids(copy)?;
    if !local_layers.is_empty() {
        return Err(CliError::Message(format!(
            "previous refused recipe-only transfer to copy `{copy_id}`: non-neutral local mask adjustments exist on layer(s) {}; no sidecar was changed",
            local_layers.join(", ")
        )));
    }
    let before_layers = copy.mask_layers.clone();
    copy.recipe = reference.clone();
    // Portable by construction: only the reference file name (no paths —
    // absolute paths are forbidden in persistent recipe data).
    let source_name = from
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("reference")
        .to_string();
    let mut extras = BTreeMap::new();
    extras.insert("step".into(), serde_json::Value::String("previous".into()));
    extras.insert("source".into(), serde_json::Value::String(source_name));
    let mut entry = HistoryEntry {
        id: "previous".into(),
        recipe: reference.clone(),
        recorded_at: None,
        extras,
    };
    entry.set_mask_state(MaskStateSnapshot::new(before_layers))?;
    copy.history.push(entry);
    document.validate()?;
    lumina_sidecar::save_sidecar_if_unchanged(&sidecar, &document, Some(&expected_revision))?;
    Ok(())
}

fn parse_local_adjustment_spec(spec: &str) -> Result<(&str, f64), CliError> {
    let (key, value) = spec.split_once('=').ok_or_else(|| {
        CliError::Message(format!(
            "invalid --set-local-adjustment `{spec}`; expected KEY=VALUE"
        ))
    })?;
    let value = value.parse::<f64>().map_err(|_| {
        CliError::Message(format!(
            "invalid --set-local-adjustment `{spec}`; VALUE must be a finite number"
        ))
    })?;
    if !value.is_finite() {
        return Err(CliError::Message(format!(
            "invalid --set-local-adjustment `{spec}`; VALUE must be finite"
        )));
    }
    Ok((key, value))
}

fn resolve_local_layer(
    document: &SidecarDocument,
    copy_id: &str,
    requested: Option<&str>,
) -> Result<String, CliError> {
    let copy = document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == copy_id)
        .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{copy_id}`")))?;
    if let Some(layer_id) = requested {
        return copy
            .mask_layers
            .iter()
            .find(|layer| layer.id == layer_id)
            .map(|layer| layer.id.clone())
            .ok_or_else(|| CliError::Message(format!("unknown mask layer `{layer_id}`")));
    }
    match copy.mask_layers.as_slice() {
        [layer] => Ok(layer.id.clone()),
        [] => Err(CliError::Message(
            "no mask layer exists; attach a layer before setting local adjustments".into(),
        )),
        _ => Err(CliError::Message(
            "--local-layer is required when the target copy has multiple mask layers".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{mask_args, mask_imported_input};
    use lumina_sidecar::{save_sidecar, save_sidecar_if_unchanged, SidecarError};

    #[test]
    fn stale_local_transaction_fails_cas_and_can_be_retried() {
        let directory = tempfile::tempdir().unwrap();
        let input = mask_imported_input(directory.path(), "local-cas.png");
        let mut add = mask_args(input.clone());
        add.add_ai_select = Some("subject".into());
        add.name = Some("Subject".into());
        crate::mask(add).unwrap();
        let mask_id = load_sidecar(&sidecar_path_for(&input))
            .unwrap()
            .virtual_copies[0]
            .mask_library[0]
            .id
            .clone();
        let mut attach = mask_args(input.clone());
        attach.attach_layer = Some(mask_id);
        crate::mask(attach).unwrap();

        let path = sidecar_path_for(&input);
        let original = load_sidecar(&path).unwrap();
        let layer_id = original.virtual_copies[0].mask_layers[0].id.clone();
        let stale = LocalAdjustmentTransaction::capture(&original, "vc-original", None).unwrap();
        let mut external = original.clone();
        external.virtual_copies[0].rating = 1;
        save_sidecar(&path, &external).unwrap();

        let mut candidate = original;
        let mut actions = Vec::new();
        apply_local_adjustment_flags(
            &mut candidate,
            "vc-original",
            Some(&layer_id),
            &["exposure=1.0".into()],
            &[],
            &mut actions,
        )
        .unwrap();
        stale
            .append_history_entry(&mut candidate, "vc-original", &actions)
            .unwrap();
        let error = save_sidecar_if_unchanged(&path, &candidate, Some(stale.expected_revision()))
            .unwrap_err();
        assert!(matches!(error, SidecarError::Conflict(_)), "{error}");

        let unchanged = load_sidecar(&path).unwrap();
        assert_eq!(unchanged.virtual_copies[0].rating, 1);
        assert!(unchanged.virtual_copies[0].mask_layers[0]
            .local_adjustments
            .is_none());
        assert!(unchanged.virtual_copies[0].history.is_empty());

        let retry = LocalAdjustmentTransaction::capture(&unchanged, "vc-original", None).unwrap();
        let mut retry_candidate = unchanged;
        let mut retry_actions = Vec::new();
        apply_local_adjustment_flags(
            &mut retry_candidate,
            "vc-original",
            Some(&layer_id),
            &["exposure=1.0".into()],
            &[],
            &mut retry_actions,
        )
        .unwrap();
        retry
            .append_history_entry(&mut retry_candidate, "vc-original", &retry_actions)
            .unwrap();
        save_sidecar_if_unchanged(&path, &retry_candidate, Some(retry.expected_revision()))
            .unwrap();
        let saved = load_sidecar(&path).unwrap();
        assert_eq!(saved.virtual_copies[0].history.len(), 1);
        assert_eq!(
            saved.virtual_copies[0].mask_layers[0]
                .local_adjustments
                .unwrap()
                .exposure,
            1.0
        );
    }
}
