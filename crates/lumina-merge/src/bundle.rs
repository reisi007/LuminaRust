//! LRPAR-G13-MERGE-15 / MERGE-IMPL-15 (F6): the **one** shared merge
//! orchestration both frontends use.
//!
//! SOLL: `feature/decisions/LRPAR-G13-MERGE-15.md` (§Pipeline-Einordnung
//! „Kein stiller Fallback zwischen CLI und GUI", §Persistenz, F6-Notiz) and
//! `feature/platform/cli-gui-wasm.md` (§ „HDR-/Panorama-Merge").
//!
//! Before this module the sequence *align → blend → recipe → digest →
//! envelope/bundle gate → DNG encode → atomic persist* was mirrored in
//! `lumina-cli/src/merge.rs` and `lumina-gui/src/merge_gui.rs` (identical
//! order, drifting details: CLI `--output` + `StagedArtifact` vs. GUI
//! `{pid}.tmp`). It now lives here exactly once; the CLI command module and the
//! GUI action module call [`run_merge`] and only supply the pieces that are
//! genuinely frontend-specific:
//!
//! - the **decode adapter** (CLI `decode_input` / GUI `decode_selection_frame`)
//!   as a closure returning [`MergeSourceFrame`], and
//! - the **exposure policy** (CLI `--exposure-times/--isos/--f-numbers`
//!   per-field override vs. GUI `Option<&[MergeExposure]>`) as a resolver
//!   closure.
//!
//! Everything else — validation, alignment/merge dispatch, recipe + digest,
//! the loud envelope/stale/missing gate, the linear DNG writer, the sidecar
//! document and the atomic DNG+sidecar publication — is this module. No second
//! image-processing implementation, no schema change: the merge sidecar stays a
//! full [`SidecarDocument`] plus document-level extras (`"type": "merge"`,
//! `merge_recipe`, `merge_artifact`).
//!
//! Failure policy (Agents.md, no silent fallback): every deviation is loud via
//! [`MergeRunError`] with the stable prefixes `merge missing:` (a source, the
//! DNG or the sidecar is gone), `merge stale:` (an existing bundle no longer
//! matches the sources; only `force` re-merges), `merge unsupported:` (decode,
//! geometry, exposure, dimension or writer limit) and `merge failed:` (an
//! unexpected I/O/serialize failure). Frontends map the structured error to
//! their surface (CLI exit code 1 + stderr, GUI status/error dialog); the
//! prefixes and the non-destructive envelope gate are identical on both paths.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use log::info;
use lumina_sidecar::{
    load_sidecar, now_rfc3339_utc, save_sidecar, sidecar_path_for, ArtifactReference,
    DecodeFingerprint, GeometryFingerprint, MergeAlignment, MergeAlignmentMethod,
    MergeDecodeContext, MergeExposure, MergeMode, MergeOutput, MergeProjection, MergeRecipe,
    MergeSource, MergeStatus, MergeTransform, SidecarDocument, SourceIdentity,
    MAX_MERGE_EXPOSURE_TIME_S, MAX_MERGE_F_NUMBER, MAX_MERGE_ISO, MAX_MERGE_SOURCES,
    MERGE_OUTPUT_BITS, MERGE_RECIPE_VERSION, MIN_MERGE_SOURCES,
};

use crate::{
    blend_panorama_transformed, encode_linear_dng, estimate_hdr_translation,
    estimate_pano_transform, merge_dng_filename, merge_hdr_weighted, validate_dng_file_name,
    DngError, DngExif, LinearImage, MergeError,
};

/// Document-level envelope discriminator of a merge-DNG sidecar
/// (MERGE-DNG-1): `"type": "merge"`. It lives on the sidecar document, not in
/// `MergeRecipe` (which rejects an unknown `type` key loudly).
pub const MERGE_ENVELOPE_TYPE: &str = "merge";
/// Document-level extra key of the persisted merge recipe.
pub const MERGE_RECIPE_KEY: &str = "merge_recipe";
/// Document-level extra key of the persisted merge DNG artifact reference.
pub const MERGE_ARTIFACT_KEY: &str = "merge_artifact";
/// DNG artifact reference format (mirrors the mask-artefact contract).
pub const MERGE_DNG_FORMAT: &str = "dng";
/// DNG artifact reference channel type.
pub const MERGE_DNG_CHANNELS: &str = "rgb16";
/// DNG artifact reference data version.
pub const MERGE_DNG_DATA_VERSION: &str = "1";
/// Pipeline version of the merge-DNG sidecar: the merged DNG re-enters the
/// normal single-image pipeline, so it carries the same version as `import`.
pub const MERGE_PIPELINE_VERSION: &str = "raster-mvp-1";
/// Neutral exposure provenance for panorama sources without EXIF exposure
/// (panorama pixels never use exposure; HDR rejects such sources loudly
/// instead — see the frontend exposure resolvers).
pub const PANO_DEFAULT_EXPOSURE_S: f64 = 0.01;
pub const PANO_DEFAULT_ISO: u32 = 100;
pub const PANO_DEFAULT_F_NUMBER: f64 = 8.0;

/// Canonical command name of a merge mode (logging, CLI `command` field).
#[must_use]
pub fn merge_command_name(mode: MergeMode) -> &'static str {
    match mode {
        MergeMode::Hdr => "merge-hdr",
        MergeMode::Panorama => "merge-pano",
    }
}

/// `blake3:<hex>` checksum contract shared with the sidecar artifact refs.
#[must_use]
pub fn merge_checksum(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}

/// Row-major RGBA8 (`0..=255`, alpha ignored) → linear `f32` RGB frame.
/// Pure; `None` on a malformed buffer (never a guessed reshape).
#[must_use]
pub fn linear_from_rgba(frame: &lumina_core::ImageFrame) -> Option<LinearImage> {
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

/// Neutral EXIF subset a frontend decode adapter hands to the orchestrator:
/// the source metadata needed for the DNG tags and the EXIF exposure
/// fallback. Raw values are stored unfiltered; the accessors apply the
/// documented validity ranges (rejected, never clipped).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MergeExif {
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub exposure_time_s: Option<f32>,
    pub f_number: Option<f32>,
    pub iso: Option<f32>,
    pub timestamp: Option<i64>,
}

