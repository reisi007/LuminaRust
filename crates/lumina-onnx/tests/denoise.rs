//! LRPAR-G14-DENOISE-IMPL-20 integration tests (default build, no `onnx-rt`):
//! the additive `denoise` capability, the hash/fixture gates, the
//! deterministic stub, the visible `unavailable` state, producer
//! provenance/staleness and the seamless tiled producer — plus the
//! Core↔Sidecar checksum contract.
//!
//! No weights, no network: everything runs against the deterministic stub and
//! synthetic RGB data.

use lumina_core::{
    apply_denoise_stage, denoise_producer_provenance, DenoisePolicy, DenoiseStageInput,
    DenoiseStageStatus, ImageFrame,
};
use lumina_onnx::{
    denoise_manifest, denoise_producer_identity, denoise_stage_status, fixture_denoise_model_hash,
    fixture_denoise_suite, produce_denoise_artifact, try_load_denoise_engine,
    verify_fixture_denoise_suite, DenoiseInference, DenoiseModelSuite, DenoiseRequest,
    DenoiseTileSpec, ModelCapabilities, OnnxError, Resolution, StubDenoiseBackend,
    DENOISE_MODEL_NAME, DENOISE_PENDING_HASH, PENDING_INTEGRATION_HASH,
};
use lumina_sidecar::{DenoiseRgbArtifact, RecordSpec, ZDataContainer};
use serde_json::json;

/// A small, fixture-pinned suite (`tile`×`tile`, `overlap` px) used so the
/// tests stay fast while exercising the real tiling/assembly path.
fn pinned_suite(tile: u32, overlap: u32) -> DenoiseModelSuite {
    let mut model = denoise_manifest();
    model.input.resolution = Resolution {
        width: tile,
        height: tile,
    };
    let planned =
        DenoiseModelSuite::new(model, DenoiseTileSpec::new(tile, tile, overlap).unwrap()).unwrap();
    let mut pinned = planned.clone();
    pinned.model.model_hash = fixture_denoise_model_hash(&planned);
    DenoiseModelSuite::new(pinned.model, pinned.tiles).unwrap()
}

fn gradient_frame(width: u32, height: u32) -> ImageFrame {
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    for (index, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        px[0] = (index * 7 % 256) as u8;
        px[1] = (index * 13 % 256) as u8;
        px[2] = (index * 29 % 256) as u8;
        px[3] = 255;
    }
    ImageFrame::new(width, height, pixels).unwrap()
}

#[test]
fn denoise_capability_is_additive_and_separate() {
    // The planned descriptor declares exactly the new capability.
    let manifest = denoise_manifest();
    assert_eq!(manifest.model_name, DENOISE_MODEL_NAME);
    assert!(manifest.capabilities.denoise);
    assert!(!manifest.capabilities.subject_segmentation);
    assert!(!manifest.capabilities.face_detect);
    assert!(!manifest.capabilities.face_embed);
    assert!(!manifest.capabilities.inpaint_heal);
    assert!(!manifest.capabilities.outpaint);
    assert!(manifest.validate().is_ok());

    // Capabilities written before the field existed keep parsing (additive
    // `#[serde(default)]`), and the field defaults to `false`.
    let legacy = json!({
        "subject_segmentation": false,
        "box_prompt": false,
        "point_prompt": false,
        "mask_prompt": false,
        "class_detection": false,
        "instance_segmentation": false,
        "inpaint_heal": false,
        "outpaint": false,
        "face_detect": false,
        "face_embed": false
    });
    let capabilities: ModelCapabilities = serde_json::from_value(legacy).unwrap();
    assert!(!capabilities.denoise);
    assert!(
        !capabilities.any(),
        "no capability declared in the legacy JSON"
    );

    // `denoise` alone is a valid, complete declaration.
    let denoise_only = ModelCapabilities {
        denoise: true,
        ..Default::default()
    };
    assert!(denoise_only.validate().is_ok());

    // Unknown capability fields are still rejected (no silent acceptance).
    let unknown = json!({ "subject_segmentation": true, "denoise": true, "future": true });
    assert!(serde_json::from_value::<ModelCapabilities>(unknown).is_err());
}

#[test]
fn hash_gate_planned_descriptor_stays_pending_fixture_is_verified() {
    let planned = DenoiseModelSuite::planned();
    assert_eq!(planned.model.model_hash, PENDING_INTEGRATION_HASH);
    assert!(!lumina_onnx::denoise_model_hash_is_pinned(&planned.model));

    let fixture = fixture_denoise_suite();
    assert!(lumina_onnx::denoise_model_hash_is_pinned(&fixture.model));
    assert_eq!(
        verify_fixture_denoise_suite(&fixture),
        lumina_onnx::ModelHashStatus::Verified
    );
    assert_eq!(
        verify_fixture_denoise_suite(&DenoiseModelSuite::planned()),
        lumina_onnx::ModelHashStatus::Pending
    );
}

