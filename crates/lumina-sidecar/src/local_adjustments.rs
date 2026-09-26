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

mod color;
mod color_grading;
mod curves;
pub use color::{
    local_point_color_entry, LOCAL_GRADING_RANGES, LOCAL_HSL_FIELDS, LOCAL_POINT_COLOR_FIELDS,
};
pub(crate) mod mask_state;
mod migration;
mod presence;
pub use presence::neutral_local_presence;
mod wire;
pub use mask_state::{mask_layers_digest, MaskStateSnapshot};
use migration::normalize_legacy_layer_extras;
pub use migration::validate_mask_layer_local_state;

/// Current version of the typed local-adjustment object.
///
/// Version 1 is the P0 object (the four scalar local controls only). Version 2
/// adds the explicitly relative white-balance delta. Version 3
/// (`MASK-LOCAL-P1.2a`) adds the local tone curve, version 4
/// (`MASK-LOCAL-P1.2b`) adds the local per-pixel color block, and version 5
/// (`MASK-LOCAL-P1.2c`) adds the local presence block. Loading an older
/// version is an explicit, lossless migration: the fields that version cannot
/// express stay absent, and a payload that writes them is rejected instead of
/// dropped.
///
/// Because each release raises this constant, the "a version that cannot
/// express a block must not carry one" gates are anchored at the *introducing*
/// version, not at `LOCAL_ADJUSTMENTS_VERSION`: a v4 document keeps its own
/// colour block, a v3 document keeps its curve, and only `version < 5` is
/// refused a presence block.
pub const LOCAL_ADJUSTMENTS_VERSION: u8 = 5;
/// The P0-only typed version (four scalar controls).
pub const LEGACY_LOCAL_ADJUSTMENTS_VERSION: u8 = 1;
/// The P1.1 typed version (P0 scalars plus the relative white-balance delta).
pub const RELATIVE_WB_LOCAL_ADJUSTMENTS_VERSION: u8 = 2;
/// The P1.2a typed version (adds the local tone curve).
pub const CURVE_LOCAL_ADJUSTMENTS_VERSION: u8 = 3;
/// The P1.2b typed version (adds the local per-pixel colour block).
pub const COLOR_LOCAL_ADJUSTMENTS_VERSION: u8 = 4;
/// The P1.2c typed version (adds the local presence block).
pub const PRESENCE_LOCAL_ADJUSTMENTS_VERSION: u8 = 5;
/// Every typed version that may be read and migrated forward. A version
/// outside this list is a loud error, never a best-effort interpretation.
pub const LEGACY_LOCAL_ADJUSTMENTS_VERSIONS: [u8; 4] = [
    LEGACY_LOCAL_ADJUSTMENTS_VERSION,
    RELATIVE_WB_LOCAL_ADJUSTMENTS_VERSION,
    CURVE_LOCAL_ADJUSTMENTS_VERSION,
    COLOR_LOCAL_ADJUSTMENTS_VERSION,
];

/// Local scalar controls.  The P0 ranges intentionally mirror the global
/// raster controls, while the kernel order is owned by the core compositor.
/// The two WB entries are *relative deltas*, never absolute WB values; the two
/// vibrance/saturation entries are additive local colour scalars, never the
/// global `adjustments["vibrance"|"saturation"]` keys.
pub const LOCAL_ADJUSTMENT_RANGES: [(&str, f64, f64); 8] = [
    ("exposure", -10.0, 10.0),
    ("contrast", -1.0, 1.0),
    ("highlights", -1.0, 1.0),
    ("shadows", -1.0, 1.0),
    ("temperature_delta_k", -5000.0, 5000.0),
    ("tint_delta", -1.0, 1.0),
    ("vibrance", -1.0, 1.0),
    ("saturation", -1.0, 1.0),
];

/// Named range aliases for callers that need to render/document the local WB
/// contract without duplicating magic numbers.
pub const LOCAL_WB_TEMPERATURE_DELTA_RANGE: (f64, f64) = (-5000.0, 5000.0);
pub const LOCAL_WB_TINT_DELTA_RANGE: (f64, f64) = (-1.0, 1.0);