impl MergeExif {
    /// Valid EXIF exposure time (`(0, MAX_MERGE_EXPOSURE_TIME_S]`, finite).
    #[must_use]
    pub fn valid_exposure_time_s(&self) -> Option<f64> {
        self.exposure_time_s
            .filter(|value| {
                value.is_finite() && *value > 0.0 && f64::from(*value) <= MAX_MERGE_EXPOSURE_TIME_S
            })
            .map(f64::from)
    }

    /// Valid EXIF ISO (`1..=MAX_MERGE_ISO`, finite, rounded to the integer
    /// EXIF contract).
    #[must_use]
    pub fn valid_iso(&self) -> Option<u32> {
        self.iso
            .filter(|value| value.is_finite() && *value >= 1.0 && *value <= MAX_MERGE_ISO as f32)
            .map(|value| value.round() as u32)
            .filter(|value| (1..=MAX_MERGE_ISO).contains(value))
    }

    /// Valid EXIF f-number (`(0, MAX_MERGE_F_NUMBER]`, finite).
    #[must_use]
    pub fn valid_f_number(&self) -> Option<f64> {
        self.f_number
            .filter(|value| {
                value.is_finite() && *value > 0.0 && f64::from(*value) <= MAX_MERGE_F_NUMBER
            })
            .map(f64::from)
    }

    /// All-or-nothing EXIF exposure (the GUI policy default). `None` when any
    /// of the three values is missing or out of range.
    #[must_use]
    pub fn exposure(&self) -> Option<MergeExposure> {
        Some(MergeExposure {
            exposure_time_s: self.valid_exposure_time_s()?,
            iso: self.valid_iso()?,
            f_number: self.valid_f_number()?,
        })
    }

    /// DNG EXIF identity for the merge artifact. Every field is optional: a
    /// missing or unusable reference field omits the tag (never guessed,
    /// never defaulted).
    #[must_use]
    pub fn to_dng_exif(&self) -> DngExif {
        let ascii = |value: &Option<String>| {
            value
                .as_ref()
                .filter(|v| !v.trim().is_empty() && !v.contains('\0'))
                .cloned()
        };
        DngExif {
            make: ascii(&self.camera_make),
            model: ascii(&self.camera_model),
            lens: ascii(&self.lens),
            exposure_time_s: self.valid_exposure_time_s(),
            f_number: self.valid_f_number(),
            iso_speed: self
                .iso
                .filter(|value| value.is_finite() && *value >= 1.0 && *value <= 65_535.0)
                .map(|value| value.round() as u32)
                .filter(|value| *value >= 1),
            timestamp: self.timestamp,
        }
    }
}

/// One decoded merge source: file identity plus the linear frame, decode
/// context and EXIF subset the recipe records. Produced by the frontend
/// decode adapter, consumed by [`run_merge`].
#[derive(Debug, Clone)]
pub struct MergeSourceFrame {
    pub path: PathBuf,
    pub content_hash: String,
    pub frame: LinearImage,
    pub decode_context: MergeDecodeContext,
    pub exif: MergeExif,
}

/// Frontend-independent run options (CLI maps its flags 1:1; the GUI uses the
/// deterministic default output next to the reference source).
#[derive(Debug, Clone)]
pub struct MergeRunOptions {
    /// Explicit DNG target (`--output`). `None` = `<reference-stem>-HDR.dng`
    /// / `<reference-stem>-Pano.dng` next to the reference source.
    pub output: Option<PathBuf>,
    pub max_shift_px: i32,
    pub blend_width_px: u32,
    /// Re-merge even when an existing bundle is stale or dangling. Without it
    /// staleness is a loud error, never a silent overwrite.
    pub force: bool,
    /// Decoder version recorded in the merge-DNG source identity. The merge
    /// DNG is always re-imported via LibRaw, so the frontend supplies
    /// `lumina_raw::libraw_decode_version()` here: `lumina-merge` stays free
    /// of the native RAW dependency.
    pub dng_decode_version: String,
}

/// Loud merge-run failure with the stable frontend prefixes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeRunError {
    /// A source, the merge DNG or its sidecar is gone: `merge missing:`.
    Missing(String),
    /// An existing bundle no longer matches the sources: `merge stale:`.
    Stale(String),
    /// Decode/geometry/exposure/dimension/writer scope limit:
    /// `merge unsupported:`.
    Unsupported(String),
    /// Unexpected I/O, serialize or validation failure: `merge failed:`.
    Failed(String),
}

impl MergeRunError {
    /// Stable stderr prefix of the variant (without the `merge `-prefix).
    #[must_use]
    pub fn prefix(&self) -> &'static str {
        match self {
            MergeRunError::Missing(_) => "missing",
            MergeRunError::Stale(_) => "stale",
            MergeRunError::Unsupported(_) => "unsupported",
            MergeRunError::Failed(_) => "failed",
        }
    }

    /// Human-readable cause (without the prefix).
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            MergeRunError::Missing(message)
            | MergeRunError::Stale(message)
            | MergeRunError::Unsupported(message)
            | MergeRunError::Failed(message) => message,
        }
    }
}

impl std::fmt::Display for MergeRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "merge {}: {}", self.prefix(), self.message())
    }
}

impl std::error::Error for MergeRunError {}

/// Outcome of one merge run. `cached = true` means the existing bundle was
/// already current (digest + DNG checksum match) and nothing was rewritten.
#[derive(Debug, Clone, PartialEq)]
pub struct MergeRunOutcome {
    pub mode: MergeMode,
    pub dng_path: PathBuf,
    pub sidecar_path: PathBuf,
    pub digest: String,
    pub checksum: String,
    pub cached: bool,
    /// Alignment residual in px (`residual_warning` above the documented
    /// threshold is a visible warning, never a silent abort).
    pub residual_px: f64,
    pub residual_warning: bool,
}

