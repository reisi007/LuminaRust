//! LRPAR-G12-FACE-ADAPTER-25 integration tests for the real ONNX Runtime face
//! adapter (`onnx-rt` feature).
//!
//! The shipped face weights are neither committed nor downloadable (Agents.md:
//! no spontaneous downloads), so — exactly like `ort_backend.rs` /
//! `face_ort.rs` — these tests drive the adapter with **crafted, hash-pinned
//! behavior fixtures**:
//!
//! * `lumina-crafted-yunet.onnx` — input `input` `[1,3,32,32]` with the twelve
//!   per-stride YuNet outputs `cls_/obj_/bbox_/kps_{8,16,32}` produced by
//!   `Constant` nodes. Two anchors encode an identical box, so the adapter must
//!   suppress one by NMS — a value-level proof that the real decode path runs.
//! * `lumina-crafted-sface.onnx` — input `data` `[1,3,112,112]` and the 128-d
//!   `fc1` output. The pipeline runs the 5-point alignment and the
//!   `BYTE_RANGE` preprocessing and decodes/units-normalizes `fc1`.
//!
//! The fixtures contain no weights; the encoder here is the source of truth and
//! `pinned_fixture_matches_encoder_source_of_truth` fails on any drift. Run
//! `cargo test -p lumina-onnx --features onnx-rt regenerate_face_fixtures --
//! --ignored` to (re)write them after a deliberate encoder change.

#![cfg(feature = "onnx-rt")]

use lumina_core::ImageFrame;
use lumina_onnx::face::ort::{OrtFaceDetector, OrtFaceEmbedder};
use lumina_onnx::{
    compute_sha256_hex, face_detect_manifest, face_embed_manifest, ChannelLayout, DetectedFace,
    FaceDetectionInference, FaceEmbeddingInference, FaceInferenceOptions, ModelHashStatus,
    OnnxError, Resolution, FACE_EMBED_DIMENSION, YUNET_OUTPUT_NAMES,
};
use lumina_sidecar::{FaceBoundingBox, FaceLandmark};
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// Compile-time pins of the committed fixtures (see `tests/fixtures/README.md`).
const YUNET_PIN: &str = "b4c76993b06bcccf1a0495fa795b2de8be8263c448b83f62630d4227da8324b1";
const SFACE_PIN: &str = "d2919decc20b9fe62e0d99e544fd0243fda0f847f3b99f32500491f6d6a5738e";
const YUNET_BYTES: &[u8] = include_bytes!("fixtures/lumina-crafted-yunet.onnx");
const SFACE_BYTES: &[u8] = include_bytes!("fixtures/lumina-crafted-sface.onnx");

const DETECT_INPUT: &str = "input";
const DETECT_SIZE: u32 = 32;
const EMBED_INPUT: &str = "data";
const EMBED_SIZE: u32 = 112;

// ---------------------------------------------------------------------------
// Minimal protobuf encoding (proto3 wire format), mirroring `ort_backend.rs`.
// ---------------------------------------------------------------------------

fn push_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

fn push_tag(out: &mut Vec<u8>, field: u32, wire_type: u64) {
    push_varint(out, ((field as u64) << 3) | wire_type);
}

fn push_len_delimited(out: &mut Vec<u8>, field: u32, payload: &[u8]) {
    push_tag(out, field, 2);
    push_varint(out, payload.len() as u64);
    out.extend_from_slice(payload);
}

fn push_string(out: &mut Vec<u8>, field: u32, value: &str) {
    push_len_delimited(out, field, value.as_bytes());
}

fn push_varint_field(out: &mut Vec<u8>, field: u32, value: u64) {
    push_tag(out, field, 0);
    push_varint(out, value);
}

fn dimension(value: i64) -> Vec<u8> {
    let mut out = Vec::new();
    push_varint_field(&mut out, 1, value as u64);
    out
}

fn shape_proto(dims: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    for dim in dims {
        push_len_delimited(&mut out, 1, &dimension(*dim));
    }
    out
}

fn tensor_type(dims: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    push_varint_field(&mut out, 1, 1); // FLOAT
    push_len_delimited(&mut out, 2, &shape_proto(dims));
    out
}

fn type_proto(dims: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    push_len_delimited(&mut out, 1, &tensor_type(dims));
    out
}

fn value_info(name: &str, dims: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    push_string(&mut out, 1, name);
    push_len_delimited(&mut out, 2, &type_proto(dims));
    out
}

