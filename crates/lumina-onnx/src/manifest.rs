//! Model manifest and capabilities (F-080).
//!
//! [`ModelManifest`] is the serializable identity + I/O contract of an ONNX
//! model; [`ModelCapabilities`] enumerates what a model can do. At least one
//! capability must be set, and unknown fields are rejected, so a manifest can
//! never silently claim a capability it does not declare.

use crate::OnnxError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Channel layout of the model input tensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelLayout {
    /// 3-channel RGB (no alpha).
    Rgb,
}

/// Memory layout of the model input tensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TensorFormat {
    /// Batch, channels, height, width.
    Nchw,
    /// Batch, height, width, channels.
    Nhwc,
}

/// Model input resolution in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

/// Input tensor specification for an ONNX model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelInputSpec {
    /// Inference (preprocessing) resolution.
    pub resolution: Resolution,
    /// Channel layout of the RGB input.
    pub channel_layout: ChannelLayout,
    /// Name of the input tensor in the ONNX graph.
    pub tensor_name: String,
    /// Tensor memory layout expected by the model.
    pub tensor_format: TensorFormat,
}

/// Model capabilities (F-080).
///
/// `subject_segmentation` is the *base* capability (automatic subject
/// segmentation, e.g. BiRefNet) with no prompts. The remaining five flags are
/// the interactive/advanced capabilities enumerated in F-080. **At least one
/// capability must be set** — see [`ModelCapabilities::validate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCapabilities {
    /// Automatic subject segmentation (no prompts required).
    pub subject_segmentation: bool,
    /// Accepts a bounding-box prompt.
    pub box_prompt: bool,
    /// Accepts point (click) prompts.
    pub point_prompt: bool,
    /// Accepts a mask prompt.
    pub mask_prompt: bool,
    /// Produces a class label / detection.
    pub class_detection: bool,
    /// Produces per-instance segmentations.
    pub instance_segmentation: bool,
}

impl ModelCapabilities {
    /// Whether any capability is declared.
    pub fn any(&self) -> bool {
        self.subject_segmentation
            || self.box_prompt
            || self.point_prompt
            || self.mask_prompt
            || self.class_detection
            || self.instance_segmentation
    }

    /// Validate that at least one capability is set.
    pub fn validate(&self) -> Result<(), OnnxError> {
        self.validate_for("model")
    }

    pub(crate) fn validate_for(&self, name: &str) -> Result<(), OnnxError> {
        if !self.any() {
            return Err(OnnxError::UnsupportedModel {
                name: name.to_owned(),
                reason: "no model capabilities declared (at least one of subject_segmentation, \
                     box_prompt, point_prompt, mask_prompt, class_detection, \
                     instance_segmentation must be true)"
                    .into(),
            });
        }
        Ok(())
    }
}

/// Unchecked deserialization form. Deserializing into this struct rejects
/// unknown fields; [`TryFrom`] then enforces the capability invariant so a
/// manifest with no capabilities set is rejected at construction time.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelManifestUnchecked {
    model_name: String,
    model_version: String,
    model_hash: String,
    license: String,
    input: ModelInputSpec,
    capabilities: ModelCapabilities,
}

/// Serializable ONNX model identity + I/O contract.
///
/// The model hash is documented as the identity artifact (F-080 / ai-masks
/// mask identity). It is intentionally a plain string here; the mapping to the
/// sidecar `ModelIdentity` happens in F-048.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelManifest {
    /// Human/model registry name, e.g. `BiRefNet`.
    pub model_name: String,
    /// Model version (semver-ish string).
    pub model_version: String,
    /// Content/artifact hash identifying the exact weights.
    pub model_hash: String,
    /// Model license, e.g. `Apache-2.0`.
    pub license: String,
    /// Input tensor specification.
    pub input: ModelInputSpec,
    /// Declared model capabilities.
    pub capabilities: ModelCapabilities,
}

impl ModelManifest {
    /// Validate the manifest (currently the capability invariant).
    pub fn validate(&self) -> Result<(), OnnxError> {
        self.capabilities.validate_for(&self.model_name)
    }

