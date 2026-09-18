use super::*;

// ---- F-097: effects (vignette + grain) recipe schema field ----

#[test]
fn effects_roundtrip_and_validate_ranges() {
    let recipe = EditRecipe {
        effects: Some(Effects {
            vignette: Some(Vignette {
                version: 1,
                amount: -0.6,
                midpoint: 0.35,
                roundness: 0.8,
                feather: 0.2,
            }),
            grain: Some(Grain {
                version: 1,
                amount: 0.5,
                size: 0.75,
                roughness: 0.25,
                seed: 123456789,
            }),
        }),
        ..Default::default()
    };
    let value = serde_json::to_value(&recipe).unwrap();
    assert!(value["effects"]["vignette"].is_object());
    assert!(value["effects"]["grain"].is_object());
    // `effects` lives at the recipe root (like `geometry`), not inside
    // `adjustments`.
    assert!(value["adjustments"].is_object());
    assert!(!value["adjustments"]
        .as_object()
        .unwrap()
        .contains_key("effects"));
    assert_eq!(recipe, serde_json::from_value(value).unwrap());

    // Full sidecar roundtrip preserves the effects block.
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.effects = Some(Effects {
        vignette: Some(Vignette {
            version: 1,
            amount: 0.4,
            midpoint: 0.1,
            roundness: -0.5,
            feather: 0.9,
        }),
        grain: Some(Grain {
            version: 1,
            amount: 0.3,
            size: 0.2,
            roughness: 0.8,
            seed: 42,
        }),
    });
    let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(decoded.virtual_copies[0].recipe, d.virtual_copies[0].recipe);
}

#[test]
fn effects_validation_rejects_invalid_values() {
    // Invalid vignette version.
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.effects = Some(Effects {
        vignette: Some(Vignette {
            version: 2,
            amount: 0.0,
            midpoint: 0.0,
            roundness: 0.0,
            feather: 0.0,
        }),
        grain: None,
    });
    assert!(d.validate().is_err());

    // Out-of-range vignette amount.
    d.virtual_copies[0].recipe.effects = Some(Effects {
        vignette: Some(Vignette {
            version: 1,
            amount: 2.0,
            midpoint: 0.0,
            roundness: 0.0,
            feather: 0.0,
        }),
        grain: None,
    });
    assert!(d.validate().is_err());

    // Out-of-range vignette midpoint (NaN).
    d.virtual_copies[0].recipe.effects = Some(Effects {
        vignette: Some(Vignette {
            version: 1,
            amount: 0.0,
            midpoint: f32::NAN,
            roundness: 0.0,
            feather: 0.0,
        }),
        grain: None,
    });
    assert!(d.validate().is_err());

    // Out-of-range grain amount.
    d.virtual_copies[0].recipe.effects = Some(Effects {
        vignette: None,
        grain: Some(Grain {
            version: 1,
            amount: -0.1,
            size: 0.0,
            roughness: 0.0,
            seed: 1,
        }),
    });
    assert!(d.validate().is_err());

    // Out-of-range grain roughness.
    d.virtual_copies[0].recipe.effects = Some(Effects {
        vignette: None,
        grain: Some(Grain {
            version: 1,
            amount: 0.0,
            size: 1.5,
            roughness: 0.0,
            seed: 1,
        }),
    });
    assert!(d.validate().is_err());
}

/// G-05 Lens Blur: recipe JSON roundtrip (root-level `lens_blur` key with
/// all bokeh variants) plus per-virtual-copy independence.
#[test]
fn lens_blur_roundtrip_and_per_copy_independence() {
    for bokeh in [
        BokehShape::Round,
        BokehShape::Elliptical,
        BokehShape::Hexagonal,
    ] {
        let recipe = EditRecipe {
            lens_blur: Some(LensBlur {
                version: 1,
                enabled: true,
                focus_rect: FocusRect {
                    x: 0.2,
                    y: 0.3,
                    width: 0.4,
                    height: 0.25,
                },
                focal_near: 0.1,
                focal_far: 0.6,
                blur_amount: 0.7,
                bokeh,
                depth_artifact: Some(DepthArtifactRef {
                    relative_path: "depth/map.bin".into(),
                    sha256: "sha256:abc".into(),
                }),
            }),
            ..Default::default()
        };
        let value = serde_json::to_value(&recipe).unwrap();
        assert!(value["lens_blur"].is_object());
        assert_eq!(
            value["lens_blur"]["bokeh"],
            serde_json::to_value(bokeh).unwrap()
        );
        assert!(!value["adjustments"]
            .as_object()
            .unwrap()
            .contains_key("lens_blur"));
        assert_eq!(recipe, serde_json::from_value(value).unwrap());
    }

    // Two virtual copies carry independent lens-blur recipes (stable IDs,
    // no positional identification).
    let mut d = SidecarDocument::new(source(), "pipeline-1");
    d.virtual_copies[0].recipe.lens_blur = Some(LensBlur {
        version: 1,
        enabled: true,
        focus_rect: FocusRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
        focal_near: 0.0,
        focal_far: 1.0,
        blur_amount: 0.25,
        bokeh: BokehShape::Round,
        depth_artifact: None,
    });
    d.duplicate_virtual_copy("vc-original", "vc-blur-hex", "Blur Hex")
        .unwrap();
    let other = d
        .virtual_copies
        .iter_mut()
        .find(|c| c.id == "vc-blur-hex")
        .unwrap();
    other.recipe.lens_blur = Some(LensBlur {
        version: 1,
        enabled: true,
        focus_rect: FocusRect {
            x: 0.1,
            y: 0.1,
            width: 0.5,
            height: 0.5,
        },
        focal_near: 0.2,
        focal_far: 0.4,
        blur_amount: 0.9,
        bokeh: BokehShape::Hexagonal,
        depth_artifact: None,
    });
    let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(decoded.virtual_copies.len(), 2);
    let first = decoded
        .virtual_copies
        .iter()
        .find(|c| c.id == "vc-original")
        .unwrap();
    let second = decoded
        .virtual_copies
        .iter()
        .find(|c| c.id == "vc-blur-hex")
        .unwrap();
    assert_ne!(first.recipe.lens_blur, second.recipe.lens_blur);
    assert_eq!(first.recipe.lens_blur, d.virtual_copies[0].recipe.lens_blur);
    assert!(decoded.validate().is_ok());
}

