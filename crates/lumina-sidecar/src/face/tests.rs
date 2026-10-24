use super::*;
use crate::{
    load_sidecar, migrate_json, save_sidecar, DecodeFingerprint, SidecarDocument, SourceIdentity,
};
use std::collections::BTreeMap;
use std::fs;

fn source() -> SourceIdentity {
    SourceIdentity {
        relative_name: "IMG_0001.ARW".into(),
        content_hash: "blake3:abc".into(),
        byte_length: 42,
        modified_at: None,
        raw_format: "ARW".into(),
        orientation: 1,
        decode_fingerprint: DecodeFingerprint {
            decoder: "libraw".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: Extras::new(),
        },
        geometry_fingerprint: GeometryFingerprint {
            width: 6000,
            height: 4000,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: Extras::new(),
        },
        extras: Extras::new(),
    }
}

fn model(name: &str) -> ModelIdentity {
    ModelIdentity {
        name: name.into(),
        version: "1.0".into(),
        hash: FACE_PENDING_MODEL_HASH.into(),
        extras: Extras::new(),
    }
}

fn vector_ref() -> FaceVectorRef {
    FaceVectorRef {
        relative_path: "IMG_0001.ARW.lumina.zdata".into(),
        format: "lumina-zdata".into(),
        checksum: "aa".repeat(32),
        dimension: 512,
        channels: "f32".into(),
        data_version: "1".into(),
        extras: Extras::new(),
    }
}

fn analysis() -> FaceAnalysis {
    let detection = FaceDetection {
        id: "face-1".into(),
        bbox: FaceBoundingBox {
            x: 0.1,
            y: 0.2,
            width: 0.3,
            height: 0.4,
        },
        score: 0.99,
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
        ],
        embedding_id: Some("emb-1".into()),
        matte: Some(ArtifactReference {
            relative_path: "IMG_0001.ARW.lumina.zdata".into(),
            format: "zdata-mask".into(),
            checksum: "bb".repeat(32),
            width: 1024,
            height: 1024,
            channels: "f32".into(),
            data_version: "1".into(),
            extras: Extras::new(),
        }),
        status: FaceArtifactStatus::Valid,
        error: None,
        extras: Extras::new(),
    };
    let embedding = FaceEmbedding {
        id: "emb-1".into(),
        detection_id: "face-1".into(),
        vector: vector_ref(),
        status: FaceArtifactStatus::Valid,
        error: None,
        extras: Extras::new(),
    };
    FaceAnalysis {
        version: FACE_SCHEMA_VERSION,
        identity: FaceIdentity {
            source: SourceFingerprint {
                content_hash: "blake3:abc".into(),
                byte_length: 42,
                extras: Extras::new(),
            },
            decode: source().decode_fingerprint,
            geometry: source().geometry_fingerprint,
            detection_model: model("scrfd"),
            embedding_model: model("arcface"),
            inference_resolution: Resolution {
                width: 640,
                height: 640,
                extras: Extras::new(),
            },
            preprocessing: Preprocessing {
                name: "landmark_align".into(),
                version: "1".into(),
                parameters: BTreeMap::from([("mean".into(), "0.5".into())]),
                extras: Extras::new(),
            },
            rescaling_method: "identity".into(),
            coordinate_system: CoordinateSystem::SourceOriented,
            clustering: FaceClusteringIdentity {
                method: "dbscan_cosine".into(),
                version: 1,
                parameters: BTreeMap::from([("eps".into(), "0.4".into())]),
                extras: Extras::new(),
            },
            extras: Extras::new(),
        },
        detections: vec![detection],
        embeddings: vec![embedding],
        clusters: vec![FaceCluster {
            id: "cluster-1".into(),
            detection_ids: vec!["face-1".into()],
            extras: Extras::new(),
        }],
        persons: vec![FacePerson {
            id: "person-1".into(),
            name: "Alex".into(),
            confirmed: true,
            cluster_ids: vec!["cluster-1".into()],
            extras: Extras::new(),
        }],
        created_at: "2026-09-16T08:00:00Z".into(),
        status: FaceArtifactStatus::Valid,
        error: None,
        extras: Extras::new(),
    }
}

fn document_with_face() -> SidecarDocument {
    let mut document = SidecarDocument::new(source(), "pipeline-1");
    document.face = Some(analysis());
    document
}

