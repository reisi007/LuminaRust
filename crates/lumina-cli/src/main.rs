use clap::{Args, Parser, Subcommand, ValueEnum};
use lumina_core::{
    denoise_producer_provenance, detect_red_eyes, downscale, export_image_with_generative,
    has_transparent_pixels, match_total_exposure_masked, render_frame, render_frame_with_denoise,
    render_frame_with_generative, resolve_denoise_status, resolve_mask_planes,
    set_denoise_producer_provenance, DenoiseIdentity, DenoisePolicy,
    DenoiseRgbArtifact as CoreDenoiseRgbArtifact, DenoiseStageInput, DenoiseStageStatus,
    DetectedRedEye, DownscaleTarget, ExportOptions, GenerativeCacheKey, GenerativeCanvasArtifact,
    GenerativeCanvasInput, GenerativeRole as CoreGenerativeRole, ImageFileFormat, ImageFrame,
    LensfunCorrectorRef, MaskContext, MaskInference, MaskLoadContext, MaskPlane, MaskPolicy,
    RenderContext, RenderOutput, SourceActionArtifact, RED_EYE_DETECT_ID_PREFIX,
};
// F-082-FOLLOWUP: under `onnx-rt` the CLI consumes the resolver surface
// `lumina_onnx::resolve::try_load_onnx_engine` (real engine or a hard error,
// never a stub). The deterministic `StubBackend` stays the wiring default for
// default builds (no `onnx-rt`); it is never substituted for a requested real
// engine. `birefnet_manifest` is the model identity/contract both paths share.
use lumina_onnx::birefnet_manifest;
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
use lumina_raw::RawError;
// GPU-first rendering path (wgpu/Metal). Optional capability: when the `gpu`
// feature is on, render/export/batch prefer the GPU adapter and fall back to the
// CPU pipeline when no adapter is present. Never compiled unless the feature is
// enabled, so the default build stays CPU-only (per `Agents.md` capability
// separation).
#[cfg(feature = "gpu")]
use lumina_gpu::{unsupported_gpu_stages_with_context, GpuContext};
// Visible backend-selection logging (no silent fallback to CPU).
use log::info;
#[allow(unused_imports)]
use lumina_sidecar::{
    append_repair_region, load_validated_source_action_bundle, load_zdata, zdata_path_for,
    RepairRegionArtifact, ZDATA_FILE_SUFFIX,
};
// LRPAR-G12-FACE-IMPL-20-REST: the `face_embedding` write path only exists in
// the `onnx-rt` build (the only build that can produce real vectors).
use lumina_sidecar::{
    apply_batch_op, artifact_status, default_meta_presets_dir, document_revision,
    is_metadata_field, load_meta_preset_file, load_sidecar, now_rfc3339_utc, render_meta_preset,
    resolve_meta_preset_path, save_denoise_rgb, save_sidecar, save_sidecar_if_unchanged,
    scan_meta_presets_dir, sidecar_path_for, validate_metadata_field_value, AiSelect, AiSelectKind,
    ArtifactStatus, BatchOp, CoordinateSystem, DecodeFingerprint, DenoiseAi, DenoiseArtifactKind,
    DenoiseArtifactRef, DenoiseModelIdentity, DenoiseRgbArtifact as SidecarDenoiseRgbArtifact,
    EditRecipe, ExportRecord, FaceAnalysis, FaceArtifactStatus, GeometryFingerprint, HistoryEntry,
    MaskDefinition, MaskLayer, MaskOperation, MaskPrompt, MaskReference, MaskStatus,
    MetaPresetEntry, MetaPresetFile, MetadataHistoryEntry, ModelIdentity, Preprocessing,
    PromptTransform, RecordSpec, RedEyeCorrection, RedEyeRegion, Resolution, SidecarDocument,
    SourceActionArtifactRef, SourceActionSpec, SourceFingerprint, SourceIdentity,
    DENOISE_AI_VERSION, MAX_KEYWORDS_PER_DOCUMENT, MAX_KEYWORD_CHARS, MAX_METADATA_HISTORY_ENTRIES,
    METADATA_FIELD_IDS, RED_EYE_MAX_REGIONS, SOURCE_ACTION_VERSION,
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
mod repair_region_input;

// LENSFUN-CALLER-37: the lens corrector construction and the process-lifetime
// diagnostics sink, extracted from main.rs for the same reason.
#[cfg(feature = "lensfun")]
mod lensfun_cli;
#[cfg(feature = "lensfun")]
use lensfun_cli::build_lensfun_corrector;
// AUTO-TONE-CLI-6: the single Auto-Tone write path shared by
// `process --auto-tone` and `regenerate --module auto-tone` (six sliders +
// six `auto_features` mirrors + analysis fingerprint), the preset/explicit-CLI
// slider layering and the `regenerate` freshness predicate. Extracted from
// `process_selected` for the file-size ratchet; the normative contract is
// `feature/architecture/pipeline.md` § Auto-Tone.
mod auto_tone_cli;
use auto_tone_cli::{apply_explicit_slider_values, apply_preset_layer, AutoToneDomain};
// MCP-PARITY-B: the Auto-Tone **write path** and the `regenerate` freshness
// predicate moved to `crates/lumina-stages/src/auto_tone.rs`, because
// `lumina-mcp` now reaches them through `lumina_regenerate op="auto_tone"` and
// a second copy could not stay consistent with the single writer. What stays in
// `auto_tone_cli` is the CLI-only preset/explicit-slider layering of
// `process --auto-tone`.
use lumina_stages::auto_tone::{apply_auto_tone_result, PersistedAutoTone};
// MASK-LOCAL-P0/P1.2a: typed local-adjustment flag parsing and mutation.
mod mask_cli;
use mask_cli::{mark_masks_pending_refresh, mask, preflight_masks};
mod mask_local;
mod mask_local_color;
mod mask_local_curves;
use mask_local::{mask_copy_mut, require_mask_name, resolve_mask_copy};
mod color_cli;
use color_cli::color;
mod denoise_cli;
mod meta_cli;
mod meta_clipboard_cli;
mod meta_preset_cli;
use denoise_cli::{denoise, DenoiseArgs};
#[cfg(test)]
use denoise_cli::{denoise_record_id, denoise_status_reason};
use meta_cli::meta;
#[cfg(test)]
pub(crate) use meta_clipboard_cli::{
    default_meta_clipboard_path, load_meta_clipboard, parse_meta_fields, MetaClipboardFile,
    META_CLIPBOARD_FORMAT, META_CLIPBOARD_VERSION,
};
use meta_clipboard_cli::{meta_copy, meta_paste};
use meta_preset_cli::{meta_preset_apply, meta_preset_list, meta_preset_show, meta_sync};
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
// GPU-RENDER-MASK-19 (Slice B2): CLI wiring of the GPU local-mask pixel pass.
#[cfg(feature = "gpu")]
mod local_mask_gpu;
// MCP-PARITY-A: the four session-based recipe stage editors (`spot`, `lens-blur`,
// `geometry`, `upright`). All of their logic lives in `lumina-stages`, which
// `lumina-mcp` calls too, so an MCP call and the equivalent CLI call share one
// implementation instead of two. This module holds only the `clap` argument
// structs, the `*Args -> *Request` mapping and the output formatting; the
// mutation/validation/listing code was removed from this file by the
// extraction (ratchet: this file may only shrink).
mod stages;
use stages::{
    decode_input, geometry, io_error, lens_blur, source_identity, spot, upright, GeometryArgs,
    LensBlurArgs, SpotArgs, UprightArgs,
};
// The infallible decode helpers moved to `lumina-stages` with the rest of the
// shared stage code (`lumina-cli` and `lumina-mcp` must agree on the source
// identity that `upright --analyze` persists).
use lumina_stages::decode::{is_raw_path, timestamp};
// MCP-PARITY-B: the five path-based / artefact commands (`collections`,
// `smart-collections`, `relocate`, `generative`, `regenerate`). Their logic
// lives in `lumina-stages` together with the stage editors, so the MCP tools
// reach the same implementation; this crate keeps only the `clap` argument
// structs, the `*Args -> *Request` mapping and the report printing.
mod library;
use library::{
    collections, generative, regenerate, relocate, smart_collections, CollectionsArgs,
    GenerativeArgs, RegenerateArgs, RelocateArgs, SmartCollectionsArgs,
};
// The shared path / render-context substrate (`require_sidecar`,
// `collect_target_sidecars`, `collect_sidecars`, `collect_tree_files`,
// `move_file_cross_volume`, `load_smart_catalog`, `resolve_source_actions`,
// `load_persisted_mask_planes`, `sanitize_camera_white_balance`) moved to
// `lumina-stages` with the commands that need it: a second copy on the MCP side
// would have to re-derive the same target list, the same cycle-safe walk and
// the same source-action checks.
use lumina_stages::paths::{collect_sidecars, collect_target_sidecars, require_sidecar};
use lumina_stages::pipeline::{
    load_persisted_mask_planes, resolve_source_actions, sanitize_camera_white_balance,
};
// The generative artefact helpers are shared with the render path, which still
// resolves the render-time artifacts in this file (`resolve_generative_artifacts`).
use lumina_stages::generative_artifact::{
    generative_expand_input, generative_identity, resolve_generative_role, stage_generative_input,
};
// MCP-PARITY-B: the Auto-Tone freshness predicate and its analysis fingerprint
// moved with `regenerate`; the in-crate tests address them directly.
#[cfg(test)]
use lumina_stages::auto_tone::{auto_tone_input_fingerprint, auto_tone_is_fresh};
// MCP-PARITY-B: the `--module` value set moved with `regenerate`.
#[cfg(test)]
use crate::library::ModuleArg;
#[cfg(test)]
use lumina_stages::regenerate::RegenerateModule;
// MCP-PARITY-B: the sidecar types the generative/membership tests build by hand
// used to be imported by the entry point; they now come from their own crate.
#[cfg(test)]
use lumina_sidecar::{Crop, GenerativeCanvas, GenerativeEdit};
// MCP-PARITY-B: the shared move/walk/tile-id helpers the in-crate tests call
// directly.
#[cfg(test)]
use lumina_stages::{paths::move_file_cross_volume, pipeline::zdata_mask_tile_id};

// `geometry --lensfun-status` report helper; reached from the tests only.
#[cfg(test)]
use stages::resolve_lensfun_report;
// MCP-PARITY-A: the stage value types the CLI tests assert on now live in
// the shared crate the stage editors were extracted into.
#[cfg(test)]
use lumina_sidecar::{AspectPreset, BokehShape};

// FILE-SIZE-1000-ALL-EXCEPTIONS-100: the former monolithic `main.rs` body is
// split by responsibility into the sibling modules below. Each one starts with
// `use super::*`, so the crate-root imports above stay the single import list
// and the moved bodies read exactly as before. Every former `crate::…` /
// `super::…` path kept working through the `use` lines in this block.
mod batch_cmd;
mod cli_args;
mod cli_log;
mod copy_selection;
mod cull_cmd;
mod develop_cmd;
mod face_cmd;
mod inspect_cmd;
mod process_cmd;
mod render_route;
mod retouch_cmd;
use batch_cmd::{batch, emit, has_image_extension, reindex};
use cli_args::*;
use cli_log::{init_cli_logger, log_backend};
use copy_selection::active_copy_index;
use cull_cmd::{cull, CullArgs};
use develop_cmd::{batch_meta, develop, export, import_file, keywords, previous, render, validate};
use face_cmd::{face, FaceArgs};
use inspect_cmd::{
    bake_metadata_into_jpeg, format_extension, inspect, output_format, parse_output_format,
    push_metadata_export_record, reject_protected_output, validate_format, validate_quality,
    write_atomically, MetadataWritten, StagedArtifact,
};
use process_cmd::{
    process, process_selected, resolve_generative_artifacts, resolve_mask_inference_engine,
};
use render_route::{
    render_standard_routed, render_standard_with_generative,
    render_standard_with_generative_report, RoutedRender,
};
use retouch_cmd::{dust_removal, red_eye};
// Test-only reach into the moved helpers (the in-crate tests call them
// directly; no production caller in the root uses these names).
#[cfg(test)]
use batch_cmd::{batch_one, collect_images, dedup_same_file_inputs};
#[cfg(test)]
use face_cmd::{face_analyze, face_artifact_evidence};
#[cfg(test)]
use inspect_cmd::reject_same_path;
#[cfg(all(test, feature = "onnx-rt"))]
use process_cmd::resolve_onnx_engine_from_path;
#[cfg(all(test, feature = "gpu"))]
use render_route::{gpu_routing_reasons, render_best_effort};
#[cfg(test)]
use retouch_cmd::parse_red_eye_region;

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
    /// MCP-PARITY-A: a failure of one of the four shared stage editors
    /// (`lumina-stages`, also used by `lumina-mcp`). `transparent` on purpose:
    /// every variant renders exactly the text the pre-extraction `CliError`
    /// variant rendered, so the CLI's stderr and exit codes are unchanged.
    #[error(transparent)]
    Stages(#[from] lumina_stages::StageError),
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

// ===========================================================================
// Shared helpers for the AI CLI slices (DENOISE / CULL / FACE).
// ===========================================================================

fn process_selected_with_report(
    args: ProcessArgs,
    quality: u8,
    virtual_copy: Option<&str>,
    policy: MaskPolicy,
    mask_warnings_out: &mut Vec<String>,
) -> Result<(Option<MetadataWritten>, RoutedRender), CliError> {
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
    // MVP rule (SOLL § Auto-Tone): auto values first, preset second, explicit
    // CLI values last, matching only after the render. All three layers go
    // through the shared Auto-Tone writer in `auto_tone_cli` — the layering is
    // the contract, and the six sliders are written as one group.
    let mut preset_sliders = BTreeMap::new();
    if let Some(path) = args.preset.as_ref() {
        preset_sliders = apply_preset_layer(&mut recipe, path, auto_requested)?;
    }
    // AUTO-TONE-ANALYSIS-INPUT-8: one source-action resolution shared by the
    // analysis frame and the render below. AFTER the preset layer, which
    // replaces `source_actions` wholesale — see `AutoToneDomain`.
    let auto_domain = AutoToneDomain::resolve(&args.input, &recipe)?;
    if auto_requested {
        // AUTO-TONE-ANALYSIS-INPUT-8: measure post SourceActions + post Crop
        // (without Adjustments), not the raw decode — the same production seam
        // the GUI uses, so CLI and GUI cannot drift onto different domains.
        let analysis_frame = auto_domain.analysis_frame(&frame, &recipe)?;
        apply_auto_tone_result(
            &mut recipe,
            &analysis_frame,
            args.target_luminance,
            PersistedAutoTone::ReuseIfComplete,
            Some(&preset_sliders),
        )?;
    }
    apply_explicit_slider_values(&mut recipe, &args);
    // --- F-048 / F-051: intelligent mask-loading decision layer ---
    // Load every persisted source-mask plane from the optional `.lumina.zdata`
    // bundle (regardless of status); the decision layer in lumina-core
    // validates identity and decides whether to use it, re-infer, or fail.
    // R2-CLI-05: a corrupt bundle surfaces as an explicit warning through the
    // same channel as the other mask warnings instead of being silent.
    let loaded_planes =
        load_persisted_mask_planes(&document, auto_domain.zdata_path(), mask_warnings_out);

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
    // Masks).  F-042-N1: the recipe's persisted source actions were resolved
    // above (shared with the Auto-Tone analysis frame).
    let active_copy = document.virtual_copies[copy_index].clone();
    // `resolved.planes` is owned by `MaskContext`, so clone once and reuse it for
    // both the warning render and the final shared encode render below.
    let mask_planes = resolved.planes.clone();
    let render_ctx = RenderContext {
        recipe: &recipe,
        camera_white_balance: wb,
        source_actions: auto_domain.source_actions(),
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
        auto_domain.source_actions(),
        auto_domain.zdata_path(),
        #[cfg(feature = "lensfun")]
        lensfun_corrector.as_ref().map(LensfunCorrectorRef),
        #[cfg(not(feature = "lensfun"))]
        None,
    )?;
    let render_execution =
        render_standard_with_generative_report(&frame, &recipe, &render_ctx, generative.input())?;
    let render_output = &render_execution.output;
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
                source_actions: auto_domain.source_actions(),
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
    Ok((metadata_written, render_execution))
}

#[cfg(test)]
mod tests;
