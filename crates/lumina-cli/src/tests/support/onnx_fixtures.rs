use super::*;

// ------------------------------------------------------------------
// F-082-FOLLOWUP — onnx-rt wiring test support.
//
// The process-level mask tests below all start in
// `write_sidecar_with_valid_layer`, which triggers the CLI's inference
// wiring gate. Under `onnx-rt` those renders request the REAL engine, so
// `LUMINA_MODEL_PATH` must point at a loadable AND runnable artifact. The
// deterministic BiRefNet-compatible crafted model below is generated at
// test runtime (no committed binary, no downloads — mirroring
// `crates/lumina-onnx/tests/ort_backend.rs`), so the whole CLI suite stays
// green with and without the feature.
// ------------------------------------------------------------------

/// Proto3 varint (mirrors `lumina-onnx/tests/ort_backend.rs`).
#[cfg(feature = "onnx-rt")]
pub(crate) fn push_varint(out: &mut Vec<u8>, mut value: u64) {
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

#[cfg(feature = "onnx-rt")]
pub(crate) fn push_tag(out: &mut Vec<u8>, field: u32, wire_type: u64) {
    push_varint(out, ((field as u64) << 3) | wire_type);
}

#[cfg(feature = "onnx-rt")]
pub(crate) fn push_len_delimited(out: &mut Vec<u8>, field: u32, payload: &[u8]) {
    push_tag(out, field, 2);
    push_varint(out, payload.len() as u64);
    out.extend_from_slice(payload);
}

#[cfg(feature = "onnx-rt")]
pub(crate) fn push_string(out: &mut Vec<u8>, field: u32, value: &str) {
    push_len_delimited(out, field, value.as_bytes());
}

#[cfg(feature = "onnx-rt")]
pub(crate) fn push_varint_field(out: &mut Vec<u8>, field: u32, value: u64) {
    push_tag(out, field, 0);
    push_varint(out, value);
}

/// `TensorShapeProto.dim` (`dim_value`), `TypeProto.Tensor`,
/// `ValueInfoProto` and `AttributeProto` encodings for the crafted graph.
#[cfg(feature = "onnx-rt")]
pub(crate) fn shape_proto(dims: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    for dim in dims {
        let mut entry = Vec::new();
        push_varint_field(&mut entry, 1, *dim as u64);
        push_len_delimited(&mut out, 1, &entry);
    }
    out
}

#[cfg(feature = "onnx-rt")]
pub(crate) fn value_info(name: &str, dims: &[i64]) -> Vec<u8> {
    let mut tensor = Vec::new();
    push_varint_field(&mut tensor, 1, 1); // FLOAT
    push_len_delimited(&mut tensor, 2, &shape_proto(dims));
    let mut type_info = Vec::new();
    push_len_delimited(&mut type_info, 1, &tensor);
    let mut out = Vec::new();
    push_string(&mut out, 1, name);
    push_len_delimited(&mut out, 2, &type_info);
    out
}

#[cfg(feature = "onnx-rt")]
pub(crate) fn reduce_max_node(input: &str, output: &str) -> Vec<u8> {
    // ReduceMax(axes=[1], keepdims=1), attributes-based (opset ≤ 17).
    let mut axes = Vec::new();
    push_string(&mut axes, 1, "axes");
    push_varint_field(&mut axes, 20, 7); // AttributeType::INTS
    let mut packed_axes = Vec::new();
    push_varint(&mut packed_axes, 1);
    push_len_delimited(&mut axes, 8, &packed_axes);

    let mut keepdims = Vec::new();
    push_string(&mut keepdims, 1, "keepdims");
    push_varint_field(&mut keepdims, 20, 2); // AttributeType::INT
    push_varint_field(&mut keepdims, 3, 1); // keepdims = true

    let mut out = Vec::new();
    push_string(&mut out, 1, input);
    push_string(&mut out, 2, output);
    push_string(&mut out, 4, "ReduceMax");
    push_len_delimited(&mut out, 5, &axes);
    push_len_delimited(&mut out, 5, &keepdims);
    out
}

/// Deterministic bytes of a **BiRefNet-compatible** crafted ONNX graph:
/// `input [1,3,1024,1024] → ReduceMax(axes=[1], keepdims=1) →
/// output [1,1,1024,1024]` (ir_version 8 / opset 13). Same structure as
/// the committed `lumina-crafted-reducemax.onnx` behavior fixture, but with
/// the BiRefNet manifest's tensor names (`input`/`output`) and inference
/// resolution, so `lumina_onnx::OrtBackend` driven by `birefnet_manifest()`
/// can load **and run** it — the CLI's `onnx-rt` wiring gets a real engine
/// in tests without a committed binary or a download. `ReduceMax` over the
/// channel axis on a uniform frame yields a deterministic uniform matte
/// (the lumina-onnx fixture test proves the same graph family under ORT).
#[cfg(feature = "onnx-rt")]
pub(crate) fn birefnet_compatible_onnx_bytes() -> Vec<u8> {
    const INPUT: &str = "input";
    const OUTPUT: &str = "output";
    const W: i64 = lumina_onnx::BIREFNET_INFERENCE_WIDTH as i64;
    const H: i64 = lumina_onnx::BIREFNET_INFERENCE_HEIGHT as i64;

    let mut opset = Vec::new();
    push_varint_field(&mut opset, 2, 13); // OperatorSetIdProto { version: 13 }

    let mut graph = Vec::new();
    push_len_delimited(&mut graph, 1, &reduce_max_node(INPUT, OUTPUT));
    push_string(&mut graph, 2, "lumina-cli-crafted-birefnet-compatible");
    push_len_delimited(&mut graph, 11, &value_info(INPUT, &[1, 3, H, W]));
    push_len_delimited(&mut graph, 12, &value_info(OUTPUT, &[1, 1, H, W]));

    let mut out = Vec::new();
    push_varint_field(&mut out, 1, 8); // ir_version 8
    push_len_delimited(&mut out, 7, &graph); // graph
    push_len_delimited(&mut out, 8, &opset); // opset_import
    out
}

/// Path to a persistent BiRefNet-compatible ONNX test model (created once
/// per process). The backing temp directory is deliberately leaked so the
/// artifact stays alive for the whole test process.
#[cfg(feature = "onnx-rt")]
pub(crate) fn onnx_test_fixture_path() -> PathBuf {
    use std::sync::OnceLock;
    static FIXTURE_DIR: OnceLock<PathBuf> = OnceLock::new();
    let dir = FIXTURE_DIR.get_or_init(|| {
        let directory = tempfile::tempdir().expect("test fixture tempdir must be creatable");
        let path = directory.path().to_path_buf();
        // Test-only leak: the directory (and the fixture file below) must
        // survive for the entire test process.
        std::mem::forget(directory);
        path
    });
    let fixture = dir.join("lumina-birefnet-compatible.onnx");
    if !fixture.exists() {
        fs::write(&fixture, birefnet_compatible_onnx_bytes())
            .expect("test ONNX fixture must be writable");
    }
    fixture
}

/// Ensures `LUMINA_MODEL_PATH` points at the runnable BiRefNet-compatible
/// test model. Every env mutation in the suite writes the **same** value
/// (idempotent), so parallel render tests never observe a broken path. The
/// wiring-semantics tests use the path-parameterized
/// `resolve_onnx_engine_from_path` and do NOT touch the env var at all.
#[cfg(feature = "onnx-rt")]
pub(crate) fn ensure_onnx_test_engine() {
    if std::env::var_os("LUMINA_MODEL_PATH").is_none() {
        std::env::set_var("LUMINA_MODEL_PATH", onnx_test_fixture_path());
    }
}
