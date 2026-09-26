//! CLI adapters for the five **path-based** / artefact commands of
//! MCP-PARITY-B: `collections`, `smart-collections`, `relocate`, `generative`
//! and `regenerate`.
//!
//! This file contains **no command logic**. It translates the `clap` argument
//! structs into the transport-neutral `*Request` of `lumina-stages`, calls the
//! one shared `run()` and prints the returned [`lumina_stages::BulkReport`]
//! verbatim. `lumina-mcp` translates a schema-validated tool call into the same
//! `*Request` and returns the same report, so the two transports share exactly
//! one implementation and the CLI keeps its documented `--json` document, its
//! human output and its exit codes unchanged.
//!
//! The extraction is a pure reduction of `main.rs`: the five handlers, the
//! membership splitter/formatter, the smart-collection catalogue loader, the
//! cross-volume move, the generative canvas/identity/producer helpers, the
//! auto-tone write path and the shared walk/identity helpers moved out.

use super::{emit, CliError};
use clap::Args;
use lumina_core::LensfunCorrectorRef;
use lumina_raw::RawMetadata;
use lumina_stages::generative_artifact::CorrectorSource;
use lumina_stages::report::BulkReport;
use lumina_stages::{
    collections, generative, regenerate, relocate, smart_collections, GenerativeRequest, Persist,
    RegenerateModule, RegenerateRequest,
};
use serde_json::Value;
use std::path::PathBuf;

// ---------------------------------------------------------------- args
/// G-15 META-MVP (Slice 2): inspect and edit the static collection memberships
/// of one image sidecar. `--add-to` takes `id=name` (split at the first `=`);
/// `--remove-from` takes the membership `id`.
#[derive(Debug, Args)]
pub struct CollectionsArgs {
    #[arg(long)]
    pub input: PathBuf,
    /// Memberships to add/rename as `id=name` (repeatable, in order).
    #[arg(long = "add-to")]
    pub add_to: Vec<String>,
    /// Membership ids to remove (repeatable, in order after `--add-to`).
    #[arg(long = "remove-from")]
    pub remove_from: Vec<String>,
    #[arg(long)]
    pub json: bool,
}

/// G-15 META-MVP (Slice 2): evaluate a portable smart-collection catalog file
/// against every sidecar found under `--input` (read-only).
#[derive(Debug, Args)]
pub struct SmartCollectionsArgs {
    #[arg(long)]
    pub input: PathBuf,
    /// Path to the catalog file
    /// (`{"format":"lumina-smart-catalog","version":1,"collections":[...]}`).
    /// A plain CLI argument, never persisted into recipe data.
    #[arg(long)]
    pub catalog: PathBuf,
    #[arg(long)]
    pub json: bool,
}

/// G-09 Library-Parität (LRPAR-G09-LIB): move one image together with its
/// `.lumina.json` / `.lumina.zdata` companions to `--to` (one folder move in
/// one); companions keep their sidecar-derived file names next to the target.
/// No schema change — only filesystem moves.
#[derive(Debug, Args)]
pub struct RelocateArgs {
    /// Source image to move (must exist).
    #[arg(long)]
    pub from: PathBuf,
    /// Destination image path (must not exist; parent must exist).
    #[arg(long)]
    pub to: PathBuf,
    #[arg(long)]
    pub json: bool,
}

