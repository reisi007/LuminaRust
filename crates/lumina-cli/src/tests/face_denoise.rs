use super::*;

/// FACE-20-IMPL-20-REST: `face --analyze` must treat a persisted analysis
/// with verified `face_embedding` records as already valid and refuse to
/// re-run without `--force` — *before* it needs any ONNX artifact, which is
/// exactly why this is observable without `onnx-rt`.
#[test]
fn face_analyze_short_circuits_on_a_verified_record_analysis() {
    use lumina_onnx::{
        detected_face_id, embedding_id_for, FaceAnalysisOutput, FaceClusteringParams,
        FaceDetectionInference, FaceEmbeddingInference, FaceEmbeddingRecord, FaceInferenceOptions,
        FaceModelSuite, StubFaceDetector, StubFaceEmbedder,
    };
    use lumina_sidecar::{
        load_sidecar, load_zdata, save_face_embeddings, sidecar_path_for, zdata_path_for,
        FaceEmbeddingArtifact, FaceVectorRef, SidecarDocument, SourceFingerprint,
    };

    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.png");
    let frame = lumina_core::ImageFrame::new(8, 6, vec![0u8; 8 * 6 * 4]).unwrap();
    fs::write(
        &input,
        frame.encode(lumina_core::ImageFileFormat::Png).unwrap(),
    )
    .unwrap();
    let sidecar = sidecar_path_for(&input);
    let mut document = SidecarDocument::new(
        lumina_sidecar::SourceIdentity {
            relative_name: "input.png".into(),
            content_hash: "blake3:fixture".into(),
            byte_length: fs::metadata(&input).unwrap().len(),
            modified_at: None,
            raw_format: "PNG".into(),
            orientation: 1,
            decode_fingerprint: lumina_sidecar::DecodeFingerprint {
                decoder: "image".into(),
                version: "1".into(),
                parameters: std::collections::BTreeMap::new(),
                extras: Default::default(),
            },
            geometry_fingerprint: lumina_sidecar::GeometryFingerprint {
                width: 8,
                height: 6,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: Default::default(),
            },
            extras: Default::default(),
        },
        "pipeline-1",
    );
    save_sidecar(&sidecar, &document).unwrap();

    let suite = FaceModelSuite::candidate();
    let detector = StubFaceDetector::new(suite.detection.clone()).unwrap();
    let detections = detector.detect(&frame).unwrap();
    let embedder =
        StubFaceEmbedder::new(suite.embedding.clone(), suite.embedding_dimension).unwrap();
    let vectors = embedder.embed(&frame, &detections).unwrap();
    let detection_ids: Vec<String> = detections.iter().map(detected_face_id).collect();
    let records: Vec<FaceEmbeddingArtifact> = vectors
        .iter()
        .enumerate()
        .map(|(index, vector)| FaceEmbeddingArtifact {
            id: embedding_id_for(&detection_ids[index]),
            dimension: vector.dimension() as u32,
            values: vector.values().to_vec(),
        })
        .collect();
    save_face_embeddings(&zdata_path_for(&input), records.clone()).unwrap();
    let embeddings: Vec<FaceEmbeddingRecord> = vectors
        .iter()
        .enumerate()
        .map(|(index, vector)| FaceEmbeddingRecord {
            detection_index: index,
            vector: vector.clone(),
            reference: FaceVectorRef {
                relative_path: "input.png.lumina.zdata".into(),
                format: "lumina-zdata".into(),
                checksum: records[index].checksum(),
                dimension: vector.dimension() as u32,
                channels: "f32".into(),
                data_version: "1".into(),
                extras: Default::default(),
            },
        })
        .collect();
    let identity = lumina_onnx::face_identity(
        &suite,
        SourceFingerprint {
            content_hash: document.source.content_hash.clone(),
            byte_length: document.source.byte_length,
            extras: Default::default(),
        },
        document.source.decode_fingerprint.clone(),
        document.source.geometry_fingerprint.clone(),
        FaceClusteringParams::default().to_identity(),
        &FaceInferenceOptions::default(),
    )
    .unwrap();
    let analysis = FaceAnalysisOutput {
        identity,
        created_at: now_rfc3339_utc(),
        detections,
        embeddings,
        clusters: vec![],
        persons: vec![],
    }
    .into_sidecar()
    .unwrap();
    document.face = Some(analysis);
    save_sidecar(&sidecar, &document).unwrap();

    // The bundle really holds verified records for the persisted analysis.
    let reloaded = load_sidecar(&sidecar).unwrap();
    let analysis = reloaded.face.as_ref().unwrap();
    let evidence = face_artifact_evidence(directory.path(), analysis);
    assert_eq!(
        evidence,
        FaceArtifactEvidence::Present {
            checksum_matches: true
        }
    );
    let bundle = load_zdata(&zdata_path_for(&input)).unwrap();
    for embedding in &analysis.embeddings {
        assert!(bundle.has_record(lumina_sidecar::RecordKind::FaceEmbedding, &embedding.id));
    }
    let loaded = load_zdata(&zdata_path_for(&input))
        .unwrap()
        .face_embedding(&analysis.embeddings[0].id)
        .unwrap();
    assert_eq!(loaded.checksum(), analysis.embeddings[0].vector.checksum);

    // `--analyze` without `--force` short-circuits before the engine load,
    // so it succeeds even without `onnx-rt` and without artifacts.
    let mut reloaded = load_sidecar(&sidecar).unwrap();
    face_analyze(
        &FaceArgs {
            input: input.clone(),
            status: false,
            analyze: true,
            force: false,
            detector: None,
            embedder: None,
            json: true,
        },
        &mut reloaded,
        &sidecar,
    )
    .expect("a verified analysis short-circuits as already valid");
}

/// AI-CLI-SLICES (DENOISE): the bundle record id is content-derived (never
/// positional) and stable across calls.
#[test]
fn denoise_record_id_is_content_derived_and_stable() {
    let checksum = "ab".repeat(32);
    let id = denoise_record_id(&checksum);
    assert_eq!(id, format!("denoise_rgb:{}", &checksum[..16]));
    assert_eq!(id, denoise_record_id(&checksum));
    assert_ne!(id, denoise_record_id(&"cd".repeat(32)));
}

/// AI-CLI-SLICES (DENOISE): the §6 reason mapping is empty exactly for the
/// two non-failing classes and non-empty for every visible non-ready one.
#[test]
fn denoise_status_reason_covers_every_status_class() {
    let zdata = Path::new("/tmp/bundle.lumina.zdata");
    assert!(denoise_status_reason(DenoiseStageStatus::Inactive, zdata).is_empty());
    assert!(denoise_status_reason(DenoiseStageStatus::Ready, zdata).is_empty());
    for status in [
        DenoiseStageStatus::Unavailable,
        DenoiseStageStatus::Missing,
        DenoiseStageStatus::Stale,
        DenoiseStageStatus::Corrupt,
    ] {
        assert!(
            !denoise_status_reason(status, zdata).is_empty(),
            "{status:?} must carry a visible reason"
        );
    }
}
