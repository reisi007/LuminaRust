#![cfg(feature = "onnx-rt")]

//! Real ONNX Runtime face backends (gated behind the `onnx-rt` feature).
//!
//! Two sessions are managed here, mirroring the subject path in
//! `ort_backend.rs`:
//!
//! * [`OrtFaceDetector`] — whole-image detection (canonical detection output
//!   contract, see [`crate::face::backend::decode_face_detections`]);
//! * [`OrtFaceEmbedder`] — deterministic 5-point similarity alignment
//!   ([`crate::face::backend::align_face_to_template`]) followed by the
//!   embedding session (canonical embedding output contract).
//!
//! ## Artifact identity (no silent fallback)
//!
//! Construction verifies the artifact bytes against the manifest `model_hash`
//! (SHA-256, [`crate::hash`]) and validates that both manifest-declared tensor
//! names exist in the graph. The resulting [`ModelHashStatus`] is kept on the
//! backend ([`OrtFaceDetector::hash_status`]): a mismatch does not prevent
//! loading but makes every inference fail with
//! [`OnnxError::ModelArtifactStale`] — stale weights never run silently.
//!
//! Failure mapping (identical to the subject backend):
//!
//! | situation | error |
//! | --- | --- |
//! | artifact absent/unreadable | [`OnnxError::MissingModel`] |
//! | artifact digest ≠ pinned hash | [`OnnxError::ModelArtifactStale`] at inference |
//! | manifest tensor name absent from the graph | [`OnnxError::InferenceFailed`] at load |
//! | output shape violates the canonical contract | [`OnnxError::InferenceFailed`] |
//! | manifest lacks the face capability | [`OnnxError::UnsupportedModel`] |
//!
//! No path substitutes a stub or a different model: a call that cannot run
//! returns an error the caller must surface.
//!
//! Real weights remain `pending-integration` (Agents.md: no spontaneous
//! downloads), so numeric correctness against a real YuNet/SFace artifact is
//! validated when weights land; the load/verify/contract paths are exercised
//! against the committed behavior fixture in `tests/face_ort.rs`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use lumina_core::ImageFrame;

use crate::face::backend::{
    align_face_to_template, decode_face_detections, decode_face_embedding, DetectedFace,
    FaceDetectionInference, FaceEmbeddingInference, FaceEmbeddingVector,
};
use crate::face::FaceInferenceOptions;
use crate::hash::{verify_model_file, ModelHashStatus};
use crate::manifest::ModelManifest;
use crate::preprocess::{normalize_rgb_to_nchw, preprocess_rgb_to_model};
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

/// Load a session and validate its declared I/O tensor names against
/// `manifest`.
fn load_session(
    model_path: &Path,
    manifest: &ModelManifest,
) -> Result<ort::session::Session, OnnxError> {
    if !model_path.exists() {
        return Err(OnnxError::MissingModel {
            path: model_path.display().to_string(),
        });
    }
    let session = ort::session::Session::builder()
        .and_then(|mut builder| builder.commit_from_file(model_path))
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
    Ok(session)
}

/// Run one single-output session for `image`, returning the raw `f32` output
/// and its shape (as `usize` axes). Preprocessing uses the manifest's declared
/// resolution, tensor name and normalization.
fn run_model(
    manifest: &ModelManifest,
    session: &mut ort::session::Session,
    image: &ImageFrame,
) -> Result<(Vec<usize>, Vec<f32>), OnnxError> {
    let res = manifest.input.resolution;
    let rgb = preprocess_rgb_to_model(image, res);
    let data = normalize_rgb_to_nchw(&rgb, &manifest.model_name, &manifest.input.normalization)?;
    let tensor =
        ort::value::Tensor::from_array((vec![1i64, 3, res.height as i64, res.width as i64], data))
            .map_err(|error| OnnxError::InferenceFailed {
                name: manifest.model_name.clone(),
                reason: format!("failed to build input tensor: {error}"),
            })?;
    let input_name = manifest.input.tensor_name.clone();
    let outputs = session
        .run(ort::inputs![input_name => tensor])
        .map_err(|error| OnnxError::InferenceFailed {
            name: manifest.model_name.clone(),
            reason: format!("inference failed: {error}"),
        })?;
    let output_name = manifest.output_tensor_name.as_str();
    let available: Vec<&str> = outputs.keys().collect();
    let output = outputs.get(output_name).ok_or_else(|| {
        tensor_name_error("output", output_name, &available, &manifest.model_name)
    })?;
    let (shape, raw) =
        output
            .try_extract_tensor::<f32>()
            .map_err(|error| OnnxError::InferenceFailed {
                name: manifest.model_name.clone(),
                reason: format!("failed to read output tensor `{output_name}`: {error}"),
            })?;
    let shape: Vec<usize> = shape.iter().map(|axis| *axis as usize).collect();
    Ok((shape, raw.to_vec()))
}

/// ONNX Runtime backed face detector.
pub struct OrtFaceDetector {
    manifest: ModelManifest,
    options: FaceInferenceOptions,
    hash_status: ModelHashStatus,
    path: PathBuf,
    session: RefCell<ort::session::Session>,
}

