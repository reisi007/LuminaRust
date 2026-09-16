//! LRPAR-G13-MERGE-15 / MERGE-CLI-1: `merge-hdr` / `merge-pano` commands.
//!
//! SOLL: `feature/decisions/LRPAR-G13-MERGE-15.md` (§Pipeline-Einordnung,
//! §Persistenz, §Abnahme CLI) and `feature/platform/cli-gui-wasm.md`
//! (§ „HDR-/Panorama-Merge (G-13, Release 1.5: CLI)"). This module is
//! orchestration only: it decodes the sources (via `lumina-raw`/core,
//! like every other CLI command), calls into `lumina-merge` for alignment,
//! merge and DNG encoding, and persists the merge-DNG sidecar bundle. It
//! contains no second image-processing implementation and changes neither
//! the sidecar schema nor the merge math.
//!
//! Failure policy (Agents.md, no silent fallback): every deviation is loud
//! on stderr with exit code 1 — `merge missing:` (a source, the DNG or the
//! sidecar is gone), `merge stale:` (an existing bundle no longer matches
//! the sources, only `--force` re-merges), `merge unsupported:` (decode,
//! geometry, exposure, dimension or writer limit). Success (including
//! „bundle already current", which rewrites nothing) exits 0.
//!
//! The merge sidecar is a full [`SidecarDocument`] (standard copy with its
//! own recipe) plus document-level extras — no schema change:
//! `"type": "merge"`, `"merge_recipe"` ([`MergeRecipe`] JSON) and
//! `"merge_artifact"` ([`ArtifactReference`] JSON for the DNG). Source
//! sidecars are never touched.

use clap::Args;
use log::info;
use lumina_merge::{
    blend_panorama_transformed, encode_linear_dng, estimate_hdr_translation,
    estimate_pano_transform, merge_dng_filename, merge_hdr_weighted, validate_dng_file_name,
    DngError, DngExif, LinearImage, MergeError,
};
use lumina_sidecar::{
    load_sidecar, now_rfc3339_utc, save_sidecar, sidecar_path_for, ArtifactReference,
    DecodeFingerprint, GeometryFingerprint, MergeAlignment, MergeAlignmentMethod,
    MergeDecodeContext, MergeExposure, MergeMode, MergeOutput, MergeProjection, MergeRecipe,
    MergeSource, MergeStatus, MergeTransform, SourceIdentity, MAX_MERGE_EXPOSURE_TIME_S,
    MAX_MERGE_F_NUMBER, MAX_MERGE_ISO, MAX_MERGE_SOURCES, MERGE_OUTPUT_BITS, MERGE_RECIPE_VERSION,
    MIN_MERGE_SOURCES,
};
use std::fs;
use std::path::{Path, PathBuf};

use super::{decode_input, emit, io_error, CliError, StagedArtifact};

/// Shared arguments for `merge-hdr` and `merge-pano` (SOLL:
/// `feature/platform/cli-gui-wasm.md`).
///
/// Every flag is honored on both commands — nothing is silently ignored.
/// The only asymmetry is exposure resolution: HDR merges from EXIF exposure
/// and fails loudly without it, while panorama needs no exposure for its
/// pixels and stores EXIF (or a documented neutral default) as provenance
/// only.
#[derive(Debug, Clone, Args)]
pub struct MergeArgs {
    /// Source images, in merge order (first = reference). Repeatable,
    /// at least 2, at most 256.
    #[arg(long, required = true)]
    pub input: Vec<PathBuf>,
    /// Merge-DNG target path. Default: `<reference-stem>-HDR.dng` /
    /// `<reference-stem>-Pano.dng` next to the reference source.
    #[arg(long)]
    pub output: Option<PathBuf>,
    /// Per-source exposure times in seconds, comma-separated, in `--input`
    /// order (HDR: explicit value wins over RAW EXIF; panorama: stored as
    /// provenance only). Empty = EXIF-or-fallback per source.
    #[arg(long, value_delimiter = ',')]
    pub exposure_times: Vec<f64>,
    /// Per-source ISO values, comma-separated, in `--input` order.
    #[arg(long, value_delimiter = ',')]
    pub isos: Vec<u32>,
    /// Per-source f-numbers, comma-separated, in `--input` order.
    #[arg(long, value_delimiter = ',')]
    pub f_numbers: Vec<f64>,
    /// Alignment search radius in pixels (both modes).
    #[arg(long, default_value_t = 16)]
    pub max_shift_px: i32,
    /// Feather-blend width in the panorama overlap (HDR stores the value
    /// as recipe metadata; its pixels use the weighted merge).
    #[arg(long, default_value_t = 64)]
    pub blend_width_px: u32,
    /// Re-merge even when an existing bundle is stale or dangling.
    /// Without it, staleness is a loud error, never a silent overwrite.
    #[arg(long)]
    pub force: bool,
    /// Machine-readable JSON report on stdout (logs stay on stderr).
    #[arg(long)]
    pub json: bool,
}

/// DNG artifact reference payload stored as `"merge_artifact"` in the merge
/// sidecar extras (mirrors the mask-artefact metadata contract: relative
/// path, format, checksum, resolution, channel type, data version).
const MERGE_DNG_FORMAT: &str = "dng";
const MERGE_DNG_CHANNELS: &str = "rgb16";
const MERGE_DNG_DATA_VERSION: &str = "1";
/// Document-level envelope discriminator of a merge-DNG sidecar
/// (MERGE-DNG-1): `"type": "merge"`. It lives on the sidecar document, not
/// in `MergeRecipe` (which rejects an unknown `type` key loudly).
const MERGE_ENVELOPE_TYPE: &str = "merge";
/// Pipeline version of the merge-DNG sidecar: the merged DNG re-enters the
/// normal single-image pipeline, so it carries the same version as `import`.
const MERGE_PIPELINE_VERSION: &str = "raster-mvp-1";
/// Neutral exposure provenance for panorama sources without EXIF exposure
/// (panorama pixels never use exposure; HDR rejects such sources loudly
/// instead — see [`resolve_hdr_exposure`]).
const PANO_DEFAULT_EXPOSURE_S: f64 = 0.01;
const PANO_DEFAULT_ISO: u32 = 100;
const PANO_DEFAULT_F_NUMBER: f64 = 8.0;

