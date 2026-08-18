use clap::{Args, Parser, Subcommand};
use lumina_core::{
    match_total_exposure_masked, render_frame, suggest_auto_tone, tone_fingerprint, AutoToneConfig,
    ImageFileFormat, ImageFrame, MaskContext, MaskPlane, MaskPolicy, RenderContext,
};
use lumina_raw::{RawError, RawMetadata};
use lumina_sidecar::{
    artifact_status, load_sidecar, save_sidecar, sidecar_path_for, AnalysisFingerprint,
    ArtifactStatus, DecodeFingerprint, GeometryFingerprint, HistoryEntry, MaskStatus, Preset,
    SidecarDocument, SourceIdentity,
};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

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
    Import(FileArgs),
    Develop(DevelopArgs),
    Render(FileArgs),
    Export(ExportArgs),
    Batch(BatchArgs),
    Mask(MaskArgs),
    Reindex(IndexArgs),
    Validate(IndexArgs),
}

#[derive(Debug, Args)]
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
    #[arg(long)]
    update_masks: bool,
    #[arg(long)]
    migrate: bool,
    #[arg(long)]
    json: bool,
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
}

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
}

#[derive(Debug, Args)]
struct InspectArgs {
    input: PathBuf,
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
    #[error("invalid preset JSON: {0}")]
    Preset(String),
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("error: {error}");
        std::process::exit(1);
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
    }
}

fn import_file(args: FileArgs) -> Result<(), CliError> {
    let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
    let (frame, raw) = decode_input(&args.input, &bytes)?;
    let path = sidecar_path_for(&args.input);
    if args.migrate && path.exists() {
        migrate_sidecar(&path)?;
    } else if path.exists() {
        load_sidecar(&path)?;
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

fn develop(args: DevelopArgs) -> Result<(), CliError> {
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
        },
        args.virtual_copy.as_deref(),
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
    // This check is deliberately before decoding and writing the export.  A
    // warning is allowed by the product contract, but an explicit update must
    // never pretend that an unavailable inference engine succeeded.
    preflight_masks(&args.input, args.virtual_copy.as_deref(), args.update_masks)?;
    let output = args.output.with_extension(format_extension(&args.format));
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
        },
        args.virtual_copy.as_deref(),
        &mut mask_warnings,
    )?;
    emit(
        args.json,
        serde_json::json!({"command":"export", "output":output, "quality":args.quality, "status":"ok", "mask_warnings":mask_warnings}),
        "exported",
    )
}

fn preflight_masks(input: &Path, virtual_copy: Option<&str>, update: bool) -> Result<(), CliError> {
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
    if update {
        return Err(CliError::Message(format!(
            "--update-masks requested for {missing} mask(s), but no AI inference engine is available; export aborted"
        )));
    }
    eprintln!(
        "warning: {missing} mask(s) are missing or unavailable; they will not be applied (use --update-masks when an inference engine is installed)"
    );
    Ok(())
}

fn mask(args: MaskArgs) -> Result<(), CliError> {
    let path = sidecar_path_for(&args.input);
    let mut document = load_sidecar(&path)?;
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
        }
        save_sidecar(&path, &document)?;
    }
    emit(
        args.json,
        serde_json::json!({"command":"mask", "input":args.input, "updated":args.update_masks, "status":"ok"}),
        "mask status updated",
    )
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

fn reindex(args: IndexArgs) -> Result<(), CliError> {
    let mut files = Vec::new();
    collect_sidecars(&args.input, &mut files)?;
    let mut valid = 0usize;
    for path in files {
        if load_sidecar(&path).is_ok() {
            valid += 1;
        }
    }
    emit(
        args.json,
        serde_json::json!({"command":"reindex", "input":args.input, "sidecars":valid, "status":"ok"}),
        "reindexed",
    )
}