/// A typed, versioned local recipe.  The P0 scalar fields, the P1.1 relative
/// WB delta and the P1.2a tone curve are retained; version 4 adds the local
/// per-pixel color block and version 5 (`MASK-LOCAL-P1.2c`) the local presence
/// block.  In particular, this type has no absolute `wb_temperature`/`wb_tint`
/// fields: those names belong to the global recipe and are rejected here rather
/// than being ambiguous aliases.  There is deliberately still no local detail,
/// AI-denoise, noise-reduction, sharpening or optics field — those stay
/// disabled.  Optics stays disabled *permanently*: lens correction and
/// perspective are geometric stages ahead of the masks, not a per-mask
/// per-pixel tone stage.  Noise reduction and AI-denoise stay disabled until
/// the F-078 model gate (weight licence, provenance, hash pin) clears.
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
    /// MASK-LOCAL-P1.2b local HSL block, reusing the global `HslAdjustments`
    /// type and the shared band rules. `None` and an all-zero block are both
    /// pixel-neutral.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hsl: Option<crate::HslAdjustments>,
    /// MASK-LOCAL-P1.2b local Point Color block, reusing the global
    /// `PointColor` type, its stable entry ids and its 8-entry limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point_color: Option<crate::PointColor>,
    /// MASK-LOCAL-P1.2b local Color Grading block, reusing the global
    /// `ColorGrading` type including `balance` and `blending`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_grading: Option<crate::ColorGrading>,
    /// MASK-LOCAL-P1.2b additive local vibrance (`-1..=1`, `0` = neutral).
    /// This is a local scalar, never the global `adjustments["vibrance"]` key.
    pub vibrance: f64,
    /// MASK-LOCAL-P1.2b additive local saturation (`-1..=1`, `0` = neutral).
    pub saturation: f64,
    /// MASK-LOCAL-P1.2c local presence block, reusing the global `Presence`
    /// type and the shared presence validator. `None` and an all-zero block
    /// are both pixel-neutral *and* byte-identical to the pre-P1.2c local
    /// kernel path, so a neutral block never costs a byte change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence: Option<crate::Presence>,
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
            hsl: None,
            point_color: None,
            color_grading: None,
            vibrance: 0.0,
            saturation: 0.0,
            presence: None,
        }
    }
}