/// Document kind of an existing sidecar at the merge-output sidecar path
/// (MERGE-DNG-1 envelope discriminator). The merge commands must never
/// silently overwrite a different document, so `Standard` and `Unknown`
/// are rejected loudly (only `--force` replaces them explicitly).
#[derive(Debug, Clone, PartialEq, Eq)]
enum MergeDocumentKind {
    /// No `"type"` key: an ordinary single-image sidecar.
    Standard,
    /// `"type": "merge"`: the merge-DNG envelope.
    Merge,
    /// Any other `"type"` value: rejected loudly.
    Unknown(String),
}

/// Classifies the ENVELOPE of an existing sidecar via its document-level
/// `"type"` extra. Missing → [`MergeDocumentKind::Standard`],
/// `"merge"` → [`MergeDocumentKind::Merge`], anything else →
/// [`MergeDocumentKind::Unknown`] (MERGE-DNG-1).
fn merge_document_kind(document: &lumina_sidecar::SidecarDocument) -> MergeDocumentKind {
    match document.extras.get("type") {
        None => MergeDocumentKind::Standard,
        Some(value) if value == &serde_json::Value::from(MERGE_ENVELOPE_TYPE) => {
            MergeDocumentKind::Merge
        }
        Some(value) => MergeDocumentKind::Unknown(
            value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_string),
        ),
    }
}

/// One decoded merge source: file-bytes identity plus the linear frame and
/// the provenance the merge recipe records.
struct DecodedSource {
    content_hash: String,
    frame: LinearImage,
    width: u32,
    height: u32,
    decode_context: MergeDecodeContext,
    raw_metadata: Option<lumina_raw::RawMetadata>,
}

/// `merge-hdr`: exposure-bracketed frames → one linear DNG.
pub fn merge_hdr(args: MergeArgs) -> Result<(), CliError> {
    run_merge(MergeMode::Hdr, &args)
}

/// `merge-pano`: overlapping frames → one linear DNG.
pub fn merge_pano(args: MergeArgs) -> Result<(), CliError> {
    run_merge(MergeMode::Panorama, &args)
}

fn run_merge(mode: MergeMode, args: &MergeArgs) -> Result<(), CliError> {
    let command = match mode {
        MergeMode::Hdr => "merge-hdr",
        MergeMode::Panorama => "merge-pano",
    };
    if args.input.len() < MIN_MERGE_SOURCES {
        return Err(CliError::Message(format!(
            "merge needs at least {MIN_MERGE_SOURCES} --input sources, got {}",
            args.input.len()
        )));
    }
    if args.input.len() > MAX_MERGE_SOURCES {
        return Err(CliError::Message(format!(
            "merge exceeds maximum of {MAX_MERGE_SOURCES} --input sources, got {}",
            args.input.len()
        )));
    }
    if args.max_shift_px < 0 {
        return Err(CliError::Message(format!(
            "invalid --max-shift-px {}: must be >= 0",
            args.max_shift_px
        )));
    }
    check_explicit_list_lengths(args)?;
    info!("{command}: merging {} source(s)", args.input.len());

    let sources = decode_sources(&args.input)?;
    for (index, source) in sources.iter().enumerate() {
        info!(
            "{command}: decoded source #{index} `{}` ({}x{}, {}, {})",
            args.input[index].display(),
            source.width,
            source.height,
            source.decode_context.decoder,
            source.content_hash,
        );
    }

    let exposures = resolve_exposures(mode, args, &sources)?;
    let frames: Vec<LinearImage> = sources.iter().map(|s| s.frame.clone()).collect();
    let (merged, transforms, residual_px, residual_flag) =
        align_and_merge(mode, args, &frames, &exposures)?;
    if residual_flag {
        eprintln!(
            "warning: {command} aligned with residual {residual_px:.2}px \
             (above the documented shift threshold); result kept, nothing cropped silently"
        );
        info!("{command}: aligned with residual {residual_px:.2}px");
    } else {
        info!("{command}: aligned (residual {residual_px:.2}px)");
    }
    info!(
        "{command}: merged frame {}x{}",
        merged.width(),
        merged.height()
    );

    let dng_path = resolve_dng_path(mode, args)?;
    let sidecar_path = sidecar_path_for(&dng_path);
    let bundle_dir = dng_path.parent().unwrap_or_else(|| Path::new("."));
    let source_paths = bundle_relative_paths(bundle_dir, &args.input)?;
    let dng_file_name = dng_file_name(&dng_path)?;
    let recipe = build_recipe(
        mode,
        args,
        &sources,
        &source_paths,
        &exposures,
        &transforms,
        residual_px,
        &dng_file_name,
    )?;
    let digest = recipe.digest();

    if let Some(outcome) = check_existing_bundle(command, args, &sidecar_path, &dng_path, &digest)?
    {
        return outcome;
    }

    let exif = dng_exif_for(sources[0].raw_metadata.as_ref());
    let dng_bytes = encode_linear_dng(&merged, mode, &exif).map_err(map_dng_error)?;
    validate_dng_file_name(&dng_file_name).map_err(map_dng_error)?;
    info!(
        "{command}: encoded linear DNG ({} bytes, {}x{})",
        dng_bytes.len(),
        merged.width(),
        merged.height()
    );

    // Stage the DNG, commit the sidecar, then publish the DNG — like
    // `process_selected`, a failed sidecar save leaves nothing behind.
    let staged = StagedArtifact::stage(&dng_path, &dng_bytes)?;
    let document = build_merge_document(&dng_file_name, &dng_bytes, &merged, &recipe)?;
    save_sidecar(&sidecar_path, &document)?;
    staged.commit()?;
    let checksum = blake3_checksum(&dng_bytes);
    info!("{command}: wrote `{}` ({checksum})", dng_path.display());
    info!("{command}: wrote `{}`", sidecar_path.display());
    emit(
        args.json,
        serde_json::json!({
            "command": command,
            "output": dng_path,
            "sidecar": sidecar_path,
            "digest": digest,
            "checksum": checksum,
            "cached": false,
            "status": "ok",
        }),
        &format!("merged {}", dng_path.display()),
    )
}

