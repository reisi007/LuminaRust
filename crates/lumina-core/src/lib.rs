//! Small, portable raster MVP shared by native CLI and future WASM clients.

use image::{
    codecs::jpeg::JpegEncoder, codecs::webp::WebPEncoder, ColorType, DynamicImage, ImageEncoder,
    ImageFormat, RgbaImage,
};
use lumina_sidecar::EditRecipe;
use std::io::Cursor;
use thiserror::Error;

pub mod cache;
pub mod masks;
pub mod pipeline;
pub mod tone;
#[cfg(not(target_arch = "wasm32"))]
pub use cache::disk::{DiskCacheError, DiskFolderCache};
pub use cache::{
    CacheEntry, CacheError, CacheStage, CacheStore, Cancellation, FolderCache, FolderCacheSettings,
    StaleTracker,
};
pub use masks::{MaskError, MaskGraph, MaskPlane};
pub use pipeline::{Pipeline, PipelineFormat, PipelineStage, RenderKey, SourceAction};
pub use tone::{
    analyze_tone, match_total_exposure, suggest_auto_tone, tone_fingerprint, AutoToneConfig,
    AutoToneResult, ToneAnalysis,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFileFormat {
    Png,
    Jpeg,
    WebP,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BitDepth {
    #[default]
    Eight,
    Sixteen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportOptions {
    pub format: ImageFileFormat,
    pub bit_depth: BitDepth,
    pub quality: u8,
    pub dither: bool,
    pub seed: u64,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: ImageFileFormat::Png,
            bit_depth: BitDepth::default(),
            quality: 90,
            dither: true,
            seed: 0,
        }
    }
}

impl ExportOptions {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.quality == 0 || self.quality > 100 {
            return Err(CoreError::InvalidAdjustment {
                name: "quality".into(),
                value: self.quality as f64,
                minimum: 1.0,
                maximum: 100.0,
            });
        }
        if self.bit_depth != BitDepth::Eight {
            return Err(CoreError::Encode(
                "16-bit export is not supported in the MVP".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageFrame {
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA8 bytes, four bytes per pixel.
    pub pixels: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("could not decode raster image: {0}")]
    Decode(String),
    #[error("invalid RGBA8 frame dimensions {width}x{height} for {length} bytes")]
    InvalidFrame {
        width: u32,
        height: u32,
        length: usize,
    },
    #[error("could not encode raster image: {0}")]
    Encode(String),
    #[error("unsupported adjustment `{key}` in raster MVP")]
    UnsupportedAdjustment { key: String },
    #[error("invalid {name}: must be finite and in {minimum}..={maximum}, got {value}")]
    InvalidAdjustment {
        name: String,
        value: f64,
        minimum: f64,
        maximum: f64,
    },
    #[error("invalid auto-tone configuration: {0}")]
    InvalidAutoToneConfig(String),
}

impl ImageFrame {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, CoreError> {
        let expected = width as usize * height as usize * 4;
        if pixels.len() != expected {
            return Err(CoreError::InvalidFrame {
                width,
                height,
                length: pixels.len(),
            });
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, CoreError> {
        let image =
            image::load_from_memory(bytes).map_err(|error| CoreError::Decode(error.to_string()))?;
        let rgba = image.to_rgba8();
        Self::new(rgba.width(), rgba.height(), rgba.into_raw())
    }

    pub fn encode(&self, format: ImageFileFormat) -> Result<Vec<u8>, CoreError> {
        // Keep the historical byte-identical API independent of new defaults.
        self.encode_with_options(ExportOptions {
            format,
            dither: false,
            ..ExportOptions::default()
        })
    }

    pub fn encode_with_options(&self, options: ExportOptions) -> Result<Vec<u8>, CoreError> {
        options.validate()?;
        let mut pixels = self.pixels.clone();
        if options.dither {
            dither_rgba8(&mut pixels, options.seed);
        }
        // image's portable WebP encoder is lossless-only. Quantizing before
        // VP8L encoding provides the documented quality-controlled lossy path
        // without adding a native dependency (quality 100 is lossless).
        if options.format == ImageFileFormat::WebP && options.quality < 100 {
            let step = ((101 - options.quality as u16) / 10).max(1) as u8;
            for (index, value) in pixels.iter_mut().enumerate() {
                if index % 4 != 3 {
                    *value = (*value / step) * step;
                }
            }
        }
        let mut output = Cursor::new(Vec::new());
        let rgba = RgbaImage::from_raw(self.width, self.height, pixels).ok_or(
            CoreError::InvalidFrame {
                width: self.width,
                height: self.height,
                length: self.pixels.len(),
            },
        )?;
        match options.format {
            ImageFileFormat::Png => {
                DynamicImage::ImageRgba8(rgba).write_to(&mut output, ImageFormat::Png)
            }
            ImageFileFormat::Jpeg => {
                let rgb: Vec<u8> = rgba
                    .chunks_exact(4)
                    .flat_map(|pixel| pixel[..3].iter().copied())
                    .collect();
                JpegEncoder::new_with_quality(&mut output, options.quality).write_image(
                    &rgb,
                    self.width,
                    self.height,
                    ColorType::Rgb8.into(),
                )
            }
            ImageFileFormat::WebP => WebPEncoder::new_lossless(&mut output).write_image(
                &rgba,
                self.width,
                self.height,
                ColorType::Rgba8.into(),
            ),
        }
        .map_err(|error| CoreError::Encode(error.to_string()))?;
        Ok(output.into_inner())
    }

    pub fn apply_recipe(&mut self, recipe: &EditRecipe) -> Result<(), CoreError> {
        for (key, value) in &recipe.adjustments {
            let (minimum, maximum) = match key.as_str() {
                "exposure" => (-10.0, 10.0),
                "contrast" | "highlights" | "shadows" | "whites" | "blacks" | "wb_tint" => {
                    (-1.0, 1.0)
                }
                "wb_temperature" => (1500.0, 12000.0),
                _ => return Err(CoreError::UnsupportedAdjustment { key: key.clone() }),
            };
            if !value.is_finite() || !(minimum..=maximum).contains(value) {
                return Err(CoreError::InvalidAdjustment {
                    name: key.clone(),
                    value: *value,
                    minimum,
                    maximum,
                });
            }
        }
        if recipe.adjustments.contains_key("wb_temperature")
            || recipe.adjustments.contains_key("wb_tint")
        {
            let temperature = recipe
                .adjustments
                .get("wb_temperature")
                .copied()
                .unwrap_or(6500.0);
            let tint = recipe.adjustments.get("wb_tint").copied().unwrap_or(0.0);
            let warmth = (temperature - 6500.0) / 5500.0;
            let gains = [1.0 - warmth * 0.35, 1.0 - tint * 0.20, 1.0 + warmth * 0.35];
            for pixel in self.pixels.chunks_exact_mut(4) {
                for (channel, gain) in pixel[..3].iter_mut().zip(gains) {
                    *channel = ((*channel as f64 * gain).round()).clamp(0.0, 255.0) as u8;
                }
            }
        }
        if let Some(exposure) = recipe.adjustments.get("exposure") {
            let multiplier = 2.0_f64.powf(*exposure);
            for channel in self
                .pixels
                .chunks_exact_mut(4)
                .flat_map(|pixel| &mut pixel[..3])
            {
                *channel = ((*channel as f64 * multiplier).round()).clamp(0.0, 255.0) as u8;
            }
        }
        if let Some(contrast) = recipe.adjustments.get("contrast") {
            // `contrast` is a linear S-curve strength: -1 is flat gray, 0 is
            // unchanged, and 1 doubles distance from the midpoint (128).
            let factor = 1.0 + *contrast;
            for channel in self
                .pixels
                .chunks_exact_mut(4)
                .flat_map(|pixel| &mut pixel[..3])
            {
                *channel =
                    (((*channel as f64 - 128.0) * factor + 128.0).round()).clamp(0.0, 255.0) as u8;
            }
        }
        if let Some(shadows) = recipe.adjustments.get("shadows") {
            for channel in self
                .pixels
                .chunks_exact_mut(4)
                .flat_map(|pixel| &mut pixel[..3])
            {
                let x = *channel as f64 / 255.0;
                let weight = ((0.5 - x) / 0.5).max(0.0).powi(2);
                *channel = ((x + shadows * weight * 0.25).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
        if let Some(highlights) = recipe.adjustments.get("highlights") {
            for channel in self
                .pixels
                .chunks_exact_mut(4)
                .flat_map(|pixel| &mut pixel[..3])
            {
                let x = *channel as f64 / 255.0;
                let weight = ((x - 0.5) / 0.5).max(0.0).powi(2);
                *channel = ((x + highlights * weight * 0.25).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
        if let Some(whites) = recipe.adjustments.get("whites") {
            for channel in self
                .pixels
                .chunks_exact_mut(4)
                .flat_map(|pixel| &mut pixel[..3])
            {
                let x = *channel as f64 / 255.0;
                let weight = ((x - 0.5) / 0.5).max(0.0);
                *channel = ((x + whites * weight * 0.25).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
        if let Some(blacks) = recipe.adjustments.get("blacks") {
            for channel in self
                .pixels
                .chunks_exact_mut(4)
                .flat_map(|pixel| &mut pixel[..3])
            {
                let x = *channel as f64 / 255.0;
                let weight = ((0.5 - x) / 0.5).max(0.0);
                *channel = ((x + blacks * weight * 0.25).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
        Ok(())
    }
}

fn dither_rgba8(pixels: &mut [u8], seed: u64) {
    let mut state = seed ^ 0x9e3779b97f4a7c15;
    for (index, value) in pixels.iter_mut().enumerate() {
        if index % 4 == 3 {
            continue;
        }
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        // The frame is already quantized to u8; a one-code stochastic offset
        // is the useful deterministic approximation for this round-trip.
        let delta = if state & 1 == 0 { -1.0 } else { 1.0 };
        *value = (*value as f64 + delta).round().clamp(0.0, 255.0) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn recipe(values: &[(&str, f64)]) -> EditRecipe {
        EditRecipe {
            adjustments: BTreeMap::from_iter(
                values.iter().map(|(key, value)| ((*key).into(), *value)),
            ),
            ..EditRecipe::default()
        }
    }

    #[test]
    fn all_supported_formats_roundtrip() {
        let frame = ImageFrame::new(2, 1, vec![0, 10, 20, 255, 250, 240, 230, 128]).unwrap();
        for format in [
            ImageFileFormat::Png,
            ImageFileFormat::Jpeg,
            ImageFileFormat::WebP,
        ] {
            let decoded = ImageFrame::decode(&frame.encode(format).unwrap()).unwrap();
            assert_eq!((decoded.width, decoded.height), (2, 1));
            assert_eq!(decoded.pixels.len(), 8);
        }
    }

    #[test]
    fn png_options_are_lossless_and_dither_is_deterministic() {
        let frame = ImageFrame::new(2, 1, vec![1, 20, 240, 255, 100, 101, 102, 17]).unwrap();
        let options = ExportOptions {
            format: ImageFileFormat::Png,
            dither: false,
            ..ExportOptions::default()
        };
        let bytes = frame.encode_with_options(options).unwrap();
        assert_eq!(ImageFrame::decode(&bytes).unwrap(), frame);
        let dithered = ExportOptions {
            dither: true,
            seed: 42,
            ..options
        };
        assert_eq!(
            frame.encode_with_options(dithered).unwrap(),
            frame.encode_with_options(dithered).unwrap()
        );
    }

    #[test]
    fn white_balance_and_edge_tones_have_identity_and_effect() {
        let original = ImageFrame::new(1, 1, vec![80, 100, 120, 9]).unwrap();
        let mut identity = original.clone();
        identity.apply_recipe(&recipe(&[])).unwrap();
        assert_eq!(identity, original);
        let mut warm = original.clone();
        warm.apply_recipe(&recipe(&[("wb_temperature", 3000.0)]))
            .unwrap();
        assert!(warm.pixels[0] > original.pixels[0]);
        assert!(warm.pixels[2] < original.pixels[2]);
        let mut edges = ImageFrame::new(2, 1, vec![20, 20, 20, 255, 230, 230, 230, 255]).unwrap();
        edges
            .apply_recipe(&recipe(&[("whites", 1.0), ("blacks", -1.0)]))
            .unwrap();
        assert!(edges.pixels[0] < 20 && edges.pixels[4] > 230);
    }

    #[test]
    fn export_options_reject_mvp_unsupported_values() {
        let frame = ImageFrame::new(1, 1, vec![1, 2, 3, 255]).unwrap();
        assert!(frame
            .encode_with_options(ExportOptions {
                quality: 0,
                ..Default::default()
            })
            .is_err());
        assert!(frame
            .encode_with_options(ExportOptions {
                bit_depth: BitDepth::Sixteen,
                ..Default::default()
            })
            .is_err());
    }

    #[test]
    fn exposure_clamps_channels_and_preserves_alpha() {
        let mut frame = ImageFrame::new(1, 1, vec![100, 200, 255, 17]).unwrap();
        frame.apply_recipe(&recipe(&[("exposure", 1.0)])).unwrap();
        assert_eq!(frame.pixels, vec![200, 255, 255, 17]);
    }

    #[test]
    fn contrast_is_linear_around_midpoint() {
        let mut frame = ImageFrame::new(1, 1, vec![64, 128, 192, 9]).unwrap();
        frame.apply_recipe(&recipe(&[("contrast", 1.0)])).unwrap();
        assert_eq!(frame.pixels, vec![0, 128, 255, 9]);
    }

    #[test]
    fn rejects_invalid_and_unknown_adjustments() {
        let mut frame = ImageFrame::new(1, 1, vec![1, 2, 3, 255]).unwrap();
        assert!(matches!(
            frame.apply_recipe(&recipe(&[("contrast", 2.0)])),
            Err(CoreError::InvalidAdjustment { .. })
        ));
        assert!(matches!(
            frame.apply_recipe(&recipe(&[("clarity", 0.5)])),
            Err(CoreError::UnsupportedAdjustment { key }) if key == "clarity"
        ));
        assert!(frame
            .apply_recipe(&recipe(&[("exposure", f64::NAN)]))
            .is_err());
    }

    #[test]
    fn rejects_non_finite_and_out_of_range_values_for_each_adjustment() {
        for (name, values) in [
            (
                "exposure",
                [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -10.1, 10.1],
            ),
            (
                "contrast",
                [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.1, 1.1],
            ),
            (
                "highlights",
                [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.1, 1.1],
            ),
            (
                "shadows",
                [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.1, 1.1],
            ),
        ] {
            for value in values {
                let mut frame = ImageFrame::new(1, 1, vec![1, 2, 3, 255]).unwrap();
                assert!(
                    matches!(
                        frame.apply_recipe(&recipe(&[(name, value)])),
                        Err(CoreError::InvalidAdjustment { .. })
                    ),
                    "{name}={value:?} should be rejected"
                );
            }
        }
    }

    #[test]
    fn accepts_both_adjustment_boundaries() {
        for (name, boundaries) in [
            ("exposure", [-10.0, 10.0]),
            ("contrast", [-1.0, 1.0]),
            ("highlights", [-1.0, 1.0]),
            ("shadows", [-1.0, 1.0]),
        ] {
            for value in boundaries {
                let mut frame = ImageFrame::new(1, 1, vec![64, 128, 192, 255]).unwrap();
                assert!(frame.apply_recipe(&recipe(&[(name, value)])).is_ok());
            }
        }
    }

    #[test]
    fn highlights_and_shadows_adjust_expected_tones_and_preserve_alpha() {
        let mut shadows = ImageFrame::new(1, 1, vec![32, 96, 160, 17]).unwrap();
        shadows.apply_recipe(&recipe(&[("shadows", 1.0)])).unwrap();
        assert_eq!(shadows.pixels, vec![68, 100, 160, 17]);

        let mut highlights = ImageFrame::new(1, 1, vec![96, 160, 224, 23]).unwrap();
        highlights
            .apply_recipe(&recipe(&[("highlights", -1.0)]))
            .unwrap();
        assert_eq!(highlights.pixels, vec![96, 156, 187, 23]);
    }

    #[test]
    fn highlights_and_shadows_leave_midpoint_unchanged_and_clamp_extremes() {
        let mut frame = ImageFrame::new(2, 1, vec![0, 128, 255, 31, 250, 1, 128, 47]).unwrap();
        frame
            .apply_recipe(&recipe(&[("shadows", -1.0), ("highlights", 1.0)]))
            .unwrap();
        assert_eq!(frame.pixels, vec![0, 128, 255, 31, 255, 0, 128, 47]);
    }

    #[test]
    fn shadows_are_applied_before_highlights() {
        let mut frame = ImageFrame::new(1, 1, vec![64, 128, 192, 255]).unwrap();
        frame
            .apply_recipe(&recipe(&[("shadows", 1.0), ("highlights", -1.0)]))
            .unwrap();
        let mut expected = ImageFrame::new(1, 1, vec![64, 128, 192, 255]).unwrap();
        expected.apply_recipe(&recipe(&[("shadows", 1.0)])).unwrap();
        expected
            .apply_recipe(&recipe(&[("highlights", -1.0)]))
            .unwrap();
        assert_eq!(frame, expected);
    }
}
