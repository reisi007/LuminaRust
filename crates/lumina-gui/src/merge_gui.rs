//! LRPAR-G13-MERGE-15 — GUI slice: HDR/Panorama merge actions + DNG status.
//!
//! SOLL: `feature/decisions/LRPAR-G13-MERGE-15.md` (§Pipeline-Einordnung,
//! §Persistenz, §Abnahmekriterien „GUI-headless") and
//! `feature/platform/cli-gui-wasm.md` § „HDR-/Panorama-Merge (G-13)".
//!
//! ## Same entry points as the CLI
//!
//! The GUI owns **no** merge image logic. Alignment, blending, the linear DNG
//! writer and the merge-recipe digest are the shared `lumina-merge` functions
//! the CLI commands call, in the same order: decode each source, align
//! (`estimate_hdr_translation` / `estimate_pano_transform`), merge
//! (`merge_hdr_weighted` / `blend_panorama_transformed`), encode
//! (`encode_linear_dng`) and persist the full sidecar bundle with the
//! `"type": "merge"` envelope. The only GUI-local pieces are the orchestration
//! (job control) and the RGBA8→linear frame conversion — the CLI command module
//! is not a library, so the shared logic is `lumina-merge` + `lumina-sidecar`,
//! exactly as the decision requires ("keine GUI-eigene Bildlogik").
//!
//! ## Envelope conflicts are loud
//!
//! A merge writes `<ref>-HDR.dng`/`<ref>-Pano.dng` next to the reference
//! source. If the target sidecar exists and is **not** a merge envelope
//! (`"type": "merge"`), if its stored recipe digest no longer matches the
//! inputs, or if it is unreadable, the merge refuses loudly — only the
//! explicit `force` re-merge replaces it. Nothing is overwritten silently.
//!
//! ## Golden gates (decision: hash/re-import anchors instead of kittest PNG)
//!
//! A merge changes a *data artifact* (linear DNG bytes), not the GUI layout;
//! a kittest PNG golden would only pin the panel chrome and could not detect a
//! merge regression. The golden gates here are therefore **data anchors**
//! (documented tolerance `lumina_merge::FLOAT_TOLERANCE_DOC` for cross-toolchain
//! floats): same inputs → byte-identical DNG (same machine), the DNG BLAKE3
//! equals the sidecar artifact checksum, and a re-import through the pinned
//! LibRaw decoder reproduces the written geometry. This is the documented
//! decision (kittest-PNG unsuitable for a DNG/algorithms change).

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use log::{error, info, warn};
use lumina_merge::{
    blend_panorama_transformed, encode_linear_dng, estimate_hdr_translation,
    estimate_pano_transform, merge_dng_filename, merge_hdr_weighted, DngExif, LinearImage,
};
use lumina_sidecar::{
    load_sidecar, now_rfc3339_utc, save_sidecar, sidecar_path_for, ArtifactReference,
    DecodeFingerprint, GeometryFingerprint, MergeAlignment, MergeAlignmentMethod,
    MergeDecodeContext, MergeExposure, MergeMode, MergeOutput, MergeProjection, MergeRecipe,
    MergeSource, MergeStatus, MergeTransform, SidecarDocument, SourceIdentity,
    MAX_MERGE_EXPOSURE_TIME_S, MAX_MERGE_F_NUMBER, MAX_MERGE_ISO, MAX_MERGE_SOURCES,
    MERGE_OUTPUT_BITS, MERGE_RECIPE_VERSION, MIN_MERGE_SOURCES,
};

use crate::i18n::Str;
use crate::GuiError;

/// Document-level `"type": "merge"` envelope discriminator (MERGE-DNG-1).
pub const MERGE_ENVELOPE_TYPE: &str = "merge";
/// DNG artifact reference payload key (mirrors the CLI constant).
pub const MERGE_ARTIFACT_KEY: &str = "merge_artifact";
/// Merge recipe payload key (mirrors the CLI constant).
pub const MERGE_RECIPE_KEY: &str = "merge_recipe";
/// Panorama provenance defaults for sources without EXIF (panorama pixels
/// never use exposure; HDR rejects such sources loudly instead).
pub const PANO_DEFAULT_EXPOSURE_S: f64 = 0.01;
pub const PANO_DEFAULT_ISO: u32 = 100;
pub const PANO_DEFAULT_F_NUMBER: f64 = 8.0;
const MERGE_DNG_FORMAT: &str = "dng";
const MERGE_DNG_CHANNELS: &str = "rgb16";
const MERGE_DNG_DATA_VERSION: &str = "1";
const MERGE_PIPELINE_VERSION: &str = "raster-mvp-1";

/// Visible status of a persisted merge bundle (decision §Persistenz).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeArtifactStatus {
    /// No merge bundle at the target (nothing to report).
    NoBundle,
    /// Source hashes, decode contexts and the DNG checksum match.
    Ok,
    /// The bundle exists but an input changed (only `force` re-merges).
    Stale,
    /// A bundle part is gone (DNG or sidecar).
    Missing,
    /// The sidecar is not a merge envelope / unreadable / the stored recipe is
    /// invalid (a hard, loud state — never overwritten silently).
    Unsupported,
}

impl MergeArtifactStatus {
    /// Visible status text (status line + panel).
    pub fn text(self) -> &'static str {
        match self {
            MergeArtifactStatus::NoBundle => Str::MergeStatusNone.t(),
            MergeArtifactStatus::Ok => Str::MergeStatusOk.t(),
            MergeArtifactStatus::Stale => Str::MergeStatusStale.t(),
            MergeArtifactStatus::Missing => Str::MergeStatusMissing.t(),
            MergeArtifactStatus::Unsupported => Str::MergeStatusUnsupported.t(),
        }
    }
}