impl OrtFaceDetector {
    /// Load a detection session, verifying the artifact identity and the
    /// manifest-declared tensor names. See the module docs for the error map.
    pub fn new(
        model_path: impl AsRef<Path>,
        manifest: ModelManifest,
        options: FaceInferenceOptions,
    ) -> Result<Self, OnnxError> {
        manifest.validate()?;
        if !manifest.capabilities.face_detect {
            return Err(OnnxError::UnsupportedModel {
                name: manifest.model_name.clone(),
                reason: "face_detect not declared".into(),
            });
        }
        options.validate()?;
        let path = model_path.as_ref().to_path_buf();
        let hash_status = verify_model_file(&path, &manifest.model_hash)?;
        let session = load_session(&path, &manifest)?;
        Ok(Self {
            manifest,
            options,
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

    fn detect_inner(&self, image: &ImageFrame) -> Result<Vec<DetectedFace>, OnnxError> {
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
        let mut session = self.session.borrow_mut();
        let (shape, data) = run_model(&self.manifest, &mut session, image)?;
        decode_face_detections(
            &self.manifest.model_name,
            &shape,
            &data,
            self.options.detection_score_threshold,
        )
    }
}

impl FaceDetectionInference for OrtFaceDetector {
    fn manifest(&self) -> &ModelManifest {
        &self.manifest
    }

    fn detect(&self, image: &ImageFrame) -> Result<Vec<DetectedFace>, OnnxError> {
        self.detect_inner(image)
    }
}

/// ONNX Runtime backed face embedder.
pub struct OrtFaceEmbedder {
    manifest: ModelManifest,
    dimension: u32,
    hash_status: ModelHashStatus,
    path: PathBuf,
    session: RefCell<ort::session::Session>,
}

impl OrtFaceEmbedder {
    /// Load an embedding session, verifying the artifact identity and the
    /// manifest-declared tensor names.
    pub fn new(
        model_path: impl AsRef<Path>,
        manifest: ModelManifest,
        dimension: u32,
    ) -> Result<Self, OnnxError> {
        manifest.validate()?;
        if !manifest.capabilities.face_embed {
            return Err(OnnxError::UnsupportedModel {
                name: manifest.model_name.clone(),
                reason: "face_embed not declared".into(),
            });
        }
        if dimension == 0 {
            return Err(OnnxError::InvalidFaceData(
                "face embedding dimension must be > 0".into(),
            ));
        }
        let path = model_path.as_ref().to_path_buf();
        let hash_status = verify_model_file(&path, &manifest.model_hash)?;
        let session = load_session(&path, &manifest)?;
        Ok(Self {
            manifest,
            dimension,
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

    fn embed_inner(
        &self,
        image: &ImageFrame,
        detections: &[DetectedFace],
    ) -> Result<Vec<FaceEmbeddingVector>, OnnxError> {
        self.hash_status
            .enforce_inference_allowed(&self.manifest.model_name)?;
        if detections.is_empty() {
            return Ok(Vec::new());
        }
        if image.width == 0 || image.height == 0 {
            return Err(OnnxError::InvalidDimensions {
                expected_width: self.manifest.input.resolution.width,
                expected_height: self.manifest.input.resolution.height,
                actual_width: image.width,
                actual_height: image.height,
            });
        }
        let resolution = (
            self.manifest.input.resolution.width,
            self.manifest.input.resolution.height,
        );
        let mut session = self.session.borrow_mut();
        let mut embeddings = Vec::with_capacity(detections.len());
        for detection in detections {
            // Deterministic 5-point alignment, then the embedding session.
            let aligned = align_face_to_template(image, &detection.landmarks, resolution)?;
            let (shape, data) = run_model(&self.manifest, &mut session, &aligned)?;
            embeddings.push(decode_face_embedding(
                &self.manifest.model_name,
                &shape,
                &data,
                self.dimension,
            )?);
        }
        Ok(embeddings)
    }
}

impl FaceEmbeddingInference for OrtFaceEmbedder {
    fn manifest(&self) -> &ModelManifest {
        &self.manifest
    }

    fn dimension(&self) -> u32 {
        self.dimension
    }

    fn embed(
        &self,
        image: &ImageFrame,
        detections: &[DetectedFace],
    ) -> Result<Vec<FaceEmbeddingVector>, OnnxError> {
        self.embed_inner(image, detections)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::face::{face_detect_manifest, face_embed_manifest};

    #[test]
    fn detector_reports_missing_model_artifact() {
        let result = OrtFaceDetector::new(
            "/nonexistent/lumina/face-detect.onnx",
            face_detect_manifest(),
            FaceInferenceOptions::default(),
        );
        assert!(
            matches!(result, Err(OnnxError::MissingModel { .. })),
            "absent artifact must surface as MissingModel"
        );
    }

    #[test]
    fn embedder_reports_missing_model_artifact() {
        let result = OrtFaceEmbedder::new(
            "/nonexistent/lumina/face-embed.onnx",
            face_embed_manifest(),
            crate::face::FACE_EMBED_DIMENSION,
        );
        assert!(matches!(result, Err(OnnxError::MissingModel { .. })));
    }

    #[test]
    fn detector_rejects_wrong_capability_manifest() {
        let result = OrtFaceDetector::new(
            "/nonexistent/lumina/face.onnx",
            face_embed_manifest(),
            FaceInferenceOptions::default(),
        );
        assert!(matches!(result, Err(OnnxError::UnsupportedModel { .. })));
    }

    #[test]
    fn tensor_name_error_lists_requested_and_available() {
        let available = vec!["out".to_owned(), "aux".to_owned()];
        let err = tensor_name_error("output", "missing", &available, "YuNet");
        let text = err.to_string();
        assert!(text.contains("`missing`"), "{text}");
        assert!(text.contains("`out`, `aux`"), "{text}");
    }

    #[test]
    fn both_backends_implement_the_face_inference_surfaces() {
        fn assert_detector<T: FaceDetectionInference>() {}
        fn assert_embedder<T: FaceEmbeddingInference>() {}
        assert_detector::<OrtFaceDetector>();
        assert_embedder::<OrtFaceEmbedder>();
    }
}
