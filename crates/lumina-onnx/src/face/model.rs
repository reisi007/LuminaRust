//! Face model descriptors, the model suite and the identity-bearing inference
//! options (LRPAR-G12-FACE-20, S2; extracted from [`super`] by
//! LRPAR-G12-FACE-ADAPTER-25 so `face.rs` stays within the file-size ratchet).
//!
//! The shipped descriptors describe the **real** OpenCV Zoo graphs
//! (FACE-20-S6 pins, re-pinned for FACE-20-FACE-ADAPTER-25):
//!
//! * **YuNet** (`input` → twelve per-stride tensors `cls_/obj_/bbox_/kps_{8,16,32}`):
//!   raw `0..=255` **BGR** input, no ImageNet preprocessing (the training mean
//!   is folded into the first convolution). The detection head is decoded by
//!   [`super::yunet`].
//! * **SFace** (`data` → `fc1`, 128-d): raw `0..=255` **RGB** input, the graph
//!   bakes `(x − 127.5) · 1/128` internally.
//!
//! Both therefore declare [`InputNormalization::BYTE_RANGE`]; a manifest whose
//! declared preprocessing does not match what the adapter feeds would be a
//! silent correctness hole, so the values are pinned by digest
//! (`tests/face_pins.rs`).

use crate::hash::PENDING_INTEGRATION_HASH;
use crate::manifest::{
    ChannelLayout, InputNormalization, ModelCapabilities, ModelInputSpec, ModelManifest,
    Resolution as ModelResolution, TensorFormat,
};
use crate::OnnxError;

/// Detection inference resolution (square) — the fixed input shape of the
/// shipped `face_detection_yunet_2023mar.onnx` graph. Part of the face identity.
pub const FACE_DETECT_INFERENCE_WIDTH: u32 = 640;
/// Detection inference resolution (square). See [`FACE_DETECT_INFERENCE_WIDTH`].
pub const FACE_DETECT_INFERENCE_HEIGHT: u32 = 640;
/// Embedding inference resolution (square, `112×112` is the standard aligned
/// face crop size of the ArcFace/MobileFaceNet family). Part of the shared
/// preprocessing identity.
pub const FACE_EMBED_INFERENCE_WIDTH: u32 = 112;
/// Embedding inference resolution (square). See [`FACE_EMBED_INFERENCE_WIDTH`].
pub const FACE_EMBED_INFERENCE_HEIGHT: u32 = 112;
/// Embedding vector length (SFace/MobileFaceNet `fc1`). Part of the persisted
/// [`lumina_sidecar::FaceVectorRef::dimension`]; a change requires re-clustering.
pub const FACE_EMBED_DIMENSION: u32 = 128;

/// Default detection score threshold applied when building persisted
/// detections. It is part of the preprocessing identity, so changing it makes
/// previously persisted analyses `stale` (never a silent re-filter).
pub const FACE_DETECTION_SCORE_THRESHOLD_DEFAULT: f32 = 0.5;
/// Default non-maximum-suppression IoU threshold for the YuNet per-stride head
/// (OpenCV `FaceDetectorYN` default). Part of the identity.
pub const FACE_DETECTION_NMS_THRESHOLD_DEFAULT: f32 = 0.3;
/// Default maximum number of detections kept after NMS (OpenCV `FaceDetectorYN`
/// demo default). Part of the identity; must be `> 0`.
pub const FACE_DETECTION_TOP_K_DEFAULT: u32 = 5000;
/// Documented embedding normalization applied before clustering and
/// persistence.
pub const FACE_EMBEDDING_NORMALIZATION: &str = "l2";

/// Whether `manifest` carries a real (non-placeholder) `model_hash`.
///
/// FACE-20 §2.2 / FACE-20-S6: the shipped face manifests carry the verified
/// upstream SHA-256 pins, so this reports `true` for both. A descriptor still
/// carrying [`PENDING_INTEGRATION_HASH`] must never be reported as verified.
#[must_use]
pub fn face_model_hash_is_pinned(manifest: &ModelManifest) -> bool {
    manifest.model_hash != PENDING_INTEGRATION_HASH
}

/// Build the detection descriptor (YuNet, MIT, pinned `model_hash`).
///
/// Declares only `face_detect`. The graph contract is the **real** YuNet
/// layout (not the canonical fused single output): input tensor `input`
/// (`640×640`, raw `0..=255` **BGR**) and twelve per-stride outputs
/// `cls_/obj_/bbox_/kps_{8,16,32}` decoded by [`super::yunet::decode_yunet_detections`].
/// `output_tensor_name` names the first per-stride tensor; the adapter
/// validates the full declared set and refuses anything else loudly — never a
/// silent reshape.
#[must_use]
pub fn face_detect_manifest() -> ModelManifest {
    ModelManifest {
        model_name: super::FACE_DETECT_MODEL_NAME.into(),
        model_version: super::FACE_DETECT_MODEL_VERSION.into(),
        model_hash: super::FACE_DETECT_MODEL_HASH.into(),
        license: super::FACE_DETECT_LICENSE.into(),
        input: ModelInputSpec {
            resolution: ModelResolution {
                width: FACE_DETECT_INFERENCE_WIDTH,
                height: FACE_DETECT_INFERENCE_HEIGHT,
            },
            channel_layout: ChannelLayout::Bgr,
            tensor_name: "input".into(),
            tensor_format: TensorFormat::Nchw,
            normalization: InputNormalization::BYTE_RANGE,
        },
        output_tensor_name: super::yunet::YUNET_OUTPUT_NAMES[0].into(),
        capabilities: ModelCapabilities {
            face_detect: true,
            ..Default::default()
        },
    }
}