/// Outcome of one merge run (status line, tests and job completion).
#[derive(Debug, Clone, PartialEq)]
pub struct MergeOutcome {
    pub mode: MergeMode,
    pub dng_path: PathBuf,
    pub sidecar_path: PathBuf,
    pub digest: String,
    pub checksum: String,
    /// `true` when the existing bundle was already current (nothing rewritten).
    pub cached: bool,
    /// Alignment residual in px (`AlignedWithResidual` above the documented
    /// threshold is a visible warning, never a silent abort).
    pub residual_px: f64,
    pub residual_warning: bool,
}

/// Running merge job (background thread + result channel). Polled by
/// `LuminaApp::poll_merge_job` in the frame loop — job control with a visible
/// status instead of a frozen UI.
pub struct MergeJob {
    pub mode: MergeMode,
    rx: mpsc::Receiver<Result<MergeOutcome, String>>,
}

impl std::fmt::Debug for MergeJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MergeJob")
            .field("mode", &self.mode)
            .finish()
    }
}

/// RGBA8 (`0..=255`, alpha ignored) → linear `f32` RGB frame. Pure; loud on a
/// malformed buffer (`None`).
pub fn linear_from_frame(frame: &lumina_core::ImageFrame) -> Option<LinearImage> {
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

/// Bundle-relative source references (portable: no absolute paths, no `..`).
pub fn bundle_relative_paths(bundle_dir: &Path, inputs: &[PathBuf]) -> Result<Vec<String>, String> {
    inputs
        .iter()
        .map(|input| {
            let stripped = input.strip_prefix(bundle_dir).map_err(|_| {
                format!(
                    "merge unsupported: source `{}` is outside the merge bundle directory `{}`",
                    input.display(),
                    bundle_dir.display()
                )
            })?;
            if stripped.as_os_str().is_empty() {
                return Err(format!(
                    "merge unsupported: source `{}` has no usable bundle-relative path",
                    input.display()
                ));
            }
            let mut parts = Vec::new();
            for component in stripped.components() {
                match component {
                    std::path::Component::Normal(part) => parts.push(
                        part.to_str()
                            .ok_or_else(|| {
                                format!(
                                    "merge unsupported: source `{}` has a non-UTF-8 path component",
                                    input.display()
                                )
                            })?
                            .to_string(),
                    ),
                    _ => {
                        return Err(format!(
                            "merge unsupported: source `{}` has no portable bundle-relative path",
                            input.display()
                        ))
                    }
                }
            }
            Ok(parts.join("/"))
        })
        .collect()
}

/// Document kind of an existing sidecar at a merge target (MERGE-DNG-1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeDocumentKind {
    Standard,
    Merge,
    Unknown(String),
}

