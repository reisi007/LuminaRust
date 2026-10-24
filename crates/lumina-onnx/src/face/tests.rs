use super::*;
use crate::face::cluster::FaceClusteringParams;

fn fingerprints() -> (SourceFingerprint, DecodeFingerprint, GeometryFingerprint) {
    (
        SourceFingerprint {
            content_hash: "blake3:abc".into(),
            byte_length: 42,
            extras: Extras::new(),
        },
        DecodeFingerprint {
            decoder: "libraw".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: Extras::new(),
        },
        GeometryFingerprint {
            width: 6000,
            height: 4000,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: Extras::new(),
        },
    )
}

fn identity() -> FaceIdentity {
    let (source, decode, geometry) = fingerprints();
    face_identity(
        &FaceModelSuite::candidate(),
        source,
        decode,
        geometry,
        FaceClusteringParams::default().to_identity(),
        &FaceInferenceOptions::default(),
    )
    .unwrap()
}

fn face() -> DetectedFace {
    DetectedFace {
        bbox: lumina_sidecar::FaceBoundingBox {
            x: 0.1,
            y: 0.2,
            width: 0.3,
            height: 0.4,
        },
        score: 0.9,
        landmarks: vec![lumina_sidecar::FaceLandmark {
            name: "left_eye".into(),
            x: 0.2,
            y: 0.3,
        }],
    }
}

#[test]
fn shipped_descriptors_declare_exactly_their_capability_and_are_pinned() {
    let detect = face_detect_manifest();
    assert!(detect.capabilities.face_detect);
    assert!(!detect.capabilities.face_embed);
    assert!(!detect.capabilities.subject_segmentation);
    assert!(!detect.capabilities.box_prompt);
    assert!(!detect.capabilities.point_prompt);
    assert!(!detect.capabilities.mask_prompt);
    assert!(!detect.capabilities.class_detection);
    assert!(!detect.capabilities.instance_segmentation);
    assert!(!detect.capabilities.inpaint_heal);
    assert!(!detect.capabilities.outpaint);
    assert_eq!(detect.model_hash, FACE_DETECT_MODEL_HASH);
    assert!(face_model_hash_is_pinned(&detect));
    assert_eq!(detect.license, FACE_DETECT_LICENSE);

    let embed = face_embed_manifest();
    assert!(embed.capabilities.face_embed);
    assert!(!embed.capabilities.face_detect);
    assert_eq!(embed.model_hash, FACE_EMBED_MODEL_HASH);
    assert!(face_model_hash_is_pinned(&embed));
    assert_eq!(embed.license, FACE_EMBED_LICENSE);

    // Both descriptors are internally consistent.
    assert!(detect.validate().is_ok());
    assert!(embed.validate().is_ok());
    assert!(FaceModelSuite::candidate().validate().is_ok());
}

/// FACE-20-S6: both shipped descriptors use the verified pins; the pin
/// well-formedness (`sha256:<64 lowercase hex>`) is asserted in
/// [`crate::face::pins`].
#[test]
fn shipped_descriptors_use_the_pinned_hashes() {
    assert_eq!(face_detect_manifest().model_hash, FACE_DETECT_MODEL_HASH);
    assert_eq!(face_embed_manifest().model_hash, FACE_EMBED_MODEL_HASH);
}

#[test]
fn suite_rejects_swapped_capabilities() {
    let err = FaceModelSuite::new(face_embed_manifest(), face_detect_manifest(), 128).unwrap_err();
    assert!(matches!(err, OnnxError::UnsupportedModel { .. }), "{err:?}");
}

#[test]
fn suite_rejects_zero_dimension() {
    let err = FaceModelSuite::new(face_detect_manifest(), face_embed_manifest(), 0).unwrap_err();
    assert!(matches!(err, OnnxError::InvalidFaceData(_)), "{err:?}");
}

#[test]
fn identity_is_deterministic_and_digest_is_stable() {
    let a = identity();
    let b = identity();
    assert_eq!(a, b);
    assert_eq!(face_identity_digest(&a), face_identity_digest(&b));
    assert!(face_identity_digest(&a).starts_with("sha256:"));
    assert_eq!(face_identity_digest(&a).len(), "sha256:".len() + 64);
}

