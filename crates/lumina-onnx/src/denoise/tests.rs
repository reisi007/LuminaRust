use super::*;
use crate::hash::ModelHashStatus;
use crate::{InputNormalization as Normalization, ModelCapabilities as Capabilities};

fn tiny_manifest() -> ModelManifest {
    ModelManifest {
        model_name: "tiny-denoise".into(),
        model_version: "1".into(),
        model_hash: PENDING_INTEGRATION_HASH.into(),
        license: "MIT".into(),
        input: ModelInputSpec {
            resolution: ModelResolution {
                width: 4,
                height: 4,
            },
            channel_layout: ChannelLayout::Rgb,
            tensor_name: "image".into(),
            tensor_format: TensorFormat::Nchw,
            normalization: Normalization::IDENTITY,
        },
        output_tensor_name: "output".into(),
        capabilities: Capabilities {
            denoise: true,
            ..Default::default()
        },
    }
}

fn tiny_suite() -> DenoiseModelSuite {
    DenoiseModelSuite::new(tiny_manifest(), DenoiseTileSpec::new(4, 4, 1).unwrap()).unwrap()
}

/// A `tile`×`tile` suite with the deterministic fixture pin (real hash).
fn pinned_suite(tile: u32, overlap: u32) -> DenoiseModelSuite {
    let mut model = tiny_manifest();
    model.input.resolution = ModelResolution {
        width: tile,
        height: tile,
    };
    let planned =
        DenoiseModelSuite::new(model, DenoiseTileSpec::new(tile, tile, overlap).unwrap()).unwrap();
    let mut pinned = planned.clone();
    pinned.model.model_hash = fixture_denoise_model_hash(&planned);
    DenoiseModelSuite::new(pinned.model, pinned.tiles).unwrap()
}

#[test]
fn planned_descriptor_declares_only_denoise_and_stays_pending() {
    let manifest = denoise_manifest();
    assert!(manifest.capabilities.denoise);
    assert!(!manifest.capabilities.subject_segmentation);
    assert!(!manifest.capabilities.face_detect);
    assert!(!manifest.capabilities.face_embed);
    assert!(!manifest.capabilities.inpaint_heal);
    assert!(!manifest.capabilities.outpaint);
    assert_eq!(manifest.model_hash, PENDING_INTEGRATION_HASH);
    assert!(!denoise_model_hash_is_pinned(&manifest));
    assert!(manifest.validate().is_ok());
    assert_eq!(manifest.input.normalization, InputNormalization::IDENTITY);
    assert!(matches!(
        verify_fixture_denoise_suite(&DenoiseModelSuite::planned()),
        ModelHashStatus::Pending
    ));
}

#[test]
fn fixture_suite_is_really_pinned_and_direction_sensitive() {
    let fixture = fixture_denoise_suite();
    assert!(denoise_model_hash_is_pinned(&fixture.model));
    assert!(fixture.model.model_hash.starts_with("sha256:"));
    assert_eq!(fixture.model.model_hash.len(), "sha256:".len() + 64);
    assert_eq!(
        verify_fixture_denoise_suite(&fixture),
        ModelHashStatus::Verified
    );
    assert!(fixture.validate().is_ok());
    assert_eq!(
        DenoiseModelSuite::planned().input_spec_digest(),
        DENOISE_DEFAULT_INPUT_SPEC_DIGEST
    );
    assert_eq!(fixture.model.model_hash, DENOISE_DEFAULT_FIXTURE_MODEL_HASH);

    // Deterministic across calls.
    assert_eq!(
        fixture.model.model_hash,
        fixture_denoise_suite().model.model_hash
    );

    // A changed spec flips the pin (the recomputed digest no longer
    // matches the stored one)…
    let mut changed = fixture.clone();
    changed.model.model_version = "2".into();
    assert!(matches!(
        verify_fixture_denoise_suite(&changed),
        ModelHashStatus::Mismatch { .. }
    ));
    assert_ne!(
        fixture_denoise_model_hash(&changed),
        fixture.model.model_hash
    );
}

