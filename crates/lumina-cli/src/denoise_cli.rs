//! CLI orchestration for the KI-Denoise status, render, and RGB-record actions.
//!
//! Extracted verbatim from `main.rs` for the file-size ratchet (SPLICE-CLI-MAIN-89,
//! denoise slice). The command arguments and entry point are crate-visible to
//! the CLI root; the implementation and resolution types remain module-private.

use super::*;
use log::info;

/// CLI-facing KI-Denoise fallback policy (R2): `warn` is the §6-Exit-0 default
/// (visible stderr warning, manual F-096 fallback), `strict` aborts loudly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliDenoisePolicy {
    Warn,
    Strict,
}

impl CliDenoisePolicy {
    fn to_policy(self) -> DenoisePolicy {
        match self {
            Self::Warn => DenoisePolicy::Warn,
            Self::Strict => DenoisePolicy::Strict,
        }
    }
}

/// KI-Denoise stage consumer. Exactly one action may be requested:
/// `--status` (default, read-only), `--render` or `--record-rgb`. Combining
/// them is a usage error (exit 2), never a silent precedence pick.
#[derive(Debug, Args)]
pub(crate) struct DenoiseArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    virtual_copy: Option<String>,
    /// Report the KI-Denoise stage status and exit (read-only; the default
    /// when neither `--render` nor `--record-rgb` is given).
    #[arg(long)]
    status: bool,
    /// Render through the KI-Denoise-aware pipeline to `--output`.
    #[arg(long)]
    render: bool,
    #[arg(long)]
    output: Option<PathBuf>,
    /// Output format for `--render` (`png|jpg|jpeg|webp`). Drives the output
    /// extension and the encoder, exactly like `render --format` (M1).
    #[arg(long, default_value = "png")]
    format: String,
    /// Encoder quality for `--render` (`1..=100`).
    #[arg(long, default_value_t = 90)]
    quality: u8,
    /// Explicit producer: import an externally produced denoised RGB image
    /// (PNG/JPEG/WebP, same geometry as the source) and persist it as the
    /// `denoise_rgb` bundle record plus the recipe/artifact reference.
    #[arg(long, value_name = "RGB")]
    record_rgb: Option<PathBuf>,
    /// Explicit re-record: replace an existing `denoise_rgb` record.
    #[arg(long)]
    force: bool,
    /// Model name for `--record-rgb` (identity-bearing).
    #[arg(long)]
    model_name: Option<String>,
    /// Model version for `--record-rgb` (identity-bearing).
    #[arg(long)]
    model_version: Option<String>,
    /// Model hash for `--record-rgb` (`sha256:<64 hex>` or
    /// `pending-integration`).
    #[arg(long)]
    model_hash: Option<String>,
    /// Input-spec digest for `--record-rgb` (`sha256:<64 hex>`).
    #[arg(long)]
    input_spec_digest: Option<String>,
    #[arg(long, default_value_t = 1.0)]
    strength: f32,
    #[arg(long, default_value_t = 0.0)]
    preserve_detail: f32,
    #[arg(long, value_enum, default_value = "warn")]
    denoise_policy: CliDenoisePolicy,
    #[arg(long)]
    json: bool,
}

/// Resolved view of the KI-Denoise stage (status + verified artifact + the
/// identity fields reported to the user).
struct DenoiseResolution {
    status: DenoiseStageStatus,
    reason: String,
    artifact: Option<CoreDenoiseRgbArtifact>,
    model_name: String,
    model_version: String,
    model_hash: String,
    input_spec_digest: String,
    artifact_checksum: Option<String>,
}

impl DenoiseResolution {
    fn inactive() -> Self {
        Self {
            status: DenoiseStageStatus::Inactive,
            reason: String::new(),
            artifact: None,
            model_name: String::new(),
            model_version: String::new(),
            model_hash: String::new(),
            input_spec_digest: String::new(),
            artifact_checksum: None,
        }
    }
}

