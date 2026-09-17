//! LRPAR-G12-FACE-IMPL-20 / S4: end-to-end tests for `lumina face`.
//!
//! SOLL: `feature/decisions/LRPAR-G12-FACE-20.md` §2.3/§3.2/§4/§6. The tests
//! close the S2/S3 file-level gap: a full detection → embedding → clustering →
//! `into_sidecar` analysis is written to a real sidecar file (`save_sidecar`)
//! with its vectors persisted as `face_embedding` records
//! (`save_face_embeddings`) and reloaded through the CLI status path. Without
//! the `onnx-rt` capability `--analyze` refuses loudly and writes nothing
//! (never a stub fallback).

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_onnx::{
    cluster_embeddings, clusters_from_labels, detected_face_id, embedding_id_for, face_identity,
    FaceAnalysisOutput, FaceClusteringParams, FaceDetectionInference, FaceEmbeddingInference,
    FaceEmbeddingRecord, FaceInferenceOptions, FaceModelSuite, StubFaceDetector, StubFaceEmbedder,
};
use lumina_sidecar::{
    load_sidecar, load_zdata, save_face_embeddings, save_sidecar, sidecar_path_for, zdata_path_for,
    FaceAnalysis, FaceArtifactStatus, FaceEmbeddingArtifact, FaceVectorRef, SidecarDocument,
    SourceFingerprint, ZDataContainer,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lumina-cli"))
}

fn gradient_png(dir: &tempfile::TempDir) -> PathBuf {
    let (w, h) = (16u32, 12u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let v = (x * 13 + y * 7) as u8;
            rgba.extend_from_slice(&[v, v.wrapping_add(30), v.wrapping_add(60), 255]);
        }
    }
    let path = dir.path().join("input.png");
    let frame = ImageFrame::new(w, h, rgba).unwrap();
    fs::write(&path, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
    path
}

fn import(input: &Path) {
    let output = cli()
        .args(["import", "--input", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "import failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Builds a complete S2/S3 analysis (stub detector + embedder + deterministic
/// clustering) and bridges it through `into_sidecar` using the exact live
/// identity the CLI derives from `document.source`.
///
/// The vectors are persisted as real `face_embedding` records and the
/// references carry exactly their record checksums (FACE-20 §3.2): the CLI
/// verifies that checksum, so a fixture with a fabricated checksum or without
/// records would (correctly) classify as `corrupt`/`missing` instead of
/// `valid`.
fn build_analysis(document: &SidecarDocument, frame: &ImageFrame, input: &Path) -> FaceAnalysis {
    let vector_path = zdata_path_for(input);
    let relative_path = vector_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap()
        .to_string();
    let suite = FaceModelSuite::candidate();
    let detector = StubFaceDetector::new(suite.detection.clone()).unwrap();
    let detections = detector.detect(frame).unwrap();
    let embedder =
        StubFaceEmbedder::new(suite.embedding.clone(), suite.embedding_dimension).unwrap();
    let vectors = embedder.embed(frame, &detections).unwrap();
    let raw: Vec<Vec<f32>> = vectors
        .iter()
        .map(|vector| vector.values().to_vec())
        .collect();
    let labels = cluster_embeddings(&raw, &FaceClusteringParams::default()).unwrap();
    let detection_ids: Vec<String> = detections.iter().map(detected_face_id).collect();
    let clusters = clusters_from_labels(&detection_ids, &labels).unwrap();
    let records: Vec<FaceEmbeddingArtifact> = vectors
        .iter()
        .enumerate()
        .map(|(index, vector)| FaceEmbeddingArtifact {
            id: embedding_id_for(&detection_ids[index]),
            dimension: vector.dimension() as u32,
            values: vector.values().to_vec(),
        })
        .collect();
    let embeddings: Vec<FaceEmbeddingRecord> = vectors
        .iter()
        .enumerate()
        .map(|(index, vector)| FaceEmbeddingRecord {
            detection_index: index,
            vector: vector.clone(),
            reference: FaceVectorRef {
                relative_path: relative_path.clone(),
                format: "lumina-zdata".into(),
                checksum: records[index].checksum(),
                dimension: vector.dimension() as u32,
                channels: "f32".into(),
                data_version: "1".into(),
                extras: Default::default(),
            },
        })
        .collect();
    save_face_embeddings(&vector_path, records).unwrap();
    let identity = face_identity(
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
    FaceAnalysisOutput {
        identity,
        created_at: "2026-09-16T00:00:00Z".into(),
        detections,
        embeddings,
        clusters,
        persons: vec![],
    }
    .into_sidecar()
    .unwrap()
}

fn status_json(input: &Path) -> (bool, String) {
    let output = cli()
        .args([
            "face",
            "--input",
            input.to_str().unwrap(),
            "--status",
            "--json",
        ])
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
    )
}

#[test]
fn status_without_analysis_reports_no_analysis() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir);
    import(&input);
    let (ok, stdout) = status_json(&input);
    assert!(ok);
    assert!(stdout.contains("\"status\":\"no-analysis\""));
}

/// into_sidecar → real sidecar file → CLI reload: the valid analysis is
/// reported with its counts (closes the S2/S3 file-level E2E gap).
#[test]
fn persisted_analysis_roundtrips_through_the_sidecar_and_reloads_as_valid() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir);
    import(&input);
    let path = sidecar_path_for(&input);
    let frame = ImageFrame::decode(&fs::read(&input).unwrap()).unwrap();

    let mut document = load_sidecar(&path).unwrap();
    let analysis = build_analysis(&document, &frame, &input);
    assert_eq!(analysis.detections.len(), 1);
    assert_eq!(analysis.clusters.len(), 1);
    document.face = Some(analysis);
    save_sidecar(&path, &document).unwrap();

    // Reload from disk proves the write persisted (not just in-memory state).
    assert!(load_sidecar(&path).unwrap().face.is_some());
    let (ok, stdout) = status_json(&input);
    assert!(ok, "valid analysis must exit 0");
    assert!(stdout.contains("\"status\":\"valid\""));
    assert!(stdout.contains("\"detections\":1"));
    assert!(stdout.contains("\"clusters\":1"));
    assert!(stdout.contains("\"persons\":0"));
}