fn batch(args: BatchArgs) -> Result<(), CliError> {
    if args.jobs == 0 {
        return Err(CliError::Message("--jobs must be greater than zero".into()));
    }
    validate_format(&args.format)?;
    validate_quality(args.quality)?;
    fs::create_dir_all(&args.output).map_err(|e| io_error(&args.output, e))?;
    let mut inputs = Vec::new();
    collect_images(&args.input, &mut inputs)?;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(args.jobs)
        .build()
        .map_err(|e| CliError::Message(e.to_string()))?;
    let results = pool.install(|| {
        inputs
            .par_iter()
            .map(|input| batch_one(input, &args))
            .collect::<Vec<_>>()
    });
    let failed = results.iter().filter(|r| r.is_err()).count();
    if args.json {
        println!(
            "{}",
            serde_json::to_string(
                &results
                    .iter()
                    .map(|r| match r {
                        Ok(v) => serde_json::json!({"status":"ok","input":v}),
                        Err(e) => serde_json::json!({"status":"failed","error":e.to_string()}),
                    })
                    .collect::<Vec<_>>()
            )
            .unwrap()
        );
    } else {
        println!(
            "batch: {} succeeded, {} failed",
            results.len() - failed,
            failed
        );
    }
    if failed != 0 {
        return Err(CliError::Message(format!("batch failed: {failed} item(s)")));
    }
    Ok(())
}

fn batch_one(input: &Path, args: &BatchArgs) -> Result<String, CliError> {
    let name = input
        .file_name()
        .ok_or_else(|| CliError::Message("input has no file name".into()))?;
    let output = args
        .output
        .join(name)
        .with_extension(format_extension(&args.format));
    let status = args
        .output
        .join(format!("{}.status.json", name.to_string_lossy()));
    if args.resume && status.exists() && output.is_file() {
        let state = fs::read_to_string(&status).map_err(|e| io_error(&status, e))?;
        if state.contains("\"status\":\"ok\"") {
            return Ok(input.display().to_string());
        }
    }
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
                },
                args.virtual_copy.as_deref(),
                &mut Vec::new(),
            ) {
                Ok(()) => {
                    last = None;
                    break;
                }
                Err(e) => last = Some(e),
            }
        }
        if let Some(e) = last {
            return Err(e);
        }
    }
    write_atomically(&status, serde_json::to_vec(&serde_json::json!({"input":input,"output":output,"status":if args.dry_run {"dry-run"} else {"ok"}})).unwrap().as_slice())?;
    Ok(input.display().to_string())
}

fn collect_images(path: &Path, output: &mut Vec<PathBuf>) -> Result<(), CliError> {
    for entry in fs::read_dir(path).map_err(|e| io_error(path, e))? {
        let entry = entry.map_err(|e| io_error(path, e))?;
        let p = entry.path();
        if p.is_dir() {
            collect_images(&p, output)?;
        } else if p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| {
                matches!(
                    e.to_ascii_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "webp" | "arw" | "cr2" | "cr3" | "dng" | "nef"
                )
            })
            .unwrap_or(false)
        {
            output.push(p);
        }
    }
    Ok(())
}
fn collect_sidecars(path: &Path, output: &mut Vec<PathBuf>) -> Result<(), CliError> {
    for entry in fs::read_dir(path).map_err(|e| io_error(path, e))? {
        let entry = entry.map_err(|e| io_error(path, e))?;
        let p = entry.path();
        if p.is_dir() {
            collect_sidecars(&p, output)?
        } else if p.to_string_lossy().ends_with(".lumina.json") {
            output.push(p)
        }
    }
    Ok(())
}
fn emit(json: bool, value: serde_json::Value, text: &str) -> Result<(), CliError> {
    if json {
        println!("{}", value);
    } else {
        println!("{text}");
    }
    Ok(())
}

fn migrate_sidecar(path: &Path) -> Result<(), CliError> {
    let json = fs::read_to_string(path).map_err(|error| io_error(path, error))?;
    let migrated = lumina_sidecar::migrate_json(&json)?;
    if migrated != json {
        write_atomically(path, migrated.as_bytes())?;
    }
    Ok(())
}

fn process(args: ProcessArgs) -> Result<(), CliError> {
    process_selected(args, None, &mut Vec::new())
}