#[test]
fn input_spec_digest_is_deterministic_and_covers_tiles_and_model() {
    let suite = tiny_suite();
    let digest = suite.input_spec_digest();
    assert!(digest.starts_with("sha256:"));
    assert_eq!(digest.len(), "sha256:".len() + 64);
    assert_eq!(digest, tiny_suite().input_spec_digest());

    // Tile geometry is identity-bearing.
    let mut tile_changed = suite.clone();
    tile_changed.tiles = DenoiseTileSpec::new(4, 4, 2).unwrap();
    assert_ne!(digest, tile_changed.input_spec_digest());

    // Inference resolution (part of the manifest input spec) is too.
    let mut resolution_changed = suite.clone();
    resolution_changed.model.input.resolution = ModelResolution {
        width: 4,
        height: 4,
    };
    resolution_changed.model.input.tensor_name = "pixels".into();
    assert_ne!(digest, resolution_changed.input_spec_digest());

    // The persisted model identity carries the denoise digest (not the
    // plain model input digest).
    let identity = suite.to_model_identity();
    let stored = identity
        .extras
        .get(INPUT_SPEC_DIGEST_KEY)
        .expect("identity must carry the input-spec digest");
    assert_eq!(stored, &serde_json::Value::String(digest));
}

#[test]
fn suite_rejects_wrong_capability_and_contract() {
    let mut wrong_capability = tiny_manifest();
    wrong_capability.capabilities = Capabilities {
        face_detect: true,
        ..Default::default()
    };
    assert!(matches!(
        DenoiseModelSuite::new(wrong_capability, DenoiseTileSpec::default()),
        Err(OnnxError::UnsupportedModel { .. })
    ));

    let mut wrong_norm = tiny_manifest();
    wrong_norm.input.normalization = InputNormalization::IMAGENET;
    assert!(matches!(
        DenoiseModelSuite::new(wrong_norm, DenoiseTileSpec::new(4, 4, 0).unwrap()),
        Err(OnnxError::UnsupportedModel { .. })
    ));

    let mut wrong_resolution = tiny_manifest();
    wrong_resolution.input.resolution = ModelResolution {
        width: 8,
        height: 8,
    };
    assert!(matches!(
        DenoiseModelSuite::new(wrong_resolution, DenoiseTileSpec::new(4, 4, 1).unwrap()),
        Err(OnnxError::InvalidDenoiseData(_))
    ));
}

#[test]
fn tile_spec_validation_is_loud() {
    assert!(DenoiseTileSpec::new(0, 4, 0).is_err());
    assert!(DenoiseTileSpec::new(4, 0, 0).is_err());
    assert!(DenoiseTileSpec::new(4, 4, 4).is_err());
    assert!(DenoiseTileSpec::new(4, 4, 5).is_err());
    let ok = DenoiseTileSpec::new(4, 4, 1).unwrap();
    assert_eq!(ok.stride_x(), 3);
    assert_eq!(ok.stride_y(), 3);
}

#[test]
fn decode_denoise_rgb_converts_nchw_and_is_loud() {
    // [1, 3, 2, 1]: R plane [0.0, 1.0], G plane [0.5, 0.5], B plane [0, 0].
    let out = decode_denoise_rgb("M", &[1, 3, 2, 1], &[0.0, 1.0, 0.5, 0.5, 0.0, 0.0]).unwrap();
    assert_eq!(out, vec![0, 128, 0, 255, 128, 0]);

    // Wrong rank/channels, wrong value count, non-finite → loud.
    assert!(decode_denoise_rgb("M", &[1, 1, 2, 2], &[0.0; 4]).is_err());
    assert!(decode_denoise_rgb("M", &[1, 3, 2, 2], &[0.0; 4]).is_err());
    assert!(decode_denoise_rgb("M", &[1, 3, 1, 2], &[0.0, f32::NAN, 0.0, 0.0, 0.0, 0.0]).is_err());
}