impl MergeDocumentKind {
    /// Classifies the document-level `"type"` extra.
    pub fn classify(document: &SidecarDocument) -> Self {
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
}

/// `blake3:<hex>` checksum contract shared with the sidecar artifact refs.
fn blake3_checksum(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}

struct DecodedSource {
    content_hash: String,
    frame: LinearImage,
    decode_context: MergeDecodeContext,
    raw_metadata: Option<lumina_raw::RawMetadata>,
}

fn decode_merge_sources(inputs: &[PathBuf]) -> Result<Vec<DecodedSource>, String> {
    let mut sources = Vec::with_capacity(inputs.len());
    for input in inputs {
        let (bytes, frame, orientation) =
            crate::decode_selection_frame(input).map_err(|error| {
                if error.contains("No such file") || error.contains("missing") {
                    format!("merge missing: {error}")
                } else {
                    format!("merge unsupported: {error}")
                }
            })?;
        let content_hash = blake3_checksum(&bytes);
        let raw_metadata = lumina_raw::read_metadata(input).ok();
        let decode_context = MergeDecodeContext {
            decoder: if raw_metadata.is_some() {
                "libraw".into()
            } else {
                "image".into()
            },
            decode_version: if raw_metadata.is_some() {
                lumina_raw::libraw_decode_version()
            } else {
                env!("CARGO_PKG_VERSION").into()
            },
            orientation,
        };
        let linear = linear_from_frame(&frame).ok_or_else(|| {
            format!(
                "merge unsupported: source `{}` has {} bytes for a {}x{} RGBA8 frame",
                input.display(),
                frame.pixels.len(),
                frame.width,
                frame.height
            )
        })?;
        sources.push(DecodedSource {
            content_hash,
            frame: linear,
            decode_context,
            raw_metadata,
        });
    }
    Ok(sources)
}

/// Per-source exposure: explicit value wins; otherwise RAW EXIF; HDR without
/// either is `unsupported` (never guessed from pixels); panorama stores EXIF or
/// the documented neutral default as provenance only.
fn resolve_exposure(
    mode: MergeMode,
    index: usize,
    explicit: Option<&[MergeExposure]>,
    source: &DecodedSource,
) -> Result<MergeExposure, String> {
    if let Some(exposure) = explicit.and_then(|values| values.get(index)) {
        return Ok(exposure.clone());
    }
    let from_exif = || -> Option<MergeExposure> {
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
    match mode {
        MergeMode::Hdr => from_exif().ok_or_else(|| {
            format!(
                "merge unsupported: HDR source #{index} has no EXIF exposure \
                 (exposure is never guessed from pixels)"
            )
        }),
        MergeMode::Panorama => Ok(from_exif().unwrap_or(MergeExposure {
            exposure_time_s: PANO_DEFAULT_EXPOSURE_S,
            iso: PANO_DEFAULT_ISO,
            f_number: PANO_DEFAULT_F_NUMBER,
        })),
    }
}

/// Result of one alignment + merge pass (named instead of a bare tuple so the
/// return type stays readable and no `type_complexity` exemption is needed).
struct MergePass {
    merged: LinearImage,
    transforms: Vec<MergeTransform>,
    residual_px: f64,
    residual_warning: bool,
}

fn align_and_merge(
    mode: MergeMode,
    frames: &[LinearImage],
    exposures: &[MergeExposure],
    max_shift_px: i32,
    blend_width_px: u32,
) -> Result<MergePass, String> {
    match mode {
        MergeMode::Hdr => {
            let mut shifts = Vec::with_capacity(frames.len());
            let mut transforms = Vec::new();
            let mut residual_px: f64 = 0.0;
            let mut residual_warning = false;
            shifts.push((0.0, 0.0));
            for (index, frame) in frames.iter().enumerate().skip(1) {
                let shift = estimate_hdr_translation(&frames[0], frame, max_shift_px)
                    .map_err(|error| format!("merge unsupported: {error}"))?;
                shifts.push((shift.dx, shift.dy));
                transforms.push(MergeTransform {
                    source_index: index,
                    matrix_3x3: [1.0, 0.0, shift.dx, 0.0, 1.0, shift.dy, 0.0, 0.0, 1.0],
                });
                residual_px = residual_px.max(shift.residual_px);
                residual_warning |= shift.status.residual_flag();
            }
            let merged = merge_hdr_weighted(frames, exposures, &shifts)
                .map_err(|error| format!("merge unsupported: {error}"))?;
            Ok(MergePass {
                merged,
                transforms,
                residual_px,
                residual_warning,
            })
        }
        MergeMode::Panorama => {
            let mut matrices = Vec::with_capacity(frames.len());
            let mut transforms = Vec::new();
            let mut residual_px: f64 = 0.0;
            let mut residual_warning = false;
            matrices.push([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
            for (index, frame) in frames.iter().enumerate().skip(1) {
                let transform = estimate_pano_transform(&frames[0], frame, max_shift_px)
                    .map_err(|error| format!("merge unsupported: {error}"))?;
                matrices.push(transform.matrix_3x3);
                transforms.push(MergeTransform {
                    source_index: index,
                    matrix_3x3: transform.matrix_3x3,
                });
                residual_px = residual_px.max(transform.residual_px);
                residual_warning |= transform.status.residual_flag();
            }
            let merged = blend_panorama_transformed(frames, &matrices, blend_width_px)
                .map_err(|error| format!("merge unsupported: {error}"))?;
            Ok(MergePass {
                merged,
                transforms,
                residual_px,
                residual_warning,
            })
        }
    }
}

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

/// Inputs of one merge-recipe build (grouped so the builder takes a single
/// argument — no `too_many_arguments` exemption).
struct RecipeInputs<'a> {
    mode: MergeMode,
    sources: &'a [DecodedSource],
    source_paths: &'a [String],
    exposures: &'a [MergeExposure],
    transforms: &'a [MergeTransform],
    residual_px: f64,
    dng_file_name: &'a str,
    blend_width_px: u32,
}

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

fn build_merge_document(
    dng_file_name: &str,
    dng_bytes: &[u8],
    width: u32,
    height: u32,
    recipe: &MergeRecipe,
) -> Result<SidecarDocument, String> {
    let checksum = blake3_checksum(dng_bytes);
    let source = SourceIdentity {
        relative_name: dng_file_name.to_string(),
        content_hash: checksum.clone(),
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
            extras: Default::default(),
        },
        extras: Default::default(),
    };
    let mut document = SidecarDocument::new(source, MERGE_PIPELINE_VERSION);
    let recipe_value =
        serde_json::to_value(recipe).map_err(|error| format!("merge failed: {error}"))?;
    let artifact = ArtifactReference {
        relative_path: dng_file_name.to_string(),
        format: MERGE_DNG_FORMAT.into(),
        checksum,
        width,
        height,
        channels: MERGE_DNG_CHANNELS.into(),
        data_version: MERGE_DNG_DATA_VERSION.into(),
        extras: Default::default(),
    };
    let artifact_value =
        serde_json::to_value(&artifact).map_err(|error| format!("merge failed: {error}"))?;
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

/// Inputs of the existing-bundle gate (grouped so the function takes a single
/// argument — no `too_many_arguments` exemption).
struct BundleGate<'a> {
    mode: MergeMode,
    force: bool,
    sidecar_path: &'a Path,
    dng_path: &'a Path,
    digest: &'a str,
    residual_px: f64,
    residual_warning: bool,
}

/// Loud stale/missing/envelope gate over an existing bundle (mirrors the CLI
/// `check_existing_bundle`). `Ok(Some(outcome))` = the bundle is already
/// current; `Ok(None)` = the merge may proceed.
fn check_existing_bundle(gate: BundleGate<'_>) -> Result<Option<MergeOutcome>, String> {
    let BundleGate {
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
            return Err(format!(
                "merge missing: merge DNG `{}` exists without its sidecar `{}` \
                 (use force to regenerate the bundle explicitly)",
                dng_path.display(),
                sidecar_path.display()
            ));
        }
        return Ok(None);
    }
    if force {
        info!("merge: force regenerates the existing bundle");
        return Ok(None);
    }
    let document = load_sidecar(sidecar_path).map_err(|error| {
        format!(
            "merge missing: existing merge sidecar `{}` is unreadable ({error}); \
             use force to regenerate the bundle explicitly",
            sidecar_path.display()
        )
    })?;
    match MergeDocumentKind::classify(&document) {
        MergeDocumentKind::Merge => {}
        MergeDocumentKind::Standard => {
            return Err(format!(
                "merge stale: existing sidecar `{}` is a standard sidecar (no `\"type\": \"{MERGE_ENVELOPE_TYPE}\"` envelope); refusing to overwrite it",
                sidecar_path.display()
            ))
        }
        MergeDocumentKind::Unknown(kind) => {
            return Err(format!(
                "merge stale: existing sidecar `{}` has unknown document type `{kind}`; refusing to overwrite it",
                sidecar_path.display()
            ))
        }
    }
    let stored_recipe = document
        .extras
        .get(MERGE_RECIPE_KEY)
        .ok_or_else(|| {
            format!(
                "merge stale: existing sidecar `{}` carries no merge recipe",
                sidecar_path.display()
            )
        })
        .and_then(|value| {
            MergeRecipe::from_json(&value.to_string()).map_err(|error| {
                format!(
                    "merge stale: stored merge recipe in `{}` is invalid ({error})",
                    sidecar_path.display()
                )
            })
        })?;
    if stored_recipe.digest() != digest {
        return Err(format!(
            "merge stale: sources, decode context or alignment changed since `{}` was written (digest mismatch)",
            sidecar_path.display()
        ));
    }
    let stored_artifact: ArtifactReference = document
        .extras
        .get(MERGE_ARTIFACT_KEY)
        .ok_or_else(|| {
            format!(
                "merge stale: existing sidecar `{}` carries no merge artifact reference",
                sidecar_path.display()
            )
        })
        .and_then(|value| {
            serde_json::from_value(value.clone()).map_err(|error| {
                format!(
                    "merge stale: stored merge artifact in `{}` is invalid ({error})",
                    sidecar_path.display()
                )
            })
        })?;
    let dng_bytes = std::fs::read(dng_path).map_err(|_| {
        format!(
            "merge missing: merge DNG `{}` is gone (its sidecar remains)",
            dng_path.display()
        )
    })?;
    if blake3_checksum(&dng_bytes) != stored_artifact.checksum {
        return Err(format!(
            "merge stale: merge DNG `{}` changed on disk (checksum mismatch)",
            dng_path.display()
        ));
    }
    info!("merge: bundle already current (digest match, nothing rewritten)");
    Ok(Some(MergeOutcome {
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

/// Runs one merge synchronously (the job thread calls this; headless tests call
/// it directly for deterministic results).
///
/// `exposures` mirrors the CLI `--exposure-times/--isos/--f-numbers` triple;
/// `None` resolves per-source EXIF (HDR: loud `unsupported` without it).
pub fn run_merge_sync(
    mode: MergeMode,
    inputs: &[PathBuf],
    exposures: Option<&[MergeExposure]>,
    force: bool,
    max_shift_px: i32,
    blend_width_px: u32,
) -> Result<MergeOutcome, String> {
    let command = match mode {
        MergeMode::Hdr => "merge-hdr",
        MergeMode::Panorama => "merge-pano",
    };
    if inputs.len() < MIN_MERGE_SOURCES || inputs.len() > MAX_MERGE_SOURCES {
        return Err(format!(
            "merge needs {MIN_MERGE_SOURCES}..={MAX_MERGE_SOURCES} sources, got {}",
            inputs.len()
        ));
    }
    if max_shift_px < 0 {
        return Err(format!("invalid max_shift_px {max_shift_px}: must be >= 0"));
    }
    if let Some(values) = exposures {
        if values.len() != inputs.len() {
            return Err(format!(
                "invalid exposure list: expected {} values, got {}",
                inputs.len(),
                values.len()
            ));
        }
    }
    info!("{command}: merging {} source(s)", inputs.len());
    let sources = decode_merge_sources(inputs)?;
    let exposures: Vec<MergeExposure> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| resolve_exposure(mode, index, exposures, source))
        .collect::<Result<_, _>>()?;
    let frames: Vec<LinearImage> = sources.iter().map(|s| s.frame.clone()).collect();
    let MergePass {
        merged,
        transforms,
        residual_px,
        residual_warning,
    } = align_and_merge(mode, &frames, &exposures, max_shift_px, blend_width_px)?;
    if residual_warning {
        warn!(
            "{command}: aligned with residual {residual_px:.2}px (above the documented threshold)"
        );
    }
    let reference = &inputs[0];
    let reference_name = reference
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            format!(
                "merge unsupported: reference `{}` has no file name",
                reference.display()
            )
        })?;
    let dng_file_name = merge_dng_filename(reference_name, mode)
        .map_err(|error| format!("merge unsupported: {error}"))?;
    let bundle_dir = reference.parent().unwrap_or_else(|| Path::new("."));
    let dng_path = bundle_dir.join(&dng_file_name);
    let sidecar_path = sidecar_path_for(&dng_path);
    let source_paths = bundle_relative_paths(bundle_dir, inputs)?;
    let recipe = build_recipe(RecipeInputs {
        mode,
        sources: &sources,
        source_paths: &source_paths,
        exposures: &exposures,
        transforms: &transforms,
        residual_px,
        dng_file_name: &dng_file_name,
        blend_width_px,
    });
    recipe
        .validate()
        .map_err(|error| format!("merge failed: {error}"))?;
    let digest = recipe.digest();

    // Existing-bundle gate BEFORE encoding: an already-current bundle is
    // reported without a wasted DNG encode (mirrors the CLI order).
    if let Some(outcome) = check_existing_bundle(BundleGate {
        mode,
        force,
        sidecar_path: &sidecar_path,
        dng_path: &dng_path,
        digest: &digest,
        residual_px,
        residual_warning,
    })? {
        return Ok(outcome);
    }
    let exif = dng_exif_for(sources[0].raw_metadata.as_ref());
    let dng_bytes = encode_linear_dng(&merged, mode, &exif)
        .map_err(|error| format!("merge unsupported: {error}"))?;
    let checksum = blake3_checksum(&dng_bytes);
    let document = build_merge_document(
        &dng_file_name,
        &dng_bytes,
        merged.width(),
        merged.height(),
        &recipe,
    )?;
    // Atomic write, CLI order: stage the DNG, commit the sidecar, publish the
    // DNG — a failed sidecar save leaves no artifact behind.
    let temp_path =
        dng_path.with_file_name(format!(".{}.{}.tmp", dng_file_name, std::process::id()));
    std::fs::write(&temp_path, &dng_bytes).map_err(|error| {
        format!(
            "merge failed: cannot stage `{}`: {error}",
            temp_path.display()
        )
    })?;
    if let Err(error) = save_sidecar(&sidecar_path, &document) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("merge failed: sidecar write failed: {error}"));
    }
    std::fs::rename(&temp_path, &dng_path).map_err(|error| {
        let _ = std::fs::remove_file(&temp_path);
        format!(
            "merge failed: cannot publish `{}`: {error}",
            dng_path.display()
        )
    })?;
    info!("{command}: wrote `{}` ({checksum})", dng_path.display());
    info!("{command}: wrote `{}`", sidecar_path.display());
    Ok(MergeOutcome {
        mode,
        dng_path,
        sidecar_path,
        digest,
        checksum,
        cached: false,
        residual_px,
        residual_warning,
    })
}