#[test]
fn capability_and_contract_gates_are_loud() {
    // A manifest that does not declare `denoise` is refused by the suite…
    let mut wrong = denoise_manifest();
    wrong.capabilities = ModelCapabilities {
        subject_segmentation: true,
        ..Default::default()
    };
    assert!(matches!(
        DenoiseModelSuite::new(wrong.clone(), DenoiseTileSpec::default()),
        Err(OnnxError::UnsupportedModel { .. })
    ));
    // …and by the stub backend.
    assert!(matches!(
        StubDenoiseBackend::new(wrong),
        Err(OnnxError::UnsupportedModel { .. })
    ));

    // A non-identity (ImageNet) preprocessing is not the denoise contract.
    let mut wrong_norm = denoise_manifest();
    wrong_norm.input.normalization = lumina_onnx::InputNormalization::IMAGENET;
    assert!(matches!(
        DenoiseModelSuite::new(wrong_norm, DenoiseTileSpec::default()),
        Err(OnnxError::UnsupportedModel { .. })
    ));

    // Inference resolution must equal the tile size (one geometry per digest).
    let mut wrong_resolution = denoise_manifest();
    wrong_resolution.input.resolution = Resolution {
        width: 8,
        height: 8,
    };
    assert!(matches!(
        DenoiseModelSuite::new(wrong_resolution, DenoiseTileSpec::new(4, 4, 1).unwrap()),
        Err(OnnxError::InvalidDenoiseData(_))
    ));
}

#[test]
fn stub_is_deterministic_and_never_a_silent_fallback() {
    let suite = pinned_suite(4, 1);
    let stub = StubDenoiseBackend::new(suite.model.clone()).unwrap();
    let frame = gradient_frame(4, 4);
    let a = stub.denoise(&frame).unwrap();
    let b = stub.denoise(&frame).unwrap();
    assert_eq!(a, b, "identical input must produce identical output");
    assert_eq!(a.len(), 4 * 4 * 3);

    // A stub reporting itself unavailable refuses — it never emits pixels.
    let unavailable = StubDenoiseBackend::new(suite.model.clone())
        .unwrap()
        .with_availability(false);
    assert!(matches!(
        unavailable.denoise(&frame),
        Err(OnnxError::ModelUnavailable { .. })
    ));
}

#[test]
fn producer_is_seamless_deterministic_and_records_provenance() {
    let suite = pinned_suite(4, 1);
    let stub = StubDenoiseBackend::new(suite.model.clone()).unwrap();

    // A constant frame denoises to a constant frame: the overlap-weighted tile
    // assembly can introduce no seam.
    let constant = ImageFrame::new(6, 5, [120u8, 60, 30, 255].repeat(6 * 5)).unwrap();
    let request = DenoiseRequest {
        strength: 0.5,
        preserve_detail: 0.0,
        source_content_hash: "blake3:source",
        decode_fingerprint: "libraw:1:raw",
        artifact_relative_path: "IMG_0001.ARW.lumina.zdata",
    };
    let a = produce_denoise_artifact(&constant, &suite, &stub, &request).unwrap();
    let b = produce_denoise_artifact(&constant, &suite, &stub, &request).unwrap();
    assert_eq!(a, b, "same inputs → byte-identical artifact + recipe");
    assert_eq!((a.artifact.width, a.artifact.height), (6, 5));
    assert!(
        a.artifact
            .pixels
            .as_chunks::<3>()
            .0
            .iter()
            .all(|p| *p == [120, 60, 30]),
        "constant tiles must reconstruct exactly, without a seam line"
    );

    // The recipe pins the producing model, the input spec and the artifact.
    let artifact_ref = a.recipe.artifact.as_ref().expect("artifact reference");
    assert_eq!(artifact_ref.checksum, a.artifact.checksum());
    assert_eq!(artifact_ref.relative_path, "IMG_0001.ARW.lumina.zdata");
    assert_eq!(a.recipe.input_spec_digest, suite.input_spec_digest());
    assert_eq!(a.recipe.model.model_hash, suite.model.model_hash);

    // The persisted provenance roundtrips and resolves `Ready`.
    let recorded = denoise_producer_provenance(&a.recipe).expect("provenance persisted");
    let current = denoise_producer_identity(
        &suite,
        "blake3:source",
        "libraw:1:raw",
        a.artifact.checksum(),
    );
    assert_eq!(recorded, current);
    assert_eq!(
        denoise_stage_status(&a.recipe, &current, &recorded, true),
        DenoiseStageStatus::Ready
    );

    // A model change invalidates the artifact (stale), never silently reused.
    let mut model_changed = current.clone();
    model_changed.model_version = "2".into();
    assert_eq!(
        denoise_stage_status(&a.recipe, &model_changed, &recorded, true),
        DenoiseStageStatus::Stale
    );
    // An input-spec change (e.g. different tiles) invalidates too.
    let mut tile_changed = current.clone();
    tile_changed.input_spec_digest = format!("sha256:{}", "11".repeat(32));
    assert_eq!(
        denoise_stage_status(&a.recipe, &tile_changed, &recorded, true),
        DenoiseStageStatus::Stale
    );
}

