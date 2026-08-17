use crate::{CoreError, ImageFrame};

/// Luminance is measured from RGBA8's encoded sRGB RGB channels, normalized to
/// 0..=1, with Rec.709 weights. Alpha is deliberately ignored in this raster
/// MVP, so transparent pixels are still samples of their RGB values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToneAnalysis {
    pub mean: f64,
    pub median: f64,
    pub p01: f64,
    pub p99: f64,
    pub sample_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoToneConfig {
    pub target_luminance: f64,
    pub epsilon: f64,
    pub exposure_bounds: (f64, f64),
    pub contrast_bounds: (f64, f64),
}

impl Default for AutoToneConfig {
    fn default() -> Self {
        Self {
            target_luminance: 0.5,
            epsilon: 1e-6,
            exposure_bounds: (-10.0, 10.0),
            contrast_bounds: (-1.0, 1.0),
        }
    }
}

impl AutoToneConfig {
    pub fn validate(&self) -> Result<(), CoreError> {
        if !self.target_luminance.is_finite() || !(0.0..=1.0).contains(&self.target_luminance) {
            return Err(CoreError::InvalidAutoToneConfig(
                "target_luminance must be finite and in 0..=1".into(),
            ));
        }
        if !self.epsilon.is_finite() || self.epsilon <= 0.0 || self.epsilon > 1.0 {
            return Err(CoreError::InvalidAutoToneConfig(
                "epsilon must be finite, greater than zero, and at most one".into(),
            ));
        }
        for (name, (minimum, maximum)) in [
            ("exposure_bounds", self.exposure_bounds),
            ("contrast_bounds", self.contrast_bounds),
        ] {
            if !minimum.is_finite() || !maximum.is_finite() || minimum > maximum {
                return Err(CoreError::InvalidAutoToneConfig(format!(
                    "{name} must contain finite values in ascending order"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoToneResult {
    pub analysis: ToneAnalysis,
    pub exposure: f64,
    pub contrast: f64,
}

fn values(frame: &ImageFrame) -> Vec<f64> {
    frame
        .pixels
        .chunks_exact(4)
        .map(|p| {
            (0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])) / 255.0
        })
        .collect()
}

pub fn analyze_tone(frame: &ImageFrame) -> ToneAnalysis {
    let mut v = values(frame);
    if v.is_empty() {
        return ToneAnalysis {
            mean: 0.0,
            median: 0.0,
            p01: 0.0,
            p99: 0.0,
            sample_count: 0,
        };
    }
    v.sort_by(f64::total_cmp);
    let percentile = |q: f64| {
        let position = q * (v.len() - 1) as f64;
        let low = position.floor() as usize;
        let high = position.ceil() as usize;
        v[low] + (v[high] - v[low]) * (position - low as f64)
    };
    ToneAnalysis {
        mean: v.iter().sum::<f64>() / v.len() as f64,
        median: percentile(0.5),
        p01: percentile(0.01),
        p99: percentile(0.99),
        sample_count: v.len(),
    }
}

pub fn suggest_auto_tone(
    frame: &ImageFrame,
    config: AutoToneConfig,
) -> Result<AutoToneResult, CoreError> {
    config.validate()?;
    let analysis = analyze_tone(frame);
    let epsilon = config.epsilon;
    let target = config.target_luminance.clamp(epsilon, 1.0);
    let exposure = if analysis.sample_count == 0 {
        0.0
    } else if analysis.median <= epsilon {
        config.exposure_bounds.1
    } else if analysis.median >= 1.0 - config.epsilon {
        config.exposure_bounds.0
    } else {
        (target / analysis.median).log2()
    }
    .clamp(config.exposure_bounds.0, config.exposure_bounds.1);
    let span = analysis.p99 - analysis.p01;
    let contrast = if span <= epsilon {
        0.0
    } else {
        (0.8 / span - 1.0).clamp(config.contrast_bounds.0, config.contrast_bounds.1)
    };
    Ok(AutoToneResult {
        analysis,
        exposure: finite(exposure),
        contrast: finite(contrast),
    })
}

pub fn match_total_exposure(frame: &ImageFrame, target_luminance: f64) -> Result<f64, CoreError> {
    if !target_luminance.is_finite() || !(0.0..=1.0).contains(&target_luminance) {
        return Err(CoreError::InvalidAdjustment {
            name: "target_luminance".into(),
            value: target_luminance,
            minimum: 0.0,
            maximum: 1.0,
        });
    }
    let current = analyze_tone(frame).mean;
    let epsilon = 1e-6;
    let value = if target_luminance <= epsilon {
        -10.0
    } else if current <= epsilon {
        10.0
    } else {
        (target_luminance / current.max(epsilon)).log2()
    };
    Ok(finite(value).clamp(-10.0, 10.0))
}

/// Stable fingerprint for deciding whether persisted analysis belongs to this frame.
pub fn tone_fingerprint(frame: &ImageFrame, config: AutoToneConfig) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    // Include the complete analysis configuration in a canonical byte stream.
    for value in [
        config.target_luminance,
        config.epsilon,
        config.exposure_bounds.0,
        config.exposure_bounds.1,
        config.contrast_bounds.0,
        config.contrast_bounds.1,
    ] {
        for byte in value.to_bits().to_le_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    for value in [frame.width, frame.height] {
        for byte in value.to_le_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    for byte in &frame.pixels {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    format!("tone-rgba8-rec709:{hash:016x}")
}

fn finite(value: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn percentile_and_mean_are_stable() {
        let f = ImageFrame::new(
            4,
            1,
            vec![
                0, 0, 0, 0, 64, 64, 64, 0, 128, 128, 128, 0, 255, 255, 255, 0,
            ],
        )
        .unwrap();
        let a = analyze_tone(&f);
        assert!((a.median - 96.0 / 255.0).abs() < 1e-9);
        assert!(a.p01 < a.p99);
    }
    #[test]
    fn dark_bright_and_targets_are_bounded() {
        for value in [0, 255] {
            let f = ImageFrame::new(1, 1, vec![value, value, value, 255]).unwrap();
            let r = suggest_auto_tone(&f, AutoToneConfig::default()).unwrap();
            assert!((-10.0..=10.0).contains(&r.exposure));
        }
        let f = ImageFrame::new(1, 1, vec![128, 128, 128, 255]).unwrap();
        assert_eq!(match_total_exposure(&f, 0.0).unwrap(), -10.0);
    }
    #[test]
    fn empty_and_near_black_are_safe() {
        let f = ImageFrame::new(0, 0, vec![]).unwrap();
        assert_eq!(
            suggest_auto_tone(&f, AutoToneConfig::default())
                .unwrap()
                .exposure,
            0.0
        );
        let f = ImageFrame::new(1, 1, vec![0, 0, 0, 255]).unwrap();
        assert_eq!(match_total_exposure(&f, 1.0).unwrap(), 10.0);
    }

    #[test]
    fn invalid_config_is_rejected() {
        for config in [
            AutoToneConfig {
                epsilon: f64::NAN,
                ..Default::default()
            },
            AutoToneConfig {
                epsilon: f64::INFINITY,
                ..Default::default()
            },
            AutoToneConfig {
                epsilon: 0.0,
                ..Default::default()
            },
            AutoToneConfig {
                target_luminance: -0.1,
                ..Default::default()
            },
            AutoToneConfig {
                target_luminance: 1.1,
                ..Default::default()
            },
            AutoToneConfig {
                exposure_bounds: (1.0, -1.0),
                ..Default::default()
            },
            AutoToneConfig {
                contrast_bounds: (f64::NEG_INFINITY, 1.0),
                ..Default::default()
            },
        ] {
            assert!(config.validate().is_err());
        }
    }

    #[test]
    fn fingerprint_includes_analysis_config() {
        let frame = ImageFrame::new(1, 1, vec![128, 128, 128, 255]).unwrap();
        let base = AutoToneConfig::default();
        assert_ne!(
            tone_fingerprint(&frame, base),
            tone_fingerprint(
                &frame,
                AutoToneConfig {
                    target_luminance: 0.6,
                    ..base
                }
            )
        );
        assert_ne!(
            tone_fingerprint(&frame, base),
            tone_fingerprint(
                &frame,
                AutoToneConfig {
                    epsilon: 1e-4,
                    ..base
                }
            )
        );
        assert_ne!(
            tone_fingerprint(&frame, base),
            tone_fingerprint(
                &frame,
                AutoToneConfig {
                    exposure_bounds: (-5.0, 5.0),
                    ..base
                }
            )
        );
    }

    #[test]
    fn fingerprint_includes_frame_geometry() {
        let pixels = vec![128, 128, 128, 255, 64, 64, 64, 255];
        let wide = ImageFrame::new(2, 1, pixels.clone()).unwrap();
        let tall = ImageFrame::new(1, 2, pixels).unwrap();
        assert_ne!(
            tone_fingerprint(&wide, AutoToneConfig::default()),
            tone_fingerprint(&tall, AutoToneConfig::default())
        );
    }

    #[test]
    fn percentiles_interpolate_and_alpha_is_ignored() {
        let frame = ImageFrame::new(2, 1, vec![0, 0, 0, 0, 255, 255, 255, 0]).unwrap();
        let analysis = analyze_tone(&frame);
        assert!((analysis.p01 - 0.01).abs() < 1e-9);
        assert!((analysis.p99 - 0.99).abs() < 1e-9);
    }

    #[test]
    fn matching_clips_and_handles_positive_and_negative_values() {
        let dark = ImageFrame::new(1, 1, vec![32, 32, 32, 255]).unwrap();
        let bright = ImageFrame::new(1, 1, vec![224, 224, 224, 255]).unwrap();
        assert!(match_total_exposure(&dark, 0.8).unwrap() > 0.0);
        assert!(match_total_exposure(&bright, 0.2).unwrap() < 0.0);
        assert_eq!(
            match_total_exposure(&dark, f64::NAN)
                .unwrap_err()
                .to_string(),
            "invalid target_luminance: must be finite and in 0..=1, got NaN"
        );
    }

    #[test]
    fn auto_tone_is_finite_and_bounded_for_deterministic_rgba8_frames() {
        let config = AutoToneConfig {
            target_luminance: 0.62,
            epsilon: 1e-5,
            exposure_bounds: (-3.5, 4.25),
            contrast_bounds: (-0.75, 0.8),
        };
        for frame_index in 0..12u8 {
            let width = 3 + u32::from(frame_index % 4);
            let height = 2 + u32::from(frame_index % 3);
            let pixels = (0..width * height)
                .flat_map(|pixel_index| {
                    let value = frame_index
                        .wrapping_mul(37)
                        .wrapping_add((pixel_index as u8).wrapping_mul(19));
                    [
                        value,
                        value.wrapping_mul(3).wrapping_add(11),
                        value.wrapping_mul(7).wrapping_add(23),
                        value.wrapping_mul(13),
                    ]
                })
                .collect();
            let frame = ImageFrame::new(width, height, pixels).unwrap();
            let result = suggest_auto_tone(&frame, config).unwrap();

            assert_eq!(result.analysis.sample_count, (width * height) as usize);
            for value in [
                result.analysis.mean,
                result.analysis.median,
                result.analysis.p01,
                result.analysis.p99,
            ] {
                assert!(value.is_finite());
                assert!((0.0..=1.0).contains(&value));
            }
            assert!(result.exposure.is_finite());
            assert!(
                (config.exposure_bounds.0..=config.exposure_bounds.1).contains(&result.exposure)
            );
            assert!(result.contrast.is_finite());
            assert!(
                (config.contrast_bounds.0..=config.contrast_bounds.1).contains(&result.contrast)
            );
        }
    }

    #[test]
    fn auto_tone_is_independent_of_alpha_for_multiple_rgb_values() {
        let rgb_values = [[0, 0, 0], [17, 129, 241], [64, 96, 128], [255, 128, 1]];
        let reference_pixels: Vec<u8> = rgb_values
            .iter()
            .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
            .collect();
        let reference = ImageFrame::new(4, 1, reference_pixels).unwrap();
        let expected = suggest_auto_tone(&reference, AutoToneConfig::default()).unwrap();

        for alpha_set in [[0, 1, 127, 255], [255, 128, 32, 0], [7, 99, 200, 250]] {
            let pixels: Vec<u8> = rgb_values
                .iter()
                .zip(alpha_set)
                .flat_map(|(rgb, alpha)| [rgb[0], rgb[1], rgb[2], alpha])
                .collect();
            let result = suggest_auto_tone(
                &ImageFrame::new(4, 1, pixels).unwrap(),
                AutoToneConfig::default(),
            )
            .unwrap();
            assert_eq!(result.analysis, expected.analysis);
            assert_eq!(result.exposure, expected.exposure);
            assert_eq!(result.contrast, expected.contrast);
        }
    }

    #[test]
    fn matching_exposure_is_bounded_and_monotonic_for_increasing_targets() {
        let frame = ImageFrame::new(
            5,
            1,
            vec![
                7, 31, 99, 0, 32, 64, 128, 17, 96, 128, 160, 127, 192, 224, 240, 200, 255, 255,
                255, 255,
            ],
        )
        .unwrap();
        let targets = [0.0, 0.01, 0.05, 0.15, 0.3, 0.5, 0.7, 0.9, 1.0];
        let values: Vec<f64> = targets
            .iter()
            .map(|target| match_total_exposure(&frame, *target).unwrap())
            .collect();

        for value in &values {
            assert!(value.is_finite());
            assert!((-10.0..=10.0).contains(value));
        }
        for pair in values.windows(2) {
            assert!(
                pair[0] <= pair[1],
                "matching exposure is not monotonic: {pair:?}"
            );
        }
    }
}