/// Reads the visible status of a merge bundle at `dng_path` (decision
/// §Persistenz). Pure I/O + validation; never rewrites anything.
pub fn merge_bundle_status(dng_path: &Path) -> (MergeArtifactStatus, String) {
    let sidecar_path = sidecar_path_for(dng_path);
    if !sidecar_path.exists() {
        return if dng_path.exists() {
            (
                MergeArtifactStatus::Missing,
                format!(
                    "merge DNG `{}` exists without its sidecar",
                    dng_path.display()
                ),
            )
        } else {
            (MergeArtifactStatus::NoBundle, String::new())
        };
    }
    let document = match load_sidecar(&sidecar_path) {
        Ok(document) => document,
        Err(error) => {
            return (
                MergeArtifactStatus::Unsupported,
                format!("merge sidecar unreadable: {error}"),
            )
        }
    };
    match MergeDocumentKind::classify(&document) {
        MergeDocumentKind::Merge => {}
        MergeDocumentKind::Standard => {
            return (
                MergeArtifactStatus::Unsupported,
                "sidecar is a standard sidecar (no merge envelope)".into(),
            )
        }
        MergeDocumentKind::Unknown(kind) => {
            return (
                MergeArtifactStatus::Unsupported,
                format!("unknown document type `{kind}`"),
            )
        }
    }
    let recipe = match document
        .extras
        .get(MERGE_RECIPE_KEY)
        .and_then(|value| MergeRecipe::from_json(&value.to_string()).ok())
    {
        Some(recipe) => recipe,
        None => {
            return (
                MergeArtifactStatus::Unsupported,
                "stored merge recipe missing or invalid".into(),
            )
        }
    };
    let artifact: Option<ArtifactReference> = document
        .extras
        .get(MERGE_ARTIFACT_KEY)
        .and_then(|value| serde_json::from_value(value.clone()).ok());
    let Some(artifact) = artifact else {
        return (
            MergeArtifactStatus::Unsupported,
            "stored merge artifact reference missing or invalid".into(),
        );
    };
    let Ok(dng_bytes) = std::fs::read(dng_path) else {
        return (MergeArtifactStatus::Missing, "merge DNG is gone".into());
    };
    if blake3_checksum(&dng_bytes) != artifact.checksum {
        return (
            MergeArtifactStatus::Stale,
            "merge DNG checksum changed on disk".into(),
        );
    }
    // Source hashes are relative to the bundle directory.
    let bundle_dir = dng_path.parent().unwrap_or_else(|| Path::new("."));
    for source in &recipe.sources {
        let source_path = bundle_dir.join(&source.path);
        let Ok(bytes) = std::fs::read(&source_path) else {
            return (
                MergeArtifactStatus::Missing,
                format!("source `{}` is missing", source.path),
            );
        };
        if blake3_checksum(&bytes) != source.content_hash {
            return (
                MergeArtifactStatus::Stale,
                format!("source `{}` changed (hash mismatch)", source.path),
            );
        }
    }
    (MergeArtifactStatus::Ok, String::new())
}

