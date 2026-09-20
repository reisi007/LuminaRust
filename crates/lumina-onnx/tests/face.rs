//! LRPAR-G12-FACE-20 / FACE-20-S2+S3 integration tests (default feature set).
//!
//! These run without ONNX Runtime, without weights and without network: the
//! deterministic, tests-only stub backends drive the detection → embedding →
//! clustering → sidecar chain. The manifest/capability/hash gates, the
//! deterministic clustering and the invalidation semantics (model change →
//! stale, missing model → visible) are the acceptance surface of S2+S3.

use lumina_core::ImageFrame;
use lumina_onnx::{
    cluster_embeddings, clusters_from_labels, detected_face_id, face_artifact_status,
    face_detect_manifest, face_embed_manifest, face_identity, face_identity_digest,
    face_identity_with_digest, DetectedFace, FaceAnalysisOutput, FaceArtifactEvidence,
    FaceClusteringParams, FaceDetectionInference, FaceEmbeddingInference, FaceEmbeddingRecord,
    FaceEmbeddingVector, FaceInferenceOptions, FaceModelSuite, ModelManifest, OnnxError,
    StubFaceDetector, StubFaceEmbedder, FACE_IDENTITY_DIGEST_KEY,
};
use lumina_sidecar::{
    DecodeFingerprint, Extras, FaceArtifactStatus, FaceBoundingBox, FaceClusteringIdentity,
    FaceLandmark, FaceVectorRef, GeometryFingerprint, SourceFingerprint,
};
use std::collections::BTreeMap;

fn frame(width: u32, height: u32) -> ImageFrame {
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for px in pixels.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    ImageFrame::new(width, height, pixels).unwrap()
}

fn detection(seed: u8) -> DetectedFace {
    let offset = f32::from(seed) * 0.01;
    DetectedFace {
        bbox: FaceBoundingBox {
            x: 0.1 + offset,
            y: 0.2 + offset,
            width: 0.3,
            height: 0.4,
        },
        score: 0.9,
        landmarks: vec![
            FaceLandmark {
                name: "left_eye".into(),
                x: 0.2,
                y: 0.3,
            },
            FaceLandmark {
                name: "right_eye".into(),
                x: 0.35,
                y: 0.3,
            },
            FaceLandmark {
                name: "nose".into(),
                x: 0.28,
                y: 0.4,
            },
            FaceLandmark {
                name: "mouth_left".into(),
                x: 0.22,
                y: 0.5,
            },
            FaceLandmark {
                name: "mouth_right".into(),
                x: 0.34,
                y: 0.5,
            },
        ],
    }
}

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

fn clustering() -> FaceClusteringIdentity {
    FaceClusteringParams::default().to_identity()
}

/// The `face_detect`/`face_embed` capability flags are additive: manifests
/// written before them keep parsing (default `false`), the face descriptors
/// round-trip, and unknown capability fields are still rejected loudly.
#[test]
fn face_capabilities_are_additive_and_roundtrip() {
    let detect = face_detect_manifest();
    let restored = ModelManifest::from_json(&detect.to_json().unwrap()).unwrap();
    assert_eq!(detect, restored);
    assert!(restored.capabilities.face_detect);

    // A pre-existing manifest JSON without any face capability field parses
    // with the flags defaulting to `false` (no migration, no schema bump).
    let legacy = serde_json::json!({
        "model_name": "Legacy",
        "model_version": "1",
        "model_hash": "pending-integration",
        "license": "MIT",
        "input": {
            "resolution": {"width": 4, "height": 4},
            "channel_layout": "rgb",
            "tensor_name": "input",
            "tensor_format": "nchw"
        },
        "capabilities": {
            "subject_segmentation": true,
            "box_prompt": false,
            "point_prompt": false,
            "mask_prompt": false,
            "class_detection": false,
            "instance_segmentation": false
        }
    })
    .to_string();
    let parsed = ModelManifest::from_json(&legacy).unwrap();
    assert!(!parsed.capabilities.face_detect);
    assert!(!parsed.capabilities.face_embed);

    // Unknown capability fields remain a loud error.
    let mut value = serde_json::to_value(detect).unwrap();
    value["capabilities"]["future_face_capability"] = serde_json::json!(true);
    assert!(ModelManifest::from_json(&value.to_string()).is_err());
}