#[test]
fn identity_digest_flips_on_model_change() {
    let base = identity();
    let mut changed = base.clone();
    changed.detection_model.hash = format!("sha256:{}", "11".repeat(32));
    assert_ne!(face_identity_digest(&base), face_identity_digest(&changed));
}

#[test]
fn identity_digest_flips_on_clustering_change() {
    let base = identity();
    let mut changed = base.clone();
    changed.clustering.version += 1;
    assert_ne!(face_identity_digest(&base), face_identity_digest(&changed));
}

#[test]
fn identity_digest_flips_on_threshold_change() {
    let (source, decode, geometry) = fingerprints();
    let suite = FaceModelSuite::candidate();
    let params = FaceClusteringParams::default().to_identity();
    let a = face_identity(
        &suite,
        source.clone(),
        decode.clone(),
        geometry.clone(),
        params.clone(),
        &FaceInferenceOptions::default(),
    )
    .unwrap();
    let b = face_identity(
        &suite,
        source,
        decode,
        geometry,
        params,
        &FaceInferenceOptions {
            detection_score_threshold: 0.75,
            ..FaceInferenceOptions::default()
        },
    )
    .unwrap();
    assert_ne!(
        face_identity_digest(&a),
        face_identity_digest(&b),
        "score threshold is part of the persisted identity"
    );
}

/// F1 (digest contract): the persisted digest extra is not part of the
/// digest, so the stored value equals a recomputation over the *same*
/// identity (no manual extra-strip needed) and adding the extra does not
/// change the digest. All *other* extras stay identity-bearing.
#[test]
fn persisted_digest_matches_recomputation_and_ignores_only_itself() {
    let (source, decode, geometry) = fingerprints();
    let with_digest = face_identity_with_digest(
        &FaceModelSuite::candidate(),
        source,
        decode,
        geometry,
        FaceClusteringParams::default().to_identity(),
        &FaceInferenceOptions::default(),
    )
    .unwrap();
    let stored = with_digest.extras.get(FACE_IDENTITY_DIGEST_KEY).unwrap();
    let serde_json::Value::String(stored) = stored else {
        panic!("digest extra must be a string");
    };
    // Invariant: stored digest == digest(&with_digest), no stripping.
    assert_eq!(&face_identity_digest(&with_digest), stored);
    assert_eq!(
        face_identity_digest(&with_digest),
        face_identity_digest(&identity()),
        "the digest extra must not change the digest"
    );

    // Any other top-level extra still flips the digest (the skip is scoped).
    let mut extra = identity();
    extra
        .extras
        .insert("note".into(), serde_json::Value::String("x".into()));
    assert_ne!(
        face_identity_digest(&identity()),
        face_identity_digest(&extra)
    );
}

/// F1 (mixed use): an identity carrying the persisted digest and one
/// without it describe the same analysis and must never report a false
/// `Stale` in either direction.
#[test]
fn mixed_digest_presence_never_reports_false_stale() {
    let with = {
        let (source, decode, geometry) = fingerprints();
        face_identity_with_digest(
            &FaceModelSuite::candidate(),
            source,
            decode,
            geometry,
            FaceClusteringParams::default().to_identity(),
            &FaceInferenceOptions::default(),
        )
        .unwrap()
    };
    let without = identity();
    let evidence = FaceArtifactEvidence::Present {
        checksum_matches: true,
    };
    assert_eq!(
        face_artifact_status(&with, &without, evidence),
        FaceArtifactStatus::Valid
    );
    assert_eq!(
        face_artifact_status(&without, &with, evidence),
        FaceArtifactStatus::Valid
    );

    // A genuine identity change is still `Stale` even with digests present.
    let mut changed = with.clone();
    changed.embedding_model.version = "9".into();
    assert_eq!(
        face_artifact_status(&changed, &with, evidence),
        FaceArtifactStatus::Stale
    );
}

#[test]
fn artifact_status_covers_valid_stale_missing_corrupt() {
    let current = identity();
    assert_eq!(
        face_artifact_status(
            &current,
            &current,
            FaceArtifactEvidence::Present {
                checksum_matches: true
            }
        ),
        FaceArtifactStatus::Valid
    );

    let mut changed = current.clone();
    changed.embedding_model.version = "9".into();
    assert_eq!(
        face_artifact_status(
            &current,
            &changed,
            FaceArtifactEvidence::Present {
                checksum_matches: true
            }
        ),
        FaceArtifactStatus::Stale
    );

    assert_eq!(
        face_artifact_status(&current, &current, FaceArtifactEvidence::Missing),
        FaceArtifactStatus::Missing
    );
    assert_eq!(
        face_artifact_status(
            &current,
            &current,
            FaceArtifactEvidence::Present {
                checksum_matches: false
            }
        ),
        FaceArtifactStatus::Corrupt
    );
}