/// Usage-level validation of the explicit exposure lists: each list is
/// either empty (per-source fallback) or carries exactly one value per
/// `--input`, in order. Anything else is a loud error, never a silent
/// truncation or repetition.
fn check_explicit_list_lengths(args: &MergeArgs) -> Result<(), CliError> {
    let count = args.input.len();
    for (flag, len) in [
        ("--exposure-times", args.exposure_times.len()),
        ("--isos", args.isos.len()),
        ("--f-numbers", args.f_numbers.len()),
    ] {
        if len != 0 && len != count {
            return Err(CliError::Message(format!(
                "invalid {flag}: expected 0 or {count} values (one per --input), got {len}"
            )));
        }
    }
    Ok(())
}

/// Reads and decodes every source. A missing/unreadable file is `missing`,
/// an undecodable file or a non-RGBA frame is `unsupported` — never a
/// silent skip of one source.
fn decode_sources(inputs: &[PathBuf]) -> Result<Vec<DecodedSource>, CliError> {
    let mut sources = Vec::with_capacity(inputs.len());
    for input in inputs {
        let bytes = fs::read(input).map_err(|error| {
            CliError::Message(format!(
                "merge missing: cannot read source `{}`: {error}",
                input.display()
            ))
        })?;
        let content_hash = blake3_checksum(&bytes);
        let (frame, raw) = decode_input(input, &bytes).map_err(|error| {
            CliError::Message(format!(
                "merge unsupported: cannot decode source `{}`: {error}",
                input.display()
            ))
        })?;
        let decode_context = MergeDecodeContext {
            decoder: if raw.is_some() { "libraw" } else { "image" }.into(),
            decode_version: if raw.is_some() {
                lumina_raw::libraw_decode_version()
            } else {
                env!("CARGO_PKG_VERSION").into()
            },
            orientation: raw.as_ref().map_or(1, |meta| meta.orientation),
        };
        let linear = linear_from_rgba(&frame).ok_or_else(|| {
            CliError::Message(format!(
                "merge unsupported: source `{}` has {} bytes for a {}x{} RGBA8 frame",
                input.display(),
                frame.pixels.len(),
                frame.width,
                frame.height
            ))
        })?;
        sources.push(DecodedSource {
            content_hash,
            frame: linear,
            width: frame.width,
            height: frame.height,
            decode_context,
            raw_metadata: raw,
        });
    }
    Ok(sources)
}

/// Row-major RGBA8 (`0..=255`, alpha ignored) → linear `f32` RGB frame.
fn linear_from_rgba(frame: &lumina_core::ImageFrame) -> Option<LinearImage> {
    let expected = frame.width as usize * frame.height as usize * 4;
    if frame.pixels.len() != expected {
        return None;
    }
    let mut pixels = Vec::with_capacity(frame.width as usize * frame.height as usize * 3);
    for rgba in frame.pixels.as_chunks::<4>().0 {
        pixels.push(rgba[0] as f32 / 255.0);
        pixels.push(rgba[1] as f32 / 255.0);
        pixels.push(rgba[2] as f32 / 255.0);
    }
    LinearImage::new(frame.width, frame.height, pixels).ok()
}

/// Per-source exposure for the recipe and (HDR) the pixel merge.
///
/// HDR: explicit value wins; otherwise the RAW EXIF exposure; otherwise
/// `unsupported` — exposure is never guessed from pixels. Panorama: the
/// pixels ignore exposure, so EXIF (or the documented neutral default)
/// is stored as provenance only.
fn resolve_exposures(
    mode: MergeMode,
    args: &MergeArgs,
    sources: &[DecodedSource],
) -> Result<Vec<MergeExposure>, CliError> {
    sources
        .iter()
        .enumerate()
        .map(|(index, source)| match mode {
            MergeMode::Hdr => resolve_hdr_exposure(index, args, source),
            MergeMode::Panorama => Ok(resolve_pano_exposure(index, args, source)),
        })
        .collect()
}

fn explicit_at<T: Copy>(values: &[T], index: usize) -> Option<T> {
    if values.is_empty() {
        None
    } else {
        Some(values[index])
    }
}

