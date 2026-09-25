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

/// The legacy `adjustment_*` extras still accept only the four P0 keys, and the
/// local presence/detail/AI-denoise/optics controls stay rejected.
#[test]
fn disabled_local_presence_detail_denoise_and_optics_stay_rejected() {
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

    // And the legacy extras path still refuses a colour or detail key.
    for (key, value) in [
        ("adjustment_hsl", json!({"version": 1})),
        ("adjustment_vibrance", json!(0.5)),
        ("adjustment_presence", json!(0.5)),
        ("adjustment_detail", json!(0.5)),
    ] {
        let mut legacy = layer_with_legacy();
        legacy.local_adjustments = None;
        legacy.extras.insert(key.into(), value);
        let document = document_with_layer(legacy);
        let error = document.to_json().unwrap_err().to_string();
        assert!(error.contains("unknown local adjustment"), "{key}: {error}");
    }
}
