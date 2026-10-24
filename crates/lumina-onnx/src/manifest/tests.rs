use super::*;

#[test]
fn birefnet_has_only_subject_capability() {
    let m = birefnet_manifest();
    assert!(m.capabilities.subject_segmentation);
    assert!(!m.capabilities.box_prompt);
    assert!(!m.capabilities.point_prompt);
    assert!(!m.capabilities.mask_prompt);
    assert!(!m.capabilities.class_detection);
    assert!(!m.capabilities.instance_segmentation);
    assert!(m.validate().is_ok());
}

#[test]
fn manifest_json_roundtrip_preserves_identity() {
    let m = birefnet_manifest();
    let json = m.to_json().unwrap();
    let back = ModelManifest::from_json(&json).unwrap();
    assert_eq!(m, back);
}

// R2-ONNX-01 — the persisted identity must carry a deterministic digest
// over the ModelInputSpec (ai-masks.md: preprocessing + inference
// resolution are identity components), not an empty extras map.
#[test]
fn model_identity_extras_carry_deterministic_input_spec_digest() {
    let identity = birefnet_manifest().to_model_identity();
    let digest = identity
        .extras
        .get(INPUT_SPEC_DIGEST_KEY)
        .unwrap_or_else(|| panic!("identity extras must carry {INPUT_SPEC_DIGEST_KEY}"));
    let serde_json::Value::String(digest) = digest else {
        panic!("digest extra must be a string, got {digest:?}");
    };
    let Some(hex) = digest.strip_prefix("sha256:") else {
        panic!("digest must be sha256-prefixed, got {digest}");
    };
    assert_eq!(hex.len(), 64, "sha256 hex length, got {hex}");
    assert!(
        hex.bytes().all(|b| b.is_ascii_hexdigit()),
        "lowercase-hex digest expected, got {hex}"
    );
    // Deterministic across calls and instances.
    assert_eq!(
        identity.extras,
        birefnet_manifest().to_model_identity().extras,
        "the same spec must always produce the same digest"
    );

    // The SAM 2.1 family shares resolution/layout but differs in name —
    // its digest must still be well-formed and stable.
    let sam = sam2_1_manifest(Sam2Variant::Tiny).to_model_identity();
    assert!(sam.extras.contains_key(INPUT_SPEC_DIGEST_KEY));
}

/// Direction test (analogous to the F-082-FOLLOWUP-HASH hash-direction
/// tests): changing **only** the input normalization must change the
/// persisted mask identity — this is exactly the stale-detection signal
/// that was previously bypassable.
#[test]
fn identity_changes_when_normalization_changes() {
    let base = birefnet_manifest();
    let mut changed = birefnet_manifest();
    changed.input.normalization.mean[0] += 0.001;

    let a = base.to_model_identity();
    let b = changed.to_model_identity();

    // Name/version/hash stay untouched — only the spec digest may differ.
    assert_eq!(a.name, b.name);
    assert_eq!(a.version, b.version);
    assert_eq!(a.hash, b.hash);
    assert_ne!(
        a.extras.get(INPUT_SPEC_DIGEST_KEY),
        b.extras.get(INPUT_SPEC_DIGEST_KEY),
        "a normalization change MUST flip the persisted input-spec digest"
    );
}

/// Direction test: changing **only** the inference resolution must also
/// flip the persisted identity digest.
#[test]
fn identity_changes_when_resolution_changes() {
    let base = birefnet_manifest();
    let mut changed = birefnet_manifest();
    changed.input.resolution = Resolution {
        width: 512,
        height: 512,
    };

    let a = base.to_model_identity();
    let b = changed.to_model_identity();
    assert_eq!((a.name, a.version, a.hash), (b.name, b.version, b.hash));
    assert_ne!(
        a.extras.get(INPUT_SPEC_DIGEST_KEY),
        b.extras.get(INPUT_SPEC_DIGEST_KEY),
        "a resolution change MUST flip the persisted input-spec digest"
    );
}

/// Any other spec component participates too: tensor format and layout
/// are part of the inference contract.
#[test]
fn identity_changes_when_tensor_format_or_layout_change() {
    let base = birefnet_manifest().to_model_identity();

    let mut nhwc = birefnet_manifest();
    nhwc.input.tensor_format = TensorFormat::Nhwc;
    assert_ne!(
        base.extras.get(INPUT_SPEC_DIGEST_KEY),
        nhwc.to_model_identity().extras.get(INPUT_SPEC_DIGEST_KEY),
    );

    let mut other_tensor = birefnet_manifest();
    other_tensor.input.tensor_name = "pixels".into();
    assert_ne!(
        base.extras.get(INPUT_SPEC_DIGEST_KEY),
        other_tensor
            .to_model_identity()
            .extras
            .get(INPUT_SPEC_DIGEST_KEY),
    );
}