#[test]
fn stub_is_deterministic_and_available_gate_is_loud() {
    let stub = StubDenoiseBackend::new(tiny_manifest()).unwrap();
    let mut pixels = vec![0u8; 4 * 4 * 4];
    for (i, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        px[0] = (i * 11 % 256) as u8;
        px[1] = (i * 17 % 256) as u8;
        px[2] = (i * 23 % 256) as u8;
        px[3] = 255;
    }
    let frame = lumina_core::ImageFrame::new(4, 4, pixels).unwrap();
    let a = stub.denoise(&frame).unwrap();
    let b = stub.denoise(&frame).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.len(), 4 * 4 * 3);

    let unavailable = StubDenoiseBackend::new(tiny_manifest())
        .unwrap()
        .with_availability(false);
    assert!(matches!(
        unavailable.denoise(&frame),
        Err(OnnxError::ModelUnavailable { .. })
    ));
    assert!(matches!(
        stub.denoise(&lumina_core::ImageFrame::new(0, 0, vec![]).unwrap()),
        Err(OnnxError::InvalidDimensions { .. })
    ));

    // A manifest without the capability is refused loudly.
    let mut wrong = tiny_manifest();
    wrong.capabilities = Capabilities {
        subject_segmentation: true,
        ..Default::default()
    };
    assert!(matches!(
        StubDenoiseBackend::new(wrong),
        Err(OnnxError::UnsupportedModel { .. })
    ));
}

#[test]
fn producer_pending_manifest_is_refused_loudly() {
    let stub = StubDenoiseBackend::new(tiny_manifest()).unwrap();
    let frame = lumina_core::ImageFrame::new(4, 4, vec![128u8; 4 * 4 * 4]).unwrap();
    let request = DenoiseRequest {
        strength: 0.5,
        preserve_detail: 0.0,
        source_content_hash: "blake3:src",
        decode_fingerprint: "libraw:1",
        artifact_relative_path: "IMG.lumina.zdata",
    };
    let err = produce_denoise_artifact(&frame, &tiny_suite(), &stub, &request).unwrap_err();
    assert!(
        matches!(err, OnnxError::ModelUnavailable { .. }),
        "a pending-integration suite must be refused, got {err:?}"
    );
}

#[test]
fn producer_is_seamless_for_constant_tiles_and_deterministic() {
    // A small, fixture-pinned suite (4x4 tile, 1px overlap).
    let small = pinned_suite(4, 1);
    let stub = StubDenoiseBackend::new(small.model.clone()).unwrap();

    // Constant frame: box mean is constant → no seam.
    let mut constant = vec![0u8; 6 * 5 * 4];
    for px in constant.as_chunks_mut::<4>().0.iter_mut() {
        px[0] = 40;
        px[1] = 50;
        px[2] = 60;
        px[3] = 255;
    }
    let frame = lumina_core::ImageFrame::new(6, 5, constant).unwrap();
    let request = DenoiseRequest {
        strength: 0.5,
        preserve_detail: 0.0,
        source_content_hash: "blake3:src",
        decode_fingerprint: "libraw:1",
        artifact_relative_path: "IMG.lumina.zdata",
    };
    let a = produce_denoise_artifact(&frame, &small, &stub, &request).unwrap();
    let b = produce_denoise_artifact(&frame, &small, &stub, &request).unwrap();
    assert_eq!(a, b, "same inputs must be byte-identical");
    assert_eq!((a.artifact.width, a.artifact.height), (6, 5));
    assert!(
        a.artifact
            .pixels
            .as_chunks::<3>()
            .0
            .iter()
            .all(|p| *p == [40, 50, 60]),
        "constant tiles must assemble a constant, seam-free frame"
    );
    // The recipe references exactly the produced artifact.
    assert_eq!(
        a.recipe.artifact.as_ref().unwrap().checksum,
        a.artifact.checksum()
    );
    assert_eq!(
        a.recipe.model.model_hash, small.model.model_hash,
        "the recipe pins the producing model"
    );
}