/// `TensorProto { dims: 1 (packed), data_type: 2 = FLOAT, name: 8, raw_data: 9 }`.
fn tensor_proto(name: &str, dims: &[i64], values: &[f32]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut packed = Vec::new();
    for dim in dims {
        push_varint(&mut packed, *dim as u64);
    }
    push_len_delimited(&mut out, 1, &packed);
    push_varint_field(&mut out, 2, 1); // FLOAT
    push_string(&mut out, 8, name);
    let mut raw = Vec::with_capacity(values.len() * 4);
    for value in values {
        raw.extend_from_slice(&value.to_le_bytes());
    }
    push_len_delimited(&mut out, 9, &raw);
    out
}

/// `NodeProto { output: 2, name: 3, op_type: 4, attribute: 5 }` for a
/// `Constant` node carrying a single float tensor.
fn constant_node(name: &str, dims: &[i64], values: &[f32]) -> Vec<u8> {
    let mut attribute = Vec::new();
    push_string(&mut attribute, 1, "value");
    push_varint_field(&mut attribute, 20, 4); // AttributeType::TENSOR
    push_len_delimited(&mut attribute, 5, &tensor_proto(name, dims, values));

    let mut out = Vec::new();
    push_string(&mut out, 2, name);
    push_string(&mut out, 3, name);
    push_string(&mut out, 4, "Constant");
    push_len_delimited(&mut out, 5, &attribute);
    out
}

fn model_bytes(graph: Vec<u8>) -> Vec<u8> {
    let mut opset = Vec::new();
    push_varint_field(&mut opset, 2, 13);
    let mut out = Vec::new();
    push_varint_field(&mut out, 1, 8); // ir_version
    push_len_delimited(&mut out, 7, &graph);
    push_len_delimited(&mut out, 8, &opset);
    out
}

const YUNET_DIMS: [&[i64]; 12] = [
    &[1, 16, 1],
    &[1, 4, 1],
    &[1, 1, 1],
    &[1, 16, 1],
    &[1, 4, 1],
    &[1, 1, 1],
    &[1, 16, 4],
    &[1, 4, 4],
    &[1, 1, 4],
    &[1, 16, 10],
    &[1, 4, 10],
    &[1, 1, 10],
];

/// Deterministic YuNet-shaped payloads: two duplicate anchors at stride 8
/// (index 0/1) with a high score; everything else is below the threshold.
fn yunet_payloads() -> Vec<Vec<f32>> {
    let mut cls_8 = vec![0.0f32; 16];
    cls_8[0] = 0.9;
    cls_8[1] = 0.9;
    let mut obj_8 = vec![0.0f32; 16];
    obj_8[0] = 1.0;
    obj_8[1] = 1.0;
    let mut bbox_8 = vec![0.0f32; 16 * 4];
    bbox_8[0..4].copy_from_slice(&[0.5, 0.5, 0.0, 0.0]);
    bbox_8[4..8].copy_from_slice(&[-0.5, 0.5, 0.0, 0.0]);
    vec![
        cls_8,
        vec![0.0; 4],
        vec![0.0; 1],
        obj_8,
        vec![0.0; 4],
        vec![0.0; 1],
        bbox_8,
        vec![0.0; 4 * 4],
        vec![0.0; 4],
        vec![0.0; 16 * 10],
        vec![0.0; 4 * 10],
        vec![0.0; 10],
    ]
}

/// The twelve-output YuNet-shaped graph. `omit` drops one output (used by the
/// negative test) while keeping the committed fixture complete.
fn yunet_graph_bytes(omit: Option<&str>) -> Vec<u8> {
    let payloads = yunet_payloads();
    let mut graph = Vec::new();
    for (index, name) in YUNET_OUTPUT_NAMES.iter().enumerate() {
        if omit == Some(*name) {
            continue;
        }
        push_len_delimited(
            &mut graph,
            1,
            &constant_node(name, YUNET_DIMS[index], &payloads[index]),
        );
    }
    push_string(&mut graph, 2, "lumina-crafted-yunet-graph");
    push_len_delimited(
        &mut graph,
        11,
        &value_info(
            DETECT_INPUT,
            &[1, 3, DETECT_SIZE as i64, DETECT_SIZE as i64],
        ),
    );
    for (index, name) in YUNET_OUTPUT_NAMES.iter().enumerate() {
        if omit == Some(*name) {
            continue;
        }
        push_len_delimited(&mut graph, 12, &value_info(name, YUNET_DIMS[index]));
    }
    model_bytes(graph)
}

