//! Versioned local mask adjustments (`MASK-LOCAL-P0`).
//!
//! This module is deliberately small and sidecar-only.  It owns the typed P0
//! values, the one loud migration from the historical flattened
//! `adjustment_*` entries, and the canonical state digest used by render/cache
//! identities.  Pixel evaluation lives in `lumina-core`; keeping the schema and
//! the migration together prevents the GUI, CLI and core from inventing three
//! subtly different interpretations of the same layer.

use super::{Extras, MaskLayer, MaskReference, SidecarDocument};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

use crate::{EditRecipe, SidecarError};

/// Current version of the typed local-adjustment object.
pub const LOCAL_ADJUSTMENTS_VERSION: u8 = 1;

/// Maximum number of layer snapshots retained in one history entry.  The
/// active-copy validator has its own copy/layer limits; this smaller bound
/// keeps hostile history payloads from becoming unbounded allocations.
pub const MAX_MASK_STATE_LAYERS: usize = 4096;

/// P0 scalar controls.  The ranges intentionally mirror the global raster
/// controls, while the kernel order is owned by the core compositor.
pub const LOCAL_ADJUSTMENT_RANGES: [(&str, f64, f64); 4] = [
    ("exposure", -10.0, 10.0),
    ("contrast", -1.0, 1.0),
    ("highlights", -1.0, 1.0),
    ("shadows", -1.0, 1.0),
];

/// A typed, versioned P0 local adjustment set.
///
/// `0.0` is the identity for every field.  There is deliberately no local WB,
/// tone-curve, HSL or colour field in this version; adding one requires a new
/// version and a corresponding core/GPU decision rather than an untyped extra.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalAdjustments {
    pub version: u8,
    #[serde(default)]
    pub exposure: f64,
    #[serde(default)]
    pub contrast: f64,
    #[serde(default)]
    pub highlights: f64,
    #[serde(default)]
    pub shadows: f64,
}

impl Default for LocalAdjustments {
    fn default() -> Self {
        Self {
            version: LOCAL_ADJUSTMENTS_VERSION,
            exposure: 0.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
        }
    }
}

/// Stable human-readable representation used by CLI status output.
///
/// The documented grammar is `v1 exposure=<number> contrast=<number>
/// highlights=<number> shadows=<number>`, always in that field order. It is a
/// presentation contract independent of the derived `Debug` layout; JSON
/// consumers should continue to use the structured object instead.
impl fmt::Display for LocalAdjustments {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "v1 exposure={} contrast={} highlights={} shadows={}",
            self.exposure, self.contrast, self.highlights, self.shadows
        )
    }
}

impl LocalAdjustments {
    /// Validate the typed object without clipping or defaulting any value.
    pub fn validate(&self) -> Result<(), SidecarError> {
        if self.version != LOCAL_ADJUSTMENTS_VERSION {
            return Err(SidecarError::Invalid(format!(
                "unsupported local_adjustments version {} (expected {LOCAL_ADJUSTMENTS_VERSION})",
                self.version
            )));
        }
        for (name, value) in [
            ("exposure", self.exposure),
            ("contrast", self.contrast),
            ("highlights", self.highlights),
            ("shadows", self.shadows),
        ] {
            let (_, minimum, maximum) = LOCAL_ADJUSTMENT_RANGES
                .iter()
                .find(|(key, _, _)| *key == name)
                .expect("P0 local adjustment range is statically defined");
            if !value.is_finite() || !(*minimum..=*maximum).contains(&value) {
                return Err(SidecarError::Invalid(format!(
                    "local adjustment `{name}` must be finite and in {minimum}..={maximum}, got {value}"
                )));
            }
        }
        Ok(())
    }

    /// True when applying this object cannot change a pixel.
    #[must_use]
    pub fn is_neutral(&self) -> bool {
        self.exposure == 0.0
            && self.contrast == 0.0
            && self.highlights == 0.0
            && self.shadows == 0.0
    }

    /// Return one scalar value by its stable P0 key.
    #[must_use]
    pub fn value(&self, key: &str) -> Option<f64> {
        match key {
            "exposure" => Some(self.exposure),
            "contrast" => Some(self.contrast),
            "highlights" => Some(self.highlights),
            "shadows" => Some(self.shadows),
            _ => None,
        }
    }

    /// Set one scalar value by its stable P0 key.  Unknown keys and invalid
    /// values fail before mutation, so callers can retain their old state.
    pub fn set_value(&mut self, key: &str, value: f64) -> Result<(), String> {
        let (_, minimum, maximum) = LOCAL_ADJUSTMENT_RANGES
            .iter()
            .find(|(name, _, _)| *name == key)
            .copied()
            .ok_or_else(|| format!("unknown local adjustment `{key}`"))?;
        if !value.is_finite() || !(minimum..=maximum).contains(&value) {
            return Err(format!(
                "local adjustment `{key}` must be finite and in {minimum}..={maximum}, got {value}"
            ));
        }
        match key {
            "exposure" => self.exposure = value,
            "contrast" => self.contrast = value,
            "highlights" => self.highlights = value,
            "shadows" => self.shadows = value,
            _ => unreachable!("validated local adjustment key"),
        }
        Ok(())
    }

