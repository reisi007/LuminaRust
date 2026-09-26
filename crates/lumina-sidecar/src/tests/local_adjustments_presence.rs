//! MASK-LOCAL-P1.2c sidecar tests: the typed local presence block, its
//! lossless migration from v1/v2/v3/v4, the loud refusals, and its digest
//! contribution.
//!
//! Split from `local_adjustments_color.rs` (file-size ratchet): that file owns
//! the colour block, this one owns the presence block. Both share the same
//! `document_with_layer`/`stored_local` helpers.

use super::local_adjustments::layer_with_legacy;
use super::*;
use crate::tests::local_adjustments_color::{document_with_layer, stored_local};
use serde_json::json;

/// A presence block with a non-neutral texture amount.
pub(super) fn textured_presence() -> Presence {
    Presence {
        version: 1,
        texture: -0.35,
        clarity: 0.0,
        dehaze: 0.0,
    }
}

/// A layer that carries exactly one typed local presence edit.
pub(super) fn presence_layer() -> MaskLayer {
    let mut layer = layer_with_legacy();
    layer.extras.clear();
    let mut local = LocalAdjustments::default();
    local
        .set_local_presence_field("texture", -0.35)
        .expect("texture is valid");
    layer.local_adjustments = Some(local);
    layer
}

/// A v5 presence payload round-trips through the sidecar file without loss.
#[test]
fn local_presence_round_trips_through_the_sidecar_file() {
    let mut layer = layer_with_legacy();
    layer.extras.clear();
    let mut local = LocalAdjustments::default();
    local
        .set_local_presence_field("texture", 0.4)
        .expect("texture");
    local
        .set_local_presence_field("clarity", -0.2)
        .expect("clarity");
    local
        .set_local_presence_field("dehaze", 0.6)
        .expect("dehaze");
    // A full stack alongside the presence block must survive together.
    local.temperature_delta_k = -900.0;
    local.tint_delta = 0.1;
    local.set_value("vibrance", 0.3).expect("vibrance");
    layer.local_adjustments = Some(local);
    let document = document_with_layer(layer);

    let json = document.to_json().unwrap();
    assert!(
        json.contains("\"presence\""),
        "the presence block must be persisted: {json}"
    );
    let loaded = SidecarDocument::from_json(&json).unwrap();
    let stored = stored_local(&loaded);
    assert_eq!(stored.version, LOCAL_ADJUSTMENTS_VERSION);
    // MASK-LOCAL-P1.2d raised the current version to 6; the presence gate stays
    // anchored at 5, so this document still owns its own presence block.
    assert_eq!(PRESENCE_LOCAL_ADJUSTMENTS_VERSION, 5);
    assert_eq!(DETAIL_LOCAL_ADJUSTMENTS_VERSION, 6);
    // The persisted amounts are `f32`, so the `f64` accessor returns the exact
    // `f32` values widened back — the same contract every other local block has.
    let (texture, clarity, dehaze) = stored.local_presence();
    assert!((texture - 0.4).abs() < 1e-6, "{texture}");
    assert!((clarity + 0.2).abs() < 1e-6, "{clarity}");
    assert!((dehaze - 0.6).abs() < 1e-6, "{dehaze}");
    assert!(stored.has_local_presence());
    assert!(!stored.is_neutral());
    // Byte-stable: a second write does not drift.
    assert_eq!(loaded.to_json().unwrap(), json);
    // The stable CLI status line names the block.
    assert_eq!(stored.presence_summary(), "texture+clarity+dehaze");
    let line = stored.to_string();
    assert!(line.contains("presence=texture+clarity+dehaze"), "{line}");
    assert!(line.contains("detail=none"), "{line}");
    assert!(line.starts_with("v6 "), "{line}");
}

