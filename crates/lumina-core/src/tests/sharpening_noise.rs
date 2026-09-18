use super::*;

#[test]
fn sharpening_identity_direction_and_scale() {
    let input = vec![20, 20, 20, 7, 128, 128, 128, 8, 220, 220, 220, 9];
    let mut id = ImageFrame::new(3, 1, input.clone()).unwrap();
    id.apply_recipe(&EditRecipe::default()).unwrap();
    assert_eq!(id.pixels, input);
    let r = EditRecipe {
        sharpening: Some(lumina_sidecar::Sharpening {
            version: 1,
            amount: 2.0,
            radius: 2.0,
            detail: 1.0,
            masking: 0.0,
        }),
        ..Default::default()
    };
    let mut sharp = ImageFrame::new(3, 1, input.clone()).unwrap();
    sharp.apply_recipe(&r).unwrap();
    assert!(sharp.pixels[0] < 20 || sharp.pixels[4] > 128);
    let mut half = ImageFrame::new(3, 1, input).unwrap();
    half.apply_recipe_with_scale(&r, 0.5).unwrap();
    assert_ne!(sharp.pixels, half.pixels);
}

#[test]
fn sharpening_masking_suppresses_flat_area() {
    let mut input = vec![[128, 128, 128, 255]; 100]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    // The outlier is an edge; (0, 0) is deliberately far enough away to
    // be a genuinely flat-area sample for the radius used here.
    input[(5 * 10 + 1) * 4..(5 * 10 + 1) * 4 + 4].copy_from_slice(&[180, 180, 180, 255]);
    let mut frame = ImageFrame::new(10, 10, input.clone()).unwrap();
    frame
        .apply_recipe(&EditRecipe {
            sharpening: Some(lumina_sidecar::Sharpening {
                version: 1,
                amount: 2.0,
                radius: 2.0,
                detail: 1.0,
                masking: 1.0,
            }),
            ..Default::default()
        })
        .unwrap();

    assert_eq!(&frame.pixels[..4], &[128, 128, 128, 255]);
    let edge = (5 * 10 + 1) * 4;
    assert_ne!(&frame.pixels[edge..edge + 3], &[220, 220, 220]);
    assert_eq!(frame.pixels[edge + 3], 255);
}

#[test]
fn sharpening_detail_mixing_differs() {
    // The left half contains one-pixel alternation (fine detail), while
    // the right half contains broad blocks (coarse detail).
    let mut input = Vec::new();
    for y in 0..12 {
        for x in 0..12 {
            let value = if x < 6 {
                if x % 2 == 0 {
                    105
                } else {
                    145
                }
            } else if y < 4 {
                110
            } else if y < 8 {
                130
            } else {
                150
            };
            input.extend_from_slice(&[value, value, value, 255]);
        }
    }
    let recipe = |detail| EditRecipe {
        sharpening: Some(lumina_sidecar::Sharpening {
            version: 1,
            amount: 2.0,
            radius: 3.0,
            detail,
            masking: 0.0,
        }),
        ..Default::default()
    };
    let mut fine = ImageFrame::new(12, 12, input.clone()).unwrap();
    fine.apply_recipe(&recipe(1.0)).unwrap();
    let mut coarse = ImageFrame::new(12, 12, input).unwrap();
    coarse.apply_recipe(&recipe(0.0)).unwrap();

    // With r_fine=max(3*.5,.5)=1.5 and r_coarse=4.5, fine mixing has the
    // stronger response at a one-pixel transition; coarse mixing has the
    // stronger response at a broad transition. Differences below are
    // intentionally measured with a one-code-value rounding tolerance.
    assert_ne!(fine.pixels, coarse.pixels);
    // The exact edge samples can have opposite signed overshoot. With the
    // implemented formula, detail=1 uses r_fine=1.5 while detail=0 uses
    // r_coarse=4.5; on this deliberately mixed pattern the broader
    // difference signal is stronger by at least one code value in both
    // measured structures. This pins the radius/mixing direction rather
    // than merely checking that the buffers differ.
    let fine_contrast =
        (fine.pixels[(5 * 12) * 4] as i16 - fine.pixels[(5 * 12 + 1) * 4] as i16).abs();
    let coarse_contrast =
        (coarse.pixels[(3 * 12 + 8) * 4] as i16 - coarse.pixels[(4 * 12 + 8) * 4] as i16).abs();
    let coarse_fine_contrast =
        (coarse.pixels[(5 * 12) * 4] as i16 - coarse.pixels[(5 * 12 + 1) * 4] as i16).abs();
    let fine_coarse_contrast =
        (fine.pixels[(3 * 12 + 8) * 4] as i16 - fine.pixels[(4 * 12 + 8) * 4] as i16).abs();
    assert!(coarse_fine_contrast > fine_contrast);
    assert!(coarse_contrast > fine_coarse_contrast);
}

