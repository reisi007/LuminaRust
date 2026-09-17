#![allow(clippy::field_reassign_with_default)]
//! Versioned, portable domain types for a Lumina sidecar.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
// Bare `thread::sleep` is used only by the native file-lock retry loop below.
use std::thread;
use std::time::{Duration, SystemTime};
use thiserror::Error;

#[cfg(feature = "zdata")]
mod zdata;
#[cfg(feature = "zdata")]
pub use zdata::{
    append_denoise_rgb, append_generative_canvas, append_repair_region,
    append_spot_heal_generative, load_zdata, save_denoise_rgb, save_face_embedding,
    save_face_embeddings, save_generative_canvas, save_zdata, zdata_path_for, DenoiseRgbArtifact,
    FaceEmbeddingArtifact, GenerativeCanvasArtifact, MaskTile, RecordKind, RecordSpec,
    RepairRegionArtifact, SpotHealGenerativeArtifact, ZDataContainer, ZDataError,
    FACE_EMBEDDING_ENCODING_VERSION, MAX_FACE_EMBEDDING_DIMENSION, RGB_ENCODING_VERSION,
};

// LRPAR-G15-IPTC-S4: file-backed IPTC metadata presets (static + dynamic).
mod meta_preset;
pub use meta_preset::{
    default_meta_presets_dir, is_meta_preset_placeholder_name, load_meta_preset_file,
    meta_preset_filename, render_meta_preset, resolve_meta_preset_path, scan_meta_presets_dir,
    MetaPresetEntry, MetaPresetError, MetaPresetFile, MetaPresetPlaceholder,
    META_PRESET_FILE_SUFFIX, META_PRESET_FORMAT, META_PRESET_VERSION,
};

// LRPAR-G13-MERGE-15 / MERGE-SCHEMA-1: versioned HDR/panorama merge recipe
// (schema + validation + digest; no DNG writer, no alignment computation).
mod merge_recipe;
pub use merge_recipe::{
    merge_digest, validate_merge_recipe, MergeAlignment, MergeAlignmentMethod, MergeDecodeContext,
    MergeExposure, MergeMode, MergeOutput, MergeProjection, MergeRecipe, MergeSource, MergeStatus,
    MergeTransform, MAX_MERGE_BLEND_WIDTH_PX, MAX_MERGE_EXPOSURE_TIME_S, MAX_MERGE_F_NUMBER,
    MAX_MERGE_ISO, MAX_MERGE_RESIDUAL_PX, MAX_MERGE_SOURCES, MERGE_HASH_HEX_LEN, MERGE_HASH_PREFIX,
    MERGE_OUTPUT_BITS, MERGE_RECIPE_VERSION, MIN_MERGE_SOURCES,
};

// LRPAR-G12-FACE-20 / FACE-20-S1: source-level face-detection schema
// (detections, embedding/vector references, clusters, person labels plus
// identity/status; no models, no clustering evaluation, no CLI/GUI).
mod face;
pub use face::{
    face_artifact_evidence, validate_face_analysis, validate_face_sha256, FaceAnalysis,
    FaceArtifactEvidence, FaceArtifactStatus, FaceBoundingBox, FaceCluster, FaceClusteringIdentity,
    FaceDetection, FaceEmbedding, FaceIdentity, FaceLandmark, FacePerson, FaceVectorRef,
    FACE_HASH_HEX_LEN, FACE_PENDING_MODEL_HASH, FACE_SCHEMA_VERSION, FACE_SHA256_PREFIX,
    MAX_FACE_CLUSTERS, MAX_FACE_CLUSTER_MEMBERS, MAX_FACE_DETECTIONS, MAX_FACE_EMBEDDINGS,
    MAX_FACE_ERROR_CHARS, MAX_FACE_ID_CHARS, MAX_FACE_LANDMARKS, MAX_FACE_NAME_CHARS,
    MAX_FACE_PERSONS,
};

// LRPAR-G14-DENOISE-20: additive `recipe.adjustments.denoise_ai` schema
// (identity + model + input-spec digest + strength/detail + binary artifact
// reference; no pipeline stage, no zdata codec, no ONNX/CLI/GUI).
mod denoise;
pub use denoise::{
    validate_denoise_ai, validate_denoise_sha256, DenoiseAi, DenoiseArtifactKind,
    DenoiseArtifactRef, DenoiseModelIdentity, DENOISE_AI_VERSION, DENOISE_ARTIFACT_KIND,
    DENOISE_HASH_HEX_LEN, DENOISE_PENDING_MODEL_HASH, DENOISE_SHA256_PREFIX,
};

// LRPAR-G09-CULL-25: source-level `culling` proposal section (assisted
// culling; never writes rating/flag/label; proposal/score/reasons/identity/
// status only — no heuristic, no ONNX, no CLI/GUI).
mod culling;
pub use culling::{
    validate_culling_reason, validate_culling_section, validate_culling_sha256, CullProposal,
    CullingAnalyzer, CullingAnalyzerKind, CullingIdentity, CullingSection, CullingStatus,
    CULLING_HASH_HEX_LEN, CULLING_PENDING_MODEL_HASH, CULLING_SCHEMA_VERSION,
    CULLING_SHA256_PREFIX, MAX_CULLING_ERROR_CHARS, MAX_CULLING_REASONS, MAX_CULLING_REASON_CHARS,
};

pub const FORMAT: &str = "lumina-sidecar";
pub const SCHEMA_VERSION: u32 = 2;

/// F-042-N1: current schema version for a persisted source-action spec. This is
/// independent of `SCHEMA_VERSION`; an unknown source-action `version` is
/// rejected during validation rather than silently ignored.
pub const SOURCE_ACTION_VERSION: u16 = 1;
pub const MAX_SIDECAR_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_VIRTUAL_COPIES: usize = 10_000;

/// G-15 META-MVP (Slice 1): bounds for source-level metadata. Keywords describe
/// the image content; static collection memberships are the sidecar-first
/// record of catalogue membership (the optional index rebuilds collections by
/// scanning sidecars, never the other way round).
pub const MAX_KEYWORDS_PER_DOCUMENT: usize = 512;
pub const MAX_KEYWORD_CHARS: usize = 128;
pub const MAX_COLLECTIONS_PER_DOCUMENT: usize = 512;
pub const MAX_COLLECTION_ID_CHARS: usize = 128;
pub const MAX_COLLECTION_NAME_CHARS: usize = 256;

/// LRPAR-G15-IPTC-S1: current schema version of the source-level IPTC
/// metadata draft (`SidecarDocument::metadata`). Independent of
/// `SCHEMA_VERSION`; an unknown `version` is rejected during validation
/// rather than silently ignored.
pub const METADATA_DRAFT_VERSION: u8 = 1;
/// LRPAR-G15-IPTC-S1: maximum number of entries kept in the metadata draft
/// history. New entries are prepended (newest first); once a mutation would
/// exceed the cap, the oldest entries fall off the end deterministically.
/// A persisted history longer than the cap fails validation loudly — it is
/// never truncated silently.
pub const MAX_METADATA_HISTORY_ENTRIES: usize = 100;
/// LRPAR-G15-IPTC-S1: the fixed draft field registry (SOLL §4). `keywords`
/// is deliberately *not* a draft field — it stays the existing source-level
/// sidecar field and is only routed/carried by draft UI, sync and export.
pub const METADATA_FIELD_IDS: &[&str] = &[
    "title",
    "headline",
    "description",
    "copyright_notice",
    "creator",
    "credit",
    "source",
    "city",
    "state_province",
    "country",
    "date_created",
];
/// LRPAR-G15-IPTC-S1: per-field Sidecar character limits (SOLL §4).
/// `date_created` carries no character limit; it is constrained to the
/// `YYYY-MM-DD` format instead (see [`validate_metadata_date_created`]).
pub const MAX_METADATA_TITLE_CHARS: usize = 256;
pub const MAX_METADATA_HEADLINE_CHARS: usize = 256;
pub const MAX_METADATA_DESCRIPTION_CHARS: usize = 2000;
pub const MAX_METADATA_COPYRIGHT_NOTICE_CHARS: usize = 256;
pub const MAX_METADATA_CREATOR_CHARS: usize = 256;
pub const MAX_METADATA_CREDIT_CHARS: usize = 256;
pub const MAX_METADATA_SOURCE_CHARS: usize = 256;
pub const MAX_METADATA_CITY_CHARS: usize = 128;
pub const MAX_METADATA_STATE_PROVINCE_CHARS: usize = 128;
pub const MAX_METADATA_COUNTRY_CHARS: usize = 128;

/// G-15 META-MVP (Slice 1): current schema version for a persisted smart
/// collection definition. Independent of `SCHEMA_VERSION`; an unknown
/// `version` is rejected during validation rather than silently ignored.
pub const SMART_COLLECTION_VERSION: u8 = 1;
/// G-15 META-MVP (Slice 1): maximum nesting depth of a `SmartRule` tree.
/// Bounds hostile/degenerate JSON so validation and evaluation stay
/// stack-safe; legitimate Lightroom-style criteria nest far shallower.
pub const MAX_SMART_RULE_DEPTH: usize = 32;

/// REVIEW-SIDECAR-TMP-1: an atomic-write temporary must be at least this old
/// before [`recover_sidecar`] considers it orphaned. A live writer keeps its
/// temporary fresh, so a concurrent reader's recovery sweep can never delete a
/// temporary that is currently being written. The value matches the stale-lock
/// threshold of [`acquire_write_lock`]: both answer the same question — "is
/// this artefact of a crashed writer?" — with the same conservative bound.
pub const TEMP_SWEEP_AGE: Duration = Duration::from_secs(30);

pub type Extras = BTreeMap<String, Value>;

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

/// F-042-N1: reference to a repair-region artifact stored in the sidecar's
/// `.lumina.zdata` bundle. `id` is the record id inside the bundle,
/// `relative_path` is the portable (never absolute) bundle file name, and
/// `checksum` is the BLAKE3 checksum of the artifact bytes. The existing
/// `ArtifactReference` is intentionally *not* reused here: it has no record
/// `id` field and carries mask-specific metadata (`channels`, `data_version`)
/// that a repair region does not need.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceActionArtifactRef {
    pub id: String,
    pub relative_path: String,
    pub checksum: String,
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
        artifact_status(bundle_root, &self.as_artifact_reference())
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
/// SPOT-SCHEMA-GEOMETRY: this typed view intentionally carries only
/// version/mode/artifact. The heal geometry (center/radius/feather/offset/
/// opacity/id/status) travels in the mirrored `extras["spot_removals"]` view
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
/// Recipe-identity note (core follow-up, not implemented here): `mode` and,
/// for generative spots, every field of `artifact` MUST be included in the
/// core `recipe_hash`/`RenderKey`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpotRemoval {
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

/// A single brush stamp: a normalized centre, a normalized radius and a sign.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BrushMark {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub sign: BrushMarkSign,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(default = "mask_layer_visible_default")]
    pub visible: bool,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

/// Serde default for [`MaskLayer::visible`]: legacy sidecars without the key
/// behave as if every layer were visible.
fn mask_layer_visible_default() -> bool {
    true
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
        // deserialize as empty.
        // SPOT-SCHEMA-GEOMETRY: when the geometry-carrying extras mirror (see
        // `Deserialize`) holds the same key, the extras loop below overwrites
        // this lossy typed view with the full entry — that precedence is
        // intentional: the typed `SpotRemoval` holds only
        // version/mode/artifact, while the extras view carries the heal
        // geometry. A typed-only recipe (no extras key) still serializes here
        // with no silent key loss.
        if !self.spot_removals.is_empty() {
            root.insert(
                "spot_removals".into(),
                serde_json::to_value(&self.spot_removals).map_err(serde::ser::Error::custom)?,
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
            root.insert(key.clone(), value.clone());
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
        // `SpotRemoval` carries only version/mode/artifact and no heal geometry
        // (center/radius/feather/offset/opacity/id/status), and serde drops
        // unknown fields silently — so consuming the top-level key into the
        // typed field alone irreversibly loses heuristic parameters (69dad91).
        // Keeping the raw value preserves them; on serialize the extras view
        // (geometry-carrying) shadows the lossy typed view for the same key.
        // GEN-ZDATA-LINK-1: an absent `spot_removals` key is the empty list.
        let spot_removals_raw = root.remove("spot_removals");
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

fn default_color_grading_blending() -> f32 {
    0.5
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NoiseReduction {
    pub version: u8,
    pub luminance: f32,
    pub color: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sharpening {
    pub version: u8,
    pub amount: f32,
    pub radius: f32,
    pub detail: f32,
    pub masking: f32,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoFeatures {
    #[serde(default)]
    pub enable_auto_tone: bool,
    #[serde(default)]
    pub match_total_exposure: bool,
    #[serde(default = "default_target_luminance")]
    pub target_luminance: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_exposure: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_contrast: Option<f64>,
    /// AUTO-TONE-2 Spiegelfelder: vom Auto-Algorithmus geschriebene Werte für
    /// `whites`/`blacks`/`highlights`/`shadows` (Core-`AutoToneResult`, Domäne
    /// je `-1..=1` wie die gleichnamigen Recipe-Adjustments). `None` = kein
    /// Auto-Wert persistiert (manuell oder nie Auto-Tone gelaufen). Analog zu
    /// `auto_exposure`/`auto_contrast`, additiv mit `#[serde(default)]`, daher
    /// ohne Migration lesbar (Altdateien → `None`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_whites: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_blacks: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_highlights: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_shadows: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_exposure: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_fingerprint: Option<AnalysisFingerprint>,
}

impl Default for AutoFeatures {
    fn default() -> Self {
        Self {
            enable_auto_tone: false,
            match_total_exposure: false,
            target_luminance: default_target_luminance(),
            auto_exposure: None,
            auto_contrast: None,
            auto_whites: None,
            auto_blacks: None,
            auto_highlights: None,
            auto_shadows: None,
            matched_exposure: None,
            analysis_fingerprint: None,
        }
    }
}

fn default_target_luminance() -> f64 {
    0.5
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub recipe: EditRecipe,
    pub recorded_at: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub recipe: EditRecipe,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportRecord {
    pub id: String,
    pub relative_path: String,
    pub format: String,
    pub exported_at: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Flag {
    #[default]
    Unflagged,
    Pick,
    Reject,
}

/// G-15 META-MVP (Slice 1): membership of the source image in one static
/// collection. Persisted per sidecar (source level) so collections are
/// rebuildable from sidecars alone; renaming a collection is a batch operation
/// over every affected sidecar. `id` is the stable identity, `name` the
/// display name mirrored at write time — never an absolute path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionMembership {
    pub id: String,
    pub name: String,
}

/// G-15 META-MVP (Slice 1): a smart-collection criterion as portable data.
/// Inputs are exclusively sidecar fields — the document's source-level
/// `keywords` plus the evaluated copy's `rating`/`flag` — so any index or CLI
/// can rebuild smart-collection results deterministically from sidecars.
/// Keyword comparison is exact and case-sensitive (no locale dependence).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SmartRule {
    /// Matches every image.
    All,
    /// Matches no image.
    None,
    /// Matches when `keyword` is among the document's keywords (exact).
    Keyword { keyword: String },
    /// Matches when the copy's rating is at least `rating` (`0..=5`).
    RatingAtLeast { rating: u8 },
    /// Matches when the copy's rating equals `rating` (`0..=5`).
    RatingEquals { rating: u8 },
    /// Matches when the copy's flag equals `flag`.
    Flag { flag: Flag },
    /// Matches when every sub-rule matches. At least one sub-rule required.
    And { rules: Vec<SmartRule> },
    /// Matches when any sub-rule matches. At least one sub-rule required.
    Or { rules: Vec<SmartRule> },
    /// Matches when the sub-rule does not match.
    Not { rule: Box<SmartRule> },
}

impl SmartRule {
    /// Pure, deterministic evaluation over sidecar inputs only.
    pub fn matches(&self, keywords: &[String], rating: u8, flag: Flag) -> bool {
        match self {
            SmartRule::All => true,
            SmartRule::None => false,
            SmartRule::Keyword { keyword } => keywords.iter().any(|k| k == keyword),
            SmartRule::RatingAtLeast { rating: min } => rating >= *min,
            SmartRule::RatingEquals { rating: expected } => rating == *expected,
            SmartRule::Flag { flag: expected } => flag == *expected,
            SmartRule::And { rules } => rules.iter().all(|r| r.matches(keywords, rating, flag)),
            SmartRule::Or { rules } => rules.iter().any(|r| r.matches(keywords, rating, flag)),
            SmartRule::Not { rule } => !rule.matches(keywords, rating, flag),
        }
    }
}

/// G-15 META-MVP (Slice 1): a versioned, portable smart-collection
/// definition. The definition itself is catalogue-level data (persisted by a
/// follow-up slice using exactly this format); it is deliberately *not*
/// duplicated into every image sidecar, where renames and edits would
/// diverge. Evaluation reads only sidecar inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartCollectionDef {
    pub version: u8,
    pub id: String,
    pub name: String,
    pub rule: SmartRule,
}

impl SmartCollectionDef {
    /// Evaluates the rule against one virtual copy: document keywords plus
    /// that copy's rating/flag. An unknown `copy_id` is a loud error, never
    /// a silent non-match.
    pub fn matches_copy(
        &self,
        document: &SidecarDocument,
        copy_id: &str,
    ) -> Result<bool, SidecarError> {
        if self.version != SMART_COLLECTION_VERSION {
            return Err(SidecarError::Invalid(
                "unsupported smart_collection version".into(),
            ));
        }
        let copy = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == copy_id)
            .ok_or_else(|| SidecarError::Invalid(format!("unknown virtual copy `{copy_id}`")))?;
        Ok(self
            .rule
            .matches(&document.keywords, copy.rating, copy.flag))
    }

    /// Evaluates the rule against every virtual copy; true when at least one
    /// copy matches. Version mismatches are reported loudly.
    pub fn matches_any_copy(&self, document: &SidecarDocument) -> Result<bool, SidecarError> {
        if self.version != SMART_COLLECTION_VERSION {
            return Err(SidecarError::Invalid(
                "unsupported smart_collection version".into(),
            ));
        }
        Ok(document.virtual_copies.iter().any(|copy| {
            self.rule
                .matches(&document.keywords, copy.rating, copy.flag)
        }))
    }
}

/// G-15 META-MVP (Slice 1): the metadata batch-operation language. Each
/// variant applies to one sidecar document via [`apply_batch_op`]; CLI/GUI
/// follow-up slices iterate it over sidecar files (one atomic write per
/// file). Mutations are limited to keywords, static collection memberships,
/// ratings and flags — recipes, masks and history are never touched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BatchOp {
    /// Adds `keyword` when absent (idempotent; present → unchanged).
    AddKeyword { keyword: String },
    /// Removes `keyword` when present (idempotent; absent → unchanged).
    RemoveKeyword { keyword: String },
    /// Adds membership `{ id, name }` when `id` is absent; refreshes `name`
    /// when the `id` already exists under a different name (rename propagation).
    AddToCollection { id: String, name: String },
    /// Removes membership `id` when present (idempotent).
    RemoveFromCollection { id: String },
    /// Sets the rating (`0..=5`) of one virtual copy.
    SetRating { copy_id: String, rating: u8 },
    /// Sets the flag of one virtual copy.
    SetFlag { copy_id: String, flag: Flag },
}

/// Applies one metadata batch operation to `document`. Returns `Ok(true)`
/// when the document changed and `Ok(false)` for idempotent no-ops. Every
/// invalid input (bad keyword, bad collection id/name, unknown `copy_id`,
/// `rating > 5`) fails loudly; a rejected operation leaves the document
/// unchanged.
pub fn apply_batch_op(document: &mut SidecarDocument, op: &BatchOp) -> Result<bool, SidecarError> {
    match op {
        BatchOp::AddKeyword { keyword } => {
            validate_keyword(keyword)?;
            if document.keywords.iter().any(|k| k == keyword) {
                return Ok(false);
            }
            if document.keywords.len() >= MAX_KEYWORDS_PER_DOCUMENT {
                return invalid(format!(
                    "keyword list exceeds limit of {MAX_KEYWORDS_PER_DOCUMENT}"
                ))
                .map(|()| false);
            }
            document.keywords.push(keyword.clone());
            Ok(true)
        }
        BatchOp::RemoveKeyword { keyword } => {
            validate_keyword(keyword)?;
            let before = document.keywords.len();
            document.keywords.retain(|k| k != keyword);
            Ok(document.keywords.len() != before)
        }
        BatchOp::AddToCollection { id, name } => {
            validate_collection_id(id)?;
            validate_collection_name(name)?;
            if let Some(existing) = document.collections.iter_mut().find(|m| m.id == *id) {
                if existing.name == *name {
                    return Ok(false);
                }
                existing.name = name.clone();
                return Ok(true);
            }
            if document.collections.len() >= MAX_COLLECTIONS_PER_DOCUMENT {
                return invalid(format!(
                    "collection list exceeds limit of {MAX_COLLECTIONS_PER_DOCUMENT}"
                ))
                .map(|()| false);
            }
            document.collections.push(CollectionMembership {
                id: id.clone(),
                name: name.clone(),
            });
            Ok(true)
        }
        BatchOp::RemoveFromCollection { id } => {
            validate_collection_id(id)?;
            let before = document.collections.len();
            document.collections.retain(|m| m.id != *id);
            Ok(document.collections.len() != before)
        }
        BatchOp::SetRating { copy_id, rating } => {
            if *rating > 5 {
                return invalid(format!(
                    "virtual copy `{copy_id}` rating must be 0..=5, got {rating}"
                ))
                .map(|()| false);
            }
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == *copy_id)
                .ok_or_else(|| {
                    SidecarError::Invalid(format!("unknown virtual copy `{copy_id}`"))
                })?;
            if copy.rating == *rating {
                return Ok(false);
            }
            copy.rating = *rating;
            Ok(true)
        }
        BatchOp::SetFlag { copy_id, flag } => {
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == *copy_id)
                .ok_or_else(|| {
                    SidecarError::Invalid(format!("unknown virtual copy `{copy_id}`"))
                })?;
            if copy.flag == *flag {
                return Ok(false);
            }
            copy.flag = *flag;
            Ok(true)
        }
    }
}

/// LRPAR-G15-IPTC-S1: one entry of the metadata draft history. The history
/// is provenance/diagnostic context — **not** an undo log. Entries are
/// stored newest-first (the first entry carries the highest `rev`);
/// [`SidecarDocument::apply_metadata_draft`] and the clear helpers prepend
/// exactly one entry per mutating call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataHistoryEntry {
    /// Strictly monotonous revision, starting at 1. Strictly decreasing in
    /// stored order (newest first).
    pub rev: u64,
    /// RFC 3339 UTC timestamp (`YYYY-MM-DDTHH:MM:SS[.frac]Z`), e.g.
    /// `2026-09-04T10:00:00Z`.
    pub timestamp: String,
    /// Mutation origin: `manual` | `cli` | `gui` | `mcp` |
    /// `preset:<name>` | `sync:<quell-sidecar-id>`.
    pub origin: String,
    /// Registry field IDs affected by the mutation (`keywords` allowed: a
    /// sync may carry keywords alongside draft fields). Sorted
    /// deterministically by the mutation helpers.
    pub changed: Vec<String>,
}

/// LRPAR-G15-IPTC-S1: the source-level IPTC metadata draft. Additive and
/// optional: absent in older sidecars reads as the empty draft (see the
/// `Default` impl) and an empty draft serializes back absent, so legacy
/// documents stay byte-stable. Drafts live on source-image level — never
/// per virtual copy — and sync/bake-in never touch recipes, masks or the
/// per-copy edit history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataDraft {
    #[serde(default = "metadata_draft_version_default")]
    pub version: u8,
    /// Draft values keyed by registry field ID. A stored value is always
    /// trimmed and non-empty; clearing a field removes its key (see
    /// [`SidecarDocument::apply_metadata_draft`]).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub draft: BTreeMap<String, String>,
    /// Provenance history, newest first, capped at
    /// [`MAX_METADATA_HISTORY_ENTRIES`] entries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<MetadataHistoryEntry>,
}

fn metadata_draft_version_default() -> u8 {
    METADATA_DRAFT_VERSION
}

impl Default for MetadataDraft {
    fn default() -> Self {
        Self {
            version: METADATA_DRAFT_VERSION,
            draft: BTreeMap::new(),
            history: Vec::new(),
        }
    }
}

impl MetadataDraft {
    /// True when both the draft map and the history are empty — the legacy
    /// identity, serialized back absent on `SidecarDocument`.
    pub fn is_empty(&self) -> bool {
        self.draft.is_empty() && self.history.is_empty()
    }

    /// Current draft value of `field`, if set.
    pub fn get(&self, field: &str) -> Option<&str> {
        self.draft.get(field).map(String::as_str)
    }

    /// Highest `rev` in the history, or 0 when the history is empty. The
    /// next mutation entry carries `latest_rev() + 1`.
    pub fn latest_rev(&self) -> u64 {
        self.history.first().map(|entry| entry.rev).unwrap_or(0)
    }

    fn validate(&self) -> Result<(), SidecarError> {
        if self.version != METADATA_DRAFT_VERSION {
            return invalid(format!(
                "unsupported metadata version {} (expected {METADATA_DRAFT_VERSION})",
                self.version
            ));
        }
        for (field, value) in &self.draft {
            validate_metadata_field_id(field)?;
            validate_metadata_stored_value(field, value)?;
        }
        if self.history.len() > MAX_METADATA_HISTORY_ENTRIES {
            return invalid(format!(
                "metadata history exceeds limit of {MAX_METADATA_HISTORY_ENTRIES} entries"
            ));
        }
        let mut previous_rev: Option<u64> = None;
        for entry in &self.history {
            if entry.rev == 0 {
                return invalid("metadata history rev must start at 1");
            }
            if let Some(previous) = previous_rev {
                if entry.rev >= previous {
                    return invalid(
                        "metadata history rev must be strictly decreasing (newest first)",
                    );
                }
            }
            previous_rev = Some(entry.rev);
            validate_metadata_timestamp(&entry.timestamp)?;
            validate_metadata_origin(&entry.origin)?;
            if entry.changed.is_empty() {
                return invalid(format!(
                    "metadata history rev {} must list at least one changed field",
                    entry.rev
                ));
            }
            let mut seen = BTreeSet::new();
            for field in &entry.changed {
                validate_metadata_changed_id(field)?;
                if !seen.insert(field) {
                    return invalid(format!(
                        "metadata history rev {} lists duplicate changed field `{field}`",
                        entry.rev
                    ));
                }
            }
        }
        Ok(())
    }
}

/// LRPAR-G15-IPTC-S1: true for the 11 registered draft field IDs (SOLL §4).
/// `keywords` is *not* a draft field (see [`METADATA_FIELD_IDS`]).
pub fn is_metadata_field(id: &str) -> bool {
    METADATA_FIELD_IDS.contains(&id)
}

/// LRPAR-G15-IPTC-S1: Sidecar character limit of a registered draft field,
/// or `None` for `date_created` (format-constrained instead). Returns `None`
/// for unknown IDs as well — callers must reject those loudly via
/// [`validate_metadata_field_id`].
pub fn metadata_field_limit(field: &str) -> Option<usize> {
    match field {
        "title" => Some(MAX_METADATA_TITLE_CHARS),
        "headline" => Some(MAX_METADATA_HEADLINE_CHARS),
        "description" => Some(MAX_METADATA_DESCRIPTION_CHARS),
        "copyright_notice" => Some(MAX_METADATA_COPYRIGHT_NOTICE_CHARS),
        "creator" => Some(MAX_METADATA_CREATOR_CHARS),
        "credit" => Some(MAX_METADATA_CREDIT_CHARS),
        "source" => Some(MAX_METADATA_SOURCE_CHARS),
        "city" => Some(MAX_METADATA_CITY_CHARS),
        "state_province" => Some(MAX_METADATA_STATE_PROVINCE_CHARS),
        "country" => Some(MAX_METADATA_COUNTRY_CHARS),
        "date_created" => None,
        _ => None,
    }
}

/// LRPAR-G15-IPTC-S1: rejects unknown draft field IDs loudly. `keywords`
/// gets an explicit routing hint instead of the generic unknown-ID error:
/// it stays the existing source-level sidecar field and is never duplicated
/// into the draft map.
fn validate_metadata_field_id(field: &str) -> Result<(), SidecarError> {
    if is_metadata_field(field) {
        return Ok(());
    }
    if field == "keywords" {
        return invalid(
            "keywords is not a metadata draft field; use the document `keywords` field",
        );
    }
    invalid(format!("unknown metadata field `{field}`"))
}

/// LRPAR-G15-IPTC-S1: IDs allowed in a history `changed` list — the draft
/// registry plus `keywords` (a field-selective sync may carry keywords
/// alongside draft fields; see SOLL §6).
fn validate_metadata_changed_id(field: &str) -> Result<(), SidecarError> {
    if is_metadata_field(field) || field == "keywords" {
        return Ok(());
    }
    invalid(format!("unknown metadata changed field `{field}`"))
}

/// LRPAR-G15-IPTC-S1: validates a candidate new value *before* mutation.
/// An empty or whitespace-only value is accepted and means "remove the
/// field" (see [`SidecarDocument::apply_metadata_draft`]); anything else
/// must be trimmed, free of control characters, within the field limit, and
/// — for `date_created` — a valid `YYYY-MM-DD` date. Unknown IDs fail
/// loudly; nothing is ever normalized silently.
pub fn validate_metadata_field_value(field: &str, value: &str) -> Result<(), SidecarError> {
    validate_metadata_field_id(field)?;
    if value.is_empty() || value.trim().is_empty() {
        return Ok(());
    }
    if value != value.trim() {
        return invalid(format!(
            "metadata field `{field}` must be trimmed (no leading/trailing whitespace)"
        ));
    }
    if value.chars().any(char::is_control) {
        return invalid(format!(
            "metadata field `{field}` must not contain control characters"
        ));
    }
    if field == "date_created" {
        return validate_metadata_date_created(value);
    }
    if let Some(limit) = metadata_field_limit(field) {
        if value.chars().count() > limit {
            return invalid(format!(
                "metadata field `{field}` exceeds limit of {limit} characters"
            ));
        }
    }
    Ok(())
}

/// LRPAR-G15-IPTC-S1: validates an already-stored draft value. Unlike the
/// mutation input (where empty means "remove"), a persisted empty or
/// untrimmed value is a loud schema violation — helpers never write such
/// states, so their presence means hand-editing or corruption.
fn validate_metadata_stored_value(field: &str, value: &str) -> Result<(), SidecarError> {
    if value.is_empty() || value != value.trim() {
        return invalid(format!(
            "metadata field `{field}` must be stored trimmed and non-empty"
        ));
    }
    if value.chars().any(char::is_control) {
        return invalid(format!(
            "metadata field `{field}` must not contain control characters"
        ));
    }
    if field == "date_created" {
        return validate_metadata_date_created(value);
    }
    if let Some(limit) = metadata_field_limit(field) {
        if value.chars().count() > limit {
            return invalid(format!(
                "metadata field `{field}` exceeds limit of {limit} characters"
            ));
        }
    }
    Ok(())
}

/// LRPAR-G15-IPTC-S1: `date_created` accepts exactly `YYYY-MM-DD` with a
/// calendar-valid month/day (including leap years). Anything else —
/// datetimes, other separators, out-of-range months or impossible days —
/// fails loudly.
fn validate_metadata_date_created(value: &str) -> Result<(), SidecarError> {
    const HINT: &str = "metadata field `date_created` must use format `YYYY-MM-DD`";
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return invalid(HINT);
    }
    let digits = |range: std::ops::Range<usize>| -> Option<u32> {
        let slice = value.get(range)?;
        if !slice.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        slice.parse().ok()
    };
    let (Some(year), Some(month), Some(day)) = (digits(0..4), digits(5..7), digits(8..10)) else {
        return invalid(HINT);
    };
    if !(1..=12).contains(&month) {
        return invalid(HINT);
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day == 0 || day > max_day {
        return invalid(HINT);
    }
    Ok(())
}

/// LRPAR-G15-IPTC-S1: validates a history `origin`: `manual` | `cli` |
/// `gui` | `mcp` | `preset:<name>` | `sync:<quell-sidecar-id>`. The suffixed
/// forms require a trimmed, non-empty name without control characters; a
/// leading `/` is rejected so no absolute path can hide in `metadata`
/// (SOLL invariant "keine absoluten Pfade"). Unknown origins fail loudly.
pub fn validate_metadata_origin(origin: &str) -> Result<(), SidecarError> {
    const SIMPLE: &[&str] = &["manual", "cli", "gui", "mcp"];
    if SIMPLE.contains(&origin) {
        return Ok(());
    }
    for prefix in ["preset:", "sync:"] {
        if let Some(name) = origin.strip_prefix(prefix) {
            if name.is_empty()
                || name != name.trim()
                || name.chars().any(char::is_control)
                || name.starts_with('/')
            {
                return invalid(format!(
                    "metadata origin `{origin}` has an invalid `{prefix}` name \
                     (must be trimmed, non-empty, without control characters or leading `/`)"
                ));
            }
            return Ok(());
        }
    }
    invalid(format!(
        "unknown metadata origin `{origin}` \
         (expected `manual`|`cli`|`gui`|`mcp`|`preset:<name>`|`sync:<quell-id>`)"
    ))
}

/// LRPAR-G15-IPTC-S1: validates a history `timestamp` as canonical RFC 3339
/// UTC (`YYYY-MM-DDTHH:MM:SS[.frac]Z` with calendar-valid fields). Offsets
/// (`+02:00`) and offset-less datetimes are rejected loudly — history
/// timestamps are always UTC (`Z`).
pub fn validate_metadata_timestamp(timestamp: &str) -> Result<(), SidecarError> {
    const HINT: &str = "metadata timestamp must be RFC 3339 UTC (`YYYY-MM-DDTHH:MM:SSZ`)";
    let inner = timestamp
        .strip_suffix('Z')
        .ok_or_else(|| SidecarError::Invalid(HINT.into()))?;
    let (date, time) = inner
        .split_once('T')
        .ok_or_else(|| SidecarError::Invalid(HINT.into()))?;
    let date_parts: Vec<&str> = date.split('-').collect();
    let time_parts: Vec<&str> = time.split(':').collect();
    if date_parts.len() != 3 || time_parts.len() != 3 {
        return invalid(HINT);
    }
    let number = |part: &str, len: usize| -> Option<u32> {
        if part.len() != len || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    let (Some(year), Some(month), Some(day)) = (
        number(date_parts[0], 4),
        number(date_parts[1], 2),
        number(date_parts[2], 2),
    ) else {
        return invalid(HINT);
    };
    let (Some(hour), Some(minute)) = (number(time_parts[0], 2), number(time_parts[1], 2)) else {
        return invalid(HINT);
    };
    let second = match time_parts[2].split_once('.') {
        None => number(time_parts[2], 2),
        Some((secs, frac)) => {
            if frac.is_empty() || !frac.bytes().all(|b| b.is_ascii_digit()) {
                None
            } else {
                number(secs, 2)
            }
        }
    };
    let Some(second) = second else {
        return invalid(HINT);
    };
    if month == 0 || month > 12 || hour > 23 || minute > 59 || second > 59 {
        return invalid(HINT);
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day == 0 || day > max_day {
        return invalid(HINT);
    }
    Ok(())
}

/// LRPAR-G15-IPTC-S1: current UTC time as canonical RFC 3339 (`...Z`),
/// std-only (no date crate needed in this crate). Mutation helpers take an
/// explicit `timestamp` so tests stay deterministic; CLI/GUI/MCP slices use
/// this constructor for real mutations.
pub fn now_rfc3339_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let second_of_day = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        second_of_day / 3_600,
        (second_of_day % 3_600) / 60,
        second_of_day % 60
    )
}

/// Days since the Unix epoch → (year, month, day). Howard Hinnant's
/// `civil_from_days` algorithm; Euclidean division keeps pre-1970 inputs
/// well-defined (practically unreachable: falls back to the epoch).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = (if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    }) as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

impl SidecarDocument {
    /// LRPAR-G15-IPTC-S1: sets draft fields and records one history entry.
    /// `fields` maps registry field IDs to new values; an empty or
    /// whitespace-only value removes the field ("leer = Feld entfernen").
    /// All-or-nothing per call: any unknown ID or invalid value rejects the
    /// whole call and leaves the document unchanged. Returns `Ok(true)` when
    /// the draft changed (exactly one history entry prepended, `rev =
    /// latest + 1`, `changed` = actually affected IDs in sorted order) and
    /// `Ok(false)` for idempotent no-ops (no entry appended). Recipes,
    /// masks and per-copy edit history are never touched.
    pub fn apply_metadata_draft(
        &mut self,
        fields: &BTreeMap<String, String>,
        origin: &str,
        timestamp: &str,
    ) -> Result<bool, SidecarError> {
        validate_metadata_origin(origin)?;
        validate_metadata_timestamp(timestamp)?;
        for (field, value) in fields {
            validate_metadata_field_value(field, value)?;
        }
        let mut draft = self.metadata.draft.clone();
        let mut changed = BTreeSet::new();
        for (field, value) in fields {
            if value.is_empty() || value.trim().is_empty() {
                if draft.remove(field).is_some() {
                    changed.insert(field.clone());
                }
            } else if draft.get(field).map(String::as_str) != Some(value.as_str()) {
                draft.insert(field.clone(), value.clone());
                changed.insert(field.clone());
            }
        }
        if changed.is_empty() {
            return Ok(false);
        }
        self.metadata.draft = draft;
        self.push_metadata_history(origin, timestamp, changed.into_iter().collect());
        self.validate().map(|()| true)
    }

    /// LRPAR-G15-IPTC-S1: removes the given draft fields (unknown IDs fail
    /// loudly, all-or-nothing). `Ok(false)` when none of the fields was set
    /// (no history entry); otherwise one entry with the removed IDs.
    pub fn clear_metadata_fields(
        &mut self,
        fields: &[&str],
        origin: &str,
        timestamp: &str,
    ) -> Result<bool, SidecarError> {
        validate_metadata_origin(origin)?;
        validate_metadata_timestamp(timestamp)?;
        for field in fields {
            validate_metadata_field_id(field)?;
        }
        let mut removed = BTreeSet::new();
        for field in fields {
            if self.metadata.draft.remove(*field).is_some() {
                removed.insert((*field).to_string());
            }
        }
        if removed.is_empty() {
            return Ok(false);
        }
        self.push_metadata_history(origin, timestamp, removed.into_iter().collect());
        self.validate().map(|()| true)
    }

    /// LRPAR-G15-IPTC-S1: removes every draft field (`--all`; the history
    /// itself is kept and gains one entry listing the removed IDs).
    /// `Ok(false)` when the draft was already empty (history untouched).
    pub fn clear_metadata_draft(
        &mut self,
        origin: &str,
        timestamp: &str,
    ) -> Result<bool, SidecarError> {
        validate_metadata_origin(origin)?;
        validate_metadata_timestamp(timestamp)?;
        if self.metadata.draft.is_empty() {
            return Ok(false);
        }
        let removed: Vec<String> = self.metadata.draft.keys().cloned().collect();
        self.metadata.draft.clear();
        self.push_metadata_history(origin, timestamp, removed);
        self.validate().map(|()| true)
    }

    /// LRPAR-G15-IPTC-S1: explicitly clears the whole draft history (the
    /// only way to empty it; there is no undo). Idempotent: clearing an
    /// already-empty history is a no-op. Draft values are kept.
    pub fn clear_metadata_history(&mut self) {
        self.metadata.history.clear();
    }

    /// Prepends one history entry (`rev = latest + 1`) and enforces the FIFO
    /// cap deterministically (oldest entries fall off the end). Callers have
    /// validated `origin`/`timestamp` and computed a non-empty, sorted
    /// `changed` list.
    fn push_metadata_history(&mut self, origin: &str, timestamp: &str, changed: Vec<String>) {
        let rev = self.metadata.latest_rev() + 1;
        self.metadata.history.insert(
            0,
            MetadataHistoryEntry {
                rev,
                timestamp: timestamp.to_string(),
                origin: origin.to_string(),
                changed,
            },
        );
        self.metadata.history.truncate(MAX_METADATA_HISTORY_ENTRIES);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VirtualCopy {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    /// Lightroom-style star rating `0..=5` (`0` = unrated). Per virtual copy
    /// (LR-01); values `> 5` are rejected loudly by [`SidecarDocument::validate`].
    #[serde(default)]
    pub rating: u8,
    /// Lightroom-style pick flag (LR-01). Per virtual copy.
    #[serde(default)]
    pub flag: Flag,
    pub recipe: EditRecipe,
    pub mask_library: Vec<MaskDefinition>,
    pub mask_layers: Vec<MaskLayer>,
    pub history: Vec<HistoryEntry>,
    pub export_records: Vec<ExportRecord>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SidecarDocument {
    pub format: String,
    pub schema_version: u32,
    pub source: SourceIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_fingerprint: Option<AnalysisFingerprint>,
    pub pipeline_version: String,
    pub virtual_copies: Vec<VirtualCopy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted_virtual_copies: Vec<VirtualCopy>,
    pub presets: Vec<Preset>,
    /// G-15 META-MVP (Slice 1): source-level keywords describing the image
    /// content (shared by all virtual copies, like mask artefacts). Additive:
    /// absent in older sidecars reads as empty and serializes back absent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// G-15 META-MVP (Slice 1): source-level static collection memberships.
    /// Additive: absent in older sidecars reads as empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collections: Vec<CollectionMembership>,
    /// LRPAR-G15-IPTC-S1: source-level IPTC metadata draft (draft map plus
    /// its own provenance history, separate from the per-copy edit
    /// history). Additive: absent in older sidecars reads as the empty
    /// draft and an empty draft serializes back absent.
    #[serde(default, skip_serializing_if = "MetadataDraft::is_empty")]
    pub metadata: MetadataDraft,
    /// LRPAR-G12-FACE-20 (FACE-20-S1): optional source-level face analysis
    /// (detections, embeddings, clusters, person labels plus their identity
    /// and status). Additive: absent in older sidecars reads as `None` and an
    /// absent section serializes back absent (no schema bump, no migration).
    /// Shared by every virtual copy — local adjustments stay per copy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<FaceAnalysis>,
    /// LRPAR-G09-CULL-25: optional source-level culling proposal (assisted
    /// culling). Additive: absent is the valid "no proposal" state and
    /// serializes back absent. The proposal never writes rating/flag/label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub culling: Option<CullingSection>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

#[derive(Debug, Error, PartialEq)]
pub enum SidecarError {
    #[error("sidecar file is missing: {0}")]
    Missing(String),
    #[error("sidecar I/O failed while {operation} `{path}`: {message}")]
    Io {
        operation: String,
        path: String,
        message: String,
    },
    #[error("invalid sidecar JSON: {0}")]
    Json(String),
    #[error("invalid sidecar: {0}")]
    Invalid(String),
    #[error("sidecar changed concurrently: {0}")]
    Conflict(String),
    #[error("XMP is not supported by Lumina sidecar schema v1")]
    XmpUnsupported,
}

/// The result of cleaning up files left behind by an interrupted atomic write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryReport {
    pub removed_temporary_files: usize,
}

/// REVIEW-SIDECAR-CAS-1 / REVIEW-SIDECAR-ZDATA-1: the per-target write lock is
/// shared by every writer of a bundle file (JSON sidecar *and* `.lumina.zdata`)
/// so plain saves, compare-and-swap saves and read-modify-write appends all
/// serialize against each other. `pub(crate)` because `zdata` reuses it for the
/// `.zdata.lock`.
pub(crate) struct WriteLock {
    path: PathBuf,
}

impl Drop for WriteLock {
    fn drop(&mut self) {
        // A failed cleanup is deliberately ignored: the next writer can report
        // the lock as stale, and the actual sidecar remains untouched.
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStatus {
    Unchanged,
    SourceChanged,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactStatus {
    /// The artifact exists and passed every check this crate can perform.
    Available,
    /// The artifact file does not exist (or is not a regular file).
    Missing,
    /// REVIEW-SIDECAR-STATUS-1 / REVIEW-SIDECAR-FOLLOWUP-1: the artifact exists
    /// on disk but is not usable — it is smaller than the 8-byte container
    /// magic (including empty), exceeds the container size limit, declares a
    /// `.lumina.zdata` format without carrying the `LUMZDATA` magic, or (for
    /// containers this crate owns) fails parsing or BLAKE3 checksum
    /// verification. A corrupt artifact must never be reported as available;
    /// callers treat it like missing data with a visible message.
    Corrupt,
}

/// Returns the sidecar path immediately next to `source`.
pub fn sidecar_path_for(source: &Path) -> PathBuf {
    let filename = source
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    source.with_file_name(format!("{filename}.lumina.json"))
}

pub fn load_sidecar(path: &Path) -> Result<SidecarDocument, SidecarError> {
    // A temporary file is never a sidecar.  Remove only files with the exact
    // tempfile prefix used by save_sidecar; a partial file must not become a
    // valid document after a crash.
    recover_sidecar(path)?;
    // REVIEW-SIDECAR-N5: the size limit is enforced *before* the file is read
    // (metadata gate) and a second time on the bounded read itself, so an
    // oversized or sparse file can never be pulled into memory in full. The
    // metadata/open race is covered by the `take` bound, not by the stat.
    let metadata = fs::metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            SidecarError::Missing(path.display().to_string())
        } else {
            io_error("reading sidecar metadata", path, error)
        }
    })?;
    if metadata.len() > MAX_SIDECAR_BYTES as u64 {
        return Err(SidecarError::Invalid(format!(
            "sidecar exceeds size limit of {MAX_SIDECAR_BYTES} bytes: `{}`",
            path.display()
        )));
    }
    let mut file = fs::File::open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            SidecarError::Missing(path.display().to_string())
        } else {
            io_error("opening", path, error)
        }
    })?;
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(MAX_SIDECAR_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| io_error("reading", path, error))?;
    if bytes.len() > MAX_SIDECAR_BYTES {
        return Err(SidecarError::Invalid(format!(
            "sidecar exceeds size limit of {MAX_SIDECAR_BYTES} bytes: `{}`",
            path.display()
        )));
    }
    let json = String::from_utf8(bytes).map_err(|_| {
        SidecarError::Json(format!("sidecar is not valid UTF-8: `{}`", path.display()))
    })?;
    SidecarDocument::from_json(&json)
}

/// Validates before writing and replaces the destination only after the complete
/// temporary file has been flushed and synced. Output and sidecar are not a
/// two-file transaction (the architecture explicitly leaves that out of v1).
///
/// REVIEW-SIDECAR-CAS-1: this is a *serialized* write — it takes the same
/// per-sidecar write lock as [`save_sidecar_if_unchanged`] and
/// [`migrate_sidecar_file`], so a plain save can no longer race an in-flight
/// compare-and-swap into a lost update. Contract for all writers: every JSON
/// sidecar write must go through `save_sidecar`, `save_sidecar_if_unchanged`
/// or `migrate_sidecar_file`; direct filesystem writes bypass conflict
/// detection entirely and are not supported.
pub fn save_sidecar(path: &Path, document: &SidecarDocument) -> Result<(), SidecarError> {
    let _lock = acquire_write_lock(path)?;
    save_sidecar_locked(path, document)
}

/// The actual atomic replace. Caller must hold the sidecar's write lock; the
/// lock deliberately lives with the public wrappers so the check-then-write
/// window of the compare-and-swap path stays inside one critical section.
fn save_sidecar_locked(path: &Path, document: &SidecarDocument) -> Result<(), SidecarError> {
    let json = document.to_json()?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let filename = path.file_name().map(|name| name.to_string_lossy());
    let filename = filename.as_deref().unwrap_or("sidecar");
    let mut temporary = tempfile::Builder::new()
        .prefix(&format!(".{filename}.tmp-"))
        .tempfile_in(parent)
        .map_err(|error| io_error("creating temporary file", parent, error))?;
    let temporary_path = temporary.path().to_path_buf();
    let result = (|| {
        temporary
            .write_all(json.as_bytes())
            .map_err(|error| io_error("writing temporary file", &temporary_path, error))?;
        temporary
            .flush()
            .map_err(|error| io_error("flushing temporary file", &temporary_path, error))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| io_error("syncing temporary file", &temporary_path, error))?;
        temporary
            .persist(path)
            .map_err(|error| io_error("renaming temporary file", path, error.error))?;
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| io_error("syncing sidecar directory", parent, error))?;
        Ok(())
    })();
    result
}

/// Write `bytes` to `path` atomically: a temporary file is created in the same
/// directory, fully written, flushed and synced, then renamed over the target.
///
/// Both the CLI and the GUI reuse this so the atomic-write semantics (and the
/// `.{name}.tmp-*` naming) are identical everywhere; only the sidecar write
/// additionally syncs the parent directory afterwards (see [`save_sidecar`]).
pub fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), SidecarError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let filename = path.file_name().map(|name| name.to_string_lossy());
    let filename = filename.as_deref().unwrap_or("artifact");
    let mut temporary = tempfile::Builder::new()
        .prefix(&format!(".{filename}.tmp-"))
        .tempfile_in(parent)
        .map_err(|error| io_error("creating temporary file", parent, error))?;
    let temporary_path = temporary.path().to_path_buf();
    let result = (|| {
        temporary
            .write_all(bytes)
            .map_err(|error| io_error("writing temporary file", &temporary_path, error))?;
        temporary
            .flush()
            .map_err(|error| io_error("flushing temporary file", &temporary_path, error))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| io_error("syncing temporary file", &temporary_path, error))?;
        temporary
            .persist(path)
            .map_err(|error| io_error("renaming temporary file", path, error.error))?;
        Ok(())
    })();
    result
}

/// Returns `true` if `input` and `output` resolve to the same file on disk.
///
/// Used by the export paths to reject an export that would overwrite the
/// original source (the non-destructive guarantee). Existing paths are
/// canonicalized directly; a not-yet-existing `output` is resolved against its
/// parent directory so a write to the source's own name is still caught.
pub fn paths_resolve_equal(input: &Path, output: &Path) -> std::io::Result<bool> {
    let input = fs::canonicalize(input)?;
    let output = if output.exists() {
        fs::canonicalize(output)?
    } else {
        let parent = output.parent().unwrap_or_else(|| Path::new("."));
        fs::canonicalize(parent)?.join(output.file_name().unwrap_or_default())
    };
    Ok(input == output)
}

pub fn save_sidecar_if_unchanged(
    path: &Path,
    document: &SidecarDocument,
    expected_revision: Option<&str>,
) -> Result<String, SidecarError> {
    let _lock = acquire_write_lock(path)?;
    if let Some(expected) = expected_revision {
        if !path.exists() {
            return Err(SidecarError::Conflict(format!(
                "sidecar disappeared: `{}`",
                path.display()
            )));
        }
        let current = document_revision(&load_sidecar(path)?)?;
        if current != expected {
            return Err(SidecarError::Conflict(path.display().to_string()));
        }
    } else if path.exists() {
        return Err(SidecarError::Conflict(format!(
            "sidecar already exists: `{}`; an expected revision is required",
            path.display()
        )));
    }
    // Already holding the lock: use the unlocked inner write (the public
    // `save_sidecar` would deadlock on the non-reentrant lock).
    save_sidecar_locked(path, document)?;
    document_revision(document)
}

pub fn document_revision(document: &SidecarDocument) -> Result<String, SidecarError> {
    let json = document.to_json()?;
    Ok(blake3::hash(json.as_bytes()).to_hex().to_string())
}

/// Remove orphaned atomic-write temporaries.  The destination is never
/// touched, and temporary contents are never parsed or promoted.
///
/// REVIEW-SIDECAR-TMP-1: only temporaries that are demonstrably older than
/// [`TEMP_SWEEP_AGE`] are removed. A live writer keeps its temporary fresh, so
/// a concurrent reader running recovery can no longer delete the temporary out
/// from under an in-flight save (which previously made the writer's atomic
/// rename fail). A temporary whose modification time cannot be determined is
/// deliberately *kept*: an un-swept leftover is harmless clutter, while
/// deleting a live writer's temporary loses data. Leftovers younger than the
/// threshold are swept by a later recovery once they have aged past it.
pub fn recover_sidecar(path: &Path) -> Result<RecoveryReport, SidecarError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let filename = path.file_name().map(|name| name.to_string_lossy());
    // Keep the prefix narrow: unrelated temporary files must survive recovery.
    let prefix = format!(".{}.tmp-", filename.as_deref().unwrap_or("sidecar"));
    let entries = fs::read_dir(parent)
        .map_err(|error| io_error("reading recovery directory", parent, error))?;
    let mut removed = 0;
    for entry in entries {
        let entry = entry.map_err(|error| io_error("reading recovery entry", parent, error))?;
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(&prefix) || !entry.path().is_file() {
            continue;
        }
        // REVIEW-SIDECAR-TMP-1: skip temporaries that may belong to a live
        // writer (fresh modification time or undeterminable metadata).
        let orphaned = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age > TEMP_SWEEP_AGE);
        if !orphaned {
            continue;
        }
        fs::remove_file(entry.path())
            .map_err(|error| io_error("removing orphaned temporary file", &entry.path(), error))?;
        removed += 1;
    }
    Ok(RecoveryReport {
        removed_temporary_files: removed,
    })
}

pub(crate) fn acquire_write_lock(path: &Path) -> Result<WriteLock, SidecarError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let filename = path.file_name().map(|name| name.to_string_lossy());
    let lock_path = parent.join(format!(
        ".{}.lock",
        filename.as_deref().unwrap_or("sidecar")
    ));
    const STALE_THRESHOLD: Duration = Duration::from_secs(30);
    for iteration in 0..100 {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(_) => return Ok(WriteLock { path: lock_path }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                // Crash recovery for a lock cannot identify a process portably;
                // only reclaim locks that are clearly abandoned. The stale
                // decision and the reclaim must be atomic: two processes must
                // not both decide "stale" and have the second delete the fresh
                // lock the first just created (TOCTOU lost update). We achieve
                // atomicity via an atomic rename: the stale lock is moved aside
                // to a unique reclaim path. Only the winner of the rename owns
                // the old lock file and can decide whether it was genuinely
                // stale; a fresh lock is restored instead of silently deleted,
                // producing an explicit Conflict for the contender.
                let stale = fs::metadata(&lock_path)
                    .and_then(|meta| meta.modified())
                    .ok()
                    .and_then(|time| SystemTime::now().duration_since(time).ok())
                    .is_some_and(|age| age > STALE_THRESHOLD);
                if !stale {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                // Generate a unique reclaim destination in the same directory so
                // the rename stays on the same filesystem and remains atomic.
                let reclaim_path = parent.join(format!(
                    ".{}.lock.reclaim-{}-{}-{}",
                    filename.as_deref().unwrap_or("sidecar"),
                    std::process::id(),
                    iteration,
                    SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos()
                ));
                match fs::rename(&lock_path, &reclaim_path) {
                    Ok(()) => {
                        // We now own the old lock file at `reclaim_path`.
                        // Verify it was genuinely stale at the instant of the
                        // atomic rename (its mtime is preserved across rename).
                        let reclaimed_stale = fs::metadata(&reclaim_path)
                            .and_then(|meta| meta.modified())
                            .ok()
                            .and_then(|time| SystemTime::now().duration_since(time).ok())
                            .is_some_and(|age| age > STALE_THRESHOLD);
                        if reclaimed_stale {
                            // Genuinely abandoned -> discard and retry creation.
                            let _ = fs::remove_file(&reclaim_path);
                            continue;
                        } else {
                            // We raced and stole a fresh lock. Restore it
                            // instead of silently deleting it; the contender
                            // must see an explicit Conflict, not a lost update.
                            match fs::rename(&reclaim_path, &lock_path) {
                                Ok(()) => {
                                    thread::sleep(Duration::from_millis(10));
                                    continue;
                                }
                                Err(_) => {
                                    // Another writer already created a new lock
                                    // while we held the stolen fresh file.
                                    // Discard the stolen copy and contend.
                                    let _ = fs::remove_file(&reclaim_path);
                                    thread::sleep(Duration::from_millis(10));
                                    continue;
                                }
                            }
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        // Lock vanished between the stale check and the rename
                        // (another reclaimer won). Retry creation immediately.
                        continue;
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                }
            }
            Err(error) => return Err(io_error("creating sidecar lock", &lock_path, error)),
        }
    }
    Err(SidecarError::Conflict(format!(
        "sidecar is locked: `{}`",
        path.display()
    )))
}

pub fn source_status(path: &Path, source: &SourceIdentity) -> Result<SourceStatus, SidecarError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SourceStatus::Missing)
        }
        Err(error) => return Err(io_error("reading source", path, error)),
    };
    let metadata =
        fs::metadata(path).map_err(|error| io_error("reading source metadata", path, error))?;
    let hash = format!("blake3:{}", blake3::hash(&bytes).to_hex());
    if metadata.len() == source.byte_length && hash == source.content_hash {
        Ok(SourceStatus::Unchanged)
    } else {
        Ok(SourceStatus::SourceChanged)
    }
}

/// Container magic of `.lumina.zdata` bundles. Duplicated from the
/// feature-gated `zdata` module so the structural checks below work in every
/// build.
const ZDATA_MAGIC: [u8; 8] = *b"LUMZDATA";

/// REVIEW-SIDECAR-FOLLOWUP-1: every format name used for `.lumina.zdata`
/// payloads embeds "zdata" (`"zdata"`, `"zdata-mask"`, `"lumina-zdata"`), so a
/// substring match covers all producer spellings without maintaining a list.
/// A reference declaring such a format must carry the container magic; that
/// errs toward visibility (reporting corrupt) instead of silently treating a
/// mislabeled file as an unverifiable opaque payload.
fn format_declares_zdata(format: &str) -> bool {
    format.contains("zdata")
}

/// Outcome of the build-independent file checks shared by both
/// `artifact_status` variants.
enum BasicArtifactCheck {
    /// Absent or not a regular file.
    Missing,
    /// Exists but can never be an artifact this crate persists.
    Corrupt,
    /// Passed the structural checks; payload verification may follow.
    Usable(PathBuf),
}

/// Existence/size floor applied identically with and without the `zdata`
/// feature (REVIEW-SIDECAR-STATUS-1 + REVIEW-SIDECAR-FOLLOWUP-1):
///
/// * absent or non-regular paths are `Missing`,
/// * files smaller than the 8-byte container magic — including empty ones —
///   are `Corrupt`; previously a non-empty <8-byte file slipped through the
///   failed magic read as `Available`,
/// * everything else is handed on with its resolved path.
fn basic_artifact_file_check(
    bundle_root: &Path,
    artifact: &ArtifactReference,
) -> BasicArtifactCheck {
    let path = bundle_root.join(&artifact.relative_path);
    let Ok(metadata) = fs::metadata(&path) else {
        return BasicArtifactCheck::Missing;
    };
    if !metadata.is_file() {
        return BasicArtifactCheck::Missing;
    }
    if metadata.len() < ZDATA_MAGIC.len() as u64 {
        return BasicArtifactCheck::Corrupt;
    }
    BasicArtifactCheck::Usable(path)
}

/// Checks whether the referenced artifact is usable.
///
/// REVIEW-SIDECAR-STATUS-1 / REVIEW-SIDECAR-FOLLOWUP-1: existence alone is no
/// longer sufficient. The check performs, in order:
///
/// 1. `Missing` when the path is absent or not a regular file,
/// 2. `Corrupt` for files smaller than the 8-byte container magic (an empty
///    or undersized artifact is never valid),
/// 3. `Corrupt` when the declared format names a `.lumina.zdata` payload but
///    the file lacks the `LUMZDATA` magic — such a file is mislabeled, not an
///    opaque payload,
/// 4. format-aware verification: files beginning with the `LUMZDATA` container
///    magic are parsed and their per-record BLAKE3 checksums verified eagerly;
///    any parse/checksum/size failure yields `Corrupt`,
/// 5. `Available` otherwise.
///
/// Known limitation (documented SOLL gap, not a silent fallback): opaque
/// non-container payloads cannot be deep-verified because this crate owns no
/// parser for them; they are reported as `Available` once they pass checks
/// 1–2. The reference's declared resolution (`width`/`height`) describes the
/// mask plane and has no generic mapping onto bundle records either; see
/// `feature/architecture/sidecar.md` ("Artefaktstatus-Prüfung") for why this
/// validation belongs to the consuming pipeline loader, not this function.
/// The deep (codec) verification variant is compiled when the
/// `.lumina.zdata` codec is available, i.e. with the `zdata` feature; the
/// structural-only variant below takes over without it so `artifact_status`
/// keeps a definition in every build configuration.
#[cfg(feature = "zdata")]
pub fn artifact_status(bundle_root: &Path, artifact: &ArtifactReference) -> ArtifactStatus {
    let path = match basic_artifact_file_check(bundle_root, artifact) {
        BasicArtifactCheck::Missing => return ArtifactStatus::Missing,
        BasicArtifactCheck::Corrupt => return ArtifactStatus::Corrupt,
        BasicArtifactCheck::Usable(path) => path,
    };
    let has_magic = starts_with_zdata_magic(&path);
    if !has_magic && format_declares_zdata(&artifact.format) {
        return ArtifactStatus::Corrupt;
    }
    if has_magic {
        return match load_zdata(&path) {
            Ok(_) => ArtifactStatus::Available,
            // An oversized/truncated/corrupt container must not count as
            // available just because the file exists.
            Err(_) => ArtifactStatus::Corrupt,
        };
    }
    ArtifactStatus::Available
}

/// Non-zdata build: without the container codec a magic-bearing file cannot
/// be deep-verified here; it
/// counts as usable once it passes the structural checks (documented
/// limitation, not a silent fallback — the same rules as the `zdata` build
/// apply up to the eager checksum pass).
#[cfg(not(feature = "zdata"))]
pub fn artifact_status(bundle_root: &Path, artifact: &ArtifactReference) -> ArtifactStatus {
    let path = match basic_artifact_file_check(bundle_root, artifact) {
        BasicArtifactCheck::Missing => return ArtifactStatus::Missing,
        BasicArtifactCheck::Corrupt => return ArtifactStatus::Corrupt,
        BasicArtifactCheck::Usable(path) => path,
    };
    if !starts_with_zdata_magic(&path) && format_declares_zdata(&artifact.format) {
        return ArtifactStatus::Corrupt;
    }
    ArtifactStatus::Available
}

/// Reads at most 8 bytes and reports whether the file begins with the zdata
/// container magic (`LUMZDATA`), so only containers this crate owns get the
/// (comparatively expensive) full parse+checksum verification. Compiled
/// without the `zdata` feature too: the magic check itself needs only
/// [`std::fs`] and keeps both `artifact_status` builds consistent.
fn starts_with_zdata_magic(path: &Path) -> bool {
    use std::io::Read;
    let mut magic = [0u8; 8];
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    file.read_exact(&mut magic).is_ok() && magic == ZDATA_MAGIC
}

/// GEN-EXPAND-CACHE-1: combined status of a persisted generative artifact.
///
/// Unlike the generic [`ArtifactStatus`] this also verifies the stored
/// generative identity against the current one. A record generated for a
/// different source/recipe/seed/canvas (or a legacy record without a pinned
/// identity) is reported as [`GenerativeArtifactStatus::Stale`] instead of
/// `Available` — the caller must never serve it silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerativeArtifactStatus {
    /// Bundle record exists, passed every checksum check and its pinned
    /// identity matches the current generative identity.
    Available,
    /// The bundle file does not exist (or is not a regular file).
    Missing,
    /// The bundle exists but is unusable (bad magic/version/checksum).
    Corrupt,
    /// The bundle is intact but was produced for a different generative
    /// identity (or carries no verifiable identity at all).
    Stale,
}

/// GEN-EXPAND-CACHE-1: identity-verified status of a generative bundle link.
///
/// `current_identity` is the digest of the generative operation the caller is
/// about to render (for the core cache this is the
/// `GenerativeCacheKey::digest()` of the current source/recipe/seed/canvas).
/// The function never returns `Available` for a mismatching or unverifiable
/// identity, so a stale persisted canvas can never be served silently.
pub fn generative_artifact_status(
    bundle_root: &Path,
    link: &GenerativeArtifactRef,
    current_identity: &str,
) -> GenerativeArtifactStatus {
    match link.artifact_status(bundle_root) {
        ArtifactStatus::Missing => GenerativeArtifactStatus::Missing,
        ArtifactStatus::Corrupt => GenerativeArtifactStatus::Corrupt,
        ArtifactStatus::Available => {
            if link.identity() == Some(current_identity) {
                GenerativeArtifactStatus::Available
            } else {
                GenerativeArtifactStatus::Stale
            }
        }
    }
}

pub fn xmp_supported() -> bool {
    false
}

/// Explicit, non-writing migration preview.
///
/// REVIEW-SIDECAR-N2 (documented decision, 2026-08-25): the historical
/// `schema_version` 0 → 1 bump lives *only* here (and in
/// [`migrate_sidecar_file`]). The loader (`SidecarDocument::from_json`) never
/// normalizes v0 silently — it rejects with an explicit migration hint. The
/// bump is a one-time historical accommodation for pre-release documents that
/// predate the versioned schema stamp; it is applied only after an explicit
/// migration request, with the usual backup + atomic-replace semantics in the
/// file-level path.
pub fn migrate_json(json: &str) -> Result<String, SidecarError> {
    let mut value: Value =
        serde_json::from_str(json).map_err(|e| SidecarError::Json(e.to_string()))?;
    let version = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| SidecarError::Invalid("missing schema_version".into()))?;
    if version > u64::from(SCHEMA_VERSION) {
        return Err(SidecarError::Invalid(format!(
            "unsupported schema_version {version}; explicit migration is required"
        )));
    }
    if version == 0 {
        value["schema_version"] = Value::from(1);
    }
    if value["schema_version"].as_u64() == Some(1) {
        value["schema_version"] = Value::from(2);
        // v1's flat map is deliberately retained; only the schema stamp changes.
    }
    let document = SidecarDocument::from_json(&serde_json::to_string(&value).unwrap())?;
    document.to_json()
}

/// Apply a pending migration only when the caller explicitly invokes this
/// operation. The original is backed up before the atomically replaced result
/// is installed. `migrate_json` remains a non-writing migration preview.
pub fn migrate_sidecar_file(path: &Path) -> Result<bool, SidecarError> {
    let _lock = acquire_write_lock(path)?;
    let original =
        fs::read(path).map_err(|error| io_error("reading sidecar for migration", path, error))?;
    let migrated = migrate_json(
        std::str::from_utf8(&original).map_err(|error| SidecarError::Json(error.to_string()))?,
    )?;
    if migrated.as_bytes() == original {
        return Ok(false);
    }
    let backup = PathBuf::from(format!("{}.bak", path.display()));
    atomic_write_bytes(&backup, &original)?;
    atomic_write_bytes(path, migrated.as_bytes())?;
    Ok(true)
}

fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<(), SidecarError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    // REVIEW-SIDECAR-N1: use the same `.{name}.tmp-` prefix as every other
    // atomic write in this crate. The crate-default tempfile name was invisible
    // to `recover_sidecar`, so an interrupted migration leaked a temporary that
    // the recovery sweep could never identify or clean up.
    let filename = path.file_name().map(|name| name.to_string_lossy());
    let filename = filename.as_deref().unwrap_or("migration");
    let mut temporary = tempfile::Builder::new()
        .prefix(&format!(".{filename}.tmp-"))
        .tempfile_in(parent)
        .map_err(|error| io_error("creating migration temporary file", parent, error))?;
    temporary
        .write_all(bytes)
        .map_err(|error| io_error("writing migration temporary file", temporary.path(), error))?;
    temporary
        .flush()
        .map_err(|error| io_error("flushing migration temporary file", temporary.path(), error))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| io_error("syncing migration temporary file", temporary.path(), error))?;
    temporary
        .persist(path)
        .map_err(|error| io_error("renaming migration temporary file", path, error.error))?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error("syncing migration directory", parent, error))
}

fn io_error(operation: &str, path: &Path, error: std::io::Error) -> SidecarError {
    SidecarError::Io {
        operation: operation.into(),
        path: path.display().to_string(),
        message: error.to_string(),
    }
}

impl SidecarDocument {
    pub fn new(source: SourceIdentity, pipeline_version: impl Into<String>) -> Self {
        Self {
            format: FORMAT.into(),
            schema_version: SCHEMA_VERSION,
            source,
            analysis_fingerprint: None,
            pipeline_version: pipeline_version.into(),
            virtual_copies: vec![VirtualCopy {
                id: "vc-original".into(),
                name: "Original".into(),
                is_default: true,
                rating: 0,
                flag: Flag::Unflagged,
                recipe: EditRecipe::default(),
                mask_library: vec![],
                mask_layers: vec![],
                history: vec![],
                export_records: vec![],
                extras: Extras::new(),
            }],
            deleted_virtual_copies: vec![],
            presets: vec![],
            keywords: vec![],
            collections: vec![],
            metadata: MetadataDraft::default(),
            face: None,
            culling: None,
            extras: Extras::new(),
        }
    }

    pub fn to_json(&self) -> Result<String, SidecarError> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|e| SidecarError::Json(e.to_string()))
    }

    pub fn from_json(json: &str) -> Result<Self, SidecarError> {
        if json.len() > MAX_SIDECAR_BYTES {
            return Err(SidecarError::Invalid("sidecar exceeds size limit".into()));
        }
        let value: Value =
            serde_json::from_str(json).map_err(|e| SidecarError::Json(e.to_string()))?;
        let version = value
            .get("schema_version")
            .and_then(Value::as_u64)
            .ok_or_else(|| SidecarError::Invalid("missing schema_version".into()))?;
        // REVIEW-SIDECAR-N2 (documented decision, 2026-08-25): the historical
        // `schema_version` 0 → 1 bump is *migration-only*. The loader never
        // normalizes silently; a v0 document is rejected loudly and points at
        // the explicit migration path (`migrate_json` / `migrate_sidecar_file`),
        // keeping `from_json` and the migration machinery consistent. This
        // matches the pre-alpha rule that incompatible documents fail with a
        // visible error instead of a best-effort interpretation.
        if version == 0 {
            return Err(SidecarError::Invalid(
                "unsupported schema_version 0; explicit migration is required \
                 (see migrate_sidecar_file)"
                    .into(),
            ));
        }
        if version != 1 && version != u64::from(SCHEMA_VERSION) {
            return Err(SidecarError::Invalid(format!(
                "unsupported schema_version {version}; explicit migration is required"
            )));
        }
        let document: Self =
            serde_json::from_value(value).map_err(|e| SidecarError::Json(e.to_string()))?;
        document.validate()?;
        Ok(document)
    }

    pub fn duplicate_virtual_copy(
        &mut self,
        source_id: &str,
        new_id: impl Into<String>,
        new_name: impl Into<String>,
    ) -> Result<(), SidecarError> {
        let mut copy = self
            .virtual_copies
            .iter()
            .find(|copy| copy.id == source_id)
            .cloned()
            .ok_or_else(|| SidecarError::Invalid(format!("unknown virtual copy `{source_id}`")))?;
        copy.id = new_id.into();
        copy.name = new_name.into();
        copy.is_default = false;
        if self
            .virtual_copies
            .iter()
            .any(|candidate| candidate.id == copy.id)
            || self
                .deleted_virtual_copies
                .iter()
                .any(|candidate| candidate.id == copy.id)
        {
            return invalid(format!("duplicate virtual copy id `{}`", copy.id));
        }
        // REVIEW-SIDECAR-N4: the document is validated with the copy in place,
        // but a failed validation rolls the insertion back so `self` stays
        // exactly as it was before the rejected call.
        self.virtual_copies.push(copy);
        if let Err(error) = self.validate() {
            self.virtual_copies.pop();
            return Err(error);
        }
        Ok(())
    }

    pub fn rename_virtual_copy(
        &mut self,
        id: &str,
        name: impl Into<String>,
    ) -> Result<(), SidecarError> {
        let name = name.into();
        // REVIEW-SIDECAR-N4: check the incoming value before mutating so an
        // invalid rename cannot leave a mutated document behind.
        validate_name("virtual copy name", &name)?;
        let copy = self
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == id)
            .ok_or_else(|| SidecarError::Invalid(format!("unknown virtual copy `{id}`")))?;
        copy.name = name;
        self.validate()
    }

    pub fn sort_virtual_copies(&mut self) {
        self.virtual_copies
            .sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    }

    pub fn delete_virtual_copy(&mut self, id: &str) -> Result<(), SidecarError> {
        if id == "vc-original" {
            return invalid("the default virtual copy cannot be deleted");
        }
        let index = self
            .virtual_copies
            .iter()
            .position(|copy| copy.id == id)
            .ok_or_else(|| SidecarError::Invalid(format!("unknown virtual copy `{id}`")))?;
        // REVIEW-SIDECAR-N4: validate the post-delete state *before* committing
        // it. Deleting a copy that still owns masks referenced from elsewhere
        // must fail loudly AND leave the document unchanged — previously the
        // copy had already been moved to `deleted_virtual_copies` when
        // validation failed, leaving callers with a mutated-but-invalid
        // document.
        let copy = self.virtual_copies.remove(index);
        if let Err(error) = self.validate() {
            // Roll back to the exact pre-call state.
            self.virtual_copies.insert(index, copy);
            return Err(error);
        }
        self.deleted_virtual_copies.push(copy);
        self.validate()
    }

    pub fn restore_virtual_copy(&mut self, id: &str) -> Result<(), SidecarError> {
        let index = self
            .deleted_virtual_copies
            .iter()
            .position(|copy| copy.id == id)
            .ok_or_else(|| SidecarError::Invalid(format!("unknown deleted virtual copy `{id}`")))?;
        // REVIEW-SIDECAR-N4: same validate-before-commit contract as
        // `delete_virtual_copy`; a rejected restore leaves the copy in the
        // deleted list at its original position.
        let copy = self.deleted_virtual_copies.remove(index);
        self.virtual_copies.push(copy);
        if let Err(error) = self.validate() {
            let copy = self.virtual_copies.pop();
            if let Some(copy) = copy {
                self.deleted_virtual_copies.insert(index, copy);
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), SidecarError> {
        if self.format != FORMAT {
            return invalid("format must be `lumina-sidecar`");
        }
        if self.schema_version != 1 && self.schema_version != SCHEMA_VERSION {
            return invalid("unsupported schema_version");
        }
        validate_name("pipeline_version", &self.pipeline_version)?;
        validate_name("source.relative_name", &self.source.relative_name)?;
        validate_relative_path("source.relative_name", &self.source.relative_name)?;
        if !(1..=8).contains(&self.source.orientation) {
            return invalid("source.orientation must be between 1 and 8");
        }
        // G-15 META-MVP (Slice 1): source-level metadata. Empty is the legacy
        // identity; present entries are validated loudly (no silent
        // deduplication, clamping or trimming).
        if self.keywords.len() > MAX_KEYWORDS_PER_DOCUMENT {
            return invalid(format!(
                "keyword list exceeds limit of {MAX_KEYWORDS_PER_DOCUMENT}"
            ));
        }
        {
            let mut seen = BTreeSet::new();
            for keyword in &self.keywords {
                validate_keyword(keyword)?;
                if !seen.insert(keyword) {
                    return invalid(format!("duplicate keyword `{keyword}`"));
                }
            }
        }
        if self.collections.len() > MAX_COLLECTIONS_PER_DOCUMENT {
            return invalid(format!(
                "collection list exceeds limit of {MAX_COLLECTIONS_PER_DOCUMENT}"
            ));
        }
        {
            let mut seen = BTreeSet::new();
            for membership in &self.collections {
                validate_collection_id(&membership.id)?;
                validate_collection_name(&membership.name)?;
                if !seen.insert(&membership.id) {
                    return invalid(format!("duplicate collection id `{}`", membership.id));
                }
            }
        }
        // LRPAR-G15-IPTC-S1: source-level metadata draft (own history,
        // separate from the per-copy edit history below).
        self.metadata.validate()?;
        // LRPAR-G12-FACE-20 (FACE-20-S1): source-level face analysis. Absent is
        // the valid legacy state; a present section is validated loudly,
        // including every detection ↔ embedding ↔ cluster ↔ person cross-link.
        if let Some(face) = &self.face {
            validate_face_analysis(face)?;
        }
        // LRPAR-G09-CULL-25: source-level culling proposal. Absent is the valid
        // "no proposal" state; a present section is validated loudly.
        if let Some(culling) = &self.culling {
            validate_culling_section(culling)?;
        }
        if self.virtual_copies.is_empty() {
            return invalid("at least one virtual copy is required");
        }
        if self.virtual_copies.len() + self.deleted_virtual_copies.len() > MAX_VIRTUAL_COPIES {
            return invalid("sidecar exceeds virtual copy limit");
        }
        let mut copy_ids = BTreeSet::new();
        let mut defaults = 0;
        for copy in &self.virtual_copies {
            validate_name("virtual copy id", &copy.id)?;
            validate_name("virtual copy name", &copy.name)?;
            validate_name("recipe_version", &copy.recipe.recipe_version)?;
            validate_adjustments(&copy.recipe)?;
            // LR-01: star ratings are 0..=5 (0 = unrated); anything else is a
            // schema violation, never silently clamped.
            if copy.rating > 5 {
                return invalid(format!(
                    "virtual copy `{}` rating must be 0..=5, got {}",
                    copy.id, copy.rating
                ));
            }
            if !copy_ids.insert(&copy.id) {
                return invalid(format!("duplicate virtual copy id `{}`", copy.id));
            }
            defaults += usize::from(copy.is_default);
            let mut mask_ids = BTreeSet::new();
            for mask in &copy.mask_library {
                validate_name("mask id", &mask.id)?;
                validate_name("mask name", &mask.name)?;
                validate_name("mask rescaling_method", &mask.rescaling_method)?;
                validate_name("mask generator_version", &mask.generator_version)?;
                // REVIEW-SIDECAR-N3: a zero-sized inference resolution can
                // never back a valid matte; reject it at the schema boundary.
                if mask.inference_resolution.width == 0 || mask.inference_resolution.height == 0 {
                    return invalid(format!(
                        "mask `{}/{}` inference_resolution must be non-zero",
                        copy.id, mask.id
                    ));
                }
                if !mask_ids.insert(&mask.id) {
                    return invalid(format!(
                        "duplicate mask id `{}` in copy `{}`",
                        mask.id, copy.id
                    ));
                }
                if let Some(a) = &mask.artifact {
                    validate_artifact(a)?;
                }
                validate_prompt(&mask.prompt)?;
                validate_ai_select(&mask.ai_select)?;
                // G-03: an AI selection names an automatic source matte, so it
                // is only meaningful on `source` nodes. Derived nodes combine
                // already-resolved mattes; stamping them `ai_select` would
                // silently change their meaning.
                if mask.ai_select.is_some() && mask.operation != MaskOperation::Source {
                    return invalid(format!(
                        "mask `{}/{}` carries ai_select on a non-source operation; ai_select requires `source`",
                        copy.id, mask.id
                    ));
                }
                // G-03: a range prompt is a deterministic source stage and
                // likewise never lives on a derived node.
                if mask.operation != MaskOperation::Source
                    && matches!(
                        mask.prompt,
                        Some(MaskPrompt::ColorRange { .. })
                            | Some(MaskPrompt::LuminanceRange { .. })
                    )
                {
                    return invalid(format!(
                        "mask `{}/{}` carries a range prompt on a non-source operation; range prompts require `source`",
                        copy.id, mask.id
                    ));
                }
                let arity_is_valid = match mask.operation {
                    MaskOperation::Source => mask.references.is_empty(),
                    MaskOperation::Invert => mask.references.len() == 1,
                    MaskOperation::Subtract => mask.references.len() == 2,
                    MaskOperation::Union | MaskOperation::Intersect => mask.references.len() >= 2,
                };
                if !arity_is_valid {
                    return invalid(format!(
                        "mask `{}/{}` operation `{}` has invalid input arity ({})",
                        copy.id,
                        mask.id,
                        serde_json::to_string(&mask.operation).unwrap_or_default(),
                        mask.references.len()
                    ));
                }
                for reference in &mask.references {
                    validate_name("mask reference copy_id", &reference.copy_id)?;
                    validate_name("mask reference mask_id", &reference.mask_id)?;
                }
            }
            for export in &copy.export_records {
                validate_name("export id", &export.id)?;
                validate_relative_path("export relative_path", &export.relative_path)?;
                validate_name("export format", &export.format)?;
            }
            validate_unique_ids("mask layer", &copy.mask_layers, |layer| &layer.id)?;
            validate_unique_ids("history entry", &copy.history, |entry| &entry.id)?;
            validate_unique_ids("export record", &copy.export_records, |export| &export.id)?;
            for layer in &copy.mask_layers {
                validate_name("mask layer id", &layer.id)?;
                // REVIEW-SIDECAR-N3: local adjustment parameters were persisted
                // without any finite/range validation. feather/blur are
                // radii-like quantities (>= 0), density is a normalized
                // 0..=1 opacity. Defaults (0/0/1) of every existing valid
                // sidecar satisfy these bounds.
                if !layer.feather.is_finite() || layer.feather < 0.0 {
                    return invalid(format!(
                        "mask layer `{}` feather must be finite and >= 0",
                        layer.id
                    ));
                }
                if !layer.blur.is_finite() || layer.blur < 0.0 {
                    return invalid(format!(
                        "mask layer `{}` blur must be finite and >= 0",
                        layer.id
                    ));
                }
                if !layer.density.is_finite() || !(0.0..=1.0).contains(&layer.density) {
                    return invalid(format!(
                        "mask layer `{}` density must be finite within 0..=1",
                        layer.id
                    ));
                }
            }
            for entry in &copy.history {
                validate_name("history entry id", &entry.id)?;
            }
        }
        for copy in &self.deleted_virtual_copies {
            validate_name("deleted virtual copy id", &copy.id)?;
            validate_name("deleted virtual copy name", &copy.name)?;
            validate_name("recipe_version", &copy.recipe.recipe_version)?;
            validate_adjustments(&copy.recipe)?;
            if !copy_ids.insert(&copy.id) {
                return invalid(format!("duplicate virtual copy id `{}`", copy.id));
            }
        }
        validate_unique_ids("preset", &self.presets, |preset| &preset.id)?;
        for preset in &self.presets {
            validate_name("preset id", &preset.id)?;
        }
        for copy in &self.virtual_copies {
            for layer in &copy.mask_layers {
                let target = self
                    .virtual_copies
                    .iter()
                    .find(|candidate| candidate.id == layer.mask.copy_id)
                    .ok_or_else(|| {
                        SidecarError::Invalid(format!(
                            "mask layer `{}` references unknown copy `{}`",
                            layer.id, layer.mask.copy_id
                        ))
                    })?;
                if !target
                    .mask_library
                    .iter()
                    .any(|mask| mask.id == layer.mask.mask_id)
                {
                    return invalid(format!(
                        "mask layer `{}` references unknown mask `{}/{}`",
                        layer.id, layer.mask.copy_id, layer.mask.mask_id
                    ));
                }
            }
        }
        if defaults != 1 {
            return invalid("exactly one default virtual copy is required");
        }
        if !self
            .virtual_copies
            .iter()
            .any(|c| c.id == "vc-original" && c.is_default)
        {
            return invalid("`vc-original` must be the default virtual copy");
        }
        for copy in &self.virtual_copies {
            if let Some(ge) = &copy.recipe.generative_edit {
                if ge.version != 1 {
                    return invalid("generative_edit.version must be 1");
                }
                let expand = ge.effective_expand();
                if expand && ge.canvas.is_none() {
                    return invalid("generative_edit.expand_beyond_image requires canvas");
                }
                if !expand && ge.canvas.is_some() {
                    // When expand is false, canvas must be None or equal to source (but we don't have source dims here)
                    // For test, any canvas with expand false is considered invalid if canvas is Some
                    // The strict rule: expand false => canvas must be None
                    // To keep lenient, we only reject when canvas is Some and expand is Some(false) explicitly
                    if ge.expand_beyond_image == Some(false) {
                        return invalid(
                            "generative_edit.canvas present but expand_beyond_image is false",
                        );
                    }
                }
                if let Some(canvas) = &ge.canvas {
                    canvas.validate()?;
                }
                ge.validate_edit_extras()?;
                if let Some(link) = &ge.artifact {
                    validate_generative_ref(link)?;
                }
            }
        }
        self.validate_mask_graph(&copy_ids)
    }

    fn validate_mask_graph(&self, copy_ids: &BTreeSet<&String>) -> Result<(), SidecarError> {
        let mut nodes = BTreeSet::new();
        let mut edges = BTreeMap::<(String, String), Vec<(String, String)>>::new();
        for copy in &self.virtual_copies {
            for mask in &copy.mask_library {
                let node = (copy.id.clone(), mask.id.clone());
                nodes.insert(node.clone());
                for reference in &mask.references {
                    edges
                        .entry(node.clone())
                        .or_default()
                        .push((reference.copy_id.clone(), reference.mask_id.clone()));
                }
            }
        }
        for (from, targets) in &edges {
            for target in targets {
                if !copy_ids.contains(&target.0) || !nodes.contains(target) {
                    return invalid(format!(
                        "mask `{}/{}' references unknown mask `{}/{}`",
                        from.0, from.1, target.0, target.1
                    ));
                }
                if from == target {
                    return invalid(format!("mask `{}/{}' references itself", from.0, from.1));
                }
            }
        }
        fn visit(
            node: &(String, String),
            edges: &BTreeMap<(String, String), Vec<(String, String)>>,
            visiting: &mut BTreeSet<(String, String)>,
            visited: &mut BTreeSet<(String, String)>,
        ) -> Result<(), SidecarError> {
            if visiting.contains(node) {
                return invalid(format!(
                    "mask graph contains a cycle at `{}/{}`",
                    node.0, node.1
                ));
            }
            if !visited.insert(node.clone()) {
                return Ok(());
            }
            visiting.insert(node.clone());
            if let Some(targets) = edges.get(node) {
                for target in targets {
                    visit(target, edges, visiting, visited)?;
                }
            }
            visiting.remove(node);
            Ok(())
        }
        let mut visiting = BTreeSet::new();
        let mut visited = BTreeSet::new();
        for node in nodes {
            visit(&node, &edges, &mut visiting, &mut visited)?;
        }
        Ok(())
    }
}

fn invalid(message: impl Into<String>) -> Result<(), SidecarError> {
    Err(SidecarError::Invalid(message.into()))
}
fn validate_name(field: &str, value: &str) -> Result<(), SidecarError> {
    if value.trim().is_empty() {
        invalid(format!("{field} must not be empty"))
    } else {
        Ok(())
    }
}

fn validate_unique_ids<T>(
    kind: &str,
    values: &[T],
    id: impl Fn(&T) -> &String,
) -> Result<(), SidecarError> {
    let mut ids = BTreeSet::new();
    for value in values {
        let value_id = id(value);
        if !ids.insert(value_id) {
            return invalid(format!("duplicate {kind} id `{value_id}`"));
        }
    }
    Ok(())
}

fn validate_relative_path(field: &str, value: &str) -> Result<(), SidecarError> {
    if value.is_empty()
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains(':')
        || value.starts_with("//")
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return invalid(format!("{field} must be a safe portable relative path"));
    }
    Ok(())
}
fn validate_artifact(a: &ArtifactReference) -> Result<(), SidecarError> {
    validate_relative_path("artifact relative_path", &a.relative_path)?;
    validate_name("artifact format", &a.format)?;
    validate_name("artifact checksum", &a.checksum)?;
    validate_name("artifact channels", &a.channels)?;
    validate_name("artifact data_version", &a.data_version)
}

/// G-15 META-MVP (Slice 1): a keyword must be stored trimmed and printable:
/// non-empty, without leading/trailing whitespace or control characters, and
/// within the character limit. Violations fail loudly — the loader never
/// trims, coerces or deduplicates silently.
fn validate_keyword(keyword: &str) -> Result<(), SidecarError> {
    if keyword.is_empty() || keyword.trim() != keyword {
        return invalid("keyword must be non-empty and without leading/trailing whitespace");
    }
    if keyword.chars().any(char::is_control) {
        return invalid("keyword must not contain control characters");
    }
    if keyword.chars().count() > MAX_KEYWORD_CHARS {
        return invalid(format!(
            "keyword exceeds limit of {MAX_KEYWORD_CHARS} characters"
        ));
    }
    Ok(())
}

/// G-15 META-MVP (Slice 1): collection ids are stable, portable identities —
/// never paths. Besides the non-empty/trimmed contract they forbid `/`, `\`
/// and `:` so a membership can never be mistaken for (or turned into) a path.
fn validate_collection_id(id: &str) -> Result<(), SidecarError> {
    if id.is_empty() || id.trim() != id {
        return invalid("collection id must be non-empty and without leading/trailing whitespace");
    }
    if id.contains('/') || id.contains('\\') || id.contains(':') {
        return invalid("collection id must not contain `/`, `\\` or `:`");
    }
    if id.chars().count() > MAX_COLLECTION_ID_CHARS {
        return invalid(format!(
            "collection id exceeds limit of {MAX_COLLECTION_ID_CHARS} characters"
        ));
    }
    Ok(())
}

fn validate_collection_name(name: &str) -> Result<(), SidecarError> {
    if name.is_empty() || name.trim() != name {
        return invalid(
            "collection name must be non-empty and without leading/trailing whitespace",
        );
    }
    if name.chars().count() > MAX_COLLECTION_NAME_CHARS {
        return invalid(format!(
            "collection name exceeds limit of {MAX_COLLECTION_NAME_CHARS} characters"
        ));
    }
    Ok(())
}

/// G-15 META-MVP (Slice 1): validates a smart-collection definition as data —
/// version pin plus a depth-bounded walk of the rule tree. Every unknown or
/// out-of-range part fails loudly; an empty `And`/`Or` is not a vacuous truth
/// but a schema violation. Public so catalogue-level persistence in follow-up
/// slices (CLI/GUI) validates definitions with the same rules.
pub fn validate_smart_collection_def(def: &SmartCollectionDef) -> Result<(), SidecarError> {
    if def.version != SMART_COLLECTION_VERSION {
        return invalid("unsupported smart_collection version");
    }
    validate_collection_id(&def.id)?;
    validate_collection_name(&def.name)?;
    validate_smart_rule(&def.rule, 0)
}

fn validate_smart_rule(rule: &SmartRule, depth: usize) -> Result<(), SidecarError> {
    if depth > MAX_SMART_RULE_DEPTH {
        return invalid(format!(
            "smart rule exceeds maximum nesting depth of {MAX_SMART_RULE_DEPTH}"
        ));
    }
    match rule {
        SmartRule::All | SmartRule::None => Ok(()),
        SmartRule::Keyword { keyword } => validate_keyword(keyword),
        SmartRule::RatingAtLeast { rating } | SmartRule::RatingEquals { rating } => {
            if *rating > 5 {
                return invalid(format!("smart rule rating must be 0..=5, got {rating}"));
            }
            Ok(())
        }
        SmartRule::Flag { .. } => Ok(()),
        SmartRule::And { rules } | SmartRule::Or { rules } => {
            if rules.is_empty() {
                return invalid("smart rule `and`/`or` requires at least one sub-rule");
            }
            for sub in rules {
                validate_smart_rule(sub, depth + 1)?;
            }
            Ok(())
        }
        SmartRule::Not { rule } => validate_smart_rule(rule, depth + 1),
    }
}

/// F-079: reject malformed prompt sources. Normalized coordinates must be
/// finite and within `0..=1`; `width`/`height` and brush `radius` must be
/// strictly positive; required point/mark lists must be non-empty.
fn validate_prompt(prompt: &Option<MaskPrompt>) -> Result<(), SidecarError> {
    let Some(prompt) = prompt else {
        return Ok(());
    };
    let in_unit = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
    match prompt {
        MaskPrompt::Box { rect, .. } => {
            if !in_unit(rect.x)
                || !in_unit(rect.y)
                || !in_unit(rect.width)
                || !in_unit(rect.height)
                || rect.width <= 0.0
                || rect.height <= 0.0
            {
                return invalid("prompt box must have finite normalized coordinates within 0..=1 with positive width/height");
            }
        }
        MaskPrompt::Brush { marks, .. } => {
            if marks.is_empty() {
                return invalid("prompt brush must contain at least one mark");
            }
            for mark in marks {
                if !in_unit(mark.x)
                    || !in_unit(mark.y)
                    || !in_unit(mark.radius)
                    || mark.radius <= 0.0
                {
                    return invalid(
                        "prompt brush marks must have finite normalized coordinates within 0..=1 with positive radius",
                    );
                }
            }
        }
        MaskPrompt::Polygon { points, .. } => {
            if points.is_empty() {
                return invalid("prompt polygon must contain at least one point");
            }
            for point in points {
                if !in_unit(point.x) || !in_unit(point.y) {
                    return invalid(
                        "prompt polygon points must have finite normalized coordinates within 0..=1",
                    );
                }
            }
        }
        MaskPrompt::Ellipse { center, radii, .. } => {
            if !in_unit(center.x)
                || !in_unit(center.y)
                || !in_unit(radii.x)
                || !in_unit(radii.y)
                || radii.x <= 0.0
                || radii.y <= 0.0
            {
                return invalid(
                    "prompt ellipse must have finite normalized coordinates within 0..=1 with positive radii",
                );
            }
        }
        MaskPrompt::Gradient {
            angle_deg,
            start,
            end,
            ..
        } => {
            if !angle_deg.is_finite() || !in_unit(*start) || !in_unit(*end) {
                return invalid(
                    "prompt gradient must have a finite angle and finite normalized start/end within 0..=1",
                );
            }
        }
        MaskPrompt::ColorRange {
            hue_center,
            hue_width,
            sat_min,
            sat_max,
            lum_min,
            lum_max,
            feather,
            ..
        } => {
            if !hue_center.is_finite() || !(0.0..=360.0).contains(hue_center) {
                return invalid("prompt color_range hue_center must be finite within 0..=360");
            }
            if !hue_width.is_finite() || !(0.0..=360.0).contains(hue_width) {
                return invalid("prompt color_range hue_width must be finite within 0..=360");
            }
            if !in_unit(*sat_min) || !in_unit(*sat_max) || sat_min > sat_max {
                return invalid(
                    "prompt color_range sat_min/sat_max must be within 0..=1 with sat_min <= sat_max",
                );
            }
            if !in_unit(*lum_min) || !in_unit(*lum_max) || lum_min > lum_max {
                return invalid(
                    "prompt color_range lum_min/lum_max must be within 0..=1 with lum_min <= lum_max",
                );
            }
            if !in_unit(*feather) {
                return invalid("prompt color_range feather must be finite within 0..=1");
            }
        }
        MaskPrompt::LuminanceRange {
            min, max, feather, ..
        } => {
            if !in_unit(*min) || !in_unit(*max) || min > max {
                return invalid(
                    "prompt luminance_range min/max must be within 0..=1 with min <= max",
                );
            }
            if !in_unit(*feather) {
                return invalid("prompt luminance_range feather must be finite within 0..=1");
            }
        }
    }
    Ok(())
}

/// Validates a declarative AI selection (G-03). `detail` accepts the
/// documented part names and any other trimmed, non-empty string without
/// control characters up to 64 chars — unknown parts stay readable and are
/// never silently remapped.
fn validate_ai_select(select: &Option<AiSelect>) -> Result<(), SidecarError> {
    let Some(select) = select else {
        return Ok(());
    };
    if let Some(detail) = &select.detail {
        if detail.len() > 64
            || detail.trim().is_empty()
            || detail != detail.trim()
            || detail.chars().any(|c| c.is_control())
        {
            return invalid(
                "ai_select detail must be trimmed, non-empty, free of control characters and at most 64 chars",
            );
        }
    }
    Ok(())
}

fn validate_source_action_ref(a: &SourceActionArtifactRef) -> Result<(), SidecarError> {
    validate_name("source_action id", &a.id)?;
    validate_relative_path("source_action relative_path", &a.relative_path)?;
    validate_name("source_action checksum", &a.checksum)
}

/// GEN-ZDATA-LINK-1: validates a generative bundle link (zdata `kind = 2/3`).
/// The id selects the record inside the bundle; the remaining fields are the
/// portable `ArtifactReference` payload. `format` must declare a zdata bundle
/// (so `artifact_status` deep-verifies instead of passing an opaque file),
/// `channels`/`data_version` pin the RGBA8 encoding (`rgba8`/`1`), and the
/// declared resolution must be non-zero. Anything else is rejected loudly.
fn validate_generative_ref(a: &GenerativeArtifactRef) -> Result<(), SidecarError> {
    validate_name("generative artifact id", &a.id)?;
    validate_relative_path("generative artifact relative_path", &a.relative_path)?;
    if !a.format.contains("zdata") {
        return invalid("generative artifact format must declare a zdata bundle");
    }
    validate_name("generative artifact checksum", &a.checksum)?;
    if a.width == 0 || a.height == 0 {
        return invalid("generative artifact width/height must be non-zero");
    }
    if a.channels != "rgba8" {
        return invalid("generative artifact channels must be `rgba8`");
    }
    if a.data_version != "1" {
        return invalid("generative artifact data_version must be `1`");
    }
    Ok(())
}

/// GEN-ZDATA-LINK-1: validates one spot removal, including the per-mode
/// exclusion rules — a heuristic spot has no bundle record and MUST NOT
/// carry an artifact link; a generative spot MAY carry the `kind = 3` link
/// (`None` = not generated yet, i.e. `missing` downstream).
fn validate_spot_removal(spot: &SpotRemoval) -> Result<(), SidecarError> {
    if spot.version != SPOT_REMOVAL_VERSION {
        return invalid("unsupported spot_removal version");
    }
    match spot.mode {
        SpotRemovalMode::Heuristic => {
            if spot.artifact.is_some() {
                return invalid("heuristic spot_removal must not carry an artifact");
            }
        }
        SpotRemovalMode::Generative => {
            if let Some(link) = &spot.artifact {
                validate_generative_ref(link)?;
            }
        }
    }
    Ok(())
}

/// SPOT-SCHEMA-GEOMETRY: validates the geometry-carrying
/// `extras["spot_removals"]` view of a recipe (heal parameters live here; the
/// typed `EditRecipe::spot_removals` holds only version/mode/artifact and is
/// checked by `validate_spot_removal`). Runs alongside the typed check from
/// `validate_adjustments`, so both views stay consistent.
///
/// Every rule fails loudly — never a silent fallback or silent reinterpretation:
/// - a present key must hold an array of objects (absent stays identity);
/// - `version` must be present and equal `SPOT_REMOVAL_VERSION`;
/// - `mode`, when absent, defaults to `heuristic` (legacy documents, mirroring
///   the tolerant core reader); any other value than
///   `heuristic`/`generative` is rejected;
/// - a `heuristic` entry MUST carry the full heal geometry (`id` non-empty;
///   `center_x`/`center_y` finite in `0..=1`; `radius` finite in `(0, 512]`;
///   `offset_dx`/`offset_dy` finite in `-1..=1`; `feather`/`opacity` are
///   optional with the core defaults `0.0`/`1.0` but must be finite in
///   `0..=1` when present) and MUST NOT carry `artifact` (no bundle record);
/// - a `generative` entry needs no geometry; an `artifact` link, when present
///   and non-null, must parse as `GenerativeArtifactRef` and pass
///   `validate_generative_ref` (absent/`None` = not generated yet, i.e.
///   `missing` downstream).
fn validate_spot_removal_extras(recipe: &EditRecipe) -> Result<(), SidecarError> {
    let Some(value) = recipe.extras.get("spot_removals") else {
        return Ok(());
    };
    let entries = value
        .as_array()
        .ok_or_else(|| SidecarError::Invalid("extras `spot_removals` must be an array".into()))?;
    for entry in entries {
        validate_spot_removal_extra_entry(entry)?;
    }
    Ok(())
}

fn extra_finite(object: &serde_json::Map<String, Value>, name: &str) -> Result<f64, SidecarError> {
    object
        .get(name)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .ok_or_else(|| {
            SidecarError::Invalid(format!(
                "heuristic spot_removal `{name}` must be present and finite"
            ))
        })
}

fn extra_optional_finite(
    object: &serde_json::Map<String, Value>,
    name: &str,
) -> Result<Option<f64>, SidecarError> {
    match object.get(name) {
        None => Ok(None),
        Some(value) => value
            .as_f64()
            .filter(|v| v.is_finite())
            .map(Some)
            .ok_or_else(|| {
                SidecarError::Invalid(format!("heuristic spot_removal `{name}` must be finite"))
            }),
    }
}

fn validate_spot_removal_extra_entry(entry: &Value) -> Result<(), SidecarError> {
    let object = entry
        .as_object()
        .ok_or_else(|| SidecarError::Invalid("spot_removal entry must be an object".into()))?;
    let version = object
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| SidecarError::Invalid("unsupported spot_removal version".into()))?;
    if version != u64::from(SPOT_REMOVAL_VERSION) {
        return invalid("unsupported spot_removal version");
    }
    let mode = object
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("heuristic");
    match mode {
        "heuristic" => {
            if object.contains_key("artifact") {
                return invalid("heuristic spot_removal must not carry an artifact");
            }
            let id = object.get("id").and_then(Value::as_str).unwrap_or_default();
            if id.trim().is_empty() {
                return invalid("heuristic spot_removal requires a non-empty `id`");
            }
            for name in ["center_x", "center_y"] {
                let v = extra_finite(object, name)?;
                if !(0.0..=1.0).contains(&v) {
                    return invalid(format!(
                        "heuristic spot_removal `{name}` must be within 0..=1"
                    ));
                }
            }
            let radius = extra_finite(object, "radius")?;
            if !(radius > 0.0 && radius <= 512.0) {
                return invalid("heuristic spot_removal `radius` must be within (0, 512]");
            }
            for name in ["offset_dx", "offset_dy"] {
                let v = extra_finite(object, name)?;
                if !(-1.0..=1.0).contains(&v) {
                    return invalid(format!(
                        "heuristic spot_removal `{name}` must be within -1..=1"
                    ));
                }
            }
            // `feather`/`opacity` fall back to the core defaults when absent.
            let feather = extra_optional_finite(object, "feather")?.unwrap_or(0.0);
            if !(0.0..=1.0).contains(&feather) {
                return invalid("heuristic spot_removal `feather` must be within 0..=1");
            }
            let opacity = extra_optional_finite(object, "opacity")?.unwrap_or(1.0);
            if !(0.0..=1.0).contains(&opacity) {
                return invalid("heuristic spot_removal `opacity` must be within 0..=1");
            }
        }
        "generative" => {
            if let Some(link_value) = object.get("artifact") {
                if !link_value.is_null() {
                    let link: GenerativeArtifactRef = serde_json::from_value(link_value.clone())
                        .map_err(|_| {
                            SidecarError::Invalid("invalid generative spot_removal artifact".into())
                        })?;
                    validate_generative_ref(&link)?;
                }
            }
            // LRPAR-G04-REMOVE: optional deterministic variant controls.
            // Present values are type-checked loudly; absent stays identity.
            if let Some(seed) = object.get("seed") {
                if seed.as_u64().is_none() {
                    return invalid("generative spot_removal `seed` must be a u64");
                }
            }
            if let Some(variant) = object.get("variant") {
                if variant.as_u64().is_none() {
                    return invalid("generative spot_removal `variant` must be a u64");
                }
            }
            if let Some(base_seed) = object.get("base_seed") {
                if base_seed.as_u64().is_none() {
                    return invalid("generative spot_removal `base_seed` must be a u64");
                }
            }
            if let Some(prompt) = object.get("prompt") {
                if prompt.as_str().is_none() {
                    return invalid("generative spot_removal `prompt` must be a string");
                }
            }
        }
        _ => return invalid(format!("unsupported spot_removal mode `{mode}`")),
    }
    Ok(())
}

fn validate_adjustments(a: &EditRecipe) -> Result<(), SidecarError> {
    // REVIEW-SIDECAR-N3: auto-features persist analysis results; a non-finite
    // target luminance would poison every downstream tone computation.
    if !a.auto_features.target_luminance.is_finite() {
        return invalid("invalid auto_features target_luminance");
    }
    // AUTO-TONE-2 Spiegelfelder: Domäne je `-1..=1` wie die gleichnamigen
    // Recipe-Adjustments (`whites`/`blacks`/`highlights`/`shadows`, s.
    // Core-`AutoToneResult`); `None` = kein Auto-Wert persistiert.
    for (name, value) in [
        ("auto_whites", a.auto_features.auto_whites),
        ("auto_blacks", a.auto_features.auto_blacks),
        ("auto_highlights", a.auto_features.auto_highlights),
        ("auto_shadows", a.auto_features.auto_shadows),
    ] {
        if let Some(v) = value {
            if !v.is_finite() || !(-1.0..=1.0).contains(&v) {
                return invalid(format!("invalid auto_features {name}"));
            }
        }
    }
    for (name, value) in &a.adjustments {
        let (lo, hi) = match name.as_str() {
            "exposure" => (-10.0, 10.0),
            "wb_temperature" => (1500.0, 12000.0),
            "contrast" | "highlights" | "shadows" | "whites" | "blacks" | "wb_tint"
            | "vibrance" | "saturation" => (-1.0, 1.0),
            // REVIEW-SIDECAR-N3: unknown adjustment keys stay accepted for
            // forward compatibility (they ride the recipe like extras), but a
            // NaN/∞ value is never a meaningful slider state and would
            // otherwise bypass validation entirely.
            _ => {
                if !value.is_finite() {
                    return invalid(format!("adjustment `{name}` must be finite"));
                }
                continue;
            }
        };
        if !value.is_finite() || !(*value >= lo && *value <= hi) {
            return invalid(format!("invalid adjustment `{name}`"));
        }
    }
    if let Some(c) = &a.curves {
        if c.version != 1 {
            return invalid("unsupported curves version");
        }
        validate_curve(&c.master)?;
        for curve in [&c.channels.red, &c.channels.green, &c.channels.blue]
            .into_iter()
            .flatten()
        {
            validate_curve(curve)?;
        }
    }
    if let Some(h) = &a.hsl {
        if h.version != 1 {
            return invalid("unsupported hsl version");
        }
        for (name, c) in [
            ("red", h.red),
            ("orange", h.orange),
            ("yellow", h.yellow),
            ("green", h.green),
            ("cyan", h.cyan),
            ("blue", h.blue),
            ("violet", h.violet),
            ("magenta", h.magenta),
        ] {
            let Some(c) = c else { continue };
            for (field, v) in [
                ("hue", c.hue),
                ("saturation", c.saturation),
                ("luminance", c.luminance),
            ] {
                if !v.is_finite() || !(-1.0..=1.0).contains(&v) {
                    return invalid(format!("invalid hsl {name}.{field}"));
                }
            }
        }
    }
    if let Some(c) = &a.color_grading {
        if c.version != 1 {
            return invalid("unsupported color_grading version");
        }
        if !c.balance.is_finite() || !(-1.0..=1.0).contains(&c.balance) {
            return invalid("invalid color_grading balance");
        }
        if !c.blending.is_finite() || !(0.0..=1.0).contains(&c.blending) {
            return invalid("invalid color_grading blending");
        }
        for (name, range) in [
            ("shadows", c.shadows),
            ("midtones", c.midtones),
            ("highlights", c.highlights),
        ] {
            if !range.hue_degrees.is_finite() || !(0.0..=360.0).contains(&range.hue_degrees) {
                return invalid(format!("invalid color_grading {name}.hue_degrees"));
            }
            if !range.saturation.is_finite() || !(0.0..=1.0).contains(&range.saturation) {
                return invalid(format!("invalid color_grading {name}.saturation"));
            }
            if !range.luminance.is_finite() || !(-1.0..=1.0).contains(&range.luminance) {
                return invalid(format!("invalid color_grading {name}.luminance"));
            }
        }
    }
    if let Some(p) = &a.point_color {
        if p.version != 1 {
            return invalid("unsupported point_color version");
        }
        if p.entries.len() > 8 {
            return invalid("too many point_color entries (max 8)");
        }
        let mut seen = std::collections::HashSet::new();
        for entry in &p.entries {
            if entry.id.is_empty() || !seen.insert(entry.id.clone()) {
                return invalid("invalid point_color entry id (empty or duplicate)");
            }
            if !entry.hue_center.is_finite() || !(0.0..=360.0).contains(&entry.hue_center) {
                return invalid(format!("invalid point_color {} hue_center", entry.id));
            }
            if !entry.hue_range.is_finite() || !(0.0..=180.0).contains(&entry.hue_range) {
                return invalid(format!("invalid point_color {} hue_range", entry.id));
            }
            for (field, v) in [
                ("hue_shift", entry.hue_shift),
                ("saturation_shift", entry.saturation_shift),
                ("luminance_shift", entry.luminance_shift),
            ] {
                if !v.is_finite() || !(-1.0..=1.0).contains(&v) {
                    return invalid(format!("invalid point_color {} {field}", entry.id));
                }
            }
        }
    }
    if let Some(p) = &a.presence {
        if p.version != 1 {
            return invalid("unsupported presence version");
        }
        for (name, v) in [
            ("texture", p.texture),
            ("clarity", p.clarity),
            ("dehaze", p.dehaze),
        ] {
            if !v.is_finite() || !(-1.0..=1.0).contains(&v) {
                return invalid(format!("invalid presence {name}"));
            }
        }
    }
    if let Some(n) = &a.noise_reduction {
        if n.version != 1 {
            return invalid("unsupported noise_reduction version");
        }
        for (name, v) in [("luminance", n.luminance), ("color", n.color)] {
            if !v.is_finite() || !(0.0..=1.0).contains(&v) {
                return invalid(format!("invalid noise_reduction {name}"));
            }
        }
    }
    // LRPAR-G14-DENOISE-20: the optional AI-denoise stage validates its own
    // contract (version, model identity, digest, strengths, artifact reference).
    if let Some(d) = &a.denoise_ai {
        validate_denoise_ai(d)?;
    }
    if let Some(s) = &a.sharpening {
        if s.version != 1 {
            return invalid("unsupported sharpening version");
        }
        for (name, v, lo, hi) in [
            ("amount", s.amount, 0.0, 3.0),
            ("radius", s.radius, 0.1, 10.0),
            ("detail", s.detail, 0.0, 1.0),
            ("masking", s.masking, 0.0, 1.0),
        ] {
            if !v.is_finite() || !(lo..=hi).contains(&v) {
                return invalid(format!("invalid sharpening {name}"));
            }
        }
    }
    // LRPAR-G14-REDEYE-15: out-of-range or non-finite values are rejected
    // loudly, never clipped; regions are identified by stable unique ids.
    if let Some(r) = &a.red_eye {
        if r.version != 1 {
            return invalid("unsupported red_eye version");
        }
        if r.regions.len() > RED_EYE_MAX_REGIONS {
            return invalid("too many red_eye regions (max 32)");
        }
        let mut seen = std::collections::HashSet::new();
        for region in &r.regions {
            if region.id.is_empty() || !seen.insert(region.id.clone()) {
                return invalid("invalid red_eye region id (empty or duplicate)");
            }
            for (field, v) in [("x", region.x), ("y", region.y)] {
                if !v.is_finite() || !(0.0..=1.0).contains(&v) {
                    return invalid(format!("invalid red_eye {} {field}", region.id));
                }
            }
            if !region.radius.is_finite() || region.radius <= 0.0 || region.radius > 1.0 {
                return invalid(format!("invalid red_eye {} radius", region.id));
            }
            for (field, v) in [("desaturate", region.desaturate), ("darken", region.darken)] {
                if !v.is_finite() || !(0.0..=1.0).contains(&v) {
                    return invalid(format!("invalid red_eye {} {field}", region.id));
                }
            }
        }
    }
    if let Some(g) = &a.geometry {
        if g.version != 1
            || !g.rotation_degrees.is_finite()
            || !(-180.0..=180.0).contains(&g.rotation_degrees)
        {
            return invalid("invalid geometry version or rotation");
        }
        if let Some(Crop::Free {
            x,
            y,
            width,
            height,
        }) = &g.crop
        {
            if ![x, y, width, height].iter().all(|v| v.is_finite())
                || *width <= 0.0
                || *height <= 0.0
                || *x < 0.0
                || *y < 0.0
                || *x + *width > 1.0
                || *y + *height > 1.0
            {
                return invalid("invalid geometry free crop");
            }
        }
    }
    if let Some(l) = &a.lens_correction {
        if l.version != 1 || l.profile.as_deref().is_some_and(|p| p.is_empty()) {
            return invalid("invalid lens_correction version or profile");
        }
        if let Some(profile) = l.profile.as_deref() {
            if !matches!(profile, "wide-light" | "tele-light" | "standard-neutral") {
                return invalid("unknown lens correction profile");
            }
        }
        for v in [
            l.distortion_k1,
            l.distortion_k2,
            l.distortion_k3,
            l.vignette_c0,
            l.vignette_c1,
            l.vignette_c2,
        ]
        .into_iter()
        .flatten()
        {
            if !v.is_finite() || !(-1.0..=1.0).contains(&v) {
                return invalid("invalid lens correction coefficient");
            }
        }
        for v in [l.ca_red, l.ca_blue].into_iter().flatten() {
            if !v.is_finite() || !(-0.05..=0.05).contains(&v) {
                return invalid("invalid chromatic aberration coefficient");
            }
        }
    }
    if let Some(p) = &a.perspective {
        if p.version != 1 {
            return invalid("unsupported perspective version");
        }
        for v in [p.vertical, p.horizontal, p.rotation, p.shift_x, p.shift_y] {
            if !v.is_finite() || !(-1.0..=1.0).contains(&v) {
                return invalid("invalid perspective coefficient");
            }
        }
        for (v, lo, hi) in [(p.scale, 0.1, 10.0), (p.aspect_ratio, 0.1, 10.0)] {
            if !v.is_finite() || !(lo..=hi).contains(&v) {
                return invalid("invalid perspective scale");
            }
        }
    }
    // LRPAR-G06-UPRIGHT-15: additive upright stage. `enabled` requires a
    // persisted analysis (loud, never a silent identity render); the
    // suggestion stays in the F-099 domain.
    if let Some(u) = &a.upright {
        if u.version != 1 {
            return invalid("unsupported upright version");
        }
        if u.enabled && u.analysis.is_none() {
            return invalid("upright enabled requires a persisted analysis");
        }
        if let Some(analysis) = &u.analysis {
            validate_name("upright algorithm", &analysis.fingerprint.algorithm)?;
            validate_name("upright algorithm version", &analysis.fingerprint.version)?;
            validate_name(
                "upright input fingerprint",
                &analysis.fingerprint.input_fingerprint,
            )?;
            for v in [analysis.vertical, analysis.horizontal, analysis.rotation] {
                if !v.is_finite() || !(-1.0..=1.0).contains(&v) {
                    return invalid("invalid upright suggestion coefficient");
                }
            }
            if !analysis.confidence.is_finite() || !(0.0..=1.0).contains(&analysis.confidence) {
                return invalid("invalid upright confidence");
            }
            if analysis.line_count > UPRIGHT_MAX_LINE_COUNT {
                return invalid("invalid upright line_count");
            }
        }
    }
    for action in &a.source_actions {
        if action.version != SOURCE_ACTION_VERSION {
            return invalid("unsupported source_action version");
        }
        validate_source_action_ref(&action.artifact)?;
    }
    for spot in &a.spot_removals {
        validate_spot_removal(spot)?;
    }
    validate_spot_removal_extras(a)?;
    validate_spot_g04_extras(a)?;
    validate_treatment_profile(a)?;
    if let Some(g) = &a.generative_edit {
        if g.version != 1 {
            return invalid("unsupported generative_edit version");
        }
        if let Some(canvas) = &g.canvas {
            canvas.validate()?;
        }
        if let Some(link) = &g.artifact {
            validate_generative_ref(link)?;
        }
        let expand = g.expand_beyond_image.unwrap_or(false);
        if expand {
            if g.canvas.is_none() {
                return invalid("expand_beyond_image true requires a canvas");
            }
        } else if g.canvas.is_some() {
            return invalid("canvas present without expand_beyond_image true");
        }
    }
    if let Some(e) = &a.effects {
        if let Some(v) = &e.vignette {
            if v.version != 1 {
                return invalid("unsupported vignette version");
            }
            for (name, val, lo, hi) in [
                ("amount", v.amount, -1.0, 1.0),
                ("midpoint", v.midpoint, 0.0, 1.0),
                ("roundness", v.roundness, -1.0, 1.0),
                ("feather", v.feather, 0.0, 1.0),
            ] {
                if !val.is_finite() || !(lo..=hi).contains(&val) {
                    return invalid(format!("invalid vignette {name}"));
                }
            }
        }
        if let Some(g) = &e.grain {
            if g.version != 1 {
                return invalid("unsupported grain version");
            }
            for (name, val, lo, hi) in [
                ("amount", g.amount, 0.0, 1.0),
                ("size", g.size, 0.0, 1.0),
                ("roughness", g.roughness, 0.0, 1.0),
            ] {
                if !val.is_finite() || !(lo..=hi).contains(&val) {
                    return invalid(format!("invalid grain {name}"));
                }
            }
        }
    }
    if let Some(b) = &a.lens_blur {
        if b.version != 1 {
            return invalid("unsupported lens_blur version");
        }
        let r = &b.focus_rect;
        for (name, v) in [
            ("x", r.x),
            ("y", r.y),
            ("width", r.width),
            ("height", r.height),
        ] {
            if !v.is_finite() {
                return invalid(format!("invalid lens_blur focus_rect.{name}"));
            }
        }
        if !(0.0..=1.0).contains(&r.x)
            || !(0.0..=1.0).contains(&r.y)
            || r.width <= 0.0
            || r.height <= 0.0
            || r.x + r.width > 1.0
            || r.y + r.height > 1.0
        {
            return invalid("invalid lens_blur focus_rect (must be a positive area inside 0..=1)");
        }
        for (name, v, lo, hi) in [
            ("focal_near", b.focal_near, 0.0, 1.0),
            ("focal_far", b.focal_far, 0.0, 1.0),
            ("blur_amount", b.blur_amount, 0.0, 1.0),
        ] {
            if !v.is_finite() || !(lo..=hi).contains(&v) {
                return invalid(format!("invalid lens_blur {name}"));
            }
        }
        if b.focal_near > b.focal_far {
            return invalid("invalid lens_blur focal range (focal_near must be <= focal_far)");
        }
        if let Some(d) = &b.depth_artifact {
            validate_relative_path("lens_blur depth_artifact relative_path", &d.relative_path)?;
            validate_name("lens_blur depth_artifact sha256", &d.sha256)?;
        }
    }
    Ok(())
}
/// LRPAR-G01-BASIC: validates the additive G-01 recipe fields —
/// `extras["treatment"]` (`"color"`|`"bw"` when present), `extras["bw_stash"]`
/// (object with optional finite `-1..=1` `saturation`/`vibrance` when
/// present) and `options["profile"]` (whitelisted name when present).
/// Every deviation fails loudly, never a silent reinterpretation.
fn validate_treatment_profile(recipe: &EditRecipe) -> Result<(), SidecarError> {
    if let Some(value) = recipe.extras.get(TREATMENT_KEY) {
        let ok = value
            .as_str()
            .is_some_and(|t| t == TREATMENT_COLOR || t == TREATMENT_BW);
        if !ok {
            return invalid(format!("extras `{TREATMENT_KEY}` must be `color` or `bw`"));
        }
    }
    if let Some(value) = recipe.extras.get(BW_STASH_KEY) {
        let map = serde_json::from_value::<BTreeMap<String, Option<f64>>>(value.clone()).map_err(
            |_| SidecarError::Invalid(format!("extras `{BW_STASH_KEY}` must be an object")),
        )?;
        for key in ["saturation", "vibrance"] {
            if let Some(Some(v)) = map.get(key) {
                if !v.is_finite() || !(-1.0..=1.0).contains(v) {
                    return invalid(format!(
                        "extras `{BW_STASH_KEY}.{key}` must be finite within -1..=1"
                    ));
                }
            }
        }
    }
    if let Some(profile) = recipe.options.get(DEVELOP_PROFILE_KEY) {
        if !DEVELOP_PROFILES.contains(&profile.as_str()) {
            return invalid(format!(
                "options `{DEVELOP_PROFILE_KEY}` must be one of {}",
                DEVELOP_PROFILES.join("|")
            ));
        }
    }
    Ok(())
}

/// LRPAR-G04-REMOVE: validates the additive G-04 recipe extras —
/// `spot_visualize_threshold` (finite `0..=1` when present, absent = off) and
/// `spot_distraction` (an object of bools when present, absent = all off).
/// Every deviation fails loudly, never a silent reinterpretation.
fn validate_spot_g04_extras(recipe: &EditRecipe) -> Result<(), SidecarError> {
    if let Some(value) = recipe.extras.get(SPOT_VISUALIZE_KEY) {
        let v = value.as_f64().filter(|v| v.is_finite()).ok_or_else(|| {
            SidecarError::Invalid(format!("extras `{SPOT_VISUALIZE_KEY}` must be finite"))
        })?;
        if !(0.0..=1.0).contains(&v) {
            return invalid(format!(
                "extras `{SPOT_VISUALIZE_KEY}` must be within 0..=1"
            ));
        }
    }
    if let Some(value) = recipe.extras.get(SPOT_DISTRACTION_KEY) {
        serde_json::from_value::<SpotDistraction>(value.clone()).map_err(|_| {
            SidecarError::Invalid(format!(
                "extras `{SPOT_DISTRACTION_KEY}` must be an object of bools"
            ))
        })?;
    }
    Ok(())
}

fn validate_curve(c: &[CurvePoint]) -> Result<(), SidecarError> {
    if !(2..=32).contains(&c.len()) {
        return invalid("curve must contain 2..=32 points");
    }
    let mut previous = -1.0;
    for p in c {
        if !p.input.is_finite()
            || !p.output.is_finite()
            || !(0.0..=1.0).contains(&p.input)
            || !(0.0..=1.0).contains(&p.output)
            || p.input <= previous
        {
            return invalid("curve points must be finite, bounded and strictly increasing");
        }
        previous = p.input;
    }
    let first = c.first().unwrap();
    let last = c.last().unwrap();
    if first.input != 0.0 || first.output != 0.0 || last.input != 1.0 || last.output != 1.0 {
        return invalid("curve must have (0,0) and (1,1) endpoints");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REVIEW-SIDECAR-TMP-1 helper: backdates a file's modification time so a
    /// recovery sweep treats it as an orphaned crash leftover.
    fn backdate(path: &Path, age: Duration) {
        let file = fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("backdate target must exist");
        file.set_modified(SystemTime::now() - age)
            .expect("set_modified must succeed on the host filesystem");
    }

    fn source() -> SourceIdentity {
        SourceIdentity {
            relative_name: "IMG_0001.ARW".into(),
            content_hash: "sha256:x".into(),
            byte_length: 42,
            modified_at: None,
            raw_format: "ARW".into(),
            orientation: 1,
            decode_fingerprint: DecodeFingerprint {
                decoder: "test".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            geometry_fingerprint: GeometryFingerprint {
                width: 10,
                height: 20,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: Extras::new(),
            },
            extras: Extras::new(),
        }
    }
    fn mask(id: &str) -> MaskDefinition {
        MaskDefinition {
            id: id.into(),
            name: id.into(),
            source_fingerprint: SourceFingerprint {
                content_hash: "sha256:x".into(),
                byte_length: 42,
                extras: Extras::new(),
            },
            decode_context: source().decode_fingerprint.clone(),
            geometry_context: source().geometry_fingerprint.clone(),
            model: ModelIdentity {
                name: "model".into(),
                version: "1".into(),
                hash: "sha256:model".into(),
                extras: Extras::new(),
            },
            inference_resolution: Resolution {
                width: 10,
                height: 20,
                extras: Extras::new(),
            },
            preprocessing: Preprocessing {
                name: "standard".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            rescaling_method: "bilinear".into(),
            rescaling_parameters: BTreeMap::new(),
            coordinate_system: CoordinateSystem::SourceOriented,
            status: MaskStatus::Valid,
            created_at: "2026-01-01T00:00:00Z".into(),
            generator_version: "generator-1".into(),
            error_text: None,
            artifact: None,
            operation: MaskOperation::Source,
            references: vec![],
            prompt: None,
            extras: Extras::new(),
            ai_select: None,
        }
    }
    #[test]
    fn complete_roundtrip() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.analysis_fingerprint = Some(AnalysisFingerprint {
            algorithm: "scene-analysis".into(),
            version: "2.1".into(),
            input_fingerprint: "sha256:analysis-input".into(),
            extras: Extras::from([("analysis_extra".into(), Value::from(true))]),
        });
        d.extras
            .insert("future_root".into(), Value::from("preserved"));

        let mut source_mask = mask("a");
        source_mask
            .source_fingerprint
            .extras
            .insert("future_source_fingerprint".into(), Value::from(7));
        source_mask
            .decode_context
            .parameters
            .insert("quality".into(), "high".into());
        source_mask.geometry_context.pixel_aspect_ratio = 1.25;
        source_mask
            .model
            .extras
            .insert("future_model".into(), Value::from("kept"));
        source_mask.inference_resolution.width = 512;
        source_mask
            .preprocessing
            .parameters
            .insert("mean".into(), "0.5".into());
        source_mask.rescaling_method = "lanczos".into();
        source_mask
            .rescaling_parameters
            .insert("radius".into(), "3".into());
        source_mask.coordinate_system = CoordinateSystem::Normalized;
        source_mask.status = MaskStatus::Corrupt;
        source_mask.created_at = "2026-02-03T04:05:06Z".into();
        source_mask.generator_version = "segmenter-2.4".into();
        source_mask.error_text = Some("model output checksum mismatch".into());
        source_mask.artifact = Some(ArtifactReference {
            relative_path: "masks/a.zdata".into(),
            format: "zdata-mask".into(),
            checksum: "sha256:artifact".into(),
            width: 512,
            height: 256,
            channels: "f32".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        });
        source_mask
            .extras
            .insert("future_mask".into(), Value::from("kept"));
        d.virtual_copies[0].mask_library.push(source_mask);
        d.virtual_copies.push(VirtualCopy {
            id: "vc-bw".into(),
            name: "B&W".into(),
            is_default: false,
            rating: 0,
            flag: Flag::Unflagged,
            recipe: EditRecipe {
                recipe_version: "1".into(),
                adjustments: BTreeMap::from([("exposure".into(), 1.25)]),
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
                generative_edit: None,
                source_actions: Vec::new(),
                spot_removals: Vec::new(),
                options: BTreeMap::from([("profile".into(), "neutral".into())]),
                auto_features: AutoFeatures::default(),
                extras: Extras::from([("future_recipe".into(), Value::from(42))]),
            },
            mask_library: vec![],
            mask_layers: vec![MaskLayer {
                id: "layer".into(),
                mask: MaskReference {
                    copy_id: "vc-original".into(),
                    mask_id: "a".into(),
                    extras: Extras::new(),
                },
                inverted: false,
                feather: 0.0,
                blur: 0.0,
                density: 1.0,
                extras: Extras::new(),
                visible: true,
            }],
            history: vec![HistoryEntry {
                id: "h".into(),
                recipe: EditRecipe {
                    recipe_version: "1".into(),
                    adjustments: BTreeMap::from([("contrast".into(), -0.4)]),
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
                    generative_edit: None,
                    source_actions: Vec::new(),
                    spot_removals: Vec::new(),
                    options: BTreeMap::from([("source".into(), "preset".into())]),
                    auto_features: AutoFeatures::default(),
                    extras: Extras::new(),
                },
                recorded_at: Some("2026-02-03T04:05:06Z".into()),
                extras: Extras::from([("future_history".into(), Value::from(true))]),
            }],
            export_records: vec![ExportRecord {
                id: "e".into(),
                relative_path: "exports/out.jpg".into(),
                format: "jpeg".into(),
                exported_at: Some("2026-02-03T04:06:06Z".into()),
                extras: Extras::from([("future_export".into(), Value::from("kept"))]),
            }],
            extras: Extras::from([("future_copy".into(), Value::from(true))]),
        });
        d.presets.push(Preset {
            id: "preset-1".into(),
            name: "Monochrome Contrast".into(),
            recipe: EditRecipe {
                recipe_version: "1".into(),
                adjustments: BTreeMap::from([("highlights".into(), -0.75)]),
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
                generative_edit: None,
                source_actions: Vec::new(),
                spot_removals: Vec::new(),
                options: BTreeMap::from([("curve".into(), "film".into())]),
                auto_features: AutoFeatures::default(),
                extras: Extras::new(),
            },
            extras: Extras::new(),
        });
        let json = d.to_json().unwrap();
        assert_eq!(d, SidecarDocument::from_json(&json).unwrap());
    }

    #[test]
    fn empty_sidecar_roundtrip() {
        let d = SidecarDocument::new(source(), "pipeline-1");
        let json = d.to_json().unwrap();
        assert_eq!(d, SidecarDocument::from_json(&json).unwrap());
    }

    #[test]
    fn auto_features_roundtrip_with_result_and_fingerprint() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        let features = &mut d.virtual_copies[0].recipe.auto_features;
        features.enable_auto_tone = true;
        features.match_total_exposure = true;
        features.target_luminance = 0.42;
        features.auto_exposure = Some(1.25);
        features.auto_contrast = Some(-0.2);
        features.auto_whites = Some(0.35);
        features.auto_blacks = Some(-0.45);
        features.auto_highlights = Some(0.15);
        features.auto_shadows = Some(-0.25);
        features.matched_exposure = Some(0.5);
        features.analysis_fingerprint = Some(AnalysisFingerprint {
            algorithm: "tone-rgba8-rec709".into(),
            version: "1".into(),
            input_fingerprint: "tone-rgba8-rec709:abc".into(),
            extras: Extras::new(),
        });
        let json = d.to_json().unwrap();
        assert!(json.contains("auto_exposure"));
        assert!(json.contains("auto_whites"));
        assert!(json.contains("auto_blacks"));
        assert!(json.contains("auto_highlights"));
        assert!(json.contains("auto_shadows"));
        assert!(json.contains("tone-rgba8-rec709:abc"));
        assert_eq!(d, SidecarDocument::from_json(&json).unwrap());
        assert!(d.validate().is_ok());
    }

    #[test]
    fn auto_tone_mirror_fields_validate_range_and_finiteness() {
        // Jede der vier AUTO-TONE-2-Spiegelfelder einzeln: gültige
        // Randwerte ±1.0 passieren, NaN/∞/Out-of-range wird laut abgelehnt.
        for (index, valid) in [0.8, -0.8, 1.0, -1.0, 0.0].iter().enumerate() {
            let mut d = SidecarDocument::new(source(), "pipeline-1");
            let features = &mut d.virtual_copies[0].recipe.auto_features;
            match index % 4 {
                0 => features.auto_whites = Some(*valid),
                1 => features.auto_blacks = Some(*valid),
                2 => features.auto_highlights = Some(*valid),
                _ => features.auto_shadows = Some(*valid),
            }
            assert!(d.validate().is_ok(), "valid value {valid} rejected");
        }
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1.5, -1.5] {
            for field in [
                "auto_whites",
                "auto_blacks",
                "auto_highlights",
                "auto_shadows",
            ] {
                let mut d = SidecarDocument::new(source(), "pipeline-1");
                let features = &mut d.virtual_copies[0].recipe.auto_features;
                match field {
                    "auto_whites" => features.auto_whites = Some(bad),
                    "auto_blacks" => features.auto_blacks = Some(bad),
                    "auto_highlights" => features.auto_highlights = Some(bad),
                    _ => features.auto_shadows = Some(bad),
                }
                assert!(
                    d.validate().is_err(),
                    "field {field} accepted invalid value {bad}"
                );
            }
        }
        // `None` (kein Auto-Wert) bleibt gültig.
        assert!(SidecarDocument::new(source(), "pipeline-1")
            .validate()
            .is_ok());
    }

    #[test]
    fn auto_tone_mirror_fields_missing_in_legacy_json_default_to_none() {
        // Additiv, keine Migration nötig: Alt-JSON ohne die vier Felder
        // parst dank `#[serde(default)]` und validiert; `schema_version`
        // bleibt unverändert.
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.auto_features.auto_exposure = Some(1.25);
        d.virtual_copies[0].recipe.auto_features.auto_contrast = Some(-0.2);
        let mut json_value = serde_json::to_value(&d).expect("sidecar serializes");
        for field in [
            "auto_whites",
            "auto_blacks",
            "auto_highlights",
            "auto_shadows",
        ] {
            json_value["virtual_copies"][0]["recipe"]["auto_features"]
                .as_object_mut()
                .expect("auto_features is an object")
                .remove(field);
        }
        let json = serde_json::to_string(&json_value).expect("json serializes");
        assert!(!json.contains("auto_whites"));
        let decoded = SidecarDocument::from_json(&json).expect("legacy json parses");
        let features = &decoded.virtual_copies[0].recipe.auto_features;
        assert_eq!(features.auto_whites, None);
        assert_eq!(features.auto_blacks, None);
        assert_eq!(features.auto_highlights, None);
        assert_eq!(features.auto_shadows, None);
        assert!(decoded.validate().is_ok());
    }

    #[test]
    fn presence_and_geometry_roundtrip_in_recipe() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.presence = Some(Presence {
            version: 1,
            texture: 0.25,
            clarity: -0.5,
            dehaze: 1.0,
        });
        d.virtual_copies[0].recipe.geometry = Some(Geometry {
            version: 1,
            crop: Some(Crop::Aspect {
                preset: AspectPreset::FourToFive,
            }),
            rotation_degrees: -12.5,
            mirror_horizontal: true,
            mirror_vertical: false,
        });
        let json = d.to_json().unwrap();
        assert!(json.contains("\"presence\""));
        assert!(json.contains("\"geometry\""));
        assert_eq!(d, SidecarDocument::from_json(&json).unwrap());
    }

    #[test]
    fn geometry_free_crop_rotation_and_both_mirrors_roundtrip() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.geometry = Some(Geometry {
            version: 1,
            crop: Some(Crop::Free {
                x: 0.125,
                y: 0.25,
                width: 0.5,
                height: 0.375,
            }),
            rotation_degrees: 90.0,
            mirror_horizontal: true,
            mirror_vertical: true,
        });
        let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
        assert_eq!(
            decoded.virtual_copies[0].recipe.geometry,
            d.virtual_copies[0].recipe.geometry
        );
    }

    #[test]
    fn presence_values_roundtrip_without_loss() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.presence = Some(Presence {
            version: 1,
            texture: -0.75,
            clarity: 0.375,
            dehaze: -1.0,
        });
        let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
        assert_eq!(
            decoded.virtual_copies[0].recipe.presence,
            d.virtual_copies[0].recipe.presence
        );
    }

    #[test]
    fn presence_and_geometry_validation_rejects_invalid_values() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.presence = Some(Presence {
            version: 2,
            texture: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
        });
        assert!(d.validate().is_err());

        d.virtual_copies[0].recipe.presence = Some(Presence {
            version: 1,
            texture: f32::NAN,
            clarity: 0.0,
            dehaze: 0.0,
        });
        assert!(d.validate().is_err());

        d.virtual_copies[0].recipe.presence = None;
        d.virtual_copies[0].recipe.geometry = Some(Geometry {
            version: 1,
            crop: Some(Crop::Free {
                x: 0.8,
                y: 0.0,
                width: 0.3,
                height: 0.5,
            }),
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        });
        assert!(d.validate().is_err());
    }

    #[test]
    fn sidecar_path_keeps_full_source_name() {
        assert_eq!(
            sidecar_path_for(Path::new("/photos/IMG_0001.ARW")),
            PathBuf::from("/photos/IMG_0001.ARW.lumina.json")
        );
    }

    #[test]
    fn file_roundtrip_and_missing_case() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("photo.png");
        let path = sidecar_path_for(&source_path);
        let document = SidecarDocument::new(source(), "pipeline-1");
        save_sidecar(&path, &document).unwrap();
        assert_eq!(load_sidecar(&path).unwrap(), document);
        assert!(matches!(
            load_sidecar(&directory.path().join("missing.json")),
            Err(SidecarError::Missing(_))
        ));
    }

    #[test]
    fn corrupt_json_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("photo.png.lumina.json");
        std::fs::write(&path, b"{not-json").unwrap();
        assert!(matches!(load_sidecar(&path), Err(SidecarError::Json(_))));
    }

    #[test]
    fn unknown_fields_roundtrip() {
        let json = r#"{"format":"lumina-sidecar","schema_version":1,"pipeline_version":"p","source":{"relative_name":"x.raw","content_hash":"h","byte_length":1,"modified_at":null,"raw_format":"RAW","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0},"future":42},"virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{"adjustments":{},"options":{}},"mask_library":[],"mask_layers":[],"history":[],"export_records":[],"future_copy":true}],"presets":[],"future_root":"kept"}"#;
        let d = SidecarDocument::from_json(json).unwrap();
        let out = d.to_json().unwrap();
        assert!(out.contains("future_root"));
        assert!(out.contains("future_copy"));
        assert!(out.contains("\"future\": 42"));
    }

    #[test]
    fn schema_version_one_missing_operation_defaults_to_source() {
        let mut document = SidecarDocument::new(source(), "pipeline-1");
        document.virtual_copies[0].mask_library.push(mask("legacy"));
        let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
        value["virtual_copies"][0]["mask_library"][0]
            .as_object_mut()
            .unwrap()
            .remove("operation");

        let legacy_json = serde_json::to_string(&value).unwrap();
        let decoded = SidecarDocument::from_json(&legacy_json).unwrap();
        assert_eq!(
            decoded.virtual_copies[0].mask_library[0].operation,
            MaskOperation::Source
        );
        let roundtripped: Value = serde_json::from_str(&decoded.to_json().unwrap()).unwrap();
        assert_eq!(
            roundtripped["virtual_copies"][0]["mask_library"][0]["operation"],
            "source"
        );
    }
    #[test]
    fn unsafe_paths_are_rejected() {
        for path in [
            "../outside",
            "a/../../x",
            "/tmp/x",
            "C:\\x",
            "C:/x",
            "\\\\server\\share\\x",
            "a\\b",
            "a/./b",
        ] {
            let mut d = SidecarDocument::new(source(), "p");
            d.source.relative_name = path.into();
            assert!(d.validate().is_err(), "{path}");
        }
    }
    #[test]
    fn default_and_original_are_exact() {
        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].id = "other".into();
        assert!(d.validate().is_err());
        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies.push(d.virtual_copies[0].clone());
        assert!(d.validate().is_err());
    }
    #[test]
    fn mask_cycles_and_invalid_targets_are_rejected() {
        let mut d = SidecarDocument::new(source(), "p");
        let mut a = mask("a");
        let mut b = mask("b");
        a.operation = MaskOperation::Invert;
        b.operation = MaskOperation::Invert;
        a.references.push(MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "b".into(),
            extras: Extras::new(),
        });
        b.references.push(MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "a".into(),
            extras: Extras::new(),
        });
        d.virtual_copies[0].mask_library = vec![a, b];
        let error = d.validate().unwrap_err().to_string();
        assert!(error.contains("cycle"));
        d.virtual_copies[0].mask_library[0].references[0].mask_id = "missing".into();
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("unknown mask"));
        d.virtual_copies[0].mask_library[0].references.clear();
        d.virtual_copies[0].mask_library[0].operation = MaskOperation::Source;
        d.virtual_copies[0].mask_layers.push(MaskLayer {
            id: "layer".into(),
            mask: MaskReference {
                copy_id: "vc-original".into(),
                mask_id: "missing".into(),
                extras: Extras::new(),
            },
            inverted: false,
            feather: 0.0,
            blur: 0.0,
            density: 1.0,
            extras: Extras::new(),
            visible: true,
        });
        assert!(d.validate().unwrap_err().to_string().contains("mask layer"));
    }

    #[test]
    fn valid_cross_copy_mask_reference_is_accepted() {
        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].mask_library.push(mask("source-mask"));
        d.virtual_copies.push(VirtualCopy {
            id: "vc-target".into(),
            name: "Target".into(),
            is_default: false,
            rating: 0,
            flag: Flag::Unflagged,
            recipe: EditRecipe::default(),
            mask_library: vec![MaskDefinition {
                operation: MaskOperation::Invert,
                references: vec![MaskReference {
                    copy_id: "vc-original".into(),
                    mask_id: "source-mask".into(),
                    extras: Extras::new(),
                }],
                ..mask("derived")
            }],
            mask_layers: vec![],
            history: vec![],
            export_records: vec![],
            extras: Extras::new(),
        });
        assert!(d.validate().is_ok());
    }

    #[test]
    fn cross_copy_mask_cycle_is_rejected() {
        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].mask_library.push(MaskDefinition {
            operation: MaskOperation::Invert,
            references: vec![MaskReference {
                copy_id: "vc-target".into(),
                mask_id: "target-mask".into(),
                extras: Extras::new(),
            }],
            ..mask("source-mask")
        });
        d.virtual_copies.push(VirtualCopy {
            id: "vc-target".into(),
            name: "Target".into(),
            is_default: false,
            rating: 0,
            flag: Flag::Unflagged,
            recipe: EditRecipe::default(),
            mask_library: vec![MaskDefinition {
                operation: MaskOperation::Invert,
                references: vec![MaskReference {
                    copy_id: "vc-original".into(),
                    mask_id: "source-mask".into(),
                    extras: Extras::new(),
                }],
                ..mask("target-mask")
            }],
            mask_layers: vec![],
            history: vec![],
            export_records: vec![],
            extras: Extras::new(),
        });
        assert!(d.validate().unwrap_err().to_string().contains("cycle"));
    }

    #[test]
    fn direct_mask_self_reference_is_rejected() {
        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].mask_library.push(MaskDefinition {
            operation: MaskOperation::Invert,
            references: vec![MaskReference {
                copy_id: "vc-original".into(),
                mask_id: "self".into(),
                extras: Extras::new(),
            }],
            ..mask("self")
        });
        let error = d.validate().unwrap_err().to_string();
        assert!(error.contains("references itself"));
    }

    #[test]
    fn mask_identity_fields_roundtrip() {
        let mut d = SidecarDocument::new(source(), "p");
        let mut definition = mask("identity");
        definition.rescaling_method = "lanczos".into();
        definition
            .rescaling_parameters
            .insert("filter_radius".into(), "3".into());
        definition.generator_version = "segmenter-2.4".into();
        d.virtual_copies[0].mask_library.push(definition);
        let json = d.to_json().unwrap();
        assert!(json.contains("rescaling_method"));
        assert!(json.contains("generator_version"));
        assert_eq!(d, SidecarDocument::from_json(&json).unwrap());
    }

    #[test]
    fn collection_ids_must_be_nonempty_and_unique() {
        let mut d = SidecarDocument::new(source(), "p");
        d.presets = vec![
            Preset {
                id: "preset".into(),
                name: "One".into(),
                recipe: EditRecipe::default(),
                extras: Extras::new(),
            },
            Preset {
                id: "preset".into(),
                name: "Two".into(),
                recipe: EditRecipe::default(),
                extras: Extras::new(),
            },
        ];
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("duplicate preset id"));

        d.presets.clear();
        d.virtual_copies[0].mask_library.push(mask("layer-mask"));
        let layer = MaskLayer {
            id: "layer".into(),
            mask: MaskReference {
                copy_id: "vc-original".into(),
                mask_id: "layer-mask".into(),
                extras: Extras::new(),
            },
            inverted: false,
            feather: 0.0,
            blur: 0.0,
            density: 1.0,
            extras: Extras::new(),
            visible: true,
        };
        d.virtual_copies[0].mask_layers = vec![layer.clone(), layer];
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("duplicate mask layer id"));

        d.virtual_copies[0].mask_layers.clear();
        let history = HistoryEntry {
            id: "history".into(),
            recipe: EditRecipe::default(),
            recorded_at: None,
            extras: Extras::new(),
        };
        d.virtual_copies[0].history = vec![history.clone(), history];
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("duplicate history entry id"));

        d.virtual_copies[0].history.clear();
        let export = ExportRecord {
            id: "export".into(),
            relative_path: "exports/out.jpg".into(),
            format: "jpeg".into(),
            exported_at: None,
            extras: Extras::new(),
        };
        d.virtual_copies[0].export_records = vec![export.clone(), export];
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("duplicate export record id"));
    }

    #[test]
    fn ids_must_be_nonempty() {
        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].id.clear();
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("virtual copy id"));

        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].mask_library.push(mask("mask"));
        d.virtual_copies[0].mask_library[0].id.clear();
        assert!(d.validate().unwrap_err().to_string().contains("mask id"));

        let mut d = SidecarDocument::new(source(), "p");
        let mut referenced_mask = mask("referenced");
        referenced_mask.operation = MaskOperation::Invert;
        referenced_mask.references.push(MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "mask".into(),
            extras: Extras::new(),
        });
        referenced_mask.references[0].copy_id.clear();
        d.virtual_copies[0].mask_library.push(referenced_mask);
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("mask reference copy_id"));

        let mut d = SidecarDocument::new(source(), "p");
        let mut referenced_mask = mask("referenced");
        referenced_mask.operation = MaskOperation::Invert;
        referenced_mask.references.push(MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "mask".into(),
            extras: Extras::new(),
        });
        referenced_mask.references[0].mask_id.clear();
        d.virtual_copies[0].mask_library.push(referenced_mask);
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("mask reference mask_id"));

        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].mask_layers.push(MaskLayer {
            id: String::new(),
            mask: MaskReference {
                copy_id: "vc-original".into(),
                mask_id: "mask".into(),
                extras: Extras::new(),
            },
            inverted: false,
            feather: 0.0,
            blur: 0.0,
            density: 1.0,
            extras: Extras::new(),
            visible: true,
        });
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("mask layer id"));

        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].history.push(HistoryEntry {
            id: String::new(),
            recipe: EditRecipe::default(),
            recorded_at: None,
            extras: Extras::new(),
        });
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("history entry id"));

        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].export_records.push(ExportRecord {
            id: String::new(),
            relative_path: "exports/out.jpg".into(),
            format: "jpeg".into(),
            exported_at: None,
            extras: Extras::new(),
        });
        assert!(d.validate().unwrap_err().to_string().contains("export id"));

        let mut d = SidecarDocument::new(source(), "p");
        d.presets.push(Preset {
            id: String::new(),
            name: "Preset".into(),
            recipe: EditRecipe::default(),
            extras: Extras::new(),
        });
        assert!(d.validate().unwrap_err().to_string().contains("preset id"));
    }

    #[test]
    fn virtual_copy_lifecycle_preserves_independent_recipe() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0]
            .recipe
            .adjustments
            .insert("exposure".into(), 1.0);
        d.duplicate_virtual_copy("vc-original", "vc-copy", "Copy")
            .unwrap();
        d.rename_virtual_copy("vc-copy", "Renamed").unwrap();
        d.virtual_copies.swap(0, 1);
        d.delete_virtual_copy("vc-copy").unwrap();
        assert_eq!(d.virtual_copies.len(), 1);
        d.restore_virtual_copy("vc-copy").unwrap();
        assert_eq!(d.virtual_copies[1].name, "Renamed");
        d.virtual_copies[1]
            .recipe
            .adjustments
            .insert("exposure".into(), -1.0);
        assert_ne!(
            d.virtual_copies[0].recipe.adjustments["exposure"],
            d.virtual_copies[1].recipe.adjustments["exposure"]
        );
        assert_eq!(
            d,
            SidecarDocument::from_json(&d.to_json().unwrap()).unwrap()
        );
    }

    #[test]
    fn rating_and_flag_roundtrip_per_copy() {
        // LR-01: rating (0..=5) and flag are per-copy metadata with a JSON
        // roundtrip; legacy documents without the fields read as unrated.
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].rating = 4;
        d.virtual_copies[0].flag = Flag::Pick;
        d.duplicate_virtual_copy("vc-original", "vc-copy", "Copy")
            .unwrap();
        // The duplicate inherits the source rating/flag as starting values.
        assert_eq!(d.virtual_copies[1].rating, 4);
        assert_eq!(d.virtual_copies[1].flag, Flag::Pick);
        d.virtual_copies[1].rating = 2;
        d.virtual_copies[1].flag = Flag::Reject;
        let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
        assert_eq!(decoded.virtual_copies[0].rating, 4);
        assert_eq!(decoded.virtual_copies[0].flag, Flag::Pick);
        assert_eq!(decoded.virtual_copies[1].rating, 2);
        assert_eq!(decoded.virtual_copies[1].flag, Flag::Reject);
        // Legacy JSON without the additive fields still loads as unrated.
        let mut legacy: Value = serde_json::from_str(&d.to_json().unwrap()).unwrap();
        for copy in legacy["virtual_copies"].as_array_mut().unwrap() {
            copy.as_object_mut().unwrap().remove("rating");
            copy.as_object_mut().unwrap().remove("flag");
        }
        let decoded = SidecarDocument::from_json(&serde_json::to_string(&legacy).unwrap()).unwrap();
        assert_eq!(decoded.virtual_copies[0].rating, 0);
        assert_eq!(decoded.virtual_copies[0].flag, Flag::Unflagged);
    }

    #[test]
    fn rating_above_five_is_rejected_loudly() {
        // LR-01: no silent clamping — 6 stars is a schema violation.
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].rating = 6;
        assert!(matches!(d.validate(), Err(SidecarError::Invalid(_))));
        assert!(matches!(d.to_json(), Err(SidecarError::Invalid(_))));
    }

    #[test]
    fn migration_unknown_fields_and_incompatible_version() {
        let d = SidecarDocument::new(source(), "pipeline-1");
        let mut value: Value = serde_json::from_str(&d.to_json().unwrap()).unwrap();
        value["schema_version"] = Value::from(0);
        value["virtual_copies"][0]["recipe"]
            .as_object_mut()
            .unwrap()
            .remove("recipe_version");
        let migrated = migrate_json(&serde_json::to_string(&value).unwrap()).unwrap();
        let decoded = SidecarDocument::from_json(&migrated).unwrap();
        assert_eq!(decoded.virtual_copies[0].recipe.recipe_version, "1");
        value["schema_version"] = Value::from(99);
        assert_eq!(
            migrate_json(&serde_json::to_string(&value).unwrap()).unwrap_err(),
            SidecarError::Invalid(
                "unsupported schema_version 99; explicit migration is required".into()
            )
        );
    }

    #[test]
    fn explicit_v1_to_v2_migration_keeps_flat_adjustments() {
        let mut document = SidecarDocument::new(source(), "pipeline-1");
        document.virtual_copies[0].recipe.adjustments.extend([
            (String::from("exposure"), 1.5),
            (String::from("contrast"), -0.25),
        ]);
        let mut legacy: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
        legacy["schema_version"] = Value::from(1);

        let migrated: Value =
            serde_json::from_str(&migrate_json(&serde_json::to_string(&legacy).unwrap()).unwrap())
                .unwrap();

        assert_eq!(migrated["schema_version"], Value::from(2));
        assert_eq!(
            migrated["virtual_copies"][0]["recipe"]["adjustments"]["exposure"],
            Value::from(1.5)
        );
        assert_eq!(
            migrated["virtual_copies"][0]["recipe"]["adjustments"]["contrast"],
            Value::from(-0.25)
        );
        assert_eq!(
            SidecarDocument::from_json(&serde_json::to_string(&migrated).unwrap())
                .unwrap()
                .schema_version,
            2
        );
    }

    #[test]
    fn curve_and_hsl_validation_reject_invalid_values() {
        let valid_sidecar = || {
            let document = SidecarDocument::new(source(), "pipeline-1");
            serde_json::to_value(document).unwrap()
        };
        let curve = |points: Vec<(f32, f32)>| {
            serde_json::json!({
                "version": 1,
                "master": points.into_iter().map(|(input, output)| {
                    serde_json::json!({"input": input, "output": output})
                }).collect::<Vec<_>>(),
                "channels": {}
            })
        };

        let invalid_curves = [
            // Fewer than two points.
            curve(vec![(0.0, 0.0)]),
            // Inputs are not strictly ascending.
            curve(vec![(0.0, 0.0), (0.5, 0.5), (0.5, 0.75), (1.0, 1.0)]),
            // Both required endpoints are absent.
            curve(vec![(0.25, 0.25), (0.75, 0.75)]),
        ];
        for invalid_curve in invalid_curves {
            let mut sidecar = valid_sidecar();
            sidecar["virtual_copies"][0]["recipe"]["adjustments"]["curves"] = invalid_curve;
            assert!(SidecarDocument::from_json(&serde_json::to_string(&sidecar).unwrap()).is_err());
        }

        let mut sidecar = valid_sidecar();
        sidecar["virtual_copies"][0]["recipe"]["adjustments"]["hsl"] = serde_json::json!({
            "version": 1,
            "red": {"hue": 1.1, "saturation": 0.0, "luminance": 0.0}
        });
        assert!(SidecarDocument::from_json(&serde_json::to_string(&sidecar).unwrap()).is_err());
    }

    #[test]
    fn legacy_flat_adjustments_api_and_json_remain_compatible() {
        let json = r#"{
            "recipe_version":"1",
            "adjustments":{"exposure":1.5,"contrast":-0.25},
            "options":{}, "auto_features":{}, "future_recipe":{"kept":true}
        }"#;
        let recipe: EditRecipe = serde_json::from_str(json).unwrap();
        assert_eq!(recipe.adjustments["exposure"], 1.5);
        assert_eq!(recipe.adjustments["contrast"], -0.25);
        assert!(recipe.curves.is_none() && recipe.hsl.is_none());
        assert!(recipe.extras.contains_key("future_recipe"));
        let encoded = serde_json::to_value(&recipe).unwrap();
        assert_eq!(encoded["adjustments"]["exposure"], 1.5);
        assert!(encoded["adjustments"].get("curves").is_none());
    }

    #[test]
    fn color_grading_roundtrips_as_nested_adjustment() {
        let recipe = EditRecipe {
            color_grading: Some(ColorGrading {
                version: 1,
                shadows: ColorGradingRange {
                    hue_degrees: 360.0,
                    saturation: 0.5,
                    luminance: 0.0,
                },
                midtones: ColorGradingRange {
                    hue_degrees: 120.0,
                    saturation: 0.25,
                    luminance: 0.0,
                },
                highlights: ColorGradingRange {
                    hue_degrees: 240.0,
                    saturation: 0.75,
                    luminance: 0.0,
                },
                balance: -0.2,
                blending: 0.5,
            }),
            ..Default::default()
        };
        let value = serde_json::to_value(&recipe).unwrap();
        assert!(value["adjustments"]["color_grading"].is_object());
        assert_eq!(recipe, serde_json::from_value(value).unwrap());
    }

    #[test]
    fn color_grading_legacy_fields_default_without_migration() {
        // Altdateien ohne `luminance`/`blending` lesen sich als
        // `luminance = 0` / `blending = 0.5` (identisches Renderverhalten).
        let legacy = serde_json::json!({
            "version": 1,
            "shadows": {"hue_degrees": 0.0, "saturation": 0.0},
            "midtones": {"hue_degrees": 0.0, "saturation": 0.0},
            "highlights": {"hue_degrees": 0.0, "saturation": 0.0},
            "balance": 0.0
        });
        let grading: ColorGrading = serde_json::from_value(legacy).unwrap();
        assert_eq!(grading.blending, 0.5);
        assert_eq!(grading.shadows.luminance, 0.0);
    }

    #[test]
    fn point_color_roundtrips_as_nested_adjustment_with_stable_ids() {
        let recipe = EditRecipe {
            point_color: Some(PointColor {
                version: 1,
                entries: vec![PointColorEntry {
                    id: "pc-1".into(),
                    hue_center: 30.0,
                    hue_range: 20.0,
                    hue_shift: 0.5,
                    saturation_shift: -0.25,
                    luminance_shift: 0.1,
                }],
            }),
            ..Default::default()
        };
        let value = serde_json::to_value(&recipe).unwrap();
        assert!(value["adjustments"]["point_color"].is_object());
        let roundtrip: EditRecipe = serde_json::from_value(value).unwrap();
        assert_eq!(recipe, roundtrip);
        assert_eq!(
            PointColorEntry::next_id(&roundtrip.point_color.expect("point color").entries),
            "pc-2"
        );
    }

    #[test]
    fn point_color_validation_rejects_bad_entries_loudly() {
        let bad = |entries: Vec<PointColorEntry>| {
            let recipe = EditRecipe {
                point_color: Some(PointColor {
                    version: 1,
                    entries,
                }),
                ..Default::default()
            };
            validate_adjustments(&recipe).is_err()
        };
        let entry = || PointColorEntry {
            id: "pc-1".into(),
            hue_center: 30.0,
            hue_range: 20.0,
            hue_shift: 0.0,
            saturation_shift: 0.0,
            luminance_shift: 0.0,
        };
        let mut out_of_range = entry();
        out_of_range.hue_center = 400.0;
        assert!(bad(vec![out_of_range]));
        let mut dup = entry();
        assert!(bad(vec![entry(), dup.clone()]));
        dup.id = String::new();
        assert!(bad(vec![dup]));
        assert!(bad(vec![entry(); 9]));
        // Gültiger Eintrag passiert die Validierung.
        assert!(!bad(vec![entry()]));
    }

    #[test]
    fn curves_use_curve_points_lists_and_hsl_channels_are_optional() {
        let recipe = EditRecipe {
            curves: Some(Curves {
                version: 1,
                master: vec![
                    CurvePoint {
                        input: 0.0,
                        output: 0.0,
                    },
                    CurvePoint {
                        input: 1.0,
                        output: 1.0,
                    },
                ],
                channels: CurveChannels::default(),
            }),
            hsl: Some(HslAdjustments {
                version: 1,
                ..Default::default()
            }),
            ..Default::default()
        };
        let value = serde_json::to_value(&recipe).unwrap();
        assert!(value["adjustments"]["curves"]["master"].is_array());
        assert!(value["adjustments"]["curves"]["master"]
            .get("points")
            .is_none());
        let roundtrip: EditRecipe = serde_json::from_value(value).unwrap();
        assert_eq!(roundtrip, recipe);
    }

    #[test]
    fn explicit_file_migration_creates_backup_and_rejects_newer_schema() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = SidecarDocument::new(source(), "pipeline-1");
        let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
        value["schema_version"] = Value::from(0);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(migrate_sidecar_file(&path).unwrap());
        assert!(path.with_file_name("image.lumina.json.bak").is_file());
        assert_eq!(load_sidecar(&path).unwrap().schema_version, SCHEMA_VERSION);

        value["schema_version"] = Value::from(99);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(migrate_sidecar_file(&path).is_err());
    }

    #[test]
    fn atomic_compare_and_swap_and_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let d = SidecarDocument::new(source(), "pipeline-1");
        let revision = save_sidecar_if_unchanged(&path, &d, None).unwrap();
        let mut changed = d.clone();
        changed.virtual_copies[0].name = "Changed".into();
        assert!(save_sidecar_if_unchanged(&path, &changed, Some("wrong")).is_err());
        save_sidecar_if_unchanged(&path, &changed, Some(&revision)).unwrap();
        // REVIEW-SIDECAR-TMP-1: only temporaries older than the sweep age are
        // orphaned; a fresh one is treated as a live writer's temporary.
        std::fs::write(
            directory.path().join(".image.lumina.json.tmp-crash"),
            b"partial",
        )
        .unwrap();
        assert!(directory
            .path()
            .join(".image.lumina.json.tmp-crash")
            .exists());
        backdate(
            &directory.path().join(".image.lumina.json.tmp-crash"),
            Duration::from_secs(60),
        );
        assert_eq!(
            load_sidecar(&path).unwrap().virtual_copies[0].name,
            "Changed"
        );
        assert!(!directory
            .path()
            .join(".image.lumina.json.tmp-crash")
            .exists());
    }

    #[test]
    fn recovery_never_promotes_partial_temporary_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let temp = directory.path().join(".image.lumina.json.tmp-crash");
        std::fs::write(&temp, b"{\"partial\": true}").unwrap();
        backdate(&temp, Duration::from_secs(60));
        assert!(matches!(load_sidecar(&path), Err(SidecarError::Missing(_))));
        assert!(!temp.exists());
    }

    /// REVIEW-SIDECAR-TMP-1 regression: a temporary belonging to a *live*
    /// writer (fresh mtime) must survive a concurrent reader's recovery sweep.
    #[test]
    fn recovery_spares_fresh_temporary_of_live_writer() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = SidecarDocument::new(source(), "pipeline-1");
        save_sidecar(&path, &document).unwrap();
        // Simulate another process mid-save: a fresh temporary with content.
        let live_temp = directory.path().join(".image.lumina.json.tmp-live");
        std::fs::write(&live_temp, b"{\"in-flight\": true").unwrap();
        // A load (which sweeps) must not delete the live writer's temporary...
        let loaded = load_sidecar(&path).unwrap();
        assert_eq!(loaded, document);
        assert!(
            live_temp.exists(),
            "recover_sidecar deleted a live writer's fresh temporary"
        );
        // ...while recover_sidecar reports nothing removed...
        let report = recover_sidecar(&path).unwrap();
        assert_eq!(report.removed_temporary_files, 0);
        assert!(live_temp.exists());
        // ...and once aged past the threshold it is swept again.
        backdate(&live_temp, TEMP_SWEEP_AGE + Duration::from_secs(1));
        let report = recover_sidecar(&path).unwrap();
        assert_eq!(report.removed_temporary_files, 1);
        assert!(!live_temp.exists());
    }

    #[test]
    fn compare_and_swap_detects_external_change() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = SidecarDocument::new(source(), "pipeline-1");
        let revision = save_sidecar_if_unchanged(&path, &document, None).unwrap();
        let mut external = document.clone();
        external.virtual_copies[0].name = "Edited elsewhere".into();
        save_sidecar(&path, &external).unwrap();
        let mut local = document;
        local.virtual_copies[0].name = "Local edit".into();
        assert!(matches!(
            save_sidecar_if_unchanged(&path, &local, Some(&revision)),
            Err(SidecarError::Conflict(_))
        ));
    }

    #[test]
    fn concurrent_compare_and_swap_allows_only_one_writer() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = SidecarDocument::new(source(), "pipeline-1");
        let revision = save_sidecar_if_unchanged(&path, &document, None).unwrap();
        let first_path = path.clone();
        let first_revision = revision.clone();
        let first = std::thread::spawn(move || {
            let mut edited = document.clone();
            edited.virtual_copies[0].name = "first".into();
            save_sidecar_if_unchanged(&first_path, &edited, Some(&first_revision))
        });
        let second_path = path.clone();
        let second_revision = revision;
        let second = std::thread::spawn(move || {
            let mut edited = SidecarDocument::new(source(), "pipeline-1");
            edited.virtual_copies[0].name = "second".into();
            save_sidecar_if_unchanged(&second_path, &edited, Some(&second_revision))
        });
        let results = [first.join().unwrap(), second.join().unwrap()];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(SidecarError::Conflict(_))))
                .count(),
            1
        );
    }

    #[test]
    fn source_and_artifact_conflicts_are_visible() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("source.raw");
        let bytes = b"source";
        std::fs::write(&source_path, bytes).unwrap();
        let mut identity = source();
        identity.byte_length = bytes.len() as u64;
        identity.content_hash = format!("blake3:{}", blake3::hash(bytes).to_hex());
        assert_eq!(
            source_status(&source_path, &identity).unwrap(),
            SourceStatus::Unchanged
        );
        std::fs::write(&source_path, b"changed").unwrap();
        assert_eq!(
            source_status(&source_path, &identity).unwrap(),
            SourceStatus::SourceChanged
        );
        std::fs::remove_file(&source_path).unwrap();
        assert_eq!(
            source_status(&source_path, &identity).unwrap(),
            SourceStatus::Missing
        );
        let artifact = ArtifactReference {
            relative_path: "masks/a.zdata".into(),
            format: "zdata".into(),
            checksum: "hash".into(),
            width: 1,
            height: 1,
            channels: "u16".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        };
        assert_eq!(
            artifact_status(directory.path(), &artifact),
            ArtifactStatus::Missing
        );
    }

    #[test]
    fn xmp_is_explicitly_unsupported() {
        assert!(!xmp_supported());
        assert!(matches!(
            SidecarError::XmpUnsupported,
            SidecarError::XmpUnsupported
        ));
    }

    #[test]
    fn noise_and_sharpening_roundtrip_and_validate_ranges() {
        let recipe = EditRecipe {
            noise_reduction: Some(NoiseReduction {
                version: 1,
                luminance: 0.4,
                color: 0.8,
            }),
            sharpening: Some(Sharpening {
                version: 1,
                amount: 2.0,
                radius: 3.0,
                detail: 0.5,
                masking: 0.7,
            }),
            ..Default::default()
        };
        let value = serde_json::to_value(&recipe).unwrap();
        assert!(value["adjustments"]["noise_reduction"].is_object());
        assert_eq!(recipe, serde_json::from_value(value).unwrap());
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.noise_reduction = Some(NoiseReduction {
            version: 2,
            luminance: 0.0,
            color: 0.0,
        });
        assert!(d.validate().is_err());
        d.virtual_copies[0].recipe.noise_reduction = Some(NoiseReduction {
            version: 1,
            luminance: f32::NAN,
            color: 0.0,
        });
        assert!(d.validate().is_err());
    }

    // ---- LRPAR-G14-REDEYE-15: red_eye recipe schema field ----

    fn red_eye_region(id: &str) -> RedEyeRegion {
        RedEyeRegion {
            id: id.into(),
            x: 0.25,
            y: 0.35,
            radius: 0.05,
            desaturate: 0.8,
            darken: 0.4,
        }
    }

    #[test]
    fn red_eye_roundtrip_and_validate_ranges() {
        let recipe = EditRecipe {
            red_eye: Some(RedEyeCorrection {
                version: 1,
                regions: vec![red_eye_region("re-1"), red_eye_region("re-2")],
            }),
            ..Default::default()
        };
        let value = serde_json::to_value(&recipe).unwrap();
        // `red_eye` lives inside `adjustments` (like `noise_reduction`).
        assert!(value["adjustments"]["red_eye"].is_object());
        assert_eq!(
            value["adjustments"]["red_eye"]["regions"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(recipe, serde_json::from_value(value).unwrap());

        // Absent key stays absent (additive, legacy identity, no migration).
        let legacy = serde_json::json!({
            "recipe_version": "1",
            "adjustments": {},
            "options": {},
            "auto_features": {"enable_auto_tone": false, "match_total_exposure": false, "target_luminance": 0.5},
        });
        let decoded: EditRecipe = serde_json::from_value(legacy).unwrap();
        assert!(decoded.red_eye.is_none());
        assert!(validate_adjustments(&decoded).is_ok());

        // Empty region list roundtrips and validates (identity).
        let empty = EditRecipe {
            red_eye: Some(RedEyeCorrection {
                version: 1,
                regions: Vec::new(),
            }),
            ..Default::default()
        };
        let value = serde_json::to_value(&empty).unwrap();
        assert_eq!(empty, serde_json::from_value(value).unwrap());
        assert!(validate_adjustments(&empty).is_ok());

        // Unknown version is rejected loudly.
        let mut bad = SidecarDocument::new(source(), "pipeline-1");
        bad.virtual_copies[0].recipe.red_eye = Some(RedEyeCorrection {
            version: 2,
            regions: vec![red_eye_region("re-1")],
        });
        assert!(bad.validate().is_err());
    }

    #[test]
    fn red_eye_rejects_out_of_range_and_nan() {
        let recipe = EditRecipe {
            red_eye: Some(RedEyeCorrection {
                version: 1,
                regions: vec![red_eye_region("re-1")],
            }),
            ..Default::default()
        };
        // Each invalid mutation fails loudly instead of being clipped.
        for mutate in [
            |r: &mut RedEyeRegion| r.x = 1.5,
            |r: &mut RedEyeRegion| r.y = -0.1,
            |r: &mut RedEyeRegion| r.x = f32::NAN,
            |r: &mut RedEyeRegion| r.radius = 0.0,
            |r: &mut RedEyeRegion| r.radius = 1.5,
            |r: &mut RedEyeRegion| r.radius = f32::INFINITY,
            |r: &mut RedEyeRegion| r.desaturate = -0.1,
            |r: &mut RedEyeRegion| r.desaturate = 1.1,
            |r: &mut RedEyeRegion| r.desaturate = f32::NAN,
            |r: &mut RedEyeRegion| r.darken = 2.0,
            |r: &mut RedEyeRegion| r.darken = f32::NAN,
        ] {
            let mut candidate = recipe.clone();
            mutate(&mut candidate.red_eye.as_mut().unwrap().regions[0]);
            assert!(validate_adjustments(&candidate).is_err());
        }
        // Empty and duplicate ids are rejected.
        let mut candidate = recipe.clone();
        candidate.red_eye.as_mut().unwrap().regions[0].id.clear();
        assert!(validate_adjustments(&candidate).is_err());
        let mut candidate = recipe.clone();
        candidate
            .red_eye
            .as_mut()
            .unwrap()
            .regions
            .push(red_eye_region("re-1"));
        assert!(validate_adjustments(&candidate).is_err());
        // The valid recipe passes.
        assert!(validate_adjustments(&recipe).is_ok());
    }

    // ---- LRPAR-G06-UPRIGHT-15: upright recipe stage ----

    fn upright_analysis() -> UprightAnalysis {
        UprightAnalysis {
            fingerprint: AnalysisFingerprint {
                algorithm: "upright-lines-v1".into(),
                version: "1".into(),
                input_fingerprint: "blake3:abc".into(),
                extras: Extras::new(),
            },
            vertical: 0.2,
            horizontal: -0.1,
            rotation: 0.05,
            line_count: 1234,
            confidence: 0.7,
        }
    }

    #[test]
    fn upright_roundtrip_and_validate_ranges() {
        // Disabled analysis (persisted suggestion, not applied) roundtrips at
        // the recipe root like `perspective`.
        let recipe = EditRecipe {
            upright: Some(Upright {
                version: 1,
                enabled: false,
                analysis: Some(upright_analysis()),
            }),
            ..Default::default()
        };
        let value = serde_json::to_value(&recipe).unwrap();
        assert!(value["upright"].is_object(), "upright lives at the root");
        assert_eq!(value["upright"]["enabled"], serde_json::Value::Bool(false));
        assert_eq!(recipe, serde_json::from_value(value).unwrap());
        assert!(validate_adjustments(&recipe).is_ok());
        // Disabled → the manual perspective stays authoritative.
        assert!(recipe.effective_perspective().is_none());

        // Enabled → the analysis supplies the effective perspective.
        let enabled = EditRecipe {
            upright: Some(Upright {
                version: 1,
                enabled: true,
                analysis: Some(upright_analysis()),
            }),
            perspective: Some(Perspective {
                version: 1,
                vertical: 0.9,
                horizontal: 0.9,
                rotation: 0.9,
                scale: 2.0,
                aspect_ratio: 1.0,
                shift_x: 0.0,
                shift_y: 0.0,
            }),
            ..Default::default()
        };
        let effective = enabled
            .effective_perspective()
            .expect("enabled upright supplies a perspective");
        assert_eq!(effective.vertical, 0.2);
        assert_eq!(effective.horizontal, -0.1);
        assert_eq!(effective.rotation, 0.05);
        assert_eq!(effective.scale, 1.0);

        // Disabling restores the manual perspective unchanged.
        let mut disabled = enabled.clone();
        disabled.upright.as_mut().unwrap().enabled = false;
        assert_eq!(
            disabled.effective_perspective().unwrap().vertical,
            0.9,
            "manual perspective returns when upright is off"
        );

        // Absent key stays absent (additive, legacy identity, no migration).
        let legacy = serde_json::json!({
            "recipe_version": "1",
            "adjustments": {},
            "options": {},
            "auto_features": {"enable_auto_tone": false, "match_total_exposure": false, "target_luminance": 0.5},
        });
        let decoded: EditRecipe = serde_json::from_value(legacy).unwrap();
        assert!(decoded.upright.is_none());
        assert!(validate_adjustments(&decoded).is_ok());
    }

    #[test]
    fn upright_enabled_without_analysis_is_rejected_loudly() {
        let recipe = EditRecipe {
            upright: Some(Upright {
                version: 1,
                enabled: true,
                analysis: None,
            }),
            ..Default::default()
        };
        assert!(validate_adjustments(&recipe).is_err());
        // Disabled without analysis is a valid no-op state.
        let disabled = EditRecipe {
            upright: Some(Upright {
                version: 1,
                enabled: false,
                analysis: None,
            }),
            ..Default::default()
        };
        assert!(validate_adjustments(&disabled).is_ok());
        // Foreign version is rejected.
        let mut bad = SidecarDocument::new(source(), "pipeline-1");
        bad.virtual_copies[0].recipe.upright = Some(Upright {
            version: 2,
            enabled: false,
            analysis: None,
        });
        assert!(bad.validate().is_err());
    }

    #[test]
    fn upright_rejects_out_of_range_and_nan() {
        let base = EditRecipe {
            upright: Some(Upright {
                version: 1,
                enabled: true,
                analysis: Some(upright_analysis()),
            }),
            ..Default::default()
        };
        for mutate in [
            |a: &mut UprightAnalysis| a.vertical = 1.5,
            |a: &mut UprightAnalysis| a.horizontal = -1.5,
            |a: &mut UprightAnalysis| a.rotation = f32::NAN,
            |a: &mut UprightAnalysis| a.confidence = 1.1,
            |a: &mut UprightAnalysis| a.confidence = f32::NAN,
            |a: &mut UprightAnalysis| a.line_count = UPRIGHT_MAX_LINE_COUNT + 1,
            |a: &mut UprightAnalysis| a.fingerprint.algorithm.clear(),
            |a: &mut UprightAnalysis| a.fingerprint.version.clear(),
            |a: &mut UprightAnalysis| a.fingerprint.input_fingerprint.clear(),
        ] {
            let mut candidate = base.clone();
            mutate(
                candidate
                    .upright
                    .as_mut()
                    .unwrap()
                    .analysis
                    .as_mut()
                    .unwrap(),
            );
            assert!(
                validate_adjustments(&candidate).is_err(),
                "invalid upright contract must be rejected loudly"
            );
        }
        assert!(validate_adjustments(&base).is_ok());
    }

    // ---- F-097: effects (vignette + grain) recipe schema field ----

    #[test]
    fn effects_roundtrip_and_validate_ranges() {
        let recipe = EditRecipe {
            effects: Some(Effects {
                vignette: Some(Vignette {
                    version: 1,
                    amount: -0.6,
                    midpoint: 0.35,
                    roundness: 0.8,
                    feather: 0.2,
                }),
                grain: Some(Grain {
                    version: 1,
                    amount: 0.5,
                    size: 0.75,
                    roughness: 0.25,
                    seed: 123456789,
                }),
            }),
            ..Default::default()
        };
        let value = serde_json::to_value(&recipe).unwrap();
        assert!(value["effects"]["vignette"].is_object());
        assert!(value["effects"]["grain"].is_object());
        // `effects` lives at the recipe root (like `geometry`), not inside
        // `adjustments`.
        assert!(value["adjustments"].is_object());
        assert!(!value["adjustments"]
            .as_object()
            .unwrap()
            .contains_key("effects"));
        assert_eq!(recipe, serde_json::from_value(value).unwrap());

        // Full sidecar roundtrip preserves the effects block.
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.effects = Some(Effects {
            vignette: Some(Vignette {
                version: 1,
                amount: 0.4,
                midpoint: 0.1,
                roundness: -0.5,
                feather: 0.9,
            }),
            grain: Some(Grain {
                version: 1,
                amount: 0.3,
                size: 0.2,
                roughness: 0.8,
                seed: 42,
            }),
        });
        let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
        assert_eq!(decoded.virtual_copies[0].recipe, d.virtual_copies[0].recipe);
    }

    #[test]
    fn effects_validation_rejects_invalid_values() {
        // Invalid vignette version.
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.effects = Some(Effects {
            vignette: Some(Vignette {
                version: 2,
                amount: 0.0,
                midpoint: 0.0,
                roundness: 0.0,
                feather: 0.0,
            }),
            grain: None,
        });
        assert!(d.validate().is_err());

        // Out-of-range vignette amount.
        d.virtual_copies[0].recipe.effects = Some(Effects {
            vignette: Some(Vignette {
                version: 1,
                amount: 2.0,
                midpoint: 0.0,
                roundness: 0.0,
                feather: 0.0,
            }),
            grain: None,
        });
        assert!(d.validate().is_err());

        // Out-of-range vignette midpoint (NaN).
        d.virtual_copies[0].recipe.effects = Some(Effects {
            vignette: Some(Vignette {
                version: 1,
                amount: 0.0,
                midpoint: f32::NAN,
                roundness: 0.0,
                feather: 0.0,
            }),
            grain: None,
        });
        assert!(d.validate().is_err());

        // Out-of-range grain amount.
        d.virtual_copies[0].recipe.effects = Some(Effects {
            vignette: None,
            grain: Some(Grain {
                version: 1,
                amount: -0.1,
                size: 0.0,
                roughness: 0.0,
                seed: 1,
            }),
        });
        assert!(d.validate().is_err());

        // Out-of-range grain roughness.
        d.virtual_copies[0].recipe.effects = Some(Effects {
            vignette: None,
            grain: Some(Grain {
                version: 1,
                amount: 0.0,
                size: 1.5,
                roughness: 0.0,
                seed: 1,
            }),
        });
        assert!(d.validate().is_err());
    }

    /// G-05 Lens Blur: recipe JSON roundtrip (root-level `lens_blur` key with
    /// all bokeh variants) plus per-virtual-copy independence.
    #[test]
    fn lens_blur_roundtrip_and_per_copy_independence() {
        for bokeh in [
            BokehShape::Round,
            BokehShape::Elliptical,
            BokehShape::Hexagonal,
        ] {
            let recipe = EditRecipe {
                lens_blur: Some(LensBlur {
                    version: 1,
                    enabled: true,
                    focus_rect: FocusRect {
                        x: 0.2,
                        y: 0.3,
                        width: 0.4,
                        height: 0.25,
                    },
                    focal_near: 0.1,
                    focal_far: 0.6,
                    blur_amount: 0.7,
                    bokeh,
                    depth_artifact: Some(DepthArtifactRef {
                        relative_path: "depth/map.bin".into(),
                        sha256: "sha256:abc".into(),
                    }),
                }),
                ..Default::default()
            };
            let value = serde_json::to_value(&recipe).unwrap();
            assert!(value["lens_blur"].is_object());
            assert_eq!(
                value["lens_blur"]["bokeh"],
                serde_json::to_value(bokeh).unwrap()
            );
            assert!(!value["adjustments"]
                .as_object()
                .unwrap()
                .contains_key("lens_blur"));
            assert_eq!(recipe, serde_json::from_value(value).unwrap());
        }

        // Two virtual copies carry independent lens-blur recipes (stable IDs,
        // no positional identification).
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.lens_blur = Some(LensBlur {
            version: 1,
            enabled: true,
            focus_rect: FocusRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            focal_near: 0.0,
            focal_far: 1.0,
            blur_amount: 0.25,
            bokeh: BokehShape::Round,
            depth_artifact: None,
        });
        d.duplicate_virtual_copy("vc-original", "vc-blur-hex", "Blur Hex")
            .unwrap();
        let other = d
            .virtual_copies
            .iter_mut()
            .find(|c| c.id == "vc-blur-hex")
            .unwrap();
        other.recipe.lens_blur = Some(LensBlur {
            version: 1,
            enabled: true,
            focus_rect: FocusRect {
                x: 0.1,
                y: 0.1,
                width: 0.5,
                height: 0.5,
            },
            focal_near: 0.2,
            focal_far: 0.4,
            blur_amount: 0.9,
            bokeh: BokehShape::Hexagonal,
            depth_artifact: None,
        });
        let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
        assert_eq!(decoded.virtual_copies.len(), 2);
        let first = decoded
            .virtual_copies
            .iter()
            .find(|c| c.id == "vc-original")
            .unwrap();
        let second = decoded
            .virtual_copies
            .iter()
            .find(|c| c.id == "vc-blur-hex")
            .unwrap();
        assert_ne!(first.recipe.lens_blur, second.recipe.lens_blur);
        assert_eq!(first.recipe.lens_blur, d.virtual_copies[0].recipe.lens_blur);
        assert!(decoded.validate().is_ok());
    }

    /// G-05 Lens Blur: every out-of-range value fails loudly (no silent
    /// clipping), including absolute depth-artifact paths.
    #[test]
    fn lens_blur_validation_rejects_invalid_values() {
        fn doc_with(mutate: impl FnOnce(&mut LensBlur)) -> SidecarDocument {
            let mut d = SidecarDocument::new(source(), "pipeline-1");
            let mut b = LensBlur {
                version: 1,
                enabled: true,
                focus_rect: FocusRect {
                    x: 0.2,
                    y: 0.2,
                    width: 0.4,
                    height: 0.4,
                },
                focal_near: 0.1,
                focal_far: 0.6,
                blur_amount: 0.5,
                bokeh: BokehShape::Round,
                depth_artifact: None,
            };
            mutate(&mut b);
            d.virtual_copies[0].recipe.lens_blur = Some(b);
            d
        }
        // Bad version.
        assert!(doc_with(|b| b.version = 2).validate().is_err());
        // Out-of-range / non-finite scalars.
        assert!(doc_with(|b| b.focal_near = -0.1).validate().is_err());
        assert!(doc_with(|b| b.focal_far = 1.5).validate().is_err());
        assert!(doc_with(|b| b.blur_amount = f32::NAN).validate().is_err());
        // Inverted focal range.
        assert!(doc_with(|b| {
            b.focal_near = 0.7;
            b.focal_far = 0.6;
        })
        .validate()
        .is_err());
        // Degenerate / out-of-bounds focus rectangles.
        assert!(doc_with(|b| b.focus_rect.width = 0.0).validate().is_err());
        assert!(doc_with(|b| b.focus_rect.x = -0.1).validate().is_err());
        assert!(doc_with(|b| {
            b.focus_rect.x = 0.8;
            b.focus_rect.width = 0.3;
        })
        .validate()
        .is_err());
        // Absolute depth-artifact path (portable sidecars stay relative).
        assert!(doc_with(|b| {
            b.depth_artifact = Some(DepthArtifactRef {
                relative_path: "/abs/depth.bin".into(),
                sha256: "sha256:abc".into(),
            });
        })
        .validate()
        .is_err());
        // Path traversal.
        assert!(doc_with(|b| {
            b.depth_artifact = Some(DepthArtifactRef {
                relative_path: "../depth.bin".into(),
                sha256: "sha256:abc".into(),
            });
        })
        .validate()
        .is_err());
        // The valid base document passes.
        assert!(doc_with(|_| {}).validate().is_ok());
    }
    #[test]
    fn lens_and_perspective_roundtrip_and_validation() {
        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0]
            .recipe
            .options
            .insert("render_profile".into(), "display-p3".into());
        d.virtual_copies[0].recipe.lens_correction = Some(LensCorrection {
            version: 1,
            profile: Some("wide-light".into()),
            distortion_k1: Some(0.0),
            distortion_k2: Some(0.0),
            distortion_k3: Some(0.0),
            vignette_c0: Some(1.0),
            vignette_c1: Some(0.0),
            vignette_c2: Some(0.0),
            ca_red: Some(0.0),
            ca_blue: Some(0.0),
        });
        d.virtual_copies[0].recipe.perspective = Some(Perspective {
            version: 1,
            vertical: 0.2,
            horizontal: 0.0,
            rotation: 0.0,
            scale: 1.0,
            aspect_ratio: 1.0,
            shift_x: 0.0,
            shift_y: 0.0,
        });
        let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
        assert_eq!(decoded.virtual_copies[0].recipe, d.virtual_copies[0].recipe);
        assert_eq!(
            decoded.virtual_copies[0]
                .recipe
                .options
                .get("render_profile"),
            Some(&"display-p3".to_string())
        );
        d.virtual_copies[0]
            .recipe
            .lens_correction
            .as_mut()
            .unwrap()
            .ca_red = Some(f32::NAN);
        assert!(d.validate().is_err());
    }

    // ---- F-042-N1: source_actions recipe schema field ----

    fn source_action_spec(version: u16, kind: SourceActionKind) -> SourceActionSpec {
        SourceActionSpec {
            version,
            kind,
            artifact: SourceActionArtifactRef {
                id: "repair-1".into(),
                relative_path: "IMG_0001.ARW.lumina.zdata".into(),
                checksum: "blake3:abc".into(),
            },
        }
    }

    #[test]
    fn source_actions_empty_list_roundtrips_and_is_absent_when_empty() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.source_actions = vec![];
        let json = d.to_json().unwrap();
        assert!(!json.contains("source_actions"));
        let decoded = SidecarDocument::from_json(&json).unwrap();
        assert!(decoded.virtual_copies[0].recipe.source_actions.is_empty());
    }

    #[test]
    fn source_actions_non_empty_roundtrip_preserves_fields() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.source_actions = vec![source_action_spec(
            SOURCE_ACTION_VERSION,
            SourceActionKind::DustRemoval,
        )];
        let json = d.to_json().unwrap();
        assert!(json.contains("source_actions"));
        let decoded = SidecarDocument::from_json(&json).unwrap();
        assert_eq!(
            decoded.virtual_copies[0].recipe.source_actions,
            vec![source_action_spec(
                SOURCE_ACTION_VERSION,
                SourceActionKind::DustRemoval
            )]
        );
    }

    #[test]
    fn source_actions_absent_key_is_empty_list() {
        let json = r#"{"format":"lumina-sidecar","schema_version":2,"source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"PNG","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","presets":[],"virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{},"mask_library":[],"mask_layers":[],"history":[],"export_records":[]}]}"#;
        let doc = SidecarDocument::from_json(json).unwrap();
        assert!(doc.virtual_copies[0].recipe.source_actions.is_empty());
    }

    #[test]
    fn source_actions_unknown_kind_is_rejected() {
        let json = r#"{"version":1,"kind":"explode","artifact":{"id":"r","relative_path":"a.zdata","checksum":"c"}}"#;
        assert!(serde_json::from_str::<SourceActionSpec>(json).is_err());
    }

    #[test]
    fn source_actions_bad_version_is_rejected() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.source_actions =
            vec![source_action_spec(99, SourceActionKind::DustRemoval)];
        assert!(d.validate().is_err());
    }

    #[test]
    fn source_actions_bad_artifact_ref_is_rejected() {
        // empty id
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.source_actions = vec![SourceActionSpec {
            version: SOURCE_ACTION_VERSION,
            kind: SourceActionKind::AiReplacement,
            artifact: SourceActionArtifactRef {
                id: String::new(),
                relative_path: "a.zdata".into(),
                checksum: "c".into(),
            },
        }];
        assert!(d.validate().is_err());

        // absolute relative_path
        let mut d2 = SidecarDocument::new(source(), "pipeline-1");
        d2.virtual_copies[0].recipe.source_actions = vec![SourceActionSpec {
            version: SOURCE_ACTION_VERSION,
            kind: SourceActionKind::DustRemoval,
            artifact: SourceActionArtifactRef {
                id: "r".into(),
                relative_path: "/abs/a.zdata".into(),
                checksum: "c".into(),
            },
        }];
        assert!(d2.validate().is_err());

        // empty checksum
        let mut d3 = SidecarDocument::new(source(), "pipeline-1");
        d3.virtual_copies[0].recipe.source_actions = vec![SourceActionSpec {
            version: SOURCE_ACTION_VERSION,
            kind: SourceActionKind::DustRemoval,
            artifact: SourceActionArtifactRef {
                id: "r".into(),
                relative_path: "a.zdata".into(),
                checksum: String::new(),
            },
        }];
        assert!(d3.validate().is_err());
    }

    // ---- GEN-ZDATA-LINK-1: generative zdata recipe links ----

    fn generative_link() -> GenerativeArtifactRef {
        GenerativeArtifactRef {
            id: "gen-canvas-1".into(),
            relative_path: "IMG_0001.ARW.lumina.zdata".into(),
            format: "lumina-zdata".into(),
            checksum: "blake3:abc123".into(),
            width: 6000,
            height: 4000,
            channels: "rgba8".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        }
    }

    fn generative_edit_with_link() -> GenerativeEdit {
        GenerativeEdit {
            version: 1,
            canvas: None,
            artifact: Some(generative_link()),
            keep_generative_content: None,
            auto_fill_transparent: None,
            expand_beyond_image: None,
            seed: Some(42),
            prompt: Some("extend the sky".into()),
            extras: Extras::new(),
        }
    }

    fn spot_removal(mode: SpotRemovalMode, artifact: Option<GenerativeArtifactRef>) -> SpotRemoval {
        SpotRemoval {
            version: SPOT_REMOVAL_VERSION,
            mode,
            artifact,
        }
    }

    #[test]
    fn generative_artifact_link_roundtrips() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.generative_edit = Some(generative_edit_with_link());
        let json = d.to_json().unwrap();
        assert!(json.contains("gen-canvas-1"));
        let decoded = SidecarDocument::from_json(&json).unwrap();
        assert_eq!(
            decoded.virtual_copies[0].recipe.generative_edit,
            Some(generative_edit_with_link())
        );
    }

    #[test]
    fn generative_link_and_spot_removals_absent_keys_are_identity() {
        // Legacy documents without the additive keys read as no link / empty
        // list and require no migration.
        let json = r#"{"format":"lumina-sidecar","schema_version":2,"source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"PNG","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","presets":[],"virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{"generative_edit":{"version":1}},"mask_library":[],"mask_layers":[],"history":[],"export_records":[]}]}"#;
        let doc = SidecarDocument::from_json(json).unwrap();
        let recipe = &doc.virtual_copies[0].recipe;
        assert_eq!(recipe.generative_edit.as_ref().unwrap().artifact, None);
        assert!(recipe.spot_removals.is_empty());
    }

    #[test]
    fn generative_and_spot_unknown_versions_are_rejected() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        let mut bad = generative_edit_with_link();
        bad.version = 99;
        d.virtual_copies[0].recipe.generative_edit = Some(bad);
        assert!(d.validate().is_err());

        let mut d2 = SidecarDocument::new(source(), "pipeline-1");
        d2.virtual_copies[0].recipe.spot_removals = vec![spot_removal(
            SpotRemovalMode::Generative,
            Some(generative_link()),
        )];
        d2.virtual_copies[0].recipe.spot_removals[0].version = 99;
        assert!(d2.validate().is_err());
    }

    #[test]
    fn generative_bad_link_is_rejected() {
        let mut bad_cases = Vec::new();
        let mut empty_id = generative_link();
        empty_id.id.clear();
        bad_cases.push(empty_id);
        let mut absolute = generative_link();
        absolute.relative_path = "/abs/a.zdata".into();
        bad_cases.push(absolute);
        let mut opaque_format = generative_link();
        opaque_format.format = "opaque".into();
        bad_cases.push(opaque_format);
        let mut empty_checksum = generative_link();
        empty_checksum.checksum.clear();
        bad_cases.push(empty_checksum);
        let mut zero_dims = generative_link();
        zero_dims.width = 0;
        bad_cases.push(zero_dims);
        let mut wrong_channels = generative_link();
        wrong_channels.channels = "f32".into();
        bad_cases.push(wrong_channels);
        let mut wrong_data_version = generative_link();
        wrong_data_version.data_version = "2".into();
        bad_cases.push(wrong_data_version);
        for link in bad_cases {
            let mut d = SidecarDocument::new(source(), "pipeline-1");
            let mut edit = generative_edit_with_link();
            edit.artifact = Some(link);
            d.virtual_copies[0].recipe.generative_edit = Some(edit);
            assert!(d.validate().is_err());
        }
    }

    #[test]
    fn spot_heuristic_rejects_artifact_generative_roundtrips() {
        // Heuristic + artifact is a loud exclusion violation.
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.spot_removals = vec![spot_removal(
            SpotRemovalMode::Heuristic,
            Some(generative_link()),
        )];
        assert!(d.validate().is_err());

        // Generative with link roundtrips; the load carries the extras mirror of
        // the same key (SPOT-SCHEMA-GEOMETRY) which stays valid (generative
        // needs no geometry, link verified).
        let mut d2 = SidecarDocument::new(source(), "pipeline-1");
        d2.virtual_copies[0].recipe.spot_removals = vec![spot_removal(
            SpotRemovalMode::Generative,
            Some(generative_link()),
        )];
        let json = d2.to_json().unwrap();
        assert!(json.contains("spot_removals"));
        let decoded = SidecarDocument::from_json(&json).unwrap();
        assert_eq!(
            decoded.virtual_copies[0].recipe.spot_removals,
            d2.virtual_copies[0].recipe.spot_removals
        );
        assert_eq!(
            decoded.virtual_copies[0].recipe.extras.get("spot_removals"),
            Some(&serde_json::to_value(&d2.virtual_copies[0].recipe.spot_removals).unwrap())
        );
    }

    #[test]
    fn spot_typed_heuristic_without_geometry_is_loudly_invalid_on_load() {
        // SPOT-SCHEMA-GEOMETRY: params-lose typed heuristic entries (no heal
        // geometry anywhere) serialize without key loss, but loading them is
        // rejected loudly — the mirrored extras entry misses the mandatory
        // geometry. Old entries stay recognizable as missing/invalid instead
        // of rendering as if no spot existed.
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.spot_removals =
            vec![spot_removal(SpotRemovalMode::Heuristic, None)];
        let json = d.to_json().unwrap();
        assert!(json.contains("spot_removals"));
        let err = SidecarDocument::from_json(&json).unwrap_err();
        assert!(
            err.to_string().contains("spot_removal"),
            "loud geometry error expected, got {err}"
        );
    }

    #[test]
    fn spot_removals_empty_list_is_absent_when_empty() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.spot_removals = vec![];
        let json = d.to_json().unwrap();
        assert!(!json.contains("spot_removals"));
        let decoded = SidecarDocument::from_json(&json).unwrap();
        assert!(decoded.virtual_copies[0].recipe.spot_removals.is_empty());
        assert!(!decoded.virtual_copies[0]
            .recipe
            .extras
            .contains_key("spot_removals"));
    }

    fn heuristic_spot_extra() -> Value {
        serde_json::json!({
            "id": "spot-1",
            "version": 1,
            "mode": "heuristic",
            "center_x": 0.25,
            "center_y": 0.5,
            "radius": 2.0,
            "feather": 0.5,
            "offset_dx": 0.5,
            "offset_dy": 0.0,
            "opacity": 1.0,
            "status": "valid"
        })
    }

    #[test]
    fn spot_removals_extras_heuristic_geometry_survives_roundtrip() {
        // SPOT-SCHEMA-GEOMETRY detector (mirrors the GUI headless test
        // `spot_heal_headless_quick_heal_q_shortcut_and_render` at sidecar
        // level): producer-written extras heal geometry must survive
        // save/load losslessly — the 69dad91 data loss may never return.
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.extras.insert(
            "spot_removals".into(),
            Value::Array(vec![heuristic_spot_extra()]),
        );
        let json = d.to_json().unwrap();
        let decoded = SidecarDocument::from_json(&json).unwrap();
        assert_eq!(
            decoded.virtual_copies[0].recipe.extras.get("spot_removals"),
            d.virtual_copies[0].recipe.extras.get("spot_removals")
        );
        assert_eq!(decoded.virtual_copies[0].recipe.spot_removals.len(), 1);
        assert_eq!(
            decoded.virtual_copies[0].recipe.spot_removals[0].mode,
            SpotRemovalMode::Heuristic
        );
        decoded.validate().unwrap();
        // Second roundtrip is a fixed point (mirror of mirror is identical).
        let decoded2 = SidecarDocument::from_json(&decoded.to_json().unwrap()).unwrap();
        assert_eq!(
            decoded2.virtual_copies[0]
                .recipe
                .extras
                .get("spot_removals"),
            d.virtual_copies[0].recipe.extras.get("spot_removals")
        );
    }

    #[test]
    fn spot_removals_extras_generative_roundtrips_without_geometry() {
        // Generative spots need no heal geometry; the extras view roundtrips
        // with mode intact and the typed view carries version/mode.
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.extras.insert(
            "spot_removals".into(),
            serde_json::json!([{"id": "g1", "version": 1, "mode": "generative"}]),
        );
        let json = d.to_json().unwrap();
        let decoded = SidecarDocument::from_json(&json).unwrap();
        assert_eq!(
            decoded.virtual_copies[0].recipe.extras.get("spot_removals"),
            d.virtual_copies[0].recipe.extras.get("spot_removals")
        );
        assert_eq!(decoded.virtual_copies[0].recipe.spot_removals.len(), 1);
        assert_eq!(
            decoded.virtual_copies[0].recipe.spot_removals[0].mode,
            SpotRemovalMode::Generative
        );
        decoded.validate().unwrap();
    }

    #[test]
    fn spot_removals_extras_generative_bad_link_is_rejected() {
        // A generative extras entry with a corrupt artifact link fails loudly.
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        let mut bad_link = generative_link();
        bad_link.checksum.clear();
        d.virtual_copies[0].recipe.extras.insert(
            "spot_removals".into(),
            serde_json::json!([{
                "id": "g1", "version": 1, "mode": "generative",
                "artifact": serde_json::to_value(&bad_link).unwrap()
            }]),
        );
        assert!(d.validate().is_err());
    }

    #[test]
    fn spot_removals_extras_validation_rejects_loudly() {
        // Every malformed extras entry fails loudly — no silent fallback, no
        // silent reinterpretation. Each case is a full entry array element.
        let mut cases: Vec<(&str, Value)> = Vec::new();
        // Unknown version.
        let mut v = heuristic_spot_extra();
        v["version"] = serde_json::json!(99);
        cases.push(("version", v));
        // Missing version.
        let mut v = heuristic_spot_extra();
        v.as_object_mut().unwrap().remove("version");
        cases.push(("missing-version", v));
        // Unknown mode.
        let mut v = heuristic_spot_extra();
        v["mode"] = serde_json::json!("clone");
        cases.push(("mode", v));
        // Missing geometry (params-lose shape).
        for field in [
            "id",
            "center_x",
            "center_y",
            "radius",
            "offset_dx",
            "offset_dy",
        ] {
            let mut v = heuristic_spot_extra();
            v.as_object_mut().unwrap().remove(field);
            cases.push(("missing-geometry", v));
        }
        // Out-of-range geometry.
        let mut v = heuristic_spot_extra();
        v["radius"] = serde_json::json!(0.0);
        cases.push(("radius-range", v));
        let mut v = heuristic_spot_extra();
        v["center_x"] = serde_json::json!(1.5);
        cases.push(("center-range", v));
        let mut v = heuristic_spot_extra();
        v["opacity"] = serde_json::json!(2.0);
        cases.push(("opacity-range", v));
        // Wrong-typed geometry (JSON has no non-finite numbers; a string must
        // fail loudly instead of being coerced or skipped).
        let mut v = heuristic_spot_extra();
        v["radius"] = serde_json::json!("wide");
        cases.push(("radius-type", v));
        // Heuristic must not carry an artifact (mirrors the typed exclusion rule).
        let mut v = heuristic_spot_extra();
        v["artifact"] = serde_json::to_value(generative_link()).unwrap();
        cases.push(("heuristic-artifact", v));
        // Non-object entry and non-array key.
        cases.push(("non-object", serde_json::json!("spot-1")));
        for (name, entry) in cases {
            let mut d = SidecarDocument::new(source(), "pipeline-1");
            d.virtual_copies[0]
                .recipe
                .extras
                .insert("spot_removals".into(), Value::Array(vec![entry]));
            assert!(
                d.validate().is_err(),
                "extras entry `{name}` must be rejected loudly"
            );
        }
        // Non-array key shape.
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.extras.insert(
            "spot_removals".into(),
            serde_json::json!({"mode": "heuristic"}),
        );
        assert!(d.validate().is_err());
    }

    // ----- LRPAR-G04-REMOVE: visualize, distraction, variant controls -----
    #[test]
    fn g04_visualize_threshold_roundtrip_and_validation() {
        let mut recipe = EditRecipe::default();
        assert_eq!(recipe.spot_visualize_threshold(), None);
        recipe.set_spot_visualize_threshold(Some(0.35)).unwrap();
        assert_eq!(recipe.spot_visualize_threshold(), Some(0.35));
        assert!(recipe.set_spot_visualize_threshold(Some(1.5)).is_err());
        assert!(recipe.set_spot_visualize_threshold(Some(f32::NAN)).is_err());
        assert_eq!(recipe.spot_visualize_threshold(), Some(0.35));
        recipe.set_spot_visualize_threshold(None).unwrap();
        assert_eq!(recipe.spot_visualize_threshold(), None);
        assert!(!recipe.extras.contains_key(SPOT_VISUALIZE_KEY));
        // Persisted value survives a document roundtrip and validates.
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0]
            .recipe
            .set_spot_visualize_threshold(Some(0.2))
            .unwrap();
        assert!(d.validate().is_ok());
        let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
        assert_eq!(
            decoded.virtual_copies[0].recipe.spot_visualize_threshold(),
            Some(0.2)
        );
        assert!(decoded.validate().is_ok());
        // A hand-edited out-of-range value fails loudly.
        let mut bad = SidecarDocument::new(source(), "pipeline-1");
        bad.virtual_copies[0]
            .recipe
            .extras
            .insert(SPOT_VISUALIZE_KEY.into(), serde_json::json!(2.0));
        assert!(bad.validate().is_err());
        let mut bad_type = SidecarDocument::new(source(), "pipeline-1");
        bad_type.virtual_copies[0]
            .recipe
            .extras
            .insert(SPOT_VISUALIZE_KEY.into(), serde_json::json!("low"));
        assert!(bad_type.validate().is_err());
    }

    #[test]
    fn g04_distraction_switches_roundtrip_and_default_off() {
        let recipe = EditRecipe::default();
        assert_eq!(recipe.spot_distraction(), SpotDistraction::default());
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        assert!(d.validate().is_ok());
        let setting = SpotDistraction {
            dust: true,
            auto_mode: true,
            ..Default::default()
        };
        d.virtual_copies[0].recipe.set_spot_distraction(setting);
        assert_eq!(d.virtual_copies[0].recipe.spot_distraction(), setting);
        assert!(d.validate().is_ok());
        let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
        assert_eq!(decoded.virtual_copies[0].recipe.spot_distraction(), setting);
        // Resetting to all-off removes the key (legacy byte-stability).
        d.virtual_copies[0]
            .recipe
            .set_spot_distraction(SpotDistraction::default());
        assert!(!d.virtual_copies[0]
            .recipe
            .extras
            .contains_key(SPOT_DISTRACTION_KEY));
        // A non-object value fails loudly.
        let mut bad = SidecarDocument::new(source(), "pipeline-1");
        bad.virtual_copies[0]
            .recipe
            .extras
            .insert(SPOT_DISTRACTION_KEY.into(), serde_json::json!("dust"));
        assert!(bad.validate().is_err());
    }

    #[test]
    fn g04_generative_variant_controls_roundtrip_and_reject_loudly() {
        // seed/variant/base_seed/prompt ride the generative extras entry and
        // validate (G04-FOLLOWUP-1: `base_seed` is the regenerate provenance).
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.extras.insert(
            "spot_removals".into(),
            serde_json::json!([{
                "id": "g1", "version": 1, "mode": "generative",
                "prompt": "remove dust", "seed": 7, "variant": 2, "base_seed": 7
            }]),
        );
        assert!(d.validate().is_ok());
        let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
        assert_eq!(
            decoded.virtual_copies[0].recipe.extras.get("spot_removals"),
            d.virtual_copies[0].recipe.extras.get("spot_removals")
        );
        for (field, value) in [
            ("seed", serde_json::json!("seven")),
            ("variant", serde_json::json!(-1)),
            ("base_seed", serde_json::json!("seven")),
            ("base_seed", serde_json::json!(-1)),
            ("prompt", serde_json::json!(42)),
        ] {
            let mut bad = SidecarDocument::new(source(), "pipeline-1");
            let mut entry = serde_json::json!({"id": "g1", "version": 1, "mode": "generative"});
            entry[field] = value;
            bad.virtual_copies[0]
                .recipe
                .extras
                .insert("spot_removals".into(), Value::Array(vec![entry]));
            assert!(
                bad.validate().is_err(),
                "generative `{field}` must be rejected loudly"
            );
        }
    }

    #[test]
    fn save_load_atomic_roundtrip_preserves_generative_links() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let mut document = SidecarDocument::new(source(), "pipeline-1");
        document.virtual_copies[0].recipe.generative_edit = Some(generative_edit_with_link());
        document.virtual_copies[0].recipe.spot_removals = vec![spot_removal(
            SpotRemovalMode::Generative,
            Some(generative_link()),
        )];
        save_sidecar(&path, &document).unwrap();
        let loaded = load_sidecar(&path).unwrap();
        assert_eq!(
            loaded.virtual_copies[0].recipe.spot_removals,
            document.virtual_copies[0].recipe.spot_removals
        );
        // SPOT-SCHEMA-GEOMETRY: the load carries the extras mirror of the
        // typed key, so full-document equality no longer holds by design —
        // assert the mirror instead (same raw value the typed view parsed).
        assert_eq!(
            loaded.virtual_copies[0].recipe.extras.get("spot_removals"),
            Some(&serde_json::to_value(&document.virtual_copies[0].recipe.spot_removals).unwrap())
        );
        // No partial atomic-write temporary may linger.
        for entry in std::fs::read_dir(directory.path()).unwrap() {
            let name = entry.unwrap().file_name();
            assert!(
                !name.to_string_lossy().starts_with(".image.lumina.json.tmp"),
                "orphaned temporary: {name:?}"
            );
        }
    }

    #[test]
    fn generative_link_status_delegates_to_artifact_status() {
        // Missing: nothing on disk.
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            generative_link().artifact_status(directory.path()),
            ArtifactStatus::Missing
        );
        // Corrupt: undersized file can never be a bundle.
        std::fs::write(directory.path().join("IMG_0001.ARW.lumina.zdata"), b"short").unwrap();
        assert_eq!(
            generative_link().artifact_status(directory.path()),
            ArtifactStatus::Corrupt
        );
        // Corrupt: zdata-declared format without container magic is mislabeled.
        std::fs::write(
            directory.path().join("IMG_0001.ARW.lumina.zdata"),
            b"definitely not a container, long enough",
        )
        .unwrap();
        assert_eq!(
            generative_link().artifact_status(directory.path()),
            ArtifactStatus::Corrupt
        );
        // Available (structural): opaque non-container payloads pass checks
        // 1-2; deep checksum verification of real bundles is covered by the
        // zdata-gated end-to-end test below.
        let opaque = ArtifactReference {
            relative_path: "payload.bin".into(),
            format: "opaque".into(),
            checksum: "c".into(),
            width: 1,
            height: 1,
            channels: "rgba8".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        };
        std::fs::write(directory.path().join("payload.bin"), b"12345678").unwrap();
        assert_eq!(
            artifact_status(directory.path(), &opaque),
            ArtifactStatus::Available
        );
    }

    // End-to-end bundle linkage needs the codec.
    #[cfg(feature = "zdata")]
    #[test]
    fn generative_links_resolve_against_real_bundle_eager() {
        use crate::{GenerativeCanvasArtifact, SpotHealGenerativeArtifact};
        let directory = tempfile::tempdir().unwrap();
        let bundle = directory.path().join("IMG_0001.ARW.lumina.zdata");
        let canvas = GenerativeCanvasArtifact {
            id: "gen-canvas-1".into(),
            width: 2,
            height: 2,
            pixels: vec![
                10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 1, 2, 3, 255,
            ],
        };
        let spot = SpotHealGenerativeArtifact {
            id: "spot-1".into(),
            width: 1,
            height: 1,
            pixels: vec![9, 9, 9, 255],
        };
        let container = ZDataContainer::new(vec![]).unwrap();
        let container = container.add_generative_canvas(canvas.clone()).unwrap();
        let container = container.add_spot_heal_generative(spot.clone()).unwrap();
        save_zdata(&bundle, &container).unwrap();

        let canvas_link = GenerativeArtifactRef {
            id: canvas.id.clone(),
            relative_path: "IMG_0001.ARW.lumina.zdata".into(),
            format: "lumina-zdata".into(),
            checksum: canvas.checksum(),
            width: canvas.width,
            height: canvas.height,
            channels: "rgba8".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        };
        let spot_link = GenerativeArtifactRef {
            id: spot.id.clone(),
            relative_path: "IMG_0001.ARW.lumina.zdata".into(),
            format: "lumina-zdata".into(),
            checksum: spot.checksum(),
            width: spot.width,
            height: spot.height,
            channels: "rgba8".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        };
        // Recipe carrying both links validates.
        let mut document = SidecarDocument::new(source(), "pipeline-1");
        let mut edit = generative_edit_with_link();
        edit.artifact = Some(canvas_link.clone());
        document.virtual_copies[0].recipe.generative_edit = Some(edit);
        document.virtual_copies[0].recipe.spot_removals = vec![spot_removal(
            SpotRemovalMode::Generative,
            Some(spot_link.clone()),
        )];
        document.validate().unwrap();
        // Eager status: intact bundle is Available for both links.
        assert_eq!(
            canvas_link.artifact_status(directory.path()),
            ArtifactStatus::Available
        );
        assert_eq!(
            spot_link.artifact_status(directory.path()),
            ArtifactStatus::Available
        );
        // Kind separation is strict: neither id resolves under the other kind.
        let loaded = load_zdata(&bundle).unwrap();
        assert!(loaded.spot_heal_generative(&canvas.id).is_err());
        assert!(loaded.generative_canvas(&spot.id).is_err());
        // Bitflip => eager Corrupt (never Available).
        let mut bytes = std::fs::read(&bundle).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 1;
        std::fs::write(&bundle, &bytes).unwrap();
        assert_eq!(
            canvas_link.artifact_status(directory.path()),
            ArtifactStatus::Corrupt
        );
        // Deleted bundle => Missing.
        std::fs::remove_file(&bundle).unwrap();
        assert_eq!(
            spot_link.artifact_status(directory.path()),
            ArtifactStatus::Missing
        );
    }

    // GEN-EXPAND-CACHE-1: the generative identity is additive in `extras` and
    // must roundtrip without an explicit schema field.
    #[test]
    fn generative_identity_roundtrips_through_json() {
        let link = GenerativeArtifactRef {
            id: "gen-canvas-1".into(),
            relative_path: "IMG_0001.ARW.lumina.zdata".into(),
            format: "lumina-zdata".into(),
            checksum: "blake3:abc".into(),
            width: 2,
            height: 2,
            channels: "rgba8".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        };
        // A link written before identity pinning carries none.
        assert!(link.identity().is_none());
        let legacy: GenerativeArtifactRef =
            serde_json::from_str(&serde_json::to_string(&link).unwrap()).unwrap();
        assert!(legacy.identity().is_none(), "no identity is invented");
        assert_eq!(legacy, link);

        // A pinned link roundtrips the identity verbatim.
        let pinned = link.clone().with_identity("gen:key-1");
        assert_eq!(pinned.identity(), Some("gen:key-1"));
        let back: GenerativeArtifactRef =
            serde_json::from_str(&serde_json::to_string(&pinned).unwrap()).unwrap();
        assert_eq!(back, pinned);
        assert_eq!(back.identity(), Some("gen:key-1"));
    }

    // GEN-EXPAND-CACHE-1: a persisted canvas may only be served when its pinned
    // identity still matches the current generative identity; otherwise it is
    // `Stale` (loud), never silently used.
    #[cfg(feature = "zdata")]
    #[test]
    fn generative_artifact_status_is_identity_verified() {
        use crate::GenerativeCanvasArtifact;
        let directory = tempfile::tempdir().unwrap();
        let bundle = directory.path().join("IMG_0001.ARW.lumina.zdata");
        let canvas = GenerativeCanvasArtifact {
            id: "gen-canvas-1".into(),
            width: 2,
            height: 2,
            pixels: vec![
                10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 1, 2, 3, 255,
            ],
        };
        let container = ZDataContainer::new(vec![])
            .unwrap()
            .add_generative_canvas(canvas.clone())
            .unwrap();
        save_zdata(&bundle, &container).unwrap();
        let link = GenerativeArtifactRef {
            id: canvas.id.clone(),
            relative_path: "IMG_0001.ARW.lumina.zdata".into(),
            format: "lumina-zdata".into(),
            checksum: canvas.checksum(),
            width: canvas.width,
            height: canvas.height,
            channels: "rgba8".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        }
        .with_identity("identity-A");

        // Matching identity + intact bundle = Available.
        assert_eq!(
            generative_artifact_status(directory.path(), &link, "identity-A"),
            GenerativeArtifactStatus::Available
        );
        // Recipe/seed/canvas changed => different identity => Stale.
        assert_eq!(
            generative_artifact_status(directory.path(), &link, "identity-B"),
            GenerativeArtifactStatus::Stale
        );
        // Legacy link without a pinned identity can never be proven current.
        let legacy = {
            let mut l = link.clone();
            l.extras.remove(GENERATIVE_IDENTITY_KEY);
            l
        };
        assert_eq!(
            generative_artifact_status(directory.path(), &legacy, "identity-A"),
            GenerativeArtifactStatus::Stale
        );
        // Missing bundle stays Missing regardless of identity.
        std::fs::remove_file(&bundle).unwrap();
        assert_eq!(
            generative_artifact_status(directory.path(), &link, "identity-A"),
            GenerativeArtifactStatus::Missing
        );
        // Corrupt bundle stays Corrupt.
        std::fs::write(&bundle, b"definitely not zdata").unwrap();
        assert_eq!(
            generative_artifact_status(directory.path(), &link, "identity-A"),
            GenerativeArtifactStatus::Corrupt
        );
    }

    // Bundle moves keep relative links valid.
    #[cfg(feature = "zdata")]
    #[test]
    fn generative_links_survive_bundle_move() {
        use crate::GenerativeCanvasArtifact;
        let directory = tempfile::tempdir().unwrap();
        let from_dir = directory.path().join("a");
        std::fs::create_dir(&from_dir).unwrap();
        let bundle = from_dir.join("IMG_0001.ARW.lumina.zdata");
        let canvas = GenerativeCanvasArtifact {
            id: "gen-canvas-1".into(),
            width: 1,
            height: 1,
            pixels: vec![1, 2, 3, 255],
        };
        let container = ZDataContainer::new(vec![])
            .unwrap()
            .add_generative_canvas(canvas.clone())
            .unwrap();
        save_zdata(&bundle, &container).unwrap();
        let link = GenerativeArtifactRef {
            id: canvas.id.clone(),
            relative_path: "IMG_0001.ARW.lumina.zdata".into(),
            format: "lumina-zdata".into(),
            checksum: canvas.checksum(),
            width: 1,
            height: 1,
            channels: "rgba8".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        };
        assert_eq!(link.artifact_status(&from_dir), ArtifactStatus::Available);
        // Move the whole bundle directory; the relative link stays valid.
        let to_dir = directory.path().join("b");
        std::fs::rename(&from_dir, &to_dir).unwrap();
        assert_eq!(link.artifact_status(&to_dir), ArtifactStatus::Available);
    }

    // GEN-ONNX-1: explicit regeneration replaces only the `generative_canvas`
    // record; the default append path stays non-destructive.
    #[cfg(feature = "zdata")]
    #[test]
    fn generative_canvas_replace_is_explicit_and_kind_safe() {
        use crate::GenerativeCanvasArtifact;
        let directory = tempfile::tempdir().unwrap();
        let bundle = directory.path().join("IMG_0001.ARW.lumina.zdata");
        let canvas = |value: u8| GenerativeCanvasArtifact {
            id: "gen-1".into(),
            width: 1,
            height: 1,
            pixels: vec![value, 2, 3, 255],
        };
        save_generative_canvas(&bundle, canvas(10), false).unwrap();
        // The non-destructive append path rejects the duplicate id.
        assert!(
            save_generative_canvas(&bundle, canvas(11), false).is_err(),
            "append must not silently overwrite an existing record"
        );
        // The explicit replace path installs the new bytes.
        save_generative_canvas(&bundle, canvas(11), true).unwrap();
        let container = load_zdata(&bundle).unwrap();
        assert_eq!(container.generative_canvas("gen-1").unwrap().pixels[0], 11);

        // The recipe link built from the record verifies `Available` and
        // round-trips its identity.
        let link = GenerativeArtifactRef::from_generative_canvas(
            &canvas(11),
            "IMG_0001.ARW.lumina.zdata",
            "id-1",
        );
        assert_eq!(
            link.artifact_status(directory.path()),
            ArtifactStatus::Available
        );
        assert_eq!(link.identity(), Some("id-1"));
        assert_eq!(
            generative_artifact_status(directory.path(), &link, "id-1"),
            GenerativeArtifactStatus::Available
        );
        assert_eq!(
            generative_artifact_status(directory.path(), &link, "id-2"),
            GenerativeArtifactStatus::Stale
        );
    }

    // GEN-ONNX-1 Welle 2a: the additive `negative_prompt` field roundtrips as a
    // top-level JSON key and a mistyped value is rejected loudly.
    #[test]
    fn generative_negative_prompt_is_additive_and_validated() {
        let mut edit = GenerativeEdit {
            version: 1,
            canvas: None,
            artifact: None,
            keep_generative_content: None,
            auto_fill_transparent: None,
            expand_beyond_image: Some(true),
            seed: Some(7),
            prompt: Some("extend".into()),
            extras: Extras::new(),
        };
        assert_eq!(edit.negative_prompt(), None);
        assert!(edit.validate_edit_extras().is_ok());

        edit.set_negative_prompt(Some("blurry".into()));
        assert_eq!(edit.negative_prompt(), Some("blurry"));

        // JSON: additive top-level field, roundtrip-stable.
        let value = serde_json::to_value(&edit).unwrap();
        assert_eq!(value["negative_prompt"], serde_json::json!("blurry"));
        let back: GenerativeEdit = serde_json::from_value(value).unwrap();
        assert_eq!(back.negative_prompt(), Some("blurry"));

        // Clearing removes the key (absent, not implicitly empty).
        edit.set_negative_prompt(None);
        assert_eq!(edit.negative_prompt(), None);
        assert!(serde_json::to_value(&edit)
            .unwrap()
            .get("negative_prompt")
            .is_none());

        // A mistyped value is a hard error, never silently ignored.
        edit.extras
            .insert(GENERATIVE_NEGATIVE_PROMPT_KEY.into(), serde_json::json!(42));
        assert!(edit.validate_edit_extras().is_err());
        // `null` is accepted as identity.
        edit.extras.insert(
            GENERATIVE_NEGATIVE_PROMPT_KEY.into(),
            serde_json::Value::Null,
        );
        assert!(edit.validate_edit_extras().is_ok());
        assert_eq!(edit.negative_prompt(), None);
    }

    // =====================================================================
    // F-077: Backup / Recovery / Conflict / Data-loss release-gate tests.
    //
    // These exercise the *failure semantics* of the committed persistence
    // machinery (atomic writes, `.bak` snapshots, the explicit migration path,
    // the version/validation guards and the revision hash) so that a regression
    // in any of them fails the release gate instead of shipping silently.
    // =====================================================================

    // ----- A) Atomic-write recovery -----

    #[test]
    fn sidecar_write_is_atomic_against_partial_temp_file() {
        // Simulate a crash mid-write: an orphaned, partial atomic-write
        // temporary left behind in the sidecar directory.
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = SidecarDocument::new(source(), "pipeline-1");
        save_sidecar(&path, &document).unwrap();
        let temp = directory.path().join(".image.lumina.json.tmp-crash");
        std::fs::write(&temp, b"{\"schema_version\": 2, \"partial\": ").unwrap();
        backdate(&temp, TEMP_SWEEP_AGE + Duration::from_secs(1));
        assert!(temp.exists());
        // The original sidecar must remain intact and readable; load_sidecar
        // recovers it and sweeps the orphaned temporary.
        assert_eq!(load_sidecar(&path).unwrap(), document);
        assert!(!temp.exists());
    }

    #[cfg(feature = "zdata")]
    #[test]
    fn zdata_write_is_atomic_against_partial_temp_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = zdata_path_for(&directory.path().join("image.raw"));
        let container = ZDataContainer::new(f077_tiles()).unwrap();
        save_zdata(&path, &container).unwrap();
        // A crash mid-write leaves a partial temporary with the zdata prefix.
        let temp = directory.path().join(".image.raw.lumina.zdata.tmp-crash");
        std::fs::write(&temp, vec![0u8; 50]).unwrap();
        // The live container is untouched by the orphan and still loads exactly.
        let loaded = load_zdata(&path).unwrap();
        assert_eq!(loaded.tile("subject", 0, 0).unwrap(), f077_tiles()[0]);
    }

    #[test]
    fn migration_creates_bak_before_overwrite_and_bak_is_recoverable() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = SidecarDocument::new(source(), "pipeline-1");
        let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
        value["schema_version"] = Value::from(0);
        let original_bytes = serde_json::to_vec(&value).unwrap();
        std::fs::write(&path, &original_bytes).unwrap();

        assert!(migrate_sidecar_file(&path).unwrap());
        let bak = path.with_file_name("image.lumina.json.bak");
        assert!(bak.is_file());
        // The snapshot is the PRE-migration content, verbatim.
        assert_eq!(std::fs::read(&bak).unwrap(), original_bytes);
        // The live target is now migrated to the current schema.
        assert_eq!(load_sidecar(&path).unwrap().schema_version, SCHEMA_VERSION);
        // REVIEW-SIDECAR-N2: the loader rejects historical v0 documents loudly
        // instead of silently normalizing them.
        let error = load_sidecar(&bak).unwrap_err();
        assert!(
            error.to_string().contains("schema_version 0"),
            "v0 backup must be rejected loudly, got {error}"
        );
        // Recovery from the snapshot goes through the explicit migration
        // path and reproduces exactly the original (pre-migration) content.
        let migrated = migrate_json(&String::from_utf8(original_bytes).unwrap()).unwrap();
        let recovered = SidecarDocument::from_json(&migrated).unwrap();
        assert_eq!(recovered.virtual_copies, document.virtual_copies);
        assert_eq!(recovered.schema_version, SCHEMA_VERSION);
    }

    // ----- B) Backup / restore -----

    #[test]
    fn save_sidecar_overwrites_in_place_without_bak() {
        // The released contract: `save_sidecar` is an atomic in-place replace
        // and does NOT leave a `.bak`; the explicit migration path
        // (`migrate_sidecar_file`) is the only backup-bearing operation. This
        // test pins that behavior so a future "silent backup on every save"
        // change is visible.
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let mut document = SidecarDocument::new(source(), "pipeline-1");
        document.virtual_copies[0].name = "First".into();
        save_sidecar(&path, &document).unwrap();
        let bak = path.with_file_name("image.lumina.json.bak");
        assert!(!bak.exists(), "save_sidecar must not create a .bak");
        document.virtual_copies[0].name = "Second".into();
        save_sidecar(&path, &document).unwrap();
        assert!(!bak.exists());
        assert_eq!(
            load_sidecar(&path).unwrap().virtual_copies[0].name,
            "Second"
        );
    }

    #[test]
    fn bak_can_restore_previous_valid_state_after_corrupt_write() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = SidecarDocument::new(source(), "pipeline-1");
        let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
        value["schema_version"] = Value::from(0);
        let original_json = serde_json::to_string(&value).unwrap();
        std::fs::write(&path, &original_json).unwrap();
        migrate_sidecar_file(&path).unwrap();
        let bak = path.with_file_name("image.lumina.json.bak");
        assert!(bak.is_file());
        // The live target is subsequently corrupted (e.g. a failed later write).
        std::fs::write(&path, b"{\"schema_version\": 2, \"corrupt\": true").unwrap();
        assert!(load_sidecar(&path).is_err());
        // REVIEW-SIDECAR-N2: the raw v0 snapshot is not silently accepted by
        // the loader; the previous valid state is recovered by explicitly
        // migrating the snapshot's bytes.
        assert!(load_sidecar(&bak).is_err());
        let migrated = migrate_json(&std::fs::read_to_string(&bak).unwrap()).unwrap();
        let recovered = SidecarDocument::from_json(&migrated).unwrap();
        let expected = {
            let mut expected_value: Value = serde_json::from_str(&original_json).unwrap();
            expected_value["schema_version"] = Value::from(SCHEMA_VERSION);
            SidecarDocument::from_json(&serde_json::to_string(&expected_value).unwrap()).unwrap()
        };
        assert_eq!(recovered, expected);
    }

    #[test]
    fn bak_is_written_atomically_not_partial() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = SidecarDocument::new(source(), "pipeline-1");
        let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
        value["schema_version"] = Value::from(0);
        let original_bytes = serde_json::to_vec(&value).unwrap();
        std::fs::write(&path, &original_bytes).unwrap();
        migrate_sidecar_file(&path).unwrap();
        let bak = path.with_file_name("image.lumina.json.bak");
        // The snapshot is byte-exact (never a truncated copy of the original).
        let bak_bytes = std::fs::read(&bak).unwrap();
        assert_eq!(bak_bytes, original_bytes);
        // REVIEW-SIDECAR-N2: it is fully valid, parseable JSON — not a partial
        // file. The raw v0 snapshot is rejected by the loader (loud failure)
        // but parses and migrates cleanly through the explicit path.
        assert!(
            serde_json::from_str::<Value>(&String::from_utf8(bak_bytes.clone()).unwrap()).is_ok()
        );
        assert!(SidecarDocument::from_json(&String::from_utf8(bak_bytes).unwrap()).is_err());
        assert!(migrate_json(&String::from_utf8(original_bytes).unwrap()).is_ok());
        // No partial `.bak` temporary should linger after the atomic write.
        for entry in std::fs::read_dir(directory.path()).unwrap() {
            let name = entry.unwrap().file_name();
            assert!(
                !name
                    .to_string_lossy()
                    .starts_with(".image.lumina.json.bak.tmp"),
                "orphaned .bak temporary: {name:?}"
            );
        }
    }

    // ----- C) Conflict scenarios -----

    #[test]
    fn concurrent_plain_saves_do_not_corrupt_sidecar() {
        // Two plain `save_sidecar` calls race on the same target. Each writes a
        // private temporary and renames it over the target, so the final file is
        // always one complete serialization — never an interleaved/corrupt one.
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let base = SidecarDocument::new(source(), "pipeline-1");
        let mut first = base.clone();
        first.virtual_copies[0].name = "First".into();
        let mut second = base;
        second.virtual_copies[0].name = "Second".into();
        let handles: Vec<_> = [first.clone(), second.clone()]
            .into_iter()
            .map(|doc| {
                let path = path.clone();
                std::thread::spawn(move || save_sidecar(&path, &doc).unwrap())
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let loaded = load_sidecar(&path).unwrap();
        assert!(loaded == first || loaded == second);
    }

    #[test]
    fn newer_schema_version_is_rejected_not_silently_downgraded() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = SidecarDocument::new(source(), "pipeline-1");
        let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
        value["schema_version"] = Value::from(99);
        let bytes = serde_json::to_vec(&value).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        // Reading explicitly rejects the unsupported version.
        assert_eq!(
            load_sidecar(&path).unwrap_err(),
            SidecarError::Invalid(
                "unsupported schema_version 99; explicit migration is required".into()
            )
        );
        // migrate_json also rejects rather than silently downgrading.
        assert_eq!(
            migrate_json(&String::from_utf8(bytes.clone()).unwrap()).unwrap_err(),
            SidecarError::Invalid(
                "unsupported schema_version 99; explicit migration is required".into()
            )
        );
        // A failed migration must not touch the original file at all.
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn invalid_sidecar_is_rejected_not_silently_accepted() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        // Truncated JSON.
        std::fs::write(&path, r#"{"format":"lumina-sidecar","schema_version":2,"#).unwrap();
        assert!(matches!(load_sidecar(&path), Err(SidecarError::Json(_))));
        // Valid JSON but missing the required `virtual_copies`.
        std::fs::write(
            &path,
            r#"{"format":"lumina-sidecar","schema_version":2,"source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"RAW","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","virtual_copies":[],"presets":[]}"#,
        )
        .unwrap();
        assert!(matches!(load_sidecar(&path), Err(SidecarError::Invalid(_))));
        // Missing `schema_version` entirely.
        std::fs::write(
            &path,
            r#"{"format":"lumina-sidecar","source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"RAW","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{"adjustments":{},"options":{}},"mask_library":[],"mask_layers":[],"history":[],"export_records":[]}],"presets":[]}"#,
        )
        .unwrap();
        assert!(matches!(load_sidecar(&path), Err(SidecarError::Invalid(_))));
        // Wrong `format` string.
        let wrong_format = r#"{"format":"other","source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"RAW","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{"adjustments":{},"options":{}},"mask_library":[],"mask_layers":[],"history":[],"export_records":[]}],"presets":[]}"#;
        std::fs::write(&path, wrong_format).unwrap();
        assert!(matches!(load_sidecar(&path), Err(SidecarError::Invalid(_))));
    }

    // ----- D) Data-loss prevention -----

    #[test]
    fn deleting_sidecar_does_not_affect_original_image() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("IMG_0001.ARW");
        let original_bytes = b"RAW-PIXEL-DATA-12345";
        std::fs::write(&source_path, original_bytes).unwrap();
        let path = sidecar_path_for(&source_path);
        save_sidecar(&path, &SidecarDocument::new(source(), "pipeline-1")).unwrap();
        assert!(path.exists());
        // Delete ONLY the sidecar.
        std::fs::remove_file(&path).unwrap();
        assert!(!path.exists());
        // The original image is byte-identical; persistence is non-destructive.
        assert_eq!(std::fs::read(&source_path).unwrap(), original_bytes);
    }

    #[cfg(feature = "zdata")]
    #[test]
    fn zdata_bundle_can_be_deleted_without_affecting_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("IMG_0001.ARW");
        std::fs::write(&source_path, b"RAW").unwrap();
        let sidecar_path = sidecar_path_for(&source_path);
        let zdata_path = zdata_path_for(&source_path);
        let document = SidecarDocument::new(source(), "pipeline-1");
        save_sidecar(&sidecar_path, &document).unwrap();
        let container = ZDataContainer::new(f077_tiles()).unwrap();
        save_zdata(&zdata_path, &container).unwrap();
        assert!(zdata_path.exists());
        let before = std::fs::read(&sidecar_path).unwrap();
        // Delete ONLY the zdata bundle (mask tile data).
        std::fs::remove_file(&zdata_path).unwrap();
        assert!(!zdata_path.exists());
        // Sidecar metadata is unchanged and still fully readable.
        assert_eq!(std::fs::read(&sidecar_path).unwrap(), before);
        assert!(load_sidecar(&sidecar_path).is_ok());
    }

    #[cfg(feature = "zdata")]
    #[test]
    fn partial_zdata_is_detected_not_silent_garbage() {
        let directory = tempfile::tempdir().unwrap();
        let path = zdata_path_for(&directory.path().join("image.raw"));
        let container = ZDataContainer::new(f077_tiles()).unwrap();
        save_zdata(&path, &container).unwrap();
        let full = std::fs::read(&path).unwrap();
        // Truncate the container body so the index/records are incomplete.
        let truncated = &full[..full.len() / 2];
        std::fs::write(&path, truncated).unwrap();
        // A truncated container must be reported, never silently returned as a
        // "valid" container that yields garbage tiles.
        assert!(load_zdata(&path).is_err());
    }

    // ----- E) Migration safety -----

    #[test]
    fn migrate_sidecar_file_creates_backup_before_migrating() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = SidecarDocument::new(source(), "pipeline-1");
        let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
        value["schema_version"] = Value::from(0);
        let original_bytes = serde_json::to_vec(&value).unwrap();
        std::fs::write(&path, &original_bytes).unwrap();
        assert!(migrate_sidecar_file(&path).unwrap());
        let bak = path.with_file_name("image.lumina.json.bak");
        assert!(bak.is_file());
        // The backup is the pre-migration original, verbatim.
        assert_eq!(std::fs::read(&bak).unwrap(), original_bytes);
        // The live target is now migrated and differs from the backup.
        let live = std::fs::read(&path).unwrap();
        assert_ne!(live, original_bytes);
        assert_eq!(load_sidecar(&path).unwrap().schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn failed_migration_leaves_original_intact() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = SidecarDocument::new(source(), "pipeline-1");
        let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
        value["schema_version"] = Value::from(99);
        let bytes = serde_json::to_vec(&value).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        // An unsupported version fails the migration...
        assert!(migrate_sidecar_file(&path).is_err());
        // ...without creating a `.bak` or touching the original file.
        assert!(!path.with_file_name("image.lumina.json.bak").exists());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn migrating_already_current_version_is_noop() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = SidecarDocument::new(source(), "pipeline-1");
        let bytes = document.to_json().unwrap();
        std::fs::write(&path, &bytes).unwrap();
        // Already at the current schema: no migration, no writes.
        assert!(!migrate_sidecar_file(&path).unwrap());
        // No backup is created for a no-op.
        assert!(!path.with_file_name("image.lumina.json.bak").exists());
        // And the on-disk content is unchanged.
        assert_eq!(std::fs::read(&path).unwrap(), bytes.as_bytes());
    }

    // ----- F) Recipe integrity under corruption -----

    #[test]
    fn corrupted_recipe_json_is_rejected_by_validate() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        // The recipe object is truncated mid-JSON.
        std::fs::write(
            &path,
            r#"{"format":"lumina-sidecar","schema_version":2,"source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"RAW","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{"adjustments":{"exposure":},"options":{}},"mask_library":[],"mask_layers":[],"history":[],"export_records":[]}],"presets":[]}"#,
        )
        .unwrap();
        assert!(matches!(load_sidecar(&path), Err(SidecarError::Json(_))));
    }

    #[test]
    fn recipe_with_out_of_range_adjustment_is_rejected() {
        // A recipe carrying a value outside its validated range must be rejected
        // by `validate()`, and therefore cannot be serialized into a valid
        // sidecar either.
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0]
            .recipe
            .adjustments
            .insert("exposure".into(), 99.0); // outside [-10, 10]
        assert!(matches!(d.validate(), Err(SidecarError::Invalid(_))));
        assert!(d.to_json().is_err());
    }

    #[test]
    fn truncated_recipe_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let valid = SidecarDocument::new(source(), "pipeline-1")
            .to_json()
            .unwrap();
        // Chop the tail so the recipe / virtual_copies section is incomplete.
        let truncated = &valid.as_bytes()[..valid.len() - 30];
        std::fs::write(&path, truncated).unwrap();
        assert!(load_sidecar(&path).is_err());
    }

    #[test]
    fn document_revision_detects_tampered_recipe() {
        // The sidecar-level integrity hash (`document_revision`, a BLAKE3 over
        // the serialized document) is the gate used by the compare-and-swap
        // write. Tampering with the recipe must change the hash, while identical
        // documents must hash identically and stably.
        let mut a = SidecarDocument::new(source(), "pipeline-1");
        a.virtual_copies[0]
            .recipe
            .adjustments
            .insert("exposure".into(), 0.5);
        let mut b = a.clone();
        b.virtual_copies[0]
            .recipe
            .adjustments
            .insert("exposure".into(), -0.5);
        let ra = document_revision(&a).unwrap();
        let rb = document_revision(&b).unwrap();
        assert_ne!(
            ra, rb,
            "tampering with the recipe must change its revision hash"
        );
        assert_eq!(ra, document_revision(&a).unwrap());
        assert_eq!(rb, document_revision(&b).unwrap());
    }

    // ----- F-077: zdata-gated helpers -----

    #[cfg(feature = "zdata")]
    fn f077_tiles() -> Vec<MaskTile> {
        vec![MaskTile {
            mask_id: "subject".into(),
            tile_x: 0,
            tile_y: 0,
            width: 2,
            height: 2,
            values: vec![0, 1, 32768, 65535],
        }]
    }

    // =====================================================================
    // F-079: prompt-capable mask sources in the mask DAG data model.
    // =====================================================================

    #[test]
    fn mask_prompt_variants_serde_roundtrip() {
        let cases: Vec<MaskPrompt> = vec![
            MaskPrompt::Box {
                rect: NormalizedRect {
                    x: 0.1,
                    y: 0.2,
                    width: 0.5,
                    height: 0.6,
                },
                transformation: PromptTransform::default(),
            },
            MaskPrompt::Brush {
                marks: vec![BrushMark {
                    x: 0.5,
                    y: 0.5,
                    radius: 0.2,
                    sign: BrushMarkSign::Positive,
                }],
                resolution: (64, 64),
                transformation: PromptTransform {
                    method: "brush-to-points".into(),
                    parameters: BTreeMap::from([("include_negatives".into(), "true".into())]),
                },
            },
            MaskPrompt::Polygon {
                points: vec![
                    Point2 { x: 0.0, y: 0.0 },
                    Point2 { x: 1.0, y: 0.0 },
                    Point2 { x: 0.5, y: 1.0 },
                ],
                transformation: PromptTransform::default(),
            },
            MaskPrompt::Ellipse {
                center: Point2 { x: 0.5, y: 0.5 },
                radii: Point2 { x: 0.3, y: 0.4 },
                transformation: PromptTransform::default(),
            },
            MaskPrompt::Gradient {
                angle_deg: 45.0,
                start: 0.0,
                end: 1.0,
                transformation: PromptTransform::default(),
            },
        ];
        for original in &cases {
            let json = serde_json::to_string(original).unwrap();
            let decoded: MaskPrompt = serde_json::from_str(&json).unwrap();
            assert_eq!(original, &decoded, "prompt roundtrip failed for {json}");
        }
    }

    #[test]
    fn mask_definition_with_prompt_roundtrips_through_document() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        let mut prompt_mask = mask("prompta");
        prompt_mask.prompt = Some(MaskPrompt::Box {
            rect: NormalizedRect {
                x: 0.25,
                y: 0.25,
                width: 0.5,
                height: 0.5,
            },
            transformation: PromptTransform {
                method: "normalize".into(),
                parameters: BTreeMap::from([("scale".into(), "1".into())]),
            },
        });
        d.virtual_copies[0].mask_library.push(prompt_mask);
        let json = d.to_json().unwrap();
        assert!(json.contains("prompt"));
        assert!(json.contains("\"box\""));
        // The prompt is stored as part of the mask identity (next to the node).
        let decoded = SidecarDocument::from_json(&json).unwrap();
        assert_eq!(decoded, d);
    }

    #[test]
    fn mask_prompt_out_of_range_is_rejected() {
        // Box with a coordinate outside [0,1].
        let mut d = SidecarDocument::new(source(), "p");
        let mut m = mask("bad");
        m.prompt = Some(MaskPrompt::Box {
            rect: NormalizedRect {
                x: -0.1,
                y: 0.0,
                width: 0.5,
                height: 0.5,
            },
            transformation: PromptTransform::default(),
        });
        d.virtual_copies[0].mask_library.push(m);
        assert!(d.validate().is_err());

        // Box with a zero width.
        let mut d2 = SidecarDocument::new(source(), "p");
        let mut m2 = mask("bad2");
        m2.prompt = Some(MaskPrompt::Box {
            rect: NormalizedRect {
                x: 0.1,
                y: 0.1,
                width: 0.0,
                height: 0.5,
            },
            transformation: PromptTransform::default(),
        });
        d2.virtual_copies[0].mask_library.push(m2);
        assert!(d2.validate().is_err());

        // Polygon with an out-of-range point.
        let mut d3 = SidecarDocument::new(source(), "p");
        let mut m3 = mask("bad3");
        m3.prompt = Some(MaskPrompt::Polygon {
            points: vec![Point2 { x: 0.0, y: 0.0 }, Point2 { x: 2.0, y: 0.5 }],
            transformation: PromptTransform::default(),
        });
        d3.virtual_copies[0].mask_library.push(m3);
        assert!(d3.validate().is_err());

        // Gradient with a finite-but-out-of-range end value.
        let mut d4 = SidecarDocument::new(source(), "p");
        let mut m4 = mask("bad4");
        m4.prompt = Some(MaskPrompt::Gradient {
            angle_deg: 0.0,
            start: 0.0,
            end: 1.5,
            transformation: PromptTransform::default(),
        });
        d4.virtual_copies[0].mask_library.push(m4);
        assert!(d4.validate().is_err());

        // Brush with empty marks is rejected.
        let mut d5 = SidecarDocument::new(source(), "p");
        let mut m5 = mask("bad5");
        m5.prompt = Some(MaskPrompt::Brush {
            marks: vec![],
            resolution: (512, 512),
            transformation: PromptTransform::default(),
        });
        d5.virtual_copies[0].mask_library.push(m5);
        assert!(d5.validate().is_err());

        // Brush with a non-positive radius is rejected.
        let mut d6 = SidecarDocument::new(source(), "p");
        let mut m6 = mask("bad6");
        m6.prompt = Some(MaskPrompt::Brush {
            marks: vec![BrushMark {
                x: 0.5,
                y: 0.5,
                radius: 0.0,
                sign: BrushMarkSign::Positive,
            }],
            resolution: (512, 512),
            transformation: PromptTransform::default(),
        });
        d6.virtual_copies[0].mask_library.push(m6);
        assert!(d6.validate().is_err());

        // Non-finite (NaN) coordinate is rejected.
        let mut d7 = SidecarDocument::new(source(), "p");
        let mut m7 = mask("bad7");
        m7.prompt = Some(MaskPrompt::Box {
            rect: NormalizedRect {
                x: f32::NAN,
                y: 0.0,
                width: 0.5,
                height: 0.5,
            },
            transformation: PromptTransform::default(),
        });
        d7.virtual_copies[0].mask_library.push(m7);
        assert!(d7.validate().is_err());

        // Valid prompt is accepted.
        let mut ok = SidecarDocument::new(source(), "p");
        let mut good = mask("good");
        good.prompt = Some(MaskPrompt::Ellipse {
            center: Point2 { x: 0.5, y: 0.5 },
            radii: Point2 { x: 0.3, y: 0.3 },
            transformation: PromptTransform::default(),
        });
        ok.virtual_copies[0].mask_library.push(good);
        assert!(ok.validate().is_ok());
    }

    // ----- G-03 Maskierungs-Parität: AiSelect, Range-Prompts, visible -----

    #[test]
    fn ai_select_kind_parse_roundtrip() {
        for kind in AiSelectKind::all() {
            assert_eq!(AiSelectKind::parse(kind.as_str()), Some(kind));
        }
        // Case-insensitive reads; unknown kinds stay unknown (no guessing).
        assert_eq!(AiSelectKind::parse("Subject"), Some(AiSelectKind::Subject));
        assert_eq!(AiSelectKind::parse("SKY"), Some(AiSelectKind::Sky));
        assert_eq!(AiSelectKind::parse("people"), Some(AiSelectKind::People));
        assert_eq!(AiSelectKind::parse("cat"), None);
        assert_eq!(AiSelectKind::parse(""), None);
    }

    #[test]
    fn ai_select_roundtrip_and_validation() {
        let mut d = SidecarDocument::new(source(), "p");
        let mut m = mask("ai");
        m.ai_select = Some(AiSelect {
            kind: AiSelectKind::People,
            detail: Some("pupil".into()),
            extras: BTreeMap::new(),
        });
        d.virtual_copies[0].mask_library.push(m);
        assert!(d.validate().is_ok());
        let json = d.to_json().unwrap();
        assert!(json.contains("ai_select"));
        assert!(json.contains("\"people\""));
        let decoded = SidecarDocument::from_json(&json).unwrap();
        assert_eq!(decoded, d);

        // Every documented part validates.
        for part in AI_SELECT_KNOWN_PARTS {
            let mut d = SidecarDocument::new(source(), "p");
            let mut m = mask("ai");
            m.ai_select = Some(AiSelect {
                kind: AiSelectKind::Subject,
                detail: Some((*part).into()),
                extras: BTreeMap::new(),
            });
            d.virtual_copies[0].mask_library.push(m);
            assert!(d.validate().is_ok(), "part `{part}` must validate");
        }

        // Untrimmed, empty, overlong and control-char details are rejected.
        for bad in [" face", "face ", "", "a".repeat(65).as_str(), "fa\tce"] {
            let mut d = SidecarDocument::new(source(), "p");
            let mut m = mask("bad");
            m.ai_select = Some(AiSelect {
                kind: AiSelectKind::Sky,
                detail: Some(bad.into()),
                extras: BTreeMap::new(),
            });
            d.virtual_copies[0].mask_library.push(m);
            assert!(d.validate().is_err(), "detail `{bad:?}` must be rejected");
        }
    }

    #[test]
    fn ai_select_on_derived_node_is_rejected() {
        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].mask_library.push(mask("a"));
        d.virtual_copies[0].mask_library.push(mask("b"));
        let mut derived = mask("combo");
        derived.operation = MaskOperation::Union;
        derived.references = vec![
            MaskReference {
                copy_id: "vc-original".into(),
                mask_id: "a".into(),
                extras: BTreeMap::new(),
            },
            MaskReference {
                copy_id: "vc-original".into(),
                mask_id: "b".into(),
                extras: BTreeMap::new(),
            },
        ];
        derived.ai_select = Some(AiSelect {
            kind: AiSelectKind::Subject,
            detail: None,
            extras: BTreeMap::new(),
        });
        d.virtual_copies[0].mask_library.push(derived);
        let error = d.validate().unwrap_err().to_string();
        assert!(error.contains("ai_select"), "unexpected error: {error}");
    }

    #[test]
    fn range_prompts_roundtrip_and_validation() {
        let mut d = SidecarDocument::new(source(), "p");
        let mut lum = mask("lum");
        lum.prompt = Some(MaskPrompt::LuminanceRange {
            min: 0.2,
            max: 0.8,
            feather: 0.5,
            transformation: PromptTransform::default(),
        });
        let mut col = mask("col");
        col.prompt = Some(MaskPrompt::ColorRange {
            hue_center: 120.0,
            hue_width: 60.0,
            sat_min: 0.1,
            sat_max: 0.9,
            lum_min: 0.0,
            lum_max: 1.0,
            feather: 0.25,
            transformation: PromptTransform::default(),
        });
        d.virtual_copies[0].mask_library.push(lum);
        d.virtual_copies[0].mask_library.push(col);
        assert!(d.validate().is_ok());
        let json = d.to_json().unwrap();
        assert!(json.contains("luminancerange") || json.contains("luminance_range"));
        let decoded = SidecarDocument::from_json(&json).unwrap();
        assert_eq!(decoded, d);

        // min > max is rejected.
        let mut bad = SidecarDocument::new(source(), "p");
        let mut m = mask("bad");
        m.prompt = Some(MaskPrompt::LuminanceRange {
            min: 0.9,
            max: 0.1,
            feather: 0.0,
            transformation: PromptTransform::default(),
        });
        bad.virtual_copies[0].mask_library.push(m);
        assert!(bad.validate().is_err());

        // hue out of degrees is rejected.
        let mut bad2 = SidecarDocument::new(source(), "p");
        let mut m2 = mask("bad2");
        m2.prompt = Some(MaskPrompt::ColorRange {
            hue_center: 400.0,
            hue_width: 60.0,
            sat_min: 0.0,
            sat_max: 1.0,
            lum_min: 0.0,
            lum_max: 1.0,
            feather: 0.0,
            transformation: PromptTransform::default(),
        });
        bad2.virtual_copies[0].mask_library.push(m2);
        assert!(bad2.validate().is_err());

        // A range prompt on a derived node is rejected.
        let mut bad3 = SidecarDocument::new(source(), "p");
        bad3.virtual_copies[0].mask_library.push(mask("a"));
        let mut inv = mask("inv");
        inv.operation = MaskOperation::Invert;
        inv.references = vec![MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "a".into(),
            extras: BTreeMap::new(),
        }];
        inv.prompt = Some(MaskPrompt::LuminanceRange {
            min: 0.0,
            max: 1.0,
            feather: 0.0,
            transformation: PromptTransform::default(),
        });
        bad3.virtual_copies[0].mask_library.push(inv);
        let error = bad3.validate().unwrap_err().to_string();
        assert!(error.contains("range prompt"), "unexpected error: {error}");
    }

    #[test]
    fn mask_layer_visible_defaults_true_and_roundtrips() {
        // Legacy JSON without `visible` reads as visible (identity).
        let json = serde_json::json!({
            "id": "layer",
            "mask": {"copy_id": "vc-original", "mask_id": "a"},
            "inverted": false,
            "feather": 0.0,
            "blur": 0.0,
            "density": 1.0
        });
        let layer: MaskLayer = serde_json::from_value(json).unwrap();
        assert!(layer.visible);

        // Explicit false survives a full document roundtrip.
        let mut d = SidecarDocument::new(source(), "p");
        d.virtual_copies[0].mask_library.push(mask("a"));
        d.virtual_copies[0].mask_layers.push(MaskLayer {
            id: "layer".into(),
            mask: MaskReference {
                copy_id: "vc-original".into(),
                mask_id: "a".into(),
                extras: BTreeMap::new(),
            },
            inverted: false,
            feather: 0.0,
            blur: 0.0,
            density: 1.0,
            visible: false,
            extras: BTreeMap::new(),
        });
        assert!(d.validate().is_ok());
        let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
        assert_eq!(decoded, d);
        assert!(!decoded.virtual_copies[0].mask_layers[0].visible);
    }

    // ----- F-103-N5: `paths_resolve_equal` non-destructive export guard -----

    #[test]
    fn paths_resolve_equal_same_file_is_true() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        std::fs::write(&source, b"data").unwrap();
        // Identical path resolves equal to itself.
        assert!(paths_resolve_equal(&source, &source).unwrap());
        // A second reference to the very same file also resolves equal.
        let same = directory.path().join("photo.png");
        assert!(paths_resolve_equal(&source, &same).unwrap());
    }

    #[test]
    fn paths_resolve_equal_different_files_is_false() {
        let directory = tempfile::tempdir().unwrap();
        let a = directory.path().join("a.png");
        let b = directory.path().join("b.png");
        std::fs::write(&a, b"data-a").unwrap();
        std::fs::write(&b, b"data-b").unwrap();
        assert!(!paths_resolve_equal(&a, &b).unwrap());
    }

    #[test]
    fn paths_resolve_equal_missing_output_same_name_is_false() {
        // A not-yet-existing export target that shares the source's file name but
        // lives in a *different* directory resolves to a different location, so the
        // non-destructive guard must NOT reject it (it is a legitimate export).
        //
        // This genuinely exercises the `!output.exists()` (missing) branch of
        // `paths_resolve_equal`: `output` does not exist, so it is resolved against
        // its parent directory and compared to the source's canonical path.
        //
        // Note: a truly *missing* output whose resolved path equals the source is
        // impossible on a normal filesystem — if the resolved path named an existing
        // file (the source) the `output` would `exists()` and take the other branch.
        // The overwrite-rejection for the source's own name is therefore covered by
        // `paths_resolve_equal_same_file_is_true` (the `exists()` branch), which is
        // the realistic non-destructive contract: the original still occupies that
        // path when an export targets it.
        let directory = tempfile::tempdir().unwrap();
        let source_dir = directory.path().join("src");
        std::fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("photo.png");
        std::fs::write(&source, b"data").unwrap();
        // Output shares the file name but sits in the parent folder and does not
        // exist yet — the missing branch is taken.
        let missing = directory.path().join("photo.png");
        assert!(!missing.exists());
        assert!(!paths_resolve_equal(&source, &missing).unwrap());
    }

    #[test]
    fn paths_resolve_equal_missing_output_other_name_is_false() {
        // A not-yet-existing output with a different name in the same folder is
        // a legitimate export target.
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("photo.png");
        std::fs::write(&source, b"data").unwrap();
        let missing = directory.path().join("photo_export.png");
        assert!(!paths_resolve_equal(&source, &missing).unwrap());
    }

    #[test]
    fn paths_resolve_equal_different_directories_is_false() {
        let parent = tempfile::tempdir().unwrap();
        let d1 = parent.path().join("d1");
        let d2 = parent.path().join("d2");
        std::fs::create_dir_all(&d1).unwrap();
        std::fs::create_dir_all(&d2).unwrap();
        let source = d1.join("photo.png");
        let other = d2.join("photo.png");
        std::fs::write(&source, b"data").unwrap();
        // Even with the same file name, different directories never resolve equal.
        assert!(!paths_resolve_equal(&source, &other).unwrap());
    }

    // ----- REVIEW-SIDECAR-LOCK-1: atomic stale-lock reclaim (TOCTOU) -----

    #[test]
    fn stale_lock_reclaim_is_atomic_no_lost_update() {
        use std::sync::{Arc, Barrier};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = SidecarDocument::new(source(), "pipeline-1");
        let revision = save_sidecar_if_unchanged(&path, &document, None).unwrap();
        let lock_path = directory.path().join(".image.lumina.json.lock");
        assert!(!lock_path.exists(), "lock must be released after save");
        // Create a stale lock (mtime 60s ago) that both contenders will see.
        {
            let file = std::fs::File::create(&lock_path).unwrap();
            let old = SystemTime::now() - Duration::from_secs(60);
            file.set_modified(old).unwrap();
        }
        let barrier = Arc::new(Barrier::new(2));
        let path1 = path.clone();
        let rev1 = revision.clone();
        let b1 = Arc::clone(&barrier);
        let t1 = std::thread::spawn(move || {
            b1.wait();
            let mut edited = SidecarDocument::new(source(), "pipeline-1");
            edited.virtual_copies[0].name = "first".into();
            save_sidecar_if_unchanged(&path1, &edited, Some(&rev1))
        });
        let path2 = path.clone();
        let rev2 = revision;
        let b2 = Arc::clone(&barrier);
        let t2 = std::thread::spawn(move || {
            b2.wait();
            let mut edited = SidecarDocument::new(source(), "pipeline-1");
            edited.virtual_copies[0].name = "second".into();
            save_sidecar_if_unchanged(&path2, &edited, Some(&rev2))
        });
        let r1 = t1.join().unwrap();
        let r2 = t2.join().unwrap();
        let successes = [&r1, &r2].iter().filter(|r| r.is_ok()).count();
        let conflicts = [&r1, &r2]
            .iter()
            .filter(|r| matches!(r, Err(SidecarError::Conflict(_))))
            .count();
        assert_eq!(
            successes, 1,
            "exactly one contender must win the atomic stale reclaim, got {r1:?} {r2:?}"
        );
        assert_eq!(
            conflicts, 1,
            "the loser must receive an explicit Conflict, not a silent lost update"
        );
        // No stale reclaim artifact must remain.
        let reclaim_leftover = std::fs::read_dir(directory.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains("reclaim"));
        assert!(
            !reclaim_leftover,
            "no .reclaim temporary must survive after atomic reclaim"
        );
        // Sidecar must be one of the two valid outcomes, not corrupted.
        let loaded = load_sidecar(&path).unwrap();
        assert!(
            loaded.virtual_copies[0].name == "first" || loaded.virtual_copies[0].name == "second"
        );
        // Lock must be released after the winner's WriteLock is dropped.
        assert!(!lock_path.exists(), "lock must be cleaned up after winner");
        // No absolute path leaked.
        assert!(!loaded.source.relative_name.contains('/'));
    }

    #[test]
    fn fresh_lock_is_not_stolen_and_yields_explicit_conflict() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = SidecarDocument::new(source(), "pipeline-1");
        let revision = save_sidecar_if_unchanged(&path, &document, None).unwrap();
        let lock_path = directory.path().join(".image.lumina.json.lock");
        // Create a fresh lock (mtime = now) – must NOT be reclaimed.
        std::fs::File::create(&lock_path).unwrap();
        let mut edited = SidecarDocument::new(source(), "pipeline-1");
        edited.virtual_copies[0].name = "contender".into();
        let result = save_sidecar_if_unchanged(&path, &edited, Some(&revision));
        assert!(
            matches!(result, Err(SidecarError::Conflict(_))),
            "fresh lock must produce explicit Conflict, got {result:?}"
        );
        // Fresh lock must survive the failed reclaim attempt (not silently deleted).
        assert!(
            lock_path.exists(),
            "fresh lock must not have been deleted by contender"
        );
        // Winner's sidecar is unchanged.
        let loaded = load_sidecar(&path).unwrap();
        assert_eq!(loaded.virtual_copies[0].name, "Original");
        let _ = std::fs::remove_file(&lock_path);
    }

    #[test]
    fn concurrent_fresh_lock_only_one_writer_wins() {
        use std::sync::{Arc, Barrier};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = SidecarDocument::new(source(), "pipeline-1");
        let revision = save_sidecar_if_unchanged(&path, &document, None).unwrap();
        let lock_path = directory.path().join(".image.lumina.json.lock");
        // Hold a fresh lock in a background thread for ~300ms to simulate a
        // concurrent writer that has not yet released its lock.
        let holder_path = lock_path.clone();
        let holder = std::thread::spawn(move || {
            std::fs::File::create(&holder_path).unwrap();
            std::thread::sleep(Duration::from_millis(300));
            let _ = std::fs::remove_file(&holder_path);
        });
        // Give holder a head start so its fresh lock is visible.
        std::thread::sleep(Duration::from_millis(20));
        assert!(lock_path.exists());
        let barrier = Arc::new(Barrier::new(2));
        let p1 = path.clone();
        let r1 = revision.clone();
        let b1 = Arc::clone(&barrier);
        let t1 = std::thread::spawn(move || {
            b1.wait();
            let mut edited = SidecarDocument::new(source(), "pipeline-1");
            edited.virtual_copies[0].name = "contender-a".into();
            save_sidecar_if_unchanged(&p1, &edited, Some(&r1))
        });
        let p2 = path.clone();
        let r2 = revision;
        let b2 = Arc::clone(&barrier);
        let t2 = std::thread::spawn(move || {
            b2.wait();
            let mut edited = SidecarDocument::new(source(), "pipeline-1");
            edited.virtual_copies[0].name = "contender-b".into();
            save_sidecar_if_unchanged(&p2, &edited, Some(&r2))
        });
        let a = t1.join().unwrap();
        let b = t2.join().unwrap();
        holder.join().unwrap();
        // Both contenders raced against the holder's fresh lock; at least one
        // must have seen an explicit Conflict. Neither may have silently stolen
        // the fresh lock.
        let conflicts = [&a, &b]
            .iter()
            .filter(|r| matches!(r, Err(SidecarError::Conflict(_))))
            .count();
        assert!(
            conflicts >= 1,
            "at least one contender must get Conflict against fresh lock, got {a:?} {b:?}"
        );
        // If one contender won after holder released, the sidecar is valid; if
        // both lost, the original remains. No lost update: sidecar is never
        // corrupted or partially written.
        let loaded = load_sidecar(&path).unwrap();
        assert!(loaded.validate().is_ok());
    }

    // =====================================================================
    // REVIEW-SIDECAR batch: lock serialization, artifact verification,
    // migration temp prefixes, v0 rejection, range validation, mutation
    // rollback and bounded sidecar reads.
    // =====================================================================

    // ----- REVIEW-SIDECAR-CAS-1: plain saves serialize against CAS -----

    #[test]
    fn serialized_writes_reject_concurrent_lock_holder() {
        // A fresh lock means another writer is mid-save. Both a plain
        // `save_sidecar` and a compare-and-swap must report an explicit
        // Conflict instead of writing concurrently (previously only the CAS
        // path locked, so a plain save could silently overwrite an in-flight
        // compare-and-swap result).
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = SidecarDocument::new(source(), "pipeline-1");
        let revision = save_sidecar_if_unchanged(&path, &document, None).unwrap();
        let lock_path = directory.path().join(".image.lumina.json.lock");
        std::fs::File::create(&lock_path).unwrap();

        let mut edited = document.clone();
        edited.virtual_copies[0].name = "plain".into();
        let plain = save_sidecar(&path, &edited);
        assert!(
            matches!(&plain, Err(SidecarError::Conflict(message)) if message.contains("locked")),
            "plain save must not bypass the write lock, got {plain:?}"
        );

        let mut cas_edited = document.clone();
        cas_edited.virtual_copies[0].name = "cas".into();
        let cas = save_sidecar_if_unchanged(&path, &cas_edited, Some(&revision));
        assert!(
            matches!(cas, Err(SidecarError::Conflict(_))),
            "CAS must see the same lock, got {cas:?}"
        );

        // The holder's sidecar is untouched by both rejected writers.
        assert_eq!(
            load_sidecar(&path).unwrap().virtual_copies[0].name,
            "Original"
        );
        std::fs::remove_file(&lock_path).unwrap();
        // After the lock is released both writers work again.
        save_sidecar(&path, &edited).unwrap();
        assert_eq!(load_sidecar(&path).unwrap().virtual_copies[0].name, "plain");
    }

    #[test]
    fn mixed_plain_and_cas_writes_stay_serialized() {
        // Threads racing plain saves against a compare-and-swap must produce
        // exactly one complete document — never a torn or lost mixture.
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = SidecarDocument::new(source(), "pipeline-1");
        let revision = save_sidecar_if_unchanged(&path, &document, None).unwrap();
        let mut plain_doc = document.clone();
        plain_doc.virtual_copies[0].name = "plain-winner".into();
        let mut cas_doc = document.clone();
        cas_doc.virtual_copies[0].name = "cas-writer".into();

        let plain_path = path.clone();
        let plain_payload = plain_doc.clone();
        let plain_thread = std::thread::spawn(move || save_sidecar(&plain_path, &plain_payload));
        let cas_path = path.clone();
        let cas_revision = revision;
        let cas_payload = cas_doc.clone();
        let cas_thread = std::thread::spawn(move || {
            save_sidecar_if_unchanged(&cas_path, &cas_payload, Some(&cas_revision))
        });
        let plain_result = plain_thread.join().unwrap();
        let cas_result = cas_thread.join().unwrap();
        plain_result.unwrap();
        // The CAS either won before the plain save or lost with an explicit
        // Conflict; a silent lost update is forbidden.
        assert!(cas_result.is_ok() || matches!(cas_result, Err(SidecarError::Conflict(_))));
        let loaded = load_sidecar(&path).unwrap();
        assert!(loaded == plain_doc || loaded == cas_doc);
    }

    // ----- REVIEW-SIDECAR-STATUS-1: corrupt artifacts are visible -----

    #[cfg(feature = "zdata")]
    #[test]
    fn artifact_status_verifies_container_content_not_just_existence() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let artifact_dir = root.join("masks");
        std::fs::create_dir_all(&artifact_dir).unwrap();
        let reference = ArtifactReference {
            relative_path: "masks/subject.zdata".into(),
            format: "zdata".into(),
            checksum: "blake3:x".into(),
            width: 2,
            height: 2,
            channels: "u16".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        };
        // Missing: nothing on disk.
        assert_eq!(artifact_status(root, &reference), ArtifactStatus::Missing);
        // Corrupt: an empty file can never be a valid artifact.
        std::fs::write(root.join(&reference.relative_path), b"").unwrap();
        assert_eq!(artifact_status(root, &reference), ArtifactStatus::Corrupt);
        // Available: a fully valid container passes parse + checksum checks.
        let container = ZDataContainer::new(f077_tiles()).unwrap();
        save_zdata(&root.join(&reference.relative_path), &container).unwrap();
        assert_eq!(artifact_status(root, &reference), ArtifactStatus::Available);
        // Corrupt: a flipped payload byte previously counted as Available
        // because only `is_file()` was consulted.
        let bytes = std::fs::read(root.join(&reference.relative_path)).unwrap();
        let header_len = 40usize;
        let mut corrupted = bytes.clone();
        corrupted[header_len] ^= 0xff;
        std::fs::write(root.join(&reference.relative_path), &corrupted).unwrap();
        assert_eq!(
            artifact_status(root, &reference),
            ArtifactStatus::Corrupt,
            "a bit-flipped zdata payload must not count as available"
        );
        // Restore intact bytes, then flip the stored checksum itself.
        let index_offset = u64::from_le_bytes(bytes[16..24].try_into().unwrap()) as usize;
        let record_offset = u64::from_le_bytes(
            bytes[index_offset + 20..index_offset + 28]
                .try_into()
                .unwrap(),
        ) as usize;
        let mut bad_checksum = bytes.clone();
        bad_checksum[record_offset + 36] ^= 1;
        std::fs::write(root.join(&reference.relative_path), &bad_checksum).unwrap();
        assert_eq!(
            artifact_status(root, &reference),
            ArtifactStatus::Corrupt,
            "a broken stored BLAKE3 digest must not count as available"
        );
    }

    /// REVIEW-SIDECAR-FOLLOWUP-1: a non-empty file shorter than the 8-byte
    /// magic used to slip past the failed magic read as `Available`, and a
    /// `zdata`-declared file without the magic used to fall through as an
    /// unverifiable "opaque" payload.
    #[cfg(feature = "zdata")]
    #[test]
    fn artifact_status_rejects_undersized_and_magicless_declared_zdata() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir_all(root.join("masks")).unwrap();
        let mut reference = ArtifactReference {
            relative_path: "masks/subject.zdata".into(),
            format: "zdata".into(),
            checksum: "blake3:x".into(),
            width: 2,
            height: 2,
            channels: "u16".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        };
        // Non-empty but smaller than the container magic.
        std::fs::write(root.join(&reference.relative_path), b"LUM").unwrap();
        assert_eq!(
            artifact_status(root, &reference),
            ArtifactStatus::Corrupt,
            "a non-empty <8-byte file can never be a container header"
        );
        // Larger than the magic but without it, while declaring `zdata`: the
        // declared format owns the container, so this is mislabeled — corrupt,
        // not an opaque payload.
        let magicless = b"not-a-lumina-container-payload";
        std::fs::write(root.join(&reference.relative_path), magicless).unwrap();
        assert_eq!(
            artifact_status(root, &reference),
            ArtifactStatus::Corrupt,
            "a format==zdata file without LUMZDATA magic must not count as available"
        );
        // The same bytes under the producer spelling written by lumina-core
        // are covered by the same rule.
        reference.format = "lumina-zdata".into();
        assert_eq!(
            artifact_status(root, &reference),
            ArtifactStatus::Corrupt,
            "the lumina-zdata producer spelling requires the container magic too"
        );
        // The identical bytes under a genuinely opaque format stay available:
        // this crate owns no parser for them (documented limitation).
        reference.format = "opaque".into();
        reference.relative_path = "masks/opaque.bin".into();
        std::fs::write(root.join(&reference.relative_path), magicless).unwrap();
        assert_eq!(
            artifact_status(root, &reference),
            ArtifactStatus::Available,
            "opaque formats remain available once they pass the structural checks"
        );
    }

    /// REVIEW-SIDECAR-STATUS-1 / REVIEW-SIDECAR-FOLLOWUP-1: runs with and
    /// without the `zdata` feature so both `artifact_status` builds enforce
    /// the same structural floor.
    #[test]
    fn empty_artifact_file_is_corrupt_even_without_zdata_support() {
        let directory = tempfile::tempdir().unwrap();
        let artifact_dir = directory.path().join("masks");
        std::fs::create_dir_all(&artifact_dir).unwrap();
        let mut reference = ArtifactReference {
            relative_path: "masks/a.bin".into(),
            format: "opaque".into(),
            checksum: "blake3:x".into(),
            width: 1,
            height: 1,
            channels: "u16".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        };
        std::fs::write(directory.path().join(&reference.relative_path), b"").unwrap();
        assert_eq!(
            artifact_status(directory.path(), &reference),
            ArtifactStatus::Corrupt
        );
        // REVIEW-SIDECAR-FOLLOWUP-1: non-empty but shorter than the container
        // magic — previously misread as `Available` when the magic read failed.
        std::fs::write(directory.path().join(&reference.relative_path), b"ab").unwrap();
        assert_eq!(
            artifact_status(directory.path(), &reference),
            ArtifactStatus::Corrupt,
            "a non-empty <8-byte file must stay corrupt in every build"
        );
        std::fs::write(
            directory.path().join(&reference.relative_path),
            b"opaque-payload",
        )
        .unwrap();
        assert_eq!(
            artifact_status(directory.path(), &reference),
            ArtifactStatus::Available
        );
        // A zdata-declared path without the container magic is corrupt in both
        // builds; with the feature it additionally fails the declared-format
        // rule before any parse would run.
        reference.format = "zdata".into();
        reference.relative_path = "masks/b.zdata".into();
        std::fs::write(
            directory.path().join(&reference.relative_path),
            b"opaque-payload",
        )
        .unwrap();
        assert_eq!(
            artifact_status(directory.path(), &reference),
            ArtifactStatus::Corrupt,
            "a zdata-declared file without LUMZDATA magic must be corrupt"
        );
    }

    // ----- REVIEW-SIDECAR-N1: migration temporaries are sweepable -----

    #[test]
    fn migration_leaves_no_stray_temporary_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = SidecarDocument::new(source(), "pipeline-1");
        let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
        value["schema_version"] = Value::from(0);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        migrate_sidecar_file(&path).unwrap();
        for entry in std::fs::read_dir(directory.path()).unwrap() {
            let name = entry.unwrap().file_name();
            assert!(
                !name.to_string_lossy().contains(".tmp"),
                "migration temporary leaked with crate-default prefix: {name:?}"
            );
        }
    }

    #[test]
    fn interrupted_migration_temporary_is_sweepable_by_recovery() {
        // An interrupted migration used to leave `<crate-default>.tmp` files
        // that recover_sidecar could never recognize. With the aligned
        // `.{name}.tmp-` prefix the sweep now cleans them up.
        let directory = tempfile::tempdir().unwrap();
        let bak = directory.path().join("image.lumina.json.bak");
        std::fs::write(&bak, b"{\"schema_version\": 0, \"partial\": ").unwrap();
        let orphaned = directory.path().join(".image.lumina.json.bak.tmp-crash");
        std::fs::write(&orphaned, b"partial migration temp").unwrap();
        backdate(&orphaned, TEMP_SWEEP_AGE + Duration::from_secs(1));
        let report = recover_sidecar(&bak).unwrap();
        assert_eq!(report.removed_temporary_files, 1);
        assert!(!orphaned.exists());
    }

    // ----- REVIEW-SIDECAR-N2: schema_version 0 requires explicit migration -----

    #[test]
    fn schema_version_zero_is_rejected_with_migration_hint() {
        let d = SidecarDocument::new(source(), "pipeline-1");
        let mut value: Value = serde_json::from_str(&d.to_json().unwrap()).unwrap();
        value["schema_version"] = Value::from(0);
        let json = serde_json::to_string(&value).unwrap();
        let error = SidecarDocument::from_json(&json).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("schema_version 0") && message.contains("explicit migration"),
            "loader must reject v0 loudly with a migration hint, got {message}"
        );
        // The explicit migration path still performs the historical bump.
        let migrated = migrate_json(&json).unwrap();
        assert_eq!(
            SidecarDocument::from_json(&migrated)
                .unwrap()
                .schema_version,
            SCHEMA_VERSION
        );
    }

    // ----- REVIEW-SIDECAR-N3: finite/range validation for local values -----

    #[test]
    fn mask_layer_local_adjustments_are_range_validated() {
        let build = |feather: f32, blur: f32, density: f32| {
            let mut d = SidecarDocument::new(source(), "p");
            d.virtual_copies[0].mask_library.push(mask("m"));
            d.virtual_copies[0].mask_layers.push(MaskLayer {
                id: "layer".into(),
                mask: MaskReference {
                    copy_id: "vc-original".into(),
                    mask_id: "m".into(),
                    extras: Extras::new(),
                },
                inverted: false,
                feather,
                blur,
                density,
                extras: Extras::new(),
                visible: true,
            });
            d.validate()
        };
        // Defaults of every existing valid sidecar stay accepted.
        assert!(build(0.0, 0.0, 1.0).is_ok());
        assert!(build(0.5, 2.0, 0.25).is_ok());
        // feather/blur must be finite and >= 0.
        assert!(build(-0.1, 0.0, 1.0).is_err());
        assert!(build(f32::NAN, 0.0, 1.0).is_err());
        assert!(build(f32::INFINITY, 0.0, 1.0).is_err());
        // blur must be finite and >= 0.
        assert!(build(0.0, -1.0, 1.0).is_err());
        assert!(build(0.0, f32::NAN, 1.0).is_err());
        // density must be finite within 0..=1.
        assert!(build(0.0, 0.0, 1.5).is_err());
        assert!(build(0.0, 0.0, -0.01).is_err());
        assert!(build(0.0, 0.0, f32::NAN).is_err());
    }

    #[test]
    fn target_luminance_must_be_finite() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].recipe.auto_features.target_luminance = f64::NAN;
        assert!(d.validate().is_err());
        d.virtual_copies[0].recipe.auto_features.target_luminance = f64::INFINITY;
        assert!(d.validate().is_err());
        d.virtual_copies[0].recipe.auto_features.target_luminance = 0.42;
        assert!(d.validate().is_ok());
    }

    #[test]
    fn unknown_adjustment_keys_must_be_finite_but_stay_forward_compatible() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        // Unknown keys remain accepted for forward compatibility...
        d.virtual_copies[0]
            .recipe
            .adjustments
            .insert("future_slider".into(), 3.0);
        let json = d.to_json().unwrap();
        assert_eq!(
            SidecarDocument::from_json(&json).unwrap().virtual_copies[0]
                .recipe
                .adjustments["future_slider"],
            3.0
        );
        // ...but NaN/∞ slider states are never meaningful.
        d.virtual_copies[0]
            .recipe
            .adjustments
            .insert("future_slider".into(), f64::NAN);
        assert!(d.validate().is_err());
        d.virtual_copies[0]
            .recipe
            .adjustments
            .insert("future_slider".into(), f64::NEG_INFINITY);
        assert!(d.validate().is_err());
    }

    #[test]
    fn zero_inference_resolution_is_rejected() {
        let mut d = SidecarDocument::new(source(), "p");
        let mut m = mask("zerores");
        m.inference_resolution.width = 0;
        d.virtual_copies[0].mask_library.push(m);
        assert!(d
            .validate()
            .unwrap_err()
            .to_string()
            .contains("inference_resolution"));

        let mut d = SidecarDocument::new(source(), "p");
        let mut m = mask("zerores-h");
        m.inference_resolution.height = 0;
        d.virtual_copies[0].mask_library.push(m);
        assert!(d.validate().is_err());
    }

    // ----- REVIEW-SIDECAR-N4: rejected mutations roll back -----

    #[test]
    fn rejected_delete_virtual_copy_leaves_document_unchanged() {
        // vc-original owns a layer that references vc-target's mask. Deleting
        // vc-target would strand that layer, so validation must reject the
        // delete *and* the document must stay byte-for-byte at its prior
        // state (previously the copy had already been moved to the deleted
        // list when validation failed).
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.virtual_copies[0].mask_library.push(mask("original-mask"));
        d.virtual_copies[0].mask_layers.push(MaskLayer {
            id: "original-layer".into(),
            mask: MaskReference {
                copy_id: "vc-target".into(),
                mask_id: "target-mask".into(),
                extras: Extras::new(),
            },
            inverted: false,
            feather: 0.0,
            blur: 0.0,
            density: 1.0,
            extras: Extras::new(),
            visible: true,
        });
        d.virtual_copies.push(VirtualCopy {
            id: "vc-target".into(),
            name: "Target".into(),
            is_default: false,
            rating: 0,
            flag: Flag::Unflagged,
            recipe: EditRecipe::default(),
            mask_library: vec![mask("target-mask")],
            mask_layers: vec![],
            history: vec![],
            export_records: vec![],
            extras: Extras::new(),
        });
        let before = d.clone();
        assert!(d.validate().is_ok());

        // Deleting vc-target breaks the surviving layer -> rejected.
        let error = d.delete_virtual_copy("vc-target").unwrap_err();
        assert!(
            error.to_string().contains("unknown copy"),
            "delete must fail on the stranded reference, got {error}"
        );
        assert_eq!(d, before, "a rejected delete must not mutate the document");

        // Deleting vc-original stays forbidden by the explicit guard.
        assert!(d.delete_virtual_copy("vc-original").is_err());
        assert_eq!(d, before);

        // Deleting vc-original's *masks* is not a copy deletion; instead
        // verify the successful path once more: removing the dependent layer
        // makes the same delete legal.
        d.virtual_copies[0].mask_layers.clear();
        d.delete_virtual_copy("vc-target").unwrap();
        assert_eq!(d.virtual_copies.len(), 1);
        assert_eq!(d.deleted_virtual_copies.len(), 1);
        assert_eq!(d.deleted_virtual_copies[0].id, "vc-target");

        // A rejected restore also leaves the document unchanged.
        let after_delete = d.clone();
        d.restore_virtual_copy("does-not-exist").unwrap_err();
        assert_eq!(d, after_delete);
    }

    #[test]
    fn rejected_duplicate_and_rename_leave_document_unchanged() {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.duplicate_virtual_copy("vc-original", "vc-copy", "Copy")
            .unwrap();
        let before = d.clone();

        // Duplicate id -> precheck rejects without inserting.
        assert!(d
            .duplicate_virtual_copy("vc-original", "vc-copy", "Other")
            .is_err());
        assert_eq!(d, before);

        // Invalid recipe content in the duplicate -> rollback on validate.
        let mut poisoned_source = d.clone();
        poisoned_source.virtual_copies[0]
            .recipe
            .adjustments
            .insert("exposure".into(), 99.0);
        assert!(poisoned_source
            .duplicate_virtual_copy("vc-original", "vc-poison", "Poison")
            .is_err());

        // Empty rename -> rejected before mutating.
        assert!(d.rename_virtual_copy("vc-copy", "   ").is_err());
        assert_eq!(d, before);

        // Unknown id rename -> rejected.
        assert!(d.rename_virtual_copy("missing", "X").is_err());
        assert_eq!(d, before);
    }

    // ----- REVIEW-SIDECAR-N5: bounded sidecar reads -----

    #[test]
    fn oversized_sidecar_file_is_rejected_without_full_read() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("huge.lumina.json");
        {
            let file = fs::File::create(&path).unwrap();
            // Sparse file: reserves length without occupying disk space.
            file.set_len(MAX_SIDECAR_BYTES as u64 + 1).unwrap();
        } // Handle closed here; an explicit `drop()` trips `clippy::drop_non_drop`.
        let error = load_sidecar(&path).unwrap_err();
        assert!(
            matches!(&error, SidecarError::Invalid(message) if message.contains("size limit")),
            "oversized sidecar must be rejected by size, got {error}"
        );
    }

    // =====================================================================
    // G-15 META-MVP Slice 1: keywords, static collections, smart-collection
    // criteria as data, and the batch-operation model.
    // =====================================================================

    fn meta_document() -> SidecarDocument {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        d.keywords = vec!["landscape".into(), "alps 2026".into()];
        d.collections = vec![
            CollectionMembership {
                id: "col-best".into(),
                name: "Best of 2026".into(),
            },
            CollectionMembership {
                id: "col-print".into(),
                name: "Print".into(),
            },
        ];
        d.virtual_copies[0].rating = 4;
        d.virtual_copies[0].flag = Flag::Pick;
        d
    }

    fn smart_def(rule: SmartRule) -> SmartCollectionDef {
        SmartCollectionDef {
            version: SMART_COLLECTION_VERSION,
            id: "smart-1".into(),
            name: "Picks".into(),
            rule,
        }
    }

    #[test]
    fn meta_keywords_and_collections_roundtrip() {
        let d = meta_document();
        let json = d.to_json().unwrap();
        assert!(json.contains("landscape"));
        assert!(json.contains("col-best"));
        let decoded = SidecarDocument::from_json(&json).unwrap();
        assert_eq!(decoded, d);
        // Second roundtrip is a fixed point.
        let decoded2 = SidecarDocument::from_json(&decoded.to_json().unwrap()).unwrap();
        assert_eq!(decoded2, d);
    }

    #[test]
    fn meta_legacy_documents_default_to_empty_and_serialize_absent() {
        // Current-schema JSON without the additive keys: absent = empty.
        let json = r#"{"format":"lumina-sidecar","schema_version":2,"source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"PNG","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","presets":[],"virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{},"mask_library":[],"mask_layers":[],"history":[],"export_records":[]}]}"#;
        let doc = SidecarDocument::from_json(json).unwrap();
        assert!(doc.keywords.is_empty());
        assert!(doc.collections.is_empty());
        assert!(doc.validate().is_ok());
        let out = doc.to_json().unwrap();
        assert!(!out.contains("keywords"));
        assert!(!out.contains("\"collections\""));
        // Schema-v1 JSON without the keys behaves identically (no migration
        // needed for additive metadata).
        let v1 = json.replace("\"schema_version\":2", "\"schema_version\":1");
        let doc_v1 = SidecarDocument::from_json(&v1).unwrap();
        assert!(doc_v1.keywords.is_empty());
        assert!(doc_v1.collections.is_empty());
    }

    #[test]
    fn meta_migration_v1_to_v2_preserves_document_without_loss() {
        // The explicit migration path stamps v1 → v2 while every other field
        // (including legacy rating/flag and recipe content) is preserved; the
        // new metadata keys default to empty.
        let mut document = SidecarDocument::new(source(), "pipeline-1");
        document.virtual_copies[0].rating = 3;
        document.virtual_copies[0].flag = Flag::Reject;
        document.virtual_copies[0]
            .recipe
            .adjustments
            .insert("exposure".into(), 1.5);
        let mut legacy: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
        legacy["schema_version"] = Value::from(1);
        let migrated = migrate_json(&serde_json::to_string(&legacy).unwrap()).unwrap();
        let decoded = SidecarDocument::from_json(&migrated).unwrap();
        assert_eq!(decoded.schema_version, SCHEMA_VERSION);
        assert!(decoded.keywords.is_empty());
        assert!(decoded.collections.is_empty());
        assert_eq!(decoded.virtual_copies[0].rating, 3);
        assert_eq!(decoded.virtual_copies[0].flag, Flag::Reject);
        assert_eq!(
            decoded.virtual_copies[0].recipe.adjustments["exposure"],
            1.5
        );
    }

    #[test]
    fn meta_keyword_validation_rejects_loudly() {
        for bad in [
            String::new(),
            "   ".into(),
            " leading".into(),
            "trailing ".into(),
            "with\ttab".into(),
            "with\nnewline".into(),
            "x".repeat(MAX_KEYWORD_CHARS + 1),
        ] {
            let mut d = SidecarDocument::new(source(), "p");
            d.keywords = vec![bad.clone()];
            assert!(
                d.validate().is_err(),
                "keyword `{bad}` must be rejected loudly"
            );
        }
        // Exact duplicates are rejected, not silently deduplicated.
        let mut d = SidecarDocument::new(source(), "p");
        d.keywords = vec!["alps".into(), "alps".into()];
        assert!(d.validate().unwrap_err().to_string().contains("duplicate"));
        // Over-limit list is rejected.
        let mut d = SidecarDocument::new(source(), "p");
        d.keywords = (0..MAX_KEYWORDS_PER_DOCUMENT + 1)
            .map(|i| format!("kw-{i}"))
            .collect();
        assert!(d.validate().is_err());
        // Valid keywords pass.
        assert!(meta_document().validate().is_ok());
    }

    #[test]
    fn meta_collection_validation_rejects_loudly() {
        // Path-like ids must never enter a portable sidecar.
        for bad_id in [
            String::new(),
            " padded ".into(),
            "../outside".into(),
            "/abs/path".into(),
            "a/b".into(),
            r"a\b".into(),
            "c:drive".into(),
        ] {
            let mut d = SidecarDocument::new(source(), "p");
            d.collections = vec![CollectionMembership {
                id: bad_id.clone(),
                name: "Name".into(),
            }];
            assert!(
                d.validate().is_err(),
                "collection id `{bad_id}` must be rejected loudly"
            );
        }
        // Empty/padded names and duplicate ids fail loudly.
        let mut d = SidecarDocument::new(source(), "p");
        d.collections = vec![CollectionMembership {
            id: "a".into(),
            name: String::new(),
        }];
        assert!(d.validate().is_err());
        let mut d = SidecarDocument::new(source(), "p");
        d.collections = vec![
            CollectionMembership {
                id: "a".into(),
                name: "One".into(),
            },
            CollectionMembership {
                id: "a".into(),
                name: "Two".into(),
            },
        ];
        assert!(d.validate().unwrap_err().to_string().contains("duplicate"));
        assert!(meta_document().validate().is_ok());
    }

    #[test]
    fn smart_rule_evaluation_matrix_is_deterministic() {
        let keywords = vec!["alps".to_string(), "night".to_string()];
        // Leaf rules.
        assert!(SmartRule::All.matches(&keywords, 0, Flag::Unflagged));
        assert!(!SmartRule::None.matches(&keywords, 5, Flag::Pick));
        assert!(SmartRule::Keyword {
            keyword: "alps".into()
        }
        .matches(&keywords, 0, Flag::Unflagged));
        assert!(!SmartRule::Keyword {
            keyword: "Alps".into()
        }
        .matches(&keywords, 0, Flag::Unflagged));
        assert!(!SmartRule::Keyword {
            keyword: "sea".into()
        }
        .matches(&keywords, 0, Flag::Unflagged));
        assert!(SmartRule::RatingAtLeast { rating: 3 }.matches(&keywords, 4, Flag::Unflagged));
        assert!(!SmartRule::RatingAtLeast { rating: 5 }.matches(&keywords, 4, Flag::Unflagged));
        assert!(SmartRule::RatingEquals { rating: 4 }.matches(&keywords, 4, Flag::Unflagged));
        assert!(!SmartRule::RatingEquals { rating: 3 }.matches(&keywords, 4, Flag::Unflagged));
        assert!(SmartRule::Flag { flag: Flag::Pick }.matches(&keywords, 0, Flag::Pick));
        assert!(!SmartRule::Flag { flag: Flag::Pick }.matches(&keywords, 0, Flag::Reject));
        // Combinators: "rated picks from the alps, but no rejects".
        let rule = SmartRule::And {
            rules: vec![
                SmartRule::Keyword {
                    keyword: "alps".into(),
                },
                SmartRule::Or {
                    rules: vec![
                        SmartRule::Flag { flag: Flag::Pick },
                        SmartRule::RatingAtLeast { rating: 4 },
                    ],
                },
                SmartRule::Not {
                    rule: Box::new(SmartRule::Flag { flag: Flag::Reject }),
                },
            ],
        };
        assert!(rule.matches(&keywords, 4, Flag::Pick));
        assert!(!rule.matches(&keywords, 4, Flag::Reject));
        assert!(!rule.matches(&keywords, 2, Flag::Unflagged));
        assert!(!rule.matches(&["sea".to_string()], 5, Flag::Pick));
    }

    #[test]
    fn smart_collection_matches_copies_and_reports_unknown_ids() {
        let mut d = meta_document();
        d.duplicate_virtual_copy("vc-original", "vc-second", "Second")
            .unwrap();
        d.virtual_copies[1].rating = 1;
        d.virtual_copies[1].flag = Flag::Reject;
        let def = smart_def(SmartRule::And {
            rules: vec![
                SmartRule::Keyword {
                    keyword: "landscape".into(),
                },
                SmartRule::RatingAtLeast { rating: 4 },
            ],
        });
        assert!(def.matches_copy(&d, "vc-original").unwrap());
        assert!(!def.matches_copy(&d, "vc-second").unwrap());
        assert!(def.matches_any_copy(&d).unwrap());
        let none = smart_def(SmartRule::Flag { flag: Flag::Pick });
        d.virtual_copies[0].flag = Flag::Unflagged;
        assert!(!none.matches_any_copy(&d).unwrap());
        // Unknown copy ids are loud errors, never silent non-matches.
        assert!(def.matches_copy(&d, "vc-missing").is_err());
    }

    #[test]
    fn smart_collection_definitions_roundtrip_and_validate_loudly() {
        let def = smart_def(SmartRule::Or {
            rules: vec![
                SmartRule::Keyword {
                    keyword: "alps".into(),
                },
                SmartRule::Not {
                    rule: Box::new(SmartRule::Flag { flag: Flag::Reject }),
                },
            ],
        });
        let json = serde_json::to_string(&def).unwrap();
        let decoded: SmartCollectionDef = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, def);
        assert!(validate_smart_collection_def(&def).is_ok());
        // Bad version.
        let mut bad = def.clone();
        bad.version = 99;
        assert!(validate_smart_collection_def(&bad).is_err());
        // Rating out of range.
        assert!(
            validate_smart_collection_def(&smart_def(SmartRule::RatingAtLeast { rating: 6 }))
                .is_err()
        );
        // Empty And/Or are schema violations, not vacuous truths.
        assert!(
            validate_smart_collection_def(&smart_def(SmartRule::And { rules: vec![] })).is_err()
        );
        assert!(
            validate_smart_collection_def(&smart_def(SmartRule::Or { rules: vec![] })).is_err()
        );
        // Empty keyword inside a rule.
        assert!(
            validate_smart_collection_def(&smart_def(SmartRule::Keyword {
                keyword: String::new()
            }))
            .is_err()
        );
        // Excessive nesting is rejected (stack-safe bound).
        let mut deep = SmartRule::All;
        for _ in 0..MAX_SMART_RULE_DEPTH + 2 {
            deep = SmartRule::Not {
                rule: Box::new(deep),
            };
        }
        assert!(validate_smart_collection_def(&smart_def(deep)).is_err());
        // Unknown rule operators fail at parse time, never as silent `None`.
        assert!(serde_json::from_str::<SmartRule>(r#"{"op":"fuzzy"}"#).is_err());
    }

    #[test]
    fn batch_ops_apply_idempotently_and_fail_loudly() {
        let mut d = SidecarDocument::new(source(), "p");
        // Add/remove keyword with changed flags.
        assert!(apply_batch_op(
            &mut d,
            &BatchOp::AddKeyword {
                keyword: "alps".into()
            }
        )
        .unwrap());
        assert!(!apply_batch_op(
            &mut d,
            &BatchOp::AddKeyword {
                keyword: "alps".into()
            }
        )
        .unwrap());
        assert_eq!(d.keywords, vec!["alps".to_string()]);
        assert!(!apply_batch_op(
            &mut d,
            &BatchOp::RemoveKeyword {
                keyword: "sea".into()
            }
        )
        .unwrap());
        assert!(apply_batch_op(
            &mut d,
            &BatchOp::RemoveKeyword {
                keyword: "alps".into()
            }
        )
        .unwrap());
        assert!(d.keywords.is_empty());
        // Collections: add, rename propagation, idempotent add, remove.
        assert!(apply_batch_op(
            &mut d,
            &BatchOp::AddToCollection {
                id: "c1".into(),
                name: "One".into()
            }
        )
        .unwrap());
        assert!(!apply_batch_op(
            &mut d,
            &BatchOp::AddToCollection {
                id: "c1".into(),
                name: "One".into()
            }
        )
        .unwrap());
        assert!(apply_batch_op(
            &mut d,
            &BatchOp::AddToCollection {
                id: "c1".into(),
                name: "Uno".into()
            }
        )
        .unwrap());
        assert_eq!(d.collections[0].name, "Uno");
        assert!(!apply_batch_op(
            &mut d,
            &BatchOp::RemoveFromCollection {
                id: "missing".into()
            }
        )
        .unwrap());
        assert!(
            apply_batch_op(&mut d, &BatchOp::RemoveFromCollection { id: "c1".into() }).unwrap()
        );
        // Rating/flag per copy with changed flags.
        assert!(apply_batch_op(
            &mut d,
            &BatchOp::SetRating {
                copy_id: "vc-original".into(),
                rating: 5
            }
        )
        .unwrap());
        assert!(!apply_batch_op(
            &mut d,
            &BatchOp::SetRating {
                copy_id: "vc-original".into(),
                rating: 5
            }
        )
        .unwrap());
        assert!(apply_batch_op(
            &mut d,
            &BatchOp::SetFlag {
                copy_id: "vc-original".into(),
                flag: Flag::Pick
            }
        )
        .unwrap());
        assert!(d.validate().is_ok());
        // Loud failures leave the document unchanged.
        let before = d.clone();
        assert!(apply_batch_op(
            &mut d,
            &BatchOp::SetRating {
                copy_id: "vc-original".into(),
                rating: 6
            }
        )
        .is_err());
        assert!(apply_batch_op(
            &mut d,
            &BatchOp::SetRating {
                copy_id: "vc-missing".into(),
                rating: 3
            }
        )
        .is_err());
        assert!(apply_batch_op(
            &mut d,
            &BatchOp::SetFlag {
                copy_id: "vc-missing".into(),
                flag: Flag::Pick
            }
        )
        .is_err());
        assert!(apply_batch_op(
            &mut d,
            &BatchOp::AddKeyword {
                keyword: "  padded".into()
            }
        )
        .is_err());
        assert!(apply_batch_op(
            &mut d,
            &BatchOp::AddToCollection {
                id: "/abs".into(),
                name: "X".into()
            }
        )
        .is_err());
        assert_eq!(d, before);
    }

    #[test]
    fn batch_ops_preserve_unrelated_state_and_roundtrip() {
        let mut d = meta_document();
        d.virtual_copies[0]
            .recipe
            .adjustments
            .insert("exposure".into(), 0.75);
        let recipe_before = d.virtual_copies[0].recipe.clone();
        assert!(apply_batch_op(
            &mut d,
            &BatchOp::AddKeyword {
                keyword: "night".into()
            }
        )
        .unwrap());
        assert!(apply_batch_op(
            &mut d,
            &BatchOp::SetFlag {
                copy_id: "vc-original".into(),
                flag: Flag::Reject
            }
        )
        .unwrap());
        // Recipe, masks and history are untouched by metadata batch ops.
        assert_eq!(d.virtual_copies[0].recipe, recipe_before);
        let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
        assert_eq!(decoded, d);
        // Batch ops themselves are portable data.
        let op = BatchOp::AddKeyword {
            keyword: "alps".into(),
        };
        let json = serde_json::to_string(&op).unwrap();
        assert_eq!(serde_json::from_str::<BatchOp>(&json).unwrap(), op);
    }

    #[test]
    fn meta_file_roundtrip_preserves_metadata_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.lumina.json");
        let document = meta_document();
        save_sidecar(&path, &document).unwrap();
        assert_eq!(load_sidecar(&path).unwrap(), document);
    }

    /// LRPAR-G01-BASIC: treatment/profile roundtrip — defaults read as
    /// `color`/`default`, `bw` stashes and restores, JSON roundtrips.
    #[test]
    fn g01_treatment_profile_roundtrip() {
        let mut recipe = EditRecipe::default();
        assert_eq!(recipe.treatment(), TREATMENT_COLOR);
        assert_eq!(recipe.develop_profile(), DEFAULT_DEVELOP_PROFILE);
        // Enabling B&W from defaults stashes absence and sets -1.
        assert!(recipe.apply_treatment(TREATMENT_BW).unwrap());
        assert_eq!(recipe.treatment(), TREATMENT_BW);
        assert_eq!(recipe.adjustments.get("saturation"), Some(&-1.0));
        assert_eq!(recipe.adjustments.get("vibrance"), Some(&-1.0));
        // Idempotent re-apply reports no change.
        assert!(!recipe.apply_treatment(TREATMENT_BW).unwrap());
        // A pre-existing saturation survives the roundtrip via the stash.
        let mut recipe2 = EditRecipe::default();
        recipe2.adjustments.insert("saturation".into(), 0.3);
        assert!(recipe2.apply_treatment(TREATMENT_BW).unwrap());
        assert!(recipe2.apply_treatment(TREATMENT_COLOR).unwrap());
        assert_eq!(recipe2.treatment(), TREATMENT_COLOR);
        assert_eq!(recipe2.adjustments.get("saturation"), Some(&0.3));
        assert!(!recipe2.adjustments.contains_key("vibrance"));
        // Absent keys are removed again, never left at -1.
        assert!(recipe.apply_treatment(TREATMENT_COLOR).unwrap());
        assert!(!recipe.adjustments.contains_key("saturation"));
        assert!(!recipe.adjustments.contains_key("vibrance"));
        assert!(!recipe.apply_treatment(TREATMENT_COLOR).unwrap());
        // Profile: default removes the key, others persist.
        assert!(!recipe
            .apply_develop_profile(DEFAULT_DEVELOP_PROFILE)
            .unwrap());
        assert!(recipe.apply_develop_profile("vivid").unwrap());
        assert_eq!(recipe.develop_profile(), "vivid");
        assert!(!recipe.apply_develop_profile("vivid").unwrap());
        assert!(recipe
            .apply_develop_profile(DEFAULT_DEVELOP_PROFILE)
            .unwrap());
        assert!(!recipe.options.contains_key(DEVELOP_PROFILE_KEY));
        // Full JSON roundtrip + validation of the bw state.
        recipe.apply_treatment(TREATMENT_BW).unwrap();
        recipe.apply_develop_profile("portrait").unwrap();
        let json = serde_json::to_value(&recipe).unwrap();
        let decoded: EditRecipe = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, recipe);
        validate_adjustments(&decoded).unwrap();
    }

    /// LRPAR-G01-BASIC: unknown/empty/corrupt treatment/profile values fail
    /// loudly — never a silent normalisation to a default.
    #[test]
    fn g01_treatment_profile_rejects_invalid() {
        let mut recipe = EditRecipe::default();
        assert!(recipe.apply_treatment("sepia").is_err());
        assert!(recipe.apply_treatment("").is_err());
        assert!(recipe.apply_develop_profile("adobe-color").is_err());
        assert!(recipe.apply_develop_profile("").is_err());
        assert!(recipe.apply_develop_profile("/abs/path").is_err());
        // Crafted invalid states fail validation.
        recipe
            .extras
            .insert(TREATMENT_KEY.into(), Value::String("sepia".into()));
        assert!(validate_adjustments(&recipe).is_err());
        recipe.extras.remove(TREATMENT_KEY);
        recipe
            .extras
            .insert(BW_STASH_KEY.into(), serde_json::json!({"saturation": 99.0}));
        assert!(validate_adjustments(&recipe).is_err());
        recipe
            .extras
            .insert(BW_STASH_KEY.into(), Value::String("corrupt".into()));
        assert!(validate_adjustments(&recipe).is_err());
        recipe.extras.remove(BW_STASH_KEY);
        recipe
            .options
            .insert(DEVELOP_PROFILE_KEY.into(), "unknown".into());
        assert!(validate_adjustments(&recipe).is_err());
        recipe.options.remove(DEVELOP_PROFILE_KEY);
        validate_adjustments(&recipe).unwrap();
    }

    // =====================================================================
    // LRPAR-G15-IPTC-S1: Sidecar-`metadata`-Draft (Datenmodell, Validierung,
    // Historie, CAS-Persistenz).
    // =====================================================================

    fn draft_fields(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    fn metadata_doc() -> SidecarDocument {
        SidecarDocument::new(source(), "pipeline-1")
    }

    #[test]
    fn iptc_s1_absent_metadata_reads_as_empty_and_serializes_absent() {
        // Legacy JSON without the additive key: absent = empty draft.
        let json = r#"{"format":"lumina-sidecar","schema_version":2,"source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"PNG","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","presets":[],"virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{},"mask_library":[],"mask_layers":[],"history":[],"export_records":[]}]}"#;
        let decoded = SidecarDocument::from_json(json).unwrap();
        assert!(decoded.metadata.is_empty());
        assert_eq!(decoded.metadata.version, METADATA_DRAFT_VERSION);
        assert_eq!(decoded.metadata.latest_rev(), 0);
        // Empty drafts serialize back absent: legacy documents stay byte-stable.
        let reserialized = decoded.to_json().unwrap();
        assert!(
            !reserialized.contains("\"metadata\""),
            "empty draft must serialize absent, got {reserialized}"
        );
        // A fresh document behaves the same.
        assert!(metadata_doc().metadata.is_empty());
        assert!(!metadata_doc().to_json().unwrap().contains("\"metadata\""));
    }

    #[test]
    fn iptc_s1_draft_roundtrip_file() {
        let mut doc = metadata_doc();
        doc.apply_metadata_draft(
            &draft_fields(&[
                ("title", "Startschuss"),
                ("description", "Mehrzeilige … Beschreibung"),
                ("city", "Berlin"),
                ("date_created", "2026-09-04"),
            ]),
            "manual",
            "2026-09-04T09:30:00Z",
        )
        .unwrap();
        doc.apply_metadata_draft(
            &draft_fields(&[("title", "Zieleinlauf"), ("creator", "Fotografin Ü")]),
            "preset:veranstaltung",
            "2026-09-04T10:00:00Z",
        )
        .unwrap();
        // JSON roundtrip is a fixed point, newest entry first.
        let json = doc.to_json().unwrap();
        assert!(json.contains("\"metadata\""));
        let decoded = SidecarDocument::from_json(&json).unwrap();
        assert_eq!(decoded, doc);
        assert_eq!(decoded.metadata.history.len(), 2);
        assert_eq!(decoded.metadata.history[0].rev, 2);
        assert_eq!(decoded.metadata.history[1].rev, 1);
        assert_eq!(decoded.metadata.get("title"), Some("Zieleinlauf"));
        // File roundtrip through the atomic + CAS write path.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("IMG_0001.ARW.lumina.json");
        save_sidecar(&path, &doc).unwrap();
        let reloaded = load_sidecar(&path).unwrap();
        assert_eq!(reloaded, doc);
        let revision = document_revision(&doc).unwrap();
        let saved_revision = save_sidecar_if_unchanged(&path, &reloaded, Some(&revision)).unwrap();
        assert_eq!(saved_revision, revision);
    }

    #[test]
    fn iptc_s1_validation_matrix() {
        // Per-field limits: exactly at the limit passes, one char over fails.
        for (field, limit) in [
            ("title", MAX_METADATA_TITLE_CHARS),
            ("headline", MAX_METADATA_HEADLINE_CHARS),
            ("description", MAX_METADATA_DESCRIPTION_CHARS),
            ("copyright_notice", MAX_METADATA_COPYRIGHT_NOTICE_CHARS),
            ("creator", MAX_METADATA_CREATOR_CHARS),
            ("credit", MAX_METADATA_CREDIT_CHARS),
            ("source", MAX_METADATA_SOURCE_CHARS),
            ("city", MAX_METADATA_CITY_CHARS),
            ("state_province", MAX_METADATA_STATE_PROVINCE_CHARS),
            ("country", MAX_METADATA_COUNTRY_CHARS),
        ] {
            assert_eq!(metadata_field_limit(field), Some(limit));
            validate_metadata_field_value(field, &"x".repeat(limit)).unwrap();
            assert!(
                validate_metadata_field_value(field, &"x".repeat(limit + 1)).is_err(),
                "field `{field}` must reject {} chars",
                limit + 1
            );
            // Multi-byte chars count as chars, not bytes.
            validate_metadata_field_value(field, &"ü".repeat(limit)).unwrap();
            assert!(
                validate_metadata_field_value(field, &"ü".repeat(limit + 1)).is_err(),
                "field `{field}` must count chars, not bytes"
            );
        }
        assert_eq!(metadata_field_limit("date_created"), None);
        // `date_created`: valid dates pass, everything else fails loudly.
        for valid in ["2026-09-04", "2024-02-29", "2000-02-29", "1999-12-31"] {
            validate_metadata_field_value("date_created", valid).unwrap();
        }
        for invalid_date in [
            "2026-9-4",
            "04.09.2026",
            "2026/09/04",
            "2026-09-04T10:00:00Z",
            "2026-13-01",
            "2026-00-10",
            "2026-02-30",
            "2023-02-29",
            "1900-02-29",
            "2026-04-31",
            "not-a-date",
        ] {
            assert!(
                validate_metadata_field_value("date_created", invalid_date).is_err(),
                "`{invalid_date}` must be rejected"
            );
        }
        // Unknown IDs, control characters and untrimmed values fail loudly.
        assert!(validate_metadata_field_value("byline_title", "x").is_err());
        assert!(validate_metadata_field_value("Title", "x").is_err());
        assert!(validate_metadata_field_value("title", "a\tb").is_err());
        assert!(validate_metadata_field_value("title", "a\nb").is_err());
        assert!(validate_metadata_field_value("title", " padded").is_err());
        assert!(validate_metadata_field_value("title", "padded ").is_err());
        // Empty / whitespace-only means "remove" at mutation time.
        validate_metadata_field_value("title", "").unwrap();
        validate_metadata_field_value("title", "   ").unwrap();
        // ...but a *stored* empty/untrimmed value is a loud schema violation.
        let mut doc = metadata_doc();
        doc.metadata.draft.insert("title".into(), "".into());
        assert!(doc.validate().is_err());
        doc.metadata.draft.insert("title".into(), " padded ".into());
        assert!(doc.validate().is_err());
        doc.metadata.draft.insert("mystery".into(), "x".into());
        assert!(doc.validate().is_err());
        // Unsupported metadata version fails loudly, never silently accepted.
        let mut versioned = metadata_doc();
        versioned.metadata.version = METADATA_DRAFT_VERSION + 1;
        assert!(versioned.validate().is_err());
    }

    #[test]
    fn iptc_s1_keywords_routing_rejected() {
        // `keywords` stays the existing source-level field: using it as a
        // draft ID fails loudly with a routing hint, on both paths.
        let error = validate_metadata_field_value("keywords", "festival").unwrap_err();
        assert!(
            matches!(&error, SidecarError::Invalid(message) if message.contains("keywords")),
            "unexpected error: {error}"
        );
        let mut doc = metadata_doc();
        let failed = doc.apply_metadata_draft(
            &draft_fields(&[("keywords", "festival")]),
            "manual",
            "2026-09-04T10:00:00Z",
        );
        assert!(failed.is_err());
        assert!(doc.metadata.is_empty());
        assert!(doc.metadata.history.is_empty());
    }

    #[test]
    fn iptc_s1_origin_timestamp_validation() {
        for valid in [
            "manual",
            "cli",
            "gui",
            "mcp",
            "preset:veranstaltung",
            "preset:a b_c-9",
            "sync:vc-original",
            "sync:copy-42",
        ] {
            validate_metadata_origin(valid).unwrap();
        }
        for bad in [
            "",
            "Manual",
            "MANUAL",
            " manual",
            "manual ",
            "preset:",
            "sync:",
            "preset: name",
            "preset:name ",
            "sync:/abs/path",
            "preset:/abs",
            "mail",
            "user:fred",
            "preset:a\tb",
        ] {
            assert!(
                validate_metadata_origin(bad).is_err(),
                "origin `{bad}` must be rejected"
            );
        }
        for valid in [
            "2026-09-04T10:00:00Z",
            "2026-09-04T10:00:00.123Z",
            "2024-02-29T23:59:59Z",
        ] {
            validate_metadata_timestamp(valid).unwrap();
        }
        for bad in [
            "",
            "yesterday",
            "2026-09-04",
            "2026-09-04T10:00:00",
            "2026-09-04T10:00:00+02:00",
            "2026-09-04 10:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-09-04T24:00:00Z",
            "2026-09-04T10:00:00.Z",
            "2026-02-30T00:00:00Z",
        ] {
            assert!(
                validate_metadata_timestamp(bad).is_err(),
                "timestamp `{bad}` must be rejected"
            );
        }
        // The std-only constructor always produces valid UTC timestamps.
        let now = now_rfc3339_utc();
        validate_metadata_timestamp(&now).unwrap();
        assert!(now.ends_with('Z'));
        // Bad origin/timestamp reject the whole mutation, all-or-nothing.
        let mut doc = metadata_doc();
        assert!(doc
            .apply_metadata_draft(
                &draft_fields(&[("title", "x")]),
                "carrier-pigeon",
                "2026-09-04T10:00:00Z",
            )
            .is_err());
        assert!(doc
            .apply_metadata_draft(&draft_fields(&[("title", "x")]), "manual", "sometime",)
            .is_err());
        assert!(doc.metadata.is_empty());
    }

    #[test]
    fn iptc_s1_apply_all_or_nothing_and_idempotent() {
        let mut doc = metadata_doc();
        doc.apply_metadata_draft(
            &draft_fields(&[("title", "Start")]),
            "manual",
            "2026-09-04T09:00:00Z",
        )
        .unwrap();
        // One invalid field among valid ones rejects everything.
        let failed = doc.apply_metadata_draft(
            &draft_fields(&[("city", "Berlin"), ("nope", "x")]),
            "manual",
            "2026-09-04T10:00:00Z",
        );
        assert!(failed.is_err());
        assert_eq!(doc.metadata.get("city"), None);
        assert_eq!(doc.metadata.history.len(), 1);
        assert_eq!(doc.metadata.latest_rev(), 1);
        // Idempotent re-application: no change, no history entry.
        assert!(!doc
            .apply_metadata_draft(
                &draft_fields(&[("title", "Start")]),
                "manual",
                "2026-09-04T11:00:00Z",
            )
            .unwrap());
        assert_eq!(doc.metadata.history.len(), 1);
        // Empty call: no change, no entry.
        assert!(!doc
            .apply_metadata_draft(&BTreeMap::new(), "manual", "2026-09-04T11:00:00Z")
            .unwrap());
        assert_eq!(doc.metadata.history.len(), 1);
        // A real change records exactly one entry with the affected IDs.
        assert!(doc
            .apply_metadata_draft(
                &draft_fields(&[("title", "Ziel"), ("city", "Berlin")]),
                "cli",
                "2026-09-04T12:00:00Z",
            )
            .unwrap());
        assert_eq!(doc.metadata.history.len(), 2);
        let entry = &doc.metadata.history[0];
        assert_eq!(entry.rev, 2);
        assert_eq!(entry.origin, "cli");
        assert_eq!(entry.timestamp, "2026-09-04T12:00:00Z");
        assert_eq!(entry.changed, vec!["city".to_string(), "title".to_string()]);
    }

    #[test]
    fn iptc_s1_empty_value_removes_field() {
        let mut doc = metadata_doc();
        doc.apply_metadata_draft(
            &draft_fields(&[("title", "Start"), ("city", "Berlin")]),
            "manual",
            "2026-09-04T09:00:00Z",
        )
        .unwrap();
        // Empty and whitespace-only both remove; unaffected fields are not
        // listed in `changed`.
        assert!(doc
            .apply_metadata_draft(
                &draft_fields(&[("title", ""), ("city", "   ")]),
                "gui",
                "2026-09-04T10:00:00Z",
            )
            .unwrap());
        assert!(doc.metadata.draft.is_empty());
        assert_eq!(
            doc.metadata.history[0].changed,
            vec!["city".to_string(), "title".to_string()]
        );
        // Removing an absent field is an idempotent no-op (no entry).
        assert!(!doc
            .apply_metadata_draft(
                &draft_fields(&[("title", "")]),
                "gui",
                "2026-09-04T11:00:00Z",
            )
            .unwrap());
        assert_eq!(doc.metadata.history.len(), 2);
    }

    #[test]
    fn iptc_s1_clear_semantics() {
        let mut doc = metadata_doc();
        // Clearing an empty draft touches neither draft nor history.
        assert!(!doc
            .clear_metadata_draft("manual", "2026-09-04T09:00:00Z")
            .unwrap());
        assert!(doc.metadata.history.is_empty());
        assert!(!doc
            .clear_metadata_fields(&["title"], "manual", "2026-09-04T09:00:00Z")
            .unwrap());
        assert!(doc.metadata.history.is_empty());
        doc.apply_metadata_draft(
            &draft_fields(&[("title", "Start"), ("city", "Berlin")]),
            "manual",
            "2026-09-04T09:00:00Z",
        )
        .unwrap();
        // Selective clear: one entry, history kept.
        assert!(doc
            .clear_metadata_fields(&["title", "headline"], "mcp", "2026-09-04T10:00:00Z")
            .unwrap());
        assert_eq!(doc.metadata.get("title"), None);
        assert_eq!(doc.metadata.get("city"), Some("Berlin"));
        assert_eq!(doc.metadata.history[0].changed, vec!["title".to_string()]);
        assert_eq!(doc.metadata.history[0].origin, "mcp");
        // `--all`: draft emptied, history kept and extended.
        assert!(doc
            .clear_metadata_draft("cli", "2026-09-04T11:00:00Z")
            .unwrap());
        assert!(doc.metadata.draft.is_empty());
        assert_eq!(doc.metadata.history.len(), 3);
        assert_eq!(doc.metadata.history[0].changed, vec!["city".to_string()]);
        // History clear is explicit and total; draft values survive it.
        doc.apply_metadata_draft(
            &draft_fields(&[("title", "Neu")]),
            "manual",
            "2026-09-04T12:00:00Z",
        )
        .unwrap();
        doc.clear_metadata_history();
        assert!(doc.metadata.history.is_empty());
        assert_eq!(doc.metadata.get("title"), Some("Neu"));
        doc.clear_metadata_history();
        assert!(doc.metadata.history.is_empty());
        // Unknown IDs fail loudly on the clear paths too.
        assert!(doc
            .clear_metadata_fields(&["mystery"], "manual", "2026-09-04T13:00:00Z")
            .is_err());
        assert!(doc.metadata.history.is_empty());
    }

    #[test]
    fn iptc_s1_history_cap_and_rev_monotonic() {
        let mut doc = metadata_doc();
        for index in 1..=(MAX_METADATA_HISTORY_ENTRIES + 5) {
            let value = format!("Titel {index}");
            assert!(doc
                .apply_metadata_draft(
                    &draft_fields(&[("title", value.as_str())]),
                    "manual",
                    "2026-09-04T10:00:00Z",
                )
                .unwrap());
        }
        // Cap is enforced FIFO-deterministically: oldest fall off the end.
        assert_eq!(doc.metadata.history.len(), MAX_METADATA_HISTORY_ENTRIES);
        assert_eq!(
            doc.metadata.history[0].rev as usize,
            MAX_METADATA_HISTORY_ENTRIES + 5
        );
        assert_eq!(doc.metadata.history[0].changed, vec!["title".to_string()]);
        assert_eq!(
            doc.metadata.latest_rev() as usize,
            MAX_METADATA_HISTORY_ENTRIES + 5
        );
        let last = doc.metadata.history.last().unwrap();
        assert_eq!(last.rev, 6);
        let mut previous = u64::MAX;
        for entry in &doc.metadata.history {
            assert!(entry.rev < previous, "revs must strictly decrease");
            previous = entry.rev;
        }
        assert_eq!(
            doc.metadata.get("title"),
            Some(format!("Titel {}", MAX_METADATA_HISTORY_ENTRIES + 5).as_str())
        );
        doc.validate().unwrap();
        // Hand-crafted violations fail loudly instead of being normalized.
        let mut ascending = metadata_doc();
        ascending.metadata.history = vec![
            MetadataHistoryEntry {
                rev: 1,
                timestamp: "2026-09-04T09:00:00Z".into(),
                origin: "manual".into(),
                changed: vec!["title".into()],
            },
            MetadataHistoryEntry {
                rev: 2,
                timestamp: "2026-09-04T10:00:00Z".into(),
                origin: "manual".into(),
                changed: vec!["title".into()],
            },
        ];
        assert!(ascending.validate().is_err());
        let mut overlong = metadata_doc();
        overlong.metadata.history = (1..=(MAX_METADATA_HISTORY_ENTRIES + 1) as u64)
            .rev()
            .map(|rev| MetadataHistoryEntry {
                rev,
                timestamp: "2026-09-04T10:00:00Z".into(),
                origin: "manual".into(),
                changed: vec!["title".into()],
            })
            .collect();
        assert!(overlong.validate().is_err());
        let mut bad_changed = metadata_doc();
        bad_changed.metadata.history = vec![MetadataHistoryEntry {
            rev: 1,
            timestamp: "2026-09-04T10:00:00Z".into(),
            origin: "manual".into(),
            changed: vec!["mystery".into()],
        }];
        assert!(bad_changed.validate().is_err());
        bad_changed.metadata.history[0].changed = vec![];
        assert!(bad_changed.validate().is_err());
        bad_changed.metadata.history[0].changed = vec!["title".into(), "title".into()];
        assert!(bad_changed.validate().is_err());
        // `keywords` is a legal `changed` ID (sync carries it); rev 0 is not.
        bad_changed.metadata.history[0].changed = vec!["keywords".into(), "title".into()];
        bad_changed.validate().unwrap();
        bad_changed.metadata.history[0].rev = 0;
        assert!(bad_changed.validate().is_err());
    }

    #[test]
    fn iptc_s1_cas_conflict_on_concurrent_metadata_edit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("IMG_0001.ARW.lumina.json");
        let mut doc = metadata_doc();
        doc.apply_metadata_draft(
            &draft_fields(&[("title", "Start")]),
            "manual",
            "2026-09-04T09:00:00Z",
        )
        .unwrap();
        save_sidecar(&path, &doc).unwrap();
        let stale_revision = document_revision(&load_sidecar(&path).unwrap()).unwrap();
        // A concurrent writer moves the file forward...
        let mut concurrent = load_sidecar(&path).unwrap();
        concurrent
            .apply_metadata_draft(
                &draft_fields(&[("city", "Berlin")]),
                "sync:other",
                "2026-09-04T10:00:00Z",
            )
            .unwrap();
        save_sidecar(&path, &concurrent).unwrap();
        // ...so the stale revision conflicts loudly instead of last-write-wins.
        let mut stale = load_sidecar(&path).unwrap();
        stale
            .apply_metadata_draft(
                &draft_fields(&[("title", "Stale")]),
                "manual",
                "2026-09-04T11:00:00Z",
            )
            .unwrap();
        let conflict = save_sidecar_if_unchanged(&path, &stale, Some(&stale_revision));
        assert!(
            matches!(&conflict, Err(SidecarError::Conflict(_))),
            "stale CAS save must conflict, got {conflict:?}"
        );
        // The concurrent edit survived; the stale one was not persisted.
        let current = load_sidecar(&path).unwrap();
        assert_eq!(current.metadata.get("city"), Some("Berlin"));
        assert_eq!(current.metadata.get("title"), Some("Start"));
    }

    #[test]
    fn iptc_s1_mutations_leave_recipes_masks_and_copy_history_untouched() {
        let mut doc = metadata_doc();
        doc.keywords = vec!["alps".into()];
        let recipe_before = serde_json::to_value(&doc.virtual_copies[0].recipe).unwrap();
        let history_before = doc.virtual_copies[0].history.clone();
        doc.apply_metadata_draft(
            &draft_fields(&[("title", "Start"), ("date_created", "2026-09-04")]),
            "preset:veranstaltung",
            "2026-09-04T10:00:00Z",
        )
        .unwrap();
        doc.clear_metadata_fields(&["title"], "sync:copy-7", "2026-09-04T11:00:00Z")
            .unwrap();
        doc.clear_metadata_draft("gui", "2026-09-04T12:00:00Z")
            .unwrap();
        doc.clear_metadata_history();
        assert_eq!(
            serde_json::to_value(&doc.virtual_copies[0].recipe).unwrap(),
            recipe_before
        );
        assert_eq!(doc.virtual_copies[0].history, history_before);
        assert!(doc.virtual_copies[0].mask_layers.is_empty());
        assert!(doc.virtual_copies[0].mask_library.is_empty());
        assert_eq!(doc.keywords, vec!["alps".to_string()]);
        doc.validate().unwrap();
    }
}
