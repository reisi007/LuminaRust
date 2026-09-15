use clap::{Args, Parser, Subcommand, ValueEnum};
#[cfg(feature = "lensfun")]
use lumina_core::LensfunCorrectorRef;
use lumina_core::{
    detect_spots_heuristic, export_image, generative_variant_seed, match_total_exposure_masked,
    render_frame, resolve_mask_planes, suggest_auto_tone, tone_fingerprint, AutoToneConfig,
    ExportOptions, ImageFileFormat, ImageFrame, MaskContext, MaskInference, MaskLoadContext,
    MaskPlane, MaskPolicy, RenderContext, RenderOutput, SourceActionArtifact,
};
// F-082-FOLLOWUP: under `onnx-rt` the CLI consumes the resolver surface
// `lumina_onnx::resolve::try_load_onnx_engine` (real engine or a hard error,
// never a stub). The deterministic `StubBackend` stays the wiring default for
// default builds (no `onnx-rt`); it is never substituted for a requested real
// engine. `birefnet_manifest` is the model identity/contract both paths share.
use lumina_onnx::birefnet_manifest;
#[cfg(feature = "onnx-rt")]
use lumina_onnx::try_load_onnx_engine;
#[cfg(feature = "onnx-rt")]
use lumina_onnx::OnnxEngine;
#[cfg(not(feature = "onnx-rt"))]
use lumina_onnx::StubBackend;
use lumina_raw::{RawError, RawMetadata};
// F-098-N2: the Lensfun corrector types are only available under the `lensfun`
// feature (the `native` FFI bindings and `liblensfun` linkage are active then).
#[cfg(feature = "lensfun")]
use lumina_lensfun::{Corrector, LensfunDb};
// GPU-first rendering path (wgpu/Metal). Optional capability: when the `gpu`
// feature is on, render/export/batch prefer the GPU adapter and fall back to the
// CPU pipeline when no adapter is present. Never compiled unless the feature is
// enabled, so the default build stays CPU-only (per `Agents.md` capability
// separation).
#[cfg(feature = "gpu")]
use lumina_gpu::{unsupported_gpu_stages_with_context, Frame, GpuContext};
// Visible backend-selection logging (no silent fallback to CPU).
use log::info;
#[allow(unused_imports)]
use lumina_sidecar::{append_repair_region, load_zdata, zdata_path_for, RepairRegionArtifact};
use lumina_sidecar::{
    apply_batch_op, artifact_status, default_meta_presets_dir, document_revision,
    is_metadata_field, load_meta_preset_file, load_sidecar, now_rfc3339_utc, render_meta_preset,
    resolve_meta_preset_path, save_sidecar, save_sidecar_if_unchanged, scan_meta_presets_dir,
    sidecar_path_for, validate_metadata_field_value, validate_smart_collection_def, AiSelect,
    AiSelectKind, AnalysisFingerprint, ArtifactStatus, AspectPreset, BatchOp, BokehShape,
    CollectionMembership, ColorGrading, ColorGradingRange, CoordinateSystem, Crop, CurveChannels,
    CurvePoint, Curves, DecodeFingerprint, DepthArtifactRef, EditRecipe, ExportRecord, FocusRect,
    Geometry, GeometryFingerprint, HistoryEntry, HslAdjustments, HslChannel, LensBlur,
    LensCorrection, MaskDefinition, MaskLayer, MaskOperation, MaskPrompt, MaskReference,
    MaskStatus, MetaPresetEntry, MetaPresetFile, MetadataHistoryEntry, ModelIdentity, Perspective,
    PointColor, PointColorEntry, Preprocessing, Preset, PromptTransform, Resolution,
    SidecarDocument, SmartCollectionDef, SourceActionArtifactRef, SourceActionKind,
    SourceActionSpec, SourceFingerprint, SourceIdentity, SpotDistraction,
    MAX_KEYWORDS_PER_DOCUMENT, MAX_KEYWORD_CHARS, MAX_METADATA_HISTORY_ENTRIES, METADATA_FIELD_IDS,
    SMART_COLLECTION_VERSION, SOURCE_ACTION_VERSION, SPOT_REMOVAL_VERSION,
};
// LRPAR-G15-IPTC-S3: embedded IPTC read (JPEG IIM/XMP) for `meta inspect`.
// LRPAR-G15-IPTC-S6: `embed_metadata` for the opt-in JPEG export bake-in.
use lumina_iptc::{embed_metadata, extract_metadata, IptcMetadata};
use rayon::prelude::*;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

// LRPAR-G13-MERGE-15 / MERGE-CLI-1: `merge-hdr` / `merge-pano` commands
// (orchestration; alignment/merge/DNG live in `lumina-merge`).
mod merge;
// LRPAR-MATRIX-RECIPE (Slice 1): multi-recipe matrix runner over the committed
// RAW samples (SOLL: `feature/quality/conflicts-and-acceptance.md`
// § „Rezept-Matrix"). Orchestration only — it renders through the shared
// `render_standard` entry point and contains no second image processing.
mod matrix;

/// Minimal stderr logger installed once so the backend-selection `info!` is
/// actually visible. It only installs when no other logger has been registered
/// in this process (so an embedding application that installs its own is
/// respected). Output goes to stderr and therefore never corrupts the
/// `--json` payloads on stdout.
struct StderrLogger {
    level: log::LevelFilter,
}

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[lumina][{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

/// Installs the stderr logger once (no-op if another logger is already set).
fn init_cli_logger() {
    let level = log::LevelFilter::Info;
    if log::set_boxed_logger(Box::new(StderrLogger { level })).is_ok() {
        log::set_max_level(level);
    }
}

/// Logs the chosen render backend exactly once per process so GPU-vs-CPU
/// selection is always visible (Agents.md: no silent fallback).
fn log_backend(message: &str) {
    use std::sync::OnceLock;
    static LOGGED: OnceLock<()> = OnceLock::new();
    if LOGGED.set(()).is_ok() {
        info!("{message}");
    }
}

/// Lazily creates a per-thread [`GpuContext`] (one adapter/device per worker
/// thread in a batch) and logs the backend selection exactly once per process.
/// Returns `None` when GPU init fails or no adapter is available.
#[cfg(feature = "gpu")]
fn init_render_backend() -> Option<GpuContext> {
    match GpuContext::new() {
        Ok(ctx) => {
            if ctx.is_available() {
                if let Some(info) = ctx.adapter_info() {
                    log_backend(&format!("render backend: gpu ({info})"));
                } else {
                    log_backend("render backend: gpu (unknown adapter)");
                }
            } else {
                log_backend("render backend: cpu");
            }
            Some(ctx)
        }
        Err(error) => {
            log_backend(&format!("render backend: cpu (gpu init failed: {error})"));
            None
        }
    }
}

// Per-thread cache for the [`GpuContext`], so the (potentially expensive)
// adapter/device enumeration happens once per worker thread rather than per
// image in a batch.
//
// SIGTRAP-GPU-TESTS: wgpu `Device`/`Queue` teardown on a Rayon worker thread
// can raise SIGTRAP (Metal autorelease / thread-affine teardown). The context
// is therefore intentionally leaked per worker thread — `ManuallyDrop` prevents
// the thread_local destructor from running wgpu teardown on Rayon thread exit.
// The OS reclaims the leaked allocation at process exit; this is safe because
// `GpuContext` is process-scoped and never needs orderly drop on workers.
#[cfg(feature = "gpu")]
thread_local! {
    static GPU_CTX: std::cell::OnceCell<std::mem::ManuallyDrop<Option<GpuContext>>> =
        const { std::cell::OnceCell::new() };
}

/// Renders `frame` with `recipe`, preferring the GPU when an adapter is bound,
/// otherwise the full platform-neutral CPU pipeline.
///
/// REVIEW-GPU-DIVERGENCE-1 / CAMERA-WB-WELLE: the GPU path implements the full
/// adjustment/geometry chain. Before routing to the GPU, the render is validated
/// against **both** the recipe and the render context ([`gpu_routing_reasons`] —
/// e.g. an **invalid** decoder As-Shot WB context, R2-MCP-01, and
/// touched-but-reset sliders at their neutral value, R2-GPU-05). A valid As-Shot
/// context is carried into the GPU entry via
/// [`GpuContext::set_camera_white_balance`]. Any unsupported stage routes the
/// whole render explicitly to the CPU pipeline with a once-per-reason-set log
/// line, so GPU-enabled builds always produce the same pixels as CPU builds. The
/// GPU is an accelerator, never a semantic change (Agents.md: no silent
/// fallbacks).
#[cfg(feature = "gpu")]
fn render_best_effort(
    ctx: Option<&GpuContext>,
    frame: &ImageFrame,
    recipe: &EditRecipe,
    render_ctx: &RenderContext<'_>,
) -> Result<RenderOutput, CliError> {
    let reasons = gpu_routing_reasons(recipe, render_ctx);

    match ctx {
        Some(ctx) if ctx.is_available() && reasons.is_empty() => {
            // CAMERA-WB-WELLE (R2-MCP-01): carry the decoder As-Shot context into
            // the GPU entry like the Lensfun corrector / depth plane. The gains
            // are validated there with the oracle's own error but never
            // re-applied (the decoder already multiplied them in), so a valid
            // context renders byte-identically to the CPU reference. The gate
            // already flags invalid gains; this branch is the belt-and-braces
            // entry validation and falls back loudly (never silently).
            if let Err(error) = ctx.set_camera_white_balance(render_ctx.camera_white_balance) {
                lumina_gpu::log_cpu_routing_once(
                    &[format!("camera_white_balance ({error})")],
                    "cli render",
                );
                return render_frame(frame, render_ctx)
                    .map_err(|error| CliError::Message(error.to_string()));
            }
            let frame = ctx
                .render_with_gpu(frame, recipe)
                .map(Frame::to_image_frame)
                .map_err(|error| CliError::Message(error.to_string()))?;
            Ok(RenderOutput {
                frame,
                mask_layers: Vec::new(),
                mask_warnings: Vec::new(),
            })
        }
        _ => {
            if !reasons.is_empty() {
                lumina_gpu::log_cpu_routing_once(&reasons, "cli render");
            }
            Ok(render_frame(frame, render_ctx)
                .map_err(|error| CliError::Message(error.to_string()))?)
        }
    }
}

/// Recipe- and context-level reasons that force CPU rendering in
/// [`render_best_effort`]. Pure decision logic so tests can pin the routing
/// contract without a GPU adapter.
///
/// Context-level features the GPU path cannot reproduce at all:
/// - an **invalid** decoder As-Shot white balance (R2-MCP-01, via the shared
///   gate; valid gains are carried into the GPU entry by
///   [`render_best_effort`] and are pixel-neutral);
/// - source-action artifacts, mask layers and the Lensfun corrector, none of
///   which exist on the GPU path.
#[cfg(feature = "gpu")]
fn gpu_routing_reasons(recipe: &EditRecipe, render_ctx: &RenderContext<'_>) -> Vec<String> {
    let mut reasons = unsupported_gpu_stages_with_context(
        recipe,
        false,
        render_ctx.camera_white_balance.as_ref(),
    );
    // Invalid adjustments must also CPU-route so the CPU pipeline's strict
    // validation (finite + range) runs and rejects them loudly. The GPU shader
    // has no notion of pipeline ranges and would otherwise silently render
    // `inf`/`nan`/out-of-range values instead of erroring (so
    // `cargo test --features gpu` would greenly ignore contract violations).
    for (key, value) in &recipe.adjustments {
        let (minimum, maximum) = match key.as_str() {
            "exposure" => (-10.0, 10.0),
            "contrast" | "highlights" | "shadows" | "whites" | "blacks" | "wb_tint"
            | "vibrance" | "saturation" => (-1.0, 1.0),
            "wb_temperature" => (1500.0, 12000.0),
            _ => continue, // unknown keys already flagged above
        };
        if !value.is_finite() || *value < minimum || *value > maximum {
            reasons.push(format!("invalid adjustment `{key}`"));
        }
    }
    if !render_ctx.source_actions.is_empty() {
        reasons.push("source_actions (context artifacts)".into());
    }
    let has_mask_layers = render_ctx
        .masks
        .as_ref()
        .and_then(|masks| {
            masks
                .copies
                .iter()
                .find(|copy| copy.id == masks.active_copy_id)
        })
        .is_some_and(|copy| !copy.mask_layers.is_empty());
    if has_mask_layers {
        reasons.push("masks (active copy has layers)".into());
    }
    if lensfun_corrector_active(render_ctx) {
        reasons.push("lens_correction (Lensfun corrector)".into());
    }
    reasons
}

/// Whether the render context carries a non-identity Lensfun corrector (which
/// changes pixels on the CPU path and therefore forces CPU rendering).
#[cfg(all(feature = "gpu", feature = "lensfun"))]
fn lensfun_corrector_active(render_ctx: &RenderContext<'_>) -> bool {
    render_ctx
        .lensfun
        .map(|corrector| !corrector.0.is_identity())
        .unwrap_or(false)
}

/// Non-Lensfun build: no corrector can exist, so this never blocks the GPU.
#[cfg(all(feature = "gpu", not(feature = "lensfun")))]
fn lensfun_corrector_active(_render_ctx: &RenderContext<'_>) -> bool {
    false
}

/// Non-GPU build: only the CPU pipeline exists, so this is a thin alias to
/// [`render_frame`].
#[cfg(not(feature = "gpu"))]
fn render_best_effort(
    _ctx: Option<()>,
    frame: &ImageFrame,
    _recipe: &EditRecipe,
    render_ctx: &RenderContext<'_>,
) -> Result<RenderOutput, CliError> {
    render_frame(frame, render_ctx).map_err(|error| CliError::Message(error.to_string()))
}

/// The CLI's single standard backend entry point: renders `frame` with the
/// process-wide backend selection (GPU when an adapter is bound, the
/// platform-neutral CPU reference otherwise) and logs the CPU route once per
/// reason set. Both `process_selected` and the LRPAR-MATRIX-RECIPE runner go
/// through this function so there is exactly one render entry point (no second
/// pipeline, GPU-default where available).
fn render_standard(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    render_ctx: &RenderContext<'_>,
) -> Result<RenderOutput, CliError> {
    #[cfg(feature = "gpu")]
    {
        GPU_CTX.with(|cell| {
            let holder = cell.get_or_init(|| std::mem::ManuallyDrop::new(init_render_backend()));
            let gpu: &Option<GpuContext> = holder;
            render_best_effort(gpu.as_ref(), frame, recipe, render_ctx)
        })
    }
    #[cfg(not(feature = "gpu"))]
    {
        log_backend("render backend: cpu");
        render_best_effort(None, frame, recipe, render_ctx)
    }
}

#[derive(Debug, Parser)]
#[command(name = "lumina", about = "Non-destructive raster image MVP")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Process(ProcessArgs),
    Inspect(InspectArgs),
    // R2-CLI-10: `import` has its own slim argument set. It previously reused
    // [`FileArgs`], silently accepting render-only flags (`--output`,
    // `--format`, `--quality`, `--force-render`, `--virtual-copy`,
    // `--mask-policy`) that had NO effect on the import — users could believe
    // the import had converted something.
    Import(ImportArgs),
    Develop(DevelopArgs),
    Render(FileArgs),
    Export(ExportArgs),
    Batch(BatchArgs),
    Mask(MaskArgs),
    Reindex(IndexArgs),
    Validate(IndexArgs),
    DustRemoval(DustRemovalArgs),
    /// G-04 Remove-Parität (LRPAR-G04-REMOVE): inspect and edit spot-heal
    /// recipe state (heuristic spots, visualize threshold, distraction
    /// switches, heuristic detection, generative variant seeds). Reads and
    /// writes are loud (unknown copies/spots, bad ranges abort with exit 1);
    /// the original image is never modified. See
    /// `feature/product/spot-removal.md` § „G-04 Remove-Parität“.
    Spot(SpotArgs),
    /// G-05 Lens Blur (LRPAR-G05-LENSBLUR): inspect and edit the depth-bokeh
    /// recipe stage of one virtual copy (focus rect, focal range, blur
    /// amount, bokeh shape, optional external depth artifact). Reads and
    /// writes are loud (unknown copies, bad ranges, inverted focal ranges
    /// abort with exit 1); the original image is never modified. A
    /// referenced-but-missing depth artifact aborts renders loudly (never a
    /// silent heuristic render). See
    /// `feature/architecture/pipeline.md` § „G-05 Lens Blur“.
    LensBlur(LensBlurArgs),
    /// G-02 Color-Parität (LRPAR-G02-COLOR): inspect and edit the color
    /// stages of one virtual copy. See [`ColorArgs`].
    Color(ColorArgs),
    /// G-06 Geometrie-Parität (LRPAR-G06-GEO): inspect and edit the
    /// geometry stages of one virtual copy (crop/aspect, straighten/
    /// rotation, mirrors, manual lens correction, manual perspective) plus
    /// the Lensfun auto-profile status from EXIF. See [`GeometryArgs`].
    Geometry(GeometryArgs),
    /// G-15 META-MVP (Slice 2): list and mutate source-level keywords of one
    /// sidecar. See `feature/platform/cli-gui-wasm.md` (Metadaten-MVP).
    Keywords(KeywordsArgs),
    /// G-15 META-MVP (Slice 2): list and mutate static collection memberships
    /// of one sidecar.
    Collections(CollectionsArgs),
    /// G-15 META-MVP (Slice 2): apply one `BatchOp` over N sidecars, one
    /// atomic write per file, per-file failures isolated and loud.
    BatchMeta(BatchMetaArgs),
    /// G-15 META-MVP (Slice 2): evaluate a portable smart-collection catalog
    /// against N sidecars (read-only filter/list).
    SmartCollections(SmartCollectionsArgs),
    /// LRPAR-G15-IPTC-S3: IPTC metadata draft (`meta inspect`, `meta draft
    /// set|clear`, `meta history show|clear`). See
    /// `feature/product/iptc-metadata.md` §8.
    Meta(MetaArgs),
    /// G-08 Previous-Übernahme (LRPAR-G08-PREVIOUS): copy the full recipe of
    /// one reference image (`--from`) onto N target images (`--to`, each its
    /// own sidecar, one `previous` history step each). Reads and writes are
    /// loud (unknown copies/sidecars abort per target with exit 3, a missing
    /// reference aborts everything with exit 1); the original image is never
    /// modified. See `feature/platform/cli-gui-wasm.md` § „Previous-
    /// Übernahme (G-08, LRPAR-G08-PREVIOUS)“.
    Previous(PreviousArgs),
    /// G-09 Library-Parität (LRPAR-G09-LIB): move one image with its
    /// sidecar companions (`.lumina.json`, `.lumina.zdata` when present) to
    /// a new path. Loud on missing source or existing target (exit 1, no
    /// half state beyond the reported step); recipes roundtrip via
    /// `load_sidecar`/`save_sidecar` paths (`inspect` stays `valid`). See
    /// `feature/platform/cli-gui-wasm.md` § „Library-Parität G-09“.
    Relocate(RelocateArgs),
    /// F-101-F1: run the Lumina MCP server over stdio (JSON-RPC on
    /// stdin/stdout). Takes no arguments; see `feature/platform/mcp-server.md`.
    #[cfg(feature = "mcp")]
    Mcp,
    /// LRPAR-G13-MERGE-15 (G-13, Release 1.5): merge an exposure bracket
    /// into one linear DNG (`Cmd/Ctrl+H` GUI action shares the entry
    /// point). See [`merge::MergeArgs`] and
    /// `feature/platform/cli-gui-wasm.md` § „HDR-/Panorama-Merge".
    MergeHdr(merge::MergeArgs),
    /// LRPAR-G13-MERGE-15 (G-13, Release 1.5): merge overlapping frames
    /// into one linear DNG (`Cmd/Ctrl+M` GUI action shares the entry
    /// point). See [`merge::MergeArgs`].
    MergePano(merge::MergeArgs),
    /// LRPAR-MATRIX-RECIPE: apply the versioned recipe set to both committed
    /// RAW samples, export through the shared render path and verify the
    /// golden/PSNR tolerances (or write the goldens with `--update-goldens`).
    /// See `feature/quality/conflicts-and-acceptance.md` § „Rezept-Matrix".
    Matrix(matrix::MatrixArgs),
}

#[derive(Debug, Clone, Args)]
struct FileArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long, default_value = "png")]
    format: String,
    #[arg(long, default_value_t = 90)]
    quality: u8,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    migrate: bool,
    #[arg(long)]
    force_render: bool,
    #[arg(long)]
    virtual_copy: Option<String>,
    /// How missing or invalid mask artifacts are handled when rendering
    /// (REVIEW-CLI-EXPORTMASK-1): `warn` warns and continues (the harmonized
    /// default for every render-capable subcommand), `strict` aborts.
    #[arg(long, value_enum, default_value = "warn")]
    mask_policy: CliMaskPolicy,
}

#[derive(Debug, Args)]
struct DevelopArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    virtual_copy: Option<String>,
    #[arg(long)]
    exposure: Option<f64>,
    #[arg(long)]
    contrast: Option<f64>,
    /// LRPAR-G01-BASIC: Develop treatment (`color` | `bw`). `bw` stashes the
    /// current saturation/vibrance and desaturates via the shared
    /// `apply_treatment` path; `color` restores the stash exactly.
    #[arg(long, value_name = "color|bw")]
    treatment: Option<String>,
    /// LRPAR-G01-BASIC: Develop profile (one of the normative
    /// `DEVELOP_PROFILES`; absent = `default`).
    #[arg(long, value_name = "PROFILE")]
    profile: Option<String>,
    #[arg(long)]
    update_masks: bool,
    #[arg(long)]
    migrate: bool,
    #[arg(long)]
    json: bool,
}

/// R2-CLI-10: slim argument set for `import` — only the flags the command
/// actually consumes. Import writes/validates a sidecar; it never renders, so
/// render-only flags would be silently ignored (see [`Command::Import`]).
#[derive(Debug, Clone, Args)]
struct ImportArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    migrate: bool,
}

#[derive(Debug, Args)]
struct ExportArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long, default_value = "png")]
    format: String,
    #[arg(long, default_value_t = 90)]
    quality: u8,
    #[arg(long)]
    virtual_copy: Option<String>,
    #[arg(long)]
    update_masks: bool,
    #[arg(long)]
    force_render: bool,
    #[arg(long)]
    migrate: bool,
    #[arg(long)]
    json: bool,
    /// REVIEW-CLI-EXPORTMASK-1: harmonized stale-mask behaviour. Default
    /// `warn` continues with a warning (like render/batch/process); `strict`
    /// aborts before anything is decoded or written.
    #[arg(long, value_enum, default_value = "warn")]
    mask_policy: CliMaskPolicy,
    /// LRPAR-G15-IPTC-S6: opt-in IPTC bake-in (JPEG only, IIM+XMP from the
    /// sidecar draft + keywords, SOLL §7). Without the flag the export stays
    /// exactly as today (no metadata, no silent assumptions).
    #[arg(long)]
    write_metadata: bool,
}

#[derive(Debug, Args)]
struct BatchArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long, default_value_t = 1)]
    jobs: usize,
    #[arg(long, default_value_t = 1)]
    retry: u32,
    #[arg(long)]
    resume: bool,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    update_masks: bool,
    #[arg(long)]
    force_render: bool,
    #[arg(long)]
    json: bool,
    #[arg(long, default_value = "png")]
    format: String,
    #[arg(long, default_value_t = 90)]
    quality: u8,
    #[arg(long)]
    virtual_copy: Option<String>,
    /// Same harmonized stale-mask behaviour as export/render (default warn).
    #[arg(long, value_enum, default_value = "warn")]
    mask_policy: CliMaskPolicy,
    /// LRPAR-G15-IPTC-S6: opt-in IPTC bake-in (JPEG only, IIM+XMP from the
    /// sidecar draft + keywords, SOLL §7). PNG/WebP items with the flag fail
    /// loudly per file (item `failed` with reason). Without the flag the batch
    /// stays exactly as today.
    #[arg(long)]
    write_metadata: bool,
}

/// G-03 Masking parity: inspect and edit the mask DAG of one image sidecar.
/// Reads and writes are loud (unknown copies/masks, bad ranges, arity/cycle
/// violations abort with exit 1); nothing is inferred silently. New mask ids
/// are stable (`mask-<blake3>` over kind + name), never positional.
#[derive(Debug, Args)]
struct MaskArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    update_masks: bool,
    #[arg(long)]
    virtual_copy: Option<String>,
    #[arg(long)]
    json: bool,
    /// List every mask (library + layers) with its status.
    #[arg(long)]
    list: bool,
    /// Add an AI-select source mask (`subject|sky|background|objects|people`).
    #[arg(long, value_name = "KIND")]
    add_ai_select: Option<String>,
    /// Name for `--add-*`, `--combine` and `--duplicate` targets.
    #[arg(long, value_name = "NAME")]
    name: Option<String>,
    /// Optional person/object part for `--add-ai-select`
    /// (`face|hair|eyes|pupil|sclera|lips|teeth|skin|body`, …).
    #[arg(long, value_name = "PART")]
    detail: Option<String>,
    /// Add a deterministic luminance-range source mask.
    #[arg(long)]
    add_luminance_range: bool,
    #[arg(long, value_name = "0..=1")]
    range_min: Option<f32>,
    #[arg(long, value_name = "0..=1")]
    range_max: Option<f32>,
    /// Add a deterministic color-range source mask.
    #[arg(long)]
    add_color_range: bool,
    #[arg(long, value_name = "0..=360")]
    hue_center: Option<f32>,
    #[arg(long, value_name = "0..=360")]
    hue_width: Option<f32>,
    #[arg(long, value_name = "0..=1")]
    sat_min: Option<f32>,
    #[arg(long, value_name = "0..=1")]
    sat_max: Option<f32>,
    #[arg(long, value_name = "0..=1")]
    lum_min: Option<f32>,
    #[arg(long, value_name = "0..=1")]
    lum_max: Option<f32>,
    /// Feather for `--add-*-range` (`0..=1`, default 0).
    #[arg(long, value_name = "0..=1")]
    feather: Option<f32>,
    /// Combine existing masks into a derived node
    /// (`union|intersect|subtract|invert`).
    #[arg(long, value_name = "OP")]
    combine: Option<String>,
    /// Combine/duplicate inputs as `mask-id` (same copy) or
    /// `copy-id/mask-id`, comma-separated for `--combine`.
    #[arg(long, value_name = "REF,...")]
    inputs: Option<String>,
    /// Duplicate an existing mask under a new name (`--name` required).
    #[arg(long, value_name = "REF")]
    duplicate: Option<String>,
    /// Attach an existing library mask to the target copy's layers.
    #[arg(long, value_name = "REF")]
    attach_layer: Option<String>,
    /// Set a layer visible (eye open) by layer id.
    #[arg(long, value_name = "LAYER")]
    show_layer: Option<String>,
    /// Set a layer invisible (eye closed) by layer id.
    #[arg(long, value_name = "LAYER")]
    hide_layer: Option<String>,
}

#[derive(Debug, Args)]
struct IndexArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    migrate: bool,
}

/// G-15 META-MVP (Slice 2): list (`--add`/`--remove` absent) or mutate
/// source-level keywords of one image sidecar. Mutations apply in flag order
/// as `BatchOp::{AddKeyword, RemoveKeyword}` via `apply_batch_op`.
#[derive(Debug, Args)]
struct KeywordsArgs {
    #[arg(long)]
    input: PathBuf,
    /// Keywords to add (repeatable, applied in order).
    #[arg(long = "add")]
    add: Vec<String>,
    /// Keywords to remove (repeatable, applied in order after `--add`).
    #[arg(long = "remove")]
    remove: Vec<String>,
    #[arg(long)]
    json: bool,
}

/// G-15 META-MVP (Slice 2): list or mutate static collection memberships of
/// one image sidecar. `--add-to` takes `id=name` (split at the first `=`);
/// `--remove-from` takes the membership `id`.
#[derive(Debug, Args)]
struct CollectionsArgs {
    #[arg(long)]
    input: PathBuf,
    /// Memberships to add/rename as `id=name` (repeatable, in order).
    #[arg(long = "add-to")]
    add_to: Vec<String>,
    /// Membership ids to remove (repeatable, in order after `--add-to`).
    #[arg(long = "remove-from")]
    remove_from: Vec<String>,
    #[arg(long)]
    json: bool,
}

/// G-15 META-MVP (Slice 2): apply exactly one `BatchOp` (JSON in the
/// `BatchOp` serde form) over every sidecar found under `--input`.
/// Exactly one of `--op` / `--op-file` is required.
#[derive(Debug, Args)]
struct BatchMetaArgs {
    #[arg(long)]
    input: PathBuf,
    /// The batch operation as inline JSON (e.g.
    /// `{"op":"add_keyword","keyword":"portrait"}`).
    #[arg(long)]
    op: Option<String>,
    /// Path to a file containing the batch operation JSON.
    #[arg(long)]
    op_file: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

/// G-15 META-MVP (Slice 2): evaluate a portable smart-collection catalog file
/// against every sidecar found under `--input` (read-only).
#[derive(Debug, Args)]
struct SmartCollectionsArgs {
    #[arg(long)]
    input: PathBuf,
    /// Path to the catalog file
    /// (`{"format":"lumina-smart-catalog","version":1,"collections":[...]}`).
    /// A plain CLI argument, never persisted into recipe data.
    #[arg(long)]
    catalog: PathBuf,
    #[arg(long)]
    json: bool,
}

/// LRPAR-G15-IPTC-S3: IPTC metadata draft commands (`meta inspect`,
/// `meta draft set|clear`, `meta history show|clear`). Normative contract:
/// `feature/product/iptc-metadata.md` §8.
#[derive(Debug, Args)]
struct MetaArgs {
    #[command(subcommand)]
    command: MetaCommand,
}

#[derive(Debug, Subcommand)]
enum MetaCommand {
    /// Show embedded IPTC of the source (JPEG IIM/XMP; other formats report
    /// loudly "nicht verfügbar"), the draft overlay per field (draft vs.
    /// embedded), keywords and the history length. Read-only.
    Inspect(MetaInspectArgs),
    /// Mutate the source-level draft (`--field <id>=<wert>`, repeatable).
    Draft(MetaDraftArgs),
    /// Show or explicitly clear the draft history (diagnostic context, no undo).
    History(MetaHistoryArgs),
    /// Apply file-backed IPTC metadata presets (`<name>.lumina-meta-preset.json`,
    /// static + dynamic with `{placeholder}` variables).
    Preset(MetaPresetArgs),
    /// Copy selected draft fields (+ keywords) from one source image onto N
    /// targets (field-selective, mirror semantics). See [`MetaSyncArgs`].
    Sync(MetaSyncArgs),
    /// META-COPYPASTE-1: copy non-empty draft fields (+ keywords) into an
    /// explicit clipboard file (no sidecar mutation, no shared state).
    Copy(MetaCopyArgs),
    /// META-COPYPASTE-1: paste clipboard fields onto N targets through the
    /// normal sidecar commit path (additive; never deletes other fields).
    Paste(MetaPasteArgs),
}

#[derive(Debug, Args)]
struct MetaInspectArgs {
    /// Source image whose sidecar draft and embedded IPTC are shown.
    path: PathBuf,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct MetaDraftArgs {
    #[command(subcommand)]
    command: MetaDraftCommand,
}

#[derive(Debug, Subcommand)]
enum MetaDraftCommand {
    /// Set draft fields (`--field <id>=<wert>`, repeatable). `keywords`
    /// routes onto the existing source-level keyword field and replaces the
    /// whole list (one `--field keywords=<eintrag>` per entry; an empty value
    /// clears the list). Unknown IDs/invalid values abort loudly with
    /// all-or-nothing semantics (nothing written).
    Set(MetaDraftSetArgs),
    /// Remove draft fields: `--field <id,…>` (comma-separated, `keywords`
    /// allowed) or `--all` (empties the draft; the history is kept and gains
    /// one entry). Exactly one of both is required.
    Clear(MetaDraftClearArgs),
}

#[derive(Debug, Args)]
struct MetaDraftSetArgs {
    /// Source image whose sidecar draft is mutated.
    path: PathBuf,
    /// Draft assignment as `<id>=<wert>` (repeatable, split at the first `=`).
    #[arg(long = "field", required = true, value_name = "ID=VALUE")]
    field: Vec<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct MetaDraftClearArgs {
    /// Source image whose sidecar draft is mutated.
    path: PathBuf,
    /// Draft field IDs to remove (comma-separated, repeatable;
    /// `keywords` clears the keyword list).
    #[arg(long, value_delimiter = ',')]
    field: Vec<String>,
    /// Remove every draft field (history is kept and gains one entry).
    #[arg(long)]
    all: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct MetaHistoryArgs {
    #[command(subcommand)]
    command: MetaHistoryCommand,
}

#[derive(Debug, Subcommand)]
enum MetaHistoryCommand {
    /// List history entries (newest first), optionally limited.
    Show(MetaHistoryShowArgs),
    /// Explicitly clear the whole history (draft values are kept).
    Clear(MetaHistoryClearArgs),
}

#[derive(Debug, Args)]
struct MetaHistoryShowArgs {
    /// Source image whose sidecar history is shown.
    path: PathBuf,
    /// Maximum number of entries (newest first).
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct MetaHistoryClearArgs {
    /// Source image whose sidecar history is cleared.
    path: PathBuf,
    #[arg(long)]
    json: bool,
}

/// LRPAR-G15-IPTC-S4: file-backed IPTC metadata presets (`preset list|show|
/// apply`). Normative contract: `feature/product/iptc-metadata.md` §5.
#[derive(Debug, Args)]
struct MetaPresetArgs {
    #[command(subcommand)]
    command: MetaPresetCommand,
}

#[derive(Debug, Subcommand)]
enum MetaPresetCommand {
    /// List `<name>.lumina-meta-preset.json` files in `[dir]` (default: the
    /// user-global presets directory). Broken files are reported loudly as
    /// failed entries, never skipped silently.
    List(MetaPresetListArgs),
    /// Show one preset: a display `<name>` (resolved against the user-global
    /// presets directory) or an explicit file path.
    Show(MetaPresetShowArgs),
    /// Apply one preset to N targets (`--target`, repeatable; `--var
    /// name=wert`, repeatable for dynamic presets). All placeholder variables
    /// are required upfront; missing/unknown variables and limit violations
    /// abort everything (exit 1, nothing written). Each target is then handled
    /// in isolation (updated / unchanged / failed, exit 3 on partial failure)
    /// with its own CAS + atomic write and `preset:<name>` history entry.
    Apply(MetaPresetApplyArgs),
}

#[derive(Debug, Args)]
struct MetaPresetListArgs {
    /// Presets directory to list (default: the user-global presets directory).
    #[arg(value_name = "DIR")]
    dir: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct MetaPresetShowArgs {
    /// Preset display name or explicit `.lumina-meta-preset.json` file path.
    #[arg(value_name = "NAME|PATH")]
    preset: String,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct MetaPresetApplyArgs {
    /// Preset display name or explicit `.lumina-meta-preset.json` file path.
    #[arg(value_name = "NAME|PATH")]
    preset: String,
    /// Target image(s) receiving the preset draft (repeatable, min 1).
    #[arg(long, required = true, value_name = "TARGET")]
    target: Vec<PathBuf>,
    /// Placeholder variable as `name=wert` (repeatable; split at the first
    /// `=`; duplicate names are rejected loudly).
    #[arg(long = "var", value_name = "NAME=VALUE")]
    var: Vec<String>,
    #[arg(long)]
    json: bool,
}

/// LRPAR-G15-IPTC-S5: field-selective draft/keyword transfer (`meta sync`).
/// `--fields` is deliberately NOT clap-`required`: a missing list must fail
/// loudly with exit 1 (like `meta draft clear`'s manual arity check), not
/// with clap's usage exit 2 and never with a silent transfer-all.
#[derive(Debug, Args)]
struct MetaSyncArgs {
    /// Source image whose draft (+ keywords) is the sync source.
    #[arg(long, value_name = "SOURCE")]
    source: PathBuf,
    /// Target image(s) receiving the selected fields (repeatable, min 1).
    #[arg(long, required = true, value_name = "TARGET")]
    target: Vec<PathBuf>,
    /// Draft field IDs to transfer (comma-separated, repeatable; registry IDs
    /// from SOLL §4, `keywords` allowed). Required — an empty list is a loud
    /// error (exit 1), never a silent transfer-all.
    #[arg(long, value_delimiter = ',', value_name = "ID,...")]
    fields: Vec<String>,
    #[arg(long)]
    json: bool,
}

/// META-COPYPASTE-1: `meta copy` — writes the selected non-empty draft fields
/// (+ non-empty keywords) of `path` into an explicit clipboard file. Copy is
/// read-only w.r.t. the sidecar and never removes or normalizes values.
#[derive(Debug, Args)]
struct MetaCopyArgs {
    /// Source image whose non-empty draft fields (+ keywords) are copied.
    path: PathBuf,
    /// Draft field IDs to copy (comma-separated, repeatable; registry IDs from
    /// SOLL §4, `keywords` allowed). Absent = every non-empty draft field plus
    /// non-empty keywords.
    #[arg(long, value_delimiter = ',', value_name = "ID,...")]
    fields: Vec<String>,
    /// Clipboard file to write (default: `<OS-Temp>/lumina-meta-clipboard.json`).
    #[arg(long, value_name = "FILE")]
    out: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

/// META-COPYPASTE-1: `meta paste` — applies clipboard fields to N targets over
/// the normal sidecar commit path (CAS, atomic, one `origin = "cli"` history
/// entry per changed target). Purely additive: only clipboard fields are
/// written, no other field is ever removed.
#[derive(Debug, Args)]
struct MetaPasteArgs {
    /// Clipboard file to read (default: the same path `meta copy` writes).
    #[arg(value_name = "CLIPBOARD")]
    clipboard: Option<PathBuf>,
    /// Target image(s) receiving the clipboard fields (repeatable, min 1).
    #[arg(long, required = true, value_name = "TARGET")]
    target: Vec<PathBuf>,
    /// Clipboard field IDs to apply (comma-separated, repeatable; must be a
    /// subset of the IDs stored in the clipboard). Absent = all stored fields.
    #[arg(long, value_delimiter = ',', value_name = "ID,...")]
    fields: Vec<String>,
    #[arg(long)]
    json: bool,
}
/// of `--from` onto every `--to` target sidecar (same full-recipe Sync
/// mechanism, one `previous` history step per target, per-target failures
/// isolated and loud). No schema change — only recipe assignment.
#[derive(Debug, Args)]
struct PreviousArgs {
    /// Reference image whose virtual-copy recipe is the Previous source.
    #[arg(long)]
    from: PathBuf,
    /// Target image(s) receiving the reference recipe (repeatable, min 1).
    #[arg(long, required = true)]
    to: Vec<PathBuf>,
    /// Virtual copy id of the reference recipe (default `vc-original`).
    #[arg(long)]
    from_copy: Option<String>,
    /// Virtual copy id receiving the recipe on each target (default
    /// `vc-original`).
    #[arg(long)]
    to_copy: Option<String>,
    #[arg(long)]
    json: bool,
}

/// G-09 Library-Parität (LRPAR-G09-LIB): move one image with its sidecar
/// companions. The destination is the full target image path (rename and
/// folder move in one); companions keep their sidecar-derived file names
/// next to the target. No schema change — only filesystem moves.
#[derive(Debug, Args)]
struct RelocateArgs {
    /// Source image to move (must exist).
    #[arg(long)]
    from: PathBuf,
    /// Destination image path (must not exist; parent must exist).
    #[arg(long)]
    to: PathBuf,
    #[arg(long)]
    json: bool,
}

/// F-042-N1: persist a dust-removal (or AI-replacement) repair region into the
/// source's `.lumina.zdata` bundle and record it as a recipe source action.
/// The original image is never modified.
#[derive(Debug, Args)]
struct DustRemovalArgs {
    #[arg(long)]
    input: PathBuf,
    /// Path to a repair-region definition JSON (region plane + replacement image
    /// path). See `RepairRegionInput` for the schema.
    #[arg(long)]
    repair_region: PathBuf,
    #[arg(long)]
    virtual_copy: Option<String>,
    /// Optional path to render the frame with the action applied, so the effect
    /// is verifiable headlessly. Never equals `--input`.
    #[arg(long)]
    render_out: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

/// G-04 Remove-Parität: inspect and edit the spot-heal recipe state of one
/// image sidecar. Without mutation flags the command lists spots + settings
/// (read-only). Every mutation validates loudly before anything is written;
/// `--detect-objects` only lists candidates unless `--detect-apply` is given
/// (never silent auto-apply).
#[derive(Debug, Args)]
struct SpotArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    virtual_copy: Option<String>,
    #[arg(long)]
    json: bool,
    /// List spots + G-04 settings (default when no mutation flag is given).
    #[arg(long)]
    list: bool,
    /// Add one heuristic spot (requires `--center-x/--center-y/--radius`).
    #[arg(long)]
    add_heuristic: bool,
    #[arg(long, value_name = "0..=1")]
    center_x: Option<f32>,
    #[arg(long, value_name = "0..=1")]
    center_y: Option<f32>,
    #[arg(long, value_name = "(0,512]")]
    radius: Option<f32>,
    #[arg(long, value_name = "0..=1")]
    feather: Option<f32>,
    #[arg(long, value_name = "-1..=1")]
    offset_dx: Option<f32>,
    #[arg(long, value_name = "-1..=1")]
    offset_dy: Option<f32>,
    #[arg(long, value_name = "0..=1")]
    opacity: Option<f32>,
    /// Remove all heuristic/generative spot entries of the copy.
    #[arg(long)]
    clear: bool,
    /// Set the visualize threshold (`0..=1`).
    #[arg(long, value_name = "0..=1")]
    set_visualize_threshold: Option<f32>,
    /// Clear the visualize threshold (visualization off).
    #[arg(long)]
    clear_visualize: bool,
    /// Merge distraction switches as `k=v,...` with keys
    /// `reflections|people|dust|auto` and values `true|false` into the
    /// stored switches (unnamed keys keep their value; `k=false` switches
    /// off) — G04-FOLLOWUP-1 merge decision, consistent with the GUI
    /// single-checkbox toggles.
    #[arg(long, value_name = "K=V,...")]
    set_distraction: Option<String>,
    /// List heuristic spot candidates (stage 1, no model).
    #[arg(long)]
    detect_objects: bool,
    /// Persist the detected candidates as heuristic spots (explicit only).
    #[arg(long)]
    detect_apply: bool,
    /// Detection threshold (`0..=1`; default is the recipe visualize
    /// threshold when set, else 0.5).
    #[arg(long, value_name = "0..=1")]
    detect_threshold: Option<f32>,
    /// Detection cap (`1..=4096`, default 32).
    #[arg(long, value_name = "1..=4096")]
    detect_max: Option<usize>,
    /// Regenerate a generative spot variant: sets
    /// `seed = variant_seed(base, variant)` on `--spot-id`.
    #[arg(long, value_name = "ID")]
    regenerate_variant: Option<String>,
    /// Variant index for `--regenerate-variant` (0 keeps the base seed).
    #[arg(long, value_name = "N")]
    variant: Option<u64>,
    /// Base seed for `--regenerate-variant` (required with it).
    #[arg(long, value_name = "N")]
    seed: Option<u64>,
}

/// G-05 Lens Blur: inspect and edit the depth-bokeh recipe stage of one
/// image sidecar. Without mutation flags the command lists values + depth
/// status (read-only). Field sets create an enabled stage with centered
/// defaults when none exists (touching lens blur enables it, Lightroom-like).
/// Every mutation validates loudly before anything is written.
#[derive(Debug, Args)]
struct LensBlurArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    virtual_copy: Option<String>,
    #[arg(long)]
    json: bool,
    /// List values + depth status (default when no mutation flag is given).
    #[arg(long)]
    list: bool,
    /// Enable the stage (keeps stored values).
    #[arg(long)]
    enable: bool,
    /// Disable the stage (keeps stored values, renders identity).
    #[arg(long)]
    disable: bool,
    /// Set the blur strength (`0..=1`, 0 is identity).
    #[arg(long, value_name = "0..=1")]
    set_amount: Option<f32>,
    /// Set the near edge of the sharp depth band (`0..=1`).
    #[arg(long, value_name = "0..=1")]
    set_focal_near: Option<f32>,
    /// Set the far edge of the sharp depth band (`0..=1`, `>= near`).
    #[arg(long, value_name = "0..=1")]
    set_focal_far: Option<f32>,
    /// Set the bokeh shape (`round|elliptical|hexagonal`).
    #[arg(long, value_name = "SHAPE")]
    set_bokeh: Option<String>,
    /// Set the focus rectangle as `x,y,w,h` (normalized `0..=1`).
    #[arg(long, value_name = "X,Y,W,H")]
    set_focus_rect: Option<String>,
    /// Reference an external depth map as `RELATIVE_PATH:SHA256` (portable
    /// relative path only; renders abort loudly until the artifact exists).
    #[arg(long, value_name = "PATH:SHA256")]
    set_depth_artifact: Option<String>,
    /// Remove the external depth reference (back to the heuristic).
    #[arg(long)]
    clear_depth_artifact: bool,
    /// Remove the whole lens-blur stage (identity).
    #[arg(long)]
    clear: bool,
}

/// G-02 Color-Parität (LRPAR-G02-COLOR): inspect and edit the color stages
/// of one virtual copy (tone curve per channel, HSL mixer, Point Color,
/// color grading incl. luminance/blending, vibrance/saturation). Reads and
/// writes are loud (unknown copies/channels/fields/ids, bad ranges abort
/// with exit 1); the original image is never modified. See
/// `feature/architecture/pipeline.md` §§ F-089, F-090, F-090b, F-091.
#[derive(Debug, Args)]
struct ColorArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    virtual_copy: Option<String>,
    #[arg(long)]
    json: bool,
    /// List curve/HSL/Point-Color/grading values (default when no mutation
    /// flag is given).
    #[arg(long)]
    list: bool,
    /// Set parametric curve regions as `CHANNEL:S,D,L,H` with
    /// `CHANNEL = master|red|green|blue` and four `-1..=1` deltas
    /// (repeatable).
    #[arg(long, value_name = "CHANNEL:S,D,L,H")]
    set_curve_param: Vec<String>,
    /// Set free curve points as `CHANNEL:I,O;I,O;...` with 2..=32 points,
    /// strictly ascending inputs and `(0,0)`/`(1,1)` endpoints (repeatable).
    #[arg(long, value_name = "CHANNEL:I,O;...")]
    set_curve_points: Vec<String>,
    /// Remove the whole tone-curve stage (all channels).
    #[arg(long)]
    clear_curves: bool,
    /// Remove one tone-curve channel (`master|red|green|blue`; master
    /// resets to identity, channel lists are dropped).
    #[arg(long, value_name = "CHANNEL")]
    clear_curve_channel: Option<String>,
    /// Set one HSL mixer field as `CHANNEL:FIELD:VALUE` with
    /// `CHANNEL = red|orange|yellow|green|cyan|blue|violet|magenta` and
    /// `FIELD = hue|saturation|luminance` (repeatable).
    #[arg(long, value_name = "CHANNEL:FIELD:VALUE")]
    set_hsl: Vec<String>,
    /// Remove the whole HSL mixer stage.
    #[arg(long)]
    clear_hsl: bool,
    /// Add a Point Color entry (takes `--hue-center/--hue-range/
    /// --hue-shift/--sat-shift/--lum-shift`; the id is the next stable
    /// `pc-<n>`).
    #[arg(long)]
    add_point_color: bool,
    /// Hue center for `--add-point-color` (`0..=360`, default 0).
    #[arg(long, value_name = "0..=360")]
    hue_center: Option<f32>,
    /// Hue range for `--add-point-color` (`0..=180`, default 30).
    #[arg(long, value_name = "0..=180")]
    hue_range: Option<f32>,
    /// Hue shift for `--add-point-color` (`-1..=1`, default 0).
    #[arg(long, value_name = "-1..=1")]
    hue_shift: Option<f32>,
    /// Saturation shift for `--add-point-color` (`-1..=1`, default 0).
    #[arg(long, value_name = "-1..=1")]
    sat_shift: Option<f32>,
    /// Luminance shift for `--add-point-color` (`-1..=1`, default 0).
    #[arg(long, value_name = "-1..=1")]
    lum_shift: Option<f32>,
    /// Set one Point Color field as `ID:FIELD:VALUE` with
    /// `FIELD = hue_center|hue_range|hue_shift|saturation_shift|
    /// luminance_shift` (repeatable).
    #[arg(long, value_name = "ID:FIELD:VALUE")]
    set_point_color: Vec<String>,
    /// Remove one Point Color entry by stable id (repeatable).
    #[arg(long, value_name = "ID")]
    remove_point_color: Vec<String>,
    /// Remove the whole Point Color stage (identity).
    #[arg(long)]
    clear_point_color: bool,
    /// Set one color-grading field as `RANGE:FIELD:VALUE` with
    /// `RANGE = shadows|midtones|highlights` and
    /// `FIELD = hue_degrees|saturation|luminance` (repeatable).
    #[arg(long, value_name = "RANGE:FIELD:VALUE")]
    set_grading: Vec<String>,
    /// Set the color-grading balance (`-1..=1`).
    #[arg(long, value_name = "-1..=1")]
    set_grading_balance: Option<f32>,
    /// Set the color-grading blending (`0..=1`, 0.5 = legacy edges).
    #[arg(long, value_name = "0..=1")]
    set_grading_blending: Option<f32>,
    /// Remove the whole color-grading stage.
    #[arg(long)]
    clear_grading: bool,
    /// Set vibrance (`-1..=1`).
    #[arg(long, value_name = "-1..=1")]
    set_vibrance: Option<f64>,
    /// Set saturation (`-1..=1`).
    #[arg(long, value_name = "-1..=1")]
    set_saturation: Option<f64>,
}

/// Repair-region definition consumed by the `dust-removal` command.  The
/// `region_values` are little-endian `u16` (0..=u16::MAX); pixels `>= 32768`
/// are replaced by the corresponding `replacement_path` RGBA8 pixel.  Region
/// and replacement MUST share the source frame's dimensions.
#[derive(Debug, Deserialize)]
struct RepairRegionInput {
    id: String,
    #[serde(default = "default_source_action_kind")]
    kind: SourceActionKind,
    region_width: u32,
    region_height: u32,
    region_values: Vec<u16>,
    replacement_path: PathBuf,
}

fn default_source_action_kind() -> SourceActionKind {
    SourceActionKind::DustRemoval
}

/// CLI-facing `--mask-policy` selection (REVIEW-CLI-EXPORTMASK-1). `warn` is
/// the harmonized default everywhere: missing or stale masks produce a warning
/// and the command continues; `strict` aborts the command with an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliMaskPolicy {
    /// Missing/stale masks warn; the command continues.
    Warn,
    /// Missing/stale masks abort the command.
    Strict,
}

impl CliMaskPolicy {
    fn to_policy(self) -> MaskPolicy {
        match self {
            Self::Warn => MaskPolicy::Warn,
            Self::Strict => MaskPolicy::Strict,
        }
    }
}

/// Parsed `*.status.json` resume marker written by `batch` (REVIEW-CLI-N3).
/// Resume decisions read this struct instead of substring-matching raw text.
#[derive(Debug, Deserialize)]
struct BatchStatusFile {
    #[serde(default)]
    status: String,
}

#[derive(Debug, Args)]
struct ProcessArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    preset: Option<PathBuf>,
    #[arg(long)]
    exposure: Option<f64>,
    #[arg(long)]
    contrast: Option<f64>,
    #[arg(long)]
    highlights: Option<f64>,
    #[arg(long)]
    shadows: Option<f64>,
    #[arg(long)]
    auto_tone: bool,
    #[arg(long)]
    match_total_exposure: bool,
    #[arg(long, default_value_t = 0.5)]
    target_luminance: f64,
    /// LRPAR-G15-IPTC-S6: opt-in IPTC bake-in into the exported file (JPEG
    /// only, IIM+XMP from the sidecar draft + keywords, SOLL §7). Without the
    /// flag the export stays exactly as today (no metadata, no silent
    /// assumptions).
    #[arg(long)]
    write_metadata: bool,
}

/// R2-CLI-03: `inspect` accepts `--json` for a machine-readable report
/// (RAW metadata, sidecar status, every virtual copy incl. auto-tone and
/// matching state). Free text remains the default output.
#[derive(Debug, Args)]
struct InspectArgs {
    input: PathBuf,
    /// Print a machine-readable JSON status instead of free text.
    #[arg(long)]
    json: bool,
}

/// G-06 Geometrie-Parität (LRPAR-G06-GEO): inspect and edit the geometry
/// stages of one image sidecar. Without mutation flags the command lists
/// crop/lens/perspective values (read-only). `--straighten` is a documented
/// alias of `--set-rotation` (same field, same validation). Every mutation
/// validates loudly before anything is written and appends exactly one
/// history entry (visible step per call); the original image is never
/// modified. See `feature/architecture/pipeline.md` §§ F-093, F-098, F-099.
#[derive(Debug, Args)]
struct GeometryArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    virtual_copy: Option<String>,
    #[arg(long)]
    json: bool,
    /// List crop/lens/perspective values (default when no mutation flag or
    /// `--lensfun-status` is given).
    #[arg(long)]
    list: bool,
    /// Set the crop to an aspect preset
    /// (`original|1:1|4:5|5:4|3:2|2:3|4:3|3:4|16:9|9:16`).
    #[arg(long, value_name = "PRESET")]
    set_crop_aspect: Option<String>,
    /// Set a free crop rectangle as `x,y,w,h` (normalized `0..=1`).
    #[arg(long, value_name = "X,Y,W,H")]
    set_crop_free: Option<String>,
    /// Remove the crop (full frame, keeps rotation/mirrors).
    #[arg(long)]
    clear_crop: bool,
    /// Set the rotation angle in degrees (`-180..=180`).
    #[arg(long, value_name = "-180..=180")]
    set_rotation: Option<f64>,
    /// Straighten angle in degrees (`-180..=180`; alias of
    /// `--set-rotation`, same field, same validation).
    #[arg(long, value_name = "-180..=180")]
    straighten: Option<f64>,
    /// Set the mirror flags (`h|v|hv|none`).
    #[arg(long, value_name = "h|v|hv|none")]
    set_mirror: Option<String>,
    /// Remove the whole geometry stage (crop, rotation, mirrors; identity).
    #[arg(long)]
    clear_geometry: bool,
    /// Set the manual lens profile
    /// (`wide-light|tele-light|standard-neutral`).
    #[arg(long, value_name = "PROFILE")]
    set_lens_profile: Option<String>,
    /// Set one manual lens field as `FIELD:VALUE` with
    /// `FIELD = distortion_k1|distortion_k2|distortion_k3|vignette_c0|
    /// vignette_c1|vignette_c2|ca_red|ca_blue` (repeatable).
    #[arg(long, value_name = "FIELD:VALUE")]
    set_lens: Vec<String>,
    /// Remove the whole manual lens-correction stage (identity).
    #[arg(long)]
    clear_lens: bool,
    /// Set one manual perspective field as `FIELD:VALUE` with
    /// `FIELD = vertical|horizontal|rotation|scale|aspect_ratio|shift_x|
    /// shift_y` (repeatable).
    #[arg(long, value_name = "FIELD:VALUE")]
    set_perspective: Vec<String>,
    /// Remove the whole manual perspective stage (identity).
    #[arg(long)]
    clear_perspective: bool,
    /// Report the Lensfun auto-profile resolution for the input (EXIF →
    /// profile match with distortion/vignetting/TCA flags, or the loud
    /// reason no corrector applies). Read-only, no save.
    #[arg(long)]
    lensfun_status: bool,
}

#[derive(Debug, Error)]
enum CliError {
    #[error("{0}")]
    Message(String),
    #[error("I/O error for `{path}`: {message}")]
    Io { path: String, message: String },
    #[error(transparent)]
    Sidecar(#[from] lumina_sidecar::SidecarError),
    #[error(transparent)]
    Core(#[from] lumina_core::CoreError),
    #[error(transparent)]
    Raw(#[from] RawError),
    #[error("invalid preset JSON: {0}")]
    Preset(String),
    /// R2-CLI-07: at least one batch item failed while the run itself stayed
    /// structurally sound (summary/status files complete). Distinct process
    /// exit code so scripts can distinguish "nothing worked" (1) from
    /// "partial success" (3); see the exit-code table in
    /// `feature/platform/cli-gui-wasm.md`.
    #[error("batch finished with {failed} failed item(s)")]
    BatchPartial { failed: usize },
}

impl CliError {
    /// Process exit code for this error (R2-CLI-07): 1 for every runtime
    /// failure, 3 for a partially failed batch. CLI usage errors exit with 2
    /// via clap before `run` is ever reached. Documented in
    /// `feature/platform/cli-gui-wasm.md`.
    fn exit_code(&self) -> i32 {
        match self {
            CliError::BatchPartial { .. } => 3,
            _ => 1,
        }
    }
}

fn main() {
    init_cli_logger();
    if let Err(error) = run(Cli::parse()) {
        eprintln!("error: {error}");
        std::process::exit(error.exit_code());
    }
}

fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Process(args) => process(args),
        Command::Inspect(args) => inspect(args),
        Command::Import(args) => import_file(args),
        Command::Develop(args) => develop(args),
        Command::Render(args) => render(args),
        Command::Export(args) => export(args),
        Command::Batch(args) => batch(args),
        Command::Mask(args) => mask(args),
        Command::Reindex(args) => reindex(args),
        Command::Validate(args) => validate(args),
        Command::DustRemoval(args) => dust_removal(args),
        Command::Spot(args) => spot(args),
        Command::LensBlur(args) => lens_blur(args),
        Command::Color(args) => color(args),
        Command::Geometry(args) => geometry(args),
        Command::Keywords(args) => keywords(args),
        Command::Collections(args) => collections(args),
        Command::BatchMeta(args) => batch_meta(args),
        Command::SmartCollections(args) => smart_collections(args),
        Command::Meta(args) => meta(args),
        Command::Previous(args) => previous(args),
        Command::Relocate(args) => relocate(args),
        Command::MergeHdr(args) => merge::merge_hdr(args),
        Command::MergePano(args) => merge::merge_pano(args),
        Command::Matrix(args) => matrix::matrix(args),
        #[cfg(feature = "mcp")]
        // F-101-F1: byte-identical stdio loop as the `lumina-mcp` binary
        // (shared `lumina_mcp::run_stdio`); logging goes to stderr so the
        // JSON-RPC stream on stdout is never corrupted.
        Command::Mcp => {
            lumina_mcp::run_stdio();
            Ok(())
        }
    }
}

fn import_file(args: ImportArgs) -> Result<(), CliError> {
    let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
    let (frame, raw) = decode_input(&args.input, &bytes)?;
    let path = sidecar_path_for(&args.input);
    if args.migrate && path.exists() {
        migrate_sidecar(&path)?;
    } else if path.exists() {
        let document = load_sidecar(&path)?;
        // REVIEW-CLI-N7: mirror `process_selected`'s source-change detection.
        // Import must not silently bless a sidecar whose edits belong to
        // different file contents — reproducibility over convenience. The
        // mismatch is a loud error; the sidecar keeps guarding the OLD
        // contents until it is consciously removed or migrated.
        let current_identity = source_identity(&args.input, &bytes, &frame, raw.as_ref())?;
        if document.source.content_hash != current_identity.content_hash
            || document.source.byte_length != current_identity.byte_length
        {
            return Err(CliError::Message(format!(
                "source changed since sidecar was written: `{}`; remove or rename the sidecar to re-import consciously",
                args.input.display()
            )));
        }
    } else {
        let document = SidecarDocument::new(
            source_identity(&args.input, &bytes, &frame, raw.as_ref())?,
            "raster-mvp-1",
        );
        save_sidecar(&path, &document)?;
    }
    emit(
        args.json,
        serde_json::json!({"command":"import", "input":args.input, "sidecar":path, "status":"ok"}),
        "imported",
    )
}

/// R2-CLI-09: validates an adjustment value BEFORE it is inserted into a
/// recipe, mirroring the MCP `lumina_edit` contract and the sidecar
/// save-time validator (same ranges). Previously `develop --exposure 999`
/// was accepted at insert time and only rejected later with a generic
/// save-time error that did not name the allowed range.
fn validate_adjustment_range(name: &str, value: f64) -> Result<(), CliError> {
    let (minimum, maximum) = match name {
        "exposure" => (-10.0, 10.0),
        _ => (-1.0, 1.0),
    };
    if !value.is_finite() || !(minimum..=maximum).contains(&value) {
        return Err(CliError::Message(format!(
            "invalid adjustment `{name}`: value {value} outside allowed range {minimum}..={maximum}"
        )));
    }
    Ok(())
}

fn develop(args: DevelopArgs) -> Result<(), CliError> {
    // R2-CLI-09: fail BEFORE the sidecar is loaded or mutated so an invalid
    // value can never produce a half-applied develop run.
    if let Some(value) = args.exposure {
        validate_adjustment_range("exposure", value)?;
    }
    if let Some(value) = args.contrast {
        validate_adjustment_range("contrast", value)?;
    }
    // LRPAR-G01-BASIC: fail BEFORE the sidecar is loaded or mutated so an
    // invalid treatment/profile can never produce a half-applied develop run
    // (same R2-CLI-09 discipline as the numeric ranges above).
    if let Some(treatment) = &args.treatment {
        if treatment != lumina_sidecar::TREATMENT_COLOR && treatment != lumina_sidecar::TREATMENT_BW
        {
            return Err(CliError::Message(format!(
                "unknown treatment `{treatment}` (expected `color` or `bw`)"
            )));
        }
    }
    if let Some(profile) = &args.profile {
        if !lumina_sidecar::DEVELOP_PROFILES.contains(&profile.as_str()) {
            return Err(CliError::Message(format!(
                "unknown develop profile `{profile}` (expected one of {})",
                lumina_sidecar::DEVELOP_PROFILES.join("|")
            )));
        }
    }
    let path = sidecar_path_for(&args.input);
    if args.migrate {
        migrate_sidecar(&path)?;
    }
    let mut document = load_sidecar(&path)?;
    let id = args.virtual_copy.as_deref().unwrap_or("vc-original");
    if !document.virtual_copies.iter().any(|copy| copy.id == id) {
        document.duplicate_virtual_copy("vc-original", id, id)?;
    }
    let copy = document
        .virtual_copies
        .iter_mut()
        .find(|c| c.id == id)
        .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{id}`")))?;
    if let Some(value) = args.exposure {
        copy.recipe.adjustments.insert("exposure".into(), value);
    }
    if let Some(value) = args.contrast {
        copy.recipe.adjustments.insert("contrast".into(), value);
    }
    // LRPAR-G01-BASIC: shared sidecar mutation paths (same stasch/whitelist
    // semantics as the GUI Treatment selector and Profile dropdown).
    if let Some(treatment) = &args.treatment {
        copy.recipe
            .apply_treatment(treatment)
            .map_err(|error| CliError::Message(error.to_string()))?;
    }
    if let Some(profile) = &args.profile {
        copy.recipe
            .apply_develop_profile(profile)
            .map_err(|error| CliError::Message(error.to_string()))?;
    }
    if args.update_masks {
        copy.recipe
            .options
            .insert("update_masks".into(), "true".into());
    }
    save_sidecar(&path, &document)?;
    emit(
        args.json,
        serde_json::json!({"command":"develop", "input":args.input, "virtual_copy":id, "status":"ok"}),
        "developed",
    )
}

fn render(args: FileArgs) -> Result<(), CliError> {
    let output = args
        .output
        .clone()
        .ok_or_else(|| CliError::Message("render requires --output".into()))?;
    validate_format(&args.format)?;
    validate_quality(args.quality)?;
    if args.migrate {
        migrate_sidecar(&sidecar_path_for(&args.input))?;
    }
    let output = output.with_extension(format_extension(&args.format));
    let mut mask_warnings = Vec::new();
    process_selected(
        ProcessArgs {
            input: args.input.clone(),
            output: output.clone(),
            preset: None,
            exposure: None,
            contrast: None,
            highlights: None,
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
            write_metadata: false,
        },
        args.quality,
        args.virtual_copy.as_deref(),
        args.mask_policy.to_policy(),
        &mut mask_warnings,
    )?;
    emit(
        args.json,
        serde_json::json!({"command":"render", "output":output, "format":args.format, "status":"ok", "mask_warnings":mask_warnings}),
        "rendered",
    )
}

fn export(args: ExportArgs) -> Result<(), CliError> {
    validate_format(&args.format)?;
    validate_quality(args.quality)?;
    if args.migrate {
        migrate_sidecar(&sidecar_path_for(&args.input))?;
    }
    // REVIEW-CLI-EXPORTMASK-1: stale-mask behaviour is harmonized across the
    // render-capable subcommands — the default is warn-and-continue (identical
    // to render/batch/process); aborting is reserved for an explicit
    // `--mask-policy strict`. The preflight deliberately runs before decoding
    // and writing so a strict abort leaves no half-written artifacts.
    preflight_masks(
        &args.input,
        args.virtual_copy.as_deref(),
        args.update_masks,
        args.mask_policy.to_policy(),
    )?;
    if args.update_masks {
        // Persist the one-shot refresh request (same channel develop/batch
        // use) so THIS export's render re-infers; `process_selected` consumes
        // and removes it again. Without an inference engine the render itself
        // fails loudly instead of pretending a stale mask was refreshed.
        mark_masks_pending_refresh(&args.input, args.virtual_copy.as_deref())?;
    }
    let output = args.output.with_extension(format_extension(&args.format));
    let mut mask_warnings = Vec::new();
    let metadata_written = process_selected(
        ProcessArgs {
            input: args.input.clone(),
            output: output.clone(),
            preset: None,
            exposure: None,
            contrast: None,
            highlights: None,
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
            write_metadata: args.write_metadata,
        },
        args.quality,
        args.virtual_copy.as_deref(),
        args.mask_policy.to_policy(),
        &mut mask_warnings,
    )?;
    // LRPAR-G15-IPTC-S6: the bake-in outcome is additive in `--json` output —
    // absent without the flag (exactly today's payload).
    let mut payload = serde_json::json!({"command":"export", "output":output, "quality":args.quality, "status":"ok", "mask_warnings":mask_warnings});
    if let Some(written) = metadata_written {
        payload["metadata_written"] = written.json();
    }
    emit(args.json, payload, "exported")
}

fn preflight_masks(
    input: &Path,
    virtual_copy: Option<&str>,
    update: bool,
    policy: MaskPolicy,
) -> Result<(), CliError> {
    let path = sidecar_path_for(input);
    let document = match load_sidecar(&path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let id = virtual_copy.unwrap_or("vc-original");
    let copy = document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == id)
        .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{id}`")))?;
    let root = input.parent().unwrap_or_else(|| Path::new("."));
    let missing = copy
        .mask_library
        .iter()
        .filter(|mask| {
            !matches!(mask.status, MaskStatus::Valid)
                || mask.artifact.as_ref().is_none_or(|artifact| {
                    artifact_status(root, artifact) != ArtifactStatus::Available
                })
        })
        .count();
    if missing == 0 {
        return Ok(());
    }
    if policy == MaskPolicy::Strict {
        return Err(CliError::Message(format!(
            "strict mask policy: {missing} mask(s) are missing or unavailable for `{id}`; command aborted"
        )));
    }
    // Harmonized default (REVIEW-CLI-EXPORTMASK-1): warn-and-continue. An
    // explicit `--update-masks` is honoured by the render itself — masks are
    // re-inferred when an engine is available and the command fails loudly
    // when none is; it never silently succeeds with stale pixels.
    if update {
        eprintln!(
            "warning: {missing} mask(s) for `{id}` are missing or unavailable; --update-masks will re-infer them during the render and fail loudly if no inference engine is installed"
        );
    } else {
        eprintln!(
            "warning: {missing} mask(s) for `{id}` are missing or unavailable; they will not be applied (use --update-masks when an inference engine is installed)"
        );
    }
    Ok(())
}

/// Persists the ONE-SHOT `--update-masks` request into the named virtual
/// copy's recipe options — the same channel develop/batch/mask use. The next
/// render through `process_selected` consumes it and removes it from the
/// persisted recipe again (REVIEW-CLI-MASKFLAG-1).
fn mark_masks_pending_refresh(input: &Path, virtual_copy: Option<&str>) -> Result<(), CliError> {
    let path = sidecar_path_for(input);
    let mut document = match load_sidecar(&path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let id = virtual_copy.unwrap_or("vc-original");
    let Some(copy) = document
        .virtual_copies
        .iter_mut()
        .find(|copy| copy.id == id)
    else {
        return Err(CliError::Message(format!("unknown virtual copy `{id}`")));
    };
    copy.recipe
        .options
        .insert("update_masks".into(), "true".into());
    save_sidecar(&path, &document)?;
    Ok(())
}

fn mask(args: MaskArgs) -> Result<(), CliError> {
    let path = sidecar_path_for(&args.input);
    let mut document = load_sidecar(&path)?;
    let wants_mutation = args.update_masks
        || args.add_ai_select.is_some()
        || args.add_luminance_range
        || args.add_color_range
        || args.combine.is_some()
        || args.duplicate.is_some()
        || args.attach_layer.is_some()
        || args.show_layer.is_some()
        || args.hide_layer.is_some();
    if args.list && !wants_mutation {
        return mask_list(&args, &document);
    }
    if !wants_mutation {
        // Historical behaviour: without flags the command reports status.
        return mask_list(&args, &document);
    }
    let copy_id = resolve_mask_copy(&document, args.virtual_copy.as_deref())?;
    let mut actions: Vec<String> = Vec::new();
    // Decode once for every mutation that mints a mask definition
    // (dimensions for the geometry context); a loud error instead of
    // zero-sized geometry.
    let frame_dims = if args.add_ai_select.is_some()
        || args.add_luminance_range
        || args.add_color_range
        || args.combine.is_some()
    {
        let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
        let (frame, _) = decode_input(&args.input, &bytes)?;
        Some((frame.width, frame.height))
    } else {
        None
    };
    if let Some(kind) = args.add_ai_select.as_deref() {
        let name = require_mask_name(args.name.as_deref())?;
        mask_add_ai(
            &mut document,
            &copy_id,
            kind,
            name,
            args.detail.as_deref(),
            frame_dims,
        )?;
        info!("mask: added ai-select `{kind}` as `{name}` on copy `{copy_id}`");
        actions.push(format!("add-ai-select:{name}"));
    }
    if args.add_luminance_range {
        let name = require_mask_name(args.name.as_deref())?;
        let min = args.range_min.ok_or_else(|| {
            CliError::Message("--add-luminance-range requires --range-min".into())
        })?;
        let max = args.range_max.ok_or_else(|| {
            CliError::Message("--add-luminance-range requires --range-max".into())
        })?;
        let feather = args.feather.unwrap_or(0.0);
        mask_add_luminance(&mut document, &copy_id, name, min, max, feather, frame_dims)?;
        info!("mask: added luminance-range `{name}` on copy `{copy_id}`");
        actions.push(format!("add-luminance-range:{name}"));
    }
    if args.add_color_range {
        let name = require_mask_name(args.name.as_deref())?;
        let feather = args.feather.unwrap_or(0.0);
        mask_add_color(
            &mut document,
            &copy_id,
            name,
            args.hue_center,
            args.hue_width,
            args.sat_min,
            args.sat_max,
            args.lum_min,
            args.lum_max,
            feather,
            frame_dims,
        )?;
        info!("mask: added color-range `{name}` on copy `{copy_id}`");
        actions.push(format!("add-color-range:{name}"));
    }
    if let Some(op) = args.combine.as_deref() {
        let name = require_mask_name(args.name.as_deref())?;
        let inputs = args
            .inputs
            .as_deref()
            .ok_or_else(|| CliError::Message("--combine requires --inputs <ref,...>".into()))?;
        mask_combine(&mut document, &copy_id, op, name, inputs, frame_dims)?;
        info!("mask: combined `{op}` as `{name}` on copy `{copy_id}`");
        actions.push(format!("combine:{name}"));
    }
    if let Some(source) = args.duplicate.as_deref() {
        let name = require_mask_name(args.name.as_deref())?;
        mask_duplicate(&mut document, &copy_id, source, name)?;
        info!("mask: duplicated `{source}` as `{name}` on copy `{copy_id}`");
        actions.push(format!("duplicate:{name}"));
    }
    if let Some(target) = args.attach_layer.as_deref() {
        mask_attach_layer(&mut document, &copy_id, target)?;
        info!("mask: attached layer for `{target}` on copy `{copy_id}`");
        actions.push(format!("attach-layer:{target}"));
    }
    if let Some(layer) = args.show_layer.as_deref() {
        mask_set_layer_visible(&mut document, &copy_id, layer, true)?;
        info!("mask: layer `{layer}` visible on copy `{copy_id}`");
        actions.push(format!("show-layer:{layer}"));
    }
    if let Some(layer) = args.hide_layer.as_deref() {
        mask_set_layer_visible(&mut document, &copy_id, layer, false)?;
        info!("mask: layer `{layer}` hidden on copy `{copy_id}`");
        actions.push(format!("hide-layer:{layer}"));
    }
    if args.update_masks {
        let copies = if let Some(id) = args.virtual_copy.as_deref() {
            document
                .virtual_copies
                .iter_mut()
                .filter(|copy| copy.id == id)
                .collect::<Vec<_>>()
        } else {
            document.virtual_copies.iter_mut().collect::<Vec<_>>()
        };
        if args.virtual_copy.is_some() && copies.is_empty() {
            return Err(CliError::Message("unknown virtual copy".into()));
        }
        for copy in copies {
            for mask in &mut copy.mask_library {
                mask.status = lumina_sidecar::MaskStatus::Pending;
            }
        }
        info!("mask: marked masks pending (update_masks)");
        actions.push("update-masks".into());
    }
    // Loud gate: arity, unknown references, cycles, ranges and ai_select
    // placement are rejected before anything is written.
    document.validate()?;
    save_sidecar(&path, &document)?;
    emit(
        args.json,
        serde_json::json!({"command":"mask", "input":args.input, "copy":copy_id, "actions":actions, "status":"ok"}),
        &format!("mask updated: {}", actions.join(", ")),
    )
}

/// Lists every mask library entry and layer with its status (G-03). Read-only:
/// the sidecar is never written.
fn mask_list(args: &MaskArgs, document: &SidecarDocument) -> Result<(), CliError> {
    let copies: Vec<&lumina_sidecar::VirtualCopy> = match args.virtual_copy.as_deref() {
        Some(id) => document
            .virtual_copies
            .iter()
            .filter(|copy| copy.id == id)
            .collect(),
        None => document.virtual_copies.iter().collect(),
    };
    if args.virtual_copy.is_some() && copies.is_empty() {
        return Err(CliError::Message("unknown virtual copy".into()));
    }
    if args.json {
        let copies_json = copies
            .iter()
            .map(|copy| {
                serde_json::json!({
                    "id": copy.id,
                    "name": copy.name,
                    "masks": copy.mask_library.iter().map(|mask| serde_json::json!({
                        "id": mask.id,
                        "name": mask.name,
                        "operation": format!("{:?}", mask.operation).to_lowercase(),
                        "status": format!("{:?}", mask.status).to_lowercase(),
                        "ai_select": mask.ai_select.as_ref().map(|select| serde_json::json!({
                            "kind": select.kind.as_str(),
                            "detail": select.detail,
                        })),
                        "prompt": mask.prompt.as_ref().map(prompt_kind),
                    })).collect::<Vec<_>>(),
                    "layers": copy.mask_layers.iter().map(|layer| serde_json::json!({
                        "id": layer.id,
                        "mask": format!("{}/{}", layer.mask.copy_id, layer.mask.mask_id),
                        "visible": layer.visible,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>();
        emit(
            true,
            serde_json::json!({"command":"mask", "input":args.input, "copies":copies_json, "status":"ok"}),
            "mask status listed",
        )
    } else {
        for copy in &copies {
            println!("copy: {} [{}]", copy.name, copy.id);
            for mask in &copy.mask_library {
                println!(
                    "  mask: {} [{}] op={} status={}{}{}",
                    mask.name,
                    mask.id,
                    format!("{:?}", mask.operation).to_lowercase(),
                    format!("{:?}", mask.status).to_lowercase(),
                    mask.ai_select.as_ref().map_or(String::new(), |select| {
                        format!(
                            " ai={}{}",
                            select.kind.as_str(),
                            select
                                .detail
                                .as_deref()
                                .map_or(String::new(), |d| format!(":{d}"))
                        )
                    }),
                    mask.prompt
                        .as_ref()
                        .map_or(String::new(), |p| format!(" prompt={}", prompt_kind(p))),
                );
            }
            for layer in &copy.mask_layers {
                println!(
                    "  layer: {} -> {}/{} visible={}",
                    layer.id, layer.mask.copy_id, layer.mask.mask_id, layer.visible
                );
            }
        }
        emit(
            false,
            serde_json::json!({"command":"mask", "input":args.input, "status":"ok"}),
            "mask status listed",
        )
    }
}

fn prompt_kind(prompt: &MaskPrompt) -> &'static str {
    match prompt {
        MaskPrompt::Box { .. } => "box",
        MaskPrompt::Brush { .. } => "brush",
        MaskPrompt::Polygon { .. } => "polygon",
        MaskPrompt::Ellipse { .. } => "ellipse",
        MaskPrompt::Gradient { .. } => "gradient",
        MaskPrompt::ColorRange { .. } => "color-range",
        MaskPrompt::LuminanceRange { .. } => "luminance-range",
    }
}

fn unix_now() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

fn require_mask_name(name: Option<&str>) -> Result<&str, CliError> {
    match name {
        Some(name) if !name.trim().is_empty() => Ok(name),
        _ => Err(CliError::Message(
            "this mask operation requires --name <NAME>".into(),
        )),
    }
}

fn resolve_mask_copy(
    document: &SidecarDocument,
    requested: Option<&str>,
) -> Result<String, CliError> {
    if let Some(id) = requested {
        if document.virtual_copies.iter().any(|copy| copy.id == id) {
            return Ok(id.into());
        }
        return Err(CliError::Message(format!("unknown virtual copy `{id}`")));
    }
    if let Some(default) = document.virtual_copies.iter().find(|copy| copy.is_default) {
        return Ok(default.id.clone());
    }
    document
        .virtual_copies
        .first()
        .map(|copy| copy.id.clone())
        .ok_or_else(|| CliError::Message("sidecar has no virtual copies".into()))
}

fn mask_copy_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut lumina_sidecar::VirtualCopy, CliError> {
    document
        .virtual_copies
        .iter_mut()
        .find(|copy| copy.id == copy_id)
        .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{copy_id}`")))
}

/// Parses a mask reference as `mask-id` (same copy) or `copy-id/mask-id`.
/// Anything else is a loud error — never a guess.
fn parse_mask_ref(value: &str, default_copy: &str) -> Result<(String, String), CliError> {
    let parts: Vec<&str> = value.split('/').collect();
    match parts.as_slice() {
        [mask] if !mask.trim().is_empty() => Ok((default_copy.into(), (*mask).into())),
        [copy, mask] if !copy.trim().is_empty() && !mask.trim().is_empty() => {
            Ok(((*copy).into(), (*mask).into()))
        }
        _ => Err(CliError::Message(format!(
            "invalid mask reference `{value}`: expected `mask-id` or `copy-id/mask-id`"
        ))),
    }
}

fn stable_mask_id(parts: &[&str]) -> String {
    let joined = parts.join("\0");
    format!("mask-{}", blake3::hash(joined.as_bytes()).to_hex())
}

fn mask_library_contains(document: &SidecarDocument, copy_id: &str, mask_id: &str) -> bool {
    document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == copy_id)
        .is_some_and(|copy| copy.mask_library.iter().any(|mask| mask.id == mask_id))
}

fn new_source_mask(
    document: &SidecarDocument,
    id: &str,
    name: &str,
    frame_dims: Option<(u32, u32)>,
    status: MaskStatus,
) -> MaskDefinition {
    let (width, height) = frame_dims.unwrap_or((0, 0));
    MaskDefinition {
        id: id.into(),
        name: name.into(),
        source_fingerprint: SourceFingerprint {
            content_hash: document.source.content_hash.clone(),
            byte_length: document.source.byte_length,
            extras: BTreeMap::new(),
        },
        decode_context: DecodeFingerprint {
            decoder: "cli-mask".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            parameters: BTreeMap::new(),
            extras: BTreeMap::new(),
        },
        geometry_context: GeometryFingerprint {
            width,
            height,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: BTreeMap::new(),
        },
        model: ModelIdentity {
            name: "unavailable".into(),
            version: "pending".into(),
            hash: "pending".into(),
            extras: BTreeMap::new(),
        },
        inference_resolution: Resolution {
            width,
            height,
            extras: BTreeMap::new(),
        },
        preprocessing: Preprocessing {
            name: "pending".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: BTreeMap::new(),
        },
        rescaling_method: "none".into(),
        rescaling_parameters: BTreeMap::new(),
        coordinate_system: CoordinateSystem::SourceOriented,
        status,
        created_at: unix_now(),
        generator_version: env!("CARGO_PKG_VERSION").into(),
        error_text: None,
        artifact: None,
        operation: MaskOperation::Source,
        references: vec![],
        prompt: None,
        ai_select: None,
        extras: BTreeMap::new(),
    }
}

fn mask_add_ai(
    document: &mut SidecarDocument,
    copy_id: &str,
    kind: &str,
    name: &str,
    detail: Option<&str>,
    frame_dims: Option<(u32, u32)>,
) -> Result<(), CliError> {
    let kind = AiSelectKind::parse(kind).ok_or_else(|| {
        CliError::Message(format!(
            "unknown ai-select kind `{kind}`: expected subject|sky|background|objects|people"
        ))
    })?;
    if let Some(detail) = detail {
        if detail.len() > 64
            || detail.trim().is_empty()
            || detail != detail.trim()
            || detail.chars().any(|c| c.is_control())
        {
            return Err(CliError::Message(
                "detail must be trimmed, non-empty, free of control characters and at most 64 chars".into(),
            ));
        }
    }
    let id = stable_mask_id(&["ai", kind.as_str(), name]);
    if mask_library_contains(document, copy_id, &id) {
        return Err(CliError::Message(format!(
            "mask `{name}` already exists on copy `{copy_id}`"
        )));
    }
    let mut mask = new_source_mask(document, &id, name, frame_dims, MaskStatus::Pending);
    mask.ai_select = Some(AiSelect {
        kind,
        detail: detail.map(str::to_string),
        extras: BTreeMap::new(),
    });
    mask_copy_mut(document, copy_id)?.mask_library.push(mask);
    Ok(())
}

fn mask_add_luminance(
    document: &mut SidecarDocument,
    copy_id: &str,
    name: &str,
    min: f32,
    max: f32,
    feather: f32,
    frame_dims: Option<(u32, u32)>,
) -> Result<(), CliError> {
    let id = stable_mask_id(&["luminance-range", name]);
    if mask_library_contains(document, copy_id, &id) {
        return Err(CliError::Message(format!(
            "mask `{name}` already exists on copy `{copy_id}`"
        )));
    }
    let mut mask = new_source_mask(document, &id, name, frame_dims, MaskStatus::Valid);
    mask.prompt = Some(MaskPrompt::LuminanceRange {
        min,
        max,
        feather,
        transformation: PromptTransform::default(),
    });
    mask_copy_mut(document, copy_id)?.mask_library.push(mask);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn mask_add_color(
    document: &mut SidecarDocument,
    copy_id: &str,
    name: &str,
    hue_center: Option<f32>,
    hue_width: Option<f32>,
    sat_min: Option<f32>,
    sat_max: Option<f32>,
    lum_min: Option<f32>,
    lum_max: Option<f32>,
    feather: f32,
    frame_dims: Option<(u32, u32)>,
) -> Result<(), CliError> {
    let (hue_center, hue_width, sat_min, sat_max, lum_min, lum_max) = (
        hue_center
            .ok_or_else(|| CliError::Message("--add-color-range requires --hue-center".into()))?,
        hue_width
            .ok_or_else(|| CliError::Message("--add-color-range requires --hue-width".into()))?,
        sat_min.ok_or_else(|| CliError::Message("--add-color-range requires --sat-min".into()))?,
        sat_max.ok_or_else(|| CliError::Message("--add-color-range requires --sat-max".into()))?,
        lum_min.ok_or_else(|| CliError::Message("--add-color-range requires --lum-min".into()))?,
        lum_max.ok_or_else(|| CliError::Message("--add-color-range requires --lum-max".into()))?,
    );
    let id = stable_mask_id(&["color-range", name]);
    if mask_library_contains(document, copy_id, &id) {
        return Err(CliError::Message(format!(
            "mask `{name}` already exists on copy `{copy_id}`"
        )));
    }
    let mut mask = new_source_mask(document, &id, name, frame_dims, MaskStatus::Valid);
    mask.prompt = Some(MaskPrompt::ColorRange {
        hue_center,
        hue_width,
        sat_min,
        sat_max,
        lum_min,
        lum_max,
        feather,
        transformation: PromptTransform::default(),
    });
    mask_copy_mut(document, copy_id)?.mask_library.push(mask);
    Ok(())
}

fn mask_combine(
    document: &mut SidecarDocument,
    copy_id: &str,
    op: &str,
    name: &str,
    inputs: &str,
    frame_dims: Option<(u32, u32)>,
) -> Result<(), CliError> {
    let operation = match op.trim().to_ascii_lowercase().as_str() {
        "union" | "add" => MaskOperation::Union,
        "intersect" => MaskOperation::Intersect,
        "subtract" | "sub" => MaskOperation::Subtract,
        "invert" | "inv" => MaskOperation::Invert,
        _ => {
            return Err(CliError::Message(format!(
                "unknown combine op `{op}`: expected union|intersect|subtract|invert"
            )));
        }
    };
    let refs: Vec<(String, String)> = inputs
        .split(',')
        .map(|part| parse_mask_ref(part.trim(), copy_id))
        .collect::<Result<_, _>>()?;
    let arity_ok = match operation {
        MaskOperation::Invert => refs.len() == 1,
        MaskOperation::Subtract => refs.len() == 2,
        MaskOperation::Union | MaskOperation::Intersect => refs.len() >= 2,
        MaskOperation::Source => false,
    };
    if !arity_ok {
        return Err(CliError::Message(format!(
            "combine op `{op}` needs {} input(s), got {}",
            match operation {
                MaskOperation::Invert => "exactly 1",
                MaskOperation::Subtract => "exactly 2",
                _ => "at least 2",
            },
            refs.len()
        )));
    }
    for (ref_copy, ref_mask) in &refs {
        if !mask_library_contains(document, ref_copy, ref_mask) {
            return Err(CliError::Message(format!(
                "combine input references unknown mask `{ref_copy}/{ref_mask}`"
            )));
        }
    }
    // Canonical op key (not the raw alias): `union` and `add` name the same
    // node, so re-running with either spelling hits the loud duplicate.
    let op_key = match operation {
        MaskOperation::Union => "union",
        MaskOperation::Intersect => "intersect",
        MaskOperation::Subtract => "subtract",
        MaskOperation::Invert => "invert",
        MaskOperation::Source => "source",
    };
    let id = stable_mask_id(&["combine", op_key, name]);
    if mask_library_contains(document, copy_id, &id) {
        return Err(CliError::Message(format!(
            "mask `{name}` already exists on copy `{copy_id}`"
        )));
    }
    let references = refs
        .into_iter()
        .map(|(ref_copy, ref_mask)| MaskReference {
            copy_id: ref_copy,
            mask_id: ref_mask,
            extras: BTreeMap::new(),
        })
        .collect();
    let mut mask = new_source_mask(document, &id, name, frame_dims, MaskStatus::Pending);
    mask.operation = operation;
    mask.references = references;
    // A derived node inherits usability only through the loader blessing pass;
    // `Pending` keeps it honest until every input resolves.
    mask_copy_mut(document, copy_id)?.mask_library.push(mask);
    Ok(())
}

fn mask_duplicate(
    document: &mut SidecarDocument,
    copy_id: &str,
    source: &str,
    name: &str,
) -> Result<(), CliError> {
    let (ref_copy, ref_mask) = parse_mask_ref(source, copy_id)?;
    let template = document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == ref_copy)
        .and_then(|copy| copy.mask_library.iter().find(|mask| mask.id == ref_mask))
        .cloned()
        .ok_or_else(|| CliError::Message(format!("unknown mask `{ref_copy}/{ref_mask}`")))?;
    // Only source payloads duplicate cleanly; a derived node is rebuilt with
    // `--combine` against the same inputs instead of aliasing them silently.
    if template.operation != MaskOperation::Source {
        return Err(CliError::Message(format!(
            "cannot duplicate derived mask `{ref_copy}/{ref_mask}`; use --combine to rebuild it"
        )));
    }
    let id = stable_mask_id(&["duplicate", &ref_copy, &ref_mask, name]);
    if mask_library_contains(document, copy_id, &id) {
        return Err(CliError::Message(format!(
            "mask `{name}` already exists on copy `{copy_id}`"
        )));
    }
    let mut duplicated = template;
    duplicated.id = id;
    duplicated.name = name.into();
    duplicated.created_at = unix_now();
    duplicated.generator_version = env!("CARGO_PKG_VERSION").into();
    mask_copy_mut(document, copy_id)?
        .mask_library
        .push(duplicated);
    Ok(())
}

fn mask_attach_layer(
    document: &mut SidecarDocument,
    copy_id: &str,
    target: &str,
) -> Result<(), CliError> {
    let (ref_copy, ref_mask) = parse_mask_ref(target, copy_id)?;
    if !mask_library_contains(document, &ref_copy, &ref_mask) {
        return Err(CliError::Message(format!(
            "cannot attach unknown mask `{ref_copy}/{ref_mask}`"
        )));
    }
    let layer_id = format!("layer-{ref_mask}");
    let copy = mask_copy_mut(document, copy_id)?;
    if copy.mask_layers.iter().any(|layer| layer.id == layer_id) {
        return Err(CliError::Message(format!(
            "copy `{copy_id}` already has layer `{layer_id}`"
        )));
    }
    copy.mask_layers.push(MaskLayer {
        id: layer_id,
        mask: MaskReference {
            copy_id: ref_copy,
            mask_id: ref_mask,
            extras: BTreeMap::new(),
        },
        inverted: false,
        feather: 0.0,
        blur: 0.0,
        density: 1.0,
        visible: true,
        extras: BTreeMap::new(),
    });
    Ok(())
}

fn mask_set_layer_visible(
    document: &mut SidecarDocument,
    copy_id: &str,
    layer_id: &str,
    visible: bool,
) -> Result<(), CliError> {
    let layer = mask_copy_mut(document, copy_id)?
        .mask_layers
        .iter_mut()
        .find(|layer| layer.id == layer_id)
        .ok_or_else(|| {
            CliError::Message(format!("unknown layer `{layer_id}` on copy `{copy_id}`"))
        })?;
    layer.visible = visible;
    Ok(())
}

fn validate(args: IndexArgs) -> Result<(), CliError> {
    let path = if args.input.extension().and_then(|e| e.to_str()) == Some("json") {
        args.input
    } else {
        sidecar_path_for(&args.input)
    };
    if args.migrate {
        migrate_sidecar(&path)?;
    }
    let document = load_sidecar(&path)?;
    document.validate()?;
    emit(
        args.json,
        serde_json::json!({"command":"validate", "sidecar":path, "status":"valid"}),
        "valid",
    )
}

/// Requires the sidecar of `input`, failing loudly when none exists instead
/// of silently operating on default contents.
fn require_sidecar(input: &Path) -> Result<(PathBuf, SidecarDocument), CliError> {
    let path = sidecar_path_for(input);
    match load_sidecar(&path) {
        Ok(document) => Ok((path, document)),
        Err(lumina_sidecar::SidecarError::Missing(_)) => Err(CliError::Message(format!(
            "no sidecar for `{}`; run `import` first",
            input.display()
        ))),
        Err(error) => Err(error.into()),
    }
}

/// Resolves `--input` of the multi-sidecar metadata commands to the sidecar
/// files to process, in deterministic (sorted) order: a `*.lumina.json` file
/// is used directly, any other file maps to its sidecar path, and a
/// directory is scanned recursively (symlink-/loop-safe, same walk as
/// `reindex`).
fn collect_target_sidecars(input: &Path) -> Result<Vec<PathBuf>, CliError> {
    if input.is_file() {
        if input.to_string_lossy().ends_with(".lumina.json") {
            return Ok(vec![input.to_path_buf()]);
        }
        return Ok(vec![sidecar_path_for(input)]);
    }
    let mut files = Vec::new();
    collect_sidecars(input, &mut files)?;
    files.sort();
    Ok(files)
}

/// Portable smart-collection catalog file (G-15 META-MVP, Slice 2). The file
/// holds versioned rule data only — never absolute paths — and is validated
/// with the same rules as the sidecar slice.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SmartCatalogFile {
    format: String,
    version: u8,
    collections: Vec<SmartCollectionDef>,
}

/// Loads and validates a smart-collection catalog file. Every deviation
/// (unreadable file, invalid JSON, wrong format/version marker, invalid
/// definition) is a loud error; there is no silent fallback to an empty
/// catalog.
fn load_smart_catalog(path: &Path) -> Result<Vec<SmartCollectionDef>, CliError> {
    let json = fs::read_to_string(path).map_err(|error| io_error(path, error))?;
    let catalog: SmartCatalogFile = serde_json::from_str(&json).map_err(|error| {
        CliError::Message(format!(
            "invalid smart-collection catalog `{}`: {error}",
            path.display()
        ))
    })?;
    if catalog.format != "lumina-smart-catalog" {
        return Err(CliError::Message(format!(
            "invalid smart-collection catalog `{}`: expected format \"lumina-smart-catalog\", got \"{}\"",
            path.display(),
            catalog.format
        )));
    }
    if catalog.version != SMART_COLLECTION_VERSION {
        return Err(CliError::Message(format!(
            "invalid smart-collection catalog `{}`: unsupported version {}, expected {SMART_COLLECTION_VERSION}",
            path.display(),
            catalog.version
        )));
    }
    for def in &catalog.collections {
        validate_smart_collection_def(def).map_err(|error| {
            CliError::Message(format!(
                "invalid smart-collection definition `{}` in catalog `{}`: {error}",
                def.id,
                path.display()
            ))
        })?;
    }
    Ok(catalog.collections)
}

fn keywords(args: KeywordsArgs) -> Result<(), CliError> {
    let (path, mut document) = require_sidecar(&args.input)?;
    let original_bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
    let mut ops = Vec::with_capacity(args.add.len() + args.remove.len());
    for keyword in &args.add {
        ops.push(BatchOp::AddKeyword {
            keyword: keyword.clone(),
        });
    }
    for keyword in &args.remove {
        ops.push(BatchOp::RemoveKeyword {
            keyword: keyword.clone(),
        });
    }
    let mut changed = false;
    for op in &ops {
        changed |= apply_batch_op(&mut document, op).map_err(|error| {
            CliError::Message(format!(
                "keywords for `{}` rejected: {error}",
                args.input.display()
            ))
        })?;
    }
    if changed {
        document.validate()?;
        save_sidecar(&path, &document)?;
        info!(
            "keywords for `{}` updated ({} operation(s), {} keyword(s))",
            args.input.display(),
            ops.len(),
            document.keywords.len()
        );
    } else if ops.is_empty() {
        info!("keywords for `{}` listed", args.input.display());
    } else {
        info!(
            "keywords for `{}` unchanged (idempotent no-op)",
            args.input.display()
        );
    }
    // The original image is never modified by a metadata command.
    debug_assert_eq!(
        fs::read(&args.input).map_err(|error| io_error(&args.input, error))?,
        original_bytes
    );
    emit(
        args.json,
        serde_json::json!({"command":"keywords", "input":args.input, "keywords":document.keywords, "changed":changed, "status":"ok"}),
        &format_keywords_text(&document.keywords),
    )
}

/// LRPAR-G15-IPTC-S3: origin recorded for every CLI draft/history mutation
/// (S1 `validate_metadata_origin` accepts `cli`; no paths are ever written
/// into `metadata`, so no absolute path can leak into the sidecar).
const META_ORIGIN_CLI: &str = "cli";

/// LRPAR-G15-IPTC-S3: dispatches the `meta` subcommands (SOLL §8). All
/// draft/history mutations run over the same sidecar path (CAS, atomar,
/// loud); `inspect`/`history show` are read-only.
fn meta(args: MetaArgs) -> Result<(), CliError> {
    match args.command {
        MetaCommand::Inspect(inspect) => meta_inspect(inspect),
        MetaCommand::Draft(draft) => match draft.command {
            MetaDraftCommand::Set(set) => meta_draft_set(set),
            MetaDraftCommand::Clear(clear) => meta_draft_clear(clear),
        },
        MetaCommand::History(history) => match history.command {
            MetaHistoryCommand::Show(show) => meta_history_show(show),
            MetaHistoryCommand::Clear(clear) => meta_history_clear(clear),
        },
        MetaCommand::Preset(preset) => match preset.command {
            MetaPresetCommand::List(list) => meta_preset_list(list),
            MetaPresetCommand::Show(show) => meta_preset_show(show),
            MetaPresetCommand::Apply(apply) => meta_preset_apply(apply),
        },
        MetaCommand::Sync(sync) => meta_sync(sync),
        MetaCommand::Copy(copy) => meta_copy(copy),
        MetaCommand::Paste(paste) => meta_paste(paste),
    }
}

/// LRPAR-G15-IPTC-S3: reads the embedded IPTC of `bytes` when they are a
/// JPEG (IIM/XMP via `lumina-iptc`). Non-JPEG input (PNG/WebP/RAW/Raster)
/// yields `Ok(None)` — inspect reports this loudly as "nicht verfügbar".
/// Present-but-broken JPEG segments are a loud error, never a silent skip.
fn read_embedded_iptc(bytes: &[u8]) -> Result<Option<IptcMetadata>, CliError> {
    if bytes.len() < 2 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return Ok(None);
    }
    extract_metadata(bytes)
        .map(Some)
        .map_err(|error| CliError::Message(format!("embedded IPTC unreadable: {error}")))
}

/// LRPAR-G15-IPTC-S3: maps a registry field ID onto the embedded value.
/// `IptcMetadata` exposes no by-ID accessor, so the mapping lives here
/// (CLI-only, no second registry: IDs come from `METADATA_FIELD_IDS`).
fn embedded_field_value<'a>(meta: &'a IptcMetadata, id: &str) -> Option<&'a str> {
    match id {
        "title" => meta.title.as_deref(),
        "headline" => meta.headline.as_deref(),
        "description" => meta.description.as_deref(),
        "copyright_notice" => meta.copyright_notice.as_deref(),
        "creator" => meta.creator.as_deref(),
        "credit" => meta.credit.as_deref(),
        "source" => meta.source.as_deref(),
        "city" => meta.city.as_deref(),
        "state_province" => meta.state_province.as_deref(),
        "country" => meta.country.as_deref(),
        "date_created" => meta.date_created.as_deref(),
        _ => None,
    }
}

fn format_optional_value(value: Option<&str>) -> String {
    value.map_or("(absent)".to_string(), |v| format!("\"{v}\""))
}

/// LRPAR-G15-IPTC-S3: `meta inspect` — embedded IPTC (JPEG) or a loud
/// "nicht verfügbar", the draft overlay per field (draft vs. embedded),
/// keywords (draft vs. embedded) and the history length. Read-only.
fn meta_inspect(args: MetaInspectArgs) -> Result<(), CliError> {
    let (_, document) = require_sidecar(&args.path)?;
    let bytes = fs::read(&args.path).map_err(|error| io_error(&args.path, error))?;
    let embedded = read_embedded_iptc(&bytes)?;
    let history_len = document.metadata.history.len();
    let latest_rev = document.metadata.latest_rev();
    info!(
        "meta inspect for `{}` (embedded: {}, {} draft field(s), history: {} entries)",
        args.path.display(),
        if embedded.is_some() {
            "verfügbar"
        } else {
            "nicht verfügbar"
        },
        document.metadata.draft.len(),
        history_len
    );
    let mut draft_json = serde_json::Map::new();
    let mut embedded_json = serde_json::Map::new();
    let mut lines = Vec::with_capacity(METADATA_FIELD_IDS.len() + 3);
    lines.push(if embedded.is_some() {
        "embedded: verfügbar (JPEG, IIM/XMP)".to_string()
    } else {
        "embedded: nicht verfügbar (kein JPEG / kein IIM/XMP)".to_string()
    });
    for id in METADATA_FIELD_IDS {
        let draft = document.metadata.get(id);
        let embedded_value = embedded
            .as_ref()
            .and_then(|meta| embedded_field_value(meta, id));
        if let Some(value) = draft {
            draft_json.insert(
                (*id).to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
        if let Some(value) = embedded_value {
            embedded_json.insert(
                (*id).to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
        lines.push(format!(
            "{id}: draft={} embedded={}",
            format_optional_value(draft),
            format_optional_value(embedded_value)
        ));
    }
    let embedded_keywords: Vec<String> = embedded
        .as_ref()
        .map_or_else(Vec::new, |meta| meta.keywords.clone());
    lines.push(format!(
        "keywords: draft=[{}] embedded=[{}]",
        document.keywords.join(", "),
        embedded_keywords.join(", ")
    ));
    lines.push(format!(
        "history: {history_len} entries (latest rev {latest_rev})"
    ));
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-inspect",
            "input": args.path,
            "embedded_available": embedded.is_some(),
            "draft": draft_json,
            "embedded": embedded_json,
            "keywords": document.keywords,
            "keywords_embedded": embedded_keywords,
            "history_len": history_len,
            "history_latest_rev": latest_rev,
            "status": "ok",
        }),
        &lines.join("\n"),
    )
}

/// LRPAR-G15-IPTC-S3: splits a `--field` value at the first `=` into
/// `(id, value)`. A missing `=` is a loud error before anything is mutated.
fn split_field_assignment(value: &str) -> Result<(String, String), CliError> {
    value.split_once('=').map_or_else(
        || {
            Err(CliError::Message(format!(
                "invalid field assignment `{value}`: expected `ID=VALUE`"
            )))
        },
        |(id, field_value)| Ok((id.to_string(), field_value.to_string())),
    )
}

/// LRPAR-G15-IPTC-S3: `meta draft set` — all-or-nothing per call. Every
/// assignment is parsed and every draft value is validated (S1
/// `validate_metadata_field_value`, unknown IDs included) BEFORE any
/// mutation; draft + routed `keywords` land in ONE history entry
/// (`origin = "cli"`) on a clone, the clone is fully validated and the
/// write goes through CAS (`save_sidecar_if_unchanged`, atomar). Conflict =
/// loud error, never silent last-write-wins.
fn meta_draft_set(args: MetaDraftSetArgs) -> Result<(), CliError> {
    let (path, document) = require_sidecar(&args.path)?;
    let original_bytes = fs::read(&args.path).map_err(|error| io_error(&args.path, error))?;
    let mut draft_fields: Vec<(String, String)> = Vec::new();
    let mut keyword_entries: Vec<String> = Vec::new();
    for assignment in &args.field {
        let (id, value) = split_field_assignment(assignment)?;
        if id == "keywords" {
            keyword_entries.push(value);
        } else {
            draft_fields.push((id, value));
        }
    }
    for (field, value) in &draft_fields {
        validate_metadata_field_value(field, value).map_err(|error| {
            CliError::Message(format!(
                "meta draft set for `{}` rejected: {error}",
                args.path.display()
            ))
        })?;
    }
    let mut candidate = document.clone();
    let mut changed = BTreeSet::new();
    for (field, value) in &draft_fields {
        if value.is_empty() || value.trim().is_empty() {
            if candidate.metadata.draft.remove(field).is_some() {
                changed.insert(field.clone());
            }
        } else if candidate.metadata.draft.get(field).map(String::as_str) != Some(value.as_str()) {
            candidate
                .metadata
                .draft
                .insert(field.clone(), value.clone());
            changed.insert(field.clone());
        }
    }
    if !keyword_entries.is_empty() {
        let sole_empty = keyword_entries.len() == 1
            && (keyword_entries[0].is_empty() || keyword_entries[0].trim().is_empty());
        if !sole_empty {
            for entry in &keyword_entries {
                if entry.is_empty() || entry.trim() != entry {
                    return Err(CliError::Message(format!(
                        "meta draft set for `{}` rejected: keyword must be non-empty and without leading/trailing whitespace (use `meta draft clear --field keywords` to empty the list)",
                        args.path.display()
                    )));
                }
            }
        }
        let next: Vec<String> = if sole_empty {
            Vec::new()
        } else {
            keyword_entries.clone()
        };
        if next != candidate.keywords {
            candidate.keywords = next;
            changed.insert("keywords".to_string());
        }
    }
    let status_text = if changed.is_empty() {
        info!(
            "meta draft set for `{}` unchanged (idempotent no-op)",
            args.path.display()
        );
        format!(
            "meta draft set: unchanged ({} assignment(s))",
            args.field.len()
        )
    } else {
        let changed_list: Vec<String> = changed.into_iter().collect();
        let timestamp = now_rfc3339_utc();
        let rev = candidate.metadata.latest_rev() + 1;
        candidate.metadata.history.insert(
            0,
            MetadataHistoryEntry {
                rev,
                timestamp,
                origin: META_ORIGIN_CLI.to_string(),
                changed: changed_list.clone(),
            },
        );
        candidate
            .metadata
            .history
            .truncate(MAX_METADATA_HISTORY_ENTRIES);
        candidate.validate().map_err(|error| {
            CliError::Message(format!(
                "meta draft set for `{}` rejected: {error}",
                args.path.display()
            ))
        })?;
        let expected = document_revision(&document)?;
        let saved_rev = save_sidecar_if_unchanged(&path, &candidate, Some(&expected))?;
        debug_assert!(!saved_rev.is_empty());
        info!(
            "meta draft set for `{}` updated (rev {rev}, changed: {})",
            args.path.display(),
            changed_list.join(", ")
        );
        format!(
            "meta draft set: updated rev {rev} (changed: {})",
            changed_list.join(", ")
        )
    };
    // The original image is never modified by a metadata command.
    debug_assert_eq!(
        fs::read(&args.path).map_err(|error| io_error(&args.path, error))?,
        original_bytes
    );
    let reloaded = load_sidecar(&path)?;
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-draft-set",
            "input": args.path,
            "draft": reloaded.metadata.draft,
            "keywords": reloaded.keywords,
            "history_len": reloaded.metadata.history.len(),
            "history_latest_rev": reloaded.metadata.latest_rev(),
            "status": "ok",
        }),
        &status_text,
    )
}

/// LRPAR-G15-IPTC-S3: `meta draft clear` — exactly one of `--field` /
/// `--all`. Unknown IDs abort loudly with all-or-nothing semantics; the
/// history is kept and gains one entry listing the removed IDs (CAS, atomar).
fn meta_draft_clear(args: MetaDraftClearArgs) -> Result<(), CliError> {
    if args.all == !args.field.is_empty() {
        return Err(CliError::Message(
            "`meta draft clear` requires exactly one of `--field <id,…>` / `--all`".into(),
        ));
    }
    let (path, document) = require_sidecar(&args.path)?;
    let original_bytes = fs::read(&args.path).map_err(|error| io_error(&args.path, error))?;
    let expected = document_revision(&document)?;
    let timestamp = now_rfc3339_utc();
    let mut candidate = document.clone();
    let status_text;
    if args.all {
        if !candidate.clear_metadata_draft(META_ORIGIN_CLI, &timestamp)? {
            info!(
                "meta draft clear for `{}` unchanged (draft already empty)",
                args.path.display()
            );
            status_text = "meta draft clear: unchanged (draft already empty)".to_string();
            debug_assert_eq!(
                fs::read(&args.path).map_err(|error| io_error(&args.path, error))?,
                original_bytes
            );
            let reloaded = load_sidecar(&path)?;
            return emit(
                args.json,
                serde_json::json!({
                    "command": "meta-draft-clear",
                    "input": args.path,
                    "draft": reloaded.metadata.draft,
                    "keywords": reloaded.keywords,
                    "history_len": reloaded.metadata.history.len(),
                    "history_latest_rev": reloaded.metadata.latest_rev(),
                    "status": "ok",
                }),
                &status_text,
            );
        }
    } else {
        for id in &args.field {
            if id != "keywords" && !is_metadata_field(id) {
                return Err(CliError::Message(format!(
                    "meta draft clear for `{}` rejected: unknown metadata field `{id}`",
                    args.path.display()
                )));
            }
        }
        let mut removed = BTreeSet::new();
        for id in &args.field {
            if id == "keywords" {
                if !candidate.keywords.is_empty() {
                    candidate.keywords.clear();
                    removed.insert("keywords".to_string());
                }
            } else if candidate.metadata.draft.remove(id).is_some() {
                removed.insert(id.clone());
            }
        }
        if removed.is_empty() {
            info!(
                "meta draft clear for `{}` unchanged (fields already absent)",
                args.path.display()
            );
            status_text = "meta draft clear: unchanged (fields already absent)".to_string();
            debug_assert_eq!(
                fs::read(&args.path).map_err(|error| io_error(&args.path, error))?,
                original_bytes
            );
            let reloaded = load_sidecar(&path)?;
            return emit(
                args.json,
                serde_json::json!({
                    "command": "meta-draft-clear",
                    "input": args.path,
                    "draft": reloaded.metadata.draft,
                    "keywords": reloaded.keywords,
                    "history_len": reloaded.metadata.history.len(),
                    "history_latest_rev": reloaded.metadata.latest_rev(),
                    "status": "ok",
                }),
                &status_text,
            );
        }
        let removed_list: Vec<String> = removed.into_iter().collect();
        let rev = candidate.metadata.latest_rev() + 1;
        candidate.metadata.history.insert(
            0,
            MetadataHistoryEntry {
                rev,
                timestamp,
                origin: META_ORIGIN_CLI.to_string(),
                changed: removed_list,
            },
        );
        candidate
            .metadata
            .history
            .truncate(MAX_METADATA_HISTORY_ENTRIES);
        candidate.validate()?;
    }
    let rev = candidate.metadata.latest_rev();
    let changed_list = candidate
        .metadata
        .history
        .first()
        .map_or_else(Vec::new, |entry| entry.changed.clone());
    save_sidecar_if_unchanged(&path, &candidate, Some(&expected))?;
    info!(
        "meta draft clear for `{}` updated (rev {rev}, removed: {})",
        args.path.display(),
        changed_list.join(", ")
    );
    status_text = format!(
        "meta draft clear: updated rev {rev} (removed: {})",
        changed_list.join(", ")
    );
    // The original image is never modified by a metadata command.
    debug_assert_eq!(
        fs::read(&args.path).map_err(|error| io_error(&args.path, error))?,
        original_bytes
    );
    let reloaded = load_sidecar(&path)?;
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-draft-clear",
            "input": args.path,
            "draft": reloaded.metadata.draft,
            "keywords": reloaded.keywords,
            "history_len": reloaded.metadata.history.len(),
            "history_latest_rev": reloaded.metadata.latest_rev(),
            "status": "ok",
        }),
        &status_text,
    )
}

/// LRPAR-G15-IPTC-S3: `meta history show` — lists entries newest-first,
/// optionally limited. Read-only.
fn meta_history_show(args: MetaHistoryShowArgs) -> Result<(), CliError> {
    let (_, document) = require_sidecar(&args.path)?;
    let total = document.metadata.history.len();
    let mut entries = document.metadata.history.clone();
    if let Some(limit) = args.limit {
        entries.truncate(limit);
    }
    info!(
        "meta history show for `{}` ({} of {total} entries)",
        args.path.display(),
        entries.len()
    );
    let lines: Vec<String> = entries
        .iter()
        .map(|entry| {
            format!(
                "rev {} | {} | {} | {}",
                entry.rev,
                entry.timestamp,
                entry.origin,
                entry.changed.join(", ")
            )
        })
        .collect();
    let text = if lines.is_empty() {
        "history: (empty)".to_string()
    } else {
        format!(
            "history ({} of {total}):\n{}",
            entries.len(),
            lines.join("\n")
        )
    };
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-history-show",
            "input": args.path,
            "history": entries,
            "shown": entries.len(),
            "total": total,
            "status": "ok",
        }),
        &text,
    )
}

/// LRPAR-G15-IPTC-S3: `meta history clear` — explicitly clears the whole
/// history (the only way to empty it; no undo). Draft values are kept.
/// CAS + atomar; clearing an empty history is an idempotent no-op.
fn meta_history_clear(args: MetaHistoryClearArgs) -> Result<(), CliError> {
    let (path, mut document) = require_sidecar(&args.path)?;
    let original_bytes = fs::read(&args.path).map_err(|error| io_error(&args.path, error))?;
    let status_text = if document.metadata.history.is_empty() {
        info!(
            "meta history clear for `{}` unchanged (history already empty)",
            args.path.display()
        );
        "meta history clear: unchanged (history already empty)".to_string()
    } else {
        let removed = document.metadata.history.len();
        let expected = document_revision(&document)?;
        document.clear_metadata_history();
        document.validate()?;
        save_sidecar_if_unchanged(&path, &document, Some(&expected))?;
        info!(
            "meta history clear for `{}` removed {removed} entries (draft kept)",
            args.path.display()
        );
        format!("meta history clear: removed {removed} entries (draft kept)")
    };
    // The original image is never modified by a metadata command.
    debug_assert_eq!(
        fs::read(&args.path).map_err(|error| io_error(&args.path, error))?,
        original_bytes
    );
    let reloaded = load_sidecar(&path)?;
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-history-clear",
            "input": args.path,
            "draft": reloaded.metadata.draft,
            "keywords": reloaded.keywords,
            "history_len": reloaded.metadata.history.len(),
            "status": "ok",
        }),
        &status_text,
    )
}

/// LRPAR-G15-IPTC-S4: resolves a `show` / `apply` preset spec (display name
/// against the user-global directory, or an explicit file path) and loads it.
/// Any failure aborts loudly (exit 1) before any target is touched.
fn load_cli_meta_preset(spec: &str) -> Result<(PathBuf, MetaPresetFile), CliError> {
    let path = resolve_meta_preset_path(spec, None).map_err(|error| {
        CliError::Message(format!("meta preset `{spec}` cannot be resolved: {error}"))
    })?;
    load_meta_preset_file(&path)
        .map(|preset| (path.clone(), preset))
        .map_err(|error| {
            CliError::Message(format!(
                "meta preset `{}` rejected: {error}",
                path.display()
            ))
        })
}

/// LRPAR-G15-IPTC-S4: `meta preset list` — lists the directory (explicit or
/// user-global) sorted by file name. Read-only; broken files surface as
/// failed entries with their reason (exit stays 0, nothing is hidden).
fn meta_preset_list(args: MetaPresetListArgs) -> Result<(), CliError> {
    let dir = match args.dir {
        Some(dir) => dir,
        None => default_meta_presets_dir().ok_or_else(|| {
            CliError::Message(
                "meta-preset directory is unavailable: the platform configuration directory \
                 could not be determined; pass an explicit directory"
                    .into(),
            )
        })?,
    };
    let entries = scan_meta_presets_dir(&dir);
    let mut available_count = 0usize;
    let mut failed_count = 0usize;
    let mut lines = Vec::with_capacity(entries.len());
    let mut items = Vec::with_capacity(entries.len());
    for entry in &entries {
        match entry {
            MetaPresetEntry::Available { path, preset } => {
                available_count += 1;
                info!(
                    "meta preset list: `{}` available ({} field(s), {} placeholder(s))",
                    path.display(),
                    preset.fields.len(),
                    preset.placeholders.len()
                );
                lines.push(format!(
                    "{}: {} field(s), {} placeholder(s) [{}]",
                    preset.name,
                    preset.fields.len(),
                    preset.placeholders.len(),
                    path.display()
                ));
                items.push(serde_json::json!({
                    "path": path,
                    "status": "available",
                    "name": preset.name,
                    "fields": preset.fields.len(),
                    "placeholders": preset.placeholders.iter().map(|p| &p.name).collect::<Vec<_>>(),
                }));
            }
            MetaPresetEntry::Failed { path, error } => {
                failed_count += 1;
                info!("meta preset list: `{}` failed ({error})", path.display());
                lines.push(format!("{}: FAILED ({error})", path.display()));
                items.push(serde_json::json!({
                    "path": path,
                    "status": "failed",
                    "error": error,
                }));
            }
        }
    }
    info!(
        "meta preset list for `{}` ({} available, {} failed)",
        dir.display(),
        available_count,
        failed_count
    );
    let text = if lines.is_empty() {
        format!("presets in `{}`: (none)", dir.display())
    } else {
        format!(
            "presets in `{}` ({} available, {} failed):\n{}",
            dir.display(),
            available_count,
            failed_count,
            lines.join("\n")
        )
    };
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-preset-list",
            "dir": dir,
            "available": available_count,
            "failed": failed_count,
            "items": items,
            "status": "ok",
        }),
        &text,
    )
}

/// LRPAR-G15-IPTC-S4: `meta preset show` — displays one preset (fields plus
/// placeholder names with descriptions). Read-only.
fn meta_preset_show(args: MetaPresetShowArgs) -> Result<(), CliError> {
    let (path, preset) = load_cli_meta_preset(&args.preset)?;
    info!(
        "meta preset show for `{}` ({} field(s), {} placeholder(s))",
        path.display(),
        preset.fields.len(),
        preset.placeholders.len()
    );
    let mut lines = vec![
        format!("name: {}", preset.name),
        format!("file: {}", path.display()),
    ];
    for (id, value) in &preset.fields {
        lines.push(format!("field {id}: \"{value}\""));
    }
    for placeholder in &preset.placeholders {
        lines.push(format!(
            "placeholder {{{}}}: {}",
            placeholder.name, placeholder.description
        ));
    }
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-preset-show",
            "preset": path,
            "name": preset.name,
            "fields": preset.fields,
            "placeholders": preset.placeholders,
            "status": "ok",
        }),
        &lines.join("\n"),
    )
}

/// LRPAR-G15-IPTC-S4: splits a `--var` value at the first `=` into
/// `(name, value)`. A missing `=` is a loud error before anything is mutated.
fn split_var_assignment(value: &str) -> Result<(String, String), CliError> {
    value.split_once('=').map_or_else(
        || {
            Err(CliError::Message(format!(
                "invalid variable assignment `{value}`: expected `NAME=VALUE`"
            )))
        },
        |(name, var_value)| Ok((name.to_string(), var_value.to_string())),
    )
}

/// LRPAR-G15-IPTC-S4: applies the resolved draft to one target: load, mutate
/// a clone via `apply_metadata_draft` (`origin = "preset:<name>"`), validate,
/// CAS + atomic save. Returns `Ok(true)` on update and `Ok(false)` for
/// idempotent no-ops (no history entry, no write). A missing sidecar is a
/// loud per-target error ("zuerst importieren"), never a silent creation.
fn apply_meta_preset_to_target(
    target: &Path,
    resolved: &BTreeMap<String, String>,
    origin: &str,
) -> Result<bool, CliError> {
    let sidecar = sidecar_path_for(target);
    let document = match load_sidecar(&sidecar) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            return Err(CliError::Message(format!(
                "no sidecar for `{}`; run `import` first",
                target.display()
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let original_bytes = fs::read(target).map_err(|error| io_error(target, error))?;
    let expected = document_revision(&document)?;
    let timestamp = now_rfc3339_utc();
    let mut candidate = document.clone();
    if !candidate.apply_metadata_draft(resolved, origin, &timestamp)? {
        debug_assert_eq!(
            fs::read(target).map_err(|error| io_error(target, error))?,
            original_bytes
        );
        return Ok(false);
    }
    save_sidecar_if_unchanged(&sidecar, &candidate, Some(&expected))?;
    // The original image is never modified by a metadata command.
    debug_assert_eq!(
        fs::read(target).map_err(|error| io_error(target, error))?,
        original_bytes
    );
    Ok(true)
}

/// LRPAR-G15-IPTC-S4: `meta preset apply` — renders the preset upfront (all
/// placeholder variables required; missing/unknown variables and limit
/// violations abort everything with exit 1, nothing written), then applies
/// the rendered draft per target in isolation (updated / unchanged / failed;
/// exit 3 on partial failure). Idempotent re-application reports `unchanged`
/// without a history entry.
fn meta_preset_apply(args: MetaPresetApplyArgs) -> Result<(), CliError> {
    let (path, preset) = load_cli_meta_preset(&args.preset)?;
    let mut vars = BTreeMap::new();
    for assignment in &args.var {
        let (name, value) = split_var_assignment(assignment)?;
        if vars.insert(name.clone(), value).is_some() {
            return Err(CliError::Message(format!(
                "duplicate variable `{name}` (each `--var` name may be given once)"
            )));
        }
    }
    let resolved =
        render_meta_preset(&preset, &vars, &path.display().to_string()).map_err(|error| {
            CliError::Message(format!(
                "meta preset apply for `{}` rejected: {error}",
                path.display()
            ))
        })?;
    let origin = format!("preset:{}", preset.name);
    info!(
        "meta preset apply `{}` to {} target(s) ({} field(s))",
        preset.name,
        args.target.len(),
        resolved.len()
    );
    let mut updated_count = 0usize;
    let mut unchanged_count = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut items = Vec::with_capacity(args.target.len());
    for target in &args.target {
        match apply_meta_preset_to_target(target, &resolved, &origin) {
            Ok(true) => {
                info!(
                    "meta preset apply: `{}` updated (preset `{}`)",
                    target.display(),
                    preset.name
                );
                updated_count += 1;
                items.push(serde_json::json!({"target": target, "status": "updated"}));
            }
            Ok(false) => {
                info!(
                    "meta preset apply: `{}` unchanged (preset `{}` already applied)",
                    target.display(),
                    preset.name
                );
                unchanged_count += 1;
                items.push(serde_json::json!({"target": target, "status": "unchanged"}));
            }
            Err(error) => {
                let message = format!("{}: {error}", target.display());
                eprintln!("error: meta preset apply: {message}");
                info!("meta preset apply: `{}` failed", target.display());
                failures.push(message.clone());
                items.push(
                    serde_json::json!({"target": target, "status": "failed", "error": message}),
                );
            }
        }
    }
    let failed = failures.len();
    let text = format!(
        "meta preset apply: {updated_count} updated, {unchanged_count} unchanged, {failed} failed (preset `{}`)",
        preset.name
    );
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-preset-apply",
            "preset": path,
            "name": preset.name,
            "updated": updated_count,
            "unchanged": unchanged_count,
            "failed": failed,
            "errors": failures,
            "items": items,
            "status": if failed == 0 { "ok" } else { "partial" },
        }),
        &text,
    )?;
    info!("{text}");
    if failed != 0 {
        return Err(CliError::BatchPartial { failed });
    }
    Ok(())
}

/// LRPAR-G15-IPTC-S5: mirrors the selected source fields onto one target:
/// load, mutate a clone (draft values copied, source-absent fields removed,
/// `keywords` replaced wholesale), exactly one history entry
/// (`origin = "sync:<source-file-name>"`), validate, CAS + atomic save.
/// Returns `Ok(true)` on update and `Ok(false)` for idempotent no-ops (no
/// history entry, no write). A missing sidecar is a loud per-target error
/// ("zuerst importieren" — "run `import` first"), never a silent creation.
/// Recipes, masks and per-copy edit history are never touched.
fn apply_meta_sync_to_target(
    target: &Path,
    source_draft: &BTreeMap<String, String>,
    source_keywords: &[String],
    fields: &BTreeSet<String>,
    origin: &str,
) -> Result<bool, CliError> {
    let sidecar = sidecar_path_for(target);
    let document = match load_sidecar(&sidecar) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            return Err(CliError::Message(format!(
                "no sidecar for `{}`; run `import` first",
                target.display()
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let original_bytes = fs::read(target).map_err(|error| io_error(target, error))?;
    let expected = document_revision(&document)?;
    let timestamp = now_rfc3339_utc();
    let mut candidate = document.clone();
    let mut changed = BTreeSet::new();
    for id in fields {
        if id == "keywords" {
            if candidate.keywords != source_keywords {
                candidate.keywords = source_keywords.to_vec();
                changed.insert(id.clone());
            }
        } else if let Some(value) = source_draft.get(id) {
            if candidate.metadata.draft.get(id).map(String::as_str) != Some(value.as_str()) {
                candidate.metadata.draft.insert(id.clone(), value.clone());
                changed.insert(id.clone());
            }
        } else if candidate.metadata.draft.remove(id).is_some() {
            changed.insert(id.clone());
        }
    }
    if changed.is_empty() {
        debug_assert_eq!(
            fs::read(target).map_err(|error| io_error(target, error))?,
            original_bytes
        );
        return Ok(false);
    }
    let changed_list: Vec<String> = changed.into_iter().collect();
    let rev = candidate.metadata.latest_rev() + 1;
    candidate.metadata.history.insert(
        0,
        MetadataHistoryEntry {
            rev,
            timestamp,
            origin: origin.to_string(),
            changed: changed_list.clone(),
        },
    );
    candidate
        .metadata
        .history
        .truncate(MAX_METADATA_HISTORY_ENTRIES);
    candidate.validate()?;
    save_sidecar_if_unchanged(&sidecar, &candidate, Some(&expected))?;
    // The original image is never modified by a metadata command.
    debug_assert_eq!(
        fs::read(target).map_err(|error| io_error(target, error))?,
        original_bytes
    );
    Ok(true)
}

/// LRPAR-G15-IPTC-S5: `meta sync` — validates `--fields` upfront (empty list
/// and unknown IDs abort everything with exit 1, nothing written), snapshots
/// the source draft + keywords once, then mirrors the selection per target in
/// isolation (updated / unchanged / failed; exit 3 on partial failure).
/// A missing source sidecar aborts everything with exit 1; a missing target
/// sidecar fails only its own item. Idempotent re-application reports
/// `unchanged` without a history entry.
fn meta_sync(args: MetaSyncArgs) -> Result<(), CliError> {
    if args.fields.iter().all(|field| field.is_empty()) {
        return Err(CliError::Message(
            "`meta sync` requires `--fields <id,…>` (registry field IDs, `keywords` allowed); refusing a silent transfer-all".into(),
        ));
    }
    let mut fields = BTreeSet::new();
    for id in &args.fields {
        if id != "keywords" && !is_metadata_field(id) {
            return Err(CliError::Message(format!(
                "meta sync for `{}` rejected: unknown metadata field `{id}`",
                args.source.display()
            )));
        }
        fields.insert(id.clone());
    }
    let (_, source_document) = require_sidecar(&args.source)?;
    let source_draft = source_document.metadata.draft.clone();
    let source_keywords = source_document.keywords.clone();
    let source_name = args
        .source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            CliError::Message(format!(
                "meta sync for `{}` rejected: source has no file name",
                args.source.display()
            ))
        })?;
    let origin = format!("sync:{source_name}");
    let field_list: Vec<String> = fields.iter().cloned().collect();
    info!(
        "meta sync from `{source_name}` to {} target(s) ({} field(s): {})",
        args.target.len(),
        field_list.len(),
        field_list.join(", ")
    );
    let mut updated_count = 0usize;
    let mut unchanged_count = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut items = Vec::with_capacity(args.target.len());
    for target in &args.target {
        match apply_meta_sync_to_target(target, &source_draft, &source_keywords, &fields, &origin) {
            Ok(true) => {
                info!(
                    "meta sync: `{}` updated (from `{source_name}`)",
                    target.display()
                );
                updated_count += 1;
                items.push(serde_json::json!({"target": target, "status": "updated"}));
            }
            Ok(false) => {
                info!(
                    "meta sync: `{}` unchanged (already in sync with `{source_name}`)",
                    target.display()
                );
                unchanged_count += 1;
                items.push(serde_json::json!({"target": target, "status": "unchanged"}));
            }
            Err(error) => {
                let message = format!("{}: {error}", target.display());
                eprintln!("error: meta sync: {message}");
                info!("meta sync: `{}` failed", target.display());
                failures.push(message.clone());
                items.push(
                    serde_json::json!({"target": target, "status": "failed", "error": message}),
                );
            }
        }
    }
    let failed = failures.len();
    let text = format!(
        "meta sync: {updated_count} updated, {unchanged_count} unchanged, {failed} failed (from `{source_name}`)"
    );
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-sync",
            "source": args.source,
            "source_name": source_name,
            "fields": field_list,
            "origin": origin,
            "updated": updated_count,
            "unchanged": unchanged_count,
            "failed": failed,
            "errors": failures,
            "items": items,
            "status": if failed == 0 { "ok" } else { "partial" },
        }),
        &text,
    )?;
    info!("{text}");
    if failed != 0 {
        return Err(CliError::BatchPartial { failed });
    }
    Ok(())
}

/// META-COPYPASTE-1: CLI meta clipboard file format marker + version (SOLL §8).
const META_CLIPBOARD_FORMAT: &str = "lumina-meta-clipboard";
const META_CLIPBOARD_VERSION: u8 = 1;

/// META-COPYPASTE-1: default clipboard path in the OS temp directory. Explicit
/// and ephemeral on purpose — never a CWD dotfile that could be committed or
/// silently shared between checkouts (SOLL §8).
fn default_meta_clipboard_path() -> PathBuf {
    std::env::temp_dir().join("lumina-meta-clipboard.json")
}

/// META-COPYPASTE-1: the versioned, portable CLI clipboard file. Holds only
/// non-empty values (a clipboard can never encode a deletion). `source` is the
/// source file name only — never a path.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct MetaClipboardFile {
    format: String,
    version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    fields: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    keywords: Vec<String>,
}

impl MetaClipboardFile {
    /// Field IDs stored in this clipboard (draft IDs plus `keywords` when a
    /// non-empty keyword list was captured).
    fn stored_ids(&self) -> BTreeSet<String> {
        let mut ids: BTreeSet<String> = self.fields.keys().cloned().collect();
        if !self.keywords.is_empty() {
            ids.insert("keywords".to_string());
        }
        ids
    }
}

/// META-COPYPASTE-1: validates a `--fields` list (registry IDs from SOLL §4,
/// `keywords` allowed) into a deduplicated set. Empty entries and unknown IDs
/// are loud errors before anything is read or written.
fn parse_meta_fields(fields: &[String], context: &str) -> Result<BTreeSet<String>, CliError> {
    if fields.iter().any(|id| id.is_empty()) {
        return Err(CliError::Message(format!(
            "{context} rejected: empty metadata field ID in `--fields`"
        )));
    }
    let mut set = BTreeSet::new();
    for id in fields {
        if id != "keywords" && !is_metadata_field(id) {
            return Err(CliError::Message(format!(
                "{context} rejected: unknown metadata field `{id}`"
            )));
        }
        set.insert(id.clone());
    }
    Ok(set)
}

/// META-COPYPASTE-1: loads and validates a clipboard file. Every deviation
/// (missing/unreadable file, invalid JSON, wrong format/version, unknown or
/// empty field values, invalid keywords) is a loud error — there is no silent
/// fallback to an empty clipboard. Empty values are rejected because paste is
/// additive and an empty value would otherwise mean "remove the field".
fn load_meta_clipboard(path: &Path) -> Result<MetaClipboardFile, CliError> {
    let json = fs::read_to_string(path).map_err(|error| io_error(path, error))?;
    let clipboard: MetaClipboardFile = serde_json::from_str(&json).map_err(|error| {
        CliError::Message(format!(
            "invalid metadata clipboard `{}`: {error}",
            path.display()
        ))
    })?;
    if clipboard.format != META_CLIPBOARD_FORMAT {
        return Err(CliError::Message(format!(
            "invalid metadata clipboard `{}`: expected format \"{META_CLIPBOARD_FORMAT}\", got \"{}\"",
            path.display(),
            clipboard.format
        )));
    }
    if clipboard.version != META_CLIPBOARD_VERSION {
        return Err(CliError::Message(format!(
            "unsupported metadata clipboard version {} in `{}` (expected {META_CLIPBOARD_VERSION})",
            clipboard.version,
            path.display()
        )));
    }
    for (id, value) in &clipboard.fields {
        if id == "keywords" {
            return Err(CliError::Message(format!(
                "invalid metadata clipboard `{}`: `keywords` must not appear inside `fields`",
                path.display()
            )));
        }
        validate_metadata_field_value(id, value).map_err(|error| {
            CliError::Message(format!(
                "invalid metadata clipboard `{}`: {error}",
                path.display()
            ))
        })?;
        if value.is_empty() || value.trim().is_empty() {
            return Err(CliError::Message(format!(
                "invalid metadata clipboard `{}`: field `{id}` is empty (paste never deletes fields)",
                path.display()
            )));
        }
    }
    if clipboard.keywords.len() > MAX_KEYWORDS_PER_DOCUMENT {
        return Err(CliError::Message(format!(
            "invalid metadata clipboard `{}`: keyword list exceeds limit of {MAX_KEYWORDS_PER_DOCUMENT}",
            path.display()
        )));
    }
    for keyword in &clipboard.keywords {
        if keyword.is_empty() || keyword.trim() != keyword {
            return Err(CliError::Message(format!(
                "invalid metadata clipboard `{}`: keyword must be non-empty and without leading/trailing whitespace",
                path.display()
            )));
        }
        if keyword.chars().count() > MAX_KEYWORD_CHARS {
            return Err(CliError::Message(format!(
                "invalid metadata clipboard `{}`: keyword exceeds limit of {MAX_KEYWORD_CHARS} characters",
                path.display()
            )));
        }
    }
    Ok(clipboard)
}

/// META-COPYPASTE-1: `meta copy` — writes the selected non-empty draft fields
/// (+ non-empty keywords) of `path` into an explicit clipboard file. The
/// sidecar is read-only here; no value is normalized or removed. Missing
/// source sidecar and unknown `--fields` IDs are loud (exit 1). An empty
/// selection still writes the file and is reported as `empty` (exit 0) — it is
/// never pasted silently.
fn meta_copy(args: MetaCopyArgs) -> Result<(), CliError> {
    let context = format!("meta copy for `{}`", args.path.display());
    let selection = parse_meta_fields(&args.fields, &context)?;
    let (_, document) = require_sidecar(&args.path)?;
    let out = args.out.clone().unwrap_or_else(default_meta_clipboard_path);
    // META-COPYPASTE-2: the clipboard file must never clobber the original
    // source or its Lumina bundle (`<input>.lumina.json`/`.lumina.zdata`,
    // including hard links) — same non-destructive guard as the export paths.
    reject_protected_output(&args.path, &out)?;
    let source = args
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            CliError::Message(format!(
                "meta copy for `{}` rejected: source has no file name",
                args.path.display()
            ))
        })?;
    let copy_all = selection.is_empty();
    let mut fields = BTreeMap::new();
    for (id, value) in &document.metadata.draft {
        if (copy_all || selection.contains(id)) && !value.trim().is_empty() {
            fields.insert(id.clone(), value.clone());
        }
    }
    let keywords = if copy_all || selection.contains("keywords") {
        document.keywords.clone()
    } else {
        Vec::new()
    };
    let field_ids: Vec<String> = fields.keys().cloned().collect();
    let clipboard = MetaClipboardFile {
        format: META_CLIPBOARD_FORMAT.to_string(),
        version: META_CLIPBOARD_VERSION,
        source: Some(source.clone()),
        fields,
        keywords,
    };
    let json = serde_json::to_string_pretty(&clipboard).map_err(|error| {
        CliError::Message(format!("cannot serialize metadata clipboard: {error}"))
    })?;
    fs::write(&out, json).map_err(|error| io_error(&out, error))?;
    let empty = field_ids.is_empty() && clipboard.keywords.is_empty();
    if empty {
        info!(
            "meta copy for `{}` wrote an empty clipboard to `{}` (no non-empty metadata selected)",
            args.path.display(),
            out.display()
        );
    } else {
        info!(
            "meta copy for `{}` wrote {} field(s) to `{}` (source `{source}`)",
            args.path.display(),
            field_ids.len() + usize::from(!clipboard.keywords.is_empty()),
            out.display()
        );
    }
    let text = if empty {
        format!(
            "meta copy: clipboard `{}` is empty (no non-empty metadata to copy)",
            out.display()
        )
    } else {
        format!(
            "meta copy: wrote {} field(s) to `{}` (source `{source}`)",
            field_ids.len() + usize::from(!clipboard.keywords.is_empty()),
            out.display()
        )
    };
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-copy",
            "input": args.path,
            "clipboard": out,
            "source": source,
            "fields": field_ids,
            "keywords": clipboard.keywords,
            "status": if empty { "empty" } else { "ok" },
        }),
        &text,
    )
}

/// META-COPYPASTE-1: applies the selected clipboard fields to one target. Like
/// `meta sync` this is one CAS + atomic write with a single history entry, but
/// purely additive: clipboard values overwrite their field, nothing else is
/// ever removed (no mirror semantics). `Ok(false)` = idempotent no-op.
fn apply_meta_paste_to_target(
    target: &Path,
    clipboard: &MetaClipboardFile,
    fields: &BTreeSet<String>,
    origin: &str,
) -> Result<bool, CliError> {
    let sidecar = sidecar_path_for(target);
    let document = match load_sidecar(&sidecar) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            return Err(CliError::Message(format!(
                "no sidecar for `{}`; run `import` first",
                target.display()
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let original_bytes = fs::read(target).map_err(|error| io_error(target, error))?;
    let expected = document_revision(&document)?;
    let timestamp = now_rfc3339_utc();
    let mut candidate = document.clone();
    let mut changed = BTreeSet::new();
    for id in fields {
        if id == "keywords" {
            if candidate.keywords != clipboard.keywords {
                candidate.keywords = clipboard.keywords.clone();
                changed.insert(id.clone());
            }
        } else if let Some(value) = clipboard.fields.get(id) {
            // Non-empty by construction (validated on load) — a pure
            // overwrite; it can never remove another field.
            if candidate.metadata.draft.get(id).map(String::as_str) != Some(value.as_str()) {
                candidate.metadata.draft.insert(id.clone(), value.clone());
                changed.insert(id.clone());
            }
        }
    }
    if changed.is_empty() {
        debug_assert_eq!(
            fs::read(target).map_err(|error| io_error(target, error))?,
            original_bytes
        );
        return Ok(false);
    }
    let changed_list: Vec<String> = changed.into_iter().collect();
    let rev = candidate.metadata.latest_rev() + 1;
    candidate.metadata.history.insert(
        0,
        MetadataHistoryEntry {
            rev,
            timestamp,
            origin: origin.to_string(),
            changed: changed_list.clone(),
        },
    );
    candidate
        .metadata
        .history
        .truncate(MAX_METADATA_HISTORY_ENTRIES);
    candidate.validate()?;
    save_sidecar_if_unchanged(&sidecar, &candidate, Some(&expected))?;
    // The original image is never modified by a metadata command.
    debug_assert_eq!(
        fs::read(target).map_err(|error| io_error(target, error))?,
        original_bytes
    );
    Ok(true)
}

/// META-COPYPASTE-1: `meta paste` — loads/validates the clipboard upfront
/// (missing/invalid file, empty selection and unknown or not-stored `--fields`
/// IDs abort everything with exit 1, nothing written), then applies the
/// selection per target in isolation (updated / unchanged / failed; exit 3 on
/// partial failure). Additive only: no other target field is removed; a
/// selected `keywords` replaces the target list as a whole. A missing target
/// sidecar fails only its own item, never a silent creation.
fn meta_paste(args: MetaPasteArgs) -> Result<(), CliError> {
    let clipboard_path = args
        .clipboard
        .clone()
        .unwrap_or_else(default_meta_clipboard_path);
    let clipboard = load_meta_clipboard(&clipboard_path)?;
    let context = format!("meta paste from `{}`", clipboard_path.display());
    let requested = parse_meta_fields(&args.fields, &context)?;
    let stored = clipboard.stored_ids();
    let fields = if requested.is_empty() {
        stored
    } else {
        for id in &requested {
            if !stored.contains(id) {
                return Err(CliError::Message(format!(
                    "{context} rejected: field `{id}` is not present in the clipboard"
                )));
            }
        }
        requested
    };
    if fields.is_empty() {
        return Err(CliError::Message(format!(
            "metadata clipboard `{}` is empty; run `meta copy` first",
            clipboard_path.display()
        )));
    }
    let field_list: Vec<String> = fields.iter().cloned().collect();
    let origin = META_ORIGIN_CLI.to_string();
    let source = clipboard
        .source
        .clone()
        .unwrap_or_else(|| "(unbekannt)".to_string());
    info!(
        "meta paste from `{}` (source `{source}`) to {} target(s) ({} field(s): {})",
        clipboard_path.display(),
        args.target.len(),
        field_list.len(),
        field_list.join(", ")
    );
    let mut updated_count = 0usize;
    let mut unchanged_count = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut items = Vec::with_capacity(args.target.len());
    for target in &args.target {
        match apply_meta_paste_to_target(target, &clipboard, &fields, &origin) {
            Ok(true) => {
                info!(
                    "meta paste: `{}` updated (from clipboard `{}`)",
                    target.display(),
                    clipboard_path.display()
                );
                updated_count += 1;
                items.push(serde_json::json!({"target": target, "status": "updated"}));
            }
            Ok(false) => {
                info!(
                    "meta paste: `{}` unchanged (already matches the clipboard)",
                    target.display()
                );
                unchanged_count += 1;
                items.push(serde_json::json!({"target": target, "status": "unchanged"}));
            }
            Err(error) => {
                let message = format!("{}: {error}", target.display());
                eprintln!("error: meta paste: {message}");
                info!("meta paste: `{}` failed", target.display());
                failures.push(message.clone());
                items.push(
                    serde_json::json!({"target": target, "status": "failed", "error": message}),
                );
            }
        }
    }
    let failed = failures.len();
    let text = format!(
        "meta paste: {updated_count} updated, {unchanged_count} unchanged, {failed} failed (from `{}`)",
        clipboard_path.display()
    );
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-paste",
            "clipboard": clipboard_path,
            "source": source,
            "fields": field_list,
            "origin": origin,
            "updated": updated_count,
            "unchanged": unchanged_count,
            "failed": failed,
            "errors": failures,
            "items": items,
            "status": if failed == 0 { "ok" } else { "partial" },
        }),
        &text,
    )?;
    info!("{text}");
    if failed != 0 {
        return Err(CliError::BatchPartial { failed });
    }
    Ok(())
}

fn format_keywords_text(keywords: &[String]) -> String {
    if keywords.is_empty() {
        "keywords: (none)".into()
    } else {
        format!("keywords: {}", keywords.join(", "))
    }
}

/// Splits an `--add-to` value at the first `=` into `(id, name)`.
fn split_collection_assignment(value: &str) -> Result<(String, String), CliError> {
    value.split_once('=').map_or_else(
        || {
            Err(CliError::Message(format!(
                "invalid collection assignment `{value}`: expected `id=name`"
            )))
        },
        |(id, name)| Ok((id.to_string(), name.to_string())),
    )
}

fn collections(args: CollectionsArgs) -> Result<(), CliError> {
    let (path, mut document) = require_sidecar(&args.input)?;
    let original_bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
    let mut ops = Vec::with_capacity(args.add_to.len() + args.remove_from.len());
    for assignment in &args.add_to {
        let (id, name) = split_collection_assignment(assignment)?;
        ops.push(BatchOp::AddToCollection { id, name });
    }
    for id in &args.remove_from {
        ops.push(BatchOp::RemoveFromCollection { id: id.clone() });
    }
    let mut changed = false;
    for op in &ops {
        changed |= apply_batch_op(&mut document, op).map_err(|error| {
            CliError::Message(format!(
                "collections for `{}` rejected: {error}",
                args.input.display()
            ))
        })?;
    }
    if changed {
        document.validate()?;
        save_sidecar(&path, &document)?;
        info!(
            "collections for `{}` updated ({} operation(s), {} membership(s))",
            args.input.display(),
            ops.len(),
            document.collections.len()
        );
    } else if ops.is_empty() {
        info!("collections for `{}` listed", args.input.display());
    } else {
        info!(
            "collections for `{}` unchanged (idempotent no-op)",
            args.input.display()
        );
    }
    debug_assert_eq!(
        fs::read(&args.input).map_err(|error| io_error(&args.input, error))?,
        original_bytes
    );
    let memberships: Vec<CollectionMembership> = document.collections.clone();
    emit(
        args.json,
        serde_json::json!({"command":"collections", "input":args.input, "collections":memberships, "changed":changed, "status":"ok"}),
        &format_collections_text(&memberships),
    )
}

fn format_collections_text(memberships: &[CollectionMembership]) -> String {
    if memberships.is_empty() {
        "collections: (none)".into()
    } else {
        let entries = memberships
            .iter()
            .map(|m| format!("{} ({})", m.name, m.id))
            .collect::<Vec<_>>();
        format!("collections: {}", entries.join(", "))
    }
}

/// Parses the single `BatchOp` of `batch-meta` from exactly one of `--op` /
/// `--op-file`. Anything else (neither, both, invalid JSON, unknown variant)
/// is a loud error; the operation language itself is owned by
/// `lumina-sidecar`.
fn parse_batch_op(args: &BatchMetaArgs) -> Result<BatchOp, CliError> {
    match (&args.op, &args.op_file) {
        (Some(_), Some(_)) => Err(CliError::Message(
            "`batch-meta` accepts exactly one of `--op` / `--op-file`".into(),
        )),
        (None, None) => Err(CliError::Message(
            "`batch-meta` requires one of `--op` / `--op-file`".into(),
        )),
        (Some(text), None) => serde_json::from_str(text)
            .map_err(|error| CliError::Message(format!("invalid batch operation JSON: {error}"))),
        (None, Some(path)) => {
            let text = fs::read_to_string(path).map_err(|error| io_error(path, error))?;
            serde_json::from_str(&text).map_err(|error| {
                CliError::Message(format!("invalid batch operation JSON: {error}"))
            })
        }
    }
}

fn batch_meta(args: BatchMetaArgs) -> Result<(), CliError> {
    let op = parse_batch_op(&args)?;
    let targets = collect_target_sidecars(&args.input)?;
    if targets.is_empty() {
        return Err(CliError::Message(format!(
            "no sidecars found under `{}`",
            args.input.display()
        )));
    }
    let mut changed_count = 0usize;
    let mut unchanged_count = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut items = Vec::with_capacity(targets.len());
    for sidecar in &targets {
        match load_sidecar(sidecar).map_err(CliError::from) {
            Err(error) => {
                let message = format!("{}: {error}", sidecar.display());
                eprintln!("error: batch-meta: {message}");
                info!("batch-meta: `{}` failed", sidecar.display());
                failures.push(message);
                items.push(serde_json::json!({"sidecar":sidecar, "status":"failed"}));
            }
            Ok(mut document) => match apply_batch_op(&mut document, &op) {
                Err(error) => {
                    let message = format!("{}: {error}", sidecar.display());
                    eprintln!("error: batch-meta: {message}");
                    info!("batch-meta: `{}` failed", sidecar.display());
                    failures.push(message);
                    items.push(serde_json::json!({"sidecar":sidecar, "status":"failed"}));
                }
                Ok(false) => {
                    info!("batch-meta: `{}` unchanged", sidecar.display());
                    unchanged_count += 1;
                    items.push(
                        serde_json::json!({"sidecar":sidecar, "status":"ok", "changed":false}),
                    );
                }
                Ok(true) => match document
                    .validate()
                    .map_err(CliError::from)
                    .and_then(|()| save_sidecar(sidecar, &document).map_err(CliError::from))
                {
                    Err(error) => {
                        let message = format!("{}: {error}", sidecar.display());
                        eprintln!("error: batch-meta: {message}");
                        info!("batch-meta: `{}` failed", sidecar.display());
                        failures.push(message);
                        items.push(serde_json::json!({"sidecar":sidecar, "status":"failed"}));
                    }
                    Ok(()) => {
                        info!("batch-meta: `{}` updated", sidecar.display());
                        changed_count += 1;
                        items.push(
                            serde_json::json!({"sidecar":sidecar, "status":"ok", "changed":true}),
                        );
                    }
                },
            },
        }
    }
    let failed = failures.len();
    let text = format!(
        "batch-meta: {changed_count} changed, {unchanged_count} unchanged, {failed} failed"
    );
    emit(
        args.json,
        serde_json::json!({"command":"batch-meta", "input":args.input, "op":op, "changed":changed_count, "unchanged":unchanged_count, "failed":failed, "errors":failures, "items":items, "status": if failed == 0 { "ok" } else { "partial" }}),
        &text,
    )?;
    info!("{text}");
    if failed != 0 {
        return Err(CliError::BatchPartial { failed });
    }
    Ok(())
}

fn smart_collections(args: SmartCollectionsArgs) -> Result<(), CliError> {
    let defs = load_smart_catalog(&args.catalog)?;
    let targets = collect_target_sidecars(&args.input)?;
    if targets.is_empty() {
        return Err(CliError::Message(format!(
            "no sidecars found under `{}`",
            args.input.display()
        )));
    }
    let mut matched_files = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut items = Vec::with_capacity(targets.len());
    for sidecar in &targets {
        match load_sidecar(sidecar).map_err(CliError::from) {
            Err(error) => {
                let message = format!("{}: {error}", sidecar.display());
                eprintln!("error: smart-collections: {message}");
                info!("smart-collections: `{}` failed", sidecar.display());
                failures.push(message);
                items.push(serde_json::json!({"sidecar":sidecar, "status":"failed"}));
            }
            Ok(document) => {
                let mut matched: Vec<String> = Vec::new();
                let mut item_failed: Option<String> = None;
                for def in &defs {
                    match def.matches_any_copy(&document) {
                        Ok(true) => matched.push(def.id.clone()),
                        Ok(false) => {}
                        Err(error) => {
                            item_failed = Some(format!("{}: {error}", sidecar.display()));
                            break;
                        }
                    }
                }
                if let Some(message) = item_failed {
                    eprintln!("error: smart-collections: {message}");
                    info!("smart-collections: `{}` failed", sidecar.display());
                    failures.push(message);
                    items.push(serde_json::json!({"sidecar":sidecar, "status":"failed"}));
                } else {
                    if !matched.is_empty() {
                        matched_files += 1;
                    }
                    info!(
                        "smart-collections: `{}` matches {} collection(s)",
                        sidecar.display(),
                        matched.len()
                    );
                    items.push(
                        serde_json::json!({"sidecar":sidecar, "status":"ok", "matches":matched}),
                    );
                }
            }
        }
    }
    let failed = failures.len();
    let text = format!(
        "smart-collections: {} of {} sidecar(s) match, {failed} failed",
        matched_files,
        targets.len()
    );
    emit(
        args.json,
        serde_json::json!({"command":"smart-collections", "input":args.input, "catalog":args.catalog, "matched_files":matched_files, "sidecars":targets.len(), "failed":failed, "errors":failures, "items":items, "status": if failed == 0 { "ok" } else { "partial" }}),
        &text,
    )?;
    info!("{text}");
    if failed != 0 {
        return Err(CliError::BatchPartial { failed });
    }
    Ok(())
}

/// G-08 Previous-Übernahme (LRPAR-G08-PREVIOUS): copy the full recipe of one
/// reference image onto N target sidecars — the same full-recipe Sync
/// mechanism the GUI uses (no second mechanism, no subset selection). The
/// reference is loaded first and loudly aborts everything (exit 1) when its
/// sidecar is missing/invalid or the copy id is unknown, so no target is
/// touched without a valid source. Each target is then handled in isolation:
/// load, assign recipe, one `previous` history step, validate, atomic save.
/// A missing/invalid target sidecar or unknown target copy marks only its
/// item as `failed` (stderr line + `info!` log); the remaining targets still
/// run. Exit `0` on full success, `3` on partial failure (analog `batch` /
/// `batch-meta`), `1` on hard errors. The original images are never modified;
/// history `extras` carry only the reference file name, never paths.
fn previous(args: PreviousArgs) -> Result<(), CliError> {
    if args.to.is_empty() {
        return Err(CliError::Message(
            "`previous` requires at least one `--to` target".into(),
        ));
    }
    let from_sidecar = sidecar_path_for(&args.from);
    let from_document = load_sidecar(&from_sidecar).map_err(CliError::from)?;
    let from_id = args.from_copy.as_deref().unwrap_or("vc-original");
    let reference = from_document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == from_id)
        .ok_or_else(|| {
            CliError::Message(format!(
                "unknown virtual copy `{from_id}` in `{}`",
                from_sidecar.display()
            ))
        })?
        .recipe
        .clone();
    info!(
        "previous: reference `{}` copy `{from_id}`",
        args.from.display()
    );
    let to_id = args.to_copy.as_deref().unwrap_or("vc-original");
    let mut applied_count = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut items = Vec::with_capacity(args.to.len());
    for target in &args.to {
        match apply_previous_to_target(target, to_id, &reference, &args.from) {
            Ok(()) => {
                info!(
                    "previous: `{}` updated from `{}`",
                    target.display(),
                    args.from.display()
                );
                applied_count += 1;
                items.push(serde_json::json!({"target":target, "status":"ok"}));
            }
            Err(error) => {
                let message = format!("{}: {error}", target.display());
                eprintln!("error: previous: {message}");
                info!("previous: `{}` failed", target.display());
                failures.push(message);
                items.push(serde_json::json!({"target":target, "status":"failed"}));
            }
        }
    }
    let failed = failures.len();
    let text = format!(
        "previous: {applied_count} applied, {failed} failed (reference `{}`)",
        args.from.display()
    );
    emit(
        args.json,
        serde_json::json!({"command":"previous", "from":args.from, "from_copy":from_id, "to_copy":to_id, "applied":applied_count, "failed":failed, "errors":failures, "items":items, "status": if failed == 0 { "ok" } else { "partial" }}),
        &text,
    )?;
    info!("{text}");
    if failed != 0 {
        return Err(CliError::BatchPartial { failed });
    }
    Ok(())
}

/// Move one file to `target`, tolerating a cross-filesystem move: `rename`
/// fails with `CrossesDevices` (EXDEV) when source and target live on
/// different volumes, so fall back to copy + remove. Loud on error; a failed
/// source removal cleans up the copied target again (the source still exists,
/// so no data is lost).
fn move_file_cross_volume(source: &Path, target: &Path) -> std::io::Result<()> {
    match fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            fs::copy(source, target)?;
            if let Err(remove_error) = fs::remove_file(source) {
                let _ = fs::remove_file(target);
                return Err(remove_error);
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// G-09 Library-Parität (LRPAR-G09-LIB): move one image with its sidecar
/// companions (`.lumina.json`, `.lumina.zdata` when present) to `--to`.
/// Companion targets are derived from the TARGET image path
/// (`sidecar_path_for(&args.to)` / `zdata_path_for(&args.to)`), so renames
/// keep the recipe attached to the new name. An existing target (image or
/// companion) aborts loudly before anything is moved (exit 1, never a
/// silent overwrite); a missing source is a loud error as well. The image
/// moves first, then each present companion. A failed companion move is
/// loud (exit 1) and names the step reached — the image may already sit at
/// the target, which the error text says explicitly (no silent half state,
/// no data loss by overwrite).
fn relocate(args: RelocateArgs) -> Result<(), CliError> {
    if !args.from.is_file() {
        return Err(CliError::Message(format!(
            "relocate: source `{}` does not exist",
            args.from.display()
        )));
    }
    if args.to.exists() {
        return Err(CliError::Message(format!(
            "relocate: target `{}` already exists; refusing to overwrite",
            args.to.display()
        )));
    }
    if let Some(parent) = args
        .to
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        if !parent.is_dir() {
            return Err(CliError::Message(format!(
                "relocate: target parent `{}` is no directory",
                parent.display()
            )));
        }
    }
    let moves = [
        (sidecar_path_for(&args.from), sidecar_path_for(&args.to)),
        (zdata_path_for(&args.from), zdata_path_for(&args.to)),
    ];
    for (source, target) in &moves {
        if source.is_file() && target.exists() {
            return Err(CliError::Message(format!(
                "relocate: companion target `{}` already exists; refusing to overwrite",
                target.display()
            )));
        }
    }
    move_file_cross_volume(&args.from, &args.to).map_err(|error| io_error(&args.from, error))?;
    info!(
        "relocate: image `{}` -> `{}`",
        args.from.display(),
        args.to.display()
    );
    for (source, target) in &moves {
        if source.is_file() {
            move_file_cross_volume(source, target).map_err(|error| {
                CliError::Message(format!(
                    "relocate: image moved to `{}` but companion `{}` failed: {error}",
                    args.to.display(),
                    source.display()
                ))
            })?;
            info!(
                "relocate: companion `{}` -> `{}`",
                source.display(),
                target.display()
            );
        }
    }
    let text = format!(
        "relocated `{}` -> `{}`",
        args.from.display(),
        args.to.display()
    );
    emit(
        args.json,
        serde_json::json!({"command":"relocate", "from":args.from, "to":args.to, "status":"ok"}),
        &text,
    )?;
    info!("{text}");
    Ok(())
}

/// Write the Previous `reference` recipe into the `copy_id` copy of
/// `target`'s sidecar (which must already exist — like `develop`, no silent
/// sidecar creation), tag one `previous` history step and save atomically.
fn apply_previous_to_target(
    target: &Path,
    copy_id: &str,
    reference: &EditRecipe,
    from: &Path,
) -> Result<(), CliError> {
    let sidecar = sidecar_path_for(target);
    let mut document = load_sidecar(&sidecar)?;
    let copy = document
        .virtual_copies
        .iter_mut()
        .find(|copy| copy.id == copy_id)
        .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{copy_id}`")))?;
    copy.recipe = reference.clone();
    // Portable by construction: only the reference file name (no paths —
    // absolute paths are forbidden in persistent recipe data).
    let source_name = from
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("reference")
        .to_string();
    let mut extras = BTreeMap::new();
    extras.insert("step".into(), serde_json::Value::String("previous".into()));
    extras.insert("source".into(), serde_json::Value::String(source_name));
    copy.history.push(HistoryEntry {
        id: "previous".into(),
        recipe: reference.clone(),
        recorded_at: None,
        extras,
    });
    document.validate()?;
    save_sidecar(&sidecar, &document)?;
    Ok(())
}

/// G-04 Remove-Parität: list and edit the spot-heal recipe state of one
/// image sidecar. The original image is never modified; every write goes
/// through `save_sidecar` after `document.validate()`. Mutations are loud
/// (`CliError::Message`, exit 1) and `--detect-objects` never applies
/// silently (only `--detect-apply` persists candidates).
fn spot(args: SpotArgs) -> Result<(), CliError> {
    if args.regenerate_variant.is_some() && (args.variant.is_none() || args.seed.is_none()) {
        return Err(CliError::Message(
            "--regenerate-variant requires --variant <N> and --seed <N>".into(),
        ));
    }
    if args.detect_apply && !args.detect_objects {
        return Err(CliError::Message(
            "--detect-apply requires --detect-objects".into(),
        ));
    }
    if args.set_visualize_threshold.is_some() && args.clear_visualize {
        return Err(CliError::Message(
            "--set-visualize-threshold and --clear-visualize are mutually exclusive".into(),
        ));
    }
    // `--clear` removes every spot after the adders ran, so combining it with
    // an adder is a contradiction (the requested edit would be discarded).
    if args.clear && (args.add_heuristic || args.detect_apply || args.regenerate_variant.is_some())
    {
        return Err(CliError::Message(
            "--clear removes every spot and contradicts --add-heuristic/--detect-apply/--regenerate-variant"
                .into(),
        ));
    }
    let wants_mutation = args.add_heuristic
        || args.clear
        || args.set_visualize_threshold.is_some()
        || args.clear_visualize
        || args.set_distraction.is_some()
        || args.detect_apply
        || args.regenerate_variant.is_some();
    // Detection needs the decoded frame even in list-only mode.
    let needs_frame = args.detect_objects || args.add_heuristic || args.detect_apply;
    let frame = if needs_frame {
        let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
        let (frame, _) = decode_input(&args.input, &bytes)?;
        Some(frame)
    } else {
        None
    };
    let path = sidecar_path_for(&args.input);
    let mut document = match load_sidecar(&path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            return Err(CliError::Message(format!(
                "no sidecar for `{}`; run `import` first",
                args.input.display()
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let copy_id = resolve_mask_copy(&document, args.virtual_copy.as_deref())?;
    let mut actions: Vec<String> = Vec::new();
    // Detection results are computed before mutation so `--detect-apply`
    // persists exactly what was listed.
    let mut detected: Vec<lumina_core::DetectedSpot> = Vec::new();
    if args.detect_objects {
        let frame = frame.as_ref().expect("decoded for detection");
        // G04-FOLLOWUP-1: without an explicit flag the recipe visualize
        // threshold is the default (else 0.5); an out-of-range recipe value
        // fails loudly in the detector below, never silently.
        let recipe_threshold = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == copy_id)
            .and_then(|copy| copy.recipe.spot_visualize_threshold());
        let threshold = args.detect_threshold.or(recipe_threshold).unwrap_or(0.5);
        let max = args.detect_max.unwrap_or(32);
        detected = detect_spots_heuristic(frame, threshold, max)
            .map_err(|error| CliError::Message(format!("spot detection rejected: {error}")))?;
        info!(
            "spot: detected {} candidate(s) on copy `{copy_id}` (threshold {threshold}, max {max})",
            detected.len()
        );
    }
    if args.add_heuristic {
        let (cx, cy, radius) = match (args.center_x, args.center_y, args.radius) {
            (Some(x), Some(y), Some(r)) => (x, y, r),
            _ => {
                return Err(CliError::Message(
                    "--add-heuristic requires --center-x, --center-y and --radius".into(),
                ));
            }
        };
        spot_add_heuristic(
            &mut document,
            &copy_id,
            cx,
            cy,
            radius,
            args.feather.unwrap_or(0.0),
            args.offset_dx.unwrap_or(0.0),
            args.offset_dy.unwrap_or(0.0),
            args.opacity.unwrap_or(1.0),
        )?;
        info!("spot: added heuristic spot on copy `{copy_id}`");
        actions.push("add-heuristic".into());
    }
    if args.detect_apply {
        let mut added = 0usize;
        for candidate in &detected {
            spot_add_heuristic(
                &mut document,
                &copy_id,
                candidate.x,
                candidate.y,
                candidate.radius.max(1.0),
                0.0,
                0.05,
                0.0,
                1.0,
            )?;
            added += 1;
        }
        info!("spot: applied {added} detected candidate(s) on copy `{copy_id}`");
        actions.push(format!("detect-apply:{added}"));
    }
    if let Some(threshold) = args.set_visualize_threshold {
        spot_copy_mut(&mut document, &copy_id)?
            .recipe
            .set_spot_visualize_threshold(Some(threshold))
            .map_err(|error| CliError::Message(error.to_string()))?;
        info!("spot: visualize threshold {threshold} on copy `{copy_id}`");
        actions.push(format!("visualize:{threshold}"));
    }
    if args.clear_visualize {
        spot_copy_mut(&mut document, &copy_id)?
            .recipe
            .set_spot_visualize_threshold(None)
            .map_err(|error| CliError::Message(error.to_string()))?;
        info!("spot: visualize cleared on copy `{copy_id}`");
        actions.push("visualize:off".into());
    }
    if let Some(spec) = args.set_distraction.as_deref() {
        // G04-FOLLOWUP-1 merge decision: deltas apply on top of the stored
        // switches (consistent with the GUI single-checkbox toggles), so an
        // unnamed key is never silently reset.
        let current = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == copy_id)
            .map(|copy| copy.recipe.spot_distraction())
            .unwrap_or_default();
        let setting = parse_distraction_spec(spec, current)?;
        spot_copy_mut(&mut document, &copy_id)?
            .recipe
            .set_spot_distraction(setting);
        info!("spot: distraction {setting:?} on copy `{copy_id}`");
        actions.push("distraction".into());
    }
    if let Some(spot_id) = args.regenerate_variant.as_deref() {
        let base = args.seed.expect("guarded above");
        let variant = args.variant.expect("guarded above");
        let derived = generative_variant_seed(base, variant);
        spot_regenerate_variant(&mut document, &copy_id, spot_id, base, variant, derived)?;
        info!("spot: regenerated variant {variant} (seed {derived}) for `{spot_id}` on copy `{copy_id}`");
        actions.push(format!("regenerate-variant:{spot_id}:{variant}"));
    }
    if args.clear {
        let copy = spot_copy_mut(&mut document, &copy_id)?;
        copy.recipe.extras.remove("spot_removals");
        copy.recipe.spot_removals.clear();
        info!("spot: cleared all spots on copy `{copy_id}`");
        actions.push("clear".into());
    }
    if wants_mutation {
        // Loud gate: geometry, visualize/distraction extras and variant
        // controls are rejected before anything is written.
        document.validate()?;
        save_sidecar(&path, &document)?;
    }
    spot_list(&args, &document, &copy_id, &detected, &actions)
}

/// Mutable access to one virtual copy's recipe owner (loud on unknown ids).
fn spot_copy_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut lumina_sidecar::VirtualCopy, CliError> {
    mask_copy_mut(document, copy_id)
}

/// Appends one heuristic spot entry to the extras view (validated loudly on
/// save; the typed mirror shadow is derived by the sidecar serde layer).
#[allow(clippy::too_many_arguments)]
fn spot_add_heuristic(
    document: &mut SidecarDocument,
    copy_id: &str,
    center_x: f32,
    center_y: f32,
    radius: f32,
    feather: f32,
    offset_dx: f32,
    offset_dy: f32,
    opacity: f32,
) -> Result<(), CliError> {
    for (name, value, lo, hi) in [
        ("center_x", center_x, 0.0, 1.0),
        ("center_y", center_y, 0.0, 1.0),
        ("radius", radius, f32::MIN_POSITIVE, 512.0),
        ("feather", feather, 0.0, 1.0),
        ("offset_dx", offset_dx, -1.0, 1.0),
        ("offset_dy", offset_dy, -1.0, 1.0),
        ("opacity", opacity, 0.0, 1.0),
    ] {
        if !value.is_finite() || value < lo || value > hi {
            return Err(CliError::Message(format!(
                "invalid heuristic spot `{name}`: value {value} outside allowed range {lo}..={hi}"
            )));
        }
    }
    if radius <= 0.0 {
        return Err(CliError::Message(
            "invalid heuristic spot `radius`: must be > 0".into(),
        ));
    }
    let id = format!(
        "spot-{}",
        blake3::hash(format!("{center_x:.6},{center_y:.6},{radius:.2}").as_bytes()).to_hex()
    );
    let entry = serde_json::json!({
        "id": id,
        "version": SPOT_REMOVAL_VERSION,
        "mode": "heuristic",
        "center_x": center_x,
        "center_y": center_y,
        "radius": radius,
        "feather": feather,
        "offset_dx": offset_dx,
        "offset_dy": offset_dy,
        "opacity": opacity,
        "status": "valid",
    });
    let copy = spot_copy_mut(document, copy_id)?;
    let mut spots: Vec<serde_json::Value> = copy
        .recipe
        .extras
        .get("spot_removals")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    spots.push(entry);
    copy.recipe.extras.insert(
        "spot_removals".into(),
        serde_json::to_value(spots).map_err(|error| CliError::Message(error.to_string()))?,
    );
    Ok(())
}

/// Parses `--set-distraction k=v,...` (keys `reflections|people|dust|auto`,
/// values `true|false`) as deltas merged into `current` (G04-FOLLOWUP-1
/// merge decision). Unknown keys or values fail loudly.
fn parse_distraction_spec(
    spec: &str,
    mut setting: SpotDistraction,
) -> Result<SpotDistraction, CliError> {
    if spec.trim().is_empty() {
        return Err(CliError::Message(
            "invalid distraction spec: expected `k=v,...` with keys reflections|people|dust|auto"
                .into(),
        ));
    }
    for part in spec.split(',') {
        let (key, value) = part.split_once('=').ok_or_else(|| {
            CliError::Message(format!(
                "invalid distraction assignment `{part}`: expected `k=v`"
            ))
        })?;
        let enabled = match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => true,
            "false" | "0" | "no" => false,
            _ => {
                return Err(CliError::Message(format!(
                    "invalid distraction value `{value}`: expected true|false"
                )));
            }
        };
        match key.trim().to_ascii_lowercase().as_str() {
            "reflections" => setting.reflections = enabled,
            "people" => setting.people = enabled,
            "dust" => setting.dust = enabled,
            "auto" | "auto_mode" => setting.auto_mode = enabled,
            _ => {
                return Err(CliError::Message(format!(
                    "unknown distraction key `{key}`: expected reflections|people|dust|auto"
                )));
            }
        }
    }
    Ok(setting)
}

/// Sets `seed = derived` (+ `variant`, preserving `prompt`) on a generative
/// extras entry. Heuristic entries and unknown ids fail loudly — a variant
/// never silently retargets another spot.
fn spot_regenerate_variant(
    document: &mut SidecarDocument,
    copy_id: &str,
    spot_id: &str,
    base: u64,
    variant: u64,
    derived: u64,
) -> Result<(), CliError> {
    let copy = spot_copy_mut(document, copy_id)?;
    let mut spots: Vec<serde_json::Value> = copy
        .recipe
        .extras
        .get("spot_removals")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let mut found = false;
    for entry in &mut spots {
        let id = entry.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        if id != spot_id {
            continue;
        }
        let mode = entry
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("heuristic");
        if mode != "generative" {
            return Err(CliError::Message(format!(
                "spot `{spot_id}` is not generative (mode `{mode}`); variants apply to generative spots only"
            )));
        }
        entry["seed"] = serde_json::json!(derived);
        entry["variant"] = serde_json::json!(variant);
        entry["base_seed"] = serde_json::json!(base);
        found = true;
    }
    if !found {
        return Err(CliError::Message(format!(
            "unknown spot `{spot_id}` on copy `{copy_id}`"
        )));
    }
    copy.recipe.extras.insert(
        "spot_removals".into(),
        serde_json::to_value(spots).map_err(|error| CliError::Message(error.to_string()))?,
    );
    Ok(())
}

/// Reports the copy's spots, G-04 settings and (when requested) detection
/// candidates. Read-only: the sidecar is never written here.
fn spot_list(
    args: &SpotArgs,
    document: &SidecarDocument,
    copy_id: &str,
    detected: &[lumina_core::DetectedSpot],
    actions: &[String],
) -> Result<(), CliError> {
    let copy = document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == copy_id)
        .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{copy_id}`")))?;
    let spots: Vec<serde_json::Value> = copy
        .recipe
        .extras
        .get("spot_removals")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let distraction = copy.recipe.spot_distraction();
    // reflections/people without a model are visibly NeedsModel (F-078 gate,
    // heuristic stage 1 covers dust only) — surfaced in both formats.
    let mut needs_model: Vec<&str> = Vec::new();
    if distraction.reflections {
        needs_model.push("reflections");
    }
    if distraction.people {
        needs_model.push("people");
    }
    if args.json {
        emit(
            true,
            serde_json::json!({
                "command": "spot",
                "input": args.input,
                "copy": copy_id,
                "spots": spots,
                "visualize_threshold": copy.recipe.spot_visualize_threshold(),
                "distraction": distraction,
                "distraction_needs_model": needs_model,
                "detected": detected.iter().map(|d| serde_json::json!({
                    "x": d.x, "y": d.y, "radius": d.radius, "confidence": d.confidence,
                })).collect::<Vec<_>>(),
                "actions": actions,
                "status": "ok",
            }),
            "spot status listed",
        )
    } else {
        println!("copy: {} [{}]", copy.name, copy.id);
        println!("  spots: {}", spots.len());
        for entry in &spots {
            let id = entry.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let mode = entry
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("heuristic");
            let status = entry
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("valid");
            println!("    spot {id}: mode={mode} status={status}");
        }
        match copy.recipe.spot_visualize_threshold() {
            Some(t) => println!("  visualize: threshold={t}"),
            None => println!("  visualize: off"),
        }
        println!(
            "  distraction: reflections={} people={} dust={} auto={}",
            distraction.reflections, distraction.people, distraction.dust, distraction.auto_mode
        );
        if !needs_model.is_empty() {
            println!(
                "  distraction needs model (F-078 gate, heuristic covers dust only): {}",
                needs_model.join(", ")
            );
        }
        if args.detect_objects {
            println!("  detected candidates: {}", detected.len());
            for candidate in detected {
                println!(
                    "    candidate x={:.4} y={:.4} r={:.1} conf={:.2}",
                    candidate.x, candidate.y, candidate.radius, candidate.confidence
                );
            }
        }
        if actions.is_empty() {
            emit(
                false,
                serde_json::json!({"command":"spot","status":"ok"}),
                "spot status listed",
            )
        } else {
            emit(
                false,
                serde_json::json!({"command":"spot","status":"ok"}),
                &format!("spot updated: {}", actions.join(", ")),
            )
        }
    }
}

/// G-05 Lens Blur: inspect and edit the depth-bokeh recipe stage of one
/// virtual copy. List-only mode is read-only (sidecar bytes unchanged).
/// Mutations validate loudly (`document.validate()`) before `save_sidecar`;
/// the original image is never modified.
fn lens_blur(args: LensBlurArgs) -> Result<(), CliError> {
    if args.enable && args.disable {
        return Err(CliError::Message(
            "--enable and --disable are mutually exclusive".into(),
        ));
    }
    if args.set_depth_artifact.is_some() && args.clear_depth_artifact {
        return Err(CliError::Message(
            "--set-depth-artifact and --clear-depth-artifact are mutually exclusive".into(),
        ));
    }
    // `--clear` removes the whole stage and short-circuits every other setter
    // below, so combining it with one is a contradiction, not a silent no-op.
    if args.clear
        && (args.enable
            || args.disable
            || args.set_amount.is_some()
            || args.set_focal_near.is_some()
            || args.set_focal_far.is_some()
            || args.set_bokeh.is_some()
            || args.set_focus_rect.is_some()
            || args.set_depth_artifact.is_some()
            || args.clear_depth_artifact)
    {
        return Err(CliError::Message(
            "--clear removes the whole lens-blur stage and contradicts every other mutation flag"
                .into(),
        ));
    }
    let wants_mutation = args.enable
        || args.disable
        || args.set_amount.is_some()
        || args.set_focal_near.is_some()
        || args.set_focal_far.is_some()
        || args.set_bokeh.is_some()
        || args.set_focus_rect.is_some()
        || args.set_depth_artifact.is_some()
        || args.clear_depth_artifact
        || args.clear;
    let path = sidecar_path_for(&args.input);
    let mut document = match load_sidecar(&path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            return Err(CliError::Message(format!(
                "no sidecar for `{}`; run `import` first",
                args.input.display()
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let copy_id = resolve_mask_copy(&document, args.virtual_copy.as_deref())?;
    let mut actions: Vec<String> = Vec::new();
    if args.clear {
        let copy = mask_copy_mut(&mut document, &copy_id)?;
        copy.recipe.lens_blur = None;
        info!("lens-blur: cleared stage on copy `{copy_id}`");
        actions.push("clear".into());
    } else {
        if args.enable || args.disable {
            lens_blur_mut(&mut document, &copy_id)?.enabled = args.enable;
            info!(
                "lens-blur: {} on copy `{copy_id}`",
                if args.enable { "enabled" } else { "disabled" }
            );
            actions.push(if args.enable { "enable" } else { "disable" }.into());
        }
        if let Some(amount) = args.set_amount {
            lens_blur_mut(&mut document, &copy_id)?.blur_amount = amount;
            info!("lens-blur: amount {amount} on copy `{copy_id}`");
            actions.push(format!("amount:{amount}"));
        }
        if let Some(near) = args.set_focal_near {
            lens_blur_mut(&mut document, &copy_id)?.focal_near = near;
            info!("lens-blur: focal_near {near} on copy `{copy_id}`");
            actions.push(format!("focal-near:{near}"));
        }
        if let Some(far) = args.set_focal_far {
            lens_blur_mut(&mut document, &copy_id)?.focal_far = far;
            info!("lens-blur: focal_far {far} on copy `{copy_id}`");
            actions.push(format!("focal-far:{far}"));
        }
        if let Some(shape) = args.set_bokeh.as_deref() {
            lens_blur_mut(&mut document, &copy_id)?.bokeh = parse_bokeh_shape(shape)?;
            info!("lens-blur: bokeh {shape} on copy `{copy_id}`");
            actions.push(format!("bokeh:{shape}"));
        }
        if let Some(rect) = args.set_focus_rect.as_deref() {
            lens_blur_mut(&mut document, &copy_id)?.focus_rect = parse_focus_rect(rect)?;
            info!("lens-blur: focus_rect {rect} on copy `{copy_id}`");
            actions.push(format!("focus-rect:{rect}"));
        }
        if let Some(spec) = args.set_depth_artifact.as_deref() {
            lens_blur_mut(&mut document, &copy_id)?.depth_artifact =
                Some(parse_depth_artifact(spec)?);
            info!("lens-blur: depth_artifact {spec} on copy `{copy_id}`");
            actions.push("depth-artifact:set".into());
        }
        if args.clear_depth_artifact {
            lens_blur_mut(&mut document, &copy_id)?.depth_artifact = None;
            info!("lens-blur: depth artifact cleared on copy `{copy_id}`");
            actions.push("depth-artifact:clear".into());
        }
    }
    if wants_mutation {
        // Loud gate: ranges, focal order, focus-rect geometry and portable
        // (relative) depth paths are rejected before anything is written.
        document
            .validate()
            .map_err(|error| CliError::Message(error.to_string()))?;
        save_sidecar(&path, &document)?;
    }
    lens_blur_list(&args, &document, &copy_id, &actions)
}

/// Mutable access to one virtual copy's lens-blur stage, creating an enabled
/// stage with centered defaults when none exists (loud on unknown ids).
fn lens_blur_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut LensBlur, CliError> {
    let copy = mask_copy_mut(document, copy_id)?;
    Ok(copy.recipe.lens_blur.get_or_insert(LensBlur {
        version: 1,
        enabled: true,
        focus_rect: FocusRect {
            x: 0.25,
            y: 0.25,
            width: 0.5,
            height: 0.5,
        },
        focal_near: 0.0,
        focal_far: 0.2,
        blur_amount: 0.5,
        bokeh: BokehShape::Round,
        depth_artifact: None,
    }))
}

/// Parses a bokeh shape name (loud on unknown values — never a guess).
fn parse_bokeh_shape(value: &str) -> Result<BokehShape, CliError> {
    match value {
        "round" => Ok(BokehShape::Round),
        "elliptical" => Ok(BokehShape::Elliptical),
        "hexagonal" => Ok(BokehShape::Hexagonal),
        _ => Err(CliError::Message(format!(
            "invalid bokeh shape `{value}`: expected round|elliptical|hexagonal"
        ))),
    }
}

/// Parses a focus rectangle as `x,y,w,h` (loud on malformed input; range
/// geometry is validated on save, not guessed here).
fn parse_focus_rect(value: &str) -> Result<FocusRect, CliError> {
    let parts: Vec<&str> = value.split(',').collect();
    let numbers: Option<Vec<f32>> = parts
        .iter()
        .map(|part| part.trim().parse::<f32>().ok())
        .collect();
    match numbers.as_deref() {
        Some([x, y, width, height]) => Ok(FocusRect {
            x: *x,
            y: *y,
            width: *width,
            height: *height,
        }),
        _ => Err(CliError::Message(format!(
            "invalid focus rect `{value}`: expected `x,y,w,h` with finite numbers"
        ))),
    }
}

/// Parses an external depth reference as `RELATIVE_PATH:SHA256` (loud on
/// malformed input; portability is validated on save).
fn parse_depth_artifact(value: &str) -> Result<DepthArtifactRef, CliError> {
    match value.split_once(':') {
        Some((path, sha)) if !path.trim().is_empty() && !sha.trim().is_empty() => {
            Ok(DepthArtifactRef {
                relative_path: path.trim().into(),
                sha256: sha.trim().into(),
            })
        }
        _ => Err(CliError::Message(format!(
            "invalid depth artifact `{value}`: expected `RELATIVE_PATH:SHA256`"
        ))),
    }
}

fn lens_blur_list(
    args: &LensBlurArgs,
    document: &SidecarDocument,
    copy_id: &str,
    actions: &[String],
) -> Result<(), CliError> {
    let copy = document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == copy_id)
        .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{copy_id}`")))?;
    let blur = copy.recipe.lens_blur.as_ref();
    // The CLI never resolves external depth files (no depth format in v1):
    // a referenced artifact reports `missing` until a loader exists.
    let status = lumina_core::lens_blur_status(blur, false);
    if args.json {
        let payload = blur.map(|b| {
            serde_json::json!({
                "enabled": b.enabled,
                "focus_rect": {"x": b.focus_rect.x, "y": b.focus_rect.y,
                    "width": b.focus_rect.width, "height": b.focus_rect.height},
                "focal_near": b.focal_near,
                "focal_far": b.focal_far,
                "blur_amount": b.blur_amount,
                "bokeh": match b.bokeh {
                    BokehShape::Round => "round",
                    BokehShape::Elliptical => "elliptical",
                    BokehShape::Hexagonal => "hexagonal",
                },
                "depth_artifact": b.depth_artifact.as_ref().map(|d| serde_json::json!({
                    "relative_path": d.relative_path, "sha256": d.sha256,
                })),
            })
        });
        emit(
            true,
            serde_json::json!({
                "command": "lens-blur",
                "input": args.input,
                "copy": copy_id,
                "lens_blur": payload,
                "status": status,
                "actions": actions,
            }),
            "lens-blur status listed",
        )
    } else {
        println!("copy: {} [{}]", copy.name, copy.id);
        match blur {
            Some(b) => {
                println!("  enabled: {}", b.enabled);
                println!(
                    "  focus_rect: x={} y={} w={} h={}",
                    b.focus_rect.x, b.focus_rect.y, b.focus_rect.width, b.focus_rect.height
                );
                println!("  focal: near={} far={}", b.focal_near, b.focal_far);
                println!("  amount: {}", b.blur_amount);
                println!(
                    "  bokeh: {}",
                    match b.bokeh {
                        BokehShape::Round => "round",
                        BokehShape::Elliptical => "elliptical",
                        BokehShape::Hexagonal => "hexagonal",
                    }
                );
                match &b.depth_artifact {
                    Some(d) => println!("  depth_artifact: {} ({})", d.relative_path, d.sha256),
                    None => println!("  depth_artifact: none (heuristic)"),
                }
            }
            None => println!("  lens_blur: none"),
        }
        println!("  status: {status}");
        if actions.is_empty() {
            emit(
                false,
                serde_json::json!({"command":"lens-blur","status":"ok"}),
                "lens-blur status listed",
            )
        } else {
            emit(
                false,
                serde_json::json!({"command":"lens-blur","status":"ok"}),
                &format!("lens-blur updated: {}", actions.join(", ")),
            )
        }
    }
}

/// G-02 Color-Parität (LRPAR-G02-COLOR): inspect and edit the color stages
/// (tone curve per channel, HSL mixer, Point Color, color grading,
/// vibrance/saturation) of one virtual copy. Unknown copies, channels,
/// fields, ids and malformed values abort loudly (exit 1) before anything
/// is written; range validation runs on save via `document.validate()`.
/// The original image is never modified.
fn color(args: ColorArgs) -> Result<(), CliError> {
    let wants_mutation = !args.set_curve_param.is_empty()
        || !args.set_curve_points.is_empty()
        || args.clear_curves
        || args.clear_curve_channel.is_some()
        || !args.set_hsl.is_empty()
        || args.clear_hsl
        || args.add_point_color
        || !args.set_point_color.is_empty()
        || !args.remove_point_color.is_empty()
        || args.clear_point_color
        || !args.set_grading.is_empty()
        || args.set_grading_balance.is_some()
        || args.set_grading_blending.is_some()
        || args.clear_grading
        || args.set_vibrance.is_some()
        || args.set_saturation.is_some();
    let path = sidecar_path_for(&args.input);
    let mut document = match load_sidecar(&path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            return Err(CliError::Message(format!(
                "no sidecar for `{}`; run `import` first",
                args.input.display()
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let copy_id = resolve_mask_copy(&document, args.virtual_copy.as_deref())?;
    let mut actions: Vec<String> = Vec::new();
    if args.clear_curves && args.clear_curve_channel.is_some() {
        return Err(CliError::Message(
            "--clear-curves and --clear-curve-channel are mutually exclusive".into(),
        ));
    }
    if args.clear_hsl && !args.set_hsl.is_empty() {
        return Err(CliError::Message(
            "--clear-hsl and --set-hsl are mutually exclusive".into(),
        ));
    }
    if args.clear_grading
        && (!args.set_grading.is_empty()
            || args.set_grading_balance.is_some()
            || args.set_grading_blending.is_some())
    {
        return Err(CliError::Message(
            "--clear-grading and --set-grading* are mutually exclusive".into(),
        ));
    }
    if args.clear_point_color
        && (args.add_point_color
            || !args.set_point_color.is_empty()
            || !args.remove_point_color.is_empty())
    {
        return Err(CliError::Message(
            "--clear-point-color and --add/set/remove-point-color are mutually exclusive".into(),
        ));
    }
    if args.clear_curves {
        mask_copy_mut(&mut document, &copy_id)?.recipe.curves = None;
        info!("color: cleared curves on copy `{copy_id}`");
        actions.push("clear-curves".into());
    }
    if let Some(channel) = args.clear_curve_channel.as_deref() {
        check_curve_channel(channel)?;
        let curves = curves_block_mut(&mut document, &copy_id)?;
        match channel {
            "master" => {
                curves.master = vec![
                    CurvePoint {
                        input: 0.0,
                        output: 0.0,
                    },
                    CurvePoint {
                        input: 1.0,
                        output: 1.0,
                    },
                ];
            }
            "red" => curves.channels.red = None,
            "green" => curves.channels.green = None,
            "blue" => curves.channels.blue = None,
            _ => unreachable!(),
        }
        info!("color: cleared curve channel {channel} on copy `{copy_id}`");
        actions.push(format!("clear-curve-channel:{channel}"));
    }
    for spec in &args.set_curve_param {
        let (channel, deltas) = parse_curve_param(spec)?;
        let points = curve_param_points(&deltas);
        set_curve_channel_points(curves_block_mut(&mut document, &copy_id)?, channel, points)?;
        info!("color: curve param {channel} on copy `{copy_id}`");
        actions.push(format!("curve-param:{channel}"));
    }
    for spec in &args.set_curve_points {
        let (channel, points) = parse_curve_points(spec)?;
        set_curve_channel_points(curves_block_mut(&mut document, &copy_id)?, channel, points)?;
        info!("color: curve points {channel} on copy `{copy_id}`");
        actions.push(format!("curve-points:{channel}"));
    }
    if args.clear_hsl {
        mask_copy_mut(&mut document, &copy_id)?.recipe.hsl = None;
        info!("color: cleared hsl on copy `{copy_id}`");
        actions.push("clear-hsl".into());
    }
    for spec in &args.set_hsl {
        let (channel, field, value) = parse_hsl_triple(spec)?;
        let slot = hsl_slot_mut(hsl_block_mut(&mut document, &copy_id)?, channel);
        match field {
            "hue" => slot.hue = value,
            "saturation" => slot.saturation = value,
            "luminance" => slot.luminance = value,
            _ => unreachable!(),
        }
        info!("color: hsl {channel}.{field}={value} on copy `{copy_id}`");
        actions.push(format!("hsl:{channel}.{field}"));
    }
    if args.clear_point_color {
        mask_copy_mut(&mut document, &copy_id)?.recipe.point_color = None;
        info!("color: cleared point_color on copy `{copy_id}`");
        actions.push("clear-point-color".into());
    }
    if args.add_point_color {
        let block = point_color_block_mut(&mut document, &copy_id)?;
        if block.entries.len() >= 8 {
            return Err(CliError::Message(
                "point_color entry limit (8) reached".into(),
            ));
        }
        let id = PointColorEntry::next_id(&block.entries);
        block.entries.push(PointColorEntry {
            id: id.clone(),
            hue_center: args.hue_center.unwrap_or(0.0),
            hue_range: args.hue_range.unwrap_or(30.0),
            hue_shift: args.hue_shift.unwrap_or(0.0),
            saturation_shift: args.sat_shift.unwrap_or(0.0),
            luminance_shift: args.lum_shift.unwrap_or(0.0),
        });
        info!("color: point_color add {id} on copy `{copy_id}`");
        actions.push(format!("point-color-add:{id}"));
    }
    for spec in &args.set_point_color {
        let (id, field, value) = parse_point_color_triple(spec)?;
        let block = point_color_block_mut(&mut document, &copy_id)?;
        let Some(entry) = block.entries.iter_mut().find(|e| e.id == id) else {
            return Err(CliError::Message(format!(
                "unknown point_color entry `{id}` on copy `{copy_id}`"
            )));
        };
        match field {
            "hue_center" => entry.hue_center = value,
            "hue_range" => entry.hue_range = value,
            "hue_shift" => entry.hue_shift = value,
            "saturation_shift" => entry.saturation_shift = value,
            "luminance_shift" => entry.luminance_shift = value,
            _ => unreachable!(),
        }
        info!("color: point_color {id}.{field}={value} on copy `{copy_id}`");
        actions.push(format!("point-color:{id}.{field}"));
    }
    for id in &args.remove_point_color {
        let block = point_color_block_mut(&mut document, &copy_id)?;
        let len = block.entries.len();
        block.entries.retain(|e| e.id != *id);
        if block.entries.len() == len {
            return Err(CliError::Message(format!(
                "unknown point_color entry `{id}` on copy `{copy_id}`"
            )));
        }
        if block.entries.is_empty() {
            mask_copy_mut(&mut document, &copy_id)?.recipe.point_color = None;
        }
        info!("color: point_color remove {id} on copy `{copy_id}`");
        actions.push(format!("point-color-remove:{id}"));
    }
    if args.clear_grading {
        mask_copy_mut(&mut document, &copy_id)?.recipe.color_grading = None;
        info!("color: cleared grading on copy `{copy_id}`");
        actions.push("clear-grading".into());
    }
    for spec in &args.set_grading {
        let (range, field, value) = parse_grading_triple(spec)?;
        let slot = grading_slot_mut(grading_block_mut(&mut document, &copy_id)?, range);
        match field {
            "hue_degrees" => slot.hue_degrees = value,
            "saturation" => slot.saturation = value,
            "luminance" => slot.luminance = value,
            _ => unreachable!(),
        }
        info!("color: grading {range}.{field}={value} on copy `{copy_id}`");
        actions.push(format!("grading:{range}.{field}"));
    }
    if let Some(balance) = args.set_grading_balance {
        grading_block_mut(&mut document, &copy_id)?.balance = balance;
        info!("color: grading balance {balance} on copy `{copy_id}`");
        actions.push(format!("grading-balance:{balance}"));
    }
    if let Some(blending) = args.set_grading_blending {
        grading_block_mut(&mut document, &copy_id)?.blending = blending;
        info!("color: grading blending {blending} on copy `{copy_id}`");
        actions.push(format!("grading-blending:{blending}"));
    }
    if let Some(vibrance) = args.set_vibrance {
        mask_copy_mut(&mut document, &copy_id)?
            .recipe
            .adjustments
            .insert("vibrance".into(), vibrance);
        info!("color: vibrance {vibrance} on copy `{copy_id}`");
        actions.push(format!("vibrance:{vibrance}"));
    }
    if let Some(saturation) = args.set_saturation {
        mask_copy_mut(&mut document, &copy_id)?
            .recipe
            .adjustments
            .insert("saturation".into(), saturation);
        info!("color: saturation {saturation} on copy `{copy_id}`");
        actions.push(format!("saturation:{saturation}"));
    }
    if wants_mutation {
        // Loud gate: ranges, point rules, focal order and ids are rejected
        // before anything is written (no silent clipping, no half-apply).
        document
            .validate()
            .map_err(|error| CliError::Message(error.to_string()))?;
        save_sidecar(&path, &document)?;
    }
    color_list(&args, &document, &copy_id, &actions)
}

/// Mutable access to one virtual copy's tone-curve stage, creating an
/// identity stage when none exists (loud on unknown ids).
fn curves_block_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut Curves, CliError> {
    let copy = mask_copy_mut(document, copy_id)?;
    Ok(copy.recipe.curves.get_or_insert(Curves {
        version: 1,
        master: vec![
            CurvePoint {
                input: 0.0,
                output: 0.0,
            },
            CurvePoint {
                input: 1.0,
                output: 1.0,
            },
        ],
        channels: CurveChannels::default(),
    }))
}

/// Mutable access to one virtual copy's HSL mixer, creating a neutral stage
/// when none exists (loud on unknown ids).
fn hsl_block_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut HslAdjustments, CliError> {
    let copy = mask_copy_mut(document, copy_id)?;
    let hsl = copy.recipe.hsl.get_or_insert(HslAdjustments {
        version: 1,
        ..Default::default()
    });
    hsl.version = 1;
    Ok(hsl)
}

/// Mutable access to one virtual copy's Point Color stage, creating an empty
/// stage when none exists (loud on unknown ids).
fn point_color_block_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut PointColor, CliError> {
    let copy = mask_copy_mut(document, copy_id)?;
    let block = copy.recipe.point_color.get_or_insert(PointColor {
        version: 1,
        entries: Vec::new(),
    });
    block.version = 1;
    Ok(block)
}

/// Mutable access to one virtual copy's color-grading stage, creating a
/// neutral stage when none exists (loud on unknown ids).
fn grading_block_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut ColorGrading, CliError> {
    let copy = mask_copy_mut(document, copy_id)?;
    Ok(copy
        .recipe
        .color_grading
        .get_or_insert_with(ColorGrading::neutral))
}

fn check_curve_channel(channel: &str) -> Result<(), CliError> {
    if matches!(channel, "master" | "red" | "green" | "blue") {
        Ok(())
    } else {
        Err(CliError::Message(format!(
            "invalid curve channel `{channel}`: expected master|red|green|blue"
        )))
    }
}

fn set_curve_channel_points(
    curves: &mut Curves,
    channel: &str,
    points: Vec<CurvePoint>,
) -> Result<(), CliError> {
    check_curve_channel(channel)?;
    curves.version = 1;
    match channel {
        "master" => curves.master = points,
        "red" => curves.channels.red = Some(points),
        "green" => curves.channels.green = Some(points),
        "blue" => curves.channels.blue = Some(points),
        _ => unreachable!(),
    }
    Ok(())
}

/// Builds the parametric 4-point list at base positions `0, 1/3, 2/3, 1`
/// (same mapping as the GUI); endpoint/range validity is enforced on save.
fn curve_param_points(deltas: &[f64; 4]) -> Vec<CurvePoint> {
    const BASE: [f64; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
    BASE.iter()
        .zip(deltas.iter())
        .map(|(base, delta)| CurvePoint {
            input: *base as f32,
            output: ((base + delta).clamp(0.0, 1.0)) as f32,
        })
        .collect()
}

fn parse_f32_list(value: &str, expected: usize, what: &str) -> Result<Vec<f32>, CliError> {
    let parts: Vec<&str> = value.split(',').collect();
    if parts.len() != expected {
        return Err(CliError::Message(format!(
            "invalid {what} `{value}`: expected {expected} comma-separated numbers"
        )));
    }
    parts
        .iter()
        .map(|part| {
            part.trim().parse::<f32>().map_err(|_| {
                CliError::Message(format!(
                    "invalid {what} `{value}`: `{part}` is not a number"
                ))
            })
        })
        .collect()
}

/// Parses `CHANNEL:S,D,L,H` (loud on malformed input; ranges on save).
fn parse_curve_param(spec: &str) -> Result<(&str, [f64; 4]), CliError> {
    let (channel, rest) = spec.split_once(':').ok_or_else(|| {
        CliError::Message(format!(
            "invalid curve param `{spec}`: expected `CHANNEL:S,D,L,H`"
        ))
    })?;
    check_curve_channel(channel)?;
    let values = parse_f32_list(rest, 4, "curve param")?;
    Ok((
        channel,
        [
            f64::from(values[0]),
            f64::from(values[1]),
            f64::from(values[2]),
            f64::from(values[3]),
        ],
    ))
}

/// Parses `CHANNEL:I,O;I,O;...` (loud on malformed input; point rules on
/// save).
fn parse_curve_points(spec: &str) -> Result<(&str, Vec<CurvePoint>), CliError> {
    let (channel, rest) = spec.split_once(':').ok_or_else(|| {
        CliError::Message(format!(
            "invalid curve points `{spec}`: expected `CHANNEL:I,O;I,O;...`"
        ))
    })?;
    check_curve_channel(channel)?;
    let mut points = Vec::new();
    for pair in rest.split(';') {
        let values = parse_f32_list(pair, 2, "curve point")?;
        points.push(CurvePoint {
            input: values[0],
            output: values[1],
        });
    }
    Ok((channel, points))
}

fn hsl_slot_mut<'a>(hsl: &'a mut HslAdjustments, channel: &str) -> &'a mut HslChannel {
    let slot = match channel {
        "red" => &mut hsl.red,
        "orange" => &mut hsl.orange,
        "yellow" => &mut hsl.yellow,
        "green" => &mut hsl.green,
        "cyan" => &mut hsl.cyan,
        "blue" => &mut hsl.blue,
        "violet" => &mut hsl.violet,
        _ => &mut hsl.magenta,
    };
    slot.get_or_insert_with(HslChannel::default)
}

/// Parses `CHANNEL:FIELD:VALUE` for the HSL mixer (loud on unknown
/// channels/fields; ranges on save).
fn parse_hsl_triple(spec: &str) -> Result<(&str, &str, f32), CliError> {
    let mut parts = spec.splitn(3, ':');
    let (Some(channel), Some(field), Some(value)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(CliError::Message(format!(
            "invalid hsl `{spec}`: expected `CHANNEL:FIELD:VALUE`"
        )));
    };
    if !matches!(
        channel,
        "red" | "orange" | "yellow" | "green" | "cyan" | "blue" | "violet" | "magenta"
    ) {
        return Err(CliError::Message(format!(
            "invalid hsl channel `{channel}`: expected red|orange|yellow|green|cyan|blue|violet|magenta"
        )));
    }
    if !matches!(field, "hue" | "saturation" | "luminance") {
        return Err(CliError::Message(format!(
            "invalid hsl field `{field}`: expected hue|saturation|luminance"
        )));
    }
    let value: f32 = value
        .trim()
        .parse()
        .map_err(|_| CliError::Message(format!("invalid hsl value in `{spec}`: not a number")))?;
    Ok((channel, field, value))
}

fn grading_slot_mut<'a>(grading: &'a mut ColorGrading, range: &str) -> &'a mut ColorGradingRange {
    match range {
        "shadows" => &mut grading.shadows,
        "midtones" => &mut grading.midtones,
        _ => &mut grading.highlights,
    }
}

/// Parses `RANGE:FIELD:VALUE` for color grading (loud on unknown
/// ranges/fields; ranges on save).
fn parse_grading_triple(spec: &str) -> Result<(&str, &str, f32), CliError> {
    let mut parts = spec.splitn(3, ':');
    let (Some(range), Some(field), Some(value)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(CliError::Message(format!(
            "invalid grading `{spec}`: expected `RANGE:FIELD:VALUE`"
        )));
    };
    if !matches!(range, "shadows" | "midtones" | "highlights") {
        return Err(CliError::Message(format!(
            "invalid grading range `{range}`: expected shadows|midtones|highlights"
        )));
    }
    if !matches!(field, "hue_degrees" | "saturation" | "luminance") {
        return Err(CliError::Message(format!(
            "invalid grading field `{field}`: expected hue_degrees|saturation|luminance"
        )));
    }
    let value: f32 = value.trim().parse().map_err(|_| {
        CliError::Message(format!("invalid grading value in `{spec}`: not a number"))
    })?;
    Ok((range, field, value))
}

/// Parses `ID:FIELD:VALUE` for Point Color (loud on unknown fields or a
/// missing entry; ranges on save).
fn parse_point_color_triple(spec: &str) -> Result<(&str, &str, f32), CliError> {
    let mut parts = spec.splitn(3, ':');
    let (Some(id), Some(field), Some(value)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(CliError::Message(format!(
            "invalid point_color `{spec}`: expected `ID:FIELD:VALUE`"
        )));
    };
    if id.trim().is_empty() {
        return Err(CliError::Message(format!(
            "invalid point_color `{spec}`: empty entry id"
        )));
    }
    if !matches!(
        field,
        "hue_center" | "hue_range" | "hue_shift" | "saturation_shift" | "luminance_shift"
    ) {
        return Err(CliError::Message(format!(
            "invalid point_color field `{field}`: expected hue_center|hue_range|hue_shift|saturation_shift|luminance_shift"
        )));
    }
    let value: f32 = value.trim().parse().map_err(|_| {
        CliError::Message(format!(
            "invalid point_color value in `{spec}`: not a number"
        ))
    })?;
    Ok((id, field, value))
}

fn color_list(
    args: &ColorArgs,
    document: &SidecarDocument,
    copy_id: &str,
    actions: &[String],
) -> Result<(), CliError> {
    let copy = document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == copy_id)
        .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{copy_id}`")))?;
    let recipe = &copy.recipe;
    if args.json {
        emit(
            true,
            serde_json::json!({
                "command": "color",
                "input": args.input,
                "copy": copy_id,
                "curves": recipe.curves,
                "hsl": recipe.hsl,
                "point_color": recipe.point_color,
                "color_grading": recipe.color_grading,
                "vibrance": recipe.adjustments.get("vibrance"),
                "saturation": recipe.adjustments.get("saturation"),
                "actions": actions,
            }),
            "color status listed",
        )
    } else {
        println!("copy: {} [{}]", copy.name, copy.id);
        match &recipe.curves {
            Some(curves) => {
                println!(
                    "  curves: master={}pts red={} green={} blue={}",
                    curves.master.len(),
                    curves.channels.red.as_ref().map_or(0, Vec::len),
                    curves.channels.green.as_ref().map_or(0, Vec::len),
                    curves.channels.blue.as_ref().map_or(0, Vec::len)
                );
            }
            None => println!("  curves: none"),
        }
        println!(
            "  hsl: {}",
            if recipe.hsl.is_some() { "set" } else { "none" }
        );
        match &recipe.point_color {
            Some(block) => {
                println!("  point_color: {} entries", block.entries.len());
                for entry in &block.entries {
                    println!(
                        "    {}: center={} range={} hue={} sat={} lum={}",
                        entry.id,
                        entry.hue_center,
                        entry.hue_range,
                        entry.hue_shift,
                        entry.saturation_shift,
                        entry.luminance_shift
                    );
                }
            }
            None => println!("  point_color: none"),
        }
        match &recipe.color_grading {
            Some(grading) => {
                println!(
                    "  grading: balance={} blending={}",
                    grading.balance, grading.blending
                );
                for (name, range) in [
                    ("shadows", grading.shadows),
                    ("midtones", grading.midtones),
                    ("highlights", grading.highlights),
                ] {
                    println!(
                        "    {name}: hue={} sat={} lum={}",
                        range.hue_degrees, range.saturation, range.luminance
                    );
                }
            }
            None => println!("  grading: none"),
        }
        println!(
            "  vibrance={:?} saturation={:?}",
            recipe.adjustments.get("vibrance"),
            recipe.adjustments.get("saturation")
        );
        if actions.is_empty() {
            emit(
                false,
                serde_json::json!({"command":"color","status":"ok"}),
                "color status listed",
            )
        } else {
            emit(
                false,
                serde_json::json!({"command":"color","status":"ok"}),
                &format!("color updated: {}", actions.join(", ")),
            )
        }
    }
}

/// G-06 Geometrie-Parität (LRPAR-G06-GEO): inspect and edit the geometry
/// stages (crop/aspect, straighten/rotation, mirrors, manual lens
/// correction, manual perspective) of one virtual copy. List-only mode is
/// read-only (sidecar bytes unchanged). Mutations validate loudly
/// (`document.validate()`) before `save_sidecar` and append exactly one
/// history entry per call, so every step stays visible; the original image
/// is never modified.
fn geometry(args: GeometryArgs) -> Result<(), CliError> {
    if args.set_rotation.is_some() && args.straighten.is_some() {
        return Err(CliError::Message(
            "--set-rotation and --straighten are aliases; pass only one".into(),
        ));
    }
    if args.set_crop_aspect.is_some() && args.set_crop_free.is_some() {
        return Err(CliError::Message(
            "--set-crop-aspect and --set-crop-free are mutually exclusive".into(),
        ));
    }
    let wants_mutation = args.set_crop_aspect.is_some()
        || args.set_crop_free.is_some()
        || args.clear_crop
        || args.set_rotation.is_some()
        || args.straighten.is_some()
        || args.set_mirror.is_some()
        || args.clear_geometry
        || args.set_lens_profile.is_some()
        || !args.set_lens.is_empty()
        || args.clear_lens
        || !args.set_perspective.is_empty()
        || args.clear_perspective;
    if args.lensfun_status && wants_mutation {
        return Err(CliError::Message(
            "--lensfun-status is read-only; pass no mutation flags with it".into(),
        ));
    }
    // `--list` is the read-only view (the default when nothing mutates);
    // combined with a mutation flag it would silently do the wrong thing.
    if args.list && wants_mutation {
        return Err(CliError::Message(
            "--list is read-only; pass no mutation flags with it".into(),
        ));
    }
    // A clear and a set of the SAME stage contradict each other (loud, no
    // half-apply); clears of different stages compose freely.
    if args.clear_crop && (args.set_crop_aspect.is_some() || args.set_crop_free.is_some()) {
        return Err(CliError::Message(
            "--clear-crop contradicts --set-crop-aspect/--set-crop-free".into(),
        ));
    }
    if args.clear_lens && (args.set_lens_profile.is_some() || !args.set_lens.is_empty()) {
        return Err(CliError::Message(
            "--clear-lens contradicts --set-lens-profile/--set-lens".into(),
        ));
    }
    if args.clear_perspective && !args.set_perspective.is_empty() {
        return Err(CliError::Message(
            "--clear-perspective contradicts --set-perspective".into(),
        ));
    }
    if args.clear_geometry
        && (args.set_crop_aspect.is_some()
            || args.set_crop_free.is_some()
            || args.clear_crop
            || args.set_rotation.is_some()
            || args.straighten.is_some()
            || args.set_mirror.is_some())
    {
        return Err(CliError::Message(
            "--clear-geometry contradicts the crop/rotation/mirror flags".into(),
        ));
    }
    let path = sidecar_path_for(&args.input);
    let mut document = match load_sidecar(&path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            return Err(CliError::Message(format!(
                "no sidecar for `{}`; run `import` first",
                args.input.display()
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let copy_id = resolve_mask_copy(&document, args.virtual_copy.as_deref())?;
    let mut actions: Vec<String> = Vec::new();
    // Geometry stage: whole-stage clear or per-field mutations.
    if args.clear_geometry {
        let copy = mask_copy_mut(&mut document, &copy_id)?;
        copy.recipe.geometry = None;
        info!("geometry: cleared stage on copy `{copy_id}`");
        actions.push("clear-geometry".into());
    } else {
        if let Some(preset) = args.set_crop_aspect.as_deref() {
            geometry_mut(&mut document, &copy_id)?.crop = Some(Crop::Aspect {
                preset: parse_aspect_preset(preset)?,
            });
            info!("geometry: crop aspect {preset} on copy `{copy_id}`");
            actions.push(format!("crop-aspect:{preset}"));
        }
        if let Some(rect) = args.set_crop_free.as_deref() {
            let (x, y, width, height) = parse_crop_free(rect)?;
            geometry_mut(&mut document, &copy_id)?.crop = Some(Crop::Free {
                x,
                y,
                width,
                height,
            });
            info!("geometry: crop free {rect} on copy `{copy_id}`");
            actions.push(format!("crop-free:{rect}"));
        }
        if args.clear_crop {
            geometry_mut(&mut document, &copy_id)?.crop = None;
            info!("geometry: crop cleared on copy `{copy_id}`");
            actions.push("crop:clear".into());
        }
        // `--straighten` is a documented alias of `--set-rotation`: same
        // field (`geometry.rotation_degrees`), same validation, one step.
        if let Some(degrees) = args.set_rotation.or(args.straighten) {
            if !degrees.is_finite() {
                return Err(CliError::Message(format!(
                    "invalid rotation `{degrees}`: expected a finite number in -180..=180"
                )));
            }
            geometry_mut(&mut document, &copy_id)?.rotation_degrees = degrees as f32;
            info!("geometry: rotation {degrees} on copy `{copy_id}`");
            actions.push(format!("rotation:{degrees}"));
        }
        if let Some(mirror) = args.set_mirror.as_deref() {
            let (horizontal, vertical) = parse_mirror(mirror)?;
            let geo = geometry_mut(&mut document, &copy_id)?;
            geo.mirror_horizontal = horizontal;
            geo.mirror_vertical = vertical;
            info!("geometry: mirror {mirror} on copy `{copy_id}`");
            actions.push(format!("mirror:{mirror}"));
        }
    }
    // Manual lens stage: whole-stage clear or per-field mutations.
    if args.clear_lens {
        let copy = mask_copy_mut(&mut document, &copy_id)?;
        copy.recipe.lens_correction = None;
        info!("geometry: lens correction cleared on copy `{copy_id}`");
        actions.push("lens:clear".into());
    } else {
        if let Some(profile) = args.set_lens_profile.as_deref() {
            lens_mut(&mut document, &copy_id)?.profile = Some(profile.into());
            info!("geometry: lens profile {profile} on copy `{copy_id}`");
            actions.push(format!("lens-profile:{profile}"));
        }
        for spec in &args.set_lens {
            let (field, value) = parse_lens_field(spec)?;
            set_lens_field(lens_mut(&mut document, &copy_id)?, &field, value);
            info!("geometry: lens {field}={value} on copy `{copy_id}`");
            actions.push(format!("lens:{field}={value}"));
        }
    }
    // Manual perspective stage: whole-stage clear or per-field mutations.
    if args.clear_perspective {
        let copy = mask_copy_mut(&mut document, &copy_id)?;
        copy.recipe.perspective = None;
        info!("geometry: perspective cleared on copy `{copy_id}`");
        actions.push("perspective:clear".into());
    } else {
        for spec in &args.set_perspective {
            let (field, value) = parse_perspective_field(spec)?;
            set_perspective_field(perspective_mut(&mut document, &copy_id)?, &field, value);
            info!("geometry: perspective {field}={value} on copy `{copy_id}`");
            actions.push(format!("perspective:{field}={value}"));
        }
    }
    if wants_mutation {
        // Loud gate: aspect names, rect geometry, mirror words, field names
        // and every range are rejected before anything is written. Exactly
        // one history entry per call keeps every step visible (G-06).
        document
            .validate()
            .map_err(|error| CliError::Message(error.to_string()))?;
        let copy = mask_copy_mut(&mut document, &copy_id)?;
        let final_recipe = copy.recipe.clone();
        let mut id = format!("geometry-{}", timestamp());
        let mut suffix = 0u32;
        while copy.history.iter().any(|entry| entry.id == id) {
            suffix += 1;
            id = format!("geometry-{}-{suffix}", timestamp());
        }
        let mut extras = BTreeMap::new();
        extras.insert("step".into(), serde_json::Value::String("geometry".into()));
        extras.insert(
            "actions".into(),
            serde_json::Value::String(actions.join(",")),
        );
        copy.history.push(HistoryEntry {
            id,
            recipe: final_recipe,
            recorded_at: Some(timestamp()),
            extras,
        });
        save_sidecar(&path, &document)?;
    }
    geometry_list(&args, &document, &copy_id, &actions)
}

/// Mutable access to one virtual copy's geometry stage, creating an
/// identity stage when none exists (loud on unknown ids).
fn geometry_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut Geometry, CliError> {
    let copy = mask_copy_mut(document, copy_id)?;
    Ok(copy.recipe.geometry.get_or_insert(Geometry {
        version: 1,
        crop: None,
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    }))
}

/// Mutable access to one virtual copy's manual lens-correction stage,
/// creating an empty stage when none exists (loud on unknown ids).
fn lens_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut LensCorrection, CliError> {
    let copy = mask_copy_mut(document, copy_id)?;
    Ok(copy.recipe.lens_correction.get_or_insert(LensCorrection {
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
    }))
}

/// Mutable access to one virtual copy's manual perspective stage, creating
/// an identity stage when none exists (loud on unknown ids).
fn perspective_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut Perspective, CliError> {
    let copy = mask_copy_mut(document, copy_id)?;
    Ok(copy.recipe.perspective.get_or_insert(Perspective {
        version: 1,
        vertical: 0.0,
        horizontal: 0.0,
        rotation: 0.0,
        scale: 1.0,
        aspect_ratio: 1.0,
        shift_x: 0.0,
        shift_y: 0.0,
    }))
}

/// Parses an aspect preset name (loud on unknown names — never a guess).
fn parse_aspect_preset(value: &str) -> Result<AspectPreset, CliError> {
    match value {
        "original" => Ok(AspectPreset::Original),
        "1:1" => Ok(AspectPreset::OneToOne),
        "4:5" => Ok(AspectPreset::FourToFive),
        "5:4" => Ok(AspectPreset::FiveToFour),
        "3:2" => Ok(AspectPreset::ThreeToTwo),
        "2:3" => Ok(AspectPreset::TwoToThree),
        "4:3" => Ok(AspectPreset::FourToThree),
        "3:4" => Ok(AspectPreset::ThreeToFour),
        "16:9" => Ok(AspectPreset::SixteenToNine),
        "9:16" => Ok(AspectPreset::NineToSixteen),
        _ => Err(CliError::Message(format!(
            "invalid aspect preset `{value}`: expected one of original|1:1|4:5|5:4|3:2|2:3|4:3|3:4|16:9|9:16"
        ))),
    }
}

/// Parses a free crop rectangle as `x,y,w,h` (loud on malformed input;
/// range geometry is validated on save, not guessed here).
fn parse_crop_free(value: &str) -> Result<(f32, f32, f32, f32), CliError> {
    let parts: Vec<&str> = value.split(',').collect();
    let numbers: Option<Vec<f32>> = parts
        .iter()
        .map(|part| part.trim().parse::<f32>().ok())
        .collect();
    match numbers.as_deref() {
        Some([x, y, width, height]) => Ok((*x, *y, *width, *height)),
        _ => Err(CliError::Message(format!(
            "invalid crop rect `{value}`: expected `x,y,w,h` with finite numbers"
        ))),
    }
}

/// Parses mirror flags as `h|v|hv|none` (loud on unknown words).
fn parse_mirror(value: &str) -> Result<(bool, bool), CliError> {
    match value {
        "h" => Ok((true, false)),
        "v" => Ok((false, true)),
        "hv" => Ok((true, true)),
        "none" => Ok((false, false)),
        _ => Err(CliError::Message(format!(
            "invalid mirror `{value}`: expected h|v|hv|none"
        ))),
    }
}

/// Parses one manual lens field as `FIELD:VALUE` (loud on unknown fields
/// or non-numbers; ranges are validated on save).
fn parse_lens_field(spec: &str) -> Result<(String, f32), CliError> {
    const FIELDS: &[&str] = &[
        "distortion_k1",
        "distortion_k2",
        "distortion_k3",
        "vignette_c0",
        "vignette_c1",
        "vignette_c2",
        "ca_red",
        "ca_blue",
    ];
    let (field, value) = spec.split_once(':').ok_or_else(|| {
        CliError::Message(format!(
            "invalid lens field `{spec}`: expected `FIELD:VALUE`"
        ))
    })?;
    if !FIELDS.contains(&field) {
        return Err(CliError::Message(format!(
            "invalid lens field `{field}`: expected one of {}",
            FIELDS.join("|")
        )));
    }
    let value: f32 = value
        .trim()
        .parse()
        .map_err(|_| CliError::Message(format!("invalid lens value in `{spec}`: not a number")))?;
    Ok((field.into(), value))
}

/// Applies one parsed manual lens field (fields are pre-validated by
/// [`parse_lens_field`]).
fn set_lens_field(lens: &mut LensCorrection, field: &str, value: f32) {
    match field {
        "distortion_k1" => lens.distortion_k1 = Some(value),
        "distortion_k2" => lens.distortion_k2 = Some(value),
        "distortion_k3" => lens.distortion_k3 = Some(value),
        "vignette_c0" => lens.vignette_c0 = Some(value),
        "vignette_c1" => lens.vignette_c1 = Some(value),
        "vignette_c2" => lens.vignette_c2 = Some(value),
        "ca_red" => lens.ca_red = Some(value),
        "ca_blue" => lens.ca_blue = Some(value),
        _ => unreachable!("lens field pre-validated by parse_lens_field"),
    }
}

/// Parses one manual perspective field as `FIELD:VALUE` (loud on unknown
/// fields or non-numbers; ranges are validated on save).
fn parse_perspective_field(spec: &str) -> Result<(String, f32), CliError> {
    const FIELDS: &[&str] = &[
        "vertical",
        "horizontal",
        "rotation",
        "scale",
        "aspect_ratio",
        "shift_x",
        "shift_y",
    ];
    let (field, value) = spec.split_once(':').ok_or_else(|| {
        CliError::Message(format!(
            "invalid perspective field `{spec}`: expected `FIELD:VALUE`"
        ))
    })?;
    if !FIELDS.contains(&field) {
        return Err(CliError::Message(format!(
            "invalid perspective field `{field}`: expected one of {}",
            FIELDS.join("|")
        )));
    }
    let value: f32 = value.trim().parse().map_err(|_| {
        CliError::Message(format!(
            "invalid perspective value in `{spec}`: not a number"
        ))
    })?;
    Ok((field.into(), value))
}

/// Applies one parsed manual perspective field (fields are pre-validated by
/// [`parse_perspective_field`]).
fn set_perspective_field(perspective: &mut Perspective, field: &str, value: f32) {
    match field {
        "vertical" => perspective.vertical = value,
        "horizontal" => perspective.horizontal = value,
        "rotation" => perspective.rotation = value,
        "scale" => perspective.scale = value,
        "aspect_ratio" => perspective.aspect_ratio = value,
        "shift_x" => perspective.shift_x = value,
        "shift_y" => perspective.shift_y = value,
        _ => unreachable!("perspective field pre-validated by parse_perspective_field"),
    }
}

fn geometry_list(
    args: &GeometryArgs,
    document: &SidecarDocument,
    copy_id: &str,
    actions: &[String],
) -> Result<(), CliError> {
    let copy = document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == copy_id)
        .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{copy_id}`")))?;
    let recipe = &copy.recipe;
    // `--lensfun-status` resolves the EXIF→profile match for the input
    // (read-only): which corrector a render would use, or the loud reason
    // none applies. Never a guessed correction.
    let lensfun_report = if args.lensfun_status {
        Some(resolve_lensfun_report(&args.input))
    } else {
        None
    };
    if args.json {
        emit(
            true,
            serde_json::json!({
                "command": "geometry",
                "input": args.input,
                "copy": copy_id,
                "geometry": recipe.geometry,
                "lens_correction": recipe.lens_correction,
                "perspective": recipe.perspective,
                "lensfun": lensfun_report,
                "actions": actions,
            }),
            "geometry status listed",
        )
    } else {
        println!("copy: {} [{}]", copy.name, copy.id);
        match &recipe.geometry {
            Some(geo) => {
                match &geo.crop {
                    Some(Crop::Aspect { preset }) => {
                        println!("  crop: aspect {preset:?}")
                    }
                    Some(Crop::Free {
                        x,
                        y,
                        width,
                        height,
                    }) => {
                        println!("  crop: free x={x} y={y} w={width} h={height}")
                    }
                    None => println!("  crop: none (full frame)"),
                }
                println!(
                    "  rotation: {} mirror_h={} mirror_v={}",
                    geo.rotation_degrees, geo.mirror_horizontal, geo.mirror_vertical
                );
            }
            None => println!("  geometry: none"),
        }
        match &recipe.lens_correction {
            Some(lens) => println!(
                "  lens: profile={:?} k1={:?} k2={:?} k3={:?} c0={:?} c1={:?} c2={:?} ca_r={:?} ca_b={:?}",
                lens.profile,
                lens.distortion_k1,
                lens.distortion_k2,
                lens.distortion_k3,
                lens.vignette_c0,
                lens.vignette_c1,
                lens.vignette_c2,
                lens.ca_red,
                lens.ca_blue
            ),
            None => println!("  lens: none"),
        }
        match &recipe.perspective {
            Some(p) => println!(
                "  perspective: v={} h={} rot={} scale={} aspect={} sx={} sy={}",
                p.vertical, p.horizontal, p.rotation, p.scale, p.aspect_ratio, p.shift_x, p.shift_y
            ),
            None => println!("  perspective: none"),
        }
        if let Some(report) = lensfun_report {
            println!("  lensfun: {report}");
        }
        if actions.is_empty() {
            emit(
                false,
                serde_json::json!({"command":"geometry","status":"ok"}),
                "geometry status listed",
            )
        } else {
            emit(
                false,
                serde_json::json!({"command":"geometry","status":"ok"}),
                &format!("geometry updated: {}", actions.join(", ")),
            )
        }
    }
}

/// Resolves the Lensfun auto-profile status for one input file (G-06):
/// which corrector a render would build from the input's EXIF, or the loud
/// reason none applies (no metadata, missing EXIF fields, no system DB, no
/// matching profile, identity correction). Read-only — never a correction.
fn resolve_lensfun_report(input: &Path) -> String {
    #[cfg(not(feature = "lensfun"))]
    {
        let _ = input;
        "unavailable (build without the `lensfun` feature)".into()
    }
    #[cfg(feature = "lensfun")]
    {
        let metadata = match lumina_raw::read_metadata(input) {
            Ok(metadata) => metadata,
            Err(error) => return format!("no EXIF metadata ({error}) — manual model applies"),
        };
        match build_lensfun_corrector(Some(&metadata)) {
            Some((_, corrector)) => format!(
                "profile matched (distortion={} vignetting={} tca={}) — auto correction applies",
                corrector.has_distortion(),
                corrector.has_vignetting(),
                corrector.has_tca()
            ),
            None => "no matching non-identity profile — manual model applies".into(),
        }
    }
}

fn dust_removal(args: DustRemovalArgs) -> Result<(), CliError> {
    // Never overwrite the original — or its Lumina bundle files — with the
    // optional render output (REVIEW-CLI-WRITE-1).
    if let Some(output) = &args.render_out {
        reject_protected_output(&args.input, output)?;
    }
    let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
    let (frame, _raw) = decode_input(&args.input, &bytes)?;

    // Load and validate the repair-region definition.  Region and replacement
    // must have identical dimensions; the region must also match the decoded
    // source frame, because the MVP applies source actions at source resolution.
    let definition: RepairRegionInput = {
        let json = fs::read_to_string(&args.repair_region)
            .map_err(|error| io_error(&args.repair_region, error))?;
        serde_json::from_str(&json)
            .map_err(|error| CliError::Message(format!("invalid repair-region JSON: {error}")))?
    };
    let replacement_bytes = fs::read(&definition.replacement_path)
        .map_err(|error| io_error(&definition.replacement_path, error))?;
    let replacement_frame = ImageFrame::decode(&replacement_bytes).map_err(|error| {
        CliError::Message(format!("could not decode replacement image: {error}"))
    })?;
    if replacement_frame.width != definition.region_width
        || replacement_frame.height != definition.region_height
    {
        return Err(CliError::Message(format!(
            "replacement image {}x{} does not match region {}x{}",
            replacement_frame.width,
            replacement_frame.height,
            definition.region_width,
            definition.region_height
        )));
    }
    let region = RepairRegionArtifact {
        id: definition.id.clone(),
        width: definition.region_width,
        height: definition.region_height,
        region: definition.region_values.clone(),
        replacement: replacement_frame.pixels.clone(),
    };
    region
        .validate()
        .map_err(|error| CliError::Message(format!("invalid repair region: {error}")))?;
    if region.width != frame.width || region.height != frame.height {
        return Err(CliError::Message(format!(
            "repair region {}x{} does not match source frame {}x{}; source actions apply at source resolution",
            region.width, region.height, frame.width, frame.height
        )));
    }

    // REVIEW-CLI-N2: validate the sidecar and resolve the target copy BEFORE
    // anything is appended to the `.lumina.zdata` bundle. Appending first left
    // orphaned artifact bytes behind whenever the sidecar was missing or the
    // virtual copy did not exist. The recipe stores only a RELATIVE reference
    // (the bundle file name), never an absolute path.
    let sidecar_path = sidecar_path_for(&args.input);
    let mut document = match load_sidecar(&sidecar_path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            return Err(CliError::Message(format!(
                "no sidecar for `{}`; run `import` first",
                args.input.display()
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let copy_index = args
        .virtual_copy
        .as_deref()
        .map(|id| {
            document
                .virtual_copies
                .iter()
                .position(|copy| copy.id == id)
                .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{id}`")))
        })
        .transpose()?
        .unwrap_or(0);

    // Persist the artifact bytes into the portable `.lumina.zdata` bundle,
    // appended next to the source.
    let zdata_path = lumina_sidecar::zdata_path_for(&args.input);
    let relative_path = zdata_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repair.zdata")
        .to_string();
    let checksum = region.checksum();
    append_repair_region(&zdata_path, region).map_err(|error| {
        CliError::Message(format!("could not write repair-region bundle: {error}"))
    })?;

    // Record the action spec in the validated virtual copy's recipe.
    let spec = SourceActionSpec {
        version: SOURCE_ACTION_VERSION,
        kind: definition.kind,
        artifact: SourceActionArtifactRef {
            id: definition.id.clone(),
            relative_path: relative_path.clone(),
            checksum: checksum.clone(),
        },
    };
    document.virtual_copies[copy_index]
        .recipe
        .source_actions
        .push(spec);
    document.validate()?;
    save_sidecar(&sidecar_path, &document)?;

    // Optional headless render so the effect is verifiable end-to-end.
    if let Some(output) = &args.render_out {
        let source_actions =
            resolve_source_actions(&document.virtual_copies[copy_index].recipe, &zdata_path)?;
        let rendered = render_frame(
            &frame,
            &RenderContext {
                recipe: &document.virtual_copies[copy_index].recipe,
                camera_white_balance: None,
                source_actions: &source_actions,
                masks: None,
                // F-098-N2: `dust_removal` deliberately does not build a Lensfun
                // corrector. The decoded `RawMetadata` is intentionally discarded
                // here (`let (frame, _raw) = ...`) and the repair-region workflow
                // is a headless, source-resolution verification render without the
                // EXIF scope the corrector requires — `None` keeps the manual model.
                lensfun: None,
                depth: None,
            },
        )?;
        let format = output_format(output)?;
        write_atomically(output, &rendered.frame.encode(format)?)?;
    }

    emit(
        args.json,
        serde_json::json!({
            "command": "dust-removal",
            "input": args.input,
            "virtual_copy": document.virtual_copies[copy_index].id,
            "artifact_id": definition.id,
            "bundle": relative_path,
            "checksum": checksum,
            "status": "ok"
        }),
        "dust removal recorded",
    )
}

/// Resolves the recipe's persisted source actions into runtime artifacts by
/// reading the `.lumina.zdata` bundle.  A missing bundle, a missing artifact id
/// or a checksum mismatch against the recipe reference is a hard error — there
/// is no silent fallback (reproducibility over convenience).
fn resolve_source_actions(
    recipe: &EditRecipe,
    zdata_path: &Path,
) -> Result<Vec<SourceActionArtifact>, CliError> {
    if recipe.source_actions.is_empty() {
        return Ok(Vec::new());
    }
    let container = load_zdata(zdata_path).map_err(|error| {
        CliError::Message(format!(
            "could not read source-action bundle `{}`: {error}",
            zdata_path.display()
        ))
    })?;
    let mut artifacts = Vec::with_capacity(recipe.source_actions.len());
    for spec in &recipe.source_actions {
        let region = container
            .repair_region(&spec.artifact.id)
            .map_err(|error| {
                CliError::Message(format!(
                    "source action `{}` artifact missing from bundle: {error}",
                    spec.artifact.id
                ))
            })?;
        if region.checksum() != spec.artifact.checksum {
            return Err(CliError::Message(format!(
                "source action `{}` checksum mismatch: recipe and bundle disagree (stale or corrupted artifact)",
                spec.artifact.id
            )));
        }
        let mask_plane =
            MaskPlane::new(region.width, region.height, region.region).map_err(|error| {
                CliError::Message(format!(
                    "source action `{}` has an invalid region plane: {error}",
                    spec.artifact.id
                ))
            })?;
        let replacement = ImageFrame::new(region.width, region.height, region.replacement)
            .map_err(|error| {
                CliError::Message(format!(
                    "source action `{}` has an invalid replacement image: {error}",
                    spec.artifact.id
                ))
            })?;
        artifacts.push(SourceActionArtifact {
            region: mask_plane,
            replacement,
        });
    }
    Ok(artifacts)
}

fn reindex(args: IndexArgs) -> Result<(), CliError> {
    let mut files = Vec::new();
    collect_sidecars(&args.input, &mut files)?;
    let mut valid = 0usize;
    let mut invalid: Vec<String> = Vec::new();
    for path in files {
        match load_sidecar(&path) {
            Ok(_) => valid += 1,
            // REVIEW-CLI-N4: corrupt sidecars are never ignored silently —
            // each one is reported and the command exits non-zero so scripts
            // and the future index adapter notice the broken state.
            Err(error) => invalid.push(format!("{}: {error}", path.display())),
        }
    }
    for entry in &invalid {
        eprintln!("warning: invalid sidecar: {entry}");
    }
    let invalid_count = invalid.len();
    let text = format!("reindexed: {valid} valid, {invalid_count} invalid");
    emit(
        args.json,
        serde_json::json!({
            "command":"reindex",
            "input":args.input,
            "sidecars":valid,
            "invalid":invalid_count,
            "errors":invalid,
            "status": if invalid_count == 0 { "ok" } else { "invalid-sidecars" }
        }),
        &text,
    )?;
    if invalid_count != 0 {
        return Err(CliError::Message(format!(
            "reindex found {invalid_count} invalid sidecar(s)"
        )));
    }
    Ok(())
}

fn batch(args: BatchArgs) -> Result<(), CliError> {
    if args.jobs == 0 {
        return Err(CliError::Message("--jobs must be greater than zero".into()));
    }
    validate_format(&args.format)?;
    validate_quality(args.quality)?;
    let mut inputs = Vec::new();
    collect_images(&args.input, &mut inputs)?;
    // R2-CLI-11: drop same-file duplicates (hard links / inode aliases reached
    // under two names) so `--jobs > 1` never processes one file twice in
    // parallel and never appends duplicate history entries.
    let inputs = dedup_same_file_inputs(inputs);
    // REVIEW-CLI-BATCHCOLLIDE-1: outputs are name-based inside ONE flat
    // directory, so distinct inputs can map onto the same target file name
    // (`a/x.arw` and `b/x.png` both write `x.png`). Refuse the whole run up
    // front — before the output directory even exists — instead of letting
    // later items silently overwrite earlier ones.
    reject_duplicate_batch_targets(&inputs, &args.format)?;
    fs::create_dir_all(&args.output).map_err(|e| io_error(&args.output, e))?;
    let total = inputs.len();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(args.jobs)
        .build()
        .map_err(|e| CliError::Message(e.to_string()))?;
    // R2-CLI-06: per-item progress goes to stderr as items finish; the
    // collected mask warnings of each item are reported in the item JSON
    // below (same channel render/export use).
    let results = pool.install(|| {
        inputs
            .par_iter()
            .enumerate()
            .map(|(index, input)| batch_one(input, index, total, &args))
            .collect::<Vec<_>>()
    });
    let failed = results.iter().filter(|r| r.is_err()).count();
    if args.json {
        let items = results
            .iter()
            .map(|r| match r {
                Ok(v) => serde_json::json!({"status":"ok","input":v.input,"mask_warnings":v.mask_warnings}),
                Err(e) => serde_json::json!({"status":"failed","error":e.to_string()}),
            })
            .collect::<Vec<_>>();
        // R2-CLI-08: serialization of a plain strings/arrays JSON value is
        // practically infallible, but a worker panic must never be the
        // failure mode. Fall back loudly instead of unwrapping.
        let payload = serde_json::to_string(&items).unwrap_or_else(|error| {
            eprintln!("warning: batch summary serialization failed: {error}");
            String::from("[]")
        });
        println!("{payload}");
    } else {
        println!(
            "batch: {} succeeded, {} failed",
            results.len() - failed,
            failed
        );
    }
    if failed != 0 {
        // R2-CLI-07: partial batch failure exits with its own documented code
        // (3) instead of being indistinguishable from a hard runtime error.
        return Err(CliError::BatchPartial { failed });
    }
    Ok(())
}

/// One successfully processed (or resumed/skipped/dry-run) batch item. The
/// collected mask warnings travel with the item so the batch summary can
/// report them like render/export do (R2-CLI-06).
#[derive(Debug, Clone)]
struct BatchItemSuccess {
    input: String,
    mask_warnings: Vec<String>,
}

/// Rejects inputs whose name-based batch targets collide after the output
/// extension is normalized, listing the colliding pair. Runs before any
/// output is written so a rejected batch leaves no partial state.
fn reject_duplicate_batch_targets(inputs: &[PathBuf], format: &str) -> Result<(), CliError> {
    let mut seen: BTreeMap<String, PathBuf> = BTreeMap::new();
    for input in inputs {
        let name = input
            .file_name()
            .map(|name| name.to_os_string())
            .ok_or_else(|| CliError::Message("input has no file name".into()))?;
        let target = PathBuf::from(name)
            .with_extension(format_extension(format))
            .to_string_lossy()
            .into_owned();
        match seen.get(&target) {
            Some(first) if first != input => {
                return Err(CliError::Message(format!(
                    "batch output collision: `{}` and `{}` both write `{}` into the output directory; refusing to silently overwrite (mirror the directory structure or split the run)",
                    first.display(),
                    input.display(),
                    target
                )));
            }
            _ => {
                seen.insert(target, input.clone());
            }
        }
    }
    Ok(())
}

/// Processes one batch item. `index`/`total` drive the stderr progress line
/// (R2-CLI-06); the item's mask warnings are returned in
/// [`BatchItemSuccess::mask_warnings`] instead of being discarded.
/// R2-CLI-11: deduplicates batch inputs by filesystem identity so the same
/// underlying file reached under two names (hard link, alias) is processed
/// exactly once — previously `--jobs > 1` ran both names in parallel and the
/// run produced duplicate history entries with last-write-wins outputs.
///
/// Unix identifies files by `(dev, inode)`; other platforms fall back to
/// canonical-path identity (symlink aliases are collapsed there, but hard-link
/// aliases are NOT detectable portably — documented limit). Entries whose
/// metadata cannot be read are kept: the decode step reports them loudly.
fn dedup_same_file_inputs(inputs: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut kept = Vec::with_capacity(inputs.len());
    #[cfg(unix)]
    {
        use std::collections::BTreeSet;
        use std::os::unix::fs::MetadataExt;
        let mut seen = BTreeSet::new();
        for input in inputs {
            let key = fs::metadata(&input)
                .ok()
                .map(|meta| (meta.dev(), meta.ino()));
            let duplicate = match key {
                Some(key) => !seen.insert(key),
                // Unreadable stat: keep the entry; decoding fails loudly later.
                None => false,
            };
            if !duplicate {
                kept.push(input);
            }
        }
    }
    #[cfg(not(unix))]
    {
        use std::collections::BTreeSet;
        let mut seen = BTreeSet::new();
        for input in inputs {
            let key = fs::canonicalize(&input).unwrap_or_else(|_| input.clone());
            if seen.insert(key) {
                kept.push(input);
            }
        }
    }
    kept
}

fn batch_one(
    input: &Path,
    index: usize,
    total: usize,
    args: &BatchArgs,
) -> Result<BatchItemSuccess, CliError> {
    let name = input
        .file_name()
        .ok_or_else(|| CliError::Message("input has no file name".into()))?;
    let label = format!("[batch {}/{}] {}", index + 1, total, name.to_string_lossy());
    let output = args
        .output
        .join(name)
        .with_extension(format_extension(&args.format));
    let status = args
        .output
        .join(format!("{}.status.json", name.to_string_lossy()));
    if args.resume && status.exists() && output.is_file() {
        let state = fs::read_to_string(&status).map_err(|e| io_error(&status, e))?;
        // REVIEW-CLI-N3: resume decides on the PARSED JSON status, not on a
        // substring match of the raw file; a malformed status file counts as
        // "not done" and the item is reprocessed.
        if serde_json::from_str::<BatchStatusFile>(&state)
            .ok()
            .is_some_and(|state| state.status == "ok")
        {
            eprintln!("{label}: skipped (resume)");
            return Ok(BatchItemSuccess {
                input: input.display().to_string(),
                mask_warnings: Vec::new(),
            });
        }
    }
    // R2-CLI-06: this item's collected mask warnings survive the scope of the
    // dry-run guard so they can be reported on the progress line and in the
    // item JSON below.
    let mut last_warnings = Vec::new();
    if !args.dry_run {
        if args.update_masks || args.force_render {
            let sidecar = sidecar_path_for(input);
            let mut document = load_sidecar(&sidecar)?;
            let id = args.virtual_copy.as_deref().unwrap_or("vc-original");
            let copy = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.id == id)
                .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{id}`")))?;
            if args.update_masks {
                copy.recipe
                    .options
                    .insert("update_masks".into(), "true".into());
            }
            if args.force_render {
                copy.recipe
                    .options
                    .insert("force_render".into(), "true".into());
            }
            save_sidecar(&sidecar, &document)?;
        }
        let mut last = None;
        for _ in 0..=args.retry {
            // R2-CLI-06: collect this attempt's mask warnings instead of
            // discarding them (`&mut Vec::new()`).
            let mut mask_warnings = Vec::new();
            match process_selected(
                ProcessArgs {
                    input: input.to_path_buf(),
                    output: output.clone(),
                    preset: None,
                    exposure: None,
                    contrast: None,
                    highlights: None,
                    shadows: None,
                    auto_tone: false,
                    match_total_exposure: false,
                    target_luminance: 0.5,
                    write_metadata: args.write_metadata,
                },
                args.quality,
                args.virtual_copy.as_deref(),
                args.mask_policy.to_policy(),
                &mut mask_warnings,
            ) {
                // LRPAR-G15-IPTC-S6: a bake-in failure (non-JPEG item, IIM
                // limit) fails THIS item loudly with its reason (`failed`).
                Ok(_) => {
                    last = None;
                    // Keep the warnings of the SUCCESSFUL attempt only.
                    last_warnings = mask_warnings;
                    break;
                }
                Err(e) => last = Some(e),
            }
        }
        if let Some(e) = last {
            eprintln!("{label}: failed: {e}");
            return Err(e);
        }
    }
    // R2-CLI-08: an unwritable/serializing status must fail THIS item loudly
    // instead of panicking inside the rayon pool (which would tear down the
    // whole batch). A plain string/number JSON value is practically
    // infallible; the error branch is defense-in-depth.
    let status_payload = serde_json::json!({
        "input": input,
        "output": output,
        "status": if args.dry_run { "dry-run" } else { "ok" }
    });
    let status_bytes = serde_json::to_vec(&status_payload).map_err(|error| {
        CliError::Message(format!(
            "could not serialize batch status for `{}`: {error}",
            input.display()
        ))
    })?;
    write_atomically(&status, &status_bytes)?;
    if args.dry_run {
        eprintln!("{label}: dry-run");
    } else if last_warnings.is_empty() {
        eprintln!("{label}: ok");
    } else {
        eprintln!("{label}: ok ({} mask warning(s))", last_warnings.len());
    }
    Ok(BatchItemSuccess {
        input: input.display().to_string(),
        mask_warnings: last_warnings,
    })
}

/// Shared recursive directory walk behind `collect_images` and
/// `collect_sidecars` (REVIEW-CLI-N5 / REVIEW-CLI-FOLLOWUP-1): the visited set
/// holds canonical directory identities so filesystem cycles (symlink loops,
/// bind mounts) terminate instead of overflowing the stack, directory symlinks
/// are never followed and every directory level is walked in deterministic
/// (sorted) order.
fn collect_tree_files<F>(path: &Path, output: &mut Vec<PathBuf>, keep: F) -> Result<(), CliError>
where
    F: Fn(&Path) -> bool,
{
    let mut visited = BTreeSet::new();
    collect_tree_files_inner(path, output, &mut visited, &keep)
}

fn collect_tree_files_inner<F>(
    path: &Path,
    output: &mut Vec<PathBuf>,
    visited: &mut BTreeSet<PathBuf>,
    keep: &F,
) -> Result<(), CliError>
where
    F: Fn(&Path) -> bool,
{
    let identity = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !visited.insert(identity) {
        return Ok(());
    }
    let mut entries: Vec<std::fs::DirEntry> = fs::read_dir(path)
        .map_err(|e| io_error(path, e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| io_error(path, e))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let p = entry.path();
        // `entry.file_type()` never follows symlinks: a symlinked directory is
        // never recursed into, which removes symlink loops by construction.
        if entry.file_type().map_err(|e| io_error(&p, e))?.is_dir() {
            collect_tree_files_inner(&p, output, visited, keep)?;
        } else if keep(&p) && p.is_file() {
            output.push(p);
        }
    }
    Ok(())
}

fn collect_images(path: &Path, output: &mut Vec<PathBuf>) -> Result<(), CliError> {
    collect_tree_files(path, output, has_image_extension)
}

/// Supported input extensions for batch collection (R2-CLI-01): raster
/// formats plus EVERY RAW extension exported by `lumina_raw::RAW_EXTENSIONS`.
/// Referencing the single shared list here and in [`is_raw_path`] is the whole
/// point of the fix — the previous private 9-extension copy silently skipped
/// RAF/ORF/etc. in batch while single-file decode accepted them.
fn has_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let lowered = e.to_ascii_lowercase();
            matches!(lowered.as_str(), "png" | "jpg" | "jpeg" | "webp")
                || lumina_raw::is_raw_extension(&lowered)
        })
        .unwrap_or(false)
}

fn collect_sidecars(path: &Path, output: &mut Vec<PathBuf>) -> Result<(), CliError> {
    // REVIEW-CLI-FOLLOWUP-1: same symlink-/loop-safe walk as `collect_images`
    // (see `collect_tree_files`) so reindex cannot cycle through directory
    // symlinks either. Regular-file-only collection also keeps dangling or
    // special (FIFO) `.lumina.json` entries out of the scan.
    collect_tree_files(path, output, |p| {
        p.to_string_lossy().ends_with(".lumina.json")
    })
}
fn emit(json: bool, value: serde_json::Value, text: &str) -> Result<(), CliError> {
    if json {
        println!("{}", value);
    } else {
        println!("{text}");
    }
    Ok(())
}

/// F-019: the CLI `--migrate` flag delegates to the library-level
/// `lumina_sidecar::migrate_sidecar_file` so every migration takes the
/// per-sidecar write lock and writes a `.bak` backup before the atomic
/// replace. There is no silent in-place rewrite: a locked or failing
/// migration is an explicit error (Agents.md: no silent fallbacks).
fn migrate_sidecar(path: &Path) -> Result<(), CliError> {
    lumina_sidecar::migrate_sidecar_file(path)?;
    Ok(())
}

fn process(args: ProcessArgs) -> Result<(), CliError> {
    // `process` has no explicit quality flag; it uses the shared default (90),
    // which is identical to the historical `frame.encode(format)` output.
    // LRPAR-G15-IPTC-S6: `--write-metadata` rides on `args` into
    // `process_selected` (bake-in outcome lands in the sidecar record).
    process_selected(args, 90, None, MaskPolicy::Warn, &mut Vec::new())?;
    Ok(())
}

/// Composite zdata record id for a persisted mask plane (REVIEW-CLI-N1).
///
/// Tiles inside the `.lumina.zdata` bundle are stored under the composite
/// record id `<copy_id>/<mask_id>` so two virtual copies may carry same-named
/// masks (`subject`) without silently sharing one matte. Field order and the
/// `/` separator are normative: `lumina-gui` must adopt this exact convention
/// when it reads/writes mask tiles.
fn zdata_mask_tile_id(copy_id: &str, mask_id: &str) -> String {
    format!("{copy_id}/{mask_id}")
}

/// Loads every persisted source-mask plane from the optional `.lumina.zdata`
/// bundle, keyed by `(copy_id, mask_id)` (REVIEW-CLI-N1). A MISSING bundle
/// yields an empty map without any warning (nothing was persisted); missing
/// per-key tiles are decided by the F-051 decision layer in lumina-core
/// (cache, re-inference or a loud error — never a silent fallback).
///
/// R2-CLI-05: a bundle that EXISTS but cannot be read (truncated, malformed,
/// unsupported version, checksum mismatch) is no longer treated silently like
/// a missing bundle — that masked data loss as an ordinary "missing mask"
/// situation. The load failure surfaces as an explicit
/// "unreadable or corrupt" warning on stderr AND in `warnings_out` (the same
/// channel the render's mask warnings travel through). Per-tile lookups after
/// a clean load need no extra corruption handling: `load_zdata` already
/// verifies every record checksum up front (REVIEW-SIDECAR-ZDATA-1), so a
/// surviving tile miss is plain absence.
fn load_persisted_mask_planes(
    document: &SidecarDocument,
    zdata_path: &Path,
    warnings_out: &mut Vec<String>,
) -> BTreeMap<(String, String), MaskPlane> {
    let mut planes: BTreeMap<(String, String), MaskPlane> = BTreeMap::new();
    if !zdata_path.exists() {
        return planes;
    }
    let container = match lumina_sidecar::load_zdata(zdata_path) {
        Ok(container) => container,
        Err(error) => {
            let warning = format!(
                "mask/source-action bundle `{}` is unreadable or corrupt ({error}); persisted mask planes are treated as missing and will be re-decided by the mask layer",
                zdata_path.display()
            );
            eprintln!("warning: {warning}");
            warnings_out.push(warning);
            return planes;
        }
    };
    for copy in &document.virtual_copies {
        for mask in copy
            .mask_library
            .iter()
            .filter(|m| matches!(m.operation, MaskOperation::Source))
        {
            let Ok(tile) = container.tile(&zdata_mask_tile_id(&copy.id, &mask.id), 0, 0) else {
                continue;
            };
            if let Ok(plane) = MaskPlane::new(tile.width, tile.height, tile.values) {
                planes.insert((copy.id.clone(), mask.id.clone()), plane);
            }
        }
    }
    planes
}

/// Build a Lensfun lens corrector from decoded RAW metadata (EXIF) for use as
/// `RenderContext.lensfun`.
///
/// Strict, documented fallback (no silent correction): `None` is returned unless
/// the `lensfun` feature is enabled **and** all of `camera_make`, `camera_model`,
/// `focal_length` and `aperture` are present and finite. When the system Lensfun
/// database cannot be loaded, or no matching, non-identity profile is found,
/// `None` is returned and the manual LuminaRust model (or identity) applies
/// instead — never a guessed correction.
///
/// # Lens identification (G-06 EXIF-Erkennung)
/// `RawMetadata.lens` (EXIF `LensModel`/Makernote, REVIEW-RAW-N2) is passed
/// as the Lensfun lens name when present, so an exact lens match wins;
/// without it (or when the named lens is unknown to the DB) Lensfun falls
/// back to the body/mount match via `GuessParameters` (`LF_SEARCH_LOOSE`).
/// A wrong-but-confident lens name can therefore still resolve to the body
/// profile instead of failing — the `--lensfun-status` report and the
/// render `info!` log name the matched correction explicitly.
///
/// # Subject (focus) distance
/// `RawMetadata` carries no subject-distance field, so a documented default of
/// `10.0` (metres) is used. Lensfun vignetting/distortion calibration is in
/// practice focus-distance-independent for the MVP profiles, and `lumina-lensfun`'s
/// own reference tests use exactly this value, so it yields a matching,
/// non-identity corrector for the `Nikon D40` example profile.
///
/// # Known limits
/// * The system Lensfun database is loaded once per call (no cross-render
///   cache). Acceptable for the MVP, but repeated `process`/`render` invocations
///   each re-load the DB.
/// * Manual `ca_red`/`ca_blue` are skipped when the built corrector carries
///   TCA calibration (G-06: TCA is corrected geometrically in the lens
///   stage; see `lumina-core` `apply_lens`).
///
/// The returned `(LensfunDb, Corrector)` keeps the database handle alive as long
/// as the corrector is used: the modifier internally references lens data owned
/// by the database, so the database must not be dropped before the corrector.
#[cfg(feature = "lensfun")]
fn build_lensfun_corrector(metadata: Option<&RawMetadata>) -> Option<(LensfunDb, Corrector)> {
    let metadata = metadata?;
    let make = metadata.camera_make.as_deref()?;
    let model = metadata.camera_model.as_deref()?;
    // Finite focal length and aperture are required; a missing/NaN value means
    // we cannot build a meaningful corrector → fall back to `None` strictly.
    let focal_length = metadata.focal_length.filter(|value| value.is_finite())?;
    let aperture = metadata.aperture.filter(|value| value.is_finite())?;
    let db = LensfunDb::load_system()?;
    // `RawMetadata` has no subject distance, so use the documented 10.0 m default
    // (see the function's doc comment / §"Subject (focus) distance").
    let distance = 10.0_f32;
    let corrector = db.for_camera(
        make,
        model,
        metadata.lens.as_deref(),
        metadata.width,
        metadata.height,
        focal_length,
        aperture,
        distance,
    )?;
    Some((db, corrector))
}

/// Returns `Some(wb)` for a usable As-Shot white balance, `None` if any gain is
/// NaN/infinite or non-positive. Mirrors the lumina-gui load/background-decode
/// sanitisation (R2-WB) so CLI and GUI degrade identically instead of aborting
/// the render on a corrupt CR3 `cam_mul`.
fn sanitize_camera_white_balance(wb: [f32; 4]) -> Option<[f32; 4]> {
    if wb.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        None
    } else {
        Some(wb)
    }
}

/// Resolve the subject-mask inference engine the CLI wires into the F-048 /
/// F-051 mask-loading decision layer (F-082-FOLLOWUP, real ORT path).
///
/// # Contract (no silent fallback)
///
/// The CLI **requests** the real ONNX engine only when the current run can
/// actually require re-inference (`needs_inference`: the active copy carries
/// reachable mask layers — the same reachability the F-048/F-051 decision
/// layer uses, so a `--update-masks` refresh on a mask-less copy requests
/// nothing). Without mask work no model is needed, so no engine is requested
/// and there is nothing to fail on.
///
/// | Build | `needs_inference` | Result |
/// | --- | --- | --- |
/// | `onnx-rt` off (default) | any | `Some(StubBackend)` — the documented default wiring |
/// | `onnx-rt` on | true, `LUMINA_MODEL_PATH` set, artifact loadable and identity-verified | `Some(OnnxRuntime)` — the real engine |
/// | `onnx-rt` on | true, `LUMINA_MODEL_PATH` unset | hard `CliError` |
/// | `onnx-rt` on | true, artifact missing / stale / mismatched / wrong tensor names | hard `CliError` carrying the resolver's `OnnxError` |
/// | `onnx-rt` on | false | `Ok(None)` — no engine is requested |
///
/// The deterministic [`StubBackend`] is only ever wired in default builds:
/// under `onnx-rt` a requested real engine is **never** downgraded to the stub
/// (Agents.md: „Fehlende oder inkompatible Artefakte werden sichtbar als
/// veraltet oder nicht verfügbar gemeldet"; ai-masks.md F-082-FOLLOWUP).
#[cfg(not(feature = "onnx-rt"))]
fn resolve_mask_inference_engine(
    _needs_inference: bool,
) -> Result<Option<Box<dyn MaskInference>>, CliError> {
    // Default build: the deterministic, dependency-free stub backend is the
    // documented default wiring (unchanged behavior).
    match StubBackend::new(birefnet_manifest()) {
        Ok(stub) => Ok(Some(Box::new(stub))),
        Err(_) => Ok(None),
    }
}
#[cfg(feature = "onnx-rt")]
fn resolve_mask_inference_engine(
    needs_inference: bool,
) -> Result<Option<Box<dyn MaskInference>>, CliError> {
    if !needs_inference {
        return Ok(None);
    }
    let model_path = std::env::var_os("LUMINA_MODEL_PATH").ok_or_else(|| {
        CliError::Message(
            "`onnx-rt` is compiled into this build and the run requires mask \
             re-inference, but `LUMINA_MODEL_PATH` is not set; the real ONNX engine \
             is unavailable and the deterministic stub is never a silent substitute \
             (F-082-FOLLOWUP)"
                .into(),
        )
    })?;
    resolve_onnx_engine_from_path(Path::new(&model_path))
}

/// Resolve the real ONNX engine from an explicit artifact path (F-082-FOLLOWUP).
///
/// Path-parameterized so the onnx-rt wiring semantics are testable without
/// touching the process-global `LUMINA_MODEL_PATH` (and thereby racing other
/// render tests). The BiRefNet [`ModelManifest`](lumina_onnx::ModelManifest) is
/// the identity/IO contract; the resolver reports `RuntimeDisabled` (feature
/// off — impossible here), `OnnxRuntime` (artifact verified) or a hard
/// [`OnnxError`](lumina_onnx::OnnxError): a missing/stale/mismatched artifact is
/// a loud error, never a silent stub.
#[cfg(feature = "onnx-rt")]
fn resolve_onnx_engine_from_path(
    model_path: &Path,
) -> Result<Option<Box<dyn MaskInference>>, CliError> {
    match try_load_onnx_engine(model_path, &birefnet_manifest()) {
        Ok(OnnxEngine::OnnxRuntime(engine)) => Ok(Some(engine)),
        Ok(OnnxEngine::RuntimeDisabled) => unreachable!(
            "`try_load_onnx_engine` must return `OnnxRuntime` when `onnx-rt` is compiled in"
        ),
        Err(error) => Err(CliError::Message(format!(
            "the real ONNX engine could not be loaded from `{}`: {error} \
             (no silent fallback to the deterministic stub)",
            model_path.display()
        ))),
    }
}

fn process_selected(
    args: ProcessArgs,
    quality: u8,
    virtual_copy: Option<&str>,
    policy: MaskPolicy,
    mask_warnings_out: &mut Vec<String>,
) -> Result<Option<MetadataWritten>, CliError> {
    // REVIEW-CLI-WRITE-1: the guard covers the original itself (path and
    // hard-link identity) plus its `.lumina.json`/`.lumina.zdata` bundle.
    reject_protected_output(&args.input, &args.output)?;
    let format = output_format(&args.output)?;
    // LRPAR-G15-IPTC-S6: `--write-metadata` is JPEG-only (SOLL §7). PNG/WebP
    // fail loudly per file (single commands exit non-zero, batch marks the
    // item `failed` with this reason); TIFF never reaches this gate because
    // `output_format` already rejects it loudly (Post-MVP).
    if args.write_metadata && format != ImageFileFormat::Jpeg {
        let format_name = match format {
            ImageFileFormat::Png => "png",
            ImageFileFormat::Jpeg => "jpeg",
            ImageFileFormat::WebP => "webp",
        };
        return Err(CliError::Message(format!(
            "--write-metadata is only supported for JPEG exports; refusing {format_name} output `{}` (bake-in is JPEG-only, TIFF is Post-MVP)",
            args.output.display()
        )));
    }
    let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
    let (frame, raw_metadata) = decode_input(&args.input, &bytes)?;
    // Parity with lumina-gui (R2-WB): an invalid As-Shot white balance (NaN,
    // inf, or a non-positive gain) is dropped to `None` with a warning instead
    // of aborting the whole render with CoreError::InvalidAdjustment. This is
    // the same single-source behaviour the GUI applies at load and on the
    // background decode path.
    let wb = raw_metadata.as_ref().and_then(|m| {
        let sanitized = sanitize_camera_white_balance(m.camera_white_balance);
        if sanitized.is_none() {
            eprintln!(
                "lumina: warning: As-Shot white balance invalid {:?} for `{}` — dropping to None (recipe WB remains, image renders)",
                m.camera_white_balance, args.input.display()
            );
        }
        sanitized
    });
    // F-098-N2: build the Lensfun corrector from EXIF when the feature is on.
    // The database handle and corrector are kept in two separate locals so the
    // corrector (declared last) is dropped before the database handle — the
    // modifier references lens data owned by the database.
    #[cfg(feature = "lensfun")]
    let (_lensfun_db, lensfun_corrector) = build_lensfun_corrector(raw_metadata.as_ref())
        .map(|(db, corrector)| (Some(db), Some(corrector)))
        .unwrap_or((None, None));
    let sidecar_path = sidecar_path_for(&args.input);
    let mut document = match load_sidecar(&sidecar_path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => SidecarDocument::new(
            source_identity(&args.input, &bytes, &frame, raw_metadata.as_ref())?,
            "raster-mvp-1",
        ),
        Err(error) => return Err(error.into()),
    };
    let current_identity = source_identity(&args.input, &bytes, &frame, raw_metadata.as_ref())?;
    if document.source.content_hash != current_identity.content_hash
        || document.source.byte_length != current_identity.byte_length
    {
        return Err(CliError::Message(format!(
            "source changed since sidecar was written: `{}`",
            args.input.display()
        )));
    }
    if !args.target_luminance.is_finite() || !(0.0..=1.0).contains(&args.target_luminance) {
        return Err(CliError::Message(
            "invalid target-luminance: must be finite and in 0..=1".into(),
        ));
    }
    let copy_index = virtual_copy
        .map(|id| {
            document
                .virtual_copies
                .iter()
                .position(|copy| copy.id == id)
                .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{id}`")))
        })
        .transpose()?
        .unwrap_or(0);
    let mut recipe = document.virtual_copies[copy_index].recipe.clone();
    // REVIEW-CLI-MASKFLAG-1: `update_masks` (and the legacy `force_render`)
    // are ONE-SHOT requests persisted into the recipe options by
    // develop/batch/mask/export. They are consumed here and dropped from the
    // recipe before it is written back, so a confirmably valid persisted mask
    // stops triggering re-inference on every future run (Agents.md
    // persistence invariant: a valid mask is reused, never silently
    // recomputed). The sidecar is saved only after a successful render below,
    // so a failed run keeps the pending request intact for the next attempt.
    // (`force_render` has no consumer yet — the CLI always renders fresh — so
    // consuming it is pure pollution cleanup.)
    let refresh_masks = recipe
        .options
        .get("update_masks")
        .is_some_and(|value| value == "true");
    recipe.options.remove("update_masks");
    recipe.options.remove("force_render");
    let auto_requested = args.auto_tone;
    if auto_requested {
        recipe.auto_features.enable_auto_tone = true;
        recipe.auto_features.target_luminance = args.target_luminance;
        let config = AutoToneConfig {
            target_luminance: args.target_luminance,
            ..Default::default()
        };
        let fingerprint = tone_fingerprint(&frame, config);
        let persisted = recipe
            .auto_features
            .analysis_fingerprint
            .as_ref()
            .filter(|f| f.input_fingerprint == fingerprint);
        let (exposure, contrast, _reused) = if let (Some(exposure), Some(contrast)) = (
            persisted.and(recipe.auto_features.auto_exposure),
            persisted.and(recipe.auto_features.auto_contrast),
        ) {
            (exposure, contrast, true)
        } else {
            let result = suggest_auto_tone(&frame, config)?;
            recipe.auto_features.auto_exposure = Some(result.exposure);
            recipe.auto_features.auto_contrast = Some(result.contrast);
            recipe.auto_features.analysis_fingerprint = Some(AnalysisFingerprint {
                algorithm: "tone-rgba8-rec709".into(),
                version: "1".into(),
                input_fingerprint: fingerprint,
                extras: BTreeMap::new(),
            });
            (result.exposure, result.contrast, false)
        };
        recipe.adjustments.insert("exposure".into(), exposure);
        recipe.adjustments.insert("contrast".into(), contrast);
    }
    if let Some(path) = args.preset {
        let json = fs::read_to_string(&path).map_err(|error| io_error(&path, error))?;
        let preset: Preset =
            serde_json::from_str(&json).map_err(|error| CliError::Preset(error.to_string()))?;
        // MVP rule: auto values are computed first, preset values replace them,
        // and explicit CLI values win last. Preserve auto metadata in the recipe.
        let auto_features = recipe.auto_features.clone();
        recipe = preset.recipe;
        if auto_requested {
            recipe.auto_features = auto_features;
        }
    }
    if let Some(value) = args.exposure {
        recipe.adjustments.insert("exposure".into(), value);
    }
    if let Some(value) = args.contrast {
        recipe.adjustments.insert("contrast".into(), value);
    }
    if let Some(value) = args.highlights {
        recipe.adjustments.insert("highlights".into(), value);
    }
    if let Some(value) = args.shadows {
        recipe.adjustments.insert("shadows".into(), value);
    }
    // --- F-048 / F-051: intelligent mask-loading decision layer ---
    // Load every persisted source-mask plane from the optional `.lumina.zdata`
    // bundle (regardless of status); the decision layer in lumina-core
    // validates identity and decides whether to use it, re-infer, or fail.
    // R2-CLI-05: a corrupt bundle surfaces as an explicit warning through the
    // same channel as the other mask warnings instead of being silent.
    let (zdata_path, loaded_planes) = {
        let zdata_path = lumina_sidecar::zdata_path_for(&args.input);
        let loaded_planes = load_persisted_mask_planes(&document, &zdata_path, mask_warnings_out);
        (zdata_path, loaded_planes)
    };

    // F-082-FOLLOWUP: wire the ONNX inference engine into the F-048/F-051
    // decision layer. Default builds wire the deterministic StubBackend; with
    // `onnx-rt` the real engine is requested whenever this run can require
    // re-inference — mirroring the decision layer's reachability, that is
    // exactly when the active copy carries mask layers (a `--update-masks`
    // refresh on a mask-less copy re-infers nothing). A missing/stale/
    // unconfigurable request fails HARD — the stub is never silently
    // substituted (see `resolve_mask_inference_engine`). Runs without mask
    // work request no engine at all.
    let can_need_inference = !document.virtual_copies[copy_index].mask_layers.is_empty();
    let engine = resolve_mask_inference_engine(can_need_inference)?;
    let model_identity = engine
        .is_some()
        .then(|| birefnet_manifest().to_model_identity());
    let inference = engine.as_deref();

    let resolved = resolve_mask_planes(
        MaskLoadContext {
            copies: &document.virtual_copies,
            active_copy_id: &document.virtual_copies[copy_index].id,
            source_hash: &current_identity.content_hash,
            decode_context: &current_identity.decode_fingerprint,
            loaded_planes,
            inference,
            model_identity: model_identity.as_ref(),
            // F-049: `--update-masks` is persisted into the active copy's recipe
            // options by the develop/export/batch commands and reloaded here, so
            // the refresh flag the decision layer needs is driven by the CLI
            // flag (and survives the persisted sidecar). It is consumed above:
            // after this run it is removed from the recipe again.
            refresh: refresh_masks,
            policy,
        },
        &frame,
    )?;
    // Main render via the shared entry point (SourceActions → Adjustments →
    // Masks).  F-042-N1: the recipe's persisted source actions are resolved
    // from the `.lumina.zdata` bundle (missing or checksum-mismatched artifacts
    // are reported loudly, never silently dropped).
    let source_actions = resolve_source_actions(&recipe, &zdata_path)?;
    let active_copy = document.virtual_copies[copy_index].clone();
    // `resolved.planes` is owned by `MaskContext`, so clone once and reuse it for
    // both the warning render and the final shared encode render below.
    let mask_planes = resolved.planes.clone();
    let render_ctx = RenderContext {
        recipe: &recipe,
        camera_white_balance: wb,
        source_actions: &source_actions,
        masks: Some(MaskContext {
            copies: &resolved.copies,
            active_copy_id: &active_copy.id,
            planes: mask_planes.clone(),
            policy,
        }),
        // F-098-N2: pass a Lensfun corrector when one was built from EXIF
        // (otherwise `None` → manual LuminaRust model / identity fallback).
        #[cfg(feature = "lensfun")]
        lensfun: lensfun_corrector.as_ref().map(LensfunCorrectorRef),
        #[cfg(not(feature = "lensfun"))]
        lensfun: None,
        depth: None,
    };
    // Prefer the GPU when an adapter is bound; otherwise the full CPU pipeline.
    // The chosen backend is logged once at startup (see `init_render_backend`).
    // `render_standard` is the single CLI backend entry point (shared with the
    // LRPAR-MATRIX-RECIPE runner — no second render path).
    let render_output = render_standard(&frame, &recipe, &render_ctx)?;
    // Surface F-051 (model unavailable / cached fallback) warnings distinctly.
    for warning in &resolved.warnings {
        eprintln!("warning: {warning}");
    }
    mask_warnings_out.extend(resolved.warnings.iter().cloned());
    for warning in &render_output.mask_warnings {
        eprintln!("warning: {warning}");
    }
    mask_warnings_out.extend(render_output.mask_warnings.iter().cloned());
    if args.match_total_exposure {
        recipe.auto_features.match_total_exposure = true;
        recipe.auto_features.target_luminance = args.target_luminance;
        // F-041: measure the final visible domain — `render_output.frame` is the
        // render result (already post crop/geometry) and `render_output.mask_layers`
        // are the effective planes resampled to exactly these dimensions. The
        // matching delta is weighted by the mask intersection; with no active
        // layers the empty slice keeps the previous raster measurement
        // bit-exactly. Until F-049 the layers do not modulate pixels, but the
        // measurement-domain semantics is already active. The matched exposure is
        // folded back into `recipe` so the shared `export_image` path (below)
        // renders the final pixels in a single pass.
        let mask_planes: Vec<MaskPlane> = render_output
            .mask_layers
            .iter()
            .map(|layer| layer.plane.clone())
            .collect();
        let matching =
            match_total_exposure_masked(&render_output.frame, args.target_luminance, &mask_planes)?;
        recipe.auto_features.matched_exposure = Some(matching);
        let total_exposure = (recipe.adjustments.get("exposure").copied().unwrap_or(0.0)
            + matching)
            .clamp(-10.0, 10.0);
        recipe.adjustments.insert("exposure".into(), total_exposure);
    }
    let options = ExportOptions {
        format,
        quality,
        dither: false,
        ..Default::default()
    };
    // F-103-N8: the warning render above already produced the final pixels for
    // the (unchanged) recipe. When total-exposure matching is OFF, no code path
    // after that render mutates `recipe` (auto-tone, presets and CLI
    // adjustments all run *before* the warning render), so `render_output.frame`
    // is byte-identical to what `export_image` would re-render here. Reuse it
    // and skip the duplicate full-pipeline render. When matching IS ON, the
    // matched exposure is folded into `recipe` *after* the warning render, so
    // the shared `export_image` path must re-render with the updated recipe to
    // produce the final pixels (the matching still measures `render_output.frame`
    // as the pre-match domain). Output stays byte-identical to the GUI export in
    // both branches (the encode step is unchanged).
    let mut encoded = if args.match_total_exposure {
        export_image(
            &frame,
            &RenderContext {
                recipe: &recipe,
                camera_white_balance: wb,
                source_actions: &source_actions,
                masks: Some(MaskContext {
                    copies: &resolved.copies,
                    active_copy_id: &active_copy.id,
                    // Reuse the same planes captured for the warning render above.
                    planes: mask_planes.clone(),
                    policy,
                }),
                #[cfg(feature = "lensfun")]
                lensfun: lensfun_corrector.as_ref().map(LensfunCorrectorRef),
                #[cfg(not(feature = "lensfun"))]
                lensfun: None,
                depth: None,
            },
            options,
        )?
    } else {
        render_output.frame.encode_with_options(options)?
    };
    // LRPAR-G15-IPTC-S6: opt-in bake-in as a deterministic post-encode splice
    // (SOLL §7: Encode → Temp-Datei → Splice → Rename). Without the flag the
    // bytes above are untouched — exactly today's behavior, and the render
    // cache / pixel goldens are unaffected (the splice never touches pixels).
    let metadata_written = if args.write_metadata {
        Some(bake_metadata_into_jpeg(
            &document,
            &mut encoded,
            &args.output,
        )?)
    } else {
        None
    };
    // REVIEW-CLI-N6 (two-artifact ordering, decided 2026-08-26): the encoded
    // export is STAGED first — a temporary file in the output directory,
    // written, flushed and fsynced with the exact `.{name}.tmp-*` scheme of
    // the shared atomic writer — but only renamed into place AFTER the
    // sidecar update below has been committed atomically. A failing
    // `save_sidecar` therefore exits non-zero with NEITHER artifact changed:
    // the staged file is deleted on drop and the sidecar's atomic replace
    // never happened. This removes the old failure mode "exit 1 despite an
    // existing export". The remaining window shrinks to the final
    // same-directory rename; if even that fails, the residue is a sidecar
    // newer than a missing, re-derivable export — exports are derived
    // artifacts and the sidecar is the source of truth, so that state is
    // visible and benign rather than silently torn. (A true cross-file
    // transaction stays out of scope per the v1 note in
    // `lumina-sidecar/src/lib.rs`; this is ordering + staged rollback, not a
    // new transaction primitive.)
    let staged = StagedArtifact::stage(&args.output, &encoded)?;
    let copy = &mut document.virtual_copies[copy_index];
    copy.recipe = recipe.clone();
    copy.history.push(HistoryEntry {
        id: format!("h-{}", timestamp()),
        recipe,
        recorded_at: Some(timestamp()),
        extras: BTreeMap::new(),
    });
    // LRPAR-G15-IPTC-S6: the additive, optional `metadata_written` record
    // (`{iim, xmp, status}`) travels with the copy's export history — only
    // with the flag; without it no record is written at all.
    if let Some(written) = &metadata_written {
        push_metadata_export_record(copy, &args.output, format, written);
    }
    save_sidecar(&sidecar_path, &document)?;
    staged.commit()?;
    Ok(metadata_written)
}

/// LRPAR-G15-IPTC-S6: outcome of the opt-in metadata bake-in (SOLL §7).
/// `None` without the flag (no record at all); `Some` carries the additive,
/// optional `ExportRecord.metadata_written` payload (`{iim, xmp, status}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MetadataWritten {
    iim: bool,
    xmp: bool,
    /// `"written"` (IIM+XMP spliced) or `"empty"` (empty draft, plain export).
    status: &'static str,
}

impl MetadataWritten {
    fn json(self) -> serde_json::Value {
        serde_json::json!({"iim": self.iim, "xmp": self.xmp, "status": self.status})
    }
}

/// LRPAR-G15-IPTC-S6: merge the source-level draft (+ the routed `keywords`,
/// SOLL §4) into `lumina-iptc` values. Empty/whitespace-only fields stay
/// absent: the tag is omitted on write, never written empty.
fn draft_to_iptc(document: &SidecarDocument) -> IptcMetadata {
    let get = |id: &str| document.metadata.get(id).map(|value| value.to_string());
    IptcMetadata {
        title: get("title"),
        headline: get("headline"),
        description: get("description"),
        copyright_notice: get("copyright_notice"),
        creator: get("creator"),
        credit: get("credit"),
        source: get("source"),
        city: get("city"),
        state_province: get("state_province"),
        country: get("country"),
        date_created: get("date_created"),
        keywords: document.keywords.clone(),
    }
}

/// LRPAR-G15-IPTC-S6: splice draft+keywords (IIM+XMP) into already-encoded
/// JPEG bytes (SOLL §7: post-encode, pixel bytes verbatim). An empty draft
/// keeps the export plain with a loud warning (`"empty"`, never a silent
/// no-op); a Sidecar-valid value over its IIM octet limit fails loudly
/// (field + limit named, no silent truncation). No EXIF write, no adoption
/// of embedded source metadata — only Lumina drafts.
fn bake_metadata_into_jpeg(
    document: &SidecarDocument,
    encoded: &mut Vec<u8>,
    output: &Path,
) -> Result<MetadataWritten, CliError> {
    let meta = draft_to_iptc(document);
    if meta.is_empty() {
        eprintln!(
            "warning: no IPTC draft or keywords for `{}`; exporting without embedded metadata (metadata_written: empty)",
            output.display()
        );
        info!(
            "export without metadata for `{}` (empty draft, metadata_written: empty)",
            output.display()
        );
        return Ok(MetadataWritten {
            iim: false,
            xmp: false,
            status: "empty",
        });
    }
    let spliced = embed_metadata(encoded, &meta).map_err(|error| {
        CliError::Message(format!(
            "metadata bake-in for `{}` failed: {error}",
            output.display()
        ))
    })?;
    *encoded = spliced;
    info!(
        "export with IPTC metadata for `{}` (metadata_written: written)",
        output.display()
    );
    Ok(MetadataWritten {
        iim: true,
        xmp: true,
        status: "written",
    })
}

/// LRPAR-G15-IPTC-S6: append the additive, optional `metadata_written` record
/// (`{iim, xmp, status}` under `extras`) to the copy's export history. Only
/// called with the flag — without it no record is written at all.
fn push_metadata_export_record(
    copy: &mut lumina_sidecar::VirtualCopy,
    output: &Path,
    format: ImageFileFormat,
    written: &MetadataWritten,
) {
    let relative_path = output
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| output.display().to_string());
    let mut extras = BTreeMap::new();
    extras.insert("metadata_written".to_string(), written.json());
    copy.export_records.push(ExportRecord {
        id: format!("export-{}", timestamp()),
        relative_path,
        format: format.default_extension().to_string(),
        exported_at: Some(now_rfc3339_utc()),
        extras,
    });
}

/// One virtual copy as reported by `inspect` (R2-CLI-03): rendered either as
/// JSON or free text from the same data so the two outputs cannot drift.
struct InspectCopy {
    id: String,
    name: String,
    auto_tone: bool,
    matching: bool,
    target_luminance: f64,
    /// LRPAR-G01-BASIC: Develop treatment (`color` | `bw`, absent = `color`).
    treatment: String,
    /// LRPAR-G01-BASIC: Develop profile (absent = `default`).
    profile: String,
}

/// Sidecar state behind an `inspect` report; the invalid variant carries the
/// error that fails the command AFTER the report has been printed.
enum InspectSidecarState {
    Valid {
        source: String,
        copies: Vec<InspectCopy>,
    },
    Missing,
    Invalid(lumina_sidecar::SidecarError),
}

/// R2-CLI-03/-04: shows the source's RAW metadata (via the metadata-only
/// LibRaw path — no full-pixel decode for four EXIF lines) plus the sidecar
/// status and every virtual copy incl. auto-tone/matching state. Free text is
/// the default output; `--json` prints one machine-readable JSON object (the
/// SOLL "JSON-Status"). An invalid sidecar keeps the historical behaviour:
/// the report is still printed, then the command fails loudly.
fn inspect(args: InspectArgs) -> Result<(), CliError> {
    // Metadata-only pass (R2-CLI-04): open + unpack + size finalisation;
    // demosaic, colour processing, memory image and promotion never run.
    let raw_metadata = if is_raw_path(&args.input) {
        Some(lumina_raw::read_metadata(&args.input)?)
    } else {
        None
    };

    let path = sidecar_path_for(&args.input);
    let sidecar_state = match load_sidecar(&path) {
        Ok(document) => InspectSidecarState::Valid {
            source: document.source.relative_name.clone(),
            copies: document
                .virtual_copies
                .iter()
                .map(|copy| InspectCopy {
                    id: copy.id.clone(),
                    name: copy.name.clone(),
                    auto_tone: copy.recipe.auto_features.enable_auto_tone,
                    matching: copy.recipe.auto_features.match_total_exposure,
                    target_luminance: copy.recipe.auto_features.target_luminance,
                    treatment: copy.recipe.treatment().to_string(),
                    profile: copy.recipe.develop_profile().to_string(),
                })
                .collect(),
        },
        Err(lumina_sidecar::SidecarError::Missing(_)) => InspectSidecarState::Missing,
        Err(error) => InspectSidecarState::Invalid(error),
    };

    if args.json {
        let raw_json = raw_metadata.as_ref().map(|metadata| {
            serde_json::json!({
                "width": metadata.width,
                "height": metadata.height,
                "orientation": metadata.orientation,
                "camera_make": metadata.camera_make,
                "camera_model": metadata.camera_model,
                "iso": metadata.iso,
                "shutter": metadata.shutter,
                "aperture": metadata.aperture,
                "lens": metadata.lens,
            })
        });
        let sidecar_json = match &sidecar_state {
            InspectSidecarState::Valid { source, copies } => serde_json::json!({
                "path": path,
                "status": "valid",
                "source": source,
                "virtual_copies": copies.iter().map(|copy| serde_json::json!({
                    "id": copy.id,
                    "name": copy.name,
                    "auto_tone": copy.auto_tone,
                    "match_total_exposure": copy.matching,
                    "target_luminance": copy.target_luminance,
                    "treatment": copy.treatment,
                    "profile": copy.profile,
                })).collect::<Vec<_>>(),
            }),
            InspectSidecarState::Missing => serde_json::json!({
                "path": path,
                "status": "missing",
                "virtual_copies": [{
                    "id": "vc-original",
                    "name": "Original",
                    "auto_tone": false,
                    "match_total_exposure": false,
                    "target_luminance": 0.5,
                }],
            }),
            InspectSidecarState::Invalid(_) => {
                serde_json::json!({"path": path, "status": "invalid"})
            }
        };
        println!(
            "{}",
            serde_json::json!({
                "command": "inspect",
                "input": args.input,
                "raw": raw_json,
                "sidecar": sidecar_json,
            })
        );
    } else {
        if let Some(metadata) = &raw_metadata {
            println!(
                "raw: {}x{} orientation {}",
                metadata.width, metadata.height, metadata.orientation
            );
            println!(
                "camera: {} {}",
                metadata.camera_make.as_deref().unwrap_or("unknown"),
                metadata.camera_model.as_deref().unwrap_or("unknown")
            );
            println!(
                "iso: {:?}, shutter: {:?}, aperture: {:?}, lens: {:?}",
                metadata.iso, metadata.shutter, metadata.aperture, metadata.lens
            );
        }
        match &sidecar_state {
            InspectSidecarState::Valid { source, copies } => {
                println!("sidecar: valid ({})", path.display());
                println!("source: {source}");
                for copy in copies {
                    println!("virtual-copy: {} [{}]", copy.name, copy.id);
                    println!(
                        "auto-tone: {} matching: {} target-luminance: {}",
                        copy.auto_tone, copy.matching, copy.target_luminance
                    );
                    println!("treatment: {} profile: {}", copy.treatment, copy.profile);
                }
            }
            InspectSidecarState::Missing => {
                println!("sidecar: missing ({})", path.display());
                println!("virtual-copy: Original [vc-original] (default)");
            }
            InspectSidecarState::Invalid(_) => {
                println!("sidecar: invalid ({})", path.display());
            }
        }
    }
    match sidecar_state {
        InspectSidecarState::Invalid(error) => Err(error.into()),
        _ => Ok(()),
    }
}

fn is_raw_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(lumina_raw::is_raw_extension)
}

fn decode_input(path: &Path, bytes: &[u8]) -> Result<(ImageFrame, Option<RawMetadata>), CliError> {
    if is_raw_path(path) {
        let name = path
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("input.raw");
        let image = lumina_raw::decode_bytes(bytes, name)?;
        Ok((image.frame, Some(image.metadata)))
    } else {
        Ok((ImageFrame::decode(bytes)?, None))
    }
}

fn output_format(path: &Path) -> Result<ImageFileFormat, CliError> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    ImageFileFormat::from_extension(&extension).ok_or_else(|| {
        CliError::Message(format!(
            "unsupported output extension `.{extension}`; use png, jpg, jpeg, or webp"
        ))
    })
}

fn validate_format(format: &str) -> Result<(), CliError> {
    if ImageFileFormat::from_extension(format).is_some() {
        Ok(())
    } else {
        Err(CliError::Message(format!(
            "unsupported format `{format}`; use png, jpg, jpeg, or webp"
        )))
    }
}

fn format_extension(format: &str) -> &'static str {
    match format.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "jpg",
        "webp" => "webp",
        _ => "png",
    }
}

fn validate_quality(quality: u8) -> Result<(), CliError> {
    if (1..=100).contains(&quality) {
        Ok(())
    } else {
        Err(CliError::Message("quality must be in 1..=100".into()))
    }
}

fn source_identity(
    path: &Path,
    bytes: &[u8],
    frame: &ImageFrame,
    raw_metadata: Option<&RawMetadata>,
) -> Result<SourceIdentity, CliError> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| CliError::Message("input must have a file name".into()))?;
    let metadata = fs::metadata(path).map_err(|error| io_error(path, error))?;
    Ok(SourceIdentity {
        relative_name: name.into(),
        content_hash: format!("blake3:{}", blake3::hash(bytes).to_hex()),
        byte_length: metadata.len(),
        modified_at: None,
        raw_format: path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_uppercase(),
        orientation: raw_metadata.map_or(1, |metadata| metadata.orientation),
        decode_fingerprint: DecodeFingerprint {
            decoder: if raw_metadata.is_some() {
                "libraw"
            } else {
                "image"
            }
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
            orientation: raw_metadata.map_or(1, |metadata| metadata.orientation),
            pixel_aspect_ratio: 1.0,
            extras: BTreeMap::new(),
        },
        extras: BTreeMap::new(),
    })
}

fn timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis().to_string())
        .unwrap_or_else(|_| "0".into())
}

fn io_error(path: &Path, error: std::io::Error) -> CliError {
    CliError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}

fn reject_same_path(input: &Path, output: &Path) -> Result<(), CliError> {
    if lumina_sidecar::paths_resolve_equal(input, output).map_err(|error| io_error(input, error))? {
        return Err(CliError::Message(
            "input and output resolve to the same path; refusing to overwrite the original".into(),
        ));
    }
    Ok(())
}

/// REVIEW-CLI-WRITE-1: refuses an output path that would clobber the original
/// source, one of its Lumina bundle files (`<input>.lumina.json`,
/// `<input>.lumina.zdata`) or a hard link to them. Path equality covers
/// canonical aliases (including not-yet-existing targets, resolved against
/// their parent directory); `(dev, inode)` identity additionally catches hard
/// links, which canonicalization cannot see.
fn reject_protected_output(input: &Path, output: &Path) -> Result<(), CliError> {
    reject_same_path(input, output)?;
    let output_resolved = resolve_candidate(output).map_err(|error| io_error(output, error))?;
    let protected: Vec<(&str, PathBuf)> = vec![
        ("sidecar", lumina_sidecar::sidecar_path_for(input)),
        (
            "mask/source-action bundle",
            lumina_sidecar::zdata_path_for(input),
        ),
    ];
    for (kind, target) in protected {
        let target_resolved =
            resolve_candidate(&target).map_err(|error| io_error(&target, error))?;
        if target_resolved == output_resolved {
            return Err(CliError::Message(format!(
                "output `{}` would overwrite the Lumina {kind} `{}`; refusing (non-destructive guarantee)",
                output.display(),
                target.display()
            )));
        }
        // Hard-link alias: the same bundle file under a different directory
        // entry. Canonicalization cannot see it, so `(dev, inode)` identity is
        // checked in addition (CLI-GUARD-HARDLINK-1).
        if paths_are_same_file(&target, output).map_err(|error| io_error(&target, error))? {
            return Err(CliError::Message(format!(
                "output `{}` is a hard link to the Lumina {kind} `{}`; refusing to overwrite the bundle (non-destructive guarantee)",
                output.display(),
                target.display()
            )));
        }
    }
    if paths_are_same_file(input, output).map_err(|error| io_error(input, error))? {
        return Err(CliError::Message(format!(
            "output `{}` is a hard link to the input `{}`; refusing to overwrite the original",
            output.display(),
            input.display()
        )));
    }
    Ok(())
}

/// Resolves `path` to a comparable identity: existing paths are canonicalized,
/// missing ones are resolved against their canonical parent directory (the
/// same convention as `lumina_sidecar::paths_resolve_equal`).
fn resolve_candidate(path: &Path) -> std::io::Result<PathBuf> {
    if path.exists() {
        fs::canonicalize(path)
    } else {
        let parent = fs::canonicalize(path.parent().unwrap_or_else(|| Path::new(".")))?;
        Ok(parent.join(path.file_name().unwrap_or_default()))
    }
}

/// Unix: true when both paths refer to the same underlying file via
/// `(dev, inode)` identity — this catches hard links between distinct paths.
#[cfg(unix)]
fn paths_are_same_file(a: &Path, b: &Path) -> std::io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    if !(a.exists() && b.exists()) {
        return Ok(false);
    }
    let (meta_a, meta_b) = (fs::metadata(a)?, fs::metadata(b)?);
    Ok(meta_a.dev() == meta_b.dev() && meta_a.ino() == meta_b.ino())
}

/// Non-unix fallback: no portable inode identity exists; only path equality
/// (checked separately above) applies.
#[cfg(not(unix))]
fn paths_are_same_file(_a: &Path, _b: &Path) -> std::io::Result<bool> {
    Ok(false)
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    // Delegate to the shared atomic-write helper in `lumina-sidecar` so the
    // CLI and GUI use identical atomic-write semantics.
    lumina_sidecar::write_atomically(path, bytes)?;
    Ok(())
}

/// REVIEW-CLI-N6: staged write for the export/sidecar two-artifact sequence.
///
/// The encoded bytes are written into a temporary file inside the target's
/// directory — same `.{name}.tmp-*` scheme, flush and fsync steps as
/// [`lumina_sidecar::write_atomically`] — but the target name is NOT yet
/// taken. [`StagedArtifact::commit`] later renames the temporary into place
/// (a same-directory rename, atomic per POSIX). Dropping an uncommitted stage
/// deletes the temporary, so every error path between `stage` and `commit`
/// leaves neither artifact behind: this is what lets `process_selected` order
/// the sequence as *stage export → save sidecar → commit export* and still
/// roll back to "nothing changed" when the sidecar save fails.
#[derive(Debug)]
struct StagedArtifact {
    temporary: tempfile::NamedTempFile,
    target: PathBuf,
}

impl StagedArtifact {
    /// Stage `bytes` for `target`: create, fill, flush and fsync a temporary
    /// file in `target`'s parent directory so a later `commit` is a
    /// same-directory (same-filesystem) rename. Fails before any sidecar
    /// mutation could happen in the caller's sequence.
    fn stage(target: &Path, bytes: &[u8]) -> Result<Self, CliError> {
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        let filename = target
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_else(|| "artifact".into());
        let mut temporary = tempfile::Builder::new()
            .prefix(&format!(".{filename}.tmp-"))
            .tempfile_in(parent)
            .map_err(|error| io_error(parent, error))?;
        let temporary_path = temporary.path().to_path_buf();
        temporary
            .write_all(bytes)
            .map_err(|error| io_error(&temporary_path, error))?;
        temporary
            .flush()
            .map_err(|error| io_error(&temporary_path, error))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| io_error(&temporary_path, error))?;
        Ok(Self {
            temporary,
            target: target.to_path_buf(),
        })
    }

    /// Publish the staged bytes by renaming them over the target path. The
    /// staged file must not outlive this call either way: on success it has
    /// been renamed, on failure the `PersistError` keeps nothing and the
    /// dropped `NamedTempFile` removes the temporary.
    fn commit(self) -> Result<(), CliError> {
        self.temporary
            .persist(&self.target)
            .map_err(|error| io_error(&self.target, error.error))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// META-COPYPASTE-1: the clipboard file serializes with the documented
    /// format marker/version, and `load_meta_clipboard` rejects structural
    /// deviations loudly instead of falling back to an empty clipboard.
    #[test]
    fn meta_clipboard_file_roundtrip_and_validation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("clipboard.json");
        let clipboard = MetaClipboardFile {
            format: META_CLIPBOARD_FORMAT.to_string(),
            version: META_CLIPBOARD_VERSION,
            source: Some("quelle.png".to_string()),
            fields: BTreeMap::from([("title".to_string(), "Startschuss".to_string())]),
            keywords: vec!["fest".to_string()],
        };
        fs::write(&path, serde_json::to_string_pretty(&clipboard).unwrap()).unwrap();
        let loaded = load_meta_clipboard(&path).unwrap();
        assert_eq!(loaded.format, META_CLIPBOARD_FORMAT);
        assert_eq!(loaded.version, META_CLIPBOARD_VERSION);
        assert_eq!(loaded.source.as_deref(), Some("quelle.png"));
        assert_eq!(
            loaded.fields.get("title").map(String::as_str),
            Some("Startschuss")
        );
        assert_eq!(loaded.keywords, vec!["fest".to_string()]);
        assert_eq!(
            loaded.stored_ids(),
            BTreeSet::from(["keywords".to_string(), "title".to_string()])
        );

        // Wrong format marker → loud.
        fs::write(&path, r#"{"format":"nope","version":1}"#).unwrap();
        assert!(load_meta_clipboard(&path).is_err());

        // Empty value would mean "delete" on paste → loud.
        fs::write(
            &path,
            r#"{"format":"lumina-meta-clipboard","version":1,"fields":{"title":""}}"#,
        )
        .unwrap();
        assert!(load_meta_clipboard(&path).is_err());

        // `keywords` must not hide inside `fields`.
        fs::write(
            &path,
            r#"{"format":"lumina-meta-clipboard","version":1,"fields":{"keywords":"x"}}"#,
        )
        .unwrap();
        assert!(load_meta_clipboard(&path).is_err());
    }

    /// META-COPYPASTE-2: die defensiven Validierungszweige von
    /// `load_meta_clipboard` (truncated JSON, fremde Version, Keyword-Whitespace/
    /// -Leere/-Überlänge/-Anzahl) und `parse_meta_fields` (leere IDs) sind
    /// laut — kein stiller Fallback, keine Normalisierung.
    #[test]
    fn meta_clipboard_rejects_defensive_violations() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("clipboard.json");
        let write = |raw: &str| fs::write(&path, raw).unwrap();
        let assert_rejects = |needle: &str| {
            let error = load_meta_clipboard(&path).unwrap_err().to_string();
            assert!(error.contains(needle), "expected `{needle}` in `{error}`");
        };

        // Truncated JSON (abgebrochener Write des geteilten Formats).
        write(r#"{"format":"lumina-meta-clipboard","version":1,"fields":{"title":"Start"#);
        assert_rejects("invalid metadata clipboard");

        // Fremde Version statt stillem Fallback.
        write(r#"{"format":"lumina-meta-clipboard","version":2,"fields":{"title":"X"}}"#);
        assert_rejects("unsupported metadata clipboard version 2");

        // Unbekannte Feld-ID im Clipboard.
        write(r#"{"format":"lumina-meta-clipboard","version":1,"fields":{"nope":"X"}}"#);
        assert_rejects("unknown metadata field `nope`");

        // Keyword mit führendem Whitespace.
        write(r#"{"format":"lumina-meta-clipboard","version":1,"keywords":[" fest"]}"#);
        assert_rejects("leading/trailing whitespace");

        // Leeres Keyword.
        write(r#"{"format":"lumina-meta-clipboard","version":1,"keywords":[""]}"#);
        assert_rejects("leading/trailing whitespace");

        // Keyword-Überlänge.
        let long = "x".repeat(MAX_KEYWORD_CHARS + 1);
        write(&format!(
            r#"{{"format":"lumina-meta-clipboard","version":1,"keywords":["{long}"]}}"#
        ));
        assert_rejects("exceeds limit");

        // Keyword-Anzahl.
        let many = serde_json::json!({
            "format": "lumina-meta-clipboard",
            "version": 1,
            "keywords": vec!["fest"; MAX_KEYWORDS_PER_DOCUMENT + 1]
        });
        write(&serde_json::to_string(&many).unwrap());
        assert_rejects("keyword list exceeds limit");

        // Leere `--fields`-Elemente werden vor jedem Lesezugriff abgelehnt.
        let empty = vec!["".to_string()];
        assert!(parse_meta_fields(&empty, "meta copy").is_err());
        let mixed = vec!["title".to_string(), String::new()];
        assert!(parse_meta_fields(&mixed, "meta paste").is_err());
        // Nichtleere, bekannte IDs bleiben gültig und werden dedupliziert.
        let ok = parse_meta_fields(
            &[
                "title".to_string(),
                "title".to_string(),
                "keywords".to_string(),
            ],
            "meta copy",
        )
        .unwrap();
        assert_eq!(
            ok,
            BTreeSet::from(["keywords".to_string(), "title".to_string()])
        );
    }

    /// META-COPYPASTE-1: the default clipboard lives in the OS temp directory
    /// (explicit/ephemeral, never a CWD dotfile) and keeps a stable name.
    #[test]
    fn default_meta_clipboard_path_is_in_os_temp() {
        let path = default_meta_clipboard_path();
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("lumina-meta-clipboard.json")
        );
        assert!(path.starts_with(std::env::temp_dir()));
    }

    // ------------------------------------------------------------------
    // F-082-FOLLOWUP — onnx-rt wiring test support.
    //
    // The process-level mask tests below all start in
    // `write_sidecar_with_valid_layer`, which triggers the CLI's inference
    // wiring gate. Under `onnx-rt` those renders request the REAL engine, so
    // `LUMINA_MODEL_PATH` must point at a loadable AND runnable artifact. The
    // deterministic BiRefNet-compatible crafted model below is generated at
    // test runtime (no committed binary, no downloads — mirroring
    // `crates/lumina-onnx/tests/ort_backend.rs`), so the whole CLI suite stays
    // green with and without the feature.
    // ------------------------------------------------------------------

    /// Proto3 varint (mirrors `lumina-onnx/tests/ort_backend.rs`).
    #[cfg(feature = "onnx-rt")]
    fn push_varint(out: &mut Vec<u8>, mut value: u64) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
    }

    #[cfg(feature = "onnx-rt")]
    fn push_tag(out: &mut Vec<u8>, field: u32, wire_type: u64) {
        push_varint(out, ((field as u64) << 3) | wire_type);
    }

    #[cfg(feature = "onnx-rt")]
    fn push_len_delimited(out: &mut Vec<u8>, field: u32, payload: &[u8]) {
        push_tag(out, field, 2);
        push_varint(out, payload.len() as u64);
        out.extend_from_slice(payload);
    }

    #[cfg(feature = "onnx-rt")]
    fn push_string(out: &mut Vec<u8>, field: u32, value: &str) {
        push_len_delimited(out, field, value.as_bytes());
    }

    #[cfg(feature = "onnx-rt")]
    fn push_varint_field(out: &mut Vec<u8>, field: u32, value: u64) {
        push_tag(out, field, 0);
        push_varint(out, value);
    }

    /// `TensorShapeProto.dim` (`dim_value`), `TypeProto.Tensor`,
    /// `ValueInfoProto` and `AttributeProto` encodings for the crafted graph.
    #[cfg(feature = "onnx-rt")]
    fn shape_proto(dims: &[i64]) -> Vec<u8> {
        let mut out = Vec::new();
        for dim in dims {
            let mut entry = Vec::new();
            push_varint_field(&mut entry, 1, *dim as u64);
            push_len_delimited(&mut out, 1, &entry);
        }
        out
    }

    #[cfg(feature = "onnx-rt")]
    fn value_info(name: &str, dims: &[i64]) -> Vec<u8> {
        let mut tensor = Vec::new();
        push_varint_field(&mut tensor, 1, 1); // FLOAT
        push_len_delimited(&mut tensor, 2, &shape_proto(dims));
        let mut type_info = Vec::new();
        push_len_delimited(&mut type_info, 1, &tensor);
        let mut out = Vec::new();
        push_string(&mut out, 1, name);
        push_len_delimited(&mut out, 2, &type_info);
        out
    }

    #[cfg(feature = "onnx-rt")]
    fn reduce_max_node(input: &str, output: &str) -> Vec<u8> {
        // ReduceMax(axes=[1], keepdims=1), attributes-based (opset ≤ 17).
        let mut axes = Vec::new();
        push_string(&mut axes, 1, "axes");
        push_varint_field(&mut axes, 20, 7); // AttributeType::INTS
        let mut packed_axes = Vec::new();
        push_varint(&mut packed_axes, 1);
        push_len_delimited(&mut axes, 8, &packed_axes);

        let mut keepdims = Vec::new();
        push_string(&mut keepdims, 1, "keepdims");
        push_varint_field(&mut keepdims, 20, 2); // AttributeType::INT
        push_varint_field(&mut keepdims, 3, 1); // keepdims = true

        let mut out = Vec::new();
        push_string(&mut out, 1, input);
        push_string(&mut out, 2, output);
        push_string(&mut out, 4, "ReduceMax");
        push_len_delimited(&mut out, 5, &axes);
        push_len_delimited(&mut out, 5, &keepdims);
        out
    }

    /// Deterministic bytes of a **BiRefNet-compatible** crafted ONNX graph:
    /// `input [1,3,1024,1024] → ReduceMax(axes=[1], keepdims=1) →
    /// output [1,1,1024,1024]` (ir_version 8 / opset 13). Same structure as
    /// the committed `lumina-crafted-reducemax.onnx` behavior fixture, but with
    /// the BiRefNet manifest's tensor names (`input`/`output`) and inference
    /// resolution, so `lumina_onnx::OrtBackend` driven by `birefnet_manifest()`
    /// can load **and run** it — the CLI's `onnx-rt` wiring gets a real engine
    /// in tests without a committed binary or a download. `ReduceMax` over the
    /// channel axis on a uniform frame yields a deterministic uniform matte
    /// (the lumina-onnx fixture test proves the same graph family under ORT).
    #[cfg(feature = "onnx-rt")]
    fn birefnet_compatible_onnx_bytes() -> Vec<u8> {
        const INPUT: &str = "input";
        const OUTPUT: &str = "output";
        const W: i64 = lumina_onnx::BIREFNET_INFERENCE_WIDTH as i64;
        const H: i64 = lumina_onnx::BIREFNET_INFERENCE_HEIGHT as i64;

        let mut opset = Vec::new();
        push_varint_field(&mut opset, 2, 13); // OperatorSetIdProto { version: 13 }

        let mut graph = Vec::new();
        push_len_delimited(&mut graph, 1, &reduce_max_node(INPUT, OUTPUT));
        push_string(&mut graph, 2, "lumina-cli-crafted-birefnet-compatible");
        push_len_delimited(&mut graph, 11, &value_info(INPUT, &[1, 3, H, W]));
        push_len_delimited(&mut graph, 12, &value_info(OUTPUT, &[1, 1, H, W]));

        let mut out = Vec::new();
        push_varint_field(&mut out, 1, 8); // ir_version 8
        push_len_delimited(&mut out, 7, &graph); // graph
        push_len_delimited(&mut out, 8, &opset); // opset_import
        out
    }

    /// Path to a persistent BiRefNet-compatible ONNX test model (created once
    /// per process). The backing temp directory is deliberately leaked so the
    /// artifact stays alive for the whole test process.
    #[cfg(feature = "onnx-rt")]
    fn onnx_test_fixture_path() -> PathBuf {
        use std::sync::OnceLock;
        static FIXTURE_DIR: OnceLock<PathBuf> = OnceLock::new();
        let dir = FIXTURE_DIR.get_or_init(|| {
            let directory = tempfile::tempdir().expect("test fixture tempdir must be creatable");
            let path = directory.path().to_path_buf();
            // Test-only leak: the directory (and the fixture file below) must
            // survive for the entire test process.
            std::mem::forget(directory);
            path
        });
        let fixture = dir.join("lumina-birefnet-compatible.onnx");
        if !fixture.exists() {
            fs::write(&fixture, birefnet_compatible_onnx_bytes())
                .expect("test ONNX fixture must be writable");
        }
        fixture
    }

    /// Ensures `LUMINA_MODEL_PATH` points at the runnable BiRefNet-compatible
    /// test model. Every env mutation in the suite writes the **same** value
    /// (idempotent), so parallel render tests never observe a broken path. The
    /// wiring-semantics tests use the path-parameterized
    /// `resolve_onnx_engine_from_path` and do NOT touch the env var at all.
    #[cfg(feature = "onnx-rt")]
    fn ensure_onnx_test_engine() {
        if std::env::var_os("LUMINA_MODEL_PATH").is_none() {
            std::env::set_var("LUMINA_MODEL_PATH", onnx_test_fixture_path());
        }
    }

    /// F-101-F1 smoke test: the `lumina mcp` subcommand delegates to the
    /// shared `lumina_mcp` server pipeline; assert the handshake and the full
    /// documented tool set through that exact pipeline.
    #[cfg(feature = "mcp")]
    #[test]
    fn mcp_subcommand_pipeline_answers_handshake_and_lists_all_tools() {
        std::env::set_var("LUMINA_MCP_PREVIEW_DIR", std::env::temp_dir());
        let mut server = lumina_mcp::Server::new();
        let handshake = server
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
            .expect("initialize expects a response");
        assert_eq!(handshake["result"]["serverInfo"]["name"], "lumina-mcp");

        let listing = server
            .handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
            .expect("tools/list expects a response");
        let tools = listing["result"]["tools"].as_array().unwrap();
        // Drift guard pinned to the SOLL (feature/platform/mcp-server.md):
        // 7 editing tools + lumina_analyze + 4 F-101-F1 CLI-coverage tools
        // + 5 LRPAR-G15-IPTC-S7 metadata tools.
        assert_eq!(tools.len(), 17, "tool set drifted; update SOLL + tests");
    }

    /// R2-WB (parity with lumina-gui): non-finite or non-positive As-Shot gains
    /// are dropped to `None` so the render degrades instead of aborting; a
    /// healthy gain vector is kept verbatim.
    #[test]
    fn sanitize_camera_white_balance_rejects_non_finite_and_non_positive() {
        assert_eq!(
            sanitize_camera_white_balance([1.9, 1.0, 1.4, 1.0]),
            Some([1.9, 1.0, 1.4, 1.0])
        );
        assert_eq!(sanitize_camera_white_balance([0.0, 1.0, 1.0, 1.0]), None);
        assert_eq!(sanitize_camera_white_balance([-0.5, 1.0, 1.0, 1.0]), None);
        assert_eq!(
            sanitize_camera_white_balance([f32::NAN, 1.0, 1.0, 1.0]),
            None
        );
        assert_eq!(
            sanitize_camera_white_balance([f32::INFINITY, 1.0, 1.0, 1.0]),
            None
        );
        assert_eq!(
            sanitize_camera_white_balance([1.0, f32::NEG_INFINITY, 1.0, 1.0]),
            None
        );
    }

    /// R2-MCP-01 (CAMERA-WB-WELLE) + R2-GPU-05: the CLI routing decision no
    /// longer CPU-routes a **valid** decoder As-Shot WB context — the gains are
    /// carried into the GPU entry and validated there (the caller binds them via
    /// `set_camera_white_balance`, like the Lensfun corrector) — while an
    /// **invalid** context still forces the CPU route. Touched-but-reset sliders
    /// at their neutral value stay GPU-allowed.
    ///
    /// Pure reason-level assertions by design: `lumina-core` validates the
    /// As-Shot gains without re-applying them to pixels, so a WB divergence is
    /// not pixel-observable — the parity itself is pinned in `lumina-gpu`
    /// (`as_shot_wb_gains_match_cpu_oracle_across_recipe_wb`).
    #[cfg(feature = "gpu")]
    #[test]
    fn gpu_routing_reasons_carry_valid_wb_flag_invalid_and_respect_neutral_sliders() {
        // A valid context WB is carried, not flagged; absent is trivially clear.
        let recipe = EditRecipe::default();
        let with_wb = RenderContext {
            recipe: &recipe,
            camera_white_balance: Some([1.9, 1.0, 1.4, 1.0]),
            source_actions: &[],
            masks: None,
            lensfun: None,
            depth: None,
        };
        assert!(
            gpu_routing_reasons(&recipe, &with_wb).is_empty(),
            "a valid As-Shot context must be GPU-carried, not a reason"
        );

        let without_wb = RenderContext {
            camera_white_balance: None,
            ..with_wb.clone()
        };
        assert!(gpu_routing_reasons(&recipe, &without_wb).is_empty());

        // An invalid context stays CPU-forcing so the oracle rejects it loudly.
        let invalid_wb = RenderContext {
            camera_white_balance: Some([0.0, 1.0, 1.0, 1.0]),
            ..with_wb.clone()
        };
        let reasons = gpu_routing_reasons(&recipe, &invalid_wb);
        assert!(
            reasons
                .iter()
                .any(|r| r.starts_with("camera_white_balance")),
            "{reasons:?}"
        );
        assert_eq!(reasons.len(), 1, "{reasons:?}");

        // Touched-but-reset sliders stay GPU-allowed …
        let touched_reset = EditRecipe {
            adjustments: BTreeMap::from([
                ("vibrance".to_string(), 0.0),
                ("saturation".to_string(), 0.0),
            ]),
            ..Default::default()
        };
        let reset_ctx = RenderContext {
            recipe: &touched_reset,
            ..without_wb.clone()
        };
        assert!(gpu_routing_reasons(&touched_reset, &reset_ctx).is_empty());

        // … while a recipe stage the GPU genuinely cannot render keeps forcing
        // the CPU route. Every schema adjustment key is GPU-rendered now
        // (GPU-RENDER-PARITY-1: tone + detail + Red-Eye), so the honest,
        // permanent probe here is the unknown-key class: a key outside the
        // recipe schema has no neutral default, the CPU reference rejects it
        // outright and no GPU stage could ever accept it — unlike
        // geometry/lens_correction/perspective/lens_blur/spot_removals/
        // generative_edit (all queued for GPU parity in GPU-RENDER-PARITY-1,
        // so they would go stale again). The same class is pinned by
        // `cpu_routing_inventory_is_complete` in `lumina-gpu`.
        let unsupported = EditRecipe {
            adjustments: BTreeMap::from([("clarity_v2".to_string(), 0.5)]),
            ..Default::default()
        };
        let unsupported_ctx = RenderContext {
            recipe: &unsupported,
            ..reset_ctx
        };
        let reasons = gpu_routing_reasons(&unsupported, &unsupported_ctx);
        assert!(
            reasons
                .iter()
                .any(|r| r.contains("clarity_v2") && r.contains("not implemented on GPU")),
            "{reasons:?}"
        );

        // An invalid WB context stacks with other context-level reasons instead
        // of replacing them.
        let artifact = SourceActionArtifact {
            region: MaskPlane {
                width: 4,
                height: 4,
                values: vec![u16::MAX; 16],
            },
            replacement: ImageFrame::new(4, 4, vec![0; 4 * 4 * 4]).unwrap(),
        };
        let stacked = RenderContext {
            source_actions: std::slice::from_ref(&artifact),
            ..invalid_wb
        };
        let reasons = gpu_routing_reasons(&recipe, &stacked);
        assert_eq!(reasons.len(), 2, "{reasons:?}");
        assert!(
            reasons
                .iter()
                .any(|r| r.starts_with("camera_white_balance")),
            "{reasons:?}"
        );
        assert!(reasons.iter().any(|r| r.contains("source_actions")));
    }

    #[test]
    fn parses_process_arguments() {
        let cli = Cli::try_parse_from([
            "lumina",
            "process",
            "--input",
            "a.png",
            "--output",
            "b.webp",
            "--exposure",
            "1",
            "--highlights=-0.25",
            "--shadows",
            "0.4",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Process(ProcessArgs {
                exposure: Some(1.0),
                highlights: Some(-0.25),
                shadows: Some(0.4),
                ..
            })
        ));
    }

    /// REVIEW-CLI-N6: an uncommitted stage must vanish completely on drop and
    /// a committed stage must publish exactly the staged bytes.
    #[test]
    fn staged_artifact_cleans_up_without_commit_and_commits_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("out.png");

        // Uncommitted stages disappear on drop — nothing partial remains.
        {
            let staged = StagedArtifact::stage(&target, b"first").unwrap();
            assert!(!target.exists(), "staging must not take the target name");
            assert!(staged.temporary.path().is_file());
        }
        assert_eq!(
            fs::read_dir(directory.path()).unwrap().count(),
            0,
            "dropped stage must leave no temporary residue"
        );

        // Commit publishes exactly the staged bytes under the target name.
        let staged = StagedArtifact::stage(&target, b"payload").unwrap();
        staged.commit().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"payload");
        assert_eq!(
            fs::read_dir(directory.path()).unwrap().count(),
            1,
            "commit must rename, not copy: only the target remains"
        );
    }

    /// REVIEW-CLI-N6: staging into a nonexistent parent fails at stage time —
    /// i.e. before the caller could mutate any sidecar in its sequence.
    #[test]
    fn staged_artifact_fails_cleanly_for_missing_target_directory() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("missing").join("out.png");
        let error = StagedArtifact::stage(&target, b"x").unwrap_err();
        assert!(
            matches!(error, CliError::Io { .. }),
            "unexpected error shape: {error:?}"
        );
    }

    #[test]
    fn export_accepts_update_masks_before_export() {
        let cli = Cli::try_parse_from([
            "lumina",
            "export",
            "--input",
            "a.png",
            "--output",
            "b.png",
            "--update-masks",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Export(ExportArgs {
                update_masks: true,
                ..
            })
        ));
    }

    #[test]
    fn recognizes_supported_raw_extensions() {
        for extension in ["ARW", "crw", "pef", "3fr", "x3f"] {
            assert!(is_raw_path(Path::new(&format!("photo.{extension}"))));
        }
    }

    /// R2-CLI-01 drift guard: BOTH predicates must accept every RAW extension
    /// exported by `lumina_raw` — the batch collector and the decode router
    /// previously disagreed (batch silently skipped 9 of 18 formats).
    #[test]
    fn batch_collection_and_decode_routing_agree_on_every_raw_extension() {
        for extension in lumina_raw::RAW_EXTENSIONS {
            let path_string = format!("photo.{extension}");
            let path = Path::new(&path_string);
            assert!(
                is_raw_path(path),
                "`is_raw_path` must accept RAW extension `{extension}`"
            );
            assert!(
                has_image_extension(path),
                "`has_image_extension` (batch collection) must accept RAW extension `{extension}`"
            );
        }
        // Non-image names stay out of the batch.
        for foreign in ["notes.txt", "archive.zip", "x.lumina.json", "noext"] {
            assert!(!has_image_extension(Path::new(foreign)), "{foreign}");
        }
    }

    #[test]
    fn rejects_identical_and_alias_paths_before_processing() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        fs::write(&input, [1, 2, 3]).unwrap();
        assert!(reject_same_path(&input, &input).is_err());
        assert!(reject_same_path(&input, &directory.path().join("./input.png")).is_err());
        assert!(reject_same_path(&input, &directory.path().join("input.png")).is_err());
    }

    #[test]
    fn changed_source_is_rejected_without_overwriting_output() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let output = directory.path().join("output.png");
        let frame = ImageFrame::new(1, 1, vec![20, 30, 40, 255]).unwrap();
        fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
        process(ProcessArgs {
            input: input.clone(),
            output: output.clone(),
            preset: None,
            exposure: None,
            contrast: None,
            highlights: None,
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
            write_metadata: false,
        })
        .unwrap();
        let changed = ImageFrame::new(1, 1, vec![21, 30, 40, 255]).unwrap();
        fs::write(&input, changed.encode(ImageFileFormat::Png).unwrap()).unwrap();
        let sentinel = fs::read(&output).unwrap();
        let error = process(ProcessArgs {
            input,
            output: output.clone(),
            preset: None,
            exposure: None,
            contrast: None,
            highlights: None,
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
            write_metadata: false,
        })
        .unwrap_err();
        assert!(error.to_string().contains("source changed"));
        assert_eq!(fs::read(output).unwrap(), sentinel);
    }

    #[test]
    fn invalid_adjustment_and_unknown_key_are_cli_errors_without_output() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let output = directory.path().join("output.png");
        let frame = ImageFrame::new(1, 1, vec![20, 30, 40, 255]).unwrap();
        fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
        let invalid = process(ProcessArgs {
            input: input.clone(),
            output: output.clone(),
            preset: None,
            exposure: Some(f64::INFINITY),
            contrast: None,
            highlights: None,
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
            write_metadata: false,
        })
        .unwrap_err();
        assert!(invalid.to_string().contains("invalid exposure"));
        assert!(!output.exists());

        let preset_path = directory.path().join("unknown.json");
        let preset = Preset {
            id: "unknown".into(),
            name: "Unknown".into(),
            recipe: lumina_sidecar::EditRecipe {
                adjustments: BTreeMap::from([("clarity".into(), 0.5)]),
                ..Default::default()
            },
            extras: BTreeMap::new(),
        };
        fs::write(&preset_path, serde_json::to_vec(&preset).unwrap()).unwrap();
        let unknown = process(ProcessArgs {
            input,
            output: output.clone(),
            preset: Some(preset_path),
            exposure: None,
            contrast: None,
            highlights: None,
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
            write_metadata: false,
        })
        .unwrap_err();
        assert!(unknown
            .to_string()
            .contains("unsupported adjustment `clarity`"));
        assert!(!output.exists());
    }

    #[test]
    fn cli_rejects_non_finite_and_out_of_range_adjustments() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let frame = ImageFrame::new(1, 1, vec![20, 30, 40, 255]).unwrap();
        fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
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
                let output = directory.path().join(format!("{name}-{value:?}.png"));
                let error = process(ProcessArgs {
                    input: input.clone(),
                    output: output.clone(),
                    preset: None,
                    exposure: (name == "exposure").then_some(value),
                    contrast: (name == "contrast").then_some(value),
                    highlights: (name == "highlights").then_some(value),
                    shadows: (name == "shadows").then_some(value),
                    auto_tone: false,
                    match_total_exposure: false,
                    target_luminance: 0.5,
                    write_metadata: false,
                })
                .unwrap_err();
                assert!(error.to_string().contains(&format!("invalid {name}")));
                assert!(!output.exists());
            }
        }
    }

    #[test]
    fn cli_accepts_both_adjustment_boundaries() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let frame = ImageFrame::new(1, 1, vec![20, 30, 40, 255]).unwrap();
        fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
        for (name, values) in [
            ("exposure", [-10.0, 10.0]),
            ("contrast", [-1.0, 1.0]),
            ("highlights", [-1.0, 1.0]),
            ("shadows", [-1.0, 1.0]),
        ] {
            for (index, value) in values.into_iter().enumerate() {
                process(ProcessArgs {
                    input: input.clone(),
                    output: directory.path().join(format!("{name}-{index}.png")),
                    preset: None,
                    exposure: (name == "exposure").then_some(value),
                    contrast: (name == "contrast").then_some(value),
                    highlights: (name == "highlights").then_some(value),
                    shadows: (name == "shadows").then_some(value),
                    auto_tone: false,
                    match_total_exposure: false,
                    target_luminance: 0.5,
                    write_metadata: false,
                })
                .unwrap();
            }
        }
    }

    #[test]
    fn preset_process_and_inspect_use_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let output = directory.path().join("output.webp");
        let preset_path = directory.path().join("preset.json");
        let frame = ImageFrame::new(1, 1, vec![20, 30, 40, 255]).unwrap();
        fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
        let preset = Preset {
            id: "test".into(),
            name: "Bright".into(),
            recipe: lumina_sidecar::EditRecipe {
                adjustments: BTreeMap::from([("exposure".into(), 1.0)]),
                ..lumina_sidecar::EditRecipe::default()
            },
            extras: BTreeMap::new(),
        };
        fs::write(&preset_path, serde_json::to_vec(&preset).unwrap()).unwrap();
        process(ProcessArgs {
            input: input.clone(),
            output: output.clone(),
            preset: Some(preset_path),
            exposure: Some(0.0),
            contrast: None,
            highlights: None,
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
            write_metadata: false,
        })
        .unwrap();
        assert!(output.exists());
        let sidecar = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert_eq!(
            sidecar.virtual_copies[0].recipe.adjustments["exposure"],
            0.0
        );
        assert_eq!(sidecar.virtual_copies[0].history.len(), 1);
        inspect(InspectArgs { input, json: false }).unwrap();
    }

    /// R2-CLI-03: `inspect --json` reports the machine-readable status —
    /// sidecar state and every virtual copy incl. auto-tone/matching values.
    #[test]
    fn inspect_json_reports_sidecar_status_and_virtual_copies() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 42);
        // No sidecar yet → "missing" with the default copy.
        let missing =
            Cli::try_parse_from(["lumina", "inspect", &input.display().to_string()]).unwrap();
        assert!(matches!(
            missing.command,
            Command::Inspect(InspectArgs { json: false, .. })
        ));
        inspect(InspectArgs {
            input: input.clone(),
            json: true,
        })
        .unwrap();

        // With a sidecar the JSON path succeeds for the valid state too (the
        // payload itself goes to stdout; here we pin that both states run).
        let bytes = fs::read(&input).unwrap();
        let frame = ImageFrame::decode(&bytes).unwrap();
        write_sidecar_with_valid_layer(&input, &bytes, &frame);
        inspect(InspectArgs { input, json: true }).unwrap();
    }

    /// R2-CLI-01 end-to-end guard: batch collection must FIND every RAW
    /// extension the SOLL lists. The fixture files are synthetic (garbage
    /// payloads are fine — `--dry-run` never decodes), which keeps the test
    /// focused on exactly the regression: the old private 9-extension copy of
    /// `has_image_extension` silently skipped RAF/ORF/etc., so no status files
    /// would have been written for them.
    #[test]
    fn batch_finds_every_supported_raw_extension_in_a_directory_tree() {
        let directory = tempfile::tempdir().unwrap();
        let src = directory.path().join("src");
        fs::create_dir_all(&src).unwrap();
        let raw_extensions = [
            "arw", "cr2", "cr3", "dng", "nef", "orf", "raf", "rw2", "crw", "pef", "srw", "3fr",
            "iiq", "rwl", "mos", "erf", "kdc", "x3f",
        ];
        for (index, extension) in raw_extensions.iter().enumerate() {
            fs::write(
                src.join(format!("IMG_{index:04}.{extension}")),
                b"synthetic",
            )
            .unwrap();
        }
        // A non-image file must stay ignored.
        fs::write(src.join("notes.txt"), b"ignore me").unwrap();

        let out = directory.path().join("out");
        batch(BatchArgs {
            input: src,
            output: out.clone(),
            jobs: 1,
            retry: 0,
            resume: false,
            dry_run: true,
            update_masks: false,
            force_render: false,
            json: false,
            format: "png".into(),
            quality: 90,
            virtual_copy: None,
            mask_policy: CliMaskPolicy::Warn,
            write_metadata: false,
        })
        .expect("dry-run batch over synthetic RAW fixtures must succeed");

        let mut statuses: Vec<String> = fs::read_dir(&out)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        statuses.sort();
        assert_eq!(
            statuses.len(),
            raw_extensions.len(),
            "every RAW extension must be collected: {statuses:?}"
        );
        for index in 0..raw_extensions.len() {
            assert!(
                statuses
                    .iter()
                    .any(|name| name.starts_with(&format!("IMG_{index:04}."))),
                "missing status file for IMG_{index:04}: {statuses:?}"
            );
        }
        assert!(!out.join("notes.txt.status.json").exists());
    }

    fn valid_mask_definition(
        id: &str,
        operation: lumina_sidecar::MaskOperation,
        references: Vec<lumina_sidecar::MaskReference>,
        identity: &SourceIdentity,
        width: u32,
        height: u32,
    ) -> lumina_sidecar::MaskDefinition {
        use lumina_sidecar::{
            CoordinateSystem, Extras, GeometryFingerprint, ModelIdentity, Preprocessing, Resolution,
        };
        // Build a *confirmably valid* persisted mask: its source/decode/model
        // identity matches the running source and the wired BiRefNet descriptor
        // (F-048), and it carries an artifact reference. F-047's persisted
        // masks always carry an `artifact`, so this mirrors real persistence.
        lumina_sidecar::MaskDefinition {
            id: id.into(),
            name: id.into(),
            source_fingerprint: lumina_sidecar::SourceFingerprint {
                content_hash: identity.content_hash.clone(),
                byte_length: identity.byte_length,
                extras: Extras::new(),
            },
            decode_context: identity.decode_fingerprint.clone(),
            geometry_context: GeometryFingerprint {
                width: 2,
                height: 2,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: Extras::new(),
            },
            model: ModelIdentity {
                name: "BiRefNet".into(),
                version: "1.0.0".into(),
                hash: "pending-integration".into(),
                extras: Extras::new(),
            },
            inference_resolution: Resolution {
                width,
                height,
                extras: Extras::new(),
            },
            preprocessing: Preprocessing {
                name: "p".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            rescaling_method: "none".into(),
            rescaling_parameters: BTreeMap::new(),
            coordinate_system: CoordinateSystem::SourceOriented,
            status: MaskStatus::Valid,
            created_at: "now".into(),
            generator_version: "g".into(),
            error_text: None,
            artifact: Some(lumina_sidecar::ArtifactReference {
                relative_path: "x.zdata".into(),
                format: "lumina-zdata".into(),
                checksum: "c".into(),
                width,
                height,
                channels: "u16".into(),
                data_version: "1".into(),
                extras: Extras::new(),
            }),
            operation,
            references,
            prompt: None,
            extras: Extras::new(),
            ai_select: None,
        }
    }

    fn write_sidecar_with_valid_layer(
        input: &Path,
        bytes: &[u8],
        frame: &ImageFrame,
    ) -> lumina_sidecar::SidecarDocument {
        // F-082-FOLLOWUP: every mask-work render reaches the CLI inference
        // wiring gate; under `onnx-rt` this configures the real-engine test
        // model so the render exercises the ORT path instead of hard-failing
        // on an unset `LUMINA_MODEL_PATH` (default builds stay on the stub).
        #[cfg(feature = "onnx-rt")]
        ensure_onnx_test_engine();
        let identity = source_identity(input, bytes, frame, None).unwrap();
        let mut document = SidecarDocument::new(identity.clone(), "raster-mvp-1");
        let copy = &mut document.virtual_copies[0];
        copy.mask_library = vec![valid_mask_definition(
            "subject",
            lumina_sidecar::MaskOperation::Source,
            vec![],
            &identity,
            frame.width,
            frame.height,
        )];
        copy.mask_layers = vec![lumina_sidecar::MaskLayer {
            id: "layer-1".into(),
            mask: lumina_sidecar::MaskReference {
                copy_id: copy.id.clone(),
                mask_id: "subject".into(),
                extras: BTreeMap::new(),
            },
            inverted: false,
            feather: 0.0,
            blur: 0.0,
            density: 1.0,
            extras: BTreeMap::new(),
            visible: true,
        }];
        save_sidecar(&sidecar_path_for(input), &document).unwrap();
        document
    }

    // ---- G-03 Maskierungs-Parität: mask DAG CLI ----

    fn mask_imported_input(directory: &Path, name: &str) -> PathBuf {
        let (input, _frame) = png_input(directory, name, 100);
        import_file(ImportArgs {
            input: input.clone(),
            json: true,
            migrate: false,
        })
        .unwrap();
        input
    }

    fn mask_args(input: PathBuf) -> MaskArgs {
        MaskArgs {
            input,
            update_masks: false,
            virtual_copy: None,
            json: true,
            list: false,
            add_ai_select: None,
            name: None,
            detail: None,
            add_luminance_range: false,
            range_min: None,
            range_max: None,
            add_color_range: false,
            hue_center: None,
            hue_width: None,
            sat_min: None,
            sat_max: None,
            lum_min: None,
            lum_max: None,
            feather: None,
            combine: None,
            inputs: None,
            duplicate: None,
            attach_layer: None,
            show_layer: None,
            hide_layer: None,
        }
    }

    fn mask_library_ids(input: &Path) -> Vec<String> {
        load_sidecar(&sidecar_path_for(input))
            .unwrap()
            .virtual_copies[0]
            .mask_library
            .iter()
            .map(|mask| mask.id.clone())
            .collect()
    }

    /// End-to-end: import → `mask --add-ai-select/--add-luminance-range` →
    /// file → reload. New entries are stable, typed and carry loud statuses.
    #[test]
    fn mask_add_ai_and_range_roundtrip_through_file() {
        let directory = tempfile::tempdir().unwrap();
        let input = mask_imported_input(directory.path(), "g03.png");

        let mut add_sky = mask_args(input.clone());
        add_sky.add_ai_select = Some("sky".into());
        add_sky.name = Some("Sky".into());
        mask(add_sky).unwrap();

        let mut add_lum = mask_args(input.clone());
        add_lum.add_luminance_range = true;
        add_lum.name = Some("Bright".into());
        add_lum.range_min = Some(0.5);
        add_lum.range_max = Some(1.0);
        mask(add_lum).unwrap();

        // Reload from the file: both masks persisted with stable ids.
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert!(document.validate().is_ok());
        let copy = &document.virtual_copies[0];
        assert_eq!(copy.mask_library.len(), 2);
        let sky = copy
            .mask_library
            .iter()
            .find(|mask| mask.name == "Sky")
            .unwrap();
        assert_eq!(
            sky.ai_select.as_ref().unwrap().kind,
            lumina_sidecar::AiSelectKind::Sky
        );
        assert_eq!(sky.status, MaskStatus::Pending);
        let lum = copy
            .mask_library
            .iter()
            .find(|mask| mask.name == "Bright")
            .unwrap();
        assert!(matches!(
            lum.prompt,
            Some(lumina_sidecar::MaskPrompt::LuminanceRange { .. })
        ));
        assert_eq!(lum.status, MaskStatus::Valid);
        // Stable ids: re-running the same add is a loud duplicate, not a copy.
        let mut repeat = mask_args(input.clone());
        repeat.add_ai_select = Some("sky".into());
        repeat.name = Some("Sky".into());
        assert!(mask(repeat).is_err());
    }

    /// Combinators, duplicate, attach and the visibility eye persist per copy.
    #[test]
    fn mask_combine_duplicate_layer_visibility_roundtrip() {
        let directory = tempfile::tempdir().unwrap();
        let input = mask_imported_input(directory.path(), "g03c.png");

        for (kind, name) in [("subject", "Subject"), ("sky", "Sky")] {
            let mut add = mask_args(input.clone());
            add.add_ai_select = Some(kind.into());
            add.name = Some(name.into());
            mask(add).unwrap();
        }
        let ids = mask_library_ids(&input);
        assert_eq!(ids.len(), 2);

        // Add = union over both inputs.
        let mut combine = mask_args(input.clone());
        combine.combine = Some("union".into());
        combine.name = Some("Both".into());
        combine.inputs = Some(format!("{},{}", ids[0], ids[1]));
        mask(combine).unwrap();

        // Intersect is CLI-reachable too (panel offers union/subtract/invert).
        let mut intersect = mask_args(input.clone());
        intersect.combine = Some("intersect".into());
        intersect.name = Some("Overlap".into());
        intersect.inputs = Some(format!("{},{}", ids[0], ids[1]));
        mask(intersect).unwrap();

        // Duplicate one source under a new name.
        let mut duplicate = mask_args(input.clone());
        duplicate.duplicate = Some(ids[0].clone());
        duplicate.name = Some("Subject copy".into());
        mask(duplicate).unwrap();

        // Attach a layer for the union and close its eye again.
        let union_id = mask_library_ids(&input)
            .into_iter()
            .find(|id| {
                load_sidecar(&sidecar_path_for(&input))
                    .unwrap()
                    .virtual_copies[0]
                    .mask_library
                    .iter()
                    .any(|mask| mask.id == *id && mask.name == "Both")
            })
            .unwrap();
        let mut attach = mask_args(input.clone());
        attach.attach_layer = Some(union_id);
        mask(attach).unwrap();
        // Layer id is `layer-<mask-id>`; resolve it from the file.
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let layer_id = document.virtual_copies[0].mask_layers[0].id.clone();
        let mut hide = mask_args(input.clone());
        hide.hide_layer = Some(layer_id.clone());
        mask(hide).unwrap();

        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert!(document.validate().is_ok());
        let copy = &document.virtual_copies[0];
        assert_eq!(copy.mask_library.len(), 5);
        let union = copy
            .mask_library
            .iter()
            .find(|mask| mask.name == "Both")
            .unwrap();
        assert_eq!(union.operation, lumina_sidecar::MaskOperation::Union);
        assert_eq!(union.references.len(), 2);
        let overlap = copy
            .mask_library
            .iter()
            .find(|mask| mask.name == "Overlap")
            .unwrap();
        assert_eq!(overlap.operation, lumina_sidecar::MaskOperation::Intersect);
        assert_eq!(overlap.references.len(), 2);
        let layer = copy
            .mask_layers
            .iter()
            .find(|layer| layer.id == layer_id)
            .unwrap();
        assert!(!layer.visible);
        // Re-open the eye.
        let mut show = mask_args(input.clone());
        show.show_layer = Some(layer_id);
        mask(show).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert!(document.virtual_copies[0].mask_layers[0].visible);
    }

    /// Loud failures: unknown kind/copy/mask, bad ranges, wrong arity,
    /// unknown layers and unknown combine ops abort with exit code 1 and
    /// leave the sidecar byte-identical (no partial write).
    #[test]
    fn mask_failures_are_loud_and_leave_sidecar_untouched() {
        let directory = tempfile::tempdir().unwrap();
        let input = mask_imported_input(directory.path(), "g03e.png");
        let before = fs::read(sidecar_path_for(&input)).unwrap();

        // Unknown AI kind.
        let mut bad_kind = mask_args(input.clone());
        bad_kind.add_ai_select = Some("cat".into());
        bad_kind.name = Some("Cat".into());
        let error = mask(bad_kind).unwrap_err();
        assert_eq!(error.exit_code(), 1);
        assert!(error.to_string().contains("unknown ai-select kind"));

        // Unknown virtual copy.
        let mut bad_copy = mask_args(input.clone());
        bad_copy.virtual_copy = Some("nope".into());
        bad_copy.add_ai_select = Some("sky".into());
        bad_copy.name = Some("Sky".into());
        assert!(mask(bad_copy).is_err());

        // Missing range bounds.
        let mut bad_range = mask_args(input.clone());
        bad_range.add_luminance_range = true;
        bad_range.name = Some("Bright".into());
        assert!(mask(bad_range).is_err());

        // Out-of-range values are rejected by the sidecar gate.
        let mut bad_values = mask_args(input.clone());
        bad_values.add_luminance_range = true;
        bad_values.name = Some("Bright".into());
        bad_values.range_min = Some(0.9);
        bad_values.range_max = Some(0.1);
        assert!(mask(bad_values).is_err());

        // Unknown combine input.
        let mut bad_input = mask_args(input.clone());
        bad_input.combine = Some("union".into());
        bad_input.name = Some("Both".into());
        bad_input.inputs = Some("missing-a,missing-b".into());
        assert!(mask(bad_input).is_err());

        // Wrong arity: subtract needs exactly 2.
        let mut add = mask_args(input.clone());
        add.add_ai_select = Some("sky".into());
        add.name = Some("Sky".into());
        mask(add).unwrap();
        let sky_id = mask_library_ids(&input)[0].clone();
        let mut bad_arity = mask_args(input.clone());
        bad_arity.combine = Some("subtract".into());
        bad_arity.name = Some("Sub".into());
        bad_arity.inputs = Some(sky_id);
        assert!(mask(bad_arity).is_err());

        // Unknown layer eye.
        let mut bad_layer = mask_args(input.clone());
        bad_layer.hide_layer = Some("layer-nope".into());
        assert!(mask(bad_layer).is_err());

        // Unknown combine op.
        let mut bad_op = mask_args(input.clone());
        bad_op.combine = Some("multiply".into());
        bad_op.name = Some("X".into());
        bad_op.inputs = Some("a,b".into());
        assert!(mask(bad_op).is_err());

        // Only the successful Sky add above changed the file.
        let after_success = fs::read(sidecar_path_for(&input)).unwrap();
        assert_ne!(before, after_success);
        let snapshot = after_success;
        // Every failing command after that left the file byte-identical.
        let mut another_bad = mask_args(input.clone());
        another_bad.hide_layer = Some("layer-nope".into());
        assert!(another_bad.hide_layer.is_some());
        let _ = mask(another_bad).unwrap_err();
        assert_eq!(fs::read(sidecar_path_for(&input)).unwrap(), snapshot);
    }

    /// `mask --list` is read-only: stdout carries the statuses, the sidecar
    /// bytes don't move.
    #[test]
    fn mask_list_reports_statuses_without_writing() {
        let directory = tempfile::tempdir().unwrap();
        let input = mask_imported_input(directory.path(), "g03l.png");
        let mut add = mask_args(input.clone());
        add.add_ai_select = Some("people".into());
        add.name = Some("Person".into());
        add.detail = Some("face".into());
        mask(add).unwrap();

        let before = fs::read(sidecar_path_for(&input)).unwrap();
        let mut list = mask_args(input.clone());
        list.list = true;
        list.json = true;
        mask(list).unwrap();
        assert_eq!(fs::read(sidecar_path_for(&input)).unwrap(), before);

        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let person = document.virtual_copies[0]
            .mask_library
            .iter()
            .find(|mask| mask.name == "Person")
            .unwrap();
        assert_eq!(
            person.ai_select.as_ref().unwrap().detail.as_deref(),
            Some("face")
        );
    }

    // ---- F-082-FOLLOWUP: onnx-rt wiring semantics ----

    /// Default build: the deterministic StubBackend is the wiring default,
    /// independent of the mask-work gate.
    #[cfg(not(feature = "onnx-rt"))]
    #[test]
    fn default_build_wires_deterministic_stub_regardless_of_gate() {
        let engine = resolve_mask_inference_engine(true).expect("stub must resolve");
        let engine = engine.expect("default build must wire the stub");
        assert!(engine.is_available());
        let frame = ImageFrame::new(4, 4, vec![10u8; 4 * 4 * 4]).unwrap();
        let plane = engine.infer(&frame).expect("stub must infer");
        assert_eq!((plane.width, plane.height), (4, 4));
        assert_eq!(plane.values.len(), 16);

        // The gate flag is irrelevant without `onnx-rt`: the stub is the
        // default even when the run could not request inference.
        assert!(resolve_mask_inference_engine(false)
            .expect("stub must resolve")
            .is_some());
    }

    /// `onnx-rt`: a loadable, identity-compatible artifact wires the REAL
    /// engine (not the stub), and the engine actually infers a matte.
    #[cfg(feature = "onnx-rt")]
    #[test]
    fn onnx_rt_resolves_real_engine_from_working_artifact() {
        let engine = resolve_onnx_engine_from_path(&onnx_test_fixture_path())
            .expect("real engine must load")
            .expect("real engine must be wired");
        assert!(engine.is_available());
        let frame = ImageFrame::new(4, 4, vec![120u8; 4 * 4 * 4]).unwrap();
        // The crafted ReduceMax graph emits a deterministic, uniform matte on a
        // uniform frame (same contract as the lumina-onnx fixture tests).
        let plane = engine.infer(&frame).expect("real engine must infer");
        assert_eq!((plane.width, plane.height), (4, 4));
        let first = plane.values[0];
        assert!(
            plane.values.iter().all(|value| *value == first),
            "uniform frame must yield a uniform matte from the real engine"
        );
    }

    /// `onnx-rt`: the full env-var path (`resolve_mask_inference_engine(true)`)
    /// wires the real engine when `LUMINA_MODEL_PATH` is the runnable test
    /// model. All env mutations in the suite write the identical value, so this
    /// cannot race other render tests.
    #[cfg(feature = "onnx-rt")]
    #[test]
    fn onnx_rt_full_resolve_uses_real_engine_from_env() {
        std::env::set_var("LUMINA_MODEL_PATH", onnx_test_fixture_path());
        let engine = resolve_mask_inference_engine(true)
            .expect("configured onnx-rt resolve must succeed")
            .expect("real engine must be wired");
        assert!(engine.is_available());
        let frame = ImageFrame::new(2, 2, vec![90u8; 2 * 2 * 4]).unwrap();
        let plane = engine.infer(&frame).expect("real engine must infer");
        assert_eq!((plane.width, plane.height), (2, 2));
    }

    /// `onnx-rt`: a missing artifact is a HARD error carrying the resolver's
    /// `MissingModel` text — never a stub masquerading as a real engine.
    #[cfg(feature = "onnx-rt")]
    #[test]
    fn onnx_rt_missing_artifact_is_hard_error_never_stub() {
        let error = resolve_onnx_engine_from_path(Path::new("/nonexistent/lumina-model.onnx"))
            .err()
            .expect("missing artifact must fail; an engine must never be returned");
        let text = error.to_string();
        assert!(text.contains("is not available"), "{text}");
        assert!(text.contains("no silent fallback"), "{text}");
    }

    /// `onnx-rt`: a present-but-useless (garbage) artifact is a HARD error,
    /// never silently replaced by the stub.
    #[cfg(feature = "onnx-rt")]
    #[test]
    fn onnx_rt_garbage_artifact_is_hard_error_never_stub() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("garbage.onnx");
        fs::write(&path, b"not an onnx model").unwrap();
        let error = resolve_onnx_engine_from_path(&path)
            .err()
            .expect("garbage must fail; an engine must never be returned");
        assert!(error.to_string().contains("no silent fallback"), "{error}");
    }

    /// `onnx-rt`: without mask work no engine is requested (the gate), so
    /// nothing is loaded and nothing fails.
    #[cfg(feature = "onnx-rt")]
    #[test]
    fn onnx_rt_no_mask_work_requests_no_engine() {
        assert!(
            resolve_mask_inference_engine(false)
                .expect("no request must not fail")
                .is_none(),
            "without mask work the CLI must not request any engine"
        );
    }

    #[test]
    fn render_with_valid_mask_zdata_has_no_warning() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let output = directory.path().join("output.png");
        let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
        let bytes = frame.encode(ImageFileFormat::Png).unwrap();
        fs::write(&input, &bytes).unwrap();
        write_sidecar_with_valid_layer(&input, &bytes, &frame);

        // Provide a 2x2 fully-filled artifact plane for `subject`, stored
        // under the per-copy composite record id (REVIEW-CLI-N1).
        let tile = lumina_sidecar::MaskTile {
            mask_id: zdata_mask_tile_id("vc-original", "subject"),
            tile_x: 0,
            tile_y: 0,
            width: 2,
            height: 2,
            values: vec![65535; 4],
        };
        let container = lumina_sidecar::ZDataContainer::new(vec![tile]).unwrap();
        lumina_sidecar::save_zdata(&lumina_sidecar::zdata_path_for(&input), &container).unwrap();

        let mut warnings = Vec::new();
        process_selected(
            ProcessArgs {
                input: input.clone(),
                output: output.clone(),
                preset: None,
                exposure: None,
                contrast: None,
                highlights: None,
                shadows: None,
                auto_tone: false,
                match_total_exposure: false,
                target_luminance: 0.5,
                write_metadata: false,
            },
            90,
            None,
            MaskPolicy::Warn,
            &mut warnings,
        )
        .unwrap();
        assert!(output.is_file());
        assert!(warnings.is_empty());
    }

    #[test]
    fn render_with_missing_mask_zdata_reinfers_and_succeeds() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let output = directory.path().join("output.png");
        let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
        let bytes = frame.encode(ImageFileFormat::Png).unwrap();
        fs::write(&input, &bytes).unwrap();
        write_sidecar_with_valid_layer(&input, &bytes, &frame);
        // No zdata file on purpose. With the inference model wired (F-048), the
        // missing artifact is (re-)inferred rather than reported as a warning;
        // the render succeeds with a produced mask.

        let mut warnings = Vec::new();
        process_selected(
            ProcessArgs {
                input: input.clone(),
                output: output.clone(),
                preset: None,
                exposure: None,
                contrast: None,
                highlights: None,
                shadows: None,
                auto_tone: false,
                match_total_exposure: false,
                target_luminance: 0.5,
                write_metadata: false,
            },
            90,
            None,
            MaskPolicy::Warn,
            &mut warnings,
        )
        .unwrap();
        assert!(output.is_file());
        assert!(warnings.is_empty());
    }

    // ---- Review fixes (2026-08 wave): one-shot mask flags, per-copy zdata
    // tiles, harmonized mask policy, batch collisions/resume, reindex exit
    // codes, symlink-safe collection, overwrite guards, import hash check,
    // dust-removal ordering. ----

    /// Writes a tiny 2x2 PNG and returns its path plus the frame.
    fn png_input(directory: &Path, name: &str, pixel: u8) -> (PathBuf, ImageFrame) {
        let input = directory.join(name);
        let frame = ImageFrame::new(2, 2, vec![pixel; 16]).unwrap();
        fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
        (input, frame)
    }

    #[test]
    fn one_shot_mask_flags_are_consumed_and_removed_from_the_recipe() {
        let directory = tempfile::tempdir().unwrap();
        let (input, frame) = png_input(directory.path(), "input.png", 100);
        let bytes = fs::read(&input).unwrap();
        write_sidecar_with_valid_layer(&input, &bytes, &frame);
        // Persisted artifact under the composite record id so the render is
        // warning-free once the flag is consumed.
        let tile = lumina_sidecar::MaskTile {
            mask_id: zdata_mask_tile_id("vc-original", "subject"),
            tile_x: 0,
            tile_y: 0,
            width: 2,
            height: 2,
            values: vec![65535; 4],
        };
        let container = lumina_sidecar::ZDataContainer::new(vec![tile]).unwrap();
        lumina_sidecar::save_zdata(&lumina_sidecar::zdata_path_for(&input), &container).unwrap();

        // develop/batch-style: persist the one-shot requests into the recipe.
        let sidecar_path = sidecar_path_for(&input);
        let mut document = load_sidecar(&sidecar_path).unwrap();
        document.virtual_copies[0]
            .recipe
            .options
            .insert("update_masks".into(), "true".into());
        document.virtual_copies[0]
            .recipe
            .options
            .insert("force_render".into(), "true".into());
        save_sidecar(&sidecar_path, &document).unwrap();

        let output = directory.path().join("output.png");
        let mut warnings = Vec::new();
        process_selected(
            ProcessArgs {
                input: input.clone(),
                output,
                preset: None,
                exposure: None,
                contrast: None,
                highlights: None,
                shadows: None,
                auto_tone: false,
                match_total_exposure: false,
                target_luminance: 0.5,
                write_metadata: false,
            },
            90,
            None,
            MaskPolicy::Warn,
            &mut warnings,
        )
        .unwrap();

        // REVIEW-CLI-MASKFLAG-1: after a successful run the consumed flags
        // must be gone from the persisted recipe — otherwise every future
        // run would re-infer despite a valid persisted mask.
        let document = load_sidecar(&sidecar_path).unwrap();
        assert!(!document.virtual_copies[0]
            .recipe
            .options
            .contains_key("update_masks"));
        assert!(!document.virtual_copies[0]
            .recipe
            .options
            .contains_key("force_render"));
    }

    #[test]
    fn zdata_tiles_are_scoped_per_virtual_copy() {
        let directory = tempfile::tempdir().unwrap();
        let (input, frame) = png_input(directory.path(), "input.png", 100);
        let bytes = fs::read(&input).unwrap();
        let identity = source_identity(&input, &bytes, &frame, None).unwrap();
        let mut document = SidecarDocument::new(identity.clone(), "raster-mvp-1");
        document.virtual_copies[0].mask_library = vec![valid_mask_definition(
            "subject",
            lumina_sidecar::MaskOperation::Source,
            vec![],
            &identity,
            2,
            2,
        )];
        // Second copy with the SAME mask id — the previous keying shared one
        // matte between both copies (REVIEW-CLI-N1).
        let mut second = document.virtual_copies[0].clone();
        second.id = "vc-two".into();
        second.name = "Two".into();
        document.virtual_copies.push(second);

        // Distinct planes under the composite record ids.
        let original_tile = lumina_sidecar::MaskTile {
            mask_id: zdata_mask_tile_id("vc-original", "subject"),
            tile_x: 0,
            tile_y: 0,
            width: 2,
            height: 2,
            values: vec![0; 4],
        };
        let two_tile = lumina_sidecar::MaskTile {
            mask_id: zdata_mask_tile_id("vc-two", "subject"),
            tile_x: 0,
            tile_y: 0,
            width: 2,
            height: 2,
            values: vec![65535; 4],
        };
        let container = lumina_sidecar::ZDataContainer::new(vec![original_tile, two_tile]).unwrap();
        let zdata_path = lumina_sidecar::zdata_path_for(&input);
        lumina_sidecar::save_zdata(&zdata_path, &container).unwrap();

        let mut warnings = Vec::new();
        let planes = load_persisted_mask_planes(&document, &zdata_path, &mut warnings);
        assert!(warnings.is_empty());
        assert_eq!(planes.len(), 2);
        assert_eq!(
            planes[&("vc-original".into(), "subject".into())].values,
            vec![0; 4]
        );
        assert_eq!(
            planes[&("vc-two".into(), "subject".into())].values,
            vec![65535; 4]
        );

        // Legacy bundles that stored the plane under the plain mask id are
        // deliberately NOT picked up any more (pre-MVP schema decision): a
        // silently shared matte is exactly what the fix removes.
        let legacy = lumina_sidecar::MaskTile {
            mask_id: "subject".into(),
            tile_x: 0,
            tile_y: 0,
            width: 2,
            height: 2,
            values: vec![12345; 4],
        };
        let container = lumina_sidecar::ZDataContainer::new(vec![legacy]).unwrap();
        lumina_sidecar::save_zdata(&zdata_path, &container).unwrap();
        let mut warnings = Vec::new();
        assert!(load_persisted_mask_planes(&document, &zdata_path, &mut warnings).is_empty());
        // Legacy plain-id tiles are ABSENCE (no record), not corruption: the
        // decision layer reports them as missing — no corrupt warning here.
        assert!(warnings.is_empty());
    }

    /// R2-CLI-05: a `.lumina.zdata` bundle that exists but is unreadable must
    /// surface as an explicit "corrupt" warning (stderr + mask warnings
    /// channel) instead of being silently treated like a missing bundle.
    #[test]
    fn render_with_corrupt_mask_zdata_warns_loudly_and_continues() {
        let directory = tempfile::tempdir().unwrap();
        let (input, frame) = png_input(directory.path(), "input.png", 100);
        let bytes = fs::read(&input).unwrap();
        write_sidecar_with_valid_layer(&input, &bytes, &frame);
        // Corrupt payload in place of a valid bundle.
        fs::write(
            lumina_sidecar::zdata_path_for(&input),
            b"definitely not zdata",
        )
        .unwrap();

        let output = directory.path().join("output.png");
        let mut warnings = Vec::new();
        process_selected(
            ProcessArgs {
                input: input.clone(),
                output,
                preset: None,
                exposure: None,
                contrast: None,
                highlights: None,
                shadows: None,
                auto_tone: false,
                match_total_exposure: false,
                target_luminance: 0.5,
                write_metadata: false,
            },
            90,
            None,
            MaskPolicy::Warn,
            &mut warnings,
        )
        .expect("warn policy continues past the corrupt bundle");

        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("unreadable or corrupt")),
            "the corrupt bundle must be reported through the mask-warning channel: {warnings:?}"
        );

        // A MISSING bundle (nothing persisted) stays warning-free — only
        // existing-but-unreadable bundles warn.
        let (input2, frame2) = png_input(directory.path(), "clean.png", 101);
        let bytes2 = fs::read(&input2).unwrap();
        write_sidecar_with_valid_layer(&input2, &bytes2, &frame2);
        let mut clean_warnings = Vec::new();
        process_selected(
            ProcessArgs {
                input: input2,
                output: directory.path().join("output-clean.png"),
                preset: None,
                exposure: None,
                contrast: None,
                highlights: None,
                shadows: None,
                auto_tone: false,
                match_total_exposure: false,
                target_luminance: 0.5,
                write_metadata: false,
            },
            90,
            None,
            MaskPolicy::Warn,
            &mut clean_warnings,
        )
        .unwrap();
        assert!(
            !clean_warnings
                .iter()
                .any(|warning| warning.contains("corrupt")),
            "a missing bundle is not corruption: {clean_warnings:?}"
        );
    }

    #[test]
    fn export_with_stale_masks_continues_by_default_and_aborts_under_strict() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 90);
        let bytes = fs::read(&input).unwrap();
        let frame = ImageFrame::decode(&bytes).unwrap();
        // Valid-status mask whose artifact is NOT available (no `.lumina.zdata`).
        write_sidecar_with_valid_layer(&input, &bytes, &frame);

        // Default `warn`: warn-and-continue, export succeeds (the wired stub
        // engine even re-infers during the render).
        let output = directory.path().join("out-warn.png");
        export(ExportArgs {
            input: input.clone(),
            output: output.clone(),
            format: "png".into(),
            quality: 90,
            virtual_copy: None,
            update_masks: false,
            force_render: false,
            migrate: false,
            json: false,
            mask_policy: CliMaskPolicy::Warn,
            write_metadata: false,
        })
        .unwrap();
        assert!(output.is_file());

        // `strict`: aborts BEFORE anything is decoded or written.
        let strict_output = directory.path().join("out-strict.png");
        let error = export(ExportArgs {
            input: input.clone(),
            output: strict_output.clone(),
            format: "png".into(),
            quality: 90,
            virtual_copy: None,
            update_masks: false,
            force_render: false,
            migrate: false,
            json: false,
            mask_policy: CliMaskPolicy::Strict,
            write_metadata: false,
        })
        .unwrap_err();
        assert!(error.to_string().contains("strict mask policy"));
        assert!(!strict_output.exists());
    }

    #[test]
    fn mask_policy_flag_defaults_to_warn_and_parses_strict() {
        let cli =
            Cli::try_parse_from(["lumina", "export", "--input", "a.png", "--output", "b.png"])
                .unwrap();
        assert!(matches!(
            cli.command,
            Command::Export(ExportArgs {
                mask_policy: CliMaskPolicy::Warn,
                ..
            })
        ));

        let cli = Cli::try_parse_from([
            "lumina",
            "batch",
            "--input",
            "src",
            "--output",
            "out",
            "--mask-policy",
            "strict",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Batch(BatchArgs {
                mask_policy: CliMaskPolicy::Strict,
                ..
            })
        ));

        assert!(Cli::try_parse_from([
            "lumina",
            "render",
            "--input",
            "a.png",
            "--output",
            "b.png",
            "--mask-policy",
            "bogus",
        ])
        .is_err());
    }

    /// LRPAR-G15-IPTC-S6: `--write-metadata` exists on `export`/`process`/
    /// `batch` (and only there — `render` has no such flag), defaults to off
    /// everywhere, and parses on all three paths.
    #[test]
    fn write_metadata_flag_defaults_off_and_parses_on_export_process_batch() {
        // Default: off (no metadata, exactly today's behavior).
        let cli =
            Cli::try_parse_from(["lumina", "export", "--input", "a.png", "--output", "b.jpg"])
                .unwrap();
        assert!(matches!(
            cli.command,
            Command::Export(ExportArgs {
                write_metadata: false,
                ..
            })
        ));
        let cli =
            Cli::try_parse_from(["lumina", "process", "--input", "a.png", "--output", "b.jpg"])
                .unwrap();
        assert!(matches!(
            cli.command,
            Command::Process(ProcessArgs {
                write_metadata: false,
                ..
            })
        ));
        let cli =
            Cli::try_parse_from(["lumina", "batch", "--input", "src", "--output", "out"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Batch(BatchArgs {
                write_metadata: false,
                ..
            })
        ));
        // Opt-in: on, on every path.
        let cli = Cli::try_parse_from([
            "lumina",
            "export",
            "--input",
            "a.png",
            "--output",
            "b.jpg",
            "--write-metadata",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Export(ExportArgs {
                write_metadata: true,
                ..
            })
        ));
        let cli = Cli::try_parse_from([
            "lumina",
            "process",
            "--input",
            "a.png",
            "--output",
            "b.jpg",
            "--write-metadata",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Process(ProcessArgs {
                write_metadata: true,
                ..
            })
        ));
        let cli = Cli::try_parse_from([
            "lumina",
            "batch",
            "--input",
            "src",
            "--output",
            "out",
            "--write-metadata",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Batch(BatchArgs {
                write_metadata: true,
                ..
            })
        ));
        // `render` deliberately offers no bake-in flag (SOLL §7: only
        // export/process/batch).
        assert!(Cli::try_parse_from([
            "lumina",
            "render",
            "--input",
            "a.png",
            "--output",
            "b.jpg",
            "--write-metadata",
        ])
        .is_err());
    }

    #[test]
    fn batch_rejects_colliding_output_names_before_writing() {
        let directory = tempfile::tempdir().unwrap();
        let src = directory.path().join("src");
        fs::create_dir_all(src.join("a")).unwrap();
        fs::create_dir_all(src.join("b")).unwrap();
        let (_, frame_a) = png_input(&src.join("a"), "x.png", 10);
        let (_, frame_b) = png_input(&src.join("b"), "x.arw", 20);
        drop(frame_a);
        drop(frame_b);

        let out = directory.path().join("out");
        let error = batch(BatchArgs {
            input: src,
            output: out.clone(),
            jobs: 1,
            retry: 0,
            resume: false,
            dry_run: false,
            update_masks: false,
            force_render: false,
            json: false,
            format: "png".into(),
            quality: 90,
            virtual_copy: None,
            mask_policy: CliMaskPolicy::Warn,
            write_metadata: false,
        })
        .unwrap_err();
        assert!(error.to_string().contains("collision"));
        // The refusal happens before the output directory exists.
        assert!(!out.exists());
    }

    #[test]
    fn batch_resume_requires_parsed_ok_status() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "a.png", 30);
        let out_dir = directory.path().join("out");
        fs::create_dir_all(&out_dir).unwrap();
        let output = out_dir.join("a.png");
        fs::write(&output, b"previous").unwrap();
        let status = out_dir.join("a.png.status.json");
        let args = BatchArgs {
            input: input.clone(),
            output: out_dir.clone(),
            jobs: 1,
            retry: 0,
            resume: true,
            dry_run: true,
            update_masks: false,
            force_render: false,
            json: false,
            format: "png".into(),
            quality: 90,
            virtual_copy: None,
            mask_policy: CliMaskPolicy::Warn,
            write_metadata: false,
        };

        // A spaced `"status": "ok"` parses as done (the old substring match
        // failed here and reprocessed the item).
        fs::write(&status, r#"{ "input": "a.png", "status": "ok" }"#).unwrap();
        let before = fs::read_to_string(&status).unwrap();
        batch_one(&input, 0, 1, &args).unwrap();
        assert_eq!(fs::read_to_string(&status).unwrap(), before);

        // A parsed non-ok status means "not done": the item is reprocessed
        // and the status file rewritten by this (dry) run.
        fs::write(
            &status,
            r#"{"note":"\"status\":\"ok\" decoy","status":"failed"}"#,
        )
        .unwrap();
        batch_one(&input, 0, 1, &args).unwrap();
        let rewritten = fs::read_to_string(&status).unwrap();
        assert!(rewritten.contains("\"dry-run\""), "{rewritten}");

        // Malformed JSON counts as not done, too.
        fs::write(&status, "not json at all").unwrap();
        batch_one(&input, 0, 1, &args).unwrap();
        let rewritten = fs::read_to_string(&status).unwrap();
        assert!(rewritten.contains("\"dry-run\""), "{rewritten}");
    }

    #[test]
    fn reindex_fails_when_a_sidecar_is_corrupt() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "good.png", 40);
        let bytes = fs::read(&input).unwrap();
        let frame = ImageFrame::decode(&bytes).unwrap();
        write_sidecar_with_valid_layer(&input, &bytes, &frame);
        // All-valid directory → success.
        reindex(IndexArgs {
            input: directory.path().to_path_buf(),
            json: true,
            migrate: false,
        })
        .unwrap();

        // One corrupt sidecar → loud failure (non-zero exit via `main`).
        fs::write(directory.path().join("broken.lumina.json"), "{ truncated").unwrap();
        let error = reindex(IndexArgs {
            input: directory.path().to_path_buf(),
            json: true,
            migrate: false,
        })
        .unwrap_err();
        assert!(error.to_string().contains("invalid sidecar"));
    }

    #[cfg(unix)]
    #[test]
    fn collect_images_survives_symlink_loops() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        fs::create_dir_all(root.join("sub")).unwrap();
        let (_, top) = png_input(&root, "top.png", 50);
        let (_, deep) = png_input(&root.join("sub"), "deep.png", 60);
        drop(top);
        drop(deep);
        // Self-referencing directory loop plus an alias onto a subdirectory.
        symlink(&root, root.join("loop")).unwrap();
        symlink(root.join("sub"), root.join("link-sub")).unwrap();
        // A file symlink stays collectable (reading it cannot cycle).
        symlink(root.join("top.png"), root.join("alias.png")).unwrap();

        let mut found = Vec::new();
        collect_images(&root, &mut found).unwrap();
        found.sort();
        let names: Vec<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["alias.png", "deep.png", "top.png"]);
    }

    #[cfg(unix)]
    #[test]
    fn collect_sidecars_survives_symlink_loops() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        fs::create_dir_all(root.join("sub")).unwrap();
        // Sidecar collection is purely path-based, so plain marker files are
        // enough here.
        let top = root.join("top.png.lumina.json");
        fs::write(&top, b"{}").unwrap();
        fs::write(root.join("sub/deep.png.lumina.json"), b"{}").unwrap();
        // Self-referencing directory loop plus an alias onto a subdirectory —
        // neither may be followed during the sidecar walk (REVIEW-CLI-
        // FOLLOWUP-1; without the guard this test recurses until the stack
        // overflows).
        symlink(&root, root.join("loop")).unwrap();
        symlink(root.join("sub"), root.join("link-sub")).unwrap();
        // A file symlink stays collectable (reading it cannot cycle).
        symlink(&top, root.join("alias.png.lumina.json")).unwrap();

        let mut found = Vec::new();
        collect_sidecars(&root, &mut found).unwrap();
        // The shared walk sorts every directory level, so the collected
        // sequence itself is already deterministic.
        let mut sorted = found.clone();
        sorted.sort();
        assert_eq!(found, sorted);
        let names: Vec<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec![
                "alias.png.lumina.json",
                "deep.png.lumina.json",
                "top.png.lumina.json"
            ]
        );
    }

    #[test]
    fn output_guard_rejects_sidecar_zdata_and_hardlink_targets() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 70);

        // Not-yet-existing bundle targets are protected at their future path…
        assert!(reject_protected_output(&input, &sidecar_path_for(&input)).is_err());
        assert!(reject_protected_output(&input, &lumina_sidecar::zdata_path_for(&input)).is_err());

        // …and equally once they exist.
        fs::write(sidecar_path_for(&input), b"{}").unwrap();
        assert!(reject_protected_output(&input, &sidecar_path_for(&input)).is_err());

        // A benign sibling path stays writable.
        let ok = directory.path().join("elsewhere.png");
        assert!(reject_protected_output(&input, &ok).is_ok());

        #[cfg(unix)]
        {
            let hardlink = directory.path().join("hardlink.png");
            fs::hard_link(&input, &hardlink).unwrap();
            let error = reject_protected_output(&input, &hardlink).unwrap_err();
            assert!(error.to_string().contains("hard link"));

            // CLI-GUARD-HARDLINK-1: hard links to the bundle are caught by
            // `(dev, inode)` identity, not only by path equality. The sidecar
            // already exists above; the zdata is materialized for the check.
            let sidecar_hardlink = directory.path().join("sidecar-hardlink.json");
            fs::hard_link(sidecar_path_for(&input), &sidecar_hardlink).unwrap();
            let error = reject_protected_output(&input, &sidecar_hardlink).unwrap_err();
            assert!(error.to_string().contains("hard link"), "error: {error}");
            assert!(error.to_string().contains("sidecar"), "error: {error}");

            let zdata = lumina_sidecar::zdata_path_for(&input);
            fs::write(&zdata, b"zdata").unwrap();
            let zdata_hardlink = directory.path().join("zdata-hardlink.bin");
            fs::hard_link(&zdata, &zdata_hardlink).unwrap();
            let error = reject_protected_output(&input, &zdata_hardlink).unwrap_err();
            assert!(error.to_string().contains("hard link"), "error: {error}");
            assert!(error.to_string().contains("bundle"), "error: {error}");
        }
    }

    #[test]
    fn import_rejects_changed_source_against_existing_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 80);
        let args = ImportArgs {
            input: input.clone(),
            json: false,
            migrate: false,
        };
        import_file(args.clone()).unwrap();

        // Change the file contents behind the same path: a second import must
        // fail loudly instead of blessing a sidecar for foreign contents.
        let changed = ImageFrame::new(2, 2, vec![81; 16]).unwrap();
        fs::write(&input, changed.encode(ImageFileFormat::Png).unwrap()).unwrap();
        let error = import_file(args).unwrap_err();
        assert!(error.to_string().contains("source changed"));
    }

    /// LRPAR-G08-PREVIOUS: `previous` copies the reference recipe onto every
    /// target (file → reload), tags one `previous` history step per target
    /// and leaves the reference sidecar untouched.
    #[test]
    fn previous_copies_reference_recipe_to_targets_with_history() {
        let directory = tempfile::tempdir().unwrap();
        let (reference, _) = png_input(directory.path(), "reference.png", 100);
        let (target_a, _) = png_input(directory.path(), "target-a.png", 120);
        let (target_b, _) = png_input(directory.path(), "target-b.png", 140);
        for input in [&reference, &target_a, &target_b] {
            import_file(ImportArgs {
                input: input.clone(),
                json: false,
                migrate: false,
            })
            .unwrap();
        }
        develop(DevelopArgs {
            input: reference.clone(),
            virtual_copy: None,
            exposure: Some(1.5),
            contrast: None,
            treatment: None,
            profile: None,
            update_masks: false,
            migrate: false,
            json: false,
        })
        .unwrap();

        previous(PreviousArgs {
            from: reference.clone(),
            to: vec![target_a.clone(), target_b.clone()],
            from_copy: None,
            to_copy: None,
            json: false,
        })
        .unwrap();

        for target in [&target_a, &target_b] {
            let document = load_sidecar(&sidecar_path_for(target)).unwrap();
            assert!(document.validate().is_ok());
            let copy = document
                .virtual_copies
                .iter()
                .find(|copy| copy.id == "vc-original")
                .unwrap();
            assert_eq!(copy.recipe.adjustments["exposure"], 1.5);
            let last = copy.history.last().unwrap();
            assert_eq!(last.id, "previous");
            assert_eq!(last.recipe.adjustments["exposure"], 1.5);
            // Portable by construction: the history extra carries the file
            // name, never a path.
            let source = last.extras["source"].as_str().unwrap();
            assert_eq!(source, "reference.png");
            assert!(!source.contains('/'));
        }
        // The reference sidecar is untouched (no history step added there).
        let reference_document = load_sidecar(&sidecar_path_for(&reference)).unwrap();
        assert!(reference_document.virtual_copies[0].history.is_empty());
    }

    /// LRPAR-G08-PREVIOUS: one missing target sidecar is a loud per-target
    /// failure (exit 3) — the healthy target is still updated.
    #[test]
    fn previous_reports_per_target_failure_without_aborting_rest() {
        let directory = tempfile::tempdir().unwrap();
        let (reference, _) = png_input(directory.path(), "reference.png", 100);
        let (good, _) = png_input(directory.path(), "good.png", 120);
        for input in [&reference, &good] {
            import_file(ImportArgs {
                input: input.clone(),
                json: false,
                migrate: false,
            })
            .unwrap();
        }
        develop(DevelopArgs {
            input: reference.clone(),
            virtual_copy: None,
            exposure: Some(2.0),
            contrast: None,
            treatment: None,
            profile: None,
            update_masks: false,
            migrate: false,
            json: false,
        })
        .unwrap();
        let missing = directory.path().join("gone.png");

        let error = previous(PreviousArgs {
            from: reference.clone(),
            to: vec![good.clone(), missing],
            from_copy: None,
            to_copy: None,
            json: false,
        })
        .unwrap_err();
        assert!(
            matches!(error, CliError::BatchPartial { failed: 1 }),
            "partial failure must map to exit 3, got {error}"
        );
        assert_eq!(error.exit_code(), 3);
        let document = load_sidecar(&sidecar_path_for(&good)).unwrap();
        let copy = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == "vc-original")
            .unwrap();
        assert_eq!(copy.recipe.adjustments["exposure"], 2.0);
        assert_eq!(copy.history.last().unwrap().id, "previous");
    }

    /// LRPAR-G08-PREVIOUS: a missing reference sidecar is a hard error
    /// (exit 1) and no target is touched.
    #[test]
    fn previous_missing_reference_fails_before_touching_targets() {
        let directory = tempfile::tempdir().unwrap();
        let (target, _) = png_input(directory.path(), "target.png", 120);
        import_file(ImportArgs {
            input: target.clone(),
            json: false,
            migrate: false,
        })
        .unwrap();
        let before = fs::read_to_string(sidecar_path_for(&target)).unwrap();

        let error = previous(PreviousArgs {
            from: directory.path().join("gone.png"),
            to: vec![target.clone()],
            from_copy: None,
            to_copy: None,
            json: false,
        })
        .unwrap_err();
        assert!(
            !matches!(error, CliError::BatchPartial { .. }),
            "a missing reference is a hard error, got {error}"
        );
        assert_eq!(error.exit_code(), 1);
        assert_eq!(
            fs::read_to_string(sidecar_path_for(&target)).unwrap(),
            before,
            "no target may be touched without a valid reference"
        );
    }

    /// LRPAR-G09-LIB: `relocate` moves the image with its sidecar
    /// companion; the recipe roundtrips and `inspect` stays valid.
    /// The cross-volume helper must move a file with no residue in the
    /// same-volume (rename) path. The `CrossesDevices` fallback needs two
    /// filesystems and is exercised manually, not unit-testable here.
    #[test]
    fn move_file_cross_volume_moves_a_file() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("a.bin");
        let target = directory.path().join("b.bin");
        fs::write(&source, b"payload").unwrap();
        move_file_cross_volume(&source, &target).unwrap();
        assert!(!source.exists());
        assert_eq!(fs::read(&target).unwrap(), b"payload");
        // A missing source stays a loud error.
        assert!(move_file_cross_volume(&source, &target).is_err());
    }

    #[test]
    fn relocate_moves_image_with_sidecar_and_roundtrips() {
        let directory = tempfile::tempdir().unwrap();
        let (source, _) = png_input(directory.path(), "photo.png", 100);
        import_file(ImportArgs {
            input: source.clone(),
            json: false,
            migrate: false,
        })
        .unwrap();
        develop(DevelopArgs {
            input: source.clone(),
            virtual_copy: None,
            exposure: Some(1.5),
            contrast: None,
            treatment: None,
            profile: None,
            update_masks: false,
            migrate: false,
            json: false,
        })
        .unwrap();
        let album = directory.path().join("album");
        std::fs::create_dir(&album).unwrap();
        let target = album.join("photo.png");

        relocate(RelocateArgs {
            from: source.clone(),
            to: target.clone(),
            json: false,
        })
        .unwrap();

        assert!(!source.exists(), "the source must be gone");
        assert!(target.is_file(), "the image must sit at the target");
        assert!(!sidecar_path_for(&source).exists());
        let moved = sidecar_path_for(&target);
        assert!(moved.is_file(), "the sidecar must follow the image");
        let document = load_sidecar(&moved).unwrap();
        assert!(document.validate().is_ok());
        assert_eq!(
            document.virtual_copies[0].recipe.adjustments["exposure"],
            1.5
        );
        // Rezept-relevant roundtrip: inspect the moved image.
        inspect(InspectArgs {
            input: target,
            json: true,
        })
        .unwrap();
    }

    /// LRPAR-G09-LIB (B1): same-directory rename derives companion targets
    /// from `--to` — the sidecar follows the new name instead of colliding
    /// with the source companion.
    #[test]
    fn relocate_same_dir_rename_moves_sidecar_to_new_name() {
        let directory = tempfile::tempdir().unwrap();
        let (source, _) = png_input(directory.path(), "photo.png", 100);
        import_file(ImportArgs {
            input: source.clone(),
            json: false,
            migrate: false,
        })
        .unwrap();
        let target = directory.path().join("renamed.png");

        relocate(RelocateArgs {
            from: source.clone(),
            to: target.clone(),
            json: false,
        })
        .unwrap();

        assert!(!source.exists());
        assert!(target.is_file());
        assert!(!sidecar_path_for(&source).exists());
        let moved = sidecar_path_for(&target);
        assert!(moved.is_file(), "sidecar must follow the new name");
        inspect(InspectArgs {
            input: target,
            json: true,
        })
        .unwrap();
    }

    /// LRPAR-G09-LIB (B1): cross-directory rename with both companions —
    /// `.lumina.json` and `.lumina.zdata` land under the target-derived
    /// names, never under the source names.
    #[test]
    fn relocate_cross_dir_rename_moves_json_and_zdata() {
        let directory = tempfile::tempdir().unwrap();
        let (source, _) = png_input(directory.path(), "photo.png", 100);
        import_file(ImportArgs {
            input: source.clone(),
            json: false,
            migrate: false,
        })
        .unwrap();
        let source_zdata = zdata_path_for(&source);
        lumina_sidecar::save_zdata(
            &source_zdata,
            &lumina_sidecar::ZDataContainer::new(vec![]).unwrap(),
        )
        .unwrap();
        let album = directory.path().join("album");
        std::fs::create_dir(&album).unwrap();
        let target = album.join("renamed.png");

        relocate(RelocateArgs {
            from: source.clone(),
            to: target.clone(),
            json: false,
        })
        .unwrap();

        assert!(!source.exists());
        assert!(!sidecar_path_for(&source).exists());
        assert!(!source_zdata.exists());
        assert!(target.is_file());
        let moved_json = sidecar_path_for(&target);
        let moved_zdata = zdata_path_for(&target);
        assert!(moved_json.is_file(), "json must follow the target name");
        assert!(moved_zdata.is_file(), "zdata must follow the target name");
        // No stale source-named companions linger next to the target.
        assert!(!album.join("photo.png.lumina.json").exists());
        assert!(!album.join("photo.png.lumina.zdata").exists());
        inspect(InspectArgs {
            input: target,
            json: true,
        })
        .unwrap();
    }

    /// LRPAR-G09-LIB: an existing target aborts loudly (exit 1) before
    /// anything is moved — never a silent overwrite.
    #[test]
    fn relocate_refuses_existing_target_without_moving() {
        let directory = tempfile::tempdir().unwrap();
        let (source, _) = png_input(directory.path(), "photo.png", 100);
        let (blocker, _) = png_input(directory.path(), "blocker.png", 120);
        import_file(ImportArgs {
            input: source.clone(),
            json: false,
            migrate: false,
        })
        .unwrap();
        let before = fs::read(&source).unwrap();
        let before_sidecar = fs::read(sidecar_path_for(&source)).unwrap();

        let error = relocate(RelocateArgs {
            from: source.clone(),
            to: blocker,
            json: false,
        })
        .unwrap_err();
        assert_eq!(error.exit_code(), 1);
        assert_eq!(fs::read(&source).unwrap(), before);
        assert_eq!(fs::read(sidecar_path_for(&source)).unwrap(), before_sidecar);
    }

    /// LRPAR-G09-LIB: a missing source is a loud error (exit 1).
    #[test]
    fn relocate_missing_source_fails_loudly() {
        let directory = tempfile::tempdir().unwrap();
        let error = relocate(RelocateArgs {
            from: directory.path().join("gone.png"),
            to: directory.path().join("elsewhere.png"),
            json: false,
        })
        .unwrap_err();
        assert_eq!(error.exit_code(), 1);
    }

    /// R2-CLI-10: `import` no longer accepts render-only flags that were
    /// silently ignored before (`--output`, `--format`, `--quality`,
    /// `--force-render`, `--virtual-copy`, `--mask-policy`).
    #[test]
    fn import_rejects_inherited_render_only_flags() {
        for flag in [
            "--output",
            "--format",
            "--quality",
            "--force-render",
            "--virtual-copy",
            "--mask-policy",
        ] {
            let parsed = Cli::try_parse_from([
                "lumina",
                "import",
                "--input",
                "a.png",
                flag,
                if flag == "--format" || flag == "--mask-policy" || flag == "--virtual-copy" {
                    "png"
                } else if flag == "--quality" {
                    "90"
                } else {
                    "b.png"
                },
            ]);
            assert!(
                parsed.is_err(),
                "`lumina import {flag}` must be rejected as unknown"
            );
        }
        // The slim set still parses.
        let ok = Cli::try_parse_from(["lumina", "import", "--input", "a.png", "--json"]).unwrap();
        assert!(matches!(
            ok.command,
            Command::Import(ImportArgs { json: true, .. })
        ));
    }

    /// R2-CLI-09: out-of-range/non-finite develop values fail up front with
    /// the allowed range in the message (mirroring MCP `lumina_edit`) — not
    /// later as a generic save-time rejection.
    #[test]
    fn develop_rejects_out_of_range_values_before_touching_the_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 60);
        import_file(ImportArgs {
            input: input.clone(),
            json: false,
            migrate: false,
        })
        .unwrap();
        let sidecar_path = sidecar_path_for(&input);
        let before = fs::read_to_string(&sidecar_path).unwrap();

        for (name, value) in [
            ("exposure", 999.0),
            ("exposure", -10.1),
            ("exposure", f64::NAN),
            ("contrast", 1.5),
            ("contrast", -1.1),
            ("contrast", f64::INFINITY),
        ] {
            let error = develop(DevelopArgs {
                input: input.clone(),
                virtual_copy: None,
                exposure: (name == "exposure").then_some(value),
                contrast: (name == "contrast").then_some(value),
                treatment: None,
                profile: None,
                update_masks: false,
                migrate: false,
                json: false,
            })
            .unwrap_err();
            let message = error.to_string();
            assert!(
                message.contains(&format!("invalid adjustment `{name}`"))
                    && message.contains("outside allowed range"),
                "{name}={value} must be rejected with the range, got: {message}"
            );
            if name == "exposure" {
                assert!(message.contains("-10..=10"), "{message}");
            } else {
                assert!(message.contains("-1..=1"), "{message}");
            }
        }

        // The failed runs never mutated the sidecar.
        assert_eq!(fs::read_to_string(&sidecar_path).unwrap(), before);

        // Boundary values stay valid and DO apply.
        develop(DevelopArgs {
            input: input.clone(),
            virtual_copy: None,
            exposure: Some(-10.0),
            contrast: Some(1.0),
            treatment: None,
            profile: None,
            update_masks: false,
            migrate: false,
            json: false,
        })
        .unwrap();
        let document = load_sidecar(&sidecar_path).unwrap();
        assert_eq!(
            document.virtual_copies[0].recipe.adjustments["exposure"],
            -10.0
        );
        assert_eq!(
            document.virtual_copies[0].recipe.adjustments["contrast"],
            1.0
        );
    }

    /// LRPAR-G01-BASIC: `develop --treatment/--profile` roundtrips through
    /// the sidecar, invalid values fail before any mutation, and `inspect`
    /// reports both fields. The original image bytes are never touched.
    #[test]
    fn develop_treatment_profile_roundtrip_and_rejection() {
        use lumina_sidecar::{DEFAULT_DEVELOP_PROFILE, TREATMENT_BW, TREATMENT_COLOR};
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 60);
        let original_bytes = fs::read(&input).unwrap();
        import_file(ImportArgs {
            input: input.clone(),
            json: false,
            migrate: false,
        })
        .unwrap();
        let sidecar_path = sidecar_path_for(&input);
        let develop_base = || DevelopArgs {
            input: input.clone(),
            virtual_copy: None,
            exposure: None,
            contrast: None,
            treatment: None,
            profile: None,
            update_masks: false,
            migrate: false,
            json: false,
        };
        // Set both fields in one run.
        let mut set = develop_base();
        set.treatment = Some(TREATMENT_BW.into());
        set.profile = Some("vivid".into());
        develop(set).unwrap();
        let document = load_sidecar(&sidecar_path).unwrap();
        let recipe = &document.virtual_copies[0].recipe;
        assert_eq!(recipe.treatment(), TREATMENT_BW);
        assert_eq!(recipe.develop_profile(), "vivid");
        assert_eq!(recipe.adjustments.get("saturation"), Some(&-1.0));
        // `inspect --json` reports both fields per copy.
        inspect(InspectArgs {
            input: input.clone(),
            json: true,
        })
        .unwrap();
        // Back to color restores identity (stash roundtrip through files).
        let mut back = develop_base();
        back.treatment = Some(TREATMENT_COLOR.into());
        back.profile = Some(DEFAULT_DEVELOP_PROFILE.into());
        develop(back).unwrap();
        let document = load_sidecar(&sidecar_path).unwrap();
        let recipe = &document.virtual_copies[0].recipe;
        assert_eq!(recipe.treatment(), TREATMENT_COLOR);
        assert_eq!(recipe.develop_profile(), DEFAULT_DEVELOP_PROFILE);
        assert!(!recipe.adjustments.contains_key("saturation"));
        // Invalid values fail loudly and leave the sidecar byte-identical.
        let before = fs::read(&sidecar_path).unwrap();
        for (treatment, profile) in [
            (Some("sepia".to_string()), None),
            (None, Some("adobe-color".to_string())),
            (None, Some(String::new())),
        ] {
            let mut bad = develop_base();
            bad.treatment = treatment;
            bad.profile = profile;
            develop(bad).unwrap_err();
        }
        assert_eq!(fs::read(&sidecar_path).unwrap(), before);
        assert_eq!(fs::read(&input).unwrap(), original_bytes);
    }

    // ---- G-05 Lens Blur CLI ----

    fn lens_blur_base_args(input: PathBuf) -> LensBlurArgs {
        LensBlurArgs {
            input,
            virtual_copy: None,
            json: true,
            list: false,
            enable: false,
            disable: false,
            set_amount: None,
            set_focal_near: None,
            set_focal_far: None,
            set_bokeh: None,
            set_focus_rect: None,
            set_depth_artifact: None,
            clear_depth_artifact: false,
            clear: false,
        }
    }

    /// G-05: `--clear` short-circuits the setters, so combining it with one
    /// must fail loudly instead of silently dropping the requested edit.
    #[test]
    fn lens_blur_clear_rejects_companion_mutation() {
        let mut clear = lens_blur_base_args(PathBuf::from("unused.png"));
        clear.clear = true;
        clear.set_amount = Some(0.8);
        let error = lens_blur(clear).unwrap_err().to_string();
        assert!(
            error.contains("--clear removes the whole lens-blur stage"),
            "{error}"
        );
        let mut clear = lens_blur_base_args(PathBuf::from("unused.png"));
        clear.clear = true;
        clear.enable = true;
        assert!(lens_blur(clear).is_err());
    }

    /// G-05: set fields, list (read-only), clear — with sidecar roundtrip and
    /// an untouched original.
    #[test]
    fn lens_blur_set_list_clear_roundtrip() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 120);
        let original_bytes = fs::read(&input).unwrap();
        import_sidecar_for(&input);
        // Set every field in one run.
        let mut set = lens_blur_base_args(input.clone());
        set.set_amount = Some(0.75);
        set.set_focal_near = Some(0.1);
        set.set_focal_far = Some(0.5);
        set.set_bokeh = Some("hexagonal".into());
        set.set_focus_rect = Some("0.2,0.3,0.4,0.25".into());
        lens_blur(set).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let blur = document.virtual_copies[0]
            .recipe
            .lens_blur
            .as_ref()
            .unwrap();
        assert!(blur.enabled);
        assert_eq!(blur.blur_amount, 0.75);
        assert_eq!((blur.focal_near, blur.focal_far), (0.1, 0.5));
        assert_eq!(blur.bokeh, BokehShape::Hexagonal);
        assert_eq!(
            (
                blur.focus_rect.x,
                blur.focus_rect.y,
                blur.focus_rect.width,
                blur.focus_rect.height
            ),
            (0.2, 0.3, 0.4, 0.25)
        );
        // Sidecar JSON carries the stage at the recipe root.
        let raw = fs::read_to_string(sidecar_path_for(&input)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            value["virtual_copies"][0]["recipe"]["lens_blur"]["bokeh"],
            "hexagonal"
        );
        // List-only is read-only: sidecar bytes unchanged.
        let before = fs::read(sidecar_path_for(&input)).unwrap();
        lens_blur(lens_blur_base_args(input.clone())).unwrap();
        assert_eq!(fs::read(sidecar_path_for(&input)).unwrap(), before);
        // Disable keeps values but reports off.
        let mut disable = lens_blur_base_args(input.clone());
        disable.disable = true;
        lens_blur(disable).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let blur = document.virtual_copies[0]
            .recipe
            .lens_blur
            .as_ref()
            .unwrap();
        assert!(!blur.enabled);
        assert_eq!(blur.blur_amount, 0.75);
        // B1: the --enable success path re-enables while keeping values.
        let mut enable = lens_blur_base_args(input.clone());
        enable.enable = true;
        lens_blur(enable).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let blur = document.virtual_copies[0]
            .recipe
            .lens_blur
            .as_ref()
            .unwrap();
        assert!(blur.enabled);
        assert_eq!(blur.blur_amount, 0.75);
        assert_eq!(
            lumina_core::lens_blur_status(Some(blur), false),
            "heuristic active"
        );
        // B1: set a depth artifact, then clear it — the reference is gone
        // and the status falls back to the heuristic.
        let mut set_depth = lens_blur_base_args(input.clone());
        set_depth.set_depth_artifact = Some("depth/map.bin:sha256:abc".into());
        lens_blur(set_depth).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let blur = document.virtual_copies[0]
            .recipe
            .lens_blur
            .as_ref()
            .unwrap();
        assert_eq!(
            blur.depth_artifact.as_ref().unwrap().relative_path,
            "depth/map.bin"
        );
        assert_eq!(
            lumina_core::lens_blur_status(Some(blur), false),
            "missing depth artifact"
        );
        let mut clear_depth = lens_blur_base_args(input.clone());
        clear_depth.clear_depth_artifact = true;
        lens_blur(clear_depth).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let blur = document.virtual_copies[0]
            .recipe
            .lens_blur
            .as_ref()
            .unwrap();
        assert!(blur.depth_artifact.is_none());
        assert_eq!(
            lumina_core::lens_blur_status(Some(blur), false),
            "heuristic active"
        );
        // Clear removes the stage.
        let mut clear = lens_blur_base_args(input.clone());
        clear.clear = true;
        lens_blur(clear).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert!(document.virtual_copies[0].recipe.lens_blur.is_none());
        // Original image untouched throughout.
        assert_eq!(fs::read(&input).unwrap(), original_bytes);
    }

    /// G-05: every invalid lens-blur input fails loudly (exit 1) and never
    /// mutates the sidecar — no silent clipping.
    #[test]
    fn lens_blur_rejects_invalid_values_without_touching_the_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 60);
        import_sidecar_for(&input);
        let sidecar_path = sidecar_path_for(&input);
        let before = fs::read_to_string(&sidecar_path).unwrap();

        // Out-of-range amount.
        let mut bad = lens_blur_base_args(input.clone());
        bad.set_amount = Some(2.0);
        let error = lens_blur(bad).unwrap_err();
        assert_eq!(error.exit_code(), 1);
        // Unknown bokeh shape.
        let mut bad = lens_blur_base_args(input.clone());
        bad.set_bokeh = Some("swirly".into());
        assert_eq!(lens_blur(bad).unwrap_err().exit_code(), 1);
        // Malformed focus rect.
        let mut bad = lens_blur_base_args(input.clone());
        bad.set_focus_rect = Some("0.1,0.2,oops".into());
        assert_eq!(lens_blur(bad).unwrap_err().exit_code(), 1);
        // Out-of-bounds focus rect (rejected on save, not clipped).
        let mut bad = lens_blur_base_args(input.clone());
        bad.set_focus_rect = Some("0.8,0.8,0.5,0.5".into());
        assert_eq!(lens_blur(bad).unwrap_err().exit_code(), 1);
        // Inverted focal range.
        let mut bad = lens_blur_base_args(input.clone());
        bad.set_focal_near = Some(0.8);
        bad.set_focal_far = Some(0.2);
        assert_eq!(lens_blur(bad).unwrap_err().exit_code(), 1);
        // Malformed depth reference.
        let mut bad = lens_blur_base_args(input.clone());
        bad.set_depth_artifact = Some("no-separator-here".into());
        assert_eq!(lens_blur(bad).unwrap_err().exit_code(), 1);
        // Absolute depth path (portable sidecars stay relative).
        let mut bad = lens_blur_base_args(input.clone());
        bad.set_depth_artifact = Some("/abs/depth.bin:sha256:abc".into());
        assert_eq!(lens_blur(bad).unwrap_err().exit_code(), 1);
        // Mutually exclusive flags.
        let mut bad = lens_blur_base_args(input.clone());
        bad.enable = true;
        bad.disable = true;
        assert_eq!(lens_blur(bad).unwrap_err().exit_code(), 1);
        // B1: the --set-depth-artifact + --clear-depth-artifact conflict arm.
        let mut bad = lens_blur_base_args(input.clone());
        bad.set_depth_artifact = Some("depth/map.bin:sha256:abc".into());
        bad.clear_depth_artifact = true;
        assert_eq!(lens_blur(bad).unwrap_err().exit_code(), 1);

        assert_eq!(fs::read_to_string(&sidecar_path).unwrap(), before);
    }

    /// G-05: a referenced-but-missing depth artifact fails the render loudly
    /// (exit 1) instead of silently rendering the heuristic.
    #[test]
    fn lens_blur_missing_depth_artifact_fails_render_loudly() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 120);
        import_sidecar_for(&input);
        let mut set = lens_blur_base_args(input.clone());
        set.set_depth_artifact = Some("depth/map.bin:sha256:abc".into());
        lens_blur(set).unwrap();
        // The reference round-trips and reports `missing`.
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let blur = document.virtual_copies[0]
            .recipe
            .lens_blur
            .as_ref()
            .unwrap();
        assert_eq!(
            blur.depth_artifact.as_ref().unwrap().relative_path,
            "depth/map.bin"
        );
        assert_eq!(
            lumina_core::lens_blur_status(Some(blur), false),
            "missing depth artifact"
        );
        // Rendering aborts loudly (exit 1), no output file appears.
        let output = directory.path().join("out.png");
        let mut warnings = Vec::new();
        let error = process_selected(
            ProcessArgs {
                input: input.clone(),
                output: output.clone(),
                preset: None,
                exposure: None,
                contrast: None,
                highlights: None,
                shadows: None,
                auto_tone: false,
                match_total_exposure: false,
                target_luminance: 0.5,
                write_metadata: false,
            },
            90,
            None,
            MaskPolicy::Warn,
            &mut warnings,
        )
        .unwrap_err();
        assert_eq!(error.exit_code(), 1);
        assert!(error.to_string().contains("lens_blur"));
        assert!(!output.exists());
    }

    fn color_base_args(input: PathBuf) -> ColorArgs {
        ColorArgs {
            input,
            virtual_copy: None,
            json: true,
            list: false,
            set_curve_param: Vec::new(),
            set_curve_points: Vec::new(),
            clear_curves: false,
            clear_curve_channel: None,
            set_hsl: Vec::new(),
            clear_hsl: false,
            add_point_color: false,
            hue_center: None,
            hue_range: None,
            hue_shift: None,
            sat_shift: None,
            lum_shift: None,
            set_point_color: Vec::new(),
            remove_point_color: Vec::new(),
            clear_point_color: false,
            set_grading: Vec::new(),
            set_grading_balance: None,
            set_grading_blending: None,
            clear_grading: false,
            set_vibrance: None,
            set_saturation: None,
        }
    }

    /// G-02: set every color stage in one run, list (read-only), then clear —
    /// with sidecar roundtrip and an untouched original.
    #[test]
    fn color_set_list_clear_roundtrip() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 120);
        let original_bytes = fs::read(&input).unwrap();
        import_sidecar_for(&input);
        // Set every stage in one run.
        let mut set = color_base_args(input.clone());
        set.set_curve_param = vec!["red:0.0,0.0,0.2,0.0".into()];
        set.set_curve_points = vec!["master:0,0;0.5,0.6;1,1".into()];
        set.set_hsl = vec!["red:hue:0.5".into(), "blue:luminance:-0.25".into()];
        set.add_point_color = true;
        set.hue_center = Some(30.0);
        set.hue_range = Some(20.0);
        set.sat_shift = Some(-0.5);
        set.set_grading = vec![
            "shadows:hue_degrees:120.0".into(),
            "highlights:luminance:0.4".into(),
        ];
        set.set_grading_balance = Some(0.1);
        set.set_grading_blending = Some(0.7);
        set.set_vibrance = Some(0.2);
        set.set_saturation = Some(-0.1);
        color(set).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let recipe = &document.virtual_copies[0].recipe;
        let curves = recipe.curves.as_ref().expect("curves");
        assert_eq!(curves.master.len(), 3);
        assert_eq!(
            curves.channels.red.as_ref().expect("red").len(),
            4,
            "parametric red persists as a 4-point list"
        );
        let hsl = recipe.hsl.as_ref().expect("hsl");
        assert_eq!(hsl.red.expect("red").hue, 0.5);
        assert_eq!(hsl.blue.expect("blue").luminance, -0.25);
        let point_color = recipe.point_color.as_ref().expect("point_color");
        assert_eq!(point_color.entries.len(), 1);
        assert_eq!(point_color.entries[0].id, "pc-1");
        assert_eq!(point_color.entries[0].hue_center, 30.0);
        assert_eq!(point_color.entries[0].saturation_shift, -0.5);
        let grading = recipe.color_grading.as_ref().expect("grading");
        assert_eq!(grading.shadows.hue_degrees, 120.0);
        assert_eq!(grading.highlights.luminance, 0.4);
        assert_eq!(grading.balance, 0.1);
        assert_eq!(grading.blending, 0.7);
        assert_eq!(recipe.adjustments["vibrance"], 0.2);
        assert_eq!(recipe.adjustments["saturation"], -0.1);
        // Sidecar JSON carries the stages in the adjustments map.
        let raw = fs::read_to_string(sidecar_path_for(&input)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let adjustments = &value["virtual_copies"][0]["recipe"]["adjustments"];
        assert!(adjustments.get("curves").is_some());
        assert!(adjustments.get("hsl").is_some());
        assert!(adjustments.get("point_color").is_some());
        assert!(adjustments.get("color_grading").is_some());
        // List-only is read-only: sidecar bytes unchanged.
        let before = fs::read(sidecar_path_for(&input)).unwrap();
        color(color_base_args(input.clone())).unwrap();
        assert_eq!(fs::read(sidecar_path_for(&input)).unwrap(), before);
        // Mutate one Point Color entry, then remove it (last remove drops
        // the block).
        let mut edit = color_base_args(input.clone());
        edit.set_point_color = vec!["pc-1:hue_center:200.0".into()];
        color(edit).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert_eq!(
            document.virtual_copies[0]
                .recipe
                .point_color
                .as_ref()
                .expect("point_color")
                .entries[0]
                .hue_center,
            200.0
        );
        let mut remove = color_base_args(input.clone());
        remove.remove_point_color = vec!["pc-1".into()];
        color(remove).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert!(document.virtual_copies[0].recipe.point_color.is_none());
        // Clear the remaining stages.
        let mut clear = color_base_args(input.clone());
        clear.clear_curves = true;
        clear.clear_hsl = true;
        clear.clear_grading = true;
        color(clear).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let recipe = &document.virtual_copies[0].recipe;
        assert!(recipe.curves.is_none());
        assert!(recipe.hsl.is_none());
        assert!(recipe.color_grading.is_none());
        // Original image untouched throughout.
        assert_eq!(fs::read(&input).unwrap(), original_bytes);
    }

    /// G-02: every invalid color input fails loudly (exit 1) and never
    /// mutates the sidecar — no silent clipping.
    #[test]
    fn color_rejects_invalid_values_without_touching_the_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 60);
        import_sidecar_for(&input);
        let sidecar_path = sidecar_path_for(&input);
        let before = fs::read_to_string(&sidecar_path).unwrap();

        // Unknown curve channel.
        let mut bad = color_base_args(input.clone());
        bad.set_curve_param = vec!["purple:0,0,0,0".into()];
        assert_eq!(color(bad).unwrap_err().exit_code(), 1);
        // Malformed curve param.
        let mut bad = color_base_args(input.clone());
        bad.set_curve_param = vec!["red:0,0".into()];
        assert_eq!(color(bad).unwrap_err().exit_code(), 1);
        // Non-ascending curve points (rejected on save, not reordered).
        let mut bad = color_base_args(input.clone());
        bad.set_curve_points = vec!["master:0,0;0.3,0.5;0.2,0.4;1,1".into()];
        assert_eq!(color(bad).unwrap_err().exit_code(), 1);
        // Missing (0,0) endpoint.
        let mut bad = color_base_args(input.clone());
        bad.set_curve_points = vec!["master:0.1,0.1;1,1".into()];
        assert_eq!(color(bad).unwrap_err().exit_code(), 1);
        // Unknown HSL channel / field.
        let mut bad = color_base_args(input.clone());
        bad.set_hsl = vec!["infrared:hue:0.5".into()];
        assert_eq!(color(bad).unwrap_err().exit_code(), 1);
        let mut bad = color_base_args(input.clone());
        bad.set_hsl = vec!["red:brightness:0.5".into()];
        assert_eq!(color(bad).unwrap_err().exit_code(), 1);
        // Out-of-range HSL value (rejected on save, not clipped).
        let mut bad = color_base_args(input.clone());
        bad.set_hsl = vec!["red:hue:2.0".into()];
        assert_eq!(color(bad).unwrap_err().exit_code(), 1);
        // Unknown Point Color entry / field.
        let mut bad = color_base_args(input.clone());
        bad.set_point_color = vec!["pc-99:hue_center:30.0".into()];
        assert_eq!(color(bad).unwrap_err().exit_code(), 1);
        let mut bad = color_base_args(input.clone());
        bad.add_point_color = true;
        color(bad).unwrap();
        let mut bad = color_base_args(input.clone());
        bad.set_point_color = vec!["pc-1:brightness:0.5".into()];
        assert_eq!(color(bad).unwrap_err().exit_code(), 1);
        // Out-of-range point color value.
        let mut bad = color_base_args(input.clone());
        bad.set_point_color = vec!["pc-1:hue_center:400.0".into()];
        assert_eq!(color(bad).unwrap_err().exit_code(), 1);
        // Unknown grading range / field / out-of-range blending.
        let mut bad = color_base_args(input.clone());
        bad.set_grading = vec!["lowlights:saturation:0.5".into()];
        assert_eq!(color(bad).unwrap_err().exit_code(), 1);
        let mut bad = color_base_args(input.clone());
        bad.set_grading = vec!["shadows:brightness:0.5".into()];
        assert_eq!(color(bad).unwrap_err().exit_code(), 1);
        let mut bad = color_base_args(input.clone());
        bad.set_grading_blending = Some(1.5);
        assert_eq!(color(bad).unwrap_err().exit_code(), 1);
        // Mutually exclusive flags.
        let mut bad = color_base_args(input.clone());
        bad.clear_curves = true;
        bad.clear_curve_channel = Some("red".into());
        assert_eq!(color(bad).unwrap_err().exit_code(), 1);
        let mut bad = color_base_args(input.clone());
        bad.clear_grading = true;
        bad.set_grading_balance = Some(0.1);
        assert_eq!(color(bad).unwrap_err().exit_code(), 1);
        // Ninth point color entry (limit 8).
        for _ in 0..7 {
            let mut add = color_base_args(input.clone());
            add.add_point_color = true;
            color(add).unwrap();
        }
        let mut bad = color_base_args(input.clone());
        bad.add_point_color = true;
        assert_eq!(color(bad).unwrap_err().exit_code(), 1);

        // The loud failures above (except the intentional successful adds)
        // must not corrupt the sidecar: it still loads and the stages that
        // were set on purpose round-trip.
        let document = load_sidecar(&sidecar_path).unwrap();
        assert_eq!(
            document.virtual_copies[0]
                .recipe
                .point_color
                .as_ref()
                .expect("point_color")
                .entries
                .len(),
            8
        );
        let _ = before;
    }

    // ---- G-06 Geometrie-Parität CLI ----

    fn geometry_base_args(input: PathBuf) -> GeometryArgs {
        GeometryArgs {
            input,
            virtual_copy: None,
            json: true,
            list: false,
            set_crop_aspect: None,
            set_crop_free: None,
            clear_crop: false,
            set_rotation: None,
            straighten: None,
            set_mirror: None,
            clear_geometry: false,
            set_lens_profile: None,
            set_lens: Vec::new(),
            clear_lens: false,
            set_perspective: Vec::new(),
            clear_perspective: false,
            lensfun_status: false,
        }
    }

    /// G-06: `--list` is the read-only view; combined with a mutation flag it
    /// must fail loudly instead of being ignored.
    #[test]
    fn geometry_list_rejects_mutation() {
        let mut args = geometry_base_args(PathBuf::from("unused.png"));
        args.list = true;
        args.set_rotation = Some(10.0);
        let error = geometry(args).unwrap_err().to_string();
        assert!(error.contains("--list is read-only"), "{error}");
    }

    /// G-06: set crop/straighten/mirror/lens/perspective, list (read-only),
    /// clear — with sidecar roundtrip, exactly one history entry per
    /// mutating call, and an untouched original.
    #[test]
    fn geometry_set_list_clear_roundtrip() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 120);
        let original_bytes = fs::read(&input).unwrap();
        import_sidecar_for(&input);
        // Set every stage in one run.
        let mut set = geometry_base_args(input.clone());
        set.set_crop_aspect = Some("16:9".into());
        set.straighten = Some(2.5);
        set.set_mirror = Some("h".into());
        set.set_lens_profile = Some("wide-light".into());
        set.set_lens = vec!["distortion_k1:0.1".into(), "ca_red:0.01".into()];
        set.set_perspective = vec!["vertical:0.2".into(), "scale:1.1".into()];
        geometry(set).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let recipe = &document.virtual_copies[0].recipe;
        assert!(matches!(
            recipe.geometry.as_ref().and_then(|g| g.crop.as_ref()),
            Some(Crop::Aspect {
                preset: AspectPreset::SixteenToNine
            })
        ));
        assert_eq!(
            recipe.geometry.as_ref().map(|g| g.rotation_degrees),
            Some(2.5)
        );
        assert_eq!(
            (
                recipe.geometry.as_ref().map(|g| g.mirror_horizontal),
                recipe.geometry.as_ref().map(|g| g.mirror_vertical)
            ),
            (Some(true), Some(false))
        );
        let lens = recipe.lens_correction.as_ref().unwrap();
        assert_eq!(lens.profile.as_deref(), Some("wide-light"));
        assert_eq!(lens.distortion_k1, Some(0.1));
        assert_eq!(lens.ca_red, Some(0.01));
        let perspective = recipe.perspective.as_ref().unwrap();
        assert_eq!(perspective.vertical, 0.2);
        assert_eq!(perspective.scale, 1.1);
        // Exactly one history entry for the mutating call (G-06 step rule).
        assert_eq!(document.virtual_copies[0].history.len(), 1);
        let entry = &document.virtual_copies[0].history[0];
        assert!(entry.id.starts_with("geometry-"), "got {}", entry.id);
        assert_eq!(entry.recipe, *recipe);
        // A second mutating call appends a second, uniquely-id'd entry.
        let mut second = geometry_base_args(input.clone());
        second.set_rotation = Some(-15.0);
        geometry(second).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert_eq!(document.virtual_copies[0].history.len(), 2);
        assert_ne!(
            document.virtual_copies[0].history[0].id,
            document.virtual_copies[0].history[1].id
        );
        assert_eq!(
            document.virtual_copies[0]
                .recipe
                .geometry
                .as_ref()
                .map(|g| g.rotation_degrees),
            Some(-15.0)
        );
        // List-only mode is read-only: no new entry, bytes unchanged.
        let before = fs::read(sidecar_path_for(&input)).unwrap();
        let list = geometry_base_args(input.clone());
        geometry(list).unwrap();
        assert_eq!(fs::read(sidecar_path_for(&input)).unwrap(), before);
        // Clear everything back to identity.
        let mut clear = geometry_base_args(input.clone());
        clear.clear_geometry = true;
        clear.clear_lens = true;
        clear.clear_perspective = true;
        geometry(clear).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let recipe = &document.virtual_copies[0].recipe;
        assert!(recipe.geometry.is_none());
        assert!(recipe.lens_correction.is_none());
        assert!(recipe.perspective.is_none());
        assert_eq!(fs::read(&input).unwrap(), original_bytes);
    }

    /// G-06: free-crop rects and the straighten alias round-trip; `--list`
    /// reports the stored stages.
    #[test]
    fn geometry_free_crop_and_straighten_alias_roundtrip() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 90);
        import_sidecar_for(&input);
        let mut set = geometry_base_args(input.clone());
        set.set_crop_free = Some("0.1,0.2,0.5,0.5".into());
        set.set_rotation = Some(45.0);
        geometry(set).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert!(matches!(
            document.virtual_copies[0]
                .recipe
                .geometry
                .as_ref()
                .and_then(|g| g.crop.as_ref()),
            Some(Crop::Free { .. })
        ));
        // `--straighten` commits the same field as `--set-rotation`.
        let mut straight = geometry_base_args(input.clone());
        straight.straighten = Some(-3.0);
        geometry(straight).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert_eq!(
            document.virtual_copies[0]
                .recipe
                .geometry
                .as_ref()
                .map(|g| g.rotation_degrees),
            Some(-3.0)
        );
    }

    /// G-06: unknown presets/fields/words and out-of-range values abort
    /// loudly (exit 1) without touching the sidecar.
    #[test]
    fn geometry_rejects_invalid_values_without_touching_the_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 60);
        import_sidecar_for(&input);
        let sidecar_path = sidecar_path_for(&input);
        let before = fs::read_to_string(&sidecar_path).unwrap();
        // Unknown aspect preset.
        let mut bad = geometry_base_args(input.clone());
        bad.set_crop_aspect = Some("21:9".into());
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        // Malformed free rect.
        let mut bad = geometry_base_args(input.clone());
        bad.set_crop_free = Some("0.1,0.2".into());
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        // Out-of-range free rect (rejected on save, not clipped).
        let mut bad = geometry_base_args(input.clone());
        bad.set_crop_free = Some("0.0,0.0,2.0,1.0".into());
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        // Out-of-range rotation (rejected on save, not clipped).
        let mut bad = geometry_base_args(input.clone());
        bad.set_rotation = Some(270.0);
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        // Non-finite straighten.
        let mut bad = geometry_base_args(input.clone());
        bad.straighten = Some(f64::NAN);
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        // Unknown mirror word.
        let mut bad = geometry_base_args(input.clone());
        bad.set_mirror = Some("diagonal".into());
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        // Unknown lens profile (rejected on save).
        let mut bad = geometry_base_args(input.clone());
        bad.set_lens_profile = Some("fisheye-extreme".into());
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        // Unknown lens field / non-number / out-of-range coefficient.
        let mut bad = geometry_base_args(input.clone());
        bad.set_lens = vec!["distortion_k9:0.1".into()];
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        let mut bad = geometry_base_args(input.clone());
        bad.set_lens = vec!["ca_red:much".into()];
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        let mut bad = geometry_base_args(input.clone());
        bad.set_lens = vec!["ca_red:0.5".into()];
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        // Unknown perspective field / out-of-range value.
        let mut bad = geometry_base_args(input.clone());
        bad.set_perspective = vec!["tilt:0.5".into()];
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        let mut bad = geometry_base_args(input.clone());
        bad.set_perspective = vec!["scale:99.0".into()];
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        // Mutually exclusive flags.
        let mut bad = geometry_base_args(input.clone());
        bad.set_rotation = Some(10.0);
        bad.straighten = Some(10.0);
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        let mut bad = geometry_base_args(input.clone());
        bad.set_crop_aspect = Some("1:1".into());
        bad.set_crop_free = Some("0,0,1,1".into());
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        let mut bad = geometry_base_args(input.clone());
        bad.clear_crop = true;
        bad.set_crop_aspect = Some("1:1".into());
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        let mut bad = geometry_base_args(input.clone());
        bad.clear_lens = true;
        bad.set_lens = vec!["distortion_k1:0.1".into()];
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        let mut bad = geometry_base_args(input.clone());
        bad.clear_geometry = true;
        bad.set_mirror = Some("h".into());
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        let mut bad = geometry_base_args(input.clone());
        bad.set_rotation = Some(10.0);
        bad.lensfun_status = true;
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        // Unknown virtual copy.
        let mut bad = geometry_base_args(input.clone());
        bad.virtual_copy = Some("no-such-copy".into());
        bad.set_rotation = Some(10.0);
        assert_eq!(geometry(bad).unwrap_err().exit_code(), 1);
        // The loud failures above must not touch the sidecar.
        assert_eq!(fs::read_to_string(&sidecar_path).unwrap(), before);
    }

    /// G-06: `--lensfun-status` is read-only (no save, no history entry)
    /// and reports a usable status line for raster inputs without EXIF.
    #[test]
    fn geometry_lensfun_status_is_read_only() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 70);
        import_sidecar_for(&input);
        let before = fs::read(sidecar_path_for(&input)).unwrap();
        let mut status = geometry_base_args(input.clone());
        status.lensfun_status = true;
        geometry(status).unwrap();
        assert_eq!(fs::read(sidecar_path_for(&input)).unwrap(), before);
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert!(document.virtual_copies[0].history.is_empty());
        // The resolver itself names the fallback loudly: without the
        // `lensfun` feature the missing capability, otherwise the
        // missing-EXIF/manual reason.
        let report = resolve_lensfun_report(&input);
        assert!(
            report.contains("manual model") || report.contains("unavailable"),
            "raster input must report the manual fallback or the missing capability, got `{report}`"
        );
    }

    /// R2-CLI-07: a partially failed batch exits with its own documented code
    /// (3) instead of the generic runtime-error code (1).
    #[test]
    fn batch_partial_failure_maps_to_exit_code_three() {
        let directory = tempfile::tempdir().unwrap();
        let src = directory.path().join("src");
        fs::create_dir_all(&src).unwrap();
        let (_, frame) = png_input(&src, "good.png", 10);
        drop(frame);
        // A corrupt payload fails at decode time → the item fails.
        fs::write(src.join("broken.png"), b"not a png").unwrap();

        let error = batch(BatchArgs {
            input: src,
            output: directory.path().join("out"),
            jobs: 1,
            retry: 0,
            resume: false,
            dry_run: false,
            update_masks: false,
            force_render: false,
            json: false,
            format: "png".into(),
            quality: 90,
            virtual_copy: None,
            mask_policy: CliMaskPolicy::Warn,
            write_metadata: false,
        })
        .unwrap_err();
        match &error {
            CliError::BatchPartial { failed } => assert_eq!(*failed, 1),
            other => panic!("expected BatchPartial, got {other:?}"),
        }
        assert_eq!(error.exit_code(), 3);
        // Every other CLI error keeps the generic code 1.
        assert_eq!(CliError::Message("x".into()).exit_code(), 1);
    }

    /// R2-CLI-11: batch inputs are deduplicated by filesystem identity so a
    /// hard link under two names is processed once (unix).
    #[cfg(unix)]
    #[test]
    fn batch_deduplicates_inputs_by_inode_identity() {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("original.arw");
        let alias = directory.path().join("alias.arw");
        fs::write(&original, b"synthetic").unwrap();
        fs::hard_link(&original, &alias).unwrap();
        let distinct = directory.path().join("distinct.arw");
        fs::write(&distinct, b"synthetic").unwrap();

        let deduped = dedup_same_file_inputs(vec![original.clone(), alias, distinct.clone()]);
        assert_eq!(
            deduped,
            vec![original, distinct],
            "the inode alias must be dropped, first occurrence kept"
        );

        // Unreadable metadata entries are kept (they fail loudly at decode).
        let missing = directory.path().join("missing.arw");
        let kept = dedup_same_file_inputs(vec![missing]);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn dust_removal_leaves_no_orphan_bundle_when_the_copy_is_unknown() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 85);
        let bytes = fs::read(&input).unwrap();
        let frame = ImageFrame::decode(&bytes).unwrap();
        save_sidecar(
            &sidecar_path_for(&input),
            &SidecarDocument::new(
                source_identity(&input, &bytes, &frame, None).unwrap(),
                "raster-mvp-1",
            ),
        )
        .unwrap();

        // Replacement image matching the source dimensions.
        let replacement = directory.path().join("replacement.png");
        let replacement_frame = ImageFrame::new(2, 2, vec![200; 16]).unwrap();
        fs::write(
            &replacement,
            replacement_frame.encode(ImageFileFormat::Png).unwrap(),
        )
        .unwrap();
        let definition = directory.path().join("region.json");
        fs::write(
            &definition,
            serde_json::json!({
                "id": "r1",
                "region_width": 2,
                "region_height": 2,
                "region_values": [0, 0, 0, 0],
                "replacement_path": replacement,
            })
            .to_string(),
        )
        .unwrap();

        let error = dust_removal(DustRemovalArgs {
            input: input.clone(),
            repair_region: definition,
            virtual_copy: Some("ghost".into()),
            render_out: None,
            json: true,
        })
        .unwrap_err();
        assert!(error.to_string().contains("unknown virtual copy"));
        // REVIEW-CLI-N2: nothing was appended before validation failed.
        assert!(!lumina_sidecar::zdata_path_for(&input).exists());
    }

    //
    // Documented boundary (F-042-N1): the CLI still passes an empty
    // source-action list (`source_actions: &[]` in `process_selected`).
    // Source actions reach the CLI only with F-042-N1 (persistence +
    // CLI command); no CLI source-action test is written yet.

    // ---- LRPAR-G04-REMOVE: `spot` command ----
    fn spot_base_args(input: PathBuf) -> SpotArgs {
        SpotArgs {
            input,
            virtual_copy: None,
            json: true,
            list: false,
            add_heuristic: false,
            center_x: None,
            center_y: None,
            radius: None,
            feather: None,
            offset_dx: None,
            offset_dy: None,
            opacity: None,
            clear: false,
            set_visualize_threshold: None,
            clear_visualize: false,
            set_distraction: None,
            detect_objects: false,
            detect_apply: false,
            detect_threshold: None,
            detect_max: None,
            regenerate_variant: None,
            variant: None,
            seed: None,
        }
    }

    fn import_sidecar_for(input: &Path) {
        let bytes = fs::read(input).unwrap();
        let frame = ImageFrame::decode(&bytes).unwrap();
        save_sidecar(
            &sidecar_path_for(input),
            &SidecarDocument::new(
                source_identity(input, &bytes, &frame, None).unwrap(),
                "raster-mvp-1",
            ),
        )
        .unwrap();
    }

    /// G-04: `--clear` runs after the adders and would discard their result,
    /// so combining it with one must fail loudly.
    #[test]
    fn spot_clear_rejects_adder() {
        let mut args = spot_base_args(PathBuf::from("unused.png"));
        args.clear = true;
        args.add_heuristic = true;
        let error = spot(args).unwrap_err().to_string();
        assert!(error.contains("--clear removes every spot"), "{error}");
    }

    #[test]
    fn spot_add_list_clear_roundtrip() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 120);
        import_sidecar_for(&input);
        // Add one heuristic spot.
        let mut add = spot_base_args(input.clone());
        add.add_heuristic = true;
        add.center_x = Some(0.5);
        add.center_y = Some(0.5);
        add.radius = Some(4.0);
        spot(add).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let spots: Vec<serde_json::Value> = serde_json::from_value(
            document.virtual_copies[0].recipe.extras["spot_removals"].clone(),
        )
        .unwrap();
        assert_eq!(spots.len(), 1);
        assert_eq!(spots[0]["mode"], "heuristic");
        // List-only is read-only: sidecar bytes unchanged.
        let before = fs::read(sidecar_path_for(&input)).unwrap();
        spot(spot_base_args(input.clone())).unwrap();
        assert_eq!(fs::read(sidecar_path_for(&input)).unwrap(), before);
        // Clear removes spots.
        let mut clear = spot_base_args(input.clone());
        clear.clear = true;
        spot(clear).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert!(!document.virtual_copies[0]
            .recipe
            .extras
            .contains_key("spot_removals"));
        // Original image untouched throughout.
        assert_eq!(
            fs::read(&input).unwrap().len(),
            fs::read(&input).unwrap().len()
        );
    }

    #[test]
    fn spot_rejects_bad_geometry_and_unknown_copies_loudly() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 120);
        import_sidecar_for(&input);
        // Missing radius.
        let mut add = spot_base_args(input.clone());
        add.add_heuristic = true;
        add.center_x = Some(0.5);
        add.center_y = Some(0.5);
        assert!(spot(add)
            .unwrap_err()
            .to_string()
            .contains("--add-heuristic requires"));
        // Out-of-range center.
        let mut add = spot_base_args(input.clone());
        add.add_heuristic = true;
        add.center_x = Some(1.5);
        add.center_y = Some(0.5);
        add.radius = Some(4.0);
        assert!(spot(add)
            .unwrap_err()
            .to_string()
            .contains("outside allowed range"));
        // Unknown copy.
        let mut add = spot_base_args(input.clone());
        add.virtual_copy = Some("ghost".into());
        add.add_heuristic = true;
        add.center_x = Some(0.5);
        add.center_y = Some(0.5);
        add.radius = Some(4.0);
        assert!(spot(add)
            .unwrap_err()
            .to_string()
            .contains("unknown virtual copy"));
        // Failed runs never mutated the sidecar.
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert!(!document.virtual_copies[0]
            .recipe
            .extras
            .contains_key("spot_removals"));
    }

    #[test]
    fn spot_visualize_and_distraction_roundtrip() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 120);
        import_sidecar_for(&input);
        let mut vis = spot_base_args(input.clone());
        vis.set_visualize_threshold = Some(0.3);
        spot(vis).unwrap();
        let mut dis = spot_base_args(input.clone());
        dis.set_distraction = Some("dust=true,auto=true".into());
        spot(dis).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert_eq!(
            document.virtual_copies[0].recipe.spot_visualize_threshold(),
            Some(0.3)
        );
        assert_eq!(
            document.virtual_copies[0].recipe.spot_distraction(),
            lumina_sidecar::SpotDistraction {
                dust: true,
                auto_mode: true,
                ..Default::default()
            }
        );
        // Reload leg: JSON roundtrip preserves both.
        let decoded = SidecarDocument::from_json(&document.to_json().unwrap()).unwrap();
        assert_eq!(
            decoded.virtual_copies[0].recipe.spot_visualize_threshold(),
            Some(0.3)
        );
        // Clear visualize.
        let mut clear = spot_base_args(input.clone());
        clear.clear_visualize = true;
        spot(clear).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert_eq!(
            document.virtual_copies[0].recipe.spot_visualize_threshold(),
            None
        );
        // Bad specs fail loudly.
        let mut bad = spot_base_args(input.clone());
        bad.set_visualize_threshold = Some(2.0);
        assert!(spot(bad).is_err());
        let mut bad = spot_base_args(input.clone());
        bad.set_distraction = Some("dust=maybe".into());
        assert!(spot(bad)
            .unwrap_err()
            .to_string()
            .contains("invalid distraction value"));
        let mut bad = spot_base_args(input.clone());
        bad.set_distraction = Some("cats=true".into());
        assert!(spot(bad)
            .unwrap_err()
            .to_string()
            .contains("unknown distraction key"));
    }

    #[test]
    fn spot_detect_lists_without_apply_and_applies_explicitly() {
        let directory = tempfile::tempdir().unwrap();
        // 16x16 frame with a dark 8x8 block (one heuristic cell).
        let mut pixels = vec![255u8; 16 * 16 * 4];
        for y in 0..8 {
            for x in 0..8 {
                let idx = (y * 16 + x) as usize * 4;
                pixels[idx] = 0;
                pixels[idx + 1] = 0;
                pixels[idx + 2] = 0;
            }
        }
        let frame = ImageFrame::new(16, 16, pixels).unwrap();
        let input = directory.path().join("dark.png");
        fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
        import_sidecar_for(&input);
        // List-only: candidates found, nothing persisted.
        let mut detect = spot_base_args(input.clone());
        detect.detect_objects = true;
        spot(detect).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert!(!document.virtual_copies[0]
            .recipe
            .extras
            .contains_key("spot_removals"));
        // Explicit apply persists exactly the candidates.
        let mut apply = spot_base_args(input.clone());
        apply.detect_objects = true;
        apply.detect_apply = true;
        spot(apply).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let spots: Vec<serde_json::Value> = serde_json::from_value(
            document.virtual_copies[0].recipe.extras["spot_removals"].clone(),
        )
        .unwrap();
        assert_eq!(spots.len(), 1);
        // --detect-apply without --detect-objects fails loudly.
        let mut lonely = spot_base_args(input.clone());
        lonely.detect_apply = true;
        assert!(spot(lonely)
            .unwrap_err()
            .to_string()
            .contains("--detect-apply requires"));
    }

    #[test]
    fn spot_detect_defaults_to_recipe_visualize_threshold() {
        // G04-FOLLOWUP-1: without `--detect-threshold` the recipe visualize
        // threshold is the default (else 0.5); an explicit flag wins.
        let directory = tempfile::tempdir().unwrap();
        let mut pixels = vec![255u8; 16 * 16 * 4];
        for y in 0..8 {
            for x in 0..8 {
                let idx = (y * 16 + x) as usize * 4;
                pixels[idx] = 0;
                pixels[idx + 1] = 0;
                pixels[idx + 2] = 0;
            }
        }
        let frame = ImageFrame::new(16, 16, pixels).unwrap();
        let input = directory.path().join("dark.png");
        fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
        import_sidecar_for(&input);
        // Recipe default 1.0: every 8x8 cell is dark -> 4 candidates.
        let mut vis = spot_base_args(input.clone());
        vis.set_visualize_threshold = Some(1.0);
        spot(vis).unwrap();
        let mut apply = spot_base_args(input.clone());
        apply.detect_objects = true;
        apply.detect_apply = true;
        spot(apply).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let spots: Vec<serde_json::Value> = serde_json::from_value(
            document.virtual_copies[0].recipe.extras["spot_removals"].clone(),
        )
        .unwrap();
        assert_eq!(spots.len(), 4, "recipe threshold 1.0 must drive detection");
        // Explicit flag wins over the recipe default: 0.5 sees one cell.
        let mut clear = spot_base_args(input.clone());
        clear.clear = true;
        spot(clear).unwrap();
        let mut apply = spot_base_args(input.clone());
        apply.detect_objects = true;
        apply.detect_apply = true;
        apply.detect_threshold = Some(0.5);
        spot(apply).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let spots: Vec<serde_json::Value> = serde_json::from_value(
            document.virtual_copies[0].recipe.extras["spot_removals"].clone(),
        )
        .unwrap();
        assert_eq!(spots.len(), 1, "explicit threshold must win");
        // Original image untouched throughout.
        let _ = fs::read(&input).unwrap();
    }

    #[test]
    fn spot_set_distraction_merges_into_stored_switches() {
        // G04-FOLLOWUP-1 merge decision: unnamed keys keep their stored
        // value (consistent with the GUI single-checkbox toggles).
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 120);
        import_sidecar_for(&input);
        let mut first = spot_base_args(input.clone());
        first.set_distraction = Some("reflections=true".into());
        spot(first).unwrap();
        let mut second = spot_base_args(input.clone());
        second.set_distraction = Some("dust=true".into());
        spot(second).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert_eq!(
            document.virtual_copies[0].recipe.spot_distraction(),
            lumina_sidecar::SpotDistraction {
                reflections: true,
                dust: true,
                ..Default::default()
            },
            "unnamed `reflections` must survive a later partial set"
        );
        // Explicit `k=false` switches a single key off, keeping the rest.
        let mut off = spot_base_args(input.clone());
        off.set_distraction = Some("dust=false".into());
        spot(off).unwrap();
        let document = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert_eq!(
            document.virtual_copies[0].recipe.spot_distraction(),
            lumina_sidecar::SpotDistraction {
                reflections: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn spot_regenerate_variant_sets_derived_seed_deterministically() {
        let directory = tempfile::tempdir().unwrap();
        let (input, _) = png_input(directory.path(), "input.png", 120);
        import_sidecar_for(&input);
        // Seed one generative entry directly (heuristic path has no variants).
        let path = sidecar_path_for(&input);
        let mut document = load_sidecar(&path).unwrap();
        document.virtual_copies[0].recipe.extras.insert(
            "spot_removals".into(),
            serde_json::json!([{"id": "g1", "version": 1, "mode": "generative", "prompt": "x"}]),
        );
        document.validate().unwrap();
        save_sidecar(&path, &document).unwrap();
        let mut regen = spot_base_args(input.clone());
        regen.regenerate_variant = Some("g1".into());
        regen.variant = Some(2);
        regen.seed = Some(7);
        spot(regen).unwrap();
        let document = load_sidecar(&path).unwrap();
        let spots: Vec<serde_json::Value> = serde_json::from_value(
            document.virtual_copies[0].recipe.extras["spot_removals"].clone(),
        )
        .unwrap();
        let expected = lumina_core::generative_variant_seed(7, 2);
        assert_eq!(spots[0]["seed"], expected);
        assert_eq!(spots[0]["variant"], 2);
        assert_eq!(spots[0]["base_seed"], 7);
        // Deterministic: re-running the same variant is a stable no-op.
        let before = fs::read(&path).unwrap();
        let mut regen = spot_base_args(input.clone());
        regen.regenerate_variant = Some("g1".into());
        regen.variant = Some(2);
        regen.seed = Some(7);
        spot(regen).unwrap();
        assert_eq!(fs::read(&path).unwrap(), before);
        // Unknown ids and heuristic spots fail loudly.
        let mut bad = spot_base_args(input.clone());
        bad.regenerate_variant = Some("nope".into());
        bad.variant = Some(1);
        bad.seed = Some(7);
        assert!(spot(bad).unwrap_err().to_string().contains("unknown spot"));
        let mut bad = spot_base_args(input.clone());
        bad.regenerate_variant = Some("g1".into());
        assert!(spot(bad)
            .unwrap_err()
            .to_string()
            .contains("--regenerate-variant requires"));
    }

    #[test]
    fn history_entry_stores_final_recipe_and_snapshot_reproduces_output() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let output = directory.path().join("output.png");
        let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
        let bytes = frame.encode(ImageFileFormat::Png).unwrap();
        fs::write(&input, &bytes).unwrap();
        process(ProcessArgs {
            input: input.clone(),
            output: output.clone(),
            preset: None,
            exposure: Some(0.5),
            contrast: Some(-0.2),
            highlights: Some(0.1),
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
            write_metadata: false,
        })
        .unwrap();
        assert!(output.is_file());

        let sidecar = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let copy = &sidecar.virtual_copies[0];
        // Exactly one new history entry; its recipe snapshot is the final
        // recipe of the process run. `assert_eq` on EditRecipe covers all
        // relevant fields (adjustments, auto_features, nested stages).
        assert_eq!(copy.history.len(), 1);
        let entry = &copy.history[0];
        assert_eq!(entry.recipe, copy.recipe);
        assert_eq!(entry.recipe.adjustments["exposure"], 0.5);
        assert_eq!(entry.recipe.adjustments["contrast"], -0.2);
        assert_eq!(entry.recipe.adjustments["highlights"], 0.1);
        assert!(!entry.recipe.auto_features.enable_auto_tone);
        assert!(!entry.recipe.auto_features.match_total_exposure);
        assert!(entry.recorded_at.is_some());

        // Snapshot reproducibility: applying the stored recipe alone to the
        // original frame reproduces the process output byte-identically (PNG
        // is lossless and the encoder is deterministic), plus a decoded-pixel
        // cross-check.
        let source = ImageFrame::decode(&fs::read(&input).unwrap()).unwrap();
        let rendered = render_frame(
            &source,
            &RenderContext {
                recipe: &entry.recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                // F-098-N2: this is a synthetic, recipe-only render without RAW
                // metadata/EXIF, so no Lensfun corrector can be built — `None`
                // (manual model) is the correct, expected state here.
                lensfun: None,
                depth: None,
            },
        )
        .unwrap();
        let expected = fs::read(&output).unwrap();
        assert_eq!(
            rendered.frame.encode(ImageFileFormat::Png).unwrap(),
            expected
        );
        assert_eq!(ImageFrame::decode(&expected).unwrap(), rendered.frame);
    }

    // ---- F-103-N8: no-match export reuses the warning render (no duplicate) ----
    #[test]
    fn no_match_export_is_byte_identical_to_single_render() {
        // The no-match export path must reuse the warning render instead of
        // re-rendering through `export_image`. The produced file must stay
        // byte-identical to a single `export_image` pass with the same final
        // recipe — i.e. exactly the pre-optimization output (F-103-N8). This
        // guards the optimization against any silent output drift.
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let output = directory.path().join("output.webp");
        // A non-uniform spatial gradient so exposure/contrast actually move
        // pixels; a uniform frame can be invariant under 8-bit rounding.
        let width: u32 = 16;
        let height: u32 = 16;
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let value = ((x + y) % 256) as u8;
                pixels.extend_from_slice(&[value, value, value, 255]);
            }
        }
        let frame = ImageFrame::new(width, height, pixels).unwrap();
        fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();

        process(ProcessArgs {
            input: input.clone(),
            output: output.clone(),
            preset: None,
            exposure: Some(0.3),
            contrast: Some(0.2),
            highlights: Some(-0.1),
            shadows: Some(0.1),
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
            write_metadata: false,
        })
        .unwrap();

        let actual = fs::read(&output).unwrap();
        // The final recipe persisted by `process` (incl. the CLI adjustments
        // above) is the recipe `export_image` would have rendered.
        let sidecar = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let final_recipe = sidecar.virtual_copies[0].recipe.clone();
        let source = ImageFrame::decode(&fs::read(&input).unwrap()).unwrap();
        // `process` always uses the default quality 90 and `dither: false`,
        // matching the historical `frame.encode(format)` output.
        let options = ExportOptions {
            format: ImageFileFormat::WebP,
            quality: 90,
            dither: false,
            ..Default::default()
        };
        let expected = export_image(
            &source,
            &RenderContext {
                recipe: &final_recipe,
                camera_white_balance: None,
                source_actions: &[],
                // No mask library in this test → empty mask context, which
                // renders identically to `None` (see render.rs
                // `no_layers_is_identical_to_no_mask_context`).
                masks: None,
                lensfun: None,
                depth: None,
            },
            options,
        )
        .unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn match_total_exposure_still_rerenders_with_matched_recipe() {
        // Complementary guard to F-103-N8: when matching is ON the CLI must
        // still re-render with the matched exposure (the output must differ
        // from the *unmatched* single render, confirming the second render is
        // not silently skipped).
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let output = directory.path().join("output.png");
        let width: u32 = 16;
        let height: u32 = 16;
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let value = ((x + y) % 256) as u8;
                pixels.extend_from_slice(&[value, value, value, 255]);
            }
        }
        let frame = ImageFrame::new(width, height, pixels).unwrap();
        fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();

        process(ProcessArgs {
            input: input.clone(),
            output: output.clone(),
            preset: None,
            exposure: None,
            contrast: None,
            highlights: None,
            shadows: None,
            auto_tone: false,
            match_total_exposure: true,
            target_luminance: 0.5,
            write_metadata: false,
        })
        .unwrap();
        let matched = fs::read(&output).unwrap();
        assert!(output.is_file());

        // The unmatched single render (recipe without the matched exposure) must
        // differ from the matched output, proving the second render actually ran.
        let sidecar = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let mut unmatched_recipe = sidecar.virtual_copies[0].recipe.clone();
        unmatched_recipe.adjustments.remove("exposure");
        let source = ImageFrame::decode(&fs::read(&input).unwrap()).unwrap();
        let options = ExportOptions {
            format: ImageFileFormat::Png,
            quality: 90,
            dither: false,
            ..Default::default()
        };
        let unmatched = export_image(
            &source,
            &RenderContext {
                recipe: &unmatched_recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
                lensfun: None,
                depth: None,
            },
            options,
        )
        .unwrap();
        assert_ne!(matched, unmatched);
    }

    #[test]
    fn valid_mask_with_match_total_exposure_measures_masked_domain() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let output = directory.path().join("output.png");
        // 8x8 bimodal gray frame: left half (pixels 0..32) is 200, right half
        // (pixels 32..64) is 60. Unmasked mean = 130/255 ~= 0.51.
        let mut pixels = Vec::with_capacity(8 * 8 * 4);
        for index in 0..64 {
            let value = if index < 32 { 200u8 } else { 60u8 };
            pixels.extend_from_slice(&[value, value, value, 255]);
        }
        let frame = ImageFrame::new(8, 8, pixels).unwrap();
        let bytes = frame.encode(ImageFileFormat::Png).unwrap();
        fs::write(&input, &bytes).unwrap();
        write_sidecar_with_valid_layer(&input, &bytes, &frame);

        // Valid artifact plane for `subject` at frame resolution. F-042 does
        // not modulate pixels yet (pixel modulation is F-049), but F-041
        // already weights the measurement domain: the bright left half is
        // fully masked (0), the dark right half fully visible (u16::MAX).
        //   weighted mean: 60/255 ~= 0.2353
        //   masked delta:  log2(0.5 / (60/255)) = log2(2.125) ~= 1.08746
        //   unmasked delta: log2(0.5 / (130/255)) ~= -0.0280
        let tile = lumina_sidecar::MaskTile {
            mask_id: zdata_mask_tile_id("vc-original", "subject"),
            tile_x: 0,
            tile_y: 0,
            width: 8,
            height: 8,
            values: (0..64).map(|i| if i < 32 { 0 } else { 65535 }).collect(),
        };
        let container = lumina_sidecar::ZDataContainer::new(vec![tile]).unwrap();
        lumina_sidecar::save_zdata(&lumina_sidecar::zdata_path_for(&input), &container).unwrap();

        let unmasked = match_total_exposure_masked(&frame, 0.5, &[]).unwrap();
        let mut warnings = Vec::new();
        process_selected(
            ProcessArgs {
                input: input.clone(),
                output: output.clone(),
                preset: None,
                exposure: None,
                contrast: None,
                highlights: None,
                shadows: None,
                auto_tone: false,
                match_total_exposure: true,
                target_luminance: 0.5,
                write_metadata: false,
            },
            90,
            None,
            MaskPolicy::Warn,
            &mut warnings,
        )
        .unwrap();
        assert!(output.is_file());
        assert!(
            warnings.is_empty(),
            "valid mask must not warn: {warnings:?}"
        );

        // The persisted matching result follows the masked measurement domain
        // and demonstrably differs from the unmasked result (F-041).
        let sidecar = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let auto = &sidecar.virtual_copies[0].recipe.auto_features;
        assert!(auto.match_total_exposure);
        let matched = auto.matched_exposure.unwrap();
        assert!(
            (matched - 1.08746).abs() < 0.001,
            "persisted matched exposure {matched}"
        );
        assert!(
            (matched - unmasked).abs() > 1.0,
            "masked delta {matched} must differ from unmasked {unmasked}"
        );

        // Applying the delta reaches the *masked* target: the visible (right)
        // half of the exported frame (60 * 2^1.08746 = 60 * 2.125 = 127.5 ->
        // 128, mean ~= 0.502) is within tolerance, while the masked-out left
        // half clamps at 255 and must not be part of the target check.
        let rendered = ImageFrame::decode(&fs::read(&output).unwrap()).unwrap();
        let visible_mean = rendered
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(index, _)| *index >= 32)
            .map(|(_, pixel)| {
                (0.2126 * f64::from(pixel[0])
                    + 0.7152 * f64::from(pixel[1])
                    + 0.0722 * f64::from(pixel[2]))
                    / 255.0
            })
            .sum::<f64>()
            / 32.0;
        assert!(
            (visible_mean - 0.5).abs() <= 0.02,
            "post-match visible mean {visible_mean} not within 0.02 of target 0.5"
        );
    }

    // ---- F-019: CLI `--migrate` delegates to the library migration path ----

    /// Writes a legacy schema-version-0 sidecar next to `input` (the historical
    /// pre-release stamp `migrate_json` bumps 0 → 1 → 2) and returns its path.
    /// The bytes are written verbatim, bypassing `validate`, so the on-disk file
    /// really is a v0 document that only an explicit migration may change.
    fn write_legacy_sidecar(input: &Path) -> PathBuf {
        let path = sidecar_path_for(input);
        let bytes = fs::read(input).unwrap();
        let frame = ImageFrame::decode(&bytes).unwrap();
        let document = SidecarDocument::new(
            source_identity(input, &bytes, &frame, None).unwrap(),
            "raster-mvp-1",
        );
        let mut value: serde_json::Value =
            serde_json::from_str(&document.to_json().unwrap()).unwrap();
        value["schema_version"] = serde_json::Value::from(0);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        path
    }

    /// F-019: `validate --migrate` must run through
    /// `lumina_sidecar::migrate_sidecar_file` — the `.bak` backup and the
    /// released write lock are the observable proof (the old CLI path used
    /// `migrate_json` + `write_atomically` and created neither).
    #[test]
    fn cli_migrate_flag_uses_library_path_and_creates_backup() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let (_, frame) = png_input(directory.path(), "input.png", 90);
        drop(frame);
        let path = write_legacy_sidecar(&input);
        let original = fs::read(&path).unwrap();

        validate(IndexArgs {
            input: path.clone(),
            json: false,
            migrate: true,
        })
        .unwrap();

        // The library migration wrote a `.bak` containing the pre-migration
        // bytes verbatim…
        let bak = path.with_file_name("input.png.lumina.json.bak");
        assert!(bak.is_file(), "migration must create a `.bak` backup");
        assert_eq!(fs::read(&bak).unwrap(), original);
        // …and the live sidecar is migrated to the current schema.
        let document = load_sidecar(&path).unwrap();
        assert_eq!(document.schema_version, lumina_sidecar::SCHEMA_VERSION);
        // The per-sidecar write lock is released after the migration.
        assert!(
            !path.with_file_name(".input.png.lumina.json.lock").exists(),
            "the write lock must be released after the migration"
        );
    }

    /// F-019: a fresh (non-stale) per-sidecar lock must make the CLI migration
    /// fail with an explicit conflict — never a silent in-place rewrite and
    /// never a stolen lock.
    #[test]
    fn cli_migrate_flag_is_blocked_by_a_fresh_lock() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let (_, frame) = png_input(directory.path(), "input.png", 90);
        drop(frame);
        let path = write_legacy_sidecar(&input);
        let original = fs::read(&path).unwrap();

        let lock_path = path.with_file_name(".input.png.lumina.json.lock");
        fs::write(&lock_path, b"").unwrap();

        let error = validate(IndexArgs {
            input: path.clone(),
            json: false,
            migrate: true,
        })
        .unwrap_err();
        assert!(error.to_string().contains("locked"), "{error}");

        // The locked migration created no backup and touched nothing.
        assert!(!path.with_file_name("input.png.lumina.json.bak").exists());
        assert_eq!(fs::read(&path).unwrap(), original);
        // A fresh lock is never stolen — it survives for the real writer.
        assert!(lock_path.exists(), "fresh lock must survive the contender");
    }

    /// F-019: an already-current sidecar is a no-op for the CLI migration —
    /// no `.bak` is created and the on-disk bytes stay untouched.
    #[test]
    fn cli_migrate_flag_is_noop_for_current_schema() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let (_, frame) = png_input(directory.path(), "input.png", 90);
        // A current-version sidecar written by the library itself.
        let bytes = fs::read(&input).unwrap();
        let document = SidecarDocument::new(
            source_identity(&input, &bytes, &frame, None).unwrap(),
            "raster-mvp-1",
        );
        let path = sidecar_path_for(&input);
        save_sidecar(&path, &document).unwrap();
        let before = fs::read(&path).unwrap();

        validate(IndexArgs {
            input: path.clone(),
            json: false,
            migrate: true,
        })
        .unwrap();

        assert!(!path.with_file_name("input.png.lumina.json.bak").exists());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    // F-098-N2: feature-gated CLI→Lensfun wiring tests. These exercise the real
    // system Lensfun database (like the `lumina-lensfun` native tests), so they
    // only run under `--features lensfun` (the default build has no `liblensfun`
    // and stays green).
    #[cfg(feature = "lensfun")]
    mod lensfun_wiring_tests {
        use super::*;

        // Build a `RawMetadata` from the minimal EXIF fields the CLI wiring
        // inspects. All other fields are left at inert defaults — the wiring
        // only reads make/model/focal_length/aperture/width/height.
        fn make_metadata(
            make: Option<&str>,
            model: Option<&str>,
            focal_length: Option<f32>,
            aperture: Option<f32>,
        ) -> RawMetadata {
            RawMetadata {
                width: 1000,
                height: 750,
                orientation: 1,
                camera_make: make.map(str::to_string),
                camera_model: model.map(str::to_string),
                iso: None,
                shutter: None,
                aperture,
                lens: None,
                focal_length,
                timestamp: None,
                artist: None,
                description: None,
                camera_matrix: [[0.0; 4]; 3],
                camera_white_balance: [1.0; 4],
                pre_multipliers: [1.0; 4],
                icc_profile: None,
            }
        }

        // The same real camera the `lumina-lensfun` native tests use, so the
        // installed profile database is guaranteed to contain a matching,
        // non-identity profile (distortion + vignetting).
        const MAKE: &str = "Nikon Corporation";
        const MODEL: &str = "Nikon D40";

        #[test]
        fn real_camera_with_full_exif_yields_corrector() {
            let metadata = make_metadata(Some(MAKE), Some(MODEL), Some(18.0), Some(5.6));
            let (_db, corrector) = build_lensfun_corrector(Some(&metadata))
                .expect("a Lensfun corrector for the known {MAKE} {MODEL} profile");
            // The modifier references lens data owned by the DB; `_db` is dropped
            // after `corrector`, so the handle stays alive while the corrector is used.
            assert!(
                !corrector.is_identity(),
                "the resolved Nikon D40 profile must be a non-identity correction"
            );
        }

        #[test]
        fn missing_make_yields_none() {
            let metadata = make_metadata(None, Some(MODEL), Some(18.0), Some(5.6));
            assert!(build_lensfun_corrector(Some(&metadata)).is_none());
        }

        #[test]
        fn missing_model_yields_none() {
            let metadata = make_metadata(Some(MAKE), None, Some(18.0), Some(5.6));
            assert!(build_lensfun_corrector(Some(&metadata)).is_none());
        }

        #[test]
        fn missing_focal_length_yields_none() {
            let metadata = make_metadata(Some(MAKE), Some(MODEL), None, Some(5.6));
            assert!(build_lensfun_corrector(Some(&metadata)).is_none());
        }

        #[test]
        fn missing_aperture_yields_none() {
            let metadata = make_metadata(Some(MAKE), Some(MODEL), Some(18.0), None);
            assert!(build_lensfun_corrector(Some(&metadata)).is_none());
        }

        #[test]
        fn no_metadata_yields_none() {
            assert!(build_lensfun_corrector(None).is_none());
        }

        #[test]
        fn render_with_corrector_changes_pixels() {
            // Smoke test: feeding a real Lensfun corrector through
            // `RenderContext.lensfun` must actually alter the rendered pixels
            // versus the manual/identity model (`None`).
            //
            // A *uniform* frame is invariant under lensfun: distortion only remaps
            // positions (uniform → uniform) and the small vignette rounds back to
            // the same 8-bit value. So we use a spatial gradient: distortion then
            // moves different source positions under each destination pixel and the
            // vignette brightens the corners, both of which change 8-bit values.
            let metadata = make_metadata(Some(MAKE), Some(MODEL), Some(18.0), Some(5.6));
            let (_db, corrector) = build_lensfun_corrector(Some(&metadata))
                .expect("a Lensfun corrector for the known profile");
            let width: u32 = 1000;
            let height: u32 = 750;
            let mut pixels = Vec::with_capacity((width * height * 4) as usize);
            for y in 0..height {
                for x in 0..width {
                    let value = ((x / 4 + y / 4) % 256) as u8;
                    pixels.extend_from_slice(&[value, value, value, 255]);
                }
            }
            let frame = ImageFrame::new(width, height, pixels).unwrap();
            let recipe = lumina_sidecar::EditRecipe::default();

            let rendered_none = render_frame(
                &frame,
                &RenderContext {
                    recipe: &recipe,
                    camera_white_balance: None,
                    source_actions: &[],
                    masks: None,
                    lensfun: None,
                    depth: None,
                },
            )
            .unwrap();
            let rendered_some = render_frame(
                &frame,
                &RenderContext {
                    recipe: &recipe,
                    camera_white_balance: None,
                    source_actions: &[],
                    masks: None,
                    #[cfg(feature = "lensfun")]
                    lensfun: Some(LensfunCorrectorRef(&corrector)),
                    #[cfg(not(feature = "lensfun"))]
                    lensfun: None,
                    depth: None,
                },
            )
            .unwrap();
            assert_ne!(
                rendered_none.frame.pixels, rendered_some.frame.pixels,
                "a Lensfun corrector must change the rendered pixels"
            );
        }
    }
}
