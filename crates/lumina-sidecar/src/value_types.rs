//! SPLICE-SIDECAR-LIB-90 (Slice 10): the value-type block of the sidecar root
//! file, moved verbatim into its own module.
//!
//! One responsibility: the persisted recipe vocabulary — capture/decode
//! fingerprints, source actions, the mask/brush/generative prompt types, the
//! `EditRecipe` itself (with its hand-written `Serialize`/`Deserialize`), and
//! the colour, presence, geometry, lens, effects and crop blocks. These are the
//! types the core renderer and every front end share.
//!
//! They deliberately stay in **one** module: `EditRecipe`'s field-accessing
//! impls (`impl Serialize for EditRecipe`, `impl Deserialize for EditRecipe`,
//! `impl Default for EditRecipe` and the two inherent `impl EditRecipe`
//! blocks) all touch the struct's fields, so splitting them across modules
//! would force every field to become `pub(crate)`. One module keeps the fields
//! private and the type cohesive.
//!
//! The public symbols keep their names and stay reachable under the same
//! paths — the root re-exports them, so `lumina_sidecar::EditRecipe`,
//! `lumina_sidecar::Curves`, `lumina_sidecar::Geometry`,
//! `lumina_sidecar::DecodeFingerprint`, … stay resolvable. The crate-internal
//! `default_color_grading_blending` helper is promoted from private to
//! `pub(crate)` (its only change) and re-exported by the root, because the
//! mask-local colour-grading block calls it as
//! `crate::default_color_grading_blending()`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