fn resolve_hdr_exposure(
    index: usize,
    args: &MergeArgs,
    source: &DecodedSource,
) -> Result<MergeExposure, CliError> {
    let path = args.input[index].display().to_string();
    let exposure_time_s = match explicit_at(&args.exposure_times, index) {
        Some(value) => {
            if !value.is_finite() || value <= 0.0 || value > MAX_MERGE_EXPOSURE_TIME_S {
                return Err(CliError::Message(format!(
                    "merge unsupported: invalid --exposure-times[#{index}] {value} \
                     (finite within (0, {MAX_MERGE_EXPOSURE_TIME_S}])"
                )));
            }
            value
        }
        None => source
            .raw_metadata
            .as_ref()
            .and_then(|meta| meta.shutter)
            .filter(|v| v.is_finite() && *v > 0.0 && *v as f64 <= MAX_MERGE_EXPOSURE_TIME_S)
            .map(|v| v as f64)
            .ok_or_else(|| {
                CliError::Message(format!(
                    "merge unsupported: source `{path}` has no EXIF exposure \
                     (pass --exposure-times/--isos/--f-numbers, one value per --input)"
                ))
            })?,
    };
    let iso = match explicit_at(&args.isos, index) {
        Some(value) => {
            if value == 0 || value > MAX_MERGE_ISO {
                return Err(CliError::Message(format!(
                    "merge unsupported: invalid --isos[#{index}] {value} \
                     (within 1..={MAX_MERGE_ISO})"
                )));
            }
            value
        }
        None => source
            .raw_metadata
            .as_ref()
            .and_then(|meta| meta.iso)
            .filter(|v| v.is_finite() && *v >= 1.0 && *v <= MAX_MERGE_ISO as f32)
            .map(|v| v.round() as u32)
            .filter(|v| (1..=MAX_MERGE_ISO).contains(v))
            .ok_or_else(|| {
                CliError::Message(format!(
                    "merge unsupported: source `{path}` has no EXIF ISO \
                     (pass --exposure-times/--isos/--f-numbers, one value per --input)"
                ))
            })?,
    };
    let f_number = match explicit_at(&args.f_numbers, index) {
        Some(value) => {
            if !value.is_finite() || value <= 0.0 || value > MAX_MERGE_F_NUMBER {
                return Err(CliError::Message(format!(
                    "merge unsupported: invalid --f-numbers[#{index}] {value} \
                     (finite within (0, {MAX_MERGE_F_NUMBER}])"
                )));
            }
            value
        }
        None => source
            .raw_metadata
            .as_ref()
            .and_then(|meta| meta.aperture)
            .filter(|v| v.is_finite() && *v > 0.0 && *v as f64 <= MAX_MERGE_F_NUMBER)
            .map(|v| v as f64)
            .ok_or_else(|| {
                CliError::Message(format!(
                    "merge unsupported: source `{path}` has no EXIF f-number \
                     (pass --exposure-times/--isos/--f-numbers, one value per --input)"
                ))
            })?,
    };
    Ok(MergeExposure {
        exposure_time_s,
        iso,
        f_number,
    })
}

fn resolve_pano_exposure(index: usize, args: &MergeArgs, source: &DecodedSource) -> MergeExposure {
    let from_exif = || {
        let meta = source.raw_metadata.as_ref()?;
        let exposure_time_s = meta
            .shutter
            .filter(|v| v.is_finite() && *v > 0.0 && *v as f64 <= MAX_MERGE_EXPOSURE_TIME_S)
            .map(|v| v as f64)?;
        let iso = meta
            .iso
            .filter(|v| v.is_finite() && *v >= 1.0 && *v <= MAX_MERGE_ISO as f32)
            .map(|v| v.round() as u32)
            .filter(|v| (1..=MAX_MERGE_ISO).contains(v))?;
        let f_number = meta
            .aperture
            .filter(|v| v.is_finite() && *v > 0.0 && *v as f64 <= MAX_MERGE_F_NUMBER)
            .map(|v| v as f64)?;
        Some(MergeExposure {
            exposure_time_s,
            iso,
            f_number,
        })
    };
    if let (Some(t), Some(i), Some(f)) = (
        explicit_at(&args.exposure_times, index),
        explicit_at(&args.isos, index),
        explicit_at(&args.f_numbers, index),
    ) {
        if t.is_finite()
            && t > 0.0
            && t <= MAX_MERGE_EXPOSURE_TIME_S
            && (1..=MAX_MERGE_ISO).contains(&i)
            && f.is_finite()
            && f > 0.0
            && f <= MAX_MERGE_F_NUMBER
        {
            return MergeExposure {
                exposure_time_s: t,
                iso: i,
                f_number: f,
            };
        }
    }
    if let Some(exif) = from_exif() {
        return exif;
    }
    info!(
        "merge-pano: source `{}` has no EXIF exposure; storing neutral \
         provenance (panorama pixels never use exposure)",
        args.input[index].display()
    );
    MergeExposure {
        exposure_time_s: PANO_DEFAULT_EXPOSURE_S,
        iso: PANO_DEFAULT_ISO,
        f_number: PANO_DEFAULT_F_NUMBER,
    }
}

