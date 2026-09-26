//! MASK-LOCAL-P1.2a sidecar tests: the typed local tone-curve block, its
//! lossless migration, the loud refusals, and its digest contribution.

use super::local_adjustments::layer_with_legacy;
use super::*;
use crate::tests::support::{mask, source};
use serde_json::json;

fn point(input: f32, output: f32) -> CurvePoint {
    CurvePoint { input, output }
}

fn lifted_master() -> CurvePoints {
    vec![point(0.0, 0.0), point(0.5, 0.7), point(1.0, 1.0)]
}

fn document_with_curves(curves: Option<Curves>) -> SidecarDocument {
    let mut layer = layer_with_legacy();
    layer.extras.clear();
    layer.local_adjustments = Some(LocalAdjustments {
        curves,
        ..LocalAdjustments::default()
    });
    let mut document = SidecarDocument::new(source(), "pipeline-1");
    document.virtual_copies[0].mask_library.push(mask("m"));
    document.virtual_copies[0].mask_layers.push(layer);
    document
}

fn stored_local(document: &SidecarDocument) -> LocalAdjustments {
    document.virtual_copies[0].mask_layers[0]
        .effective_local_adjustments()
        .unwrap()
        .expect("typed local recipe")
}

/// A v3 payload with a master curve and one channel curve round-trips through
/// the sidecar file without loss and stays a v3 object.
#[test]
fn local_curve_round_trips_through_the_sidecar_file() {
    let mut curves = Curves::identity();
    curves.master = lifted_master();
    curves.channels.green = Some(vec![point(0.0, 0.0), point(0.25, 0.1), point(1.0, 1.0)]);
    let document = document_with_curves(Some(curves.clone()));
    let json = document.to_json().unwrap();
    let loaded = SidecarDocument::from_json(&json).unwrap();
    let local = stored_local(&loaded);
    assert_eq!(local.version, LOCAL_ADJUSTMENTS_VERSION);
    assert_eq!(local.curves.as_ref().unwrap(), &curves);
    assert!(local.has_local_curves());
    // Byte-stable: a second write does not drift.
    assert_eq!(loaded.to_json().unwrap(), json);
    // The stable CLI status line names the stored channels.
    assert_eq!(local.curve_summary(), "master:3,green:3");
    // The P1.2b colour fields follow the curve summary in the status line.
    assert!(
        local
            .to_string()
            .contains("curves=master:3,green:3 hsl=none point_color=none color_grading=none"),
        "{}",
        local
    );
}

/// A v1 and a v2 payload migrate forward without loss; the curve namespace of
/// an older version is a loud error, never a silent drop.
#[test]
fn legacy_versions_migrate_losslessly_and_refuse_a_smuggled_curve() {
    for (version, expected_delta) in [(1u64, 0.0f64), (2, -900.0f64)] {
        let mut value: Value =
            serde_json::from_str(&document_with_curves(None).to_json().unwrap()).unwrap();
        let mut payload = json!({
            "version": version,
            "exposure": 0.5,
            "contrast": 0.0,
            "highlights": 0.0,
            "shadows": 0.0,
        });
        if version == 2 {
            payload["temperature_delta_k"] = json!(-900);
        }
        value["virtual_copies"][0]["mask_layers"][0]["local_adjustments"] = payload;
        let loaded = SidecarDocument::from_json(&serde_json::to_string(&value).unwrap()).unwrap();
        let local = stored_local(&loaded);
        assert_eq!(local.version, LOCAL_ADJUSTMENTS_VERSION);
        assert_eq!(local.exposure, 0.5);
        assert_eq!(local.temperature_delta_k, expected_delta);
        assert!(
            local.curves.is_none(),
            "v{version} must migrate to no curve"
        );

        // A curve in an older version is a loud refusal, not a silent drop.
        value["virtual_copies"][0]["mask_layers"][0]["local_adjustments"]["curves"] = json!({
            "version": 1,
            "master": [{"input": 0.0, "output": 0.0}, {"input": 0.5, "output": 0.7}, {"input": 1.0, "output": 1.0}],
        });
        let error = SidecarDocument::from_json(&serde_json::to_string(&value).unwrap())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("cannot contain a tone curve"),
            "v{version} smuggling must be loud, got: {error}"
        );
    }
}

