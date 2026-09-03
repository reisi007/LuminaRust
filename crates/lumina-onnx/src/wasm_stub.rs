//! WASM stub for `lumina-onnx` — native-only capability.
//!
//! On `wasm32` the real ONNX Runtime (`ort`) and `zstd`-backed codec are not
//! available. The crate still compiles for `cargo check --target wasm32`
//! but every inference surface reports "not available" explicitly; callers
//! must handle `RuntimeDisabled` / `ModelUnavailable` visibly.

use crate::OnnxError;
use lumina_core::{ImageFrame, MaskPlane};
use std::path::Path;

pub trait SubjectInference {
    fn manifest(&self) -> &DummyManifest;
    fn is_available(&self) -> bool {
        false
    }
    fn infer(&self, _image: &ImageFrame) -> Result<MaskPlane, OnnxError>;
}

#[derive(Debug, Clone)]
pub struct DummyManifest {
    pub model_name: String,
}

// Minimal stubs so consumer imports compile on wasm32.
pub fn birefnet_manifest() -> DummyManifest {
    DummyManifest {
        model_name: "BiRefNet".into(),
    }
}
pub fn sam2_1_manifest(_v: u8) -> DummyManifest {
    DummyManifest {
        model_name: "sam2-wasm-stub".into(),
    }
}
pub fn sam2_1_manifests() -> Vec<DummyManifest> {
    Vec::new()
}
pub fn inpaint_heal_manifest() -> DummyManifest {
    DummyManifest {
        model_name: "inpaint-heal-xl".into(),
    }
}
/// Outpaint descriptor stub (GEN-EXPAND-1): present so consumer imports
/// compile on wasm32, but inference itself is unavailable — see
/// [`StubOutpaintBackend::expand`].
pub fn outpaint_expand_manifest() -> DummyManifest {
    DummyManifest {
        model_name: "inpaint-outpaint-xl".into(),
    }
}
pub fn select_variant(_profile: &DummyManifest) -> u8 {
    0
}

pub struct StubBackend {
    manifest: DummyManifest,
}

impl StubBackend {
    pub fn new(manifest: DummyManifest) -> Result<Self, OnnxError> {
        Ok(Self { manifest })
    }
    pub fn with_availability(self, _available: bool) -> Self {
        self
    }
    pub fn is_available(&self) -> bool {
        false
    }
    pub fn generate_model_matte(&self) -> Result<MaskPlane, OnnxError> {
        Err(OnnxError::ModelUnavailable {
            name: self.manifest.model_name.clone(),
        })
    }
}

impl SubjectInference for StubBackend {
    fn manifest(&self) -> &DummyManifest {
        &self.manifest
    }
    fn is_available(&self) -> bool {
        false
    }
    fn infer(&self, _image: &ImageFrame) -> Result<MaskPlane, OnnxError> {
        Err(OnnxError::ModelUnavailable {
            name: self.manifest.model_name.clone(),
        })
    }
}

impl lumina_core::MaskInference for StubBackend {
    fn is_available(&self) -> bool {
        false
    }
    fn infer(&self, _frame: &ImageFrame) -> Result<MaskPlane, lumina_core::CoreError> {
        Err(lumina_core::CoreError::MaskInference {
            reason: "onnx inference not available on wasm32".into(),
        })
    }
}

/// Target canvas geometry for an outpaint expansion (wasm32 stub shape —
/// mirrors the native [`OutpaintCanvas`](crate::OutpaintCanvas) fields so
/// consumer code compiles; never executed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutpaintCanvas {
    pub output_width: u32,
    pub output_height: u32,
    pub source_offset_x: i32,
    pub source_offset_y: i32,
}

/// Generative outpaint request (wasm32 stub shape).
#[derive(Debug, Clone)]
pub struct OutpaintRequest {
    pub prompt: String,
    pub negative_prompt: Option<String>,
    pub seed: u64,
    pub canvas: OutpaintCanvas,
}

/// Outpaint stub for wasm32: construction compiles, but [`StubOutpaintBackend::expand`]
/// always reports [`OnnxError::ModelUnavailable`] — the visible
/// `RuntimeDisabled` equivalent for the generative path (F-070, off by
/// default via `onnx-wasm`; never a silent fallback).
pub struct StubOutpaintBackend {
    manifest: DummyManifest,
    available: bool,
}

impl StubOutpaintBackend {
    pub fn new(manifest: DummyManifest) -> Result<Self, OnnxError> {
        Ok(Self {
            manifest,
            available: false,
        })
    }
    pub fn with_availability(self, available: bool) -> Self {
        Self { available, ..self }
    }
    pub fn manifest(&self) -> &DummyManifest {
        &self.manifest
    }
    pub fn is_available(&self) -> bool {
        false
    }
    pub fn expand(
        &self,
        _image: &[u8],
        _width: u32,
        _height: u32,
        _request: &OutpaintRequest,
    ) -> Result<Vec<u8>, OnnxError> {
        let _ = self.available;
        Err(OnnxError::ModelUnavailable {
            name: self.manifest.model_name.clone(),
        })
    }
}

#[derive(Debug)]
pub enum OnnxEngine {
    RuntimeDisabled,
}

pub fn try_load_onnx_engine(
    _model_path: &Path,
    _manifest: &DummyManifest,
) -> Result<OnnxEngine, OnnxError> {
    Ok(OnnxEngine::RuntimeDisabled)
}
