//! The inherent `ImageFrame` API: construction, accessors, decode/encode and the
//! geometry, crop and recipe stages that operate on a frame.
//!
//! Pure structural split out of `lib.rs` (FILE-SIZE-1000-ALL-EXCEPTIONS-100):
//! every method body is byte-identical to its former root location.

use image::{
    codecs::jpeg::JpegEncoder, codecs::png::PngEncoder, codecs::webp::WebPEncoder, ColorType,
    ImageEncoder,
};
use lumina_sidecar::EditRecipe;
use std::io::Cursor;

use crate::effects::{
    apply_grain, apply_noise_reduction, apply_red_eye, apply_sharpening, apply_vignette,
    dither_rgba8,
};
use crate::geometry_stage_api::{validate_lens, validate_perspective};
#[cfg(feature = "lensfun")]
use crate::lens_geometry::EMPTY_LENS;
use crate::lens_geometry::{
    apply_ca, apply_channel_lut_adjustments, apply_lens, apply_perspective, crop_frame, crop_rect,
    default_content_crop, flip_horizontal, flip_vertical, perspective_dimensions,
    rotate_dimensions, rotate_frame,
};
use crate::{
    color_stages, for_each_rgba_mut, monotone_curve, presence_stages, validate_nested_adjustments,
    ChannelLutParams, CoreError, ExportOptions, ImageFileFormat, ImageFrame, MeasurementDomain,
    MemoryBudget,
};

