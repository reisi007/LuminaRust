//! LRPAR-G12-FACE-20 / FACE-20-S2 integration tests for the real ONNX Runtime
//! face path (`onnx-rt` feature).
//!
//! Real face weights are neither committed nor downloadable (Agents.md: no
//! spontaneous downloads), so — exactly like the subject-model tests in
//! `ort_backend.rs` — these tests use the committed, hash-pinned behavior
//! fixture `fixtures/lumina-crafted-reducemax.onnx` (a minimal, deterministic
//! ReduceMax graph, no model weights). It exercises the load/verify/tensor-name
//! paths and the canonical-contract refusal without claiming numeric detection
//! correctness against real weights.
//!
//! Covered gates:
//!
//! * missing artifact → [`lumina_onnx::OnnxError::MissingModel`];
//! * digest ≠ pinned hash → `ModelArtifactStale` at inference;
//! * manifest tensor name absent from the graph → `InferenceFailed` at load;
//! * output shape violating the canonical contract → `InferenceFailed`
//!   (never a silent reshape);
//! * `try_load_face_engine` resolves the real engine or fails visibly — never
//!   a stub fallback.

#![cfg(feature = "onnx-rt")]

use lumina_core::ImageFrame;
use lumina_onnx::face::ort::{OrtFaceDetector, OrtFaceEmbedder};
use lumina_onnx::{
    compute_sha256_hex, try_load_face_engine, ChannelLayout, DetectedFace, FaceDetectionInference,
    FaceEmbeddingInference, FaceInferenceOptions, FaceModelSuite, FaceOnnxEngine,
    InputNormalization, ModelCapabilities, ModelHashStatus, ModelInputSpec, ModelManifest,
    OnnxError, Resolution, TensorFormat, PENDING_INTEGRATION_HASH,
};
use lumina_sidecar::{FaceBoundingBox, FaceLandmark};
use std::io::Cursor;
use std::path::Path;

/// SHA-256 pin of the committed behavior fixture (see `tests/fixtures/README.md`).
const FIXTURE_PIN: &str = "2a2ede6659e8c59b3fd972242b27677ef23cb98d3c422616a1c65f50dcaca18d";
const FIXTURE_BYTES: &[u8] = include_bytes!("fixtures/lumina-crafted-reducemax.onnx");

const W: u32 = 8;
const H: u32 = 8;
const INPUT_NAME: &str = "x";
const OUTPUT_NAME: &str = "y";

fn write_fixture(tag: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "lumina-onnx-face-{tag}-{}-{}.onnx",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&path, FIXTURE_BYTES).expect("write face fixture");
    path
}

fn frame(width: u32, height: u32) -> ImageFrame {
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for px in pixels.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    ImageFrame::new(width, height, pixels).unwrap()
}

fn detect_manifest(model_hash: String, input: &str, output: &str) -> ModelManifest {
    ModelManifest {
        model_name: "CraftedFaceDetect".into(),
        model_version: "0.0.1".into(),
        model_hash,
        license: "MIT".into(),
        input: ModelInputSpec {
            resolution: Resolution {
                width: W,
                height: H,
            },
            channel_layout: ChannelLayout::Rgb,
            tensor_name: input.into(),
            tensor_format: TensorFormat::Nchw,
            normalization: InputNormalization::IMAGENET,
        },
        output_tensor_name: output.into(),
        capabilities: ModelCapabilities {
            face_detect: true,
            ..Default::default()
        },
    }
}

fn embed_manifest(model_hash: String, input: &str, output: &str) -> ModelManifest {
    ModelManifest {
        model_name: "CraftedFaceEmbed".into(),
        model_version: "0.0.1".into(),
        model_hash,
        license: "MIT".into(),
        input: ModelInputSpec {
            resolution: Resolution {
                width: W,
                height: H,
            },
            channel_layout: ChannelLayout::Rgb,
            tensor_name: input.into(),
            tensor_format: TensorFormat::Nchw,
            normalization: InputNormalization::IMAGENET,
        },
        output_tensor_name: output.into(),
        capabilities: ModelCapabilities {
            face_embed: true,
            ..Default::default()
        },
    }
}

