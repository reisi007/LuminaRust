//! Integration tests for model-artifact hash verification
//! (REVIEW-ONNX-HASH-1). Runs on the default feature set — the pure hashing
//! and status logic needs no ONNX Runtime, no model weights and no network.

use lumina_onnx::{
    birefnet_manifest, compute_sha256_hex, sam2_1_manifests, verify_model_file, verify_model_hash,
    ModelHashStatus, PENDING_INTEGRATION_HASH,
};
use std::io::Cursor;
use std::path::Path;

fn scratch_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "lumina-onnx-it-hash-{tag}-{}-{}.bin",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ))
}

/// Built-in descriptors keep using the documented placeholder until real,
/// hash-pinned fixtures are committed — they must therefore report `Pending`,
/// not `Verified`.
#[test]
fn builtin_descriptors_are_pending_not_verified() {
    assert_eq!(birefnet_manifest().model_hash, PENDING_INTEGRATION_HASH);
    for m in sam2_1_manifests() {
        assert_eq!(m.model_hash, PENDING_INTEGRATION_HASH);
        assert_eq!(
            verify_model_hash(&m.model_hash, "any-digest"),
            ModelHashStatus::Pending
        );
    }
}

/// A pinned identity is verified against the exact artifact bytes.
#[test]
fn pinned_hash_is_verified_against_artifact_bytes() {
    let path = scratch_path("verified");
    std::fs::write(&path, b"fake-onnx-weights").unwrap();
    let digest = compute_sha256_hex(Cursor::new(b"fake-onnx-weights".as_slice())).unwrap();
    assert_eq!(
        verify_model_file(&path, &digest).unwrap(),
        ModelHashStatus::Verified
    );
    let _ = std::fs::remove_file(&path);
}

/// A modified artifact under a pinned identity is stale — reported as
/// `Mismatch`, never coerced into a success (no silent fallbacks).
#[test]
fn changed_artifact_is_mismatched_under_pinned_identity() {
    let path = scratch_path("stale");
    std::fs::write(&path, b"original weights").unwrap();
    let digest = compute_sha256_hex(Cursor::new(b"tampered weights".as_slice())).unwrap();
    match verify_model_file(Path::new(&path), &digest).unwrap() {
        ModelHashStatus::Mismatch { expected, actual } => {
            // expected = digest of the *tampered* content passed as identity…
            assert_eq!(expected, digest);
            // …and actual differs from the on-disk original's digest.
            assert_ne!(actual, digest);
            assert!(!actual.is_empty());
        }
        other => panic!("expected Mismatch for changed artifact, got {other:?}"),
    }
    let _ = std::fs::remove_file(&path);
}

/// A vanished artifact stays a hard `MissingModel`, independent of status.
#[test]
fn missing_artifact_is_missing_model() {
    let err = verify_model_file(Path::new("/nonexistent/lumina-int/model.onnx"), "some-hash")
        .unwrap_err();
    assert!(
        matches!(err, lumina_onnx::OnnxError::MissingModel { .. }),
        "{err:?}"
    );
}

// F-082-FOLLOWUP-HASH — the backend refuse branch (`ModelArtifactStale`) is
// enforced through one shared, feature-free gate; these tests keep it covered
// in the default build (the end-to-end ORT variant lives behind `onnx-rt` in
// `tests/ort_backend.rs`).
#[test]
fn mismatch_status_enforces_model_artifact_stale() {
    let status = ModelHashStatus::Mismatch {
        expected: "pinned-identity".to_owned(),
        actual: "artifact-digest".to_owned(),
    };
    assert!(!status.allows_inference());
    match status.enforce_inference_allowed("BiRefNet") {
        Err(lumina_onnx::OnnxError::ModelArtifactStale {
            name,
            expected,
            actual,
        }) => {
            assert_eq!(name, "BiRefNet");
            assert_eq!(expected, "pinned-identity");
            assert_eq!(actual, "artifact-digest");
        }
        other => panic!("expected ModelArtifactStale, got {other:?}"),
    }
}

#[test]
fn verified_and_pending_statuses_allow_inference() {
    let pinned = "cafe01";
    assert!(verify_model_hash(pinned, pinned)
        .enforce_inference_allowed("BiRefNet")
        .is_ok());
    assert!(verify_model_hash(PENDING_INTEGRATION_HASH, "any-digest")
        .enforce_inference_allowed("BiRefNet")
        .is_ok());
}
