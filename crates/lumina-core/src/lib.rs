//! Small, portable raster MVP shared by native CLI and future WASM clients.

use image::{
    codecs::jpeg::JpegEncoder, codecs::webp::WebPEncoder, ColorType, DynamicImage, ImageEncoder,
    ImageFormat, RgbaImage,
};
use lumina_sidecar::EditRecipe;
use std::io::Cursor;
use thiserror::Error;

pub mod cache;
pub mod histogram;
pub mod mask_loader;
pub mod mask_modulation;
pub mod masks;
pub mod memory;
pub mod pipeline;
pub mod render;
pub mod tone;
#[cfg(test)]
mod tone_props;
#[cfg(not(target_arch = "wasm32"))]
pub use cache::disk::{DiskCacheError, DiskFolderCache};
pub use cache::{
    CacheEntry, CacheError, CacheStage, CacheStore, Cancellation, FolderCache, FolderCacheSettings,
    StaleTracker,
};
pub use histogram::LuminanceHistogram;
pub use mask_loader::{
    resolve_mask_planes, MaskInference, MaskLoadContext, MaskLoadOutcome, MaskLoadResult,
    MaskResolvedFrom,
};
pub use mask_modulation::modulate_mask_plane;
pub use masks::{MaskError, MaskGraph, MaskPlane};
pub use memory::{MemoryBudget, MemoryBudgetError};
pub use pipeline::{OutputSpec, Pipeline, PipelineFormat, PipelineStage, RenderKey, SourceAction};
pub use render::{
    render_frame, MaskContext, MaskLayerResult, MaskPolicy, RenderContext, RenderOutput,
    SourceActionArtifact,
};
pub use tone::{
    analyze_tone, match_total_exposure, match_total_exposure_masked, suggest_auto_tone,
    tone_fingerprint, AutoToneConfig, AutoToneResult, ToneAnalysis,
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeasurementDomain {
    pub output_width: u32,
    pub output_height: u32,
    /// Normalized source rectangle before rotation/mirroring.
    pub source_x: f32,
    pub source_y: f32,
    pub source_width: f32,
    pub source_height: f32,
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
    #[error("invalid source action: {0}")]
    InvalidSourceAction(String),
    #[error("invalid mask plane dimensions {width}x{height} for {length} values")]
    InvalidMaskPlane {
        width: u32,
        height: u32,
        length: usize,
    },
    #[error("mask `{copy_id}/{mask_id}` is unavailable (status {status})")]
    MaskUnavailable {
        copy_id: String,
        mask_id: String,
        status: String,
    },
    #[error("mask `{copy_id}/{mask_id}` could not be evaluated: {reason}")]
    MaskEvaluation {
        copy_id: String,
        mask_id: String,
        reason: String,
    },
    #[error("mask re-inference failed: {reason}")]
    MaskInference { reason: String },
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

    /// Apply the crop stage: distortion → vignette → perspective → CA → crop
    /// → rotation → mirroring. Coordinates for the crop are normalized on the
    /// perspective-transformed image. All resampling is inverse bilinear with
    /// black (zero RGBA) outside the source.
    ///
    /// When the `lensfun` feature is enabled, an optional [`lumina_lensfun::Corrector`]
    /// overrides the manual distortion + vignette model per pixel (and is applied
    /// even when the recipe carries no manual `LensCorrection`). Chromatic
    /// aberration stays manual (recipe lens only), matching the F-098 MVP limit.
    pub fn apply_geometry(
        &mut self,
        geometry: Option<&lumina_sidecar::Geometry>,
        lens: Option<&lumina_sidecar::LensCorrection>,
        perspective: Option<&lumina_sidecar::Perspective>,
        #[cfg(feature = "lensfun")] lensfun: Option<&lumina_lensfun::Corrector>,
    ) -> Result<(), CoreError> {
        // Distortion + vignette: a Lensfun corrector overrides the manual model
        // and is applied even when the recipe carries no manual `LensCorrection`
        // (F-098-N1). Chromatic aberration stays manual (recipe lens only).
        #[cfg(feature = "lensfun")]
        let use_corrector = lensfun.is_some();
        #[cfg(feature = "lensfun")]
        if use_corrector {
            if let Some(l) = lens {
                validate_lens(l)?;
            }
            let manual = lens.unwrap_or(&EMPTY_LENS);
            apply_lens(self, manual, lensfun);
        } else if let Some(l) = lens {
            validate_lens(l)?;
            apply_lens(
                self,
                l,
                #[cfg(feature = "lensfun")]
                None,
            );
        }
        #[cfg(not(feature = "lensfun"))]
        if let Some(l) = lens {
            validate_lens(l)?;
            apply_lens(self, l);
        }
        if let Some(p) = perspective {
            validate_perspective(p)?;
            *self = apply_perspective(self, p);
        }
        // CA stays manual (recipe lens only), applied after perspective like the
        // original order (distortion → perspective → CA → crop).
        if let Some(l) = lens {
            apply_ca(self, l);
        }
        let Some(geometry) = geometry else {
            return Ok(());
        };
        if geometry.version != 1
            || !geometry.rotation_degrees.is_finite()
            || !(-180.0..=180.0).contains(&geometry.rotation_degrees)
        {
            return Err(CoreError::InvalidAdjustment {
                name: "geometry.version/rotation".into(),
                value: geometry.rotation_degrees as f64,
                minimum: -180.0,
                maximum: 180.0,
            });
        }
        let (x, y, w, h) = crop_rect(self.width, self.height, geometry.crop.as_ref())?;
        let cropped = crop_frame(self, x, y, w, h)?;
        let mut transformed = rotate_frame(&cropped, geometry.rotation_degrees);
        if geometry.mirror_horizontal {
            flip_horizontal(&mut transformed);
        }
        if geometry.mirror_vertical {
            flip_vertical(&mut transformed);
        }
        *self = transformed;
        Ok(())
    }

    pub fn measurement_domain(
        &self,
        geometry: Option<&lumina_sidecar::Geometry>,
    ) -> Result<MeasurementDomain, CoreError> {
        self.measurement_domain_with_perspective(geometry, None, None)
    }

    /// Computes dimensions in the same order as rendering: lens (same bounds),
    /// perspective (projected-corner bounding box), crop, rotation, mirror.
    pub fn measurement_domain_with_perspective(
        &self,
        geometry: Option<&lumina_sidecar::Geometry>,
        lens: Option<&lumina_sidecar::LensCorrection>,
        perspective: Option<&lumina_sidecar::Perspective>,
    ) -> Result<MeasurementDomain, CoreError> {
        if let Some(l) = lens {
            validate_lens(l)?;
        }
        let (base_width, base_height) =
            perspective_dimensions(self.width, self.height, perspective)?;
        let Some(g) = geometry else {
            return Ok(MeasurementDomain {
                output_width: base_width,
                output_height: base_height,
                source_x: 0.0,
                source_y: 0.0,
                source_width: 1.0,
                source_height: 1.0,
            });
        };
        let (x, y, w, h) = crop_rect(base_width, base_height, g.crop.as_ref())?;
        let rotated = rotate_dimensions(w, h, g.rotation_degrees);
        Ok(MeasurementDomain {
            output_width: rotated.0,
            output_height: rotated.1,
            source_x: x as f32 / base_width as f32,
            source_y: y as f32 / base_height as f32,
            source_width: w as f32 / base_width as f32,
            source_height: h as f32 / base_height as f32,
        })
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
        self.apply_recipe_with_scale_and_white_balance(recipe, 1.0, None)
    }

    /// Applies adjustments with an explicit As-Shot white-balance context.
    ///
    /// `camera_white_balance` carries the RAW decoder's As-Shot gains
    /// (`RawMetadata.camera_white_balance`, cam_mul) and is the explicit basis
    /// that makes As-Shot rendering available at the core API.  Because the
    /// decoder already applied those gains to the frame, they are **not**
    /// applied again: a recipe without `wb_temperature`/`wb_tint` keeps the
    /// identity semantics, and a recipe with those keys keeps the exact
    /// deterministic sRGB approximation used by `apply_recipe`.  The context
    /// is validated before any pixel mutation: `Some(gains)` requires all four
    /// values to be finite and strictly greater than zero, otherwise
    /// [`CoreError::InvalidAdjustment`] is returned and the frame is left
    /// unchanged.  `None` keeps the previous identity semantics (this is what
    /// [`Self::apply_recipe`] uses).
    pub fn apply_recipe_with_white_balance(
        &mut self,
        recipe: &EditRecipe,
        camera_white_balance: Option<[f32; 4]>,
    ) -> Result<(), CoreError> {
        self.apply_recipe_with_scale_and_white_balance(recipe, 1.0, camera_white_balance)
    }

    /// Applies adjustments at an explicit effective output scale.  Keeping the
    /// old method above preserves CLI/GUI API compatibility; radius-sensitive
    /// sharpening uses this scale in source-pixel units.
    pub fn apply_recipe_with_scale(
        &mut self,
        recipe: &EditRecipe,
        effective_scale: f32,
    ) -> Result<(), CoreError> {
        self.apply_recipe_with_scale_and_white_balance(recipe, effective_scale, None)
    }

    fn apply_recipe_with_scale_and_white_balance(
        &mut self,
        recipe: &EditRecipe,
        effective_scale: f32,
        camera_white_balance: Option<[f32; 4]>,
    ) -> Result<(), CoreError> {
        if !effective_scale.is_finite() || effective_scale <= 0.0 {
            return Err(CoreError::InvalidAdjustment {
                name: "effective_scale".into(),
                value: effective_scale as f64,
                minimum: f32::MIN_POSITIVE as f64,
                maximum: f32::MAX as f64,
            });
        }
        if let Some(gains) = camera_white_balance {
            for gain in gains {
                if !gain.is_finite() || gain <= 0.0 {
                    return Err(CoreError::InvalidAdjustment {
                        name: "camera_white_balance".into(),
                        value: gain as f64,
                        minimum: f32::MIN_POSITIVE as f64,
                        maximum: f64::MAX,
                    });
                }
            }
        }
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
        // White-balance gains are derived exactly as before (only when a WB key
        // is present). They feed the fused channel-LUT kernel below, which
        // composes WB + exposure + contrast + shadows + highlights + whites +
        // blacks into a single pass via precomputed per-channel lookup tables
        // (see `apply_channel_lut_adjustments`). The fusion is byte-identical to
        // the previous per-pixel pass-by-pass implementation.
        let wb_gains = if recipe.adjustments.contains_key("wb_temperature")
            || recipe.adjustments.contains_key("wb_tint")
        {
            let temperature = recipe
                .adjustments
                .get("wb_temperature")
                .copied()
                .unwrap_or(6500.0);
            let tint = recipe.adjustments.get("wb_tint").copied().unwrap_or(0.0);
            let warmth = (temperature - 6500.0) / 5500.0;
            Some([1.0 - warmth * 0.35, 1.0 - tint * 0.20, 1.0 + warmth * 0.35])
        } else {
            None
        };
        let exposure_multiplier = recipe
            .adjustments
            .get("exposure")
            .map(|exposure| 2.0_f64.powf(*exposure));
        let contrast_factor = recipe.adjustments.get("contrast").map(|c| 1.0 + *c);
        let shadows = recipe.adjustments.get("shadows").copied();
        let highlights = recipe.adjustments.get("highlights").copied();
        let whites = recipe.adjustments.get("whites").copied();
        let blacks = recipe.adjustments.get("blacks").copied();
        apply_channel_lut_adjustments(
            &mut self.pixels,
            &ChannelLutParams {
                wb_gains,
                exposure_multiplier,
                contrast_factor,
                shadows,
                highlights,
                whites,
                blacks,
            },
        );
        if let Some(presence) = &recipe.presence {
            apply_presence(&mut self.pixels, self.width, self.height, presence);
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
        apply_vibrance_and_saturation(
            &mut self.pixels,
            recipe.adjustments.get("vibrance"),
            recipe.adjustments.get("saturation"),
        );
        if let Some(color_grading) = &recipe.color_grading {
            apply_color_grading(&mut self.pixels, color_grading);
        }
        if let Some(noise) = &recipe.noise_reduction {
            apply_noise_reduction(&mut self.pixels, self.width, self.height, noise);
        }
        if let Some(sharpening) = &recipe.sharpening {
            apply_sharpening(
                &mut self.pixels,
                self.width,
                self.height,
                sharpening,
                effective_scale,
            );
        }
        // F-097: vignette + grain are the LAST sub-stage of `Adjustments`,
        // after sharpening and before masks / crop. The pixel tuple is unchanged.
        if let Some(effects) = &recipe.effects {
            if let Some(vignette) = &effects.vignette {
                apply_vignette(&mut self.pixels, self.width, self.height, vignette);
            }
            if let Some(grain) = &effects.grain {
                apply_grain(&mut self.pixels, self.width, self.height, grain);
            }
        }
        Ok(())
    }
}

/// Fuses the per-channel scalar adjustment stages — white balance, followed by
/// exposure, contrast, shadows, highlights, whites and blacks — into a single
/// pass over the pixels using one precomputed 256-entry lookup table per channel.
///
/// Each stage is a pure per-channel function `u8 -> u8`: it only touches the
/// channel it operates on, reading the rounded and clamped `u8` output of the
/// preceding stage. Their sequential composition is therefore also a pure
/// per-channel function, so computing one 256-entry table per channel once and
/// applying it with three table lookups per pixel is **byte-identical** to the
/// original pass-by-pass implementation, while moving all floating point work
/// (the exact same `f64` formulas as the original) out of the hot pixel loop.
///
/// The intermediate values are exact integers in `[0, 255]` that are
/// representable exactly by `f64`, so composing the stages in `f64` without
/// re-casting to `u8` between them yields the same result as the original code
/// that casts back to `u8` after every stage. This keeps the kernel fully
/// Portable (no native/SIMD intrinsics) and therefore WASM-compatible.
///
/// Bundles the per-channel scalar adjustment parameters so the fused kernel keeps
/// a small, clippy-clean signature while remaining easy to extend.
struct ChannelLutParams {
    wb_gains: Option<[f64; 3]>,
    exposure_multiplier: Option<f64>,
    contrast_factor: Option<f64>,
    shadows: Option<f64>,
    highlights: Option<f64>,
    whites: Option<f64>,
    blacks: Option<f64>,
}

fn apply_channel_lut_adjustments(pixels: &mut [u8], params: &ChannelLutParams) {
    let ChannelLutParams {
        wb_gains,
        exposure_multiplier,
        contrast_factor,
        shadows,
        highlights,
        whites,
        blacks,
    } = params;
    if wb_gains.is_none()
        && exposure_multiplier.is_none()
        && contrast_factor.is_none()
        && shadows.is_none()
        && highlights.is_none()
        && whites.is_none()
        && blacks.is_none()
    {
        return;
    }

    // Per-channel lookup tables: `lut[channel][value]` is the composed result for
    // that channel at the given input byte. The table build runs the same `f64`
    // math as the original per-pixel passes, but only 256 times per channel.
    let mut lut = [[0u8; 256]; 3];
    for (channel, lut_channel) in lut.iter_mut().enumerate() {
        for input in 0u16..=255 {
            let mut value = input as f64;
            if let Some(gains) = wb_gains {
                value = (value * gains[channel]).round().clamp(0.0, 255.0);
            }
            if let Some(multiplier) = exposure_multiplier {
                value = (value * multiplier).round().clamp(0.0, 255.0);
            }
            if let Some(factor) = contrast_factor {
                value = ((value - 128.0) * factor + 128.0).round().clamp(0.0, 255.0);
            }
            if let Some(amount) = shadows {
                let x = value / 255.0;
                let weight = ((0.5 - x) / 0.5).max(0.0).powi(2);
                value = ((x + amount * weight * 0.25).clamp(0.0, 1.0) * 255.0).round();
            }
            if let Some(amount) = highlights {
                let x = value / 255.0;
                let weight = ((x - 0.5) / 0.5).max(0.0).powi(2);
                value = ((x + amount * weight * 0.25).clamp(0.0, 1.0) * 255.0).round();
            }
            if let Some(amount) = whites {
                let x = value / 255.0;
                let weight = ((x - 0.5) / 0.5).max(0.0);
                value = ((x + amount * weight * 0.25).clamp(0.0, 1.0) * 255.0).round();
            }
            if let Some(amount) = blacks {
                let x = value / 255.0;
                let weight = ((0.5 - x) / 0.5).max(0.0);
                value = ((x - amount * weight * 0.25).clamp(0.0, 1.0) * 255.0).round();
            }
            lut_channel[input as usize] = value.clamp(0.0, 255.0) as u8;
        }
    }

    // Single fused pass: three table lookups per pixel, no floating point.
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[0] = lut[0][pixel[0] as usize];
        pixel[1] = lut[1][pixel[1] as usize];
        pixel[2] = lut[2][pixel[2] as usize];
    }
}

fn validate_lens(l: &lumina_sidecar::LensCorrection) -> Result<(), CoreError> {
    if l.version != 1 {
        return Err(CoreError::InvalidAdjustment {
            name: "lens_correction.version".into(),
            value: l.version as f64,
            minimum: 1.0,
            maximum: 1.0,
        });
    }
    if let Some(profile) = l.profile.as_deref() {
        if !matches!(profile, "wide-light" | "tele-light" | "standard-neutral") {
            return Err(CoreError::UnsupportedAdjustment {
                key: format!("lens profile `{profile}`"),
            });
        }
    }
    for (name, v, lo, hi) in [
        ("distortion_k1", l.distortion_k1, -1., 1.),
        ("distortion_k2", l.distortion_k2, -1., 1.),
        ("distortion_k3", l.distortion_k3, -1., 1.),
        ("vignette_c0", l.vignette_c0, -1., 1.),
        ("vignette_c1", l.vignette_c1, -1., 1.),
        ("vignette_c2", l.vignette_c2, -1., 1.),
        ("ca_red", l.ca_red, -0.05, 0.05),
        ("ca_blue", l.ca_blue, -0.05, 0.05),
    ]
    .into_iter()
    .filter_map(|(name, value, lo, hi)| value.map(|v| (name, v, lo, hi)))
    {
        if !v.is_finite() || !(lo..=hi).contains(&v) {
            return Err(CoreError::InvalidAdjustment {
                name: name.into(),
                value: v as f64,
                minimum: lo as f64,
                maximum: hi as f64,
            });
        }
    }
    Ok(())
}
fn validate_perspective(p: &lumina_sidecar::Perspective) -> Result<(), CoreError> {
    if p.version != 1 {
        return Err(CoreError::InvalidAdjustment {
            name: "perspective.version".into(),
            value: p.version as f64,
            minimum: 1.,
            maximum: 1.,
        });
    }
    for (name, v, lo, hi) in [
        ("vertical", p.vertical, -1., 1.),
        ("horizontal", p.horizontal, -1., 1.),
        ("rotation", p.rotation, -1., 1.),
        ("shift_x", p.shift_x, -1., 1.),
        ("shift_y", p.shift_y, -1., 1.),
        ("scale", p.scale, 0.1, 10.),
        ("aspect_ratio", p.aspect_ratio, 0.1, 10.),
    ] {
        if !v.is_finite() || !(lo..=hi).contains(&v) {
            return Err(CoreError::InvalidAdjustment {
                name: name.into(),
                value: v as f64,
                minimum: lo as f64,
                maximum: hi as f64,
            });
        }
    }
    Ok(())
}

/// Forward matrix is `T(shift) * R(rotation*pi/4) * S(scale,aspect) *
/// Hy(vertical) * Hx(horizontal)` on column vectors and normalized corners
/// `[-1,1]^2`.  `Hx=[[1,0,0],[0,1,0],[tan(h),0,1]]`,
/// `Hy=[[1,0,0],[0,1,0],[0,tan(v),1]]`, `S=diag(scale,scale*aspect,1)`,
/// and `T,R` are the usual translation and counter-clockwise rotation.
/// Rendering uses the inverse of this exact matrix for every output pixel.
fn perspective_matrix(p: &lumina_sidecar::Perspective) -> [[f32; 3]; 3] {
    let sh = (p.horizontal * std::f32::consts::FRAC_PI_4).tan();
    let sv = (p.vertical * std::f32::consts::FRAC_PI_4).tan();
    let a = p.rotation * std::f32::consts::FRAC_PI_4;
    let (s, c) = (a.sin(), a.cos());
    // `scale` is an output magnification: scale=2 doubles the projected
    // bounding box instead of shrinking it to half size.
    let sx = p.scale;
    let sy = p.scale * p.aspect_ratio;
    let t = [[1., 0., p.shift_x], [0., 1., p.shift_y], [0., 0., 1.]];
    let r = [[c, -s, 0.], [s, c, 0.], [0., 0., 1.]];
    let scale = [[sx, 0., 0.], [0., sy, 0.], [0., 0., 1.]];
    let hy = [[1., 0., 0.], [0., 1., 0.], [0., sv, 1.]];
    let hx = [[1., 0., 0.], [0., 1., 0.], [sh, 0., 1.]];
    fn mul(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
        let mut o = [[0.; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                for k in 0..3 {
                    o[i][j] += a[i][k] * b[k][j];
                }
            }
        }
        o
    }
    mul(mul(mul(mul(t, r), scale), hy), hx)
}

fn perspective_dimensions(
    w: u32,
    h: u32,
    p: Option<&lumina_sidecar::Perspective>,
) -> Result<(u32, u32), CoreError> {
    let Some(p) = p else {
        return Ok((w, h));
    };
    validate_perspective(p)?;
    let m = perspective_matrix(p);
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            let d = m[2][0] * x + m[2][1] * y + m[2][2];
            let q = [
                (m[0][0] * x + m[0][1] * y + m[0][2]) / d,
                (m[1][0] * x + m[1][1] * y + m[1][2]) / d,
            ];
            min[0] = min[0].min(q[0]);
            max[0] = max[0].max(q[0]);
            min[1] = min[1].min(q[1]);
            max[1] = max[1].max(q[1]);
        }
    }
    Ok((
        ((max[0] - min[0]) * w as f32 / 2.0).ceil().max(1.0) as u32,
        ((max[1] - min[1]) * h as f32 / 2.0).ceil().max(1.0) as u32,
    ))
}

