//! Small, portable raster MVP shared by native CLI and future WASM clients.

use image::{DynamicImage, ImageFormat, RgbaImage};
use lumina_sidecar::EditRecipe;
use std::io::Cursor;
use thiserror::Error;

pub mod masks;
pub use masks::{MaskError, MaskGraph, MaskPlane};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFileFormat {
    Png,
    Jpeg,
    WebP,
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
        let rgba = RgbaImage::from_raw(self.width, self.height, self.pixels.clone()).ok_or(
            CoreError::InvalidFrame {
                width: self.width,
                height: self.height,
                length: self.pixels.len(),
            },
        )?;
        let mut output = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(rgba)
            .write_to(&mut output, format.image_format())
            .map_err(|error| CoreError::Encode(error.to_string()))?;
        Ok(output.into_inner())
    }

    pub fn apply_recipe(&mut self, recipe: &EditRecipe) -> Result<(), CoreError> {
        for (key, value) in &recipe.adjustments {
            let (minimum, maximum) = match key.as_str() {
                "exposure" => (-10.0, 10.0),
                "contrast" => (-1.0, 1.0),
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
        Ok(())
    }
}

impl ImageFileFormat {
    fn image_format(self) -> ImageFormat {
        match self {
            Self::Png => ImageFormat::Png,
            Self::Jpeg => ImageFormat::Jpeg,
            Self::WebP => ImageFormat::WebP,
        }
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
        for (name, boundaries) in [("exposure", [-10.0, 10.0]), ("contrast", [-1.0, 1.0])] {
            for value in boundaries {
                let mut frame = ImageFrame::new(1, 1, vec![64, 128, 192, 255]).unwrap();
                assert!(frame.apply_recipe(&recipe(&[(name, value)])).is_ok());
            }
        }
    }
}