/// Alignment (via `lumina-merge`) plus the pixel merge.
///
/// Returns the merged frame, the recipe transforms (reference frame
/// conventionally carries no entry), the maximum residual in pixels and
/// whether any frame exceeded the documented shift threshold.
#[allow(clippy::type_complexity)]
fn align_and_merge(
    mode: MergeMode,
    args: &MergeArgs,
    frames: &[LinearImage],
    exposures: &[MergeExposure],
) -> Result<(LinearImage, Vec<MergeTransform>, f64, bool), CliError> {
    match mode {
        MergeMode::Hdr => {
            let mut shifts = Vec::with_capacity(frames.len());
            let mut transforms = Vec::new();
            let mut residual_px: f64 = 0.0;
            let mut residual_flag = false;
            shifts.push((0.0, 0.0));
            for (index, frame) in frames.iter().enumerate().skip(1) {
                let shift = estimate_hdr_translation(&frames[0], frame, args.max_shift_px)
                    .map_err(map_merge_error)?;
                shifts.push((shift.dx, shift.dy));
                transforms.push(MergeTransform {
                    source_index: index,
                    matrix_3x3: [1.0, 0.0, shift.dx, 0.0, 1.0, shift.dy, 0.0, 0.0, 1.0],
                });
                residual_px = residual_px.max(shift.residual_px);
                residual_flag |= shift.status.residual_flag();
            }
            let merged = merge_hdr_weighted(frames, exposures, &shifts).map_err(map_merge_error)?;
            Ok((merged, transforms, residual_px, residual_flag))
        }
        MergeMode::Panorama => {
            // Full 3x3 matrix per frame (translation + rotation about the
            // frame centre, `lumina-merge::pano_matrix`). The blend applies
            // each frame through its **full inverse matrix**
            // ([`blend_panorama_transformed`]). The previous wiring fed only
            // `matrix[2]`/`matrix[5]` integer offsets into the
            // translation-only `blend_panorama`, which silently dropped the
            // estimated rotation (F1-Auflage, MERGE-CORE-1-Rework).
            let mut matrices = Vec::with_capacity(frames.len());
            let mut transforms = Vec::new();
            let mut residual_px: f64 = 0.0;
            let mut residual_flag = false;
            matrices.push([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
            for (index, frame) in frames.iter().enumerate().skip(1) {
                let transform = estimate_pano_transform(&frames[0], frame, args.max_shift_px)
                    .map_err(map_merge_error)?;
                matrices.push(transform.matrix_3x3);
                transforms.push(MergeTransform {
                    source_index: index,
                    matrix_3x3: transform.matrix_3x3,
                });
                info!(
                    "merge-pano: source #{index} aligned with rotation {:.3}deg \
                     (matrix {:?})",
                    transform.rotation_deg, transform.matrix_3x3
                );
                residual_px = residual_px.max(transform.residual_px);
                residual_flag |= transform.status.residual_flag();
            }
            let merged = blend_panorama_transformed(frames, &matrices, args.blend_width_px)
                .map_err(map_merge_error)?;
            Ok((merged, transforms, residual_px, residual_flag))
        }
    }
}

fn map_merge_error(error: MergeError) -> CliError {
    // Every merge-domain failure (scope limit or incompatible input set)
    // is `unsupported`: loud, with the cause, never a silent fallback.
    CliError::Message(format!("merge unsupported: {error}"))
}

fn map_dng_error(error: DngError) -> CliError {
    match error {
        DngError::Unsupported(message) => {
            CliError::Message(format!("merge unsupported: {message}"))
        }
        other => CliError::Message(format!("merge failed: {other}")),
    }
}

/// Target DNG path: explicit `--output` (must end in `.dng` and must not
/// overwrite a source) or the deterministic default next to the reference
/// source.
fn resolve_dng_path(mode: MergeMode, args: &MergeArgs) -> Result<PathBuf, CliError> {
    if let Some(output) = &args.output {
        let is_dng = output
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dng"));
        if !is_dng {
            return Err(CliError::Message(format!(
                "invalid --output `{}`: merge writes a linear DNG, the target must end in `.dng`",
                output.display()
            )));
        }
        if output
            .file_name()
            .is_none_or(|name| name.is_empty() || name == "." || name == "..")
        {
            return Err(CliError::Message(format!(
                "invalid --output `{}`: target must have a file name",
                output.display()
            )));
        }
        for input in &args.input {
            if lumina_sidecar::paths_resolve_equal(input, output)
                .map_err(|error| io_error(input, error))?
            {
                return Err(CliError::Message(format!(
                    "invalid --output `{}`: refusing to overwrite the merge source `{}`",
                    output.display(),
                    input.display()
                )));
            }
        }
        return Ok(output.clone());
    }
    let reference = &args.input[0];
    let reference_name = reference
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            CliError::Message(format!(
                "merge unsupported: reference source `{}` has no usable file name",
                reference.display()
            ))
        })?;
    let file_name = merge_dng_filename(reference_name, mode).map_err(map_dng_error)?;
    let parent = reference.parent().unwrap_or_else(|| Path::new("."));
    Ok(parent.join(file_name))
}

/// Bundle-relative source references (portable: no absolute paths, no
/// `..` segments — the recipe schema rejects both loudly). Sources must
/// live inside the bundle directory; anything else is `unsupported`.
fn bundle_relative_paths(bundle_dir: &Path, inputs: &[PathBuf]) -> Result<Vec<String>, CliError> {
    inputs
        .iter()
        .map(|input| {
            let stripped = input.strip_prefix(bundle_dir).map_err(|_| {
                CliError::Message(format!(
                    "merge unsupported: source `{}` is outside the merge bundle directory \
                     `{}` (relative references cannot leave the bundle; place the sources \
                     next to the merge DNG)",
                    input.display(),
                    bundle_dir.display()
                ))
            })?;
            if stripped.as_os_str().is_empty() {
                return Err(CliError::Message(format!(
                    "merge unsupported: source `{}` has no usable bundle-relative path",
                    input.display()
                )));
            }
            let mut parts = Vec::new();
            for component in stripped.components() {
                match component {
                    std::path::Component::Normal(part) => {
                        let text = part.to_str().ok_or_else(|| {
                            CliError::Message(format!(
                                "merge unsupported: source `{}` has a non-UTF-8 path component",
                                input.display()
                            ))
                        })?;
                        parts.push(text.to_string());
                    }
                    _ => {
                        return Err(CliError::Message(format!(
                            "merge unsupported: source `{}` has no portable bundle-relative \
                             path (absolute paths and `.`/`..` are forbidden)",
                            input.display()
                        )));
                    }
                }
            }
            Ok(parts.join("/"))
        })
        .collect()
}

fn dng_file_name(dng_path: &Path) -> Result<String, CliError> {
    let name = dng_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            CliError::Message(format!(
                "merge failed: target `{}` has no usable file name",
                dng_path.display()
            ))
        })?;
    validate_dng_file_name(name).map_err(map_dng_error)?;
    Ok(name.to_string())
}