// The root's private modules and its own types. A child module does not
// inherit the parent's item names unqualified, so they are imported here; a
// child module may import the parent's private items, not the other way round.
// `color_blocks` and `spot` are private modules of the root and stay private.
use crate::color_blocks;
use crate::spot;
use crate::ArtifactStatus;
use crate::AutoFeatures;
use crate::DenoiseAi;
use crate::Extras;
use crate::LocalAdjustments;
use crate::NoiseReduction;
use crate::Sharpening;
use crate::SidecarError;
use crate::SourceActionArtifactRef;
// `invalid` lives in the crate root; a child module does not inherit the
// parent's item names unqualified, so it is imported here. Its visibility is
// unchanged (`pub(crate)` in the root).
use crate::invalid;
// Referenced only by the intra-doc links of the moved block; a child module
// does not inherit the parent's re-exports, so the imports keep those links
// resolvable (`[`artifact_status`]`, `[`generative_artifact_status`]`).
#[allow(unused_imports)]
use crate::artifact_status;
#[allow(unused_imports)]
use crate::generative_artifact_status;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecodeFingerprint {
    pub decoder: String,
    pub version: String,
    pub parameters: BTreeMap<String, String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeometryFingerprint {
    pub width: u32,
    pub height: u32,
    pub orientation: u8,
    pub pixel_aspect_ratio: f32,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisFingerprint {
    pub algorithm: String,
    pub version: String,
    pub input_fingerprint: String,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceFingerprint {
    pub content_hash: String,
    pub byte_length: u64,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceIdentity {
    pub relative_name: String,
    pub content_hash: String,
    pub byte_length: u64,
    pub modified_at: Option<String>,
    pub raw_format: String,
    pub orientation: u8,
    pub decode_fingerprint: DecodeFingerprint,
    pub geometry_fingerprint: GeometryFingerprint,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Photo {
    pub source: SourceIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_fingerprint: Option<AnalysisFingerprint>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactReference {
    pub relative_path: String,
    pub format: String,
    pub checksum: String,
    pub width: u32,
    pub height: u32,
    pub channels: String,
    pub data_version: String,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

/// F-042-N1: the kind of a persisted source action. Mirrors the core
/// `SourceAction` enum (`DustRemoval` | `AiReplacement`) at the recipe level so
/// the persisted spec and the runtime artifact stay aligned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceActionKind {
    DustRemoval,
    AiReplacement,
}

/// GEN-ZDATA-LINK-1: link from a recipe to a generative RGBA8 record inside
/// the sidecar's `.lumina.zdata` bundle.
///
/// `id` is the record id inside the bundle (`kind = 2 generative_canvas` or
/// `kind = 3 spot_heal_generative`); the remaining fields are the portable
/// `ArtifactReference` payload (relative path, format, BLAKE3 checksum over
/// the uncompressed RGBA8 stream, resolution, channel type, data version).
/// Absolute paths are forbidden; the bundle stays valid when moved as a whole.
///
/// Expected producer values: `format = "lumina-zdata"` (contains `zdata`, so
/// [`artifact_status`] deep-verifies magic-bearing bundles instead of
/// treating a mislabeled file as opaque), `channels = "rgba8"`,
/// `data_version = "1"` (RGBA encoding version 1, see `zdata` module).
/// Deviations are rejected loudly — never silently reinterpreted.
///
/// Recipe-identity note (core follow-up, not implemented here): every field
/// of this link (id, checksum, dimensions, format/channels/data_version) is
/// part of the recipe identity and MUST be included in the core
/// `recipe_hash`/`RenderKey`. Any change invalidates preview/export from the
/// generative stage on; a missing/corrupt bundle is reported visibly
/// (`missing`/`corrupt`), never silently re-generated or skipped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerativeArtifactRef {
    pub id: String,
    pub relative_path: String,
    pub format: String,
    pub checksum: String,
    pub width: u32,
    pub height: u32,
    pub channels: String,
    pub data_version: String,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

/// GEN-EXPAND-CACHE-1: `extras` key of the generative identity digest stored on
/// a [`GenerativeArtifactRef`]. Additive (absent = legacy/unverifiable), no
/// schema bump.
pub const GENERATIVE_IDENTITY_KEY: &str = "generative_identity";

impl GenerativeArtifactRef {
    /// View this link as a plain [`ArtifactReference`] so [`artifact_status`]
    /// (including its eager `Available`/`Missing`/`Corrupt` verification)
    /// applies unchanged to generative links.
    pub fn as_artifact_reference(&self) -> ArtifactReference {
        ArtifactReference {
            relative_path: self.relative_path.clone(),
            format: self.format.clone(),
            checksum: self.checksum.clone(),
            width: self.width,
            height: self.height,
            channels: self.channels.clone(),
            data_version: self.data_version.clone(),
            extras: self.extras.clone(),
        }
    }

    /// GEN-EXPAND-CACHE-1: attaches the generative identity digest (the exact
    /// `GenerativeCacheKey::digest()` of the run that produced this canvas) so a
    /// later render can prove the persisted record still matches the current
    /// source/recipe/seed/canvas. Stored in the additive `extras` map — no
    /// schema bump, unknown-field roundtrip preserved.
    #[must_use]
    pub fn with_identity(mut self, identity: impl Into<String>) -> Self {
        self.extras.insert(
            GENERATIVE_IDENTITY_KEY.to_owned(),
            Value::String(identity.into()),
        );
        self
    }

    /// The stored identity digest, or `None` for a legacy link written before
    /// identity pinning. A missing identity is treated as unverifiable/stale by
    /// [`generative_artifact_status`] when a current identity is expected —
    /// never silently accepted.
    #[must_use]
    pub fn identity(&self) -> Option<&str> {
        self.extras.get(GENERATIVE_IDENTITY_KEY)?.as_str()
    }

    /// Eager bundle status for this link: `Available` only if the referenced
    /// bundle file exists and (for `LUMZDATA` containers, with the `zdata`
    /// feature on native) parses with intact BLAKE3 checksums; `Missing` when
    /// absent, `Corrupt` when present but unusable. Callers must treat any
    /// non-`Available` status as visible, never as a silent fallback.
    pub fn artifact_status(&self, bundle_root: &Path) -> ArtifactStatus {
        spot::verified_generative_artifact_status(self, bundle_root)
    }

    /// GEN-ONNX-1: build the portable recipe link for a persisted
    /// `generative_canvas` record.
    ///
    /// `relative_path` is the bundle path relative to the sidecar directory
    /// (never absolute). The identity digest (the exact operation identity, e.g.
    /// the core `GenerativeCacheKey::digest()`) is attached via
    /// [`Self::with_identity`], so a later render can prove availability
    /// instead of re-generating.
    #[cfg(feature = "zdata")]
    #[must_use]
    pub fn from_generative_canvas(
        canvas: &crate::GenerativeCanvasArtifact,
        relative_path: impl Into<String>,
        identity: impl Into<String>,
    ) -> Self {
        Self {
            id: canvas.id.clone(),
            relative_path: relative_path.into(),
            format: "lumina-zdata".into(),
            checksum: canvas.checksum(),
            width: canvas.width,
            height: canvas.height,
            channels: "rgba8".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        }
        .with_identity(identity)
    }
}

/// GEN-ZDATA-LINK-1: current schema version for a persisted spot-removal
/// spec. Independent of `SCHEMA_VERSION`; an unknown `version` is rejected
/// during validation rather than silently ignored.
pub const SPOT_REMOVAL_VERSION: u8 = 1;

/// LRPAR-G04-REMOVE: extras key of the optional visualize-spots threshold
/// (`f32` in `0..=1`, absent = visualization off). Part of the recipe
/// identity.
pub const SPOT_VISUALIZE_KEY: &str = "spot_visualize_threshold";

/// LRPAR-G04-REMOVE: extras key of the explicit distraction switches
/// (see [`SpotDistraction`], all default off). Absent = all off.
pub const SPOT_DISTRACTION_KEY: &str = "spot_distraction";

/// LRPAR-G01-BASIC: extras key of the Develop treatment (`"color"` or
/// `"bw"`). Absent = `"color"` (default). Additive, no migration.
pub const TREATMENT_KEY: &str = "treatment";
/// LRPAR-G01-BASIC: the two normative treatment values.
pub const TREATMENT_COLOR: &str = "color";
pub const TREATMENT_BW: &str = "bw";
/// LRPAR-G01-BASIC: extras key stashing the pre-B&W `saturation`/`vibrance`
/// values (`{saturation: f64|null, vibrance: f64|null}`; null = key was
/// unset). Managed solely by [`EditRecipe::apply_treatment`].
pub const BW_STASH_KEY: &str = "bw_stash";
/// LRPAR-G01-BASIC: options key of the Develop profile (named look).
/// Absent = [`DEFAULT_DEVELOP_PROFILE`]. Additive, no migration.
pub const DEVELOP_PROFILE_KEY: &str = "profile";
/// LRPAR-G01-BASIC: normative Develop-profile whitelist. MVP renders every
/// known profile identically (persisted selection intent, see
/// `feature/architecture/pipeline.md` § G-01); unknown names are rejected
/// loudly, never silently normalised.
pub const DEVELOP_PROFILES: &[&str] = &[
    "default",
    "neutral",
    "vivid",
    "portrait",
    "landscape",
    "monochrome",
];
/// LRPAR-G01-BASIC: default Develop profile (also the absent-key meaning).
pub const DEFAULT_DEVELOP_PROFILE: &str = "default";

/// SPOT-REMOVE-1: the mode of a persisted spot removal. `Heuristic` is the
/// instant CPU heal (recipe parameters only, no model, no bundle record);
/// `Generative` is the local ONNX inpaint whose replaced tile lives in the
/// `.lumina.zdata` bundle as a `kind = 3 spot_heal_generative` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpotRemovalMode {
    Heuristic,
    Generative,
}

/// SPOT-REMOVE-1: a persisted spot-removal recipe operation. Additive
/// schema-v2 field (see `EditRecipe::spot_removals`): an absent
/// `spot_removals` key deserializes as the empty list, requires no migration
/// and does not change `schema_version`.
///
/// R5-DUST-23-FOLLOWUP: every operation has a stable `id`, including a
/// typed-only generative operation which has no geometry view. Older
/// documents without that field are assigned a deterministic compatibility ID
/// while loading; explicit IDs remain untouched.
///
/// SPOT-SCHEMA-GEOMETRY: this typed view intentionally carries only
/// id/version/mode/artifact. The heal geometry (center/radius/feather/offset/
/// opacity/status) travels in the mirrored `extras["spot_removals"]` view
/// (see `EditRecipe`'s `Deserialize` impl) and is validated by
/// `validate_spot_removal_extras`. Extending this struct with geometry fields
/// was rejected: it would break every existing struct literal in downstream
/// crates without restoring the extras roundtrip the GUI detector relies on.
///
/// Exclusion rules (validated loudly): a `Heuristic` spot MUST NOT carry an
/// `artifact` (it has no bundle record); a `Generative` spot carries the
/// `kind = 3` record link after generation (`None` before generation means
/// `missing`, never a silent heuristic fallback).
///
/// Recipe-identity note: `id`, `mode` and, for generative spots, every field of
/// `artifact` are included in the core `recipe_hash`/`RenderKey`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpotRemoval {
    #[serde(default)]
    pub id: String,
    pub version: u8,
    pub mode: SpotRemovalMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<GenerativeArtifactRef>,
}

/// LRPAR-G04-REMOVE: explicit distraction-removal switches, persisted as
/// recipe `extras["spot_distraction"]`. Every switch defaults to off;
/// `auto_mode` alone lists candidates and never applies anything silently.
/// Additive (absent key = all off, no migration, no schema bump).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SpotDistraction {
    #[serde(default)]
    pub reflections: bool,
    #[serde(default)]
    pub people: bool,
    #[serde(default)]
    pub dust: bool,
    #[serde(default)]
    pub auto_mode: bool,
}

impl EditRecipe {
    /// Optional visualize threshold (`0..=1`), `None` when off/absent.
    /// A present but unparsable value reads as `None` here and fails loudly
    /// in validation instead (never a silent reinterpretation).
    pub fn spot_visualize_threshold(&self) -> Option<f32> {
        self.extras
            .get(SPOT_VISUALIZE_KEY)?
            .as_f64()
            .map(|v| v as f32)
    }

    /// Sets (`Some`) or clears (`None`) the visualize threshold. Out-of-range
    /// or non-finite values fail loudly; clearing removes the key.
    pub fn set_spot_visualize_threshold(&mut self, value: Option<f32>) -> Result<(), SidecarError> {
        match value {
            None => {
                self.extras.remove(SPOT_VISUALIZE_KEY);
                Ok(())
            }
            Some(v) => {
                if !v.is_finite() || !(0.0..=1.0).contains(&v) {
                    return invalid(format!(
                        "extras `{SPOT_VISUALIZE_KEY}` must be finite within 0..=1"
                    ));
                }
                self.extras
                    .insert(SPOT_VISUALIZE_KEY.into(), Value::from(f64::from(v)));
                Ok(())
            }
        }
    }

    /// Explicit distraction switches; absent reads as all-off.
    pub fn spot_distraction(&self) -> SpotDistraction {
        self.extras
            .get(SPOT_DISTRACTION_KEY)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    /// Persists the distraction switches (all-off removes the key so legacy
    /// documents stay byte-stable).
    pub fn set_spot_distraction(&mut self, setting: SpotDistraction) {
        if setting == SpotDistraction::default() {
            self.extras.remove(SPOT_DISTRACTION_KEY);
        } else if let Ok(value) = serde_json::to_value(setting) {
            self.extras.insert(SPOT_DISTRACTION_KEY.into(), value);
        }
    }

    /// LRPAR-G01-BASIC: current Develop treatment (`"color"` or `"bw"`).
    /// Absent or unparsable reads as `"color"` here; validation fails loudly
    /// instead (never a silent reinterpretation).
    pub fn treatment(&self) -> &str {
        self.extras
            .get(TREATMENT_KEY)
            .and_then(|v| v.as_str())
            .filter(|t| *t == TREATMENT_COLOR || *t == TREATMENT_BW)
            .unwrap_or(TREATMENT_COLOR)
    }

    /// LRPAR-G01-BASIC: current Develop profile. Absent or unknown reads as
    /// [`DEFAULT_DEVELOP_PROFILE`] here; validation fails loudly instead.
    pub fn develop_profile(&self) -> &str {
        self.options
            .get(DEVELOP_PROFILE_KEY)
            .map(String::as_str)
            .filter(|p| DEVELOP_PROFILES.contains(p))
            .unwrap_or(DEFAULT_DEVELOP_PROFILE)
    }

    /// LRPAR-G01-BASIC: single mutation path for the Treatment selector
    /// (shared by the GUI `V` toggle and `lumina develop --treatment`).
    /// `"bw"` stashes the current `saturation`/`vibrance` (including absence)
    /// in `extras["bw_stash"]` and sets both to `-1.0` through the shared
    /// pipeline stage (no caller-side pixel logic). `"color"` removes the
    /// marker and restores the stashed values exactly (unset keys are removed
    /// again, never left at `-1`; a missing/corrupt stash falls back to
    /// identity with the marker still removed). Anything else fails loudly.
    /// Returns whether the recipe changed.
    pub fn apply_treatment(&mut self, treatment: &str) -> Result<bool, SidecarError> {
        match treatment {
            TREATMENT_BW => {
                if self.treatment() == TREATMENT_BW {
                    return Ok(false);
                }
                let mut stash = BTreeMap::new();
                for key in ["saturation", "vibrance"] {
                    stash.insert(key.to_string(), self.adjustments.get(key).copied());
                }
                self.extras.insert(
                    BW_STASH_KEY.into(),
                    serde_json::to_value(&stash).map_err(|e| {
                        SidecarError::Invalid(format!("cannot encode bw stash: {e}"))
                    })?,
                );
                self.extras
                    .insert(TREATMENT_KEY.into(), Value::String(TREATMENT_BW.into()));
                self.adjustments.insert("saturation".into(), -1.0);
                self.adjustments.insert("vibrance".into(), -1.0);
                Ok(true)
            }
            TREATMENT_COLOR => {
                if self.treatment() == TREATMENT_COLOR && !self.extras.contains_key(TREATMENT_KEY) {
                    return Ok(false);
                }
                let stash = self.extras.remove(BW_STASH_KEY);
                self.extras.remove(TREATMENT_KEY);
                match stash
                    .and_then(|v| serde_json::from_value::<BTreeMap<String, Option<f64>>>(v).ok())
                {
                    Some(map) => {
                        for key in ["saturation", "vibrance"] {
                            match map.get(key).copied().flatten() {
                                Some(value) => {
                                    self.adjustments.insert(key.into(), value);
                                }
                                None => {
                                    self.adjustments.remove(key);
                                }
                            }
                        }
                    }
                    None => {
                        // No (or corrupt) stash: `-1` values must not linger
                        // silently — drop both keys so the recipe is identity.
                        self.adjustments.remove("saturation");
                        self.adjustments.remove("vibrance");
                    }
                }
                Ok(true)
            }
            other => Err(SidecarError::Invalid(format!(
                "unknown treatment `{other}` (expected `color` or `bw`)"
            ))),
        }
    }

    /// LRPAR-G01-BASIC: single mutation path for the Profile dropdown
    /// (shared by GUI and `lumina develop --profile`). `"default"` removes
    /// the key (absent = default, legacy documents stay byte-stable); known
    /// non-default names are stored; anything else (empty included) fails
    /// loudly. Returns whether the recipe changed.
    pub fn apply_develop_profile(&mut self, profile: &str) -> Result<bool, SidecarError> {
        if !DEVELOP_PROFILES.contains(&profile) {
            return Err(SidecarError::Invalid(format!(
                "unknown develop profile `{profile}` (expected one of {})",
                DEVELOP_PROFILES.join("|")
            )));
        }
        if profile == DEFAULT_DEVELOP_PROFILE {
            return Ok(self.options.remove(DEVELOP_PROFILE_KEY).is_some());
        }
        let changed = self.options.get(DEVELOP_PROFILE_KEY).map(String::as_str) != Some(profile);
        self.options
            .insert(DEVELOP_PROFILE_KEY.into(), profile.to_string());
        Ok(changed)
    }
}

/// F-042-N1: a persisted source-action recipe operation. This is an additive
/// pre-MVP schema field: an absent `source_actions` key is interpreted as an
/// empty list, requires no migration and does not change `schema_version`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceActionSpec {
    pub version: u16,
    pub kind: SourceActionKind,
    pub artifact: SourceActionArtifactRef,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelIdentity {
    pub name: String,
    pub version: String,
    pub hash: String,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preprocessing {
    pub name: String,
    pub version: String,
    pub parameters: BTreeMap<String, String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoordinateSystem {
    SourceOriented,
    ModelInput,
    Normalized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaskStatus {
    Valid,
    Stale,
    Missing,
    Corrupt,
    Pending,
}

/// AI-select source kind (G-03 Masking parity): declarative, versioned recipe
/// selector for automatic segmentation. Persisted lowercase (`subject`,
/// `sky`, `background`, `objects`, `people`); parsing is case-insensitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiSelectKind {
    Subject,
    Sky,
    Background,
    Objects,
    People,
}

impl AiSelectKind {
    /// Case-insensitive parse of the persisted form. Returns `None` for
    /// unknown kinds instead of guessing (no silent fallback).
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "subject" => Some(Self::Subject),
            "sky" => Some(Self::Sky),
            "background" => Some(Self::Background),
            "objects" => Some(Self::Objects),
            "people" => Some(Self::People),
            _ => None,
        }
    }

    /// Canonical persisted form.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Subject => "subject",
            Self::Sky => "sky",
            Self::Background => "background",
            Self::Objects => "objects",
            Self::People => "people",
        }
    }

    /// All selectable kinds in UI order.
    pub fn all() -> [Self; 5] {
        [
            Self::Subject,
            Self::Sky,
            Self::Background,
            Self::Objects,
            Self::People,
        ]
    }
}

/// Documented person/object part names for [`AiSelect::detail`]. The list is
/// advisory: validation accepts any trimmed, non-empty string without control
/// characters up to 64 chars, so future parts stay readable (never silently
/// remapped).
pub const AI_SELECT_KNOWN_PARTS: &[&str] = &[
    "face", "hair", "eyes", "pupil", "sclera", "lips", "teeth", "skin", "body",
];

/// Declarative AI-selection source (G-03). Additive schema-v2 field on
/// [`MaskDefinition`]: `None` is the legacy/geometry behaviour and needs no
/// migration. The selected matte still requires a loaded or inferred plane —
/// an AI mask never falls back to geometric rasterization (see
/// `lumina-core::masks`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiSelect {
    pub kind: AiSelectKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

/// The operation performed by a mask definition. `Source` is the default so
/// schema-1 definitions that predate operational masks remain readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MaskOperation {
    #[default]
    Source,
    Union,
    Intersect,
    Subtract,
    Invert,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaskDefinition {
    pub id: String,
    pub name: String,
    pub source_fingerprint: SourceFingerprint,
    pub decode_context: DecodeFingerprint,
    pub geometry_context: GeometryFingerprint,
    pub model: ModelIdentity,
    pub inference_resolution: Resolution,
    pub preprocessing: Preprocessing,
    pub rescaling_method: String,
    pub rescaling_parameters: BTreeMap<String, String>,
    pub coordinate_system: CoordinateSystem,
    pub status: MaskStatus,
    pub created_at: String,
    pub generator_version: String,
    pub error_text: Option<String>,
    pub artifact: Option<ArtifactReference>,
    #[serde(default)]
    pub operation: MaskOperation,
    pub references: Vec<MaskReference>,
    /// Optional user-guided segmentation prompt (F-079). Absent (`None`) is the
    /// legacy/auto mask behaviour and requires no migration. This is a real
    /// field (not part of `extras`) and additive to the schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<MaskPrompt>,
    /// Optional declarative AI selection (G-03: subject/sky/background/
    /// objects/people + part detail). Additive schema-v2 field; `None` is the
    /// legacy behaviour and requires no migration. Only valid on `source`
    /// nodes and never a geometric fallback (see module docs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_select: Option<AiSelect>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaskReference {
    pub copy_id: String,
    pub mask_id: String,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

/// A rectangle with all coordinates normalized to `0..=1` in source space
/// (origin top-left). Used for the bounding-box / coarse object prompt.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NormalizedRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A 2D point normalized to `0..=1` in source space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point2 {
    pub x: f32,
    pub y: f32,
}

/// The sign of a brush mark: painted as foreground (`Positive`) or erased as
/// background (`Negative`). Mirrors the positive/negative prompt points a
/// SAM-style model expects (F-079 / F-082).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrushMarkSign {
    Positive,
    Negative,
}

/// A single brush stamp: a normalized centre, a normalized radius, a sign and
/// the R5-BRUSH-24 edge/opacity controls.
///
/// `softness` and `flow` are additive fields: legacy sidecars without them
/// decode as the original hard, fully opaque brush (`0.0` / `1.0`). New marks
/// snapshot the live tool controls so reopening the mask reconstructs the same
/// matte instead of depending on later session state.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BrushMark {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub sign: BrushMarkSign,
    #[serde(default)]
    pub softness: f32,
    #[serde(default = "default_brush_flow")]
    pub flow: f32,
}