/// Local curves reuse the *global* point rules. Anything the global validator
/// rejects must be rejected here too, and the block version is checked.
#[test]
fn local_curve_points_use_the_existing_global_ranges() {
    let mut local = LocalAdjustments::default();
    assert!(local
        .set_local_curve_channel("master", lifted_master())
        .is_ok());
    assert!(local
        .set_local_curve_channel("red", lifted_master())
        .is_ok());

    for (name, points) in [
        ("too few", vec![point(0.0, 0.0)]),
        (
            "non-ascending",
            vec![
                point(0.0, 0.0),
                point(0.7, 0.7),
                point(0.4, 0.4),
                point(1.0, 1.0),
            ],
        ),
        (
            "out of range",
            vec![point(0.0, 0.0), point(0.5, 1.5), point(1.0, 1.0)],
        ),
        (
            "missing first endpoint",
            vec![point(0.1, 0.1), point(1.0, 1.0)],
        ),
        (
            "missing last endpoint",
            vec![point(0.0, 0.0), point(0.9, 0.9)],
        ),
        ("empty", vec![]),
    ] {
        let before = local.curves.clone();
        let error = local
            .set_local_curve_channel("master", points.clone())
            .expect_err("invalid local curve points must be refused");
        assert!(error.contains("local tone curve"), "{name}: {error}");
        // A refused point list leaves the layer byte-for-byte unchanged.
        assert_eq!(local.curves, before, "{name} must not mutate the recipe");
    }
    assert!(local
        .set_local_curve_channel("luma", lifted_master())
        .unwrap_err()
        .contains("unknown local curve channel"));

    // The block version is the global one.
    let mut wrong_version = local.clone();
    wrong_version.curves.as_mut().unwrap().version = 2;
    assert!(wrong_version
        .validate()
        .unwrap_err()
        .to_string()
        .contains("unsupported curves version"));

    // The identical list is accepted by the global recipe as well, so the two
    // surfaces cannot disagree about what a valid curve is.
    let mut global_curves = Curves::identity();
    global_curves.master = lifted_master();
    validate_curves(&global_curves).expect("the global validator accepts the same list");
}

/// `None`, a persisted identity and a default object are all neutral; a real
/// curve is not. Resetting a channel drops the block entirely once every
/// channel is identity again, so a reset is byte-identical to "never edited".
#[test]
fn neutral_absent_and_identity_curves_read_the_same_and_reset_clears() {
    let mut local = LocalAdjustments::default();
    assert!(local.is_neutral());
    assert!(!local.has_local_curves());
    assert_eq!(local.curve_summary(), "none");

    let mut identity = LocalAdjustments {
        curves: Some(Curves::identity()),
        ..LocalAdjustments::default()
    };
    assert!(identity.is_neutral());
    assert!(!identity.has_local_curves());
    // A per-channel identity is neutral too.
    identity
        .set_local_curve_channel("red", identity_curve_points())
        .unwrap();
    assert!(identity.is_neutral());

    let digest_absent = LocalAdjustments::default().digest();
    let digest_identity = LocalAdjustments {
        curves: Some(Curves::identity()),
        ..LocalAdjustments::default()
    }
    .digest();
    // The stored *form* is part of the identity even when the pixels agree.
    assert_ne!(digest_absent, digest_identity);

    local
        .set_local_curve_channel("master", lifted_master())
        .unwrap();
    local
        .set_local_curve_channel("blue", lifted_master())
        .unwrap();
    assert!(!local.is_neutral());
    assert_eq!(local.curve_summary(), "master:3,blue:3");
    // Resetting one channel keeps the other.
    local.reset_local_curve_channel("blue").unwrap();
    assert!(local.curves.as_ref().unwrap().channels.blue.is_none());
    assert!(local.has_local_curves());
    // Resetting the last non-identity channel drops the whole block, so the
    // reset is byte-identical to a layer that was never edited.
    local.reset_local_curve_channel("master").unwrap();
    assert!(local.curves.is_none());
    assert_eq!(local.curve_summary(), "none");
    local.reset_local_curves();
    assert!(local.curves.is_none());
    // Resetting an absent channel is a no-op, not an error.
    local.reset_local_curve_channel("red").unwrap();
    assert!(local
        .reset_local_curve_channel("luma")
        .unwrap_err()
        .contains("unknown local curve channel"));
}

