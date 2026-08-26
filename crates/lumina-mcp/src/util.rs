//! Shared helpers used across MCP tools: format detection, source identity,
//! recipe hashing, rendering, and fast bilinear downscaling.

use crate::error::{map_core_error, McpError};
use crate::session::ImageState;
use lumina_core::{
    render_frame, BitDepth, ExportOptions, ImageFileFormat, ImageFrame, RenderContext,
};
#[cfg(feature = "gpu")]
use lumina_gpu::{log_cpu_routing_once, unsupported_gpu_stages_with_context, GpuContext};
use lumina_raw::RawMetadata;
use lumina_sidecar::{DecodeFingerprint, EditRecipe, GeometryFingerprint, SourceIdentity};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Raster extensions accepted directly by [`ImageFrame::decode`].
const RASTER_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];

/// Returns `true` if the path points at a RAW file. R2-CLI-02: routed through
/// the single-source [`lumina_raw::RAW_EXTENSIONS`] list (`is_raw_extension`)
/// instead of the previous private copy that could drift from it again.
pub fn is_raw_path(path: &Path) -> bool {
    extension(path)
        .map(|ext| lumina_raw::is_raw_extension(&ext))
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
    let supported = lumina_raw::is_raw_extension(&ext) || RASTER_EXTENSIONS.contains(&ext.as_str());
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
/// `lumina-cli` `source_identity` logic exactly (R2-CLI-02): a missing file
/// name and a failing `fs::metadata` are **loud** errors — the previous silent
/// `bytes.len()` fallback could mask a source that changed (or became
/// unreadable) between the byte read and the identity build, which would then
/// bless a sidecar against a wrong `byte_length`.
pub fn build_source_identity(
    path: &Path,
    bytes: &[u8],
    frame: &ImageFrame,
    raw_metadata: Option<&RawMetadata>,
) -> Result<SourceIdentity, McpError> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| McpError::InvalidParams("input must have a file name".into()))?
        .to_string();
    // CLI parity: `byte_length` is the on-disk length from `fs::metadata`.
    // A metadata failure aborts loudly like the CLI's `source_identity`
    // instead of silently falling back to the in-memory length.
    let byte_length = fs::metadata(path)
        .map_err(|error| McpError::FileNotFound(format!("{path:?}: {error}")))?
        .len();
    let orientation = raw_metadata.map_or(1u8, |metadata| metadata.orientation);
    let raw_format = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_uppercase();
    Ok(SourceIdentity {
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
    })
}

/// Deterministic short hash of a recipe, used as the `recipe_hash` returned by
/// the tools. Serialization order is fixed by `BTreeMap`, so equal recipes
/// always hash identically (idempotency guarantee).
pub fn recipe_hash(recipe: &EditRecipe) -> String {
    let json = serde_json::to_string(recipe).unwrap_or_default();
    blake3::hash(json.as_bytes()).to_hex().to_string()
}

/// Reads the file bytes and decodes a source image by extension: RAW via
/// `lumina-raw` (bytes-based, mirroring the CLI's `decode_input`), raster via
/// [`ImageFrame::decode`]. Returns the bytes (needed for the source content
/// hash) together with the decoded frame and optional RAW metadata.
pub fn read_and_decode(
    path: &Path,
) -> Result<(Vec<u8>, ImageFrame, Option<RawMetadata>), McpError> {
    let bytes =
        fs::read(path).map_err(|error| McpError::FileNotFound(format!("{path:?}: {error}")))?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("input.raw");
    let (frame, raw_metadata) = if is_raw_path(path) {
        let image = lumina_raw::decode_bytes(&bytes, name)
            .map_err(|error| McpError::Decode(format!("{error}")))?;
        (image.frame, Some(image.metadata))
    } else {
        let frame =
            ImageFrame::decode(&bytes).map_err(|error| McpError::Decode(format!("{error}")))?;
        (frame, None)
    };
    Ok((bytes, frame, raw_metadata))
}

/// Returns `true` when the path carries an extension accepted by batch
/// collection (R2-CLI-02): raster formats plus EVERY RAW extension from the
/// single-source [`lumina_raw::RAW_EXTENSIONS`] list — the same predicate as
/// the CLI's `has_image_extension`. The previous private 9-extension copy
/// silently skipped RAF/ORF/etc. in `lumina_batch`.
pub fn has_batch_image_extension(path: &Path) -> bool {
    extension(path).is_some_and(|ext| {
        matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp")
            || lumina_raw::is_raw_extension(&ext)
    })
}

