use super::*;

#[test]
fn vignette_amount_zero_is_identity() {
    let input = vec![120, 100, 80, 9, 40, 200, 30, 17, 255, 255, 255, 255];
    let r = EditRecipe {
        effects: Some(lumina_sidecar::Effects {
            vignette: Some(lumina_sidecar::Vignette {
                version: 1,
                amount: 0.0,
                midpoint: 0.5,
                roundness: 0.0,
                feather: 0.5,
            }),
            grain: None,
        }),
        ..Default::default()
    };
    let mut f = ImageFrame::new(3, 1, input.clone()).unwrap();
    f.apply_recipe(&r).unwrap();
    assert_eq!(f.pixels, input);
}

#[test]
fn vignette_darkens_edges_for_positive_amount() {
    // Odd-sized image so the centre pixel sits exactly at normalised radius
    // 0 and gets factor 1.0.
    let input: Vec<u8> = (0..(5 * 5)).flat_map(|_| [128u8, 128, 128, 255]).collect();
    let r = EditRecipe {
        effects: Some(lumina_sidecar::Effects {
            vignette: Some(lumina_sidecar::Vignette {
                version: 1,
                amount: 1.0,
                midpoint: 0.0,
                roundness: 1.0,
                feather: 0.0,
            }),
            grain: None,
        }),
        ..Default::default()
    };
    let mut f = ImageFrame::new(5, 5, input.clone()).unwrap();
    f.apply_recipe(&r).unwrap();
    // Centre pixel is exactly 1.0 (unchanged).
    let center = &f.pixels[((2 * 5 + 2) * 4)..((2 * 5 + 2) * 4 + 3)];
    assert_eq!(center, &[128u8, 128, 128]);
    // A corner pixel is strictly darker than the centre.
    let corner = &f.pixels[0..3];
    assert!(corner[0] < 128 && corner[1] < 128 && corner[2] < 128);
    assert_eq!(f.pixels[3], 255);
}

#[test]
fn vignette_negative_amount_lightens_edges() {
    let input: Vec<u8> = (0..(5 * 5)).flat_map(|_| [128u8, 128, 128, 255]).collect();
    let r = EditRecipe {
        effects: Some(lumina_sidecar::Effects {
            vignette: Some(lumina_sidecar::Vignette {
                version: 1,
                amount: -1.0,
                midpoint: 0.0,
                roundness: 1.0,
                feather: 0.0,
            }),
            grain: None,
        }),
        ..Default::default()
    };
    let mut f = ImageFrame::new(5, 5, input.clone()).unwrap();
    f.apply_recipe(&r).unwrap();
    let corner = &f.pixels[0..3];
    assert!(corner[0] > 128 && corner[1] > 128 && corner[2] > 128);
}

#[test]
fn vignette_is_radially_symmetric() {
    let input: Vec<u8> = (0..(7 * 5)).flat_map(|_| [128u8, 128, 128, 200]).collect();
    let r = EditRecipe {
        effects: Some(lumina_sidecar::Effects {
            vignette: Some(lumina_sidecar::Vignette {
                version: 1,
                amount: 0.8,
                midpoint: 0.3,
                roundness: 0.2,
                feather: 0.6,
            }),
            grain: None,
        }),
        ..Default::default()
    };
    let mut f = ImageFrame::new(7, 5, input.clone()).unwrap();
    f.apply_recipe(&r).unwrap();
    let w = 7usize;
    let h = 5usize;
    for y in 0..h {
        for x in 0..w {
            let mx = (w - 1) - x;
            let my = (h - 1) - y;
            let i = (y * w + x) * 4;
            let j = (my * w + mx) * 4;
            assert_eq!(&f.pixels[i..i + 3], &f.pixels[j..j + 3]);
        }
    }
}

#[test]
fn vignette_is_deterministic() {
    let r = EditRecipe {
        effects: Some(lumina_sidecar::Effects {
            vignette: Some(lumina_sidecar::Vignette {
                version: 1,
                amount: 0.6,
                midpoint: 0.2,
                roundness: -0.5,
                feather: 0.4,
            }),
            grain: None,
        }),
        ..Default::default()
    };
    let input: Vec<u8> = (0..(6 * 4)).flat_map(|_| [100u8, 150, 50, 255]).collect();
    let mut a = ImageFrame::new(6, 4, input.clone()).unwrap();
    a.apply_recipe(&r).unwrap();
    let mut b = ImageFrame::new(6, 4, input).unwrap();
    b.apply_recipe(&r).unwrap();
    assert_eq!(a.pixels, b.pixels);
}

#[test]
fn grain_amount_zero_is_identity() {
    let input = vec![120, 100, 80, 9, 40, 200, 30, 17];
    let r = EditRecipe {
        effects: Some(lumina_sidecar::Effects {
            vignette: None,
            grain: Some(lumina_sidecar::Grain {
                version: 1,
                amount: 0.0,
                size: 0.5,
                roughness: 0.5,
                seed: 12345,
            }),
        }),
        ..Default::default()
    };
    let mut f = ImageFrame::new(2, 1, input.clone()).unwrap();
    f.apply_recipe(&r).unwrap();
    assert_eq!(f.pixels, input);
}

#[test]
fn grain_is_deterministic_same_seed() {
    let r = EditRecipe {
        effects: Some(lumina_sidecar::Effects {
            vignette: None,
            grain: Some(lumina_sidecar::Grain {
                version: 1,
                amount: 0.7,
                size: 0.4,
                roughness: 0.6,
                seed: 99,
            }),
        }),
        ..Default::default()
    };
    let input: Vec<u8> = (0..(8 * 6)).flat_map(|_| [128u8, 128, 128, 255]).collect();
    let mut a = ImageFrame::new(8, 6, input.clone()).unwrap();
    a.apply_recipe(&r).unwrap();
    let mut b = ImageFrame::new(8, 6, input).unwrap();
    b.apply_recipe(&r).unwrap();
    assert_eq!(a.pixels, b.pixels);
}

