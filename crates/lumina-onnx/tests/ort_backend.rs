//! Integration tests for the real ONNX Runtime backend (`onnx-rt` feature).
//!
//! Every test below needs a **loadable** `.onnx` artifact, but real model
//! weights are neither committed nor downloadable (Agents.md: no spontaneous
//! downloads; fixtures must be reproducible). Instead this file generates the
//! bytes of a tiny, deterministic ONNX graph by hand — raw protobuf, no
//! `onnx` crate dependency:
//!
//! ```text
//! x: float[1,3,H,W] ──ReduceMax(axes=[1], keepdims=1)──▶ y: float[1,1,H,W]
//! ```
//!
//! `ReduceMax` over the channel axis with `keepdims=1` turns the adapter's
//! NCHW RGB input into exactly the matte-shaped output
//! (`[1, 1, H, W]`, see `validate_output_shape`) that `OrtBackend` expects.
//!
//! Covered review findings:
//!
//! * **F-082-FOLLOWUP-ORT** — unknown input/output tensor names surface as
//!   [`lumina_onnx::OnnxError::InferenceFailed`] instead of panicking;
//! * **F-082-FOLLOWUP-HASH** — the `ModelArtifactStale` refuse branch runs
//!   end-to-end against an artifact whose actual digest differs from the
//!   pinned `model_hash`.

#![cfg(feature = "onnx-rt")]

use lumina_core::ImageFrame;
use lumina_onnx::ort_backend::OrtBackend;
use lumina_onnx::{
    compute_sha256_hex, ChannelLayout, InputNormalization, ModelCapabilities, ModelHashStatus,
    ModelInputSpec, ModelManifest, OnnxError, Resolution, SubjectInference, TensorFormat,
    PENDING_INTEGRATION_HASH,
};
use std::fs::File;

/// Inference resolution of the crafted graph (kept tiny: fast ORT session).
const W: u32 = 8;
const H: u32 = 8;
const INPUT_NAME: &str = "x";
const OUTPUT_NAME: &str = "y";

// ---------------------------------------------------------------------------
// Minimal protobuf encoding (proto3 wire format) for the crafted model.
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

/// `Dimension { dim_value }`.
fn dimension(value: i64) -> Vec<u8> {
    let mut out = Vec::new();
    push_varint_field(&mut out, 1, value as u64);
    out
}

/// `TensorShapeProto { dim: [...] }` (field 1, repeated).
fn shape_proto(dims: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    for dim in dims {
        push_len_delimited(&mut out, 1, &dimension(*dim));
    }
    out
}

/// `TypeProto.Tensor { elem_type: FLOAT(=1), shape }`.
fn tensor_type(dims: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    push_varint_field(&mut out, 1, 1); // FLOAT
    push_len_delimited(&mut out, 2, &shape_proto(dims));
    out
}

/// `TypeProto { tensor_type }` (field 1).
fn type_proto(dims: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    push_len_delimited(&mut out, 1, &tensor_type(dims));
    out
}

/// `ValueInfoProto { name: 1, type: 2 }`.
fn value_info(name: &str, dims: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    push_string(&mut out, 1, name);
    push_len_delimited(&mut out, 2, &type_proto(dims));
    out
}

/// `AttributeProto { name: 1, type: 20 = INTS(7), ints: 8 (packed) }`.
fn axes_attribute(axes: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    push_string(&mut out, 1, "axes");
    push_varint_field(&mut out, 20, 7); // AttributeType::INTS
    let mut packed = Vec::new();
    for axis in axes {
        push_varint(&mut packed, *axis as u64);
    }
    push_len_delimited(&mut out, 8, &packed);
    out
}

/// `AttributeProto { name: 1, type: 20 = INT(2), i: 3 = 1 }`.
fn keepdims_attribute() -> Vec<u8> {
    let mut out = Vec::new();
    push_string(&mut out, 1, "keepdims");
    push_varint_field(&mut out, 20, 2); // AttributeType::INT
    push_varint_field(&mut out, 3, 1); // keepdims = true
    out
}

/// `NodeProto { input: 1, output: 2, op_type: 4, attribute: 5 }`.
fn reduce_max_node() -> Vec<u8> {
    let mut out = Vec::new();
    push_string(&mut out, 1, INPUT_NAME);
    push_string(&mut out, 2, OUTPUT_NAME);
    push_string(&mut out, 4, "ReduceMax");
    push_len_delimited(&mut out, 5, &axes_attribute(&[1]));
    push_len_delimited(&mut out, 5, &keepdims_attribute());
    out
}

