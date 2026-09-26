//! CLI adapters for the four session-based recipe stage editors (`spot`,
//! `lens-blur`, `geometry`, `upright`) — MCP-PARITY-A.
//!
//! This file contains **no stage logic**. It translates the `clap` argument
//! structs into the transport-neutral `*Request` of `lumina-stages`, calls the
//! one shared `run()` and prints the returned [`lumina_stages::StageReport`].
//! `lumina-mcp` translates a validated JSON tool call into the same `*Request`
//! and returns the same report, so the two transports share exactly one
//! implementation (`crates/lumina-stages/`) and the CLI keeps its documented
//! `--json` document, its human output and its exit codes unchanged.
//!
//! The extraction is what pays for the slice: it is a pure reduction of
//! `main.rs` (the four handlers, their list/payload builders, the geometry and
//! lens-blur field parsers, and the shared decode/identity helpers moved out).

use super::emit;
use super::CliError;
use clap::Args;
use lumina_core::ImageFrame;
use lumina_raw::RawMetadata;
use lumina_sidecar::SourceIdentity;
use lumina_stages::report::{Persist, StageRun};
use lumina_stages::{
    geometry, lens_blur, spot, upright, GeometryRequest, LensBlurRequest, SpotRequest,
    UprightRequest,
};
use serde_json::json;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------- args
/// G-04 Remove-Parität: inspect and edit the spot-heal recipe state of one
/// image sidecar. Without mutation flags the command lists spots + settings
/// (read-only). Every mutation validates loudly before anything is written;
/// `--detect-objects` only lists candidates unless `--detect-apply` is given
/// (never silent auto-apply).
#[derive(Debug, Args)]
pub struct SpotArgs {
    #[arg(long)]
    pub input: PathBuf,
    #[arg(long)]
    pub virtual_copy: Option<String>,
    #[arg(long)]
    pub json: bool,
    /// List spots + G-04 settings (default when no mutation flag is given).
    #[arg(long)]
    pub list: bool,
    /// Add one heuristic spot (requires `--center-x/--center-y/--radius`).
    #[arg(long)]
    pub add_heuristic: bool,
    #[arg(long, value_name = "0..=1")]
    pub center_x: Option<f32>,
    #[arg(long, value_name = "0..=1")]
    pub center_y: Option<f32>,
    #[arg(long, value_name = "(0,512]")]
    pub radius: Option<f32>,
    #[arg(long, value_name = "0..=1")]
    pub feather: Option<f32>,
    #[arg(long, value_name = "-1..=1")]
    pub offset_dx: Option<f32>,
    #[arg(long, value_name = "-1..=1")]
    pub offset_dy: Option<f32>,
    #[arg(long, value_name = "0..=1")]
    pub opacity: Option<f32>,
    /// Remove all heuristic/generative spot entries of the copy.
    #[arg(long)]
    pub clear: bool,
    /// Select one spot by id for the `--set-*` updates below (requires at
    /// least one `--set-*`; heuristic entries only).
    #[arg(long, value_name = "ID")]
    pub spot_id: Option<String>,
    /// Update the selected heuristic spot's radius (`(0,512]`).
    #[arg(long, value_name = "(0,512]")]
    pub set_radius: Option<f32>,
    /// Update the selected heuristic spot's feather (`0..=1`).
    #[arg(long, value_name = "0..=1")]
    pub set_feather: Option<f32>,
    /// Update the selected heuristic spot's opacity (`0..=1`).
    #[arg(long, value_name = "0..=1")]
    pub set_opacity: Option<f32>,
    /// Update the selected heuristic spot's source-offset x value (`-1..=1`).
    #[arg(long, value_name = "-1..=1")]
    pub set_offset_dx: Option<f32>,
    /// Update the selected heuristic spot's source-offset y value (`-1..=1`).
    #[arg(long, value_name = "-1..=1")]
    pub set_offset_dy: Option<f32>,
    /// Remove one spot entry by id (loud on unknown ids).
    #[arg(long, value_name = "ID")]
    pub remove_spot: Option<String>,
    /// Set the visualize threshold (`0..=1`).
    #[arg(long, value_name = "0..=1")]
    pub set_visualize_threshold: Option<f32>,
    /// Clear the visualize threshold (visualization off).
    #[arg(long)]
    pub clear_visualize: bool,
    /// Merge distraction switches as `k=v,...` with keys
    /// `reflections|people|dust|auto` and values `true|false` into the
    /// stored switches (unnamed keys keep their value; `k=false` switches
    /// off) — G04-FOLLOWUP-1 merge decision, consistent with the GUI
    /// single-checkbox toggles.
    #[arg(long, value_name = "K=V,...")]
    pub set_distraction: Option<String>,
    /// List heuristic spot candidates (stage 1, no model).
    #[arg(long)]
    pub detect_objects: bool,
    /// Persist the detected candidates as heuristic spots (explicit only).
    #[arg(long)]
    pub detect_apply: bool,
    /// Detection threshold (`0..=1`; default is the recipe visualize
    /// threshold when set, else 0.5).
    #[arg(long, value_name = "0..=1")]
    pub detect_threshold: Option<f32>,
    /// Detection cap (`1..=4096`, default 32).
    #[arg(long, value_name = "1..=4096")]
    pub detect_max: Option<usize>,
    /// Regenerate the generative spot identified by `<ID>`: sets
    /// `seed = variant_seed(base, variant)`.
    #[arg(long, value_name = "ID")]
    pub regenerate_variant: Option<String>,
    /// Variant index for `--regenerate-variant` (0 keeps the base seed).
    #[arg(long, value_name = "N")]
    pub variant: Option<u64>,
    /// Base seed for `--regenerate-variant` (required with it).
    #[arg(long, value_name = "N")]
    pub seed: Option<u64>,
}
/// G-05 Lens Blur: inspect and edit the depth-bokeh recipe stage of one
/// image sidecar. Without mutation flags the command lists values + depth
/// status (read-only). Field sets create an enabled stage with centered
/// defaults when none exists (touching lens blur enables it, Lightroom-like).
/// Every mutation validates loudly before anything is written.
#[derive(Debug, Args)]
pub struct LensBlurArgs {
    #[arg(long)]
    pub input: PathBuf,
    #[arg(long)]
    pub virtual_copy: Option<String>,
    #[arg(long)]
    pub json: bool,
    /// List values + depth status (default when no mutation flag is given).
    #[arg(long)]
    pub list: bool,
    /// Enable the stage (keeps stored values).
    #[arg(long)]
    pub enable: bool,
    /// Disable the stage (keeps stored values, renders identity).
    #[arg(long)]
    pub disable: bool,
    /// Set the blur strength (`0..=1`, 0 is identity).
    #[arg(long, value_name = "0..=1")]
    pub set_amount: Option<f32>,
    /// Set the near edge of the sharp depth band (`0..=1`).
    #[arg(long, value_name = "0..=1")]
    pub set_focal_near: Option<f32>,
    /// Set the far edge of the sharp depth band (`0..=1`, `>= near`).
    #[arg(long, value_name = "0..=1")]
    pub set_focal_far: Option<f32>,
    /// Set the bokeh shape (`round|elliptical|hexagonal`).
    #[arg(long, value_name = "SHAPE")]
    pub set_bokeh: Option<String>,
    /// Set the focus rectangle as `x,y,w,h` (normalized `0..=1`).
    #[arg(long, value_name = "X,Y,W,H")]
    pub set_focus_rect: Option<String>,
    /// Reference an external depth map as `RELATIVE_PATH:SHA256` (portable
    /// relative path only; renders abort loudly until the artifact exists).
    #[arg(long, value_name = "PATH:SHA256")]
    pub set_depth_artifact: Option<String>,
    /// Remove the external depth reference (back to the heuristic).
    #[arg(long)]
    pub clear_depth_artifact: bool,
    /// Remove the whole lens-blur stage (identity).
    #[arg(long)]
    pub clear: bool,
}
/// LRPAR-G06-UPRIGHT-15 (Release 1.5): inspect and edit the persisted
/// automatic upright analysis of one virtual copy. The analysis is classic,
/// model-free and deterministic; `--analyze` binds it to the current source
/// identity (`upright_input_fingerprint`), so a changed source is reported as
/// stale, never silently recomputed. Every mutation validates loudly and
/// appends exactly one history entry. See `feature/architecture/pipeline.md`
/// § F-099.
#[derive(Debug, Clone, Args)]
pub struct UprightArgs {
    #[arg(long)]
    pub input: PathBuf,
    #[arg(long)]
    pub virtual_copy: Option<String>,
    #[arg(long)]
    pub json: bool,
    /// List the persisted upright stage and whether its fingerprint is still
    /// fresh for the current source (read-only; the default).
    #[arg(long)]
    pub list: bool,
    /// Run the deterministic `upright-lines-v1` analysis on the decoded source
    /// and persist it (fingerprint bound to the source identity). Enables the
    /// stage in the same step (use `--disable` to keep a suggestion without
    /// applying it).
    #[arg(long)]
    pub analyze: bool,
    /// Apply the persisted analysis as the effective F-099 perspective.
    #[arg(long)]
    pub enable: bool,
    /// Stop applying the persisted analysis; the manual perspective returns.
    #[arg(long)]
    pub disable: bool,
    /// Remove the whole upright stage (analysis included).
    #[arg(long)]
    pub clear: bool,
}
/// G-06 Geometrie-Parität (LRPAR-G06-GEO): inspect and edit the geometry
/// stages of one image sidecar. Without mutation flags the command lists
/// crop/lens/perspective values (read-only). `--straighten` is a documented
/// alias of `--set-rotation` (same field, same validation). Every mutation
/// validates loudly before anything is written and appends exactly one
/// history entry (visible step per call); the original image is never
/// modified. See `feature/architecture/pipeline.md` §§ F-093, F-098, F-099.
#[derive(Debug, Args)]
pub struct GeometryArgs {
    #[arg(long)]
    pub input: PathBuf,
    #[arg(long)]
    pub virtual_copy: Option<String>,
    #[arg(long)]
    pub json: bool,
    /// List crop/lens/perspective values (default when no mutation flag or
    /// `--lensfun-status` is given).
    #[arg(long)]
    pub list: bool,
    /// Set the crop to an aspect preset
    /// (`original|1:1|4:5|5:4|3:2|2:3|4:3|3:4|16:9|9:16`).
    #[arg(long, value_name = "PRESET")]
    pub set_crop_aspect: Option<String>,
    /// Set a free crop rectangle as `x,y,w,h` (normalized `0..=1`).
    #[arg(long, value_name = "X,Y,W,H")]
    pub set_crop_free: Option<String>,
    /// Remove the crop (full frame, keeps rotation/mirrors).
    #[arg(long)]
    pub clear_crop: bool,
    /// Set the rotation angle in degrees (`-180..=180`).
    #[arg(long, value_name = "-180..=180")]
    pub set_rotation: Option<f64>,
    /// Straighten angle in degrees (`-180..=180`; alias of
    /// `--set-rotation`, same field, same validation).
    #[arg(long, value_name = "-180..=180")]
    pub straighten: Option<f64>,
    /// Set the mirror flags (`h|v|hv|none`).
    #[arg(long, value_name = "h|v|hv|none")]
    pub set_mirror: Option<String>,
    /// Remove the whole geometry stage (crop, rotation, mirrors; identity).
    #[arg(long)]
    pub clear_geometry: bool,
    /// Set the manual lens profile
    /// (`wide-light|tele-light|standard-neutral`).
    #[arg(long, value_name = "PROFILE")]
    pub set_lens_profile: Option<String>,
    /// Set one manual lens field as `FIELD:VALUE` with
    /// `FIELD = distortion_k1|distortion_k2|distortion_k3|vignette_c0|
    /// vignette_c1|vignette_c2|ca_red|ca_blue` (repeatable).
    #[arg(long, value_name = "FIELD:VALUE")]
    pub set_lens: Vec<String>,
    /// Remove the whole manual lens-correction stage (identity).
    #[arg(long)]
    pub clear_lens: bool,
    /// Set one manual perspective field as `FIELD:VALUE` with
    /// `FIELD = vertical|horizontal|rotation|scale|aspect_ratio|shift_x|
    /// shift_y` (repeatable).
    #[arg(long, value_name = "FIELD:VALUE")]
    pub set_perspective: Vec<String>,
    /// Remove the whole manual perspective stage (identity).
    #[arg(long)]
    pub clear_perspective: bool,
    /// Report the Lensfun auto-profile resolution for the input (EXIF →
    /// profile match with distortion/vignetting/TCA flags, or the loud
    /// reason no corrector applies). Read-only, no save.
    #[arg(long)]
    pub lensfun_status: bool,
}

