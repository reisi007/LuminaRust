use clap::{Args, Parser, Subcommand, ValueEnum};
#[cfg(feature = "lensfun")]
use lumina_core::LensfunCorrectorRef;
use lumina_core::{
    export_image, match_total_exposure_masked, render_frame, resolve_mask_planes,
    suggest_auto_tone, tone_fingerprint, AutoToneConfig, ExportOptions, ImageFileFormat,
    ImageFrame, MaskContext, MaskInference, MaskLoadContext, MaskPlane, MaskPolicy, RenderContext,
    RenderOutput, SourceActionArtifact,
};
use lumina_onnx::{birefnet_manifest, StubBackend};
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
use lumina_gpu::{unsupported_gpu_stages, Frame, GpuContext};
// Visible backend-selection logging (no silent fallback to CPU).
use log::info;
use lumina_sidecar::{
    append_repair_region, artifact_status, load_sidecar, load_zdata, save_sidecar,
    sidecar_path_for, AnalysisFingerprint, ArtifactStatus, DecodeFingerprint, EditRecipe,
    GeometryFingerprint, HistoryEntry, MaskOperation, MaskStatus, Preset, RepairRegionArtifact,
    SidecarDocument, SourceActionArtifactRef, SourceActionKind, SourceActionSpec, SourceIdentity,
    SOURCE_ACTION_VERSION,
};
use rayon::prelude::*;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

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
#[cfg(feature = "gpu")]
thread_local! {
    static GPU_CTX: std::cell::OnceCell<Option<GpuContext>> = const { std::cell::OnceCell::new() };
}