/// SOLL FACE-20 §2.3: a stub backend that reports itself unavailable refuses
/// inference loudly (the visible "missing model" state), never silently emits
/// faces.
#[test]
fn missing_model_is_loud_and_never_silent() {
    let detector = StubFaceDetector::new(face_detect_manifest())
        .unwrap()
        .with_availability(false);
    let err = detector.detect(&frame(64, 64)).unwrap_err();
    assert!(matches!(err, OnnxError::ModelUnavailable { .. }), "{err:?}");

    let embedder = StubFaceEmbedder::new(face_embed_manifest(), 4)
        .unwrap()
        .with_availability(false);
    let err = embedder.embed(&frame(64, 64), &[detection(0)]).unwrap_err();
    assert!(matches!(err, OnnxError::ModelUnavailable { .. }), "{err:?}");
}

/// SOLL FACE-20 §2.1/§2.3: the full deterministic chain produces detections,
/// embeddings, clusters and person labels and bridges them into the S1 sidecar
/// schema with stable, content-derived ids.
#[test]
fn stub_pipeline_clusters_and_bridges_to_sidecar() {
    let face_frame = frame(640, 480);
    let detections = vec![detection(0), detection(1), detection(2)];
    let detector = StubFaceDetector::new(face_detect_manifest())
        .unwrap()
        .with_detections(detections.clone())
        .unwrap();
    let detected = detector.detect(&face_frame).unwrap();
    assert_eq!(detected.len(), 3);

    // Two identical identities + one orthogonal identity → two clusters.
    let embedder = StubFaceEmbedder::new(face_embed_manifest(), 2)
        .unwrap()
        .with_vectors(vec![vec![1.0, 0.0], vec![1.0, 0.0], vec![0.0, 1.0]])
        .unwrap();
    let embeddings = embedder.embed(&face_frame, &detected).unwrap();
    assert_eq!(embeddings.len(), 3);

    let vectors: Vec<Vec<f32>> = embeddings
        .iter()
        .map(|vector| vector.values().to_vec())
        .collect();
    let labels = cluster_embeddings(&vectors, &FaceClusteringParams::default()).unwrap();
    assert_eq!(labels[0], labels[1], "identical vectors must cluster");
    assert_ne!(labels[0], labels[2], "orthogonal vectors must not cluster");

    let detection_ids: Vec<String> = detected.iter().map(detected_face_id).collect();
    let clusters = clusters_from_labels(&detection_ids, &labels).unwrap();
    assert_eq!(clusters.len(), 2);
    for cluster in &clusters {
        assert!(
            cluster.id.starts_with("cluster-"),
            "cluster ids are content-derived, never array positions"
        );
    }

    let records = embeddings
        .iter()
        .enumerate()
        .map(|(index, vector)| FaceEmbeddingRecord {
            detection_index: index,
            vector: vector.clone(),
            reference: FaceVectorRef {
                relative_path: "IMAGE.ARW.lumina.zdata".into(),
                format: "lumina-zdata".into(),
                checksum: format!("{:064x}", index),
                dimension: 2,
                channels: "f32".into(),
                data_version: "1".into(),
                extras: Extras::new(),
            },
        })
        .collect();

    let (source, decode, geometry) = fingerprints();
    let identity = face_identity(
        &FaceModelSuite::candidate(),
        source,
        decode,
        geometry,
        clustering(),
        &FaceInferenceOptions::default(),
    )
    .unwrap();
    let analysis = FaceAnalysisOutput {
        identity,
        created_at: "2026-09-16T08:00:00Z".into(),
        detections: detected,
        embeddings: records,
        clusters: clusters.clone(),
        persons: vec![],
    }
    .into_sidecar()
    .unwrap();

    assert_eq!(analysis.detections.len(), 3);
    assert_eq!(analysis.embeddings.len(), 3);
    assert_eq!(analysis.clusters.len(), 2);
    // Reciprocal links are present and resolve.
    for detection in &analysis.detections {
        let embedding_id = detection.embedding_id.as_deref().unwrap();
        assert!(analysis
            .embeddings
            .iter()
            .any(|embedding| embedding.id == embedding_id));
    }
}

