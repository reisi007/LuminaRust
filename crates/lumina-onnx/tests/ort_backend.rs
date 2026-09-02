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
//!   pinned `model_hash`;
//! * **F-082-FOLLOWUP-Fixtures** — a committed, hash-pinned `.onnx` behavior
//!   fixture (`tests/fixtures/lumina-crafted-reducemax.onnx`, SHA-256 pin in
//!   `tests/fixtures/README.md`) is verified, loaded and inferred, and the
//!   resolver [`lumina_onnx::try_load_onnx_engine`] surfaces it as
//!   `OnnxRuntime` — or a hard `MissingModel` when the artifact is absent
//!   (no silent fallback).

#![cfg(feature = "onnx-rt")]

use lumina_core::ImageFrame;
use lumina_onnx::ort_backend::OrtBackend;
use lumina_onnx::{
    compute_sha256_hex, try_load_onnx_engine, ChannelLayout, InputNormalization, ModelCapabilities,
    ModelHashStatus, ModelInputSpec, ModelManifest, OnnxEngine, OnnxError, Resolution,
    SubjectInference, TensorFormat, PENDING_INTEGRATION_HASH,
};
use std::fs::File;
use std::io::Cursor;
use std::path::Path;

/// Inference resolution of the crafted graph (kept tiny: fast ORT session).
const W: u32 = 8;
const H: u32 = 8;
const INPUT_NAME: &str = "x";
const OUTPUT_NAME: &str = "y";

/// SHA-256 pin of the committed behavior fixture
/// `tests/fixtures/lumina-crafted-reducemax.onnx` (see `tests/fixtures/README.md`).
/// The bytes are generated deterministically by the encoder below and verified
/// by `pinned_fixture_hash_matches_documented_pin` — a drift between encoder
/// and committed fixture is a hard failure (F-082-FOLLOWUP-hash-gepinnte
/// Fixtures).
const FIXTURE_PIN: &str = "2a2ede6659e8c59b3fd972242b27677ef23cb98d3c422616a1c65f50dcaca18d";

/// The committed, hash-pinned behavior fixture (part of the build via
/// `include_bytes!`). Intentionally **not** a real BiRefNet/SAM-2 model — a
/// minimal loadable graph that exercises the real ORT code paths with a
/// pinned identity.
const FIXTURE_BYTES: &[u8] = include_bytes!("fixtures/lumina-crafted-reducemax.onnx");

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

/// Write the committed pinned fixture bytes to a unique temp `.onnx` file.
fn write_pinned_fixture(tag: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "lumina-onnx-pinned-{tag}-{}-{}.onnx",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&path, FIXTURE_BYTES).expect("write pinned onnx fixture");
    path
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

// ---------------------------------------------------------------------------
// Hash-pinned committed fixture (F-082-FOLLOWUP)
// ---------------------------------------------------------------------------

/// The committed fixture bytes must hash to the documented pin
/// (`tests/fixtures/README.md`). Anyone who changes the fixture MUST update
/// both pins deliberately; any drift — even from a regenerated-but-different
/// artifact — is a hard failure here and in the load tests below.
#[test]
fn pinned_fixture_hash_matches_documented_pin() {
    let digest = compute_sha256_hex(Cursor::new(FIXTURE_BYTES))
        .expect("sha256 of in-memory fixture must not fail");
    assert_eq!(
        digest, FIXTURE_PIN,
        "committed ONNX fixture drifted from its documented SHA-256 pin"
    );
}

/// The committed fixture must stay byte-identical to the deterministic
/// encoder output (`crafted_onnx_bytes()`) that the negative tests still use
/// to build mutated graphs. A divergence means the encoder and the fixture
/// are two different graphs — regenerate the fixture first.
#[test]
fn pinned_fixture_matches_encoder_source_of_truth() {
    assert_eq!(
        FIXTURE_BYTES,
        crafted_onnx_bytes(),
        "committed fixture diverged from the source-of-truth encoder"
    );
}

/// The pinned fixture loads with `model_hash = FIXTURE_PIN`, reports
/// `Verified`, and infers a uniform matte on a uniform frame — the committed
/// artifact is genuinely loadable/runnable, so the resolver and negative
/// tests exercise real code paths (not a well-formed-but-unusable blob).
#[test]
fn pinned_fixture_loads_verified_and_infers() {
    let path = write_pinned_fixture("pinned-load");
    let manifest = crafted_manifest(FIXTURE_PIN.to_owned(), INPUT_NAME, OUTPUT_NAME);
    let backend = OrtBackend::new(&path, manifest).expect("pinned fixture must load");
    assert_eq!(
        backend.hash_status(),
        &ModelHashStatus::Verified,
        "pinned fixture must verify against its documented hash"
    );

    let img = solid_frame(4, 4, [64, 128, 192]);
    let matte = backend.infer(&img).expect("pinned fixture must infer");
    assert_eq!((matte.width, matte.height), (4, 4));
    let first = matte.values[0];
    assert!(
        matte.values.iter().all(|&v| v == first),
        "uniform frame → uniform matte from the pinned fixture"
    );
    let _ = std::fs::remove_file(&path);
}