/// String form of the live decode context used by the §6 identity comparison.
/// Producer (`--record-rgb`) and consumer (`--status`/`--render`) both derive
/// it from `SidecarDocument::source`, so a moved or re-decoded source is
/// detectable.
fn denoise_decode_fingerprint(source: &SourceIdentity) -> String {
    format!(
        "{}:{}:{}x{}",
        source.decode_fingerprint.decoder,
        source.decode_fingerprint.version,
        source.geometry_fingerprint.width,
        source.geometry_fingerprint.height
    )
}

/// Deterministic bundle record id for a `denoise_rgb` checksum (content
/// derived, never positional — same convention as the generative records).
pub(crate) fn denoise_record_id(checksum: &str) -> String {
    let prefix = checksum.get(..16).unwrap_or(checksum);
    format!("denoise_rgb:{prefix}")
}

pub(crate) fn denoise_status_reason(status: DenoiseStageStatus, zdata: &Path) -> String {
    match status {
        DenoiseStageStatus::Inactive | DenoiseStageStatus::Ready => String::new(),
        DenoiseStageStatus::Unavailable => {
            "model hash is `pending-integration` (no licence-pinned weights)".into()
        }
        DenoiseStageStatus::Missing => {
            format!("no `denoise_rgb` record in `{}`", zdata.display())
        }
        DenoiseStageStatus::Corrupt => {
            "artifact checksum does not match the recipe reference".into()
        }
        DenoiseStageStatus::Stale => {
            "source/decode/model/input-spec or the persisted producer provenance changed".into()
        }
    }
}

/// Classifies the §6 status of the active `denoise_ai` stage from the recipe
/// and the `.lumina.zdata` bundle. Pure read; never writes and never
/// recomputes.
fn denoise_resolution(
    input: &Path,
    document: &SidecarDocument,
    recipe: &EditRecipe,
) -> Result<DenoiseResolution, CliError> {
    let Some(denoise) = recipe.denoise_ai.as_ref() else {
        return Ok(DenoiseResolution::inactive());
    };
    let mut resolution = DenoiseResolution {
        status: DenoiseStageStatus::Inactive,
        reason: String::new(),
        artifact: None,
        model_name: denoise.model.name.clone(),
        model_version: denoise.model.version.clone(),
        model_hash: denoise.model.model_hash.clone(),
        input_spec_digest: denoise.input_spec_digest.clone(),
        artifact_checksum: None,
    };
    if denoise.is_identity() {
        return Ok(resolution);
    }

    let zdata = zdata_path_for(input);
    let mut artifact_present = false;
    let mut artifact_checksum = String::new();
    let mut artifact: Option<CoreDenoiseRgbArtifact> = None;
    let mut corrupt_reason: Option<String> = None;
    if zdata.exists() {
        match load_zdata(&zdata) {
            Ok(container) => match container.decode_all() {
                Ok(records) => {
                    for record in records {
                        if let RecordSpec::DenoiseRgb(record) = record {
                            let checksum = record.checksum();
                            let matches = denoise
                                .artifact
                                .as_ref()
                                .is_some_and(|reference| reference.checksum == checksum);
                            if !artifact_present || matches {
                                artifact_present = true;
                                artifact_checksum = checksum;
                                artifact = CoreDenoiseRgbArtifact::new(
                                    record.width,
                                    record.height,
                                    record.pixels,
                                )
                                .ok();
                            }
                            if matches {
                                break;
                            }
                        }
                    }
                }
                Err(error) => corrupt_reason = Some(error.to_string()),
            },
            Err(error) => corrupt_reason = Some(error.to_string()),
        }
    }
    if let Some(error) = corrupt_reason {
        resolution.status = DenoiseStageStatus::Corrupt;
        resolution.reason = format!(
            "`denoise_rgb` bundle `{}` is unreadable or corrupt: {error}",
            zdata.display()
        );
        return Ok(resolution);
    }

    // R1: `None` producer provenance is the default identity and is therefore
    // loudly `stale`, never silently `ready`.
    let current = DenoiseIdentity {
        source_content_hash: document.source.content_hash.clone(),
        decode_fingerprint: denoise_decode_fingerprint(&document.source),
        model_name: denoise.model.name.clone(),
        model_version: denoise.model.version.clone(),
        model_hash: denoise.model.model_hash.clone(),
        input_spec_digest: denoise.input_spec_digest.clone(),
        artifact_checksum: artifact_checksum.clone(),
    };
    let recorded = denoise_producer_provenance(denoise).unwrap_or_default();
    let status = resolve_denoise_status(denoise, &current, &recorded, artifact_present);
    resolution.reason = denoise_status_reason(status, &zdata);

    if status == DenoiseStageStatus::Ready {
        // A ready stage must describe exactly this frame; a dimension mismatch
        // is corrupt (loud), never a silent drop.
        match artifact.as_ref() {
            Some(artifact)
                if artifact.width != document.source.geometry_fingerprint.width
                    || artifact.height != document.source.geometry_fingerprint.height =>
            {
                resolution.status = DenoiseStageStatus::Corrupt;
                resolution.reason = format!(
                    "denoise artifact {}x{} does not match the source geometry {}x{}",
                    artifact.width,
                    artifact.height,
                    document.source.geometry_fingerprint.width,
                    document.source.geometry_fingerprint.height
                );
                return Ok(resolution);
            }
            Some(_) => {}
            None => {
                resolution.status = DenoiseStageStatus::Corrupt;
                resolution.reason =
                    "a `denoise_rgb` record was found but its payload is unusable".into();
                return Ok(resolution);
            }
        }
    }

    resolution.status = status;
    resolution.artifact = artifact;
    resolution.artifact_checksum = (!artifact_checksum.is_empty()).then_some(artifact_checksum);
    Ok(resolution)
}

