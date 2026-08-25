#![cfg(feature = "onnx-rt")]

//! Real ONNX Runtime backend (gated behind the `onnx-rt` feature).
//!
//! The `ort` crate (v2.0.0-rc.13) is fetchable and builds in this environment,
//! including its prebuilt ONNX Runtime binary download, so it is wired in here
//! behind a non-default feature. This implementation is **compile-verified**
//! and handles the [`OnnxError::MissingModel`] case without weights; numeric
//! correctness against an actual BiRefNet `.onnx` artifact (input/output tensor
//! names, value ranges) is validated later in F-048/F-082 once model weights are
//! provided. The default, fully tested surface remains the [`StubBackend`].
//!
//! ## Artifact identity (REVIEW-ONNX-HASH-1)
//!
//! Loading verifies the artifact bytes against the manifest `model_hash`
//! (SHA-256, see [`crate::hash`]). The resulting [`ModelHashStatus`] is kept
//! on the backend and queryable via [`OrtBackend::hash_status`]:
//!
//! * `Verified` — artifact matches the pinned identity;
//! * `Pending` — the manifest still carries the documented
//!   `pending-integration` placeholder, so nothing can be checked yet;
//! * `Mismatch` — **stale/mismatched weights**: inference is refused with
//!   [`OnnxError::ModelArtifactStale`] instead of silently running wrong
//!   weights.
//!
//! ## Preprocessing contract (REVIEW-ONNX-PREPROC-1)
//!
//! Input normalization (ImageNet mean/std), the input tensor name, the output
//! tensor name and the expected output shape all come from the
//! [`ModelManifest`] — nothing is hardcoded here anymore. The output tensor
//! must match the declared inference resolution exactly or inference fails;
//! there are no silent reshapes.

use crate::hash::{verify_model_file, ModelHashStatus};
use crate::manifest::ModelManifest;
use crate::preprocess::{
    matte_values_from_unit_f32, normalize_rgb_to_nchw, preprocess_rgb_to_model,
    rescale_model_matte, validate_output_shape,
};
use crate::{OnnxError, SubjectInference};
use lumina_core::{ImageFrame, MaskPlane};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// ONNX Runtime backed inference for an `.onnx` model artifact.
pub struct OrtBackend {
    manifest: ModelManifest,
    #[allow(dead_code)]
    model_path: PathBuf,
    /// Result of verifying the loaded artifact against the manifest
    /// `model_hash` (see [`Self::hash_status`]).
    hash_status: ModelHashStatus,
    // `ort::Session::run` requires `&mut self`; the shared `SubjectInference`
    // trait uses `&self`, so we use interior mutability.
    session: RefCell<ort::session::Session>,
}

impl OrtBackend {
    /// Load a session from `model_path`. Returns [`OnnxError::MissingModel`] if
    /// the artifact is absent/unreadable and [`OnnxError::InferenceFailed`] if
    /// the session cannot be built. The artifact's SHA-256 digest is computed
    /// here once and compared against `manifest.model_hash`
    /// ([`Self::hash_status`]); a mismatch does not prevent loading but makes
    /// every subsequent [`SubjectInference::infer`] fail with
    /// [`OnnxError::ModelArtifactStale`] — stale weights never run silently.
    pub fn new(model_path: impl AsRef<Path>, manifest: ModelManifest) -> Result<Self, OnnxError> {
        manifest.validate()?;
        let model_path = model_path.as_ref().to_path_buf();
        if !model_path.exists() {
            return Err(OnnxError::MissingModel {
                path: model_path.display().to_string(),
            });
        }
        let hash_status = verify_model_file(&model_path, &manifest.model_hash)?;
        let session = ort::session::Session::builder()
            .and_then(|mut builder| builder.commit_from_file(&model_path))
            .map_err(|e| OnnxError::InferenceFailed {
                name: manifest.model_name.clone(),
                reason: format!("failed to load ONNX session: {e}"),
            })?;
        Ok(Self {
            manifest,
            model_path,
            hash_status,
            session: RefCell::new(session),
        })
    }

    /// Result of verifying the loaded artifact against the manifest
    /// `model_hash`. [`ModelHashStatus::Mismatch`] marks stale/mismatched
    /// weights; [`ModelHashStatus::Pending`] marks the documented pre-integration
    /// state (`pending-integration`) in which no pinned identity exists yet.
    pub fn hash_status(&self) -> &ModelHashStatus {
        &self.hash_status
    }