#[test]
fn face_section_is_absent_by_default_and_serializes_absent() {
    let document = SidecarDocument::new(source(), "pipeline-1");
    assert!(document.face.is_none());
    let json = document.to_json().unwrap();
    assert!(!json.contains("\"face\""), "absent face must not serialize");
    let decoded = SidecarDocument::from_json(&json).unwrap();
    assert!(decoded.face.is_none());
}

#[test]
fn face_roundtrip_is_lossless_and_byte_stable() {
    let document = document_with_face();
    let json = document.to_json().unwrap();
    assert!(json.contains("face-1"));
    assert!(json.contains("arcface"));
    let decoded = SidecarDocument::from_json(&json).unwrap();
    assert_eq!(decoded, document);
    assert_eq!(decoded.to_json().unwrap(), json);
}

#[test]
fn face_unknown_fields_roundtrip_as_extras() {
    let document = document_with_face();
    let mut value: serde_json::Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
    value["face"]["future_face"] = serde_json::Value::from(1);
    value["face"]["identity"]["detection_model"]["future_model"] = serde_json::Value::from("kept");
    value["face"]["detections"][0]["future_detection"] = serde_json::Value::from(true);
    let decoded = SidecarDocument::from_json(&serde_json::to_string(&value).unwrap()).unwrap();
    let reencoded: serde_json::Value = serde_json::from_str(&decoded.to_json().unwrap()).unwrap();
    assert_eq!(reencoded["face"]["future_face"], serde_json::Value::from(1));
    assert_eq!(
        reencoded["face"]["identity"]["detection_model"]["future_model"],
        serde_json::Value::from("kept")
    );
    assert_eq!(
        reencoded["face"]["detections"][0]["future_detection"],
        serde_json::Value::from(true)
    );
}

#[test]
fn face_legacy_documents_load_without_face_and_migrate_without_loss() {
    // Current-schema JSON without the additive key.
    let legacy = r#"{"format":"lumina-sidecar","schema_version":2,"source":{"relative_name":"x","content_hash":"h","byte_length":1,"raw_format":"PNG","orientation":1,"decode_fingerprint":{"decoder":"d","version":"1","parameters":{}},"geometry_fingerprint":{"width":1,"height":1,"orientation":1,"pixel_aspect_ratio":1.0}},"pipeline_version":"p","presets":[],"virtual_copies":[{"id":"vc-original","name":"Original","is_default":true,"recipe":{},"mask_library":[],"mask_layers":[],"history":[],"export_records":[]}]}"#;
    let document = SidecarDocument::from_json(legacy).unwrap();
    assert!(document.face.is_none());
    assert!(!document.to_json().unwrap().contains("\"face\""));
    // Explicit v1 → v2 migration preserves the legacy document.
    let v1 = legacy.replace("\"schema_version\":2", "\"schema_version\":1");
    let migrated = migrate_json(&v1).unwrap();
    let decoded = SidecarDocument::from_json(&migrated).unwrap();
    assert_eq!(decoded.schema_version, crate::SCHEMA_VERSION);
    assert!(decoded.face.is_none());
}

#[test]
fn face_file_roundtrip_is_atomic_and_reloadable() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("IMAGE.lumina.json");
    let document = document_with_face();
    save_sidecar(&path, &document).unwrap();
    let loaded = load_sidecar(&path).unwrap();
    assert_eq!(loaded, document);
    // Recovery: a crashed temporary is swept, the valid sidecar survives.
    let temp = directory.path().join(".IMAGE.lumina.json.tmp-crash");
    fs::write(&temp, b"{\"partial\": true}").unwrap();
    let file = fs::OpenOptions::new().write(true).open(&temp).unwrap();
    file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(60))
        .unwrap();
    let reloaded = load_sidecar(&path).unwrap();
    assert_eq!(reloaded, document);
    assert!(!temp.exists());
}

#[test]
fn face_validation_rejects_unknown_version() {
    let mut face = analysis();
    face.version = 2;
    assert!(validate_face_analysis(&face).is_err());
}

#[test]
fn face_validation_rejects_inconsistent_references() {
    // Detection points at an embedding of another detection.
    let mut face = analysis();
    face.embeddings[0].detection_id = "face-other".into();
    assert!(validate_face_analysis(&face).is_err());
    // Detection points at an unknown embedding.
    let mut face = analysis();
    face.detections[0].embedding_id = Some("emb-missing".into());
    assert!(validate_face_analysis(&face).is_err());
    // Cluster references an unknown detection.
    let mut face = analysis();
    face.clusters[0].detection_ids = vec!["face-missing".into()];
    assert!(validate_face_analysis(&face).is_err());
    // Person references an unknown cluster.
    let mut face = analysis();
    face.persons[0].cluster_ids = vec!["cluster-missing".into()];
    assert!(validate_face_analysis(&face).is_err());
    // Duplicate detection ids are rejected, never merged.
    let mut face = analysis();
    let duplicate = face.detections[0].clone();
    face.detections.push(duplicate);
    assert!(validate_face_analysis(&face).is_err());
}