fn process_selected(
    args: ProcessArgs,
    virtual_copy: Option<&str>,
    mask_warnings_out: &mut Vec<String>,
) -> Result<(), CliError> {
    reject_same_path(&args.input, &args.output)?;
    let format = output_format(&args.output)?;
    let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
    let (mut frame, raw_metadata) = decode_input(&args.input, &bytes)?;
    let wb = raw_metadata.as_ref().map(|m| m.camera_white_balance);
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
                .ok_or_else(|| CliError::Message(format!("unknown virtual copy `{id}")))
        })
        .transpose()?
        .unwrap_or(0);
    let mut recipe = document.virtual_copies[copy_index].recipe.clone();
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
    // Mask artifact planes loaded from the optional `.lumina.zdata` sidecar.
    // Missing or unreadable zdata is *not* a hard error: affected layers are
    // skipped and reported via the `MaskPolicy::Warn` path.
    let mut planes: BTreeMap<(String, String), MaskPlane> = BTreeMap::new();
    let zdata_path = lumina_sidecar::zdata_path_for(&args.input);
    if zdata_path.exists() {
        if let Ok(container) = lumina_sidecar::load_zdata(&zdata_path) {
            let copy = &document.virtual_copies[copy_index];
            for mask in copy
                .mask_library
                .iter()
                .filter(|m| matches!(m.status, MaskStatus::Valid))
            {
                if let Ok(tile) = container.tile(&mask.id, 0, 0) {
                    if let Ok(plane) = MaskPlane::new(tile.width, tile.height, tile.values) {
                        planes.insert((copy.id.clone(), mask.id.clone()), plane);
                    }
                }
            }
        }
    }
    // Main render via the shared entry point (SourceActions → Adjustments →
    // Masks), with an empty source-action list until F-042-N1.
    let active_copy = document.virtual_copies[copy_index].clone();
    let render_output = render_frame(
        &frame,
        &RenderContext {
            recipe: &recipe,
            camera_white_balance: wb,
            source_actions: &[],
            masks: Some(MaskContext {
                copies: &document.virtual_copies,
                active_copy_id: &active_copy.id,
                planes,
                policy: MaskPolicy::Warn,
            }),
        },
    )?;
    for warning in &render_output.mask_warnings {
        eprintln!("warning: {warning}");
    }
    mask_warnings_out.extend(render_output.mask_warnings.iter().cloned());
    frame = render_output.frame;
    if args.match_total_exposure {
        recipe.auto_features.match_total_exposure = true;
        recipe.auto_features.target_luminance = args.target_luminance;
        // F-041: measure the final visible domain — `frame` is the render
        // result (already post crop/geometry) and `render_output.mask_layers`
        // are the effective planes resampled to exactly these dimensions. The
        // matching delta is weighted by the mask intersection; with no active
        // layers the empty slice keeps the previous raster measurement
        // bit-exactly. Until F-049 the layers do not modulate pixels, but the
        // measurement-domain semantics is already active.
        let mask_planes: Vec<MaskPlane> = render_output
            .mask_layers
            .iter()
            .map(|layer| layer.plane.clone())
            .collect();
        let matching = match_total_exposure_masked(&frame, args.target_luminance, &mask_planes)?;
        recipe.auto_features.matched_exposure = Some(matching);
        let total_exposure = (recipe.adjustments.get("exposure").copied().unwrap_or(0.0)
            + matching)
            .clamp(-10.0, 10.0);
        recipe.adjustments.insert("exposure".into(), total_exposure);
        frame.apply_recipe_with_white_balance(
            &lumina_sidecar::EditRecipe {
                adjustments: BTreeMap::from([(String::from("exposure"), matching)]),
                ..Default::default()
            },
            wb,
        )?;
    }
    let encoded = frame.encode(format)?;
    write_atomically(&args.output, &encoded)?;

    let copy = &mut document.virtual_copies[copy_index];
    copy.recipe = recipe.clone();
    copy.history.push(HistoryEntry {
        id: format!("h-{}", timestamp()),
        recipe,
        recorded_at: Some(timestamp()),
        extras: BTreeMap::new(),
    });
    save_sidecar(&sidecar_path, &document)?;
    Ok(())
}