fn face() -> DetectedFace {
    let landmark = |name: &str, x: f32, y: f32| FaceLandmark {
        name: name.into(),
        x,
        y,
    };
    DetectedFace {
        bbox: FaceBoundingBox {
            x: 0.25,
            y: 0.25,
            width: 0.5,
            height: 0.5,
        },
        score: 0.9,
        landmarks: vec![
            landmark("left_eye", 0.35, 0.40),
            landmark("right_eye", 0.65, 0.40),
            landmark("nose", 0.50, 0.50),
            landmark("mouth_left", 0.40, 0.65),
            landmark("mouth_right", 0.60, 0.65),
        ],
    }
}

#[test]
fn fixture_matches_its_documented_pin() {
    let digest = compute_sha256_hex(Cursor::new(FIXTURE_BYTES)).unwrap();
    assert_eq!(digest, FIXTURE_PIN, "fixture drifted from its pin");
}

#[test]
fn face_detector_reports_missing_model() {
    let result = OrtFaceDetector::new(
        "/nonexistent/lumina/face-detect.onnx",
        detect_manifest(PENDING_INTEGRATION_HASH.into(), INPUT_NAME, OUTPUT_NAME),
        FaceInferenceOptions::default(),
    );
    assert!(matches!(result, Err(OnnxError::MissingModel { .. })));
}

#[test]
fn face_embedder_reports_missing_model() {
    let result = OrtFaceEmbedder::new(
        "/nonexistent/lumina/face-embed.onnx",
        embed_manifest(PENDING_INTEGRATION_HASH.into(), INPUT_NAME, OUTPUT_NAME),
        64,
    );
    assert!(matches!(result, Err(OnnxError::MissingModel { .. })));
}

#[test]
fn face_detector_refuses_stale_weights_end_to_end() {
    let path = write_fixture("detect-stale");
    let pinned = format!("{:064x}", 0xdead_beef_u64);
    let backend = OrtFaceDetector::new(
        &path,
        detect_manifest(pinned.clone(), INPUT_NAME, OUTPUT_NAME),
        FaceInferenceOptions::default(),
    )
    .expect("a hash mismatch must not prevent loading (status is queryable)");
    match backend.hash_status() {
        ModelHashStatus::Mismatch { expected, .. } => assert_eq!(expected, &pinned),
        other => panic!("expected Mismatch, got {other:?}"),
    }
    let err = backend.detect(&frame(4, 4)).unwrap_err();
    let _ = std::fs::remove_file(&path);
    assert!(
        matches!(err, OnnxError::ModelArtifactStale { .. }),
        "stale weights must refuse inference, got {err:?}"
    );
}

#[test]
fn face_detector_rejects_unknown_tensor_names_at_load() {
    let path = write_fixture("detect-names");
    let result = OrtFaceDetector::new(
        &path,
        detect_manifest(FIXTURE_PIN.into(), INPUT_NAME, "does-not-exist"),
        FaceInferenceOptions::default(),
    );
    let _ = std::fs::remove_file(&path);
    match result {
        Err(OnnxError::InferenceFailed { reason, .. }) => {
            assert!(reason.contains("`does-not-exist`"), "{reason}");
            assert!(reason.contains("`y`"), "{reason}");
        }
        Err(other) => panic!("expected InferenceFailed, got {other:?}"),
        Ok(_) => panic!("a manifest with a wrong output name must not load"),
    }
}

/// The input-name branch of `load_session` is checked *before* the output
/// branch. This is exactly where a real `SFace` graph fails today (`data` ≠
/// the manifest's `input`), so the refusal must be loud and must list the
/// available inputs — never a silent load with the wrong tensor.
#[test]
fn face_detector_rejects_unknown_input_tensor_names_at_load() {
    let path = write_fixture("detect-input-names");
    let result = OrtFaceDetector::new(
        &path,
        detect_manifest(FIXTURE_PIN.into(), "does-not-exist", OUTPUT_NAME),
        FaceInferenceOptions::default(),
    );
    let _ = std::fs::remove_file(&path);
    match result {
        Err(OnnxError::InferenceFailed { reason, .. }) => {
            assert!(reason.contains("`does-not-exist`"), "{reason}");
            assert!(reason.contains("input"), "{reason}");
            assert!(reason.contains("`x`"), "{reason}");
        }
        Err(other) => panic!("expected InferenceFailed, got {other:?}"),
        Ok(_) => panic!("a manifest with a wrong input name must not load"),
    }
}

