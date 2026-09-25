use super::*;
use crate::tests::support::{mask, source};
use serde_json::json;

pub(super) fn layer_with_legacy() -> MaskLayer {
    let mut extras = Extras::new();
    extras.insert("adjustment_exposure".into(), json!(1.25));
    extras.insert("adjustment_shadows".into(), json!(-0.4));
    MaskLayer {
        id: "layer-local".into(),
        mask: MaskReference {
            copy_id: "vc-original".into(),
            mask_id: "m".into(),
            extras: Extras::new(),
        },
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        visible: true,
        local_adjustments: None,
        extras,
    }
}

fn document_with_layer(layer: MaskLayer) -> SidecarDocument {
    let mut document = SidecarDocument::new(source(), "pipeline-1");
    document.virtual_copies[0].mask_library.push(mask("m"));
    document.virtual_copies[0].mask_layers.push(layer);
    document
}

#[test]
fn legacy_adjustment_extras_migrate_without_loss_and_roundtrip() {
    let document = document_with_layer(layer_with_legacy());
    let json = document.to_json().unwrap();
    assert!(!json.contains("adjustment_exposure"));
    assert!(json.contains("local_adjustments"));

    let loaded = SidecarDocument::from_json(&json).unwrap();
    let layer = &loaded.virtual_copies[0].mask_layers[0];
    let local = layer.effective_local_adjustments().unwrap().unwrap();
    assert_eq!(local.version, LOCAL_ADJUSTMENTS_VERSION);
    assert_eq!(local.exposure, 1.25);
    assert_eq!(local.shadows, -0.4);
    assert_eq!(local.contrast, 0.0);
    assert!(!layer
        .extras
        .keys()
        .any(|key| key.starts_with("adjustment_")));
    assert_eq!(loaded.to_json().unwrap(), json);
}

#[test]
fn typed_v1_migrates_to_the_current_version_with_neutral_deltas_and_no_curve() {
    let mut typed_layer = layer_with_legacy();
    typed_layer.local_adjustments = None;
    typed_layer.extras.clear();
    let document = document_with_layer(typed_layer);
    let mut value: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
    value["virtual_copies"][0]["mask_layers"][0]["local_adjustments"] = json!({
        "version": 1,
        "exposure": 0.75,
        "contrast": -0.25,
        "highlights": 0.0,
        "shadows": 0.5
    });
    let loaded = SidecarDocument::from_json(&serde_json::to_string(&value).unwrap()).unwrap();
    let local = loaded.virtual_copies[0].mask_layers[0]
        .effective_local_adjustments()
        .unwrap()
        .unwrap();
    assert_eq!(local.version, LOCAL_ADJUSTMENTS_VERSION);
    assert_eq!(local.exposure, 0.75);
    assert_eq!(local.shadows, 0.5);
    assert_eq!(local.temperature_delta_k, 0.0);
    assert_eq!(local.tint_delta, 0.0);
    // MASK-LOCAL-P1.2a: a v1 payload cannot express a curve, so the
    // lossless migration is `curves: None` and no new key is written.
    assert!(local.curves.is_none());
    let json = loaded.to_json().unwrap();
    assert!(json.contains(&format!("\"version\": {LOCAL_ADJUSTMENTS_VERSION}")));
    assert!(json.contains("temperature_delta_k"));
    assert!(!json.contains("\"curves\""));
}

#[test]
fn relative_wb_fields_are_relative_only_and_finite_range_checked() {
    let mut local = LocalAdjustments::default();
    assert!(local.set_value("temperature_delta_k", 5000.0).is_ok());
    assert!(local.set_value("tint_delta", -1.0).is_ok());
    assert!(local.set_value("wb_temperature", 6500.0).is_err());
    assert!(local.set_value("wb_tint", 0.0).is_err());
    assert!(local.set_value("temperature_delta_k", 5000.1).is_err());
    assert!(local.set_value("tint_delta", f64::NAN).is_err());

    let tint_only: LocalAdjustments = serde_json::from_value(json!({
        "version": LOCAL_ADJUSTMENTS_VERSION,
        "tint_delta": 0.3
    }))
    .unwrap();
    assert_eq!(tint_only.temperature_delta_k, 0.0);
    assert_eq!(tint_only.tint_delta, 0.3);
    let temperature_only: LocalAdjustments = serde_json::from_value(json!({
        "version": LOCAL_ADJUSTMENTS_VERSION,
        "temperature_delta_k": -900
    }))
    .unwrap();
    assert_eq!(temperature_only.temperature_delta_k, -900.0);
    assert_eq!(temperature_only.tint_delta, 0.0);

    let mut value = serde_json::to_value(local).unwrap();
    value["temperature_delta_k"] = json!(f64::INFINITY);
    assert!(serde_json::from_value::<LocalAdjustments>(value).is_err());
}

#[test]
fn explicit_non_number_wb_deltas_are_loud_never_a_silent_zero() {
    // An absent delta is the neutral zero. Every *explicit* non-number — JSON
    // `null`, a string, and the former internal object sentinel — must stay a
    // loud parse error instead of coercing to `0.0`.
    for sentinel in [
        json!({ "__lumina_missing_local_delta__": true }),
        json!(true),
        json!("0.5"),
        json!([0.0]),
        Value::Null,
    ] {
        let decoded: Result<LocalAdjustments, _> = serde_json::from_value(json!({
            "version": LOCAL_ADJUSTMENTS_VERSION,
            "temperature_delta_k": sentinel.clone(),
        }));
        assert!(
            decoded.is_err(),
            "explicit non-number must be rejected: {sentinel}"
        );
        let decoded_tint: Result<LocalAdjustments, _> = serde_json::from_value(json!({
            "version": LOCAL_ADJUSTMENTS_VERSION,
            "tint_delta": sentinel,
        }));
        assert!(decoded_tint.is_err(), "tint_delta must reject it too");
    }

    // The absent-field case still migrates to the neutral zero.
    let absent: LocalAdjustments = serde_json::from_value(json!({
        "version": LOCAL_ADJUSTMENTS_VERSION,
    }))
    .unwrap();
    assert_eq!(absent.temperature_delta_k, 0.0);
    assert_eq!(absent.tint_delta, 0.0);

    // A v1 payload must not smuggle either delta field in.
    let smuggled: Result<LocalAdjustments, _> = serde_json::from_value(json!({
        "version": LEGACY_LOCAL_ADJUSTMENTS_VERSION,
        "tint_delta": 0.25,
    }));
    assert!(smuggled.is_err(), "v1 must not accept a v2 delta field");
}