impl crate::LuminaApp {
    /// Selected merge sources (filmstrip selection, else the loaded image), in
    /// the deterministic sorted path order (reference = first).
    pub fn merge_sources(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self.filmstrip_selection.iter().map(PathBuf::from).collect();
        if paths.is_empty() && !self.path.is_empty() {
            paths.push(PathBuf::from(&self.path));
        }
        paths.sort();
        paths
    }

    /// Whether a merge job is currently running.
    pub fn merge_running(&self) -> bool {
        self.merge_job.is_some()
    }

    /// Last merge outcome text (status line).
    pub fn merge_status_text(&self) -> &str {
        &self.merge_status
    }

    /// Starts a merge on the current selection as a background job (job
    /// control: the UI stays responsive; `poll_merge_job` reports the result).
    pub fn start_merge(&mut self, mode: MergeMode) -> Result<(), GuiError> {
        if self.merge_job.is_some() {
            return Err(GuiError::Io(Str::MergeAlreadyRunning.t().to_string()));
        }
        let inputs = self.merge_sources();
        if inputs.len() < MIN_MERGE_SOURCES {
            return Err(GuiError::Io(
                Str::MergeNeedsSelection.format_arg(&MIN_MERGE_SOURCES.to_string()),
            ));
        }
        let command = match mode {
            MergeMode::Hdr => "merge-hdr",
            MergeMode::Panorama => "merge-pano",
        };
        info!("{command}: GUI job started for {} source(s)", inputs.len());
        let (tx, rx) = mpsc::channel();
        let job_inputs = inputs.clone();
        std::thread::spawn(move || {
            let result = run_merge_sync(mode, &job_inputs, None, false, 16, 64);
            let _ = tx.send(result);
        });
        self.merge_job = Some(MergeJob { mode, rx });
        self.merge_status = Str::MergeRunningPattern.format_arg(command).to_string();
        self.status = self.merge_status.clone();
        Ok(())
    }

