//! The canonical mask-state digests and the additive history snapshot.
//!
//! Both live beside (not inside) the recipe type: the snapshot is a *history*
//! contract, not a local-adjustment contract, and it is versioned against the
//! same schema so a legacy snapshot is normalized at the input boundary instead
//! of being interpreted best-effort.

use super::{
    validate_mask_layer_local_state, LEGACY_LOCAL_ADJUSTMENTS_VERSIONS, LOCAL_ADJUSTMENTS_VERSION,
};
use crate::{MaskLayer, SidecarError};
use serde::{Deserialize, Deserializer, Serialize};

/// Maximum number of layer snapshots retained in one history entry.  The
/// active-copy validator has its own copy/layer limits; this smaller bound
/// keeps hostile history payloads from becoming unbounded allocations.
pub const MAX_MASK_STATE_LAYERS: usize = 4096;

/// Canonical digest for the complete ordered mask-layer state.  The vector is
/// intentionally serialized in persisted order: reordering overlapping layers
/// is an edit, not an equivalent state.
#[must_use]
pub fn mask_layers_digest(layers: &[MaskLayer]) -> String {
    let value = serde_json::to_value(layers).expect("MaskLayer is serializable");
    let bytes = serde_json::to_vec(&value).expect("canonical mask state is serializable");
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"lumina-mask-state-v2\0");
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(&bytes);
    format!("blake3:{}", hasher.finalize().to_hex())
}

/// A complete additive mask-state snapshot for a history entry.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MaskStateSnapshot {
    pub version: u8,
    pub layers: Vec<MaskLayer>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaskStateSnapshotWire {
    version: u8,
    layers: Vec<MaskLayer>,
}

impl<'de> Deserialize<'de> for MaskStateSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = MaskStateSnapshotWire::deserialize(deserializer)?;
        if wire.version != LOCAL_ADJUSTMENTS_VERSION
            && !LEGACY_LOCAL_ADJUSTMENTS_VERSIONS.contains(&wire.version)
        {
            return Err(serde::de::Error::custom(format!(
                "unsupported mask state snapshot version {} (expected one of {:?} or {LOCAL_ADJUSTMENTS_VERSION})",
                wire.version, LEGACY_LOCAL_ADJUSTMENTS_VERSIONS
            )));
        }
        // A legacy snapshot is normalized at the input boundary.  Its layers
        // are already migrated by MaskLayer's deserializer, and the fields an
        // older snapshot version cannot express stay absent.
        let snapshot = Self {
            version: LOCAL_ADJUSTMENTS_VERSION,
            layers: wire.layers,
        };
        snapshot
            .validate()
            .map_err(|error| serde::de::Error::custom(error.to_string()))?;
        Ok(snapshot)
    }
}

impl MaskStateSnapshot {
    #[must_use]
    pub fn new(layers: Vec<MaskLayer>) -> Self {
        Self {
            version: LOCAL_ADJUSTMENTS_VERSION,
            layers,
        }
    }

    /// Canonical digest for a complete history/Previous snapshot. It includes
    /// the snapshot version and persisted layer order, including the new WB
    /// delta fields.
    #[must_use]
    pub fn digest(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("MaskStateSnapshot is serializable");
        format!("blake3:{}", blake3::hash(&bytes).to_hex())
    }

    pub fn validate(&self) -> Result<(), SidecarError> {
        if self.version != LOCAL_ADJUSTMENTS_VERSION
            && !LEGACY_LOCAL_ADJUSTMENTS_VERSIONS.contains(&self.version)
        {
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