// ---------------------------------------------------------------- spot

/// G-04 Remove-Parität: list and edit the spot-heal recipe state of one image
/// sidecar. The original image is never modified; every write goes through the
/// shared `lumina-stages::spot::run` (validate → atomic save).
pub fn spot(args: SpotArgs) -> Result<(), CliError> {
    let request = SpotRequest {
        input: args.input.display().to_string(),
        virtual_copy: args.virtual_copy,
        list: args.list,
        add_heuristic: args.add_heuristic,
        center_x: args.center_x,
        center_y: args.center_y,
        radius: args.radius,
        feather: args.feather,
        offset_dx: args.offset_dx,
        offset_dy: args.offset_dy,
        opacity: args.opacity,
        clear: args.clear,
        spot_id: args.spot_id,
        set_radius: args.set_radius,
        set_feather: args.set_feather,
        set_opacity: args.set_opacity,
        set_offset_dx: args.set_offset_dx,
        set_offset_dy: args.set_offset_dy,
        remove_spot: args.remove_spot,
        set_visualize_threshold: args.set_visualize_threshold,
        clear_visualize: args.clear_visualize,
        set_distraction: args.set_distraction,
        detect_objects: args.detect_objects,
        detect_apply: args.detect_apply,
        detect_threshold: args.detect_threshold,
        detect_max: args.detect_max,
        regenerate_variant: args.regenerate_variant,
        variant: args.variant,
        seed: args.seed,
    };
    print(args.json, spot::run(&request, Persist::Immediately)?)
}