    /// Map this manifest's identity onto the sidecar [`ModelIdentity`] used by
    /// the mask-loading decision layer (F-048 / F-051) for stale-detection.
    pub fn to_model_identity(&self) -> lumina_sidecar::ModelIdentity {
        lumina_sidecar::ModelIdentity {
            name: self.model_name.clone(),
            version: self.model_version.clone(),
            hash: self.model_hash.clone(),
            extras: BTreeMap::new(),
        }
    }

    /// Serialize to JSON.
    pub fn to_json(&self) -> Result<String, OnnxError> {
        serde_json::to_string(self).map_err(|e| OnnxError::InvalidManifest(e.to_string()))
    }

    /// Deserialize from JSON, rejecting unknown fields and enforcing the
    /// capability invariant.
    pub fn from_json(json: &str) -> Result<Self, OnnxError> {
        let unchecked: ModelManifestUnchecked =
            serde_json::from_str(json).map_err(|e| OnnxError::InvalidManifest(e.to_string()))?;
        unchecked.try_into()
    }
}

impl TryFrom<ModelManifestUnchecked> for ModelManifest {
    type Error = OnnxError;

    fn try_from(raw: ModelManifestUnchecked) -> Result<Self, Self::Error> {
        raw.capabilities.validate_for(&raw.model_name)?;
        Ok(Self {
            model_name: raw.model_name,
            model_version: raw.model_version,
            model_hash: raw.model_hash,
            license: raw.license,
            input: raw.input,
            capabilities: raw.capabilities,
        })
    }
}

/// Inference resolution width for BiRefNet.
pub const BIREFNET_INFERENCE_WIDTH: u32 = 1024;
/// Inference resolution height for BiRefNet.
pub const BIREFNET_INFERENCE_HEIGHT: u32 = 1024;

/// Inference resolution width for the SAM 2.1 `hiera_*` model family.
pub const SAM2_INFERENCE_WIDTH: u32 = 1024;
/// Inference resolution height for the SAM 2.1 `hiera_*` model family.
pub const SAM2_INFERENCE_HEIGHT: u32 = 1024;

/// SAM 2.1 release version string used for every `sam2.1_hiera_*` descriptor.
///
/// The family is the Meta "SAM 2.1" release (checkpoint family `092824`,
/// Apache-2.0). `model_hash` stays `pending-integration` until real,
/// hash-pinned ONNX fixtures are committed (no spontaneous downloads — see
/// `Agents.md`).
pub const SAM2_RELEASE_VERSION: &str = "2.1.0";

/// SAM 2.1 model-family variants (F-082).
///
/// The adapter selects one of these at runtime via [`select_variant`] /
/// [`DeviceProfile`] so the same code path serves the whole `hiera_*` family.
/// The exact `model_name` is persisted in the mask identity, keeping re-runs
/// reproducible regardless of which device class ran the original inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sam2Variant {
    /// `sam2.1_hiera_tiny` — lowest CPU load, fastest encoder (~38.9 M params).
    Tiny,
    /// `sam2.1_hiera_small` — small quality bump (~46.0 M params).
    Small,
    /// `sam2.1_hiera_base_plus` — Meta's standard balanced variant (~80.8 M).
    BasePlus,
    /// `sam2.1_hiera_large` — highest quality, high-end only (~224.4 M).
    Large,
}

impl Sam2Variant {
    /// All four variants, in ascending cost/quality order.
    pub const ALL: [Sam2Variant; 4] = [
        Sam2Variant::Tiny,
        Sam2Variant::Small,
        Sam2Variant::BasePlus,
        Sam2Variant::Large,
    ];

    /// Exact `model_name` persisted in the mask identity.
    pub fn model_name(self) -> &'static str {
        match self {
            Sam2Variant::Tiny => "sam2.1_hiera_tiny",
            Sam2Variant::Small => "sam2.1_hiera_small",
            Sam2Variant::BasePlus => "sam2.1_hiera_base_plus",
            Sam2Variant::Large => "sam2.1_hiera_large",
        }
    }

    /// Build the [`ModelManifest`] descriptor for this variant.
    pub fn manifest(self) -> ModelManifest {
        sam2_1_manifest(self)
    }
}

