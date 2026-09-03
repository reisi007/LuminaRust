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
    append_generative_canvas, append_repair_region, append_spot_heal_generative, load_zdata,
    save_zdata, zdata_path_for, GenerativeCanvasArtifact, MaskTile, RecordKind, RecordSpec,
    RepairRegionArtifact, SpotHealGenerativeArtifact, ZDataContainer, ZDataError,
};

pub const FORMAT: &str = "lumina-sidecar";
pub const SCHEMA_VERSION: u32 = 2;

/// F-042-N1: current schema version for a persisted source-action spec. This is
/// independent of `SCHEMA_VERSION`; an unknown source-action `version` is
/// rejected during validation rather than silently ignored.
pub const SOURCE_ACTION_VERSION: u16 = 1;
pub const MAX_SIDECAR_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_VIRTUAL_COPIES: usize = 10_000;

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

    /// Eager bundle status for this link: `Available` only if the referenced
    /// bundle file exists and (for `LUMZDATA` containers, with the `zdata`
    /// feature on native) parses with intact BLAKE3 checksums; `Missing` when
    /// absent, `Corrupt` when present but unusable. Callers must treat any
    /// non-`Available` status as visible, never as a silent fallback.
    pub fn artifact_status(&self, bundle_root: &Path) -> ArtifactStatus {
        artifact_status(bundle_root, &self.as_artifact_reference())
    }
}

/// GEN-ZDATA-LINK-1: current schema version for a persisted spot-removal
/// spec. Independent of `SCHEMA_VERSION`; an unknown `version` is rejected
/// during validation rather than silently ignored.
pub const SPOT_REMOVAL_VERSION: u8 = 1;

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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaskLayer {
    pub id: String,
    pub mask: MaskReference,
    pub inverted: bool,
    pub feather: f32,
    pub blur: f32,
    pub density: f32,
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
}
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
    /// Pre-MVP schema decision: these optional fields are additive in schema v2;
    /// absent values remain identity and require no migration.
    pub color_grading: Option<ColorGrading>,
    pub presence: Option<Presence>,
    pub noise_reduction: Option<NoiseReduction>,
    pub sharpening: Option<Sharpening>,
    /// Optional top-level geometric transform. Absent is the identity.
    pub geometry: Option<Geometry>,
    /// Optional F-098 lens model, additive in schema v2.
    pub lens_correction: Option<LensCorrection>,
    /// Optional F-099 perspective model, additive in schema v2.
    pub perspective: Option<Perspective>,
    /// Optional F-097 stylistic effects (vignette + grain). Additive in schema
    /// v2; absent is no effects and requires no migration. Serialized into the
    /// root map (like `geometry`) so both effect objects flow into the
    /// `recipe_hash`/`RenderKey` and invalidate preview/export.
    pub effects: Option<Effects>,
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
        if let Some(sharpening) = &self.sharpening {
            adjustment.insert(
                "sharpening".into(),
                serde_json::to_value(sharpening).map_err(serde::ser::Error::custom)?,
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
        if let Some(effects) = &self.effects {
            root.insert(
                "effects".into(),
                serde_json::to_value(effects).map_err(serde::ser::Error::custom)?,
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
        let mut color_grading = None;
        let mut presence = None;
        let mut noise_reduction = None;
        let mut sharpening = None;
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
        let effects = root
            .remove("effects")
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
            if let Some(value) = object.remove("sharpening") {
                sharpening = Some(serde_json::from_value(value).map_err(serde::de::Error::custom)?);
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
            color_grading,
            presence,
            noise_reduction,
            sharpening,
            geometry,
            lens_correction,
            perspective,
            effects,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorGrading {
    pub version: u8,
    pub shadows: ColorGradingRange,
    pub midtones: ColorGradingRange,
    pub highlights: ColorGradingRange,
    pub balance: f32,
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
            color_grading: None,
            presence: None,
            noise_reduction: None,
            sharpening: None,
            geometry: None,
            lens_correction: None,
            perspective: None,
            effects: None,
            source_actions: Vec::new(),
            spot_removals: Vec::new(),
            generative_edit: None,
            options: BTreeMap::new(),
            auto_features: AutoFeatures::default(),
            extras: Extras::new(),
        }
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
                color_grading: None,
                presence: None,
                noise_reduction: None,
                sharpening: None,
                geometry: None,
                lens_correction: None,
                perspective: None,
                effects: None,
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
            }],
            history: vec![HistoryEntry {
                id: "h".into(),
                recipe: EditRecipe {
                    recipe_version: "1".into(),
                    adjustments: BTreeMap::from([("contrast".into(), -0.4)]),
                    curves: None,
                    hsl: None,
                    color_grading: None,
                    presence: None,
                    noise_reduction: None,
                    sharpening: None,
                    geometry: None,
                    lens_correction: None,
                    perspective: None,
                    effects: None,
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
                color_grading: None,
                presence: None,
                noise_reduction: None,
                sharpening: None,
                geometry: None,
                lens_correction: None,
                perspective: None,
                effects: None,
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
                },
                midtones: ColorGradingRange {
                    hue_degrees: 120.0,
                    saturation: 0.25,
                },
                highlights: ColorGradingRange {
                    hue_degrees: 240.0,
                    saturation: 0.75,
                },
                balance: -0.2,
            }),
            ..Default::default()
        };
        let value = serde_json::to_value(&recipe).unwrap();
        assert!(value["adjustments"]["color_grading"].is_object());
        assert_eq!(recipe, serde_json::from_value(value).unwrap());
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
}