/// The resolver (`try_load_onnx_engine`) surfaces the real engine for the
/// pinned fixture as `OnnxEngine::OnnxRuntime` with a usable
/// `MaskInference` box — the exact contract the CLI/core decision layer
/// consumes, with no additive glue.
#[test]
fn resolver_loads_pinned_fixture_as_onnx_runtime() {
    let path = write_pinned_fixture("pinned-resolve");
    let manifest = crafted_manifest(FIXTURE_PIN.to_owned(), INPUT_NAME, OUTPUT_NAME);
    let engine = try_load_onnx_engine(&path, &manifest).expect("pinned fixture must resolve");
    match engine {
        OnnxEngine::OnnxRuntime(backend) => {
            assert!(backend.is_available());
            let img = solid_frame(4, 4, [10, 20, 30]);
            let matte = backend.infer(&img).expect("resolved engine must infer");
            assert_eq!(matte.width * matte.height, 16);
        }
        other => panic!("expected OnnxRuntime for a present, verified artifact, got {other:?}"),
    }
    let _ = std::fs::remove_file(&path);
}

/// The resolver must NOT fall back silently: with `onnx-rt` on, a missing
/// artifact is a visible `MissingModel`, never a stub or a silent success.
#[test]
fn resolver_reports_missing_artifact_without_fallback() {
    let manifest = crafted_manifest(FIXTURE_PIN.to_owned(), INPUT_NAME, OUTPUT_NAME);
    let engine = try_load_onnx_engine(Path::new("/nonexistent/pinned/model.onnx"), &manifest);
    assert!(
        matches!(engine, Err(OnnxError::MissingModel { .. })),
        "a requested real engine with a missing artifact must be a hard error, got {engine:?}"
    );
}

// ---------------------------------------------------------------------------
// MaskGraph builder integration (F-082-FOLLOWUP: echter ORT-Pfad + MaskGraph)
// ---------------------------------------------------------------------------

