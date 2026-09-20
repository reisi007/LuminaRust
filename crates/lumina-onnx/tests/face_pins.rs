//! LRPAR-G12-FACE-20 / FACE-20-S6 — license/pin acceptance tests (default
//! feature set, no ONNX Runtime, no weights, no network).
//!
//! The shipped face descriptors (YuNet = OpenCV Zoo `face_detection_yunet`,
//! MIT; SFace = `face_recognition_sface`, Apache-2.0) must carry the verified
//! upstream SHA-256 pins and the pinned canonical `input_spec_digest` values.
//! A drift in either is a deliberate re-pin and must be accompanied by an
//! update to `feature/quality/fixtures-licensing.md` §5,
//! `THIRD-PARTY-NOTICES.md` and the `licenses/models/` texts.

use lumina_onnx::{
    face_detect_manifest, face_embed_manifest, face_model_hash_is_pinned, verify_model_hash,
    ModelHashStatus, FACE_DETECT_MODEL_HASH, FACE_EMBED_MODEL_HASH, PENDING_INTEGRATION_HASH,
};

/// FACE-20-S6: the shipped face descriptors carry the verified upstream
/// SHA-256 pins — they are no longer `pending-integration`, and
/// `verify_model_hash` treats the pin as a real digest (a match verifies,
/// anything else is a loud mismatch; a missing artifact is `MissingModel`).
#[test]
fn builtin_face_descriptors_carry_verified_pins() {
    for (manifest, expected) in [
        (face_detect_manifest(), FACE_DETECT_MODEL_HASH),
        (face_embed_manifest(), FACE_EMBED_MODEL_HASH),
    ] {
        assert_eq!(manifest.model_hash, expected);
        assert_ne!(manifest.model_hash, PENDING_INTEGRATION_HASH);
        assert!(face_model_hash_is_pinned(&manifest));
        assert_eq!(
            verify_model_hash(&manifest.model_hash, expected),
            ModelHashStatus::Verified
        );
        assert!(matches!(
            verify_model_hash(&manifest.model_hash, "any-other-digest"),
            ModelHashStatus::Mismatch { .. }
        ));
        assert!(manifest.validate().is_ok());
    }
    let detect = face_detect_manifest();
    assert!(detect.capabilities.face_detect);
    assert!(!detect.capabilities.face_embed);
    let embed = face_embed_manifest();
    assert!(embed.capabilities.face_embed);
    assert!(!embed.capabilities.face_detect);
}

/// The shipped `sha256:<64 lowercase hex>` pins match the exact upstream
/// artifact digests documented in `feature/quality/fixtures-licensing.md` §5.
#[test]
fn shipped_model_hashes_equal_the_documented_upstream_pins() {
    assert_eq!(
        FACE_DETECT_MODEL_HASH,
        "sha256:8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4"
    );
    assert_eq!(
        FACE_EMBED_MODEL_HASH,
        "sha256:0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79"
    );
}

/// These digests pin the full canonical I/O contract (resolution, channel
/// layout, tensor names, memory layout and normalization). A change to any of
/// them MUST be a deliberate re-pin here and in
/// `feature/quality/fixtures-licensing.md` §5 — it also invalidates every
/// persisted face identity (the digest is part of the model identity).
#[test]
fn shipped_input_spec_digests_are_pinned_to_the_canonical_contract() {
    let cases = [
        (
            "YuNet (detect)",
            face_detect_manifest(),
            "sha256:e992dce8b7c2c51bdd9872106550f57ebdaf0ee86c5774f2212b47a79182d5dc",
        ),
        (
            "SFace (embed)",
            face_embed_manifest(),
            "sha256:a5c289c6f0c6466b927a7c895db2fc35cc22c96846db950bdd13bc741012270f",
        ),
    ];
    for (label, manifest, expected) in cases {
        let identity = manifest.to_model_identity();
        let digest = identity.extras["input_spec_digest"]
            .as_str()
            .expect("input_spec_digest must be a string");
        assert_eq!(digest, expected, "{label}");
    }
}
