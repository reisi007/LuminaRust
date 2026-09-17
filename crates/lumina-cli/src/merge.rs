//! LRPAR-G13-MERGE-15 / MERGE-CLI-1 + MERGE-IMPL-15 (F6): `merge-hdr` /
//! `merge-pano` commands.
//!
//! SOLL: `feature/decisions/LRPAR-G13-MERGE-15.md` (§Pipeline-Einordnung,
//! §Persistenz, §Abnahme CLI) and `feature/platform/cli-gui-wasm.md`
//! (§ „HDR-/Panorama-Merge (G-13, Release 1.5: CLI)"). This module is the
//! **CLI wiring only**: it decodes the sources (via `lumina-raw`/core, like
//! every other CLI command), resolves the `--exposure-times/--isos/
//! --f-numbers` policy and hands both to the shared orchestration
//! [`lumina_merge::bundle::run_merge`], which the GUI actions call with the
//! same step sequence (F6 dedup). There is no second image-processing
//! implementation and neither the sidecar schema nor the merge math changes.
//!
//! Failure policy (Agents.md, no silent fallback): every deviation is loud
//! on stderr with exit code 1 — `merge missing:` (a source, the DNG or the
//! sidecar is gone), `merge stale:` (an existing bundle no longer matches
//! the sources, only `--force` re-merges), `merge unsupported:` (decode,
//! geometry, exposure, dimension or writer limit). Success (including
//! „bundle already current", which rewrites nothing) exits 0.

use clap::Args;
use log::info;
use lumina_merge::bundle::{
    linear_from_rgba, merge_checksum, merge_command_name, run_merge as run_merge_bundle, MergeExif,
    MergeRunError, MergeRunOptions, MergeSourceFrame, PANO_DEFAULT_EXPOSURE_S,
    PANO_DEFAULT_F_NUMBER, PANO_DEFAULT_ISO,
};
use lumina_sidecar::{
    MergeDecodeContext, MergeExposure, MergeMode, MAX_MERGE_EXPOSURE_TIME_S, MAX_MERGE_F_NUMBER,
    MAX_MERGE_ISO,
};
use std::fs;
use std::path::{Path, PathBuf};

use super::{decode_input, emit, CliError};

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

/// `merge-hdr`: exposure-bracketed frames → one linear DNG.
pub fn merge_hdr(args: MergeArgs) -> Result<(), CliError> {
    run_merge(MergeMode::Hdr, &args)
}

/// `merge-pano`: overlapping frames → one linear DNG.
pub fn merge_pano(args: MergeArgs) -> Result<(), CliError> {
    run_merge(MergeMode::Panorama, &args)
}

