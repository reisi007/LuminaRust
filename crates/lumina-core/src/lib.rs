//! Small, portable raster MVP shared by the native CLI and GUI.
use image::{
    codecs::jpeg::JpegEncoder, codecs::png::PngEncoder, codecs::webp::WebPEncoder, ColorType,
    ImageEncoder,
};
use lumina_sidecar::EditRecipe;
use std::io::Cursor;
use thiserror::Error;
pub mod cache;
// The per-pixel colour stages shared by the global kernel and the mask-local
// MASK-LOCAL-P1.2b chain (see the module docs for why they are separate from
// their `u8` wrappers).
pub(crate) mod color_stages;
pub mod crop_max_rect;
mod curve_math;
pub(crate) mod detail_stages;
pub(crate) use curve_math::monotone_curve;
pub mod denoise;
pub mod generative;
pub mod histogram;
pub mod lens_blur;
pub mod lensfun_map;
pub(crate) mod mask_alignment;
pub mod mask_loader;
pub mod mask_modulation;
pub mod mask_tiles;
pub mod masks;
pub mod memory;
pub mod merge_geom;
pub mod pipeline;
// MASK-LOCAL-P1.2c: the presence mathematics, literally shared by the global
// recipe and the mask-local presence kernel. The module is crate-private
// because the two callers are the only legal users of it.
pub(crate) mod presence_stages;
pub mod preview_cache;
pub mod range_masks;
pub mod red_eye;
pub mod render;
pub mod spot_heal;
pub mod stage_cache;
pub mod tone;
pub mod upright;
// This module holds real `proptest` properties.
#[cfg(test)]
mod tone_props;
// R2-LENS-01 row-path contracts (extracted from `lib.rs`, DoD §8 ratchet).
#[cfg(all(test, feature = "lensfun"))]
mod lens_row_tests;
pub use cache::disk::{DiskCacheError, DiskFolderCache};
pub use cache::{
    CacheEntry, CacheError, CacheStage, CacheStore, Cancellation, FolderCache, FolderCacheSettings,
    StaleTracker,
};
pub use crop_max_rect::{maximum_content_rect, PixelRect, CONTENT_ALPHA_MIN};
pub use denoise::{
    apply_denoise_blend, apply_denoise_stage, assemble_denoise_tiles, denoise_producer_provenance,
    resolve_denoise_status, set_denoise_producer_provenance, DenoiseIdentity, DenoiseOutcome,
    DenoisePolicy, DenoiseRgbArtifact, DenoiseStageInput, DenoiseStageStatus, DenoiseTile,
    DENOISE_DETAIL_SCALE, DENOISE_PRODUCER_PROVENANCE_KEY, DENOISE_RGB_ENCODING_VERSION,
};
pub use generative::{
    apply_generative_expand_cached, clear_generative_cache, composite_auto_fill, composite_expand,
    effective_keep as effective_keep_generative, fill_transparent_cached,
    fill_transparent_cached_global, fill_transparent_heuristic, generative_cache_stats,
    generative_canvas, generative_edit, generative_input_digest, has_transparent_pixels,
    materialize_canvas_for_crop, materialize_canvas_for_crop_with_source,
    resolve_canvas_for_recipe, FillOutcome, GenerativeCache, GenerativeCacheKey,
    GenerativeCacheStats, GenerativeCanvasArtifact, GenerativeCanvasInput, GenerativeIdentity,
    GenerativeRole,
};
pub use histogram::LuminanceHistogram;
pub use lens_blur::{apply_lens_blur, lens_blur_status, validate_lens_blur, DepthPlane};
pub use lensfun_map::LensfunMap;
pub use mask_loader::{
    resolve_mask_planes, MaskInference, MaskLoadContext, MaskLoadOutcome, MaskLoadResult,
    MaskResolvedFrom,
};
pub use mask_modulation::modulate_mask_plane;
pub use masks::{MaskError, MaskGraph, MaskPlane};
pub use memory::{MemoryBudget, MemoryBudgetError};
pub use pipeline::{OutputSpec, Pipeline, PipelineFormat, PipelineStage, RenderKey, SourceAction};
pub use preview_cache::PreviewDiskCache;
pub use preview_cache::{
    decode_webp, encode_webp_lossless, prefetch_window, LruPreviewCache, PrefetchSlot,
    PreviewEncode, PreviewKey, PreviewKind,
};
pub use red_eye::{
    detect_red_eyes, DetectedRedEye, RedEyeDetection, RED_EYE_DETECT_DEFAULT_DARKEN,
    RED_EYE_DETECT_DEFAULT_DESATURATE, RED_EYE_DETECT_ID_PREFIX, RED_EYE_DETECT_MAX_RADIUS,
    RED_EYE_DETECT_MAX_REGIONS, RED_EYE_DETECT_MIN_PIXELS, RED_EYE_DETECT_RADIUS_MARGIN,
    RED_EYE_DETECT_REDNESS_THRESHOLD,
};
pub use render::{
    apply_spot_heals_from_recipe, generative_input_frames, prepare_source_base, render_frame,
    render_frame_from_base, render_frame_from_base_with_denoise,
    render_frame_from_base_with_generative, render_frame_from_base_with_generative_and_denoise,
    render_frame_with_denoise, render_frame_with_generative,
    render_frame_with_generative_and_denoise, LensfunCorrectorRef, MaskContext, MaskLayerResult,
    MaskPolicy, RenderContext, RenderOutput, SourceActionArtifact, StageWork,
};
pub use spot_heal::{
    apply_spot_heals, apply_visualize_overlay, detect_spots_heuristic, distraction_candidates,
    generative_variant_seed, psnr, spots_from_recipe, visualize_spots_mask, DetectedSpot,
    DistractionKind, DistractionSetting, DistractionStatus, SpotHeuristic,
};
pub use stage_cache::StageFrameCache;
pub use tone::{
    analyze_tone, analyze_tone_with_histogram, match_total_exposure, match_total_exposure_masked,
    suggest_auto_tone, tone_fingerprint, AutoToneConfig, AutoToneResult, ToneAnalysis,
};
pub use upright::{
    analyze_upright, upright_analysis, upright_input_fingerprint, UprightSuggestion,
    UPRIGHT_ALGORITHM, UPRIGHT_ALGORITHM_VERSION,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFileFormat {
    Png,
    Jpeg,
    WebP,
}
impl ImageFileFormat {
    /// Parse a file extension (without the leading dot) into an export format.
    /// Recognises `png`, `jpg`, `jpeg` and `webp` (case-insensitive). Returns
    /// `None` for anything else so callers can reject unknown formats loudly
    /// instead of silently picking one.
    pub fn from_extension(extension: &str) -> Option<ImageFileFormat> {
        match extension.to_ascii_lowercase().as_str() {
            "png" => Some(ImageFileFormat::Png),
            "jpg" | "jpeg" => Some(ImageFileFormat::Jpeg),
            "webp" => Some(ImageFileFormat::WebP),
            _ => None,
        }
    }
    /// Canonical file extension (without the leading dot) for this format.
    /// `Jpeg` maps to `jpg` so saved files match the CLI's `format_extension`
    /// convention and the byte-identical export contract.
    pub fn default_extension(self) -> &'static str {
        match self {
            ImageFileFormat::Png => "png",
            ImageFileFormat::Jpeg => "jpg",
            ImageFileFormat::WebP => "webp",
        }
    }
}