/// v1, v2, v3 and v4 migrate forward losslessly to v5 with `presence: None`,
/// and a `presence` field in any of them — including an explicit `null` and a
/// non-object — is a loud refusal, never a silent drop or coercion.
#[test]
fn legacy_versions_migrate_losslessly_and_refuse_a_smuggled_presence_block() {
    let full_v4 = json!({
        "version": 4,
        "exposure": 0.5,
        "contrast": 0.25,
        "highlights": -0.2,
        "shadows": 0.3,
        "temperature_delta_k": -900,
        "tint_delta": 0.1,
        "curves": {
            "version": 1,
            "master": [{"input": 0.0, "output": 0.0}, {"input": 0.5, "output": 0.7}, {"input": 1.0, "output": 1.0}],
        },
        "hsl": {"version": 1, "red": {"hue": -0.25, "saturation": 0.0, "luminance": 0.0}},
        "vibrance": 0.4,
        "saturation": -0.2,
    });
    for (version, fields) in [
        (1u64, json!({"exposure": 0.5})),
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
        (4, full_v4),
    ] {
        let mut value: Value =
            serde_json::from_str(&document_with_layer(presence_layer()).to_json().unwrap())
                .unwrap();
        let mut payload = json!({
            "version": version,
            "contrast": 0.25,
            "highlights": -0.2,
            "shadows": 0.3,
        });
        for (key, field) in fields.as_object().unwrap() {
            payload[key] = field.clone();
        }
        value["virtual_copies"][0]["mask_layers"][0]["local_adjustments"] = payload;
        let loaded = SidecarDocument::from_json(&serde_json::to_string(&value).unwrap()).unwrap();
        let local = stored_local(&loaded);
        assert_eq!(
            local.version, LOCAL_ADJUSTMENTS_VERSION,
            "v{version} must migrate forward"
        );
        assert_eq!(
            local.presence, None,
            "v{version} must migrate to no presence"
        );
        assert!(!local.has_local_presence());
        assert_eq!(local.presence_summary(), "none");
        assert_eq!(local.exposure, 0.5);
        // Every *later* block the document owned must survive the migration:
        // this is the versions-richtig gate, the exact class of bug that made
        // P1.2b's colour gate version-anchored.
        if version >= 3 {
            assert!(local.curves.is_some(), "v{version} keeps its curve");
        } else {
            assert!(local.curves.is_none(), "v{version} migrates to no curve");
        }
        if version >= 4 {
            assert!(local.hsl.is_some(), "v4 keeps its own colour block");
            assert_eq!(local.vibrance, 0.4);
            assert_eq!(local.saturation, -0.2);
        } else {
            assert!(local.hsl.is_none(), "v{version} migrates to no HSL block");
            assert_eq!(local.vibrance, 0.0);
            assert_eq!(local.saturation, 0.0);
        }

        // Smuggling a presence into an older version is loud — for *every*
        // shape, not just a well-formed block.
        for (label, smuggled) in [
            (
                "a well-formed block",
                json!({"version": 1, "texture": 0.5, "clarity": 0.0, "dehaze": 0.0}),
            ),
            ("an explicit null", Value::Null),
            ("a number", json!(0.5)),
            ("a string", json!("texture=0.5")),
            ("an array", json!([0.5])),
            ("a sentinel-shaped object", json!({"__unset__": true})),
        ] {
            let mut smuggled_value = value.clone();
            smuggled_value["virtual_copies"][0]["mask_layers"][0]["local_adjustments"]
                ["presence"] = smuggled.clone();
            let error =
                SidecarDocument::from_json(&serde_json::to_string(&smuggled_value).unwrap())
                    .unwrap_err()
                    .to_string();
            assert!(
                error.contains("cannot contain"),
                "v{version} smuggling {label} must be loud, got: {error}"
            );
        }
        // The very same block *is* accepted by the current version, so the
        // refusal above is about the version, never about the value.
        let mut current = value.clone();
        current["virtual_copies"][0]["mask_layers"][0]["local_adjustments"]["version"] =
            json!(LOCAL_ADJUSTMENTS_VERSION);
        current["virtual_copies"][0]["mask_layers"][0]["local_adjustments"]["presence"] =
            json!({"version": 1, "texture": 0.5, "clarity": 0.0, "dehaze": 0.0});
        let accepted =
            SidecarDocument::from_json(&serde_json::to_string(&current).unwrap()).unwrap();
        assert!(
            (stored_local(&accepted).local_presence().0 - 0.5).abs() < 1e-6,
            "v{LOCAL_ADJUSTMENTS_VERSION} must accept the very same block"
        );
    }
}

