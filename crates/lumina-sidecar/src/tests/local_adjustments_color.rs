//! MASK-LOCAL-P1.2b sidecar tests: the typed local colour block, its lossless
//! migration from v1/v2/v3, the loud refusals, and its digest contribution.

use super::local_adjustments::layer_with_legacy;
use super::*;
use crate::tests::support::{mask, source};
use serde_json::json;

/// A band block with one non-neutral shift in `red`.
fn red_hue() -> HslAdjustments {
    HslAdjustments {
        version: 1,
        red: Some(HslChannel {
            hue: -0.25,
            ..HslChannel::default()
        }),
        ..HslAdjustments::default()
    }
}

/// A grading block that tints the shadows teal and lifts the midtones.
fn shadows_teal() -> ColorGrading {
    ColorGrading {
        shadows: ColorGradingRange {
            hue_degrees: 180.0,
            saturation: 0.4,
            luminance: 0.0,
        },
        midtones: ColorGradingRange {
            hue_degrees: 0.0,
            saturation: 0.0,
            luminance: 0.15,
        },
        highlights: ColorGradingRange::neutral(),
        balance: -0.3,
        blending: 0.7,
        version: 1,
    }
}

/// A point-colour block with one entry.
fn one_point_color() -> PointColor {
    PointColor {
        version: 1,
        entries: vec![local_point_color_entry(
            "pc-1".into(),
            30.0,
            45.0,
            0.2,
            0.1,
            -0.1,
        )],
    }
}

/// A layer that carries exactly one typed local colour edit.
pub(super) fn color_layer() -> MaskLayer {
    let mut layer = layer_with_legacy();
    layer.extras.clear();
    let mut local = LocalAdjustments::default();
    local
        .set_local_hsl_band("red", "hue", -0.25)
        .expect("red hue is valid");
    layer.local_adjustments = Some(local);
    layer
}

pub(super) fn document_with_layer(layer: MaskLayer) -> SidecarDocument {
    let mut document = SidecarDocument::new(source(), "pipeline-1");
    document.virtual_copies[0].mask_library.push(mask("m"));
    document.virtual_copies[0].mask_layers.push(layer);
    document
}

pub(super) fn stored_local(document: &SidecarDocument) -> LocalAdjustments {
    document.virtual_copies[0].mask_layers[0]
        .effective_local_adjustments()
        .unwrap()
        .expect("typed local recipe")
}

/// A v4 payload with all four colour areas round-trips through the sidecar
/// file without loss and stays a v4 object.
#[test]
fn local_color_round_trips_through_the_sidecar_file() {
    let mut layer = layer_with_legacy();
    layer.extras.clear();
    let mut local = LocalAdjustments::default();
    local
        .set_local_hsl_band("red", "hue", -0.25)
        .expect("red hue");
    local.set_value("vibrance", 0.4).expect("vibrance");
    local.set_value("saturation", -0.2).expect("saturation");
    local.hsl = Some(red_hue());
    local.point_color = Some(one_point_color());
    local.color_grading = Some(shadows_teal());
    layer.local_adjustments = Some(local);
    let document = document_with_layer(layer);

    let json = document.to_json().unwrap();
    let loaded = SidecarDocument::from_json(&json).unwrap();
    let stored = stored_local(&loaded);
    assert_eq!(stored.version, LOCAL_ADJUSTMENTS_VERSION);
    assert_eq!(stored.hsl, Some(red_hue()));
    assert_eq!(stored.point_color, Some(one_point_color()));
    assert_eq!(stored.color_grading, Some(shadows_teal()));
    assert_eq!(stored.vibrance, 0.4);
    assert_eq!(stored.saturation, -0.2);
    assert!(stored.has_local_color());
    assert!(!stored.is_neutral());
    // Byte-stable: a second write does not drift.
    assert_eq!(loaded.to_json().unwrap(), json);
    // The stable CLI status line names the blocks.
    assert_eq!(stored.hsl_summary(), "1bands");
    assert_eq!(stored.point_color_summary(), "1entries");
    assert_eq!(stored.color_grading_summary(), "balance+shadows+midtones");
    let line = stored.to_string();
    assert!(line.contains("hsl=1bands"), "{line}");
    assert!(line.contains("point_color=1entries"), "{line}");
    assert!(line.contains("vibrance=0.4"), "{line}");
}

