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
//! ## Manifest ↔ artifact I/O contract (F-082-FOLLOWUP-ORT)
//!
//! The manifest declares the graph's tensor names, so loading validates them
//! against the session's actual inputs/outputs: a mismatch fails at load with
//! [`OnnxError::InferenceFailed`] instead of panicking at first inference.
//! The runtime output lookup stays defensive as well — an unknown name maps
//! to [`OnnxError::InferenceFailed`], never a panic (`ort`'s `Index` impl
//! would panic; we use `SessionOutputs::get`).
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

/// Build the shared, descriptive [`OnnxError::InferenceFailed`] for a
/// manifest-declared tensor name that does not exist in the loaded graph
/// (F-082-FOLLOWUP-ORT). Used by both the load-time validation and the
/// defensive runtime output lookup, so every mismatch reports *what* was
/// requested and *what the artifact actually provides*.
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
    /// the session cannot be built or if the manifest-declared input/output
    /// tensor names do not exist in the graph (F-082-FOLLOWUP-ORT: a name
    /// mismatch is a visible load error, never a panic at inference time).
    /// The artifact's SHA-256 digest is computed here once and compared
    /// against `manifest.model_hash` ([`Self::hash_status`]); a mismatch does
    /// not prevent loading but makes every subsequent
    /// [`SubjectInference::infer`] fail with [`OnnxError::ModelArtifactStale`]
    /// — stale weights never run silently.
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
        // The manifest is the I/O contract: both declared tensor names must
        // exist in the loaded graph, otherwise every later inference would be
        // doomed (and `ort`'s output indexing would panic). Fail visibly here.
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
        // silent fallback (REVIEW-ONNX-HASH-1). The shared gate is unit-tested
        // feature-free in `crate::hash` / `tests/model_hash.rs` and end-to-end
        // against a loadable artifact in `tests/ort_backend.rs`.
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
        // `SessionOutputs::get` returns an Option — unlike the `Index` impl,
        // an unknown name can never panic here (F-082-FOLLOWUP-ORT).
        let available_outputs: Vec<&str> = outputs.keys().collect();
        let output = outputs.get(output_name).ok_or_else(|| {
            tensor_name_error(
                "output",
                output_name,
                &available_outputs,
                &self.manifest.model_name,
            )
        })?;
        let (shape, raw) =
            output
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

/// Bridge the native ORT adapter to `lumina-core`'s platform-neutral
/// [`lumina_core::MaskInference`] trait (R2-ONNX-03).
///
/// Previously only [`StubBackend`](crate::StubBackend) implemented this bridge,
/// so the real backend could not be plugged into the F-048/F-051 mask-loading
/// decision layer without additive caller code — an asymmetric contract. The
/// mapping mirrors `backend.rs` exactly: adapter errors become
/// [`lumina_core::CoreError::MaskInference`] with the full reason text, never a
/// silent fallback.
impl lumina_core::MaskInference for OrtBackend {
    fn is_available(&self) -> bool {
        // A successfully constructed `OrtBackend` holds a loaded session with
        // manifest-verified tensor names; unavailability surfaces at
        // construction time as `OnnxError::MissingModel`, so there is no
        // post-construction "not available" state to report.
        true
    }

    fn infer(&self, frame: &ImageFrame) -> Result<MaskPlane, lumina_core::CoreError> {
        self.infer_inner(frame)
            .map_err(|error| lumina_core::CoreError::MaskInference {
                reason: error.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::birefnet_manifest;

    /// R2-ONNX-03 — the real backend must satisfy the same `lumina-core`
    /// bridge contract as the stub, so it can be plugged into the F-048/F-051
    /// decision layer without additive caller code. (A behavioral test needs
    /// real weights — pending F-048 — so the contract is pinned at the type
    /// level here; the error mapping itself is identical to the unit-tested
    /// `StubBackend` bridge.)
    #[test]
    fn ort_backend_implements_core_mask_inference_bridge() {
        fn assert_mask_inference<T: lumina_core::MaskInference>() {}
        assert_mask_inference::<OrtBackend>();
    }

    #[test]
    fn reports_missing_model_artifact() {
        let backend = OrtBackend::new("/nonexistent/path/to/model.onnx", birefnet_manifest());
        assert!(
            matches!(backend, Err(OnnxError::MissingModel { .. })),
            "absent artifact must surface as MissingModel"
        );
    }

    /// The shared F-082-FOLLOWUP-ORT error always names the requested tensor
    /// and lists what the artifact actually provides (or `<none>`).
    #[test]
    fn tensor_name_error_lists_requested_and_available() {
        let available = vec!["alpha_matte".to_owned(), "aux".to_owned()];
        let err = tensor_name_error("output", "missing_out", &available, "BiRefNet");
        match &err {
            OnnxError::InferenceFailed { name, reason } => {
                assert_eq!(name, "BiRefNet");
                assert!(reason.contains("`missing_out`"), "{reason}");
                assert!(reason.contains("`alpha_matte`, `aux`"), "{reason}");
            }
            other => panic!("expected InferenceFailed, got {other:?}"),
        }

        let empty: Vec<String> = Vec::new();
        let err = tensor_name_error("input", "x", &empty, "M");
        assert!(err.to_string().contains("<none>"), "{err}");
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