/// The fixture is loadable and hash-verifies, but its rank-4 matte-shaped
/// output violates the canonical detection contract — a loud `InferenceFailed`,
/// never a silent reshape into detections.
#[test]
fn face_detector_refuses_contract_violating_output_shape() {
    let path = write_fixture("detect-shape");
    let backend = OrtFaceDetector::new(
        &path,
        detect_manifest(FIXTURE_PIN.into(), INPUT_NAME, OUTPUT_NAME),
        FaceInferenceOptions::default(),
    )
    .expect("the pinned fixture must load");
    assert_eq!(backend.hash_status(), &ModelHashStatus::Verified);
    let err = backend.detect(&frame(4, 4)).unwrap_err();
    let _ = std::fs::remove_file(&path);
    match err {
        OnnxError::InferenceFailed { reason, .. } => {
            assert!(reason.contains("detection output shape"), "{reason}");
        }
        other => panic!("expected InferenceFailed, got {other:?}"),
    }
}

/// The `pending-integration` placeholder is not refused at load (it is the
/// documented pre-integration state), but the contract violation is still
/// loud — `Pending` never means "guess".
#[test]
fn face_detector_pending_placeholder_loads_but_refuses_bad_shape() {
    let path = write_fixture("detect-pending");
    let backend = OrtFaceDetector::new(
        &path,
        detect_manifest(PENDING_INTEGRATION_HASH.into(), INPUT_NAME, OUTPUT_NAME),
        FaceInferenceOptions::default(),
    )
    .expect("pending placeholder must load");
    assert_eq!(backend.hash_status(), &ModelHashStatus::Pending);
    let err = backend.detect(&frame(4, 4)).unwrap_err();
    let _ = std::fs::remove_file(&path);
    assert!(matches!(err, OnnxError::InferenceFailed { .. }), "{err:?}");
}

/// The embedder runs the deterministic 5-point alignment before the session;
/// the fixture's spatial output is then refused loudly (no silent flattening).
#[test]
fn face_embedder_aligns_then_refuses_contract_violating_output() {
    let path = write_fixture("embed-shape");
    let backend = OrtFaceEmbedder::new(
        &path,
        embed_manifest(FIXTURE_PIN.into(), INPUT_NAME, OUTPUT_NAME),
        64,
    )
    .expect("the pinned fixture must load");
    assert_eq!(backend.hash_status(), &ModelHashStatus::Verified);
    let err = backend.embed(&frame(64, 64), &[face()]).unwrap_err();
    let _ = std::fs::remove_file(&path);
    match err {
        OnnxError::InferenceFailed { reason, .. } => {
            assert!(reason.contains("embedding output shape"), "{reason}");
        }
        other => panic!("expected InferenceFailed, got {other:?}"),
    }
}

#[test]
fn engine_resolver_is_explicit_and_never_falls_back() {
    // Missing artifacts → hard error, never a stub.
    let suite = FaceModelSuite::candidate();
    let missing = try_load_face_engine(
        Path::new("/nonexistent/lumina/detect.onnx"),
        &suite,
        Path::new("/nonexistent/lumina/embed.onnx"),
        &FaceInferenceOptions::default(),
    );
    assert!(matches!(missing, Err(OnnxError::MissingModel { .. })));

    // A loadable pair resolves to the real engine.
    let detect_path = write_fixture("engine-detect");
    let embed_path = write_fixture("engine-embed");
    let suite = FaceModelSuite::new(
        detect_manifest(FIXTURE_PIN.into(), INPUT_NAME, OUTPUT_NAME),
        embed_manifest(FIXTURE_PIN.into(), INPUT_NAME, OUTPUT_NAME),
        64,
    )
    .unwrap();
    let engine = try_load_face_engine(
        &detect_path,
        &suite,
        &embed_path,
        &FaceInferenceOptions::default(),
    )
    .expect("pinned fixtures must resolve");
    let _ = std::fs::remove_file(&detect_path);
    let _ = std::fs::remove_file(&embed_path);
    assert!(
        matches!(engine, FaceOnnxEngine::OnnxRuntime { .. }),
        "expected the real engine, got {engine:?}"
    );
}
