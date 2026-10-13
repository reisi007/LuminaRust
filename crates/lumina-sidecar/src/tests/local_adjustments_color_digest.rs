//! MASK-LOCAL-P1.2b sidecar tests, part 2: the digest contribution of the
//! local colour block, history snapshots and the still-disabled stages.
//!
//! Split from `local_adjustments_color.rs` (file-size ratchet): that half owns
//! the typed block, the migration and the shared validators, this half owns the
//! identity, history and refusal contracts.

use super::local_adjustments::layer_with_legacy;
use super::local_adjustments_color::{color_layer, document_with_layer};
use super::*;
use serde_json::json;

/// The canonical digests must cover the whole colour block, otherwise a local
/// colour edit could reuse stale cached pixels.
#[test]
fn canonical_digests_cover_the_local_color_block() {
    let base = color_layer();
    let mut graded = base.clone();
    graded
        .local_adjustments
        .as_mut()
        .unwrap()
        .set_local_color_grading_field("shadows", "saturation", 0.4)
        .unwrap();
    let mut pointed = base.clone();
    pointed
        .local_adjustments
        .as_mut()
        .unwrap()
        .add_local_point_color_entry(30.0, 45.0, 0.2, 0.1, -0.1)
        .unwrap();
    let mut vibrance = base.clone();
    vibrance
        .local_adjustments
        .as_mut()
        .unwrap()
        .set_value("vibrance", 0.4)
        .unwrap();
    let mut band = base.clone();
    band.local_adjustments
        .as_mut()
        .unwrap()
        .set_local_hsl_band("blue", "saturation", 0.2)
        .unwrap();

    let layers = [&base, &graded, &pointed, &vibrance, &band];
    for (index, layer) in layers.iter().enumerate() {
        for other in layers.iter().skip(index + 1) {
            assert_ne!(
                layer.local_adjustments.as_ref().unwrap().digest(),
                other.local_adjustments.as_ref().unwrap().digest()
            );
            assert_ne!(
                mask_layers_digest(std::slice::from_ref(layer)),
                mask_layers_digest(std::slice::from_ref(other))
            );
            assert_ne!(
                MaskStateSnapshot::new(vec![(*layer).clone()]).digest(),
                MaskStateSnapshot::new(vec![(*other).clone()]).digest()
            );
        }
    }
    // Deterministic for identical state.
    assert_eq!(
        mask_layers_digest(&[graded.clone()]),
        mask_layers_digest(&[graded])
    );
}

/// The colour block rides in the layer, so an in-memory history snapshot and a
/// full sidecar reload both restore it.
#[test]
fn local_color_survives_history_snapshot_and_reload() {
    let mut layer = layer_with_legacy();
    layer.extras.clear();
    let mut local = LocalAdjustments::default();
    local.set_local_hsl_band("red", "hue", -0.25).unwrap();
    local
        .add_local_point_color_entry(30.0, 45.0, 0.2, 0.1, -0.1)
        .unwrap();
    local
        .set_local_color_grading_field("shadows", "saturation", 0.4)
        .unwrap();
    local.set_value("vibrance", 0.4).unwrap();
    layer.local_adjustments = Some(local);

    let document = document_with_layer(layer);
    let snapshot = MaskStateSnapshot::new(document.virtual_copies[0].mask_layers.clone());
    let mut document = document;
    let entry = {
        let mut entry = HistoryEntry {
            id: "history-local-color".into(),
            recipe: EditRecipe::default(),
            recorded_at: None,
            extras: Extras::new(),
        };
        entry.set_mask_state(snapshot.clone()).unwrap();
        entry
    };
    document.virtual_copies[0].history.push(entry);
    document.validate().unwrap();
    let json = document.to_json().unwrap();
    let reloaded = SidecarDocument::from_json(&json).unwrap();
    let restored = reloaded.virtual_copies[0].history[0]
        .mask_state()
        .unwrap()
        .unwrap();
    assert_eq!(
        restored.layers[0].local_adjustments,
        snapshot.layers[0].local_adjustments
    );
    assert!(restored.layers[0]
        .local_adjustments
        .as_ref()
        .unwrap()
        .has_local_color());
}

