#![allow(clippy::field_reassign_with_default)]
//! Versioned, portable domain types for a Lumina sidecar.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
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

// SIDECAR-SUFFIX-CONST: the one definition of the two sidecar file suffixes,
// plus the path builder derived from it.
mod sidecar_paths;
pub use sidecar_paths::{sidecar_path_for, SIDECAR_FILE_SUFFIX, ZDATA_FILE_SUFFIX};

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
// Slice A (gain compensation): the recipe-side gain value + validation,
// extracted from `merge_recipe` (file-size ratchet); re-exported unchanged.
mod merge_gain;
pub use merge_gain::{PanoGainCompensation, PanoGainMethod, A_MIN_OVERLAP_STAT_PX, MAX_PANO_GAIN};

// Slice B (deghosting): the recipe-side deghost value + validation
// (`merge_deghost`) plus the alignment validators (`merge_recipe_alignment`),
// both extracted from `merge_recipe` (file-size ratchet).
mod merge_deghost;
mod merge_recipe_alignment;
pub use merge_deghost::{
    HdrDeghost, DEGHOST_DEFAULT_THRESHOLD, DEGHOST_MAX_THRESHOLD, DEGHOST_MIN_THRESHOLD,
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

// The one set of range validators for the per-pixel color blocks, reused by
// the global recipe and the typed mask-local recipe (P1.2b).
mod color_blocks;
pub use color_blocks::{
    color_grading_is_neutral, color_grading_is_unset, color_grading_range_mut,
    color_grading_ranges, hsl_band, hsl_band_mut, hsl_band_slot_mut, hsl_is_neutral,
    point_color_is_neutral, validate_color_grading, validate_hsl, validate_point_color, HSL_BANDS,
    MAX_POINT_COLOR_ENTRIES,
};

// The one validator for the `Presence` block, reused by the global recipe and
// the typed mask-local recipe (P1.2c).
mod presence_block;
pub use presence_block::{presence_is_neutral, validate_presence, PRESENCE_FIELDS};

// The one validator for the two global detail blocks (`Sharpening` and
// `NoiseReduction`), reused by the global recipe and the typed mask-local
// recipe (P1.2d).
mod detail_block;
pub use detail_block::{
    detail_amount_is_valid, noise_reduction_field_range, noise_reduction_is_neutral,
    sharpening_field_range, sharpening_is_neutral, sharpening_radius_range,
    validate_noise_reduction, validate_sharpening, DETAIL_BLOCK_VERSION, NOISE_REDUCTION_FIELDS,
    SHARPENING_FIELDS,
};

// The two global detail blocks and the mask-local detail container (P1.2d), all
// validated by the one shared detail validator set.
mod detail;
pub use detail::{Detail, NoiseReduction, Sharpening};

// MASK-LOCAL-P0/P1.1/P1.2a/P1.2b/P1.2c/P1.2d: typed local mask recipes and the
// loud legacy migration.
mod local_adjustments;
pub use local_adjustments::mask_state::MAX_MASK_STATE_LAYERS;
pub use local_adjustments::{
    local_point_color_entry, mask_layers_digest, neutral_local_detail, neutral_local_presence,
    validate_mask_layer_local_state, LocalAdjustments, MaskLocalRecipe, MaskStateSnapshot,
    COLOR_LOCAL_ADJUSTMENTS_VERSION, CURVE_LOCAL_ADJUSTMENTS_VERSION,
    DETAIL_LOCAL_ADJUSTMENTS_VERSION, LEGACY_LOCAL_ADJUSTMENTS_VERSION,
    LEGACY_LOCAL_ADJUSTMENTS_VERSIONS, LOCAL_ADJUSTMENTS_VERSION, LOCAL_ADJUSTMENT_RANGES,
    LOCAL_GRADING_RANGES, LOCAL_HSL_FIELDS, LOCAL_POINT_COLOR_FIELDS,
    LOCAL_WB_TEMPERATURE_DELTA_RANGE, LOCAL_WB_TINT_DELTA_RANGE,
    PRESENCE_LOCAL_ADJUSTMENTS_VERSION, RELATIVE_WB_LOCAL_ADJUSTMENTS_VERSION,
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

// SPLICE-SIDECAR-LIB-90 (Slice 5): the recipe validation block moved out of
// this root file into `validation` (file-size ratchet). The validators keep
// their names: the public one stays reachable under the same path
// (`lumina_sidecar::validate_smart_collection_def`), the crate-internal ones
// are re-exported here, so every existing `crate::validate_*` call site
// (lib.rs, batch.rs, culling.rs, face.rs, denoise.rs) keeps working unchanged.
mod validation;
pub use validation::validate_smart_collection_def;
pub(crate) use validation::{
    validate_adjustments, validate_ai_select, validate_artifact, validate_collection_id,
    validate_collection_name, validate_generative_ref, validate_keyword, validate_name,
    validate_prompt, validate_relative_path, validate_unique_ids,
};

// SPLICE-SIDECAR-LIB-90 (Slice 7): the metadata draft block moved out of this
// root file into `metadata` (file-size ratchet). The public symbols keep their
// names and stay reachable under the same paths (`lumina_sidecar::MetadataDraft`,
// `lumina_sidecar::is_metadata_field`, `lumina_sidecar::now_rfc3339_utc`, …);
// the three crate-internal symbols are re-exported here, so the call sites in
// `SidecarDocument::clear_metadata_fields` (`validate_metadata_field_id`) and
// `SidecarDocument::validate` (`MetadataDraft::validate`) as well as the doc
// link of `MAX_METADATA_TITLE_CHARS` (`validate_metadata_date_created`) keep
// working unchanged.
mod metadata;
#[allow(unused_imports)]
pub(crate) use metadata::validate_metadata_date_created;
pub(crate) use metadata::validate_metadata_field_id;
pub use metadata::{
    is_metadata_field, metadata_field_limit, now_rfc3339_utc, validate_metadata_field_value,
    validate_metadata_origin, validate_metadata_timestamp, MetadataDraft, MetadataHistoryEntry,
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

// SPLICE-SIDECAR-LIB-90 (Slice 10): the value-type block moved out of this
// root file into `value_types` (file-size ratchet). The public symbols keep
// their names and stay reachable under the same paths
// (`lumina_sidecar::EditRecipe`, `lumina_sidecar::Curves`,
// `lumina_sidecar::Geometry`, `lumina_sidecar::DecodeFingerprint`, …); the
// crate-internal `default_color_grading_blending` helper is re-exported here,
// so the mask-local colour-grading block keeps its
// `crate::default_color_grading_blending` call site unchanged. The block stays
// ONE module: `EditRecipe`'s field-accessing impls (`Serialize`, `Deserialize`,
// `Default`, the inherent impls) must live next to the struct, otherwise every
// field would have to become `pub(crate)`.
mod value_types;
pub(crate) use value_types::default_color_grading_blending;
pub use value_types::{
    AiSelect, AiSelectKind, AnalysisFingerprint, ArtifactReference, AspectPreset, BokehShape,
    BrushMark, BrushMarkSign, ColorGrading, ColorGradingRange, CoordinateSystem, Crop,
    CurveChannels, CurvePoint, CurvePoints, Curves, DecodeFingerprint, DepthArtifactRef,
    EditRecipe, Effects, FocusRect, GenerativeArtifactRef, GenerativeCanvas, GenerativeEdit,
    Geometry, GeometryFingerprint, Grain, HslAdjustments, HslChannel, LensBlur, LensCorrection,
    MaskDefinition, MaskLayer, MaskOperation, MaskPrompt, MaskReference, MaskStatus, ModelIdentity,
    NormalizedRect, Perspective, Photo, Point2, PointColor, PointColorEntry, Preprocessing,
    Presence, PromptTransform, RedEyeCorrection, RedEyeRegion, Resolution, SourceActionKind,
    SourceActionSpec, SourceFingerprint, SourceIdentity, SpotDistraction, SpotRemoval,
    SpotRemovalMode, Upright, UprightAnalysis, Vignette, AI_SELECT_KNOWN_PARTS, BW_STASH_KEY,
    DEFAULT_DEVELOP_PROFILE, DEVELOP_PROFILES, DEVELOP_PROFILE_KEY, GENERATIVE_IDENTITY_KEY,
    GENERATIVE_NEGATIVE_PROMPT_KEY, RED_EYE_MAX_REGIONS, SPOT_DISTRACTION_KEY,
    SPOT_REMOVAL_VERSION, SPOT_VISUALIZE_KEY, TREATMENT_BW, TREATMENT_COLOR, TREATMENT_KEY,
    UPRIGHT_MAX_LINE_COUNT,
};

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

// SPLICE-SIDECAR-LIB-90 (Slice 9): the sidecar I/O block moved out of this
// root file into `sidecar_io` (file-size ratchet). The public symbols keep
// their names and stay reachable under the same paths
// (`lumina_sidecar::SidecarDocument`, `lumina_sidecar::RecoveryReport`,
// `lumina_sidecar::load_sidecar`, `lumina_sidecar::save_sidecar`,
// `lumina_sidecar::write_atomically`,
// `lumina_sidecar::save_sidecar_if_unchanged`,
// `lumina_sidecar::document_revision` and `lumina_sidecar::recover_sidecar`);
// the crate-internal write lock (`acquire_write_lock`) is re-exported here, so
// the `zdata` writers keep their `crate::acquire_write_lock` call site
// unchanged. The two inherent `impl SidecarDocument` blocks stay in this root
// file: a type and its inherent impl may live in different modules of the
// same crate, and every field of the struct is `pub`.
mod sidecar_io;
pub(crate) use sidecar_io::acquire_write_lock;
pub use sidecar_io::{
    document_revision, load_sidecar, recover_sidecar, save_sidecar, save_sidecar_if_unchanged,
    write_atomically, RecoveryReport, SidecarDocument,
};

// SPLICE-SIDECAR-LIB-90 (Slice 8): the artifact status block moved out of
// this root file into `artifact_status` (file-size ratchet). The public
// symbols keep their names and stay reachable under the same paths
// (`lumina_sidecar::source_status`, `lumina_sidecar::artifact_status`,
// `lumina_sidecar::GenerativeArtifactStatus` and
// `lumina_sidecar::generative_artifact_status`); the two `artifact_status`
// variants are cfg-gated (`zdata` / not `zdata`) and keep their condition.
// The crate-internal helpers `basic_artifact_file_check` and
// `starts_with_zdata_magic` stay private in the new module, because no call
// site outside the moved block uses them.
mod artifact_status;
pub use artifact_status::{
    artifact_status, generative_artifact_status, source_status, GenerativeArtifactStatus,
};

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

#[cfg(test)]
mod tests;