/// The canonical digests must cover the curve block, otherwise a local curve
/// edit could reuse stale cached pixels.
#[test]
fn canonical_digests_cover_the_local_curve_block() {
    let mut base = layer_with_legacy();
    base.extras.clear();
    base.local_adjustments = Some(LocalAdjustments::default());
    let mut curved = base.clone();
    curved
        .local_adjustments
        .as_mut()
        .unwrap()
        .set_local_curve_channel("master", lifted_master())
        .unwrap();
    let mut red = curved.clone();
    red.local_adjustments
        .as_mut()
        .unwrap()
        .set_local_curve_channel("red", lifted_master())
        .unwrap();

    assert_ne!(
        base.local_adjustments.as_ref().unwrap().digest(),
        curved.local_adjustments.as_ref().unwrap().digest()
    );
    assert_ne!(
        mask_layers_digest(&[base.clone()]),
        mask_layers_digest(&[curved.clone()])
    );
    assert_ne!(
        mask_layers_digest(&[curved.clone()]),
        mask_layers_digest(&[red.clone()])
    );
    assert_ne!(
        MaskStateSnapshot::new(vec![curved.clone()]).digest(),
        MaskStateSnapshot::new(vec![red.clone()]).digest()
    );
    // Deterministic for identical state.
    assert_eq!(
        mask_layers_digest(&[curved.clone()]),
        mask_layers_digest(&[curved])
    );
}

/// The local curve rides in the layer, so an in-memory history snapshot and a
/// full sidecar reload both restore it.
#[test]
fn local_curve_survives_history_snapshot_and_reload() {
    let mut curves = Curves::identity();
    curves.master = lifted_master();
    let document = document_with_curves(Some(curves));
    let layers = document.virtual_copies[0].mask_layers.clone();
    let snapshot = MaskStateSnapshot::new(layers.clone());
    let mut document = document;
    let entry = {
        let mut entry = HistoryEntry {
            id: "history-local-curve".into(),
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
        .has_local_curves());
}

/// A legacy `adjustment_curves` extra stays an *unknown* local adjustment, and
/// the four P0 keys still migrate losslessly.
#[test]
fn legacy_extras_still_accept_only_the_four_p0_keys() {
    // A legacy-only layer: no typed object, so the unknown key is what the
    // migration has to reject.
    let mut legacy = layer_with_legacy();
    legacy.local_adjustments = None;
    legacy
        .extras
        .insert("adjustment_curves".into(), json!("0,0;1,1"));
    let mut document = SidecarDocument::new(source(), "pipeline-1");
    document.virtual_copies[0].mask_library.push(mask("m"));
    document.virtual_copies[0].mask_layers.push(legacy);
    let error = document.to_json().unwrap_err().to_string();
    assert!(error.contains("unknown local adjustment"), "{error}");

    let mut legacy = layer_with_legacy();
    legacy.local_adjustments = None;
    legacy
        .extras
        .insert("adjustment_contrast".into(), json!(0.2));
    let mut document = SidecarDocument::new(source(), "pipeline-1");
    document.virtual_copies[0].mask_library.push(mask("m"));
    document.virtual_copies[0].mask_layers.push(legacy);
    let loaded = SidecarDocument::from_json(&document.to_json().unwrap()).unwrap();
    let local = stored_local(&loaded);
    assert_eq!(local.version, LOCAL_ADJUSTMENTS_VERSION);
    assert_eq!(local.exposure, 1.25);
    assert_eq!(local.shadows, -0.4);
    assert_eq!(local.contrast, 0.2);
    assert!(local.curves.is_none());
}

/// The local tone curve itself is still addressed only through the typed
/// `curves` block: a curve is not a scalar key, and the P1.2b colour controls
/// are not curve controls. The detail/optics stages stay disabled. (Local
/// presence is a P1.2c *typed block*, not a scalar key, so `presence` is still
/// rejected here — for the same reason `curves` is.)
#[test]
fn curves_are_not_a_scalar_key_and_detail_stages_stay_rejected() {
    let mut local = LocalAdjustments::default();
    for key in ["curves", "curve_points", "presence", "detail", "optics"] {
        assert!(
            local.set_value(key, 0.1).is_err(),
            "{key} must not be a local adjustment key"
        );
    }
    // A default object has no optional block at all: neither the curve nor the
    // P1.2b colour areas are written.
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
}
