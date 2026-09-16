#![cfg(feature = "onnx-rt")]

//! Real ONNX Runtime KI-Denoise backend (gated behind the `onnx-rt` feature).
//!
//! [`OrtDenoiser`] manages one RGB-to-RGB denoiser session, mirroring the
//! subject path in `ort_backend.rs` and the face path in `face/ort.rs`.
//!
//! ## Artifact identity (no silent fallback)
//!
//! Construction verifies the artifact bytes against the manifest `model_hash`
//! (SHA-256, [`crate::hash`]) and validates that both manifest-declared tensor
//! names exist in the graph. The resulting [`ModelHashStatus`] is kept on the
//! backend ([`OrtDenoiser::hash_status`]): a mismatch does not prevent loading
//! but makes every inference fail with [`OnnxError::ModelArtifactStale`] —
//! stale weights never run silently.
//!
//! ## `pending-integration` is refused loudly
//!
//! Unlike the established mask/face paths, the KI-Denoise decision
//! (`feature/decisions/LRPAR-G14-DENOISE-20.md` §3.1) makes the
//! `pending-integration` placeholder an explicit **unavailable** state: a
//! descriptor without a pinned identity must never drive inference, so
//! [`OrtDenoiser::new`] refuses it with [`OnnxError::ModelUnavailable`] instead
//! of loading unverifiable weights. The deterministic, tests-only
//! [`crate::denoise::StubDenoiseBackend`] is **never** a substitute.
//!
//! ## Tile contract
//!
//! A denoiser is fully-convolutional: [`OrtDenoiser::denoise`] feeds the actual
//! tile geometry (`[1, 3, H, W]`, values `[0, 1]` via identity preprocessing)
//! so edge tiles may be smaller than the declared tile resolution. The output
//! must be `[1, 3, H, W]`; any other shape is a loud
//! [`OnnxError::InferenceFailed`] (`decode_denoise_rgb`), never a silent
//! reshape.
//!
//! Real denoise weights remain `pending-integration` (Agents.md: no downloads),
//! so numeric correctness against a real checkpoint is validated when weights
//! land; the load/verify/tensor-name/contract paths are exercised against the
//! committed behavior fixture in `tests/denoise_ort.rs`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use lumina_core::ImageFrame;

use crate::denoise::{decode_denoise_rgb, DenoiseInference};
use crate::hash::{verify_model_file, ModelHashStatus};
use crate::manifest::ModelManifest;
use crate::preprocess::normalize_rgb_to_nchw;
use crate::OnnxError;

/// Descriptive [`OnnxError::InferenceFailed`] for a manifest-declared tensor
/// name missing from the loaded graph (mirrors `ort_backend.rs`).
fn tensor_name_error<T: AsRef<str>>(
    kind: &str,
    requested: &str,
    available: &[T],
    model_name: &str,
) -> OnnxError {
    let listed = if available.is_empty() {
        "<none>".to_owned()
    } else {
        available
            .iter()
            .map(|name| format!("`{}`", name.as_ref()))
            .collect::<Vec<_>>()
            .join(", ")
    };
    OnnxError::InferenceFailed {
        name: model_name.to_owned(),
        reason: format!(
            "the loaded ONNX graph has no {kind} tensor `{requested}` \
             (available {kind}s: {listed})"
        ),
    }
}

/// ONNX Runtime backed KI-Denoise backend.
pub struct OrtDenoiser {
    manifest: ModelManifest,
    hash_status: ModelHashStatus,
    path: PathBuf,
    // `ort::Session::run` requires `&mut self`; the shared `DenoiseInference`
    // trait uses `&self`, so we use interior mutability.
    session: RefCell<ort::session::Session>,
}

impl OrtDenoiser {
    /// Load a denoise session, verifying the artifact identity and the
    /// manifest-declared tensor names.
    ///
    /// Returns [`OnnxError::ModelUnavailable`] for a `pending-integration`
    /// manifest (no pinned identity → the visible `unavailable` state),
    /// [`OnnxError::MissingModel`] for an absent/unreadable artifact,
    /// [`OnnxError::UnsupportedModel`] when the manifest does not declare
    /// `denoise`, and [`OnnxError::InferenceFailed`] when the session cannot be
    /// built or a declared tensor name is absent. A digest mismatch does not
    /// prevent loading but makes inference fail with
    /// [`OnnxError::ModelArtifactStale`] — stale weights never run silently.
    pub fn new(model_path: impl AsRef<Path>, manifest: ModelManifest) -> Result<Self, OnnxError> {
        manifest.validate()?;
        if !manifest.capabilities.denoise {
            return Err(OnnxError::UnsupportedModel {
                name: manifest.model_name.clone(),
                reason: "denoise not declared".into(),
            });
        }
        let path = model_path.as_ref().to_path_buf();
        let hash_status = verify_model_file(&path, &manifest.model_hash)?;
        if matches!(hash_status, ModelHashStatus::Pending) {
            return Err(OnnxError::ModelUnavailable {
                name: manifest.model_name.clone(),
            });
        }
        let session = ort::session::Session::builder()
            .and_then(|mut builder| builder.commit_from_file(&path))
            .map_err(|error| OnnxError::InferenceFailed {
                name: manifest.model_name.clone(),
                reason: format!("failed to load ONNX session: {error}"),
            })?;
        let input_names: Vec<&str> = session.inputs().iter().map(|io| io.name()).collect();
        if !input_names
            .iter()
            .any(|name| *name == manifest.input.tensor_name)
        {
            return Err(tensor_name_error(
                "input",
                &manifest.input.tensor_name,
                &input_names,
                &manifest.model_name,
            ));
        }
        let output_names: Vec<&str> = session.outputs().iter().map(|io| io.name()).collect();
        if !output_names
            .iter()
            .any(|name| *name == manifest.output_tensor_name)
        {
            return Err(tensor_name_error(
                "output",
                &manifest.output_tensor_name,
                &output_names,
                &manifest.model_name,
            ));
        }
        Ok(Self {
            manifest,
            hash_status,
            path,
            session: RefCell::new(session),
        })
    }