/// The still-disabled local stages stay rejected — the **one canonical pin** of
/// this family (audit TEST-AUDIT-36, M-A mutation „`set_value` akzeptiert
/// `detail`“): the scalar setter must refuse every disabled key, typed or
/// dotted. Consolidated here from the three representatives the audit found:
/// every non-duplicated statement of the presence/detail_state variants lives
/// below, with the same fail mechanism, so only this one remains.
#[test]
fn disabled_local_presence_detail_denoise_and_optics_stay_rejected() {
    // The scalar setter stays closed for every disabled stage: the flat
    // presence/clarity/optics/... keys and the detail sub-block names are the
    // typed setters' business only, and the dotted sub-block paths are refused
    // just like the plain ones.
    let mut local = LocalAdjustments::default();
    for key in [
        "presence",
        "texture",
        "clarity",
        "dehaze",
        "detail",
        "noise_reduction",
        "denoise_ai",
        "sharpening",
        "optics",
        "lens_correction",
        "luminance",
        "color",
        "radius",
        "sharpening.amount",
        "noise_reduction.luminance",
    ] {
        assert!(
            local.set_value(key, 0.1).is_err(),
            "{key} must not be a local adjustment key"
        );
    }
    // The serialized object has exactly the documented field set.
    let json = serde_json::to_value(&local).unwrap();
    let mut keys: Vec<&String> = json.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "contrast",
            "exposure",
            "highlights",
            "saturation",
            "shadows",
            "temperature_delta_k",
            "tint_delta",
            "version",
            "vibrance",
        ]
    );

    // The flattened legacy `adjustment_*` extras stay limited to the four P0
    // scalars: every other key is refused by the same loud validator the
    // migration uses, and the refusal names the key it refused.
    for key in [
        "adjustment_hsl",
        "adjustment_vibrance",
        "adjustment_presence",
        "adjustment_texture",
        "adjustment_clarity",
        "adjustment_dehaze",
        "adjustment_detail",
        "adjustment_sharpening",
        "adjustment_noise_reduction",
        "adjustment_denoise_ai",
        "adjustment_optics",
        "adjustment_luminance",
        "adjustment_masking",
        "adjustment_radius",
    ] {
        let mut legacy = layer_with_legacy();
        legacy.extras.clear();
        legacy.extras.insert(key.into(), json!(0.5));
        let error = validate_mask_layer_local_state(&legacy)
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
        let mut legacy = layer_with_legacy();
        legacy.extras.clear();
        legacy.extras.insert(key.into(), json!(0.5));
        validate_mask_layer_local_state(&legacy)
            .unwrap_or_else(|error| panic!("{key} is a legal P0 legacy key: {error}"));
    }
    // The typed detail block carries exactly the two enabled sub-blocks: no
    // disabled stage and no render-scale field may ride in it.
    let block = serde_json::to_value(neutral_local_detail()).unwrap();
    let mut block_keys: Vec<String> = block.as_object().unwrap().keys().cloned().collect();
    block_keys.sort();
    assert_eq!(
        block_keys,
        vec!["noise_reduction".to_string(), "sharpening".to_string()]
    );
    for forbidden in [
        "denoise_ai",
        "optics",
        "lens_correction",
        "scale",
        "render_scale",
    ] {
        assert!(
            !block_keys.iter().any(|key| key == forbidden),
            "{forbidden}"
        );
    }
    // The wire decoder refuses a disabled field outright, so no surface can
    // smuggle one in — not the disabled stages and not a scale field.
    for disabled in ["optics", "denoise_ai", "lens_correction", "scale"] {
        let mut layer = layer_with_legacy();
        layer.extras.clear();
        layer.local_adjustments = Some(LocalAdjustments::default());
        let document = document_with_layer(layer);
        let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
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
