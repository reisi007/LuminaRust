use clap::{Args, Parser, Subcommand, ValueEnum};
use lumina_core::LensfunCorrectorRef;
use lumina_core::{
    analyze_upright, denoise_producer_provenance, detect_red_eyes, detect_spots_heuristic,
    export_image_with_generative, generative_variant_seed, has_transparent_pixels,
    match_total_exposure_masked, render_frame, render_frame_with_denoise,
    render_frame_with_generative, resolve_denoise_status, resolve_mask_planes,
    set_denoise_producer_provenance, suggest_auto_tone, tone_fingerprint, upright_analysis,
    upright_input_fingerprint, AutoToneConfig, DenoiseIdentity, DenoisePolicy,
    DenoiseRgbArtifact as CoreDenoiseRgbArtifact, DenoiseStageInput, DenoiseStageStatus,
    DetectedRedEye, ExportOptions, GenerativeCacheKey, GenerativeCanvasArtifact,
    GenerativeCanvasInput, GenerativeRole as CoreGenerativeRole, ImageFileFormat, ImageFrame,
    MaskContext, MaskInference, MaskLoadContext, MaskPlane, MaskPolicy, RenderContext,
    RenderOutput, SourceActionArtifact, RED_EYE_DETECT_ID_PREFIX,
};
// F-082-FOLLOWUP: under `onnx-rt` the CLI consumes the resolver surface
// `lumina_onnx::resolve::try_load_onnx_engine` (real engine or a hard error,
// never a stub). The deterministic `StubBackend` stays the wiring default for
// default builds (no `onnx-rt`); it is never substituted for a requested real
// engine. `birefnet_manifest` is the model identity/contract both paths share.
use lumina_onnx::birefnet_manifest;
// GEN-ONNX-1 Welle 1: deterministic generative canvas producer (fixture model)
// and the documented real-model attachment surface.
use lumina_onnx::generative::{
    produce_canvas, GenerativeCanvasOutput, GenerativeModelSource,
    GenerativeRole as OnnxGenerativeRole,
};
// LRPAR-G12-FACE-IMPL-20 / S4: the `face` command resolves the real face
// engine loudly (`try_load_face_engine`) — never a silent stub fallback — and
// evaluates a persisted analysis against the live source/decode/model context.
#[cfg(feature = "onnx-rt")]
use lumina_onnx::try_load_onnx_engine;
#[cfg(feature = "onnx-rt")]
use lumina_onnx::OnnxEngine;
#[cfg(not(feature = "onnx-rt"))]
use lumina_onnx::StubBackend;
#[cfg(feature = "onnx-rt")]
use lumina_onnx::{
    cluster_embeddings, clusters_from_labels, detected_face_id, embedding_id_for,
    FaceAnalysisOutput, FaceDetectionInference, FaceEmbeddingInference, FaceEmbeddingRecord,
};
use lumina_onnx::{
    face_artifact_status, face_identity, try_load_face_engine, FaceArtifactEvidence,
    FaceClusteringParams, FaceInferenceOptions, FaceModelSuite, FaceOnnxEngine,
};
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
use lumina_sidecar::{
    append_repair_region, load_validated_source_action_bundle, load_zdata, zdata_path_for,
    RepairRegionArtifact,
};
// LRPAR-G12-FACE-IMPL-20-REST: the `face_embedding` write path only exists in
// the `onnx-rt` build (the only build that can produce real vectors).
use lumina_sidecar::{
    apply_batch_op, artifact_status, default_meta_presets_dir, document_revision,
    generative_artifact_status, is_metadata_field, load_meta_preset_file, load_sidecar,
    now_rfc3339_utc, render_meta_preset, resolve_meta_preset_path, save_denoise_rgb,
    save_generative_canvas, save_sidecar, save_sidecar_if_unchanged, scan_meta_presets_dir,
    sidecar_path_for, spot_removal_entries, validate_metadata_field_value,
    validate_smart_collection_def, AiSelect, AiSelectKind, AnalysisFingerprint, ArtifactStatus,
    AspectPreset, BatchOp, BokehShape, CollectionMembership, ColorGrading, ColorGradingRange,
    CoordinateSystem, Crop, CurveChannels, CurvePoint, Curves, DecodeFingerprint, DenoiseAi,
    DenoiseArtifactKind, DenoiseArtifactRef, DenoiseModelIdentity,
    DenoiseRgbArtifact as SidecarDenoiseRgbArtifact, DepthArtifactRef, EditRecipe, ExportRecord,
    FaceAnalysis, FaceArtifactStatus, FocusRect, GenerativeArtifactRef, GenerativeArtifactStatus,
    GenerativeCanvas, GenerativeCanvasArtifact as SidecarGenerativeCanvas, GenerativeEdit,
    Geometry, GeometryFingerprint, HistoryEntry, HslAdjustments, HslChannel, LensBlur,
    LensCorrection, MaskDefinition, MaskLayer, MaskOperation, MaskPrompt, MaskReference,
    MaskStatus, MetaPresetEntry, MetaPresetFile, MetadataHistoryEntry, ModelIdentity, Perspective,
    PointColor, PointColorEntry, Preprocessing, Preset, PromptTransform, RecordSpec,
    RedEyeCorrection, RedEyeRegion, Resolution, SidecarDocument, SmartCollectionDef,
    SourceActionArtifactRef, SourceActionKind, SourceActionSpec, SourceFingerprint, SourceIdentity,
    Upright, DENOISE_AI_VERSION, MAX_KEYWORDS_PER_DOCUMENT, MAX_KEYWORD_CHARS,
    MAX_METADATA_HISTORY_ENTRIES, METADATA_FIELD_IDS, RED_EYE_MAX_REGIONS,
    SMART_COLLECTION_VERSION, SOURCE_ACTION_VERSION,
};
#[cfg(feature = "onnx-rt")]
use lumina_sidecar::{save_face_embeddings, FaceEmbeddingArtifact as SidecarFaceEmbeddingArtifact};
// LRPAR-G15-IPTC-S3: embedded IPTC read (JPEG IIM/XMP) for `meta inspect`.
// LRPAR-G15-IPTC-S6: `embed_metadata` for the opt-in JPEG export bake-in.
use lumina_iptc::{embed_metadata, extract_metadata, IptcMetadata};
// LRPAR-G09-CULL-IMPL-25 (CLI slice): deterministic Stage-1 assisted culling.
// The command orchestrates `lumina-cull` (analysis + source-level sidecar
// binding) and never touches rating/flag/label. Reads/writes are explicit.
use lumina_cull::{
    analyze_selection, evaluate_culling, heuristic_identity, CullConfig, CullSourceInput,
    CullingReadState,
};
use rayon::prelude::*;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

// LRPAR-G09-CULL-IMPL-25: live-source persistence guard extracted from the
// oversized CLI entrypoint (file-size ratchet; orchestration remains in main).
mod cull_cli;
// MASK-LOCAL-P0: typed local-adjustment flag parsing and mutation.
mod mask_local;
use mask_local::{mask_copy_mut, require_mask_name, resolve_mask_copy};
// LRPAR-G13-MERGE-15 / MERGE-CLI-1: `merge-hdr` / `merge-pano` commands
// (orchestration; alignment/merge/DNG live in `lumina-merge`).
mod merge;
// LRPAR-MATRIX-RECIPE (Slice 1): multi-recipe matrix runner over the committed
// RAW samples (SOLL: `feature/quality/conflicts-and-acceptance.md`
// § „Rezept-Matrix"). Orchestration only — it renders through the shared
// `render_standard` entry point and contains no second image processing.
mod matrix;
// GPU-LENSFUN-PARITY-1 (F7): CLI-side strict-corrector map bind (ratchet §8).
#[cfg(feature = "gpu")]
mod lensfun_gpu;
// R5-DUST-23-FOLLOWUP: spot-removal mutation ops (add/update/remove-single/
// regenerate-variant/distraction-spec) — extracted for the file-size ratchet.
mod spot_ops;
use spot_ops::{
    display_spot_entries_for_input, parse_distraction_spec, reject_spot_remove_conflicts,
    spot_add_heuristic, spot_regenerate_variant, spot_remove_entry, spot_update_params,
};

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
    static GPU_CTX: std::cell::RefCell<std::mem::ManuallyDrop<Option<GpuContext>>> =
        const { std::cell::RefCell::new(std::mem::ManuallyDrop::new(None)) };
}

/// Which backend the standard render entry actually used, with the loud CPU
/// routing reasons (empty for a GPU render).
///
/// The router computes this decision in exactly one place
/// ([`render_best_effort`]); the LRPAR-MATRIX-RECIPE `--require-gpu` gate
/// consumes the returned value instead of re-deriving or string-matching a log
/// line, so a future routing change can never silently disagree with the gate.
/// The `Gpu` variant only exists in GPU-enabled builds; the accessors below
/// abstract that away so callers never need `#[cfg]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenderRoute {
    #[cfg(feature = "gpu")]
    Gpu,
    Cpu {
        reasons: Vec<String>,
    },
}

impl RenderRoute {
    /// `true` when the render ran on the GPU.
    pub(crate) fn is_gpu(&self) -> bool {
        match self {
            #[cfg(feature = "gpu")]
            RenderRoute::Gpu => true,
            RenderRoute::Cpu { .. } => false,
        }
    }

    /// Short route label for reports (`"gpu"`/`"cpu"`).
    pub(crate) fn label(&self) -> &'static str {
        if self.is_gpu() {
            "gpu"
        } else {
            "cpu"
        }
    }

    /// CPU routing reasons (empty for a GPU render).
    pub(crate) fn reasons(&self) -> &[String] {
        match self {
            #[cfg(feature = "gpu")]
            RenderRoute::Gpu => &[],
            RenderRoute::Cpu { reasons } => reasons,
        }
    }
}

/// Renders `frame` with `recipe`, preferring the GPU when an adapter is bound,
/// otherwise the full platform-neutral CPU pipeline. Returns the actual route
/// alongside the result so callers can assert parity (see [`RenderRoute`]).
///
/// REVIEW-GPU-DIVERGENCE-1 / CAMERA-WB-WELLE: the GPU path implements the full
/// adjustment/geometry chain. Before routing to the GPU, the render is validated
/// against **both** the recipe and the render context ([`gpu_routing_reasons`] —
/// e.g. an **invalid** decoder As-Shot WB context, R2-MCP-01, and
/// touched-but-reset sliders at their neutral value, R2-GPU-05). A valid As-Shot
/// context is carried into the GPU entry via
/// [`GpuContext::set_camera_white_balance`], and a strict Lensfun corrector's
/// map via [`lensfun_gpu::bind`]. Any unsupported stage routes the whole render
/// explicitly to the CPU pipeline with a once-per-reason-set log line, so
/// GPU-enabled builds always produce the same pixels as CPU builds. The GPU is
/// an accelerator, never a semantic change (Agents.md: no silent fallbacks).
#[cfg(feature = "gpu")]
fn render_best_effort(
    ctx: Option<&mut GpuContext>,
    frame: &ImageFrame,
    recipe: &EditRecipe,
    render_ctx: &RenderContext<'_>,
    generative: GenerativeCanvasInput<'_>,
) -> Result<(RenderOutput, RenderRoute), CliError> {
    let mut reasons = gpu_routing_reasons(recipe, render_ctx);

    match ctx {
        Some(ctx) if ctx.is_available() && reasons.is_empty() => {
            // GPU-LENSFUN-PARITY-1 (F7): build/bind the strict corrector's map
            // for this source/dimensions before the GPU entry. An unbound map or
            // a distortion corrector without an explicit crop keeps the exact CPU
            // reference and surfaces the reason — never a silent corrector loss.
            if let Some(reason) = lensfun_gpu::bind(ctx, render_ctx, frame.width, frame.height) {
                reasons.push(reason.to_string());
                lumina_gpu::log_cpu_routing_once(&reasons, "cli render");
                let output = render_frame_with_generative(frame, render_ctx, generative)
                    .map_err(|error| CliError::Message(error.to_string()))?;
                return Ok((output, RenderRoute::Cpu { reasons }));
            }
            // CAMERA-WB-WELLE (R2-MCP-01): carry the decoder As-Shot context into
            // the GPU entry like the Lensfun corrector / depth plane. The gains
            // are validated there with the oracle's own error but never
            // re-applied (the decoder already multiplied them in), so a valid
            // context renders byte-identically to the CPU reference. The gate
            // already flags invalid gains; this branch is the belt-and-braces
            // entry validation and falls back loudly (never silently).
            if let Err(error) = ctx.set_camera_white_balance(render_ctx.camera_white_balance) {
                let reasons = vec![format!("camera_white_balance ({error})")];
                lumina_gpu::log_cpu_routing_once(&reasons, "cli render");
                let output = render_frame_with_generative(frame, render_ctx, generative)
                    .map_err(|error| CliError::Message(error.to_string()))?;
                return Ok((output, RenderRoute::Cpu { reasons }));
            }
            // GEN-ONNX-1 Welle 2a: the generative artifact compositing runs on
            // the GPU (mid-geometry `Substitute` steps) — no blanket CPU route.
            let frame = ctx
                .render_with_gpu_and_generative(frame, recipe, &generative)
                .map(Frame::to_image_frame)
                .map_err(|error| CliError::Message(error.to_string()))?;
            Ok((
                RenderOutput {
                    // MASK-LOCAL-P1.1: the CLI has no local-WB picker, so it
                    // never asks for (and never pays for) the pre-local stage.
                    effective_source_stage: None,
                    frame,
                    mask_layers: Vec::new(),
                    mask_warnings: Vec::new(),
                },
                RenderRoute::Gpu,
            ))
        }
        _ => {
            if !reasons.is_empty() {
                lumina_gpu::log_cpu_routing_once(&reasons, "cli render");
            }
            let output = render_frame_with_generative(frame, render_ctx, generative)
                .map_err(|error| CliError::Message(error.to_string()))?;
            Ok((output, RenderRoute::Cpu { reasons }))
        }
    }
}

/// Recipe- and context-level reasons that force CPU rendering in
/// [`render_best_effort`]. Pure decision logic so tests can pin the routing
/// contract without a GPU adapter.
///
/// Context-level features the GPU path cannot reproduce here: an **invalid**
/// decoder As-Shot white balance (R2-MCP-01; valid gains are carried into the
/// GPU entry and are pixel-neutral) and source-action artifacts / mask layers.
/// The Lensfun corrector is **not** a static reason: [`lensfun_gpu::bind`]
/// builds/binds its map and reports only an unbound map or a distortion
/// corrector without an explicit crop as a loud CPU route.
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
    reasons
}

/// Non-GPU build: only the CPU pipeline exists, so this is a thin alias to
/// [`render_frame_with_generative`] returning the (always CPU) route.
#[cfg(not(feature = "gpu"))]
fn render_best_effort(
    _ctx: Option<()>,
    frame: &ImageFrame,
    _recipe: &EditRecipe,
    render_ctx: &RenderContext<'_>,
    generative: GenerativeCanvasInput<'_>,
) -> Result<(RenderOutput, RenderRoute), CliError> {
    let output = render_frame_with_generative(frame, render_ctx, generative)
        .map_err(|error| CliError::Message(error.to_string()))?;
    Ok((
        output,
        RenderRoute::Cpu {
            reasons: Vec::new(),
        },
    ))
}

/// GEN-ONNX-1: [`render_standard_routed`] with caller-supplied generative
/// canvas artifacts (artifact compositing on the CPU oracle path).
fn render_standard_with_generative(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    render_ctx: &RenderContext<'_>,
    generative: GenerativeCanvasInput<'_>,
) -> Result<RenderOutput, CliError> {
    render_standard_routed_with_generative(frame, recipe, render_ctx, generative)
        .map(|(output, _route)| output)
}

/// [`render_standard`] returning the actual [`RenderRoute`] as well. The
/// matrix's `--require-gpu` gate uses it to prove GPU parity from the **same**
/// decision that produced the pixels (never a re-derived or log-scraped guess).
pub(crate) fn render_standard_routed(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    render_ctx: &RenderContext<'_>,
) -> Result<(RenderOutput, RenderRoute), CliError> {
    render_standard_routed_with_generative(
        frame,
        recipe,
        render_ctx,
        GenerativeCanvasInput::default(),
    )
}

