//! LRPAR-G14-DENOISE-IMPL-20 integration tests for the real ONNX Runtime
//! denoise path (`onnx-rt` feature).
//!
//! Real denoise weights are neither committed nor downloadable (Agents.md: no
//! spontaneous downloads), so — exactly like `face_ort.rs` / `ort_backend.rs` —
//! these tests use the committed, hash-pinned behavior fixture
//! `fixtures/lumina-crafted-reducemax.onnx` (a minimal deterministic ReduceMax
//! graph, no weights). It exercises the load/verify/tensor-name paths and the
//! canonical output-contract refusal without claiming numeric denoise
//! correctness against real weights.
//!
//! Covered gates:
//!
//! * missing artifact → [`OnnxError::MissingModel`];
//! * `pending-integration` manifest → [`OnnxError::ModelUnavailable`] (the
//!   visible §6 `unavailable` state, never unverifiable inference);
//! * digest ≠ pinned hash → `ModelArtifactStale` at inference;
//! * manifest tensor name absent from the graph → `InferenceFailed` at load;
//! * output shape violating the canonical `[1, 3, H, W]` contract →
//!   `InferenceFailed` (never a silent reshape);
//! * `try_load_denoise_engine` resolves the real engine or fails visibly —
//!   never a stub fallback — and the ORT backend plugs into the tiled producer.

#![cfg(feature = "onnx-rt")]

use lumina_core::ImageFrame;
use lumina_onnx::denoise::ort::OrtDenoiser;
use lumina_onnx::{
    compute_sha256_hex, denoise_manifest, produce_denoise_artifact, try_load_denoise_engine,
    DenoiseInference, DenoiseModelSuite, DenoiseOnnxEngine, DenoiseRequest, DenoiseTileSpec,
    ModelManifest, OnnxError, Resolution,
};
use std::io::Cursor;
use std::path::Path;

/// SHA-256 pin of the committed behavior fixture
/// (see `tests/fixtures/README.md`).
const FIXTURE_PIN: &str = "2a2ede6659e8c59b3fd972242b27677ef23cb98d3c422616a1c65f50dcaca18d";
const FIXTURE_BYTES: &[u8] = include_bytes!("fixtures/lumina-crafted-reducemax.onnx");

const W: u32 = 8;
const H: u32 = 8;
const INPUT_NAME: &str = "x";
const OUTPUT_NAME: &str = "y";

fn write_fixture(tag: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "lumina-onnx-denoise-{tag}-{}-{}.onnx",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&path, FIXTURE_BYTES).expect("write denoise fixture");
    path
}

fn manifest(model_hash: &str) -> ModelManifest {
    let mut manifest = denoise_manifest();
    manifest.input.resolution = Resolution {
        width: W,
        height: H,
    };
    manifest.input.tensor_name = INPUT_NAME.into();
    manifest.output_tensor_name = OUTPUT_NAME.into();
    manifest.model_hash = model_hash.into();
    manifest
}

fn frame(width: u32, height: u32) -> ImageFrame {
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for (index, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        px[0] = (index % 256) as u8;
        px[1] = ((index * 3) % 256) as u8;
        px[2] = ((index * 7) % 256) as u8;
        px[3] = 255;
    }
    ImageFrame::new(width, height, pixels).unwrap()
}

#[test]
fn fixture_matches_its_documented_pin() {
    let digest = compute_sha256_hex(Cursor::new(FIXTURE_BYTES)).unwrap();
    assert_eq!(digest, FIXTURE_PIN, "fixture drifted from its pin");
}

#[test]
fn denoiser_reports_missing_model() {
    let result = OrtDenoiser::new("/nonexistent/lumina/denoise.onnx", manifest(FIXTURE_PIN));
    assert!(matches!(result, Err(OnnxError::MissingModel { .. })));
}

#[test]
fn denoiser_refuses_pending_integration_loudly() {
    let path = write_fixture("pending");
    let result = OrtDenoiser::new(&path, denoise_manifest());
    let _ = std::fs::remove_file(&path);
    assert!(
        matches!(result, Err(OnnxError::ModelUnavailable { .. })),
        "a pending-integration manifest must be refused loudly"
    );
}

