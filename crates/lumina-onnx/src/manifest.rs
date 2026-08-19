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
}