/// SOLL FACE-20 §4: changing either model (or the clustering version) makes a
/// persisted analysis `stale`; a missing/corrupt artifact is visible as such.
#[test]
fn model_and_artifact_changes_are_visible_states() {
    let (source, decode, geometry) = fingerprints();
    let current = face_identity(
        &FaceModelSuite::candidate(),
        source.clone(),
        decode.clone(),
        geometry.clone(),
        clustering(),
        &FaceInferenceOptions::default(),
    )
    .unwrap();

    // Model swap → stale (no silent re-inference).
    let mut changed_suite = FaceModelSuite::candidate();
    changed_suite.embedding.model_version = "2099".into();
    let changed_model = face_identity(
        &changed_suite,
        source.clone(),
        decode.clone(),
        geometry.clone(),
        clustering(),
        &FaceInferenceOptions::default(),
    )
    .unwrap();
    assert_ne!(
        face_identity_digest(&current),
        face_identity_digest(&changed_model)
    );
    assert_eq!(
        face_artifact_status(
            &changed_model,
            &current,
            FaceArtifactEvidence::Present {
                checksum_matches: true
            }
        ),
        FaceArtifactStatus::Stale
    );

    // Clustering version change → stale.
    let mut clustering_changed = clustering();
    clustering_changed.version += 1;
    let changed_clustering = face_identity(
        &FaceModelSuite::candidate(),
        source,
        decode,
        geometry,
        clustering_changed,
        &FaceInferenceOptions::default(),
    )
    .unwrap();
    assert_eq!(
        face_artifact_status(
            &changed_clustering,
            &current,
            FaceArtifactEvidence::Present {
                checksum_matches: true
            }
        ),
        FaceArtifactStatus::Stale
    );

    // Missing / corrupt override the identity comparison.
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
}

/// SOLL FACE-20 §2.3/§4: the analysis identity is derived from source/decode/
/// models/clustering only — person names are user data and never leak into the
/// identity digest. Cluster ids are content-derived from detection ids, not
/// names.
#[test]
fn person_names_never_enter_the_analysis_identity() {
    let (source, decode, geometry) = fingerprints();
    let build = |name: &str| {
        let identity = face_identity(
            &FaceModelSuite::candidate(),
            source.clone(),
            decode.clone(),
            geometry.clone(),
            clustering(),
            &FaceInferenceOptions::default(),
        )
        .unwrap();
        let detection = detection(0);
        let detection_id = detected_face_id(&detection);
        let clusters = clusters_from_labels(
            &[detection_id],
            &cluster_embeddings(&[vec![1.0, 0.0]], &FaceClusteringParams::default()).unwrap(),
        )
        .unwrap();
        FaceAnalysisOutput {
            identity,
            created_at: "2026-09-16T08:00:00Z".into(),
            detections: vec![detection],
            embeddings: vec![],
            clusters,
            persons: vec![lumina_sidecar::FacePerson {
                id: "person-1".into(),
                name: name.into(),
                confirmed: true,
                cluster_ids: vec!["cluster-ignored".into()],
                extras: Extras::new(),
            }],
        }
    };
    // Different names, same analysis identity → same digest (names are not
    // identity-bearing). The person record itself is not part of the digest.
    let a = build("Example One");
    let b = build("Example Two");
    assert_eq!(
        face_identity_digest(&a.identity),
        face_identity_digest(&b.identity)
    );
    assert_eq!(a.clusters[0].id, b.clusters[0].id);
}

