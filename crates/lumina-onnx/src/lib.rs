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
//! This crate is **native-only** and mirrors `lumina_raw`: the entire module
//! is gated out on `wasm32`, so no native code is ever compiled for the browser
//! target. The real ONNX Runtime backend is additionally gated behind the
//! non-default `onnx-rt` feature.
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

#[cfg(not(target_arch = "wasm32"))]
pub mod backend;
#[cfg(not(target_arch = "wasm32"))]
pub mod hash;
#[cfg(not(target_arch = "wasm32"))]
pub mod manifest;
#[cfg(not(target_arch = "wasm32"))]
pub mod preprocess;
#[cfg(not(target_arch = "wasm32"))]
pub mod resolve;
#[cfg(not(target_arch = "wasm32"))]
pub mod sam2;

#[cfg(all(not(target_arch = "wasm32"), feature = "onnx-rt"))]
pub mod ort_backend;

#[cfg(not(target_arch = "wasm32"))]
pub use backend::{StubBackend, SubjectInference};
#[cfg(not(target_arch = "wasm32"))]
pub use hash::{
    compute_sha256_hex, verify_model_file, verify_model_hash, ModelHashStatus,
    PENDING_INTEGRATION_HASH,
};
#[cfg(not(target_arch = "wasm32"))]
pub use manifest::{
    birefnet_manifest, sam2_1_manifest, sam2_1_manifests, select_variant, ChannelLayout,
    DeviceProfile, InputNormalization, ModelCapabilities, ModelInputSpec, ModelManifest,
    Resolution, Sam2Variant, TensorFormat, BIREFNET_INFERENCE_HEIGHT, BIREFNET_INFERENCE_WIDTH,
    INPUT_SPEC_DIGEST_KEY, SAM2_INFERENCE_HEIGHT, SAM2_INFERENCE_WIDTH,
};
#[cfg(not(target_arch = "wasm32"))]
pub use preprocess::{
    matte_values_from_unit_f32, normalize_rgb_to_nchw, preprocess_rgb_to_model,
    rescale_model_matte, validate_output_shape,
};
#[cfg(not(target_arch = "wasm32"))]
pub use resolve::{try_load_onnx_engine, OnnxEngine};
#[cfg(not(target_arch = "wasm32"))]
pub use sam2::{
    model_point_to_source, source_box_to_model, source_point_to_model, BoxPrompt, MaskPromptLogits,
    PointLabel, PromptMaskInference, PromptPoint, SegmentationPrompt, SourceBox, SourcePoint,
    StubSam2Backend,
};

// ── wasm32 stub ──────────────────────────────────────────────────────────────
// Native ONNX (ort) is not wasm-compilable. On wasm32 the crate still exists
// so `cargo check --target wasm32-unknown-unknown` stays green, but every
// real inference surface reports "not available" explicitly (no silent fallback).
#[cfg(target_arch = "wasm32")]
pub mod wasm_stub;

#[cfg(target_arch = "wasm32")]
pub use wasm_stub::{
    birefnet_manifest, sam2_1_manifest, sam2_1_manifests, select_variant, try_load_onnx_engine,
    OnnxEngine, StubBackend, SubjectInference,
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
}