/// Document kind of an existing sidecar at a merge target (MERGE-DNG-1
/// envelope discriminator). Only [`MergeDocumentKind::Merge`] may be replaced
/// without explicit `force`; the others are rejected loudly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeDocumentKind {
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
#[must_use]
pub fn classify_merge_document(document: &SidecarDocument) -> MergeDocumentKind {
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

/// Parses the persisted merge recipe out of a sidecar document (loud
/// `Err(reason)`; never a silent default).
pub fn stored_merge_recipe(document: &SidecarDocument) -> Result<MergeRecipe, String> {
    let value = document
        .extras
        .get(MERGE_RECIPE_KEY)
        .ok_or_else(|| "has no merge recipe".to_string())?;
    MergeRecipe::from_json(&value.to_string())
        .map_err(|error| format!("has an invalid merge recipe ({error})"))
}

/// Parses the persisted merge DNG artifact reference out of a sidecar
/// document (loud `Err(reason)`; never a silent default).
pub fn stored_merge_artifact(document: &SidecarDocument) -> Result<ArtifactReference, String> {
    let value = document
        .extras
        .get(MERGE_ARTIFACT_KEY)
        .ok_or_else(|| "has no merge artifact reference".to_string())?;
    serde_json::from_value(value.clone())
        .map_err(|error| format!("has an invalid merge artifact reference ({error})"))
}

/// The **single** HDR/panorama merge orchestration.
///
/// Sequence (identical on CLI and GUI, SOLL §Pipeline-Einordnung):
/// validate count/options → decode each source → resolve per-source exposure →
/// align + merge (`lumina-merge`) → resolve the DNG target → build + validate
/// the recipe → digest → envelope/stale/missing gate → encode the linear DNG →
/// build the sidecar document → atomic DNG+sidecar publication.
///
/// `decode` is the frontend decode adapter; `resolve_exposure` the frontend
/// exposure policy (both return [`MergeRunError`] for loud failures). The
/// returned [`MergeRunOutcome`] carries everything a frontend needs to report
/// (paths, digest, checksum, cached flag, residual warning).
pub fn run_merge<D, R>(
    mode: MergeMode,
    inputs: &[PathBuf],
    options: &MergeRunOptions,
    mut decode: D,
    mut resolve_exposure: R,
) -> Result<MergeRunOutcome, MergeRunError>
where
    D: FnMut(&Path) -> Result<MergeSourceFrame, MergeRunError>,
    R: FnMut(MergeMode, usize, &MergeSourceFrame) -> Result<MergeExposure, MergeRunError>,
{
    let command = merge_command_name(mode);
    validate_source_count(inputs.len())?;
    if options.max_shift_px < 0 {
        return Err(MergeRunError::Failed(format!(
            "invalid --max-shift-px {}: must be >= 0",
            options.max_shift_px
        )));
    }
    info!("{command}: merging {} source(s)", inputs.len());

    let mut sources = Vec::with_capacity(inputs.len());
    for (index, input) in inputs.iter().enumerate() {
        let source = decode(input)?;
        info!(
            "{command}: decoded source #{index} `{}` ({}x{}, {}, {})",
            input.display(),
            source.frame.width(),
            source.frame.height(),
            source.decode_context.decoder,
            source.content_hash,
        );
        sources.push(source);
    }

    let mut exposures = Vec::with_capacity(sources.len());
    for (index, source) in sources.iter().enumerate() {
        exposures.push(resolve_exposure(mode, index, source)?);
    }

    let frames: Vec<LinearImage> = sources.iter().map(|source| source.frame.clone()).collect();
    let pass = align_and_merge(
        mode,
        &frames,
        &exposures,
        options.max_shift_px,
        options.blend_width_px,
    )?;
    info!(
        "{command}: merged frame {}x{}",
        pass.merged.width(),
        pass.merged.height()
    );

    let dng_path = resolve_dng_path(mode, &sources, options.output.as_deref())?;
    let sidecar_path = sidecar_path_for(&dng_path);
    let bundle_dir = dng_path.parent().unwrap_or_else(|| Path::new("."));
    let input_paths: Vec<PathBuf> = sources.iter().map(|source| source.path.clone()).collect();
    let source_paths = bundle_relative_paths(bundle_dir, &input_paths)?;
    let dng_file_name = dng_file_name(&dng_path)?;
    let recipe = build_recipe(RecipeInputs {
        mode,
        sources: &sources,
        source_paths: &source_paths,
        exposures: &exposures,
        transforms: &pass.transforms,
        residual_px: pass.residual_px,
        dng_file_name: &dng_file_name,
        blend_width_px: options.blend_width_px,
    });
    recipe
        .validate()
        .map_err(|error| MergeRunError::Failed(error.to_string()))?;
    let digest = recipe.digest();

    if let Some(outcome) = check_existing_bundle(BundleGate {
        command,
        mode,
        force: options.force,
        sidecar_path: &sidecar_path,
        dng_path: &dng_path,
        digest: &digest,
        residual_px: pass.residual_px,
        residual_warning: pass.residual_warning,
    })? {
        return Ok(outcome);
    }

    let exif = sources[0].exif.to_dng_exif();
    let dng_bytes = encode_linear_dng(&pass.merged, mode, &exif).map_err(map_dng_error)?;
    validate_dng_file_name(&dng_file_name).map_err(map_dng_error)?;
    info!(
        "{command}: encoded linear DNG ({} bytes, {}x{})",
        dng_bytes.len(),
        pass.merged.width(),
        pass.merged.height()
    );

    // Stage the DNG, commit the sidecar, then publish the DNG — like
    // `process_selected`, a failed sidecar save leaves nothing behind.
    let document = build_merge_document(
        &dng_file_name,
        &dng_bytes,
        &pass.merged,
        &recipe,
        &options.dng_decode_version,
    )?;
    stage_and_publish(&dng_path, &dng_bytes, &sidecar_path, &document)?;
    let checksum = merge_checksum(&dng_bytes);
    info!("{command}: wrote `{}` ({checksum})", dng_path.display());
    info!("{command}: wrote `{}`", sidecar_path.display());

    Ok(MergeRunOutcome {
        mode,
        dng_path,
        sidecar_path,
        digest,
        checksum,
        cached: false,
        residual_px: pass.residual_px,
        residual_warning: pass.residual_warning,
    })
}

/// Source-count validation (both frontends share the same loud contract).
fn validate_source_count(count: usize) -> Result<(), MergeRunError> {
    if count < MIN_MERGE_SOURCES {
        return Err(MergeRunError::Failed(format!(
            "merge needs at least {MIN_MERGE_SOURCES} sources (max {MAX_MERGE_SOURCES}), got {count}"
        )));
    }
    if count > MAX_MERGE_SOURCES {
        return Err(MergeRunError::Failed(format!(
            "merge exceeds maximum of {MAX_MERGE_SOURCES} sources, got {count}"
        )));
    }
    Ok(())
}

/// Result of one alignment + merge pass.
struct MergePass {
    merged: LinearImage,
    transforms: Vec<MergeTransform>,
    residual_px: f64,
    residual_warning: bool,
}

/// Alignment (via `lumina-merge`) plus the pixel merge, exactly as the CLI and
/// GUI performed it before the F6 dedup.
fn align_and_merge(
    mode: MergeMode,
    frames: &[LinearImage],
    exposures: &[MergeExposure],
    max_shift_px: i32,
    blend_width_px: u32,
) -> Result<MergePass, MergeRunError> {
    match mode {
        MergeMode::Hdr => {
            let mut shifts = Vec::with_capacity(frames.len());
            let mut transforms = Vec::new();
            let mut residual_px: f64 = 0.0;
            let mut residual_warning = false;
            shifts.push((0.0, 0.0));
            for (index, frame) in frames.iter().enumerate().skip(1) {
                let shift = estimate_hdr_translation(&frames[0], frame, max_shift_px)
                    .map_err(map_merge_error)?;
                shifts.push((shift.dx, shift.dy));
                transforms.push(MergeTransform {
                    source_index: index,
                    matrix_3x3: [1.0, 0.0, shift.dx, 0.0, 1.0, shift.dy, 0.0, 0.0, 1.0],
                });
                residual_px = residual_px.max(shift.residual_px);
                residual_warning |= shift.status.residual_flag();
            }
            let merged = merge_hdr_weighted(frames, exposures, &shifts).map_err(map_merge_error)?;
            Ok(MergePass {
                merged,
                transforms,
                residual_px,
                residual_warning,
            })
        }
        MergeMode::Panorama => {
            // Full 3x3 matrix per frame (translation + rotation about the
            // frame centre, `lumina-merge::pano_matrix`). The blend applies
            // each frame through its **full inverse matrix**
            // ([`blend_panorama_transformed`]), never only the integer
            // offsets (which silently dropped the estimated rotation).
            let mut matrices = Vec::with_capacity(frames.len());
            let mut transforms = Vec::new();
            let mut residual_px: f64 = 0.0;
            let mut residual_warning = false;
            matrices.push([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
            for (index, frame) in frames.iter().enumerate().skip(1) {
                let transform = estimate_pano_transform(&frames[0], frame, max_shift_px)
                    .map_err(map_merge_error)?;
                matrices.push(transform.matrix_3x3);
                transforms.push(MergeTransform {
                    source_index: index,
                    matrix_3x3: transform.matrix_3x3,
                });
                info!(
                    "merge-pano: source #{index} aligned with rotation {:.3}deg (matrix {:?})",
                    transform.rotation_deg, transform.matrix_3x3
                );
                residual_px = residual_px.max(transform.residual_px);
                residual_warning |= transform.status.residual_flag();
            }
            let merged = blend_panorama_transformed(frames, &matrices, blend_width_px)
                .map_err(map_merge_error)?;
            Ok(MergePass {
                merged,
                transforms,
                residual_px,
                residual_warning,
            })
        }
    }
}

/// Every merge-domain failure (scope limit or incompatible input set) is
/// `unsupported`: loud, with the cause, never a silent fallback.
fn map_merge_error(error: MergeError) -> MergeRunError {
    match error {
        MergeError::Unsupported(message) => MergeRunError::Unsupported(message),
        MergeError::Invalid(message) => {
            MergeRunError::Unsupported(format!("invalid merge input: {message}"))
        }
    }
}

fn map_dng_error(error: DngError) -> MergeRunError {
    match error {
        DngError::Unsupported(message) => MergeRunError::Unsupported(message),
        other => MergeRunError::Failed(other.to_string()),
    }
}

/// Target DNG path: explicit `--output` (must end in `.dng`, must have a file
/// name and must not overwrite a source) or the deterministic default next to
/// the reference source.
fn resolve_dng_path(
    mode: MergeMode,
    sources: &[MergeSourceFrame],
    output: Option<&Path>,
) -> Result<PathBuf, MergeRunError> {
    if let Some(output) = output {
        let is_dng = output
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dng"));
        if !is_dng {
            return Err(MergeRunError::Failed(format!(
                "invalid --output `{}`: merge writes a linear DNG, the target must end in `.dng`",
                output.display()
            )));
        }
        if output
            .file_name()
            .is_none_or(|name| name.is_empty() || name == "." || name == "..")
        {
            return Err(MergeRunError::Failed(format!(
                "invalid --output `{}`: target must have a file name",
                output.display()
            )));
        }
        for source in sources {
            if lumina_sidecar::paths_resolve_equal(&source.path, output).map_err(|error| {
                MergeRunError::Failed(format!(
                    "cannot resolve `{}` and `{}`: {error}",
                    source.path.display(),
                    output.display()
                ))
            })? {
                return Err(MergeRunError::Failed(format!(
                    "invalid --output `{}`: refusing to overwrite the merge source `{}`",
                    output.display(),
                    source.path.display()
                )));
            }
        }
        return Ok(output.to_path_buf());
    }
    let reference = &sources[0].path;
    let reference_name = reference
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            MergeRunError::Unsupported(format!(
                "reference source `{}` has no usable file name",
                reference.display()
            ))
        })?;
    let file_name = merge_dng_filename(reference_name, mode).map_err(map_dng_error)?;
    let parent = reference.parent().unwrap_or_else(|| Path::new("."));
    Ok(parent.join(file_name))
}

/// Bundle-relative source references (portable: no absolute paths, no
/// `..` segments — the recipe schema rejects both loudly). Sources must live
/// inside the bundle directory; anything else is `unsupported`.
fn bundle_relative_paths(
    bundle_dir: &Path,
    inputs: &[PathBuf],
) -> Result<Vec<String>, MergeRunError> {
    inputs
        .iter()
        .map(|input| {
            let stripped = input.strip_prefix(bundle_dir).map_err(|_| {
                MergeRunError::Unsupported(format!(
                    "source `{}` is outside the merge bundle directory `{}` \
                     (relative references cannot leave the bundle; place the sources \
                     next to the merge DNG)",
                    input.display(),
                    bundle_dir.display()
                ))
            })?;
            if stripped.as_os_str().is_empty() {
                return Err(MergeRunError::Unsupported(format!(
                    "source `{}` has no usable bundle-relative path",
                    input.display()
                )));
            }
            let mut parts = Vec::new();
            for component in stripped.components() {
                match component {
                    std::path::Component::Normal(part) => {
                        let text = part.to_str().ok_or_else(|| {
                            MergeRunError::Unsupported(format!(
                                "source `{}` has a non-UTF-8 path component",
                                input.display()
                            ))
                        })?;
                        parts.push(text.to_string());
                    }
                    _ => {
                        return Err(MergeRunError::Unsupported(format!(
                            "source `{}` has no portable bundle-relative path \
                             (absolute paths and `.`/`..` are forbidden)",
                            input.display()
                        )));
                    }
                }
            }
            Ok(parts.join("/"))
        })
        .collect()
}

fn dng_file_name(dng_path: &Path) -> Result<String, MergeRunError> {
    let name = dng_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            MergeRunError::Failed(format!(
                "target `{}` has no usable file name",
                dng_path.display()
            ))
        })?;
    validate_dng_file_name(name).map_err(map_dng_error)?;
    Ok(name.to_string())
}

