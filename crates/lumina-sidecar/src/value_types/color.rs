//! Curves, colour-grading, presence, red-eye, geometry, lens, effects and crop
//! value types of the persisted recipe vocabulary.
//!
//! Pure move out of `crates/lumina-sidecar/src/value_types.rs` (file-size
//! ceiling): the items below are byte-identical to their former place; only
//! this module documentation and the `use` block are new. `value_types.rs`
//! re-exports every name, so `lumina_sidecar::Curves`, `lumina_sidecar::Geometry`,
//! … keep their paths. `default_color_grading_blending` keeps its
//! `pub(crate)` visibility and is re-exported by `value_types.rs`.

use serde::{Deserialize, Serialize};

use crate::color_blocks;
use crate::AnalysisFingerprint;

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