/// Assembles and validates the merge recipe (validation is loud: unknown
/// modes/methods and out-of-range values are rejected, never clipped).
#[allow(clippy::too_many_arguments)]
fn build_recipe(
    mode: MergeMode,
    args: &MergeArgs,
    sources: &[DecodedSource],
    source_paths: &[String],
    exposures: &[MergeExposure],
    transforms: &[MergeTransform],
    residual_px: f64,
    dng_file_name: &str,
) -> Result<MergeRecipe, CliError> {
    let (method, projection) = match mode {
        MergeMode::Hdr => (MergeAlignmentMethod::HdrTranslate, MergeProjection::None),
        MergeMode::Panorama => (
            MergeAlignmentMethod::PanoCylindricalHomography,
            MergeProjection::Cylindrical,
        ),
    };
    let recipe_sources = sources
        .iter()
        .zip(source_paths.iter().zip(exposures.iter()))
        .map(|(source, (path, exposure))| MergeSource {
            path: path.clone(),
            content_hash: source.content_hash.clone(),
            decode_context: source.decode_context.clone(),
            exposure: exposure.clone(),
        })
        .collect();
    let recipe = MergeRecipe {
        merge_version: MERGE_RECIPE_VERSION,
        mode,
        sources: recipe_sources,
        alignment: MergeAlignment {
            method,
            transforms: transforms.to_vec(),
            residual_px,
            projection,
            blend_width_px: args.blend_width_px,
        },
        output: MergeOutput {
            file: dng_file_name.to_string(),
            bits: MERGE_OUTPUT_BITS,
            mosaic: false,
        },
        created_at: now_rfc3339_utc(),
        status: MergeStatus::Ok,
        error: None,
    };
    recipe.validate().map_err(CliError::from)?;
    Ok(recipe)
}

/// Stale/missing gate over an existing bundle (SOLL §Persistenz: a merge is
/// valid while source hashes, decode contexts and the DNG checksum match).
///
/// Returns `Ok(None)` when the merge may proceed, or `Ok(Some(outcome))`
/// with the early exit for the already-current bundle.
fn check_existing_bundle(
    command: &str,
    args: &MergeArgs,
    sidecar_path: &Path,
    dng_path: &Path,
    digest: &str,
) -> Result<Option<Result<(), CliError>>, CliError> {
    if !sidecar_path.exists() {
        if dng_path.exists() && !args.force {
            return Err(CliError::Message(format!(
                "merge missing: merge DNG `{}` exists without its sidecar `{}` \
                 (run with --force to regenerate the bundle explicitly)",
                dng_path.display(),
                sidecar_path.display()
            )));
        }
        return Ok(None);
    }
    if args.force {
        info!("{command}: --force regenerates the existing bundle");
        return Ok(None);
    }
    let document = load_sidecar(sidecar_path).map_err(|error| {
        CliError::Message(format!(
            "merge missing: existing merge sidecar `{}` is unreadable ({error}); \
             run with --force to regenerate the bundle explicitly",
            sidecar_path.display()
        ))
    })?;
    match merge_document_kind(&document) {
        MergeDocumentKind::Merge => {}
        MergeDocumentKind::Standard => {
            return Err(CliError::Message(format!(
                "merge stale: existing sidecar `{}` is a standard sidecar \
                 (no `\"type\": \"{}\"` envelope); refusing to overwrite it — \
                 run with --force to replace the bundle explicitly",
                sidecar_path.display(),
                MERGE_ENVELOPE_TYPE
            )));
        }
        MergeDocumentKind::Unknown(kind) => {
            return Err(CliError::Message(format!(
                "merge stale: existing sidecar `{}` has unknown document type \
                 `{kind}` (expected `\"type\": \"{}\"`); refusing to overwrite \
                 it — run with --force to replace the bundle explicitly",
                sidecar_path.display(),
                MERGE_ENVELOPE_TYPE
            )));
        }
    }
    let stored_recipe_value = document.extras.get("merge_recipe").ok_or_else(|| {
        CliError::Message(format!(
            "merge stale: existing sidecar `{}` carries no merge recipe; \
             run with --force to regenerate the bundle explicitly",
            sidecar_path.display()
        ))
    })?;
    let stored_recipe =
        MergeRecipe::from_json(&stored_recipe_value.to_string()).map_err(|error| {
            CliError::Message(format!(
                "merge stale: stored merge recipe in `{}` is invalid ({error}); \
             run with --force to regenerate the bundle explicitly",
                sidecar_path.display()
            ))
        })?;
    if stored_recipe.digest() != digest {
        return Err(CliError::Message(format!(
            "merge stale: sources, decode context or alignment changed since `{}` \
             was written (digest mismatch); run with --force to re-merge explicitly",
            sidecar_path.display()
        )));
    }
    let stored_artifact_value = document.extras.get("merge_artifact").ok_or_else(|| {
        CliError::Message(format!(
            "merge stale: existing sidecar `{}` carries no merge artifact reference; \
             run with --force to regenerate the bundle explicitly",
            sidecar_path.display()
        ))
    })?;
    let stored_artifact: ArtifactReference = serde_json::from_value(stored_artifact_value.clone())
        .map_err(|error| {
            CliError::Message(format!(
                "merge stale: stored merge artifact in `{}` is invalid ({error}); \
                 run with --force to regenerate the bundle explicitly",
                sidecar_path.display()
            ))
        })?;
    let dng_bytes = fs::read(dng_path).map_err(|_| {
        CliError::Message(format!(
            "merge missing: merge DNG `{}` is gone (its sidecar `{}` remains); \
             run with --force to regenerate the bundle explicitly",
            dng_path.display(),
            sidecar_path.display()
        ))
    })?;
    if blake3_checksum(&dng_bytes) != stored_artifact.checksum {
        return Err(CliError::Message(format!(
            "merge stale: merge DNG `{}` changed on disk (checksum mismatch); \
             run with --force to re-merge explicitly",
            dng_path.display()
        )));
    }
    info!("{command}: bundle already current (digest match, nothing rewritten)");
    Ok(Some(emit(
        args.json,
        serde_json::json!({
            "command": command,
            "output": dng_path,
            "sidecar": sidecar_path,
            "digest": digest,
            "checksum": stored_artifact.checksum,
            "cached": true,
            "status": "ok",
        }),
        &format!("merge already current: {}", dng_path.display()),
    )))
}