#[test]
fn typed_legacy_conflict_unknown_key_and_invalid_range_are_loud() {
    let base = document_with_layer(layer_with_legacy());
    let mut value: Value = serde_json::from_str(&base.to_json().unwrap()).unwrap();
    let layer = &mut value["virtual_copies"][0]["mask_layers"][0];
    layer["local_adjustments"] = json!({
        "version": 1,
        "exposure": 1.25,
        "contrast": 0.0,
        "highlights": 0.0,
        "shadows": -0.4
    });
    layer["adjustment_exposure"] = json!(1.25);
    let error = SidecarDocument::from_json(&serde_json::to_string(&value).unwrap())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("conflict"),
        "unexpected conflict error: {error}"
    );

    let mut unknown = document_with_layer(layer_with_legacy());
    unknown.virtual_copies[0].mask_layers[0]
        .extras
        .insert("adjustment_wb_temperature".into(), json!(6500));
    let unknown_error = unknown.to_json().unwrap_err().to_string();
    assert!(
        unknown_error.contains("unknown local adjustment"),
        "unexpected error: {unknown_error}"
    );

    let mut invalid = document_with_layer(layer_with_legacy());
    invalid.normalize_legacy_local_adjustments().unwrap();
    invalid.virtual_copies[0].mask_layers[0]
        .local_adjustments
        .as_mut()
        .unwrap()
        .exposure = 10.01;
    assert!(invalid
        .to_json()
        .unwrap_err()
        .to_string()
        .contains("local adjustment"));

    let mut version = document_with_layer(layer_with_legacy());
    version.normalize_legacy_local_adjustments().unwrap();
    version.virtual_copies[0].mask_layers[0]
        .local_adjustments
        .as_mut()
        .unwrap()
        .version = 4;
    assert!(version
        .to_json()
        .unwrap_err()
        .to_string()
        .contains("version"));
}

#[test]
fn document_normalization_is_atomic_when_a_later_layer_is_invalid() {
    let mut document = document_with_layer(layer_with_legacy());
    let mut invalid = layer_with_legacy();
    invalid.id = "layer-invalid".into();
    invalid
        .extras
        .insert("adjustment_contrast".into(), json!(2.0));
    document.virtual_copies[0].mask_layers.push(invalid);
    let before = serde_json::to_value(&document).unwrap();

    let error = document
        .normalize_legacy_local_adjustments()
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("local adjustment"),
        "unexpected error: {error}"
    );
    assert_eq!(serde_json::to_value(&document).unwrap(), before);
}

#[test]
fn canonical_mask_digest_covers_typed_values_and_persisted_order() {
    let mut first = layer_with_legacy();
    first.local_adjustments = Some(LocalAdjustments {
        exposure: 1.0,
        ..LocalAdjustments::default()
    });
    first.extras.clear();
    let mut second = first.clone();
    second.id = "layer-other".into();
    second.local_adjustments.as_mut().unwrap().contrast = 0.25;
    second
        .local_adjustments
        .as_mut()
        .unwrap()
        .temperature_delta_k = 250.0;

    let a = mask_layers_dcore(&[first.clone()]);
    let b = mask_layers_dcore(&[second.clone()]);
    assert_ne!(a, b);
    assert_eq!(a, mask_layers_dcore(&[first.clone()]));
    assert_ne!(
        mask_layers_dcore(&[first.clone(), second.clone()]),
        mask_layers_dcore(&[second.clone(), first.clone()])
    );
    let snapshot_a = MaskStateSnapshot::new(vec![first.clone()]);
    let snapshot_b = MaskStateSnapshot::new(vec![second]);
    assert_ne!(snapshot_a.digest(), snapshot_b.digest());
}

fn mask_layers_dcore(layers: &[MaskLayer]) -> String {
    mask_layers_digest(layers)
}

#[test]
fn history_mask_state_snapshot_is_additive_and_validated() {
    let layer = layer_with_legacy();
    let mut entry = HistoryEntry {
        id: "history-local".into(),
        recipe: EditRecipe::default(),
        recorded_at: None,
        extras: Extras::new(),
    };
    assert!(entry.mask_state().unwrap().is_none());
    entry
        .set_mask_state(MaskStateSnapshot::new(vec![layer.clone()]))
        .unwrap();
    let value = entry.extras[HISTORY_MASK_STATE_KEY].clone();
    let restored: MaskStateSnapshot = serde_json::from_value(value).unwrap();
    assert_eq!(restored.layers.len(), 1);
    assert_eq!(restored.layers[0].id, layer.id);

    let mut document = document_with_layer(MaskLayer {
        id: "layer-local".into(),
        mask: layer.mask,
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        visible: true,
        local_adjustments: None,
        extras: Extras::new(),
    });
    document.virtual_copies[0].history.push(entry);
    assert!(document.validate().is_ok());
}