#[test]
fn engine_resolver_refuses_pending_and_reports_capability_absence() {
    // The pending placeholder is a loud `unavailable`, in every build —
    // never a silent `RuntimeDisabled` or a stub.
    assert!(matches!(
        try_load_denoise_engine(
            std::path::Path::new("/nonexistent/denoise.onnx"),
            denoise_manifest()
        ),
        Err(OnnxError::ModelUnavailable { .. })
    ));
    let mut wrong = tiny_manifest();
    wrong.capabilities = Capabilities {
        subject_segmentation: true,
        ..Default::default()
    };
    assert!(matches!(
        try_load_denoise_engine(std::path::Path::new("/nonexistent/denoise.onnx"), wrong),
        Err(OnnxError::UnsupportedModel { .. })
    ));

    // A pinned manifest without the `onnx-rt` feature yields the explicit
    // capability statement (the ORT path itself is covered in
    // `tests/denoise_ort.rs`).
    #[cfg(not(feature = "onnx-rt"))]
    {
        let pinned = pinned_suite(4, 1);
        let engine = try_load_denoise_engine(
            std::path::Path::new("/nonexistent/denoise.onnx"),
            pinned.model,
        )
        .expect("feature-off load must succeed with the explicit flag");
        assert!(
            matches!(engine, DenoiseOnnxEngine::RuntimeDisabled),
            "expected the explicit capability statement, got {engine:?}"
        );
    }
}

#[test]
fn producer_records_provenance_and_status_resolution_is_visible() {
    use lumina_core::denoise_producer_provenance;
    use lumina_core::DenoiseStageStatus;

    let suite = pinned_suite(4, 1);
    let stub = StubDenoiseBackend::new(suite.model.clone()).unwrap();
    let frame = lumina_core::ImageFrame::new(4, 4, [10u8, 20, 30, 255].repeat(16)).unwrap();
    let request = DenoiseRequest {
        strength: 0.5,
        preserve_detail: 0.0,
        source_content_hash: "blake3:src",
        decode_fingerprint: "libraw:1",
        artifact_relative_path: "IMG.lumina.zdata",
    };
    let produced = produce_denoise_artifact(&frame, &suite, &stub, &request).unwrap();

    let recorded = denoise_producer_provenance(&produced.recipe)
        .expect("the producer must persist its provenance");
    let current = denoise_producer_identity(
        &suite,
        "blake3:src",
        "libraw:1",
        produced.artifact.checksum(),
    );
    assert_eq!(recorded, current);
    assert_eq!(
        denoise_stage_status(&produced.recipe, &current, &recorded, true),
        DenoiseStageStatus::Ready
    );

    // Model change → stale.
    let mut changed = current.clone();
    changed.model_version = "9".into();
    assert_eq!(
        denoise_stage_status(&produced.recipe, &changed, &recorded, true),
        DenoiseStageStatus::Stale
    );
    // Producer-provenance drift → stale.
    let mut drift = recorded.clone();
    drift.input_spec_digest = format!("{DENOISE_SHA256_PREFIX}{}", "00".repeat(32));
    assert_eq!(
        denoise_stage_status(&produced.recipe, &current, &drift, true),
        DenoiseStageStatus::Stale
    );
    // Absent artifact → missing; checksum mismatch → corrupt.
    assert_eq!(
        denoise_stage_status(&produced.recipe, &current, &recorded, false),
        DenoiseStageStatus::Missing
    );
    let mut corrupt = current.clone();
    corrupt.artifact_checksum = "00".repeat(32);
    assert_eq!(
        denoise_stage_status(&produced.recipe, &corrupt, &recorded, true),
        DenoiseStageStatus::Corrupt
    );
    // pending-integration → unavailable even with a perfect artifact.
    let mut pending = produced.recipe.clone();
    pending.model.model_hash = DENOISE_PENDING_MODEL_HASH.into();
    assert_eq!(
        denoise_stage_status(&pending, &current, &recorded, true),
        DenoiseStageStatus::Unavailable
    );
}