/// v1, v2 and v3 migrate forward without loss; a colour field in one of them is
/// a loud refusal, never a silent drop or a coercion.
#[test]
fn legacy_versions_migrate_losslessly_and_refuse_a_smuggled_color_block() {
    for (version, fields) in [
        (
            1u64,
            json!({"exposure": 0.5, "temperature_delta_k": json!(null), "tint_delta": json!(null)}),
        ),
        (
            2,
            json!({"exposure": 0.5, "temperature_delta_k": -900, "tint_delta": 0.1}),
        ),
        (
            3,
            json!({
                "exposure": 0.5,
                "temperature_delta_k": -900,
                "tint_delta": 0.1,
                "curves": {
                    "version": 1,
                    "master": [{"input": 0.0, "output": 0.0}, {"input": 0.5, "output": 0.7}, {"input": 1.0, "output": 1.0}],
                },
            }),
        ),
    ] {
        let mut value: Value =
            serde_json::from_str(&document_with_layer(color_layer()).to_json().unwrap()).unwrap();
        let mut payload = json!({
            "version": version,
            "contrast": 0.0,
            "highlights": 0.0,
            "shadows": 0.0,
        });
        for (key, field) in fields.as_object().unwrap() {
            if field.is_null() {
                continue;
            }
            payload[key] = field.clone();
        }
        value["virtual_copies"][0]["mask_layers"][0]["local_adjustments"] = payload;
        let loaded = SidecarDocument::from_json(&serde_json::to_string(&value).unwrap()).unwrap();
        let local = stored_local(&loaded);
        assert_eq!(local.version, LOCAL_ADJUSTMENTS_VERSION);
        assert_eq!(local.exposure, 0.5);
        assert_eq!(
            local.temperature_delta_k,
            if version >= 2 { -900.0 } else { 0.0 }
        );
        assert_eq!(local.tint_delta, if version >= 2 { 0.1 } else { 0.0 });
        assert!(
            local.hsl.is_none(),
            "v{version} must migrate to no HSL block"
        );
        assert!(local.point_color.is_none());
        assert!(local.color_grading.is_none());
        assert_eq!(local.vibrance, 0.0);
        assert_eq!(local.saturation, 0.0);
        if version >= 3 {
            assert!(local.curves.is_some(), "v3 keeps its curve");
        } else {
            assert!(
                local.curves.is_none(),
                "v{version} must migrate to no curve"
            );
        }

        // Smuggling any colour field into an older version is loud.
        for (field, smuggled) in [
            ("hsl", json!({"version": 1})),
            ("point_color", json!({"version": 1, "entries": []})),
            ("color_grading", json!({"version": 1})),
            ("hsl", Value::Null),
            ("vibrance", json!(0.5)),
            ("saturation", json!(-0.5)),
        ] {
            value["virtual_copies"][0]["mask_layers"][0]["local_adjustments"][field] =
                smuggled.clone();
            let error = SidecarDocument::from_json(&serde_json::to_string(&value).unwrap())
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("cannot contain"),
                "v{version} smuggling `{field}` must be loud, got: {error}"
            );
            value["virtual_copies"][0]["mask_layers"][0]["local_adjustments"]
                .as_object_mut()
                .unwrap()
                .remove(field);
        }
        // A completely unknown field is refused by the same wire shape.
        value["virtual_copies"][0]["mask_layers"][0]["local_adjustments"]["optics"] = json!(1);
        assert!(SidecarDocument::from_json(&serde_json::to_string(&value).unwrap()).is_err());
    }
}

/// The colour block reuses the *global* validators: everything the global
/// recipe rejects must be rejected here too, and the block versions are the
/// global ones.
#[test]
fn local_color_uses_the_existing_global_ranges() {
    let mut local = LocalAdjustments::default();
    assert!(local.set_local_hsl_band("red", "hue", -0.25).is_ok());
    assert!(local
        .add_local_point_color_entry(30.0, 45.0, 0.2, 0.1, -0.1)
        .is_ok());
    assert!(local
        .set_local_color_grading_field("shadows", "hue", 180.0)
        .is_ok());

    // The very same blocks are accepted by the global recipe validators.
    validate_hsl(local.hsl.as_ref().unwrap()).expect("shared hsl rules");
    validate_point_color(local.point_color.as_ref().unwrap()).expect("shared point color rules");
    validate_color_grading(local.color_grading.as_ref().unwrap()).expect("shared grading rules");

    for (band, field, value) in [
        ("red", "hue", 1.5),
        ("red", "hue", f64::NAN),
        ("red", "brightness", 0.2),
        ("luma", "hue", 0.2),
    ] {
        let before = local.clone();
        let error = local
            .set_local_hsl_band(band, field, value)
            .expect_err("out-of-range local hsl must be refused");
        assert!(!error.is_empty());
        assert_eq!(local.hsl, before.hsl, "{band}.{field} must not mutate");
    }
    for (target, field, value) in [
        ("shadows", "hue", 400.0),
        ("shadows", "saturation", 1.4),
        ("midtones", "luminance", -1.5),
        ("balance", "value", 1.2),
        ("blending", "value", 1.2),
        ("whites", "hue", 30.0),
    ] {
        let before = local.clone();
        assert!(local
            .set_local_color_grading_field(target, field, value)
            .is_err());
        assert_eq!(
            local.color_grading, before.color_grading,
            "{target}.{field}"
        );
    }
    for values in [
        [400.0, 45.0, 0.2, 0.1, -0.1],
        [30.0, 200.0, 0.2, 0.1, -0.1],
        [30.0, 45.0, 2.0, 0.1, -0.1],
        [f64::NAN, 45.0, 0.2, 0.1, -0.1],
    ] {
        let before = local.clone();
        assert!(local
            .add_local_point_color_entry(values[0], values[1], values[2], values[3], values[4],)
            .is_err());
        assert_eq!(local.point_color, before.point_color);
    }
    // The shared 8-entry limit applies locally as well (one entry already
    // exists from the block set up above).
    for _ in 1..MAX_POINT_COLOR_ENTRIES {
        local
            .add_local_point_color_entry(30.0, 45.0, 0.1, 0.1, 0.1)
            .unwrap();
    }
    assert!(local
        .add_local_point_color_entry(30.0, 45.0, 0.1, 0.1, 0.1)
        .unwrap_err()
        .contains("maximum"));

    // A block version the global recipe would reject is rejected locally too.
    let mut wrong = local.clone();
    wrong.hsl.as_mut().unwrap().version = 2;
    assert!(wrong
        .validate()
        .unwrap_err()
        .to_string()
        .contains("unsupported hsl version"));
    let mut wrong = local.clone();
    wrong.point_color.as_mut().unwrap().version = 2;
    assert!(wrong
        .validate()
        .unwrap_err()
        .to_string()
        .contains("unsupported point_color version"));
    let mut wrong = local.clone();
    wrong.color_grading.as_mut().unwrap().version = 2;
    assert!(wrong
        .validate()
        .unwrap_err()
        .to_string()
        .contains("unsupported color_grading version"));
}