/// Inputs of one merge-recipe build (grouped so the builder takes a single
/// argument).
struct RecipeInputs<'a> {
    mode: MergeMode,
    sources: &'a [MergeSourceFrame],
    source_paths: &'a [String],
    exposures: &'a [MergeExposure],
    transforms: &'a [MergeTransform],
    residual_px: f64,
    dng_file_name: &'a str,
    blend_width_px: u32,
}

/// Assembles the merge recipe (validation happens in [`run_merge`]: unknown
/// modes/methods and out-of-range values are rejected, never clipped).
fn build_recipe(inputs: RecipeInputs<'_>) -> MergeRecipe {
    let RecipeInputs {
        mode,
        sources,
        source_paths,
        exposures,
        transforms,
        residual_px,
        dng_file_name,
        blend_width_px,
    } = inputs;
    let (method, projection) = match mode {
        MergeMode::Hdr => (MergeAlignmentMethod::HdrTranslate, MergeProjection::None),
        MergeMode::Panorama => (
            MergeAlignmentMethod::PanoCylindricalHomography,
            MergeProjection::Cylindrical,
        ),
    };
    MergeRecipe {
        merge_version: MERGE_RECIPE_VERSION,
        mode,
        sources: sources
            .iter()
            .zip(source_paths.iter().zip(exposures.iter()))
            .map(|(source, (path, exposure))| MergeSource {
                path: path.clone(),
                content_hash: source.content_hash.clone(),
                decode_context: source.decode_context.clone(),
                exposure: exposure.clone(),
            })
            .collect(),
        alignment: MergeAlignment {
            method,
            transforms: transforms.to_vec(),
            residual_px,
            projection,
            blend_width_px,
        },
        output: MergeOutput {
            file: dng_file_name.to_string(),
            bits: MERGE_OUTPUT_BITS,
            mosaic: false,
        },
        created_at: now_rfc3339_utc(),
        status: MergeStatus::Ok,
        error: None,
    }
}

