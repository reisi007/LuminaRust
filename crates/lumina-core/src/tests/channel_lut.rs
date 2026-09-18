use super::*;

#[test]
fn fused_channel_lut_kernel_is_byte_identical_to_reference() {
    // Deterministic SplitMix64 so the property inputs are stable across runs.
    let mut state = 0x5EED_u64;
    let mut rng = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (z ^ (z >> 31)) as u8
    };

    // Combinatorial trigger coverage: every one of the 8 channel triggers
    // (wb_temperature / wb_tint share the WB gains) is independently
    // present/absent => 2^7 = 128 combinations, including the all-off
    // identity and the all-on case ("allen Reglern gleichzeitig"). The
    // default recipe (= identity) is the all-off combination.
    let mut total = 0usize;
    for wb in [false, true] {
        for exposure in [false, true] {
            for contrast in [false, true] {
                for shadows_on in [false, true] {
                    for highlights_on in [false, true] {
                        for whites_on in [false, true] {
                            for blacks_on in [false, true] {
                                let wb_gains = if wb {
                                    Some([
                                        1.0 - rng() as f64 / 1500.0,
                                        1.0 - rng() as f64 / 2500.0,
                                        1.0 + rng() as f64 / 1500.0,
                                    ])
                                } else {
                                    None
                                };
                                let exposure_multiplier = if exposure {
                                    Some(2.0_f64.powf(rng() as f64 / 255.0 * 4.0 - 2.0))
                                } else {
                                    None
                                };
                                let contrast_factor = if contrast {
                                    Some(1.0 + (rng() as f64 / 255.0 * 2.0 - 1.0))
                                } else {
                                    None
                                };
                                let shadows = if shadows_on {
                                    Some(rng() as f64 / 255.0 * 2.0 - 1.0)
                                } else {
                                    None
                                };
                                let highlights = if highlights_on {
                                    Some(rng() as f64 / 255.0 * 2.0 - 1.0)
                                } else {
                                    None
                                };
                                let whites = if whites_on {
                                    Some(rng() as f64 / 255.0 * 2.0 - 1.0)
                                } else {
                                    None
                                };
                                let blacks = if blacks_on {
                                    Some(rng() as f64 / 255.0 * 2.0 - 1.0)
                                } else {
                                    None
                                };

                                let params = ChannelLutParams {
                                    wb_gains,
                                    exposure_multiplier,
                                    contrast_factor,
                                    shadows,
                                    highlights,
                                    whites,
                                    blacks,
                                };

                                let mut optimized = vec![0u8; 48 * 4];
                                let mut reference = vec![0u8; 48 * 4];
                                for pixel in optimized.as_chunks_mut::<4>().0 {
                                    pixel[0] = rng();
                                    pixel[1] = rng();
                                    pixel[2] = rng();
                                    pixel[3] = 255;
                                }
                                reference.copy_from_slice(&optimized);

                                apply_channel_lut_adjustments(&mut optimized, &params);
                                reference_channel_lut_adjustments(&mut reference, &params);

                                assert_eq!(
                                    optimized, reference,
                                    "byte mismatch: wb={wb} exp={exposure} con={contrast} \
                                     sh={shadows_on} hi={highlights_on} wh={whites_on} bl={blacks_on}"
                                );
                                total += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(total, 128);

    // Explicit default-recipe identity: with no triggers the kernel leaves
    // every byte untouched.
    let identity = ChannelLutParams {
        wb_gains: None,
        exposure_multiplier: None,
        contrast_factor: None,
        shadows: None,
        highlights: None,
        whites: None,
        blacks: None,
    };
    let mut frame = vec![10u8, 20, 30, 255, 200, 60, 30, 200];
    let original = frame.clone();
    apply_channel_lut_adjustments(&mut frame, &identity);
    assert_eq!(frame, original);
}
