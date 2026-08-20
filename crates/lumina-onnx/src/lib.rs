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
//! This crate is **native-only** and mirrors [`lumina_raw`]: the entire module
//! is gated out on `wasm32`, so no native code is ever compiled for the browser
//! target. The real ONNX Runtime backend is additionally gated behind the
//! non-default `onnx-rt` feature.
//!
//! ## Exchangeable surface
//!
//! The adapter boundary is the [`SubjectInference`] trait. The default, fully
//! tested surface is the deterministic [`StubBackend`] (no weights, no network).
//! A real ONNX Runtime backend lives in [`ort_backend`] behind `onnx-rt`
//! (see [`README.md`](crate::README) / crate docs for the landing plan).
//!
//! ## Model identity
//!
//! [`ModelManifest`] and [`ModelCapabilities`] (F-080) declare a model's
//! identity and capabilities. They do **not** depend on `lumina-sidecar`; the
//! mapping to the sidecar `ModelIdentity` is deferred to F-048.

#![cfg(not(target_arch = "wasm32"))]

pub mod backend;
pub mod manifest;
pub mod preprocess;
pub mod sam2;

#[cfg(feature = "onnx-rt")]
pub mod ort_backend;

pub use backend::{StubBackend, SubjectInference};
pub use manifest::{
    birefnet_manifest, sam2_1_manifest, sam2_1_manifests, select_variant, ChannelLayout,
    DeviceProfile, ModelCapabilities, ModelInputSpec, ModelManifest, Resolution, Sam2Variant,
    TensorFormat, BIREFNET_INFERENCE_HEIGHT, BIREFNET_INFERENCE_WIDTH, SAM2_INFERENCE_HEIGHT,
    SAM2_INFERENCE_WIDTH,
};
pub use preprocess::{preprocess_rgb_to_model, rescale_model_matte};
pub use sam2::{
    BoxPrompt, MaskPromptLogits, PointLabel, PromptMaskInference, PromptPoint, SegmentationPrompt,
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
    /// A segmentation prompt was unsupported or invalid (e.g. a capability the
    /// model does not declare, inverted/empty box, out-of-bounds coordinates, or
    /// a mask-logits size mismatch). Reported, never silently downgraded.
    #[error("invalid segmentation prompt for model `{name}`: {reason}")]
    InvalidPrompt { name: String, reason: String },
    /// The manifest could not be (de)serialized or failed validation.
    #[error("invalid model manifest: {0}")]
    InvalidManifest(String),
}