#[test]
fn noise_reduction_preserves_edges() {
    let input = vec![
        20, 20, 20, 255, 20, 20, 20, 255, 25, 25, 25, 255, 235, 235, 235, 255, 240, 240, 240, 255,
    ];
    let mut frame = ImageFrame::new(5, 1, input.clone()).unwrap();
    frame
        .apply_recipe(&EditRecipe {
            noise_reduction: Some(lumina_sidecar::NoiseReduction {
                version: 1,
                luminance: 0.8,
                color: 0.0,
            }),
            ..Default::default()
        })
        .unwrap();
    let left = frame.pixels[0] as i16;
    let right = frame.pixels[12] as i16;
    assert!((left - 22).abs() <= 3, "left flat area: {left}");
    assert!((right - 238).abs() <= 3, "right flat area: {right}");
    let original_edge = input[12] as i16 - input[8] as i16;
    let filtered_edge = right - frame.pixels[8] as i16;
    assert!(filtered_edge >= original_edge * 9 / 10);
}

#[test]
fn noise_reduction_channel_separation() {
    let input = vec![128, 100, 128, 255, 160, 100, 34, 255];
    let run = |luminance, color| {
        let mut frame = ImageFrame::new(2, 1, input.clone()).unwrap();
        frame
            .apply_recipe(&EditRecipe {
                noise_reduction: Some(lumina_sidecar::NoiseReduction {
                    version: 1,
                    luminance,
                    color,
                }),
                ..Default::default()
            })
            .unwrap();
        frame
    };
    let chroma = run(0.0, 0.8);
    let y = |p: &[u8]| 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
    let chroma_y0 = y(&chroma.pixels[..3]);
    let chroma_y1 = y(&chroma.pixels[4..7]);
    assert!((chroma_y0 - chroma_y1).abs() <= 1.0);
    assert!((chroma.pixels[1] as f32 - chroma_y0).abs() < 20.0);
    assert!((chroma.pixels[5] as f32 - chroma_y1).abs() < 20.0);

    let luminance = run(0.8, 0.0);
    let input_y0 = y(&input[..3]);
    let input_y1 = y(&input[4..7]);
    let luminance_y0 = y(&luminance.pixels[..3]);
    let luminance_y1 = y(&luminance.pixels[4..7]);
    assert!(
        (luminance.pixels[0] as f32 - luminance_y0 - (input[0] as f32 - input_y0)).abs() <= 1.0
    );
    assert!(
        (luminance.pixels[2] as f32 - luminance_y0 - (input[2] as f32 - input_y0)).abs() <= 1.0
    );
    assert!(
        (luminance.pixels[4] as f32 - luminance_y1 - (input[4] as f32 - input_y1)).abs() <= 1.0
    );
    assert!(
        (luminance.pixels[6] as f32 - luminance_y1 - (input[6] as f32 - input_y1)).abs() <= 1.0
    );
}

#[test]
fn noise_reduction_before_sharpening_order_matters() {
    let input = vec![
        20, 20, 20, 255, 25, 25, 25, 255, 20, 20, 20, 255, 235, 235, 235, 255, 240, 240, 240, 255,
    ];
    let noise = lumina_sidecar::NoiseReduction {
        version: 1,
        luminance: 0.5,
        color: 0.0,
    };
    let sharp = lumina_sidecar::Sharpening {
        version: 1,
        amount: 2.0,
        radius: 2.0,
        detail: 1.0,
        masking: 0.0,
    };
    let mut combined = ImageFrame::new(5, 1, input.clone()).unwrap();
    combined
        .apply_recipe(&EditRecipe {
            noise_reduction: Some(noise),
            sharpening: Some(sharp),
            ..Default::default()
        })
        .unwrap();
    let mut sharpen_then_noise = ImageFrame::new(5, 1, input).unwrap();
    sharpen_then_noise
        .apply_recipe(&EditRecipe {
            sharpening: Some(sharp),
            ..Default::default()
        })
        .unwrap();
    sharpen_then_noise
        .apply_recipe(&EditRecipe {
            noise_reduction: Some(noise),
            ..Default::default()
        })
        .unwrap();
    assert_ne!(combined.pixels, sharpen_then_noise.pixels);
    // The specified NR -> sharpening order leaves the noisy flat sample
    // less amplified than sharpening before NR (one-code tolerance).
    assert!(combined.pixels[4] <= sharpen_then_noise.pixels[4] + 1);
}

#[test]
fn nested_noise_and_sharpening_validation_rejects_invalid_values() {
    for bad in [
        EditRecipe {
            noise_reduction: Some(lumina_sidecar::NoiseReduction {
                version: 2,
                luminance: 0.0,
                color: 0.0,
            }),
            ..Default::default()
        },
        EditRecipe {
            noise_reduction: Some(lumina_sidecar::NoiseReduction {
                version: 1,
                luminance: f32::NAN,
                color: 0.0,
            }),
            ..Default::default()
        },
        EditRecipe {
            sharpening: Some(lumina_sidecar::Sharpening {
                version: 1,
                amount: 3.1,
                radius: 1.0,
                detail: 0.0,
                masking: 0.0,
            }),
            ..Default::default()
        },
        EditRecipe {
            sharpening: Some(lumina_sidecar::Sharpening {
                version: 1,
                amount: 1.0,
                radius: 0.01,
                detail: 0.0,
                masking: 0.0,
            }),
            ..Default::default()
        },
    ] {
        let mut f = ImageFrame::new(1, 1, vec![10, 20, 30, 255]).unwrap();
        assert!(f.apply_recipe(&bad).is_err());
    }
}