/// GEN-ONNX-1: [`render_standard_routed`] with generative canvas artifacts.
pub(crate) fn render_standard_routed_with_generative(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    render_ctx: &RenderContext<'_>,
    generative: GenerativeCanvasInput<'_>,
) -> Result<(RenderOutput, RenderRoute), CliError> {
    #[cfg(feature = "gpu")]
    {
        GPU_CTX.with(|cell| {
            let mut gpu = cell.borrow_mut();
            if gpu.is_none() {
                **gpu = init_render_backend();
            }
            match gpu.as_mut() {
                Some(ctx) => render_best_effort(Some(ctx), frame, recipe, render_ctx, generative),
                None => render_best_effort(None, frame, recipe, render_ctx, generative),
            }
        })
    }
    #[cfg(not(feature = "gpu"))]
    {
        log_backend("render backend: cpu");
        render_best_effort(None, frame, recipe, render_ctx, generative)
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
    /// LRPAR-G06-UPRIGHT-15 (Release 1.5): automatic upright analysis as a
    /// persisted recipe stage. `--analyze` computes the deterministic,
    /// model-free line analysis of the decoded source and persists it with a
    /// source fingerprint; `--enable`/`--disable` toggle whether the persisted
    /// suggestion supplies the effective F-099 perspective (Lightroom
    /// semantics: the manual perspective stays persisted and returns when
    /// disabled); `--clear` removes the stage. Reads are loud (unknown copies,
    /// stale fingerprints are reported, never silently recomputed). See
    /// `feature/architecture/pipeline.md` § F-099.
    Upright(UprightArgs),
    /// G-14 Rote Augen (LRPAR-G14-REDEYE-15/‑AUTO-15, Release 1.5/2.0):
    /// inspect and edit the persisted red-eye regions of one virtual copy.
    /// Region marking is explicit (`--set ID:x,y,radius,desaturate,darken`);
    /// LRPAR-G14-REDEYE-AUTO-15 adds the deterministic, model-free pupil
    /// detection (`--detect` lists candidates read-only, `--detect-apply`
    /// persists them explicitly — never implicit). The correction itself is
    /// the deterministic, model-free G-14 formula, applied on CPU and GPU with
    /// oracle parity. See `feature/architecture/pipeline.md` § G-14.
    RedEye(RedEyeArgs),
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
    /// GEN-ONNX-1 (Welle 1): produce/inspect the persisted `generative_canvas`
    /// artifact of one virtual copy (local ONNX fixture model; expands or
    /// auto-fills). Loud on a missing model/canvas, a stale identity or a
    /// corrupt bundle. See `feature/product/generative-expand.md`.
    Generative(GenerativeArgs),
    /// LRPAR-G14-DENOISE-IMPL-20 (CLI slice): inspect the KI-Denoise stage
    /// status, render through the denoise-aware pipeline, or explicitly record
    /// an externally produced `denoise_rgb` artifact. Never auto-recomputes and
    /// never silently falls back (default `Warn` surfaces the manual F-096
    /// fallback, `--denoise-policy strict` aborts). See
    /// `feature/decisions/LRPAR-G14-DENOISE-20.md` §6 and
    /// `feature/architecture/pipeline.md` §F-096a.
    Denoise(DenoiseArgs),
    /// LRPAR-G09-CULL-IMPL-25 (CLI slice): explicit Stage-1 assisted-culling
    /// analysis over a caller-chosen selection (or read-only status). Persists
    /// only the source-level `culling` proposal — never rating/flag/label.
    /// See `feature/decisions/LRPAR-G09-CULL-25.md` §2/§5/§7.3.
    Cull(CullArgs),
    /// LRPAR-G12-FACE-IMPL-20 / S4: inspect the persisted source-level face
    /// analysis or run the real face engine loudly. With `onnx-rt` and
    /// verified artifacts the analysis chain is persisted (sidecar-first);
    /// without the capability the command refuses loudly (no stub fallback).
    /// See `feature/decisions/LRPAR-G12-FACE-20.md` §2.3/§4/§6.
    Face(FaceArgs),
    /// GUI-GEN-GRANULAR-10 (F-100, Release 1.0): explicit per-module
    /// regeneration of the 1.0 derivable AI/analysis values (AI masks,
    /// Auto-Tone, Exposure Matching). No `--module` is the collective default
    /// (`alle veralteten/fehlenden neu generieren`); an explicit module forces
    /// exactly that value and leaves every other artifact untouched. Never
    /// implicit. See `feature/platform/cli-gui-wasm.md` § F-100.
    Regenerate(RegenerateArgs),
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

/// GUI-GEN-GRANULAR-10 (F-100, Release 1.0): the derivable AI/analysis values
/// present in 1.0 that each have their own explicit regeneration action. The
/// list is the `--module` value set of `lumina regenerate`; later modules
/// (denoise/face/cull/merge) extend it without changing the convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum RegenerateModule {
    /// AI mask inference. Regeneration is an explicit refresh request
    /// (status `Pending`) consumed by the next render; the `.lumina.zdata`
    /// artifact persistence is the documented F-082 open item, so no stub
    /// matte is ever persisted as a valid artifact.
    Masks,
    /// Auto-Tone: the six sliders (`exposure`/`contrast` plus the AUTO-TONE-2
    /// end/balance mirrors) and the `analysis_fingerprint`.
    #[value(name = "auto-tone")]
    AutoTone,
    /// F-008 Exposure Matching (the persisted `matched_exposure`).
    Matching,
}

impl RegenerateModule {
    fn as_str(self) -> &'static str {
        match self {
            Self::Masks => "masks",
            Self::AutoTone => "auto-tone",
            Self::Matching => "matching",
        }
    }
}

/// GUI-GEN-GRANULAR-10 (F-100): explicit per-module regeneration of the 1.0
/// derivable AI/analysis values.
///
/// Without `--module` the command is the F-100 **collective default** and
/// regenerates only stale or missing values (independent per module). An
/// explicit `--module` **forces** exactly that module, even when its current
/// value looks fresh, and leaves every other persisted artifact untouched.
/// Nothing is ever recomputed implicitly; the sidecar is written atomically
/// and only when something actually changed.
#[derive(Debug, Args)]
struct RegenerateArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    virtual_copy: Option<String>,
    /// Modules to regenerate (repeatable): `masks`, `auto-tone`, `matching`.
    /// Omitted = regenerate every stale/missing module.
    #[arg(long = "module", value_enum)]
    modules: Vec<RegenerateModule>,
    #[arg(long, default_value_t = 0.5)]
    target_luminance: f64,
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
    /// Target layer for the P0 local-adjustment flags. If omitted, a copy with
    /// exactly one layer is accepted; ambiguous copies fail loudly.
    #[arg(long, value_name = "LAYER")]
    local_layer: Option<String>,
    /// Set one local control (`exposure|contrast|highlights|shadows|
    /// temperature_delta_k|tint_delta=value`). Repeatable; all values are
    /// validated before the sidecar is written.
    #[arg(long = "set-local-adjustment", value_name = "KEY=VALUE")]
    set_local_adjustments: Vec<String>,
    /// Reset one local control to zero. Repeatable.
    #[arg(long = "reset-local-adjustment", value_name = "KEY")]
    reset_local_adjustments: Vec<String>,
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
/// isolated and loud). Non-neutral local-mask state is refused rather than
/// silently omitted or copied; no-local-mask recipes retain the historical
/// copy path.
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
    /// Select one spot by id for the `--set-*` updates below (requires at
    /// least one `--set-*`; heuristic entries only).
    #[arg(long, value_name = "ID")]
    spot_id: Option<String>,
    /// Update the selected heuristic spot's radius (`(0,512]`).
    #[arg(long, value_name = "(0,512]")]
    set_radius: Option<f32>,
    /// Update the selected heuristic spot's feather (`0..=1`).
    #[arg(long, value_name = "0..=1")]
    set_feather: Option<f32>,
    /// Update the selected heuristic spot's opacity (`0..=1`).
    #[arg(long, value_name = "0..=1")]
    set_opacity: Option<f32>,
    /// Update the selected heuristic spot's source-offset x value (`-1..=1`).
    #[arg(long, value_name = "-1..=1")]
    set_offset_dx: Option<f32>,
    /// Update the selected heuristic spot's source-offset y value (`-1..=1`).
    #[arg(long, value_name = "-1..=1")]
    set_offset_dy: Option<f32>,
    /// Remove one spot entry by id (loud on unknown ids).
    #[arg(long, value_name = "ID")]
    remove_spot: Option<String>,
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
    /// Regenerate the generative spot identified by `<ID>`: sets
    /// `seed = variant_seed(base, variant)`.
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

/// LRPAR-G06-UPRIGHT-15 (Release 1.5): inspect and edit the persisted
/// automatic upright analysis of one virtual copy. The analysis is classic,
/// model-free and deterministic; `--analyze` binds it to the current source
/// identity (`upright_input_fingerprint`), so a changed source is reported as
/// stale, never silently recomputed. Every mutation validates loudly and
/// appends exactly one history entry. See `feature/architecture/pipeline.md`
/// § F-099.
#[derive(Debug, Clone, Args)]
struct UprightArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    virtual_copy: Option<String>,
    #[arg(long)]
    json: bool,
    /// List the persisted upright stage and whether its fingerprint is still
    /// fresh for the current source (read-only; the default).
    #[arg(long)]
    list: bool,
    /// Run the deterministic `upright-lines-v1` analysis on the decoded source
    /// and persist it (fingerprint bound to the source identity). Enables the
    /// stage in the same step (use `--disable` to keep a suggestion without
    /// applying it).
    #[arg(long)]
    analyze: bool,
    /// Apply the persisted analysis as the effective F-099 perspective.
    #[arg(long)]
    enable: bool,
    /// Stop applying the persisted analysis; the manual perspective returns.
    #[arg(long)]
    disable: bool,
    /// Remove the whole upright stage (analysis included).
    #[arg(long)]
    clear: bool,
}

#[derive(Debug, Args)]
struct RedEyeArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    virtual_copy: Option<String>,
    #[arg(long)]
    json: bool,
    /// List the persisted red-eye regions (read-only; the default).
    #[arg(long)]
    list: bool,
    /// Mark one region as `ID:x,y,radius,desaturate,darken` (normalized
    /// coordinates; repeatable). Regions are identified by their stable id,
    /// never by list position; an existing id is replaced loudly.
    #[arg(long, value_name = "ID:X,Y,RADIUS,DESAT,DARKEN")]
    set: Vec<String>,
    /// Remove regions by id (comma-separated, repeatable).
    #[arg(long, value_delimiter = ',')]
    remove: Vec<String>,
    /// Remove the whole red-eye stage (identity).
    #[arg(long)]
    clear: bool,
    /// LRPAR-G14-REDEYE-AUTO-15: run the deterministic, model-free pupil
    /// detection on the decoded source pixels and list the candidates. This is
    /// read-only; nothing is persisted until `--detect-apply` is passed.
    #[arg(long)]
    detect: bool,
    /// Persist the candidates found by `--detect` (`auto-re-` regions).
    /// Requires `--detect`; replaces only previously auto-detected regions and
    /// leaves manual regions untouched. Loud when the 32-region cap would be
    /// exceeded (no silent truncation).
    #[arg(long)]
    detect_apply: bool,
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
    /// LRPAR-G09-CULL-IMPL-25: a culling analysis/sidecar failure. Loud, never
    /// a guessed proposal.
    #[error(transparent)]
    Cull(#[from] lumina_cull::CullError),
    #[error("invalid preset JSON: {0}")]
    Preset(String),
    /// R2-CLI-07: a usage error detected after clap parsing (contradictory or
    /// mutually exclusive flags, e.g. multiple KI-Denoise actions). Mirrors
    /// clap's own usage exit code (2) instead of the runtime-failure code (1),
    /// so scripts can tell "wrong invocation" from "the run failed".
    #[error("{0}")]
    Usage(String),
    /// R2-CLI-07: at least one batch item failed while the run itself stayed
    /// structurally sound (summary/status files complete). Distinct process
    /// exit code so scripts can distinguish "nothing worked" (1) from
    /// "partial success" (3); see the exit-code table in
    /// `feature/platform/cli-gui-wasm.md`.
    #[error("batch finished with {failed} failed item(s)")]
    BatchPartial { failed: usize },
    /// Generalized partial-failure exit (3) for multi-item commands that are
    /// not `batch` but follow the same isolation contract (`cull`).
    #[error("{command} finished with {failed} failed item(s)")]
    Partial {
        command: &'static str,
        failed: usize,
    },
}