// ---------------------------------------------------------------- lens-blur

/// G-05 Lens Blur: inspect and edit the depth-bokeh recipe stage of one virtual
/// copy through the shared `lumina-stages::lens_blur::run`.
pub fn lens_blur(args: LensBlurArgs) -> Result<(), CliError> {
    let request = LensBlurRequest {
        input: args.input.display().to_string(),
        virtual_copy: args.virtual_copy,
        list: args.list,
        enable: args.enable,
        disable: args.disable,
        set_amount: args.set_amount,
        set_focal_near: args.set_focal_near,
        set_focal_far: args.set_focal_far,
        set_bokeh: args.set_bokeh,
        set_focus_rect: args.set_focus_rect,
        set_depth_artifact: args.set_depth_artifact,
        clear_depth_artifact: args.clear_depth_artifact,
        clear: args.clear,
    };
    print(args.json, lens_blur::run(&request, Persist::Immediately)?)
}

// ---------------------------------------------------------------- geometry

/// G-06 Geometrie-Parität: inspect and edit the geometry stages of one virtual
/// copy through the shared `lumina_stages::geometry::run`.
pub fn geometry(args: GeometryArgs) -> Result<(), CliError> {
    let request = GeometryRequest {
        input: args.input.display().to_string(),
        virtual_copy: args.virtual_copy,
        list: args.list,
        set_crop_aspect: args.set_crop_aspect,
        set_crop_free: args.set_crop_free,
        clear_crop: args.clear_crop,
        set_rotation: args.set_rotation,
        straighten: args.straighten,
        set_mirror: args.set_mirror,
        clear_geometry: args.clear_geometry,
        set_lens_profile: args.set_lens_profile,
        set_lens: args.set_lens,
        clear_lens: args.clear_lens,
        set_perspective: args.set_perspective,
        clear_perspective: args.clear_perspective,
        lensfun_status: args.lensfun_status,
        lensfun_report: args
            .lensfun_status
            .then(|| resolve_lensfun_report(&args.input)),
    };
    print(args.json, geometry::run(&request, Persist::Immediately)?)
}