/// EXIF identity carried from the reference (first) source into the DNG.
/// Every field is optional: a missing or unusable reference field omits
/// the tag (never guessed, never defaulted).
fn dng_exif_for(metadata: Option<&lumina_raw::RawMetadata>) -> DngExif {
    let Some(meta) = metadata else {
        return DngExif::default();
    };
    let ascii = |value: &Option<String>| {
        value
            .as_ref()
            .filter(|v| !v.trim().is_empty() && !v.contains('\0'))
            .cloned()
    };
    DngExif {
        make: ascii(&meta.camera_make),
        model: ascii(&meta.camera_model),
        lens: ascii(&meta.lens),
        exposure_time_s: meta
            .shutter
            .filter(|v| v.is_finite() && *v > 0.0 && *v as f64 <= MAX_MERGE_EXPOSURE_TIME_S)
            .map(|v| v as f64),
        f_number: meta
            .aperture
            .filter(|v| v.is_finite() && *v > 0.0 && *v as f64 <= MAX_MERGE_F_NUMBER)
            .map(|v| v as f64),
        iso_speed: meta
            .iso
            .filter(|v| v.is_finite() && *v >= 1.0 && *v <= 65_535.0)
            .map(|v| v.round() as u32)
            .filter(|v| *v >= 1),
        timestamp: meta.timestamp,
    }
}

/// Merge-DNG sidecar document: source identity of the new DNG plus the
/// `"type": "merge"` envelope, the recipe and the DNG artifact reference
/// as document-level extras (additive — no schema change).
fn build_merge_document(
    dng_file_name: &str,
    dng_bytes: &[u8],
    merged: &LinearImage,
    recipe: &MergeRecipe,
) -> Result<lumina_sidecar::SidecarDocument, CliError> {
    let (width, height) = (merged.width(), merged.height());
    let source = SourceIdentity {
        relative_name: dng_file_name.to_string(),
        content_hash: blake3_checksum(dng_bytes),
        byte_length: dng_bytes.len() as u64,
        modified_at: None,
        raw_format: "DNG".into(),
        orientation: 1,
        decode_fingerprint: DecodeFingerprint {
            decoder: "libraw".into(),
            version: lumina_raw::libraw_decode_version(),
            parameters: std::collections::BTreeMap::from([(
                "geometry".into(),
                format!("{width}x{height}"),
            )]),
            extras: std::collections::BTreeMap::from([(
                "orientation_applied".into(),
                "true".into(),
            )]),
        },
        geometry_fingerprint: GeometryFingerprint {
            width,
            height,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: std::collections::BTreeMap::new(),
        },
        extras: std::collections::BTreeMap::new(),
    };
    let mut document = lumina_sidecar::SidecarDocument::new(source, MERGE_PIPELINE_VERSION);
    let recipe_value =
        serde_json::to_value(recipe).map_err(|error| CliError::Message(error.to_string()))?;
    let artifact = ArtifactReference {
        relative_path: dng_file_name.to_string(),
        format: MERGE_DNG_FORMAT.into(),
        checksum: blake3_checksum(dng_bytes),
        width,
        height,
        channels: MERGE_DNG_CHANNELS.into(),
        data_version: MERGE_DNG_DATA_VERSION.into(),
        extras: std::collections::BTreeMap::new(),
    };
    let artifact_value =
        serde_json::to_value(&artifact).map_err(|error| CliError::Message(error.to_string()))?;
    document
        .extras
        .insert("type".into(), serde_json::Value::from(MERGE_ENVELOPE_TYPE));
    document.extras.insert("merge_recipe".into(), recipe_value);
    document
        .extras
        .insert("merge_artifact".into(), artifact_value);
    Ok(document)
}