/// The presence block reuses the *global* validator: everything the global
/// recipe rejects must be rejected here too, and the block version is the
/// global one.
#[test]
fn local_presence_uses_the_existing_global_ranges() {
    let mut local = LocalAdjustments::default();
    for (field, value) in [("texture", 1.0), ("clarity", -1.0), ("dehaze", 0.5)] {
        local
            .set_local_presence_field(field, value)
            .unwrap_or_else(|error| panic!("{field}={value} must be valid: {error}"));
    }
    // The very same block is accepted by the global validator.
    validate_presence(local.presence.as_ref().unwrap()).expect("shared presence rules");
    assert!(!presence_is_neutral(local.presence.as_ref().unwrap()));
    assert_eq!(PRESENCE_FIELDS.len(), 3);

    // Out-of-range, non-finite and unknown fields are refused *before* they
    // mutate, so a rejected edit leaves the layer byte-for-byte unchanged.
    for (field, value) in [
        ("texture", 1.000_001),
        ("texture", -1.5),
        ("clarity", f64::NAN),
        ("clarity", f64::INFINITY),
        ("dehaze", -2.0),
        ("grain", 0.5),
        ("detail", 0.5),
        ("", 0.5),
    ] {
        let before = serde_json::to_string(&local).unwrap();
        let error = local
            .set_local_presence_field(field, value)
            .expect_err("an invalid local presence amount must be refused");
        assert!(!error.is_empty());
        assert_eq!(
            serde_json::to_string(&local).unwrap(),
            before,
            "a refused `{field}` must leave the layer byte-for-byte unchanged"
        );
    }
    // A block version the global recipe would reject is rejected locally too.
    for version in [0u8, 2, 7] {
        let mut wrong = local.clone();
        wrong.presence.as_mut().unwrap().version = version;
        assert!(
            wrong
                .validate()
                .unwrap_err()
                .to_string()
                .contains("unsupported presence version"),
            "block version {version} must be refused"
        );
        // And the *global* recipe refuses the same block.
        // And the *global* nested-adjustment validator refuses the same block,
        // because it now calls the very same `validate_presence`.
        let global = EditRecipe {
            presence: wrong.presence,
            ..Default::default()
        };
        assert!(
            validate_adjustments(&global).is_err(),
            "the global recipe must refuse block version {version} too"
        );
    }
    // A hand-constructed out-of-range block is refused by both surfaces.
    for (field, value) in [("texture", 1.5), ("clarity", -2.0), ("dehaze", f64::NAN)] {
        let mut block = neutral_local_presence();
        match field {
            "texture" => block.texture = value as f32,
            "clarity" => block.clarity = value as f32,
            _ => block.dehaze = value as f32,
        }
        assert!(validate_presence(&block).is_err(), "global {field}={value}");
        let mut local = LocalAdjustments::default();
        local.presence = Some(block);
        assert!(local.validate().is_err(), "local {field}={value}");
        let global = EditRecipe {
            presence: Some(block),
            ..Default::default()
        };
        assert!(
            validate_adjustments(&global).is_err(),
            "recipe {field}={value}"
        );
    }
}

