use lumina_onnx::{birefnet_manifest, ModelManifest};
use serde_json::json;

#[test]
fn manifest_json_roundtrip_preserves_identity() {
    let m = birefnet_manifest();
    let json = m.to_json().expect("serialize");
    let back: ModelManifest = ModelManifest::from_json(&json).expect("deserialize");
    assert_eq!(m, back);
}

#[test]
fn manifest_rejects_unknown_top_level_field() {
    let mut value = serde_json::to_value(birefnet_manifest()).unwrap();
    value["unknown_top_level"] = json!(1);
    let err = ModelManifest::from_json(&value.to_string()).unwrap_err();
    assert!(
        err.to_string().contains("unknown field"),
        "expected unknown-field error, got {err}"
    );
}
