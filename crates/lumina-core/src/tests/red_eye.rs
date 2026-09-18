use super::*;

#[test]
fn noise_reduction_identity_determinism_and_alpha() {
    let input = vec![40, 42, 44, 9, 200, 198, 196, 17, 41, 45, 43, 25];
    let mut a = ImageFrame::new(3, 1, input.clone()).unwrap();
    a.apply_recipe(&EditRecipe {
        noise_reduction: Some(lumina_sidecar::NoiseReduction {
            version: 1,
            luminance: 0.0,
            color: 0.0,
        }),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(a.pixels, input);
    let r = EditRecipe {
        noise_reduction: Some(lumina_sidecar::NoiseReduction {
            version: 1,
            luminance: 0.8,
            color: 0.8,
        }),
        ..Default::default()
    };
    let mut b =
        ImageFrame::new(3, 1, vec![40, 42, 44, 9, 200, 198, 196, 17, 41, 45, 43, 25]).unwrap();
    let original = b.pixels.clone();
    b.apply_recipe(&r).unwrap();
    let once = b.pixels.clone();
    let mut c = ImageFrame::new(3, 1, original).unwrap();
    c.apply_recipe(&r).unwrap();
    assert_eq!(once, c.pixels);
    assert_eq!(&once[3..4], &[9]);
}

#[test]
fn red_eye_identity_and_determinism_and_alpha() {
    let (frame, _) = red_eye_frame();
    // `None` is identity.
    let mut none = frame.clone();
    none.apply_recipe(&EditRecipe::default()).unwrap();
    assert_eq!(none.pixels, frame.pixels);
    // Empty region list is identity.
    let mut empty = frame.clone();
    empty
        .apply_recipe(&EditRecipe {
            red_eye: Some(lumina_sidecar::RedEyeCorrection {
                version: 1,
                regions: Vec::new(),
            }),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(empty.pixels, frame.pixels);
    // Zero strengths are identity.
    let mut zero = frame.clone();
    zero.apply_recipe(&red_eye_recipe(0.0, 0.0)).unwrap();
    assert_eq!(zero.pixels, frame.pixels);
    // Determinism: two runs are byte-identical, alpha preserved.
    let recipe = red_eye_recipe(1.0, 0.5);
    let mut a = frame.clone();
    a.apply_recipe(&recipe).unwrap();
    let mut b = frame.clone();
    b.apply_recipe(&recipe).unwrap();
    assert_eq!(a.pixels, b.pixels);
    for pixel in a.pixels.as_chunks::<4>().0 {
        assert!(pixel[3] == 200 || pixel[3] == 77);
    }
}

#[test]
fn red_eye_corrects_red_pupil_and_spares_grey() {
    let (frame, center) = red_eye_frame();
    let mut corrected = frame.clone();
    corrected.apply_recipe(&red_eye_recipe(1.0, 0.0)).unwrap();
    // Red pupil pixel: red channel pulled toward luminance (down),
    // green/blue unchanged by desaturation alone.
    assert!(corrected.pixels[center] < frame.pixels[center]);
    assert_eq!(corrected.pixels[center + 1], frame.pixels[center + 1]);
    assert_eq!(corrected.pixels[center + 2], frame.pixels[center + 2]);
    // Grey surround pixel far from the pupil is untouched.
    assert_eq!(&corrected.pixels[..4], &frame.pixels[..4]);
    // Darkening additionally lowers all three channels of the pupil.
    let mut darkened = frame.clone();
    darkened.apply_recipe(&red_eye_recipe(1.0, 1.0)).unwrap();
    assert!(darkened.pixels[center] <= corrected.pixels[center]);
    assert!(darkened.pixels[center + 1] <= frame.pixels[center + 1]);
    assert!(darkened.pixels[center + 2] <= frame.pixels[center + 2]);
}

#[test]
fn red_eye_monotonicity_and_clipping() {
    let (frame, center) = red_eye_frame();
    // Monotonicity over the full strength grid: stronger desaturation
    // never raises the red channel, stronger darkening never raises any
    // channel; every output stays within `0..=255` (clipping property).
    let mut prev_r = u8::MAX;
    for step in 0..=10 {
        let s = step as f32 / 10.0;
        let mut f = frame.clone();
        f.apply_recipe(&red_eye_recipe(s, 0.0)).unwrap();
        assert!(f.pixels[center] <= prev_r);
        prev_r = f.pixels[center];
    }
    let mut prev = [u8::MAX; 3];
    for step in 0..=10 {
        let s = step as f32 / 10.0;
        let mut f = frame.clone();
        f.apply_recipe(&red_eye_recipe(1.0, s)).unwrap();
        assert!(f.pixels[center] <= prev[0]);
        assert!(f.pixels[center + 1] <= prev[1]);
        assert!(f.pixels[center + 2] <= prev[2]);
        prev = [f.pixels[center], f.pixels[center + 1], f.pixels[center + 2]];
    }
    // Value-range sweep: every channel combination in a tiny frame stays
    // in range and alpha is preserved under full strength.
    for v in [0u8, 1, 127, 128, 254, 255] {
        let input = vec![v, v, v, 9, 255, 0, 0, 10, 0, 255, 0, 11];
        let mut f = ImageFrame::new(3, 1, input.clone()).unwrap();
        f.apply_recipe(&red_eye_recipe(1.0, 1.0)).unwrap();
        assert_eq!(&f.pixels[3..4], &[9]);
        assert_eq!(&f.pixels[7..8], &[10]);
        assert_eq!(&f.pixels[11..12], &[11]);
    }
    // Pure-red pixel under full correction: red dominance is removed
    // deterministically (desaturate pulls R toward luminance, darken
    // scales the rest to black).
    let mut f = ImageFrame::new(1, 1, vec![255, 0, 0, 255]).unwrap();
    f.apply_recipe(&EditRecipe {
        red_eye: Some(lumina_sidecar::RedEyeCorrection {
            version: 1,
            regions: vec![lumina_sidecar::RedEyeRegion {
                id: "re-1".into(),
                x: 0.0,
                y: 0.0,
                radius: 1.0,
                desaturate: 1.0,
                darken: 1.0,
            }],
        }),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(f.pixels, vec![0, 0, 0, 255]);
}

#[test]
fn red_eye_validation_rejects_invalid_values() {
    // Unsupported version.
    let mut recipe = red_eye_recipe(0.5, 0.5);
    recipe.red_eye.as_mut().unwrap().version = 2;
    assert!(ImageFrame::new(1, 1, vec![10, 10, 10, 255])
        .unwrap()
        .apply_recipe(&recipe)
        .is_err());
    // Each out-of-range / non-finite mutation fails loudly.
    for mutate in [
        |r: &mut lumina_sidecar::RedEyeRegion| r.x = 2.0,
        |r: &mut lumina_sidecar::RedEyeRegion| r.y = f32::NAN,
        |r: &mut lumina_sidecar::RedEyeRegion| r.radius = 0.0,
        |r: &mut lumina_sidecar::RedEyeRegion| r.radius = 1.1,
        |r: &mut lumina_sidecar::RedEyeRegion| r.radius = f32::INFINITY,
        |r: &mut lumina_sidecar::RedEyeRegion| r.desaturate = -1.0,
        |r: &mut lumina_sidecar::RedEyeRegion| r.desaturate = f32::NAN,
        |r: &mut lumina_sidecar::RedEyeRegion| r.darken = 1.5,
        |r: &mut lumina_sidecar::RedEyeRegion| r.darken = f32::NEG_INFINITY,
    ] {
        let mut candidate = red_eye_recipe(0.5, 0.5);
        mutate(&mut candidate.red_eye.as_mut().unwrap().regions[0]);
        assert!(ImageFrame::new(1, 1, vec![10, 10, 10, 255])
            .unwrap()
            .apply_recipe(&candidate)
            .is_err());
    }
    // Empty and duplicate ids are rejected.
    let mut candidate = red_eye_recipe(0.5, 0.5);
    candidate.red_eye.as_mut().unwrap().regions[0].id.clear();
    assert!(ImageFrame::new(1, 1, vec![10, 10, 10, 255])
        .unwrap()
        .apply_recipe(&candidate)
        .is_err());
    let mut candidate = red_eye_recipe(0.5, 0.5);
    candidate
        .red_eye
        .as_mut()
        .unwrap()
        .regions
        .push(lumina_sidecar::RedEyeRegion {
            id: "re-1".into(),
            x: 0.1,
            y: 0.1,
            radius: 0.1,
            desaturate: 0.5,
            darken: 0.5,
        });
    assert!(ImageFrame::new(1, 1, vec![10, 10, 10, 255])
        .unwrap()
        .apply_recipe(&candidate)
        .is_err());
}

/// G-14 (L3): dedicated CPU ordering test. Red-Eye is applied *after*
/// sharpening (F-095) and *before* the effects (F-097). Using only the
/// public `apply_recipe` API, the combined recipe must equal the sequential
/// `sharpening → red_eye → effects` composition and differ from both
/// reversed boundary orders, so the documented order is material on this
/// fixture (not accidentally commutative).
#[test]
fn red_eye_runs_after_sharpening_and_before_effects() {
    let (frame, _) = red_eye_frame();
    let sharpening = lumina_sidecar::Sharpening {
        version: 1,
        amount: 2.0,
        radius: 2.0,
        detail: 1.0,
        masking: 0.0,
    };
    let red_eye = red_eye_recipe(1.0, 0.6).red_eye.unwrap();
    let effects = lumina_sidecar::Effects {
        vignette: None,
        grain: Some(lumina_sidecar::Grain {
            version: 1,
            amount: 0.3,
            size: 0.5,
            roughness: 0.5,
            seed: 7,
        }),
    };
    let run = |recipes: &[EditRecipe]| {
        let mut frame = frame.clone();
        for recipe in recipes {
            frame.apply_recipe(recipe).unwrap();
        }
        frame.pixels
    };
    let sharp = EditRecipe {
        sharpening: Some(sharpening),
        ..Default::default()
    };
    let redeye = EditRecipe {
        red_eye: Some(red_eye.clone()),
        ..Default::default()
    };
    let fx = EditRecipe {
        effects: Some(effects.clone()),
        ..Default::default()
    };
    let combined = run(&[EditRecipe {
        sharpening: Some(sharpening),
        red_eye: Some(red_eye),
        effects: Some(effects),
        ..Default::default()
    }]);
    let canonical = run(&[sharp.clone(), redeye.clone(), fx.clone()]);
    assert_eq!(
        combined, canonical,
        "combined recipe must compose differently only via the documented order"
    );
    assert_ne!(
        combined,
        run(&[redeye.clone(), sharp.clone(), fx.clone()]),
        "red-eye must run after sharpening"
    );
    assert_ne!(
        combined,
        run(&[sharp.clone(), fx.clone(), redeye.clone()]),
        "red-eye must run before effects"
    );
}