pub(crate) fn denoise(args: DenoiseArgs) -> Result<(), CliError> {
    // F5: `--status`, `--render` and `--record-rgb` are three alternative
    // actions. More than one used to win silently by hard-coded precedence
    // (`--record-rgb` > `--render` > `--status`), so a mistyped invocation
    // could render or record when the caller only wanted a status report.
    // Reject the combination as a usage error (exit 2) before any work.
    let action_count = usize::from(args.status)
        + usize::from(args.render)
        + usize::from(args.record_rgb.is_some());
    if action_count > 1 {
        return Err(CliError::Usage(
            "--status, --render and --record-rgb are mutually exclusive; pick exactly one action"
                .into(),
        ));
    }
    let sidecar = sidecar_path_for(&args.input);
    let mut document = load_sidecar(&sidecar)?;
    let copy_index = active_copy_index(&document, args.virtual_copy.as_deref())?;

    if let Some(rgb) = args.record_rgb.clone() {
        return denoise_record_rgb(&args, &mut document, copy_index, &rgb, &sidecar);
    }

    let recipe = document.virtual_copies[copy_index].recipe.clone();
    let resolution = denoise_resolution(&args.input, &document, &recipe)?;

    if args.render {
        // The denoise-aware entry point takes the whole `RenderContext`; mask
        // layers are resolved by the shared render path, which is deliberately
        // not duplicated here. A masked recipe is refused loudly instead of
        // silently rendering without its masks.
        if !document.virtual_copies[copy_index].mask_layers.is_empty() {
            return Err(CliError::Message(
                "denoise --render does not resolve mask layers; masked denoise rendering \
                 awaits the shared-path wiring (no silent render without masks)"
                    .into(),
            ));
        }
        return denoise_render(&args, &recipe, &resolution);
    }

    let status = resolution.status.as_str();
    let payload = serde_json::json!({
        "command": "denoise",
        "action": "status",
        "input": args.input,
        "virtual_copy": document.virtual_copies[copy_index].id,
        "status": status,
        "reason": resolution.reason,
        "model": {
            "name": resolution.model_name,
            "version": resolution.model_version,
            "model_hash": resolution.model_hash,
            "input_spec_digest": resolution.input_spec_digest,
        },
        "artifact_checksum": resolution.artifact_checksum,
    });
    let text = if resolution.reason.is_empty() {
        format!("denoise_ai {status}")
    } else {
        format!("denoise_ai {status}: {}", resolution.reason)
    };
    emit(args.json, payload, &text)?;
    if resolution.status == DenoiseStageStatus::Corrupt {
        return Err(CliError::Message(
            "denoise_ai artifact is corrupt; explicit re-record/re-inference required (never automatic)"
                .into(),
        ));
    }
    if matches!(
        resolution.status,
        DenoiseStageStatus::Stale | DenoiseStageStatus::Missing | DenoiseStageStatus::Unavailable
    ) {
        // §6/R2: visible stderr warning, exit 0 (the render falls back to the
        // manual F-096 noise reduction). `warn!` also reaches the CLI logger.
        eprintln!(
            "warning: denoise_ai is {status}: {}; render falls back to manual noise reduction \
             (F-096) — visible, not silent",
            resolution.reason
        );
    }
    Ok(())
}