/// The one-output SFace-shaped graph: `data` → `fc1` `[1,128]`.
fn sface_graph_bytes() -> Vec<u8> {
    let values = vec![1.0f32; FACE_EMBED_DIMENSION as usize];
    let mut graph = Vec::new();
    push_len_delimited(
        &mut graph,
        1,
        &constant_node("fc1", &[1, FACE_EMBED_DIMENSION as i64], &values),
    );
    push_string(&mut graph, 2, "lumina-crafted-sface-graph");
    push_len_delimited(
        &mut graph,
        11,
        &value_info(EMBED_INPUT, &[1, 3, EMBED_SIZE as i64, EMBED_SIZE as i64]),
    );
    push_len_delimited(
        &mut graph,
        12,
        &value_info("fc1", &[1, FACE_EMBED_DIMENSION as i64]),
    );
    model_bytes(graph)
}

// ---------------------------------------------------------------------------
// Fixtures / helpers
// ---------------------------------------------------------------------------

fn temp_fixture(tag: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "lumina-onnx-face-adapter-{tag}-{}-{}.onnx",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&path, bytes).expect("write crafted face fixture");
    path
}

fn frame(width: u32, height: u32) -> ImageFrame {
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for (index, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        px[0] = (index % 256) as u8;
        px[1] = ((index * 5) % 256) as u8;
        px[2] = ((index * 11) % 256) as u8;
        px[3] = 255;
    }
    ImageFrame::new(width, height, pixels).unwrap()
}

fn detection() -> DetectedFace {
    let landmark = |name: &str, x: f32, y: f32| FaceLandmark {
        name: name.into(),
        x,
        y,
    };
    DetectedFace {
        bbox: FaceBoundingBox {
            x: 0.25,
            y: 0.25,
            width: 0.5,
            height: 0.5,
        },
        score: 0.9,
        landmarks: vec![
            landmark("left_eye", 0.35, 0.40),
            landmark("right_eye", 0.65, 0.40),
            landmark("nose", 0.50, 0.50),
            landmark("mouth_left", 0.40, 0.65),
            landmark("mouth_right", 0.60, 0.65),
        ],
    }
}

/// A YuNet manifest matching the crafted fixture geometry.
fn yunet_manifest(model_hash: &str, size: u32, output: &str) -> lumina_onnx::ModelManifest {
    let mut manifest = face_detect_manifest();
    manifest.model_hash = model_hash.into();
    manifest.input.resolution = Resolution {
        width: size,
        height: size,
    };
    manifest.output_tensor_name = output.into();
    manifest
}

fn sface_manifest(model_hash: &str) -> lumina_onnx::ModelManifest {
    let mut manifest = face_embed_manifest();
    manifest.model_hash = model_hash.into();
    manifest
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn fixture_pins_match_the_documented_values() {
    let yunet = compute_sha256_hex(Cursor::new(YUNET_BYTES)).unwrap();
    let sface = compute_sha256_hex(Cursor::new(SFACE_BYTES)).unwrap();
    assert_eq!(yunet, YUNET_PIN, "YuNet fixture drifted from its pin");
    assert_eq!(sface, SFACE_PIN, "SFace fixture drifted from its pin");
}

#[test]
fn pinned_fixtures_match_the_encoder_source_of_truth() {
    assert_eq!(YUNET_BYTES, yunet_graph_bytes(None).as_slice());
    assert_eq!(SFACE_BYTES, sface_graph_bytes().as_slice());
}

/// (Re)write the committed fixtures after a deliberate encoder change. Run
/// with `--ignored`.
#[test]
#[ignore = "fixture regeneration; run explicitly after an encoder change"]
fn regenerate_face_fixtures() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let yunet = dir.join("lumina-crafted-yunet.onnx");
    let sface = dir.join("lumina-crafted-sface.onnx");
    std::fs::write(&yunet, yunet_graph_bytes(None)).unwrap();
    std::fs::write(&sface, sface_graph_bytes()).unwrap();
    println!("wrote {}", yunet.display());
    println!("wrote {}", sface.display());
}

