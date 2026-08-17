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
                "contrast" | "highlights" | "shadows" | "whites" | "blacks" | "wb_tint"
                | "vibrance" | "saturation" => (-1.0, 1.0),
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
        validate_nested_adjustments(recipe)?;
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
                *channel = ((x - blacks * weight * 0.25).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
        if let Some(curves) = &recipe.curves {
            for pixel in self.pixels.chunks_exact_mut(4) {
                let original = [
                    pixel[0] as f64 / 255.0,
                    pixel[1] as f64 / 255.0,
                    pixel[2] as f64 / 255.0,
                ];
                let luminance = 0.2126 * original[0] + 0.7152 * original[1] + 0.0722 * original[2];
                let master = monotone_curve(&curves.master, luminance as f32) as f64;
                let channels = [
                    &curves.channels.red,
                    &curves.channels.green,
                    &curves.channels.blue,
                ];
                for i in 0..3 {
                    let value = channels[i].as_ref().map_or(original[i], |c| {
                        monotone_curve(c, original[i] as f32) as f64
                    });
                    let value = if luminance > 1e-9 {
                        value * master / luminance
                    } else {
                        master
                    };
                    pixel[i] = (value.clamp(0.0, 1.0) * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        if let Some(hsl) = &recipe.hsl {
            apply_hsl(&mut self.pixels, hsl)?;
        }
        // F-092 deliberately follows HSL: vibrance is the selective operation,
        // then global saturation scales the resulting HSL saturation.
        apply_presence(
            &mut self.pixels,
            recipe.adjustments.get("vibrance"),
            recipe.adjustments.get("saturation"),
        );
        if let Some(color_grading) = &recipe.color_grading {
            apply_color_grading(&mut self.pixels, color_grading);
        }
        Ok(())
    }
}

/// Validate the structured adjustments here rather than relying on sidecar
/// deserialization/validation.  Recipes can be constructed directly by API
/// consumers, so this must run before any renderer indexes into a curve or
/// applies an HSL value.
fn validate_nested_adjustments(recipe: &EditRecipe) -> Result<(), CoreError> {
    if let Some(curves) = &recipe.curves {
        if curves.version != 1 {
            return Err(CoreError::InvalidAdjustment {
                name: "curves.version".into(),
                value: curves.version as f64,
                minimum: 1.0,
                maximum: 1.0,
            });
        }
        validate_curve("curves.master", &curves.master)?;
        for (name, curve) in [
            ("curves.channels.red", &curves.channels.red),
            ("curves.channels.green", &curves.channels.green),
            ("curves.channels.blue", &curves.channels.blue),
        ] {
            if let Some(curve) = curve {
                validate_curve(name, curve)?;
            }
        }
    }

    if let Some(hsl) = &recipe.hsl {
        if hsl.version != 1 {
            return Err(CoreError::InvalidAdjustment {
                name: "hsl.version".into(),
                value: hsl.version as f64,
                minimum: 1.0,
                maximum: 1.0,
            });
        }
        for (name, channel) in [
            ("hsl.red", &hsl.red),
            ("hsl.orange", &hsl.orange),
            ("hsl.yellow", &hsl.yellow),
            ("hsl.green", &hsl.green),
            ("hsl.cyan", &hsl.cyan),
            ("hsl.blue", &hsl.blue),
            ("hsl.violet", &hsl.violet),
            ("hsl.magenta", &hsl.magenta),
        ] {
            if let Some(channel) = channel {
                for (field, value) in [
                    ("hue", channel.hue),
                    ("saturation", channel.saturation),
                    ("luminance", channel.luminance),
                ] {
                    if !value.is_finite() || !(-1.0..=1.0).contains(&value) {
                        return Err(CoreError::InvalidAdjustment {
                            name: format!("{name}.{field}"),
                            value: value as f64,
                            minimum: -1.0,
                            maximum: 1.0,
                        });
                    }
                }
            }
        }
    }

    if let Some(c) = &recipe.color_grading {
        if c.version != 1 {
            return Err(CoreError::InvalidAdjustment {
                name: "color_grading.version".into(),
                value: c.version as f64,
                minimum: 1.0,
                maximum: 1.0,
            });
        }
        if !c.balance.is_finite() || !(-1.0..=1.0).contains(&c.balance) {
            return Err(CoreError::InvalidAdjustment {
                name: "color_grading.balance".into(),
                value: c.balance as f64,
                minimum: -1.0,
                maximum: 1.0,
            });
        }
        for (name, range) in [
            ("shadows", c.shadows),
            ("midtones", c.midtones),
            ("highlights", c.highlights),
        ] {
            if !range.hue_degrees.is_finite() || !(0.0..=360.0).contains(&range.hue_degrees) {
                return Err(CoreError::InvalidAdjustment {
                    name: format!("color_grading.{name}.hue_degrees"),
                    value: range.hue_degrees as f64,
                    minimum: 0.0,
                    maximum: 360.0,
                });
            }
            if !range.saturation.is_finite() || !(0.0..=1.0).contains(&range.saturation) {
                return Err(CoreError::InvalidAdjustment {
                    name: format!("color_grading.{name}.saturation"),
                    value: range.saturation as f64,
                    minimum: 0.0,
                    maximum: 1.0,
                });
            }
        }
    }
    Ok(())
}

fn validate_curve(name: &str, curve: &[lumina_sidecar::CurvePoint]) -> Result<(), CoreError> {
    if !(2..=32).contains(&curve.len()) {
        return Err(CoreError::InvalidAdjustment {
            name: format!("{name}.points"),
            value: curve.len() as f64,
            minimum: 2.0,
            maximum: 32.0,
        });
    }

    for (index, point) in curve.iter().enumerate() {
        for (field, value) in [("input", point.input), ("output", point.output)] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(CoreError::InvalidAdjustment {
                    name: format!("{name}.points[{index}].{field}"),
                    value: value as f64,
                    minimum: 0.0,
                    maximum: 1.0,
                });
            }
        }
        if index > 0 && point.input <= curve[index - 1].input {
            return Err(CoreError::InvalidAdjustment {
                name: format!("{name}.points[{index}].input"),
                value: point.input as f64,
                minimum: curve[index - 1].input as f64,
                maximum: 1.0,
            });
        }
    }

    let first = curve[0];
    let last = curve[curve.len() - 1];
    if first.input != 0.0 || first.output != 0.0 {
        return Err(CoreError::InvalidAdjustment {
            name: format!("{name}.points[0]"),
            value: first.output as f64,
            minimum: 0.0,
            maximum: 0.0,
        });
    }
    if last.input != 1.0 || last.output != 1.0 {
        return Err(CoreError::InvalidAdjustment {
            name: format!("{name}.points[{}]", curve.len() - 1),
            value: last.output as f64,
            minimum: 1.0,
            maximum: 1.0,
        });
    }
    Ok(())
}

fn monotone_curve(curve: &[lumina_sidecar::CurvePoint], x: f32) -> f32 {
    let p = curve;
    let x = x.clamp(0.0, 1.0);
    let i = p
        .windows(2)
        .position(|w| x <= w[1].input)
        .unwrap_or(p.len() - 2);
    let (a, b) = (&p[i], &p[i + 1]);
    let h = b.input - a.input;
    let t = ((x - a.input) / h).clamp(0.0, 1.0);
    let slope = |j: usize| {
        if j == 0 {
            (p[1].output - p[0].output) / (p[1].input - p[0].input)
        } else if j + 1 == p.len() {
            (p[j].output - p[j - 1].output) / (p[j].input - p[j - 1].input)
        } else {
            (p[j + 1].output - p[j - 1].output) / (p[j + 1].input - p[j - 1].input)
        }
    };
    let m0 = slope(i);
    let m1 = slope(i + 1);
    let d = (b.output - a.output) / h;
    let (m0, m1) = if d == 0.0 {
        (0.0, 0.0)
    } else {
        (m0.clamp(0.0, 3.0 * d), m1.clamp(0.0, 3.0 * d))
    };
    let t2 = t * t;
    let t3 = t2 * t;
    ((2.0 * t3 - 3.0 * t2 + 1.0) * a.output
        + (t3 - 2.0 * t2 + t) * h * m0
        + (-2.0 * t3 + 3.0 * t2) * b.output
        + (t3 - t2) * h * m1)
        .clamp(0.0, 1.0)
}

fn apply_hsl(pixels: &mut [u8], h: &lumina_sidecar::HslAdjustments) -> Result<(), CoreError> {
    let channels = [
        h.red, h.orange, h.yellow, h.green, h.cyan, h.blue, h.violet, h.magenta,
    ];
    // These are deliberately not an evenly spaced `i * 30` sequence: green
    // through blue use the conventional Lightroom-like 60 degree sectors,
    // while violet and magenta remain distinct adjacent controls.
    const CENTERS: [f32; 8] = [0.0, 30.0, 60.0, 120.0, 180.0, 240.0, 270.0, 300.0];
    for px in pixels.chunks_exact_mut(4) {
        let (mut hue, mut sat, mut l) = rgb_to_hsl(
            px[0] as f32 / 255.0,
            px[1] as f32 / 255.0,
            px[2] as f32 / 255.0,
        );
        let mut dh = 0.0;
        let mut ds = 0.0;
        let mut dl = 0.0;
        let weights: [f32; 8] = CENTERS.map(|center| {
            let i = CENTERS.iter().position(|&c| c == center).unwrap();
            let previous = if i == 0 {
                360.0 - CENTERS[7]
            } else {
                center - CENTERS[i - 1]
            };
            let next = if i + 1 == CENTERS.len() {
                360.0 - center + CENTERS[0]
            } else {
                CENTERS[i + 1] - center
            };
            // Piecewise-linear cyclic triangle: the weight reaches zero at
            // each neighbouring centre and is one at this centre.
            let clockwise = (hue - center).rem_euclid(360.0);
            let counterclockwise = (center - hue).rem_euclid(360.0);
            if clockwise <= next {
                1.0 - clockwise / next
            } else if counterclockwise <= previous {
                1.0 - counterclockwise / previous
            } else {
                0.0
            }
        });
        let weight_sum: f32 = weights.iter().sum();
        if weight_sum <= f32::EPSILON {
            continue;
        }
        for (i, channel) in channels.iter().enumerate() {
            let w = weights[i] / weight_sum;
            if let Some(channel) = channel {
                dh += channel.hue * 30.0 * w;
                ds += channel.saturation * w;
                dl += channel.luminance * w;
            }
        }
        hue = (hue + dh).rem_euclid(360.0);
        sat = (sat + ds).clamp(0.0, 1.0);
        l = (l + dl).clamp(0.0, 1.0);
        let rgb = hsl_to_rgb(hue, sat, l);
        px[0] = (rgb[0] * 255.0).round() as u8;
        px[1] = (rgb[1] * 255.0).round() as u8;
        px[2] = (rgb[2] * 255.0).round() as u8;
    }
    Ok(())
}

fn apply_presence(pixels: &mut [u8], vibrance: Option<&f64>, saturation: Option<&f64>) {
    if vibrance.is_none() && saturation.is_none() {
        return;
    }
    let vibrance = vibrance.copied().unwrap_or(0.0) as f32;
    let saturation = saturation.copied().unwrap_or(0.0) as f32;
    for px in pixels.chunks_exact_mut(4) {
        let (hue, mut sat, lightness) = rgb_to_hsl(
            px[0] as f32 / 255.0,
            px[1] as f32 / 255.0,
            px[2] as f32 / 255.0,
        );
        if vibrance != 0.0 {
            // Skin protection is 0 in the soft core [15°,55°], ramps linearly
            // to 1 in [5°,15°] and [55°,65°], and is 1 outside those ramps.
            let skin_protection = if !(5.0..=65.0).contains(&hue) {
                1.0
            } else if hue < 15.0 {
                (15.0 - hue) / 10.0
            } else if hue <= 55.0 {
                0.0
            } else {
                (hue - 55.0) / 10.0
            };
            // The low-saturation factor protects already vivid colours. For a
            // negative value, multiplying by sat also avoids a linear desaturator.
            let protection = (1.0 - sat) * skin_protection;
            let direction_weight = if vibrance >= 0.0 { 1.0 - sat } else { sat };
            sat = (sat + vibrance * protection * direction_weight).clamp(0.0, 1.0);
        }
        sat = (sat * (1.0 + saturation)).clamp(0.0, 1.0);
        let rgb = hsl_to_rgb(hue, sat, lightness);
        px[0] = (rgb[0] * 255.0).round() as u8;
        px[1] = (rgb[1] * 255.0).round() as u8;
        px[2] = (rgb[2] * 255.0).round() as u8;
    }
}

fn apply_color_grading(pixels: &mut [u8], grading: &lumina_sidecar::ColorGrading) {
    // Positive balance moves both transition points downward (0.15 max): the
    // highlight region expands toward shadows, matching Lightroom's direction.
    let shadow_edge = 0.65 - grading.balance * 0.15;
    let highlight_edge = 0.35 - grading.balance * 0.15;
    let smooth = |edge: f32, value: f32| {
        let t = (value / edge).clamp(0.0, 1.0);
        1.0 - t * t * (3.0 - 2.0 * t)
    };
    for px in pixels.chunks_exact_mut(4) {
        let rgb = [
            px[0] as f32 / 255.0,
            px[1] as f32 / 255.0,
            px[2] as f32 / 255.0,
        ];
        let luminance = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
        let shadow = smooth(shadow_edge, luminance);
        let highlight = {
            let t = ((luminance - highlight_edge) / (1.0 - highlight_edge)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        let midtone = (1.0 - shadow - highlight).max(0.0);
        let sum = shadow + midtone + highlight;
        let weights = [shadow / sum, midtone / sum, highlight / sum];
        let ranges = [grading.shadows, grading.midtones, grading.highlights];
        let mut output = rgb;
        for (weight, range) in weights.into_iter().zip(ranges) {
            if range.saturation == 0.0 || weight == 0.0 {
                continue;
            }
            // Tint is the fully saturated HSL colour at L=0.5. Mixing is
            // channel-wise: x' = x + (tint - x) * weight * saturation.
            let tint = hsl_to_rgb(range.hue_degrees.rem_euclid(360.0), 1.0, 0.5);
            let amount = weight * range.saturation;
            for channel in 0..3 {
                output[channel] += (tint[channel] - output[channel]) * amount;
            }
        }
        for channel in 0..3 {
            px[channel] = (output[channel].clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let mut h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    if h < 0.0 {
        h += 360.0
    }
    (h, s, l)
}
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let q = if h < 60.0 {
        [c, x, 0.0]
    } else if h < 120.0 {
        [x, c, 0.0]
    } else if h < 180.0 {
        [0.0, c, x]
    } else if h < 240.0 {
        [0.0, x, c]
    } else if h < 300.0 {
        [x, 0.0, c]
    } else {
        [c, 0.0, x]
    };
    [q[0] + m, q[1] + m, q[2] + m]
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
            .apply_recipe(&recipe(&[("whites", 1.0), ("blacks", 1.0)]))
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

    #[test]
    fn identity_curve_and_hsl_preserve_rgb_and_alpha() {
        let identity = vec![
            lumina_sidecar::CurvePoint {
                input: 0.0,
                output: 0.0,
            },
            lumina_sidecar::CurvePoint {
                input: 1.0,
                output: 1.0,
            },
        ];
        let hsl = lumina_sidecar::HslAdjustments {
            version: 1,
            red: None,
            orange: None,
            yellow: None,
            green: None,
            cyan: None,
            blue: None,
            violet: None,
            magenta: None,
        };
        let mut frame = ImageFrame::new(1, 1, vec![80, 140, 210, 7]).unwrap();
        let recipe = EditRecipe {
            curves: Some(lumina_sidecar::Curves {
                version: 1,
                master: identity,
                channels: Default::default(),
            }),
            hsl: Some(hsl),
            ..Default::default()
        };
        frame.apply_recipe(&recipe).unwrap();
        assert_eq!(frame.pixels, vec![80, 140, 210, 7]);
    }

    #[test]
    fn apply_recipe_rejects_invalid_nested_curves() {
        let point = |input, output| lumina_sidecar::CurvePoint { input, output };
        let invalid_curves = [
            lumina_sidecar::Curves {
                version: 2,
                master: vec![point(0.0, 0.0), point(1.0, 1.0)],
                channels: Default::default(),
            },
            lumina_sidecar::Curves {
                version: 1,
                master: vec![point(0.0, 0.0)],
                channels: Default::default(),
            },
            lumina_sidecar::Curves {
                version: 1,
                master: vec![point(0.0, 0.0), point(0.5, 0.5), point(0.4, 1.0)],
                channels: Default::default(),
            },
            lumina_sidecar::Curves {
                version: 1,
                master: vec![point(0.0, 0.1), point(1.0, 1.0)],
                channels: Default::default(),
            },
            lumina_sidecar::Curves {
                version: 1,
                master: vec![point(0.0, 0.0), point(1.0, 0.9)],
                channels: lumina_sidecar::CurveChannels {
                    red: Some(vec![point(0.0, 0.0), point(1.0, f32::NAN)]),
                    ..Default::default()
                },
            },
        ];

        for curves in invalid_curves {
            let mut frame = ImageFrame::new(1, 1, vec![10, 20, 30, 255]).unwrap();
            let recipe = EditRecipe {
                curves: Some(curves),
                ..Default::default()
            };
            assert!(matches!(
                frame.apply_recipe(&recipe),
                Err(CoreError::InvalidAdjustment { .. })
            ));
        }
    }

    #[test]
    fn apply_recipe_rejects_invalid_nested_hsl() {
        for (version, channel) in [
            (2, None),
            (
                1,
                Some(lumina_sidecar::HslChannel {
                    hue: f32::INFINITY,
                    ..Default::default()
                }),
            ),
            (
                1,
                Some(lumina_sidecar::HslChannel {
                    saturation: -1.01,
                    ..Default::default()
                }),
            ),
            (
                1,
                Some(lumina_sidecar::HslChannel {
                    luminance: f32::NAN,
                    ..Default::default()
                }),
            ),
        ] {
            let mut frame = ImageFrame::new(1, 1, vec![10, 20, 30, 255]).unwrap();
            let recipe = EditRecipe {
                hsl: Some(lumina_sidecar::HslAdjustments {
                    version,
                    red: channel,
                    ..Default::default()
                }),
                ..Default::default()
            };
            assert!(matches!(
                frame.apply_recipe(&recipe),
                Err(CoreError::InvalidAdjustment { .. })
            ));
        }
    }

    #[test]
    fn vibrance_and_saturation_preserve_alpha_and_have_identity() {
        let original = ImageFrame::new(2, 1, vec![80, 140, 210, 7, 220, 80, 80, 19]).unwrap();
        let mut identity = original.clone();
        identity.apply_recipe(&recipe(&[])).unwrap();
        assert_eq!(identity, original);
        let mut adjusted = original.clone();
        adjusted
            .apply_recipe(&recipe(&[("vibrance", 0.5), ("saturation", 0.25)]))
            .unwrap();
        assert_eq!(&adjusted.pixels[3..4], &[7]);
        assert_eq!(&adjusted.pixels[7..8], &[19]);
        assert_ne!(&adjusted.pixels[..3], &original.pixels[..3]);
    }

    #[test]
    fn color_grading_accepts_cyclic_hue_and_preserves_alpha() {
        let range = |hue_degrees| lumina_sidecar::ColorGradingRange {
            hue_degrees,
            saturation: 0.7,
        };
        let grading = lumina_sidecar::ColorGrading {
            version: 1,
            shadows: range(360.0),
            midtones: range(120.0),
            highlights: range(240.0),
            balance: 0.0,
        };
        let mut a = ImageFrame::new(1, 1, vec![30, 40, 50, 13]).unwrap();
        let mut b = a.clone();
        let mut zero = grading.clone();
        zero.shadows.hue_degrees = 0.0;
        a.apply_recipe(&EditRecipe {
            color_grading: Some(grading),
            ..Default::default()
        })
        .unwrap();
        b.apply_recipe(&EditRecipe {
            color_grading: Some(zero),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(a.pixels, b.pixels);
        assert_eq!(a.pixels[3], 13);
    }

    #[test]
    fn color_grading_rejects_invalid_fields() {
        let base = lumina_sidecar::ColorGradingRange {
            hue_degrees: 0.0,
            saturation: 0.0,
        };
        for grading in [
            lumina_sidecar::ColorGrading {
                version: 2,
                shadows: base,
                midtones: base,
                highlights: base,
                balance: 0.0,
            },
            lumina_sidecar::ColorGrading {
                version: 1,
                shadows: lumina_sidecar::ColorGradingRange {
                    hue_degrees: 361.0,
                    ..base
                },
                midtones: base,
                highlights: base,
                balance: 0.0,
            },
            lumina_sidecar::ColorGrading {
                version: 1,
                shadows: lumina_sidecar::ColorGradingRange {
                    saturation: 1.1,
                    ..base
                },
                midtones: base,
                highlights: base,
                balance: 0.0,
            },
            lumina_sidecar::ColorGrading {
                version: 1,
                shadows: base,
                midtones: base,
                highlights: base,
                balance: 1.1,
            },
        ] {
            let mut frame = ImageFrame::new(1, 1, vec![10, 20, 30, 255]).unwrap();
            assert!(matches!(
                frame.apply_recipe(&EditRecipe {
                    color_grading: Some(grading),
                    ..Default::default()
                }),
                Err(CoreError::InvalidAdjustment { .. })
            ));
        }
    }

    fn hsl_recipe(channel: usize, adjustment: lumina_sidecar::HslChannel) -> EditRecipe {
        let mut channels = [None; 8];
        channels[channel] = Some(adjustment);
        EditRecipe {
            hsl: Some(lumina_sidecar::HslAdjustments {
                version: 1,
                red: channels[0],
                orange: channels[1],
                yellow: channels[2],
                green: channels[3],
                cyan: channels[4],
                blue: channels[5],
                violet: channels[6],
                magenta: channels[7],
            }),
            ..Default::default()
        }
    }

    #[test]
    fn hsl_violet_and_magenta_have_distinct_centres() {
        let mut frame = ImageFrame::new(1, 1, {
            let rgb = hsl_to_rgb(270.0, 1.0, 0.5);
            vec![
                (rgb[0] * 255.0).round() as u8,
                (rgb[1] * 255.0).round() as u8,
                (rgb[2] * 255.0).round() as u8,
                255,
            ]
        })
        .unwrap();
        frame
            .apply_recipe(&hsl_recipe(
                6,
                lumina_sidecar::HslChannel {
                    hue: 1.0,
                    ..Default::default()
                },
            ))
            .unwrap();
        let (hue, _, _) = rgb_to_hsl(
            frame.pixels[0] as f32 / 255.0,
            frame.pixels[1] as f32 / 255.0,
            frame.pixels[2] as f32 / 255.0,
        );
        assert!(
            (hue - 300.0).abs() < 1.0,
            "violet centre must be 270 degrees, got {hue}"
        );

        let mut frame = ImageFrame::new(1, 1, {
            let rgb = hsl_to_rgb(300.0, 1.0, 0.5);
            vec![
                (rgb[0] * 255.0).round() as u8,
                (rgb[1] * 255.0).round() as u8,
                (rgb[2] * 255.0).round() as u8,
                255,
            ]
        })
        .unwrap();
        frame
            .apply_recipe(&hsl_recipe(
                7,
                lumina_sidecar::HslChannel {
                    hue: -1.0,
                    ..Default::default()
                },
            ))
            .unwrap();
        let (hue, _, _) = rgb_to_hsl(
            frame.pixels[0] as f32 / 255.0,
            frame.pixels[1] as f32 / 255.0,
            frame.pixels[2] as f32 / 255.0,
        );
        assert!(
            (hue - 270.0).abs() < 1.0,
            "magenta centre must be 300 degrees, got {hue}"
        );
    }

    #[test]
    fn hsl_neighbour_contributions_are_normalized() {
        let rgb = hsl_to_rgb(45.0, 0.6, 0.5);
        let mut frame = ImageFrame::new(
            1,
            1,
            vec![
                (rgb[0] * 255.0).round() as u8,
                (rgb[1] * 255.0).round() as u8,
                (rgb[2] * 255.0).round() as u8,
                255,
            ],
        )
        .unwrap();
        let mut recipe = hsl_recipe(
            1,
            lumina_sidecar::HslChannel {
                hue: 1.0,
                ..Default::default()
            },
        );
        recipe.hsl.as_mut().unwrap().yellow = Some(lumina_sidecar::HslChannel {
            hue: -1.0,
            ..Default::default()
        });
        frame.apply_recipe(&recipe).unwrap();
        assert_eq!(
            frame.pixels[0..3],
            [
                (rgb[0] * 255.0).round() as u8,
                (rgb[1] * 255.0).round() as u8,
                (rgb[2] * 255.0).round() as u8,
            ]
        );
    }

    #[test]
    fn hsl_saturation_and_luminance_are_additive() {
        let rgb = hsl_to_rgb(0.0, 0.4, 0.5);
        let mut frame = ImageFrame::new(
            1,
            1,
            vec![
                (rgb[0] * 255.0).round() as u8,
                (rgb[1] * 255.0).round() as u8,
                (rgb[2] * 255.0).round() as u8,
                255,
            ],
        )
        .unwrap();
        frame
            .apply_recipe(&hsl_recipe(
                0,
                lumina_sidecar::HslChannel {
                    saturation: 0.2,
                    luminance: 0.1,
                    ..Default::default()
                },
            ))
            .unwrap();
        let (_, saturation, luminance) = rgb_to_hsl(
            frame.pixels[0] as f32 / 255.0,
            frame.pixels[1] as f32 / 255.0,
            frame.pixels[2] as f32 / 255.0,
        );
        assert!((saturation - 0.6).abs() < 0.02);
        assert!((luminance - 0.6).abs() < 0.01);
    }
}