    /// Polls the running merge job (called once per frame). Completion updates
    /// the status line, raises a loud error dialog on failure and refreshes the
    /// directory so the new DNG appears in the Library.
    pub fn poll_merge_job(&mut self, now: f64) {
        let Some(job) = &self.merge_job else {
            return;
        };
        let mode = job.mode;
        match job.rx.try_recv() {
            Ok(Ok(outcome)) => {
                self.merge_job = None;
                let message = if outcome.cached {
                    Str::MergeCurrentPattern.format_arg(&outcome.dng_path.display().to_string())
                } else {
                    Str::MergeDonePattern.format_arg(&outcome.dng_path.display().to_string())
                };
                self.merge_status = message.clone();
                self.status = message.clone();
                self.show_toast(message, now);
                // The new DNG is a supported source: a targeted single-file
                // refresh lists it without a full rescan.
                self.refresh_entry(&outcome.dng_path);
                info!(
                    "merge job finished: mode={mode:?} dng={} cached={} digest={}",
                    outcome.dng_path.display(),
                    outcome.cached,
                    outcome.digest
                );
            }
            Ok(Err(message)) => {
                self.merge_job = None;
                self.merge_status = message.clone();
                error!("merge job failed: {message}");
                self.show_error(message);
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.merge_job = None;
                let message = Str::MergeFailed.t().to_string();
                self.merge_status = message.clone();
                error!("merge job thread disconnected before reporting a result");
                self.show_error(message);
            }
        }
    }

