//! SPLICE-SIDECAR-LIB-90 (Slice 10): the value-type block of the sidecar root
//! file, moved verbatim into its own module.
//!
//! One responsibility: the persisted recipe vocabulary — capture/decode
//! fingerprints, source actions, the mask/brush/generative prompt types, the
//! `EditRecipe` itself (with its hand-written `Serialize`/`Deserialize`), and
//! the colour, presence, geometry, lens, effects and crop blocks. These are the
//! types the core renderer and every front end share.
//!
//! `EditRecipe` and its field-accessing impls (`impl Serialize for
//! EditRecipe`, `impl Deserialize for EditRecipe`, `impl Default for
//! EditRecipe` and the two inherent `impl EditRecipe` blocks) deliberately
//! stay in **one** module: they all touch the struct's fields, so splitting
//! them across modules would force every field to become `pub(crate)`. One
//! module keeps the fields private and the type cohesive; the surrounding
//! vocabulary is grouped by subject in the child modules `source`, `masks`,
//! `generative` and `color`, which this module re-exports.
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

// The root's private modules and its own types. A child module does not
// inherit the parent's item names unqualified, so they are imported here.
use crate::invalid;
use crate::spot;
use crate::AutoFeatures;
use crate::DenoiseAi;
use crate::Extras;
use crate::NoiseReduction;
use crate::Sharpening;
use crate::SidecarError;

// The recipe vocabulary is grouped by subject in child modules (file-size
// ceiling); every name stays reachable through this module via the
// re-exports below. `EditRecipe` itself and all five of its impls stay in this
// file: they access the struct's fields, so a split would force every field to
// become `pub(crate)`.
mod color;
mod generative;
mod masks;
mod source;

pub(crate) use color::default_color_grading_blending;
pub use color::{
    AspectPreset, BokehShape, ColorGrading, ColorGradingRange, Crop, CurveChannels, CurvePoint,
    CurvePoints, Curves, DepthArtifactRef, Effects, FocusRect, Geometry, Grain, HslAdjustments,
    HslChannel, LensBlur, LensCorrection, Perspective, PointColor, PointColorEntry, Presence,
    RedEyeCorrection, RedEyeRegion, Upright, UprightAnalysis, Vignette, RED_EYE_MAX_REGIONS,
    UPRIGHT_MAX_LINE_COUNT,
};
pub use generative::{GenerativeCanvas, GenerativeEdit, GENERATIVE_NEGATIVE_PROMPT_KEY};
pub use masks::{
    AiSelect, AiSelectKind, BrushMark, BrushMarkSign, CoordinateSystem, MaskDefinition, MaskLayer,
    MaskOperation, MaskPrompt, MaskReference, MaskStatus, NormalizedRect, Point2, PromptTransform,
    AI_SELECT_KNOWN_PARTS,
};
pub use source::{
    AnalysisFingerprint, ArtifactReference, DecodeFingerprint, GenerativeArtifactRef,
    GeometryFingerprint, ModelIdentity, Photo, Preprocessing, Resolution, SourceActionKind,
    SourceActionSpec, SourceFingerprint, SourceIdentity, SpotDistraction, SpotRemoval,
    SpotRemovalMode, BW_STASH_KEY, DEFAULT_DEVELOP_PROFILE, DEVELOP_PROFILES, DEVELOP_PROFILE_KEY,
    GENERATIVE_IDENTITY_KEY, SPOT_DISTRACTION_KEY, SPOT_REMOVAL_VERSION, SPOT_VISUALIZE_KEY,
    TREATMENT_BW, TREATMENT_COLOR, TREATMENT_KEY,
};

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
