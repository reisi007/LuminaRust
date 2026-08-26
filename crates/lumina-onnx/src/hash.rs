//! Model-artifact hash verification (REVIEW-ONNX-HASH-1).
//!
//! The manifest's `model_hash` is the documented identity of the exact weights
//! (`feature/product/ai-masks.md`, F-004/F-082: "Modellname, Modellversion und
//! Modell-Hash", `model_hash` = Artefakt-SHA256). Until hash-pinned fixtures
//! are committed, descriptors carry the documented placeholder
//! [`PENDING_INTEGRATION_HASH`].
//!
//! This module is deliberately free of ONNX Runtime dependencies so it is
//! compiled (and tested) in every build, including the default stub-only build:
//!
//! * [`compute_sha256_hex`] — deterministic SHA-256 hex digest of a byte stream.
//! * [`verify_model_hash`] — pure comparison against the manifest value.
//! * [`verify_model_file`] — stream-hash a model file from disk.
//!
//! There are **no silent fallbacks**: a digest mismatch is surfaced as
//! [`ModelHashStatus::Mismatch`] and must never be treated as a load success.

use crate::OnnxError;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

/// Documented placeholder for `model_hash` until real, hash-pinned ONNX
/// fixtures are committed (no spontaneous downloads — see `Agents.md`). A
/// manifest carrying this value **cannot** be verified yet; verification
/// reports [`ModelHashStatus::Pending`] instead of guessing.
pub const PENDING_INTEGRATION_HASH: &str = "pending-integration";

/// Result of verifying a loaded artifact against the manifest `model_hash`.
///
/// This is the adapter-side counterpart of the sidecar mask-identity status:
/// `Verified` maps to a valid identity, `Mismatch` to `stale` weights (the
/// artifact on disk does not match the pinned identity), and `Pending` marks
/// the documented pre-integration state in which the identity cannot be
/// checked yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelHashStatus {
    /// Computed artifact digest equals the manifest `model_hash`.
    Verified,
    /// The manifest still carries [`PENDING_INTEGRATION_HASH`]; no pinned
    /// identity exists yet, so the artifact could not be verified. This state
    /// is visible through [`ModelHashStatus`](self) and must not be presented
    /// as verified.
    Pending,
    /// The computed digest differs from the manifest `model_hash`: the
    /// artifact is stale/mismatched. Backends refuse inference with this
    /// status instead of silently using wrong weights.
    Mismatch {
        /// Hash pinned by the manifest.
        expected: String,
        /// SHA-256 hex digest computed from the actual artifact bytes.
        actual: String,
    },
}

impl ModelHashStatus {
    /// Whether inference may proceed with this status. Only `Mismatch` is
    /// refused (stale weights must never run silently).
    pub fn allows_inference(&self) -> bool {
        matches!(self, ModelHashStatus::Verified | ModelHashStatus::Pending)
    }

    /// Enforce this status as a gate in front of inference
    /// (F-082-FOLLOWUP-HASH): `Verified`/`Pending` allow inference,
    /// `Mismatch` is refused with [`OnnxError::ModelArtifactStale`] carrying
    /// the pinned/actual digests — a visible error instead of silently
    /// running wrong weights (REVIEW-ONNX-HASH-1).
    ///
    /// This is the single shared decision point for every backend that
    /// verifies artifact identity, kept dependency-free so the refuse branch
    /// stays unit-testable without ONNX Runtime or model weights.
    pub fn enforce_inference_allowed(&self, model_name: &str) -> Result<(), OnnxError> {
        match self {
            ModelHashStatus::Verified | ModelHashStatus::Pending => Ok(()),
            ModelHashStatus::Mismatch { expected, actual } => Err(OnnxError::ModelArtifactStale {
                name: model_name.to_owned(),
                expected: expected.clone(),
                actual: actual.clone(),
            }),
        }
    }
}

/// Lowercase hex encoding (used for SHA-256 digests; 32 bytes → 64 chars).
fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Compute the SHA-256 hex digest of everything readable from `reader`.
///
/// Deterministic and streaming (models can be large); I/O errors are returned
/// unchanged so callers can distinguish them from hash mismatches.
pub fn compute_sha256_hex(mut reader: impl Read) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    // 64 KiB chunks: bounded memory for multi-hundred-MB checkpoints.
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(e) => return Err(e),
        }
    }
    Ok(to_hex(&hasher.finalize()))
}

/// Compare an expected manifest hash against a computed artifact digest.
///
/// * expected == [`PENDING_INTEGRATION_HASH`] → [`ModelHashStatus::Pending`]
/// * expected == actual → [`ModelHashStatus::Verified`]
/// * otherwise → [`ModelHashStatus::Mismatch`]
pub fn verify_model_hash(expected: &str, actual: &str) -> ModelHashStatus {
    if expected == PENDING_INTEGRATION_HASH {
        ModelHashStatus::Pending
    } else if expected == actual {
        ModelHashStatus::Verified
    } else {
        ModelHashStatus::Mismatch {
            expected: expected.to_owned(),
            actual: actual.to_owned(),
        }
    }
}