/// SOLL FACE-20 §2.1: clustering is deterministic and permutation-invariant
/// end-to-end (the labels returned for a permuted embedding order describe the
/// same partition).
#[test]
fn clustering_is_deterministic_and_permutation_invariant() {
    let vectors = vec![
        vec![1.0f32, 0.0],
        vec![1.0, 0.0],
        vec![0.0, 1.0],
        vec![0.0, 1.0],
    ];
    let params = FaceClusteringParams::default();
    let base = cluster_embeddings(&vectors, &params).unwrap();
    assert_eq!(
        base,
        cluster_embeddings(&vectors, &params).unwrap(),
        "same input must yield identical labels"
    );

    let permutation = [3usize, 1, 0, 2];
    let permuted: Vec<Vec<f32>> = permutation.iter().map(|&i| vectors[i].clone()).collect();
    let permuted_labels = cluster_embeddings(&permuted, &params).unwrap();
    for (new_index, &old_index) in permutation.iter().enumerate() {
        assert_eq!(permuted_labels[new_index], base[old_index]);
    }

    // Empty input and a single face behave as documented.
    assert!(cluster_embeddings(&[], &params).unwrap().is_empty());
    assert_eq!(
        cluster_embeddings(&[vec![1.0, 0.0]], &params).unwrap(),
        vec![Some(0)]
    );
}

/// SOLL FACE-20 §2.1/§4: confirm/split/merge are pure data operations on
/// clusters and persons; every invalid operation is a loud error.
#[test]
fn cluster_operations_are_pure_and_loud() {
    use lumina_onnx::{confirm_person, merge_clusters, split_cluster};
    use lumina_sidecar::FaceCluster;

    let cluster = |id: &str, members: &[&str]| FaceCluster {
        id: id.into(),
        detection_ids: members.iter().map(|m| (*m).into()).collect(),
        extras: BTreeMap::new(),
    };
    let clusters = vec![
        cluster("cluster-a", &["face-1", "face-2"]),
        cluster("cluster-b", &["face-3"]),
    ];

    // Confirm.
    let (_, persons) = confirm_person(&clusters, &[], "cluster-a", "person-1", "Alex").unwrap();
    assert_eq!(persons.len(), 1);
    assert!(persons[0].confirmed);
    assert!(confirm_person(&clusters, &persons, "cluster-a", "person-2", "Sam").is_err());

    // Split.
    let split = split_cluster(&clusters, "cluster-a", &["face-2".into()], "cluster-c").unwrap();
    assert_eq!(split.len(), 3);
    assert!(split_cluster(&clusters, "cluster-a", &[], "cluster-c").is_err());
    assert!(split_cluster(
        &clusters,
        "cluster-a",
        &["face-1".into(), "face-2".into()],
        "cluster-c"
    )
    .is_err());

    // Merge.
    let merged = merge_clusters(&clusters, "cluster-a", "cluster-b", "cluster-a").unwrap();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].detection_ids.len(), 3);
    assert!(merge_clusters(&clusters, "cluster-a", "cluster-a", "cluster-a").is_err());
}

/// The planned descriptors and fixtures contain no personal data: the shipped
/// model descriptors never carry a name, and the clustering identity is
/// name-free.
#[test]
fn shipped_descriptors_and_identity_are_name_free() {
    for manifest in [face_detect_manifest(), face_embed_manifest()] {
        let json = manifest.to_json().unwrap();
        assert!(!json.to_lowercase().contains("person"));
        assert!(!json.to_lowercase().contains("alex"));
    }
    let identity = clustering();
    let serialized = serde_json::to_string(&identity).unwrap();
    assert!(!serialized.contains("name"));
}

