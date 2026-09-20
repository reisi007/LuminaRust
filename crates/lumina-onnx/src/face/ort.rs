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
//! The two shipped face weights carry verified `sha256:` pins (FACE-20-S6), so
//! an artifact is hash-checked against a real identity; the real graphs are
//! decoded through their declared contract (YuNet = twelve per-stride outputs
//! via `face::yunet`, SFace = `data`→`fc1`), see `face::pins`. A graph whose
//! tensors do not match the manifest is refused loudly at load. The
//! load/verify/contract paths are exercised against the committed behavior
//! fixtures in `tests/face_adapter_ort.rs` / `tests/face_ort.rs`; no download
//! occurs at build or test time.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use lumina_core::ImageFrame;

use crate::face::backend::{
    align_face_to_template, decode_face_detections, decode_face_embedding, DetectedFace,
    FaceDetectionInference, FaceEmbeddingInference, FaceEmbeddingVector,
};
use crate::face::ort_io::{
    detection_output_names, find_output, load_session, run_model, run_model_named,
};
use crate::face::yunet::{
    decode_yunet_detections, detection_head_for, FaceDetectHead, YunetStrideTensors, YUNET_STRIDES,
};
use crate::face::FaceInferenceOptions;
use crate::hash::{verify_model_file, ModelHashStatus};
use crate::manifest::ModelManifest;
use crate::OnnxError;

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
        let expected_outputs = detection_output_names(&manifest);
        let session = load_session(&path, &manifest, &expected_outputs)?;
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
        match detection_head_for(&self.manifest) {
            Some(FaceDetectHead::YuNetPerStride) => {
                let names = detection_output_names(&self.manifest);
                let outputs = run_model_named(&self.manifest, &mut session, image, &names)?;
                let mut per_stride = Vec::with_capacity(YUNET_STRIDES.len());
                for stride in YUNET_STRIDES {
                    let name = |kind: &str| format!("{kind}_{stride}");
                    per_stride.push(YunetStrideTensors {
                        stride,
                        cls: find_output(&outputs, &name("cls"), &self.manifest.model_name)?,
                        obj: find_output(&outputs, &name("obj"), &self.manifest.model_name)?,
                        bbox: find_output(&outputs, &name("bbox"), &self.manifest.model_name)?,
                        kps: find_output(&outputs, &name("kps"), &self.manifest.model_name)?,
                    });
                }
                let resolution = self.manifest.input.resolution;
                decode_yunet_detections(
                    &self.manifest.model_name,
                    resolution.width,
                    resolution.height,
                    &per_stride,
                    self.options.detection_score_threshold,
                    self.options.detection_nms_threshold,
                    self.options.detection_top_k as usize,
                )
            }
            None => {
                let (shape, data) = run_model(&self.manifest, &mut session, image)?;
                decode_face_detections(
                    &self.manifest.model_name,
                    &shape,
                    &data,
                    self.options.detection_score_threshold,
                )
            }
        }
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
        let session = load_session(
            &path,
            &manifest,
            std::slice::from_ref(&manifest.output_tensor_name),
        )?;
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
    use crate::face::ort_io::tensor_name_error;
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