/// Shared render + encode path used by both the CLI and the GUI export module.
///
/// Takes an already-decoded source frame, the full render context (recipe,
/// white balance, source actions and masks) and the export options (format,
/// quality, …) and returns the encoded artifact bytes. It performs **no**
/// filesystem I/O so it stays platform-neutral and produces identical output
/// for every caller — that is exactly what makes the GUI export byte-identical
/// to the CLI export (F-103-N5): both feed the same frame, recipe and
/// `ExportOptions` into this single function.
pub fn export_image(
    frame: &ImageFrame,
    context: &RenderContext,
    options: ExportOptions,
) -> Result<Vec<u8>, CoreError> {
    export_image_with_generative(
        frame,
        context,
        options,
        crate::generative::GenerativeCanvasInput::default(),
    )
}

/// GEN-ONNX-1: [`export_image`] with a caller-supplied generative canvas
/// artifact (artifact compositing instead of the former heuristic BFS).
pub fn export_image_with_generative(
    frame: &ImageFrame,
    context: &RenderContext,
    options: ExportOptions,
    generative: crate::generative::GenerativeCanvasInput<'_>,
) -> Result<Vec<u8>, CoreError> {
    let rendered = render_frame_with_generative(frame, context, generative)?;
    rendered.frame.encode_with_options(options)
}