/// Resolves the Lensfun auto-profile status for one input file (G-06): which
/// corrector a render would build from the input's EXIF, or the loud reason none
/// applies (no metadata, missing EXIF fields, no system DB, no matching
/// profile, identity correction). Read-only — never a correction.
///
/// Stays in the CLI: it needs the CLI's `lensfun`-gated corrector build, and the
/// shared crate deliberately carries no native Lensfun capability. The result is
/// handed to the shared `geometry::run` so both transports format one value.
pub fn resolve_lensfun_report(input: &Path) -> String {
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
        match super::build_lensfun_corrector(Some(&metadata)) {
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

// ---------------------------------------------------------------- upright

/// LRPAR-G06-UPRIGHT-15: analyze/enable/disable/clear/status the persisted
/// upright stage of one virtual copy through the shared
/// `lumina_stages::upright::run`.
pub fn upright(args: UprightArgs) -> Result<(), CliError> {
    let request = UprightRequest {
        input: args.input.display().to_string(),
        virtual_copy: args.virtual_copy,
        list: args.list,
        analyze: args.analyze,
        enable: args.enable,
        disable: args.disable,
        clear: args.clear,
    };
    print(args.json, upright::run(&request, Persist::Immediately)?)
}

// ---------------------------------------------------------------- shared glue

/// Prints a stage report exactly as the pre-extraction `*_list` functions did:
/// the shared `--json` document, or the shared human lines followed by the
/// one-line status.
fn print(json_mode: bool, run: StageRun) -> Result<(), CliError> {
    let report = run.report;
    if json_mode {
        return emit(true, report.payload, &report.summary);
    }
    for line in &report.lines {
        println!("{line}");
    }
    emit(
        false,
        json!({ "command": report.command, "status": "ok" }),
        &report.summary,
    )
}

/// Thin error-retyping shims. The shared helpers live in `lumina-stages` so
/// `lumina-cli` and `lumina-mcp` derive the *same* source identity and decode
/// the *same* bytes (`upright --analyze` embeds the derived fingerprint in the
/// sidecar, so a second implementation would be a silent divergence). The
/// shims only re-type the error: they map the shared `StageError` back onto the
/// `CliError` variant the pre-extraction code constructed, so both the rendered
/// text and the matched variant shape are unchanged.
pub fn io_error(path: &Path, error: std::io::Error) -> CliError {
    match lumina_stages::error::StageError::io(path, error) {
        lumina_stages::error::StageError::Io { path, message } => CliError::Io { path, message },
        other => other.into(),
    }
}

pub fn decode_input(
    path: &Path,
    bytes: &[u8],
) -> Result<(ImageFrame, Option<RawMetadata>), CliError> {
    match lumina_stages::decode::decode_input(path, bytes) {
        Ok(decoded) => Ok(decoded),
        Err(lumina_stages::error::StageError::Core(error)) => Err(error.into()),
        Err(lumina_stages::error::StageError::Raw(error)) => Err(error.into()),
        Err(other) => Err(other.into()),
    }
}

pub fn source_identity(
    path: &Path,
    bytes: &[u8],
    frame: &ImageFrame,
    raw_metadata: Option<&RawMetadata>,
) -> Result<SourceIdentity, CliError> {
    match lumina_stages::decode::source_identity(path, bytes, frame, raw_metadata) {
        Ok(identity) => Ok(identity),
        Err(lumina_stages::error::StageError::Message(message)) => Err(CliError::Message(message)),
        Err(lumina_stages::error::StageError::Io { path, message }) => {
            Err(CliError::Io { path, message })
        }
        Err(other) => Err(other.into()),
    }
}
