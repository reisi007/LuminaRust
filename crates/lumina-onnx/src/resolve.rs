//! Backend resolution for consumers (CLI / core integration).
//!
//! `lumina-onnx` ships two subject-inference backends:
//!
//! * the deterministic, dependency-free [`StubBackend`](crate::StubBackend)
//!   (default surface, no weights/network) and
//! * the real ONNX Runtime backend (`OrtBackend`, behind the non-default
//!   `onnx-rt` feature).
//!
//! This module answers the integration question **"how does a caller obtain
//! the real engine — and how does it know when it cannot?"** It is the
//! capability surface `lumina-cli` / `lumina-core` consume (or will consume)
//! to wire the real ORT path **without a silent fallback**: a requested real
//! engine that is unavailable (feature off, missing artifact, stale weights,
//! wrong tensor names) is reported, never guessed.

use crate::manifest::ModelManifest;
use crate::OnnxError;
use std::path::Path;

/// A resolvable, real ONNX inference engine — or the explicit statement that
/// no real engine is possible in this build.
pub enum OnnxEngine {
    /// A real, artifact-verified ONNX Runtime backend is loaded.
    ///
    /// Only exists when the `onnx-rt` feature is compiled in (the `ort`
    /// dependency is optional). The boxed `lumina_core::MaskInference`
    /// contract is the exact surface the mask-loading decision layer
    /// (F-048/F-051) consumes, so a call site can hand the engine straight to
    /// `lumina_core::resolve_mask_planes` without additive glue or a behavior
    /// split between the stub and the real backend.
    #[cfg(feature = "onnx-rt")]
    OnnxRuntime(Box<dyn lumina_core::MaskInference>),
    /// The `onnx-rt` capability is **not compiled into this build** (default
    /// feature set). This is a deliberate, visible state — never a silent
    /// fallback: the caller asked for the real engine and is told it is
    /// unavailable *by capability*, so it can explicitly choose the
    /// [`StubBackend`](crate::StubBackend) or no engine at all.
    RuntimeDisabled,
}

impl std::fmt::Debug for OnnxEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The boxed `dyn MaskInference` is deliberately NOT printed (no Debug
        // bound on the trait); the debug output only states *which* engine was
        // resolved, keeping `OnnxEngine` printable for error messages.
        match self {
            #[cfg(feature = "onnx-rt")]
            OnnxEngine::OnnxRuntime(_) => f.write_str("OnnxRuntime(<real engine>)"),
            OnnxEngine::RuntimeDisabled => f.write_str("RuntimeDisabled"),
        }
    }
}

/// Attempt to load the real ONNX Runtime engine for `manifest` from
/// `model_path`.
///
/// # Contract (no silent fallback)
///
/// | Situation | Result |
/// | --- | --- |
/// | `onnx-rt` **off** (capability not compiled) | `Ok(OnnxEngine::RuntimeDisabled)` — the caller decides explicitly between stub and no engine |
/// | `onnx-rt` **on**, artifact present and identity-verified (SHA-256 vs `manifest.model_hash`, declared tensor names exist in the graph) | `Ok(OnnxEngine::OnnxRuntime(backend))` |
/// | `onnx-rt` **on**, artifact missing / unreadable | `Err(OnnxError::MissingModel)` |
/// | `onnx-rt` **on**, artifact hash differs from `manifest.model_hash` | `Err(OnnxError::ModelArtifactStale)` |
/// | `onnx-rt` **on**, manifest-declared tensor names absent from the graph | `Err(OnnxError::InferenceFailed)` |
///
/// There is **no fallback to `StubBackend` under any error condition** — a
/// missing/stale/mismatched artifact is surfaced to the caller, which must
/// report it visibly (Agents.md: „Fehlende oder inkompatible Artefakte werden
/// sichtbar als veraltet oder nicht verfügbar gemeldet").
#[cfg_attr(not(feature = "onnx-rt"), allow(unused_variables))]
pub fn try_load_onnx_engine(
    model_path: &Path,
    manifest: &ModelManifest,
) -> Result<OnnxEngine, OnnxError> {
    #[cfg(feature = "onnx-rt")]
    {
        let backend = crate::ort_backend::OrtBackend::new(model_path, manifest.clone())?;
        Ok(OnnxEngine::OnnxRuntime(Box::new(backend)))
    }
    #[cfg(not(feature = "onnx-rt"))]
    {
        Ok(OnnxEngine::RuntimeDisabled)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(feature = "onnx-rt"))]
    use std::path::Path;

    /// Default (no `onnx-rt`) build: requesting the real engine must yield the
    /// explicit `RuntimeDisabled` state — the capability statement, never a
    /// stub masquerading as a real engine. (With `onnx-rt` enabled this test
    /// is compiled out; the integration coverage lives in
    /// `tests/ort_backend.rs`.)
    #[cfg(not(feature = "onnx-rt"))]
    #[test]
    fn runtime_disabled_is_explicit_when_feature_is_off() {
        let engine = super::try_load_onnx_engine(
            Path::new("/nonexistent/model.onnx"),
            &crate::birefnet_manifest(),
        )
        .expect("feature-off load must succeed with the explicit flag");
        assert!(
            matches!(engine, super::OnnxEngine::RuntimeDisabled),
            "expected the explicit capability statement, got {engine:?}"
        );
    }
}
