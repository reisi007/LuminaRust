//! Shared helpers used across MCP tools: format detection, source identity,
//! recipe hashing, rendering, and fast bilinear downscaling.

use crate::error::{map_core_error, McpError};
use crate::session::ImageState;
use lumina_core::{
    render_frame, BitDepth, ExportOptions, ImageFileFormat, ImageFrame, RenderContext,
};
#[cfg(feature = "gpu")]
use lumina_gpu::GpuContext;
use lumina_raw::RawMetadata;
use lumina_sidecar::{DecodeFingerprint, EditRecipe, GeometryFingerprint, SourceIdentity};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// RAW extensions accepted by [`lumina_raw`].
const RAW_EXTENSIONS: &[&str] = &[
    "arw", "cr2", "cr3", "dng", "nef", "orf", "raf", "rw2", "crw", "pef", "srw", "3fr", "iiq",
    "rwl", "mos", "erf", "kdc", "x3f",
];

/// Raster extensions accepted directly by [`ImageFrame::decode`].
const RASTER_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];

/// Returns `true` if the path points at a RAW file.
pub fn is_raw_path(path: &Path) -> bool {
    extension(path)
        .map(|ext| RAW_EXTENSIONS.contains(&ext.as_str()))
        .unwrap_or(false)
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
}

/// Detects and returns the canonical format string, or `UnsupportedFormat`.
pub fn detect_format(path: &Path) -> Result<String, McpError> {
    let ext = extension(path).unwrap_or_default();
    let supported =
        RAW_EXTENSIONS.contains(&ext.as_str()) || RASTER_EXTENSIONS.contains(&ext.as_str());
    if supported {
        Ok(ext)
    } else {
        Err(McpError::UnsupportedFormat(ext))
    }
}

/// Reads a required string argument from a tool's `arguments` object.
pub fn get_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str, McpError> {
    args.get(key)
        .and_then(|value| value.as_str())
        .ok_or_else(|| McpError::InvalidParams(format!("missing string argument `{key}`")))
}

/// Parses an optional unsigned integer argument strictly.
///
/// The JSON value must be a non-negative integer within `minimum..=maximum`;
/// fractional values, negatives, non-numbers, and values that would only fit
/// after truncation (e.g. `quality: 256` squeezed into a u8) are rejected with
/// a JSON-RPC `InvalidParams` error. An absent field — or an explicit `null` —
/// means "not provided". Schema annotations (`minimum`/`maximum`) are advisory
/// documentation for clients; this server-side check is the authoritative one.
pub fn parse_bounded_uint(
    args: &serde_json::Value,
    key: &str,
    minimum: u64,
    maximum: u64,
) -> Result<Option<u64>, McpError> {
    let Some(value) = args.get(key).filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let parsed = value.as_u64().ok_or_else(|| {
        McpError::InvalidParams(format!(
            "`{key}` must be an integer in {minimum}..={maximum}, got `{value}`"
        ))
    })?;
    if !(minimum..=maximum).contains(&parsed) {
        return Err(McpError::InvalidParams(format!(
            "`{key}` must be an integer in {minimum}..={maximum}, got `{parsed}`"
        )));
    }
    Ok(Some(parsed))
}

/// Validates that `output`'s extension agrees with the requested export
/// format. Writing JPEG bytes into a `.png` file would produce an artifact
/// that lies about its container, so mismatches are rejected loudly instead of
/// silently encoding whatever the extension suggests (same rule as the CLI's
/// `output_format` gate).
pub fn validate_output_extension(output: &Path, format: ImageFileFormat) -> Result<(), McpError> {
    let extension = output
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    match ImageFileFormat::from_extension(extension) {
        Some(derived) if derived == format => Ok(()),
        Some(derived) => Err(McpError::InvalidParams(format!(
            "output extension `.{extension}` encodes {} but format `{}` was requested; \
             align the extension with the format",
            derived.default_extension(),
            format.default_extension()
        ))),
        None => Err(McpError::UnsupportedFormat(format!(
            "unsupported output extension `.{extension}`; use .{}, .jpg/.jpeg or .webp",
            format.default_extension()
        ))),
    }
}