/// Bilinear downscale of an RGBA8 frame so that the output **width** does not
/// exceed `max_width`. Aspect ratio is preserved and upscaling never occurs.
///
/// The operation is fully deterministic (pure math, no randomness), which makes
/// previews and other width-limited renderings reproducible.
///
/// Moved out of `lumina-mcp` (R2-MCP-06): this is platform-neutral image
/// processing and belongs with the rest of the shared pipeline in `lumina-core`
/// for reuse and golden-testing, not in the MCP orchestration layer. Unlike the
/// old mcp helper it returns a `Result` instead of panicking via `expect` when
/// the derived dimensions are inconsistent (which cannot happen for the exact
/// `new_width * new_height * 4` allocation used here, but the fallible
/// [`ImageFrame::new`] contract is surfaced rather than swallowed).
pub fn downscale_bilinear(frame: &ImageFrame, max_width: u32) -> Result<ImageFrame, CoreError> {
    let (width, height) = (frame.width, frame.height);
    if width == 0 || height == 0 {
        return Ok(frame.clone());
    }
    let new_width = width.min(max_width).max(1);
    let new_height = ((new_width as f64 / width as f64) * height as f64)
        .round()
        .max(1.0) as u32;
    if new_width == width && new_height == height {
        return Ok(frame.clone());
    }
    let mut pixels = vec![0u8; new_width as usize * new_height as usize * 4];
    for y in 0..new_height {
        let source_y = (y as f64 + 0.5) * height as f64 / new_height as f64 - 0.5;
        let y0 = source_y.floor().max(0.0) as u32;
        let y1 = (y0 + 1).min(height - 1);
        let ty = (source_y - y0 as f64).clamp(0.0, 1.0);
        for x in 0..new_width {
            let source_x = (x as f64 + 0.5) * width as f64 / new_width as f64 - 0.5;
            let x0 = source_x.floor().max(0.0) as u32;
            let x1 = (x0 + 1).min(width - 1);
            let tx = (source_x - x0 as f64).clamp(0.0, 1.0);
            for channel in 0..4 {
                let p00 = frame.pixels[((y0 * width + x0) * 4 + channel) as usize];
                let p01 = frame.pixels[((y0 * width + x1) * 4 + channel) as usize];
                let p10 = frame.pixels[((y1 * width + x0) * 4 + channel) as usize];
                let p11 = frame.pixels[((y1 * width + x1) * 4 + channel) as usize];
                let top = p00 as f64 + (p01 as f64 - p00 as f64) * tx;
                let bottom = p10 as f64 + (p11 as f64 - p10 as f64) * tx;
                pixels[((y * new_width + x) * 4 + channel) as usize] =
                    (top + (bottom - top) * ty).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    ImageFrame::new(new_width, new_height, pixels)
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
    #[error("invalid {name}: must be finite and in {minimum:e}..={maximum:e}, got {value:e}")]
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
    #[error("local mask adjustment unsupported: {reason}")]
    LocalAdjustmentUnsupported { reason: String },
    #[error("invalid local mask adjustment: {reason}")]
    InvalidLocalAdjustment { reason: String },
    /// LRPAR-G14-DENOISE-IMPL-20: an active `denoise_ai` stage whose artifact/
    /// model is not usable under [`crate::DenoisePolicy::Strict`]. `status` is
    /// the visible §6 state (`unavailable`/`stale`/`missing`/`corrupt`).
    #[error("denoise_ai is {status}: {reason}")]
    Denoise { status: String, reason: String },
}

/// PERF-GUI-* (CPU quick-wins): iterate over each RGBA pixel of `pixels` (length
/// must be a multiple of 4) with a per-pixel closure.
///
/// When the `parallel` feature is enabled the chunks are processed with
/// `rayon::par_chunks_exact_mut(4)`; otherwise sequentially with
/// `as_chunks_mut::<4>()` (which LLVM can auto-vectorize). Every closure only
/// touches its own 4-byte slice, so execution order is irrelevant and the result
/// is bit-identical to a sequential pass. Use this only for order-independent
/// per-pixel work (the adjustment passes read and write a single pixel).
#[cfg(feature = "parallel")]
pub(crate) fn for_each_rgba_mut<F>(pixels: &mut [u8], f: F)
where
    F: Fn(&mut [u8]) + Sync + Send,
{
    use rayon::prelude::*;
    debug_assert_eq!(pixels.len() % 4, 0);
    pixels.par_chunks_exact_mut(4).for_each(f);
}

#[cfg(not(feature = "parallel"))]
pub(crate) fn for_each_rgba_mut<F>(pixels: &mut [u8], f: F)
where
    F: Fn(&mut [u8]),
{
    debug_assert_eq!(pixels.len() % 4, 0);
    pixels
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .for_each(|pixel| f(&mut pixel[..]));
}

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
        Ok(Self {
            width,
            height,
            pixels,
        })
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

    /// PERF-GUI-3: bilinear downscale so the long edge is at most `max_dim`.
    ///
    /// Used by the GUI to render a fast draft preview at viewport resolution
    /// instead of the full (e.g. 45 MP) source while a slider is dragged. The
    /// result is always a multiple of 4 bytes and `image`-decoded identical for
    /// `max_dim >= max(width, height)` (i.e. when no downscaling is needed).
    pub fn downscale(&self, max_dim: u32) -> ImageFrame {
        let long = self.width.max(self.height);
        if max_dim == 0 || long <= max_dim {
            return self.clone();
        }
        let scale = max_dim as f32 / long as f32;
        let width = (self.width as f32 * scale).round().max(1.0) as u32;
        let height = (self.height as f32 * scale).round().max(1.0) as u32;
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        for y in 0..height {
            let sy = ((y as f32 + 0.5) / height as f32 * self.height as f32 - 0.5)
                .clamp(0.0, self.height as f32 - 1.0);
            let y0 = sy.floor() as u32;
            let y1 = (y0 + 1).min(self.height - 1);
            let fy = sy - y0 as f32;
            for x in 0..width {
                let sx = ((x as f32 + 0.5) / width as f32 * self.width as f32 - 0.5)
                    .clamp(0.0, self.width as f32 - 1.0);
                let x0 = sx.floor() as u32;
                let x1 = (x0 + 1).min(self.width - 1);
                let fx = sx - x0 as f32;
                let at = |xx: u32, yy: u32| -> [u8; 4] {
                    let base = (yy * self.width + xx) as usize * 4;
                    [
                        self.pixels[base],
                        self.pixels[base + 1],
                        self.pixels[base + 2],
                        self.pixels[base + 3],
                    ]
                };
                let top = at(x0, y0);
                let top_r = top[0] as f32 * (1.0 - fx) + at(x1, y0)[0] as f32 * fx;
                let top_g = top[1] as f32 * (1.0 - fx) + at(x1, y0)[1] as f32 * fx;
                let top_b = top[2] as f32 * (1.0 - fx) + at(x1, y0)[2] as f32 * fx;
                let top_a = top[3] as f32 * (1.0 - fx) + at(x1, y0)[3] as f32 * fx;
                let bot = at(x0, y1);
                let bot_r = bot[0] as f32 * (1.0 - fx) + at(x1, y1)[0] as f32 * fx;
                let bot_g = bot[1] as f32 * (1.0 - fx) + at(x1, y1)[1] as f32 * fx;
                let bot_b = bot[2] as f32 * (1.0 - fx) + at(x1, y1)[2] as f32 * fx;
                let bot_a = bot[3] as f32 * (1.0 - fx) + at(x1, y1)[3] as f32 * fx;
                let dst = (y * width + x) as usize * 4;
                pixels[dst] = (top_r * (1.0 - fy) + bot_r * fy).round() as u8;
                pixels[dst + 1] = (top_g * (1.0 - fy) + bot_g * fy).round() as u8;
                pixels[dst + 2] = (top_b * (1.0 - fy) + bot_b * fy).round() as u8;
                pixels[dst + 3] = (top_a * (1.0 - fy) + bot_a * fy).round() as u8;
            }
        }
        ImageFrame {
            width,
            height,
            pixels,
        }
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
/// portable (no native/SIMD intrinsics).
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
    // `for_each_rgba_mut` keeps this auto-vectorizable (and parallel under the
    // `parallel` feature) while staying bit-identical to a sequential loop.
    for_each_rgba_mut(pixels, |pixel| {
        pixel[0] = lut[0][pixel[0] as usize];
        pixel[1] = lut[1][pixel[1] as usize];
        pixel[2] = lut[2][pixel[2] as usize];
    });
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
fn validate_generative_edit(g: &lumina_sidecar::GenerativeEdit) -> Result<(), CoreError> {
    // GEN-PIPELINE-DECOUPLE: structural validation of the GenerativeEdit
    // recipe stage (mirrors the sidecar rules). Any violation is
    // `InvalidAdjustment` — never a silent fallback to "render as if the
    // stage were absent".
    if g.version != 1 {
        return Err(CoreError::InvalidAdjustment {
            name: "generative_edit.version".into(),
            value: g.version as f64,
            minimum: 1.0,
            maximum: 1.0,
        });
    }
    let expand = g.expand_beyond_image.unwrap_or(false);
    if expand && g.canvas.is_none() {
        return Err(CoreError::InvalidAdjustment {
            name: "generative_edit.canvas".into(),
            value: 0.0,
            minimum: 1.0,
            maximum: 1.0,
        });
    }
    if !expand && g.canvas.is_some() {
        return Err(CoreError::InvalidAdjustment {
            name: "generative_edit.canvas".into(),
            value: 1.0,
            minimum: 0.0,
            maximum: 0.0,
        });
    }
    if let Some(canvas) = &g.canvas {
        if canvas.output_width == 0 || canvas.output_height == 0 {
            return Err(CoreError::InvalidAdjustment {
                name: "generative_edit.canvas.output".into(),
                value: 0.0,
                minimum: 1.0,
                maximum: f64::from(u32::MAX),
            });
        }
    }
    Ok(())
}
/// LRPAR-G06-UPRIGHT-15: validiert die additive Upright-Rezept-Stufe laut.
/// `enabled` ohne persistierte Analyse wird abgelehnt (kein stiller
/// Identitäts-Render); die Vorschlagswerte bleiben in der F-099-Domäne.
pub fn validate_upright(u: &lumina_sidecar::Upright) -> Result<(), CoreError> {
    let invalid = |name: &str, value: f64, lo: f64, hi: f64| CoreError::InvalidAdjustment {
        name: name.into(),
        value,
        minimum: lo,
        maximum: hi,
    };
    if u.version != 1 {
        return Err(invalid("upright.version", u.version as f64, 1.0, 1.0));
    }
    if u.enabled && u.analysis.is_none() {
        return Err(CoreError::UnsupportedAdjustment {
            key: "upright enabled without a persisted analysis".into(),
        });
    }
    if let Some(analysis) = &u.analysis {
        for (field, value) in [
            ("vertical", analysis.vertical),
            ("horizontal", analysis.horizontal),
            ("rotation", analysis.rotation),
        ] {
            if !value.is_finite() || !(-1.0..=1.0).contains(&value) {
                return Err(invalid(
                    &format!("upright.{field}"),
                    value as f64,
                    -1.0,
                    1.0,
                ));
            }
        }
        if !analysis.confidence.is_finite() || !(0.0..=1.0).contains(&analysis.confidence) {
            return Err(invalid(
                "upright.confidence",
                analysis.confidence as f64,
                0.0,
                1.0,
            ));
        }
        if analysis.line_count > lumina_sidecar::UPRIGHT_MAX_LINE_COUNT {
            return Err(invalid(
                "upright.line_count",
                analysis.line_count as f64,
                0.0,
                lumina_sidecar::UPRIGHT_MAX_LINE_COUNT as f64,
            ));
        }
        for (field, value) in [
            ("algorithm", &analysis.fingerprint.algorithm),
            ("version", &analysis.fingerprint.version),
            ("input_fingerprint", &analysis.fingerprint.input_fingerprint),
        ] {
            if value.is_empty() {
                return Err(CoreError::UnsupportedAdjustment {
                    key: format!("upright fingerprint {field} must not be empty"),
                });
            }
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
            if !d.is_finite() || d.abs() < 1e-6 {
                return Err(CoreError::InvalidAdjustment {
                    name: "perspective".into(),
                    value: d as f64,
                    minimum: -1.0,
                    maximum: 1.0,
                });
            }
            let q = [
                (m[0][0] * x + m[0][1] * y + m[0][2]) / d,
                (m[1][0] * x + m[1][1] * y + m[1][2]) / d,
            ];
            if !q[0].is_finite() || !q[1].is_finite() {
                return Err(CoreError::InvalidAdjustment {
                    name: "perspective".into(),
                    value: q[0] as f64,
                    minimum: -1.0,
                    maximum: 1.0,
                });
            }
            min[0] = min[0].min(q[0]);
            max[0] = max[0].max(q[0]);
            min[1] = min[1].min(q[1]);
            max[1] = max[1].max(q[1]);
        }
    }
    let ow_f = ((max[0] - min[0]) * w as f32 / 2.0).ceil().max(1.0);
    let oh_f = ((max[1] - min[1]) * h as f32 / 2.0).ceil().max(1.0);
    if !ow_f.is_finite() || !oh_f.is_finite() {
        return Err(CoreError::InvalidAdjustment {
            name: "perspective".into(),
            value: ow_f as f64,
            minimum: 1.0,
            maximum: 1_000_000.0,
        });
    }
    let ow = ow_f as u32;
    let oh = oh_f as u32;
    // Bounding box limit + MemoryBudget guard (REVIEW-CORE-PERSP-1).
    MemoryBudget::default()
        .check_decode(ow as u64, oh as u64, 4, 1)
        .map_err(|e| CoreError::InvalidAdjustment {
            name: "perspective canvas too large".into(),
            value: ow as f64 * oh as f64,
            minimum: 0.0,
            maximum: e.limit() as f64,
        })?;
    Ok((ow, oh))
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
pub(crate) fn sample(frame: &ImageFrame, x: f32, y: f32, ch: usize) -> f32 {
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
pub(crate) const EMPTY_LENS: lumina_sidecar::LensCorrection = lumina_sidecar::LensCorrection {
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

pub(crate) fn apply_lens(
    frame: &mut ImageFrame,
    l: &lumina_sidecar::LensCorrection,
    #[cfg(feature = "lensfun")] lensfun: Option<&lumina_lensfun::Corrector>,
) {
    // F-098-N1: a Lensfun corrector (when present and non-identity) replaces the
    // manual radial-distortion Newton iteration and the vignette polynomial with
    // the database profile. The corrector's geometry maps a destination
    // (corrected) pixel `(x, y)` in `[0, width-1] × [0, height-1]` to the
    // source (distorted) pixel to sample — the same pixel space `apply_lens`
    // iterates over. Vignetting is applied on the RGB channels only; the alpha
    // channel is left untouched (same structure as the manual model).
    //
    // R2-LENS-01: the loop is *row-wise* — every row crosses the lensfun FFI
    // boundary once per pixel for the geometry via `geometry_row` (which
    // issues one native width=1 call per column, bit-identical to per-pixel
    // `geometry` — a single multi-pixel native geometry batch call is
    // provably wrong on x86_64, see the `lumina-lensfun` docs) plus once per
    // row for the vignetting via `apply_vignetting_row`. The vignetting batch
    // results are not byte-identical to the previous per-pixel `color_gain`
    // calls: the first column is bit-identical, but later columns drift by
    // float rounding that grows with the row width (documented in the
    // `lumina-lensfun` wrapper contract). That is why this switch
    // deliberately changes the output and requires a Golden rebaseline
    // (F-043), it is not a silent fallback.
    //
    // G-06 Lensfun-Vollausbau (TCA): when the corrector carries TCA
    // calibration (`has_tca`), R/G/B are sampled at separate subpixel
    // coordinates (`subpixel_row`, green = reference like the manual CA
    // model) in the SAME lens-stage resampling pass — TCA is a geometric
    // lens property, so it is corrected together with the distortion, before
    // perspective (unlike the manual channel-scale hack, which stays after
    // perspective in `apply_perspective_stage`). The manual `apply_ca` must
    // therefore be skipped whenever this TCA path ran (no double
    // correction); see `lensfun_tca_active`.
    #[cfg(feature = "lensfun")]
    if let Some(corrector) = lensfun {
        if !corrector.is_identity() {
            let src = frame.clone();
            let width = frame.width as usize;
            if corrector.has_tca() {
                let mut coords = vec![((0.0, 0.0), (0.0, 0.0), (0.0, 0.0)); width];
                let mut rgb = vec![0f32; width * 3];
                for y in 0..frame.height {
                    corrector.subpixel_row(0.0, y as f64, &mut coords);
                    for (i, (r, g, b)) in coords.iter().enumerate() {
                        rgb[i * 3] = sample(&src, r.0 as f32, r.1 as f32, 0);
                        rgb[i * 3 + 1] = sample(&src, g.0 as f32, g.1 as f32, 1);
                        rgb[i * 3 + 2] = sample(&src, b.0 as f32, b.1 as f32, 2);
                    }
                    corrector.apply_vignetting_row(&mut rgb, 0.0, y as f64);
                    let row_base = y as usize * width * 4;
                    for (i, (_, g, _)) in coords.iter().enumerate() {
                        let dst = row_base + i * 4;
                        frame.pixels[dst] = rgb[i * 3].round().clamp(0.0, 255.0) as u8;
                        frame.pixels[dst + 1] = rgb[i * 3 + 1].round().clamp(0.0, 255.0) as u8;
                        frame.pixels[dst + 2] = rgb[i * 3 + 2].round().clamp(0.0, 255.0) as u8;
                        frame.pixels[dst + 3] = sample(&src, g.0 as f32, g.1 as f32, 3)
                            .round()
                            .clamp(0.0, 255.0)
                            as u8;
                    }
                }
                return;
            }
            let mut coords = vec![(0.0, 0.0); width];
            let mut rgb = vec![0f32; width * 3];
            for y in 0..frame.height {
                // One batch FFI call maps the whole destination row to its
                // source row (identical to `geometry` per pixel up to the
                // documented sub-pixel drift).
                corrector.geometry_row(0.0, y as f64, &mut coords);
                for (i, (ref sx, ref sy)) in coords.iter().enumerate() {
                    rgb[i * 3] = sample(&src, *sx as f32, *sy as f32, 0);
                    rgb[i * 3 + 1] = sample(&src, *sx as f32, *sy as f32, 1);
                    rgb[i * 3 + 2] = sample(&src, *sx as f32, *sy as f32, 2);
                }
                // One batch FFI call applies the vignetting gain to the whole
                // sampled RGB row in place.
                corrector.apply_vignetting_row(&mut rgb, 0.0, y as f64);
                let row_base = y as usize * width * 4;
                for (i, (ref sx, ref sy)) in coords.iter().enumerate() {
                    let dst = row_base + i * 4;
                    frame.pixels[dst] = rgb[i * 3].round().clamp(0.0, 255.0) as u8;
                    frame.pixels[dst + 1] = rgb[i * 3 + 1].round().clamp(0.0, 255.0) as u8;
                    frame.pixels[dst + 2] = rgb[i * 3 + 2].round().clamp(0.0, 255.0) as u8;
                    frame.pixels[dst + 3] = sample(&src, *sx as f32, *sy as f32, 3)
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
fn apply_perspective(
    src: &ImageFrame,
    p: &lumina_sidecar::Perspective,
) -> Result<ImageFrame, CoreError> {
    if p.vertical == 0.
        && p.horizontal == 0.
        && p.rotation == 0.
        && p.scale == 1.
        && p.aspect_ratio == 1.
        && p.shift_x == 0.
        && p.shift_y == 0.
    {
        return Ok(src.clone());
    }
    let m = perspective_matrix(p);
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            let d = m[2][0] * x + m[2][1] * y + m[2][2];
            if !d.is_finite() || d.abs() < 1e-6 {
                return Err(CoreError::InvalidAdjustment {
                    name: "perspective".into(),
                    value: d as f64,
                    minimum: -1.0,
                    maximum: 1.0,
                });
            }
            let q = [
                (m[0][0] * x + m[0][1] * y + m[0][2]) / d,
                (m[1][0] * x + m[1][1] * y + m[1][2]) / d,
            ];
            if !q[0].is_finite() || !q[1].is_finite() {
                return Err(CoreError::InvalidAdjustment {
                    name: "perspective".into(),
                    value: q[0] as f64,
                    minimum: -1.0,
                    maximum: 1.0,
                });
            }
            min[0] = min[0].min(q[0]);
            max[0] = max[0].max(q[0]);
            min[1] = min[1].min(q[1]);
            max[1] = max[1].max(q[1]);
        }
    }
    let ow_f = ((max[0] - min[0]) * src.width as f32 / 2.0).ceil().max(1.0);
    let oh_f = ((max[1] - min[1]) * src.height as f32 / 2.0)
        .ceil()
        .max(1.0);
    if !ow_f.is_finite() || !oh_f.is_finite() {
        return Err(CoreError::InvalidAdjustment {
            name: "perspective".into(),
            value: ow_f as f64,
            minimum: 1.0,
            maximum: 1_000_000.0,
        });
    }
    let ow = ow_f as u32;
    let oh = oh_f as u32;
    MemoryBudget::default()
        .check_decode(ow as u64, oh as u64, 4, 1)
        .map_err(|e| CoreError::InvalidAdjustment {
            name: "perspective canvas too large".into(),
            value: ow as f64 * oh as f64,
            minimum: 0.0,
            maximum: e.limit() as f64,
        })?;
    let ow = ow.max(1);
    let oh = oh.max(1);
    let alloc_len = (ow as usize)
        .checked_mul(oh as usize)
        .and_then(|v| v.checked_mul(4))
        .ok_or(CoreError::InvalidAdjustment {
            name: "perspective canvas too large".into(),
            value: ow as f64 * oh as f64,
            minimum: 0.0,
            maximum: MemoryBudget::default().max_alloc_bytes as f64,
        })?;
    // Guard already via MemoryBudget, but keep explicit overflow check.
    let mut out = ImageFrame::new(ow, oh, vec![0; alloc_len]).unwrap();
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if !det.is_finite() || det.abs() < 1e-6 {
        return Err(CoreError::InvalidAdjustment {
            name: "perspective".into(),
            value: det as f64,
            minimum: -1.0,
            maximum: 1.0,
        });
    }
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
            if !sd.is_finite() || sd.abs() < 1e-6 {
                continue;
            }
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
    Ok(out)
}

pub(crate) fn crop_rect(
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
        // REVIEW-CORE-CROP-1: normalized coordinates are bounded by 1 on BOTH
        // ends. Without the explicit upper bound the 1e-6 tolerance window of
        // the rectangle sum allowed `x` slightly above 1, whose rounded pixel
        // origin could land past the frame edge and underflow the
        // `width - px` extent below.
        || x > 1.0
        || y > 1.0
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
    // Rounding can move the origin onto (or past) the frame edge even for a
    // legal `0..=1` rectangle (for example x == 1.0 with width rounding to
    // exactly `width`). Clamp the origin into range — keeping at least one
    // pixel for non-degenerate frames — and derive the extent with saturating
    // arithmetic so no u32 underflow can produce a bogus crop rect
    // (REVIEW-CORE-CROP-1).
    if width == 0 || height == 0 {
        return Err(CoreError::InvalidAdjustment {
            name: "geometry.crop (empty frame)".into(),
            value: -1.0,
            minimum: 0.0,
            maximum: 1.0,
        });
    }
    let px = (((x * width as f64).round() as i64).clamp(0, i64::from(width) - 1)) as u32;
    let py = (((y * height as f64).round() as i64).clamp(0, i64::from(height) - 1)) as u32;
    let pw = ((w * width as f64).round() as u32).max(1).min(width - px);
    let ph = ((h * height as f64).round() as u32).max(1).min(height - py);
    // REVIEW-CORE-CROP-1: an empty extent is a hard error, never a zero-size
    // frame that would flow through the rest of the pipeline. With both
    // clamps above this only triggers on degenerate inputs; it stays as an
    // explicit guard instead of an implicit invariant.
    if pw == 0 || ph == 0 {
        return Err(CoreError::InvalidAdjustment {
            name: "geometry.crop (empty crop rectangle)".into(),
            value: -1.0,
            minimum: 0.0,
            maximum: 1.0,
        });
    }
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

/// CROP-MAXRECT-1: the largest all-content rectangle of `frame`, or the full
/// frame when it has no content at all (degenerate all-transparent frame). The
/// full-frame fallback is the documented identity of the default crop — it
/// never collapses to an empty rectangle.
fn default_content_crop(frame: &ImageFrame) -> (u32, u32, u32, u32) {
    crate::crop_max_rect::maximum_content_rect(frame).unwrap_or((0, 0, frame.width, frame.height))
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
    if let Some(b) = &recipe.lens_blur {
        crate::lens_blur::validate_lens_blur(b)?;
    }
    if let Some(p) = &recipe.perspective {
        validate_perspective(p)?;
    }
    if let Some(u) = &recipe.upright {
        validate_upright(u)?;
    }
    if let Some(g) = &recipe.generative_edit {
        validate_generative_edit(g)?;
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
        if !c.blending.is_finite() || !(0.0..=1.0).contains(&c.blending) {
            return Err(CoreError::InvalidAdjustment {
                name: "color_grading.blending".into(),
                value: c.blending as f64,
                minimum: 0.0,
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
            if !range.luminance.is_finite() || !(-1.0..=1.0).contains(&range.luminance) {
                return Err(CoreError::InvalidAdjustment {
                    name: format!("color_grading.{name}.luminance"),
                    value: range.luminance as f64,
                    minimum: -1.0,
                    maximum: 1.0,
                });
            }
        }
    }
    if let Some(p) = &recipe.point_color {
        if p.version != 1 {
            return Err(CoreError::InvalidAdjustment {
                name: "point_color.version".into(),
                value: p.version as f64,
                minimum: 1.0,
                maximum: 1.0,
            });
        }
        if p.entries.len() > 8 {
            return Err(CoreError::InvalidAdjustment {
                name: "point_color.entries".into(),
                value: p.entries.len() as f64,
                minimum: 0.0,
                maximum: 8.0,
            });
        }
        let mut seen = std::collections::HashSet::new();
        for entry in &p.entries {
            if entry.id.is_empty() || !seen.insert(entry.id.clone()) {
                return Err(CoreError::UnsupportedAdjustment {
                    key: format!("point_color entry id `{}` (empty or duplicate)", entry.id),
                });
            }
            for (field, value, lo, hi) in [
                ("hue_center", entry.hue_center, 0.0_f32, 360.0_f32),
                ("hue_range", entry.hue_range, 0.0_f32, 180.0_f32),
                ("hue_shift", entry.hue_shift, -1.0_f32, 1.0_f32),
                (
                    "saturation_shift",
                    entry.saturation_shift,
                    -1.0_f32,
                    1.0_f32,
                ),
                ("luminance_shift", entry.luminance_shift, -1.0_f32, 1.0_f32),
            ] {
                if !value.is_finite() || !(lo..=hi).contains(&value) {
                    return Err(CoreError::InvalidAdjustment {
                        name: format!("point_color.{}.{}", entry.id, field),
                        value: value as f64,
                        minimum: lo as f64,
                        maximum: hi as f64,
                    });
                }
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
    // LRPAR-G14-DENOISE-IMPL-20: `denoise_ai` is validated against the sidecar
    // contract so a directly constructed (not sidecar-loaded) recipe is
    // rejected loudly instead of rendering an invalid stage.
    if let Some(d) = &recipe.denoise_ai {
        d.validate().map_err(|error| CoreError::Denoise {
            status: "invalid".into(),
            reason: error.to_string(),
        })?;
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
    // LRPAR-G14-REDEYE-15: out-of-range or non-finite values are rejected
    // loudly, never clipped; regions are identified by stable unique ids.
    if let Some(r) = &recipe.red_eye {
        if r.version != 1 {
            return Err(CoreError::InvalidAdjustment {
                name: "red_eye.version".into(),
                value: r.version as f64,
                minimum: 1.0,
                maximum: 1.0,
            });
        }
        if r.regions.len() > lumina_sidecar::RED_EYE_MAX_REGIONS {
            return Err(CoreError::InvalidAdjustment {
                name: "red_eye.regions".into(),
                value: r.regions.len() as f64,
                minimum: 0.0,
                maximum: lumina_sidecar::RED_EYE_MAX_REGIONS as f64,
            });
        }
        let mut seen = std::collections::HashSet::new();
        for region in &r.regions {
            if region.id.is_empty() || !seen.insert(region.id.clone()) {
                return Err(CoreError::UnsupportedAdjustment {
                    key: format!("red_eye region id `{}` (empty or duplicate)", region.id),
                });
            }
            for (field, value, lo, hi) in [
                ("x", region.x, 0.0_f32, 1.0_f32),
                ("y", region.y, 0.0_f32, 1.0_f32),
                ("desaturate", region.desaturate, 0.0_f32, 1.0_f32),
                ("darken", region.darken, 0.0_f32, 1.0_f32),
            ] {
                if !value.is_finite() || !(lo..=hi).contains(&value) {
                    return Err(CoreError::InvalidAdjustment {
                        name: format!("red_eye.{}.{}", region.id, field),
                        value: value as f64,
                        minimum: lo as f64,
                        maximum: hi as f64,
                    });
                }
            }
            if !region.radius.is_finite() || region.radius <= 0.0 || region.radius > 1.0 {
                return Err(CoreError::InvalidAdjustment {
                    name: format!("red_eye.{}.radius", region.id),
                    value: region.radius as f64,
                    minimum: f32::MIN_POSITIVE as f64,
                    maximum: 1.0,
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

/// F-096: Y is filtered with a 5x5 bilateral kernel
/// `exp(-d²/(2*1.5²))*exp(-(Y-Yn)²/(2*0.12²))`; chroma offsets (R-Y,B-Y)
/// use the same 5x5 spatial window with sigma 2.0 and no similarity term.
/// Strength linearly mixes the source and filtered value. Edges replicate.
///
/// Every number lives in [`detail_stages`], which the mask-local P1.2d chain
/// shares verbatim; this wrapper only keeps the global kernel's `u8`
/// quantization points, so extracting the maths changes **no** global byte.
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
    let plane = detail_stages::Rgba8Plane {
        pixels: &src,
        width: w,
        height: h,
    };
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let out = detail_stages::noise_reduction_write(&plane, x, y, n);
            for (c, value) in [out.red, out.green, out.blue].into_iter().enumerate() {
                pixels[i + c] = value.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// LRPAR-G14-REDEYE-15 (Release 1.5): deterministic red-eye correction.
///
/// Each region is a normalized pupil center (`x * width`, `y * height`) with
/// a pixel radius of `radius * min(width, height)`. A pixel's correction
/// weight is its red-dominance
/// (`clamp((R - max(G, B)) / max(R, ε), 0, 1)`, so grey and non-red pixels
/// are untouched) times a spatial falloff (full strength inside 75 % of the
/// radius, linear falloff to the edge). Desaturation pulls the red channel
/// toward the Rec.709 luminance; darkening scales all three channels. Alpha
/// is preserved; results are rounded and clipped to `0..=255`. An empty
/// region list (or only zero strengths) is identity. Pure, platform-neutral
/// arithmetic: no FS/IO, no randomness — identical inputs render
/// byte-identical outputs.
fn apply_red_eye(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    red_eye: &lumina_sidecar::RedEyeCorrection,
) {
    if red_eye.regions.is_empty() {
        return;
    }
    let w = width as usize;
    let h = height as usize;
    if w == 0 || h == 0 {
        return;
    }
    if red_eye
        .regions
        .iter()
        .all(|r| r.desaturate == 0.0 && r.darken == 0.0)
    {
        return;
    }
    let min_dim = w.min(h) as f32;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let r = pixels[i] as f32 / 255.0;
            let g = pixels[i + 1] as f32 / 255.0;
            let b = pixels[i + 2] as f32 / 255.0;
            let redness = ((r - g.max(b)) / r.max(1e-3)).clamp(0.0, 1.0);
            if redness <= 0.0 {
                continue;
            }
            let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let (mut out_r, mut out_g, mut out_b) = (r, g, b);
            for region in &red_eye.regions {
                let cx = region.x * w as f32;
                let cy = region.y * h as f32;
                let radius_px = region.radius * min_dim;
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let dist = dx.hypot(dy);
                if dist > radius_px {
                    continue;
                }
                // Full strength inside 75 % of the radius, linear falloff to
                // the edge (guarded against a degenerate zero radius, which
                // validation rejects but costs nothing to tolerate here).
                let feather = (radius_px * 0.25).max(1e-6);
                let falloff = ((radius_px - dist) / feather).clamp(0.0, 1.0);
                let desat_k = region.desaturate * falloff * redness;
                out_r += (luminance - out_r) * desat_k;
                let darken_k = region.darken * falloff * redness;
                let factor = 1.0 - darken_k;
                out_r *= factor;
                out_g *= factor;
                out_b *= factor;
            }
            pixels[i] = (out_r.clamp(0.0, 1.0) * 255.0).round() as u8;
            pixels[i + 1] = (out_g.clamp(0.0, 1.0) * 255.0).round() as u8;
            pixels[i + 2] = (out_b.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}

/// F-095: separable Gaussian (three-sigma support, replicate edges) on Rec.709
/// luminance. `r_fine=0.5*r`, `r_coarse=1.5*r` (both >=.5); final detail is
/// `detail*d_fine+(1-detail)*d_coarse`. Masking uses
/// `((1-masking)+masking*clamp(|gx|+|gy| / global_max,0,1))`.
///
/// Every number lives in [`detail_stages`], which the mask-local P1.2d chain
/// shares verbatim — including the global `render_scale` radius formula, which
/// the local block follows. This wrapper only keeps the global kernel's `u8`
/// quantization points, so extracting the maths changes **no** global byte.
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
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| detail_stages::luminance(f32::from(p[0]), f32::from(p[1]), f32::from(p[2])))
        .collect();
    let (fine_radius, coarse_radius) = detail_stages::sharpen_blur_radii(s);
    let fine = detail_stages::gaussian_blur(&lum, w, h, fine_radius, scale);
    let coarse = detail_stages::gaussian_blur(&lum, w, h, coarse_radius, scale);
    let (gradients, max_gradient) = detail_stages::gradient_plane(&lum, w, h);
    for (idx, p) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let detail = detail_stages::sharpen_detail(lum[idx], fine[idx], coarse[idx], s.detail);
        let edge = detail_stages::sharpen_edge_factor(gradients[idx], max_gradient);
        let ratio =
            detail_stages::sharpen_ratio(lum[idx], detail_stages::sharpen_amount(s, edge) * detail);
        for channel in p.iter_mut().take(3) {
            *channel = (f32::from(*channel) * ratio).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// F-090b Point Color: targeted color selection with a free hue center.
/// Each entry weights pixels by a cyclic triangular function of the hue
/// distance to `hue_center` (`1` at the center, linearly to `0` at
/// `hue_range`; `hue_range == 0` matches only the exact center hue) and
/// applies its shifts weighted: hue rotation (`hue_shift * 30°`), additive
/// saturation and luminance. Entries apply sequentially in list order in
/// sRGB-codified HSL; all-zero shifts are identity. Outputs clip to `0..=1`.
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
mod tests;