fn inspect(args: InspectArgs) -> Result<(), CliError> {
    let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
    if is_raw_path(&args.input) {
        let image = lumina_raw::decode_bytes(
            &bytes,
            args.input
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("input.raw"),
        )?;
        println!(
            "raw: {}x{} orientation {}",
            image.metadata.width, image.metadata.height, image.metadata.orientation
        );
        println!(
            "camera: {} {}",
            image.metadata.camera_make.as_deref().unwrap_or("unknown"),
            image.metadata.camera_model.as_deref().unwrap_or("unknown")
        );
        println!(
            "iso: {:?}, shutter: {:?}, aperture: {:?}, lens: {:?}",
            image.metadata.iso,
            image.metadata.shutter,
            image.metadata.aperture,
            image.metadata.lens
        );
    }
    let path = sidecar_path_for(&args.input);
    match load_sidecar(&path) {
        Ok(document) => {
            println!("sidecar: valid ({})", path.display());
            println!("source: {}", document.source.relative_name);
            for copy in document.virtual_copies {
                println!("virtual-copy: {} [{}]", copy.name, copy.id);
                println!(
                    "auto-tone: {} matching: {} target-luminance: {}",
                    copy.recipe.auto_features.enable_auto_tone,
                    copy.recipe.auto_features.match_total_exposure,
                    copy.recipe.auto_features.target_luminance
                );
            }
        }
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            println!("sidecar: missing ({})", path.display());
            println!("virtual-copy: Original [vc-original] (default)");
        }
        Err(error) => {
            println!("sidecar: invalid ({})", path.display());
            return Err(error.into());
        }
    }
    Ok(())
}

fn is_raw_path(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    [
        "arw", "cr2", "cr3", "dng", "nef", "orf", "raf", "rw2", "crw", "pef", "srw", "3fr", "iiq",
        "rwl", "mos", "erf", "kdc", "x3f",
    ]
    .contains(&extension.as_str())
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
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Ok(ImageFileFormat::Png),
        "jpg" | "jpeg" => Ok(ImageFileFormat::Jpeg),
        "webp" => Ok(ImageFileFormat::WebP),
        extension => Err(CliError::Message(format!(
            "unsupported output extension `.{extension}`; use png, jpg, jpeg, or webp"
        ))),
    }
}