/// `lumina generative` arguments (see `feature/product/generative-expand.md`).
#[derive(Debug, Clone, Args)]
pub struct GenerativeArgs {
    #[arg(long)]
    pub input: PathBuf,
    #[arg(long)]
    pub virtual_copy: Option<String>,
    /// Report the persisted artifact status and exit (read-only).
    #[arg(long)]
    pub status: bool,
    /// Produce and persist the composited canvas artifact.
    #[arg(long)]
    pub generate: bool,
    /// Explicit regeneration: replace an existing record for the same identity.
    #[arg(long)]
    pub force: bool,
    /// Remove the persisted artifact link from the recipe (the bundle record
    /// and the original image are left untouched).
    #[arg(long)]
    pub remove: bool,
    /// Prompt (identity-bearing, roundtrip-stable; may be empty).
    #[arg(long)]
    pub prompt: Option<String>,
    /// Negative prompt (identity-bearing; additive schema field).
    #[arg(long)]
    pub negative_prompt: Option<String>,
    /// Deterministic seed (identity-bearing).
    #[arg(long)]
    pub seed: Option<u64>,
    /// `auto_fill_transparent`: fill transparent pixels after lens correction.
    #[arg(long)]
    pub auto_fill: bool,
    /// `expand_beyond_image`: enlarge the canvas (requires `--canvas`).
    #[arg(long)]
    pub expand: bool,
    /// Target canvas `WxH+X+Y` (offsets may be negative) for `--expand`.
    #[arg(long)]
    pub canvas: Option<String>,
    /// `keep_generative_content` crop decision.
    #[arg(long)]
    pub keep: Option<bool>,
    #[arg(long)]
    pub json: bool,
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
pub struct RegenerateArgs {
    #[arg(long)]
    pub input: PathBuf,
    #[arg(long)]
    pub virtual_copy: Option<String>,
    /// Modules to regenerate (repeatable): `masks`, `auto-tone`, `matching`.
    /// Omitted = regenerate every stale/missing module.
    #[arg(long = "module")]
    pub modules: Vec<ModuleArg>,
    #[arg(long, default_value_t = 0.5)]
    pub target_luminance: f64,
    #[arg(long)]
    pub json: bool,
}

/// The `--module` value set of `lumina regenerate` (F-100). The three words are
/// the whole 1.0 set; later modules (denoise/face/cull/merge) extend the
/// shared `RegenerateModule` enum without changing the convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ModuleArg {
    /// AI mask inference (an explicit refresh request).
    Masks,
    /// The six Auto-Tone sliders plus their mirrors and the fingerprint.
    #[value(name = "auto-tone")]
    AutoTone,
    /// F-008 exposure matching.
    Matching,
}

impl From<ModuleArg> for RegenerateModule {
    fn from(value: ModuleArg) -> Self {
        match value {
            ModuleArg::Masks => Self::Masks,
            ModuleArg::AutoTone => Self::AutoTone,
            ModuleArg::Matching => Self::Matching,
        }
    }
}

impl From<RegenerateModule> for ModuleArg {
    fn from(value: RegenerateModule) -> Self {
        match value {
            RegenerateModule::Masks => Self::Masks,
            RegenerateModule::AutoTone => Self::AutoTone,
            RegenerateModule::Matching => Self::Matching,
        }
    }
}

// ---------------------------------------------------------------- commands

/// G-15 META-MVP (Slice 2): list and edit the collection memberships of one
/// image sidecar through the shared `lumina-stages::collections::run`.
pub fn collections(args: CollectionsArgs) -> Result<(), CliError> {
    let request = lumina_stages::collections::CollectionsRequest {
        input: args.input.display().to_string(),
        add_to: args.add_to,
        remove_from: args.remove_from,
    };
    print(
        args.json,
        collections::run(&request, Persist::Immediately)?.report,
    )
}

/// G-15 META-MVP (Slice 2): evaluate the catalogue against every sidecar under
/// `input` through the shared `lumina-stages::smart_collections::run`. A
/// partially failed run keeps its report and then exits 3, exactly as before.
pub fn smart_collections(args: SmartCollectionsArgs) -> Result<(), CliError> {
    let request = lumina_stages::smart_collections::SmartCollectionsRequest {
        input: args.input.display().to_string(),
        catalog: args.catalog.display().to_string(),
    };
    let (run, failures) = smart_collections::run(&request)?;
    print(args.json, run.report)?;
    if failures.is_partial() {
        return Err(CliError::BatchPartial {
            failed: failures.failed,
        });
    }
    Ok(())
}

/// G-09 Library-Parität: move one image with its companions through the shared
/// `lumina-stages::relocate::run`.
pub fn relocate(args: RelocateArgs) -> Result<(), CliError> {
    let request = lumina_stages::relocate::RelocateRequest {
        from: args.from.display().to_string(),
        to: args.to.display().to_string(),
    };
    print(args.json, relocate::run(&request)?.report)
}

/// `lumina generative` — produce/report/remove the persisted
/// `generative_canvas` artefact through the shared
/// `lumina-stages::generative::run`.
pub fn generative(args: GenerativeArgs) -> Result<(), CliError> {
    let request = GenerativeRequest {
        input: args.input.display().to_string(),
        virtual_copy: args.virtual_copy,
        status: args.status,
        generate: args.generate,
        force: args.force,
        remove: args.remove,
        prompt: args.prompt,
        negative_prompt: args.negative_prompt,
        seed: args.seed,
        auto_fill: args.auto_fill,
        expand: args.expand,
        canvas: args.canvas,
        keep: args.keep,
    };
    let mut correctors = CliCorrectors;
    print(
        args.json,
        generative::run(&request, &mut correctors, Persist::Immediately)?.report,
    )
}