/// Existing-bundle gate inputs (grouped to keep the argument list readable).
struct BundleGate<'a> {
    command: &'a str,
    mode: MergeMode,
    force: bool,
    sidecar_path: &'a Path,
    dng_path: &'a Path,
    digest: &'a str,
    residual_px: f64,
    residual_warning: bool,
}

/// Stale/missing gate over an existing bundle (SOLL §Persistenz: a merge is
/// valid while source hashes, decode contexts and the DNG checksum match).
///
/// Returns `Ok(None)` when the merge may proceed, or `Ok(Some(outcome))` with
/// the early exit for the already-current bundle.
fn check_existing_bundle(gate: BundleGate<'_>) -> Result<Option<MergeRunOutcome>, MergeRunError> {
    let BundleGate {
        command,
        mode,
        force,
        sidecar_path,
        dng_path,
        digest,
        residual_px,
        residual_warning,
    } = gate;
    if !sidecar_path.exists() {
        if dng_path.exists() && !force {
            return Err(MergeRunError::Missing(format!(
                "merge DNG `{}` exists without its sidecar `{}` \
                 (regenerate the bundle explicitly with force)",
                dng_path.display(),
                sidecar_path.display()
            )));
        }
        return Ok(None);
    }
    if force {
        info!("{command}: force regenerates the existing bundle");
        return Ok(None);
    }
    let document = load_sidecar(sidecar_path).map_err(|error| {
        MergeRunError::Missing(format!(
            "existing merge sidecar `{}` is unreadable ({error}); \
             regenerate the bundle explicitly with force",
            sidecar_path.display()
        ))
    })?;
    match classify_merge_document(&document) {
        MergeDocumentKind::Merge => {}
        MergeDocumentKind::Standard => {
            return Err(MergeRunError::Stale(format!(
                "existing sidecar `{}` is a standard sidecar \
                 (no `\"type\": \"{}\"` envelope); refusing to overwrite it — \
                 regenerate the bundle explicitly with force",
                sidecar_path.display(),
                MERGE_ENVELOPE_TYPE
            )));
        }
        MergeDocumentKind::Unknown(kind) => {
            return Err(MergeRunError::Stale(format!(
                "existing sidecar `{}` has unknown document type `{kind}` \
                 (expected `\"type\": \"{}\"`); refusing to overwrite it — \
                 regenerate the bundle explicitly with force",
                sidecar_path.display(),
                MERGE_ENVELOPE_TYPE
            )));
        }
    }
    let stored_recipe = stored_merge_recipe(&document)
        .map_err(|reason| stale_bundle_error(sidecar_path, &reason))?;
    if stored_recipe.digest() != digest {
        return Err(MergeRunError::Stale(format!(
            "sources, decode context or alignment changed since `{}` \
             was written (digest mismatch); re-merge explicitly with force",
            sidecar_path.display()
        )));
    }
    let stored_artifact = stored_merge_artifact(&document)
        .map_err(|reason| stale_bundle_error(sidecar_path, &reason))?;
    let dng_bytes = fs::read(dng_path).map_err(|_| {
        MergeRunError::Missing(format!(
            "merge DNG `{}` is gone (its sidecar `{}` remains); \
             regenerate the bundle explicitly with force",
            dng_path.display(),
            sidecar_path.display()
        ))
    })?;
    if merge_checksum(&dng_bytes) != stored_artifact.checksum {
        return Err(MergeRunError::Stale(format!(
            "merge DNG `{}` changed on disk (checksum mismatch); \
             re-merge explicitly with force",
            dng_path.display()
        )));
    }
    info!("{command}: bundle already current (digest match, nothing rewritten)");
    Ok(Some(MergeRunOutcome {
        mode,
        dng_path: dng_path.to_path_buf(),
        sidecar_path: sidecar_path.to_path_buf(),
        digest: digest.to_string(),
        checksum: stored_artifact.checksum.clone(),
        cached: true,
        residual_px,
        residual_warning,
    }))
}

