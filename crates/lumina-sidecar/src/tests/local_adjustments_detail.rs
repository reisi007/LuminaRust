//! MASK-LOCAL-P1.2d sidecar tests: the typed local detail block, its lossless
//! migration from v1..v5, the loud refusals, and its digest contribution.
//!
//! Split from `local_adjustments_presence.rs` (file-size ratchet): that file owns
//! the presence block, this one owns the detail block. Both share the same
//! `document_with_layer` / `stored_local` helpers.

use super::local_adjustments::layer_with_legacy;
use super::*;
use crate::tests::local_adjustments_color::{document_with_layer, stored_local};
use crate::tests::support::source;
use serde_json::json;

/// A layer that carries exactly one typed local sharpening edit.
pub(super) fn sharpened_layer() -> MaskLayer {
    let mut layer = layer_with_legacy();
    layer.extras.clear();
    let mut local = LocalAdjustments::default();
    local
        .set_local_sharpening_field("amount", 1.25)
        .expect("amount is valid");
    layer.local_adjustments = Some(local);
    layer
}

/// A v6 detail payload round-trips through the sidecar file without loss.
#[test]
fn local_detail_round_trips_through_the_sidecar_file() {
    let mut layer = layer_with_legacy();
    layer.extras.clear();
    let mut local = LocalAdjustments::default();
    for (field, value) in [
        ("amount", 1.25),
        ("radius", 3.5),
        ("detail", 0.25),
        ("masking", 0.75),
    ] {
        local
            .set_local_sharpening_field(field, value)
            .unwrap_or_else(|error| panic!("{field}={value} must be valid: {error}"));
    }
    for (field, value) in [("luminance", 0.5), ("color", 0.25)] {
        local
            .set_local_noise_reduction_field(field, value)
            .unwrap_or_else(|error| panic!("{field}={value} must be valid: {error}"));
    }
    // A full stack alongside the detail block must survive together.
    local.temperature_delta_k = -900.0;
    local.tint_delta = 0.1;
    local.set_value("vibrance", 0.3).expect("vibrance");
    local
        .set_local_presence_field("texture", 0.2)
        .expect("texture");
    local.hsl = Some(HslAdjustments {
        version: 1,
        red: Some(HslChannel {
            hue: -0.25,
            ..HslChannel::default()
        }),
        ..HslAdjustments::default()
    });
    layer.local_adjustments = Some(local);
    let document = document_with_layer(layer);

    let json = document.to_json().unwrap();
    for key in ["\"detail\"", "\"sharpening\"", "\"noise_reduction\""] {
        assert!(json.contains(key), "{key} must be persisted: {json}");
    }
    let loaded = SidecarDocument::from_json(&json).unwrap();
    let stored = stored_local(&loaded);
    assert_eq!(stored.version, LOCAL_ADJUSTMENTS_VERSION);
    assert_eq!(stored.version, DETAIL_LOCAL_ADJUSTMENTS_VERSION);
    // The detail block is stored with the *global* types at the *global* ranges.
    let detail = stored.detail.expect("detail block");
    let sharpening = detail.sharpening.expect("sharpening");
    assert_eq!(sharpening.version, 1);
    assert!((sharpening.amount - 1.25).abs() < 1e-6);
    assert!((sharpening.radius - 3.5).abs() < 1e-6);
    assert!((sharpening.detail - 0.25).abs() < 1e-6);
    assert!((sharpening.masking - 0.75).abs() < 1e-6);
    let noise = detail.noise_reduction.expect("noise reduction");
    assert_eq!(noise.version, 1);
    assert!((noise.luminance - 0.5).abs() < 1e-6);
    assert!((noise.color - 0.25).abs() < 1e-6);
    assert!(stored.has_local_detail());
    assert!(stored.has_local_sharpening());
    assert!(stored.has_local_noise_reduction());
    assert!(!stored.is_neutral());
    // Byte-stable: a second write does not drift.
    assert_eq!(loaded.to_json().unwrap(), json);
    // The stable CLI status line names the block.
    assert_eq!(stored.detail_summary(), "sharpening+noise_reduction");
    let line = stored.to_string();
    assert!(line.contains("detail=sharpening+noise_reduction"), "{line}");
    assert!(line.starts_with("v6 "), "{line}");
    // Every other block survived too.
    assert!(stored.has_local_presence());
    assert!(stored.hsl.is_some());
    assert_eq!(stored.vibrance, 0.3);
}