/// Stable human-readable representation used by CLI status output.
///
/// The documented grammar is `v5 exposure=<number> contrast=<number>
/// highlights=<number> shadows=<number> temperature_delta_k=<number>
/// tint_delta=<number> curves=<summary> hsl=<summary> point_color=<summary>
/// color_grading=<summary> vibrance=<number> saturation=<number>
/// presence=<summary>`, always in that field order. The `curves` summary is
/// `curves=none` for a neutral block, otherwise
/// `curves=<channel>:<point-count>[,...]` in the canonical
/// master/red/green/blue order, listing only stored channels. The color
/// summaries use the same `none` convention, and so does `presence`. It is a
/// presentation contract independent of the derived `Debug` layout; JSON
/// consumers should continue to use the structured object instead.
impl fmt::Display for LocalAdjustments {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "v{} exposure={} contrast={} highlights={} shadows={} temperature_delta_k={} tint_delta={} curves={} hsl={} point_color={} color_grading={} vibrance={} saturation={} presence={}",
            self.version,
            self.exposure,
            self.contrast,
            self.highlights,
            self.shadows,
            self.temperature_delta_k,
            self.tint_delta,
            self.curve_summary(),
            self.hsl_summary(),
            self.point_color_summary(),
            self.color_grading_summary(),
            self.vibrance,
            self.saturation,
            self.presence_summary(),
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
        // hand-constructed object can never disagree with a loaded one. The
        // boundary is "older than the version that introduced the curve", so
        // the version that *does* own the curve still round-trips.
        if self.version < CURVE_LOCAL_ADJUSTMENTS_VERSION && self.curves.is_some() {
            return Err(SidecarError::Invalid(format!(
                "local_adjustments version {} cannot contain a tone curve",
                self.version
            )));
        }
        // Same rule for the P1.2b color block: a version older than the one
        // that introduced it must not carry it, not even a neutral one, so no
        // surface can smuggle a later field into an older version. The gate is
        // anchored at `COLOR_LOCAL_ADJUSTMENTS_VERSION`, *not* at
        // `LOCAL_ADJUSTMENTS_VERSION`: raising the current version to 5 for
        // P1.2c must not make a v4 document lose its own colour block.
        if self.version < COLOR_LOCAL_ADJUSTMENTS_VERSION {
            if self.hsl.is_some() {
                return Err(SidecarError::Invalid(format!(
                    "local_adjustments version {} cannot contain a local HSL block",
                    self.version
                )));
            }
            if self.point_color.is_some() {
                return Err(SidecarError::Invalid(format!(
                    "local_adjustments version {} cannot contain a local point color block",
                    self.version
                )));
            }
            if self.color_grading.is_some() {
                return Err(SidecarError::Invalid(format!(
                    "local_adjustments version {} cannot contain a local color grading block",
                    self.version
                )));
            }
            if self.vibrance != 0.0 || self.saturation != 0.0 {
                return Err(SidecarError::Invalid(format!(
                    "local_adjustments version {} cannot contain local vibrance or saturation",
                    self.version
                )));
            }
        }
        // And the same rule for the P1.2c presence block. A v1..v4 object must
        // not carry `presence` — not even a neutral one, and not an explicit
        // `null` on the wire (that is refused by the decoder, which keeps raw
        // key presence).
        if self.version < PRESENCE_LOCAL_ADJUSTMENTS_VERSION && self.presence.is_some() {
            return Err(SidecarError::Invalid(format!(
                "local_adjustments version {} cannot contain a local presence block",
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
            ("vibrance", self.vibrance),
            ("saturation", self.saturation),
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
        // The color blocks are validated by exactly the validators the global
        // recipe uses, so a local block can never accept a value the global
        // pipeline would reject (or the other way round).
        if let Some(hsl) = &self.hsl {
            crate::validate_hsl(hsl)
                .map_err(|error| SidecarError::Invalid(format!("local hsl: {error}")))?;
        }
        if let Some(point_color) = &self.point_color {
            crate::validate_point_color(point_color)
                .map_err(|error| SidecarError::Invalid(format!("local point color: {error}")))?;
        }
        if let Some(grading) = &self.color_grading {
            crate::validate_color_grading(grading)
                .map_err(|error| SidecarError::Invalid(format!("local color grading: {error}")))?;
        }
        // Same validator as the global recipe: a local presence can never
        // accept a value the global pipeline would reject, or vice versa.
        if let Some(presence) = &self.presence {
            crate::validate_presence(presence)
                .map_err(|error| SidecarError::Invalid(format!("local presence: {error}")))?;
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
            && self.vibrance == 0.0
            && self.saturation == 0.0
            && self.curves.as_ref().is_none_or(Curves::is_identity)
            && !self.has_local_color()
            && !self.has_local_presence()
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
            "vibrance" => Some(self.vibrance),
            "saturation" => Some(self.saturation),
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
            "vibrance" => self.vibrance = value,
            "saturation" => self.saturation = value,
            _ => unreachable!("validated local adjustment key"),
        }
        Ok(())
    }

    /// Build a minimal global-recipe-shaped object for the shared core kernel.
    /// The core compositor uses this rather than reimplementing exposure,
    /// contrast, shadows or highlights arithmetic.  Relative WB is deliberately
    /// absent: it is applied by the core's post-global local WB pass. The local
    /// curve and color blocks are absent for the same reason — they are applied
    /// by the core's own float chain, and putting them into the *global* recipe
    /// shape would be exactly the global mutation this type must never cause.
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