#[test]
fn pending_integration_is_unavailable_without_a_silent_fallback() {
    let suite = pinned_suite(4, 1);
    let stub = StubDenoiseBackend::new(suite.model.clone()).unwrap();
    let frame = gradient_frame(4, 4);

    // Producing with the planned (pending) descriptor is refused loudly.
    let request = DenoiseRequest {
        strength: 0.5,
        preserve_detail: 0.0,
        source_content_hash: "blake3:source",
        decode_fingerprint: "libraw:1:raw",
        artifact_relative_path: "IMG.lumina.zdata",
    };
    let err = produce_denoise_artifact(&frame, &DenoiseModelSuite::planned(), &stub, &request)
        .unwrap_err();
    assert!(matches!(err, OnnxError::ModelUnavailable { .. }), "{err:?}");

    // The engine resolver reports the same visible state instead of a stub.
    assert!(matches!(
        try_load_denoise_engine(
            std::path::Path::new("/nonexistent/denoise.onnx"),
            denoise_manifest()
        ),
        Err(OnnxError::ModelUnavailable { .. })
    ));

    // End-to-end: a persisted artifact whose model identity is the placeholder
    // resolves to `unavailable`, and the core stage refuses it loudly under
    // `Strict` / falls back explicitly under `Warn` — never a silent no-op.
    let produced = produce_denoise_artifact(&frame, &suite, &stub, &request).unwrap();
    let recorded = denoise_producer_provenance(&produced.recipe).unwrap();
    let mut pending = produced.recipe.clone();
    pending.model.model_hash = DENOISE_PENDING_HASH.into();
    let status = denoise_stage_status(&pending, &recorded, &recorded, true);
    assert_eq!(status, DenoiseStageStatus::Unavailable);

    let original = frame.clone();
    let mut strict = frame.clone();
    let error = apply_denoise_stage(
        &mut strict,
        Some(&pending),
        &DenoiseStageInput::non_ready(status, "pending-integration"),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        lumina_core::CoreError::Denoise { ref status, .. } if status == "unavailable"
    ));
    assert_eq!(strict.pixels, original.pixels);

    let mut warn = frame.clone();
    let outcome = apply_denoise_stage(
        &mut warn,
        Some(&pending),
        &DenoiseStageInput::non_ready(status, "pending-integration")
            .with_policy(DenoisePolicy::Warn),
    )
    .unwrap();
    assert!(!outcome.applied, "the fallback is explicit, not silent");
    assert_eq!(warn.pixels, original.pixels);
}

/// B4 (Auflage Kern-Verifizierung 2026-09-16): the artifact checksum the
/// producer writes into the recipe must equal the sidecar `.lumina.zdata`
/// `denoise_rgb` record checksum for the same pixels (one canonical encoding),
/// and the zdata roundtrip must preserve the payload.
#[test]
fn cross_crate_checksum_matches_the_sidecar_denoise_record() {
    let suite = pinned_suite(4, 1);
    let stub = StubDenoiseBackend::new(suite.model.clone()).unwrap();
    let frame = gradient_frame(4, 4);
    let request = DenoiseRequest {
        strength: 1.0,
        preserve_detail: 0.0,
        source_content_hash: "blake3:source",
        decode_fingerprint: "libraw:1:raw",
        artifact_relative_path: "IMG.lumina.zdata",
    };
    let produced = produce_denoise_artifact(&frame, &suite, &stub, &request).unwrap();

    let sidecar = DenoiseRgbArtifact {
        id: "denoise-1".into(),
        width: produced.artifact.width,
        height: produced.artifact.height,
        pixels: produced.artifact.pixels.clone(),
    };
    assert_eq!(
        produced.artifact.checksum(),
        sidecar.checksum(),
        "core and sidecar must share the canonical RGB8 digest"
    );
    assert_eq!(
        produced.recipe.artifact.as_ref().unwrap().checksum,
        sidecar.checksum()
    );

    let container =
        ZDataContainer::new_with(vec![RecordSpec::DenoiseRgb(sidecar.clone())]).unwrap();
    let read_back = container.denoise_rgb("denoise-1").unwrap();
    assert_eq!(read_back, sidecar);
    assert_eq!(read_back.checksum(), produced.artifact.checksum());
}

/// The `onnx-rt` capability statement is explicit when the feature is off.
#[cfg(not(feature = "onnx-rt"))]
#[test]
fn engine_resolver_is_explicit_without_the_feature() {
    let engine = try_load_denoise_engine(
        std::path::Path::new("/nonexistent/denoise.onnx"),
        pinned_suite(4, 1).model,
    )
    .expect("feature-off load must succeed with the explicit flag");
    assert!(matches!(
        engine,
        lumina_onnx::DenoiseOnnxEngine::RuntimeDisabled
    ));
}