/// Stream-hash the file at `path` and verify it against `expected`.
///
/// Returns [`OnnxError::MissingModel`] when the file cannot be opened or read
/// (an unreadable artifact is unavailable for inference), otherwise the
/// resulting [`ModelHashStatus`]. No silent fallbacks: a mismatch is reported,
/// never coerced into a success.
pub fn verify_model_file(path: &Path, expected: &str) -> Result<ModelHashStatus, OnnxError> {
    let path_display = path.display().to_string();
    let file = std::fs::File::open(path).map_err(|_| OnnxError::MissingModel {
        path: path_display.clone(),
    })?;
    let actual =
        compute_sha256_hex(file).map_err(|_| OnnxError::MissingModel { path: path_display })?;
    Ok(verify_model_hash(expected, &actual))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn sha256_known_vectors() {
        // Well-known SHA-256 test vectors (NIST / FIPS 180 examples).
        let empty = compute_sha256_hex(Cursor::new(Vec::<u8>::new())).unwrap();
        assert_eq!(
            empty,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let abc = compute_sha256_hex(Cursor::new("abc")).unwrap();
        assert_eq!(
            abc,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_is_chunking_stable() {
        // >64 KiB input forces multiple read cycles; result must equal the
        // one-shot digest of the same bytes.
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let streamed = compute_sha256_hex(Cursor::new(&data)).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(&data);
        assert_eq!(streamed, to_hex(&hasher.finalize()));
    }

    #[test]
    fn verify_placeholder_is_pending_not_verified() {
        assert_eq!(
            verify_model_hash(PENDING_INTEGRATION_HASH, "deadbeef"),
            ModelHashStatus::Pending
        );
        assert!(verify_model_hash(PENDING_INTEGRATION_HASH, "x").allows_inference());
    }

    #[test]
    fn verify_equal_is_verified() {
        assert_eq!(
            verify_model_hash("cafe01", "cafe01"),
            ModelHashStatus::Verified
        );
        assert!(verify_model_hash("cafe01", "cafe01").allows_inference());
    }

    #[test]
    fn verify_difference_is_mismatch_and_refuses_inference() {
        let status = verify_model_hash("expected-hash", "actual-hash");
        match &status {
            ModelHashStatus::Mismatch { expected, actual } => {
                assert_eq!(expected, "expected-hash");
                assert_eq!(actual, "actual-hash");
            }
            other => panic!("expected Mismatch, got {other:?}"),
        }
        assert!(!status.allows_inference(), "mismatch must refuse inference");
    }

    // F-082-FOLLOWUP-HASH — the refuse branch itself must be executable: a
    // `Mismatch` status enforces `ModelArtifactStale` with both digests.
    #[test]
    fn enforce_refuses_mismatch_with_model_artifact_stale() {
        let status = ModelHashStatus::Mismatch {
            expected: "pinned-digest".to_owned(),
            actual: "on-disk-digest".to_owned(),
        };
        let err = status.enforce_inference_allowed("BiRefNet").unwrap_err();
        match err {
            OnnxError::ModelArtifactStale {
                name,
                expected,
                actual,
            } => {
                assert_eq!(name, "BiRefNet");
                assert_eq!(expected, "pinned-digest");
                assert_eq!(actual, "on-disk-digest");
            }
            other => panic!("expected ModelArtifactStale, got {other:?}"),
        }
    }

    /// `Verified` and `Pending` must never be refused by the gate — the
    /// documented pre-integration placeholder is not a stale artifact.
    #[test]
    fn enforce_allows_verified_and_pending() {
        assert!(ModelHashStatus::Verified
            .enforce_inference_allowed("M")
            .is_ok());
        assert!(ModelHashStatus::Pending
            .enforce_inference_allowed("M")
            .is_ok());
    }

    fn scratch_path(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        dir.join(format!(
            "lumina-onnx-hash-{tag}-{}-{}.bin",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ))
    }

    #[test]
    fn verify_model_file_reports_missing_artifact() {
        let err = verify_model_file(Path::new("/nonexistent/lumina/model.onnx"), "h").unwrap_err();
        assert!(
            matches!(err, OnnxError::MissingModel { .. }),
            "unreadable artifact must surface as MissingModel, got {err:?}"
        );
    }

    #[test]
    fn verify_model_file_roundtrip() {
        let path = scratch_path("roundtrip");
        std::fs::write(&path, b"abc").unwrap();
        let actual = compute_sha256_hex(Cursor::new("abc")).unwrap();
        // Verified against the true digest…
        assert_eq!(
            verify_model_file(&path, &actual).unwrap(),
            ModelHashStatus::Verified
        );
        // …pending against the placeholder…
        assert_eq!(
            verify_model_file(&path, PENDING_INTEGRATION_HASH).unwrap(),
            ModelHashStatus::Pending
        );
        // …and mismatched against anything else.
        assert!(matches!(
            verify_model_file(&path, "not-the-digest").unwrap(),
            ModelHashStatus::Mismatch { .. }
        ));
        let _ = std::fs::remove_file(&path);
    }
}