/// `GraphProto { node: 1, name: 2, input: 11, output: 12 }`.
fn graph_proto() -> Vec<u8> {
    let mut out = Vec::new();
    push_len_delimited(&mut out, 1, &reduce_max_node());
    push_string(&mut out, 2, "lumina-crafted-graph");
    push_len_delimited(
        &mut out,
        11,
        &value_info(INPUT_NAME, &[1, 3, H as i64, W as i64]),
    );
    push_len_delimited(
        &mut out,
        12,
        &value_info(OUTPUT_NAME, &[1, 1, H as i64, W as i64]),
    );
    out
}

/// `ModelProto { ir_version: 1, graph: 7, opset_import: 8 }` — opset 13 keeps
/// `ReduceMax` in its attributes-based form (axes moved to an input only at
/// opset 18).
fn crafted_onnx_bytes() -> Vec<u8> {
    let mut opset = Vec::new();
    // OperatorSetIdProto { domain: "" (default, omitted), version: 13 }.
    push_varint_field(&mut opset, 2, 13);

    let mut out = Vec::new();
    push_varint_field(&mut out, 1, 8); // ir_version 8
    push_len_delimited(&mut out, 7, &graph_proto());
    push_len_delimited(&mut out, 8, &opset);
    out
}

// ---------------------------------------------------------------------------
// Fixtures / helpers
// ---------------------------------------------------------------------------

/// Write the crafted graph to a unique temp `.onnx` file and return its path.
fn write_crafted_artifact(tag: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "lumina-onnx-crafted-{tag}-{}-{}.onnx",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&path, crafted_onnx_bytes()).expect("write crafted onnx artifact");
    path
}

fn artifact_digest(path: &std::path::Path) -> String {
    compute_sha256_hex(File::open(path).expect("artifact exists")).expect("digest")
}

/// Manifest for the crafted graph; tensor names are parameterized so tests
/// can declare intentionally wrong names against the same loadable artifact.
fn crafted_manifest(model_hash: String, input_tensor: &str, output_tensor: &str) -> ModelManifest {
    ModelManifest {
        model_name: "CraftedReduceMax".into(),
        model_version: "0.0.1".into(),
        model_hash,
        license: "MIT".into(),
        input: ModelInputSpec {
            resolution: Resolution {
                width: W,
                height: H,
            },
            channel_layout: ChannelLayout::Rgb,
            tensor_name: input_tensor.into(),
            tensor_format: TensorFormat::Nchw,
            normalization: InputNormalization::IMAGENET,
        },
        output_tensor_name: output_tensor.into(),
        capabilities: ModelCapabilities {
            subject_segmentation: true,
            ..Default::default()
        },
    }
}

