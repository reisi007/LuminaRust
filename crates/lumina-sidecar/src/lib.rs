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

mod brush_prompt;
mod source_action_refs;
#[cfg(feature = "zdata")]
pub use source_action_refs::load_validated_source_action_bundle;
pub use source_action_refs::{
    validate_source_action_bundle_reference, validate_source_action_spec, SourceActionArtifactRef,
};

// R5-DUST-23-FOLLOWUP: shared spot IDs, entry normalization and artifact
// status live in one small module so CLI and GUI cannot classify the same
// typed/extras operation differently.
mod spot;
pub use spot::{
    set_spot_removal_entries, spot_removal_entries, spot_removal_status,
    spot_removal_status_for_identity, stable_spot_id, SpotRemovalStatus, SPOT_REMOVAL_ID_PREFIX,
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

// UX-LOOK-HISTORY-18: structured edit-history steps (parameter/from/to) carried
// additively as typed extras; loud validation (see module docs).
mod history;
pub use history::{
    validate_history_entry, HistoryChange, HistoryEntry, HISTORY_CHANGES_KEY,
    HISTORY_MASK_STATE_KEY, MAX_HISTORY_CHANGES, MAX_HISTORY_CHANGE_PARAMETER_CHARS,
    MAX_HISTORY_CHANGE_VALUE_CHARS,
};

// Tone-curve channel accessors plus the one shared point-rule validator,
// reused by the global recipe and the typed mask-local recipe (P1.2a).
mod curves;
pub use curves::{
    curve_points_are_identity, identity_curve_points, validate_curves, CURVE_CHANNELS,
};

// MASK-LOCAL-P0/P1.1/P1.2a: typed local mask recipes and the loud legacy migration.
mod local_adjustments;
pub use local_adjustments::{
    mask_layers_digest, validate_mask_layer_local_state, LocalAdjustments, MaskLocalRecipe,
    MaskStateSnapshot, LEGACY_LOCAL_ADJUSTMENTS_VERSION, LEGACY_LOCAL_ADJUSTMENTS_VERSIONS,
    LOCAL_ADJUSTMENTS_VERSION, LOCAL_ADJUSTMENT_RANGES, LOCAL_WB_TEMPERATURE_DELTA_RANGE,
    LOCAL_WB_TINT_DELTA_RANGE, MAX_MASK_STATE_LAYERS, RELATIVE_WB_LOCAL_ADJUSTMENTS_VERSION,
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
    DenoiseArtifactRef, DenoiseModelIdentity, DENOISE_AI_VERSION, DENOISE_ARTIFACT_CHANNELS,
    DENOISE_ARTIFACT_DATA_VERSION, DENOISE_ARTIFACT_FORMAT_MARKER, DENOISE_ARTIFACT_KIND,
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

// G-15 META-MVP (Slice 1): the metadata batch-operation language. Moved out of
// this root file (file-size ratchet) together with LRPAR-G15-STACK-15.
mod batch;
pub use batch::{apply_batch_op, BatchOp};

// LRPAR-G15-STACK-15: source-level image-stack membership (sidecar-first,
// same-folder, additive optional `SidecarDocument::stack` section).
mod stack;
pub use stack::{
    validate_stack_id, validate_stack_member_name, StackMembership, MAX_STACK_ID_CHARS,
    MAX_STACK_MEMBERS, MAX_STACK_MEMBER_CHARS, STACK_SCHEMA_VERSION,
};

// LRPAR-G03-MASKGROUP-03: per-copy mask groups (Copy vs. Duplicate). Schema in
// `mask_group`, pure document operations/validation in `mask_group_ops`.
mod mask_group;
mod mask_group_ops;
pub use mask_group::{
    group_id_for_mask, mask_groups_of, set_mask_groups, validate_group_id, validate_group_name,
    MaskGroup, MASK_GROUPS_EXTRAS_KEY, MASK_GROUP_VERSION, MAX_MASK_GROUP_ID_CHARS,
    MAX_MASK_GROUP_MEMBERS, MAX_MASK_GROUP_NAME_CHARS,
};
pub use mask_group_ops::{
    apply_group_parameter_offsets, delete_mask_node, dissolve_group, group_masks,
    move_group_member, set_group_collapsed, validate_mask_groups, MaskDeletionOutcome,
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
    /// LRPAR-G15-STACK-15: optional source-level image-stack membership.
    /// Additive schema-v2 field: absent is the legitimate "not stacked" state
    /// and serializes back absent (legacy documents stay byte-stable).
    /// Shared by all virtual copies — a stack is per source image, never a
    /// recipe/mask/history mutation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<StackMembership>,
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
            stack: None,
            extras: Extras::new(),
        }
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
        let mut document: Self =
            serde_json::from_value(value).map_err(|e| SidecarError::Json(e.to_string()))?;
        document.normalize_legacy_local_adjustments()?;
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
        // LRPAR-G15-STACK-15: optional source-level image-stack membership.
        // Absent is the valid "not stacked" state; a present section is
        // validated loudly (version pin, bare same-folder member names,
        // sorted/unique members, cover ∈ members).
        if let Some(stack) = &self.stack {
            stack.validate()?;
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
                validate_mask_layer_local_state(layer)
                    .map_err(|error| SidecarError::Invalid(error.to_string()))?;
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
                validate_history_entry(entry)?;
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
        mask_group_ops::validate_mask_graph(self, &copy_ids)?;
        mask_group_ops::validate_mask_groups(self)
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> Result<(), SidecarError> {
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
pub(crate) fn validate_keyword(keyword: &str) -> Result<(), SidecarError> {
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
pub(crate) fn validate_collection_id(id: &str) -> Result<(), SidecarError> {
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

pub(crate) fn validate_collection_name(name: &str) -> Result<(), SidecarError> {
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
        MaskPrompt::Brush {
            marks, resolution, ..
        } => {
            if resolution.0 == 0 || resolution.1 == 0 {
                return invalid("prompt brush resolution width/height must be strictly positive");
            }
            brush_prompt::validate_brush_marks(marks)?;
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
    validate_name("spot_removal id", &spot.id)?;
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
/// typed `EditRecipe::spot_removals` holds only id/version/mode/artifact and is
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
    if let Some(id) = spot::duplicate_spot_id(entries) {
        return invalid(format!("duplicate spot_removal id `{id}`"));
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
            let id = object.get("id").and_then(Value::as_str).unwrap_or_default();
            if id.trim().is_empty() {
                return invalid("generative spot_removal requires a non-empty `id`");
            }
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
        validate_curves(c)?;
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
    let mut spot_ids = BTreeSet::new();
    for spot in &a.spot_removals {
        validate_spot_removal(spot)?;
        if !spot_ids.insert(spot.id.as_str()) {
            return invalid(format!("duplicate spot_removal id `{}`", spot.id));
        }
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

#[cfg(test)]
mod tests;