#[test]
fn denoiser_refuses_stale_weights_end_to_end() {
    let path = write_fixture("stale");
    let pinned = format!("{:064x}", 0xdead_beef_u64);
    let backend = OrtDenoiser::new(&path, manifest(&pinned))
        .expect("a hash mismatch must not prevent loading (status is queryable)");
    assert!(matches!(
        backend.hash_status(),
        lumina_onnx::ModelHashStatus::Mismatch { expected, .. } if expected == &pinned
    ));
    let err = backend.denoise(&frame(4, 4)).unwrap_err();
    let _ = std::fs::remove_file(&path);
    assert!(
        matches!(err, OnnxError::ModelArtifactStale { .. }),
        "stale weights must refuse inference, got {err:?}"
    );
}

#[test]
fn denoiser_rejects_unknown_tensor_names_at_load() {
    let path = write_fixture("names");
    let result = OrtDenoiser::new(&path, {
        let mut manifest = manifest(FIXTURE_PIN);
        manifest.output_tensor_name = "does-not-exist".into();
        manifest
    });
    let _ = std::fs::remove_file(&path);
    match result {
        Err(OnnxError::InferenceFailed { reason, .. }) => {
            assert!(reason.contains("`does-not-exist`"), "{reason}");
            assert!(reason.contains("`y`"), "{reason}");
        }
        Err(other) => panic!("expected InferenceFailed, got {other:?}"),
        Ok(_) => panic!("a manifest with a wrong output name must not load"),
    }
}

/// The fixture is loadable and hash-verifies, but its single-channel
/// `[1, 1, H, W]` output violates the RGB denoise contract — a loud
/// `InferenceFailed`, never a silent reshape into RGB.
#[test]
fn denoiser_refuses_contract_violating_output_shape() {
    let path = write_fixture("shape");
    let backend = OrtDenoiser::new(&path, manifest(FIXTURE_PIN)).expect("the pinned fixture loads");
    assert_eq!(
        backend.hash_status(),
        &lumina_onnx::ModelHashStatus::Verified
    );
    let err = backend.denoise(&frame(W, H)).unwrap_err();
    let _ = std::fs::remove_file(&path);
    match err {
        OnnxError::InferenceFailed { reason, .. } => {
            assert!(
                reason.contains("unexpected denoise output shape"),
                "{reason}"
            );
        }
        other => panic!("expected InferenceFailed, got {other:?}"),
    }
}

/// The ORT backend plugs straight into the tiled producer; the fixture's
/// contract violation is therefore surfaced through
/// `produce_denoise_artifact` too — never a silently assembled artifact.
#[test]
fn producer_with_the_real_backend_surfaces_the_contract_violation() {
    let path = write_fixture("producer");
    let backend = OrtDenoiser::new(&path, manifest(FIXTURE_PIN)).expect("the pinned fixture loads");
    let suite = DenoiseModelSuite::new(
        manifest(FIXTURE_PIN),
        DenoiseTileSpec::new(W, H, 0).unwrap(),
    )
    .unwrap();
    let request = DenoiseRequest {
        strength: 0.5,
        preserve_detail: 0.0,
        source_content_hash: "blake3:source",
        decode_fingerprint: "libraw:1:raw",
        artifact_relative_path: "IMG.lumina.zdata",
    };
    let err = produce_denoise_artifact(&frame(W, H), &suite, &backend, &request).unwrap_err();
    let _ = std::fs::remove_file(&path);
    assert!(
        matches!(err, OnnxError::InferenceFailed { .. }),
        "the producer must surface the backend contract violation, got {err:?}"
    );
}

#[test]
fn engine_resolver_is_explicit_and_never_falls_back() {
    // Missing artifacts → hard error, never a stub.
    assert!(matches!(
        try_load_denoise_engine(
            Path::new("/nonexistent/lumina/denoise.onnx"),
            manifest(FIXTURE_PIN)
        ),
        Err(OnnxError::MissingModel { .. })
    ));

    // Pending placeholder → the visible `unavailable` state.
    let path = write_fixture("engine-pending");
    assert!(matches!(
        try_load_denoise_engine(&path, denoise_manifest()),
        Err(OnnxError::ModelUnavailable { .. })
    ));
    let _ = std::fs::remove_file(&path);

    // A loadable, pinned artifact resolves to the real engine.
    let path = write_fixture("engine-pinned");
    let engine = try_load_denoise_engine(&path, manifest(FIXTURE_PIN))
        .expect("a pinned fixture must resolve");
    let _ = std::fs::remove_file(&path);
    assert!(
        matches!(engine, DenoiseOnnxEngine::OnnxRuntime(_)),
        "expected the real engine, got {engine:?}"
    );
}