fn solid_frame(width: u32, height: u32, rgb: [u8; 3]) -> ImageFrame {
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for p in pixels.as_chunks_mut::<4>().0 {
        p[0] = rgb[0];
        p[1] = rgb[1];
        p[2] = rgb[2];
        p[3] = 255;
    }
    ImageFrame::new(width, height, pixels).unwrap()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// The crafted artifact loads under a matching pin and infers a matte at the
/// source resolution — proving the fixture is genuinely loadable/runnable so
/// the negative tests below exercise real code paths.
#[test]
fn crafted_graph_loads_verified_and_infers_matte() {
    let path = write_crafted_artifact("verified");
    let digest = artifact_digest(&path);
    let backend = OrtBackend::new(
        &path,
        crafted_manifest(digest.clone(), INPUT_NAME, OUTPUT_NAME),
    )
    .expect("crafted graph must load");
    assert_eq!(backend.hash_status(), &ModelHashStatus::Verified);

    let img = solid_frame(4, 4, [128, 128, 128]);
    let matte = backend.infer(&img).expect("inference must succeed");
    assert_eq!((matte.width, matte.height), (4, 4));
    assert_eq!(matte.values.len(), 16);

    // Solid frame → nearest-neighbor resize stays solid → ReduceMax over the
    // three normalized channels is constant → uniform plane with the value
    // derived from the manifest normalization, clamped to [0, 1] × 65535.
    let norm = InputNormalization::IMAGENET;
    let peak = (0..3)
        .map(|c| (128f32 / 255.0 - norm.mean[c]) / norm.std[c])
        .fold(f32::MIN, f32::max);
    let expected = ((peak.clamp(0.0, 1.0) * 65535.0).round() as i32).clamp(0, 65535) as u16;
    assert!(
        matte.values.iter().all(|&v| v == expected),
        "uniform frame must yield the uniform matte value {expected}, got {:?}",
        &matte.values[..4]
    );
    let _ = std::fs::remove_file(&path);
}

/// The documented pre-integration placeholder stays inferable (`Pending` is
/// not refused) — the gate refuses only genuine mismatches.
#[test]
fn pending_placeholder_hash_still_infers() {
    let path = write_crafted_artifact("pending");
    let backend = OrtBackend::new(
        &path,
        crafted_manifest(PENDING_INTEGRATION_HASH.to_owned(), INPUT_NAME, OUTPUT_NAME),
    )
    .expect("pending placeholder must load");
    assert_eq!(backend.hash_status(), &ModelHashStatus::Pending);
    let img = solid_frame(8, 8, [200, 10, 10]);
    assert!(backend.infer(&img).is_ok(), "Pending allows inference");
    let _ = std::fs::remove_file(&path);
}

/// F-082-FOLLOWUP-ORT — a manifest naming an output the graph does not have
/// fails visibly with `InferenceFailed` at load time (naming requested and
/// available outputs); it must never panic.
#[test]
fn unknown_output_name_fails_with_inference_failed_not_panic() {
    let path = write_crafted_artifact("bad-output");
    let digest = artifact_digest(&path);
    let manifest = crafted_manifest(digest, INPUT_NAME, "does-not-exist");
    let result = OrtBackend::new(&path, manifest);
    let _ = std::fs::remove_file(&path);
    match result {
        Err(OnnxError::InferenceFailed { name, reason }) => {
            assert_eq!(name, "CraftedReduceMax");
            assert!(reason.contains("`does-not-exist`"), "{reason}");
            assert!(
                reason.contains(format!("`{OUTPUT_NAME}`").as_str()),
                "{reason}"
            );
        }
        Err(other) => panic!("expected InferenceFailed, got {other:?}"),
        Ok(_) => panic!("a manifest with a wrong output name must not load"),
    }
}

/// F-082-FOLLOWUP-ORT (input side) — a wrong input tensor name fails the same
/// clean way at load time instead of blowing up inside `session.run`.
#[test]
fn unknown_input_name_fails_with_inference_failed_not_panic() {
    let path = write_crafted_artifact("bad-input");
    let digest = artifact_digest(&path);
    let manifest = crafted_manifest(digest, "not-an-input", OUTPUT_NAME);
    let result = OrtBackend::new(&path, manifest);
    let _ = std::fs::remove_file(&path);
    match result {
        Err(OnnxError::InferenceFailed { name, reason }) => {
            assert_eq!(name, "CraftedReduceMax");
            assert!(reason.contains("`not-an-input`"), "{reason}");
            assert!(
                reason.contains(format!("`{INPUT_NAME}`").as_str()),
                "{reason}"
            );
        }
        Err(other) => panic!("expected InferenceFailed, got {other:?}"),
        Ok(_) => panic!("a manifest with a wrong input name must not load"),
    }
}

/// F-082-FOLLOWUP-HASH — the refuse branch, executed end-to-end: loading an
/// artifact whose actual SHA-256 differs from the pinned `model_hash`
/// succeeds (visible via `hash_status`), but every inference returns
/// `ModelArtifactStale` carrying both digests — never a silent matte.
#[test]
fn mismatched_pinned_hash_refuses_inference_end_to_end() {
    let path = write_crafted_artifact("stale");
    let actual_digest = artifact_digest(&path);
    // A well-formed but wrong pin (≠ the artifact's true digest).
    let pinned = format!("{:064x}", 0xcafebabe_u64);
    assert_ne!(pinned, actual_digest);

    let backend = OrtBackend::new(
        &path,
        crafted_manifest(pinned.clone(), INPUT_NAME, OUTPUT_NAME),
    )
    .expect("a hash mismatch must not prevent loading (status is queryable)");
    match backend.hash_status() {
        ModelHashStatus::Mismatch { expected, actual } => {
            assert_eq!(expected, &pinned);
            assert_eq!(actual, &actual_digest);
        }
        other => panic!("expected Mismatch status, got {other:?}"),
    }

    let img = solid_frame(4, 4, [10, 20, 30]);
    match backend.infer(&img) {
        Err(OnnxError::ModelArtifactStale {
            name,
            expected,
            actual,
        }) => {
            assert_eq!(name, "CraftedReduceMax");
            assert_eq!(expected, pinned);
            assert_eq!(actual, actual_digest);
        }
        other => panic!("expected ModelArtifactStale, got {other:?}"),
    }
    let _ = std::fs::remove_file(&path);
}