/// A changed source/decode context makes the persisted analysis visible as
/// `stale` (never silently re-run).
#[test]
fn changed_decode_context_is_stale() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir);
    import(&input);
    let path = sidecar_path_for(&input);
    let frame = ImageFrame::decode(&fs::read(&input).unwrap()).unwrap();
    let mut document = load_sidecar(&path).unwrap();
    document.face = Some(build_analysis(&document, &frame, &input));
    // Simulate a decode change after the analysis was produced.
    document.source.decode_fingerprint.version = "other-decoder-version".into();
    save_sidecar(&path, &document).unwrap();

    let (ok, stdout) = status_json(&input);
    assert!(ok);
    assert!(stdout.contains("\"status\":\"stale\""));
}

/// Persisted `missing`/`corrupt` states are surfaced as-is; `corrupt` is a hard
/// exit (never an automatic re-run).
#[test]
fn persisted_missing_and_corrupt_states_are_visible() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir);
    import(&input);
    let path = sidecar_path_for(&input);
    let frame = ImageFrame::decode(&fs::read(&input).unwrap()).unwrap();
    let mut document = load_sidecar(&path).unwrap();
    document.face = Some(build_analysis(&document, &frame, &input));
    save_sidecar(&path, &document).unwrap();

    document.face.as_mut().unwrap().status = FaceArtifactStatus::Missing;
    save_sidecar(&path, &document).unwrap();
    let (ok, stdout) = status_json(&input);
    assert!(ok);
    assert!(stdout.contains("\"status\":\"missing\""));

    document.face.as_mut().unwrap().status = FaceArtifactStatus::Corrupt;
    save_sidecar(&path, &document).unwrap();
    let (ok, stdout) = status_json(&input);
    assert!(!ok, "corrupt must exit non-zero");
    assert!(stdout.contains("\"status\":\"corrupt\""));
}