/// Builds a [`SourceIdentity`] for a freshly created sidecar, mirroring the
/// `lumina-cli` import logic.
pub fn build_source_identity(
    path: &Path,
    bytes: &[u8],
    frame: &ImageFrame,
    raw_metadata: Option<&RawMetadata>,
) -> SourceIdentity {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("image")
        .to_string();
    let byte_length = fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(bytes.len() as u64);
    let orientation = raw_metadata.map_or(1u8, |metadata| metadata.orientation);
    let raw_format = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_uppercase();
    SourceIdentity {
        relative_name: name,
        content_hash: format!("blake3:{}", blake3::hash(bytes).to_hex()),
        byte_length,
        modified_at: None,
        raw_format,
        orientation,
        decode_fingerprint: DecodeFingerprint {
            decoder: (if raw_metadata.is_some() {
                "libraw"
            } else {
                "image"
            })
            .into(),
            version: if raw_metadata.is_some() {
                lumina_raw::libraw_decode_version()
            } else {
                env!("CARGO_PKG_VERSION").into()
            },
            parameters: BTreeMap::from([(
                "geometry".into(),
                format!("{}x{}", frame.width, frame.height),
            )]),
            extras: BTreeMap::from([("orientation_applied".into(), "true".into())]),
        },
        geometry_fingerprint: GeometryFingerprint {
            width: frame.width,
            height: frame.height,
            orientation,
            pixel_aspect_ratio: 1.0,
            extras: BTreeMap::new(),
        },
        extras: BTreeMap::new(),
    }
}

/// Deterministic short hash of a recipe, used as the `recipe_hash` returned by
/// the tools. Serialization order is fixed by `BTreeMap`, so equal recipes
/// always hash identically (idempotency guarantee).
pub fn recipe_hash(recipe: &EditRecipe) -> String {
    let json = serde_json::to_string(recipe).unwrap_or_default();
    blake3::hash(json.as_bytes()).to_hex().to_string()
}

/// Renders a virtual copy from the decoded source frame using the shared
/// `render_frame` entry point. No masks or source actions are applied in the
/// MVP (see F-101 architecture boundaries).
///
/// When the `gpu` feature is enabled this prefers the GPU adapter (via
/// [`render_best_effort`]) and falls back to the CPU pipeline when no adapter is
/// available; the chosen backend is logged once at startup.
#[cfg(feature = "gpu")]
pub fn render_copy(
    state: &ImageState,
    copy: &lumina_sidecar::VirtualCopy,
    camera_white_balance: Option<[f32; 4]>,
) -> Result<ImageFrame, McpError> {
    GPU_CTX.with(|cell| {
        let ctx = cell.get_or_init(init_render_backend);
        render_best_effort(
            ctx.as_ref(),
            &state.frame,
            &copy.recipe,
            camera_white_balance,
        )
    })
}

/// Renders a virtual copy from the decoded source frame using the shared
/// `render_frame` entry point. No masks or source actions are applied in the
/// MVP (see F-101 architecture boundaries).
#[cfg(not(feature = "gpu"))]
pub fn render_copy(
    state: &ImageState,
    copy: &lumina_sidecar::VirtualCopy,
    camera_white_balance: Option<[f32; 4]>,
) -> Result<ImageFrame, McpError> {
    let output = render_frame(
        &state.frame,
        &RenderContext {
            recipe: &copy.recipe,
            camera_white_balance,
            source_actions: &[],
            masks: None,
            lensfun: None,
        },
    )
    .map_err(map_core_error)?;
    Ok(output.frame)
}