/// The embedding vector type carries no persistence path: only a
/// `FaceVectorRef` (relative path + checksum) is persisted, and absolute paths
/// are rejected by the sidecar schema.
#[test]
fn embeddings_persist_as_references_not_inline_vectors() {
    let vector = FaceEmbeddingVector::new(vec![0.6, 0.8]).unwrap();
    let record = FaceEmbeddingRecord {
        detection_index: 0,
        vector,
        reference: FaceVectorRef {
            relative_path: "/abs/vector.bin".into(),
            format: "lumina-zdata".into(),
            checksum: "aa".repeat(32),
            dimension: 2,
            channels: "f32".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        },
    };
    let (source, decode, geometry) = fingerprints();
    let result = FaceAnalysisOutput {
        identity: face_identity(
            &FaceModelSuite::candidate(),
            source,
            decode,
            geometry,
            clustering(),
            &FaceInferenceOptions::default(),
        )
        .unwrap(),
        created_at: "2026-09-16T08:00:00Z".into(),
        detections: vec![detection(0)],
        embeddings: vec![record],
        clusters: vec![],
        persons: vec![],
    }
    .into_sidecar();
    assert!(
        matches!(result, Err(OnnxError::InvalidFaceData(_))),
        "absolute artifact paths must be rejected: {result:?}"
    );
}

/// F2: in the default (no `onnx-rt`) build, requesting the real face engine is
/// the explicit `RuntimeDisabled` capability statement — even for a
/// nonexistent artifact path — never a silent stub fallback. (With `onnx-rt`
/// enabled this test is compiled out; the real-path coverage lives in
/// `tests/face_ort.rs`.)
#[cfg(not(feature = "onnx-rt"))]
#[test]
fn face_engine_reports_runtime_disabled_without_the_feature() {
    use lumina_onnx::{try_load_face_engine, FaceOnnxEngine};
    use std::path::Path;

    let engine = try_load_face_engine(
        Path::new("/nonexistent/lumina/face-detect.onnx"),
        &FaceModelSuite::candidate(),
        Path::new("/nonexistent/lumina/face-embed.onnx"),
        &FaceInferenceOptions::default(),
    )
    .expect("feature-off load must succeed with the explicit capability statement");
    assert!(
        matches!(engine, FaceOnnxEngine::RuntimeDisabled),
        "expected the explicit capability statement, got {engine:?}"
    );
}

/// F1: the persisted identity digest must not be self-referential. A persisted
/// analysis carrying the extra and one without it describe the same identity
/// and resolve to `Valid` (never a false `Stale`); a real model change is
/// still `Stale`.
#[test]
fn persisted_identity_digest_is_not_self_referential() {
    let (source, decode, geometry) = fingerprints();
    let build = |suite: &FaceModelSuite| {
        face_identity_with_digest(
            suite,
            source.clone(),
            decode.clone(),
            geometry.clone(),
            clustering(),
            &FaceInferenceOptions::default(),
        )
        .unwrap()
    };
    let with_digest = build(&FaceModelSuite::candidate());
    let stored = with_digest.extras.get(FACE_IDENTITY_DIGEST_KEY).unwrap();
    let serde_json::Value::String(stored) = stored else {
        panic!("digest extra must be a string");
    };
    assert_eq!(
        &face_identity_digest(&with_digest),
        stored,
        "the stored digest must equal a recomputation over the digest-bearing identity"
    );

    let without_digest = face_identity(
        &FaceModelSuite::candidate(),
        source.clone(),
        decode.clone(),
        geometry.clone(),
        clustering(),
        &FaceInferenceOptions::default(),
    )
    .unwrap();
    let evidence = FaceArtifactEvidence::Present {
        checksum_matches: true,
    };
    assert_eq!(
        face_artifact_status(&with_digest, &without_digest, evidence),
        FaceArtifactStatus::Valid,
        "mixed digest presence must not report a false stale"
    );
    assert_eq!(
        face_artifact_status(&without_digest, &with_digest, evidence),
        FaceArtifactStatus::Valid
    );

    // A genuine identity change is still `stale`, digest extra present or not.
    let mut changed_suite = FaceModelSuite::candidate();
    changed_suite.embedding.model_version = "2099".into();
    let changed = build(&changed_suite);
    assert_eq!(
        face_artifact_status(&changed, &with_digest, evidence),
        FaceArtifactStatus::Stale
    );
}
