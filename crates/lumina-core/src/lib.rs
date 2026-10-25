//! Small, portable raster MVP shared by the native CLI and GUI.
use lumina_sidecar::EditRecipe;
use thiserror::Error;
pub mod cache;
// The per-pixel colour stages shared by the global kernel and the mask-local
// MASK-LOCAL-P1.2b chain (see the module docs for why they are separate from
// their `u8` wrappers).
pub(crate) mod color_stages;
pub mod crop_max_rect;
mod curve_math;
pub(crate) mod detail_stages;
mod effects;
pub(crate) use curve_math::monotone_curve;
pub mod denoise;
pub mod generative;
pub mod geometry_stage_api;
use geometry_stage_api::{validate_lens, validate_perspective};
pub mod histogram;
mod image_frame;
pub mod lens_blur;
mod lens_geometry;
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
pub use render::mask_evaluation::evaluate_local_mask_layers;
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

/// Zielform des einen Downscale-Pfad des Workspace (DOWNSCALER-UNIFY-71).
///
/// Die beiden Zielformen unterscheiden sich **nur** in der Geometrie-Funktion
/// vor dem gemeinsamen f64-bilinearen Kernel ([`downscale`]); der Kernel selbst
/// ist identisch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownscaleTarget {
    /// Laengste Kante hoechstens `max_dim` (Seitenverhaeltnis erhalten).
    ///
    /// Identisch zur bisherigen `ImageFrame::downscale`-Geometrie, aber in f64
    /// gerechnet (bewusste Aenderung, Qualitaet zuerst; gemessen: 82 von
    /// 1.120.249 reprasentativen `(width, height, max_dim)`-Tripeln rundeten
    /// in f32 anders als in f64 — siehe der Anker-Test
    /// `crates/lumina-core/tests/downscale_anchor.rs`).
    LongEdge(u32),
    /// Breite hoechstens `max_width` (Seitenverhaeltnis erhalten).
    ///
    /// Byte-identisch zur bisherigen `downscale_bilinear`-Geometrie inklusive
    /// der Early-Returns — dieser Zielform ist der Byte-Anker des Tasks.
    MaxWidth(u32),
}

/// Der eine Downscale-Pfad des Workspace.
///
/// Bilinear downscale of an RGBA8 frame onto [`DownscaleTarget`]:
/// `DownscaleTarget::MaxWidth(w)` limits the output **width**, `LongEdge(d)`
/// the **long edge**. Aspect ratio is preserved and upscaling never occurs.
///
/// The operation is fully deterministic (pure math, no randomness), which makes
/// previews and other width-limited renderings reproducible.
///
/// Moved out of `lumina-mcp` (R2-MCP-06) and consolidated with
/// `ImageFrame::downscale` and `lumina-gui`'s nearest-neighbour thumbnails
/// (DOWNSCALER-UNIFY-71): this is platform-neutral image
/// processing and belongs with the rest of the shared pipeline in `lumina-core`
/// for reuse and golden-testing, not in the MCP orchestration layer. It returns
/// a `Result` instead of panicking via `expect` when
/// the derived dimensions are inconsistent (which cannot happen for the exact
/// `new_width * new_height * 4` allocation used here, but the fallible
/// [`ImageFrame::new`] contract is surfaced rather than swallowed).
pub fn downscale(frame: &ImageFrame, target: DownscaleTarget) -> Result<ImageFrame, CoreError> {
    let (width, height) = (frame.width, frame.height);
    if width == 0 || height == 0 {
        return Ok(frame.clone());
    }
    let (new_width, new_height) = match target {
        DownscaleTarget::MaxWidth(max_width) => {
            // Die Casts sind Teil des Byte-Ankers, nicht Kosmetik: ohne sie
            // liefe die Ganzzahldivision `new_width / width` auf 0.
            let new_width = width.min(max_width).max(1);
            let new_height = ((new_width as f64 / width as f64) * height as f64)
                .round()
                .max(1.0) as u32;
            (new_width, new_height)
        }
        DownscaleTarget::LongEdge(max_dim) => {
            let long = width.max(height);
            if max_dim == 0 || long <= max_dim {
                return Ok(frame.clone());
            }
            // f64-Fassung der bisherigen f32-Rechnung (Qualitaet zuerst);
            // `.round()` vor `.max(1.0)` wie in `ImageFrame::downscale`.
            let scale = max_dim as f64 / long as f64;
            let new_width = (width as f64 * scale).round().max(1.0) as u32;
            let new_height = (height as f64 * scale).round().max(1.0) as u32;
            (new_width, new_height)
        }
    };
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

// The canonical `ImageFrame::downscale` delegation stays in this file:
// `crates/lumina-core/tests/downscale_anchor/resampler_gate.rs` pins
// `pub fn downscale(&self,` to `crates/lumina-core/src/lib.rs` next to the
// canonical entry point above. The rest of the inherent API lives in
// `image_frame.rs`.
impl ImageFrame {
    /// PERF-GUI-3: bilinear downscale so the long edge is at most `max_dim`.
    ///
    /// Used by the GUI to render a fast draft preview at viewport resolution
    /// instead of the full (e.g. 45 MP) source while a slider is dragged. The
    /// result is always a multiple of 4 bytes and `image`-decoded identical for
    /// `max_dim >= max(width, height)` (i.e. when no downscaling is needed).
    ///
    /// DOWNSCALER-UNIFY-71: this method is now a thin delegation on the
    /// canonical path [`downscale`]`(self, DownscaleTarget::LongEdge(max_dim))`
    /// — same kernel, same guard (`long <= max_dim` -> unchanged clone), same
    /// f64 geometry. The former in-place f32 resampling loop is gone; no second
    /// resampler remains. The `expect` documents a fact, not a fallback:
    /// [`downscale`] can only fail inside [`ImageFrame::new`], and that
    /// constructor cannot reject the exact `width * height * 4` pixel
    /// allocation this kernel produces (the contract `lib.rs` documents for
    /// [`ImageFrame::new`]).
    pub fn downscale(&self, max_dim: u32) -> ImageFrame {
        downscale(self, DownscaleTarget::LongEdge(max_dim))
            .expect("ImageFrame::new cannot fail for the exact width*height*4 pixel allocation")
    }
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

// Row-major RGBA8 frame: the type, its process-local instance identity and its
// content-based equality live in `frame_identity` (GPU-UPLOAD-IDENTITY-92,
// file-size ratchet §8). The re-export keeps the public
// `lumina_core::ImageFrame` path unchanged.
mod frame_identity;
pub use frame_identity::ImageFrame;

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

// FILE-SIZE-1000-ALL-EXCEPTIONS-100: the inherent `ImageFrame` API, the
// geometry/lens kernels and the pixel effects live in their own modules now.
// These re-exports keep every former `crate::…` / `lumina_core::…` path
// working, so no call site changes. `apply_lens`, `sample` and `EMPTY_LENS`
// are reached only from the `lensfun`-gated test modules (and `sample` only
// from there at all), so the list is allowed to look unused in other builds.
pub use lens_geometry::validate_upright;
#[cfg(feature = "lensfun")]
#[allow(unused_imports)]
pub(crate) use lens_geometry::EMPTY_LENS;
#[allow(unused_imports)]
pub(crate) use lens_geometry::{
    apply_channel_lut_adjustments, apply_lens, crop_frame, crop_rect, flip_horizontal,
    flip_vertical, rotate_frame, sample, validate_generative_edit,
};

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
#[cfg(test)]
mod tests;