/// Renders `frame` with `recipe`, preferring the GPU when an adapter is bound,
/// otherwise the full platform-neutral CPU pipeline.
///
/// REVIEW-GPU-DIVERGENCE-1: the GPU bootstrap stage implements only white
/// balance + the seven tone sliders. Before routing to the GPU, the render is
/// validated against **both** the recipe ([`unsupported_gpu_stages`]) and the
/// render context (source actions, mask layers, Lensfun corrector — none of
/// which exist on the GPU path). Any unsupported stage routes the whole render
/// explicitly to the CPU pipeline with a once-per-reason-set log line, so
/// GPU-enabled builds always produce the same pixels as CPU builds. The GPU is
/// an accelerator, never a semantic change (Agents.md: no silent fallbacks).
#[cfg(feature = "gpu")]
fn render_best_effort(
    ctx: Option<&GpuContext>,
    frame: &ImageFrame,
    recipe: &EditRecipe,
    render_ctx: &RenderContext<'_>,
) -> Result<RenderOutput, CliError> {
    // Context-level features the GPU path cannot reproduce at all.
    let mut reasons = unsupported_gpu_stages(recipe);
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

    match ctx {
        Some(ctx) if ctx.is_available() && reasons.is_empty() => {
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
    /// F-101-F1: run the Lumina MCP server over stdio (JSON-RPC on
    /// stdin/stdout). Takes no arguments; see `feature/platform/mcp-server.md`.
    #[cfg(feature = "mcp")]
    Mcp,
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
}

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
        },
        args.quality,
        args.virtual_copy.as_deref(),
        args.mask_policy.to_policy(),
        &mut mask_warnings,
    )?;
    emit(
        args.json,
        serde_json::json!({"command":"export", "output":output, "quality":args.quality, "status":"ok", "mask_warnings":mask_warnings}),
        "exported",
    )
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
        save_sidecar(&path, &document)?;
    }
    emit(
        args.json,
        serde_json::json!({"command":"mask", "input":args.input, "updated":args.update_masks, "status":"ok"}),
        "mask status updated",
    )
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
                },
                args.quality,
                args.virtual_copy.as_deref(),
                args.mask_policy.to_policy(),
                &mut mask_warnings,
            ) {
                Ok(()) => {
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

fn migrate_sidecar(path: &Path) -> Result<(), CliError> {
    let json = fs::read_to_string(path).map_err(|error| io_error(path, error))?;
    let migrated = lumina_sidecar::migrate_json(&json)?;
    if migrated != json {
        write_atomically(path, migrated.as_bytes())?;
    }
    Ok(())
}

fn process(args: ProcessArgs) -> Result<(), CliError> {
    // `process` has no explicit quality flag; it uses the shared default (90),
    // which is identical to the historical `frame.encode(format)` output.
    process_selected(args, 90, None, MaskPolicy::Warn, &mut Vec::new())
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
/// # Subject (focus) distance
/// `RawMetadata` carries no subject-distance field, so a documented default of
/// `10.0` (metres) is used. Lensfun vignetting/distortion calibration is in
/// practice focus-distance-independent for the MVP profiles, and `lumina-lensfun`'s
/// own reference tests use exactly this value, so it yields a matching,
/// non-identity corrector for the `Nikon D40` example profile.
///
/// # Known limits (MVP)
/// * The system Lensfun database is loaded once per call (no cross-render
///   cache). Acceptable for the MVP, but repeated `process`/`render` invocations
///   each re-load the DB.
/// * `lens_name` is intentionally `None`: the camera body alone selects a lens
///   profile instead of risking a spurious match on an EXIF lens string. (LibRaw
///   now populates `RawMetadata.lens` from EXIF-LensModel/Makernote, but the
///   corrector deliberately uses body-match via Lensfun `GuessParameters` rather
///   than the EXIF lens name — see REVIEW-RAW-N2.)
/// * CA (transverse chromatic aberration) stays manual — documented F-098-N1
///   limit.
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
        None,
        metadata.width,
        metadata.height,
        focal_length,
        aperture,
        distance,
    )?;
    Some((db, corrector))
}

fn process_selected(
    args: ProcessArgs,
    quality: u8,
    virtual_copy: Option<&str>,
    policy: MaskPolicy,
    mask_warnings_out: &mut Vec<String>,
) -> Result<(), CliError> {
    // REVIEW-CLI-WRITE-1: the guard covers the original itself (path and
    // hard-link identity) plus its `.lumina.json`/`.lumina.zdata` bundle.
    reject_protected_output(&args.input, &args.output)?;
    let format = output_format(&args.output)?;
    let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
    let (frame, raw_metadata) = decode_input(&args.input, &bytes)?;
    let wb = raw_metadata.as_ref().map(|m| m.camera_white_balance);
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
    let zdata_path = lumina_sidecar::zdata_path_for(&args.input);
    let loaded_planes = load_persisted_mask_planes(&document, &zdata_path, mask_warnings_out);

    // Wire the ONNX adapter (StubBackend / BiRefNet descriptor) when available.
    // `None` means no inference engine is installed at all (F-051: the decision
    // layer then relies on cached artifacts or fails clearly).
    let manifest = birefnet_manifest();
    let backend = StubBackend::new(manifest.clone()).ok();
    let (inference, model_identity) = match &backend {
        Some(backend) => (
            Some(backend as &dyn MaskInference),
            Some(manifest.to_model_identity()),
        ),
        None => (None, None),
    };

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
    };
    // Prefer the GPU when an adapter is bound; otherwise the full CPU pipeline.
    // The chosen backend is logged once at startup (see `init_render_backend`).
    // The GPU path currently mirrors the CPU recipe-application bootstrap stub;
    // the complete mask/WB/source-action pipeline runs on the CPU branch.
    #[cfg(feature = "gpu")]
    let render_output = GPU_CTX.with(|cell| {
        let ctx = cell.get_or_init(init_render_backend);
        render_best_effort(ctx.as_ref(), &frame, &recipe, &render_ctx)
    })?;
    #[cfg(not(feature = "gpu"))]
    let render_output = {
        log_backend("render backend: cpu");
        render_best_effort(None, &frame, &recipe, &render_ctx)?
    };
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
    let encoded = if args.match_total_exposure {
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
            },
            options,
        )?
    } else {
        render_output.frame.encode_with_options(options)?
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
    save_sidecar(&sidecar_path, &document)?;
    staged.commit()?;
    Ok(())
}

/// One virtual copy as reported by `inspect` (R2-CLI-03): rendered either as
/// JSON or free text from the same data so the two outputs cannot drift.
struct InspectCopy {
    id: String,
    name: String,
    auto_tone: bool,
    matching: bool,
    target_luminance: f64,
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
    let protected = [
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
        // 7 editing tools + lumina_analyze + 4 F-101-F1 CLI-coverage tools.
        assert_eq!(tools.len(), 12, "tool set drifted; update SOLL + tests");
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
        }
    }

    fn write_sidecar_with_valid_layer(
        input: &Path,
        bytes: &[u8],
        frame: &ImageFrame,
    ) -> lumina_sidecar::SidecarDocument {
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
        }];
        save_sidecar(&sidecar_path_for(input), &document).unwrap();
        document
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