/// FACE-20 §3.2: a referenced `face_embedding` record whose payload no longer
/// hashes to the persisted checksum is a hard `corrupt` (exit 1) — never a
/// silently trusted `valid`.
#[test]
fn tampered_vector_record_is_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir);
    import(&input);
    let path = sidecar_path_for(&input);
    let frame = ImageFrame::decode(&fs::read(&input).unwrap()).unwrap();
    let mut document = load_sidecar(&path).unwrap();
    let analysis = build_analysis(&document, &frame, &input);
    let embedding_id = analysis.embeddings[0].id.clone();
    document.face = Some(analysis);
    save_sidecar(&path, &document).unwrap();

    // Persist exactly the mutated vector under the referenced id: the payload
    // no longer hashes to the persisted reference checksum.
    let vector_path = zdata_path_for(&input);
    let mut record = load_zdata(&vector_path)
        .unwrap()
        .face_embedding(&embedding_id)
        .unwrap();
    let original_checksum = record.checksum();
    record.values[0] = f32::from_bits(record.values[0].to_bits() - 1);
    assert_ne!(record.checksum(), original_checksum);
    lumina_sidecar::save_face_embeddings(&vector_path, vec![record]).unwrap();

    let (ok, stdout) = status_json(&input);
    assert!(!ok, "a checksum mismatch must be a hard corrupt");
    assert!(stdout.contains("\"status\":\"corrupt\""));
    assert!(stdout.contains("\"artifact_evidence\":\"checksum-mismatch\""));
}

/// FACE-20 §3.2: a bundle that is not a loadable container while the referenced
/// file exists is `corrupt` (the payload exists but is unusable), never a
/// false `valid`.
#[test]
fn unusable_bundle_is_corrupt_not_valid() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir);
    import(&input);
    let path = sidecar_path_for(&input);
    let frame = ImageFrame::decode(&fs::read(&input).unwrap()).unwrap();
    let mut document = load_sidecar(&path).unwrap();
    document.face = Some(build_analysis(&document, &frame, &input));
    save_sidecar(&path, &document).unwrap();

    fs::write(zdata_path_for(&input), b"tampered-face-vectors").unwrap();

    let (ok, stdout) = status_json(&input);
    assert!(!ok, "an unloadable bundle must be a hard corrupt");
    assert!(stdout.contains("\"status\":\"corrupt\""));
    assert!(stdout.contains("\"artifact_evidence\":\"checksum-mismatch\""));
}

/// FACE-20 §3.2: an analysis whose records were never written (the pre-record
/// layout: references exist, the bundle does not) is `missing` (exit 0), never
/// a false `valid`.
#[test]
fn missing_vector_records_are_missing_not_valid() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir);
    import(&input);
    let path = sidecar_path_for(&input);
    let frame = ImageFrame::decode(&fs::read(&input).unwrap()).unwrap();
    let mut document = load_sidecar(&path).unwrap();
    document.face = Some(build_analysis(&document, &frame, &input));
    save_sidecar(&path, &document).unwrap();

    fs::remove_file(zdata_path_for(&input)).unwrap();

    let (ok, stdout) = status_json(&input);
    assert!(ok, "a missing artifact is not a hard error");
    assert!(stdout.contains("\"status\":\"missing\""));
    assert!(stdout.contains("\"artifact_evidence\":\"missing\""));
}

/// FACE-20 §3.2: a bundle that exists but holds no `face_embedding` record for
/// the reference (e.g. only mask tiles) is `missing` — the reference dangles,
/// so validity cannot be proven.
#[test]
fn dangling_embedding_reference_is_missing_not_valid() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir);
    import(&input);
    let path = sidecar_path_for(&input);
    let frame = ImageFrame::decode(&fs::read(&input).unwrap()).unwrap();
    let mut document = load_sidecar(&path).unwrap();
    document.face = Some(build_analysis(&document, &frame, &input));
    save_sidecar(&path, &document).unwrap();

    // Keep the bundle, drop every face record.
    let tiles = vec![lumina_sidecar::MaskTile {
        mask_id: "subject".into(),
        tile_x: 0,
        tile_y: 0,
        width: 1,
        height: 1,
        values: vec![32768],
    }];
    lumina_sidecar::save_zdata(
        &zdata_path_for(&input),
        &ZDataContainer::new(tiles).unwrap(),
    )
    .unwrap();

    let (ok, stdout) = status_json(&input);
    assert!(ok);
    assert!(stdout.contains("\"status\":\"missing\""));
    assert!(stdout.contains("\"artifact_evidence\":\"missing\""));
}