#[test]
fn detected_face_id_is_content_derived_and_stable() {
    let face = face();
    let id = detected_face_id(&face);
    assert!(id.starts_with("face-"));
    assert_eq!(id, detected_face_id(&face.clone()));
    let mut other = face.clone();
    other.score = 0.5;
    assert_ne!(id, detected_face_id(&other));
}

#[test]
fn output_bridges_to_sidecar_and_validates() {
    let face = face();
    let vector = FaceEmbeddingVector::new(vec![0.6, 0.8]).unwrap();
    let record = FaceEmbeddingRecord {
        detection_index: 0,
        vector,
        reference: FaceVectorRef {
            relative_path: "IMAGE.ARW.lumina.zdata".into(),
            format: "lumina-zdata".into(),
            checksum: "aa".repeat(32),
            dimension: 2,
            channels: "f32".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        },
    };
    let detection_id = detected_face_id(&face);
    let analysis = FaceAnalysisOutput {
        identity: identity(),
        created_at: "2026-09-16T08:00:00Z".into(),
        detections: vec![face],
        embeddings: vec![record],
        clusters: vec![FaceCluster {
            id: "cluster-1".into(),
            detection_ids: vec![detection_id.clone()],
            extras: Extras::new(),
        }],
        persons: vec![FacePerson {
            id: "person-1".into(),
            name: "Example".into(),
            confirmed: true,
            cluster_ids: vec!["cluster-1".into()],
            extras: Extras::new(),
        }],
    }
    .into_sidecar()
    .unwrap();

    assert_eq!(analysis.version, lumina_sidecar::FACE_SCHEMA_VERSION);
    assert_eq!(analysis.detections[0].id, detection_id);
    assert_eq!(
        analysis.detections[0].embedding_id.as_deref(),
        Some(embedding_id_for(&detection_id).as_str())
    );
    assert_eq!(analysis.embeddings[0].detection_id, detection_id);
}

#[test]
fn output_rejects_non_normalized_embedding() {
    let record = FaceEmbeddingRecord {
        detection_index: 0,
        vector: FaceEmbeddingVector::new(vec![1.0, 1.0]).unwrap(),
        reference: FaceVectorRef {
            relative_path: "IMAGE.ARW.lumina.zdata".into(),
            format: "lumina-zdata".into(),
            checksum: "aa".repeat(32),
            dimension: 2,
            channels: "f32".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        },
    };
    let err = FaceAnalysisOutput {
        identity: identity(),
        created_at: "2026-09-16T08:00:00Z".into(),
        detections: vec![face()],
        embeddings: vec![record],
        clusters: vec![],
        persons: vec![],
    }
    .into_sidecar()
    .unwrap_err();
    assert!(matches!(err, OnnxError::InvalidFaceData(_)), "{err:?}");
}

/// F3: the persisted [`FaceVectorRef::dimension`] must equal the actual
/// (normalized) vector length — a 2-value vector with a `512` reference
/// is a loud `InvalidFaceData`, never silently persisted.
#[test]
fn output_rejects_reference_dimension_mismatch() {
    let record = FaceEmbeddingRecord {
        detection_index: 0,
        // Two-dimensional and unit-normalized, so only the dimension check
        // can reject it.
        vector: FaceEmbeddingVector::new(vec![0.6, 0.8]).unwrap(),
        reference: FaceVectorRef {
            relative_path: "IMAGE.ARW.lumina.zdata".into(),
            format: "lumina-zdata".into(),
            checksum: "aa".repeat(32),
            dimension: 512,
            channels: "f32".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        },
    };
    let err = FaceAnalysisOutput {
        identity: identity(),
        created_at: "2026-09-16T08:00:00Z".into(),
        detections: vec![face()],
        embeddings: vec![record],
        clusters: vec![],
        persons: vec![],
    }
    .into_sidecar()
    .unwrap_err();
    assert!(matches!(err, OnnxError::InvalidFaceData(_)), "{err:?}");
}
