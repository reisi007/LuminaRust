//! LRPAR-G13-MERGE-15 / MERGE-CLI-1 + MERGE-IMPL-15 (F6): `merge-hdr` /
//! `merge-pano` commands.
//!
//! SOLL: `feature/decisions/LRPAR-G13-MERGE-15.md` (§Pipeline-Einordnung,
//! §Persistenz, §Abnahme CLI) and `feature/platform/cli-gui-wasm.md`
//! (§ „HDR-/Panorama-Merge (G-13, Release 1.5: CLI)"). This module is the
//! **CLI wiring only**: it maps the clap flags onto the shared orchestration
//! [`lumina_merge::bundle::run_merge`], which the GUI actions call with the
//! same step sequence (F6 dedup), and hands it the **shared** decode +
//! exposure adapter [`lumina_stages::merge`] — the MCP tools
//! (`lumina_merge_hdr`/`lumina_merge_pano`, MCP-PARITY-C) will call the very
//! same adapter, so there is exactly one implementation of the decode
//! context, the exposure policy and their error texts. There is no second
//! image-processing implementation and neither the sidecar schema nor the
//! merge math changes.
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
    merge_command_name, run_merge as run_merge_bundle, HdrDeghostMode, MergeRunOptions,
    MergeSourceFrame,
};
use lumina_sidecar::MergeMode;
use std::path::PathBuf;

use super::{emit, CliError};

/// Slice B: the `--deghost` value parser (`off` | `<threshold-float>`).
///
/// A custom parser, not `PossibleValuesParser`: one accepted value is a
/// number, and the range `(0.05, 2.0]` belongs to the shared
/// [`HdrDeghostMode`] rule rather than to a clap-side copy of it. A rejected
/// value reaches the user as the CLI's own usage error with the accepted
/// values and the range — the merge domain rejects a persisted out-of-range
/// threshold separately (`merge failed:`), never silently.
fn parse_deghost(value: &str) -> Result<HdrDeghostMode, String> {
    HdrDeghostMode::parse(value)
}

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
    /// Slice A: panorama brightness matching. `off` (default) keeps the
    /// pixels untouched; `overlap-median` scales each frame by the median
    /// overlap-luminance ratio vs. the reference before blending and records
    /// the gains in the recipe. Panorama only: any other value on `merge-hdr`
    /// is a loud `merge failed:`.
    #[arg(long, default_value = "off", value_parser = clap::builder::PossibleValuesParser::new(["off", "overlap-median"]))]
    pub gain_compensation: String,
    /// Slice B: HDR ghost removal. `off` (default) keeps the plain hat-weight
    /// mean; a number in `(0.05, 2.0]` is the relative radiance deviation
    /// above which a frame counts as a ghost at that pixel (per channel, vs.
    /// the reference frame) and contributes weight 0 — the opt-in suggestion
    /// is `0.25`. HDR only: any other value on `merge-pano` is a loud
    /// `merge failed:`.
    #[arg(long, default_value = "off", value_parser = parse_deghost)]
    pub deghost: HdrDeghostMode,
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

/// Thin CLI wiring over the shared orchestration
/// ([`lumina_merge::bundle::run_merge`]) plus the shared decode + exposure
/// adapter ([`lumina_stages::merge`]): map the clap flags onto the
/// frontend-independent run options, wrap the adapter errors into
/// [`CliError`] (their class and text are transport-independent), render the
/// outcome. No merge logic lives here — the MCP tools call the same adapter.
fn run_merge(mode: MergeMode, args: &MergeArgs) -> Result<(), CliError> {
    let command = merge_command_name(mode);
    let explicit = lumina_stages::merge::MergeExplicitInputs {
        input: &args.input,
        exposure_times: &args.exposure_times,
        isos: &args.isos,
        f_numbers: &args.f_numbers,
    };
    lumina_stages::merge::check_explicit_list_lengths(&explicit).map_err(CliError::Message)?;
    let gain_compensation = lumina_merge::bundle::PanoGainMode::parse(&args.gain_compensation)
        .map_err(CliError::Message)?;
    let options = MergeRunOptions {
        output: args.output.clone(),
        max_shift_px: args.max_shift_px,
        blend_width_px: args.blend_width_px,
        gain_compensation,
        deghost: args.deghost,
        force: args.force,
        dng_decode_version: lumina_raw::libraw_decode_version(),
    };
    let resolve = |mode: MergeMode, index: usize, source: &MergeSourceFrame| {
        lumina_stages::merge::resolve_exposure(mode, index, &explicit, source)
    };
    let outcome = run_merge_bundle(
        mode,
        &args.input,
        &options,
        lumina_stages::merge::decode_source,
        resolve,
    )
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
