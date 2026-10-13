//! MCP-PARITY-C / Impl-A: the shared merge decode + exposure adapter.
//!
//! `lumina merge-hdr` / `lumina merge-pano` (CLI) and their MCP twins
//! (`lumina_merge_hdr` / `lumina_merge_pano`, later slice) must feed the shared
//! orchestration [`lumina_merge::bundle::run_merge`] from the *same* bytes with
//! the *same* decode context and the *same* exposure policy — otherwise the
//! merge digest, the DNG checksum, the sidecar and the error classes drift
//! between the transports (SOLL: `feature/quality/mcp-parity-c-soll.md` §4.3
//! „Shared Decode+Exposure-Adapter", §6 Invarianten 1–2). This module is that
//! ONE implementation: CLI and MCP both call [`decode_source`] and
//! [`resolve_exposure`] below (F6-Dedup), there is no second copy.
//!
//! The interface is deliberately **pure**: no clap, no `CliError`, no MCP
//! types. A transport builds [`MergeExplicitInputs`] (the clap-free mirror of
//! the four `MergeArgs` fields the adapter reads) and maps the two error
//! carriers itself: [`MergeRunError`] for decode/exposure failures (identical
//! class and text on every frontend) and the list-arity message as a plain
//! `String` (the CLI wraps it in `CliError::Message`, the MCP tool in
//! `InvalidParams` — the *rule* is shared, the transport error type is not).
//!
//! Decode itself is **not** re-implemented here: it goes through
//! [`crate::decode::decode_input`], the same bytes-to-frame helper the stage
//! editors use (MCP-PARITY-A). The decode dispatch (`is_raw_path`, LibRaw vs.
//! image crate), the decoder version and the orientation metadata therefore
//! cannot diverge per frontend either — the only place a second decode path
//! could still drift the digest.

use log::info;
use lumina_merge::bundle::{
    linear_from_rgba, merge_checksum, MergeExif, MergeRunError, MergeSourceFrame,
    PANO_DEFAULT_EXPOSURE_S, PANO_DEFAULT_F_NUMBER, PANO_DEFAULT_ISO,
};
use lumina_raw::RawMetadata;
use lumina_sidecar::{
    MergeDecodeContext, MergeExposure, MergeMode, MAX_MERGE_EXPOSURE_TIME_S, MAX_MERGE_F_NUMBER,
    MAX_MERGE_ISO,
};
use std::fs;
use std::path::{Path, PathBuf};

/// Decoder version recorded in the merge-source decode context of a non-RAW
/// source (MCP-PARITY-C / Impl-A follow-up).
///
/// This was `env!("CARGO_PKG_VERSION")` while the adapter lived in
/// `lumina-cli`, where it expanded to the CLI's package version. The workspace
/// does **not** inherit versions (no `[workspace.package]`; every crate pins
/// its own `version`), so the same expression here would expand to the
/// **`lumina-stages`** version — a `lumina-stages`-only bump would then
/// silently rewrite this value. The value is *persisted* inside every merge
/// source identity and is part of the digested [`MergeRecipe`], so a silent
/// rewrite drifts the merge digest of every new sidecar away from the
/// CLI/GUI anchor (and marks existing bundles stale). It is therefore a pinned
/// literal, following the precedent of `crate::decode::IMAGE_DECODER_VERSION`
/// (`decode.rs`): keep it in lockstep with `lumina-cli`'s package version.
const MERGE_IMAGE_DECODER_VERSION: &str = "0.1.0";

/// The clap-free mirror of the four CLI `MergeArgs` fields the merge adapter
/// consumes: the source paths in merge order (first = reference) plus the
/// explicit `--exposure-times`/`--isos`/`--f-numbers` lists.
///
/// The CLI fills it from its parsed flags, the MCP tool from its JSON arrays —
/// with the *same* shape, so the adapter below sees one input type on both
/// transports. `output`/`max_shift_px`/`blend_width_px`/`force`/`json` are
/// **not** part of it: they belong to the run options
/// (`lumina_merge::bundle::MergeRunOptions`) and the transport output, not to
/// the decode/exposure policy.
#[derive(Debug, Clone, Copy)]
pub struct MergeExplicitInputs<'a> {
    /// Source paths in merge order (`--input`); `input[0]` is the reference.
    pub input: &'a [PathBuf],
    /// Per-source exposure times in seconds (`--exposure-times`); empty =
    /// EXIF-or-fallback per source.
    pub exposure_times: &'a [f64],
    /// Per-source ISO values (`--isos`); empty = EXIF-or-fallback per source.
    pub isos: &'a [u32],
    /// Per-source f-numbers (`--f-numbers`); empty = EXIF-or-fallback per
    /// source.
    pub f_numbers: &'a [f64],
}