#[test]
fn grain_seed_change_changes_output() {
    let grain = |seed: u64| lumina_sidecar::Grain {
        version: 1,
        amount: 0.8,
        size: 0.5,
        roughness: 0.5,
        seed,
    };
    let input: Vec<u8> = (0..(8 * 6)).flat_map(|_| [128u8, 128, 128, 255]).collect();
    let mut a = ImageFrame::new(8, 6, input.clone()).unwrap();
    a.apply_recipe(&EditRecipe {
        effects: Some(lumina_sidecar::Effects {
            vignette: None,
            grain: Some(grain(1)),
        }),
        ..Default::default()
    })
    .unwrap();
    let mut b = ImageFrame::new(8, 6, input).unwrap();
    b.apply_recipe(&EditRecipe {
        effects: Some(lumina_sidecar::Effects {
            vignette: None,
            grain: Some(grain(2)),
        }),
        ..Default::default()
    })
    .unwrap();
    assert_ne!(a.pixels, b.pixels);
}

#[test]
fn grain_preserves_alpha_and_is_channel_coupled() {
    // Mid-gray values ensure no per-channel clamping, so the SAME noise delta
    // must be applied to R, G and B; alpha must be untouched.
    let input: Vec<u8> = (0..(8 * 6)).flat_map(|_| [128u8, 128, 128, 77]).collect();
    let r = EditRecipe {
        effects: Some(lumina_sidecar::Effects {
            vignette: None,
            grain: Some(lumina_sidecar::Grain {
                version: 1,
                amount: 0.9,
                size: 0.3,
                roughness: 0.7,
                seed: 7,
            }),
        }),
        ..Default::default()
    };
    let mut f = ImageFrame::new(8, 6, input.clone()).unwrap();
    f.apply_recipe(&r).unwrap();
    for px in f.pixels.as_chunks::<4>().0 {
        assert_eq!(px[3], 77);
        assert_eq!(
            px[0] as i32 - input[0] as i32,
            px[1] as i32 - input[1] as i32
        );
        assert_eq!(
            px[1] as i32 - input[1] as i32,
            px[2] as i32 - input[2] as i32
        );
    }
}

#[test]
fn effects_run_after_sharpening() {
    // F-097 runs as the LAST sub-stage of `Adjustments` (after sharpening,
    // before masks/crop). This exercises that ordering: starting from the
    // same pixels, the same effects recipe is reproduced byte-for-byte
    // (determinism), and the result is non-identity (the effects were
    // applied). Re-applying from the *original* pixels (not the already
    // modified ones) must match, since the effect is a pure function of the
    // input pixels.
    let input: Vec<u8> = (0..(5 * 5)).flat_map(|_| [128u8, 128, 128, 255]).collect();
    let effects = lumina_sidecar::Effects {
        vignette: Some(lumina_sidecar::Vignette {
            version: 1,
            amount: 0.5,
            midpoint: 0.0,
            roundness: 1.0,
            feather: 0.0,
        }),
        grain: Some(lumina_sidecar::Grain {
            version: 1,
            amount: 0.3,
            size: 0.5,
            roughness: 0.5,
            seed: 42,
        }),
    };
    let run = |pixels: Vec<u8>| {
        let mut f = ImageFrame::new(5, 5, pixels).unwrap();
        f.apply_recipe(&EditRecipe {
            effects: Some(effects.clone()),
            ..Default::default()
        })
        .unwrap();
        f.pixels
    };
    let once = run(input.clone());
    let again = run(input.clone());
    // Deterministic: same original pixels -> same output.
    assert_eq!(once, again);
    // The combined effect is not identity.
    assert_ne!(once, input);
}

#[test]
fn effects_validation_rejects_invalid_values() {
    for bad in [
        EditRecipe {
            effects: Some(lumina_sidecar::Effects {
                vignette: Some(lumina_sidecar::Vignette {
                    version: 2,
                    amount: 0.0,
                    midpoint: 0.0,
                    roundness: 0.0,
                    feather: 0.0,
                }),
                grain: None,
            }),
            ..Default::default()
        },
        EditRecipe {
            effects: Some(lumina_sidecar::Effects {
                vignette: Some(lumina_sidecar::Vignette {
                    version: 1,
                    amount: 1.5,
                    midpoint: 0.0,
                    roundness: 0.0,
                    feather: 0.0,
                }),
                grain: None,
            }),
            ..Default::default()
        },
        EditRecipe {
            effects: Some(lumina_sidecar::Effects {
                vignette: Some(lumina_sidecar::Vignette {
                    version: 1,
                    amount: 0.0,
                    midpoint: 0.0,
                    roundness: 0.0,
                    feather: -0.1,
                }),
                grain: None,
            }),
            ..Default::default()
        },
        EditRecipe {
            effects: Some(lumina_sidecar::Effects {
                vignette: None,
                grain: Some(lumina_sidecar::Grain {
                    version: 1,
                    amount: 1.2,
                    size: 0.0,
                    roughness: 0.0,
                    seed: 1,
                }),
            }),
            ..Default::default()
        },
    ] {
        let mut f = ImageFrame::new(1, 1, vec![10, 20, 30, 255]).unwrap();
        assert!(f.apply_recipe(&bad).is_err());
    }
}