/// Returns `true` when the path looks like a Lumina sidecar JSON
/// (`*.lumina.json`) — the exact predicate of the CLI reindex scan.
pub fn is_sidecar_json_path(path: &Path) -> bool {
    path.to_string_lossy().ends_with(".lumina.json")
}

/// Cycle-safe recursive file collection behind `lumina_batch` and
/// `lumina_reindex`, mirroring the CLI's reviewed `collect_tree_files`
/// behavior: the visited set holds canonical directory identities so
/// filesystem cycles terminate instead of overflowing the stack, directory
/// symlinks are never followed, and every directory level is walked in
/// deterministic (sorted) order.
pub fn collect_tree_files<F>(
    path: &Path,
    output: &mut Vec<PathBuf>,
    keep: &F,
) -> Result<(), McpError>
where
    F: Fn(&Path) -> bool,
{
    let mut visited = std::collections::BTreeSet::new();
    collect_tree_files_inner(path, output, &mut visited, keep)
}

fn collect_tree_files_inner<F>(
    path: &Path,
    output: &mut Vec<PathBuf>,
    visited: &mut std::collections::BTreeSet<PathBuf>,
    keep: &F,
) -> Result<(), McpError>
where
    F: Fn(&Path) -> bool,
{
    let identity = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !visited.insert(identity) {
        return Ok(());
    }
    let entries: Vec<std::fs::DirEntry> = fs::read_dir(path)
        .map_err(|error| McpError::FileNotFound(format!("{path:?}: {error}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| McpError::FileNotFound(format!("{path:?}: {error}")))?;
    // Deterministic order regardless of filesystem enumeration order.
    let mut sorted = entries;
    sorted.sort_by_key(|entry| entry.file_name());
    for entry in sorted {
        let entry_path = entry.path();
        // `file_type()` never follows symlinks: symlinked directories are not
        // recursed into, which removes cycles by construction.
        let file_type = entry
            .file_type()
            .map_err(|error| McpError::FileNotFound(format!("{entry_path:?}: {error}")))?;
        if file_type.is_dir() {
            collect_tree_files_inner(&entry_path, output, visited, keep)?;
        } else if keep(&entry_path) && entry_path.is_file() {
            output.push(entry_path);
        }
    }
    Ok(())
}

/// Renders `frame` with `recipe` through the shared `render_frame` entry
/// point — the single choke point used by preview/save/analyze. With the
/// `gpu` feature enabled this prefers the GPU adapter and falls back to the
/// CPU pipeline when none is available (backend logged once per process).
#[cfg(feature = "gpu")]
pub fn render_recipe(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    camera_white_balance: Option<[f32; 4]>,
) -> Result<ImageFrame, McpError> {
    GPU_CTX.with(|cell| {
        let ctx = cell.get_or_init(init_render_backend);
        render_best_effort(ctx.as_ref(), frame, recipe, camera_white_balance)
    })
}

/// CPU-only variant of [`render_recipe`] (see the `gpu` feature).
#[cfg(not(feature = "gpu"))]
pub fn render_recipe(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    camera_white_balance: Option<[f32; 4]>,
) -> Result<ImageFrame, McpError> {
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

/// Renders a virtual copy from the decoded source frame using the shared
/// `render_frame` entry point. No masks or source actions are applied in the
/// MVP (see F-101 architecture boundaries).
pub fn render_copy(
    state: &ImageState,
    copy: &lumina_sidecar::VirtualCopy,
    camera_white_balance: Option<[f32; 4]>,
) -> Result<ImageFrame, McpError> {
    render_recipe(&state.frame, &copy.recipe, camera_white_balance)
}

/// Renders `frame` with `recipe`, preferring the GPU when an adapter is bound,
/// otherwise the full platform-neutral CPU pipeline. Mirrors the `lumina-cli`
/// routing through the shared decision function (R2-MCP-01): recipes with
/// GPU-unsupported stages **and** renders carrying the decoder As-Shot
/// white-balance context route explicitly to the CPU pipeline — logged once
/// per reason set, so identical source + sidecar produce identical pixels
/// across feature sets. The GPU is an accelerator, never a semantic change
/// (Agents.md: no silent fallbacks).
#[cfg(feature = "gpu")]
fn render_best_effort(
    ctx: Option<&GpuContext>,
    frame: &ImageFrame,
    recipe: &EditRecipe,
    camera_white_balance: Option<[f32; 4]>,
) -> Result<ImageFrame, McpError> {
    // Consult the shared routing gate BEFORE entering the GPU path. Recipe
    // stages alone would also be caught inside `render_with_gpu`; checking
    // here additionally covers the render-context half (As-Shot WB) and keeps
    // CLI/MCP byte-for-byte on the same decision function.
    let reasons = unsupported_gpu_stages_with_context(recipe, false, camera_white_balance.as_ref());
    match ctx {
        Some(ctx) if ctx.is_available() && reasons.is_empty() => ctx
            .render_with_gpu(frame, recipe)
            .map(|rendered| rendered.to_image_frame())
            .map_err(|error| McpError::Render(error.to_string())),
        _ => {
            if !reasons.is_empty() {
                log_cpu_routing_once(&reasons, "mcp render");
            }
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

/// Non-destructive output guard (F-101-F1 bulk tools; since Review R2 also
/// `lumina_save`).
///
/// Refuses when `target` resolves onto the source image or one of its Lumina
/// bundle files (`<source>.lumina.json`, `<source>.lumina.zdata`). Two
/// identity checks are combined (defense in depth):
///
/// * **Candidate path equality** — canonical aliases including
///   not-yet-existing targets and symlinks (canonicalization follows them),
///   resolved against the canonical parent for missing paths. This mirrors
///   the CLI's `reject_protected_output`.
/// * **`(dev, inode)` identity** (Unix) — catches hard links between distinct
///   directory entries, which canonicalization cannot see.
///
/// Called before any mutation by `lumina_save`, `lumina_dust_removal` and
/// every bulk write.
pub fn reject_protected_target(source: &Path, target: &Path) -> Result<(), McpError> {
    let target_resolved = resolve_candidate(target).map_err(|error| {
        McpError::Encode(format!(
            "could not resolve output path `{}`: {error}",
            target.display()
        ))
    })?;
    let protected: [(&str, PathBuf); 3] = [
        ("source image", source.to_path_buf()),
        ("sidecar", lumina_sidecar::sidecar_path_for(source)),
        (
            "mask/source-action bundle",
            lumina_sidecar::zdata_path_for(source),
        ),
    ];
    for (kind, path) in protected {
        // Both sides use the candidate convention (existing paths are
        // canonicalized; missing ones resolve against their canonical parent),
        // mirroring the CLI's `reject_protected_output`. A plain
        // canonicalize-the-first-argument comparison would fail on the
        // not-yet-existing sidecar/zdata candidates.
        let path_resolved = resolve_candidate(&path).map_err(|error| {
            McpError::Encode(format!("could not resolve `{}`: {error}", path.display()))
        })?;
        if path_resolved == target_resolved {
            return Err(McpError::Encode(format!(
                "output `{}` would overwrite the {kind} `{}`; refusing (non-destructive guarantee)",
                target.display(),
                path.display()
            )));
        }
        // Hard-link alias: the same underlying file under a different
        // directory entry. A rename over the alias would not touch the
        // protected file's own entry, but writing through it must still be
        // refused loudly so a future non-rename write path can never
        // silently clobber the bundle (REVIEW R2-MCP-02 defense in depth).
        #[cfg(unix)]
        if paths_are_same_file(&path, target).unwrap_or(false) {
            return Err(McpError::Encode(format!(
                "output `{}` is a hard-link alias of the {kind} `{}`; \
                 refusing (non-destructive guarantee)",
                target.display(),
                path.display()
            )));
        }
    }
    Ok(())
}

/// Unix: true when both paths refer to the same underlying file via
/// `(dev, inode)` identity — catches hard links between distinct paths.
#[cfg(unix)]
fn paths_are_same_file(a: &Path, b: &Path) -> std::io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    if !(a.exists() && b.exists()) {
        return Ok(false);
    }
    let (meta_a, meta_b) = (fs::metadata(a)?, fs::metadata(b)?);
    Ok(meta_a.dev() == meta_b.dev() && meta_a.ino() == meta_b.ino())
}

/// Non-unix fallback: no portable inode identity exists; only candidate path
/// equality (checked separately above) applies.
#[cfg(not(unix))]
fn paths_are_same_file(_a: &Path, _b: &Path) -> std::io::Result<bool> {
    Ok(false)
}

/// Resolves `path` to a comparable identity (CLI `resolve_candidate` parity):
/// existing paths are canonicalized, missing ones are resolved against their
/// canonical parent directory.
fn resolve_candidate(path: &Path) -> std::io::Result<PathBuf> {
    if path.exists() {
        fs::canonicalize(path)
    } else {
        let parent = fs::canonicalize(path.parent().unwrap_or_else(|| Path::new(".")))?;
        Ok(parent.join(path.file_name().unwrap_or_default()))
    }
}

/// Guard + atomic write (see [`reject_protected_target`]). Used by
/// `lumina_batch`, `lumina_dust_removal` and — since Review R2 —
/// `lumina_save`.
pub fn write_output_guarded(source: &Path, target: &Path, bytes: &[u8]) -> Result<(), McpError> {
    reject_protected_target(source, target)?;
    lumina_sidecar::write_atomically(target, bytes)
        .map_err(|error| McpError::Encode(format!("could not write `{target:?}`: {error}")))
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

#[cfg(all(test, feature = "gpu"))]
mod routing_tests {
    use super::*;

    fn gradient_frame(width: u32, height: u32) -> ImageFrame {
        let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&[
                    ((x as f32 / width as f32) * 255.0).round() as u8,
                    ((y as f32 / height as f32) * 255.0).round() as u8,
                    160,
                    255,
                ]);
            }
        }
        ImageFrame::new(width, height, pixels).expect("synthetic gradient frame")
    }

    fn cpu_oracle(
        frame: &ImageFrame,
        recipe: &EditRecipe,
        camera_white_balance: Option<[f32; 4]>,
    ) -> ImageFrame {
        render_frame(
            frame,
            &RenderContext {
                recipe,
                camera_white_balance,
                source_actions: &[],
                masks: None,
                lensfun: None,
            },
        )
        .expect("CPU oracle render")
        .frame
    }

    /// Invariant of both routing fixes: the CPU branch stays byte-identical to
    /// the reference pipeline — routing changes never alter CPU pixels
    /// (`Agents.md`: CPU is the oracle).
    #[test]
    fn cpu_branch_matches_render_frame_byte_identically() {
        let frame = gradient_frame(16, 16);
        let wb = [1.7f32, 1.0, 1.3, 1.0];
        let recipe = EditRecipe::default();
        let routed =
            render_best_effort(None, &frame, &recipe, Some(wb)).expect("CPU branch renders");
        assert_eq!(routed.pixels, cpu_oracle(&frame, &recipe, Some(wb)).pixels);
    }

    /// R2-MCP-01: invalid As-Shot gains must fail loudly through the routing
    /// choke point. Before the fix a bound adapter silently ignored the whole
    /// context; now it always CPU-routes into core's validation, which rejects
    /// non-positive gains before any pixel mutation.
    #[test]
    fn invalid_as_shot_gains_fail_loudly_on_every_backend() {
        let frame = gradient_frame(8, 8);
        let bad_wb = [0.0f32, 1.0, 1.0, 1.0];
        let recipe = EditRecipe::default();

        // Without an adapter the CPU branch validates directly …
        assert!(render_best_effort(None, &frame, &recipe, Some(bad_wb)).is_err());

        // … and with an adapter the route must still reach that validation.
        let Ok(ctx) = GpuContext::new() else {
            eprintln!("GPU context init failed - adapter-side assertion skipped");
            return;
        };
        if !ctx.is_available() {
            eprintln!("GPU adapter unavailable - adapter-side assertion skipped");
            return;
        }
        assert!(
            render_best_effort(Some(&ctx), &frame, &recipe, Some(bad_wb)).is_err(),
            "a bound adapter must not bypass As-Shot gain validation"
        );
    }

    /// R2-MCP-01 end-to-end: with an adapter available, carrying a valid
    /// As-Shot context produces exactly the CPU-oracle pixels (the render is
    /// CPU-routed instead of dropping the context on the GPU).
    #[test]
    fn wb_context_reroutes_to_cpu_reference_pixels() {
        let frame = gradient_frame(16, 16);
        let wb = [1.6f32, 1.0, 1.25, 1.0];
        let recipe = EditRecipe::default();
        let expected = cpu_oracle(&frame, &recipe, Some(wb));

        let Ok(ctx) = GpuContext::new() else {
            eprintln!("GPU context init failed - routing assertion skipped");
            return;
        };
        if !ctx.is_available() {
            eprintln!("GPU adapter unavailable - routing assertion skipped");
            return;
        }
        let routed = render_best_effort(Some(&ctx), &frame, &recipe, Some(wb))
            .expect("WB-context render must succeed via the CPU route");
        assert_eq!(
            routed.pixels, expected.pixels,
            "As-Shot context must render through the CPU reference pipeline"
        );
    }
}