    /// Result of verifying the loaded artifact against the manifest
    /// `model_hash`.
    #[must_use]
    pub fn hash_status(&self) -> &ModelHashStatus {
        &self.hash_status
    }

    /// The artifact path this backend was loaded from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn denoise_inner(&self, image: &ImageFrame) -> Result<Vec<u8>, OnnxError> {
        self.hash_status
            .enforce_inference_allowed(&self.manifest.model_name)?;
        if image.width == 0 || image.height == 0 {
            return Err(OnnxError::InvalidDimensions {
                expected_width: self.manifest.input.resolution.width,
                expected_height: self.manifest.input.resolution.height,
                actual_width: image.width,
                actual_height: image.height,
            });
        }
        // Interleaved RGB at native tile geometry (no resize: the denoiser is
        // fully-convolutional and must keep the pixel grid).
        let mut rgb = Vec::with_capacity(image.width as usize * image.height as usize * 3);
        for pixel in image.pixels.as_chunks::<4>().0 {
            rgb.extend_from_slice(&pixel[..3]);
        }
        let data = normalize_rgb_to_nchw(
            &rgb,
            &self.manifest.model_name,
            &self.manifest.input.normalization,
        )?;
        let tensor = ort::value::Tensor::from_array((
            vec![1i64, 3, image.height as i64, image.width as i64],
            data,
        ))
        .map_err(|error| OnnxError::InferenceFailed {
            name: self.manifest.model_name.clone(),
            reason: format!("failed to build input tensor: {error}"),
        })?;
        let input_name = self.manifest.input.tensor_name.clone();
        let mut session = self.session.borrow_mut();
        let outputs = session
            .run(ort::inputs![input_name => tensor])
            .map_err(|error| OnnxError::InferenceFailed {
                name: self.manifest.model_name.clone(),
                reason: format!("inference failed: {error}"),
            })?;
        let output_name = self.manifest.output_tensor_name.as_str();
        let available: Vec<&str> = outputs.keys().collect();
        let output = outputs.get(output_name).ok_or_else(|| {
            tensor_name_error("output", output_name, &available, &self.manifest.model_name)
        })?;
        let (shape, raw) =
            output
                .try_extract_tensor::<f32>()
                .map_err(|error| OnnxError::InferenceFailed {
                    name: self.manifest.model_name.clone(),
                    reason: format!("failed to read output tensor `{output_name}`: {error}"),
                })?;
        let shape: Vec<usize> = shape.iter().map(|axis| *axis as usize).collect();
        decode_denoise_rgb(&self.manifest.model_name, &shape, raw)
    }
}

impl DenoiseInference for OrtDenoiser {
    fn manifest(&self) -> &ModelManifest {
        &self.manifest
    }

    fn denoise(&self, image: &ImageFrame) -> Result<Vec<u8>, OnnxError> {
        self.denoise_inner(image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::denoise::denoise_manifest;

    #[test]
    fn denoiser_reports_missing_model_artifact() {
        let result = OrtDenoiser::new("/nonexistent/lumina/denoise.onnx", denoise_manifest());
        assert!(
            matches!(result, Err(OnnxError::MissingModel { .. })),
            "absent artifact must surface as MissingModel"
        );
    }

    #[test]
    fn denoiser_reports_pending_integration_loudly() {
        // A present, readable artifact is still refused when the manifest has
        // no pinned identity: `pending-integration` is the visible
        // `unavailable` state, never unverifiable inference.
        let path = std::env::temp_dir().join(format!(
            "lumina-onnx-denoise-pending-{}-{}.onnx",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&path, b"unverifiable bytes").unwrap();
        let result = OrtDenoiser::new(&path, denoise_manifest());
        let _ = std::fs::remove_file(&path);
        assert!(
            matches!(result, Err(OnnxError::ModelUnavailable { .. })),
            "a pending-integration manifest must be refused loudly"
        );
    }

    #[test]
    fn denoiser_rejects_wrong_capability_manifest() {
        let mut manifest = denoise_manifest();
        manifest.capabilities.denoise = false;
        manifest.capabilities.subject_segmentation = true;
        let result = OrtDenoiser::new("/nonexistent/lumina/denoise.onnx", manifest);
        assert!(matches!(result, Err(OnnxError::UnsupportedModel { .. })));
    }

    #[test]
    fn tensor_name_error_lists_requested_and_available() {
        let available = vec!["out".to_owned(), "aux".to_owned()];
        let err = tensor_name_error("output", "missing", &available, "denoise");
        let text = err.to_string();
        assert!(text.contains("`missing`"), "{text}");
        assert!(text.contains("`out`, `aux`"), "{text}");
    }

    #[test]
    fn denoiser_implements_the_denoise_inference_surface() {
        fn assert_denoise<T: DenoiseInference>() {}
        assert_denoise::<OrtDenoiser>();
    }
}