/// `None`, an all-zero persisted block and a freshly reset block all read the
/// same and are byte-identical; a non-neutral block is not neutral.
#[test]
fn neutral_absent_and_neutral_presence_blocks_read_the_same_and_reset_clears() {
    let local = LocalAdjustments::default();
    assert!(local.is_neutral());
    assert!(!local.has_local_presence());
    assert_eq!(local.presence_summary(), "none");
    assert_eq!(local.local_presence(), (0.0, 0.0, 0.0));

    // A persisted all-zero block is pixel-neutral and takes the old kernel path,
    // but its *stored form* is still part of the identity.
    let zero_block = LocalAdjustments {
        presence: Some(neutral_local_presence()),
        ..LocalAdjustments::default()
    };
    assert!(zero_block.is_neutral());
    assert!(!zero_block.has_local_presence());
    assert_eq!(zero_block.presence_summary(), "none");
    assert_eq!(zero_block.local_presence(), (0.0, 0.0, 0.0));
    assert_ne!(
        LocalAdjustments::default().digest(),
        zero_block.digest(),
        "the stored form of a neutral block is part of the identity"
    );

    // A real edit makes the layer non-neutral and reads back.
    let mut edited = LocalAdjustments::default();
    edited
        .set_local_presence_field("dehaze", 0.25)
        .expect("dehaze");
    assert!(!edited.is_neutral());
    assert!(edited.has_local_presence());
    assert_eq!(edited.presence_summary(), "dehaze");
    // Setting the last non-neutral amount back to zero drops the block entirely.
    edited
        .set_local_presence_field("dehaze", 0.0)
        .expect("dehaze");
    assert!(edited.presence.is_none());
    assert!(edited.is_neutral());

    // Reset-all clears the whole presence block.
    let mut full = LocalAdjustments::default();
    full.set_local_presence_field("texture", 0.5).unwrap();
    full.set_local_presence_field("clarity", 0.5).unwrap();
    full.set_local_presence_field("dehaze", 0.5).unwrap();
    assert_eq!(full.presence_summary(), "texture+clarity+dehaze");
    full.reset_local_presence();
    assert!(full.presence.is_none());
    assert!(full.is_neutral());
    assert_eq!(
        full.digest(),
        LocalAdjustments::default().digest(),
        "a reset must return the layer to the never-edited identity"
    );
}

/// The still-disabled local stages stay refused. Local presence (P1.2c) and
/// local detail (P1.2d) are the only new local blocks: AI-Denoise and Optics
/// must have neither a field, nor a key, nor a renderer stub.
///
/// MASK-LOCAL-P1.2d turned `detail` into a typed block, so it is removed from the
/// forbidden list below — and, to keep the test from getting *weaker*, the
/// replacements are stronger assertions: the default recipe must not serialize
/// the block, and the scalar setter must not exist for the sub-block operations
/// while the typed setters do.
#[test]
fn disabled_local_presence_detail_denoise_and_optics_stay_rejected() {
    // `adjustment_presence` is not a legacy key: the flattened `adjustment_*`
    // extras stay limited to the four P0 scalars.
    for key in [
        "adjustment_presence",
        "adjustment_texture",
        "adjustment_clarity",
        "adjustment_dehaze",
        "adjustment_detail",
        "adjustment_sharpening",
        "adjustment_noise_reduction",
        "adjustment_denoise_ai",
        "adjustment_optics",
    ] {
        let mut layer = layer_with_legacy();
        layer.extras.clear();
        layer.extras.insert(key.into(), json!(0.5));
        let error = validate_mask_layer_local_state(&layer)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown local adjustment"), "{key}: {error}");
        assert!(
            error.contains(&key[key.len()..].to_string()),
            "{key}: {error}"
        );
    }
    // And no typed field exists for them.
    let json = serde_json::to_value(LocalAdjustments::default()).unwrap();
    let keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
    for disabled in [
        "denoise_ai",
        "optics",
        "lens_correction",
        "luminance",
        "color",
        "radius",
    ] {
        assert!(
            !keys.iter().any(|key| key == disabled),
            "the local recipe must not carry a `{disabled}` field"
        );
        let mut recipe = LocalAdjustments::default();
        assert!(recipe.set_value(disabled, 0.1).is_err());
    }
    // MASK-LOCAL-P1.2d: `detail` is a typed block now, so the default recipe must
    // *not* serialize it, and the sub-block operations must not be reachable
    // through the scalar setter — they go through the typed setters only.
    assert!(!keys.iter().any(|key| key == "detail"), "{keys:?}");
    let mut detail_recipe = LocalAdjustments::default();
    for scalar in [
        "sharpening",
        "noise_reduction",
        "sharpening.amount",
        "noise_reduction.luminance",
    ] {
        assert!(detail_recipe.set_value(scalar, 0.1).is_err(), "{scalar}");
    }
    assert!(detail_recipe
        .set_local_sharpening_field("amount", 0.1)
        .is_ok());
    // The wire decoder refuses a disabled field outright, so no surface can
    // smuggle one in.
    for disabled in ["optics", "denoise_ai", "lens_correction"] {
        let mut value: Value =
            serde_json::from_str(&document_with_layer(presence_layer()).to_json().unwrap())
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
