//! Mask, brush and prompt value types of the persisted recipe vocabulary.
//!
//! Pure move out of `crates/lumina-sidecar/src/value_types.rs` (file-size
//! ceiling): the items below are byte-identical to their former place; only
//! this module documentation and the `use` block are new. `value_types.rs`
//! re-exports every name, so `lumina_sidecar::MaskDefinition`,
//! `lumina_sidecar::MaskPrompt`, … keep their paths.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::{
    ArtifactReference, DecodeFingerprint, Extras, GeometryFingerprint, LocalAdjustments,
    ModelIdentity, Preprocessing, Resolution, SourceFingerprint,
};

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