/// v1, v2, v3, v4 and v5 migrate forward losslessly to v6 with `detail: None`,
/// and a `detail` field in any of them — including an explicit `null` and a
/// non-object — is a loud refusal, never a silent drop or coercion.
#[test]
fn legacy_versions_migrate_losslessly_and_refuse_a_smuggled_detail_block() {
    let full_v5 = json!({
        "version": 5,
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
        "presence": {"version": 1, "texture": 0.5, "clarity": 0.0, "dehaze": 0.0},
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
        (
            4,
            json!({
                "version": 4,
                "exposure": 0.5,
                "temperature_delta_k": -900,
                "tint_delta": 0.1,
                "curves": {
                    "version": 1,
                    "master": [{"input": 0.0, "output": 0.0}, {"input": 0.5, "output": 0.7}, {"input": 1.0, "output": 1.0}],
                },
                "hsl": {"version": 1, "red": {"hue": -0.25, "saturation": 0.0, "luminance": 0.0}},
                "vibrance": 0.4,
                "saturation": -0.2,
            }),
        ),
        (5, full_v5),
    ] {
        let mut value: Value =
            serde_json::from_str(&document_with_layer(sharpened_layer()).to_json().unwrap())
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
        assert_eq!(local.detail, None, "v{version} must migrate to no detail");
        assert!(!local.has_local_detail());
        assert_eq!(local.detail_summary(), "none");
        assert_eq!(local.exposure, 0.5);
        // Every block the document *owned* must survive the migration. This is
        // the versions-richtig gate: raising the current version to 6 must not
        // make a v5 document lose its own presence block.
        if version >= 3 {
            assert!(local.curves.is_some(), "v{version} keeps its curve");
        } else {
            assert!(local.curves.is_none(), "v{version} migrates to no curve");
        }
        if version >= 4 {
            assert!(local.hsl.is_some(), "v{version} keeps its own colour block");
            assert_eq!(local.vibrance, 0.4);
            assert_eq!(local.saturation, -0.2);
        } else {
            assert!(local.hsl.is_none(), "v{version} migrates to no HSL block");
            assert_eq!(local.vibrance, 0.0);
            assert_eq!(local.saturation, 0.0);
        }
        if version >= 5 {
            assert!(local.presence.is_some(), "v5 keeps its own presence block");
            assert!((local.presence.as_ref().unwrap().texture - 0.5).abs() < 1e-6);
        } else {
            assert!(
                local.presence.is_none(),
                "v{version} migrates to no presence block"
            );
        }

        // Smuggling a detail block into an older version is loud — for *every*
        // shape, not just a well-formed block.
        for (label, smuggled) in [
            (
                "a well-formed block",
                json!({"sharpening": {"version": 1, "amount": 1.0, "radius": 2.0, "detail": 0.5, "masking": 0.0}}),
            ),
            (
                "a noise-reduction-only block",
                json!({"noise_reduction": {"version": 1, "luminance": 0.4, "color": 0.0}}),
            ),
            ("an explicit null", Value::Null),
            ("a number", json!(0.5)),
            ("a string", json!("sharpening.amount=1.0")),
            ("an array", json!([0.5])),
            ("a sentinel-shaped object", json!({"__unset__": true})),
        ] {
            let mut smuggled_value = value.clone();
            smuggled_value["virtual_copies"][0]["mask_layers"][0]["local_adjustments"]["detail"] =
                smuggled.clone();
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
        current["virtual_copies"][0]["mask_layers"][0]["local_adjustments"]["detail"] = json!({"sharpening": {"version": 1, "amount": 1.0, "radius": 2.0, "detail": 0.5, "masking": 0.0}});
        let accepted =
            SidecarDocument::from_json(&serde_json::to_string(&current).unwrap()).unwrap();
        assert!(
            stored_local(&accepted).has_local_sharpening(),
            "v{LOCAL_ADJUSTMENTS_VERSION} must accept the very same block"
        );
    }
}

/// The detail block reuses the *global* validators: everything the global recipe
/// rejects must be rejected here too, and both sub-block versions are the global
/// ones. The ranges are the **global** ranges, neither widened nor narrowed.
#[test]
fn local_detail_uses_the_existing_global_ranges() {
    let mut local = LocalAdjustments::default();
    for (field, value) in [
        ("amount", 3.0),
        ("radius", 0.1),
        ("detail", 1.0),
        ("masking", 1.0),
    ] {
        local
            .set_local_sharpening_field(field, value)
            .unwrap_or_else(|error| panic!("{field}={value} must be valid: {error}"));
    }
    for (field, value) in [("luminance", 1.0), ("color", 0.0)] {
        local
            .set_local_noise_reduction_field(field, value)
            .unwrap_or_else(|error| panic!("{field}={value} must be valid: {error}"));
    }
    // The very same blocks are accepted by the global validators, and the
    // *global* recipe accepts them through the very same functions.
    let detail = local.detail.expect("detail block");
    validate_sharpening(&detail.sharpening.expect("sharpening")).expect("shared rules");
    validate_noise_reduction(&detail.noise_reduction.expect("noise reduction"))
        .expect("shared rules");
    let mut global = EditRecipe::default();
    global.sharpening = detail.sharpening;
    global.noise_reduction = detail.noise_reduction;
    let mut probe = SidecarDocument::new(source(), "pipeline-1");
    probe.virtual_copies[0].recipe = global.clone();
    probe
        .validate()
        .unwrap_or_else(|error| panic!("the global recipe accepts them too: {error}"));
    assert!(!sharpening_is_neutral(
        &detail.sharpening.expect("sharpening")
    ));

    // The two radius boundaries are legal, and the values just outside them are
    // not — pinned so the range can never silently widen or narrow.
    assert_eq!(sharpening_radius_range(), (0.1, 10.0));
    for (field, value) in [
        ("amount", 3.000_001),
        ("amount", -0.5),
        ("radius", 0.099_999),
        ("radius", 10.000_001),
        ("detail", 1.000_001),
        ("masking", -0.5),
        ("amount", f64::NAN),
        ("radius", f64::INFINITY),
        ("luminance", 0.5),
        ("grain", 0.5),
        ("", 0.5),
    ] {
        let error = local
            .set_local_sharpening_field(field, value)
            .expect_err("must be refused");
        assert!(error.contains(field), "{error}");
    }
    for (field, value) in [
        ("luminance", 1.000_001),
        ("color", -0.5),
        ("luminance", f64::NAN),
        ("amount", 0.5),
        ("", 0.5),
    ] {
        let error = local
            .set_local_noise_reduction_field(field, value)
            .expect_err("must be refused");
        assert!(error.contains(field), "{error}");
    }
    // The shared range helper accepts exactly the values every detail field
    // accepts, and refuses everything outside the narrowest of them.
    assert!(detail_amount_is_valid(0.0));
    assert!(detail_amount_is_valid(1.0));
    assert!(!detail_amount_is_valid(-0.000_001));
    assert!(!detail_amount_is_valid(1.000_001));
    assert!(!detail_amount_is_valid(f64::NAN));
    let mut wrong_sharpening_version = neutral_local_detail();
    wrong_sharpening_version
        .sharpening
        .as_mut()
        .expect("sharpening")
        .version = 2;
    let error = wrong_sharpening_version.validate().unwrap_err().to_string();
    assert!(error.contains("unsupported sharpening version"), "{error}");
    let mut wrong_noise_version = neutral_local_detail();
    wrong_noise_version
        .noise_reduction
        .as_mut()
        .expect("noise reduction")
        .version = 2;
    let error = wrong_noise_version.validate().unwrap_err().to_string();
    assert!(
        error.contains("unsupported noise_reduction version"),
        "{error}"
    );
    // And the global recipe refuses the same wrong versions through the same
    // validators.
    let global_error = |recipe: EditRecipe| {
        let mut probe = SidecarDocument::new(source(), "pipeline-1");
        probe.virtual_copies[0].recipe = recipe;
        probe.validate().unwrap_err().to_string()
    };
    let mut global = EditRecipe::default();
    global.sharpening = wrong_sharpening_version.sharpening;
    let error = global_error(global);
    assert!(error.contains("unsupported sharpening version"), "{error}");
    let mut global = EditRecipe::default();
    global.noise_reduction = wrong_noise_version.noise_reduction;
    let error = global_error(global);
    assert!(
        error.contains("unsupported noise_reduction version"),
        "{error}"
    );
}

/// An absent block, a persisted all-neutral block and a persisted block with
/// non-default but neutral sub-blocks all read the same and are all pixel-neutral;
/// a reset clears them.
#[test]
fn neutral_absent_and_neutral_detail_blocks_read_the_same_and_reset_clears() {
    let default = LocalAdjustments::default();
    assert!(default.detail.is_none());
    assert!(!default.has_local_detail());
    assert!(default.is_neutral());
    assert_eq!(default.detail_summary(), "none");
    let baseline = default.digest();

    let neutral = neutral_local_detail();
    assert!(neutral.is_neutral(), "a neutral block is neutral");
    let mut stored = LocalAdjustments::default();
    stored.detail = Some(neutral);
    assert!(!stored.has_local_detail());
    assert!(!stored.has_local_sharpening());
    assert!(!stored.has_local_noise_reduction());
    assert!(stored.is_neutral());
    // It is *persisted*, so it is part of the state identity even though it
    // cannot change a pixel — the renderer is what treats it as the old path.
    assert_ne!(stored.digest(), baseline);

    // Writing the amount back to zero removes the sharpening sub-block and, with
    // it, the whole container.
    let mut edited = LocalAdjustments::default();
    edited
        .set_local_sharpening_field("amount", 1.0)
        .expect("amount");
    edited
        .set_local_sharpening_field("radius", 2.0)
        .expect("radius");
    assert!(edited.has_local_sharpening());
    assert_eq!(edited.detail_summary(), "sharpening");
    edited
        .set_local_sharpening_field("amount", 0.0)
        .expect("amount");
    assert!(edited.detail.is_none());
    assert!(edited.is_neutral());
    assert_eq!(edited.digest(), baseline);

    // Reset per sub-block and for the whole block.
    let mut full = LocalAdjustments::default();
    full.set_local_sharpening_field("amount", 1.0).unwrap();
    full.set_local_noise_reduction_field("luminance", 0.5)
        .unwrap();
    assert_eq!(full.detail_summary(), "sharpening+noise_reduction");
    full.reset_local_detail_field("noise_reduction").unwrap();
    assert_eq!(full.detail_summary(), "sharpening");
    assert!(full.has_local_sharpening());
    full.reset_local_detail_field("sharpening").unwrap();
    assert!(full.detail.is_none());
    assert_eq!(full.digest(), baseline);
    // And the whole-block reset does the same in one step.
    let mut both = LocalAdjustments::default();
    both.set_local_sharpening_field("amount", 1.0).unwrap();
    both.set_local_noise_reduction_field("luminance", 0.5)
        .unwrap();
    both.reset_local_detail();
    assert!(both.detail.is_none());
    assert_eq!(both.digest(), baseline);
    // A `masking = 0` block with a non-zero amount is NOT neutral: the flat-area
    // suppression is the only thing masking does, so `masking = 0` is the
    // strongest setting, never a no-op.
    let mut strongest = LocalAdjustments::default();
    strongest.set_local_sharpening_field("amount", 1.0).unwrap();
    strongest.set_local_sharpening_field("radius", 2.0).unwrap();
    strongest.set_local_sharpening_field("detail", 0.5).unwrap();
    let block = strongest.detail.as_ref().unwrap().sharpening.unwrap();
    assert_eq!(block.masking, 0.0);
    assert!(!sharpening_is_neutral(&block));
    assert!(strongest.has_local_detail());
    assert!(strongest.has_local_sharpening());
    assert!(!strongest.is_neutral());
    // A noise reduction with both strengths at zero IS neutral.
    assert!(noise_reduction_is_neutral(&NoiseReduction {
        version: 1,
        luminance: 0.0,
        color: 0.0,
    }));
    assert!(!noise_reduction_is_neutral(&NoiseReduction {
        version: 1,
        luminance: 0.0,
        color: 0.5,
    }));
}