/// GUI-GEN-GRANULAR-10 (F-100): regenerate the selected (or every stale)
/// module through the shared `lumina-stages::regenerate::run`, with the CLI's
/// own render entry so its optional GPU routing is unchanged.
pub fn regenerate(args: RegenerateArgs) -> Result<(), CliError> {
    let request = RegenerateRequest {
        input: args.input.display().to_string(),
        virtual_copy: args.virtual_copy,
        modules: args
            .modules
            .into_iter()
            .map(RegenerateModule::from)
            .collect(),
        target_luminance: args.target_luminance,
    };
    let mut correctors = CliCorrectors;
    print(
        args.json,
        regenerate::run(&request, &mut correctors, &CliRender, Persist::Immediately)?.report,
    )
}

// ---------------------------------------------------------------- shared ports

/// The CLI's render entry for the `matching` module. It is the pre-extraction
/// call, unchanged: `render_standard_with_generative` with no generative canvas
/// input, so the CLI keeps its GPU routing and its loud CPU-fallback behaviour.
/// In a build without the `gpu` feature this is literally
/// `lumina_core::render_frame`, which is what the MCP server calls — the
/// configuration the byte-identity test measures.
struct CliRender;

impl regenerate::MatchingRender for CliRender {
    fn render(
        &self,
        frame: &lumina_core::ImageFrame,
        context: &lumina_core::RenderContext<'_>,
    ) -> Result<lumina_core::RenderOutput, lumina_stages::StageError> {
        super::render_standard_with_generative(
            frame,
            context.recipe,
            context,
            lumina_core::GenerativeCanvasInput::default(),
        )
        .map_err(|error| lumina_stages::StageError::Message(error.to_string()))
    }
}

/// The CLI's Lensfun corrector source for the artefact commands.
///
/// Stays in the CLI because it needs the CLI's `lensfun`-gated corrector build
/// and the process-lifetime diagnostics sink of `lensfun_cli`; the shared crate
/// deliberately carries no native Lensfun capability. The shared code hands over
/// the metadata **it** decoded, so the corrector is built exactly once, in the
/// pre-extraction order.
struct CliCorrectors;

/// Keeps the corrector's database alive for as long as the corrector borrow
/// (the modifier references lens data owned by the database).
#[cfg(feature = "lensfun")]
type CorrectorHolder = Option<(lumina_lensfun::LensfunDb, lumina_lensfun::Corrector)>;

/// Without the `lensfun` feature there is nothing to keep alive: the
/// pre-extraction `#[cfg(not(feature = "lensfun"))]` branch passed `None`.
#[cfg(not(feature = "lensfun"))]
type CorrectorHolder = Option<()>;

#[cfg(feature = "lensfun")]
impl CorrectorSource for CliCorrectors {
    type Owner = CorrectorHolder;

    fn build<'o>(
        &mut self,
        metadata: Option<&RawMetadata>,
        owner: &'o mut CorrectorHolder,
    ) -> Option<LensfunCorrectorRef<'o>> {
        *owner = super::build_lensfun_corrector(metadata);
        owner
            .as_ref()
            .map(|(_, corrector)| LensfunCorrectorRef(corrector))
    }
}

#[cfg(not(feature = "lensfun"))]
impl CorrectorSource for CliCorrectors {
    type Owner = CorrectorHolder;

    fn build<'o>(
        &mut self,
        _metadata: Option<&RawMetadata>,
        _owner: &'o mut CorrectorHolder,
    ) -> Option<LensfunCorrectorRef<'o>> {
        None
    }
}

// ---------------------------------------------------------------- shared glue

/// Prints a bulk report exactly as the pre-extraction handlers did through
/// `emit`: the shared `--json` document, or the shared human line. A command
/// that prints nothing on a transport (`generative --remove`) prints nothing
/// here too.
fn print(json_mode: bool, report: BulkReport) -> Result<(), CliError> {
    if json_mode {
        if let Some(payload) = report.payload {
            return emit(true, payload, "");
        }
        return Ok(());
    }
    let Some(line) = report.lines.first().cloned() else {
        return Ok(());
    };
    emit(false, Value::Null, &line)
}
