//! MASK-LOCAL-P1.2d sidecar *state* tests for the local detail block: the
//! canonical digests, the history/previous snapshot and the still-disabled local
//! AI-denoise / optics stages.
//!
//! Split from `local_adjustments_detail.rs` (file-size ratchet): that file owns
//! the schema and the migration, this one owns the identity contracts.

use super::local_adjustments::layer_with_legacy;
use super::local_adjustments_color::document_with_layer;
use super::local_adjustments_detail::sharpened_layer;
use super::*;
use serde_json::json;

/// The canonical digests and the history/previous snapshot carry the complete
/// detail block.
#[test]
fn canonical_digests_cover_the_local_detail_block() {
    let mut sharpen_only = LocalAdjustments::default();
    sharpen_only
        .set_local_sharpening_field("amount", 1.0)
        .unwrap();
    let mut noise_only = LocalAdjustments::default();
    noise_only
        .set_local_noise_reduction_field("luminance", 0.3)
        .unwrap();
    let mut both = LocalAdjustments::default();
    both.set_local_sharpening_field("amount", 1.0).unwrap();
    both.set_local_noise_reduction_field("luminance", 0.3)
        .unwrap();
    let digests = [sharpen_only.digest(), noise_only.digest(), both.digest()];
    // All three are distinct: neither sub-block may be dropped from the identity.
    assert_ne!(digests[0], digests[1]);
    assert_ne!(digests[0], digests[2]);
    assert_ne!(digests[1], digests[2]);

    let layer_of = |local: LocalAdjustments| {
        let mut layer = layer_with_legacy();
        layer.extras.clear();
        layer.local_adjustments = Some(local);
        layer.normalize_local_adjustments().unwrap();
        layer
    };
    let single = [
        vec![layer_of(sharpen_only)],
        vec![layer_of(noise_only)],
        vec![layer_of(both.clone())],
    ];
    let digests: Vec<String> = single
        .iter()
        .map(|layers| mask_layers_digest(layers))
        .collect();
    assert_ne!(digests[0], digests[1]);
    assert_ne!(digests[0], digests[2]);
    assert_ne!(digests[1], digests[2]);
    // Reordering the persisted layer list is a change of the state identity.
    let mut reordered = single[2].clone();
    let extra = layer_of(LocalAdjustments::default());
    reordered.push(extra);
    assert_ne!(mask_layers_digest(&reordered), digests[2]);

    // A history snapshot carries the block verbatim.
    let snapshot = MaskStateSnapshot::new(vec![layer_of(both.clone())]);
    assert!(snapshot.validate().is_ok());
    let restored = snapshot.layers[0]
        .local_adjustments
        .as_ref()
        .expect("typed local recipe");
    assert_eq!(restored.detail, both.detail);
    assert!(restored.has_local_detail());
    // A legacy snapshot version migrates forward and keeps the block.
    let mut legacy: Value =
        serde_json::from_value(serde_json::to_value(&snapshot).unwrap()).expect("serializable");
    legacy["version"] = json!(LOCAL_ADJUSTMENTS_VERSION);
    let loaded: MaskStateSnapshot = serde_json::from_value(legacy).unwrap();
    assert_eq!(loaded.version, LOCAL_ADJUSTMENTS_VERSION);
    assert!(loaded.layers[0]
        .local_adjustments
        .as_ref()
        .expect("typed local recipe")
        .has_local_detail());
}

/// The still-disabled local AI-denoise and optics have no field, no key and no
/// renderer stub; the legacy flattened `adjustment_*` extras stay limited to the
/// four P0 scalars.
#[test]
fn disabled_local_denoise_and_optics_stay_rejected_and_the_legacy_keys_stay_p0() {
    // `adjustment_detail`, `adjustment_sharpening` and `adjustment_noise_reduction`
    // are not legacy keys: the flattened extras stay limited to the four P0
    // scalars.
    for key in [
        "adjustment_detail",
        "adjustment_sharpening",
        "adjustment_noise_reduction",
        "adjustment_luminance",
        "adjustment_masking",
        "adjustment_denoise_ai",
        "adjustment_optics",
        "adjustment_radius",
    ] {
        let mut layer = layer_with_legacy();
        layer.extras.clear();
        layer.extras.insert(key.into(), json!(0.5));
        let error = validate_mask_layer_local_state(&layer)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown local adjustment"), "{key}: {error}");
        assert!(
            error.contains(&key["adjustment_".len()..]),
            "{key}: {error}"
        );
    }
    // And the four P0 legacy keys still migrate.
    for key in [
        "adjustment_exposure",
        "adjustment_contrast",
        "adjustment_highlights",
        "adjustment_shadows",
    ] {
        let mut layer = layer_with_legacy();
        layer.extras.clear();
        layer.extras.insert(key.into(), json!(0.5));
        validate_mask_layer_local_state(&layer)
            .unwrap_or_else(|error| panic!("{key} is a legal P0 legacy key: {error}"));
    }
    // The typed block has no AI-denoise or optics field, and no scale field.
    let json = serde_json::to_value(neutral_local_detail()).unwrap();
    let keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
    // The persisted sub-block key order follows the struct declaration order.
    let mut expected = vec!["sharpening".to_string(), "noise_reduction".to_string()];
    expected.sort();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(sorted, expected);
    for forbidden in [
        "denoise_ai",
        "optics",
        "lens_correction",
        "scale",
        "render_scale",
    ] {
        assert!(!keys.iter().any(|key| key == forbidden), "{forbidden}");
    }
    // The wire decoder refuses a disabled field outright, so no surface can
    // smuggle one in.
    for disabled in ["optics", "denoise_ai", "lens_correction", "scale"] {
        let mut value: Value =
            serde_json::from_str(&document_with_layer(sharpened_layer()).to_json().unwrap())
                .unwrap();
        value["virtual_copies"][0]["mask_layers"][0]["local_adjustments"][disabled] = json!(0.5);
        let error = SidecarDocument::from_json(&serde_json::to_string(&value).unwrap())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("unknown field"),
            "`{disabled}` must be refused by the wire shape, got: {error}"
        );
    }
}
