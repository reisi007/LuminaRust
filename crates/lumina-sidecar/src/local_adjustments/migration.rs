//! Loud typed/legacy migration and layer-state helpers.

use super::{LocalAdjustments, LOCAL_ADJUSTMENT_RANGES};
use crate::{MaskLayer, SidecarDocument, SidecarError};
use serde_json::Value;

/// Validate a layer's typed/legacy local state without changing it.
pub fn validate_mask_layer_local_state(layer: &MaskLayer) -> Result<(), SidecarError> {
    if let Some(adjustments) = &layer.local_adjustments {
        adjustments.validate()?;
    }
    let legacy_keys: Vec<&String> = layer
        .extras
        .keys()
        .filter(|key| key.starts_with("adjustment_"))
        .collect();
    if !legacy_keys.is_empty() && layer.local_adjustments.is_some() {
        return Err(SidecarError::Invalid(format!(
            "mask layer `{}` has both typed local_adjustments and legacy adjustment_* extras",
            layer.id
        )));
    }
    for key in legacy_keys {
        let name = key
            .strip_prefix("adjustment_")
            .expect("filtered adjustment_ key");
        if !LOCAL_ADJUSTMENT_RANGES[..4]
            .iter()
            .any(|(known, _, _)| *known == name)
        {
            return Err(SidecarError::Invalid(format!(
                "mask layer `{}` has unknown local adjustment `{name}` in legacy extras",
                layer.id
            )));
        }
        let value = layer
            .extras
            .get(key)
            .and_then(Value::as_f64)
            .ok_or_else(|| {
                SidecarError::Invalid(format!(
                    "mask layer `{}` legacy local adjustment `{name}` must be a number",
                    layer.id
                ))
            })?;
        // Use the same range checker as the typed object, without clipping.
        let mut probe = LocalAdjustments::default();
        probe.set_value(name, value).map_err(|message| {
            SidecarError::Invalid(format!("mask layer `{}`: {message}", layer.id))
        })?;
    }
    Ok(())
}

/// Normalize valid legacy `adjustment_*` entries into the typed object and
/// remove the migrated keys.  A typed/legacy conflict is always loud; even
/// identical values are rejected because accepting them would leave two
/// sources of truth in a persisted layer.
pub(crate) fn normalize_legacy_layer_extras(layer: &mut MaskLayer) -> Result<(), String> {
    let legacy_keys: Vec<String> = layer
        .extras
        .keys()
        .filter(|key| key.starts_with("adjustment_"))
        .cloned()
        .collect();
    if legacy_keys.is_empty() {
        if let Some(adjustments) = layer.local_adjustments.clone() {
            let normalized = adjustments.normalized_version()?;
            layer.local_adjustments = Some(normalized);
        }
        return Ok(());
    }
    if layer.local_adjustments.is_some() {
        return Err(format!(
            "mask layer `{}` has conflicting typed local_adjustments and legacy adjustment_* extras",
            layer.id
        ));
    }
    let mut migrated = LocalAdjustments::default();
    for key in &legacy_keys {
        let name = key
            .strip_prefix("adjustment_")
            .expect("filtered adjustment_ key");
        if !LOCAL_ADJUSTMENT_RANGES[..4]
            .iter()
            .any(|(known, _, _)| *known == name)
        {
            return Err(format!(
                "mask layer `{}` has unknown local adjustment `{name}` in legacy extras",
                layer.id
            ));
        }
        let value = layer
            .extras
            .get(key)
            .and_then(Value::as_f64)
            .ok_or_else(|| {
                format!(
                    "mask layer `{}` legacy local adjustment `{name}` must be a number",
                    layer.id
                )
            })?;
        migrated
            .set_value(name, value)
            .map_err(|message| format!("mask layer `{}`: {message}", layer.id))?;
    }
    migrated.validate().map_err(|error| error.to_string())?;
    // Commit only after every legacy value parsed and validated. A malformed
    // later key therefore cannot leave the layer half-migrated.
    for key in legacy_keys {
        layer.extras.remove(&key);
    }
    layer.local_adjustments = Some(migrated);
    Ok(())
}

impl MaskLayer {
    /// Normalize this in-memory layer after a caller constructed or loaded it
    /// through a non-Serde API.  This is the explicit migration primitive used
    /// by sidecar writers and GUI transactions.
    pub fn normalize_local_adjustments(&mut self) -> Result<(), SidecarError> {
        normalize_legacy_layer_extras(self).map_err(SidecarError::Invalid)
    }

    /// Resolve the typed object, including a valid legacy value that has not
    /// yet been normalized in memory.  The returned value is owned so callers
    /// can evaluate without mutating the sidecar model.
    pub fn effective_local_adjustments(&self) -> Result<Option<LocalAdjustments>, SidecarError> {
        validate_mask_layer_local_state(self)?;
        if let Some(adjustments) = &self.local_adjustments {
            return Ok(Some(
                adjustments
                    .clone()
                    .normalized_version()
                    .map_err(SidecarError::Invalid)?,
            ));
        }
        let legacy_keys: Vec<&String> = self
            .extras
            .keys()
            .filter(|key| key.starts_with("adjustment_"))
            .collect();
        if legacy_keys.is_empty() {
            return Ok(None);
        }
        let mut migrated = LocalAdjustments::default();
        for key in legacy_keys {
            let name = key
                .strip_prefix("adjustment_")
                .expect("filtered adjustment_ key");
            let value = layer_value(self, key).ok_or_else(|| {
                SidecarError::Invalid(format!(
                    "mask layer `{}` legacy local adjustment `{name}` must be a number",
                    self.id
                ))
            })?;
            migrated.set_value(name, value).map_err(|message| {
                SidecarError::Invalid(format!("mask layer `{}`: {message}", self.id))
            })?;
        }
        Ok(Some(migrated))
    }
}

fn layer_value(layer: &MaskLayer, key: &str) -> Option<f64> {
    layer.extras.get(key).and_then(Value::as_f64)
}

impl SidecarDocument {
    /// Normalize valid legacy `adjustment_*` layer extras into the typed
    /// MASK-LOCAL-P0 object. Invalid/conflicting values return an error and
    /// leave the document untouched.
    pub fn normalize_legacy_local_adjustments(&mut self) -> Result<(), SidecarError> {
        // Normalize a clone and commit only after every layer and history
        // snapshot succeeded. This makes the documented no-partial-migration
        // guarantee hold for the whole document, not just one layer.
        let mut normalized = self.clone();
        normalize_document_local_adjustments(&mut normalized)?;
        *self = normalized;
        Ok(())
    }

    pub fn to_json(&self) -> Result<String, SidecarError> {
        // Normalize a clone so a legacy in-memory model cannot leak old extras
        // into a newly written sidecar, and never partially mutate the caller.
        let mut normalized = self.clone();
        normalize_document_local_adjustments(&mut normalized)?;
        normalized.validate()?;
        serde_json::to_string_pretty(&normalized).map_err(|e| SidecarError::Json(e.to_string()))
    }
}

fn normalize_document_local_adjustments(
    document: &mut SidecarDocument,
) -> Result<(), SidecarError> {
    for copy in document
        .virtual_copies
        .iter_mut()
        .chain(document.deleted_virtual_copies.iter_mut())
    {
        for layer in &mut copy.mask_layers {
            layer.normalize_local_adjustments()?;
        }
        for entry in &mut copy.history {
            if let Some(snapshot) = entry.mask_state()? {
                entry.set_mask_state(snapshot)?;
            }
        }
    }
    Ok(())
}