/// Build the embedding descriptor (SFace/MobileFaceNet, Apache-2.0, pinned
/// `model_hash`).
///
/// Declares only `face_embed`. The graph contract is the **real** SFace layout:
/// input tensor `data` (`112×112`, raw `0..=255` **RGB**; the graph bakes
/// `(x − 127.5) · 1/128` internally) and the 128-d `fc1` output, decoded by
/// [`super::backend::decode_face_embedding`] — no ImageNet preprocessing in
/// front of the graph.
#[must_use]
pub fn face_embed_manifest() -> ModelManifest {
    ModelManifest {
        model_name: super::FACE_EMBED_MODEL_NAME.into(),
        model_version: super::FACE_EMBED_MODEL_VERSION.into(),
        model_hash: super::FACE_EMBED_MODEL_HASH.into(),
        license: super::FACE_EMBED_LICENSE.into(),
        input: ModelInputSpec {
            resolution: ModelResolution {
                width: FACE_EMBED_INFERENCE_WIDTH,
                height: FACE_EMBED_INFERENCE_HEIGHT,
            },
            channel_layout: ChannelLayout::Rgb,
            tensor_name: "data".into(),
            tensor_format: TensorFormat::Nchw,
            normalization: InputNormalization::BYTE_RANGE,
        },
        output_tensor_name: "fc1".into(),
        capabilities: ModelCapabilities {
            face_embed: true,
            ..Default::default()
        },
    }
}

/// The pair of face models plus the embedding dimension that fully describes
/// stages 1/2 of the pipeline.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceModelSuite {
    /// Detection model descriptor (must declare `face_detect`).
    pub detection: ModelManifest,
    /// Embedding model descriptor (must declare `face_embed`).
    pub embedding: ModelManifest,
    /// Embedding vector length (`> 0`), persisted in the vector reference.
    pub embedding_dimension: u32,
}

impl FaceModelSuite {
    /// Build a suite from explicit descriptors, validating the capability and
    /// dimension contract loudly (no silent capability guessing).
    pub fn new(
        detection: ModelManifest,
        embedding: ModelManifest,
        embedding_dimension: u32,
    ) -> Result<Self, OnnxError> {
        let suite = Self {
            detection,
            embedding,
            embedding_dimension,
        };
        suite.validate()?;
        Ok(suite)
    }

    /// The shipped descriptor pair (YuNet MIT + SFace Apache-2.0, both with a
    /// verified, pinned `model_hash`).
    #[must_use]
    pub fn candidate() -> Self {
        Self {
            detection: face_detect_manifest(),
            embedding: face_embed_manifest(),
            embedding_dimension: FACE_EMBED_DIMENSION,
        }
    }

    /// Validate both manifests and the declared capabilities/dimension.
    pub fn validate(&self) -> Result<(), OnnxError> {
        self.detection.validate()?;
        self.embedding.validate()?;
        if !self.detection.capabilities.face_detect {
            return Err(OnnxError::UnsupportedModel {
                name: self.detection.model_name.clone(),
                reason: "detection model does not declare the `face_detect` capability".into(),
            });
        }
        if !self.embedding.capabilities.face_embed {
            return Err(OnnxError::UnsupportedModel {
                name: self.embedding.model_name.clone(),
                reason: "embedding model does not declare the `face_embed` capability".into(),
            });
        }
        if self.embedding_dimension == 0 {
            return Err(OnnxError::InvalidFaceData(
                "face embedding_dimension must be > 0".into(),
            ));
        }
        Ok(())
    }
}

/// Identity-bearing inference options (part of the persisted preprocessing
/// contract, so a change invalidates persisted analyses).
#[derive(Debug, Clone, PartialEq)]
pub struct FaceInferenceOptions {
    /// Detection score threshold applied when persisting detections.
    pub detection_score_threshold: f32,
    /// Non-maximum-suppression IoU threshold for the per-stride head
    /// (LRPAR-G12-FACE-ADAPTER-25). Part of the identity.
    pub detection_nms_threshold: f32,
    /// Maximum number of detections kept after NMS (`> 0`). Part of the
    /// identity.
    pub detection_top_k: u32,
    /// Embedding normalization applied before clustering/persistence.
    pub embedding_normalization: String,
}

impl Default for FaceInferenceOptions {
    fn default() -> Self {
        Self {
            detection_score_threshold: FACE_DETECTION_SCORE_THRESHOLD_DEFAULT,
            detection_nms_threshold: FACE_DETECTION_NMS_THRESHOLD_DEFAULT,
            detection_top_k: FACE_DETECTION_TOP_K_DEFAULT,
            embedding_normalization: FACE_EMBEDDING_NORMALIZATION.into(),
        }
    }
}

impl FaceInferenceOptions {
    /// Validate the option contract loudly (finite threshold in `0..=1`,
    /// non-empty normalization name, `top_k > 0`).
    pub fn validate(&self) -> Result<(), OnnxError> {
        if !self.detection_score_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.detection_score_threshold)
        {
            return Err(OnnxError::InvalidFaceData(format!(
                "face detection_score_threshold must be finite within 0..=1, got {}",
                self.detection_score_threshold
            )));
        }
        if !self.detection_nms_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.detection_nms_threshold)
        {
            return Err(OnnxError::InvalidFaceData(format!(
                "face detection_nms_threshold must be finite within 0..=1, got {}",
                self.detection_nms_threshold
            )));
        }
        if self.detection_top_k == 0 {
            return Err(OnnxError::InvalidFaceData(
                "face detection_top_k must be > 0".into(),
            ));
        }
        if self.embedding_normalization.trim().is_empty() {
            return Err(OnnxError::InvalidFaceData(
                "face embedding_normalization must not be empty".into(),
            ));
        }
        Ok(())
    }
}