    fn infer_inner(&self, image: &ImageFrame) -> Result<MaskPlane, OnnxError> {
        // Stale/mismatched weights are refused — visible failure instead of a
        // silent fallback (REVIEW-ONNX-HASH-1).
        if let ModelHashStatus::Mismatch { expected, actual } = &self.hash_status {
            return Err(OnnxError::ModelArtifactStale {
                name: self.manifest.model_name.clone(),
                expected: expected.clone(),
                actual: actual.clone(),
            });
        }
        if image.width == 0 || image.height == 0 {
            return Err(OnnxError::InvalidDimensions {
                expected_width: self.manifest.input.resolution.width,
                expected_height: self.manifest.input.resolution.height,
                actual_width: image.width,
                actual_height: image.height,
            });
        }
        let res = self.manifest.input.resolution;
        let rgb = preprocess_rgb_to_model(image, res);
        // Normalization (ImageNet mean/std) and CHW ordering per manifest
        // (REVIEW-ONNX-PREPROC-1); NCHW float tensor, shape [1, 3, H, W].
        let data = normalize_rgb_to_nchw(
            &rgb,
            &self.manifest.model_name,
            &self.manifest.input.normalization,
        )?;
        let tensor = ort::value::Tensor::from_array((
            vec![1i64, 3, res.height as i64, res.width as i64],
            data,
        ))
        .map_err(|e| OnnxError::InferenceFailed {
            name: self.manifest.model_name.clone(),
            reason: format!("failed to build input tensor: {e}"),
        })?;
        // Input/output tensor names come from the manifest, not hardcoded.
        let input_name = self.manifest.input.tensor_name.clone();
        let mut session = self.session.borrow_mut();
        let outputs = session
            .run(ort::inputs![input_name => tensor])
            .map_err(|e| OnnxError::InferenceFailed {
                name: self.manifest.model_name.clone(),
                reason: format!("inference failed: {e}"),
            })?;
        let output_name = self.manifest.output_tensor_name.as_str();
        let (shape, raw) = outputs[output_name]
            .try_extract_tensor::<f32>()
            .map_err(|e| OnnxError::InferenceFailed {
                name: self.manifest.model_name.clone(),
                reason: format!("failed to read output tensor `{output_name}`: {e}"),
            })?;
        // The emitted matte must match what the manifest claims — no silent
        // reshapes/truncations of unexpected output shapes.
        validate_output_shape(shape, res, &self.manifest.model_name)?;
        if raw.len() != (res.width * res.height) as usize {
            return Err(OnnxError::InferenceFailed {
                name: self.manifest.model_name.clone(),
                reason: format!(
                    "output tensor `{}` holds {} values, expected {}",
                    output_name,
                    raw.len(),
                    res.width * res.height
                ),
            });
        }
        let values = matte_values_from_unit_f32(raw);
        let model_matte = MaskPlane::new(res.width, res.height, values).map_err(|_| {
            OnnxError::InvalidDimensions {
                expected_width: res.width,
                expected_height: res.height,
                actual_width: res.width,
                actual_height: res.height,
            }
        })?;
        rescale_model_matte(&model_matte, res, (image.width, image.height))
    }
}

impl SubjectInference for OrtBackend {
    fn manifest(&self) -> &ModelManifest {
        &self.manifest
    }

    fn infer(&self, image: &ImageFrame) -> Result<MaskPlane, OnnxError> {
        self.infer_inner(image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::birefnet_manifest;

    #[test]
    fn reports_missing_model_artifact() {
        let backend = OrtBackend::new("/nonexistent/path/to/model.onnx", birefnet_manifest());
        assert!(
            matches!(backend, Err(OnnxError::MissingModel { .. })),
            "absent artifact must surface as MissingModel"
        );
    }

    /// A present-but-garbage artifact fails at hash/session level, never
    /// silently. (The pure hash/status logic itself is tested feature-free in
    /// `crate::hash` and `tests/model_hash.rs`.)
    #[test]
    fn garbage_artifact_does_not_load_as_success() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "lumina-onnx-garbage-{}-{}.bin",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&path, b"not an onnx model").unwrap();
        let result = OrtBackend::new(&path, birefnet_manifest());
        let _ = std::fs::remove_file(&path);
        assert!(result.is_err(), "garbage artifact must not load");
    }
}