#[test]
fn face_validation_rejects_out_of_range_geometry_and_scores() {
    let cases: [fn(&mut FaceAnalysis); 13] = [
        |face| face.detections[0].score = 1.5,
        |face| face.detections[0].score = f32::NAN,
        |face| face.detections[0].bbox.width = 0.0,
        |face| face.detections[0].bbox.x = 0.9,
        |face| face.detections[0].landmarks[0].x = 1.5,
        |face| face.detections[0].landmarks[0].name.clear(),
        |face| face.detections[0].error = Some("   ".into()),
        |face| face.embeddings[0].vector.dimension = 0,
        |face| face.embeddings[0].vector.relative_path = "/abs/vector.bin".into(),
        |face| face.detections[0].matte.as_mut().unwrap().width = 0,
        |face| face.identity.clustering.version = 0,
        |face| face.identity.detection_model.hash = "dummy".into(),
        |face| face.created_at = "not-a-timestamp".into(),
    ];
    for mutate in cases {
        let mut face = analysis();
        mutate(&mut face);
        assert!(
            validate_face_analysis(&face).is_err(),
            "face contract violation must be rejected loudly"
        );
    }
}

#[test]
fn face_accepts_real_sha256_and_pending_model_hash() {
    let mut face = analysis();
    face.identity.detection_model.hash = format!("{}{}", FACE_SHA256_PREFIX, "ab".repeat(32));
    face.identity.embedding_model.hash = FACE_PENDING_MODEL_HASH.into();
    validate_face_analysis(&face).expect("real pin + pending are both legal");
    face.identity.embedding_model.hash = format!("{}{}", FACE_SHA256_PREFIX, "AB".repeat(32));
    assert!(
        validate_face_analysis(&face).is_err(),
        "uppercase hex rejected"
    );
}

/// D1-Nacharbeit (Schema-Verifizierung): every `FaceArtifactStatus`
/// variant must survive a serde roundtrip on its documented lowercase wire
/// form. The earlier coverage only exercised `Valid`; this table adds the
/// visible degraded states `stale`/`missing`/`corrupt` (per analysis and
/// per detection/embedding).
#[test]
fn serde_roundtrip_covers_every_artifact_status() {
    for (variant, wire) in [
        (FaceArtifactStatus::Valid, "\"valid\""),
        (FaceArtifactStatus::Stale, "\"stale\""),
        (FaceArtifactStatus::Missing, "\"missing\""),
        (FaceArtifactStatus::Corrupt, "\"corrupt\""),
    ] {
        let json = serde_json::to_string(&variant).unwrap();
        assert_eq!(json, wire, "FaceArtifactStatus wire form");
        let decoded: FaceArtifactStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, variant);
    }
}

/// D1-Nacharbeit: the nested face analysis must roundtrip losslessly with
/// the degraded statuses set on the section, the detection and the
/// embedding at once — including an explicit error text. This pins the
/// placement of the status field in all three locations.
#[test]
fn serde_roundtrip_matrix_covers_degraded_artifact_statuses() {
    for status in [
        FaceArtifactStatus::Valid,
        FaceArtifactStatus::Stale,
        FaceArtifactStatus::Missing,
        FaceArtifactStatus::Corrupt,
    ] {
        let mut face = analysis();
        face.status = status;
        face.detections[0].status = status;
        face.embeddings[0].status = status;
        let error = (!matches!(status, FaceArtifactStatus::Valid))
            .then(|| format!("artifact state {status:?}"));
        face.error = error.clone();
        face.detections[0].error = error.clone();
        face.embeddings[0].error = error;
        validate_face_analysis(&face).unwrap_or_else(|e| panic!("{status:?} must be valid: {e}"));
        let json = serde_json::to_string(&face).unwrap();
        let decoded: FaceAnalysis = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, face);
        assert_eq!(serde_json::to_string(&decoded).unwrap(), json);
        assert_eq!(decoded.status, status);
        assert_eq!(decoded.detections[0].status, status);
        assert_eq!(decoded.embeddings[0].status, status);
    }
}