/// BiRefNet descriptor: the first automatic subject model.
///
/// - Automatic subject segmentation from a single RGB input to an alpha matte.
/// - No prompts: only `subject_segmentation` is true.
/// - Documented inference resolution 1024×1024 (square resize).
/// - License `Apache-2.0` (BiRefNet by Zheng et al., arXiv:2401.03407 — verified,
///   no download performed in this iteration). `model_hash` is a placeholder
///   (`pending-integration`) until real weights are provided in F-048.
pub fn birefnet_manifest() -> ModelManifest {
    ModelManifest {
        model_name: "BiRefNet".into(),
        model_version: "1.0.0".into(),
        model_hash: "pending-integration".into(),
        license: "Apache-2.0".into(),
        input: ModelInputSpec {
            resolution: Resolution {
                width: BIREFNET_INFERENCE_WIDTH,
                height: BIREFNET_INFERENCE_HEIGHT,
            },
            channel_layout: ChannelLayout::Rgb,
            tensor_name: "input".into(),
            tensor_format: TensorFormat::Nchw,
        },
        capabilities: ModelCapabilities {
            subject_segmentation: true,
            box_prompt: false,
            point_prompt: false,
            mask_prompt: false,
            class_detection: false,
            instance_segmentation: false,
        },
    }
}

/// SAM 2.1 descriptor for a single variant.
///
/// Every variant is an interactive segmentation model: it declares
/// `box_prompt`, `point_prompt` and `mask_prompt` (the three new interactive
/// capabilities) and **not** `subject_segmentation` (SAM 2 is prompted, not
/// automatic). All four share the documented 1024x1024 RGB NCHW encoder
/// contract with the image tensor named `images`.
///
/// `model_hash` is `pending-integration` (same placeholder format as
/// BiRefNet) until real, hash-pinned ONNX fixtures are committed. License is
/// `Apache-2.0` (facebookresearch/sam2, verified — no download in this
/// iteration).
pub fn sam2_1_manifest(variant: Sam2Variant) -> ModelManifest {
    ModelManifest {
        model_name: variant.model_name().into(),
        model_version: SAM2_RELEASE_VERSION.into(),
        model_hash: "pending-integration".into(),
        license: "Apache-2.0".into(),
        input: ModelInputSpec {
            resolution: Resolution {
                width: SAM2_INFERENCE_WIDTH,
                height: SAM2_INFERENCE_HEIGHT,
            },
            channel_layout: ChannelLayout::Rgb,
            tensor_name: "images".into(),
            tensor_format: TensorFormat::Nchw,
        },
        capabilities: ModelCapabilities {
            subject_segmentation: false,
            box_prompt: true,
            point_prompt: true,
            mask_prompt: true,
            class_detection: false,
            instance_segmentation: false,
        },
    }
}

/// All four SAM 2.1 variant descriptors, in ascending cost/quality order.
pub fn sam2_1_manifests() -> Vec<ModelManifest> {
    Sam2Variant::ALL.iter().map(|v| v.manifest()).collect()
}

/// Device capability profile used to pick a SAM 2.1 family variant at runtime
/// (F-082 dynamic variant selection).
///
/// The selected variant is **not** part of the mask identity; the identity
/// persists the exact `model_name`/`model_hash` of the variant actually used,
/// so re-runs are reproducible regardless of device class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceProfile {
    /// Logical CPU cores reported by the host.
    pub cores: u32,
    /// Explicit variant override; wins over the core-count heuristic.
    pub r#override: Option<Sam2Variant>,
}