/// `None`, an all-zero HSL block, an all-zero point-colour entry list, a neutral
/// grading block and `vibrance = saturation = 0` are all pixel-neutral, and a
/// reset drops the block so the layer is byte-identical to "never edited".
#[test]
fn neutral_absent_and_neutral_color_blocks_read_the_same_and_reset_clears() {
    let local = LocalAdjustments::default();
    assert!(local.is_neutral());
    assert!(!local.has_local_color());
    assert_eq!(local.hsl_summary(), "none");
    assert_eq!(local.point_color_summary(), "none");
    assert_eq!(local.color_grading_summary(), "none");

    let mut neutral_blocks = LocalAdjustments {
        hsl: Some(HslAdjustments {
            version: 1,
            red: Some(HslChannel::default()),
            ..HslAdjustments::default()
        }),
        point_color: Some(PointColor {
            version: 1,
            entries: vec![local_point_color_entry(
                "pc-1".into(),
                30.0,
                45.0,
                0.0,
                0.0,
                0.0,
            )],
        }),
        color_grading: Some(ColorGrading::neutral()),
        vibrance: 0.0,
        saturation: 0.0,
        ..LocalAdjustments::default()
    };
    assert!(neutral_blocks.is_neutral());
    assert!(!neutral_blocks.has_local_color());
    // The stored *form* is part of the identity even when the pixels agree.
    assert_ne!(
        LocalAdjustments::default().digest(),
        neutral_blocks.digest()
    );
    // And a non-default `blending` alone is still neutral: it only moves the
    // edges of three zero-weight ranges.
    neutral_blocks
        .set_local_color_grading_field("blending", "value", 0.9)
        .unwrap();
    assert!(neutral_blocks.is_neutral());
    neutral_blocks
        .set_local_color_grading_field("blending", "value", 0.5)
        .unwrap();
    assert_ne!(
        LocalAdjustments::default().digest(),
        neutral_blocks.digest()
    );

    // A real edit makes the layer non-neutral and survives a round trip.
    let mut edited = LocalAdjustments::default();
    edited.set_local_hsl_band("red", "hue", -0.25).unwrap();
    assert!(!edited.is_neutral());
    assert!(edited.has_local_color());
    assert_eq!(edited.local_hsl_band("red").unwrap().hue, -0.25);
    // Setting a shift back to zero drops the whole block again.
    edited.set_local_hsl_band("red", "hue", 0.0).unwrap();
    assert!(edited.hsl.is_none());
    assert!(edited.is_neutral());

    // Reset-all clears every colour area.
    let mut full = LocalAdjustments::default();
    full.set_local_hsl_band("red", "hue", -0.25).unwrap();
    full.set_value("vibrance", 0.4).unwrap();
    full.add_local_point_color_entry(30.0, 45.0, 0.2, 0.1, -0.1)
        .unwrap();
    full.set_local_color_grading_field("shadows", "saturation", 0.4)
        .unwrap();
    full.reset_local_color();
    assert!(full.is_neutral());
    assert!(full.hsl.is_none());
    assert!(full.point_color.is_none());
    assert!(full.color_grading.is_none());
    assert_eq!(full.vibrance, 0.0);
    assert_eq!(full.saturation, 0.0);
}