impl ImageFrame {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, CoreError> {
        // Checked so a pathological dimension pair can neither overflow the
        // multiplication nor wrap into a false match (REVIEW-CORE-DECODE-1,
        // defense in depth alongside the decode-budget guard).
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|value| value.checked_mul(4));
        if expected != Some(pixels.len()) {
            return Err(CoreError::InvalidFrame {
                width,
                height,
                length: pixels.len(),
            });
        }
        Ok(Self::with_new_identity(width, height, pixels))
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Row-major RGBA8 bytes, four bytes per pixel.
    ///
    /// The length is always `width * height * 4`; [`ImageFrame::new`]
    /// enforces it and the mutable view returned by
    /// [`ImageFrame::pixels_mut`] cannot change it.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Mutable view onto the row-major RGBA8 bytes, four bytes per pixel.
    ///
    /// The length stays `width * height * 4` because a `&mut [u8]` view
    /// cannot change the allocation's length — this is what protects the
    /// invariant from outside the crate.
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    /// Consumes the frame and returns the row-major RGBA8 bytes, four bytes
    /// per pixel (length `width * height * 4`).
    #[must_use]
    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }

    /// Decodes an encoded raster image (PNG/JPEG/WebP) into an RGBA8 frame.
    ///
    /// REVIEW-CORE-DECODE-1: the image geometry is read from the container
    /// header **before** any pixel buffer is allocated and checked against the
    /// configured [`MemoryBudget`] (`check_decode` with RGBA8 geometry, i.e.
    /// four bytes per pixel). An oversized (or corrupt) input therefore fails
    /// fast with [`CoreError::Decode`] instead of triggering an unbounded
    /// allocation inside the decoder. The budget honours the `LUMINA_MAX_*`
    /// environment overrides like every other F-075 check point.
    pub fn decode(bytes: &[u8]) -> Result<Self, CoreError> {
        let (header_width, header_height) = image::ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|error| CoreError::Decode(error.to_string()))?
            .into_dimensions()
            .map_err(|error| CoreError::Decode(error.to_string()))?;
        MemoryBudget::from_env()
            .check_decode(u64::from(header_width), u64::from(header_height), 4, 1)
            .map_err(|error| {
                CoreError::Decode(format!(
                    "image {header_width}x{header_height} exceeds the decode memory budget: {error}"
                ))
            })?;
        let image =
            image::load_from_memory(bytes).map_err(|error| CoreError::Decode(error.to_string()))?;
        let rgba = image.to_rgba8();
        Self::new(rgba.width(), rgba.height(), rgba.into_raw())
    }

    /// PERF-GUI-5: crop a rectangular region `(x, y, w, h)` (in source pixels,
    /// clamped to the frame bounds) into a new frame. Used by the GUI to render
    /// only the visible viewport bounding box when zoomed in (ROI). The cropped
    /// frame keeps `image`-decoded byte order; the alpha channel is copied.
    pub fn crop_region(&self, x: u32, y: u32, w: u32, h: u32) -> Result<ImageFrame, CoreError> {
        let x = x.min(self.width.saturating_sub(1));
        let y = y.min(self.height.saturating_sub(1));
        let w = w.min(self.width - x);
        let h = h.min(self.height - y);
        if w == 0 || h == 0 {
            return Err(CoreError::InvalidFrame {
                width: w,
                height: h,
                length: 0,
            });
        }
        let mut pixels = vec![0u8; w as usize * h as usize * 4];
        for dy in 0..h {
            let src = ((y + dy) * self.width + x) as usize * 4;
            let dst = (dy * w) as usize * 4;
            pixels[dst..dst + w as usize * 4]
                .copy_from_slice(&self.pixels[src..src + w as usize * 4]);
        }
        ImageFrame::new(w, h, pixels)
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
            *self = apply_perspective(self, p)?;
        }
        // CA stays manual (recipe lens only), applied after perspective like the
        // original order (distortion → perspective → CA → crop) — unless a
        // non-identity TCA-capable Lensfun corrector already corrected CA in
        // the lens stage (G-06, no double correction).
        if let Some(l) = lens {
            #[cfg(feature = "lensfun")]
            let tca_active = lensfun.is_some_and(|c| !c.is_identity() && c.has_tca());
            #[cfg(not(feature = "lensfun"))]
            let tca_active = false;
            if !tca_active {
                apply_ca(self, l);
            }
        }
        // CROP-MAXRECT-1: a lens/perspective correction can introduce
        // transparent wedges; the default crop then excludes them. Without an
        // active correction the crop stage stays the identity full frame.
        #[cfg(feature = "lensfun")]
        let corrected = lens.is_some() || perspective.is_some() || lensfun.is_some();
        #[cfg(not(feature = "lensfun"))]
        let corrected = lens.is_some() || perspective.is_some();
        self.apply_crop_stage(geometry, corrected)
    }

    /// GEN-PIPELINE-DECOUPLE: apply crop stage only (crop → rotation →
    /// mirroring). Coordinates for the crop are normalized on the
    /// perspective-transformed (and possibly generatively expanded) image.
    /// This is the fifth geometry sub-stage (`Crop`); together with
    /// [`Self::apply_lens_stage`], [`Self::apply_auto_fill_transparent`],
    /// [`Self::apply_perspective_stage`] and
    /// [`crate::generative::composite_expand`] it forms the decoupled
    /// order `Lens → Fill → Perspective → Expand → Crop`. [`Self::apply_geometry`]
    /// and [`Self::apply_geometry_with_auto_fill`] delegate to these stages so
    /// the legacy 5-in-1 entry points stay byte-identical.
    ///
    /// CROP-MAXRECT-1: `use_content_default` selects the default when the
    /// recipe carries no explicit crop. `false` keeps the historical identity
    /// full frame; `true` uses the largest all-content rectangle
    /// ([`crate::crop_max_rect::maximum_content_rect`]) so lens/perspective
    /// transparent wedges are excluded without an explicit user crop. An
    /// explicit crop is always authoritative and is never adjusted.
    pub fn apply_crop_stage(
        &mut self,
        geometry: Option<&lumina_sidecar::Geometry>,
        use_content_default: bool,
    ) -> Result<(), CoreError> {
        // Resolve the target rectangle first. Validation runs before any pixel
        // mutation, matching the previous early-error contract.
        let (x, y, w, h) = match geometry {
            Some(geometry) => {
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
                match geometry.crop.as_ref() {
                    // An explicit user crop stays untouched — the maximum-rect
                    // default never overrides an authored rectangle.
                    Some(crop) => crop_rect(self.width, self.height, Some(crop))?,
                    None if use_content_default => default_content_crop(self),
                    None => (0, 0, self.width, self.height),
                }
            }
            None if use_content_default => default_content_crop(self),
            None => return Ok(()),
        };
        // Only materialize a cropped copy when the rectangle is not already the
        // full frame (identity keeps the historic zero-copy behavior).
        if (x, y, w, h) != (0, 0, self.width, self.height) {
            *self = crop_frame(self, x, y, w, h)?;
        }
        let Some(geometry) = geometry else {
            return Ok(());
        };
        let mut transformed = rotate_frame(self, geometry.rotation_degrees);
        if geometry.mirror_horizontal {
            flip_horizontal(&mut transformed);
        }
        if geometry.mirror_vertical {
            flip_vertical(&mut transformed);
        }
        *self = transformed;
        Ok(())
    }

    /// GEN-FILL-01: apply lens stage only (distortion+vignette) without perspective/crop.
    pub fn apply_lens_stage(
        &mut self,
        lens: Option<&lumina_sidecar::LensCorrection>,
        #[cfg(feature = "lensfun")] lensfun: Option<&lumina_lensfun::Corrector>,
    ) -> Result<(), CoreError> {
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
        Ok(())
    }

    /// GEN-FILL-01: apply perspective stage (perspective + CA) without lens/crop.
    ///
    /// The manual `apply_ca` is skipped when a non-identity TCA-capable
    /// Lensfun corrector is passed (G-06): `apply_lens` already corrected CA
    /// in the lens stage then, and a second channel-scale pass would be a
    /// double correction. Without TCA the manual model stays authoritative.
    pub fn apply_perspective_stage(
        &mut self,
        lens: Option<&lumina_sidecar::LensCorrection>,
        perspective: Option<&lumina_sidecar::Perspective>,
        #[cfg(feature = "lensfun")] lensfun: Option<&lumina_lensfun::Corrector>,
    ) -> Result<(), CoreError> {
        if let Some(p) = perspective {
            validate_perspective(p)?;
            *self = apply_perspective(self, p)?;
        }
        #[cfg(feature = "lensfun")]
        let tca_active = lensfun.is_some_and(|c| !c.is_identity() && c.has_tca());
        #[cfg(not(feature = "lensfun"))]
        let tca_active = false;
        if let Some(l) = lens {
            if !tca_active {
                apply_ca(self, l);
            }
        }
        Ok(())
    }

    /// GEN-FILL-01: heuristic auto-fill for transparent pixels after lens correction.
    ///
    /// **Not on the render path since GEN-ONNX-1 Welle 1** (the render composites
    /// a model-produced artifact via [`crate::generative::composite_auto_fill`]);
    /// kept as an explicit standalone/test utility with its cache contract.
    pub fn apply_auto_fill_transparent(&mut self, auto_fill_transparent: bool, seed: u64) -> bool {
        if !auto_fill_transparent {
            return false;
        }
        if !crate::generative::has_transparent_pixels(self) {
            return false;
        }
        crate::generative::fill_transparent_cached_global(self, seed)
    }

    /// GEN-FILL-01: full geometry with auto-fill insertion (Lens → Fill → Perspective → Crop).
    pub fn apply_geometry_with_auto_fill(
        &mut self,
        geometry: Option<&lumina_sidecar::Geometry>,
        lens: Option<&lumina_sidecar::LensCorrection>,
        perspective: Option<&lumina_sidecar::Perspective>,
        #[cfg(feature = "lensfun")] lensfun: Option<&lumina_lensfun::Corrector>,
        auto_fill_transparent: bool,
        seed: u64,
    ) -> Result<(), CoreError> {
        self.apply_lens_stage(
            lens,
            #[cfg(feature = "lensfun")]
            lensfun,
        )?;
        self.apply_auto_fill_transparent(auto_fill_transparent, seed);
        #[cfg(feature = "lensfun")]
        {
            self.apply_perspective_stage(lens, perspective, lensfun)?;
        }
        #[cfg(not(feature = "lensfun"))]
        {
            self.apply_perspective_stage(lens, perspective)?;
        }
        // CROP-MAXRECT-1: auto-fill can remove the wedges again; the default
        // crop is content-based, so a fully filled frame stays the identity.
        #[cfg(feature = "lensfun")]
        let corrected = lens.is_some() || perspective.is_some() || lensfun.is_some();
        #[cfg(not(feature = "lensfun"))]
        let corrected = lens.is_some() || perspective.is_some();
        self.apply_crop_stage(geometry, corrected)
    }

    pub fn measurement_domain(
        &self,
        geometry: Option<&lumina_sidecar::Geometry>,
    ) -> Result<MeasurementDomain, CoreError> {
        self.measurement_domain_with_perspective(geometry, None, None)
    }

    /// Computes dimensions in the same order as rendering: lens (same bounds),
    /// perspective (projected-corner bounding box), crop, rotation, mirror.
    ///
    /// CROP-MAXRECT-1 note: this predicts dimensions from geometry parameters
    /// only. It does **not** include the content-based maximum-rectangle
    /// default crop applied by the render when no explicit crop is set (that
    /// rectangle depends on the actual pixels after lens/perspective, which
    /// this dimensions-only API cannot see). When `geometry.crop` is `None`
    /// and a lens/perspective correction is active, the rendered frame can
    /// therefore be smaller than the reported domain. Pass an explicit
    /// `geometry.crop` to get a domain that matches the render.
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

        // Only clone the pixel buffer when we must mutate it in place:
        // stochastic dithering, or lossy WebP quantization. For plain
        // PNG/JPEG/WebP-lossless export the original buffer is passed by
        // reference straight to the encoder, avoiding a full 16 MB copy at
        // 2048² (F-074-A4). The frame is already validated by `ImageFrame::new`,
        // so `pixels_ref.len()` always matches `width * height * 4`.
        let mut owned: Option<Vec<u8>> = None;
        if options.dither {
            let buf = owned.get_or_insert_with(|| self.pixels.clone());
            dither_rgba8(buf, options.seed);
        }
        // image's portable WebP encoder is lossless-only. Quantizing before
        // VP8L encoding provides the documented quality-controlled lossy path
        // without adding a native dependency (quality 100 is lossless).
        if options.format == ImageFileFormat::WebP && options.quality < 100 {
            let buf = owned.get_or_insert_with(|| self.pixels.clone());
            let step = ((101 - options.quality as u16) / 10).max(1) as u8;
            for (index, value) in buf.iter_mut().enumerate() {
                if index % 4 != 3 {
                    *value = (*value / step) * step;
                }
            }
        }
        let pixels_ref: &[u8] = match &owned {
            Some(buf) => buf,
            None => &self.pixels,
        };

        let mut output = Cursor::new(Vec::new());
        match options.format {
            ImageFileFormat::Png => PngEncoder::new(&mut output).write_image(
                pixels_ref,
                self.width,
                self.height,
                ColorType::Rgba8.into(),
            ),
            ImageFileFormat::Jpeg => {
                // JPEG is RGB; drop the alpha channel. Build it directly from
                // the (possibly mutated) buffer without an intermediate clone.
                let rgb: Vec<u8> = pixels_ref
                    .as_chunks::<4>()
                    .0
                    .iter()
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
                pixels_ref,
                self.width,
                self.height,
                ColorType::Rgba8.into(),
            ),
        }
        .map_err(|error| CoreError::Encode(error.to_string()))?;
        Ok(output.into_inner())
    }

    pub fn apply_recipe(&mut self, recipe: &EditRecipe) -> Result<(), CoreError> {
        self.apply_recipe_with_scale_white_balance_and_denoise(
            recipe,
            1.0,
            None,
            &crate::DenoiseStageInput::inactive(),
        )
    }

    /// LRPAR-G14-DENOISE-IMPL-20: [`Self::apply_recipe`] with a caller-resolved
    /// KI-Denoise stage (artifact + §6 status + fallback policy). Existing
    /// entry points pass [`crate::DenoiseStageInput::inactive`], so a recipe
    /// without `denoise_ai` renders byte-identically.
    pub fn apply_recipe_with_denoise(
        &mut self,
        recipe: &EditRecipe,
        denoise: &crate::DenoiseStageInput<'_>,
    ) -> Result<(), CoreError> {
        self.apply_recipe_with_scale_white_balance_and_denoise(recipe, 1.0, None, denoise)
    }

    /// LRPAR-G14-DENOISE-IMPL-20: [`Self::apply_recipe_with_scale`] with a
    /// caller-resolved KI-Denoise stage.
    pub fn apply_recipe_with_scale_and_denoise(
        &mut self,
        recipe: &EditRecipe,
        effective_scale: f32,
        denoise: &crate::DenoiseStageInput<'_>,
    ) -> Result<(), CoreError> {
        self.apply_recipe_with_scale_white_balance_and_denoise(
            recipe,
            effective_scale,
            None,
            denoise,
        )
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
        self.apply_recipe_with_scale_white_balance_and_denoise(
            recipe,
            1.0,
            camera_white_balance,
            &crate::DenoiseStageInput::inactive(),
        )
    }

    /// LRPAR-G14-DENOISE-IMPL-20: [`Self::apply_recipe_with_white_balance`] with
    /// a caller-resolved KI-Denoise stage.
    pub fn apply_recipe_with_white_balance_and_denoise(
        &mut self,
        recipe: &EditRecipe,
        camera_white_balance: Option<[f32; 4]>,
        denoise: &crate::DenoiseStageInput<'_>,
    ) -> Result<(), CoreError> {
        self.apply_recipe_with_scale_white_balance_and_denoise(
            recipe,
            1.0,
            camera_white_balance,
            denoise,
        )
    }

    /// Applies adjustments at an explicit effective output scale.  Keeping the
    /// old method above preserves CLI/GUI API compatibility; radius-sensitive
    /// sharpening uses this scale in source-pixel units.
    pub fn apply_recipe_with_scale(
        &mut self,
        recipe: &EditRecipe,
        effective_scale: f32,
    ) -> Result<(), CoreError> {
        self.apply_recipe_with_scale_white_balance_and_denoise(
            recipe,
            effective_scale,
            None,
            &crate::DenoiseStageInput::inactive(),
        )
    }

    /// Shared implementation of every `apply_recipe*` entry point. The
    /// KI-Denoise stage (`denoise_ai`) runs after the colour stages and
    /// immediately before the manual F-096 noise reduction; `denoise` is the
    /// caller-resolved stage state (default: inactive, i.e. identity).
    pub fn apply_recipe_with_scale_white_balance_and_denoise(
        &mut self,
        recipe: &EditRecipe,
        effective_scale: f32,
        camera_white_balance: Option<[f32; 4]>,
        denoise: &crate::DenoiseStageInput<'_>,
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
            presence_stages::apply_presence(&mut self.pixels, self.width, self.height, presence);
        }
        if let Some(curves) = &recipe.curves {
            for_each_rgba_mut(&mut self.pixels, |pixel| {
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
            });
        }
        if let Some(hsl) = &recipe.hsl {
            color_stages::apply_hsl(&mut self.pixels, hsl)?;
        }
        // F-090b Point Color follows HSL: targeted selection before the
        // global vibrance/saturation scaling (pipeline order HSL → Point
        // Color → Vibrance/Saturation → Color Grading).
        if let Some(point_color) = &recipe.point_color {
            color_stages::apply_point_color(&mut self.pixels, point_color);
        }
        // F-092 deliberately follows HSL: vibrance is the selective operation,
        // then global saturation scales the resulting HSL saturation.
        color_stages::apply_vibrance_and_saturation(
            &mut self.pixels,
            recipe.adjustments.get("vibrance"),
            recipe.adjustments.get("saturation"),
        );
        if let Some(color_grading) = &recipe.color_grading {
            color_stages::apply_color_grading(&mut self.pixels, color_grading);
        }
        // LRPAR-G14-DENOISE-IMPL-20: KI-Denoise (optional, additive) runs
        // immediately before the manual F-096 noise reduction, which stays the
        // visible fallback anchor. A non-active field is identity.
        crate::denoise::apply_denoise_stage(self, recipe.denoise_ai.as_ref(), denoise)?;
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
        // LRPAR-G14-REDEYE-15: red-eye correction runs after sharpening and
        // before effects (F-097), so grain applies uniformly over corrected
        // pupils. The pixel tuple is unchanged.
        if let Some(red_eye) = &recipe.red_eye {
            apply_red_eye(&mut self.pixels, self.width, self.height, red_eye);
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
