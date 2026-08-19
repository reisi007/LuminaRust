use lumina_core::{ImageFrame, MaskPlane};
use lumina_onnx::{
    birefnet_manifest, preprocess_rgb_to_model, rescale_model_matte, Resolution, StubBackend,
    SubjectInference,
};

#[test]
fn rescale_rejects_inference_resolution_mismatch() {
    // model matte claims 4x4 but the manifest/inference resolution is 2x2
    let model = MaskPlane::new(4, 4, vec![0u16; 16]).unwrap();
    let err = rescale_model_matte(
        &model,
        Resolution {
            width: 2,
            height: 2,
        },
        (8, 8),
    )
    .unwrap_err();
    assert!(
        matches!(err, lumina_onnx::OnnxError::InvalidDimensions { .. }),
        "stub/ORT input vs. inference resolution mismatch must error, got {err:?}"
    );
}

#[test]
fn infer_rejects_zero_dimension_input() {
    let backend = StubBackend::new(birefnet_manifest()).unwrap();
    let img = ImageFrame::new(0, 0, vec![]).unwrap();
    let err = backend.infer(&img).unwrap_err();
    assert!(
        matches!(err, lumina_onnx::OnnxError::InvalidDimensions { .. }),
        "zero-area input must error, got {err:?}"
    );
}

#[test]
fn preprocess_dimensions_are_correct() {
    let img = ImageFrame::new(
        2,
        2,
        vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ],
    )
    .unwrap();
    let out = preprocess_rgb_to_model(
        &img,
        Resolution {
            width: 4,
            height: 4,
        },
    );
    assert_eq!(out.len(), 4 * 4 * 3);
}