fn stale_bundle_error(sidecar_path: &Path, reason: &str) -> MergeRunError {
    MergeRunError::Stale(format!(
        "existing sidecar `{}` {reason}; regenerate the bundle explicitly with force",
        sidecar_path.display()
    ))
}

/// Merge-DNG sidecar document: source identity of the new DNG plus the
/// `"type": "merge"` envelope, the recipe and the DNG artifact reference as
/// document-level extras (additive — no schema change).
fn build_merge_document(
    dng_file_name: &str,
    dng_bytes: &[u8],
    merged: &LinearImage,
    recipe: &MergeRecipe,
    dng_decode_version: &str,
) -> Result<SidecarDocument, MergeRunError> {
    let (width, height) = (merged.width(), merged.height());
    let checksum = merge_checksum(dng_bytes);
    let source = SourceIdentity {
        relative_name: dng_file_name.to_string(),
        content_hash: checksum.clone(),
        byte_length: dng_bytes.len() as u64,
        modified_at: None,
        raw_format: "DNG".into(),
        orientation: 1,
        decode_fingerprint: DecodeFingerprint {
            decoder: "libraw".into(),
            version: dng_decode_version.to_string(),
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
    let mut document = SidecarDocument::new(source, MERGE_PIPELINE_VERSION);
    let recipe_value =
        serde_json::to_value(recipe).map_err(|error| MergeRunError::Failed(error.to_string()))?;
    let artifact = ArtifactReference {
        relative_path: dng_file_name.to_string(),
        format: MERGE_DNG_FORMAT.into(),
        checksum,
        width,
        height,
        channels: MERGE_DNG_CHANNELS.into(),
        data_version: MERGE_DNG_DATA_VERSION.into(),
        extras: std::collections::BTreeMap::new(),
    };
    let artifact_value = serde_json::to_value(&artifact)
        .map_err(|error| MergeRunError::Failed(error.to_string()))?;
    document
        .extras
        .insert("type".into(), serde_json::Value::from(MERGE_ENVELOPE_TYPE));
    document
        .extras
        .insert(MERGE_RECIPE_KEY.into(), recipe_value);
    document
        .extras
        .insert(MERGE_ARTIFACT_KEY.into(), artifact_value);
    Ok(document)
}

/// Atomic publication in the CLI order: stage the DNG (same-directory temp +
/// fsync), commit the sidecar, then rename the DNG into place. A failed
/// sidecar save leaves nothing behind; incomplete files never count as valid.
fn stage_and_publish(
    dng_path: &Path,
    dng_bytes: &[u8],
    sidecar_path: &Path,
    document: &SidecarDocument,
) -> Result<(), MergeRunError> {
    let parent = dng_path.parent().unwrap_or_else(|| Path::new("."));
    let filename = dng_path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "artifact".into());
    let mut temporary = tempfile::Builder::new()
        .prefix(&format!(".{filename}.tmp-"))
        .tempfile_in(parent)
        .map_err(|error| {
            MergeRunError::Failed(format!("cannot stage `{}`: {error}", dng_path.display()))
        })?;
    let temporary_path = temporary.path().to_path_buf();
    temporary.write_all(dng_bytes).map_err(|error| {
        MergeRunError::Failed(format!(
            "cannot stage `{}`: {error}",
            temporary_path.display()
        ))
    })?;
    temporary.flush().map_err(|error| {
        MergeRunError::Failed(format!(
            "cannot stage `{}`: {error}",
            temporary_path.display()
        ))
    })?;
    temporary.as_file().sync_all().map_err(|error| {
        MergeRunError::Failed(format!(
            "cannot stage `{}`: {error}",
            temporary_path.display()
        ))
    })?;
    if let Err(error) = save_sidecar(sidecar_path, document) {
        return Err(MergeRunError::Failed(format!(
            "sidecar write failed: {error}"
        )));
    }
    temporary.persist(dng_path).map_err(|error| {
        MergeRunError::Failed(format!(
            "cannot publish `{}`: {}",
            dng_path.display(),
            error.error
        ))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn explicit_exposures(count: usize) -> Vec<MergeExposure> {
        (0..count)
            .map(|index| MergeExposure {
                exposure_time_s: 0.01 * (index as f64 + 1.0),
                iso: 100,
                f_number: 8.0,
            })
            .collect()
    }

    fn options() -> MergeRunOptions {
        MergeRunOptions {
            output: None,
            max_shift_px: 16,
            blend_width_px: 64,
            force: false,
            dng_decode_version: "test".into(),
        }
    }

    /// Minimal frontend-style decode adapter for the PNG test fixtures.
    fn decode_source(path: &Path) -> Result<MergeSourceFrame, MergeRunError> {
        let bytes = fs::read(path).map_err(|error| {
            MergeRunError::Missing(format!("cannot read source `{}`: {error}", path.display()))
        })?;
        let frame = lumina_core::ImageFrame::decode(&bytes)
            .map_err(|error| MergeRunError::Unsupported(error.to_string()))?;
        let linear = linear_from_rgba(&frame)
            .ok_or_else(|| MergeRunError::Unsupported("malformed RGBA8 frame".to_string()))?;
        Ok(MergeSourceFrame {
            path: path.to_path_buf(),
            content_hash: merge_checksum(&bytes),
            frame: linear,
            decode_context: MergeDecodeContext {
                decoder: "image".into(),
                decode_version: env!("CARGO_PKG_VERSION").into(),
                orientation: 1,
            },
            exif: MergeExif::default(),
        })
    }

    fn resolve_explicit(
        exposures: &[MergeExposure],
    ) -> impl FnMut(MergeMode, usize, &MergeSourceFrame) -> Result<MergeExposure, MergeRunError> + '_
    {
        move |_mode, index, _source| Ok(exposures[index].clone())
    }

    /// Runs the shared orchestrator end-to-end through a frontend-style
    /// decode closure and explicit per-source exposures.
    fn run_hdr_with(
        inputs: &[PathBuf],
        options: &MergeRunOptions,
    ) -> Result<MergeRunOutcome, MergeRunError> {
        let exposures = explicit_exposures(inputs.len());
        run_merge(
            MergeMode::Hdr,
            inputs,
            options,
            decode_source,
            resolve_explicit(&exposures),
        )
    }

    fn run_hdr(inputs: &[PathBuf]) -> Result<MergeRunOutcome, MergeRunError> {
        run_hdr_with(inputs, &options())
    }

    fn write_gradient(dir: &Path, name: &str, level: u8) -> PathBuf {
        let pixels: Vec<u8> = (0..64 * 48)
            .flat_map(|i| {
                let x = (i % 64) as u8;
                let value = level.saturating_add(x % 16);
                [value, value, value, 255]
            })
            .collect();
        let png = lumina_core::ImageFrame::new(64, 48, pixels)
            .unwrap()
            .encode(lumina_core::ImageFileFormat::Png)
            .unwrap();
        let path = dir.join(name);
        fs::write(&path, png).unwrap();
        path
    }

    #[test]
    fn run_merge_writes_bundle_and_reports_cached_on_second_run() {
        let dir = tempfile::tempdir().unwrap();
        let a = write_gradient(dir.path(), "a.png", 40);
        let b = write_gradient(dir.path(), "b.png", 160);
        let inputs = vec![a, b];
        let first = run_hdr(&inputs).unwrap();
        assert!(!first.cached);
        assert_eq!(first.dng_path, dir.path().join("a-HDR.dng"));
        assert!(first.dng_path.is_file());
        assert!(first.sidecar_path.is_file());
        let document = load_sidecar(&first.sidecar_path).unwrap();
        assert_eq!(classify_merge_document(&document), MergeDocumentKind::Merge);
        let recipe = stored_merge_recipe(&document).unwrap();
        assert_eq!(recipe.digest(), first.digest);
        let artifact = stored_merge_artifact(&document).unwrap();
        assert_eq!(artifact.checksum, first.checksum);
        assert_eq!(
            merge_checksum(&fs::read(&first.dng_path).unwrap()),
            first.checksum,
            "DNG checksum anchor"
        );

        // The envelope, recipe and artifact travel as sidecar extras and
        // survive a save/load roundtrip without absolute paths.
        let json = document.to_json().unwrap();
        assert!(json.contains("\"type\": \"merge\""));
        assert!(
            !json.contains(&dir.path().to_string_lossy().to_string()),
            "merge sidecar must stay bundle-relative: {json}"
        );
        let parsed = SidecarDocument::from_json(&json).unwrap();
        assert_eq!(classify_merge_document(&parsed), MergeDocumentKind::Merge);
        assert_eq!(stored_merge_recipe(&parsed).unwrap().digest(), first.digest);
        assert_eq!(
            stored_merge_artifact(&parsed).unwrap().checksum,
            first.checksum
        );

        let second = run_hdr(&inputs).unwrap();
        assert!(second.cached, "unchanged inputs report the current bundle");
        assert_eq!(second.checksum, first.checksum);
    }

    #[test]
    fn run_merge_is_deterministic_for_identical_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let a = write_gradient(dir.path(), "d_a.png", 40);
        let b = write_gradient(dir.path(), "d_b.png", 160);
        let inputs = vec![a, b];
        let first = run_hdr(&inputs).unwrap();
        let second = run_hdr(&inputs).unwrap();
        assert_eq!(first.digest, second.digest);
        assert_eq!(first.checksum, second.checksum);
    }

    #[test]
    fn standard_sidecar_envelope_conflict_is_loud_and_non_destructive() {
        let dir = tempfile::tempdir().unwrap();
        let a = write_gradient(dir.path(), "c_a.png", 40);
        let b = write_gradient(dir.path(), "c_b.png", 160);
        let inputs = vec![a, b];
        let first = run_hdr(&inputs).unwrap();
        let mut document = load_sidecar(&first.sidecar_path).unwrap();
        document.extras.remove("type");
        save_sidecar(&first.sidecar_path, &document).unwrap();
        let before = fs::read(&first.sidecar_path).unwrap();

        let error = run_hdr(&inputs).unwrap_err();
        assert!(matches!(error, MergeRunError::Stale(_)), "{error}");
        assert!(error.to_string().contains("standard sidecar"), "{error}");
        assert_eq!(fs::read(&first.sidecar_path).unwrap(), before);
    }

    #[test]
    fn unknown_envelope_is_rejected_loudly() {
        let dir = tempfile::tempdir().unwrap();
        let a = write_gradient(dir.path(), "u_a.png", 40);
        let b = write_gradient(dir.path(), "u_b.png", 160);
        let inputs = vec![a, b];
        let first = run_hdr(&inputs).unwrap();
        let mut document = load_sidecar(&first.sidecar_path).unwrap();
        document
            .extras
            .insert("type".into(), serde_json::Value::from("merge-hdr"));
        save_sidecar(&first.sidecar_path, &document).unwrap();
        let error = run_hdr(&inputs).unwrap_err();
        assert!(matches!(error, MergeRunError::Stale(_)), "{error}");
        assert!(
            error.to_string().contains("unknown document type"),
            "{error}"
        );
    }

    #[test]
    fn digest_mismatch_reports_stale_until_forced() {
        let dir = tempfile::tempdir().unwrap();
        let a = write_gradient(dir.path(), "s_a.png", 40);
        let b = write_gradient(dir.path(), "s_b.png", 160);
        let inputs = vec![a, b];
        run_hdr(&inputs).unwrap();
        // Change a source: the stored digest no longer matches.
        write_gradient(dir.path(), "s_b.png", 200);
        let error = run_hdr(&inputs).unwrap_err();
        assert!(matches!(error, MergeRunError::Stale(_)), "{error}");
        assert!(error.to_string().contains("merge stale"), "{error}");

        let mut forced = options();
        forced.force = true;
        let outcome = run_hdr_with(&inputs, &forced).unwrap();
        assert!(!outcome.cached);
    }

    #[test]
    fn missing_dng_with_sidecar_present_is_loud() {
        let dir = tempfile::tempdir().unwrap();
        let a = write_gradient(dir.path(), "m_a.png", 40);
        let b = write_gradient(dir.path(), "m_b.png", 160);
        let inputs = vec![a, b];
        let first = run_hdr(&inputs).unwrap();
        fs::remove_file(&first.dng_path).unwrap();
        let error = run_hdr(&inputs).unwrap_err();
        assert!(matches!(error, MergeRunError::Missing(_)), "{error}");
        assert!(error.to_string().contains("merge missing"), "{error}");
    }

    #[test]
    fn source_count_and_shift_are_validated_loudly() {
        let dir = tempfile::tempdir().unwrap();
        let a = write_gradient(dir.path(), "v_a.png", 40);
        let error = run_hdr(&[a]).unwrap_err();
        assert!(error.to_string().contains("at least 2"), "{error}");

        let dir = tempfile::tempdir().unwrap();
        let a = write_gradient(dir.path(), "n_a.png", 40);
        let b = write_gradient(dir.path(), "n_b.png", 160);
        let mut bad = options();
        bad.max_shift_px = -1;
        let error = run_hdr_with(&[a, b], &bad).unwrap_err();
        assert!(error.to_string().contains("must be >= 0"), "{error}");
    }

    #[test]
    fn exif_accessors_reject_out_of_range_values() {
        let exif = MergeExif {
            exposure_time_s: Some(f32::NAN),
            f_number: Some(f32::INFINITY),
            iso: Some(0.0),
            ..MergeExif::default()
        };
        assert_eq!(exif.valid_exposure_time_s(), None);
        assert_eq!(exif.valid_f_number(), None);
        assert_eq!(exif.valid_iso(), None);
        assert_eq!(exif.exposure(), None);

        let exif = MergeExif {
            exposure_time_s: Some(0.01),
            f_number: Some(8.0),
            iso: Some(100.0),
            ..MergeExif::default()
        };
        let exposure = exif.exposure().unwrap();
        assert!((exposure.exposure_time_s - 0.01).abs() < 1e-9);
        assert_eq!(exposure.iso, 100);
    }

    #[test]
    fn classify_covers_standard_merge_and_unknown() {
        let make = |value: Option<&str>| {
            let source = SourceIdentity {
                relative_name: "a-HDR.dng".into(),
                content_hash: merge_checksum(b"x"),
                byte_length: 1,
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
            let mut document = SidecarDocument::new(source, MERGE_PIPELINE_VERSION);
            if let Some(value) = value {
                document
                    .extras
                    .insert("type".into(), serde_json::Value::from(value));
            }
            document
        };
        assert_eq!(
            classify_merge_document(&make(None)),
            MergeDocumentKind::Standard
        );
        assert_eq!(
            classify_merge_document(&make(Some("merge"))),
            MergeDocumentKind::Merge
        );
        assert_eq!(
            classify_merge_document(&make(Some("pano"))),
            MergeDocumentKind::Unknown("pano".into())
        );
    }

    #[test]
    fn bundle_relative_paths_cover_same_dir_subdir_and_absolute() {
        let bundle = Path::new("/tmp/bundle");
        let inputs = vec![
            PathBuf::from("/tmp/bundle/a.png"),
            PathBuf::from("/tmp/bundle/sub/b.png"),
        ];
        assert_eq!(
            bundle_relative_paths(bundle, &inputs).unwrap(),
            vec!["a.png", "sub/b.png"]
        );
        assert!(bundle_relative_paths(bundle, &[PathBuf::from("/tmp/other/a.png")]).is_err());
        assert!(bundle_relative_paths(bundle, &[PathBuf::from("/tmp/bundle/../a.png")]).is_err());
    }

    #[test]
    fn linear_conversion_is_exact_and_loud() {
        let frame =
            lumina_core::ImageFrame::new(2, 1, vec![255, 0, 0, 128, 0, 128, 0, 255]).unwrap();
        let linear = linear_from_rgba(&frame).unwrap();
        assert_eq!((linear.width(), linear.height()), (2, 1));
        assert_eq!(&linear.pixels()[0..3], &[1.0, 0.0, 0.0]);
        let mut broken = frame.clone();
        broken.pixels.pop();
        assert!(linear_from_rgba(&broken).is_none());
    }
}
