#![cfg(feature = "onnx-rt")]

//! Real ONNX Runtime backend (gated behind the `onnx-rt` feature).
//!
//! The `ort` crate (v2.0.0-rc.13) is fetchable and builds in this environment,
//! including its prebuilt ONNX Runtime binary download, so it is wired in here
//! behind a non-default feature. This implementation is **compile-verified** and
//! handles the [`OnnxError::MissingModel`] case without weights; numeric
//! correctness against an actual BiRefNet `.onnx` artifact (input/output tensor
//! names, value ranges) is validated later in F-048/F-082 once model weights are
//! provided. The default, fully tested surface remains the [`StubBackend`].

use crate::manifest::ModelManifest;
use crate::preprocess::{preprocess_rgb_to_model, rescale_model_matte};
use crate::{OnnxError, SubjectInference};
use lumina_core::{ImageFrame, MaskPlane};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// ONNX Runtime backed inference for an `.onnx` model artifact.
pub struct OrtBackend {
    manifest: ModelManifest,
    #[allow(dead_code)]
    model_path: PathBuf,
    // `ort::Session::run` requires `&mut self`; the shared `SubjectInference`
    // trait uses `&self`, so we use interior mutability.
    session: RefCell<ort::session::Session>,
}

impl OrtBackend {
    /// Load a session from `model_path`. Returns [`OnnxError::MissingModel`] if
    /// the artifact is absent and [`OnnxError::InferenceFailed`] if the session
    /// cannot be built.
    pub fn new(model_path: impl AsRef<Path>, manifest: ModelManifest) -> Result<Self, OnnxError> {
        manifest.validate()?;
        let model_path = model_path.as_ref().to_path_buf();
        if !model_path.exists() {
            return Err(OnnxError::MissingModel {
                path: model_path.display().to_string(),
            });
        }
        let session = ort::session::Session::builder()
            .and_then(|mut builder| builder.commit_from_file(&model_path))
            .map_err(|e| OnnxError::InferenceFailed {
                name: manifest.model_name.clone(),
                reason: format!("failed to load ONNX session: {e}"),
            })?;
        Ok(Self {
            manifest,
            model_path,
            session: RefCell::new(session),
        })
    }

    fn infer_inner(&self, image: &ImageFrame) -> Result<MaskPlane, OnnxError> {
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
        // NCHW float tensor in [0,1], shape [1, 3, H, W].
        let mut data = Vec::with_capacity((res.width * res.height * 3) as usize);
        for &channel in &rgb {
            data.push(channel as f32 / 255.0);
        }
        let tensor = ort::value::Tensor::from_array((
            vec![1i64, 3, res.height as i64, res.width as i64],
            data,
        ))
        .map_err(|e| OnnxError::InferenceFailed {
            name: self.manifest.model_name.clone(),
            reason: format!("failed to build input tensor: {e}"),
        })?;
        let mut session = self.session.borrow_mut();
        let outputs = session.run(ort::inputs!["input" => tensor]).map_err(|e| {
            OnnxError::InferenceFailed {
                name: self.manifest.model_name.clone(),
                reason: format!("inference failed: {e}"),
            }
        })?;
        let (_shape, raw) = outputs["output"].try_extract_tensor::<f32>().map_err(|e| {
            OnnxError::InferenceFailed {
                name: self.manifest.model_name.clone(),
                reason: format!("failed to read output tensor: {e}"),
            }
        })?;
        let mut values = Vec::with_capacity((res.width * res.height) as usize);
        for &v in raw {
            values.push(((v.clamp(0.0, 1.0) * 65535.0).round() as i32).clamp(0, 65535) as u16);
        }
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
}
