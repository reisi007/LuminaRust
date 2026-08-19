use lumina_onnx::OnnxError;

/// `MissingModel` semantics are part of the public error contract: an absent
/// artifact must surface as `MissingModel`, never a silent fallback.
#[test]
fn missing_model_variant_semantics() {
    let e = OnnxError::MissingModel {
        path: "models/birefnet.onnx".into(),
    };
    assert!(matches!(e, OnnxError::MissingModel { .. }));
    assert!(
        e.to_string().contains("models/birefnet.onnx"),
        "error must name the missing artifact, got {e}"
    );
}

/// When the `onnx-rt` feature is enabled, the real backend reports an absent
/// artifact as `MissingModel` without any weights (no silent fallback).
#[cfg(feature = "onnx-rt")]
#[test]
fn ort_backend_reports_missing_model_artifact() {
    let backend = lumina_onnx::ort_backend::OrtBackend::new(
        "/nonexistent/path/to/model.onnx",
        lumina_onnx::birefnet_manifest(),
    );
    assert!(
        matches!(backend, Err(OnnxError::MissingModel { .. })),
        "absent artifact must surface as MissingModel"
    );
}