/// G-05 Lens Blur: every out-of-range value fails loudly (no silent
/// clipping), including absolute depth-artifact paths.
#[test]
fn lens_blur_validation_rejects_invalid_values() {
    fn doc_with(mutate: impl FnOnce(&mut LensBlur)) -> SidecarDocument {
        let mut d = SidecarDocument::new(source(), "pipeline-1");
        let mut b = LensBlur {
            version: 1,
            enabled: true,
            focus_rect: FocusRect {
                x: 0.2,
                y: 0.2,
                width: 0.4,
                height: 0.4,
            },
            focal_near: 0.1,
            focal_far: 0.6,
            blur_amount: 0.5,
            bokeh: BokehShape::Round,
            depth_artifact: None,
        };
        mutate(&mut b);
        d.virtual_copies[0].recipe.lens_blur = Some(b);
        d
    }
    // Bad version.
    assert!(doc_with(|b| b.version = 2).validate().is_err());
    // Out-of-range / non-finite scalars.
    assert!(doc_with(|b| b.focal_near = -0.1).validate().is_err());
    assert!(doc_with(|b| b.focal_far = 1.5).validate().is_err());
    assert!(doc_with(|b| b.blur_amount = f32::NAN).validate().is_err());
    // Inverted focal range.
    assert!(doc_with(|b| {
        b.focal_near = 0.7;
        b.focal_far = 0.6;
    })
    .validate()
    .is_err());
    // Degenerate / out-of-bounds focus rectangles.
    assert!(doc_with(|b| b.focus_rect.width = 0.0).validate().is_err());
    assert!(doc_with(|b| b.focus_rect.x = -0.1).validate().is_err());
    assert!(doc_with(|b| {
        b.focus_rect.x = 0.8;
        b.focus_rect.width = 0.3;
    })
    .validate()
    .is_err());
    // Absolute depth-artifact path (portable sidecars stay relative).
    assert!(doc_with(|b| {
        b.depth_artifact = Some(DepthArtifactRef {
            relative_path: "/abs/depth.bin".into(),
            sha256: "sha256:abc".into(),
        });
    })
    .validate()
    .is_err());
    // Path traversal.
    assert!(doc_with(|b| {
        b.depth_artifact = Some(DepthArtifactRef {
            relative_path: "../depth.bin".into(),
            sha256: "sha256:abc".into(),
        });
    })
    .validate()
    .is_err());
    // The valid base document passes.
    assert!(doc_with(|_| {}).validate().is_ok());
}

#[test]
fn lens_and_perspective_roundtrip_and_validation() {
    let mut d = SidecarDocument::new(source(), "p");
    d.virtual_copies[0]
        .recipe
        .options
        .insert("render_profile".into(), "display-p3".into());
    d.virtual_copies[0].recipe.lens_correction = Some(LensCorrection {
        version: 1,
        profile: Some("wide-light".into()),
        distortion_k1: Some(0.0),
        distortion_k2: Some(0.0),
        distortion_k3: Some(0.0),
        vignette_c0: Some(1.0),
        vignette_c1: Some(0.0),
        vignette_c2: Some(0.0),
        ca_red: Some(0.0),
        ca_blue: Some(0.0),
    });
    d.virtual_copies[0].recipe.perspective = Some(Perspective {
        version: 1,
        vertical: 0.2,
        horizontal: 0.0,
        rotation: 0.0,
        scale: 1.0,
        aspect_ratio: 1.0,
        shift_x: 0.0,
        shift_y: 0.0,
    });
    let decoded = SidecarDocument::from_json(&d.to_json().unwrap()).unwrap();
    assert_eq!(decoded.virtual_copies[0].recipe, d.virtual_copies[0].recipe);
    assert_eq!(
        decoded.virtual_copies[0]
            .recipe
            .options
            .get("render_profile"),
        Some(&"display-p3".to_string())
    );
    d.virtual_copies[0]
        .recipe
        .lens_correction
        .as_mut()
        .unwrap()
        .ca_red = Some(f32::NAN);
    assert!(d.validate().is_err());
}
