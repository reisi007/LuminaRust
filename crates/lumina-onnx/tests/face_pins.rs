//! LRPAR-G12-FACE-20 / FACE-20-S6 + FACE-20-FACE-ADAPTER-25 — license/pin
//! acceptance tests (default feature set, no ONNX Runtime, no weights, no
//! network).
//!
//! The shipped face descriptors (YuNet = OpenCV Zoo `face_detection_yunet`,
//! MIT; SFace = `face_recognition_sface`, Apache-2.0) must carry the verified
//! upstream SHA-256 pins and the pinned real I/O contract. FACE-20-FACE-ADAPTER-25
//! replaced the canonical ImageNet/RGB single-output declaration with the real
//! graph contract (YuNet = raw `0..=255` BGR / twelve per-stride outputs; SFace
//! = raw `0..=255` RGB `data` → 128-d `fc1`), so both `input_spec_digest`
//! values were deliberately re-pinned. A drift in any of them must be
//! accompanied by an update to `feature/quality/fixtures-licensing.md` §5,
//! `THIRD-PARTY-NOTICES.md` and the `license/` texts.

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

/// These digests pin the full **real** I/O contract (resolution, channel
/// layout, tensor names, memory layout and normalization) as declared by
/// FACE-20-FACE-ADAPTER-25. A change to any of them MUST be a deliberate
/// re-pin here and in `feature/quality/fixtures-licensing.md` §5 — it also
/// invalidates every persisted face identity (the digest is part of the model
/// identity).
#[test]
fn shipped_input_spec_digests_are_pinned_to_the_real_contract() {
    let cases = [
        (
            "YuNet (detect)",
            face_detect_manifest(),
            "sha256:03ce26b03baf5d45f5905e4a84c4d6e0fe70ae6b2ceaffc398cd2dd5b178a8d9",
        ),
        (
            "SFace (embed)",
            face_embed_manifest(),
            "sha256:e2e2919ae7b8f18ef5c67598ed44d4dc9e258badc38bde47eef577af0f2b01b6",
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

/// FACE-20-FACE-ADAPTER-25: the shipped descriptors declare the **real** graph
/// I/O — YuNet raw `0..=255` BGR (`input` → twelve per-stride outputs), SFace
/// raw `0..=255` RGB (`data` → `fc1`) — with no ImageNet preprocessing.
#[test]
fn shipped_descriptors_declare_the_real_io_contract() {
    use lumina_onnx::{ChannelLayout, InputNormalization, YUNET_OUTPUT_NAMES};

    let detect = face_detect_manifest();
    assert_eq!(detect.input.tensor_name, "input");
    assert_eq!(detect.input.channel_layout, ChannelLayout::Bgr);
    assert_eq!(detect.input.normalization, InputNormalization::BYTE_RANGE);
    assert_eq!(detect.output_tensor_name, YUNET_OUTPUT_NAMES[0]);

    let embed = face_embed_manifest();
    assert_eq!(embed.input.tensor_name, "data");
    assert_eq!(embed.input.channel_layout, ChannelLayout::Rgb);
    assert_eq!(embed.input.normalization, InputNormalization::BYTE_RANGE);
    assert_eq!(embed.output_tensor_name, "fc1");
}

/// The NMS threshold and top-k are identity-bearing: changing only them must
/// flip the persisted face-identity digest (a change re-filters detections, so
/// a stale analysis must not silently stay `valid`).
#[test]
fn nms_options_are_identity_bearing() {
    use lumina_onnx::{
        face_identity, face_identity_digest, FaceClusteringParams, FaceInferenceOptions,
        FaceModelSuite,
    };
    use lumina_sidecar::{DecodeFingerprint, Extras, GeometryFingerprint, SourceFingerprint};
    use std::collections::BTreeMap;

    let source = SourceFingerprint {
        content_hash: "blake3:abc".into(),
        byte_length: 42,
        extras: Extras::new(),
    };
    let decode = DecodeFingerprint {
        decoder: "libraw".into(),
        version: "1".into(),
        parameters: BTreeMap::new(),
        extras: Extras::new(),
    };
    let geometry = GeometryFingerprint {
        width: 6000,
        height: 4000,
        orientation: 1,
        pixel_aspect_ratio: 1.0,
        extras: Extras::new(),
    };
    let build = |options: &FaceInferenceOptions| {
        face_identity(
            &FaceModelSuite::candidate(),
            source.clone(),
            decode.clone(),
            geometry.clone(),
            FaceClusteringParams::default().to_identity(),
            options,
        )
        .unwrap()
    };
    let base = build(&FaceInferenceOptions::default());
    let nms = FaceInferenceOptions {
        detection_nms_threshold: 0.7,
        ..FaceInferenceOptions::default()
    };
    assert_ne!(
        face_identity_digest(&base),
        face_identity_digest(&build(&nms)),
        "the NMS threshold is part of the identity"
    );
    let top_k = FaceInferenceOptions {
        detection_top_k: 1,
        ..FaceInferenceOptions::default()
    };
    assert_ne!(
        face_identity_digest(&base),
        face_identity_digest(&build(&top_k)),
        "top_k is part of the identity"
    );
    // Invalid values are loud.
    assert!(FaceInferenceOptions {
        detection_nms_threshold: 1.5,
        ..FaceInferenceOptions::default()
    }
    .validate()
    .is_err());
    assert!(FaceInferenceOptions {
        detection_top_k: 0,
        ..FaceInferenceOptions::default()
    }
    .validate()
    .is_err());
}

/// B1 (FACE-20-FACE-ADAPTER-25): a persisted analysis recorded with the
/// pre-adapter input spec (canonical ImageNet/RGB) must resolve to `Stale`
/// against the current, re-pinned descriptor — the `input_spec_digest` is part
/// of the persisted detection-model identity (`face_identity` →
/// `detection_model.extras` → `face_identity_digest`). Control: the current
/// spec against itself is `Valid`.
#[test]
fn changed_input_spec_makes_a_persisted_analysis_stale() {
    use lumina_onnx::{
        face_artifact_status, face_identity, face_identity_digest, ChannelLayout,
        FaceArtifactEvidence, FaceClusteringParams, FaceInferenceOptions, FaceModelSuite,
        InputNormalization,
    };
    use lumina_sidecar::{
        DecodeFingerprint, Extras, FaceArtifactStatus, GeometryFingerprint, SourceFingerprint,
    };
    use std::collections::BTreeMap;

    let source = SourceFingerprint {
        content_hash: "blake3:abc".into(),
        byte_length: 42,
        extras: Extras::new(),
    };
    let decode = DecodeFingerprint {
        decoder: "libraw".into(),
        version: "1".into(),
        parameters: BTreeMap::new(),
        extras: Extras::new(),
    };
    let geometry = GeometryFingerprint {
        width: 6000,
        height: 4000,
        orientation: 1,
        pixel_aspect_ratio: 1.0,
        extras: Extras::new(),
    };
    let build = |suite: &FaceModelSuite| {
        face_identity(
            suite,
            source.clone(),
            decode.clone(),
            geometry.clone(),
            FaceClusteringParams::default().to_identity(),
            &FaceInferenceOptions::default(),
        )
        .unwrap()
    };

    // Persisted: the S6 canonical declaration (ImageNet, RGB) before the
    // FACE-20-FACE-ADAPTER-25 re-pin.
    let mut legacy_suite = FaceModelSuite::candidate();
    legacy_suite.detection.input.normalization = InputNormalization::IMAGENET;
    legacy_suite.detection.input.channel_layout = ChannelLayout::Rgb;
    let persisted = build(&legacy_suite);

    // Current: the re-pinned real contract (BYTE_RANGE, BGR).
    let current = build(&FaceModelSuite::candidate());

    // Only the detection input spec differs, so the identity digests diverge.
    assert_ne!(
        face_identity_digest(&current),
        face_identity_digest(&persisted),
        "an input-spec re-pin must flip the persisted face-identity digest"
    );
    let evidence = FaceArtifactEvidence::Present {
        checksum_matches: true,
    };
    assert_eq!(
        face_artifact_status(&current, &persisted, evidence),
        FaceArtifactStatus::Stale,
        "a persisted analysis with the old input spec must be stale"
    );
    // Control: the current identity against itself is Valid.
    assert_eq!(
        face_artifact_status(&current, &build(&FaceModelSuite::candidate()), evidence),
        FaceArtifactStatus::Valid
    );
}
