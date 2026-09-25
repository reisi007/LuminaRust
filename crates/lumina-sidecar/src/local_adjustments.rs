//! Versioned local mask recipes (`MASK-LOCAL-P0/P1.1`).
//!
//! This module is deliberately small and sidecar-only. It owns the typed P0
//! controls, the P1.1 relative-WB delta, the loud migrations from v1 and the
//! historical flattened `adjustment_*` entries, and the canonical state digest
//! used by render/cache identities. Pixel evaluation lives in `lumina-core`;
//! keeping schema and migration together prevents GUI, CLI and core from
//! inventing subtly different interpretations of the same layer.

use super::{Curves, Extras, MaskLayer, MaskReference};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::fmt;

use crate::{EditRecipe, SidecarError};

mod curves;
mod migration;
mod wire;
use migration::normalize_legacy_layer_extras;
pub use migration::validate_mask_layer_local_state;

/// Current version of the typed local-adjustment object.
///
/// Version 1 is the P0 object (the four scalar local controls only). Version 2
/// adds the explicitly relative white-balance delta. Version 3
/// (`MASK-LOCAL-P1.2a`) adds the local tone curve. Loading an older version is
/// an explicit, lossless migration: the fields that version cannot express stay
/// absent, and a payload that writes them is rejected instead of dropped.
pub const LOCAL_ADJUSTMENTS_VERSION: u8 = 3;
/// The P0-only typed version (four scalar controls).
pub const LEGACY_LOCAL_ADJUSTMENTS_VERSION: u8 = 1;
/// The P1.1 typed version (P0 scalars plus the relative white-balance delta).
pub const RELATIVE_WB_LOCAL_ADJUSTMENTS_VERSION: u8 = 2;
/// Every typed version that may be read and migrated forward. A version
/// outside this list is a loud error, never a best-effort interpretation.
pub const LEGACY_LOCAL_ADJUSTMENTS_VERSIONS: [u8; 2] = [
    LEGACY_LOCAL_ADJUSTMENTS_VERSION,
    RELATIVE_WB_LOCAL_ADJUSTMENTS_VERSION,
];

/// Maximum number of layer snapshots retained in one history entry.  The
/// active-copy validator has its own copy/layer limits; this smaller bound
/// keeps hostile history payloads from becoming unbounded allocations.
pub const MAX_MASK_STATE_LAYERS: usize = 4096;

/// Local scalar controls.  The P0 ranges intentionally mirror the global
/// raster controls, while the kernel order is owned by the core compositor.
/// The two WB entries are *relative deltas*, never absolute WB values.
pub const LOCAL_ADJUSTMENT_RANGES: [(&str, f64, f64); 6] = [
    ("exposure", -10.0, 10.0),
    ("contrast", -1.0, 1.0),
    ("highlights", -1.0, 1.0),
    ("shadows", -1.0, 1.0),
    ("temperature_delta_k", -5000.0, 5000.0),
    ("tint_delta", -1.0, 1.0),
];

/// Named range aliases for callers that need to render/document the local WB
/// contract without duplicating magic numbers.
pub const LOCAL_WB_TEMPERATURE_DELTA_RANGE: (f64, f64) = (-5000.0, 5000.0);
pub const LOCAL_WB_TINT_DELTA_RANGE: (f64, f64) = (-1.0, 1.0);

/// A typed, versioned local recipe.  The P0 scalar fields and the P1.1
/// relative WB delta are retained; version 3 adds only the local tone curve.
/// In particular, this type has no absolute `wb_temperature`/`wb_tint` fields:
/// those names belong to the global recipe and are rejected here rather than
/// being ambiguous aliases.  There is deliberately no local HSL, point-color,
/// grading, presence or detail field either — those stay disabled.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LocalAdjustments {
    pub version: u8,
    pub exposure: f64,
    pub contrast: f64,
    pub highlights: f64,
    pub shadows: f64,
    pub temperature_delta_k: f64,
    pub tint_delta: f64,
    /// MASK-LOCAL-P1.2a local tone curve (master plus optional RGB channels),
    /// reusing the global `Curves` types and point rules. `None` and a
    /// persisted identity are both pixel-neutral; the option distinguishes
    /// "no curve" from "an explicitly stored identity curve" for history and
    /// digest purposes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curves: Option<Curves>,
}

/// The task-facing name for the typed local recipe.  Keep the P0 name as an
/// alias so existing sidecar/GUI/CLI integrations remain source compatible.
pub type MaskLocalRecipe = LocalAdjustments;

impl Default for LocalAdjustments {
    fn default() -> Self {
        Self {
            version: LOCAL_ADJUSTMENTS_VERSION,
            exposure: 0.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            temperature_delta_k: 0.0,
            tint_delta: 0.0,
            curves: None,
        }
    }
}