// Presets are deliberately small and built in: wide-light (k1=0.12,k2=-0.04,k3=0.01,
// c0=1,c1=0,c2=0, CA R=0.006 B=-0.006), standard-neutral (0,0,0,1,0,0,0,0),
// tele-light (k1=-0.08,k2=0.02,k3=0,c0=1,c1=0,c2=0, CA R=-0.004 B=0.004).
fn lens_coefficients(l: &lumina_sidecar::LensCorrection) -> [f32; 8] {
    let mut c = match l.profile.as_deref() {
        Some("wide-light") => [0.12, -0.04, 0.01, 1., 0., 0., 0.006, -0.006],
        Some("tele-light") => [-0.08, 0.02, 0., 1., 0., 0., -0.004, 0.004],
        Some("standard-neutral") => [0., 0., 0., 1., 0., 0., 0., 0.],
        None => [0., 0., 0., 1., 0., 0., 0., 0.],
        Some(other) => panic!("validated lens profile unexpectedly reached renderer: {other}"),
    };
    let explicit = [
        l.distortion_k1.unwrap_or(c[0]),
        l.distortion_k2.unwrap_or(c[1]),
        l.distortion_k3.unwrap_or(c[2]),
        l.vignette_c0.unwrap_or(c[3]),
        l.vignette_c1.unwrap_or(c[4]),
        l.vignette_c2.unwrap_or(c[5]),
        l.ca_red.unwrap_or(c[6]),
        l.ca_blue.unwrap_or(c[7]),
    ];
    c.copy_from_slice(&explicit);
    c
}
fn sample(frame: &ImageFrame, x: f32, y: f32, ch: usize) -> f32 {
    if x < 0.0 || y < 0.0 || x >= frame.width as f32 || y >= frame.height as f32 {
        return 0.0;
    }
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(frame.width - 1);
    let y1 = (y0 + 1).min(frame.height - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let at = |xx, yy| frame.pixels[(yy * frame.width + xx) as usize * 4 + ch] as f32;
    (at(x0, y0) * (1. - fx) + at(x1, y0) * fx) * (1. - fy)
        + (at(x0, y1) * (1. - fx) + at(x1, y1) * fx) * fy
}
/// Empty manual lens model used to drive [`apply_lens`] through the Lensfun
/// corrector path when the recipe carries no manual `LensCorrection` (F-098-N1).
#[cfg(feature = "lensfun")]
const EMPTY_LENS: lumina_sidecar::LensCorrection = lumina_sidecar::LensCorrection {
    version: 1,
    profile: None,
    distortion_k1: None,
    distortion_k2: None,
    distortion_k3: None,
    vignette_c0: None,
    vignette_c1: None,
    vignette_c2: None,
    ca_red: None,
    ca_blue: None,
};

fn apply_lens(
    frame: &mut ImageFrame,
    l: &lumina_sidecar::LensCorrection,
    #[cfg(feature = "lensfun")] lensfun: Option<&lumina_lensfun::Corrector>,
) {
    // F-098-N1: a Lensfun corrector (when present and non-identity) replaces the
    // manual radial-distortion Newton iteration and the vignette polynomial with
    // the database profile, per pixel. The corrector's geometry maps a
    // destination (corrected) pixel `(x, y)` in `[0, width-1] × [0, height-1]`
    // to the source (distorted) pixel to sample — the same pixel space
    // `apply_lens` iterates over. Vignetting is applied via `color_gain` on the
    // RGB channels only; the alpha channel is left untouched (same structure as
    // the manual model).
    #[cfg(feature = "lensfun")]
    if let Some(corrector) = lensfun {
        if !corrector.is_identity() {
            let src = frame.clone();
            for y in 0..frame.height {
                for x in 0..frame.width {
                    let (sx, sy) = corrector.geometry(x as f64, y as f64);
                    let i = (y * frame.width + x) as usize * 4;
                    let r = sample(&src, sx as f32, sy as f32, 0);
                    let g = sample(&src, sx as f32, sy as f32, 1);
                    let b = sample(&src, sx as f32, sy as f32, 2);
                    let (cr, cg, cb) = corrector.color_gain(r, g, b, x as f64, y as f64);
                    frame.pixels[i] = (cr).round().clamp(0.0, 255.0) as u8;
                    frame.pixels[i + 1] = (cg).round().clamp(0.0, 255.0) as u8;
                    frame.pixels[i + 2] = (cb).round().clamp(0.0, 255.0) as u8;
                    frame.pixels[i + 3] = sample(&src, sx as f32, sy as f32, 3)
                        .round()
                        .clamp(0.0, 255.0) as u8;
                }
            }
            return;
        }
    }
    // Manual model (unchanged behaviour).
    let c = lens_coefficients(l);
    let src = frame.clone();
    let (w, h) = (frame.width as f32, frame.height as f32);
    let diag = (w * w + h * h).sqrt() / 2.;
    for y in 0..frame.height {
        for x in 0..frame.width {
            let nx = (x as f32 - (w - 1.) / 2.) / diag;
            let ny = (y as f32 - (h - 1.) / 2.) / diag;
            let target = (nx * nx + ny * ny).sqrt();
            let mut r = target;
            for _ in 0..8 {
                let f = r * (1. + c[0] * r * r + c[1] * r.powi(4) + c[2] * r.powi(6)) - target;
                let d = 1. + 3. * c[0] * r * r + 5. * c[1] * r.powi(4) + 7. * c[2] * r.powi(6);
                r = (r - f / d).max(0.);
            }
            let q = if target > 1e-6 { r / target } else { 1. };
            let sx = (w - 1.) / 2. + nx * q * diag;
            let sy = (h - 1.) / 2. + ny * q * diag;
            let vig = (c[3] + c[4] * target * target + c[5] * target.powi(4)).max(0.01);
            let i = (y * frame.width + x) as usize * 4;
            for ch in 0..3 {
                frame.pixels[i + ch] =
                    (sample(&src, sx, sy, ch) * vig).round().clamp(0., 255.) as u8;
            }
            frame.pixels[i + 3] = sample(&src, sx, sy, 3).round().clamp(0., 255.) as u8;
        }
    }
}
fn apply_ca(frame: &mut ImageFrame, l: &lumina_sidecar::LensCorrection) {
    let c = lens_coefficients(l);
    let src = frame.clone();
    let cx = (frame.width as f32 - 1.) / 2.;
    let cy = (frame.height as f32 - 1.) / 2.;
    for y in 0..frame.height {
        for x in 0..frame.width {
            let i = (y * frame.width + x) as usize * 4;
            for (ch, k) in [(0, c[6]), (2, c[7])] {
                let sx = cx + (x as f32 - cx) * (1. + k);
                let sy = cy + (y as f32 - cy) * (1. + k);
                frame.pixels[i + ch] = sample(&src, sx, sy, ch).round().clamp(0., 255.) as u8;
            }
        }
    }
}
fn apply_perspective(src: &ImageFrame, p: &lumina_sidecar::Perspective) -> ImageFrame {
    if p.vertical == 0.
        && p.horizontal == 0.
        && p.rotation == 0.
        && p.scale == 1.
        && p.aspect_ratio == 1.
        && p.shift_x == 0.
        && p.shift_y == 0.
    {
        return src.clone();
    }
    let m = perspective_matrix(p);
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            let d = m[2][0] * x + m[2][1] * y + m[2][2];
            let q = [
                (m[0][0] * x + m[0][1] * y + m[0][2]) / d,
                (m[1][0] * x + m[1][1] * y + m[1][2]) / d,
            ];
            min[0] = min[0].min(q[0]);
            max[0] = max[0].max(q[0]);
            min[1] = min[1].min(q[1]);
            max[1] = max[1].max(q[1]);
        }
    }
    let ow = ((max[0] - min[0]) * src.width as f32 / 2.0).ceil().max(1.0) as u32;
    let oh = ((max[1] - min[1]) * src.height as f32 / 2.0)
        .ceil()
        .max(1.0) as u32;
    let mut out = ImageFrame::new(
        ow.max(1),
        oh.max(1),
        vec![0; ow.max(1) as usize * oh.max(1) as usize * 4],
    )
    .unwrap();
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let inv = |x: f32, y: f32| {
        let z = [x, y, 1.];
        let a = (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * z[0]
            + (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * z[1]
            + (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * z[2];
        let b = (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * z[0]
            + (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * z[1]
            + (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * z[2];
        let d = (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * z[0]
            + (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * z[1]
            + (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * z[2];
        (a / det, b / det, d / det)
    };
    for y in 0..out.height {
        for x in 0..out.width {
            // Keep the output canvas centered. Translation therefore remains
            // visible as a shift within the projected bounding box instead of
            // being cancelled by translating both bbox endpoints.
            let range_x = max[0] - min[0];
            let range_y = max[1] - min[1];
            let canvas_min_x = -range_x / 2.0;
            let canvas_min_y = -range_y / 2.0;
            let nx =
                canvas_min_x + (x as f32 / (out.width.saturating_sub(1).max(1)) as f32) * range_x;
            let ny =
                canvas_min_y + (y as f32 / (out.height.saturating_sub(1).max(1)) as f32) * range_y;
            let (sx, sy, sd) = inv(nx, ny);
            let sx = sx / sd;
            let sy = sy / sd;
            let px = (sx / 2. + 0.5) * (src.width - 1) as f32;
            let py = (sy / 2. + 0.5) * (src.height - 1) as f32;
            let i = (y * out.width + x) as usize * 4;
            for ch in 0..4 {
                out.pixels[i + ch] = sample(src, px, py, ch).round().clamp(0., 255.) as u8;
            }
        }
    }
    out
}

fn crop_rect(
    width: u32,
    height: u32,
    crop: Option<&lumina_sidecar::Crop>,
) -> Result<(u32, u32, u32, u32), CoreError> {
    let (x, y, w, h) = match crop {
        None => (0.0, 0.0, 1.0, 1.0),
        Some(lumina_sidecar::Crop::Free {
            x,
            y,
            width,
            height,
        }) => (*x as f64, *y as f64, *width as f64, *height as f64),
        Some(lumina_sidecar::Crop::Aspect { preset }) => {
            let ratio = match preset {
                lumina_sidecar::AspectPreset::Original => width as f64 / height as f64,
                lumina_sidecar::AspectPreset::OneToOne => 1.0,
                lumina_sidecar::AspectPreset::FourToFive => 4.0 / 5.0,
                lumina_sidecar::AspectPreset::FiveToFour => 5.0 / 4.0,
                lumina_sidecar::AspectPreset::ThreeToTwo => 3.0 / 2.0,
                lumina_sidecar::AspectPreset::TwoToThree => 2.0 / 3.0,
                lumina_sidecar::AspectPreset::FourToThree => 4.0 / 3.0,
                lumina_sidecar::AspectPreset::ThreeToFour => 3.0 / 4.0,
                lumina_sidecar::AspectPreset::SixteenToNine => 16.0 / 9.0,
                lumina_sidecar::AspectPreset::NineToSixteen => 9.0 / 16.0,
            };
            let source_ratio = width as f64 / height as f64;
            if source_ratio > ratio {
                (
                    (1.0 - ratio / source_ratio) / 2.0,
                    0.0,
                    ratio / source_ratio,
                    1.0,
                )
            } else {
                (
                    0.0,
                    (1.0 - source_ratio / ratio) / 2.0,
                    1.0,
                    source_ratio / ratio,
                )
            }
        }
    };
    if ![x, y, w, h].iter().all(|v| v.is_finite())
        || w <= 0.0
        || h <= 0.0
        || x < 0.0
        || y < 0.0
        || x + w > 1.0 + 1e-6
        || y + h > 1.0 + 1e-6
    {
        return Err(CoreError::InvalidAdjustment {
            name: "geometry.crop".into(),
            value: -1.0,
            minimum: 0.0,
            maximum: 1.0,
        });
    }
    let px = (x * width as f64).round() as u32;
    let py = (y * height as f64).round() as u32;
    let pw = ((w * width as f64).round() as u32).max(1).min(width - px);
    let ph = ((h * height as f64).round() as u32).max(1).min(height - py);
    Ok((px, py, pw, ph))
}

fn crop_frame(frame: &ImageFrame, x: u32, y: u32, w: u32, h: u32) -> Result<ImageFrame, CoreError> {
    let mut out = vec![0; w as usize * h as usize * 4];
    for row in 0..h {
        let src = ((y + row) * frame.width + x) as usize * 4;
        let dst = (row * w) as usize * 4;
        out[dst..dst + w as usize * 4].copy_from_slice(&frame.pixels[src..src + w as usize * 4]);
    }
    ImageFrame::new(w, h, out)
}
fn rotate_dimensions(w: u32, h: u32, degrees: f32) -> (u32, u32) {
    let quarter_turn = degrees.rem_euclid(180.0).abs() < 1e-4;
    if quarter_turn {
        return (w.max(1), h.max(1));
    }
    let right_angle = (degrees - 90.0).rem_euclid(180.0).abs() < 1e-4;
    if right_angle {
        return (h.max(1), w.max(1));
    }
    let r = degrees.to_radians();
    (
        (w as f32 * r.cos().abs() + h as f32 * r.sin().abs())
            .ceil()
            .max(1.0) as u32,
        (w as f32 * r.sin().abs() + h as f32 * r.cos().abs())
            .ceil()
            .max(1.0) as u32,
    )
}
fn rotate_frame(frame: &ImageFrame, degrees: f32) -> ImageFrame {
    let turns = (degrees / 90.0).round();
    if (degrees - turns * 90.0).abs() < 1e-4 {
        let turn = (turns as i32).rem_euclid(4);
        if turn == 0 {
            return frame.clone();
        }
        let (ow, oh) = if turn % 2 == 0 {
            (frame.width, frame.height)
        } else {
            (frame.height, frame.width)
        };
        let mut out = vec![0; ow as usize * oh as usize * 4];
        for y in 0..frame.height {
            for x in 0..frame.width {
                let (dx, dy) = match turn {
                    1 => (frame.height - 1 - y, x),
                    2 => (frame.width - 1 - x, frame.height - 1 - y),
                    _ => (y, frame.width - 1 - x),
                };
                let source = (y * frame.width + x) as usize * 4;
                let destination = (dy * ow + dx) as usize * 4;
                out[destination..destination + 4]
                    .copy_from_slice(&frame.pixels[source..source + 4]);
            }
        }
        return ImageFrame::new(ow, oh, out).unwrap();
    }
    if degrees.rem_euclid(360.0).abs() < f32::EPSILON {
        return frame.clone();
    }
    let (ow, oh) = rotate_dimensions(frame.width, frame.height, degrees);
    let mut out = vec![0; ow as usize * oh as usize * 4];
    let r = degrees.to_radians();
    let (s, c) = (r.sin(), r.cos());
    for y in 0..oh {
        for x in 0..ow {
            let dx = x as f32 - (ow as f32 - 1.0) / 2.0;
            let dy = y as f32 - (oh as f32 - 1.0) / 2.0;
            let sx = c * dx + s * dy + (frame.width as f32 - 1.0) / 2.0;
            let sy = -s * dx + c * dy + (frame.height as f32 - 1.0) / 2.0;
            if sx >= 0.0 && sy >= 0.0 && sx < frame.width as f32 && sy < frame.height as f32 {
                let x0 = sx.floor() as u32;
                let y0 = sy.floor() as u32;
                let x1 = (x0 + 1).min(frame.width - 1);
                let y1 = (y0 + 1).min(frame.height - 1);
                let fx = sx - x0 as f32;
                let fy = sy - y0 as f32;
                let oi = (y * ow + x) as usize * 4;
                for ch in 0..4 {
                    let a = frame.pixels[(y0 * frame.width + x0) as usize * 4 + ch] as f32;
                    let b = frame.pixels[(y0 * frame.width + x1) as usize * 4 + ch] as f32;
                    let d = frame.pixels[(y1 * frame.width + x0) as usize * 4 + ch] as f32;
                    let e = frame.pixels[(y1 * frame.width + x1) as usize * 4 + ch] as f32;
                    out[oi + ch] = ((a * (1.0 - fx) + b * fx) * (1.0 - fy)
                        + (d * (1.0 - fx) + e * fx) * fy)
                        .round() as u8;
                }
            }
        }
    }
    ImageFrame::new(ow, oh, out).unwrap()
}
fn flip_horizontal(f: &mut ImageFrame) {
    for y in 0..f.height {
        for x in 0..f.width / 2 {
            let a = (y * f.width + x) as usize * 4;
            let b = (y * f.width + f.width - 1 - x) as usize * 4;
            for c in 0..4 {
                f.pixels.swap(a + c, b + c);
            }
        }
    }
}
fn flip_vertical(f: &mut ImageFrame) {
    for y in 0..f.height / 2 {
        for x in 0..f.width {
            let a = (y * f.width + x) as usize * 4;
            let b = ((f.height - 1 - y) * f.width + x) as usize * 4;
            for c in 0..4 {
                f.pixels.swap(a + c, b + c);
            }
        }
    }
}

/// Validate the structured adjustments here rather than relying on sidecar
/// deserialization/validation.  Recipes can be constructed directly by API
/// consumers, so this must run before any renderer indexes into a curve or
/// applies an HSL value.
fn validate_nested_adjustments(recipe: &EditRecipe) -> Result<(), CoreError> {
    if let Some(l) = &recipe.lens_correction {
        validate_lens(l)?;
    }
    if let Some(p) = &recipe.perspective {
        validate_perspective(p)?;
    }
    if let Some(g) = &recipe.geometry {
        if g.version != 1
            || !g.rotation_degrees.is_finite()
            || !(-180.0..=180.0).contains(&g.rotation_degrees)
        {
            return Err(CoreError::InvalidAdjustment {
                name: "geometry.version/rotation".into(),
                value: g.rotation_degrees as f64,
                minimum: -180.0,
                maximum: 180.0,
            });
        }
        if let Some(lumina_sidecar::Crop::Free {
            x,
            y,
            width,
            height,
        }) = &g.crop
        {
            if ![x, y, width, height].iter().all(|v| v.is_finite())
                || *width <= 0.0
                || *height <= 0.0
                || *x < 0.0
                || *y < 0.0
                || *x + *width > 1.0
                || *y + *height > 1.0
            {
                return Err(CoreError::InvalidAdjustment {
                    name: "geometry.crop".into(),
                    value: -1.0,
                    minimum: 0.0,
                    maximum: 1.0,
                });
            }
        }
    }
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

    if let Some(p) = &recipe.presence {
        if p.version != 1 {
            return Err(CoreError::InvalidAdjustment {
                name: "presence.version".into(),
                value: p.version as f64,
                minimum: 1.0,
                maximum: 1.0,
            });
        }
        for (name, value) in [
            ("texture", p.texture),
            ("clarity", p.clarity),
            ("dehaze", p.dehaze),
        ] {
            if !value.is_finite() || !(-1.0..=1.0).contains(&value) {
                return Err(CoreError::InvalidAdjustment {
                    name: format!("presence.{name}"),
                    value: value as f64,
                    minimum: -1.0,
                    maximum: 1.0,
                });
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
    if let Some(n) = &recipe.noise_reduction {
        if n.version != 1 {
            return Err(CoreError::InvalidAdjustment {
                name: "noise_reduction.version".into(),
                value: n.version as f64,
                minimum: 1.0,
                maximum: 1.0,
            });
        }
        for (name, value) in [("luminance", n.luminance), ("color", n.color)] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(CoreError::InvalidAdjustment {
                    name: format!("noise_reduction.{name}"),
                    value: value as f64,
                    minimum: 0.0,
                    maximum: 1.0,
                });
            }
        }
    }
    if let Some(s) = &recipe.sharpening {
        if s.version != 1 {
            return Err(CoreError::InvalidAdjustment {
                name: "sharpening.version".into(),
                value: s.version as f64,
                minimum: 1.0,
                maximum: 1.0,
            });
        }
        for (name, value, lo, hi) in [
            ("amount", s.amount, 0.0, 3.0),
            ("radius", s.radius, 0.1, 10.0),
            ("detail", s.detail, 0.0, 1.0),
            ("masking", s.masking, 0.0, 1.0),
        ] {
            if !value.is_finite() || !(lo..=hi).contains(&value) {
                return Err(CoreError::InvalidAdjustment {
                    name: format!("sharpening.{name}"),
                    value: value as f64,
                    minimum: lo as f64,
                    maximum: hi as f64,
                });
            }
        }
    }
    if let Some(e) = &recipe.effects {
        if let Some(v) = &e.vignette {
            if v.version != 1 {
                return Err(CoreError::InvalidAdjustment {
                    name: "effects.vignette.version".into(),
                    value: v.version as f64,
                    minimum: 1.0,
                    maximum: 1.0,
                });
            }
            for (name, value, lo, hi) in [
                ("effects.vignette.amount", v.amount, -1.0_f32, 1.0_f32),
                ("effects.vignette.midpoint", v.midpoint, 0.0_f32, 1.0_f32),
                ("effects.vignette.roundness", v.roundness, -1.0_f32, 1.0_f32),
                ("effects.vignette.feather", v.feather, 0.0_f32, 1.0_f32),
            ] {
                if !value.is_finite() || !(lo..=hi).contains(&value) {
                    return Err(CoreError::InvalidAdjustment {
                        name: name.into(),
                        value: value as f64,
                        minimum: lo as f64,
                        maximum: hi as f64,
                    });
                }
            }
        }
        if let Some(g) = &e.grain {
            if g.version != 1 {
                return Err(CoreError::InvalidAdjustment {
                    name: "effects.grain.version".into(),
                    value: g.version as f64,
                    minimum: 1.0,
                    maximum: 1.0,
                });
            }
            for (name, value, lo, hi) in [
                ("effects.grain.amount", g.amount, 0.0_f32, 1.0_f32),
                ("effects.grain.size", g.size, 0.0_f32, 1.0_f32),
                ("effects.grain.roughness", g.roughness, 0.0_f32, 1.0_f32),
            ] {
                if !value.is_finite() || !(lo..=hi).contains(&value) {
                    return Err(CoreError::InvalidAdjustment {
                        name: name.into(),
                        value: value as f64,
                        minimum: lo as f64,
                        maximum: hi as f64,
                    });
                }
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

/// F-094 deterministic raster heuristic. DoG is `x - box_blur(x, radius)`;
/// radius is 1..3 for Texture and 8..32 for Clarity. A box kernel is used as
/// the portable, separable Gaussian approximation (edge pixels replicate).
fn apply_presence(pixels: &mut [u8], width: u32, height: u32, p: &lumina_sidecar::Presence) {
    let texture_radius = 1 + (p.texture.abs() * 2.0).round() as usize;
    let clarity_radius = 8 + (p.clarity.abs() * 24.0).round() as usize;
    apply_dog(pixels, width, height, texture_radius, p.texture);
    apply_dog(pixels, width, height, clarity_radius, p.clarity);
    if p.dehaze == 0.0 {
        return;
    }
    // Dark channel is min(R,G,B) followed by a radius-2 local minimum. A is
    // the deterministic 95th percentile of that channel, with a floor.
    let n = width as usize * height as usize;
    let mut dark = vec![0.0f32; n];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let mut m: f32 = 1.0;
            for yy in y.saturating_sub(2)..=(y + 2).min(height as usize - 1) {
                for xx in x.saturating_sub(2)..=(x + 2).min(width as usize - 1) {
                    let i = (yy * width as usize + xx) * 4;
                    m = m.min(pixels[i].min(pixels[i + 1]).min(pixels[i + 2]) as f32 / 255.0);
                }
            }
            dark[y * width as usize + x] = m;
        }
    }
    let mut sorted = dark.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let a = sorted[((sorted.len() as f32 * 0.95) as usize).min(sorted.len().saturating_sub(1))]
        .max(0.05);
    for (index, px) in pixels.chunks_exact_mut(4).enumerate() {
        let base_t = (1.0 - 0.95 * dark[index] / a).clamp(0.05, 1.0);
        let t = if p.dehaze > 0.0 {
            1.0 - p.dehaze * (1.0 - base_t)
        } else {
            1.0 + (-p.dehaze) * 0.5 * (1.0 - base_t)
        };
        for c in &mut px[..3] {
            let x = *c as f32 / 255.0;
            *c = (((x - a) / t + a).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}

fn apply_dog(pixels: &mut [u8], width: u32, height: u32, radius: usize, amount: f32) {
    if amount == 0.0 {
        return;
    }
    let source = pixels.to_vec();
    let w = width as usize;
    let h = height as usize;
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                let mut sum = 0.0;
                for yy in y.saturating_sub(radius)..=(y + radius).min(h - 1) {
                    for xx in x.saturating_sub(radius)..=(x + radius).min(w - 1) {
                        sum += source[(yy * w + xx) * 4 + c] as f32;
                    }
                }
                let count = ((y + radius).min(h - 1) - y.saturating_sub(radius) + 1)
                    * ((x + radius).min(w - 1) - x.saturating_sub(radius) + 1);
                let i = (y * w + x) * 4 + c;
                let detail = source[i] as f32 - sum / count as f32;
                pixels[i] = (source[i] as f32 + amount * detail)
                    .clamp(0.0, 255.0)
                    .round() as u8;
            }
        }
    }
}

/// F-096: Y is filtered with a 5x5 bilateral kernel
/// `exp(-d²/(2*1.5²))*exp(-(Y-Yn)²/(2*0.12²))`; chroma offsets (R-Y,B-Y)
/// use the same 5x5 spatial window with sigma 2.0 and no similarity term.
/// Strength linearly mixes the source and filtered value. Edges replicate.
fn apply_noise_reduction(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    n: &lumina_sidecar::NoiseReduction,
) {
    if n.luminance == 0.0 && n.color == 0.0 {
        return;
    }
    let w = width as usize;
    let h = height as usize;
    let src = pixels.to_vec();
    let y_of =
        |i: usize| 0.2126 * src[i] as f32 + 0.7152 * src[i + 1] as f32 + 0.0722 * src[i + 2] as f32;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let base_y = y_of(i);
            let mut ly = 0.0;
            let mut cy_r = 0.0;
            let mut cy_b = 0.0;
            let mut sum = 0.0;
            let mut csum = 0.0;
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    let xx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                    let yy = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                    let j = (yy * w + xx) * 4;
                    let d2 = (dx * dx + dy * dy) as f32;
                    let spatial = (-d2 / (2.0 * 1.5 * 1.5)).exp();
                    let lum =
                        (-((base_y - y_of(j)).powi(2)) / (2.0 * 0.12 * 255.0 * 0.12 * 255.0)).exp();
                    let weight = spatial * lum;
                    ly += weight * y_of(j);
                    sum += weight;
                    let cw = (-d2 / (2.0 * 2.0 * 2.0)).exp();
                    csum += cw;
                    cy_r += cw * (src[j] as f32 - y_of(j));
                    cy_b += cw * (src[j + 2] as f32 - y_of(j));
                }
            }
            let filtered_y = ly / sum;
            let yv = base_y * (1.0 - n.luminance) + filtered_y * n.luminance;
            let cr = (src[i] as f32 - base_y) * (1.0 - n.color) + (cy_r / csum) * n.color;
            let cb = (src[i + 2] as f32 - base_y) * (1.0 - n.color) + (cy_b / csum) * n.color;
            let cg = src[i + 1] as f32 - base_y; // preserve green chroma by deriving it from source
            let out = [yv + cr, yv + cg, yv + cb];
            for c in 0..3 {
                pixels[i + c] = out[c].round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// F-095: separable Gaussian (three-sigma support, replicate edges) on Rec.709
/// luminance. `r_fine=0.5*r`, `r_coarse=1.5*r` (both >=.5); final detail is
/// `detail*d_fine+(1-detail)*d_coarse`. Masking uses
/// `((1-masking)+masking*clamp(|gx|+|gy| / global_max,0,1))`.
fn apply_sharpening(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    s: &lumina_sidecar::Sharpening,
    scale: f32,
) {
    if s.amount == 0.0 {
        return;
    }
    let w = width as usize;
    let h = height as usize;
    let lum: Vec<f32> = pixels
        .chunks_exact(4)
        .map(|p| 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32)
        .collect();
    let blur = |radius: f32| -> Vec<f32> {
        let sigma = (radius * scale).max(0.5);
        let r = (sigma * 3.0).ceil() as i32;
        let mut kernel = Vec::new();
        for k in -r..=r {
            kernel.push((-(k * k) as f32 / (2.0 * sigma * sigma)).exp());
        }
        let z: f32 = kernel.iter().sum();
        for v in &mut kernel {
            *v /= z;
        }
        let mut tmp = vec![0.0; w * h];
        let mut out = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                for k in -r..=r {
                    tmp[y * w + x] += kernel[(k + r) as usize]
                        * lum[y * w + (x as i32 + k).clamp(0, w as i32 - 1) as usize];
                }
            }
        }
        for y in 0..h {
            for x in 0..w {
                for k in -r..=r {
                    out[y * w + x] += kernel[(k + r) as usize]
                        * tmp[(y as i32 + k).clamp(0, h as i32 - 1) as usize * w + x];
                }
            }
        }
        out
    };
    let fine = blur((s.radius * 0.5).max(0.5));
    let coarse = blur((s.radius * 1.5).max(0.5));
    let mut gradients = vec![0.0; w * h];
    let mut maxg: f32 = 0.0;
    for y in 0..h {
        for x in 0..w {
            let gx = lum[y * w + (x as i32 + 1).min(w as i32 - 1) as usize]
                - lum[y * w + x.saturating_sub(1)];
            let gy = lum[((y as i32 + 1).min(h as i32 - 1) as usize) * w + x]
                - lum[y.saturating_sub(1) * w + x];
            gradients[y * w + x] = gx.abs() + gy.abs();
            maxg = maxg.max(gradients[y * w + x]);
        }
    }
    for (idx, p) in pixels.chunks_exact_mut(4).enumerate() {
        let d = s.detail * (lum[idx] - fine[idx]) + (1.0 - s.detail) * (lum[idx] - coarse[idx]);
        let edge = if maxg > 0.0 {
            (gradients[idx] / maxg).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let amount = s.amount * ((1.0 - s.masking) + s.masking * edge);
        let ny = (lum[idx] + amount * d).clamp(0.0, 255.0);
        let ratio = if lum[idx] > 1e-6 { ny / lum[idx] } else { 0.0 };
        for channel in p.iter_mut().take(3) {
            *channel = (*channel as f32 * ratio).round().clamp(0.0, 255.0) as u8;
        }
    }
}

fn apply_vibrance_and_saturation(
    pixels: &mut [u8],
    vibrance: Option<&f64>,
    saturation: Option<&f64>,
) {
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

/// Smooth Hermite interpolation `t*t*(3-2t)` clamped to `[0,1]` over
/// `[edge0, edge1]`. Used by the vignette transition.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// F-097: deterministic radial vignette. Applied to the RGB channels only; the
/// alpha channel (index 3) is never touched.
///
/// Invariants: `amount == 0` is an early-return identity; the centre pixel(s)
/// of the image always keep factor `1.0` (normalised radius `0`); for positive
/// `amount` the edges/corners are darkened and for negative `amount` they are
/// lightened; the factor is symmetric under reflection through the centre and
/// monotonic in the normalised radius. `midpoint` shifts where the falloff
/// begins, `roundness` controls the elliptical aspect (1 = circular) and
/// `feather` controls transition softness.
fn apply_vignette(pixels: &mut [u8], width: u32, height: u32, v: &lumina_sidecar::Vignette) {
    if v.amount == 0.0 {
        return;
    }
    let w = width as usize;
    let h = height as usize;
    let cx = (width - 1) as f32 / 2.0;
    let cy = (height - 1) as f32 / 2.0;
    let half_w = ((width - 1) as f32 / 2.0).max(1.0);
    let half_h = ((height - 1) as f32 / 2.0).max(1.0);
    // `roundness == 1` is circular; lower values stretch the falloff along y
    // (elliptical aspect).
    let ry_scale = 1.0 + (1.0 - v.roundness) * 0.5;
    // First pass: normalised radius per pixel, tracking the min/max so the
    // centre pixel(s) always map to radius `0` (factor `1.0`) regardless of
    // parity.
    let mut radii = vec![0.0f32; w * h];
    let mut r_min = f32::MAX;
    let mut r_max = 0.0f32;
    for y in 0..h {
        for x in 0..w {
            let dx = (x as f32 - cx) / half_w;
            let dy = (y as f32 - cy) / half_h * ry_scale;
            let r = (dx * dx + dy * dy).sqrt();
            let idx = y * w + x;
            radii[idx] = r;
            r_min = r_min.min(r);
            r_max = r_max.max(r);
        }
    }
    let denom = (r_max - r_min).max(1e-6);
    let feather_width = 0.15 + v.feather * 0.7;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let rn = (radii[y * w + x] - r_min) / denom;
            // `midpoint` shifts where the falloff begins (0 at the centre).
            let t = ((rn - v.midpoint) / (1.0 - v.midpoint).max(1e-6)).clamp(0.0, 1.0);
            let falloff = smoothstep(0.5 - feather_width / 2.0, 0.5 + feather_width / 2.0, t);
            let factor = 1.0 - v.amount * falloff;
            for c in 0..3 {
                pixels[i + c] = (pixels[i + c] as f32 * factor).clamp(0.0, 255.0).round() as u8;
            }
        }
    }
}

/// Deterministic, dependency-free integer hash producing a `u32`. Used to
/// derive the per-cell grain noise.
fn grain_hash(mut z: u32) -> u32 {
    z = z.wrapping_add(0x9e3779b9);
    z = (z ^ (z >> 16)).wrapping_mul(0x85ebca6b);
    z = (z ^ (z >> 13)).wrapping_mul(0xc2b2ae35);
    z ^= z >> 16;
    z
}

/// F-097: deterministic procedural grain. One noise value is generated per
/// spatial cell (size controls the cell scale) and the SAME value is added to
/// the R, G and B channels of every pixel in that cell (channel-coupled).
/// `roughness` blends between a smoothed low-frequency field and the raw
/// per-cell noise. The effective seed is folded with the image dimensions, so
/// the same `seed` on the same image reproduces identical grain while a
/// different `seed` changes it. `amount == 0` is an early-return identity; the
/// alpha channel is never touched and channels are clamped to `[0, 255]`.
fn apply_grain(pixels: &mut [u8], width: u32, height: u32, g: &lumina_sidecar::Grain) {
    if g.amount == 0.0 {
        return;
    }
    let w = width as usize;
    let h = height as usize;
    // Derive a dimension-aware seed (deterministic proxy for the RenderKey,
    // which includes the image dimensions).
    let mut seed_state = g.seed;
    seed_state = seed_state.wrapping_add((width as u64) << 32);
    seed_state = seed_state.wrapping_add(height as u64);
    seed_state ^= seed_state >> 32;
    seed_state = seed_state.wrapping_mul(0x9e3779b9);
    let seed32 = grain_hash(seed_state as u32);
    let cell = (1 + (g.size * 7.0).round() as usize).max(1);
    let noise = |cx: u32, cy: u32| -> f32 {
        let n = grain_hash(cx.wrapping_add(seed32)) ^ grain_hash(cy.wrapping_mul(0x85ebca6b));
        (grain_hash(n) as f32 / u32::MAX as f32) * 2.0 - 1.0
    };
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let cx = (x / cell) as u32;
            let cy = (y / cell) as u32;
            let raw = noise(cx, cy);
            // 3x3 neighbourhood average gives a smoothed, low-frequency field;
            // `roughness` blends between it and the raw per-cell noise.
            let mut sum = 0.0f32;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let ncx = (cx as i32 + dx).max(0) as u32;
                    let ncy = (cy as i32 + dy).max(0) as u32;
                    sum += noise(ncx, ncy);
                }
            }
            let low = sum / 9.0;
            let value = low * (1.0 - g.roughness) + raw * g.roughness;
            let delta = (value * g.amount * 40.0).round() as i32;
            for c in 0..3 {
                let v = pixels[i + c] as i32 + delta;
                pixels[i + c] = v.clamp(0, 255) as u8;
            }
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
    fn apply_recipe_with_white_balance_none_matches_apply_recipe() {
        let pixel_data = vec![80, 100, 120, 9, 200, 60, 30, 200, 10, 250, 128, 77];
        for r in [
            recipe(&[("exposure", 0.5), ("contrast", 0.25)]),
            recipe(&[
                ("wb_temperature", 3000.0),
                ("wb_tint", -0.5),
                ("exposure", 0.5),
            ]),
        ] {
            let mut with_context = ImageFrame::new(3, 1, pixel_data.clone()).unwrap();
            let mut without = ImageFrame::new(3, 1, pixel_data.clone()).unwrap();
            with_context
                .apply_recipe_with_white_balance(&r, None)
                .unwrap();
            without.apply_recipe(&r).unwrap();
            assert_eq!(with_context, without);
        }
    }

    #[test]
    fn as_shot_context_is_identity_without_wb_keys() {
        let pixel_data = vec![80, 100, 120, 9, 200, 60, 30, 200, 10, 250, 128, 77];
        let r = recipe(&[("exposure", 0.5), ("contrast", 0.25)]);
        for gains in [[1.0, 1.0, 1.0, 1.0], [2.0, 1.0, 0.5, 1.0]] {
            let mut with_context = ImageFrame::new(3, 1, pixel_data.clone()).unwrap();
            with_context
                .apply_recipe_with_white_balance(&r, Some(gains))
                .unwrap();
            let mut without = ImageFrame::new(3, 1, pixel_data.clone()).unwrap();
            without.apply_recipe(&r).unwrap();
            assert_eq!(
                with_context, without,
                "As-Shot gains must not be re-applied (decoder already applied them)"
            );
        }
    }

    #[test]
    fn manual_wb_anchors_unchanged_with_as_shot_context() {
        let pixel_data = vec![80, 100, 120, 9, 200, 60, 30, 200, 10, 250, 128, 77];
        let r = recipe(&[
            ("wb_temperature", 3000.0),
            ("wb_tint", -0.5),
            ("exposure", 0.5),
        ]);
        let mut with_context = ImageFrame::new(3, 1, pixel_data.clone()).unwrap();
        with_context
            .apply_recipe_with_white_balance(&r, Some([2.0, 1.0, 0.5, 1.0]))
            .unwrap();
        let mut without = ImageFrame::new(3, 1, pixel_data.clone()).unwrap();
        without.apply_recipe(&r).unwrap();
        assert_eq!(
            with_context, without,
            "manual wb keys keep the deterministic sRGB approximation"
        );
    }

    #[test]
    fn invalid_camera_white_balance_rejected_without_mutation() {
        let original = ImageFrame::new(2, 1, vec![80, 100, 120, 9, 200, 60, 30, 200]).unwrap();
        let r = recipe(&[("wb_temperature", 3000.0)]);
        for gains in [
            [0.0, 1.0, 1.0, 1.0],
            [-1.0, 1.0, 1.0, 1.0],
            [f32::NAN, 1.0, 1.0, 1.0],
            [f32::INFINITY, 1.0, 1.0, 1.0],
        ] {
            let mut frame = original.clone();
            let result = frame.apply_recipe_with_white_balance(&r, Some(gains));
            assert!(
                matches!(
                    result,
                    Err(CoreError::InvalidAdjustment { name, .. }) if name == "camera_white_balance"
                ),
                "expected InvalidAdjustment for gains {gains:?}"
            );
            assert_eq!(
                frame, original,
                "frame must stay byte-identical when gains {gains:?} are invalid"
            );
        }
    }

    #[test]
    fn wb_application_preserves_alpha_with_context() {
        let mut frame = ImageFrame::new(1, 1, vec![80, 100, 120, 77]).unwrap();
        frame
            .apply_recipe_with_white_balance(
                &recipe(&[("wb_temperature", 3000.0)]),
                Some([2.0, 1.0, 0.5, 1.0]),
            )
            .unwrap();
        assert_eq!(frame.pixels[3], 77);
        assert_ne!(frame.pixels[0], 80);
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

    #[test]
    fn presence_is_deterministic_clipped_and_preserves_alpha() {
        let original = ImageFrame::new(
            3,
            1,
            vec![20, 30, 40, 7, 128, 96, 64, 19, 240, 220, 200, 31],
        )
        .unwrap();
        let recipe = EditRecipe {
            presence: Some(lumina_sidecar::Presence {
                version: 1,
                texture: 1.0,
                clarity: 1.0,
                dehaze: 1.0,
            }),
            ..Default::default()
        };
        let mut first = original.clone();
        let mut second = original.clone();
        first.apply_recipe(&recipe).unwrap();
        second.apply_recipe(&recipe).unwrap();
        assert_eq!(first, second);
        assert_ne!(&first.pixels[..9], &original.pixels[..9]);
        assert_eq!(
            [first.pixels[3], first.pixels[7], first.pixels[11]],
            [7, 19, 31]
        );
        assert!(first.pixels[..]
            .chunks_exact(4)
            .flat_map(|pixel| &pixel[..3])
            .any(|channel| *channel == 0 || *channel == 255));
    }

    #[test]
    fn presence_validation_rejects_invalid_nested_values() {
        for presence in [
            lumina_sidecar::Presence {
                version: 2,
                texture: 0.0,
                clarity: 0.0,
                dehaze: 0.0,
            },
            lumina_sidecar::Presence {
                version: 1,
                texture: f32::NAN,
                clarity: 0.0,
                dehaze: 0.0,
            },
            lumina_sidecar::Presence {
                version: 1,
                texture: 0.0,
                clarity: 1.01,
                dehaze: 0.0,
            },
            lumina_sidecar::Presence {
                version: 1,
                texture: 0.0,
                clarity: 0.0,
                dehaze: -1.01,
            },
        ] {
            let mut frame = ImageFrame::new(1, 1, vec![10, 20, 30, 255]).unwrap();
            assert!(matches!(
                frame.apply_recipe(&EditRecipe {
                    presence: Some(presence),
                    ..Default::default()
                }),
                Err(CoreError::InvalidAdjustment { .. })
            ));
        }
    }

    #[test]
    fn geometry_crop_rotation_and_mirror_are_applied_in_order() {
        let mut frame = ImageFrame::new(
            3,
            2,
            vec![
                1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255, 5, 0, 0, 255, 6, 0, 0, 255,
            ],
        )
        .unwrap();
        frame
            .apply_geometry(
                Some(&lumina_sidecar::Geometry {
                    version: 1,
                    crop: Some(lumina_sidecar::Crop::Free {
                        x: 1.0 / 3.0,
                        y: 0.0,
                        width: 2.0 / 3.0,
                        height: 1.0,
                    }),
                    rotation_degrees: 0.0,
                    mirror_horizontal: true,
                    mirror_vertical: false,
                }),
                None,
                None,
                #[cfg(feature = "lensfun")]
                None,
            )
            .unwrap();
        assert_eq!((frame.width, frame.height), (2, 2));
        assert_eq!(
            frame.pixels,
            vec![3, 0, 0, 255, 2, 0, 0, 255, 6, 0, 0, 255, 5, 0, 0, 255]
        );
    }

    #[test]
    fn geometry_measurement_domain_tracks_crop_and_quarter_turn() {
        let frame = ImageFrame::new(4, 2, vec![0; 32]).unwrap();
        let domain = frame
            .measurement_domain(Some(&lumina_sidecar::Geometry {
                version: 1,
                crop: Some(lumina_sidecar::Crop::Free {
                    x: 0.25,
                    y: 0.0,
                    width: 0.5,
                    height: 1.0,
                }),
                rotation_degrees: 90.0,
                mirror_horizontal: false,
                mirror_vertical: false,
            }))
            .unwrap();
        assert_eq!((domain.output_width, domain.output_height), (2, 2));
        assert_eq!((domain.source_x, domain.source_y), (0.25, 0.0));
        assert_eq!((domain.source_width, domain.source_height), (0.5, 1.0));
    }

    fn test_perspective() -> lumina_sidecar::Perspective {
        lumina_sidecar::Perspective {
            version: 1,
            vertical: 0.0,
            horizontal: 0.0,
            rotation: 0.0,
            scale: 1.0,
            aspect_ratio: 1.0,
            shift_x: 0.0,
            shift_y: 0.0,
        }
    }

    #[test]
    fn perspective_identity_preserves_bytes_and_dimensions() {
        let pixels = vec![
            10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255,
        ];
        let mut rendered = ImageFrame::new(2, 2, pixels.clone()).unwrap();
        let p = test_perspective();
        rendered
            .apply_geometry(
                None,
                None,
                Some(&p),
                #[cfg(feature = "lensfun")]
                None,
            )
            .unwrap();
        assert_eq!((rendered.width, rendered.height), (2, 2));
        assert_eq!(rendered.pixels, pixels);
        let domain = rendered
            .measurement_domain_with_perspective(None, None, Some(&p))
            .unwrap();
        assert_eq!((domain.output_width, domain.output_height), (2, 2));
    }

    #[test]
    fn perspective_scale_two_doubles_bounding_box_and_projects_corners() {
        let pixels = vec![10, 0, 0, 255, 20, 0, 0, 255, 30, 0, 0, 255, 40, 0, 0, 255];
        let mut rendered = ImageFrame::new(2, 2, pixels).unwrap();
        let mut p = test_perspective();
        p.scale = 2.0;
        rendered
            .apply_geometry(
                None,
                None,
                Some(&p),
                #[cfg(feature = "lensfun")]
                None,
            )
            .unwrap();
        assert_eq!((rendered.width, rendered.height), (4, 4));
        assert_eq!(rendered.pixels[0], 10);
        assert_eq!(rendered.pixels[3 * 4], 20);
        assert_eq!(rendered.pixels[(3 * 4) * rendered.width as usize], 30);
        assert_eq!(rendered.pixels[((4 * rendered.width - 1) * 4) as usize], 40);
        let domain = ImageFrame::new(2, 2, vec![0; 16])
            .unwrap()
            .measurement_domain_with_perspective(None, None, Some(&p))
            .unwrap();
        assert_eq!((domain.output_width, domain.output_height), (4, 4));
    }

    #[test]
    fn perspective_shift_rotation_and_direction_change_rendered_geometry() {
        let base =
            ImageFrame::new(4, 2, (0..8).flat_map(|v| [v * 20, 100, 50, 255]).collect()).unwrap();
        let mut shifted = base.clone();
        let mut p = test_perspective();
        p.shift_x = 0.5;
        p.shift_y = -0.25;
        shifted
            .apply_geometry(
                None,
                None,
                Some(&p),
                #[cfg(feature = "lensfun")]
                None,
            )
            .unwrap();
        assert_eq!((shifted.width, shifted.height), (4, 2));
        assert_ne!(shifted.pixels, base.pixels);

        let mut rotated = base.clone();
        p = test_perspective();
        p.rotation = 1.0;
        rotated
            .apply_geometry(
                None,
                None,
                Some(&p),
                #[cfg(feature = "lensfun")]
                None,
            )
            .unwrap();
        assert!(rotated.width > base.width && rotated.height > base.height);

        for (vertical, horizontal) in [(0.5, 0.0), (0.0, 0.5), (-0.5, 0.0), (0.0, -0.5)] {
            let mut directional = base.clone();
            p = test_perspective();
            p.vertical = vertical;
            p.horizontal = horizontal;
            directional
                .apply_geometry(
                    None,
                    None,
                    Some(&p),
                    #[cfg(feature = "lensfun")]
                    None,
                )
                .unwrap();
            assert!(directional.width >= base.width && directional.height >= base.height);
        }
    }

    #[test]
    fn perspective_combines_lens_then_perspective_then_crop_and_measurement() {
        let mut frame =
            ImageFrame::new(8, 4, (0..32).flat_map(|v| [v, 100, 50, 255]).collect()).unwrap();
        let lens = lumina_sidecar::LensCorrection {
            version: 1,
            profile: None,
            distortion_k1: Some(0.2),
            distortion_k2: None,
            distortion_k3: None,
            vignette_c0: Some(1.0),
            vignette_c1: None,
            vignette_c2: None,
            ca_red: None,
            ca_blue: None,
        };
        let mut p = test_perspective();
        p.scale = 2.0;
        p.aspect_ratio = 0.5;
        let geometry = lumina_sidecar::Geometry {
            version: 1,
            crop: Some(lumina_sidecar::Crop::Free {
                x: 0.25,
                y: 0.25,
                width: 0.5,
                height: 0.5,
            }),
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        };
        let expected = frame
            .measurement_domain_with_perspective(Some(&geometry), Some(&lens), Some(&p))
            .unwrap();
        frame
            .apply_geometry(
                Some(&geometry),
                Some(&lens),
                Some(&p),
                #[cfg(feature = "lensfun")]
                None,
            )
            .unwrap();
        assert_eq!(
            (frame.width, frame.height),
            (expected.output_width, expected.output_height)
        );
        assert!(frame.pixels.iter().any(|&v| v != 0));
    }

    #[test]
    fn lens_explicit_zero_overrides_profile_and_invalid_geometry_is_rejected() {
        let source =
            ImageFrame::new(5, 5, (0..25).flat_map(|v| [v * 7, 20, 30, 255]).collect()).unwrap();
        let profile = lumina_sidecar::LensCorrection {
            version: 1,
            profile: Some("wide-light".into()),
            distortion_k1: None,
            distortion_k2: None,
            distortion_k3: None,
            vignette_c0: None,
            vignette_c1: None,
            vignette_c2: None,
            ca_red: None,
            ca_blue: None,
        };
        let mut explicit = profile.clone();
        explicit.distortion_k1 = Some(0.0);
        let mut a = source.clone();
        let mut b = source.clone();
        a.apply_geometry(
            None,
            Some(&profile),
            None,
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
        b.apply_geometry(
            None,
            Some(&explicit),
            None,
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
        assert_ne!(a.pixels, b.pixels);

        for bad in [
            lumina_sidecar::Perspective {
                version: 2,
                ..test_perspective()
            },
            lumina_sidecar::Perspective {
                scale: f32::NAN,
                ..test_perspective()
            },
        ] {
            assert!(source
                .clone()
                .apply_geometry(
                    None,
                    None,
                    Some(&bad),
                    #[cfg(feature = "lensfun")]
                    None
                )
                .is_err());
        }
        let bad_lens = lumina_sidecar::LensCorrection {
            distortion_k1: Some(1.1),
            ..profile
        };
        assert!(source
            .clone()
            .apply_geometry(
                None,
                Some(&bad_lens),
                None,
                #[cfg(feature = "lensfun")]
                None
            )
            .is_err());
        let bad_ca = lumina_sidecar::LensCorrection {
            ca_red: Some(f32::NAN),
            ..bad_lens
        };
        assert!(source
            .clone()
            .apply_geometry(
                None,
                Some(&bad_ca),
                None,
                #[cfg(feature = "lensfun")]
                None
            )
            .is_err());
    }

    #[test]
    fn all_aspect_crop_presets_have_expected_dimensions_and_centering() {
        let presets = [
            (lumina_sidecar::AspectPreset::Original, 200, 100, 0),
            (lumina_sidecar::AspectPreset::OneToOne, 100, 100, 50),
            (lumina_sidecar::AspectPreset::FourToFive, 80, 100, 60),
            (lumina_sidecar::AspectPreset::FiveToFour, 125, 100, 38),
            (lumina_sidecar::AspectPreset::ThreeToTwo, 150, 100, 25),
            (lumina_sidecar::AspectPreset::TwoToThree, 67, 100, 67),
            (lumina_sidecar::AspectPreset::FourToThree, 133, 100, 33),
            (lumina_sidecar::AspectPreset::ThreeToFour, 75, 100, 63),
            (lumina_sidecar::AspectPreset::SixteenToNine, 178, 100, 11),
            (lumina_sidecar::AspectPreset::NineToSixteen, 56, 100, 72),
        ];
        for (preset, expected_width, expected_height, expected_x) in presets {
            let frame = ImageFrame::new(200, 100, vec![0; 200 * 100 * 4]).unwrap();
            let domain = frame
                .measurement_domain(Some(&lumina_sidecar::Geometry {
                    version: 1,
                    crop: Some(lumina_sidecar::Crop::Aspect { preset }),
                    rotation_degrees: 0.0,
                    mirror_horizontal: false,
                    mirror_vertical: false,
                }))
                .unwrap();
            assert_eq!(
                (domain.output_width, domain.output_height),
                (expected_width, expected_height)
            );
            assert_eq!((domain.source_x * 200.0).round() as u32, expected_x);
            assert_eq!(domain.source_y, 0.0);
        }
    }

    #[test]
    fn free_crop_50_by_25_percent_is_exactly_centered() {
        let frame = ImageFrame::new(200, 100, vec![0; 200 * 100 * 4]).unwrap();
        let domain = frame
            .measurement_domain(Some(&lumina_sidecar::Geometry {
                version: 1,
                crop: Some(lumina_sidecar::Crop::Free {
                    x: 0.25,
                    y: 0.25,
                    width: 0.5,
                    height: 0.5,
                }),
                rotation_degrees: 0.0,
                mirror_horizontal: false,
                mirror_vertical: false,
            }))
            .unwrap();
        assert_eq!((domain.output_width, domain.output_height), (100, 50));
        assert_eq!(
            (
                domain.source_x,
                domain.source_y,
                domain.source_width,
                domain.source_height
            ),
            (0.25, 0.25, 0.5, 0.5)
        );
    }

    #[test]
    fn rotation_90_and_180_have_exact_non_square_dimensions_and_pattern() {
        let geometry = |degrees| lumina_sidecar::Geometry {
            version: 1,
            crop: None,
            rotation_degrees: degrees,
            mirror_horizontal: false,
            mirror_vertical: false,
        };
        let mut quarter =
            ImageFrame::new(2, 2, (1..=4).flat_map(|v| [v, 0, 0, 255]).collect()).unwrap();
        quarter
            .apply_geometry(
                Some(&geometry(90.0)),
                None,
                None,
                #[cfg(feature = "lensfun")]
                None,
            )
            .unwrap();
        assert_eq!((quarter.width, quarter.height), (2, 2));
        assert_eq!(
            quarter
                .pixels
                .iter()
                .step_by(4)
                .copied()
                .collect::<Vec<_>>(),
            vec![3, 1, 4, 2]
        );
        let mut half =
            ImageFrame::new(2, 2, (1..=4).flat_map(|v| [v, 0, 0, 255]).collect()).unwrap();
        half.apply_geometry(
            Some(&geometry(180.0)),
            None,
            None,
            #[cfg(feature = "lensfun")]
            None,
        )
        .unwrap();
        assert_eq!((half.width, half.height), (2, 2));
        assert_eq!(
            half.pixels.iter().step_by(4).copied().collect::<Vec<_>>(),
            vec![4, 3, 2, 1]
        );
    }

    #[test]
    fn non_quarter_rotation_keeps_black_pixels_outside_non_square_source() {
        let mut frame = ImageFrame::new(3, 2, vec![255; 3 * 2 * 4]).unwrap();
        frame
            .apply_geometry(
                Some(&lumina_sidecar::Geometry {
                    version: 1,
                    crop: None,
                    rotation_degrees: 45.0,
                    mirror_horizontal: false,
                    mirror_vertical: false,
                }),
                None,
                None,
                #[cfg(feature = "lensfun")]
                None,
            )
            .unwrap();
        assert_eq!((frame.width, frame.height), (4, 4));
        for index in [0, 3, 12, 15] {
            assert_eq!(&frame.pixels[index * 4..index * 4 + 4], &[0, 0, 0, 0]);
        }
    }

    #[test]
    fn horizontal_and_vertical_mirrors_are_exact() {
        let geometry = |horizontal, vertical| lumina_sidecar::Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: horizontal,
            mirror_vertical: vertical,
        };
        let source =
            || ImageFrame::new(2, 2, (1..=4).flat_map(|v| [v, 0, 0, 255]).collect()).unwrap();
        let mut horizontal = source();
        horizontal
            .apply_geometry(
                Some(&geometry(true, false)),
                None,
                None,
                #[cfg(feature = "lensfun")]
                None,
            )
            .unwrap();
        assert_eq!(
            horizontal
                .pixels
                .iter()
                .step_by(4)
                .copied()
                .collect::<Vec<_>>(),
            vec![2, 1, 4, 3]
        );
        let mut vertical = source();
        vertical
            .apply_geometry(
                Some(&geometry(false, true)),
                None,
                None,
                #[cfg(feature = "lensfun")]
                None,
            )
            .unwrap();
        assert_eq!(
            vertical
                .pixels
                .iter()
                .step_by(4)
                .copied()
                .collect::<Vec<_>>(),
            vec![3, 4, 1, 2]
        );
    }

    #[test]
    fn geometry_none_is_identity_and_core_validates_version_rotation_and_free_crop() {
        let frame = ImageFrame::new(4, 2, vec![7; 32]).unwrap();
        let mut unchanged = frame.clone();
        unchanged
            .apply_geometry(
                Some(&lumina_sidecar::Geometry {
                    version: 1,
                    crop: None,
                    rotation_degrees: 0.0,
                    mirror_horizontal: false,
                    mirror_vertical: false,
                }),
                None,
                None,
                #[cfg(feature = "lensfun")]
                None,
            )
            .unwrap();
        assert_eq!(unchanged, frame);
        let identity = frame.measurement_domain(None).unwrap();
        assert_eq!(
            (
                identity.output_width,
                identity.output_height,
                identity.source_x,
                identity.source_y,
                identity.source_width,
                identity.source_height
            ),
            (4, 2, 0.0, 0.0, 1.0, 1.0)
        );
        for geometry in [
            lumina_sidecar::Geometry {
                version: 2,
                crop: None,
                rotation_degrees: 0.0,
                mirror_horizontal: false,
                mirror_vertical: false,
            },
            lumina_sidecar::Geometry {
                version: 1,
                crop: None,
                rotation_degrees: 181.0,
                mirror_horizontal: false,
                mirror_vertical: false,
            },
            lumina_sidecar::Geometry {
                version: 1,
                crop: Some(lumina_sidecar::Crop::Free {
                    x: 0.8,
                    y: 0.0,
                    width: 0.3,
                    height: 0.5,
                }),
                rotation_degrees: 0.0,
                mirror_horizontal: false,
                mirror_vertical: false,
            },
        ] {
            let mut candidate = frame.clone();
            assert!(candidate
                .apply_recipe(&EditRecipe {
                    geometry: Some(geometry),
                    ..Default::default()
                })
                .is_err());
        }
    }

    #[test]
    fn presence_is_identity_for_zero_and_texture_is_edge_directional() {
        let original =
            ImageFrame::new(3, 1, vec![20, 20, 20, 9, 200, 200, 200, 9, 20, 20, 20, 9]).unwrap();
        let mut identity = original.clone();
        identity
            .apply_recipe(&EditRecipe {
                presence: Some(lumina_sidecar::Presence {
                    version: 1,
                    texture: 0.0,
                    clarity: 0.0,
                    dehaze: 0.0,
                }),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(identity, original);
        let mut texture = original.clone();
        texture
            .apply_recipe(&EditRecipe {
                presence: Some(lumina_sidecar::Presence {
                    version: 1,
                    texture: 1.0,
                    clarity: 0.0,
                    dehaze: 0.0,
                }),
                ..Default::default()
            })
            .unwrap();
        assert!(texture.pixels[0] < original.pixels[0] && texture.pixels[4] > original.pixels[4]);
    }

    #[test]
    fn clarity_has_broader_edge_influence_than_texture() {
        let pixels: Vec<u8> = (0..40)
            .flat_map(|x| {
                let v = if x < 20 { 20 } else { 200 };
                [v, v, v, 255]
            })
            .collect();
        let mut texture = ImageFrame::new(40, 1, pixels.clone()).unwrap();
        let mut clarity = ImageFrame::new(40, 1, pixels).unwrap();
        let p = |texture, clarity| EditRecipe {
            presence: Some(lumina_sidecar::Presence {
                version: 1,
                texture,
                clarity,
                dehaze: 0.0,
            }),
            ..Default::default()
        };
        texture.apply_recipe(&p(1.0, 0.0)).unwrap();
        clarity.apply_recipe(&p(0.0, 1.0)).unwrap();
        let texture_changed = texture
            .pixels
            .chunks_exact(4)
            .filter(|px| px[0] != 20 && px[0] != 200)
            .count();
        let clarity_changed = clarity
            .pixels
            .chunks_exact(4)
            .filter(|px| px[0] != 20 && px[0] != 200)
            .count();
        assert!(clarity_changed > texture_changed);
    }

    #[test]
    fn dehaze_positive_direction_and_negative_direction_are_bounded() {
        let source = ImageFrame::new(
            3,
            1,
            vec![40, 40, 40, 255, 120, 120, 120, 255, 220, 220, 220, 255],
        )
        .unwrap();
        let make = |amount| EditRecipe {
            presence: Some(lumina_sidecar::Presence {
                version: 1,
                texture: 0.0,
                clarity: 0.0,
                dehaze: amount,
            }),
            ..Default::default()
        };
        let mut positive = source.clone();
        positive.apply_recipe(&make(1.0)).unwrap();
        let mut negative = source.clone();
        negative.apply_recipe(&make(-1.0)).unwrap();
        assert!(positive.pixels[8] > source.pixels[8]);
        assert!(negative.pixels[4] < source.pixels[4] && negative.pixels[8] < source.pixels[8]);
        assert!(
            negative
                .pixels
                .iter()
                .zip(source.pixels.iter())
                .map(|(a, b)| (*a as i16 - *b as i16).abs())
                .sum::<i16>()
                < 300
        );
    }

    #[test]
    fn presence_is_applied_before_curves() {
        let curve = lumina_sidecar::Curves {
            version: 1,
            master: vec![
                lumina_sidecar::CurvePoint {
                    input: 0.0,
                    output: 0.0,
                },
                lumina_sidecar::CurvePoint {
                    input: 0.5,
                    output: 1.0,
                },
                lumina_sidecar::CurvePoint {
                    input: 1.0,
                    output: 1.0,
                },
            ],
            channels: Default::default(),
        };
        let presence = lumina_sidecar::Presence {
            version: 1,
            texture: 1.0,
            clarity: 0.0,
            dehaze: 0.0,
        };
        let input = vec![
            60, 60, 60, 255, 80, 80, 80, 255, 100, 100, 100, 255, 120, 120, 120, 255, 140, 140,
            140, 255,
        ];
        let mut combined = ImageFrame::new(5, 1, input.clone()).unwrap();
        combined
            .apply_recipe(&EditRecipe {
                presence: Some(presence),
                curves: Some(curve.clone()),
                ..Default::default()
            })
            .unwrap();
        let mut reverse = ImageFrame::new(5, 1, input).unwrap();
        reverse
            .apply_recipe(&EditRecipe {
                curves: Some(curve),
                ..Default::default()
            })
            .unwrap();
        reverse
            .apply_recipe(&EditRecipe {
                presence: Some(presence),
                ..Default::default()
            })
            .unwrap();
        assert_ne!(combined.pixels, reverse.pixels);
    }

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
            20, 20, 20, 255, 20, 20, 20, 255, 25, 25, 25, 255, 235, 235, 235, 255, 240, 240, 240,
            255,
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
            20, 20, 20, 255, 25, 25, 25, 255, 20, 20, 20, 255, 235, 235, 235, 255, 240, 240, 240,
            255,
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
        for px in f.pixels.chunks_exact(4) {
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

    /// Independent reimplementation of the original pass-by-pass channel
    /// adjustments (pre-F-074-A1). Used only by the byte-identity property test
    /// to prove the fused LUT kernel (`apply_channel_lut_adjustments`) produces
    /// byte-identical output. Mirrors the original per-stage loops exactly
    /// (same `f64` formulas, same per-channel application order).
    fn reference_channel_lut_adjustments(pixels: &mut [u8], params: &ChannelLutParams) {
        let ChannelLutParams {
            wb_gains,
            exposure_multiplier,
            contrast_factor,
            shadows,
            highlights,
            whites,
            blacks,
        } = params;
        if let Some(gains) = wb_gains {
            for pixel in pixels.chunks_exact_mut(4) {
                for (channel, gain) in pixel[..3].iter_mut().zip(*gains) {
                    *channel = ((*channel as f64 * gain).round()).clamp(0.0, 255.0) as u8;
                }
            }
        }
        if let Some(multiplier) = exposure_multiplier {
            for channel in pixels.chunks_exact_mut(4).flat_map(|pixel| &mut pixel[..3]) {
                *channel = ((*channel as f64 * multiplier).round()).clamp(0.0, 255.0) as u8;
            }
        }
        if let Some(factor) = contrast_factor {
            for channel in pixels.chunks_exact_mut(4).flat_map(|pixel| &mut pixel[..3]) {
                *channel =
                    (((*channel as f64 - 128.0) * factor + 128.0).round()).clamp(0.0, 255.0) as u8;
            }
        }
        if let Some(shadows) = shadows {
            for channel in pixels.chunks_exact_mut(4).flat_map(|pixel| &mut pixel[..3]) {
                let x = *channel as f64 / 255.0;
                let weight = ((0.5 - x) / 0.5).max(0.0).powi(2);
                *channel = ((x + shadows * weight * 0.25).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
        if let Some(highlights) = highlights {
            for channel in pixels.chunks_exact_mut(4).flat_map(|pixel| &mut pixel[..3]) {
                let x = *channel as f64 / 255.0;
                let weight = ((x - 0.5) / 0.5).max(0.0).powi(2);
                *channel = ((x + highlights * weight * 0.25).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
        if let Some(whites) = whites {
            for channel in pixels.chunks_exact_mut(4).flat_map(|pixel| &mut pixel[..3]) {
                let x = *channel as f64 / 255.0;
                let weight = ((x - 0.5) / 0.5).max(0.0);
                *channel = ((x + whites * weight * 0.25).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
        if let Some(blacks) = blacks {
            for channel in pixels.chunks_exact_mut(4).flat_map(|pixel| &mut pixel[..3]) {
                let x = *channel as f64 / 255.0;
                let weight = ((0.5 - x) / 0.5).max(0.0);
                *channel = ((x - blacks * weight * 0.25).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }

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
                                    for pixel in optimized.chunks_exact_mut(4) {
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
}
