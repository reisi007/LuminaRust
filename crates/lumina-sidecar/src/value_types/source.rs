//! Source identity, source-action, artifact-link and spot-removal value types.
//!
//! Pure move out of `crates/lumina-sidecar/src/value_types.rs` (file-size
//! ceiling): the items below are byte-identical to their former place; only
//! this module documentation and the `use` block are new. `value_types.rs`
//! re-exports every name, so `lumina_sidecar::DecodeFingerprint`,
//! `lumina_sidecar::SpotRemoval`, … keep their paths. The spot/Develop extras
//! keys travel with the spec types they belong to; `EditRecipe`'s inherent
//! impl reaches them through the re-export.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

// The root's private modules and its own types. A child module does not
// inherit the parent's item names unqualified, so they are imported here.
use crate::spot;
use crate::{ArtifactStatus, Extras, SourceActionArtifactRef};
// Referenced only by the intra-doc links of the moved block; a child module
// does not inherit the parent's re-exports, so the imports keep those links
// resolvable (`[`artifact_status`]`, `[`generative_artifact_status`]`).
#[allow(unused_imports)]
use crate::artifact_status;
#[allow(unused_imports)]
use crate::generative_artifact_status;
// Referenced only by the intra-doc link of `BW_STASH_KEY`; the struct itself
// stays in the parent module.
#[allow(unused_imports)]
use super::EditRecipe;

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
