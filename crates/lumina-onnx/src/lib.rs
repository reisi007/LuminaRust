//! # lumina-onnx — native ONNX inference adapter for Lumina
//!
//! `lumina-onnx` encapsulates native ONNX inference, model management and mask
//! artifacts so the portable, platform-neutral [`lumina_core`] is never burdened
//! with native dependencies. It is the crate described in `Agents.md`
//! ("`lumina-onnx` kapselt native Inferenz, Modellverwaltung und
//! Maskenartefakte") and the F-047 adapter for `BiRefNet` as the first automatic
//! subject model, with an exchangeable backend surface.
//!
//! ## Native-only
//!
//! This crate is **native-only** and mirrors `lumina_raw`: no code in this
//! crate is built for the browser target. The real ONNX Runtime backend is
//! additionally gated behind the non-default `onnx-rt` feature.
//!
//! ## Exchangeable surface
//!
//! The adapter boundary is
//! [the `SubjectInference` trait](crate::SubjectInference).
//! The default, fully tested surface is the deterministic [`StubBackend`]
//! (no weights, no network).
//! A real ONNX Runtime backend lives in `ort_backend` behind `onnx-rt`
//! (see `README.md` / crate docs for the landing plan).
//! [`try_load_onnx_engine`] is the capability surface for consumers (CLI/core):
//! it loads the real engine when `onnx-rt` is compiled in and the artifact
//! verifies, and otherwise reports the explicit states `RuntimeDisabled`,
//! [`OnnxError::MissingModel`], [`OnnxError::ModelArtifactStale`] or
//! [`OnnxError::InferenceFailed`] — **never a silent fallback to the stub**.
//!
//! ## Model identity
//!
//! [`ModelManifest`] and [`ModelCapabilities`] (F-080) declare a model's
//! identity and capabilities. The mapping to the sidecar `ModelIdentity`
//! happens via [`ModelManifest::to_model_identity`] (F-048; `lumina-sidecar`
//! is a dependency solely for that identity type — no native/ONNX concern
//! leaks into the platform-neutral core).
//!
//! ## Face pipeline (LRPAR-G12-FACE-20, S2 + S3)
//!
//! The [`face`] module adds the native face-detection/embedding stages
//! (deterministic, tests-only stubs by default; a real ORT path behind
//! `onnx-rt`) and the model-free, deterministic clustering stage, plus the
//! mapping onto the S1 sidecar `face` schema. Every planned weight descriptor
//! stays `pending-integration` until hash-pinned weights land — there is no
//! download and no silent fallback. See [`face`] for the licence findings and
//! the full identity/invalidation contract.

pub mod backend;
pub mod face;
pub mod generative;
pub mod hash;
pub mod inpaint;
pub mod manifest;
pub mod outpaint;
pub mod preprocess;
pub mod resolve;
pub mod sam2;

#[cfg(feature = "onnx-rt")]
pub mod ort_backend;