#[test]
fn manifest_rejects_unknown_field() {
    let mut value = serde_json::to_value(birefnet_manifest()).unwrap();
    value["unknown_top_level"] = serde_json::json!(1);
    let err = ModelManifest::from_json(&value.to_string()).unwrap_err();
    assert!(
        err.to_string().contains("unknown field"),
        "expected unknown-field error, got {err}"
    );
}

#[test]
fn no_capabilities_set_is_rejected() {
    let json = serde_json::json!({
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
        matches!(err, OnnxError::UnsupportedModel { .. }),
        "expected UnsupportedModel, got {err:?}"
    );
}

#[test]
fn capabilities_reject_unknown_field() {
    let json = serde_json::json!({
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

// F-083 #2 — every SAM 2.1 variant descriptor is well-formed.
#[test]
fn sam2_1_manifests_yield_four_valid_variants() {
    let manifests = sam2_1_manifests();
    assert_eq!(manifests.len(), 4);
    let names: Vec<&str> = manifests.iter().map(|m| m.model_name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "sam2.1_hiera_tiny",
            "sam2.1_hiera_small",
            "sam2.1_hiera_base_plus",
            "sam2.1_hiera_large",
        ]
    );
    for m in &manifests {
        // Three interactive capabilities declared.
        assert!(m.capabilities.box_prompt);
        assert!(m.capabilities.point_prompt);
        assert!(m.capabilities.mask_prompt);
        // Not automatic subject segmentation.
        assert!(!m.capabilities.subject_segmentation);
        // Valid input spec: 1024² RGB NCHW, encoder tensor named `images`.
        assert_eq!(m.input.resolution.width, SAM2_INFERENCE_WIDTH);
        assert_eq!(m.input.resolution.height, SAM2_INFERENCE_HEIGHT);
        assert_eq!(m.input.channel_layout, ChannelLayout::Rgb);
        assert_eq!(m.input.tensor_format, TensorFormat::Nchw);
        assert_eq!(m.input.tensor_name, "images");
        // model_hash uses the same placeholder format as BiRefNet.
        assert_eq!(m.model_hash, "pending-integration");
        assert_eq!(m.license, "Apache-2.0");
        assert!(m.validate().is_ok());
    }
}

#[test]
fn sam2_1_manifest_matches_variant_name() {
    for v in Sam2Variant::ALL {
        let m = sam2_1_manifest(v);
        assert_eq!(m.model_name, v.model_name());
        assert_eq!(m.model_version, SAM2_RELEASE_VERSION);
    }
}

// F-083 #3 — deterministic variant selection with exact thresholds.
#[test]
fn select_variant_boundaries() {
    assert_eq!(
        select_variant(&DeviceProfile::with_cores(3)),
        Sam2Variant::Tiny
    );
    assert_eq!(
        select_variant(&DeviceProfile::with_cores(4)),
        Sam2Variant::Small
    );
    assert_eq!(
        select_variant(&DeviceProfile::with_cores(7)),
        Sam2Variant::Small
    );
    assert_eq!(
        select_variant(&DeviceProfile::with_cores(8)),
        Sam2Variant::BasePlus
    );
    assert_eq!(
        select_variant(&DeviceProfile::with_cores(15)),
        Sam2Variant::BasePlus
    );
    assert_eq!(
        select_variant(&DeviceProfile::with_cores(16)),
        Sam2Variant::Large
    );
    // Above the top threshold still maps to large.
    assert_eq!(
        select_variant(&DeviceProfile::with_cores(128)),
        Sam2Variant::Large
    );
}

#[test]
fn select_variant_override_wins() {
    // Override beats the core-count heuristic at every threshold.
    assert_eq!(
        select_variant(&DeviceProfile::with_override(Sam2Variant::Large)),
        Sam2Variant::Large
    );
    assert_eq!(
        select_variant(&DeviceProfile::new(3, Some(Sam2Variant::Large))),
        Sam2Variant::Large,
        "override must win even on a tiny-core profile"
    );
    assert_eq!(
        select_variant(&DeviceProfile::new(128, Some(Sam2Variant::Tiny))),
        Sam2Variant::Tiny,
        "override must win even on a high-core profile"
    );
}

#[test]
fn select_variant_zero_cores_is_tiny_fallback() {
    // The conservative fallback (cores == 0, as returned when
    // available_parallelism cannot be queried) maps to tiny.
    assert_eq!(
        select_variant(&DeviceProfile::with_cores(0)),
        Sam2Variant::Tiny
    );
}

#[test]
fn detect_returns_a_valid_variant() {
    // detect() must not panic and must yield one of the four variants.
    let profile = DeviceProfile::detect();
    let variant = profile.select_variant();
    assert!(Sam2Variant::ALL.contains(&variant));
    // The method and free function agree.
    assert_eq!(profile.select_variant(), select_variant(&profile));
}

// REVIEW-ONNX-N2 — non-empty hash/license, valid resolutions, non-empty
// tensor names and a sane normalization are enforced by validate().
#[test]
fn validate_rejects_empty_hash_and_license() {
    let mut m = birefnet_manifest();
    m.model_hash = "  ".to_owned();
    let err = m.validate().unwrap_err();
    assert!(
        err.to_string().contains("model_hash must not be empty"),
        "got {err}"
    );

    let mut m = birefnet_manifest();
    m.license = String::new();
    let err = m.validate().unwrap_err();
    assert!(
        err.to_string().contains("license must not be empty"),
        "got {err}"
    );
}

#[test]
fn validate_rejects_zero_resolution() {
    let mut m = birefnet_manifest();
    m.input.resolution = Resolution {
        width: 0,
        height: 32,
    };
    let err = m.validate().unwrap_err();
    assert!(err.to_string().contains("must be non-zero"), "got {err}");
}

#[test]
fn validate_rejects_empty_tensor_names() {
    let mut m = birefnet_manifest();
    m.input.tensor_name = String::new();
    assert!(m.validate().is_err());

    let mut m = birefnet_manifest();
    m.output_tensor_name = " ".to_owned();
    assert!(m
        .validate()
        .unwrap_err()
        .to_string()
        .contains("output_tensor_name must not be empty"));
}

#[test]
fn validate_rejects_degenerate_normalization() {
    let mut m = sam2_1_manifest(Sam2Variant::Tiny);
    m.input.normalization.std = [0.229, 0.0, 0.225];
    let err = m.validate().unwrap_err();
    assert!(
        err.to_string().contains("normalization.std[1]"),
        "zero std must be rejected, got {err}"
    );

    let mut m = sam2_1_manifest(Sam2Variant::Tiny);
    m.input.normalization.mean[2] = f32::NAN;
    assert!(m
        .validate()
        .unwrap_err()
        .to_string()
        .contains("normalization.mean[2] must be finite"));
}

#[test]
fn from_json_enforces_field_invariants_too() {
    let json = serde_json::json!({
        "model_name": "X",
        "model_version": "1",
        "model_hash": "",
        "license": "MIT",
        "input": {
            "resolution": {"width": 4, "height": 4},
            "channel_layout": "rgb",
            "tensor_name": "input",
            "tensor_format": "nchw"
        },
        "capabilities": {"subject_segmentation": true}
    })
    .to_string();
    let err = ModelManifest::from_json(&json).unwrap_err();
    assert!(
        matches!(err, OnnxError::InvalidManifest(_)),
        "empty hash via JSON must be rejected, got {err:?}"
    );
}

/// Manifests written before `normalization`/`output_tensor_name` existed
/// must keep parsing: normalization defaults to ImageNet, output name to
/// `output` (documented single-output v1 contract).
#[test]
fn from_json_defaults_new_fields_for_pre_existing_manifests() {
    let json = serde_json::json!({
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
            "subject_segmentation": true,
            "box_prompt": false,
            "point_prompt": false,
            "mask_prompt": false,
            "class_detection": false,
            "instance_segmentation": false
        }
    })
    .to_string();
    let m = ModelManifest::from_json(&json).unwrap();
    assert_eq!(m.output_tensor_name, "output");
    assert_eq!(m.input.normalization, InputNormalization::IMAGENET);
}

/// Explicitly serialized manifests round-trip the new fields verbatim.
#[test]
fn json_roundtrip_preserves_normalization_and_output_name() {
    let mut m = birefnet_manifest();
    m.input.normalization = InputNormalization {
        mean: [0.5, 0.5, 0.5],
        std: [0.25, 0.25, 0.25],
    };
    m.output_tensor_name = "matte".into();
    let back = ModelManifest::from_json(&m.to_json().unwrap()).unwrap();
    assert_eq!(m, back);
}