fn validate_format(format: &str) -> Result<(), CliError> {
    match format.to_ascii_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "webp" => Ok(()),
        _ => Err(CliError::Message(format!(
            "unsupported format `{format}`; use png, jpg, jpeg, or webp"
        ))),
    }
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
            version: env!("CARGO_PKG_VERSION").into(),
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
    let input = fs::canonicalize(input).map_err(|error| io_error(input, error))?;
    let output = if output.exists() {
        fs::canonicalize(output).map_err(|error| io_error(output, error))?
    } else {
        let parent = output.parent().unwrap_or_else(|| Path::new("."));
        fs::canonicalize(parent)
            .map(|parent| parent.join(output.file_name().unwrap_or_default()))
            .map_err(|error| io_error(parent, error))?
    };
    if input == output {
        return Err(CliError::Message(
            "input and output resolve to the same path; refusing to overwrite the original".into(),
        ));
    }
    Ok(())
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| CliError::Message("output must have a valid file name".into()))?;
    let mut temporary = tempfile::Builder::new()
        .prefix(&format!(".{name}.tmp-"))
        .tempfile_in(parent)
        .map_err(|error| io_error(parent, error))?;
    let temporary_path = temporary.path().to_path_buf();
    let result = (|| {
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
        temporary
            .persist(path)
            .map_err(|error| io_error(path, error.error))?;
        Ok(())
    })();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_process_arguments() {
        let cli = Cli::try_parse_from([
            "lumina",
            "process",
            "--input",
            "a.png",
            "--output",
            "b.webp",
            "--exposure",
            "1",
            "--highlights=-0.25",
            "--shadows",
            "0.4",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Process(ProcessArgs {
                exposure: Some(1.0),
                highlights: Some(-0.25),
                shadows: Some(0.4),
                ..
            })
        ));
    }

    #[test]
    fn export_accepts_update_masks_before_export() {
        let cli = Cli::try_parse_from([
            "lumina",
            "export",
            "--input",
            "a.png",
            "--output",
            "b.png",
            "--update-masks",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Export(ExportArgs {
                update_masks: true,
                ..
            })
        ));
    }

    #[test]
    fn recognizes_supported_raw_extensions() {
        for extension in ["ARW", "crw", "pef", "3fr", "x3f"] {
            assert!(is_raw_path(Path::new(&format!("photo.{extension}"))));
        }
    }

    #[test]
    fn rejects_identical_and_alias_paths_before_processing() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        fs::write(&input, [1, 2, 3]).unwrap();
        assert!(reject_same_path(&input, &input).is_err());
        assert!(reject_same_path(&input, &directory.path().join("./input.png")).is_err());
        assert!(reject_same_path(&input, &directory.path().join("input.png")).is_err());
    }

    #[test]
    fn changed_source_is_rejected_without_overwriting_output() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let output = directory.path().join("output.png");
        let frame = ImageFrame::new(1, 1, vec![20, 30, 40, 255]).unwrap();
        fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
        process(ProcessArgs {
            input: input.clone(),
            output: output.clone(),
            preset: None,
            exposure: None,
            contrast: None,
            highlights: None,
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
        })
        .unwrap();
        let changed = ImageFrame::new(1, 1, vec![21, 30, 40, 255]).unwrap();
        fs::write(&input, changed.encode(ImageFileFormat::Png).unwrap()).unwrap();
        let sentinel = fs::read(&output).unwrap();
        let error = process(ProcessArgs {
            input,
            output: output.clone(),
            preset: None,
            exposure: None,
            contrast: None,
            highlights: None,
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
        })
        .unwrap_err();
        assert!(error.to_string().contains("source changed"));
        assert_eq!(fs::read(output).unwrap(), sentinel);
    }

    #[test]
    fn invalid_adjustment_and_unknown_key_are_cli_errors_without_output() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let output = directory.path().join("output.png");
        let frame = ImageFrame::new(1, 1, vec![20, 30, 40, 255]).unwrap();
        fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
        let invalid = process(ProcessArgs {
            input: input.clone(),
            output: output.clone(),
            preset: None,
            exposure: Some(f64::INFINITY),
            contrast: None,
            highlights: None,
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
        })
        .unwrap_err();
        assert!(invalid.to_string().contains("invalid exposure"));
        assert!(!output.exists());

        let preset_path = directory.path().join("unknown.json");
        let preset = Preset {
            id: "unknown".into(),
            name: "Unknown".into(),
            recipe: lumina_sidecar::EditRecipe {
                adjustments: BTreeMap::from([("clarity".into(), 0.5)]),
                ..Default::default()
            },
            extras: BTreeMap::new(),
        };
        fs::write(&preset_path, serde_json::to_vec(&preset).unwrap()).unwrap();
        let unknown = process(ProcessArgs {
            input,
            output: output.clone(),
            preset: Some(preset_path),
            exposure: None,
            contrast: None,
            highlights: None,
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
        })
        .unwrap_err();
        assert!(unknown
            .to_string()
            .contains("unsupported adjustment `clarity`"));
        assert!(!output.exists());
    }

    #[test]
    fn cli_rejects_non_finite_and_out_of_range_adjustments() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let frame = ImageFrame::new(1, 1, vec![20, 30, 40, 255]).unwrap();
        fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
        for (name, values) in [
            (
                "exposure",
                [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -10.1, 10.1],
            ),
            (
                "contrast",
                [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.1, 1.1],
            ),
            (
                "highlights",
                [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.1, 1.1],
            ),
            (
                "shadows",
                [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.1, 1.1],
            ),
        ] {
            for value in values {
                let output = directory.path().join(format!("{name}-{value:?}.png"));
                let error = process(ProcessArgs {
                    input: input.clone(),
                    output: output.clone(),
                    preset: None,
                    exposure: (name == "exposure").then_some(value),
                    contrast: (name == "contrast").then_some(value),
                    highlights: (name == "highlights").then_some(value),
                    shadows: (name == "shadows").then_some(value),
                    auto_tone: false,
                    match_total_exposure: false,
                    target_luminance: 0.5,
                })
                .unwrap_err();
                assert!(error.to_string().contains(&format!("invalid {name}")));
                assert!(!output.exists());
            }
        }
    }

    #[test]
    fn cli_accepts_both_adjustment_boundaries() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let frame = ImageFrame::new(1, 1, vec![20, 30, 40, 255]).unwrap();
        fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
        for (name, values) in [
            ("exposure", [-10.0, 10.0]),
            ("contrast", [-1.0, 1.0]),
            ("highlights", [-1.0, 1.0]),
            ("shadows", [-1.0, 1.0]),
        ] {
            for (index, value) in values.into_iter().enumerate() {
                process(ProcessArgs {
                    input: input.clone(),
                    output: directory.path().join(format!("{name}-{index}.png")),
                    preset: None,
                    exposure: (name == "exposure").then_some(value),
                    contrast: (name == "contrast").then_some(value),
                    highlights: (name == "highlights").then_some(value),
                    shadows: (name == "shadows").then_some(value),
                    auto_tone: false,
                    match_total_exposure: false,
                    target_luminance: 0.5,
                })
                .unwrap();
            }
        }
    }

    #[test]
    fn preset_process_and_inspect_use_sidecar() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let output = directory.path().join("output.webp");
        let preset_path = directory.path().join("preset.json");
        let frame = ImageFrame::new(1, 1, vec![20, 30, 40, 255]).unwrap();
        fs::write(&input, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();
        let preset = Preset {
            id: "test".into(),
            name: "Bright".into(),
            recipe: lumina_sidecar::EditRecipe {
                adjustments: BTreeMap::from([("exposure".into(), 1.0)]),
                ..lumina_sidecar::EditRecipe::default()
            },
            extras: BTreeMap::new(),
        };
        fs::write(&preset_path, serde_json::to_vec(&preset).unwrap()).unwrap();
        process(ProcessArgs {
            input: input.clone(),
            output: output.clone(),
            preset: Some(preset_path),
            exposure: Some(0.0),
            contrast: None,
            highlights: None,
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
        })
        .unwrap();
        assert!(output.exists());
        let sidecar = load_sidecar(&sidecar_path_for(&input)).unwrap();
        assert_eq!(
            sidecar.virtual_copies[0].recipe.adjustments["exposure"],
            0.0
        );
        assert_eq!(sidecar.virtual_copies[0].history.len(), 1);
        inspect(InspectArgs { input }).unwrap();
    }

    fn valid_mask_definition(
        id: &str,
        operation: lumina_sidecar::MaskOperation,
        references: Vec<lumina_sidecar::MaskReference>,
    ) -> lumina_sidecar::MaskDefinition {
        use lumina_sidecar::{
            CoordinateSystem, DecodeFingerprint, Extras, GeometryFingerprint, ModelIdentity,
            Preprocessing, Resolution, SourceFingerprint,
        };
        lumina_sidecar::MaskDefinition {
            id: id.into(),
            name: id.into(),
            source_fingerprint: SourceFingerprint {
                content_hash: "h".into(),
                byte_length: 1,
                extras: Extras::new(),
            },
            decode_context: DecodeFingerprint {
                decoder: "d".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            geometry_context: GeometryFingerprint {
                width: 2,
                height: 2,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: Extras::new(),
            },
            model: ModelIdentity {
                name: "m".into(),
                version: "1".into(),
                hash: "h".into(),
                extras: Extras::new(),
            },
            inference_resolution: Resolution {
                width: 2,
                height: 2,
                extras: Extras::new(),
            },
            preprocessing: Preprocessing {
                name: "p".into(),
                version: "1".into(),
                parameters: BTreeMap::new(),
                extras: Extras::new(),
            },
            rescaling_method: "none".into(),
            rescaling_parameters: BTreeMap::new(),
            coordinate_system: CoordinateSystem::SourceOriented,
            status: MaskStatus::Valid,
            created_at: "now".into(),
            generator_version: "g".into(),
            error_text: None,
            artifact: None,
            operation,
            references,
            extras: Extras::new(),
        }
    }

    fn write_sidecar_with_valid_layer(
        input: &Path,
        bytes: &[u8],
        frame: &ImageFrame,
    ) -> lumina_sidecar::SidecarDocument {
        let mut document = SidecarDocument::new(
            source_identity(input, bytes, frame, None).unwrap(),
            "raster-mvp-1",
        );
        let copy = &mut document.virtual_copies[0];
        copy.mask_library = vec![valid_mask_definition(
            "subject",
            lumina_sidecar::MaskOperation::Source,
            vec![],
        )];
        copy.mask_layers = vec![lumina_sidecar::MaskLayer {
            id: "layer-1".into(),
            mask: lumina_sidecar::MaskReference {
                copy_id: copy.id.clone(),
                mask_id: "subject".into(),
                extras: BTreeMap::new(),
            },
            inverted: false,
            feather: 0.0,
            blur: 0.0,
            density: 1.0,
            extras: BTreeMap::new(),
        }];
        save_sidecar(&sidecar_path_for(input), &document).unwrap();
        document
    }

    #[test]
    fn render_with_valid_mask_zdata_has_no_warning() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let output = directory.path().join("output.png");
        let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
        let bytes = frame.encode(ImageFileFormat::Png).unwrap();
        fs::write(&input, &bytes).unwrap();
        write_sidecar_with_valid_layer(&input, &bytes, &frame);

        // Provide a 2x2 fully-filled artifact plane for `subject`.
        let tile = lumina_sidecar::MaskTile {
            mask_id: "subject".into(),
            tile_x: 0,
            tile_y: 0,
            width: 2,
            height: 2,
            values: vec![65535; 4],
        };
        let container = lumina_sidecar::ZDataContainer::new(vec![tile]).unwrap();
        lumina_sidecar::save_zdata(&lumina_sidecar::zdata_path_for(&input), &container).unwrap();

        let mut warnings = Vec::new();
        process_selected(
            ProcessArgs {
                input: input.clone(),
                output: output.clone(),
                preset: None,
                exposure: None,
                contrast: None,
                highlights: None,
                shadows: None,
                auto_tone: false,
                match_total_exposure: false,
                target_luminance: 0.5,
            },
            None,
            &mut warnings,
        )
        .unwrap();
        assert!(output.is_file());
        assert!(warnings.is_empty());
    }

    #[test]
    fn render_with_missing_mask_zdata_warns_but_succeeds() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let output = directory.path().join("output.png");
        let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
        let bytes = frame.encode(ImageFileFormat::Png).unwrap();
        fs::write(&input, &bytes).unwrap();
        write_sidecar_with_valid_layer(&input, &bytes, &frame);
        // No zdata file on purpose: the layer is reported as unavailable.

        let mut warnings = Vec::new();
        process_selected(
            ProcessArgs {
                input: input.clone(),
                output: output.clone(),
                preset: None,
                exposure: None,
                contrast: None,
                highlights: None,
                shadows: None,
                auto_tone: false,
                match_total_exposure: false,
                target_luminance: 0.5,
            },
            None,
            &mut warnings,
        )
        .unwrap();
        assert!(output.is_file());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("layer-1"));
    }

    // ---- F-085: history steps and mask × matching interplay ----
    //
    // Documented boundary (F-042-N1): the CLI still passes an empty
    // source-action list (`source_actions: &[]` in `process_selected`).
    // Source actions reach the CLI only with F-042-N1 (persistence +
    // CLI command); no CLI source-action test is written yet.

    #[test]
    fn history_entry_stores_final_recipe_and_snapshot_reproduces_output() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let output = directory.path().join("output.png");
        let frame = ImageFrame::new(2, 2, vec![100; 16]).unwrap();
        let bytes = frame.encode(ImageFileFormat::Png).unwrap();
        fs::write(&input, &bytes).unwrap();
        process(ProcessArgs {
            input: input.clone(),
            output: output.clone(),
            preset: None,
            exposure: Some(0.5),
            contrast: Some(-0.2),
            highlights: Some(0.1),
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
        })
        .unwrap();
        assert!(output.is_file());

        let sidecar = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let copy = &sidecar.virtual_copies[0];
        // Exactly one new history entry; its recipe snapshot is the final
        // recipe of the process run. `assert_eq` on EditRecipe covers all
        // relevant fields (adjustments, auto_features, nested stages).
        assert_eq!(copy.history.len(), 1);
        let entry = &copy.history[0];
        assert_eq!(entry.recipe, copy.recipe);
        assert_eq!(entry.recipe.adjustments["exposure"], 0.5);
        assert_eq!(entry.recipe.adjustments["contrast"], -0.2);
        assert_eq!(entry.recipe.adjustments["highlights"], 0.1);
        assert!(!entry.recipe.auto_features.enable_auto_tone);
        assert!(!entry.recipe.auto_features.match_total_exposure);
        assert!(entry.recorded_at.is_some());

        // Snapshot reproducibility: applying the stored recipe alone to the
        // original frame reproduces the process output byte-identically (PNG
        // is lossless and the encoder is deterministic), plus a decoded-pixel
        // cross-check.
        let source = ImageFrame::decode(&fs::read(&input).unwrap()).unwrap();
        let rendered = render_frame(
            &source,
            &RenderContext {
                recipe: &entry.recipe,
                camera_white_balance: None,
                source_actions: &[],
                masks: None,
            },
        )
        .unwrap();
        let expected = fs::read(&output).unwrap();
        assert_eq!(
            rendered.frame.encode(ImageFileFormat::Png).unwrap(),
            expected
        );
        assert_eq!(ImageFrame::decode(&expected).unwrap(), rendered.frame);
    }

    #[test]
    fn valid_mask_with_match_total_exposure_measures_masked_domain() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.png");
        let output = directory.path().join("output.png");
        // 8x8 bimodal gray frame: left half (pixels 0..32) is 200, right half
        // (pixels 32..64) is 60. Unmasked mean = 130/255 ~= 0.51.
        let mut pixels = Vec::with_capacity(8 * 8 * 4);
        for index in 0..64 {
            let value = if index < 32 { 200u8 } else { 60u8 };
            pixels.extend_from_slice(&[value, value, value, 255]);
        }
        let frame = ImageFrame::new(8, 8, pixels).unwrap();
        let bytes = frame.encode(ImageFileFormat::Png).unwrap();
        fs::write(&input, &bytes).unwrap();
        write_sidecar_with_valid_layer(&input, &bytes, &frame);

        // Valid artifact plane for `subject` at frame resolution. F-042 does
        // not modulate pixels yet (pixel modulation is F-049), but F-041
        // already weights the measurement domain: the bright left half is
        // fully masked (0), the dark right half fully visible (u16::MAX).
        //   weighted mean: 60/255 ~= 0.2353
        //   masked delta:  log2(0.5 / (60/255)) = log2(2.125) ~= 1.08746
        //   unmasked delta: log2(0.5 / (130/255)) ~= -0.0280
        let tile = lumina_sidecar::MaskTile {
            mask_id: "subject".into(),
            tile_x: 0,
            tile_y: 0,
            width: 8,
            height: 8,
            values: (0..64).map(|i| if i < 32 { 0 } else { 65535 }).collect(),
        };
        let container = lumina_sidecar::ZDataContainer::new(vec![tile]).unwrap();
        lumina_sidecar::save_zdata(&lumina_sidecar::zdata_path_for(&input), &container).unwrap();

        let unmasked = match_total_exposure_masked(&frame, 0.5, &[]).unwrap();
        let mut warnings = Vec::new();
        process_selected(
            ProcessArgs {
                input: input.clone(),
                output: output.clone(),
                preset: None,
                exposure: None,
                contrast: None,
                highlights: None,
                shadows: None,
                auto_tone: false,
                match_total_exposure: true,
                target_luminance: 0.5,
            },
            None,
            &mut warnings,
        )
        .unwrap();
        assert!(output.is_file());
        assert!(
            warnings.is_empty(),
            "valid mask must not warn: {warnings:?}"
        );

        // The persisted matching result follows the masked measurement domain
        // and demonstrably differs from the unmasked result (F-041).
        let sidecar = load_sidecar(&sidecar_path_for(&input)).unwrap();
        let auto = &sidecar.virtual_copies[0].recipe.auto_features;
        assert!(auto.match_total_exposure);
        let matched = auto.matched_exposure.unwrap();
        assert!(
            (matched - 1.08746).abs() < 0.001,
            "persisted matched exposure {matched}"
        );
        assert!(
            (matched - unmasked).abs() > 1.0,
            "masked delta {matched} must differ from unmasked {unmasked}"
        );

        // Applying the delta reaches the *masked* target: the visible (right)
        // half of the exported frame (60 * 2^1.08746 = 60 * 2.125 = 127.5 ->
        // 128, mean ~= 0.502) is within tolerance, while the masked-out left
        // half clamps at 255 and must not be part of the target check.
        let rendered = ImageFrame::decode(&fs::read(&output).unwrap()).unwrap();
        let visible_mean = rendered
            .pixels
            .chunks_exact(4)
            .enumerate()
            .filter(|(index, _)| *index >= 32)
            .map(|(_, pixel)| {
                (0.2126 * f64::from(pixel[0])
                    + 0.7152 * f64::from(pixel[1])
                    + 0.0722 * f64::from(pixel[2]))
                    / 255.0
            })
            .sum::<f64>()
            / 32.0;
        assert!(
            (visible_mean - 0.5).abs() <= 0.02,
            "post-match visible mean {visible_mean} not within 0.02 of target 0.5"
        );
    }
}
