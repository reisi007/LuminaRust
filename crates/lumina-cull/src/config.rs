//! Documented Stage-1 analysis thresholds, weights and the score scale.
//!
//! The defaults are the normative scale of this slice: changing them changes
//! the analyzer identity (the version in [`crate::identity`] must then be
//! bumped), because a persisted proposal is only comparable at one scale.
//! Thresholds are normalized to `0..=1` wherever the raw measure lives on a
//! bounded scale, so the analysis stays resolution-independent (analysis runs
//! at a fixed [`CullConfig::analysis_max_width`]).

use crate::CullError;

/// Stage-1 culling configuration. All fields are validated by
/// [`CullConfig::validate`]; production code uses [`CullConfig::default`].
#[derive(Debug, Clone, PartialEq)]
pub struct CullConfig {
    /// Downscale bound (px) for the analysis frame. The decoded source may be
    /// larger; the analysis resolution is part of the persisted identity.
    pub analysis_max_width: u32,

    /// Map slope of the monotone sharpness score `1 - exp(-m / scale)`.
    pub sharpness_scale: f64,
    /// `sharpness_score` below this value emits `sharpness_low`.
    pub sharpness_low_threshold: f64,
    /// `motion_blur_suspect` requires `anisotropy >= this` (and low sharpness).
    pub motion_blur_anisotropy_threshold: f64,
    /// `motion_blur_suspect` requires sharpness below this score.
    pub motion_blur_sharpness_threshold: f64,

    /// Map slope of the monotone noise score `1 - exp(-sigma / scale)`.
    pub noise_scale: f64,
    /// `noise_high_iso` threshold when no ISO is known.
    pub noise_score_threshold: f64,
    /// `noise_high_iso` threshold when `iso >= 1600` (digital-noise plausible).
    pub noise_score_threshold_high_iso: f64,
    /// `noise_high_iso` threshold when `iso < 400` (noise less plausible).
    pub noise_score_threshold_low_iso: f64,

    /// Clipping fraction (shadow + highlight) above this emits `exposure_clipped`.
    pub clip_fraction_threshold: f64,
    /// Mean luminance below this emits `exposure_underexposed`.
    pub underexposed_mean_threshold: f64,
    /// Mean luminance above this emits `exposure_overexposed`.
    pub overexposed_mean_threshold: f64,

    /// dHash (64-bit) Hamming distance at or below which two images are
    /// near-duplicates.
    pub duplicate_hash_distance: u32,
    /// Normalized 32-bin histogram L1 distance at or below which two images are
    /// near-duplicates.
    pub duplicate_histogram_distance: f64,
    /// dHash Hamming distance at or below which two images resemble a series.
    pub series_hash_distance: u32,
    /// Histogram L1 distance at or below which two images resemble a series.
    pub series_histogram_distance: f64,
    /// Score subtraction for a redundant (non-best) member of a similar group.
    pub duplicate_score_penalty: f64,

    /// Weight of the sharpness component in the base score.
    pub weight_sharpness: f64,
    /// Weight of the exposure component in the base score.
    pub weight_exposure: f64,
    /// Weight of the (inverted) noise component in the base score.
    pub weight_noise: f64,

    /// `score >= keep_threshold` maps to `keep` (absent a lower reject match).
    pub keep_threshold: f64,
    /// `score <= reject_threshold` maps to `reject-kandidat`.
    pub reject_threshold: f64,
}

impl Default for CullConfig {
    fn default() -> Self {
        Self {
            analysis_max_width: 2048,
            sharpness_scale: 0.005,
            sharpness_low_threshold: 0.5,
            motion_blur_anisotropy_threshold: 0.5,
            motion_blur_sharpness_threshold: 0.6,
            noise_scale: 0.05,
            noise_score_threshold: 0.5,
            noise_score_threshold_high_iso: 0.35,
            noise_score_threshold_low_iso: 0.7,
            clip_fraction_threshold: 0.005,
            underexposed_mean_threshold: 0.12,
            overexposed_mean_threshold: 0.88,
            duplicate_hash_distance: 6,
            duplicate_histogram_distance: 0.10,
            series_hash_distance: 12,
            series_histogram_distance: 0.25,
            duplicate_score_penalty: 0.15,
            weight_sharpness: 0.6,
            weight_exposure: 0.25,
            weight_noise: 0.15,
            keep_threshold: 0.7,
            reject_threshold: 0.4,
        }
    }
}

impl CullConfig {
    /// ISO (inclusive) at or above which the "high ISO" noise threshold applies.
    pub const HIGH_ISO: u32 = 1600;
    /// ISO (exclusive) below which the "low ISO" noise threshold applies.
    pub const LOW_ISO: u32 = 400;