impl CliError {
    /// Process exit code for this error (R2-CLI-07): 1 for every runtime
    /// failure, 2 for a usage error detected after clap parsing, 3 for a
    /// partially failed batch/multi-item run. CLI usage errors that clap
    /// itself catches exit with 2 before `run` is ever reached. Documented in
    /// `feature/platform/cli-gui-wasm.md`.
    fn exit_code(&self) -> i32 {
        match self {
            CliError::Usage(_) => 2,
            CliError::BatchPartial { .. } | CliError::Partial { .. } => 3,
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
        Command::Upright(args) => upright(args),
        Command::RedEye(args) => red_eye(args),
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
        Command::Generative(args) => generative(args),
        Command::Denoise(args) => denoise(args),
        Command::Cull(args) => cull(args),
        Command::Face(args) => face(args),
        Command::Regenerate(args) => regenerate(args),
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

/// GUI-GEN-GRANULAR-10 (F-100): does this source mask need a fresh artifact?
///
/// Conservative pre-filter for the *collective* default only; an explicit
/// `--module masks` forces the refresh regardless (range prompts excluded by
/// the caller, see `regenerate`). It checks the cheap, CLI-owned parts of the
/// decision-layer validity rule (status, source content hash, artifact
/// availability). The **authoritative** identity check stays in
/// `lumina-core::mask_loader` — decode context, model identity and plane
/// dimensions are deliberately *not* duplicated here, because that module is
/// the single owner of the inference contract. A fresh-looking value that is
/// actually stale on those dimensions is still caught (and reported loudly) by
/// the decision layer at render time.
fn mask_needs_regeneration(
    root: &Path,
    current: &lumina_sidecar::SourceIdentity,
    mask: &lumina_sidecar::MaskDefinition,
) -> bool {
    if !matches!(mask.status, MaskStatus::Valid) {
        return true;
    }
    if mask.source_fingerprint.content_hash != current.content_hash {
        return true;
    }
    match mask.artifact.as_ref() {
        None => true,
        Some(artifact) => artifact_status(root, artifact) != ArtifactStatus::Available,
    }
}

/// The six AUTO-TONE-2 sliders (adjustment keys) written by Auto-Tone. Shared
/// by the writer ([`apply_auto_tone_result`]) and the freshness predicate of
/// `regenerate`, so a value written by either path is recognized as complete.
const AUTO_TONE_ADJUSTMENT_KEYS: [&str; 6] = [
    "exposure",
    "contrast",
    "whites",
    "blacks",
    "highlights",
    "shadows",
];

/// Writes the full AUTO-TONE-2 result into `recipe`: six sliders plus the six
/// `auto_features` mirrors and the analysis fingerprint. Single source of
/// truth for `lumina regenerate --module auto-tone` (the GUI writes the same
/// six-slider contract in `LuminaApp::auto_tone`).
fn apply_auto_tone_result(
    recipe: &mut EditRecipe,
    frame: &ImageFrame,
    target_luminance: f64,
) -> Result<(), CliError> {
    let config = AutoToneConfig {
        target_luminance,
        ..Default::default()
    };
    let input_fingerprint = tone_fingerprint(frame, config);
    let result = suggest_auto_tone(frame, config)?;
    for (key, value) in [
        (AUTO_TONE_ADJUSTMENT_KEYS[0], result.exposure),
        (AUTO_TONE_ADJUSTMENT_KEYS[1], result.contrast),
        (AUTO_TONE_ADJUSTMENT_KEYS[2], result.whites),
        (AUTO_TONE_ADJUSTMENT_KEYS[3], result.blacks),
        (AUTO_TONE_ADJUSTMENT_KEYS[4], result.highlights),
        (AUTO_TONE_ADJUSTMENT_KEYS[5], result.shadows),
    ] {
        recipe.adjustments.insert(key.into(), value);
    }
    recipe.auto_features.enable_auto_tone = true;
    recipe.auto_features.target_luminance = target_luminance;
    recipe.auto_features.auto_exposure = Some(result.exposure);
    recipe.auto_features.auto_contrast = Some(result.contrast);
    recipe.auto_features.auto_whites = Some(result.whites);
    recipe.auto_features.auto_blacks = Some(result.blacks);
    recipe.auto_features.auto_highlights = Some(result.highlights);
    recipe.auto_features.auto_shadows = Some(result.shadows);
    recipe.auto_features.analysis_fingerprint = Some(AnalysisFingerprint {
        algorithm: "tone-rgba8-rec709".into(),
        version: "1".into(),
        input_fingerprint,
        extras: BTreeMap::new(),
    });
    Ok(())
}

/// GUI-GEN-GRANULAR-10: explicit per-module regeneration of the 1.0 derivable
/// AI/analysis values (F-100). See [`RegenerateArgs`] and
/// `feature/platform/cli-gui-wasm.md` § F-100.
///
/// The three modules are handled in dependency order (masks, auto-tone,
/// matching) and each one is independent: a single-module call never touches
/// another module's persisted state, the collective call only touches
/// stale/missing values, and nothing is recomputed without an explicit request.
fn regenerate(args: RegenerateArgs) -> Result<(), CliError> {
    if !args.target_luminance.is_finite() || !(0.0..=1.0).contains(&args.target_luminance) {
        return Err(CliError::Message(
            "invalid target-luminance: must be finite and in 0..=1".into(),
        ));
    }
    let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
    let (frame, raw_metadata) = decode_input(&args.input, &bytes)?;
    let wb = raw_metadata.as_ref().and_then(|m| {
        let sanitized = sanitize_camera_white_balance(m.camera_white_balance);
        if sanitized.is_none() {
            eprintln!(
                "lumina: warning: As-Shot white balance invalid {:?} for `{}` — dropping to None (recipe WB remains, image renders)",
                m.camera_white_balance,
                args.input.display()
            );
        }
        sanitized
    });
    #[cfg(feature = "lensfun")]
    let (_lensfun_db, lensfun_corrector) = build_lensfun_corrector(raw_metadata.as_ref())
        .map(|(db, corrector)| (Some(db), Some(corrector)))
        .unwrap_or((None, None));
    let sidecar_path = sidecar_path_for(&args.input);
    // Current source identity, reused for a freshly created sidecar and for
    // the mask stale pre-filter (N1).
    let current_identity = source_identity(&args.input, &bytes, &frame, raw_metadata.as_ref())?;
    let mut document = match load_sidecar(&sidecar_path) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            SidecarDocument::new(current_identity.clone(), "raster-mvp-1")
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
    let copy_id = document.virtual_copies[copy_index].id.clone();
    // `all` = collective default (no `--module`); otherwise only the named
    // modules run, and those are forced (the explicit user request).
    let collective = args.modules.is_empty();
    let selected = |module: RegenerateModule| args.modules.contains(&module);
    let wants = |module: RegenerateModule| collective || selected(module);
    let forced = |module: RegenerateModule| selected(module);
    let mut changed = false;
    let mut report: Vec<serde_json::Value> = Vec::new();

    // ---- Module `masks` ----
    if wants(RegenerateModule::Masks) {
        let root = args.input.parent().unwrap_or_else(|| Path::new("."));
        let mask_ids: Vec<String> = document.virtual_copies[copy_index]
            .mask_library
            .iter()
            // N2: deterministic range prompts carry no artifact and are always
            // reproducible from pixels — they are excluded even by the forced
            // `--module masks` path (a refresh request for them is meaningless).
            .filter(|mask| {
                matches!(mask.operation, MaskOperation::Source)
                    && !lumina_core::range_masks::is_range_prompt(mask.prompt.as_ref())
            })
            .filter(|mask| {
                forced(RegenerateModule::Masks)
                    || mask_needs_regeneration(root, &current_identity, mask)
            })
            .map(|mask| mask.id.clone())
            .collect();
        if mask_ids.is_empty() {
            report.push(serde_json::json!({
                "module": RegenerateModule::Masks.as_str(),
                "action": "skipped",
                "reason": if forced(RegenerateModule::Masks) {
                    "no source masks"
                } else {
                    "fresh"
                },
            }));
        } else {
            let mut masks_changed = false;
            for mask in document.virtual_copies[copy_index].mask_library.iter_mut() {
                if mask_ids.contains(&mask.id) && mask.status != MaskStatus::Pending {
                    // An explicit, persisted refresh request. The actual
                    // re-inference is consumed by the next render; the zdata
                    // artifact persistence is the documented F-082 open item,
                    // so no stub matte is ever written as a valid artifact.
                    mask.status = MaskStatus::Pending;
                    masks_changed = true;
                }
            }
            // M2: an explicit `--module masks` additionally arms the copy-wide
            // ONE-SHOT refresh in the recipe so the next render's decision layer
            // re-infers the complete module with `refresh == true` and does NOT
            // report the deliberately requested work as an implicit
            // re-inference. `process_selected` consumes and removes the flag
            // after the successful render (REVIEW-CLI-MASKFLAG-1).
            //
            // M2b: the collective default deliberately does NOT arm it. The
            // copy-wide flag overrides the persisted-valid fastpath for *every*
            // reachable source mask, so arming it here would also re-infer fresh
            // `Valid` masks — contradicting "nur veraltete oder fehlende" and
            // the GUI (`regenerate_stale` marks only stale masks). The `Pending`
            // markers set above are the per-mask refresh request; the decision
            // layer treats a persisted `Pending` marker as explicitly requested
            // and stays quiet for it.
            if forced(RegenerateModule::Masks) {
                let options = &mut document.virtual_copies[copy_index].recipe.options;
                if options.get("update_masks").map(String::as_str) != Some("true") {
                    options.insert("update_masks".into(), "true".into());
                    masks_changed = true;
                }
            }
            // An already-outstanding `Pending` request is idempotent: the
            // sidecar is not rewritten when nothing transitions.
            changed |= masks_changed;
            report.push(serde_json::json!({
                "module": RegenerateModule::Masks.as_str(),
                "action": "requested",
                "reason": if forced(RegenerateModule::Masks) { "explicit" } else { "stale-or-missing" },
                "masks": mask_ids,
            }));
        }
    }

    // ---- Module `auto-tone` ----
    if wants(RegenerateModule::AutoTone) {
        let current = &document.virtual_copies[copy_index].recipe.auto_features;
        let config = AutoToneConfig {
            target_luminance: args.target_luminance,
            ..Default::default()
        };
        let fingerprint = tone_fingerprint(&frame, config);
        // Fresh = enabled AND the *full* AUTO-TONE-2 contract is persisted:
        // all six sliders, all six `auto_features` mirrors and the analysis
        // fingerprint. A recipe that only carries the historic
        // `process --auto-tone` two-slider subset (exposure/contrast without
        // the end/balance mirrors) is therefore stale and regenerated by the
        // collective default.
        let adjustments = &document.virtual_copies[copy_index].recipe.adjustments;
        let fresh = current.enable_auto_tone
            && current.auto_exposure.is_some()
            && current.auto_contrast.is_some()
            && current.auto_whites.is_some()
            && current.auto_blacks.is_some()
            && current.auto_highlights.is_some()
            && current.auto_shadows.is_some()
            && AUTO_TONE_ADJUSTMENT_KEYS
                .iter()
                .all(|key| adjustments.contains_key(*key))
            && current.analysis_fingerprint.as_ref().is_some_and(|f| {
                f.algorithm == "tone-rgba8-rec709" && f.input_fingerprint == fingerprint
            });
        if fresh && !forced(RegenerateModule::AutoTone) {
            report.push(serde_json::json!({
                "module": RegenerateModule::AutoTone.as_str(),
                "action": "skipped",
                "reason": "fresh",
            }));
        } else if !current.enable_auto_tone && !forced(RegenerateModule::AutoTone) {
            // Deliberately disabled by the user: the collective default never
            // turns it on behind their back.
            report.push(serde_json::json!({
                "module": RegenerateModule::AutoTone.as_str(),
                "action": "skipped",
                "reason": "not-enabled",
            }));
        } else {
            let mut recipe = document.virtual_copies[copy_index].recipe.clone();
            apply_auto_tone_result(&mut recipe, &frame, args.target_luminance)?;
            document.virtual_copies[copy_index].recipe = recipe;
            changed = true;
            report.push(serde_json::json!({
                "module": RegenerateModule::AutoTone.as_str(),
                "action": "generated",
                "reason": if forced(RegenerateModule::AutoTone) { "explicit" } else { "stale-or-missing" },
            }));
        }
    }

    // ---- Module `matching` ----
    if wants(RegenerateModule::Matching) {
        let current = &document.virtual_copies[copy_index].recipe.auto_features;
        let fresh = current.match_total_exposure && current.matched_exposure.is_some();
        if fresh && !forced(RegenerateModule::Matching) {
            report.push(serde_json::json!({
                "module": RegenerateModule::Matching.as_str(),
                "action": "skipped",
                "reason": "fresh",
            }));
        } else if !current.match_total_exposure && !forced(RegenerateModule::Matching) {
            report.push(serde_json::json!({
                "module": RegenerateModule::Matching.as_str(),
                "action": "skipped",
                "reason": "not-enabled",
            }));
        } else {
            let mut recipe = document.virtual_copies[copy_index].recipe.clone();
            let zdata_path = zdata_path_for(&args.input);
            let mut ignored_warnings = Vec::new();
            // Deliberately no mask decision layer here: regenerating the
            // matching value must not re-infer masks (that is the `masks`
            // module). All persisted planes are handed to the render, but
            // `MaskContext`/`render_frame` evaluates only `MaskStatus::Valid`
            // definitions, so stale/missing masks are skipped (with a
            // `MaskPolicy::Warn` note) instead of being recomputed.
            let loaded_planes =
                load_persisted_mask_planes(&document, &zdata_path, &mut ignored_warnings);
            let source_actions = resolve_source_actions(&recipe, &zdata_path)?;
            let copies = document.virtual_copies.clone();
            let render_ctx = RenderContext {
                recipe: &recipe,
                camera_white_balance: wb,
                source_actions: &source_actions,
                masks: Some(MaskContext {
                    copies: &copies,
                    active_copy_id: &copy_id,
                    planes: loaded_planes,
                    policy: MaskPolicy::Warn,
                    source_roi: None,
                }),
                depth: None,
                #[cfg(feature = "lensfun")]
                lensfun: lensfun_corrector.as_ref().map(LensfunCorrectorRef),
                #[cfg(not(feature = "lensfun"))]
                lensfun: None,
            };
            let output = render_standard_with_generative(
                &frame,
                &recipe,
                &render_ctx,
                GenerativeCanvasInput::default(),
            )?;
            let mask_planes: Vec<MaskPlane> = output
                .mask_layers
                .iter()
                .map(|layer| layer.plane.clone())
                .collect();
            let matching =
                match_total_exposure_masked(&output.frame, args.target_luminance, &mask_planes)?;
            recipe.auto_features.match_total_exposure = true;
            recipe.auto_features.target_luminance = args.target_luminance;
            recipe.auto_features.matched_exposure = Some(matching);
            let total_exposure = (recipe.adjustments.get("exposure").copied().unwrap_or(0.0)
                + matching)
                .clamp(-10.0, 10.0);
            recipe.adjustments.insert("exposure".into(), total_exposure);
            document.virtual_copies[copy_index].recipe = recipe;
            changed = true;
            report.push(serde_json::json!({
                "module": RegenerateModule::Matching.as_str(),
                "action": "generated",
                "reason": if forced(RegenerateModule::Matching) { "explicit" } else { "stale-or-missing" },
                "matched_exposure": matching,
            }));
        }
    }

    if changed {
        document.validate()?;
        save_sidecar(&sidecar_path, &document)?;
    }
    let text = report
        .iter()
        .map(|entry| {
            format!(
                "{}={}",
                entry["module"].as_str().unwrap_or("?"),
                entry["action"].as_str().unwrap_or("?")
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    info!(
        "regenerate: copy `{copy_id}` {} [{text}]",
        if changed { "updated" } else { "unchanged" }
    );
    emit(
        args.json,
        serde_json::json!({
            "command": "regenerate",
            "input": args.input,
            "virtual_copy": copy_id,
            "status": if changed { "updated" } else { "unchanged" },
            "modules": report,
        }),
        &format!("regenerated: {text}"),
    )
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
        || args.hide_layer.is_some()
        || args.local_layer.is_some()
        || !args.set_local_adjustments.is_empty()
        || !args.reset_local_adjustments.is_empty();
    if args.list && !wants_mutation {
        return mask_list(&args, &document);
    }
    if !wants_mutation {
        // Historical behaviour: without flags the command reports status.
        return mask_list(&args, &document);
    }
    let copy_id = resolve_mask_copy(&document, args.virtual_copy.as_deref())?;
    let local_set_specs = &args.set_local_adjustments;
    let local_reset_specs = &args.reset_local_adjustments;
    let local_transaction = if local_set_specs.is_empty() && local_reset_specs.is_empty() {
        None
    } else {
        Some(mask_local::LocalAdjustmentTransaction::capture(
            &document,
            &copy_id,
            args.local_layer.as_deref(),
        )?)
    };
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
    mask_local::apply_local_adjustment_flags(
        &mut document,
        &copy_id,
        args.local_layer.as_deref(),
        local_set_specs,
        local_reset_specs,
        &mut actions,
    )?;
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
            // GUI-GEN-GRANULAR-10 / M2: arm the ONE-SHOT explicit refresh in
            // the recipe as well. Without it the next render would re-infer the
            // pending masks with `refresh == false` and report the deliberately
            // requested work as an implicit re-inference. `process_selected`
            // consumes and removes the flag after the successful render
            // (REVIEW-CLI-MASKFLAG-1).
            copy.recipe
                .options
                .insert("update_masks".into(), "true".into());
        }
        info!("mask: marked masks pending (update_masks)");
        actions.push("update-masks".into());
    }
    if let Some(transaction) = local_transaction.as_ref() {
        transaction.append_history_entry(&mut document, &copy_id, &actions)?;
    }
    // Loud gate: arity, unknown references, cycles, ranges and ai_select
    // placement are rejected before anything is written.
    document.validate()?;
    if let Some(transaction) = local_transaction.as_ref() {
        save_sidecar_if_unchanged(&path, &document, Some(transaction.expected_revision()))?;
    } else {
        save_sidecar(&path, &document)?;
    }
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
                        "local_adjustments": layer.local_adjustments,
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
                    "  layer: {} -> {}/{} visible={} local={}",
                    layer.id,
                    layer.mask.copy_id,
                    layer.mask.mask_id,
                    layer.visible,
                    layer
                        .local_adjustments
                        .map(|adjustments| adjustments.to_string())
                        .unwrap_or_else(|| "none".into())
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
        local_adjustments: None,
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
/// A missing/invalid target sidecar, an unknown target copy, or non-neutral
/// local-mask state on that target marks only its item as `failed` (stderr
/// line + `info!` log); the remaining targets still run. Exit `0` on full
/// success, `3` on partial failure (analog `batch` / `batch-meta`), `1` on
/// hard errors. The original images are never modified; history `extras` carry
/// only the reference file name, never paths.
///
/// MASK-LOCAL-P0/P1.1: `previous` is a **recipe-only** cross-image transfer.
/// A non-neutral local mask state on the *reference* copy aborts the whole
/// command loudly (exit 1, no target touched); a non-neutral local mask state
/// on a *target* copy fails only that target (exit 3, target bytes unchanged).
/// Mask layers and their P0/P1.1 deltas are never transferred automatically —
/// an explicit full-look/mask copy is a separate, later action and is not part
/// of P1.1.
fn previous(args: PreviousArgs) -> Result<(), CliError> {
    if args.to.is_empty() {
        return Err(CliError::Message(
            "`previous` requires at least one `--to` target".into(),
        ));
    }
    let from_sidecar = sidecar_path_for(&args.from);
    let from_document = load_sidecar(&from_sidecar).map_err(CliError::from)?;
    let from_id = args.from_copy.as_deref().unwrap_or("vc-original");
    let reference_copy = from_document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == from_id)
        .ok_or_else(|| {
            CliError::Message(format!(
                "unknown virtual copy `{from_id}` in `{}`",
                from_sidecar.display()
            ))
        })?;
    let local_layers = mask_local::non_neutral_local_layer_ids(reference_copy)?;
    if !local_layers.is_empty() {
        return Err(CliError::Message(format!(
            "previous refused recipe-only transfer from copy `{from_id}`: non-neutral local mask adjustments exist on layer(s) {}; cross-image mask state transfer is unsafe",
            local_layers.join(", ")
        )));
    }
    let reference = reference_copy.recipe.clone();
    info!(
        "previous: reference `{}` copy `{from_id}`",
        args.from.display()
    );
    let to_id = args.to_copy.as_deref().unwrap_or("vc-original");
    let mut applied_count = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut items = Vec::with_capacity(args.to.len());
    for target in &args.to {
        match mask_local::apply_previous_to_target(target, to_id, &reference, &args.from) {
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

/// G-04 Remove-Parität: list and edit the spot-heal recipe state of one
/// image sidecar. The original image is never modified; every write goes
/// through `save_sidecar` after `document.validate()`. Mutations are loud
/// (`CliError::Message`, exit 1) and `--detect-objects` never applies
/// silently (only `--detect-apply` persists candidates).
fn spot(args: SpotArgs) -> Result<(), CliError> {
    reject_spot_remove_conflicts(&args)?;
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
    // R5-DUST-23-FOLLOWUP: the single-spot editor (`--spot-id` + `--set-*`,
    // `--remove-spot`) contradicts `--clear` the same way.
    if args.clear
        && (args.add_heuristic
            || args.detect_apply
            || args.regenerate_variant.is_some()
            || args.remove_spot.is_some()
            || args.spot_id.is_some())
    {
        return Err(CliError::Message(
            "--clear removes every spot and contradicts --add-heuristic/--detect-apply/--regenerate-variant/--spot-id/--remove-spot"
                .into(),
        ));
    }
    // R5-DUST-23-FOLLOWUP: `--set-*` needs `--spot-id` (and vice versa) —
    // either alone would be a silent no-op.
    let wants_update = args.set_radius.is_some()
        || args.set_feather.is_some()
        || args.set_opacity.is_some()
        || args.set_offset_dx.is_some()
        || args.set_offset_dy.is_some();
    if wants_update && args.spot_id.is_none() {
        return Err(CliError::Message(
            "--set-radius/--set-feather/--set-opacity/--set-offset-dx/--set-offset-dy require --spot-id <ID>".into(),
        ));
    }
    if args.spot_id.is_some() && !wants_update {
        return Err(CliError::Message(
            "--spot-id requires at least one --set-radius/--set-feather/--set-opacity/--set-offset-dx/--set-offset-dy".into(),
        ));
    }
    let wants_mutation = args.add_heuristic
        || args.clear
        || args.set_visualize_threshold.is_some()
        || args.clear_visualize
        || args.set_distraction.is_some()
        || args.detect_apply
        || args.regenerate_variant.is_some()
        || args.spot_id.is_some()
        || args.remove_spot.is_some();
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
    // R5-DUST-23-FOLLOWUP: per-spot param edits (heuristic only, loud
    // otherwise) and single-spot removal (loud on unknown ids).
    if let Some(spot_id) = args.spot_id.as_deref() {
        spot_update_params(
            &mut document,
            &copy_id,
            spot_id,
            args.set_radius,
            args.set_feather,
            args.set_opacity,
            args.set_offset_dx,
            args.set_offset_dy,
        )?;
        info!("spot: updated `{spot_id}` on copy `{copy_id}`");
        actions.push(format!("update-spot:{spot_id}"));
    }
    if let Some(spot_id) = args.remove_spot.as_deref() {
        spot_remove_entry(&mut document, &copy_id, spot_id)?;
        info!("spot: removed `{spot_id}` on copy `{copy_id}`");
        actions.push(format!("remove-spot:{spot_id}"));
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

/// R5-DUST-23-FOLLOWUP: spot mutation ops live in `spot_ops` (ratchet);
///
/// Mutable access to one virtual copy's recipe owner (loud on unknown ids).
fn spot_copy_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut lumina_sidecar::VirtualCopy, CliError> {
    mask_copy_mut(document, copy_id)
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
    let spots = spot_removal_entries(&copy.recipe);
    // Preserve every entry (including null/missing references) and annotate
    // status from the same decision layer used by the GUI.
    let display_spots = display_spot_entries_for_input(&spots, &args.input);
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
                "spots": display_spots,
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
        for entry in &display_spots {
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

/// LRPAR-G06-UPRIGHT-15: analyze/enable/disable/clear/status the persisted
/// upright stage of one virtual copy. `--analyze` decodes the source, runs the
/// deterministic `upright-lines-v1` analysis and persists the suggestion bound
/// to the current source fingerprint; the manual perspective stays persisted
/// and is authoritative whenever upright is disabled.
fn upright(args: UprightArgs) -> Result<(), CliError> {
    if args.enable && args.disable {
        return Err(CliError::Message(
            "--enable and --disable are mutually exclusive".into(),
        ));
    }
    if args.analyze && args.clear {
        return Err(CliError::Message(
            "--analyze and --clear are mutually exclusive".into(),
        ));
    }
    let wants_mutation = args.analyze || args.enable || args.disable || args.clear;
    if args.list && wants_mutation {
        return Err(CliError::Message(
            "--list is read-only; pass no mutation flags with it".into(),
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

    if args.clear {
        mask_copy_mut(&mut document, &copy_id)?.recipe.upright = None;
        info!("upright: cleared stage on copy `{copy_id}`");
        actions.push("upright:clear".into());
    } else {
        if args.analyze {
            let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
            let (frame, raw) = decode_input(&args.input, &bytes)?;
            let identity = source_identity(&args.input, &bytes, &frame, raw.as_ref())?;
            let fingerprint = upright_input_fingerprint(
                &identity.content_hash,
                frame.width,
                frame.height,
                identity.orientation,
            );
            let suggestion = analyze_upright(&frame);
            info!(
                "upright: analyzed copy `{copy_id}` ({} line pixels, confidence {:.3}, \
                 vertical {:.3}, horizontal {:.3}, rotation {:.3})",
                suggestion.line_count,
                suggestion.confidence,
                suggestion.vertical,
                suggestion.horizontal,
                suggestion.rotation
            );
            // `--analyze` applies the fresh suggestion; `--disable` in the same
            // call keeps it persisted but inactive.
            let enabled = !args.disable;
            mask_copy_mut(&mut document, &copy_id)?.recipe.upright = Some(Upright {
                version: 1,
                enabled,
                analysis: Some(upright_analysis(suggestion, fingerprint)),
            });
            actions.push("upright:analyze".into());
            actions.push(format!(
                "upright:{}",
                if enabled { "enable" } else { "disable" }
            ));
        } else if args.enable || args.disable {
            let enabled = args.enable;
            let upright = upright_mut(&mut document, &copy_id)?;
            if upright.analysis.is_none() {
                return Err(CliError::Message(
                    "no persisted upright analysis; run `upright --analyze` first".into(),
                ));
            }
            upright.enabled = enabled;
            info!(
                "upright: {} on copy `{copy_id}`",
                if enabled { "enabled" } else { "disabled" }
            );
            actions.push(format!(
                "upright:{}",
                if enabled { "enable" } else { "disable" }
            ));
        }
    }

    if wants_mutation {
        document
            .validate()
            .map_err(|error| CliError::Message(error.to_string()))?;
        let copy = mask_copy_mut(&mut document, &copy_id)?;
        let final_recipe = copy.recipe.clone();
        let mut id = format!("upright-{}", timestamp());
        let mut suffix = 0u32;
        while copy.history.iter().any(|entry| entry.id == id) {
            suffix += 1;
            id = format!("upright-{}-{suffix}", timestamp());
        }
        let mut extras = BTreeMap::new();
        extras.insert("step".into(), serde_json::Value::String("upright".into()));
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
    upright_list(&args, &document, &copy_id, &actions)
}

/// Mutable access to one virtual copy's upright stage, creating an empty
/// (disabled, no analysis) stage when none exists.
fn upright_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut Upright, CliError> {
    let copy = mask_copy_mut(document, copy_id)?;
    Ok(copy.recipe.upright.get_or_insert(Upright {
        version: 1,
        enabled: false,
        analysis: None,
    }))
}

/// Read-only upright status: persisted stage plus `fresh`/`stale` vs. the
/// current source fingerprint. A stale analysis is reported, never silently
/// recomputed (SOLL: Identität/Veraltung).
fn upright_list(
    args: &UprightArgs,
    document: &SidecarDocument,
    copy_id: &str,
    actions: &[String],
) -> Result<(), CliError> {
    let copy = document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == copy_id)
        .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{copy_id}`")))?;
    let stage = copy.recipe.upright.as_ref();
    // Current fingerprint from the sidecar source identity geometry; a change
    // to the source bytes flips to `stale` loudly.
    let current_content_hash = match fs::read(&args.input) {
        Ok(bytes) => format!("blake3:{}", blake3::hash(&bytes).to_hex()),
        Err(_) => document.source.content_hash.clone(),
    };
    let current = upright_input_fingerprint(
        &current_content_hash,
        document.source.geometry_fingerprint.width,
        document.source.geometry_fingerprint.height,
        document.source.orientation,
    );
    let status = match stage.and_then(|stage| stage.analysis.as_ref()) {
        None => "none",
        Some(analysis) if analysis.fingerprint.input_fingerprint == current => "fresh",
        Some(_) => "stale",
    };
    if args.json {
        emit(
            true,
            serde_json::json!({
                "command": "upright",
                "input": args.input,
                "copy": copy_id,
                "enabled": stage.map(|stage| stage.enabled),
                "status": status,
                "upright": stage,
                "actions": actions,
            }),
            "upright status listed",
        )
    } else {
        match stage {
            None => println!("  upright: none"),
            Some(stage) => {
                println!("  upright: enabled={} status={status}", stage.enabled);
                if let Some(analysis) = &stage.analysis {
                    println!(
                        "    analysis: {} v{} vertical={} horizontal={} rotation={} \
                         lines={} confidence={}",
                        analysis.fingerprint.algorithm,
                        analysis.fingerprint.version,
                        analysis.vertical,
                        analysis.horizontal,
                        analysis.rotation,
                        analysis.line_count,
                        analysis.confidence
                    );
                    println!(
                        "    fingerprint: {}",
                        analysis.fingerprint.input_fingerprint
                    );
                }
            }
        }
        emit(
            false,
            serde_json::json!({"command":"upright","status":"ok"}),
            if actions.is_empty() {
                "upright status listed"
            } else {
                "upright updated"
            },
        )
    }
}

/// G-14: inspect/edit the persisted red-eye regions of one virtual copy.
///
/// LRPAR-G14-REDEYE-AUTO-15 (2.0): `--detect` runs the deterministic,
/// model-free pupil heuristic on the decoded source pixels and lists the
/// candidates read-only; `--detect-apply` persists them explicitly. Detection
/// never runs implicitly and never prefills a recipe on its own.
fn red_eye(args: RedEyeArgs) -> Result<(), CliError> {
    if args.detect_apply && !args.detect {
        return Err(CliError::Message(
            "--detect-apply requires --detect (detection is never implicit)".into(),
        ));
    }
    if args.clear && (!args.set.is_empty() || !args.remove.is_empty() || args.detect_apply) {
        return Err(CliError::Message(
            "--clear contradicts --set/--remove/--detect-apply".into(),
        ));
    }
    let wants_mutation =
        !args.set.is_empty() || !args.remove.is_empty() || args.clear || args.detect_apply;
    if args.list && wants_mutation {
        return Err(CliError::Message(
            "--list is read-only; pass no mutation flags with it".into(),
        ));
    }
    // `--set`/`--remove`/`--clear` are explicit edits and always record a
    // history step (unchanged behaviour); `--detect-apply` only writes when it
    // actually changed the persisted regions (idempotent re-application).
    let mut changed = !args.set.is_empty() || !args.remove.is_empty() || args.clear;
    // Detection runs on the decoded source before any recipe stage. It is
    // computed once, before the mutation, so `--detect-apply` persists exactly
    // what `--detect` listed.
    let mut detected: Vec<DetectedRedEye> = Vec::new();
    let mut dropped = 0usize;
    if args.detect {
        let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
        let (frame, _) = decode_input(&args.input, &bytes)?;
        let detection = detect_red_eyes(&frame);
        detected = detection.candidates;
        dropped = detection.dropped;
        if dropped > 0 {
            // Loud, deterministic cap handling: the strongest candidates are
            // kept, the rest are reported — never silently discarded.
            eprintln!(
                "lumina: warning: red-eye detection found {} candidate(s); keeping the \
                 {RED_EYE_MAX_REGIONS} strongest and dropping {dropped}",
                detected.len() + dropped
            );
        }
        info!(
            "red-eye: detected {} candidate(s) ({} dropped) on `{}`",
            detected.len(),
            dropped,
            args.input.display()
        );
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

    if args.clear {
        mask_copy_mut(&mut document, &copy_id)?.recipe.red_eye = None;
        info!("red-eye: cleared stage on copy `{copy_id}`");
        actions.push("red-eye:clear".into());
    } else {
        for spec in &args.set {
            let region = parse_red_eye_region(spec)?;
            let correction = red_eye_mut(&mut document, &copy_id)?;
            match correction
                .regions
                .iter_mut()
                .find(|existing| existing.id == region.id)
            {
                Some(existing) => *existing = region.clone(),
                None => {
                    if correction.regions.len() >= RED_EYE_MAX_REGIONS {
                        return Err(CliError::Message(format!(
                            "red-eye region limit of {RED_EYE_MAX_REGIONS} reached"
                        )));
                    }
                    correction.regions.push(region.clone());
                }
            }
            info!(
                "red-eye: marked `{}` at ({}, {}) r={} on copy `{copy_id}`",
                region.id, region.x, region.y, region.radius
            );
            actions.push(format!("red-eye:set:{}", region.id));
        }
        for id in &args.remove {
            let correction = red_eye_mut(&mut document, &copy_id)?;
            let before = correction.regions.len();
            correction.regions.retain(|region| &region.id != id);
            if correction.regions.len() == before {
                return Err(CliError::Message(format!("unknown red-eye region `{id}`")));
            }
            info!("red-eye: removed `{id}` on copy `{copy_id}`");
            actions.push(format!("red-eye:remove:{id}"));
        }
    }

    if args.detect_apply {
        // Replace only the automatically detected regions; manually marked
        // regions are never touched (deterministic, idempotent apply).
        let before_stage = mask_copy_mut(&mut document, &copy_id)?
            .recipe
            .red_eye
            .clone();
        let applied = {
            let correction = red_eye_mut(&mut document, &copy_id)?;
            correction
                .regions
                .retain(|region| !region.id.starts_with(RED_EYE_DETECT_ID_PREFIX));
            if correction.regions.len() + detected.len() > RED_EYE_MAX_REGIONS {
                return Err(CliError::Message(format!(
                    "red-eye detection would exceed the {RED_EYE_MAX_REGIONS}-region cap: \
                     {} manual region(s) + {} detected; remove regions or clear the stage \
                     (nothing was written)",
                    correction.regions.len(),
                    detected.len()
                )));
            }
            for candidate in &detected {
                let region = candidate.to_region();
                match correction
                    .regions
                    .iter_mut()
                    .find(|existing| existing.id == region.id)
                {
                    Some(existing) => *existing = region,
                    None => correction.regions.push(region),
                }
            }
            correction.regions.len()
        };
        if applied == 0 {
            // A fresh detection that found nothing removes only stale auto
            // regions; an empty stage is stored as absence (identity).
            mask_copy_mut(&mut document, &copy_id)?.recipe.red_eye = None;
        }
        changed |= before_stage != mask_copy_mut(&mut document, &copy_id)?.recipe.red_eye;
        info!(
            "red-eye: applied {} detected region(s) on copy `{copy_id}`",
            detected.len()
        );
        actions.push(format!("detect-apply:{}", detected.len()));
        if dropped > 0 {
            actions.push(format!("detect-dropped:{dropped}"));
        }
    }

    if changed {
        // Loud gate: ranges, ids and the 32-region cap are rejected before
        // anything is written (the sidecar validator re-checks after the edit).
        document
            .validate()
            .map_err(|error| CliError::Message(error.to_string()))?;
        let copy = mask_copy_mut(&mut document, &copy_id)?;
        let final_recipe = copy.recipe.clone();
        let mut id = format!("red-eye-{}", timestamp());
        let mut suffix = 0u32;
        while copy.history.iter().any(|entry| entry.id == id) {
            suffix += 1;
            id = format!("red-eye-{}-{suffix}", timestamp());
        }
        let mut extras = BTreeMap::new();
        extras.insert("step".into(), serde_json::Value::String("red-eye".into()));
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
    red_eye_list(&args, &document, &copy_id, &detected, dropped, &actions)
}

/// Mutable access to one virtual copy's red-eye stage, creating an empty
/// (`version = 1`, no regions) stage when none exists.
fn red_eye_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut RedEyeCorrection, CliError> {
    let copy = mask_copy_mut(document, copy_id)?;
    Ok(copy.recipe.red_eye.get_or_insert_with(|| RedEyeCorrection {
        version: 1,
        regions: Vec::new(),
    }))
}

/// Parses `ID:x,y,radius,desaturate,darken` with normalized values and loud
/// range checks (never clipped).
fn parse_red_eye_region(spec: &str) -> Result<RedEyeRegion, CliError> {
    let (id, values) = spec.split_once(':').ok_or_else(|| {
        CliError::Message(format!(
            "invalid red-eye region `{spec}`: expected `ID:x,y,radius,desaturate,darken`"
        ))
    })?;
    if id.is_empty() {
        return Err(CliError::Message(
            "red-eye region id must not be empty".into(),
        ));
    }
    let numbers: Vec<&str> = values.split(',').collect();
    if numbers.len() != 5 {
        return Err(CliError::Message(format!(
            "invalid red-eye region `{spec}`: expected 5 values after the id"
        )));
    }
    let parse = |raw: &str, field: &str| -> Result<f32, CliError> {
        raw.trim().parse::<f32>().map_err(|_| {
            CliError::Message(format!(
                "invalid red-eye {field} `{raw}`: expected a number"
            ))
        })
    };
    let x = parse(numbers[0], "x")?;
    let y = parse(numbers[1], "y")?;
    let radius = parse(numbers[2], "radius")?;
    let desaturate = parse(numbers[3], "desaturate")?;
    let darken = parse(numbers[4], "darken")?;
    if !x.is_finite() || !(0.0..=1.0).contains(&x) {
        return Err(CliError::Message(format!("red-eye x `{x}` out of 0..=1")));
    }
    if !y.is_finite() || !(0.0..=1.0).contains(&y) {
        return Err(CliError::Message(format!("red-eye y `{y}` out of 0..=1")));
    }
    if !radius.is_finite() || radius <= 0.0 || radius > 1.0 {
        return Err(CliError::Message(format!(
            "red-eye radius `{radius}` out of (0, 1]"
        )));
    }
    for (field, value) in [("desaturate", desaturate), ("darken", darken)] {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(CliError::Message(format!(
                "red-eye {field} `{value}` out of 0..=1"
            )));
        }
    }
    Ok(RedEyeRegion {
        id: id.into(),
        x,
        y,
        radius,
        desaturate,
        darken,
    })
}

fn red_eye_list(
    args: &RedEyeArgs,
    document: &SidecarDocument,
    copy_id: &str,
    detected: &[DetectedRedEye],
    dropped: usize,
    actions: &[String],
) -> Result<(), CliError> {
    let copy = document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == copy_id)
        .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{copy_id}`")))?;
    let correction = copy.recipe.red_eye.as_ref();
    let regions = correction.map(|c| c.regions.as_slice()).unwrap_or(&[]);
    if args.json {
        let detected_json: Vec<serde_json::Value> = detected
            .iter()
            .map(|candidate| {
                serde_json::json!({
                    "id": candidate.id,
                    "x": candidate.x,
                    "y": candidate.y,
                    "radius": candidate.radius,
                    "confidence": candidate.confidence,
                })
            })
            .collect();
        emit(
            true,
            serde_json::json!({
                "command": "red-eye",
                "input": args.input,
                "copy": copy_id,
                "count": regions.len(),
                "red_eye": correction,
                "detected": detected_json,
                "dropped": dropped,
                "actions": actions,
            }),
            "red-eye status listed",
        )
    } else {
        if regions.is_empty() {
            println!("  red-eye: none");
        } else {
            println!("  red-eye: {} region(s)", regions.len());
            for region in regions {
                println!(
                    "    {} x={} y={} radius={} desaturate={} darken={}",
                    region.id, region.x, region.y, region.radius, region.desaturate, region.darken
                );
            }
        }
        if args.detect {
            if detected.is_empty() {
                println!("  red-eye detection: no red pupils found");
            } else {
                println!("  red-eye detection: {} candidate(s)", detected.len());
                for candidate in detected {
                    println!(
                        "    {} x={} y={} radius={} confidence={}",
                        candidate.id,
                        candidate.x,
                        candidate.y,
                        candidate.radius,
                        candidate.confidence
                    );
                }
            }
            if dropped > 0 {
                println!(
                    "  red-eye detection: {dropped} candidate(s) dropped above the \
                     {RED_EYE_MAX_REGIONS}-region cap"
                );
            }
        }
        emit(
            false,
            serde_json::json!({"command":"red-eye","status":"ok"}),
            if actions.is_empty() {
                "red-eye status listed"
            } else {
                "red-eye updated"
            },
        )
    }
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
/// reading the `.lumina.zdata` bundle. A malformed path/version, missing bundle,
/// missing artifact id, or checksum mismatch is a hard error — there is no
/// silent fallback (reproducibility over convenience).
fn resolve_source_actions(
    recipe: &EditRecipe,
    zdata_path: &Path,
) -> Result<Vec<SourceActionArtifact>, CliError> {
    if recipe.source_actions.is_empty() {
        return Ok(Vec::new());
    }
    let container =
        load_validated_source_action_bundle(recipe, zdata_path).map_err(CliError::Message)?;
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
/// # Lens identification (G-06 EXIF-Erkennung, GUI-ROUTING-N6)
/// `RawMetadata.lens` (REVIEW-RAW-N2) is passed as the Lensfun lens name. A
/// **named** lens must exist in the database (strict, no `LF_SEARCH_LOOSE`);
/// otherwise `None` and the manual model apply. Loose matching fabricated
/// profiles (`EOS R1`→`EOS R`, `RF200-800mm`→`RF 24-240mm`) and applied wrong
/// corrections — forbidden ("nie ein geratenes Profil"). Without a lens name
/// the documented body/mount fallback applies; `--lensfun-status` and the
/// render `info!` log name the match explicitly.
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

// ---------------------------------------------------------------------------
// GEN-ONNX-1 Welle 1: generative canvas artifact (local ONNX fixture model).
// ---------------------------------------------------------------------------

/// `lumina generative` arguments (see `feature/product/generative-expand.md`).
#[derive(Debug, Clone, Args)]
struct GenerativeArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    virtual_copy: Option<String>,
    /// Report the persisted artifact status and exit (read-only).
    #[arg(long)]
    status: bool,
    /// Produce and persist the composited canvas artifact.
    #[arg(long)]
    generate: bool,
    /// Explicit regeneration: replace an existing record for the same identity.
    #[arg(long)]
    force: bool,
    /// Remove the persisted artifact link from the recipe (the bundle record
    /// and the original image are left untouched).
    #[arg(long)]
    remove: bool,
    /// Prompt (identity-bearing, roundtrip-stable; may be empty).
    #[arg(long)]
    prompt: Option<String>,
    /// Negative prompt (identity-bearing; additive schema field).
    #[arg(long)]
    negative_prompt: Option<String>,
    /// Deterministic seed (identity-bearing).
    #[arg(long)]
    seed: Option<u64>,
    /// `auto_fill_transparent`: fill transparent pixels after lens correction.
    #[arg(long)]
    auto_fill: bool,
    /// `expand_beyond_image`: enlarge the canvas (requires `--canvas`).
    #[arg(long)]
    expand: bool,
    /// Target canvas `WxH+X+Y` (offsets may be negative) for `--expand`.
    #[arg(long)]
    canvas: Option<String>,
    /// `keep_generative_content` crop decision.
    #[arg(long)]
    keep: Option<bool>,
    #[arg(long)]
    json: bool,
}

/// Owned pair of caller-supplied generative canvas artifacts.
#[derive(Default)]
struct GenerativeArtifacts {
    auto_fill: Option<GenerativeCanvasArtifact>,
    expand: Option<GenerativeCanvasArtifact>,
}

impl GenerativeArtifacts {
    fn input(&self) -> GenerativeCanvasInput<'_> {
        GenerativeCanvasInput {
            auto_fill: self.auto_fill.as_ref(),
            expand: self.expand.as_ref(),
        }
    }
}

fn onnx_generative_role(role: CoreGenerativeRole) -> OnnxGenerativeRole {
    match role {
        CoreGenerativeRole::Expand => OnnxGenerativeRole::Expand,
        CoreGenerativeRole::AutoFillTransparent => OnnxGenerativeRole::AutoFillTransparent,
    }
}

/// The prompt/model identity of a persisted generative edit.
///
/// `model_hash` is the pinned hash of the fixture model the CLI produces with
/// (`fixture_manifest(role)`), so producer (render-time verification) and
/// consumer (the ONNX producer) derive the exact same identity. `prompt` is the
/// persisted prompt; `negative_prompt` is the extras-backed additive field
/// (`GenerativeEdit::negative_prompt()`, `None` when unset) — `None` is
/// identity and matches the producer, which derives the same value.
fn generative_identity(
    role: CoreGenerativeRole,
    edit: &GenerativeEdit,
) -> lumina_core::GenerativeIdentity {
    lumina_core::GenerativeIdentity {
        model_hash: lumina_onnx::fixture_manifest(onnx_generative_role(role)).model_hash,
        prompt: edit.prompt.clone().unwrap_or_default(),
        negative_prompt: edit.negative_prompt().map(str::to_owned),
    }
}

/// Parse the CLI canvas spec `WxH+X+Y` / `WxH-X-Y`.
fn parse_generative_canvas(spec: &str) -> Result<GenerativeCanvas, CliError> {
    let invalid = || {
        CliError::Message(format!(
            "invalid --canvas `{spec}`: expected WxH+X+Y (offsets may be negative)"
        ))
    };
    let spec = spec.trim();
    let x_pos = spec.find('x').ok_or_else(invalid)?;
    let width: u32 = spec[..x_pos].trim().parse().map_err(|_| invalid())?;
    let rest = &spec[x_pos + 1..];
    let sign_pos = rest.find(['+', '-']).unwrap_or(rest.len());
    let height: u32 = rest[..sign_pos].trim().parse().map_err(|_| invalid())?;
    let offsets = &rest[sign_pos..];
    let mut parts: Vec<&str> = Vec::new();
    let mut start = 0usize;
    for (index, byte) in offsets.bytes().enumerate() {
        if index > 0 && (byte == b'+' || byte == b'-') {
            parts.push(&offsets[start..index]);
            start = index;
        }
    }
    if !offsets.is_empty() {
        parts.push(&offsets[start..]);
    }
    let (source_offset_x, source_offset_y) = match parts.as_slice() {
        [x, y] => (
            x.parse::<i32>().map_err(|_| invalid())?,
            y.parse::<i32>().map_err(|_| invalid())?,
        ),
        _ => return Err(invalid()),
    };
    let canvas = GenerativeCanvas {
        output_width: width,
        output_height: height,
        source_offset_x,
        source_offset_y,
        extras: Default::default(),
    };
    canvas
        .validate()
        .map_err(|error| CliError::Message(format!("invalid --canvas `{spec}`: {error}")))?;
    Ok(canvas)
}

/// Deterministic bundle record id for a generative identity digest.
fn generative_record_id(identity_digest: &str) -> String {
    let prefix = identity_digest.get(..16).unwrap_or(identity_digest);
    format!("generative_canvas:{prefix}")
}

/// Stage the frame exactly up to the generative stage, returning the frame
/// entering the auto-fill role (`after_lens`) and the frame entering the expand
/// role (`after_perspective`). Delegates to the core helper so producer and
/// consumer agree on the operation identity and no pipeline logic is
/// duplicated.
fn stage_generative_input(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    camera_white_balance: Option<[f32; 4]>,
    source_actions: &[SourceActionArtifact],
    lensfun: Option<LensfunCorrectorRef<'_>>,
) -> Result<(ImageFrame, ImageFrame), CliError> {
    Ok(lumina_core::generative_input_frames(
        frame,
        recipe,
        camera_white_balance,
        source_actions,
        lensfun,
    )?)
}

/// Produce one role's composited canvas with the deterministic fixture model.
///
/// The persisted edit may carry both roles; `produce_canvas` reads a single
/// role from the flags, so a role-scoped copy is passed — the other role's flag
/// never silently changes which canvas is produced.
fn produce_generative_role_canvas(
    input_frame: &ImageFrame,
    edit: &GenerativeEdit,
    role: CoreGenerativeRole,
) -> Result<GenerativeCanvasOutput, CliError> {
    let mut scoped = edit.clone();
    match role {
        CoreGenerativeRole::Expand => {
            scoped.expand_beyond_image = Some(true);
            scoped.auto_fill_transparent = Some(false);
        }
        CoreGenerativeRole::AutoFillTransparent => {
            scoped.expand_beyond_image = Some(false);
            scoped.auto_fill_transparent = Some(true);
        }
    }
    let model = GenerativeModelSource::Fixture(onnx_generative_role(role));
    produce_canvas(input_frame, &scoped, &model)
        .map_err(|error| CliError::Message(format!("generative {role:?} canvas failed: {error}")))
}

/// Persist one produced canvas into the sidecar `.lumina.zdata` bundle and
/// return its portable recipe link, its `--json` role object and its human
/// summary. The record id is the deterministic identity id
/// (`generative_record_id`) — the same convention the GUI producer uses, so GUI
/// and CLI address the same record.
fn persist_generative_role_canvas(
    zdata_path: &Path,
    relative_path: &str,
    force: bool,
    output: GenerativeCanvasOutput,
) -> Result<(GenerativeArtifactRef, serde_json::Value, String), CliError> {
    let identity = output.identity_digest;
    let role = output.role;
    let model_name = output.manifest.model_name.clone();
    let model_hash = output.manifest.model_hash.clone();
    let record = SidecarGenerativeCanvas {
        id: generative_record_id(&identity),
        width: output.width,
        height: output.height,
        pixels: output.pixels,
    };
    save_generative_canvas(zdata_path, record.clone(), force).map_err(|error| {
        CliError::Message(format!(
            "could not write generative canvas bundle `{}`: {error}",
            zdata_path.display()
        ))
    })?;
    let link =
        GenerativeArtifactRef::from_generative_canvas(&record, relative_path, identity.clone());
    let role_json = serde_json::json!({
        "role": format!("{role:?}"),
        "width": record.width,
        "height": record.height,
        "record": record.id,
        "identity": identity,
        "model": model_name,
        "model_hash": model_hash,
    });
    let summary = format!(
        "role={role:?} {}x{} record={} identity={identity}",
        record.width, record.height, record.id
    );
    Ok((link, role_json, summary))
}

/// The frame entering the expand role when the auto-fill role produced a
/// canvas: the auto-filled frame with `Lens → auto-fill → Perspective` applied,
/// mirroring the render order `Lens → auto-fill → Perspective → expand → Crop`
/// (GEN-ONNX-1 double-role record). Without an applied auto-fill canvas the
/// caller uses `after_perspective` directly.
///
/// The CLI must **not** call `lumina-core`'s `#[cfg(feature = "lensfun")]`
/// `ImageFrame::apply_perspective_stage` directly: the CLI's own `lensfun`
/// feature can be off while `lumina-core`'s is on (e.g. `cargo check
/// --workspace`, where the GUI's default `lensfun` unifies the core feature),
/// which would select the wrong arity. The frame is therefore extracted through
/// the shared, feature-uniform core render pipeline: supplying the auto-fill
/// artifact makes `composite_auto_fill` replace the lens result with the
/// artifact frame, and the perspective stage runs on it exactly as the real
/// render does. A full-frame crop and `expand = false` keep the remaining
/// stages identity, so the CLI still owns no image math.
fn generative_expand_input(
    source: &ImageFrame,
    recipe: &EditRecipe,
    camera_white_balance: Option<[f32; 4]>,
    source_actions: &[SourceActionArtifact],
    auto_fill: &GenerativeCanvasArtifact,
    lensfun: Option<LensfunCorrectorRef<'_>>,
) -> Result<ImageFrame, CliError> {
    let mut staged = recipe.clone();
    // Neutralize crop/rotation/mirroring and lens blur; they run after the
    // expand stage in the real pipeline and must not alter the extracted frame.
    staged.geometry = Some(Geometry {
        version: 1,
        crop: Some(Crop::Free {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }),
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    });
    staged.lens_blur = None;
    if let Some(edit) = staged.generative_edit.as_mut() {
        // Only the auto-fill substitution runs; the expand stage is the caller's
        // next step. `expand = false` requires `canvas == None`
        // (`validate_generative_edit`), and the canvas is unused by the
        // perspective stage.
        edit.expand_beyond_image = Some(false);
        edit.canvas = None;
    }
    let output = render_frame_with_generative(
        source,
        &RenderContext {
            recipe: &staged,
            camera_white_balance,
            source_actions,
            masks: None,
            lensfun,
            depth: None,
        },
        GenerativeCanvasInput {
            auto_fill: Some(auto_fill),
            expand: None,
        },
    )?;
    Ok(output.frame)
}

/// Resolve one active generative role's canvas.
///
/// The persisted record is addressed by the deterministic identity record id
/// (`generative_record_id(digest)`, the same convention the GUI producer uses),
/// so the *unlinked* second role of a double-role record is resolvable by
/// construction: a matching record proves the exact identity (role, seed,
/// canvas, prompt, model and input are all in the digest, and the zdata load
/// verifies the BLAKE3 checksum). When no matching record exists, `link` — the
/// recipe link, pre-scoped by the caller to the role it belongs to — yields the
/// loud `missing`/`stale`/`corrupt` diagnosis. There is no silent fallback to
/// an unexpanded render.
///
/// `owns_link` marks the role the single recipe link belongs to. Its link must
/// be present: an explicit `--remove` unlink is a deliberate state and must not
/// be silently reconstructed from the still-present bundle record.
fn resolve_generative_role(
    bundle_root: &Path,
    zdata_path: &Path,
    link: Option<&GenerativeArtifactRef>,
    owns_link: bool,
    role: CoreGenerativeRole,
    digest: &str,
) -> Result<GenerativeCanvasArtifact, CliError> {
    if owns_link && link.is_none() {
        return Err(CliError::Message(format!(
            "generative {role:?} is active but no `generative_canvas` artifact is linked; run \
             `lumina generative --generate --input <file>` (no silent fallback)"
        )));
    }
    let record_id = generative_record_id(digest);
    if zdata_path.exists() {
        if let Ok(container) = load_zdata(zdata_path) {
            if let Ok(record) = container.generative_canvas(&record_id) {
                let frame = ImageFrame::new(record.width, record.height, record.pixels)
                    .map_err(|error| CliError::Message(error.to_string()))?;
                return Ok(GenerativeCanvasArtifact::new(role, frame));
            }
        }
    }
    // The role-scoped recipe link is the canonical diagnosis: a
    // `Stale`/`Missing`/`Corrupt` link stays loud; a link that is current but
    // whose record is unreadable must never render "as if not generated".
    if let Some(link) = link {
        let status = generative_artifact_status(bundle_root, link, digest);
        if status != GenerativeArtifactStatus::Available {
            return Err(CliError::Message(format!(
                "generative canvas `{}` is {status:?} for identity {digest}; refusing to render \
                 (run `lumina generative --generate` to rebuild it) — no silent fallback",
                link.id
            )));
        }
        return Err(CliError::Message(format!(
            "generative {role:?} link `{}` is current but its bundle record `{record_id}` is \
             unreadable; refusing to render (no silent fallback)",
            link.id
        )));
    }
    Err(CliError::Message(format!(
        "generative {role:?} is active but no `generative_canvas` artifact is available for \
         identity {digest}; run `lumina generative --generate --input <file>` (no silent fallback)"
    )))
}

/// Non-fatal per-role status used by `--status`: `(status, resolved frame)`.
///
/// Mirrors [`resolve_generative_role`] without aborting, so a double-role
/// record reports every active role; the frame is returned when the auto-fill
/// canvas resolved (the expand identity is derived from it).
fn generative_role_status(
    bundle_root: &Path,
    zdata_path: &Path,
    link: Option<&GenerativeArtifactRef>,
    owns_link: bool,
    digest: &str,
) -> (String, Option<ImageFrame>) {
    // The link-owning role with an explicitly removed link is `missing` — the
    // record must not be re-adopted silently (`--remove`).
    if owns_link && link.is_none() {
        return ("missing".to_owned(), None);
    }
    let record_id = generative_record_id(digest);
    if zdata_path.exists() {
        if let Ok(container) = load_zdata(zdata_path) {
            if let Ok(record) = container.generative_canvas(&record_id) {
                if let Ok(frame) = ImageFrame::new(record.width, record.height, record.pixels) {
                    return ("available".to_owned(), Some(frame));
                }
            }
        }
    }
    if let Some(link) = link {
        let status = format!(
            "{:?}",
            generative_artifact_status(bundle_root, link, digest)
        )
        .to_lowercase();
        return (status, None);
    }
    ("missing".to_owned(), None)
}

/// Resolve the render-time generative canvas artifacts for `recipe`.
///
/// GEN-ONNX-1 (double-role record): a single `GenerativeEdit` may carry both
/// `auto_fill_transparent` and `expand_beyond_image` (the SOLL order is
/// `Lens → auto-fill → Perspective → expand → Crop`). Both roles are resolved;
/// the expand identity is derived from the auto-filled frame with the
/// perspective stage applied, exactly as the producer built it. A
/// missing/invalid/absent artifact for an active role aborts the render — there
/// is no silent fallback.
fn resolve_generative_artifacts(
    frame: &ImageFrame,
    recipe: &EditRecipe,
    camera_white_balance: Option<[f32; 4]>,
    source_actions: &[SourceActionArtifact],
    zdata_path: &Path,
    lensfun: Option<LensfunCorrectorRef<'_>>,
) -> Result<GenerativeArtifacts, CliError> {
    let Some(edit) = recipe.generative_edit.as_ref() else {
        return Ok(GenerativeArtifacts::default());
    };
    let auto_fill_active = edit.auto_fill_transparent.unwrap_or(false);
    let expand_active = edit.effective_expand();
    if !auto_fill_active && !expand_active {
        return Ok(GenerativeArtifacts::default());
    }
    let seed = edit.seed.unwrap_or(0);
    let (after_lens, after_perspective) =
        stage_generative_input(frame, recipe, camera_white_balance, source_actions, lensfun)?;
    let bundle_root = zdata_path.parent().unwrap_or_else(|| Path::new("."));
    let link = edit.artifact.as_ref();
    // The single recipe link belongs to the canvas-defining role the producer
    // persisted: expand when it is active, otherwise auto-fill (the double-role
    // producer links the expand canvas). It is scoped per role so a
    // `stale`/`missing`/`corrupt` diagnosis is never reported for the wrong
    // role.
    let link_role = if expand_active {
        Some(CoreGenerativeRole::Expand)
    } else if auto_fill_active {
        Some(CoreGenerativeRole::AutoFillTransparent)
    } else {
        None
    };
    let role_link = |role: CoreGenerativeRole| link.filter(|_| link_role == Some(role));
    let mut artifacts = GenerativeArtifacts::default();
    // Auto-fill first: its composited frame is the input of the expand role
    // (`Lens → auto-fill → Perspective → expand`). Auto-fill without
    // transparent pixels after lens needs no artifact (normative caller
    // convention, `auto_fill = None` is the identity).
    if auto_fill_active && has_transparent_pixels(&after_lens) {
        let identity = generative_identity(CoreGenerativeRole::AutoFillTransparent, edit);
        let digest = GenerativeCacheKey::auto_fill(&after_lens, seed, &identity).digest();
        artifacts.auto_fill = Some(resolve_generative_role(
            bundle_root,
            zdata_path,
            role_link(CoreGenerativeRole::AutoFillTransparent),
            link_role == Some(CoreGenerativeRole::AutoFillTransparent),
            CoreGenerativeRole::AutoFillTransparent,
            &digest,
        )?);
    }
    if expand_active {
        let canvas = edit.canvas.as_ref().ok_or_else(|| {
            CliError::Message("expand_beyond_image requires a `canvas` (output_* + offsets)".into())
        })?;
        // Mirror the render order `Lens → auto-fill → Perspective → expand` so
        // the expand identity matches the canvas the producer built (the expand
        // canvas is authoritative and must embed the auto-filled pixels).
        let expand_input = match artifacts.auto_fill.as_ref() {
            Some(auto_fill) => generative_expand_input(
                frame,
                recipe,
                camera_white_balance,
                source_actions,
                auto_fill,
                lensfun,
            )?,
            None => after_perspective.clone(),
        };
        let identity = generative_identity(CoreGenerativeRole::Expand, edit);
        let digest = GenerativeCacheKey::expand(&expand_input, canvas, seed, &identity).digest();
        artifacts.expand = Some(resolve_generative_role(
            bundle_root,
            zdata_path,
            role_link(CoreGenerativeRole::Expand),
            link_role == Some(CoreGenerativeRole::Expand),
            CoreGenerativeRole::Expand,
            &digest,
        )?);
    }
    Ok(artifacts)
}

/// `lumina generative` — produce/report/remove the persisted `generative_canvas`
/// artifact of one virtual copy (GEN-ONNX-1 Welle 1).
fn generative(args: GenerativeArgs) -> Result<(), CliError> {
    let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
    let (frame, raw_metadata) = decode_input(&args.input, &bytes)?;
    let wb = raw_metadata
        .as_ref()
        .and_then(|metadata| sanitize_camera_white_balance(metadata.camera_white_balance));
    #[cfg(feature = "lensfun")]
    let (_lensfun_db, lensfun_corrector) = build_lensfun_corrector(raw_metadata.as_ref())
        .map(|(db, corrector)| (Some(db), Some(corrector)))
        .unwrap_or((None, None));
    // Feature-uniform corrector reference (Cargo feature unification safe).
    #[cfg(feature = "lensfun")]
    let generative_lensfun = lensfun_corrector.as_ref().map(LensfunCorrectorRef);
    #[cfg(not(feature = "lensfun"))]
    let generative_lensfun: Option<LensfunCorrectorRef<'_>> = None;
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
    if document.source.content_hash != current_identity.content_hash {
        return Err(CliError::Message(format!(
            "source changed since sidecar was written: `{}`",
            args.input.display()
        )));
    }
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
    let mut recipe = document.virtual_copies[copy_index].recipe.clone();
    let zdata_path = zdata_path_for(&args.input);
    let bundle_root = zdata_path.parent().unwrap_or_else(|| Path::new("."));

    if args.remove {
        if let Some(edit) = recipe.generative_edit.as_mut() {
            edit.artifact = None;
        }
        document.virtual_copies[copy_index].recipe = recipe;
        save_sidecar(&sidecar_path, &document)?;
        info!("generative: artifact link removed (bundle record kept)");
        return Ok(());
    }

    // Merge the CLI overrides into the persisted generative edit (additive).
    let mut edit = recipe.generative_edit.clone().unwrap_or(GenerativeEdit {
        version: 1,
        canvas: None,
        artifact: None,
        keep_generative_content: None,
        auto_fill_transparent: None,
        expand_beyond_image: None,
        seed: None,
        prompt: None,
        extras: Default::default(),
    });
    if edit.version != 1 {
        return Err(CliError::Message(format!(
            "unsupported generative_edit version {}; explicit migration required",
            edit.version
        )));
    }
    if let Some(prompt) = args.prompt.clone() {
        edit.prompt = Some(prompt);
    }
    if let Some(negative_prompt) = args.negative_prompt.clone() {
        edit.set_negative_prompt(Some(negative_prompt));
    }
    if let Some(seed) = args.seed {
        edit.seed = Some(seed);
    }
    if args.auto_fill {
        edit.auto_fill_transparent = Some(true);
    }
    if args.expand {
        let spec = args
            .canvas
            .as_deref()
            .ok_or_else(|| CliError::Message("--expand requires --canvas WxH+X+Y".into()))?;
        edit.expand_beyond_image = Some(true);
        edit.canvas = Some(parse_generative_canvas(spec)?);
    } else if let Some(spec) = args.canvas.as_deref() {
        edit.canvas = Some(parse_generative_canvas(spec)?);
    }
    if let Some(keep) = args.keep {
        edit.keep_generative_content = Some(keep);
    }

    let seed = edit.seed.unwrap_or(0);

    if args.status {
        let status_source_actions = resolve_source_actions(&recipe, &zdata_path)?;
        return report_generative_status(
            &edit,
            &frame,
            &recipe,
            wb,
            &status_source_actions,
            bundle_root,
            &zdata_path,
            seed,
            args.json,
            generative_lensfun,
        );
    }
    if !args.generate {
        return Err(CliError::Message(
            "specify one of --status, --generate or --remove".into(),
        ));
    }
    let auto_fill_active = edit.auto_fill_transparent.unwrap_or(false);
    let expand_active = edit.effective_expand();
    if !auto_fill_active && !expand_active {
        return Err(CliError::Message(
            "no generative role active: pass --expand or --auto-fill".into(),
        ));
    }
    let source_actions = resolve_source_actions(&recipe, &zdata_path)?;
    let (after_lens, after_perspective) =
        stage_generative_input(&frame, &recipe, wb, &source_actions, generative_lensfun)?;
    let relative_path = zdata_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "generative.zdata".into());

    // SOLL order `Lens → auto-fill → Perspective → expand`: the auto-fill
    // canvas is produced first; the expand canvas is then built from the
    // auto-filled frame with the perspective stage applied, so the composited
    // auto-fill pixels are authoritative in the expand canvas (GEN-ONNX-1
    // double-role record).
    let mut auto_fill_link: Option<GenerativeArtifactRef> = None;
    let mut expand_link: Option<GenerativeArtifactRef> = None;
    let mut auto_fill_frame: Option<ImageFrame> = None;
    let mut json_roles: Vec<serde_json::Value> = Vec::new();
    let mut summaries: Vec<String> = Vec::new();
    if auto_fill_active {
        if !has_transparent_pixels(&after_lens) {
            // Normative caller convention: no transparent pixels after lens →
            // identity, no artifact required (never a silent synthetic fill).
            info!(
                "generative: auto_fill active but no transparent pixels after lens; \
                 no auto-fill canvas produced (identity)"
            );
        } else {
            let output = produce_generative_role_canvas(
                &after_lens,
                &edit,
                CoreGenerativeRole::AutoFillTransparent,
            )?;
            auto_fill_frame = Some(
                output
                    .to_frame()
                    .map_err(|error| CliError::Message(error.to_string()))?,
            );
            let (link, role_json, summary) =
                persist_generative_role_canvas(&zdata_path, &relative_path, args.force, output)?;
            auto_fill_link = Some(link);
            json_roles.push(role_json);
            summaries.push(summary);
        }
    }
    if expand_active {
        // Mirror the render order `Lens → auto-fill → Perspective → expand` so
        // the produced expand canvas embeds the auto-filled pixels (and its
        // identity matches what the render-time resolver recomputes). The
        // pending double-role edit must be visible in the staged recipe — the
        // persisted recipe is only written below.
        let mut expand_recipe = recipe.clone();
        expand_recipe.generative_edit = Some(edit.clone());
        let expand_input = match auto_fill_frame.as_ref() {
            Some(filled) => {
                let artifact = GenerativeCanvasArtifact::new(
                    CoreGenerativeRole::AutoFillTransparent,
                    filled.clone(),
                );
                generative_expand_input(
                    &frame,
                    &expand_recipe,
                    wb,
                    &source_actions,
                    &artifact,
                    generative_lensfun,
                )?
            }
            None => after_perspective.clone(),
        };
        let output =
            produce_generative_role_canvas(&expand_input, &edit, CoreGenerativeRole::Expand)?;
        let (link, role_json, summary) =
            persist_generative_role_canvas(&zdata_path, &relative_path, args.force, output)?;
        expand_link = Some(link);
        json_roles.push(role_json);
        summaries.push(summary);
    }
    if auto_fill_link.is_none() && expand_link.is_none() {
        return Err(CliError::Message(
            "--auto-fill: no transparent pixels after lens correction; nothing to generate \
             (no artifact written, no silent synthetic fill)"
                .into(),
        ));
    }
    // The single recipe link is the canvas-defining expand role when both are
    // active (SOLL: `Lens → GenerativeEdit → Perspective → Crop`); the
    // auto-fill record stays addressable by its deterministic identity id.
    edit.artifact = expand_link.or(auto_fill_link);
    recipe.generative_edit = Some(edit);
    document.virtual_copies[copy_index].recipe = recipe;
    save_sidecar(&sidecar_path, &document)?;
    let summary = format!("generative canvas written: {}", summaries.join("; "));
    if args.json {
        if let [only] = json_roles.as_slice() {
            // Preserve the documented single-role payload shape.
            println!(
                "{}",
                serde_json::json!({
                    "status": "generated",
                    "role": only["role"],
                    "width": only["width"],
                    "height": only["height"],
                    "record": only["record"],
                    "identity": only["identity"],
                    "model": only["model"],
                    "model_hash": only["model_hash"],
                })
            );
        } else {
            println!(
                "{}",
                serde_json::json!({"status": "generated", "roles": json_roles})
            );
        }
    } else {
        println!("{summary}");
    }
    info!("generative: {summary}");
    Ok(())
}

/// Source actions are needed for exact stage parity; reused by `--status`.
///
/// GEN-ONNX-1 (double-role record): every active role is reported. The expand
/// identity embeds the auto-filled pixels, so it is only computed once the
/// auto-fill canvas resolved; without it the expand role is reported `missing`
/// (loud) instead of guessing a digest from the wrong frame.
#[allow(clippy::too_many_arguments)]
fn report_generative_status(
    edit: &GenerativeEdit,
    frame: &ImageFrame,
    recipe: &EditRecipe,
    camera_white_balance: Option<[f32; 4]>,
    source_actions: &[SourceActionArtifact],
    bundle_root: &Path,
    zdata_path: &Path,
    seed: u64,
    json: bool,
    lensfun: Option<LensfunCorrectorRef<'_>>,
) -> Result<(), CliError> {
    let auto_fill_active = edit.auto_fill_transparent.unwrap_or(false);
    let expand_active = edit.effective_expand();
    if !auto_fill_active && !expand_active {
        if json {
            println!("{}", serde_json::json!({"status": "inactive"}));
        } else {
            println!("generative: inactive (no role active)");
        }
        return Ok(());
    }
    let (after_lens, after_perspective) =
        stage_generative_input(frame, recipe, camera_white_balance, source_actions, lensfun)?;
    let link = edit.artifact.as_ref();
    let link_role = if expand_active {
        Some(CoreGenerativeRole::Expand)
    } else {
        Some(CoreGenerativeRole::AutoFillTransparent)
    };
    let role_link = |role: CoreGenerativeRole| link.filter(|_| link_role == Some(role));
    // (role, identity digest, status, record id)
    let mut roles: Vec<(&'static str, Option<String>, String, Option<String>)> = Vec::new();
    let mut auto_fill_frame: Option<ImageFrame> = None;
    let auto_fill_required = auto_fill_active && has_transparent_pixels(&after_lens);
    if auto_fill_active {
        if !auto_fill_required {
            roles.push(("AutoFillTransparent", None, "not-required".to_owned(), None));
        } else {
            let identity = generative_identity(CoreGenerativeRole::AutoFillTransparent, edit);
            let digest = GenerativeCacheKey::auto_fill(&after_lens, seed, &identity).digest();
            let owns_link = link_role == Some(CoreGenerativeRole::AutoFillTransparent);
            let (status, resolved) = generative_role_status(
                bundle_root,
                zdata_path,
                role_link(CoreGenerativeRole::AutoFillTransparent),
                owns_link,
                &digest,
            );
            auto_fill_frame = resolved;
            roles.push((
                "AutoFillTransparent",
                Some(digest.clone()),
                status,
                Some(generative_record_id(&digest)),
            ));
        }
    }
    if expand_active {
        if auto_fill_required && auto_fill_frame.is_none() {
            // The expand identity embeds the auto-filled pixels; without the
            // auto-fill canvas its status cannot be verified.
            roles.push(("Expand", None, "missing".to_owned(), None));
        } else {
            let canvas = edit.canvas.as_ref().ok_or_else(|| {
                CliError::Message("expand_beyond_image requires a `canvas`".into())
            })?;
            let expand_input = match auto_fill_frame.as_ref() {
                Some(filled) => {
                    let artifact = GenerativeCanvasArtifact::new(
                        CoreGenerativeRole::AutoFillTransparent,
                        filled.clone(),
                    );
                    generative_expand_input(
                        frame,
                        recipe,
                        camera_white_balance,
                        source_actions,
                        &artifact,
                        lensfun,
                    )?
                }
                None => after_perspective.clone(),
            };
            let identity = generative_identity(CoreGenerativeRole::Expand, edit);
            let digest =
                GenerativeCacheKey::expand(&expand_input, canvas, seed, &identity).digest();
            let (status, _) = generative_role_status(
                bundle_root,
                zdata_path,
                role_link(CoreGenerativeRole::Expand),
                link_role == Some(CoreGenerativeRole::Expand),
                &digest,
            );
            roles.push((
                "Expand",
                Some(digest.clone()),
                status,
                Some(generative_record_id(&digest)),
            ));
        }
    }
    let ok = |status: &str| status == "available" || status == "not-required";
    let failed: Vec<String> = roles
        .iter()
        .filter(|(_, _, status, _)| !ok(status))
        .map(|(role, _, status, _)| format!("{role}={status}"))
        .collect();
    let combined = if failed.is_empty() {
        if roles.iter().any(|(_, _, status, _)| status == "available") {
            "available".to_owned()
        } else {
            "not-required".to_owned()
        }
    } else {
        failed.join(", ")
    };
    if json {
        if let [role] = roles.as_slice() {
            let (role, identity, status, record) = role;
            println!(
                "{}",
                serde_json::json!({
                    "status": status,
                    "role": role,
                    "identity": identity,
                    "record": record,
                    "model": "inpaint-outpaint-xl",
                })
            );
        } else {
            let role_json: Vec<serde_json::Value> = roles
                .iter()
                .map(|(role, identity, status, record)| {
                    serde_json::json!({
                        "role": role,
                        "status": status,
                        "identity": identity,
                        "record": record,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::json!({
                    "status": combined,
                    "roles": role_json,
                    "model": "inpaint-outpaint-xl",
                })
            );
        }
    } else if let [role] = roles.as_slice() {
        println!("generative: status={} role={}", role.2, role.0);
    } else {
        let detail = roles
            .iter()
            .map(|(role, _, status, _)| format!("{role}:{status}"))
            .collect::<Vec<_>>()
            .join(", ");
        println!("generative: status={combined} roles={detail}");
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(CliError::Message(format!(
            "generative canvas is `{combined}` (no silent fallback; run `lumina generative --generate`)"
        )))
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
            source_roi: None,
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
    //
    // GEN-ONNX-1: an active generative edit is rendered by *compositing* the
    // persisted `generative_canvas` artifact. The artifact is resolved (and its
    // identity verified) here; a missing/stale/corrupt canvas aborts loudly.
    let generative = resolve_generative_artifacts(
        &frame,
        &recipe,
        wb,
        &source_actions,
        &zdata_path,
        #[cfg(feature = "lensfun")]
        lensfun_corrector.as_ref().map(LensfunCorrectorRef),
        #[cfg(not(feature = "lensfun"))]
        None,
    )?;
    let render_output =
        render_standard_with_generative(&frame, &recipe, &render_ctx, generative.input())?;
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
        export_image_with_generative(
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
                    source_roi: None,
                }),
                #[cfg(feature = "lensfun")]
                lensfun: lensfun_corrector.as_ref().map(LensfunCorrectorRef),
                #[cfg(not(feature = "lensfun"))]
                lensfun: None,
                depth: None,
            },
            options,
            generative.input(),
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
    parse_output_format(format).map(|_| ())
}

/// Parses the `--format` value into an [`ImageFileFormat`] or returns the same
/// loud usage error `validate_format` produced. Single source so callers that
/// must actually encode in the requested format (e.g. `denoise --render`) do
/// not re-implement the mapping (M1).
fn parse_output_format(format: &str) -> Result<ImageFileFormat, CliError> {
    ImageFileFormat::from_extension(format).ok_or_else(|| {
        CliError::Message(format!(
            "unsupported format `{format}`; use png, jpg, jpeg, or webp"
        ))
    })
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

// ===========================================================================
// Shared helpers for the AI CLI slices (DENOISE / CULL / FACE).
// ===========================================================================

/// Resolves the active virtual copy. Without `--virtual-copy` the first
/// (standard) copy is used; an unknown id is a loud error.
fn active_copy_index(
    document: &SidecarDocument,
    virtual_copy: Option<&str>,
) -> Result<usize, CliError> {
    match virtual_copy {
        Some(id) => document
            .virtual_copies
            .iter()
            .position(|copy| copy.id == id)
            .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{id}`"))),
        None => {
            if document.virtual_copies.is_empty() {
                Err(CliError::Message(
                    "sidecar has no virtual copies; run `import` first".into(),
                ))
            } else {
                Ok(0)
            }
        }
    }
}

// ===========================================================================
// LRPAR-G14-DENOISE-IMPL-20 (CLI slice): `denoise`.
//
// SOLL: `feature/decisions/LRPAR-G14-DENOISE-20.md` §6 and
// `feature/architecture/pipeline.md` §F-096a. The command is the CLI consumer
// of the core KI-Denoise stage: it reports the visible §6 status, renders
// through `render_frame_with_denoise` with an explicit `DenoisePolicy` (default
// `Warn`, so a non-ready stage surfaces a stderr warning and exits 0 — R2) and
// can explicitly record an externally produced `denoise_rgb` artifact
// (producer provenance via `set_denoise_producer_provenance` — R1/B2). It never
// recomputes automatically and never falls back silently.
// ===========================================================================

/// CLI-facing KI-Denoise fallback policy (R2): `warn` is the §6-Exit-0 default
/// (visible stderr warning, manual F-096 fallback), `strict` aborts loudly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliDenoisePolicy {
    Warn,
    Strict,
}

impl CliDenoisePolicy {
    fn to_policy(self) -> DenoisePolicy {
        match self {
            Self::Warn => DenoisePolicy::Warn,
            Self::Strict => DenoisePolicy::Strict,
        }
    }
}

/// KI-Denoise stage consumer. Exactly one action may be requested:
/// `--status` (default, read-only), `--render` or `--record-rgb`. Combining
/// them is a usage error (exit 2), never a silent precedence pick.
#[derive(Debug, Args)]
struct DenoiseArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    virtual_copy: Option<String>,
    /// Report the KI-Denoise stage status and exit (read-only; the default
    /// when neither `--render` nor `--record-rgb` is given).
    #[arg(long)]
    status: bool,
    /// Render through the KI-Denoise-aware pipeline to `--output`.
    #[arg(long)]
    render: bool,
    #[arg(long)]
    output: Option<PathBuf>,
    /// Output format for `--render` (`png|jpg|jpeg|webp`). Drives the output
    /// extension and the encoder, exactly like `render --format` (M1).
    #[arg(long, default_value = "png")]
    format: String,
    /// Encoder quality for `--render` (`1..=100`).
    #[arg(long, default_value_t = 90)]
    quality: u8,
    /// Explicit producer: import an externally produced denoised RGB image
    /// (PNG/JPEG/WebP, same geometry as the source) and persist it as the
    /// `denoise_rgb` bundle record plus the recipe/artifact reference.
    #[arg(long, value_name = "RGB")]
    record_rgb: Option<PathBuf>,
    /// Explicit re-record: replace an existing `denoise_rgb` record.
    #[arg(long)]
    force: bool,
    /// Model name for `--record-rgb` (identity-bearing).
    #[arg(long)]
    model_name: Option<String>,
    /// Model version for `--record-rgb` (identity-bearing).
    #[arg(long)]
    model_version: Option<String>,
    /// Model hash for `--record-rgb` (`sha256:<64 hex>` or
    /// `pending-integration`).
    #[arg(long)]
    model_hash: Option<String>,
    /// Input-spec digest for `--record-rgb` (`sha256:<64 hex>`).
    #[arg(long)]
    input_spec_digest: Option<String>,
    #[arg(long, default_value_t = 1.0)]
    strength: f32,
    #[arg(long, default_value_t = 0.0)]
    preserve_detail: f32,
    #[arg(long, value_enum, default_value = "warn")]
    denoise_policy: CliDenoisePolicy,
    #[arg(long)]
    json: bool,
}

/// Resolved view of the KI-Denoise stage (status + verified artifact + the
/// identity fields reported to the user).
struct DenoiseResolution {
    status: DenoiseStageStatus,
    reason: String,
    artifact: Option<CoreDenoiseRgbArtifact>,
    model_name: String,
    model_version: String,
    model_hash: String,
    input_spec_digest: String,
    artifact_checksum: Option<String>,
}

impl DenoiseResolution {
    fn inactive() -> Self {
        Self {
            status: DenoiseStageStatus::Inactive,
            reason: String::new(),
            artifact: None,
            model_name: String::new(),
            model_version: String::new(),
            model_hash: String::new(),
            input_spec_digest: String::new(),
            artifact_checksum: None,
        }
    }
}

/// String form of the live decode context used by the §6 identity comparison.
/// Producer (`--record-rgb`) and consumer (`--status`/`--render`) both derive
/// it from `SidecarDocument::source`, so a moved or re-decoded source is
/// detectable.
fn denoise_decode_fingerprint(source: &SourceIdentity) -> String {
    format!(
        "{}:{}:{}x{}",
        source.decode_fingerprint.decoder,
        source.decode_fingerprint.version,
        source.geometry_fingerprint.width,
        source.geometry_fingerprint.height
    )
}

/// Deterministic bundle record id for a `denoise_rgb` checksum (content
/// derived, never positional — same convention as the generative records).
fn denoise_record_id(checksum: &str) -> String {
    let prefix = checksum.get(..16).unwrap_or(checksum);
    format!("denoise_rgb:{prefix}")
}

fn denoise_status_reason(status: DenoiseStageStatus, zdata: &Path) -> String {
    match status {
        DenoiseStageStatus::Inactive | DenoiseStageStatus::Ready => String::new(),
        DenoiseStageStatus::Unavailable => {
            "model hash is `pending-integration` (no licence-pinned weights)".into()
        }
        DenoiseStageStatus::Missing => {
            format!("no `denoise_rgb` record in `{}`", zdata.display())
        }
        DenoiseStageStatus::Corrupt => {
            "artifact checksum does not match the recipe reference".into()
        }
        DenoiseStageStatus::Stale => {
            "source/decode/model/input-spec or the persisted producer provenance changed".into()
        }
    }
}

/// Classifies the §6 status of the active `denoise_ai` stage from the recipe
/// and the `.lumina.zdata` bundle. Pure read; never writes and never
/// recomputes.
fn denoise_resolution(
    input: &Path,
    document: &SidecarDocument,
    recipe: &EditRecipe,
) -> Result<DenoiseResolution, CliError> {
    let Some(denoise) = recipe.denoise_ai.as_ref() else {
        return Ok(DenoiseResolution::inactive());
    };
    let mut resolution = DenoiseResolution {
        status: DenoiseStageStatus::Inactive,
        reason: String::new(),
        artifact: None,
        model_name: denoise.model.name.clone(),
        model_version: denoise.model.version.clone(),
        model_hash: denoise.model.model_hash.clone(),
        input_spec_digest: denoise.input_spec_digest.clone(),
        artifact_checksum: None,
    };
    if denoise.is_identity() {
        return Ok(resolution);
    }

    let zdata = zdata_path_for(input);
    let mut artifact_present = false;
    let mut artifact_checksum = String::new();
    let mut artifact: Option<CoreDenoiseRgbArtifact> = None;
    let mut corrupt_reason: Option<String> = None;
    if zdata.exists() {
        match load_zdata(&zdata) {
            Ok(container) => match container.decode_all() {
                Ok(records) => {
                    for record in records {
                        if let RecordSpec::DenoiseRgb(record) = record {
                            let checksum = record.checksum();
                            let matches = denoise
                                .artifact
                                .as_ref()
                                .is_some_and(|reference| reference.checksum == checksum);
                            if !artifact_present || matches {
                                artifact_present = true;
                                artifact_checksum = checksum;
                                artifact = CoreDenoiseRgbArtifact::new(
                                    record.width,
                                    record.height,
                                    record.pixels,
                                )
                                .ok();
                            }
                            if matches {
                                break;
                            }
                        }
                    }
                }
                Err(error) => corrupt_reason = Some(error.to_string()),
            },
            Err(error) => corrupt_reason = Some(error.to_string()),
        }
    }
    if let Some(error) = corrupt_reason {
        resolution.status = DenoiseStageStatus::Corrupt;
        resolution.reason = format!(
            "`denoise_rgb` bundle `{}` is unreadable or corrupt: {error}",
            zdata.display()
        );
        return Ok(resolution);
    }

    // R1: `None` producer provenance is the default identity and is therefore
    // loudly `stale`, never silently `ready`.
    let current = DenoiseIdentity {
        source_content_hash: document.source.content_hash.clone(),
        decode_fingerprint: denoise_decode_fingerprint(&document.source),
        model_name: denoise.model.name.clone(),
        model_version: denoise.model.version.clone(),
        model_hash: denoise.model.model_hash.clone(),
        input_spec_digest: denoise.input_spec_digest.clone(),
        artifact_checksum: artifact_checksum.clone(),
    };
    let recorded = denoise_producer_provenance(denoise).unwrap_or_default();
    let status = resolve_denoise_status(denoise, &current, &recorded, artifact_present);
    resolution.reason = denoise_status_reason(status, &zdata);

    if status == DenoiseStageStatus::Ready {
        // A ready stage must describe exactly this frame; a dimension mismatch
        // is corrupt (loud), never a silent drop.
        match artifact.as_ref() {
            Some(artifact)
                if artifact.width != document.source.geometry_fingerprint.width
                    || artifact.height != document.source.geometry_fingerprint.height =>
            {
                resolution.status = DenoiseStageStatus::Corrupt;
                resolution.reason = format!(
                    "denoise artifact {}x{} does not match the source geometry {}x{}",
                    artifact.width,
                    artifact.height,
                    document.source.geometry_fingerprint.width,
                    document.source.geometry_fingerprint.height
                );
                return Ok(resolution);
            }
            Some(_) => {}
            None => {
                resolution.status = DenoiseStageStatus::Corrupt;
                resolution.reason =
                    "a `denoise_rgb` record was found but its payload is unusable".into();
                return Ok(resolution);
            }
        }
    }

    resolution.status = status;
    resolution.artifact = artifact;
    resolution.artifact_checksum = (!artifact_checksum.is_empty()).then_some(artifact_checksum);
    Ok(resolution)
}

fn denoise(args: DenoiseArgs) -> Result<(), CliError> {
    // F5: `--status`, `--render` and `--record-rgb` are three alternative
    // actions. More than one used to win silently by hard-coded precedence
    // (`--record-rgb` > `--render` > `--status`), so a mistyped invocation
    // could render or record when the caller only wanted a status report.
    // Reject the combination as a usage error (exit 2) before any work.
    let action_count = usize::from(args.status)
        + usize::from(args.render)
        + usize::from(args.record_rgb.is_some());
    if action_count > 1 {
        return Err(CliError::Usage(
            "--status, --render and --record-rgb are mutually exclusive; pick exactly one action"
                .into(),
        ));
    }
    let sidecar = sidecar_path_for(&args.input);
    let mut document = load_sidecar(&sidecar)?;
    let copy_index = active_copy_index(&document, args.virtual_copy.as_deref())?;

    if let Some(rgb) = args.record_rgb.clone() {
        return denoise_record_rgb(&args, &mut document, copy_index, &rgb, &sidecar);
    }

    let recipe = document.virtual_copies[copy_index].recipe.clone();
    let resolution = denoise_resolution(&args.input, &document, &recipe)?;

    if args.render {
        // The denoise-aware entry point takes the whole `RenderContext`; mask
        // layers are resolved by the shared render path, which is deliberately
        // not duplicated here. A masked recipe is refused loudly instead of
        // silently rendering without its masks.
        if !document.virtual_copies[copy_index].mask_layers.is_empty() {
            return Err(CliError::Message(
                "denoise --render does not resolve mask layers; masked denoise rendering \
                 awaits the shared-path wiring (no silent render without masks)"
                    .into(),
            ));
        }
        return denoise_render(&args, &recipe, &resolution);
    }

    let status = resolution.status.as_str();
    let payload = serde_json::json!({
        "command": "denoise",
        "action": "status",
        "input": args.input,
        "virtual_copy": document.virtual_copies[copy_index].id,
        "status": status,
        "reason": resolution.reason,
        "model": {
            "name": resolution.model_name,
            "version": resolution.model_version,
            "model_hash": resolution.model_hash,
            "input_spec_digest": resolution.input_spec_digest,
        },
        "artifact_checksum": resolution.artifact_checksum,
    });
    let text = if resolution.reason.is_empty() {
        format!("denoise_ai {status}")
    } else {
        format!("denoise_ai {status}: {}", resolution.reason)
    };
    emit(args.json, payload, &text)?;
    if resolution.status == DenoiseStageStatus::Corrupt {
        return Err(CliError::Message(
            "denoise_ai artifact is corrupt; explicit re-record/re-inference required (never automatic)"
                .into(),
        ));
    }
    if matches!(
        resolution.status,
        DenoiseStageStatus::Stale | DenoiseStageStatus::Missing | DenoiseStageStatus::Unavailable
    ) {
        // §6/R2: visible stderr warning, exit 0 (the render falls back to the
        // manual F-096 noise reduction). `warn!` also reaches the CLI logger.
        eprintln!(
            "warning: denoise_ai is {status}: {}; render falls back to manual noise reduction \
             (F-096) — visible, not silent",
            resolution.reason
        );
    }
    Ok(())
}

fn denoise_render(
    args: &DenoiseArgs,
    recipe: &EditRecipe,
    resolution: &DenoiseResolution,
) -> Result<(), CliError> {
    let output = args
        .output
        .clone()
        .ok_or_else(|| CliError::Message("denoise --render requires --output".into()))?;
    // M1: honor `--format`/`--quality` exactly like the neighboring `render`
    // command — the requested format drives the output extension AND the
    // encoder options, instead of being validated and then discarded while the
    // format is guessed from the output extension.
    let format = parse_output_format(&args.format)?;
    validate_quality(args.quality)?;
    let output = output.with_extension(format_extension(&args.format));
    reject_protected_output(&args.input, &output)?;
    let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
    let (frame, raw) = decode_input(&args.input, &bytes)?;
    let source_actions = resolve_source_actions(recipe, &zdata_path_for(&args.input))?;
    let camera_white_balance = raw
        .as_ref()
        .and_then(|metadata| sanitize_camera_white_balance(metadata.camera_white_balance));

    let policy = args.denoise_policy.to_policy();
    let stage_input = match resolution.status {
        DenoiseStageStatus::Ready => match resolution.artifact.as_ref() {
            Some(artifact) => DenoiseStageInput::ready(artifact).with_policy(policy),
            None => DenoiseStageInput::non_ready(
                DenoiseStageStatus::Corrupt,
                "denoise_ai resolved `ready` but carried no artifact",
            )
            .with_policy(policy),
        },
        other => DenoiseStageInput::non_ready(other, resolution.reason.clone()).with_policy(policy),
    };
    let render_ctx = RenderContext {
        recipe,
        camera_white_balance,
        source_actions: &source_actions,
        masks: None,
        lensfun: None,
        depth: None,
    };
    let rendered = render_frame_with_denoise(&frame, &render_ctx, &stage_input)?;
    if resolution.status != DenoiseStageStatus::Ready {
        eprintln!(
            "warning: denoise_ai is {}: {}; rendered with the manual noise-reduction fallback \
             (F-096) — visible, not silent",
            resolution.status.as_str(),
            resolution.reason
        );
    }
    let options = ExportOptions {
        format,
        quality: args.quality,
        dither: false,
        ..Default::default()
    };
    write_atomically(&output, &rendered.frame.encode_with_options(options)?)?;
    emit(
        args.json,
        serde_json::json!({
            "command": "denoise",
            "action": "render",
            "output": output,
            "format": args.format,
            "quality": args.quality,
            "status": resolution.status.as_str(),
            "reason": resolution.reason,
            "policy": match policy {
                DenoisePolicy::Warn => "warn",
                DenoisePolicy::Strict => "strict",
            },
        }),
        "rendered",
    )
}

fn denoise_record_rgb(
    args: &DenoiseArgs,
    document: &mut SidecarDocument,
    copy_index: usize,
    rgb_path: &Path,
    sidecar: &Path,
) -> Result<(), CliError> {
    let require = |value: &Option<String>, flag: &str| {
        value
            .clone()
            .ok_or_else(|| CliError::Message(format!("denoise --record-rgb requires --{flag}")))
    };
    let model_name = require(&args.model_name, "model-name")?;
    let model_version = require(&args.model_version, "model-version")?;
    let model_hash = require(&args.model_hash, "model-hash")?;
    let input_spec_digest = require(&args.input_spec_digest, "input-spec-digest")?;

    let source_bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
    let (frame, _raw) = decode_input(&args.input, &source_bytes)?;
    let rgb_bytes = fs::read(rgb_path).map_err(|error| io_error(rgb_path, error))?;
    let rgb_frame = ImageFrame::decode(&rgb_bytes)?;
    if rgb_frame.width != frame.width || rgb_frame.height != frame.height {
        return Err(CliError::Message(format!(
            "--record-rgb artifact is {}x{} but the source is {}x{}; a denoise artifact must \
             describe exactly the source geometry",
            rgb_frame.width, rgb_frame.height, frame.width, frame.height
        )));
    }
    let mut pixels = Vec::with_capacity(rgb_frame.pixels.len() / 4 * 3);
    for pixel in rgb_frame.pixels.as_chunks::<4>().0 {
        pixels.extend_from_slice(&pixel[..3]);
    }
    let core_artifact = CoreDenoiseRgbArtifact::new(rgb_frame.width, rgb_frame.height, pixels)?;
    let checksum = core_artifact.checksum();
    let record_id = denoise_record_id(&checksum);
    let zdata = zdata_path_for(&args.input);
    let record = SidecarDenoiseRgbArtifact {
        id: record_id.clone(),
        width: core_artifact.width,
        height: core_artifact.height,
        pixels: core_artifact.pixels.clone(),
    };
    save_denoise_rgb(&zdata, record, args.force).map_err(|error| {
        CliError::Message(format!(
            "could not write `denoise_rgb` bundle `{}`: {error}",
            zdata.display()
        ))
    })?;

    let relative_path = zdata
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("bundle.lumina.zdata")
        .to_string();
    let mut denoise = DenoiseAi {
        version: DENOISE_AI_VERSION,
        enabled: true,
        model: DenoiseModelIdentity {
            name: model_name.clone(),
            version: model_version.clone(),
            model_hash: model_hash.clone(),
            extras: Default::default(),
        },
        input_spec_digest: input_spec_digest.clone(),
        strength: args.strength,
        preserve_detail: args.preserve_detail,
        artifact: Some(DenoiseArtifactRef {
            kind: DenoiseArtifactKind::DenoiseRgb,
            relative_path,
            format: "lumina-zdata".into(),
            checksum: checksum.clone(),
            width: core_artifact.width,
            height: core_artifact.height,
            channels: "rgb8".into(),
            data_version: "1".into(),
            extras: Default::default(),
        }),
        extras: Default::default(),
    };
    denoise.validate()?;
    // R1/B2: persist the producer identity (source/decode/model/input-spec +
    // artifact checksum) so a later consumer proves the artifact belongs to
    // exactly this context; an absent provenance is loudly `stale`.
    set_denoise_producer_provenance(
        &mut denoise,
        &DenoiseIdentity {
            source_content_hash: document.source.content_hash.clone(),
            decode_fingerprint: denoise_decode_fingerprint(&document.source),
            model_name,
            model_version,
            model_hash,
            input_spec_digest,
            artifact_checksum: checksum.clone(),
        },
    );
    document.virtual_copies[copy_index].recipe.denoise_ai = Some(denoise);
    document.validate()?;
    save_sidecar(sidecar, document)?;
    info!(
        "denoise_rgb artifact recorded: bundle={} record={record_id} checksum={checksum}",
        zdata.display()
    );
    emit(
        args.json,
        serde_json::json!({
            "command": "denoise",
            "action": "record-rgb",
            "input": args.input,
            "virtual_copy": document.virtual_copies[copy_index].id,
            "bundle": zdata,
            "record": record_id,
            "checksum": checksum,
            "width": core_artifact.width,
            "height": core_artifact.height,
            "status": "ok",
        }),
        "recorded denoise_rgb artifact",
    )
}

// ===========================================================================
// LRPAR-G09-CULL-IMPL-25 (CLI slice): `cull`.
//
// SOLL: `feature/decisions/LRPAR-G09-CULL-25.md` §2/§5/§7.3. Assisted culling
// over an explicit selection: `--analyze` runs the deterministic Stage-1
// heuristic, persists the source-level proposal and never writes
// rating/flag/label; `--status` reports the persisted state read-only.
// Multi-item failures are isolated (exit 3), a hard error is exit 1.
// ===========================================================================

#[derive(Debug, Args)]
struct CullArgs {
    /// Explicit selection (repeatable). A file is one item; a directory
    /// expands to its supported images (sorted, non-recursive).
    #[arg(long = "input", required = true)]
    input: Vec<PathBuf>,
    /// Report the persisted proposal status and exit (read-only; the default
    /// when `--analyze` is absent).
    #[arg(long)]
    status: bool,
    /// Explicitly analyze the selection and persist the proposals.
    #[arg(long)]
    analyze: bool,
    /// Re-analyze even when a valid, identity-matching proposal already exists.
    #[arg(long)]
    force: bool,
    #[arg(long)]
    json: bool,
}

/// Expands the explicit selection (files and directories) and deduplicates by
/// path while keeping the caller's order. Never walks symlinks/loops.
fn cull_selection(inputs: &[PathBuf]) -> Result<Vec<PathBuf>, CliError> {
    let mut selection: Vec<PathBuf> = Vec::new();
    for input in inputs {
        if input.is_dir() {
            let mut entries: Vec<PathBuf> = fs::read_dir(input)
                .map_err(|error| io_error(input, error))?
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.is_file() && has_image_extension(path))
                .collect();
            entries.sort();
            selection.extend(entries);
        } else {
            selection.push(input.clone());
        }
    }
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    selection.retain(|path| seen.insert(path.clone()));
    if selection.is_empty() {
        return Err(CliError::Message(
            "cull selection is empty (no supported images found)".into(),
        ));
    }
    Ok(selection)
}

fn cull(args: CullArgs) -> Result<(), CliError> {
    if args.status && args.analyze {
        return Err(CliError::Usage(
            "--status and --analyze are mutually exclusive".into(),
        ));
    }
    let selection = cull_selection(&args.input)?;
    if args.analyze {
        cull_analyze(&args, &selection)
    } else {
        cull_status(&args, &selection)
    }
}

fn cull_status(args: &CullArgs, selection: &[PathBuf]) -> Result<(), CliError> {
    let config = CullConfig::default();
    let mut items = Vec::with_capacity(selection.len());
    let mut failed = 0usize;
    for input in selection {
        match cull_status_one(input, &config) {
            Ok(item) => items.push(item),
            Err(error) => {
                failed += 1;
                eprintln!("error: cull status `{}` failed: {error}", input.display());
                items.push(serde_json::json!({
                    "input": input,
                    "status": "failed",
                    "error": error.to_string(),
                }));
            }
        }
    }
    emit(
        args.json,
        serde_json::json!({"command": "cull", "action": "status", "items": items}),
        "cull status",
    )?;
    if failed > 0 {
        return Err(CliError::Partial {
            command: "cull",
            failed,
        });
    }
    Ok(())
}

fn cull_status_one(input: &Path, config: &CullConfig) -> Result<serde_json::Value, CliError> {
    let bytes = fs::read(input).map_err(|error| io_error(input, error))?;
    let (frame, _raw) = decode_input(input, &bytes)?;
    let document = load_sidecar(&sidecar_path_for(input))?;
    let analysis_frame = lumina_core::downscale_bilinear(&frame, config.analysis_max_width)?;
    let current = heuristic_identity(
        cull_cli::source_fingerprint(&bytes),
        document.source.decode_fingerprint.clone(),
        document.source.geometry_fingerprint.clone(),
        Resolution {
            width: analysis_frame.width,
            height: analysis_frame.height,
            extras: Default::default(),
        },
    );
    let (status, section) = match evaluate_culling(&document, &current) {
        CullingReadState::NoProposal => ("no-proposal", serde_json::json!(null)),
        CullingReadState::Valid(section) => ("valid", cull_section_json(&section, &[])),
        CullingReadState::Stale {
            section,
            mismatches,
        } => {
            let mismatch_labels: Vec<String> =
                mismatches.iter().map(|m| format!("{m:?}")).collect();
            ("stale", cull_section_json(&section, &mismatch_labels))
        }
        CullingReadState::Unusable { section } => ("unusable", cull_section_json(&section, &[])),
    };
    Ok(serde_json::json!({
        "input": input,
        "status": status,
        "proposal": section,
    }))
}

fn cull_section_json(
    section: &lumina_sidecar::CullingSection,
    mismatches: &[String],
) -> serde_json::Value {
    serde_json::json!({
        "proposal": section.proposal,
        "score": section.score,
        "reasons": section.reasons,
        "created_at": section.created_at,
        "status": section.status,
        "analyzer": {
            "kind": section.identity.analyzer.kind,
            "name": section.identity.analyzer.name,
            "version": section.identity.analyzer.version,
        },
        "mismatches": mismatches,
    })
}

fn cull_analyze(args: &CullArgs, selection: &[PathBuf]) -> Result<(), CliError> {
    let config = CullConfig::default();
    let mut frames: Vec<ImageFrame> = Vec::new();
    let mut isos: Vec<Option<u32>> = Vec::new();
    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut source_fingerprints: Vec<SourceFingerprint> = Vec::new();
    let mut items: Vec<serde_json::Value> = Vec::new();
    let mut failed = 0usize;

    for input in selection {
        let decoded = fs::read(input)
            .map_err(|error| io_error(input, error))
            .and_then(|bytes| {
                let source = cull_cli::source_fingerprint(&bytes);
                decode_input(input, &bytes).map(|decoded| (decoded, source))
            });
        match decoded {
            Ok(((frame, raw), source)) => {
                isos.push(
                    raw.as_ref()
                        .and_then(|metadata| metadata.iso)
                        .map(|iso| iso.round().max(0.0) as u32),
                );
                frames.push(frame);
                inputs.push(input.clone());
                source_fingerprints.push(source);
            }
            Err(error) => {
                failed += 1;
                eprintln!("error: cull analysis `{}` failed: {error}", input.display());
                items.push(serde_json::json!({
                    "input": input,
                    "status": "failed",
                    "error": error.to_string(),
                }));
            }
        }
    }
    if frames.is_empty() {
        return Err(CliError::Message(format!(
            "cull analysis: no decodable inputs ({failed} failed)"
        )));
    }

    let sources: Vec<CullSourceInput<'_>> = frames
        .iter()
        .zip(isos.iter())
        .map(|(frame, iso)| CullSourceInput { frame, iso: *iso })
        .collect();
    let selection_analysis = analyze_selection(&sources, &config)?;

    for (index, analysis) in selection_analysis.images.iter().enumerate() {
        let input = inputs[index].clone();
        match cull_cli::persist_one(&input, &source_fingerprints[index], analysis, args.force) {
            Ok(item) => items.push(item),
            Err(error) => {
                failed += 1;
                eprintln!("error: cull persist `{}` failed: {error}", input.display());
                items.push(serde_json::json!({
                    "input": input,
                    "status": "failed",
                    "error": error.to_string(),
                }));
            }
        }
    }

    emit(
        args.json,
        serde_json::json!({
            "command": "cull",
            "action": "analyze",
            "items": items,
            "groups": selection_analysis.groups,
        }),
        "cull analysis",
    )?;
    if failed > 0 {
        return Err(CliError::Partial {
            command: "cull",
            failed,
        });
    }
    Ok(())
}

// ===========================================================================
// LRPAR-G12-FACE-IMPL-20 / S4: `face`.
//
// SOLL: `feature/decisions/LRPAR-G12-FACE-20.md` §2.3/§4/§6. `--status`
// re-evaluates the persisted source-level analysis against the live
// source/decode/model context (visible valid/stale/missing/corrupt); `--analyze`
// runs the real ONNX face engine loudly (`try_load_face_engine`) — without the
// `onnx-rt` capability the command refuses with exit 1 and writes nothing (no
// silent stub fallback). Person names are never synthesized.
// ===========================================================================

#[derive(Debug, Args)]
struct FaceArgs {
    #[arg(long)]
    input: PathBuf,
    /// Report the persisted analysis status and exit (read-only; the default
    /// when `--analyze` is absent).
    #[arg(long)]
    status: bool,
    /// Run the real face engine and persist the analysis.
    #[arg(long)]
    analyze: bool,
    /// Re-analyze even when a valid, identity-matching analysis already exists.
    #[arg(long)]
    force: bool,
    /// Real detection model artifact (`.onnx`); falls back to
    /// `LUMINA_FACE_DETECT_MODEL_PATH`.
    #[arg(long)]
    detector: Option<PathBuf>,
    /// Real embedding model artifact (`.onnx`); falls back to
    /// `LUMINA_FACE_EMBED_MODEL_PATH`.
    #[arg(long)]
    embedder: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

fn face_status_str(status: FaceArtifactStatus) -> &'static str {
    match status {
        FaceArtifactStatus::Valid => "valid",
        FaceArtifactStatus::Stale => "stale",
        FaceArtifactStatus::Missing => "missing",
        FaceArtifactStatus::Corrupt => "corrupt",
    }
}

/// Real evidence about the binary face artifacts a persisted analysis
/// references, derived from the `.lumina.zdata` records themselves — never
/// from mere existence.
///
/// This is the shared `lumina_sidecar::face_artifact_evidence` contract (FACE-20
/// §3.2), used verbatim by CLI and GUI, so both classify the same sidecar
/// identically:
///
/// - an analysis with no persisted embedding references, a missing bundle, a
///   missing `face_embedding` record or an unreadable file →
///   [`FaceArtifactEvidence::Missing`]
/// - a present record whose record checksum (BLAKE3 over the canonical
///   `encoding_version || dimension || f32` stream) differs from the persisted
///   `FaceVectorRef.checksum`, or whose dimension differs, or a bundle that is
///   not loadable → `Present { checksum_matches: false }` (classified
///   `corrupt`)
/// - every referenced record verified → `Present { checksum_matches: true }`
fn face_artifact_evidence(bundle_root: &Path, analysis: &FaceAnalysis) -> FaceArtifactEvidence {
    lumina_sidecar::face_artifact_evidence(bundle_root, analysis)
}

/// Stable machine label for the artifact evidence reported by `face --status`
/// (mirrors the GUI's evidence classification).
fn face_evidence_str(evidence: FaceArtifactEvidence) -> &'static str {
    match evidence {
        FaceArtifactEvidence::Missing => "missing",
        FaceArtifactEvidence::Present {
            checksum_matches: true,
        } => "checksum-verified",
        FaceArtifactEvidence::Present {
            checksum_matches: false,
        } => "checksum-mismatch",
    }
}

/// Bundle root (directory holding the source and its `.lumina.json`/`.zdata`),
/// used to resolve the analysis' relative artifact references.
fn face_bundle_root(input: &Path) -> &Path {
    input.parent().unwrap_or_else(|| Path::new("."))
}

/// Builds the live face identity (source/decode/geometry + the shipped
/// candidate model suite + clustering identity) for the status comparison.
fn face_current_identity(
    document: &SidecarDocument,
) -> Result<lumina_sidecar::FaceIdentity, CliError> {
    face_identity(
        &FaceModelSuite::candidate(),
        SourceFingerprint {
            content_hash: document.source.content_hash.clone(),
            byte_length: document.source.byte_length,
            extras: Default::default(),
        },
        document.source.decode_fingerprint.clone(),
        document.source.geometry_fingerprint.clone(),
        FaceClusteringParams::default().to_identity(),
        &FaceInferenceOptions::default(),
    )
    .map_err(|error| CliError::Message(format!("face identity error: {error}")))
}

fn face(args: FaceArgs) -> Result<(), CliError> {
    if args.status && args.analyze {
        return Err(CliError::Usage(
            "--status and --analyze are mutually exclusive".into(),
        ));
    }
    let path = sidecar_path_for(&args.input);
    let mut document = load_sidecar(&path)?;
    if args.analyze {
        face_analyze(&args, &mut document, &path)
    } else {
        face_status(&args, &document)
    }
}

fn face_status(args: &FaceArgs, document: &SidecarDocument) -> Result<(), CliError> {
    let Some(analysis) = document.face.as_ref() else {
        return emit(
            args.json,
            serde_json::json!({
                "command": "face",
                "action": "status",
                "input": args.input,
                "status": "no-analysis",
                "detections": 0,
            }),
            "face analysis: no analysis",
        );
    };
    let current = face_current_identity(document)?;
    // M2: real evidence from the record checksums (BLAKE3 per referenced
    // `face_embedding` record), identical to the GUI classification. A
    // persisted `valid` analysis whose
    // references are absent/empty/tampered must never be reported `valid`.
    let evidence = face_artifact_evidence(face_bundle_root(&args.input), analysis);
    let status = if analysis.status != FaceArtifactStatus::Valid {
        analysis.status
    } else {
        face_artifact_status(&current, &analysis.identity, evidence)
    };
    emit(
        args.json,
        serde_json::json!({
            "command": "face",
            "action": "status",
            "input": args.input,
            "status": face_status_str(status),
            "detections": analysis.detections.len(),
            "embeddings": analysis.embeddings.len(),
            "clusters": analysis.clusters.len(),
            "persons": analysis.persons.len(),
            "created_at": analysis.created_at,
            "artifact_evidence": face_evidence_str(evidence),
        }),
        &format!("face analysis: {}", face_status_str(status)),
    )?;
    if status == FaceArtifactStatus::Corrupt {
        return Err(CliError::Message(
            "face analysis is corrupt; explicit `--analyze` required (never automatic)".into(),
        ));
    }
    Ok(())
}

fn face_analyze(
    args: &FaceArgs,
    document: &mut SidecarDocument,
    _sidecar: &Path,
) -> Result<(), CliError> {
    let current = face_current_identity(document)?;
    // No automatic re-inference: a valid, identity-matching analysis with
    // verified artifact references is kept unless `--force`. M2: the check
    // uses the same real BLAKE3 evidence as `--status`, so an analysis whose
    // referenced vectors are missing/empty/tampered is re-run instead of being
    // reported as an unchanged valid result.
    if !args.force {
        if let Some(existing) = document.face.as_ref() {
            let evidence = face_artifact_evidence(face_bundle_root(&args.input), existing);
            if existing.status == FaceArtifactStatus::Valid
                && face_artifact_status(&current, &existing.identity, evidence)
                    == FaceArtifactStatus::Valid
            {
                return emit(
                    args.json,
                    serde_json::json!({
                        "command": "face",
                        "action": "analyze",
                        "input": args.input,
                        "status": "valid",
                        "changed": false,
                    }),
                    "face analysis already valid (use --force to re-run)",
                );
            }
        }
    }

    let detector = args
        .detector
        .clone()
        .or_else(|| std::env::var_os("LUMINA_FACE_DETECT_MODEL_PATH").map(PathBuf::from))
        .ok_or_else(|| {
            CliError::Message(
                "face --analyze requires --detector or LUMINA_FACE_DETECT_MODEL_PATH".into(),
            )
        })?;
    let embedder = args
        .embedder
        .clone()
        .or_else(|| std::env::var_os("LUMINA_FACE_EMBED_MODEL_PATH").map(PathBuf::from))
        .ok_or_else(|| {
            CliError::Message(
                "face --analyze requires --embedder or LUMINA_FACE_EMBED_MODEL_PATH".into(),
            )
        })?;
    let suite = FaceModelSuite::candidate();
    let options = FaceInferenceOptions::default();
    let engine = try_load_face_engine(&detector, &suite, &embedder, &options)
        .map_err(|error| CliError::Message(format!("face engine load failed: {error}")))?;

    match engine {
        FaceOnnxEngine::RuntimeDisabled => Err(CliError::Message(
            "face analysis unavailable: `onnx-rt` is not compiled into this build; \
             no analysis was produced (no silent stub fallback)"
                .into(),
        )),
        #[cfg(feature = "onnx-rt")]
        FaceOnnxEngine::OnnxRuntime { detector, embedder } => {
            let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
            let (frame, _raw) = decode_input(&args.input, &bytes)?;
            let detections = detector
                .detect(&frame)
                .map_err(|error| CliError::Message(format!("face detection failed: {error}")))?;
            let vectors = embedder
                .embed(&frame, &detections)
                .map_err(|error| CliError::Message(format!("face embedding failed: {error}")))?;
            if vectors.len() != detections.len() {
                return Err(CliError::Message(format!(
                    "face embedder returned {} vectors for {} detections",
                    vectors.len(),
                    detections.len()
                )));
            }
            let raw_vectors: Vec<Vec<f32>> = vectors
                .iter()
                .map(|vector| vector.values().to_vec())
                .collect();
            let labels = cluster_embeddings(&raw_vectors, &FaceClusteringParams::default())
                .map_err(|error| CliError::Message(format!("face clustering failed: {error}")))?;
            let detection_ids: Vec<String> = detections.iter().map(detected_face_id).collect();
            let clusters = clusters_from_labels(&detection_ids, &labels).map_err(|error| {
                CliError::Message(format!("face cluster build failed: {error}"))
            })?;
            // FACE-20 §3.2: persist the normalized vectors as `face_embedding`
            // records in the shared bundle *before* the JSON sidecar records
            // their references, so a failure can never leave a dangling
            // reference. The record checksum is the persisted reference
            // checksum (BLAKE3 over the canonical raw vector stream).
            let zdata_path = zdata_path_for(&args.input);
            let relative_path = zdata_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| CliError::Message("face bundle path is not valid UTF-8".into()))?
                .to_string();
            let mut records = Vec::with_capacity(detections.len());
            let mut embeddings = Vec::with_capacity(detections.len());
            for (index, vector) in vectors.iter().enumerate() {
                let detection_id = detection_ids
                    .get(index)
                    .expect("one vector per detection, checked above");
                let embedding_id = embedding_id_for(detection_id);
                let dimension = vector.dimension() as u32;
                let record = SidecarFaceEmbeddingArtifact {
                    id: embedding_id.clone(),
                    dimension,
                    values: vector.values().to_vec(),
                };
                embeddings.push(FaceEmbeddingRecord {
                    detection_index: index,
                    vector: vector.clone(),
                    reference: lumina_sidecar::FaceVectorRef {
                        relative_path: relative_path.clone(),
                        format: "lumina-zdata".into(),
                        checksum: record.checksum(),
                        dimension,
                        channels: "f32".into(),
                        data_version: "1".into(),
                        extras: Default::default(),
                    },
                });
                records.push(record);
            }
            if !records.is_empty() {
                save_face_embeddings(&zdata_path, records).map_err(|error| {
                    CliError::Message(format!("face vector persistence failed: {error}"))
                })?;
            }
            let output = FaceAnalysisOutput {
                identity: current,
                created_at: now_rfc3339_utc(),
                detections,
                embeddings,
                clusters,
                persons: vec![],
            };
            let analysis = output.into_sidecar().map_err(|error| {
                CliError::Message(format!("face analysis build failed: {error}"))
            })?;
            document.face = Some(analysis);
            document.validate()?;
            save_sidecar(_sidecar, document)?;
            let analysis = document.face.as_ref().expect("just assigned");
            info!(
                "face analysis persisted: detections={} embeddings={} clusters={}",
                analysis.detections.len(),
                analysis.embeddings.len(),
                analysis.clusters.len()
            );
            emit(
                args.json,
                serde_json::json!({
                    "command": "face",
                    "action": "analyze",
                    "input": args.input,
                    "status": "valid",
                    "changed": true,
                    "detections": analysis.detections.len(),
                    "clusters": analysis.clusters.len(),
                    "embeddings_persisted": !analysis.embeddings.is_empty(),
                }),
                "face analysis written",
            )
        }
    }
}

#[cfg(test)]
mod tests;