    /// Build a minimal global-recipe-shaped object for the shared core kernel.
    /// The core compositor uses this rather than reimplementing exposure,
    /// contrast, shadows or highlights arithmetic.
    #[must_use]
    pub fn as_recipe(&self) -> EditRecipe {
        let mut adjustments = BTreeMap::new();
        // Keep all four keys, including neutral ones, so the typed object has
        // one explicit and deterministic kernel contract.  The global kernel
        // treats zero as its documented identity.
        adjustments.insert("exposure".into(), self.exposure);
        adjustments.insert("contrast".into(), self.contrast);
        adjustments.insert("shadows".into(), self.shadows);
        adjustments.insert("highlights".into(), self.highlights);
        EditRecipe {
            adjustments,
            ..EditRecipe::default()
        }
    }

    /// Canonical digest of this typed object.  JSON object key ordering and
    /// the explicit version are part of the identity; no map iteration or
    /// display formatting is involved.
    #[must_use]
    pub fn digest(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("LocalAdjustments is serializable");
        format!("blake3:{}", blake3::hash(&bytes).to_hex())
    }
}

/// Canonical digest for the complete ordered mask-layer state.  The vector is
/// intentionally serialized in persisted order: reordering overlapping layers
/// is an edit, not an equivalent state.
#[must_use]
pub fn mask_layers_digest(layers: &[MaskLayer]) -> String {
    let value = serde_json::to_value(layers).expect("MaskLayer is serializable");
    let bytes = serde_json::to_vec(&value).expect("canonical mask state is serializable");
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"lumina-mask-state-v1\0");
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(&bytes);
    format!("blake3:{}", hasher.finalize().to_hex())
}

/// A complete additive mask-state snapshot for a history entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaskStateSnapshot {
    pub version: u8,
    pub layers: Vec<MaskLayer>,
}

impl MaskStateSnapshot {
    #[must_use]
    pub fn new(layers: Vec<MaskLayer>) -> Self {
        Self {
            version: LOCAL_ADJUSTMENTS_VERSION,
            layers,
        }
    }

    pub fn validate(&self) -> Result<(), SidecarError> {
        if self.version != LOCAL_ADJUSTMENTS_VERSION {
            return Err(SidecarError::Invalid(format!(
                "unsupported mask state snapshot version {}",
                self.version
            )));
        }
        if self.layers.len() > MAX_MASK_STATE_LAYERS {
            return Err(SidecarError::Invalid(format!(
                "mask state snapshot has {} layers (limit {MAX_MASK_STATE_LAYERS})",
                self.layers.len()
            )));
        }
        let mut ids = std::collections::BTreeSet::new();
        for layer in &self.layers {
            if layer.id.is_empty() {
                return Err(SidecarError::Invalid(
                    "mask state snapshot contains an empty layer id".into(),
                ));
            }
            if !ids.insert(&layer.id) {
                return Err(SidecarError::Invalid(format!(
                    "mask state snapshot contains duplicate layer id `{}`",
                    layer.id
                )));
            }
            validate_mask_layer_local_state(layer)?;
        }
        Ok(())
    }
}

/// Wire shape used only while deserializing a `MaskLayer`.  Keeping this
/// helper private lets the public struct retain ordinary construction and
/// serialization while the migration runs exactly once at the input boundary.
#[derive(Deserialize)]
struct MaskLayerWire {
    id: String,
    mask: MaskReference,
    inverted: bool,
    feather: f32,
    blur: f32,
    density: f32,
    #[serde(default = "mask_layer_visible_default")]
    visible: bool,
    #[serde(default)]
    local_adjustments: Option<LocalAdjustments>,
    #[serde(flatten, default)]
    extras: Extras,
}

fn mask_layer_visible_default() -> bool {
    true
}

impl<'de> Deserialize<'de> for MaskLayer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = MaskLayerWire::deserialize(deserializer)?;
        let mut layer = MaskLayer {
            id: wire.id,
            mask: wire.mask,
            inverted: wire.inverted,
            feather: wire.feather,
            blur: wire.blur,
            density: wire.density,
            visible: wire.visible,
            local_adjustments: wire.local_adjustments,
            extras: wire.extras,
        };
        normalize_legacy_layer_extras(&mut layer).map_err(serde::de::Error::custom)?;
        Ok(layer)
    }
}

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
        if !LOCAL_ADJUSTMENT_RANGES
            .iter()
            .any(|(known, _, _)| *known == name)
        {
            return Err(SidecarError::Invalid(format!(
                "mask layer `{}` has unknown legacy local adjustment `{name}`",
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
        if let Some(adjustments) = &layer.local_adjustments {
            adjustments.validate().map_err(|error| error.to_string())?;
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
        if let Some(adjustments) = self.local_adjustments {
            return Ok(Some(adjustments));
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
