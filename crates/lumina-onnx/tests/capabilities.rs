use lumina_onnx::{ModelCapabilities, ModelManifest};
use serde_json::json;

#[test]
fn no_capabilities_set_is_rejected_on_manifest() {
    let json = json!({
        "model_name": "X",
        "model_version": "1",
        "model_hash": "h",
        "license": "MIT",
        "input": {
            "resolution": {"width": 4, "height": 4},
            "channel_layout": "rgb",
            "tensor_name": "input",
            "tensor_format": "nchw"
        },
        "capabilities": {
            "subject_segmentation": false,
            "box_prompt": false,
            "point_prompt": false,
            "mask_prompt": false,
            "class_detection": false,
            "instance_segmentation": false
        }
    })
    .to_string();
    let err = ModelManifest::from_json(&json).unwrap_err();
    assert!(
        matches!(err, lumina_onnx::OnnxError::UnsupportedModel { .. }),
        "expected UnsupportedModel, got {err:?}"
    );
}

#[test]
fn unknown_capability_field_is_rejected() {
    let json = json!({
        "subject_segmentation": true,
        "box_prompt": false,
        "point_prompt": false,
        "mask_prompt": false,
        "class_detection": false,
        "instance_segmentation": false,
        "future_capability": true
    })
    .to_string();
    let err = serde_json::from_str::<ModelCapabilities>(&json).unwrap_err();
    assert!(
        err.to_string().contains("unknown field"),
        "expected unknown-field error, got {err}"
    );
}

#[test]
fn capabilities_validate_directly() {
    assert!(ModelCapabilities::default().validate().is_err());
    let ok = ModelCapabilities {
        subject_segmentation: true,
        ..Default::default()
    };
    assert!(ok.validate().is_ok());
}