fn denoise_render(
    args: &DenoiseArgs,
    recipe: &EditRecipe,
    resolution: &DenoiseResolution,
) -> Result<(), CliError> {
    let output = args
        .output
        .clone()
        .ok_or_else(|| CliError::Message("denoise --render requires --output".into()))?;
    // M1: honor `--format`/`--quality` exactly like the neighboring `render`
    // command — the requested format drives the output extension AND the
    // encoder options, instead of being validated and then discarded while the
    // format is guessed from the output extension.
    let format = parse_output_format(&args.format)?;
    validate_quality(args.quality)?;
    let output = output.with_extension(format_extension(&args.format));
    reject_protected_output(&args.input, &output)?;
    let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
    let (frame, raw) = decode_input(&args.input, &bytes)?;
    let source_actions = resolve_source_actions(recipe, &zdata_path_for(&args.input))?;
    let camera_white_balance = raw
        .as_ref()
        .and_then(|metadata| sanitize_camera_white_balance(metadata.camera_white_balance));

    let policy = args.denoise_policy.to_policy();
    let stage_input = match resolution.status {
        DenoiseStageStatus::Ready => match resolution.artifact.as_ref() {
            Some(artifact) => DenoiseStageInput::ready(artifact).with_policy(policy),
            None => DenoiseStageInput::non_ready(
                DenoiseStageStatus::Corrupt,
                "denoise_ai resolved `ready` but carried no artifact",
            )
            .with_policy(policy),
        },
        other => DenoiseStageInput::non_ready(other, resolution.reason.clone()).with_policy(policy),
    };
    let render_ctx = RenderContext {
        recipe,
        camera_white_balance,
        source_actions: &source_actions,
        masks: None,
        lensfun: None,
        depth: None,
    };
    let rendered = render_frame_with_denoise(&frame, &render_ctx, &stage_input)?;
    if resolution.status != DenoiseStageStatus::Ready {
        eprintln!(
            "warning: denoise_ai is {}: {}; rendered with the manual noise-reduction fallback \
             (F-096) — visible, not silent",
            resolution.status.as_str(),
            resolution.reason
        );
    }
    let options = ExportOptions {
        format,
        quality: args.quality,
        dither: false,
        ..Default::default()
    };
    write_atomically(&output, &rendered.frame.encode_with_options(options)?)?;
    emit(
        args.json,
        serde_json::json!({
            "command": "denoise",
            "action": "render",
            "output": output,
            "format": args.format,
            "quality": args.quality,
            "status": resolution.status.as_str(),
            "reason": resolution.reason,
            "policy": match policy {
                DenoisePolicy::Warn => "warn",
                DenoisePolicy::Strict => "strict",
            },
        }),
        "rendered",
    )
}