fn blake3_checksum(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merge_args(inputs: &[&str]) -> MergeArgs {
        MergeArgs {
            input: inputs.iter().map(PathBuf::from).collect(),
            output: None,
            exposure_times: vec![],
            isos: vec![],
            f_numbers: vec![],
            max_shift_px: 16,
            blend_width_px: 64,
            force: false,
            json: false,
        }
    }

    #[test]
    fn explicit_lists_accept_empty_or_exactly_per_input() {
        let mut args = merge_args(&["a.png", "b.png"]);
        check_explicit_list_lengths(&args).unwrap();
        args.exposure_times = vec![0.01, 0.02];
        args.isos = vec![100, 200];
        args.f_numbers = vec![8.0, 5.6];
        check_explicit_list_lengths(&args).unwrap();
        args.isos = vec![100];
        assert!(check_explicit_list_lengths(&args).is_err());
    }

    #[test]
    fn bundle_relative_paths_cover_same_dir_and_subdir() {
        let bundle = Path::new("/tmp/bundle");
        let inputs = vec![
            PathBuf::from("/tmp/bundle/a.png"),
            PathBuf::from("/tmp/bundle/sub/b.png"),
        ];
        assert_eq!(
            bundle_relative_paths(bundle, &inputs).unwrap(),
            vec!["a.png", "sub/b.png"]
        );
    }

    #[test]
    fn bundle_relative_paths_reject_outside_and_absolute() {
        let bundle = Path::new("/tmp/bundle");
        assert!(bundle_relative_paths(bundle, &[PathBuf::from("/tmp/other/a.png")]).is_err());
        assert!(bundle_relative_paths(bundle, &[PathBuf::from("/tmp/bundle/../a.png")]).is_err());
        assert!(bundle_relative_paths(Path::new("."), &[PathBuf::from("/abs/a.png")]).is_err());
    }

    #[test]
    fn linear_from_rgba_scales_and_ignores_alpha() {
        let frame =
            lumina_core::ImageFrame::new(2, 1, vec![255, 0, 0, 128, 0, 128, 0, 255]).unwrap();
        let linear = linear_from_rgba(&frame).unwrap();
        assert_eq!((linear.width(), linear.height()), (2, 1));
        assert_eq!(linear.pixels()[0..3], [1.0, 0.0, 0.0]);
        let green = 128.0 / 255.0;
        assert!((linear.pixels()[4] - green).abs() < 1e-6);
    }

    #[test]
    fn linear_from_rgba_rejects_length_mismatch() {
        let mut frame = lumina_core::ImageFrame::new(1, 1, vec![1, 2, 3, 4]).unwrap();
        frame.pixels.pop();
        assert!(linear_from_rgba(&frame).is_none());
    }

    #[test]
    fn dng_file_name_gate_uses_the_file_name_component() {
        let dir = Path::new("/tmp/x");
        assert_eq!(
            dng_file_name(&dir.join("IMG_0001-HDR.dng")).unwrap(),
            "IMG_0001-HDR.dng"
        );
        // Only the file name is gated here (bundle membership of the
        // sources is enforced separately by `bundle_relative_paths`).
        assert_eq!(dng_file_name(&dir.join("sub/x.dng")).unwrap(), "x.dng");
        assert!(dng_file_name(&dir.join("..")).is_err());
    }

    #[test]
    fn merge_envelope_classifies_standard_merge_and_unknown() {
        // MERGE-DNG-1: missing `type` → Standard, `"merge"` → Merge,
        // anything else → Unknown (each rejected loudly by the command).
        assert_eq!(
            merge_document_kind(&envelope_document(None)),
            MergeDocumentKind::Standard
        );
        assert_eq!(
            merge_document_kind(&envelope_document(Some(MERGE_ENVELOPE_TYPE))),
            MergeDocumentKind::Merge
        );
        assert_eq!(
            merge_document_kind(&envelope_document(Some("pano"))),
            MergeDocumentKind::Unknown("pano".into())
        );
        assert_eq!(
            merge_document_kind(&envelope_document(Some("merge-hdr"))),
            MergeDocumentKind::Unknown("merge-hdr".into())
        );
    }

    /// Sidecar document with (or without) a document-level `"type"` extra.
    fn envelope_document(type_value: Option<&str>) -> lumina_sidecar::SidecarDocument {
        let source = SourceIdentity {
            relative_name: "a-HDR.dng".into(),
            content_hash: format!("blake3:{}", "00".repeat(32)),
            byte_length: 4,
            modified_at: None,
            raw_format: "DNG".into(),
            orientation: 1,
            decode_fingerprint: DecodeFingerprint {
                decoder: "libraw".into(),
                version: "test".into(),
                parameters: std::collections::BTreeMap::new(),
                extras: std::collections::BTreeMap::new(),
            },
            geometry_fingerprint: GeometryFingerprint {
                width: 4,
                height: 4,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: std::collections::BTreeMap::new(),
            },
            extras: std::collections::BTreeMap::new(),
        };
        let mut document = lumina_sidecar::SidecarDocument::new(source, MERGE_PIPELINE_VERSION);
        if let Some(value) = type_value {
            document
                .extras
                .insert("type".into(), serde_json::Value::from(value));
        }
        document
    }

    #[test]
    fn merge_envelope_roundtrips_through_sidecar_extras() {
        // The `"type": "merge"` envelope, recipe and artifact travel as
        // document-level extras (no schema change) and survive a
        // save/load roundtrip byte-stably.
        let recipe = MergeRecipe {
            merge_version: MERGE_RECIPE_VERSION,
            mode: MergeMode::Hdr,
            sources: vec![
                MergeSource {
                    path: "a.png".into(),
                    content_hash: format!("blake3:{}", "ab".repeat(32)),
                    decode_context: MergeDecodeContext {
                        decoder: "image".into(),
                        decode_version: "0.1.0".into(),
                        orientation: 1,
                    },
                    exposure: MergeExposure {
                        exposure_time_s: 0.01,
                        iso: 100,
                        f_number: 8.0,
                    },
                },
                MergeSource {
                    path: "b.png".into(),
                    content_hash: format!("blake3:{}", "cd".repeat(32)),
                    decode_context: MergeDecodeContext {
                        decoder: "image".into(),
                        decode_version: "0.1.0".into(),
                        orientation: 1,
                    },
                    exposure: MergeExposure {
                        exposure_time_s: 0.02,
                        iso: 100,
                        f_number: 8.0,
                    },
                },
            ],
            alignment: MergeAlignment {
                method: MergeAlignmentMethod::HdrTranslate,
                transforms: vec![],
                residual_px: 0.0,
                projection: MergeProjection::None,
                blend_width_px: 64,
            },
            output: MergeOutput {
                file: "a-HDR.dng".into(),
                bits: MERGE_OUTPUT_BITS,
                mosaic: false,
            },
            created_at: "2026-09-05T00:00:00Z".into(),
            status: MergeStatus::Ok,
            error: None,
        };
        recipe.validate().unwrap();
        let merged = LinearImage::solid(32, 24, [0.4, 0.4, 0.4]);
        let document = build_merge_document("a-HDR.dng", b"dng-bytes", &merged, &recipe).unwrap();
        let json = document.to_json().unwrap();
        assert!(json.contains("\"type\": \"merge\""));
        assert!(!json.contains("C:") && !json.contains("/tmp"));
        let parsed = lumina_sidecar::SidecarDocument::from_json(&json).unwrap();
        assert_eq!(
            parsed.extras.get("type"),
            Some(&serde_json::Value::from("merge"))
        );
        let stored = MergeRecipe::from_json(&parsed.extras["merge_recipe"].to_string()).unwrap();
        assert_eq!(stored, recipe);
        assert_eq!(stored.digest(), recipe.digest());
    }
}