const fn default_brush_flow() -> f32 {
    1.0
}

/// The prompt→model coordinate transformation stored as part of the mask
/// identity, so a generated matte can be recomputed deterministically without
/// losing the user's selection (F-079). `Default` is the identity/empty
/// transformation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PromptTransform {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub method: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, String>,
}

/// A user-guided segmentation prompt source. All coordinates are normalized to
/// `0..=1` in source space. This is a *source* mask node: it carries no
/// references and, when no inferred (model) plane is loaded, is rasterized by a
/// deterministic, model-free geometric rasterizer in `lumina-core`.
///
/// The stored `transformation` keeps the conversion (e.g. box → model coords,
/// or brush → positive/negative points) as part of the mask identity so the
/// same matte can be rebuilt later (F-079). No network/model code lives here;
/// SAM 2 integration is F-082.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MaskPrompt {
    /// Coarse object bounding box, transformed into the model coordinate system.
    Box {
        rect: NormalizedRect,
        transformation: PromptTransform,
    },
    /// Brush mask used as a mask prompt or converted to positive/negative marks.
    Brush {
        marks: Vec<BrushMark>,
        resolution: (u32, u32),
        transformation: PromptTransform,
    },
    /// Polygon prompt expressed as normalized vertices (even-odd fill).
    Polygon {
        points: Vec<Point2>,
        transformation: PromptTransform,
    },
    /// Ellipse prompt with normalized centre and radii.
    Ellipse {
        center: Point2,
        radii: Point2,
        transformation: PromptTransform,
    },
    /// Linear gradient prompt along `angle_deg`, mapping `start`→`end` (0..=1).
    Gradient {
        angle_deg: f32,
        start: f32,
        end: f32,
        transformation: PromptTransform,
    },
    /// Deterministic color-range recipe stage (G-03): pure function of the
    /// source pixels + parameters (no model, no RNG). `hue_center` and
    /// `hue_width` are degrees (`0..=360`), `sat_*`/`lum_*`/`feather` are
    /// `0..=1`. Evaluation lives in `lumina-core::range_masks`.
    ColorRange {
        hue_center: f32,
        hue_width: f32,
        sat_min: f32,
        sat_max: f32,
        lum_min: f32,
        lum_max: f32,
        feather: f32,
        transformation: PromptTransform,
    },
    /// Deterministic luminance-range recipe stage (G-03): pure function of
    /// the source pixels + parameters (no model, no RNG). `min`/`max`/
    /// `feather` are `0..=1` with `min <= max`; Rec.709 luminance with
    /// trapezoid ramps (`ramp = feather·(max-min)/2`).
    LuminanceRange {
        min: f32,
        max: f32,
        feather: f32,
        transformation: PromptTransform,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MaskLayer {
    pub id: String,
    pub mask: MaskReference,
    pub inverted: bool,
    pub feather: f32,
    pub blur: f32,
    pub density: f32,
    /// Visibility eye of the mask list (G-03), persisted per virtual copy.
    /// Absent in older sidecars reads as `true` (legacy identity). An
    /// invisible layer is skipped by the render mask stage (explicit user
    /// choice, no warning).
    pub visible: bool,
    /// MASK-LOCAL-P0/P1.1: typed, versioned local controls. The custom
    /// `Deserialize` implementation in `local_adjustments` migrates valid
    /// legacy `adjustment_*` entries and v1 recipes, and rejects conflicts or
    /// absolute/local aliases loudly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_adjustments: Option<LocalAdjustments>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerativeCanvas {
    pub output_width: u32,
    pub output_height: u32,
    pub source_offset_x: i32,
    pub source_offset_y: i32,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerativeEdit {
    pub version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas: Option<GenerativeCanvas>,
    /// GEN-ZDATA-LINK-1 (GEN-EXPAND-1 `generative_canvas`, zdata `kind = 2`):
    /// link to the full composited canvas record. Additive schema-v2 field;
    /// absent (`None`) means no persisted canvas yet (`missing`, never a
    /// silent fallback). Part of the recipe identity (see
    /// [`GenerativeArtifactRef`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<GenerativeArtifactRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_generative_content: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_fill_transparent: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expand_beyond_image: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

impl GenerativeEdit {
    pub fn effective_keep(&self) -> bool {
        self.keep_generative_content.unwrap_or(true)
    }
    pub fn effective_expand(&self) -> bool {
        self.expand_beyond_image.unwrap_or(false)
    }

    /// GEN-ONNX-1 Welle 2a: the persisted optional negative prompt.
    ///
    /// Additive schema field (top-level JSON `"negative_prompt"`). It is stored
    /// in the flattened [`Self::extras`] map rather than a typed struct field:
    /// a new field would break every `GenerativeEdit` struct literal outside
    /// this wave's scope (notably `lumina-gui`, Welle 2b). The JSON shape is
    /// still the additive top-level field the SOLL documents, and a present
    /// value that is neither a string nor `null` is rejected loudly by
    /// [`Self::validate_edit_extras`]/the document validator — never silently
    /// ignored.
    #[must_use]
    pub fn negative_prompt(&self) -> Option<&str> {
        self.extras
            .get(GENERATIVE_NEGATIVE_PROMPT_KEY)
            .and_then(Value::as_str)
    }

    /// Sets (or clears) the additive negative prompt.
    pub fn set_negative_prompt(&mut self, value: Option<String>) {
        match value {
            Some(value) => {
                self.extras.insert(
                    GENERATIVE_NEGATIVE_PROMPT_KEY.to_owned(),
                    Value::String(value),
                );
            }
            None => {
                self.extras.remove(GENERATIVE_NEGATIVE_PROMPT_KEY);
            }
        }
    }

    /// Loud validation of the additive extras-backed generative fields.
    ///
    /// `negative_prompt` must be a string or `null`; any other type is a hard
    /// error (F2 lesson: an unknown/mistyped field is never silently ignored).
    pub fn validate_edit_extras(&self) -> Result<(), SidecarError> {
        if let Some(value) = self.extras.get(GENERATIVE_NEGATIVE_PROMPT_KEY) {
            if !value.is_null() && !value.is_string() {
                return invalid("generative_edit.negative_prompt must be a string or null");
            }
        }
        Ok(())
    }
}

/// GEN-ONNX-1 Welle 2a: additive `GenerativeEdit` extras key of the optional
/// negative prompt (top-level JSON `"negative_prompt"`).
pub const GENERATIVE_NEGATIVE_PROMPT_KEY: &str = "negative_prompt";
impl GenerativeCanvas {
    pub fn validate_with_source(
        &self,
        source_width: u32,
        source_height: u32,
    ) -> Result<(), SidecarError> {
        self.validate()?;
        if self.output_width <= source_width && self.output_height <= source_height {
            return invalid("expand canvas must be larger than source (output_* > source_*)");
        }
        let ox = self.source_offset_x as i64;
        let oy = self.source_offset_y as i64;
        let sw = source_width as i64;
        let sh = source_height as i64;
        let ow = self.output_width as i64;
        let oh = self.output_height as i64;
        if ox + sw < 0 || oy + sh < 0 || ox >= ow || oy >= oh {
            return invalid("canvas source_offset out of bounds");
        }
        if ox + sw > ow || oy + sh > oh {
            return invalid("canvas source_offset out of bounds");
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), SidecarError> {
        if self.output_width == 0 || self.output_height == 0 {
            return invalid("generative canvas output dimensions must be > 0");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EditRecipe {
    pub recipe_version: String,
    pub adjustments: BTreeMap<String, f64>,
    pub curves: Option<Curves>,
    pub hsl: Option<HslAdjustments>,
    /// Optional F-090b point color (targeted color selection + shifts).
    /// Additive in schema v2; absent is identity and requires no migration.
    pub point_color: Option<PointColor>,
    /// Pre-MVP schema decision: these optional fields are additive in schema v2;
    /// absent values remain identity and require no migration.
    pub color_grading: Option<ColorGrading>,
    pub presence: Option<Presence>,
    pub noise_reduction: Option<NoiseReduction>,
    /// LRPAR-G14-DENOISE-20 (Release 2.0): optional additive AI-denoise stage
    /// (`recipe.adjustments.denoise_ai`, schema v2). `None` is identity and
    /// MVP recipes stay byte-identical; it runs *before* the manual F-096
    /// `noise_reduction` and never replaces it. See the `denoise` module for
    /// the field contract and the documented artifact-shape decision.
    pub denoise_ai: Option<DenoiseAi>,
    pub sharpening: Option<Sharpening>,
    /// Optional G-14 red-eye correction (LRPAR-G14-REDEYE-15, Release 1.5).
    /// Additive in schema v2; absent is identity and requires no migration.
    pub red_eye: Option<RedEyeCorrection>,
    /// Optional top-level geometric transform. Absent is the identity.
    pub geometry: Option<Geometry>,
    /// Optional F-098 lens model, additive in schema v2.
    pub lens_correction: Option<LensCorrection>,
    /// Optional F-099 perspective model, additive in schema v2.
    pub perspective: Option<Perspective>,
    /// Optional LRPAR-G06-UPRIGHT-15 automatic upright analysis, additive in
    /// schema v2. When `enabled` it supplies the effective F-099 perspective
    /// (see [`EditRecipe::effective_perspective`]); absent is identity.
    pub upright: Option<Upright>,
    /// Optional F-097 stylistic effects (vignette + grain). Additive in schema
    /// v2; absent is no effects and requires no migration. Serialized into the
    /// root map (like `geometry`) so both effect objects flow into the
    /// `recipe_hash`/`RenderKey` and invalidate preview/export.
    pub effects: Option<Effects>,
    /// Optional G-05 lens-blur depth bokeh. Additive in schema v2; absent or
    /// disabled is identity and requires no migration. Serialized as a
    /// top-level key (like `geometry`) so it flows into the core
    /// `recipe_hash`/`RenderKey` and invalidates preview/export.
    pub lens_blur: Option<LensBlur>,
    /// Optional F-042-N1 source-action recipe operations (dust removal, AI
    /// replacement). Additive in schema v2; absent is the empty list and
    /// requires no migration.
    pub source_actions: Vec<SourceActionSpec>,
    /// GEN-ZDATA-LINK-1 (SPOT-REMOVE-1 `spot_heal_generative`, zdata
    /// `kind = 3`): persisted spot removals of this virtual copy. Additive
    /// in schema v2; absent is the empty list and requires no migration.
    /// Serialized as a top-level key (like `source_actions`) so the entries
    /// flow into the `recipe_hash`/`RenderKey` on the core side (follow-up).
    pub spot_removals: Vec<SpotRemoval>,
    pub generative_edit: Option<GenerativeEdit>,
    pub options: BTreeMap<String, String>,
    pub auto_features: AutoFeatures,
    pub extras: Extras,
}

impl Serialize for EditRecipe {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut root = serde_json::Map::new();
        root.insert(
            "recipe_version".into(),
            Value::String(self.recipe_version.clone()),
        );
        let mut adjustment = serde_json::Map::new();
        for (key, value) in &self.adjustments {
            adjustment.insert(
                key.clone(),
                serde_json::to_value(value).map_err(serde::ser::Error::custom)?,
            );
        }
        if let Some(curves) = &self.curves {
            adjustment.insert(
                "curves".into(),
                serde_json::to_value(curves).map_err(serde::ser::Error::custom)?,
            );
        }
        if let Some(hsl) = &self.hsl {
            adjustment.insert(
                "hsl".into(),
                serde_json::to_value(hsl).map_err(serde::ser::Error::custom)?,
            );
        }
        if let Some(point_color) = &self.point_color {
            adjustment.insert(
                "point_color".into(),
                serde_json::to_value(point_color).map_err(serde::ser::Error::custom)?,
            );
        }
        if let Some(color_grading) = &self.color_grading {
            adjustment.insert(
                "color_grading".into(),
                serde_json::to_value(color_grading).map_err(serde::ser::Error::custom)?,
            );
        }
        if let Some(presence) = &self.presence {
            adjustment.insert(
                "presence".into(),
                serde_json::to_value(presence).map_err(serde::ser::Error::custom)?,
            );
        }
        if let Some(noise_reduction) = &self.noise_reduction {
            adjustment.insert(
                "noise_reduction".into(),
                serde_json::to_value(noise_reduction).map_err(serde::ser::Error::custom)?,
            );
        }
        // LRPAR-G14-DENOISE-20: additive nested adjustment (`None` = identity,
        // key omitted so legacy recipes roundtrip byte-stable).
        if let Some(denoise_ai) = &self.denoise_ai {
            adjustment.insert(
                "denoise_ai".into(),
                serde_json::to_value(denoise_ai).map_err(serde::ser::Error::custom)?,
            );
        }
        if let Some(sharpening) = &self.sharpening {
            adjustment.insert(
                "sharpening".into(),
                serde_json::to_value(sharpening).map_err(serde::ser::Error::custom)?,
            );
        }
        if let Some(red_eye) = &self.red_eye {
            adjustment.insert(
                "red_eye".into(),
                serde_json::to_value(red_eye).map_err(serde::ser::Error::custom)?,
            );
        }
        if let Some(geometry) = &self.geometry {
            root.insert(
                "geometry".into(),
                serde_json::to_value(geometry).map_err(serde::ser::Error::custom)?,
            );
        }
        if let Some(lens) = &self.lens_correction {
            root.insert(
                "lens_correction".into(),
                serde_json::to_value(lens).map_err(serde::ser::Error::custom)?,
            );
        }
        if let Some(perspective) = &self.perspective {
            root.insert(
                "perspective".into(),
                serde_json::to_value(perspective).map_err(serde::ser::Error::custom)?,
            );
        }
        if let Some(upright) = &self.upright {
            root.insert(
                "upright".into(),
                serde_json::to_value(upright).map_err(serde::ser::Error::custom)?,
            );
        }
        if let Some(effects) = &self.effects {
            root.insert(
                "effects".into(),
                serde_json::to_value(effects).map_err(serde::ser::Error::custom)?,
            );
        }
        if let Some(lens_blur) = &self.lens_blur {
            root.insert(
                "lens_blur".into(),
                serde_json::to_value(lens_blur).map_err(serde::ser::Error::custom)?,
            );
        }
        // F-042-N1: `source_actions` is a top-level additive key (consistent
        // with `geometry`/`lens_correction`/`perspective`), skipped entirely
        // when empty so legacy documents without the key deserialize as empty.
        if !self.source_actions.is_empty() {
            root.insert(
                "source_actions".into(),
                serde_json::to_value(&self.source_actions).map_err(serde::ser::Error::custom)?,
            );
        }
        // GEN-ZDATA-LINK-1: `spot_removals` is a top-level additive key,
        // skipped entirely when empty so legacy documents without the key
        // deserialize as empty. R5-DUST-23-FOLLOWUP fills compatibility IDs
        // for typed-only generative entries before either view is written.
        let mut normalized_spot_removals = self.spot_removals.clone();
        spot::normalize_typed_spot_removals(&mut normalized_spot_removals);
        if !normalized_spot_removals.is_empty() {
            root.insert(
                "spot_removals".into(),
                serde_json::to_value(&normalized_spot_removals)
                    .map_err(serde::ser::Error::custom)?,
            );
        }
        if let Some(generative_edit) = &self.generative_edit {
            root.insert(
                "generative_edit".into(),
                serde_json::to_value(generative_edit).map_err(serde::ser::Error::custom)?,
            );
        }
        root.insert("adjustments".into(), Value::Object(adjustment));
        root.insert(
            "options".into(),
            serde_json::to_value(&self.options).map_err(serde::ser::Error::custom)?,
        );
        root.insert(
            "auto_features".into(),
            serde_json::to_value(&self.auto_features).map_err(serde::ser::Error::custom)?,
        );
        for (key, value) in &self.extras {
            let mut value = value.clone();
            if key == "spot_removals" {
                spot::normalize_spot_removal_array(&mut value);
            }
            root.insert(key.clone(), value);
        }
        Value::Object(root).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EditRecipe {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut root = serde_json::Map::<String, Value>::deserialize(deserializer)?;
        let recipe_version = root
            .remove("recipe_version")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(default_recipe_version);
        let mut adjustments = BTreeMap::new();
        let mut curves = None;
        let mut hsl = None;
        let mut point_color = None;
        let mut color_grading = None;
        let mut presence = None;
        let mut noise_reduction = None;
        let mut denoise_ai = None;
        let mut sharpening = None;
        let mut red_eye = None;
        let geometry = root
            .remove("geometry")
            .map(serde_json::from_value)
            .transpose()
            .map_err(serde::de::Error::custom)?;
        let lens_correction = root
            .remove("lens_correction")
            .map(serde_json::from_value)
            .transpose()
            .map_err(serde::de::Error::custom)?;
        let perspective = root
            .remove("perspective")
            .map(serde_json::from_value)
            .transpose()
            .map_err(serde::de::Error::custom)?;
        let upright = root
            .remove("upright")
            .map(serde_json::from_value)
            .transpose()
            .map_err(serde::de::Error::custom)?;
        let effects = root
            .remove("effects")
            .map(serde_json::from_value)
            .transpose()
            .map_err(serde::de::Error::custom)?;
        let lens_blur = root
            .remove("lens_blur")
            .map(serde_json::from_value)
            .transpose()
            .map_err(serde::de::Error::custom)?;
        // F-042-N1: an absent `source_actions` key is the empty list.
        let source_actions = root
            .remove("source_actions")
            .map(serde_json::from_value)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or_default();
        // SPOT-SCHEMA-GEOMETRY: the raw `spot_removals` JSON value is mirrored
        // into `extras` below (the typed parse reads from a clone). Rationale:
        // `SpotRemoval` carries only id/version/mode/artifact and no heal geometry
        // (center/radius/feather/offset/opacity/id/status), and serde drops
        // unknown fields silently — so consuming the top-level key into the
        // typed field alone irreversibly loses heuristic parameters (69dad91).
        // Keeping the raw value preserves them; on serialize the extras view
        // (geometry-carrying) shadows the lossy typed view for the same key.
        // GEN-ZDATA-LINK-1: an absent `spot_removals` key is the empty list.
        let mut spot_removals_raw = root.remove("spot_removals");
        if let Some(raw) = &mut spot_removals_raw {
            // R5-DUST-23-FOLLOWUP: a valid typed-only generative operation
            // written before explicit IDs receives a deterministic ID while
            // loading. The normalized raw value is also retained in extras so
            // GUI/CLI selection and status use the same identity.
            spot::normalize_spot_removal_array(raw);
        }
        let spot_removals = spot_removals_raw
            .clone()
            .map(serde_json::from_value)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or_default();
        let generative_edit = root
            .remove("generative_edit")
            .map(serde_json::from_value)
            .transpose()
            .map_err(serde::de::Error::custom)?;
        if let Some(Value::Object(mut object)) = root.remove("adjustments") {
            if let Some(value) = object.remove("curves") {
                curves = Some(serde_json::from_value(value).map_err(serde::de::Error::custom)?);
            }
            if let Some(value) = object.remove("hsl") {
                hsl = Some(serde_json::from_value(value).map_err(serde::de::Error::custom)?);
            }
            if let Some(value) = object.remove("point_color") {
                point_color =
                    Some(serde_json::from_value(value).map_err(serde::de::Error::custom)?);
            }
            if let Some(value) = object.remove("color_grading") {
                color_grading =
                    Some(serde_json::from_value(value).map_err(serde::de::Error::custom)?);
            }
            if let Some(value) = object.remove("presence") {
                presence = Some(serde_json::from_value(value).map_err(serde::de::Error::custom)?);
            }
            if let Some(value) = object.remove("noise_reduction") {
                noise_reduction =
                    Some(serde_json::from_value(value).map_err(serde::de::Error::custom)?);
            }
            // LRPAR-G14-DENOISE-20: nested object (not an f64 slider), so it is
            // consumed before the remaining keys fall into `adjustments`.
            if let Some(value) = object.remove("denoise_ai") {
                denoise_ai = Some(serde_json::from_value(value).map_err(serde::de::Error::custom)?);
            }
            if let Some(value) = object.remove("sharpening") {
                sharpening = Some(serde_json::from_value(value).map_err(serde::de::Error::custom)?);
            }
            if let Some(value) = object.remove("red_eye") {
                red_eye = Some(serde_json::from_value(value).map_err(serde::de::Error::custom)?);
            }
            for (key, value) in object {
                adjustments.insert(
                    key,
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?,
                );
            }
        }
        let options = root
            .remove("options")
            .map(serde_json::from_value)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or_default();
        let auto_features = root
            .remove("auto_features")
            .map(serde_json::from_value)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .unwrap_or_default();
        // SPOT-SCHEMA-GEOMETRY: mirror the raw `spot_removals` value into
        // `extras` so heal geometry survives the roundtrip (see above). An
        // absent key stays absent (additive, no migration); a present key is
        // available both as the typed schema-v2 view and as the
        // geometry-carrying extras view validated by
        // `validate_spot_removal_extras`.
        let mut extras: Extras = root.into_iter().collect();
        if let Some(raw) = spot_removals_raw {
            extras.insert("spot_removals".into(), raw);
        }
        Ok(Self {
            recipe_version,
            adjustments,
            curves,
            hsl,
            point_color,
            color_grading,
            presence,
            noise_reduction,
            denoise_ai,
            sharpening,
            red_eye,
            geometry,
            lens_correction,
            perspective,
            upright,
            effects,
            lens_blur,
            source_actions,
            spot_removals,
            generative_edit,
            options,
            auto_features,
            extras,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Curves {
    pub version: u8,
    pub master: CurvePoints,
    #[serde(default)]
    pub channels: CurveChannels,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CurveChannels {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub red: Option<CurvePoints>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub green: Option<CurvePoints>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blue: Option<CurvePoints>,
}
pub type CurvePoints = Vec<CurvePoint>;
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub input: f32,
    pub output: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct HslAdjustments {
    pub version: u8,
    #[serde(default)]
    pub red: Option<HslChannel>,
    #[serde(default)]
    pub orange: Option<HslChannel>,
    #[serde(default)]
    pub yellow: Option<HslChannel>,
    #[serde(default)]
    pub green: Option<HslChannel>,
    #[serde(default)]
    pub cyan: Option<HslChannel>,
    #[serde(default)]
    pub blue: Option<HslChannel>,
    #[serde(default)]
    pub violet: Option<HslChannel>,
    #[serde(default)]
    pub magenta: Option<HslChannel>,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct HslChannel {
    #[serde(default)]
    pub hue: f32,
    #[serde(default)]
    pub saturation: f32,
    #[serde(default)]
    pub luminance: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColorGradingRange {
    pub hue_degrees: f32,
    pub saturation: f32,
    /// G-02 Feinschliff: additive lightness offset of the tint (`-1..=1`,
    /// `0` = identity). Missing in legacy documents means `0` (additive,
    /// no migration; identical rendering).
    #[serde(default)]
    pub luminance: f32,
}

impl ColorGradingRange {
    /// Neutral range (no tint, no luminance shift).
    pub fn neutral() -> Self {
        Self {
            hue_degrees: 0.0,
            saturation: 0.0,
            luminance: 0.0,
        }
    }
}

pub(crate) fn default_color_grading_blending() -> f32 {
    color_blocks::default_color_grading_blending()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorGrading {
    pub version: u8,
    pub shadows: ColorGradingRange,
    pub midtones: ColorGradingRange,
    pub highlights: ColorGradingRange,
    pub balance: f32,
    /// G-02 Feinschliff: overlap width of the range weights (`0..=1`).
    /// `0.5` reproduces the pre-refinement edges exactly; missing in legacy
    /// documents means `0.5` (additive, no migration; identical rendering).
    #[serde(default = "default_color_grading_blending")]
    pub blending: f32,
}

impl ColorGrading {
    /// Neutral grading (no tint, centered balance, legacy blending).
    pub fn neutral() -> Self {
        Self {
            version: 1,
            shadows: ColorGradingRange::neutral(),
            midtones: ColorGradingRange::neutral(),
            highlights: ColorGradingRange::neutral(),
            balance: 0.0,
            blending: default_color_grading_blending(),
        }
    }
}

/// F-090b Point Color (G-02, LRPAR-G02-COLOR): targeted color selection with
/// a free hue center plus hue/saturation/luminance shifts. Serialized into
/// the `adjustments` map (like `hsl`); absent is identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointColor {
    pub version: u8,
    #[serde(default)]
    pub entries: Vec<PointColorEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointColorEntry {
    /// Stable id, unique within the recipe (e.g. `pc-1`); entries are
    /// never identified by their list position.
    pub id: String,
    /// Selection center, cyclic degrees `0..=360`.
    pub hue_center: f32,
    /// Half flank width of the triangular hue weighting `0..=180`
    /// (`0` matches only the exact center hue).
    pub hue_range: f32,
    /// Hue rotation of at most ±30° (`-1..=1`).
    pub hue_shift: f32,
    /// Additive saturation shift (`-1..=1`).
    pub saturation_shift: f32,
    /// Additive luminance shift (`-1..=1`).
    pub luminance_shift: f32,
}

impl PointColorEntry {
    /// Next stable entry id over the current recipe state (`pc-<n>`,
    /// `n` = one past the highest numeric `pc-` suffix, starting at 1).
    /// Deterministic given the recipe; stable across roundtrips.
    pub fn next_id(entries: &[PointColorEntry]) -> String {
        let mut max: u32 = 0;
        for entry in entries {
            if let Some(suffix) = entry.id.strip_prefix("pc-") {
                if let Ok(n) = suffix.parse::<u32>() {
                    max = max.max(n);
                }
            }
        }
        format!("pc-{}", max + 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Presence {
    pub version: u8,
    #[serde(default)]
    pub texture: f32,
    #[serde(default)]
    pub clarity: f32,
    #[serde(default)]
    pub dehaze: f32,
}

/// LRPAR-G14-REDEYE-15 (Release 1.5): a single persisted red-eye correction
/// region. `x`/`y` is the normalized pupil center (`0..=1`, mapping to
/// `x * width` / `y * height` in pixels); `radius` is normalized
/// (`0 < radius <= 1`, pixel radius `radius * min(width, height)`).
/// `desaturate`/`darken` are per-region correction strengths (`0..=1`,
/// `0` = no effect). Regions are identified by their stable `id`, never by
/// their list position. See `feature/architecture/pipeline.md` § G-14.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RedEyeRegion {
    /// Stable id, unique within the recipe (e.g. `re-1`).
    pub id: String,
    /// Normalized pupil-center x (`0..=1`).
    pub x: f32,
    /// Normalized pupil-center y (`0..=1`).
    pub y: f32,
    /// Normalized radius (`0 < radius <= 1`).
    pub radius: f32,
    /// Desaturation strength (`0..=1`).
    pub desaturate: f32,
    /// Darkening strength (`0..=1`).
    pub darken: f32,
}

/// LRPAR-G14-REDEYE-15 (Release 1.5): red-eye correction recipe stage.
/// Additive in schema v2; absent is identity and requires no migration.
/// Serialized into the `adjustments` map (like `noise_reduction`);
/// `None`/empty is identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RedEyeCorrection {
    pub version: u8,
    #[serde(default)]
    pub regions: Vec<RedEyeRegion>,
}

/// LRPAR-G14-REDEYE-15: maximum number of persisted red-eye regions.
pub const RED_EYE_MAX_REGIONS: usize = 32;

/// LRPAR-G06-UPRIGHT-15: upper bound on the reported number of supporting
/// line pixels of one upright analysis (hostile-document guard).
pub const UPRIGHT_MAX_LINE_COUNT: u32 = 100_000_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    pub version: u8,
    pub crop: Option<Crop>,
    pub rotation_degrees: f32,
    pub mirror_horizontal: bool,
    pub mirror_vertical: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LensCorrection {
    pub version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distortion_k1: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distortion_k2: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distortion_k3: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vignette_c0: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vignette_c1: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vignette_c2: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_red: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_blue: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Perspective {
    pub version: u8,
    pub vertical: f32,
    pub horizontal: f32,
    pub rotation: f32,
    pub scale: f32,
    pub aspect_ratio: f32,
    pub shift_x: f32,
    pub shift_y: f32,
}

/// LRPAR-G06-UPRIGHT-15 (Release 1.5): persisted automatic upright analysis.
/// Classic, model-free line detection (see the `upright-lines-v1` algorithm in
/// `lumina-core`). The suggestion lives in the F-099 `Perspective` domain so it
/// can be applied as the effective perspective. `fingerprint` binds the
/// analysis to the exact source/decode/geometry context; `line_count` and
/// `confidence` are deterministic evidence, never a user-facing score to
/// optimize.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UprightAnalysis {
    pub fingerprint: AnalysisFingerprint,
    /// Suggested vertical keystone correction (`-1..=1`).
    pub vertical: f32,
    /// Suggested horizontal keystone correction (`-1..=1`).
    pub horizontal: f32,
    /// Suggested in-plane rotation (`-1..=1`).
    pub rotation: f32,
    /// Number of supporting line pixels found by the detector.
    pub line_count: u32,
    /// Deterministic confidence `0..=1` of the suggestion.
    pub confidence: f32,
}

/// LRPAR-G06-UPRIGHT-15: additive top-level recipe stage. `enabled` selects
/// whether the persisted [`UprightAnalysis`] supplies the effective F-099
/// perspective (`EditRecipe::effective_perspective`); the manual
/// `recipe.perspective` stays persisted and is restored when disabled. An
/// absent field is identity and requires no migration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Upright {
    pub version: u8,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis: Option<UprightAnalysis>,
}

impl Upright {
    /// Resolves the effective F-099 perspective supplied by this stage when it
    /// is enabled and carries an analysis. `None` when disabled or without an
    /// analysis (validation rejects `enabled` without `analysis` loudly).
    pub fn resolved_perspective(&self) -> Option<Perspective> {
        if !self.enabled {
            return None;
        }
        self.analysis.as_ref().map(|analysis| Perspective {
            version: 1,
            vertical: analysis.vertical,
            horizontal: analysis.horizontal,
            rotation: analysis.rotation,
            scale: 1.0,
            aspect_ratio: 1.0,
            shift_x: 0.0,
            shift_y: 0.0,
        })
    }
}

/// F-097 Vignette: a deterministic radial edge-darkening (or -lightening) effect.
/// See `feature/architecture/pipeline.md` for the SOLL ranges. The effect is
/// applied as the LAST sub-stage of the `Adjustments` stage, to the RGB
/// channels only; the alpha channel is never touched.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vignette {
    pub version: u8,
    /// `-1..=1`: positive darkens the edges, negative lightens them, 0 is identity.
    pub amount: f32,
    /// `0..=1`: where the falloff begins (0 starts at the centre, 1 only at the edge).
    pub midpoint: f32,
    /// `-1..=1`: control of the elliptical aspect (1 = circular).
    pub roundness: f32,
    /// `0..=1`: transition softness (0 = sharp, 1 = very soft).
    pub feather: f32,
}

/// F-097 Grain: deterministic, procedural, channel-coupled noise added to the
/// RGB channels. The effective noise seed is derived deterministically from
/// `seed` and the image dimensions, so the same seed reproduces the same grain
/// and a different seed (or size) changes the result.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Grain {
    pub version: u8,
    /// `0..=1`: overall grain intensity (0 = identity).
    pub amount: f32,
    /// `0..=1`: grain spatial scale (0 = per-pixel, 1 = coarse blocks).
    pub size: f32,
    /// `0..=1`: texture variation (0 = smooth/low-frequency, 1 = raw per-cell).
    pub roughness: f32,
    /// `u64`: deterministic seed.
    pub seed: u64,
}

/// F-097 container: both optional effect objects live under `recipe.effects`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Effects {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vignette: Option<Vignette>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain: Option<Grain>,
}

/// G-05 Lens Blur: deterministic depth bokeh. See
/// `feature/architecture/pipeline.md` § „G-05 Lens Blur". Additive schema-v2
/// field on `EditRecipe` (`recipe.lens_blur`); absent is identity and
/// requires no migration. Serialized as a top-level key (like `geometry`)
/// so it flows into the core `recipe_hash`/`RenderKey`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FocusRect {
    /// Normalized left edge `0..=1`.
    pub x: f32,
    /// Normalized top edge `0..=1`.
    pub y: f32,
    /// Normalized width (`> 0`, `x + width <= 1`).
    pub width: f32,
    /// Normalized height (`> 0`, `y + height <= 1`).
    pub height: f32,
}

/// G-05 bokeh kernel shapes. Every variant is a deterministic integer kernel
/// mask (no randomness); the variants render visibly different blurs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BokehShape {
    Round,
    Elliptical,
    Hexagonal,
}

/// G-05 external depth-map reference. Portable: relative path only (absolute
/// paths rejected), content-addressed via SHA-256. When present, the render
/// caller MUST supply the depth plane; a missing/checksum-mismatched
/// artifact aborts the render loudly (never a silent heuristic fallback).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DepthArtifactRef {
    pub relative_path: String,
    pub sha256: String,
}

/// G-05 Lens Blur recipe stage (per virtual copy).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LensBlur {
    pub version: u8,
    /// Master switch. `false` (or absent `lens_blur`) is identity.
    pub enabled: bool,
    pub focus_rect: FocusRect,
    /// Near edge of the sharp depth band `0..=1`.
    pub focal_near: f32,
    /// Far edge of the sharp depth band `0..=1` (`>= focal_near`).
    pub focal_far: f32,
    /// Blur strength `0..=1` (0 is identity, no blur pass).
    pub blur_amount: f32,
    pub bokeh: BokehShape,
    /// Optional external depth map; `None` selects the deterministic
    /// focus-rect heuristic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth_artifact: Option<DepthArtifactRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum Crop {
    Aspect {
        preset: AspectPreset,
    },
    Free {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AspectPreset {
    #[serde(rename = "original")]
    Original,
    #[serde(rename = "1:1")]
    OneToOne,
    #[serde(rename = "4:5")]
    FourToFive,
    #[serde(rename = "5:4")]
    FiveToFour,
    #[serde(rename = "3:2")]
    ThreeToTwo,
    #[serde(rename = "2:3")]
    TwoToThree,
    #[serde(rename = "4:3")]
    FourToThree,
    #[serde(rename = "3:4")]
    ThreeToFour,
    #[serde(rename = "16:9")]
    SixteenToNine,
    #[serde(rename = "9:16")]
    NineToSixteen,
}

fn default_recipe_version() -> String {
    "1".into()
}

impl Default for EditRecipe {
    fn default() -> Self {
        Self {
            recipe_version: default_recipe_version(),
            adjustments: BTreeMap::new(),
            curves: None,
            hsl: None,
            point_color: None,
            color_grading: None,
            presence: None,
            noise_reduction: None,
            denoise_ai: None,
            sharpening: None,
            red_eye: None,
            geometry: None,
            lens_correction: None,
            perspective: None,
            upright: None,
            effects: None,
            lens_blur: None,
            source_actions: Vec::new(),
            spot_removals: Vec::new(),
            generative_edit: None,
            options: BTreeMap::new(),
            auto_features: AutoFeatures::default(),
            extras: Extras::new(),
        }
    }
}

impl EditRecipe {
    /// LRPAR-G06-UPRIGHT-15: the effective F-099 perspective used by the
    /// renderer. When the `upright` stage is enabled and carries an analysis,
    /// its suggestion is authoritative (the manual `perspective` stays
    /// persisted and returns when `upright.enabled` is `false`); otherwise the
    /// manual `recipe.perspective` is used. Pure recipe derivation — no pixel
    /// or platform dependency, used identically by CPU and GPU.
    pub fn effective_perspective(&self) -> Option<Perspective> {
        self.upright
            .as_ref()
            .and_then(Upright::resolved_perspective)
            .or(self.perspective)
    }
}