pub use backend::{StubBackend, SubjectInference};
pub use face::backend::{
    align_face_to_template, decode_face_detections, decode_face_embedding, FACE_ALIGN_TEMPLATE_5PT,
    FACE_ALIGN_TEMPLATE_BASE,
};
pub use face::cluster::{
    cluster_embeddings, cluster_id_for, clusters_from_labels, confirm_person, merge_clusters,
    split_cluster, FaceClusteringParams, FACE_CLUSTERING_EPS_DEFAULT, FACE_CLUSTERING_METHOD,
    FACE_CLUSTERING_MIN_SAMPLES_DEFAULT, FACE_CLUSTERING_VERSION,
};
pub use face::{
    detected_face_id, embedding_id_for, face_artifact_status, face_detect_manifest,
    face_embed_manifest, face_identity, face_identity_digest, face_identity_with_digest,
    face_model_hash_is_pinned, try_load_face_engine, DetectedFace, FaceAnalysisOutput,
    FaceArtifactEvidence, FaceDetectionInference, FaceEmbeddingInference, FaceEmbeddingRecord,
    FaceEmbeddingVector, FaceInferenceOptions, FaceModelSuite, FaceOnnxEngine, StubFaceDetector,
    StubFaceEmbedder, FACE_DETECTION_SCORE_THRESHOLD_DEFAULT, FACE_DETECT_INFERENCE_HEIGHT,
    FACE_DETECT_INFERENCE_WIDTH, FACE_DETECT_LICENSE, FACE_DETECT_MODEL_NAME,
    FACE_DETECT_MODEL_VERSION, FACE_EMBEDDING_NORMALIZATION, FACE_EMBED_DIMENSION,
    FACE_EMBED_INFERENCE_HEIGHT, FACE_EMBED_INFERENCE_WIDTH, FACE_EMBED_LICENSE,
    FACE_EMBED_MODEL_NAME, FACE_EMBED_MODEL_VERSION, FACE_IDENTITY_DIGEST_KEY,
    FACE_LANDMARK_NAMES_5PT, FACE_PREPROCESSING_NAME, FACE_PREPROCESSING_VERSION,
    FACE_RESCALING_METHOD,
};
pub use generative::{
    fixture_manifest, fixture_model_hash, manifest_hash_is_pinned, outpaint_canvas_from_sidecar,
    produce_canvas, role_from_core, transparent_mask, verify_fixture_manifest,
    GenerativeCanvasOutput, GenerativeModelSource, GenerativeRole, GENERATIVE_FIXTURE_ALGORITHM,
};
pub use hash::{
    compute_sha256_hex, verify_model_file, verify_model_hash, ModelHashStatus,
    PENDING_INTEGRATION_HASH,
};
pub use inpaint::{InpaintRequest, StubInpaintBackend};
pub use manifest::{
    birefnet_manifest, inpaint_heal_manifest, outpaint_expand_manifest, sam2_1_manifest,
    sam2_1_manifests, select_variant, ChannelLayout, DeviceProfile, InputNormalization,
    ModelCapabilities, ModelInputSpec, ModelManifest, Resolution, Sam2Variant, TensorFormat,
    BIREFNET_INFERENCE_HEIGHT, BIREFNET_INFERENCE_WIDTH, INPUT_SPEC_DIGEST_KEY,
    OUTPAINT_INFERENCE_HEIGHT, OUTPAINT_INFERENCE_WIDTH, SAM2_INFERENCE_HEIGHT,
    SAM2_INFERENCE_WIDTH,
};
pub use outpaint::{OutpaintCanvas, OutpaintRequest, StubOutpaintBackend};
pub use preprocess::{
    matte_values_from_unit_f32, normalize_rgb_to_nchw, preprocess_rgb_to_model,
    rescale_model_matte, validate_output_shape,
};
pub use resolve::{try_load_onnx_engine, OnnxEngine};
pub use sam2::{
    model_point_to_source, source_box_to_model, source_point_to_model, BoxPrompt, MaskPromptLogits,
    PointLabel, PromptMaskInference, PromptPoint, SegmentationPrompt, SourceBox, SourcePoint,
    StubSam2Backend,
};

use thiserror::Error;

/// Errors produced by the ONNX adapter. There are deliberately **no silent
/// fallbacks**: a missing or mismatched artifact is reported, never guessed.
#[derive(Debug, Error)]
pub enum OnnxError {
    /// The model manifest, license or capability set is unsupported or invalid
    /// (e.g. no capability declared, capability/license mismatch).
    #[error("model `{name}` is unsupported: {reason}")]
    UnsupportedModel { name: String, reason: String },
    /// Inference (or model loading) failed on a present artifact.
    #[error("inference failed for model `{name}`: {reason}")]
    InferenceFailed { name: String, reason: String },
    /// Mask or image dimensions were invalid or disagreed (e.g. model matte vs.
    /// declared inference resolution, or zero-area input).
    #[error(
        "invalid mask dimensions: expected {expected_width}x{expected_height}, \
         got {actual_width}x{actual_height}"
    )]
    InvalidDimensions {
        expected_width: u32,
        expected_height: u32,
        actual_width: u32,
        actual_height: u32,
    },
    /// A model artifact required for inference is not available.
    #[error("model artifact `{path}` is not available")]
    MissingModel { path: String },
    /// The configured model **reported itself unavailable** (availability
    /// flag, e.g. the stub's simulated missing installation) — there is no
    /// concrete artifact path to name. Deliberately distinct from
    /// [`OnnxError::MissingModel`], whose `path` names a real file, so the
    /// two causes are distinguishable in logs and user-facing messages
    /// (R2-ONNX-05).
    #[error("model `{name}` reported unavailable (not installed)")]
    ModelUnavailable { name: String },
    /// The loaded artifact's hash differs from the manifest `model_hash`
    /// (stale/mismatched weights). Reported instead of silently inferring
    /// with the wrong weights (REVIEW-ONNX-HASH-1).
    #[error(
        "model artifact for `{name}` is stale: manifest pins hash `{expected}`, \
         but the artifact hashes to `{actual}`"
    )]
    ModelArtifactStale {
        name: String,
        expected: String,
        actual: String,
    },
    /// A segmentation prompt was unsupported or invalid (e.g. a capability the
    /// model does not declare, inverted/empty box, out-of-bounds coordinates, or
    /// a mask-logits size mismatch). Reported, never silently downgraded.
    #[error("invalid segmentation prompt for model `{name}`: {reason}")]
    InvalidPrompt { name: String, reason: String },
    /// The manifest could not be (de)serialized or failed validation.
    #[error("invalid model manifest: {0}")]
    InvalidManifest(String),
    /// Face-analysis data violated its documented contract (embedding/box
    /// validation, clustering thresholds, cluster/person operations). Reported
    /// loudly — never silently clamped, repaired or dropped (FACE-20-S2/S3).
    #[error("invalid face data: {0}")]
    InvalidFaceData(String),
}