/// Stable human-readable representation used by CLI status output.
///
/// The documented grammar is `v3 exposure=<number> contrast=<number>
/// highlights=<number> shadows=<number> temperature_delta_k=<number>
/// tint_delta=<number> curves=<summary>`, always in that field order. The
/// curve summary is `curves=none` for a neutral block, otherwise
/// `curves=<channel>:<point-count>[,...]` in the canonical
/// master/red/green/blue order, listing only stored channels. It is a
/// presentation contract independent of the derived `Debug` layout; JSON
/// consumers should continue to use the structured object instead.
impl fmt::Display for LocalAdjustments {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "v{} exposure={} contrast={} highlights={} shadows={} temperature_delta_k={} tint_delta={} curves={}",
            self.version,
            self.exposure,
            self.contrast,
            self.highlights,
            self.shadows,
            self.temperature_delta_k,
            self.tint_delta,
            self.curve_summary(),
        )
    }
}

impl LocalAdjustments {
    /// Validate the typed object without clipping or defaulting any value.
    pub fn validate(&self) -> Result<(), SidecarError> {
        if self.version != LOCAL_ADJUSTMENTS_VERSION
            && !LEGACY_LOCAL_ADJUSTMENTS_VERSIONS.contains(&self.version)
        {
            return Err(SidecarError::Invalid(format!(
                "unsupported local_adjustments version {} (expected one of {:?} or {LOCAL_ADJUSTMENTS_VERSION})",
                self.version, LEGACY_LOCAL_ADJUSTMENTS_VERSIONS
            )));
        }
        if self.version == LEGACY_LOCAL_ADJUSTMENTS_VERSION
            && (self.temperature_delta_k != 0.0 || self.tint_delta != 0.0)
        {
            return Err(SidecarError::Invalid(
                "local_adjustments version 1 cannot contain relative white-balance delta fields"
                    .into(),
            ));
        }
        // Mirrors the wire decoder exactly: a version that cannot express a
        // curve must not carry one, not even an identity one, so a
        // hand-constructed object can never disagree with a loaded one.
        if self.version != LOCAL_ADJUSTMENTS_VERSION && self.curves.is_some() {
            return Err(SidecarError::Invalid(format!(
                "local_adjustments version {} cannot contain a tone curve",
                self.version
            )));
        }
        for (name, value) in [
            ("exposure", self.exposure),
            ("contrast", self.contrast),
            ("highlights", self.highlights),
            ("shadows", self.shadows),
            ("temperature_delta_k", self.temperature_delta_k),
            ("tint_delta", self.tint_delta),
        ] {
            let (_, minimum, maximum) = LOCAL_ADJUSTMENT_RANGES
                .iter()
                .find(|(key, _, _)| *key == name)
                .expect("local adjustment range is statically defined");
            if !value.is_finite() || !(*minimum..=*maximum).contains(&value) {
                return Err(SidecarError::Invalid(format!(
                    "local adjustment `{name}` must be finite and in {minimum}..={maximum}, got {value}"
                )));
            }
        }
        if let Some(curves) = &self.curves {
            crate::validate_curves(curves)
                .map_err(|error| SidecarError::Invalid(format!("local tone curve: {error}")))?;
        }
        Ok(())
    }

    fn normalized_version(mut self) -> Result<Self, String> {
        self.validate().map_err(|error| error.to_string())?;
        self.version = LOCAL_ADJUSTMENTS_VERSION;
        Ok(self)
    }

    /// True when applying this object cannot change a pixel.
    #[must_use]
    pub fn is_neutral(&self) -> bool {
        self.exposure == 0.0
            && self.contrast == 0.0
            && self.highlights == 0.0
            && self.shadows == 0.0
            && self.temperature_delta_k == 0.0
            && self.tint_delta == 0.0
            && self.curves.as_ref().is_none_or(Curves::is_identity)
    }

    /// Return one scalar value by its stable local key.
    #[must_use]
    pub fn value(&self, key: &str) -> Option<f64> {
        match key {
            "exposure" => Some(self.exposure),
            "contrast" => Some(self.contrast),
            "highlights" => Some(self.highlights),
            "shadows" => Some(self.shadows),
            "temperature_delta_k" => Some(self.temperature_delta_k),
            "tint_delta" => Some(self.tint_delta),
            _ => None,
        }
    }

    /// Set one scalar value by its stable local key.  Unknown keys and invalid
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
            "temperature_delta_k" => self.temperature_delta_k = value,
            "tint_delta" => self.tint_delta = value,
            _ => unreachable!("validated local adjustment key"),
        }
        Ok(())
    }

    /// Build a minimal global-recipe-shaped object for the shared core kernel.
    /// The core compositor uses this rather than reimplementing exposure,
    /// contrast, shadows or highlights arithmetic.  Relative WB is deliberately
    /// absent: it is applied by the core's post-global local WB pass.
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

    /// Deterministic float gains for the relative local WB delta.  This is
    /// deliberately derived from the delta alone, never from the global
    /// absolute temperature/tint recipe fields.
    #[must_use]
    pub fn relative_white_balance_gains(&self) -> [f64; 3] {
        let warmth = self.temperature_delta_k / 5500.0;
        [
            1.0 - warmth * 0.35,
            1.0 - self.tint_delta * 0.20,
            1.0 + warmth * 0.35,
        ]
    }

    /// Canonical digest of this typed object.  JSON object key ordering and
    /// the explicit version are part of the identity; no map iteration or
    /// display formatting is involved.  The optional tone-curve block is part
    /// of the serialized object, so a local curve edit can never reuse stale
    /// pixels.
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