fn denoise_record_rgb(
    args: &DenoiseArgs,
    document: &mut SidecarDocument,
    copy_index: usize,
    rgb_path: &Path,
    sidecar: &Path,
) -> Result<(), CliError> {
    let require = |value: &Option<String>, flag: &str| {
        value
            .clone()
            .ok_or_else(|| CliError::Message(format!("denoise --record-rgb requires --{flag}")))
    };
    let model_name = require(&args.model_name, "model-name")?;
    let model_version = require(&args.model_version, "model-version")?;
    let model_hash = require(&args.model_hash, "model-hash")?;
    let input_spec_digest = require(&args.input_spec_digest, "input-spec-digest")?;

    let source_bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
    let (frame, _raw) = decode_input(&args.input, &source_bytes)?;
    let rgb_bytes = fs::read(rgb_path).map_err(|error| io_error(rgb_path, error))?;
    let rgb_frame = ImageFrame::decode(&rgb_bytes)?;
    if rgb_frame.width() != frame.width() || rgb_frame.height() != frame.height() {
        return Err(CliError::Message(format!(
            "--record-rgb artifact is {}x{} but the source is {}x{}; a denoise artifact must \
             describe exactly the source geometry",
            rgb_frame.width(),
            rgb_frame.height(),
            frame.width(),
            frame.height()
        )));
    }
    let mut pixels = Vec::with_capacity(rgb_frame.pixels().len() / 4 * 3);
    for pixel in rgb_frame.pixels().as_chunks::<4>().0 {
        pixels.extend_from_slice(&pixel[..3]);
    }
    let core_artifact = CoreDenoiseRgbArtifact::new(rgb_frame.width(), rgb_frame.height(), pixels)?;
    let checksum = core_artifact.checksum();
    let record_id = denoise_record_id(&checksum);
    let zdata = zdata_path_for(&args.input);
    let record = SidecarDenoiseRgbArtifact {
        id: record_id.clone(),
        width: core_artifact.width,
        height: core_artifact.height,
        pixels: core_artifact.pixels.clone(),
    };
    save_denoise_rgb(&zdata, record, args.force).map_err(|error| {
        CliError::Message(format!(
            "could not write `denoise_rgb` bundle `{}`: {error}",
            zdata.display()
        ))
    })?;

    let relative_path = zdata
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("bundle{ZDATA_FILE_SUFFIX}"));
    let mut denoise = DenoiseAi {
        version: DENOISE_AI_VERSION,
        enabled: true,
        model: DenoiseModelIdentity {
            name: model_name.clone(),
            version: model_version.clone(),
            model_hash: model_hash.clone(),
            extras: Default::default(),
        },
        input_spec_digest: input_spec_digest.clone(),
        strength: args.strength,
        preserve_detail: args.preserve_detail,
        artifact: Some(DenoiseArtifactRef {
            kind: DenoiseArtifactKind::DenoiseRgb,
            relative_path,
            format: "lumina-zdata".into(),
            checksum: checksum.clone(),
            width: core_artifact.width,
            height: core_artifact.height,
            channels: "rgb8".into(),
            data_version: "1".into(),
            extras: Default::default(),
        }),
        extras: Default::default(),
    };
    denoise.validate()?;
    // R1/B2: persist the producer identity (source/decode/model/input-spec +
    // artifact checksum) so a later consumer proves the artifact belongs to
    // exactly this context; an absent provenance is loudly `stale`.
    set_denoise_producer_provenance(
        &mut denoise,
        &DenoiseIdentity {
            source_content_hash: document.source.content_hash.clone(),
            decode_fingerprint: denoise_decode_fingerprint(&document.source),
            model_name,
            model_version,
            model_hash,
            input_spec_digest,
            artifact_checksum: checksum.clone(),
        },
    );
    document.virtual_copies[copy_index].recipe.denoise_ai = Some(denoise);
    document.validate()?;
    save_sidecar(sidecar, document)?;
    info!(
        "denoise_rgb artifact recorded: bundle={} record={record_id} checksum={checksum}",
        zdata.display()
    );
    emit(
        args.json,
        serde_json::json!({
            "command": "denoise",
            "action": "record-rgb",
            "input": args.input,
            "virtual_copy": document.virtual_copies[copy_index].id,
            "bundle": zdata,
            "record": record_id,
            "checksum": checksum,
            "width": core_artifact.width,
            "height": core_artifact.height,
            "status": "ok",
        }),
        "recorded denoise_rgb artifact",
    )
}