    /// Visible merge panel (Library metadata column): mode actions, job state,
    /// selection count and the merge-bundle status of the loaded image.
    pub(crate) fn draw_merge_section(&mut self, ui: &mut crate::egui::Ui) {
        // Collapsed by default like every other Library sub-section (the
        // panel's 320 px default width only carries the narrow header).
        ui.collapsing(Str::MergeSection.t(), |ui| {
            let selected = self.merge_sources().len();
            ui.add(
                crate::egui::Label::new(
                    Str::MergeSelectionPattern.format_arg(&selected.to_string()),
                )
                .wrap(),
            );
            let running = self.merge_running();
            // Stacked (not a wide row) so the panel width stays stable.
            ui.add_enabled_ui(!running, |ui| {
                if ui.button(Str::MergeHdr.t()).clicked() {
                    if let Err(error) = self.start_merge(MergeMode::Hdr) {
                        self.show_error(error);
                    }
                }
                if ui.button(Str::MergePano.t()).clicked() {
                    if let Err(error) = self.start_merge(MergeMode::Panorama) {
                        self.show_error(error);
                    }
                }
            });
            if !self.merge_status.is_empty() {
                ui.add(
                    crate::egui::Label::new(
                        Str::MergeStatusPattern.format_arg(&self.merge_status.clone()),
                    )
                    .wrap(),
                );
            }
            // Bundle status of the loaded image (if it is a merge DNG).
            if !self.path.is_empty() {
                let (status, reason) = merge_bundle_status(Path::new(&self.path));
                if status != MergeArtifactStatus::NoBundle {
                    ui.add(
                        crate::egui::Label::new(Str::MergeBundlePattern.format_arg(status.text()))
                            .wrap(),
                    );
                    if !reason.is_empty() {
                        ui.add(crate::egui::Label::new(reason).wrap());
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LuminaApp;
    use lumina_core::{ImageFileFormat, ImageFrame};

    fn new_app() -> LuminaApp {
        LuminaApp::new(crate::egui::Context::default())
    }

    /// Synthetic 64×48 RGB frame encoded as PNG (brightness `level`). The
    /// gradient gives the alignment a non-flat signal.
    fn write_png(path: &Path, level: u8) {
        let pixels: Vec<u8> = (0..64 * 48)
            .flat_map(|i| {
                let x = (i % 64) as u8;
                let value = level.saturating_add(x % 16);
                [value, value, value, 255]
            })
            .collect();
        let png = ImageFrame::new(64, 48, pixels)
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap();
        std::fs::write(path, png).unwrap();
    }

    fn exposure(t: f64) -> Vec<MergeExposure> {
        vec![
            MergeExposure {
                exposure_time_s: t,
                iso: 100,
                f_number: 8.0,
            },
            MergeExposure {
                exposure_time_s: t * 4.0,
                iso: 100,
                f_number: 8.0,
            },
        ]
    }

    /// Golden gate (HDR): hash anchor + sidecar envelope + deterministic
    /// re-run + LibRaw re-import.
    #[test]
    fn hdr_merge_writes_dng_bundle_and_reimports() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("shot_a.png");
        let b = dir.path().join("shot_b.png");
        write_png(&a, 40);
        write_png(&b, 160);
        let exposures = exposure(0.004);
        let inputs = vec![a.clone(), b.clone()];
        let outcome =
            run_merge_sync(MergeMode::Hdr, &inputs, Some(&exposures), false, 16, 64).unwrap();
        assert!(!outcome.cached);
        assert_eq!(outcome.dng_path, dir.path().join("shot_a-HDR.dng"));
        assert!(outcome.dng_path.is_file());
        assert!(outcome.sidecar_path.is_file());
        let dng_bytes = std::fs::read(&outcome.dng_path).unwrap();
        assert_eq!(blake3_checksum(&dng_bytes), outcome.checksum, "hash anchor");

        let document = load_sidecar(&outcome.sidecar_path).unwrap();
        assert_eq!(
            MergeDocumentKind::classify(&document),
            MergeDocumentKind::Merge
        );
        let artifact: ArtifactReference =
            serde_json::from_value(document.extras[MERGE_ARTIFACT_KEY].clone()).unwrap();
        assert_eq!(
            artifact.checksum, outcome.checksum,
            "sidecar artifact checksum"
        );
        assert_eq!((artifact.width, artifact.height), (64, 48));
        assert_eq!(artifact.channels, "rgb16");
        assert_eq!(
            (document.virtual_copies[0].recipe.denoise_ai.as_ref(),),
            (None,),
            "merge DNG carries a fresh standard recipe"
        );

        // Re-import anchor through the pinned decoder.
        let (_, frame, _) = crate::decode_selection_frame(&outcome.dng_path).unwrap();
        assert_eq!((frame.width, frame.height), (64, 48));

        // Deterministic re-run with `force` (same machine → byte-identical).
        let again =
            run_merge_sync(MergeMode::Hdr, &inputs, Some(&exposures), true, 16, 64).unwrap();
        let again_bytes = std::fs::read(&again.dng_path).unwrap();
        assert_eq!(
            blake3_checksum(&again_bytes),
            outcome.checksum,
            "same inputs must reproduce the DNG (tolerance {})",
            lumina_merge::FLOAT_TOLERANCE_DOC
        );

        // Without `force` an already-current bundle is reported cached, not
        // rewritten.
        let cached =
            run_merge_sync(MergeMode::Hdr, &inputs, Some(&exposures), false, 16, 64).unwrap();
        assert!(cached.cached);
    }

    /// Golden gate (Panorama): overlap geometry, cylindrical projection and the
    /// deterministic digest.
    #[test]
    fn panorama_merge_writes_dng_and_persists_projection() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("pano_a.png");
        let b = dir.path().join("pano_b.png");
        write_png(&a, 90);
        write_png(&b, 90);
        let inputs = vec![a.clone(), b.clone()];
        // Identical frames overlap completely: a valid panorama with no shift.
        let outcome = run_merge_sync(MergeMode::Panorama, &inputs, None, false, 16, 64).unwrap();
        assert!(outcome.dng_path.is_file());
        let document = load_sidecar(&outcome.sidecar_path).unwrap();
        let recipe =
            MergeRecipe::from_json(&document.extras[MERGE_RECIPE_KEY].to_string()).unwrap();
        assert_eq!(recipe.mode, MergeMode::Panorama);
        assert_eq!(recipe.alignment.projection, MergeProjection::Cylindrical);
        assert_eq!(
            recipe.alignment.method,
            MergeAlignmentMethod::PanoCylindricalHomography
        );
        assert_eq!(recipe.sources.len(), 2);
        // Panorama provenance defaults are stored for EXIF-less sources.
        assert!(
            (recipe.sources[0].exposure.exposure_time_s - PANO_DEFAULT_EXPOSURE_S).abs() < 1e-9
        );
        assert!((recipe.sources[0].exposure.f_number - PANO_DEFAULT_F_NUMBER).abs() < 1e-9);
    }

    /// HDR without EXIF and without explicit exposures is `unsupported`
    /// (never guessed from pixels).
    #[test]
    fn hdr_without_exposure_is_unsupported_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("noexif_a.png");
        let b = dir.path().join("noexif_b.png");
        write_png(&a, 40);
        write_png(&b, 160);
        let error = run_merge_sync(MergeMode::Hdr, &[a.clone(), b.clone()], None, false, 16, 64)
            .unwrap_err();
        assert!(error.contains("unsupported"), "{error}");
        assert!(!dir.path().join("noexif_a-HDR.dng").exists());
        assert!(!sidecar_path_for(&dir.path().join("noexif_a-HDR.dng")).exists());
    }

    /// Envelope conflict: a standard sidecar at the target is refused loudly
    /// and left byte-identical.
    #[test]
    fn standard_sidecar_envelope_conflict_is_loud_and_non_destructive() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("conflict_a.png");
        let b = dir.path().join("conflict_b.png");
        write_png(&a, 40);
        write_png(&b, 160);
        let inputs = vec![a.clone(), b.clone()];
        let exposures = exposure(0.004);
        // First merge creates the bundle, then we deface the envelope.
        run_merge_sync(MergeMode::Hdr, &inputs, Some(&exposures), false, 16, 64).unwrap();
        let sidecar = sidecar_path_for(&dir.path().join("conflict_a-HDR.dng"));
        let mut document = load_sidecar(&sidecar).unwrap();
        document.extras.remove("type");
        lumina_sidecar::save_sidecar(&sidecar, &document).unwrap();
        let before = std::fs::read(&sidecar).unwrap();
        let error =
            run_merge_sync(MergeMode::Hdr, &inputs, Some(&exposures), false, 16, 64).unwrap_err();
        assert!(
            error.contains("merge stale") || error.contains("standard sidecar"),
            "{error}"
        );
        assert_eq!(
            std::fs::read(&sidecar).unwrap(),
            before,
            "no silent overwrite"
        );
        // The bundle status reports the conflict visibly.
        let (status, reason) = merge_bundle_status(&dir.path().join("conflict_a-HDR.dng"));
        assert_eq!(status, MergeArtifactStatus::Unsupported);
        assert!(!reason.is_empty());
    }

    /// Status matrix: ok / stale / missing.
    #[test]
    fn bundle_status_reports_ok_stale_missing() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("st_a.png");
        let b = dir.path().join("st_b.png");
        write_png(&a, 40);
        write_png(&b, 160);
        let inputs = vec![a.clone(), b.clone()];
        let exposures = exposure(0.004);
        let outcome =
            run_merge_sync(MergeMode::Hdr, &inputs, Some(&exposures), false, 16, 64).unwrap();
        assert_eq!(
            merge_bundle_status(&outcome.dng_path).0,
            MergeArtifactStatus::Ok
        );

        // Change a source: stale.
        write_png(&b, 20);
        assert_eq!(
            merge_bundle_status(&outcome.dng_path).0,
            MergeArtifactStatus::Stale
        );
        // Remove the DNG: missing.
        std::fs::remove_file(&outcome.dng_path).unwrap();
        assert_eq!(
            merge_bundle_status(&outcome.dng_path).0,
            MergeArtifactStatus::Missing
        );
    }

    /// DoD §3 (class completeness): every [`MergeArtifactStatus`] variant has a
    /// distinct, non-empty visible text, and `NoBundle` is the quiet state
    /// (empty reason) at a target that carries nothing.
    #[test]
    fn merge_artifact_status_covers_every_variant() {
        let variants = [
            MergeArtifactStatus::NoBundle,
            MergeArtifactStatus::Ok,
            MergeArtifactStatus::Stale,
            MergeArtifactStatus::Missing,
            MergeArtifactStatus::Unsupported,
        ];
        let mut texts: Vec<&str> = variants.iter().map(|status| status.text()).collect();
        assert!(texts.iter().all(|text| !text.is_empty()), "{texts:?}");
        texts.sort_unstable();
        texts.dedup();
        assert_eq!(
            texts.len(),
            variants.len(),
            "status texts must be distinct: {texts:?}"
        );
        assert_eq!(
            MergeArtifactStatus::NoBundle.text(),
            Str::MergeStatusNone.t()
        );
        let dir = tempfile::tempdir().unwrap();
        let (status, reason) = merge_bundle_status(&dir.path().join("absent.dng"));
        assert_eq!(status, MergeArtifactStatus::NoBundle);
        assert!(reason.is_empty(), "`NoBundle` must stay silent: {reason:?}");
    }

    /// Job control: `start_merge` runs in the background and completion is
    /// observable through `poll_merge_job` (no frozen UI, no silent failure).
    #[test]
    fn merge_job_runs_in_background_and_reports_via_poll() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("job_a.png");
        let b = dir.path().join("job_b.png");
        write_png(&a, 90);
        write_png(&b, 90);
        let mut app = new_app();
        app.set_directory(dir.path().display().to_string());
        app.list_directory_flat();
        app.open_file(a.display().to_string());
        // The filmstrip selection is the RAW-only UI selection; this headless
        // test injects exactly the state the UI produces for a selection of
        // two sources (the merge entry point itself is source-format agnostic).
        app.filmstrip_selection =
            std::collections::BTreeSet::from([a.display().to_string(), b.display().to_string()]);
        assert_eq!(app.merge_sources().len(), 2);
        app.start_merge(MergeMode::Panorama).unwrap();
        assert!(app.merge_running());
        // A second start while running is refused loudly.
        assert!(app.start_merge(MergeMode::Panorama).is_err());
        let mut outcome = None;
        for _ in 0..2000 {
            app.poll_merge_job(0.0);
            if !app.merge_running() {
                outcome = Some(app.merge_status_text().to_string());
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let message = outcome.expect("merge job must finish");
        assert!(message.contains("job_a-Pano.dng"), "{message}");
        assert!(dir.path().join("job_a-Pano.dng").is_file());
        // The new DNG is listed after completion (targeted entry refresh).
        assert!(app
            .entries()
            .iter()
            .any(|entry| entry.name == "job_a-Pano.dng"));
    }

    /// Too few sources is a loud refusal (no partial panorama/HDR).
    #[test]
    fn merge_needs_at_least_two_sources() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("solo.png");
        write_png(&a, 90);
        let mut app = new_app();
        app.open_file(a.display().to_string());
        let error = app.start_merge(MergeMode::Hdr).unwrap_err();
        assert!(error.to_string().contains("2"), "{error}");
    }

    /// Bundled path safety: sources outside the bundle directory are refused.
    #[test]
    fn outside_bundle_sources_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside/ref.png");
        std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
        write_png(&outside, 90);
        let b = dir.path().join("inside.png");
        write_png(&b, 90);
        let error =
            run_merge_sync(MergeMode::Panorama, &[outside, b], None, false, 16, 64).unwrap_err();
        assert!(error.contains("unsupported"), "{error}");
    }

    /// The RGBA8→linear conversion is pure and loud on a malformed buffer.
    #[test]
    fn linear_conversion_is_exact_and_loud() {
        let frame = ImageFrame::new(2, 1, vec![255, 0, 0, 128, 0, 128, 0, 255]).unwrap();
        let linear = linear_from_frame(&frame).unwrap();
        assert_eq!(linear.width(), 2);
        assert_eq!(&linear.pixels()[0..3], &[1.0, 0.0, 0.0]);
        let mut broken = frame.clone();
        broken.pixels.pop();
        assert!(linear_from_frame(&broken).is_none());
    }
}
