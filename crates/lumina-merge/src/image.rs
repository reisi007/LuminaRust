//! Decoded linear frame: row-major interleaved RGB `f32` in linear light.
//!
//! Values are normally in `[0, 1]` for LDR inputs; HDR merge outputs may
//! exceed 1 (linear scene data, never clipped above). NaN/infinite inputs
//! are rejected loudly at construction.

use crate::MergeError;

/// One decoded linear RGB frame (`f32` per channel, interleaved).
#[derive(Debug, Clone, PartialEq)]
pub struct LinearImage {
    width: u32,
    height: u32,
    pixels: Vec<f32>,
}

impl LinearImage {
    /// Construct; rejects dimension mismatch and non-finite pixels loudly
    /// (never silently clipped or defaulted).
    pub fn new(width: u32, height: u32, pixels: Vec<f32>) -> Result<Self, MergeError> {
        if width == 0 || height == 0 {
            return Err(MergeError::Invalid(format!(
                "merge image dimensions must be non-zero, got {width}x{height}"
            )));
        }
        let expected = width as usize * height as usize * 3;
        if pixels.len() != expected {
            return Err(MergeError::Invalid(format!(
                "merge image {width}x{height} needs {expected} floats, got {}",
                pixels.len()
            )));
        }
        if pixels.iter().any(|v| !v.is_finite()) {
            return Err(MergeError::Invalid(
                "merge image pixels must be finite".into(),
            ));
        }
        if pixels.iter().any(|v| *v < 0.0) {
            return Err(MergeError::Invalid(
                "merge image pixels must be non-negative (linear light)".into(),
            ));
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[must_use]
    pub fn pixels(&self) -> &[f32] {
        &self.pixels
    }

    /// Luminance plane (Rec.709) for alignment metrics.
    #[must_use]
    pub fn luminance(&self) -> Vec<f32> {
        self.pixels
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2])
            .collect()
    }

    /// One channel plane (0 = R, 1 = G, 2 = B).
    #[must_use]
    pub fn channel_plane(&self, channel: usize) -> Vec<f32> {
        self.pixels
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| p[channel.min(2)])
            .collect()
    }

    /// Solid-colour fixture constructor (tests/fixtures only).
    #[must_use]
    pub fn solid(width: u32, height: u32, rgb: [f32; 3]) -> Self {
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 3);
        for _ in 0..width as usize * height as usize {
            pixels.extend_from_slice(&rgb);
        }
        Self {
            width,
            height,
            pixels,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_mismatch_nonfinite_negative() {
        assert!(LinearImage::new(2, 2, vec![0.0; 11]).is_err());
        assert!(LinearImage::new(0, 2, vec![]).is_err());
        assert!(LinearImage::new(1, 1, vec![f32::NAN, 0.0, 0.0]).is_err());
        assert!(LinearImage::new(1, 1, vec![-0.1, 0.0, 0.0]).is_err());
        assert!(LinearImage::new(2, 1, vec![0.5; 6]).is_ok());
    }
}