/// M2: an analysis with no persisted embedding references has no verifiable
/// payload at all and must be `missing`, never a false `valid` (mirrors the
/// GUI's `face_artifact_evidence`).
#[test]
fn empty_embedding_references_are_missing_not_valid() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir);
    import(&input);
    let path = sidecar_path_for(&input);
    let frame = ImageFrame::decode(&fs::read(&input).unwrap()).unwrap();
    let mut document = load_sidecar(&path).unwrap();
    let mut analysis = build_analysis(&document, &frame, &input);
    analysis.embeddings.clear();
    for detection in &mut analysis.detections {
        detection.embedding_id = None;
    }
    document.face = Some(analysis);
    save_sidecar(&path, &document).unwrap();

    let (ok, stdout) = status_json(&input);
    assert!(ok);
    assert!(stdout.contains("\"status\":\"missing\""));
    assert!(stdout.contains("\"artifact_evidence\":\"missing\""));
}

/// M2: `--analyze` must not treat a persisted `valid` analysis with missing
/// references as already valid; it proceeds (and without `onnx-rt` fails loudly
/// before writing anything).
#[cfg(not(feature = "onnx-rt"))]
#[test]
fn analyze_does_not_short_circuit_when_references_are_missing() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir);
    import(&input);
    let path = sidecar_path_for(&input);
    let frame = ImageFrame::decode(&fs::read(&input).unwrap()).unwrap();
    let mut document = load_sidecar(&path).unwrap();
    document.face = Some(build_analysis(&document, &frame, &input));
    save_sidecar(&path, &document).unwrap();
    fs::remove_file(zdata_path_for(&input)).unwrap();
    let before = fs::read(&path).unwrap();

    let output = cli()
        .args([
            "face",
            "--input",
            input.to_str().unwrap(),
            "--analyze",
            "--detector",
            dir.path().join("missing-detector.onnx").to_str().unwrap(),
            "--embedder",
            dir.path().join("missing-embedder.onnx").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "an unverifiable analysis must not short-circuit as already valid"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("onnx-rt"),
        "the re-run must reach the engine loader: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "a refused analysis must not touch the sidecar"
    );
    assert!(load_sidecar(&path).unwrap().face.is_some());
}

/// Without the `onnx-rt` capability the real engine is unavailable: the command
/// refuses loudly (exit 1) and writes nothing — never a silent stub fallback.
#[cfg(not(feature = "onnx-rt"))]
#[test]
fn analyze_without_onnx_rt_is_loud_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir);
    import(&input);
    let path = sidecar_path_for(&input);
    let before = fs::read(&path).unwrap();

    let output = cli()
        .args([
            "face",
            "--input",
            input.to_str().unwrap(),
            "--analyze",
            "--detector",
            dir.path().join("missing-detector.onnx").to_str().unwrap(),
            "--embedder",
            dir.path().join("missing-embedder.onnx").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("onnx-rt"),
        "stderr must name the missing capability: {stderr}"
    );
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "a refused analysis must not touch the sidecar"
    );
    assert!(load_sidecar(&path).unwrap().face.is_none());
}

/// F2: `--status` and `--analyze` are mutually exclusive flags — a usage error
/// (exit 2), not a runtime failure (exit 1) and never a silent precedence pick.
#[test]
fn status_and_analyze_are_a_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let input = gradient_png(&dir);
    import(&input);
    let output = cli()
        .args([
            "face",
            "--input",
            input.to_str().unwrap(),
            "--status",
            "--analyze",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "mutually exclusive flags must exit 2: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("mutually exclusive"),
        "stderr must explain the conflict: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