/// Renders `frame` with `recipe`, preferring the GPU when an adapter is bound,
/// otherwise the full platform-neutral CPU pipeline. Mirrors the `lumina-cli`
/// routing: the GPU bootstrap stub applies the recipe only (mask/WB/source-
/// action stages land with later shader subagents), so the CPU branch keeps the
/// complete pipeline.
#[cfg(feature = "gpu")]
fn render_best_effort(
    ctx: Option<&GpuContext>,
    frame: &ImageFrame,
    recipe: &EditRecipe,
    camera_white_balance: Option<[f32; 4]>,
) -> Result<ImageFrame, McpError> {
    match ctx {
        Some(ctx) if ctx.is_available() => ctx
            .render_with_gpu(frame, recipe)
            .map(|rendered| rendered.to_image_frame())
            .map_err(|error| McpError::Render(error.to_string())),
        _ => {
            let output = render_frame(
                frame,
                &RenderContext {
                    recipe,
                    camera_white_balance,
                    source_actions: &[],
                    masks: None,
                    lensfun: None,
                },
            )
            .map_err(map_core_error)?;
            Ok(output.frame)
        }
    }
}

// Per-thread cache for the [`GpuContext`] (one adapter/device per worker
// thread) and the once-per-process backend-selection log.
#[cfg(feature = "gpu")]
thread_local! {
    static GPU_CTX: std::cell::OnceCell<Option<GpuContext>> = const { std::cell::OnceCell::new() };
}

/// Lazily creates a [`GpuContext`] and logs the backend selection exactly once
/// per process. Returns `None` when GPU init fails or no adapter is available.
#[cfg(feature = "gpu")]
fn init_render_backend() -> Option<GpuContext> {
    use std::sync::OnceLock;
    static LOGGED: OnceLock<()> = OnceLock::new();
    let log_once = |message: &str| {
        if LOGGED.set(()).is_ok() {
            log::info!("{message}");
        }
    };
    match GpuContext::new() {
        Ok(ctx) => {
            if ctx.is_available() {
                if let Some(info) = ctx.adapter_info() {
                    log_once(&format!("render backend: gpu ({info})"));
                } else {
                    log_once("render backend: gpu (unknown adapter)");
                }
            } else {
                log_once("render backend: cpu");
            }
            Some(ctx)
        }
        Err(error) => {
            log_once(&format!("render backend: cpu (gpu init failed: {error})"));
            None
        }
    }
}

/// Maps a textual output-format argument to [`ImageFileFormat`].
pub fn parse_output_format(format: &str) -> Result<ImageFileFormat, McpError> {
    match format.to_ascii_lowercase().as_str() {
        "png" => Ok(ImageFileFormat::Png),
        "jpg" | "jpeg" => Ok(ImageFileFormat::Jpeg),
        "webp" => Ok(ImageFileFormat::WebP),
        other => Err(McpError::UnsupportedFormat(other.to_string())),
    }
}

/// Encodes a frame with the given format and quality.
pub fn encode_with_quality(
    frame: &ImageFrame,
    format: ImageFileFormat,
    quality: u8,
) -> Result<Vec<u8>, McpError> {
    let options = ExportOptions {
        format,
        bit_depth: BitDepth::Eight,
        quality,
        dither: false,
        seed: 0,
    };
    frame.encode_with_options(options).map_err(map_core_error)
}

/// Bilinear downscale of an RGBA8 frame so that the output width does not
/// exceed `max_width`. Aspect ratio is preserved and upscaling never occurs.
/// The operation is fully deterministic (pure math, no randomness), which makes
/// `lumina_preview` reproducible.
pub fn downscale_bilinear(frame: &ImageFrame, max_width: u32) -> ImageFrame {
    let (width, height) = (frame.width, frame.height);
    if width == 0 || height == 0 {
        return frame.clone();
    }
    let new_width = width.min(max_width).max(1);
    let new_height = ((new_width as f64 / width as f64) * height as f64)
        .round()
        .max(1.0) as u32;
    if new_width == width && new_height == height {
        return frame.clone();
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
    ImageFrame::new(new_width, new_height, pixels).expect("downscaled dimensions are consistent")
}