fn run_merge(mode: MergeMode, args: &MergeArgs) -> Result<(), CliError> {
    let command = merge_command_name(mode);
    check_explicit_list_lengths(args)?;
    let options = MergeRunOptions {
        output: args.output.clone(),
        max_shift_px: args.max_shift_px,
        blend_width_px: args.blend_width_px,
        force: args.force,
        dng_decode_version: lumina_raw::libraw_decode_version(),
    };
    let resolve = |mode: MergeMode, index: usize, source: &MergeSourceFrame| {
        resolve_exposure(mode, index, args, source)
    };
    let outcome = run_merge_bundle(mode, &args.input, &options, decode_source, resolve)
        .map_err(|error| CliError::Message(error.to_string()))?;

    if outcome.residual_warning {
        eprintln!(
            "warning: {command} aligned with residual {:.2}px \
             (above the documented shift threshold); result kept, nothing cropped silently",
            outcome.residual_px
        );
    } else {
        info!("{command}: aligned (residual {:.2}px)", outcome.residual_px);
    }

    let message = if outcome.cached {
        format!("merge already current: {}", outcome.dng_path.display())
    } else {
        format!("merged {}", outcome.dng_path.display())
    };
    emit(
        args.json,
        serde_json::json!({
            "command": command,
            "output": outcome.dng_path,
            "sidecar": outcome.sidecar_path,
            "digest": outcome.digest,
            "checksum": outcome.checksum,
            "cached": outcome.cached,
            "status": "ok",
        }),
        &message,
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

/// Reads and decodes one source (CLI decode adapter for the shared
/// orchestration). A missing/unreadable file is `missing`, an undecodable
/// file or a non-RGBA frame is `unsupported` — never a silent skip.
fn decode_source(input: &Path) -> Result<MergeSourceFrame, MergeRunError> {
    let bytes = fs::read(input).map_err(|error| {
        MergeRunError::Missing(format!("cannot read source `{}`: {error}", input.display()))
    })?;
    let content_hash = merge_checksum(&bytes);
    let (frame, raw) = decode_input(input, &bytes).map_err(|error| {
        MergeRunError::Unsupported(format!(
            "cannot decode source `{}`: {error}",
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
        MergeRunError::Unsupported(format!(
            "source `{}` has {} bytes for a {}x{} RGBA8 frame",
            input.display(),
            frame.pixels.len(),
            frame.width,
            frame.height
        ))
    })?;
    Ok(MergeSourceFrame {
        path: input.to_path_buf(),
        content_hash,
        frame: linear,
        decode_context,
        exif: merge_exif_from_raw(raw.as_ref()),
    })
}

/// Maps the RAW metadata onto the neutral [`MergeExif`] subset (missing
/// fields stay `None`; validity is enforced by the accessors).
fn merge_exif_from_raw(raw: Option<&lumina_raw::RawMetadata>) -> MergeExif {
    raw.map_or_else(MergeExif::default, |meta| MergeExif {
        camera_make: meta.camera_make.clone(),
        camera_model: meta.camera_model.clone(),
        lens: meta.lens.clone(),
        exposure_time_s: meta.shutter,
        f_number: meta.aperture,
        iso: meta.iso,
        timestamp: meta.timestamp,
    })
}

/// Per-source exposure for the recipe and (HDR) the pixel merge.
///
/// HDR: explicit value wins; otherwise the RAW EXIF exposure; otherwise
/// `unsupported` — exposure is never guessed from pixels. Panorama: the
/// pixels ignore exposure, so EXIF (or the documented neutral default)
/// is stored as provenance only.
fn resolve_exposure(
    mode: MergeMode,
    index: usize,
    args: &MergeArgs,
    source: &MergeSourceFrame,
) -> Result<MergeExposure, MergeRunError> {
    match mode {
        MergeMode::Hdr => resolve_hdr_exposure(index, args, source),
        MergeMode::Panorama => Ok(resolve_pano_exposure(index, args, source)),
    }
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
    source: &MergeSourceFrame,
) -> Result<MergeExposure, MergeRunError> {
    let path = args.input[index].display().to_string();
    let exposure_time_s = match explicit_at(&args.exposure_times, index) {
        Some(value) => {
            if !value.is_finite() || value <= 0.0 || value > MAX_MERGE_EXPOSURE_TIME_S {
                return Err(MergeRunError::Unsupported(format!(
                    "invalid --exposure-times[#{index}] {value} \
                     (finite within (0, {MAX_MERGE_EXPOSURE_TIME_S}])"
                )));
            }
            value
        }
        None => source.exif.valid_exposure_time_s().ok_or_else(|| {
            MergeRunError::Unsupported(format!(
                "source `{path}` has no EXIF exposure \
                 (pass --exposure-times/--isos/--f-numbers, one value per --input)"
            ))
        })?,
    };
    let iso = match explicit_at(&args.isos, index) {
        Some(value) => {
            if value == 0 || value > MAX_MERGE_ISO {
                return Err(MergeRunError::Unsupported(format!(
                    "invalid --isos[#{index}] {value} (within 1..={MAX_MERGE_ISO})"
                )));
            }
            value
        }
        None => source.exif.valid_iso().ok_or_else(|| {
            MergeRunError::Unsupported(format!(
                "source `{path}` has no EXIF ISO \
                 (pass --exposure-times/--isos/--f-numbers, one value per --input)"
            ))
        })?,
    };
    let f_number = match explicit_at(&args.f_numbers, index) {
        Some(value) => {
            if !value.is_finite() || value <= 0.0 || value > MAX_MERGE_F_NUMBER {
                return Err(MergeRunError::Unsupported(format!(
                    "invalid --f-numbers[#{index}] {value} \
                     (finite within (0, {MAX_MERGE_F_NUMBER}])"
                )));
            }
            value
        }
        None => source.exif.valid_f_number().ok_or_else(|| {
            MergeRunError::Unsupported(format!(
                "source `{path}` has no EXIF f-number \
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

fn resolve_pano_exposure(
    index: usize,
    args: &MergeArgs,
    source: &MergeSourceFrame,
) -> MergeExposure {
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
    if let Some(exif) = source.exif.exposure() {
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

    fn source(path: &str) -> MergeSourceFrame {
        MergeSourceFrame {
            path: PathBuf::from(path),
            content_hash: format!("blake3:{}", "ab".repeat(32)),
            frame: lumina_merge::LinearImage::solid(4, 4, [0.3, 0.3, 0.3]),
            decode_context: MergeDecodeContext {
                decoder: "image".into(),
                decode_version: "0.1.0".into(),
                orientation: 1,
            },
            exif: MergeExif::default(),
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
    fn hdr_without_explicit_or_exif_exposure_is_unsupported() {
        let args = merge_args(&["a.png", "b.png"]);
        let error = resolve_exposure(MergeMode::Hdr, 0, &args, &source("a.png")).unwrap_err();
        assert!(matches!(error, MergeRunError::Unsupported(_)), "{error}");
        assert!(error.to_string().contains("no EXIF exposure"), "{error}");
    }

    #[test]
    fn hdr_rejects_out_of_range_explicit_values() {
        let mut args = merge_args(&["a.png", "b.png"]);
        args.exposure_times = vec![0.0, 0.02];
        let error = resolve_exposure(MergeMode::Hdr, 0, &args, &source("a.png")).unwrap_err();
        assert!(error.to_string().contains("--exposure-times"), "{error}");
        let mut args = merge_args(&["a.png", "b.png"]);
        args.isos = vec![0, 100];
        let error = resolve_exposure(MergeMode::Hdr, 0, &args, &source("a.png")).unwrap_err();
        assert!(error.to_string().contains("--isos"), "{error}");
    }

    #[test]
    fn pano_without_exif_uses_the_documented_neutral_default() {
        let args = merge_args(&["a.png", "b.png"]);
        let exposure = resolve_exposure(MergeMode::Panorama, 0, &args, &source("a.png")).unwrap();
        assert!((exposure.exposure_time_s - PANO_DEFAULT_EXPOSURE_S).abs() < 1e-9);
        assert_eq!(exposure.iso, PANO_DEFAULT_ISO);
        assert!((exposure.f_number - PANO_DEFAULT_F_NUMBER).abs() < 1e-9);
    }
}