impl DeviceProfile {
    /// Build a profile from an explicit core count and optional override.
    pub fn new(cores: u32, r#override: Option<Sam2Variant>) -> Self {
        Self { cores, r#override }
    }

    /// Profile from a core count only (no override).
    pub fn with_cores(cores: u32) -> Self {
        Self {
            cores,
            r#override: None,
        }
    }

    /// Profile with an explicit variant override (core count ignored).
    pub fn with_override(variant: Sam2Variant) -> Self {
        Self {
            cores: 0,
            r#override: Some(variant),
        }
    }

    /// Detect the host profile via [`std::thread::available_parallelism`]
    /// (no extra dependencies). On failure (cannot query the platform) it
    /// returns a conservative `cores = 0` profile, which maps to `tiny`.
    pub fn detect() -> Self {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(0);
        Self {
            cores,
            r#override: None,
        }
    }

    /// Select the SAM 2.1 variant for this profile (see [`select_variant`]).
    pub fn select_variant(&self) -> Sam2Variant {
        select_variant(self)
    }
}

/// Select the SAM 2.1 variant for a [`DeviceProfile`] using the documented,
/// deterministic thresholds (F-082 SOLL, start values, later benchmark-tuned):
///
/// | cores            | variant    |
/// | ---------------- | ---------- |
/// | `< 4`            | `tiny`     |
/// | `4 ..= 7`        | `small`    |
/// | `8 ..= 15`       | `base_plus`|
/// | `>= 16`          | `large`    |
///
/// An explicit `override` always wins, independent of `cores`.
pub fn select_variant(profile: &DeviceProfile) -> Sam2Variant {
    if let Some(variant) = profile.r#override {
        return variant;
    }
    match profile.cores {
        0..=3 => Sam2Variant::Tiny,
        4..=7 => Sam2Variant::Small,
        8..=15 => Sam2Variant::BasePlus,
        _ => Sam2Variant::Large,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn birefnet_has_only_subject_capability() {
        let m = birefnet_manifest();
        assert!(m.capabilities.subject_segmentation);
        assert!(!m.capabilities.box_prompt);
        assert!(!m.capabilities.point_prompt);
        assert!(!m.capabilities.mask_prompt);
        assert!(!m.capabilities.class_detection);
        assert!(!m.capabilities.instance_segmentation);
        assert!(m.validate().is_ok());
    }

    #[test]
    fn manifest_json_roundtrip_preserves_identity() {
        let m = birefnet_manifest();
        let json = m.to_json().unwrap();
        let back = ModelManifest::from_json(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn manifest_rejects_unknown_field() {
        let mut value = serde_json::to_value(birefnet_manifest()).unwrap();
        value["unknown_top_level"] = serde_json::json!(1);
        let err = ModelManifest::from_json(&value.to_string()).unwrap_err();
        assert!(
            err.to_string().contains("unknown field"),
            "expected unknown-field error, got {err}"
        );
    }

    #[test]
    fn no_capabilities_set_is_rejected() {
        let json = serde_json::json!({
            "model_name": "X",
            "model_version": "1",
            "model_hash": "h",
            "license": "MIT",
            "input": {
                "resolution": {"width": 4, "height": 4},
                "channel_layout": "rgb",
                "tensor_name": "input",
                "tensor_format": "nchw"
            },
            "capabilities": {
                "subject_segmentation": false,
                "box_prompt": false,
                "point_prompt": false,
                "mask_prompt": false,
                "class_detection": false,
                "instance_segmentation": false
            }
        })
        .to_string();
        let err = ModelManifest::from_json(&json).unwrap_err();
        assert!(
            matches!(err, OnnxError::UnsupportedModel { .. }),
            "expected UnsupportedModel, got {err:?}"
        );
    }

    #[test]
    fn capabilities_reject_unknown_field() {
        let json = serde_json::json!({
            "subject_segmentation": true,
            "box_prompt": false,
            "point_prompt": false,
            "mask_prompt": false,
            "class_detection": false,
            "instance_segmentation": false,
            "future_capability": true
        })
        .to_string();
        let err = serde_json::from_str::<ModelCapabilities>(&json).unwrap_err();
        assert!(
            err.to_string().contains("unknown field"),
            "expected unknown-field error, got {err}"
        );
    }

    #[test]
    fn capabilities_validate_directly() {
        assert!(ModelCapabilities::default().validate().is_err());
        let ok = ModelCapabilities {
            subject_segmentation: true,
            ..Default::default()
        };
        assert!(ok.validate().is_ok());
    }

    // F-083 #2 — every SAM 2.1 variant descriptor is well-formed.
    #[test]
    fn sam2_1_manifests_yield_four_valid_variants() {
        let manifests = sam2_1_manifests();
        assert_eq!(manifests.len(), 4);
        let names: Vec<&str> = manifests.iter().map(|m| m.model_name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "sam2.1_hiera_tiny",
                "sam2.1_hiera_small",
                "sam2.1_hiera_base_plus",
                "sam2.1_hiera_large",
            ]
        );
        for m in &manifests {
            // Three interactive capabilities declared.
            assert!(m.capabilities.box_prompt);
            assert!(m.capabilities.point_prompt);
            assert!(m.capabilities.mask_prompt);
            // Not automatic subject segmentation.
            assert!(!m.capabilities.subject_segmentation);
            // Valid input spec: 1024² RGB NCHW, encoder tensor named `images`.
            assert_eq!(m.input.resolution.width, SAM2_INFERENCE_WIDTH);
            assert_eq!(m.input.resolution.height, SAM2_INFERENCE_HEIGHT);
            assert_eq!(m.input.channel_layout, ChannelLayout::Rgb);
            assert_eq!(m.input.tensor_format, TensorFormat::Nchw);
            assert_eq!(m.input.tensor_name, "images");
            // model_hash uses the same placeholder format as BiRefNet.
            assert_eq!(m.model_hash, "pending-integration");
            assert_eq!(m.license, "Apache-2.0");
            assert!(m.validate().is_ok());
        }
    }

    #[test]
    fn sam2_1_manifest_matches_variant_name() {
        for v in Sam2Variant::ALL {
            let m = sam2_1_manifest(v);
            assert_eq!(m.model_name, v.model_name());
            assert_eq!(m.model_version, SAM2_RELEASE_VERSION);
        }
    }

    // F-083 #3 — deterministic variant selection with exact thresholds.
    #[test]
    fn select_variant_boundaries() {
        assert_eq!(
            select_variant(&DeviceProfile::with_cores(3)),
            Sam2Variant::Tiny
        );
        assert_eq!(
            select_variant(&DeviceProfile::with_cores(4)),
            Sam2Variant::Small
        );
        assert_eq!(
            select_variant(&DeviceProfile::with_cores(7)),
            Sam2Variant::Small
        );
        assert_eq!(
            select_variant(&DeviceProfile::with_cores(8)),
            Sam2Variant::BasePlus
        );
        assert_eq!(
            select_variant(&DeviceProfile::with_cores(15)),
            Sam2Variant::BasePlus
        );
        assert_eq!(
            select_variant(&DeviceProfile::with_cores(16)),
            Sam2Variant::Large
        );
        // Above the top threshold still maps to large.
        assert_eq!(
            select_variant(&DeviceProfile::with_cores(128)),
            Sam2Variant::Large
        );
    }

    #[test]
    fn select_variant_override_wins() {
        // Override beats the core-count heuristic at every threshold.
        assert_eq!(
            select_variant(&DeviceProfile::with_override(Sam2Variant::Large)),
            Sam2Variant::Large
        );
        assert_eq!(
            select_variant(&DeviceProfile::new(3, Some(Sam2Variant::Large))),
            Sam2Variant::Large,
            "override must win even on a tiny-core profile"
        );
        assert_eq!(
            select_variant(&DeviceProfile::new(128, Some(Sam2Variant::Tiny))),
            Sam2Variant::Tiny,
            "override must win even on a high-core profile"
        );
    }

    #[test]
    fn select_variant_zero_cores_is_tiny_fallback() {
        // The conservative fallback (cores == 0, as returned when
        // available_parallelism cannot be queried) maps to tiny.
        assert_eq!(
            select_variant(&DeviceProfile::with_cores(0)),
            Sam2Variant::Tiny
        );
    }

    #[test]
    fn detect_returns_a_valid_variant() {
        // detect() must not panic and must yield one of the four variants.
        let profile = DeviceProfile::detect();
        let variant = profile.select_variant();
        assert!(Sam2Variant::ALL.contains(&variant));
        // The method and free function agree.
        assert_eq!(profile.select_variant(), select_variant(&profile));
    }
}
