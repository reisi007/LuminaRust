//! Face model identity pins (LRPAR-G12-FACE-20 / FACE-20-S6, F-078).
//!
//! Model name, version, verified licence and verified SHA-256 `model_hash` of
//! the two shipped face models. The exact `.onnx` bytes were fetched once from
//! the upstream source and hashed; these values are the manifest identity and
//! change only together with `feature/quality/fixtures-licensing.md` §5 and
//! `THIRD-PARTY-NOTICES.md` (item 6).
//!
//! **Licence / weight grant (verified 2026-09-20):** the per-model-directory
//! `LICENSE` plus its `README.md` clause *"all files in this directory"* is the
//! **weight grant** (not merely a code licence) — the earlier
//! `pending-integration` candidate state waited for exactly this. Source:
//! `opencv/opencv_zoo` `main` @ `47534e27c9851bb1128ccc0102f1145e27f23f98`.
//!
//! **Real I/O contract (decoded by the FACE-20-FACE-ADAPTER-25 adapter):** the
//! shipped graphs carry the real OpenCV contract, not the canonical fused
//! single output. **YuNet** consumes raw `0..=255` **BGR** `input` and emits
//! twelve per-stride tensors (`cls_/obj_/bbox_/kps_{8,16,32}`, decoded by
//! [`super::yunet`]); **SFace** consumes raw `0..=255` **RGB** `data` and emits
//! the 128-d `fc1` (its graph bakes `(x−127.5)·1/128` internally). Both
//! manifests therefore declare [`crate::manifest::InputNormalization::BYTE_RANGE`].
//! A graph that does not match its declared contract is refused loudly — no
//! silent reshape and no stub substitution.

/// Detection model name (OpenCV Zoo `face_detection_yunet`).
pub const FACE_DETECT_MODEL_NAME: &str = "YuNet";
/// Detection model version (OpenCV Zoo release tag; fixed 640×640 graph).
pub const FACE_DETECT_MODEL_VERSION: &str = "2023mar";
/// Detection model licence: **MIT** (© 2020 Shiqi Yu).
///
/// Weight grant verified at the source (FACE-20-S6, F-078): the model
/// directory `models/face_detection_yunet/` ships a `LICENSE` (MIT, © 2020
/// Shiqi Yu, blob `4cdf89a4…`) and its `README.md` states *"All files in this
/// directory are licensed under MIT License"* — the grant therefore covers the
/// weight artifact `face_detection_yunet_2023mar.onnx` sitting next to it, not
/// just the demo code.
pub const FACE_DETECT_LICENSE: &str = "MIT";
/// Pinned SHA-256 of the detection weight artifact
/// `face_detection_yunet_2023mar.onnx` (232 589 bytes) as published by
/// `opencv/opencv_zoo` (Git LFS object id; re-verified 2026-09-20 by fetching
/// the LFS content and hashing the exact bytes). This is the manifest
/// `model_hash`; provenance in `feature/quality/fixtures-licensing.md` §5.
pub const FACE_DETECT_MODEL_HASH: &str =
    "sha256:8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4";
/// Embedding model name (OpenCV Zoo `face_recognition_sface`, MobileFaceNet).
pub const FACE_EMBED_MODEL_NAME: &str = "SFace";
/// Embedding model version (OpenCV Zoo release tag; 112×112 → 128-d graph).
pub const FACE_EMBED_MODEL_VERSION: &str = "2021dec";
/// Embedding model licence: **Apache-2.0** (© 2021 Shenzhen Institute of
/// Artificial Intelligence and Robotics for Society).
///
/// Weight grant verified at the source (FACE-20-S6, F-078): the model
/// directory `models/face_recognition_sface/` ships the verbatim Apache-2.0
/// `LICENSE` (blob `d6456956…`) and its `README.md` states *"All files in this
/// directory are licensed under Apache 2.0 License"* — the grant covers the
/// weight artifact `face_recognition_sface_2021dec.onnx`. The `sface.py`
/// header names the copyright holder.
pub const FACE_EMBED_LICENSE: &str = "Apache-2.0";
/// Pinned SHA-256 of the embedding weight artifact
/// `face_recognition_sface_2021dec.onnx` (38 696 353 bytes) as published by
/// `opencv/opencv_zoo` (Git LFS object id; re-verified 2026-09-20 by fetching
/// the LFS content and hashing the exact bytes). This is the manifest
/// `model_hash`; provenance in `feature/quality/fixtures-licensing.md` §5.
pub const FACE_EMBED_MODEL_HASH: &str =
    "sha256:0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::PENDING_INTEGRATION_HASH;

    /// FACE-20-S6: both shipped pins use the sidecar SOLL spelling
    /// `sha256:<64 lowercase hex>` (the same algorithm
    /// [`crate::hash::verify_model_file`] computes), never the
    /// `pending-integration` placeholder.
    #[test]
    fn shipped_model_hashes_are_wellformed_sha256_pins() {
        for (label, hash) in [
            ("YuNet", FACE_DETECT_MODEL_HASH),
            ("SFace", FACE_EMBED_MODEL_HASH),
        ] {
            assert_ne!(hash, PENDING_INTEGRATION_HASH, "{label} still pending");
            let hex = hash.strip_prefix("sha256:").unwrap_or_else(|| {
                panic!("{label} pin must carry the `sha256:` prefix, got {hash}")
            });
            assert_eq!(hex.len(), 64, "{label} pin must be 64 hex chars");
            assert!(
                hex.bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
                "{label} pin must be lowercase hex, got {hex}"
            );
        }
    }
}