    /// Loud validation of the documented domain. A badly configured analyzer is
    /// a visible error, never a silently clamped run.
    pub fn validate(&self) -> Result<(), CullError> {
        let unit = |name: &str, value: f64| -> Result<(), CullError> {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(CullError::InvalidConfig(format!(
                    "{name} must be finite within 0..=1, got {value}"
                )));
            }
            Ok(())
        };
        let positive = |name: &str, value: f64| -> Result<(), CullError> {
            if !value.is_finite() || value <= 0.0 {
                return Err(CullError::InvalidConfig(format!(
                    "{name} must be finite and > 0, got {value}"
                )));
            }
            Ok(())
        };

        if self.analysis_max_width == 0 {
            return Err(CullError::InvalidConfig(
                "analysis_max_width must be non-zero".into(),
            ));
        }
        positive("sharpness_scale", self.sharpness_scale)?;
        positive("noise_scale", self.noise_scale)?;
        positive(
            "duplicate_histogram_distance",
            self.duplicate_histogram_distance,
        )?;
        positive("series_histogram_distance", self.series_histogram_distance)?;

        unit("sharpness_low_threshold", self.sharpness_low_threshold)?;
        unit(
            "motion_blur_sharpness_threshold",
            self.motion_blur_sharpness_threshold,
        )?;
        unit(
            "motion_blur_anisotropy_threshold",
            self.motion_blur_anisotropy_threshold,
        )?;
        unit("noise_score_threshold", self.noise_score_threshold)?;
        unit(
            "noise_score_threshold_high_iso",
            self.noise_score_threshold_high_iso,
        )?;
        unit(
            "noise_score_threshold_low_iso",
            self.noise_score_threshold_low_iso,
        )?;
        unit("clip_fraction_threshold", self.clip_fraction_threshold)?;
        unit(
            "underexposed_mean_threshold",
            self.underexposed_mean_threshold,
        )?;
        unit(
            "overexposed_mean_threshold",
            self.overexposed_mean_threshold,
        )?;
        unit(
            "duplicate_histogram_distance",
            self.duplicate_histogram_distance,
        )?;
        unit("series_histogram_distance", self.series_histogram_distance)?;
        unit("duplicate_score_penalty", self.duplicate_score_penalty)?;
        unit("keep_threshold", self.keep_threshold)?;
        unit("reject_threshold", self.reject_threshold)?;

        if !(1..=64).contains(&self.duplicate_hash_distance) {
            return Err(CullError::InvalidConfig(
                "duplicate_hash_distance must be within 1..=64".into(),
            ));
        }
        if !(1..=64).contains(&self.series_hash_distance) {
            return Err(CullError::InvalidConfig(
                "series_hash_distance must be within 1..=64".into(),
            ));
        }
        if self.series_hash_distance < self.duplicate_hash_distance {
            return Err(CullError::InvalidConfig(
                "series_hash_distance must be >= duplicate_hash_distance".into(),
            ));
        }
        if self.series_histogram_distance < self.duplicate_histogram_distance {
            return Err(CullError::InvalidConfig(
                "series_histogram_distance must be >= duplicate_histogram_distance".into(),
            ));
        }
        if self.underexposed_mean_threshold >= self.overexposed_mean_threshold {
            return Err(CullError::InvalidConfig(
                "underexposed_mean_threshold must be < overexposed_mean_threshold".into(),
            ));
        }
        if self.reject_threshold >= self.keep_threshold {
            return Err(CullError::InvalidConfig(
                "reject_threshold must be < keep_threshold".into(),
            ));
        }
        if self.weight_sharpness < 0.0
            || self.weight_exposure < 0.0
            || self.weight_noise < 0.0
            || !self.weight_sharpness.is_finite()
            || !self.weight_exposure.is_finite()
            || !self.weight_noise.is_finite()
        {
            return Err(CullError::InvalidConfig(
                "component weights must be finite and >= 0".into(),
            ));
        }
        let weight_sum = self.weight_sharpness + self.weight_exposure + self.weight_noise;
        if !weight_sum.is_finite() || weight_sum <= 0.0 {
            return Err(CullError::InvalidConfig(
                "component weights must sum to > 0".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        CullConfig::default().validate().expect("default is valid");
    }

    #[test]
    fn invalid_configs_are_rejected_loudly() {
        let cases: [fn(&mut CullConfig); 8] = [
            |c| c.analysis_max_width = 0,
            |c| c.sharpness_scale = 0.0,
            |c| c.noise_scale = f64::NAN,
            |c| {
                c.keep_threshold = 0.2;
                c.reject_threshold = 0.4;
            },
            |c| {
                c.weight_sharpness = 0.0;
                c.weight_exposure = 0.0;
                c.weight_noise = 0.0;
            },
            |c| {
                c.underexposed_mean_threshold = 0.95;
                c.overexposed_mean_threshold = 0.9;
            },
            |c| c.duplicate_hash_distance = 0,
            |c| {
                c.duplicate_hash_distance = 8;
                c.series_hash_distance = 4;
            },
        ];
        for mutate in cases {
            let mut config = CullConfig::default();
            mutate(&mut config);
            assert!(
                config.validate().is_err(),
                "invalid config must be rejected"
            );
        }
    }
}