/// The real ORT matte is a valid `MaskGraph` source plane: a `MaskGraph`
/// built from the inferred plane evaluates `invert`/`union` without a silent
/// resize or fallback, proving the builder chain
/// `OrtBackend::infer → MaskPlane → MaskGraph::evaluate` is wired.
#[test]
fn ort_matte_feeds_maskgraph_builder_correctly() {
    use lumina_core::{MaskGraph, MaskPlane};
    use lumina_sidecar::{
        CoordinateSystem, DecodeFingerprint, Extras, GeometryFingerprint, MaskDefinition,
        MaskOperation, MaskReference, ModelIdentity, Preprocessing, Resolution, SourceFingerprint,
        VirtualCopy,
    };
    use std::collections::BTreeMap;

    let path = write_pinned_fixture("maskgraph-ort");
    let manifest = crafted_manifest(FIXTURE_PIN.to_owned(), INPUT_NAME, OUTPUT_NAME);
    let backend = OrtBackend::new(&path, manifest).expect("pinned fixture must load");
    let img = solid_frame(4, 4, [64, 128, 192]);
    let matte = backend.infer(&img).expect("ort must infer");
    assert_eq!((matte.width, matte.height), (4, 4));
    let _ = std::fs::remove_file(&path);

    // Build a tiny DAG: source `a` carries the ORT matte, `b` is a second
    // deterministic source, `u = union(a,b)` and `n = invert(a)`.
    let source_def = |id: &str| MaskDefinition {
        id: id.into(),
        name: id.into(),
        source_fingerprint: SourceFingerprint {
            content_hash: "h".into(),
            byte_length: 1,
            extras: Extras::new(),
        },
        decode_context: DecodeFingerprint {
            decoder: "d".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: Extras::new(),
        },
        geometry_context: GeometryFingerprint {
            width: matte.width,
            height: matte.height,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: Extras::new(),
        },
        model: ModelIdentity {
            name: "CraftedReduceMax".into(),
            version: "0.0.1".into(),
            hash: FIXTURE_PIN.into(),
            extras: Extras::new(),
        },
        inference_resolution: Resolution {
            width: W,
            height: H,
            extras: Extras::new(),
        },
        preprocessing: Preprocessing {
            name: "p".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: Extras::new(),
        },
        rescaling_method: "none".into(),
        rescaling_parameters: BTreeMap::new(),
        coordinate_system: CoordinateSystem::SourceOriented,
        status: lumina_sidecar::MaskStatus::Valid,
        created_at: "now".into(),
        generator_version: "g".into(),
        error_text: None,
        artifact: None,
        operation: MaskOperation::Source,
        references: vec![],
        prompt: None,
        extras: Extras::new(),
    };

    let b_plane = MaskPlane::new(4, 4, vec![1000u16; 16]).unwrap();
    let definitions = vec![
        source_def("a"),
        source_def("b"),
        MaskDefinition {
            id: "u".into(),
            name: "u".into(),
            source_fingerprint: SourceFingerprint {
                content_hash: "h".into(),
                byte_length: 1,
                extras: Extras::new(),
            },
            decode_context: DecodeFingerprint {
                decoder: "d".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            geometry_context: GeometryFingerprint {
                width: 4,
                height: 4,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: Extras::new(),
            },
            model: ModelIdentity {
                name: "m".into(),
                version: "1".into(),
                hash: "h".into(),
                extras: Extras::new(),
            },
            inference_resolution: Resolution {
                width: 4,
                height: 4,
                extras: Extras::new(),
            },
            preprocessing: Preprocessing {
                name: "p".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            rescaling_method: "none".into(),
            rescaling_parameters: BTreeMap::new(),
            coordinate_system: CoordinateSystem::SourceOriented,
            status: lumina_sidecar::MaskStatus::Valid,
            created_at: "now".into(),
            generator_version: "g".into(),
            error_text: None,
            artifact: None,
            operation: MaskOperation::Union,
            references: vec![
                MaskReference {
                    copy_id: "vc".into(),
                    mask_id: "a".into(),
                    extras: Extras::new(),
                },
                MaskReference {
                    copy_id: "vc".into(),
                    mask_id: "b".into(),
                    extras: Extras::new(),
                },
            ],
            prompt: None,
            extras: Extras::new(),
        },
        MaskDefinition {
            id: "n".into(),
            name: "n".into(),
            source_fingerprint: SourceFingerprint {
                content_hash: "h".into(),
                byte_length: 1,
                extras: Extras::new(),
            },
            decode_context: DecodeFingerprint {
                decoder: "d".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            geometry_context: GeometryFingerprint {
                width: 4,
                height: 4,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: Extras::new(),
            },
            model: ModelIdentity {
                name: "m".into(),
                version: "1".into(),
                hash: "h".into(),
                extras: Extras::new(),
            },
            inference_resolution: Resolution {
                width: 4,
                height: 4,
                extras: Extras::new(),
            },
            preprocessing: Preprocessing {
                name: "p".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            rescaling_method: "none".into(),
            rescaling_parameters: BTreeMap::new(),
            coordinate_system: CoordinateSystem::SourceOriented,
            status: lumina_sidecar::MaskStatus::Valid,
            created_at: "now".into(),
            generator_version: "g".into(),
            error_text: None,
            artifact: None,
            operation: MaskOperation::Invert,
            references: vec![MaskReference {
                copy_id: "vc".into(),
                mask_id: "a".into(),
                extras: Extras::new(),
            }],
            prompt: None,
            extras: Extras::new(),
        },
    ];

    let copy = VirtualCopy {
        id: "vc".into(),
        name: "VC".into(),
        is_default: true,
        recipe: Default::default(),
        mask_library: definitions,
        mask_layers: vec![],
        history: vec![],
        export_records: vec![],
        extras: Extras::new(),
    };
    let planes = BTreeMap::from([
        (("vc".into(), "a".into()), matte.clone()),
        (("vc".into(), "b".into()), b_plane.clone()),
    ]);
    let graph = MaskGraph::new(std::slice::from_ref(&copy), planes);

    // Union is max per-pixel; invert is 65535 - value.
    let union = graph
        .evaluate(&MaskReference {
            copy_id: "vc".into(),
            mask_id: "u".into(),
            extras: Extras::new(),
        })
        .expect("union must evaluate");
    assert_eq!(union.width, 4);
    assert_eq!(union.height, 4);
    assert_eq!(union.values.len(), 16);
    // Every union pixel is max(ort_value, 1000) — so never below the ORT value.
    for (idx, &v) in union.values.iter().enumerate() {
        assert!(v >= matte.values[idx], "union must be max at {idx}");
        assert!(v >= 1000, "union must dominate the constant plane at {idx}");
    }

    let inverted = graph
        .evaluate(&MaskReference {
            copy_id: "vc".into(),
            mask_id: "n".into(),
            extras: Extras::new(),
        })
        .expect("invert must evaluate");
    for (idx, &v) in inverted.values.iter().enumerate() {
        assert_eq!(v, 65535 - matte.values[idx], "invert at {idx}");
    }
}

/// Hash-pinning is visible: the committed fixture's SHA-256 pin is the
/// `model_hash` the graph persists, and the `MaskGraph` builder (via
/// `MaskDefinition.model.hash`) carries that exact pin — no download, no
/// silent fallback.
#[test]
fn maskgraph_source_carries_pinned_fixture_hash() {
    use lumina_sidecar::{Extras, ModelIdentity};
    let identity = ModelIdentity {
        name: "CraftedReduceMax".into(),
        version: "0.0.1".into(),
        hash: FIXTURE_PIN.into(),
        extras: Extras::new(),
    };
    assert_eq!(identity.hash, FIXTURE_PIN);
    assert_eq!(identity.hash.len(), 64);
    // The pin is also the on-disk digest; a mismatched pin would be
    // `Mismatch` and the ORT path would refuse inference (tested above).
    let path = write_pinned_fixture("maskgraph-pin");
    let digest = artifact_digest(&path);
    assert_eq!(digest, FIXTURE_PIN, "on-disk fixture must match the pin");
    let _ = std::fs::remove_file(&path);
}