/// Reads and decodes one source (the decode adapter for the shared
/// orchestration). A missing/unreadable file is `missing`, an undecodable
/// file or a non-RGBA frame is `unsupported` — never a silent skip.
pub fn decode_source(input: &Path) -> Result<MergeSourceFrame, MergeRunError> {
    let bytes = fs::read(input).map_err(|error| {
        MergeRunError::Missing(format!("cannot read source `{}`: {error}", input.display()))
    })?;
    let content_hash = merge_checksum(&bytes);
    let (frame, raw) = crate::decode::decode_input(input, &bytes).map_err(|error| {
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
            MERGE_IMAGE_DECODER_VERSION.into()
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
pub fn merge_exif_from_raw(raw: Option<&RawMetadata>) -> MergeExif {
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

/// Usage-level validation of the explicit exposure lists: each list is
/// either empty (per-source fallback) or carries exactly one value per
/// `--input`, in order. Anything else is a loud error, never a silent
/// truncation or repetition.
///
/// Returns the ready-made message (identical on every frontend); each
/// transport wraps it in its own error type (`CliError::Message` in the CLI,
/// `McpError::InvalidParams` in the MCP tool).
pub fn check_explicit_list_lengths(inputs: &MergeExplicitInputs<'_>) -> Result<(), String> {
    let count = inputs.input.len();
    for (flag, len) in [
        ("--exposure-times", inputs.exposure_times.len()),
        ("--isos", inputs.isos.len()),
        ("--f-numbers", inputs.f_numbers.len()),
    ] {
        if len != 0 && len != count {
            return Err(format!(
                "invalid {flag}: expected 0 or {count} values (one per --input), got {len}"
            ));
        }
    }
    Ok(())
}

/// Per-source exposure for the recipe and (HDR) the pixel merge.
///
/// HDR: explicit value wins; otherwise the RAW EXIF exposure; otherwise
/// `unsupported` — exposure is never guessed from pixels. Panorama: the
/// pixels ignore exposure, so EXIF (or the documented neutral default)
/// is stored as provenance only.
pub fn resolve_exposure(
    mode: MergeMode,
    index: usize,
    inputs: &MergeExplicitInputs<'_>,
    source: &MergeSourceFrame,
) -> Result<MergeExposure, MergeRunError> {
    match mode {
        MergeMode::Hdr => resolve_hdr_exposure(index, inputs, source),
        MergeMode::Panorama => Ok(resolve_pano_exposure(index, inputs, source)),
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
    inputs: &MergeExplicitInputs<'_>,
    source: &MergeSourceFrame,
) -> Result<MergeExposure, MergeRunError> {
    let path = inputs.input[index].display().to_string();
    let exposure_time_s = match explicit_at(inputs.exposure_times, index) {
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
    let iso = match explicit_at(inputs.isos, index) {
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
    let f_number = match explicit_at(inputs.f_numbers, index) {
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
    inputs: &MergeExplicitInputs<'_>,
    source: &MergeSourceFrame,
) -> MergeExposure {
    if let (Some(t), Some(i), Some(f)) = (
        explicit_at(inputs.exposure_times, index),
        explicit_at(inputs.isos, index),
        explicit_at(inputs.f_numbers, index),
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
        inputs.input[index].display()
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

    /// Builds the adapter input from the parts a test cares about (mirrors the
    /// clap `MergeArgs` the CLI passes in production).
    fn explicit_inputs<'a>(
        input: &'a [PathBuf],
        exposure_times: &'a [f64],
        isos: &'a [u32],
        f_numbers: &'a [f64],
    ) -> MergeExplicitInputs<'a> {
        MergeExplicitInputs {
            input,
            exposure_times,
            isos,
            f_numbers,
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

    fn two_inputs() -> Vec<PathBuf> {
        vec![PathBuf::from("a.png"), PathBuf::from("b.png")]
    }

    #[test]
    fn explicit_lists_accept_empty_or_exactly_per_input() {
        let input = two_inputs();
        let mut inputs = explicit_inputs(&input, &[], &[], &[]);
        check_explicit_list_lengths(&inputs).unwrap();
        inputs.exposure_times = &[0.01, 0.02];
        inputs.isos = &[100, 200];
        inputs.f_numbers = &[8.0, 5.6];
        check_explicit_list_lengths(&inputs).unwrap();
        inputs.isos = &[100];
        assert!(check_explicit_list_lengths(&inputs).is_err());
    }

    #[test]
    fn hdr_without_explicit_or_exif_exposure_is_unsupported() {
        let input = two_inputs();
        let inputs = explicit_inputs(&input, &[], &[], &[]);
        let error = resolve_exposure(MergeMode::Hdr, 0, &inputs, &source("a.png")).unwrap_err();
        assert!(matches!(error, MergeRunError::Unsupported(_)), "{error}");
        assert!(error.to_string().contains("no EXIF exposure"), "{error}");
    }

    #[test]
    fn hdr_rejects_out_of_range_explicit_values() {
        let input = two_inputs();
        let inputs = explicit_inputs(&input, &[0.0, 0.02], &[], &[]);
        let error = resolve_exposure(MergeMode::Hdr, 0, &inputs, &source("a.png")).unwrap_err();
        assert!(error.to_string().contains("--exposure-times"), "{error}");
        let inputs = explicit_inputs(&input, &[], &[0, 100], &[]);
        let error = resolve_exposure(MergeMode::Hdr, 0, &inputs, &source("a.png")).unwrap_err();
        assert!(error.to_string().contains("--isos"), "{error}");
    }

    #[test]
    fn pano_without_exif_uses_the_documented_neutral_default() {
        let input = two_inputs();
        let inputs = explicit_inputs(&input, &[], &[], &[]);
        let exposure = resolve_exposure(MergeMode::Panorama, 0, &inputs, &source("a.png")).unwrap();
        assert!((exposure.exposure_time_s - PANO_DEFAULT_EXPOSURE_S).abs() < 1e-9);
        assert_eq!(exposure.iso, PANO_DEFAULT_ISO);
        assert!((exposure.f_number - PANO_DEFAULT_F_NUMBER).abs() < 1e-9);
    }
}
