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

/// R2-ONNX-05 — a model *reporting itself unavailable* (availability flag) is
/// a distinct public error variant with no artifact-path wording, so it cannot
/// be confused with a genuinely missing `.onnx` file.
#[test]
fn model_unavailable_variant_semantics() {
    let e = OnnxError::ModelUnavailable {
        name: "BiRefNet".into(),
    };
    assert!(matches!(e, OnnxError::ModelUnavailable { .. }));
    let text = e.to_string();
    assert!(
        text.contains("BiRefNet") && text.contains("reported unavailable"),
        "error must name the model and carry availability wording, got {text}"
    );
    assert!(
        !text.contains("artifact"),
        "the flag-based case must not suggest an artifact path, got {text}"
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