/// The crafted YuNet fixture decodes through the real adapter: the two
/// duplicate anchors collapse to one detection by NMS, with the documented box.
#[test]
fn yunet_adapter_decodes_and_suppresses_duplicates() {
    let path = temp_fixture("yunet-decode", YUNET_BYTES);
    let backend = OrtFaceDetector::new(
        &path,
        yunet_manifest(YUNET_PIN, DETECT_SIZE, "cls_8"),
        FaceInferenceOptions::default(),
    )
    .expect("the pinned YuNet-shaped fixture must load");
    assert_eq!(backend.hash_status(), &ModelHashStatus::Verified);
    let detections = backend
        .detect(&frame(DETECT_SIZE, DETECT_SIZE))
        .expect("the adapter must decode the crafted YuNet outputs");
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        detections.len(),
        1,
        "two identical anchors must collapse to one detection via NMS"
    );
    let bbox = detections[0].bbox;
    assert!((bbox.x - 0.0).abs() < 1e-6, "{bbox:?}");
    assert!((bbox.y - 0.0).abs() < 1e-6, "{bbox:?}");
    assert!((bbox.width - 0.25).abs() < 1e-6, "{bbox:?}");
    assert!((bbox.height - 0.25).abs() < 1e-6, "{bbox:?}");
    assert!((detections[0].score - 0.948_683_3).abs() < 1e-4);
}

/// A graph missing one of the twelve declared tensors is refused loudly at
/// load — never a partial decode.
#[test]
fn yunet_adapter_refuses_a_missing_stride_tensor() {
    let bytes = yunet_graph_bytes(Some("kps_32"));
    let path = temp_fixture("yunet-missing", &bytes);
    let result = OrtFaceDetector::new(
        &path,
        yunet_manifest(YUNET_PIN, DETECT_SIZE, "cls_8"),
        FaceInferenceOptions::default(),
    );
    let _ = std::fs::remove_file(&path);
    match result {
        Err(OnnxError::InferenceFailed { reason, .. }) => {
            assert!(reason.contains("`kps_32`"), "{reason}");
        }
        Err(other) => panic!("expected InferenceFailed, got {other:?}"),
        Ok(_) => panic!("a graph missing `kps_32` must not load"),
    }
}

/// A manifest that does not carry the YuNet identity decodes the canonical
/// single-output contract, so the same multi-output graph fails loudly on the
/// missing `detections` tensor — the two contracts never silently mix.
#[test]
fn canonical_manifest_does_not_silently_decode_the_yunet_graph() {
    let path = temp_fixture("yunet-canonical", YUNET_BYTES);
    let mut manifest = yunet_manifest(YUNET_PIN, DETECT_SIZE, "detections");
    manifest.model_name = "SCRFD".into();
    manifest.input.channel_layout = ChannelLayout::Rgb;
    let result = OrtFaceDetector::new(&path, manifest, FaceInferenceOptions::default());
    let _ = std::fs::remove_file(&path);
    match result {
        Err(OnnxError::InferenceFailed { reason, .. }) => {
            assert!(reason.contains("`detections`"), "{reason}");
        }
        Err(other) => panic!("expected InferenceFailed, got {other:?}"),
        Ok(_) => panic!("the canonical contract must not decode the YuNet graph"),
    }
}

/// The crafted SFace fixture drives the real embedder: 5-point alignment +
/// `BYTE_RANGE` preprocessing + `fc1` decode (+ L2 normalization).
#[test]
fn sface_adapter_runs_alignment_and_decodes_fc1() {
    let path = temp_fixture("sface-decode", SFACE_BYTES);
    let backend = OrtFaceEmbedder::new(&path, sface_manifest(SFACE_PIN), FACE_EMBED_DIMENSION)
        .expect("the pinned SFace-shaped fixture must load");
    assert_eq!(backend.hash_status(), &ModelHashStatus::Verified);
    let embeddings = backend
        .embed(&frame(64, 64), &[detection()])
        .expect("the adapter must decode the crafted fc1 output");
    let _ = std::fs::remove_file(&path);

    assert_eq!(embeddings.len(), 1);
    assert_eq!(embeddings[0].dimension(), FACE_EMBED_DIMENSION as usize);
    assert!(
        embeddings[0].is_normalized(),
        "the decoded embedding must be L2-normalized"
    );
}

/// An SFace manifest pointed at the YuNet graph fails on the `data` input —
/// never a silent wrong-tensor load.
#[test]
fn sface_adapter_refuses_the_wrong_input_tensor() {
    let path = temp_fixture("sface-wrong-input", YUNET_BYTES);
    let result = OrtFaceEmbedder::new(&path, sface_manifest(SFACE_PIN), FACE_EMBED_DIMENSION);
    let _ = std::fs::remove_file(&path);
    match result {
        Err(OnnxError::InferenceFailed { reason, .. }) => {
            assert!(reason.contains("`data`"), "{reason}");
            assert!(reason.contains("`input`"), "{reason}");
        }
        Err(other) => panic!("expected InferenceFailed, got {other:?}"),
        Ok(_) => panic!("the wrong input tensor must not load"),
    }
}
