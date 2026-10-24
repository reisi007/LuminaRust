#![allow(clippy::field_reassign_with_default)]
//! Versioned, portable domain types for a Lumina sidecar.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
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
// `crate::default_color_grading_blending` call site unchanged. That module is
// grouped into child modules (source/masks/generative/color), but `EditRecipe`
// and its field-accessing impls (`Serialize`, `Deserialize`, `Default`, the
// inherent impls) stay together in the root, so no field becomes `pub(crate)`.
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
// unchanged. The two inherent `impl SidecarDocument` blocks live in the child
// module `document` (declared below): a type and its inherent impl may live in
// different modules of the same crate, and every field of the struct is `pub`.
mod sidecar_io;
pub(crate) use sidecar_io::acquire_write_lock;
pub use sidecar_io::{
    acquire_sidecar_write_lock, document_revision, load_sidecar, recover_sidecar, save_sidecar,
    save_sidecar_if_unchanged, write_atomically, RecoveryReport, SidecarDocument, SidecarWriteLock,
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

// SPLICE-SIDECAR-LIB-90 (Slice 11): the two inherent `impl SidecarDocument`
// blocks moved out of this root file into `document` (file-size ratchet). The
// methods keep their own visibility; the fields of the struct are `pub`, so a
// child module can implement it while the root keeps the struct, its constants
// and the free functions.
mod document;

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

pub(crate) fn invalid(message: impl Into<String>) -> Result<(), SidecarError> {
    Err(SidecarError::Invalid(message.into()))
}

#[cfg(test)]
mod tests;
