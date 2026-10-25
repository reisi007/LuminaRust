//! `cull` (LRPAR-G09-CULL-IMPL-25 CLI slice): the persistent Stage-1
//! assisted-culling command, its argument type and its multi-item report.
//!
//! Extracted verbatim from `main.rs` for the file-size ratchet
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100, cull slice). Pure move: bodies are
//! byte-identical; only the `pub(super)`/`pub(crate)` tokens, the `use` line
//! and this header are new.

use super::*;
// ===========================================================================
// LRPAR-G09-CULL-IMPL-25 (CLI slice): `cull`.
//
// SOLL: `feature/decisions/LRPAR-G09-CULL-25.md` §2/§5/§7.3. Assisted culling
// over an explicit selection: `--analyze` runs the deterministic Stage-1
// heuristic, persists the source-level proposal and never writes
// rating/flag/label; `--status` reports the persisted state read-only.
// Multi-item failures are isolated (exit 3), a hard error is exit 1.
// ===========================================================================

#[derive(Debug, Args)]
pub(crate) struct CullArgs {
    /// Explicit selection (repeatable). A file is one item; a directory
    /// expands to its supported images (sorted, non-recursive).
    #[arg(long = "input", required = true)]
    input: Vec<PathBuf>,
    /// Report the persisted proposal status and exit (read-only; the default
    /// when `--analyze` is absent).
    #[arg(long)]
    status: bool,
    /// Explicitly analyze the selection and persist the proposals.
    #[arg(long)]
    analyze: bool,
    /// Re-analyze even when a valid, identity-matching proposal already exists.
    #[arg(long)]
    force: bool,
    #[arg(long)]
    json: bool,
}

/// Expands the explicit selection (files and directories) and deduplicates by
/// path while keeping the caller's order. Never walks symlinks/loops.
fn cull_selection(inputs: &[PathBuf]) -> Result<Vec<PathBuf>, CliError> {
    let mut selection: Vec<PathBuf> = Vec::new();
    for input in inputs {
        if input.is_dir() {
            let mut entries: Vec<PathBuf> = fs::read_dir(input)
                .map_err(|error| io_error(input, error))?
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.is_file() && has_image_extension(path))
                .collect();
            entries.sort();
            selection.extend(entries);
        } else {
            selection.push(input.clone());
        }
    }
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    selection.retain(|path| seen.insert(path.clone()));
    if selection.is_empty() {
        return Err(CliError::Message(
            "cull selection is empty (no supported images found)".into(),
        ));
    }
    Ok(selection)
}

pub(super) fn cull(args: CullArgs) -> Result<(), CliError> {
    if args.status && args.analyze {
        return Err(CliError::Usage(
            "--status and --analyze are mutually exclusive".into(),
        ));
    }
    let selection = cull_selection(&args.input)?;
    if args.analyze {
        cull_analyze(&args, &selection)
    } else {
        cull_status(&args, &selection)
    }
}

fn cull_status(args: &CullArgs, selection: &[PathBuf]) -> Result<(), CliError> {
    let config = CullConfig::default();
    let mut items = Vec::with_capacity(selection.len());
    let mut failed = 0usize;
    for input in selection {
        match cull_status_one(input, &config) {
            Ok(item) => items.push(item),
            Err(error) => {
                failed += 1;
                eprintln!("error: cull status `{}` failed: {error}", input.display());
                items.push(serde_json::json!({
                    "input": input,
                    "status": "failed",
                    "error": error.to_string(),
                }));
            }
        }
    }
    emit(
        args.json,
        serde_json::json!({"command": "cull", "action": "status", "items": items}),
        "cull status",
    )?;
    if failed > 0 {
        return Err(CliError::Partial {
            command: "cull",
            failed,
        });
    }
    Ok(())
}

fn cull_status_one(input: &Path, config: &CullConfig) -> Result<serde_json::Value, CliError> {
    let bytes = fs::read(input).map_err(|error| io_error(input, error))?;
    let (frame, _raw) = decode_input(input, &bytes)?;
    let document = load_sidecar(&sidecar_path_for(input))?;
    let analysis_frame = downscale(&frame, DownscaleTarget::MaxWidth(config.analysis_max_width))?;
    let current = heuristic_identity(
        cull_cli::source_fingerprint(&bytes),
        document.source.decode_fingerprint.clone(),
        document.source.geometry_fingerprint.clone(),
        Resolution {
            width: analysis_frame.width(),
            height: analysis_frame.height(),
            extras: Default::default(),
        },
    );
    let (status, section) = match evaluate_culling(&document, &current) {
        CullingReadState::NoProposal => ("no-proposal", serde_json::json!(null)),
        CullingReadState::Valid(section) => ("valid", cull_section_json(&section, &[])),
        CullingReadState::Stale {
            section,
            mismatches,
        } => {
            let mismatch_labels: Vec<String> =
                mismatches.iter().map(|m| format!("{m:?}")).collect();
            ("stale", cull_section_json(&section, &mismatch_labels))
        }
        CullingReadState::Unusable { section } => ("unusable", cull_section_json(&section, &[])),
    };
    Ok(serde_json::json!({
        "input": input,
        "status": status,
        "proposal": section,
    }))
}

fn cull_section_json(
    section: &lumina_sidecar::CullingSection,
    mismatches: &[String],
) -> serde_json::Value {
    serde_json::json!({
        "proposal": section.proposal,
        "score": section.score,
        "reasons": section.reasons,
        "created_at": section.created_at,
        "status": section.status,
        "analyzer": {
            "kind": section.identity.analyzer.kind,
            "name": section.identity.analyzer.name,
            "version": section.identity.analyzer.version,
        },
        "mismatches": mismatches,
    })
}

fn cull_analyze(args: &CullArgs, selection: &[PathBuf]) -> Result<(), CliError> {
    let config = CullConfig::default();
    let mut frames: Vec<ImageFrame> = Vec::new();
    let mut isos: Vec<Option<u32>> = Vec::new();
    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut source_fingerprints: Vec<SourceFingerprint> = Vec::new();
    let mut items: Vec<serde_json::Value> = Vec::new();
    let mut failed = 0usize;

    for input in selection {
        let decoded = fs::read(input)
            .map_err(|error| io_error(input, error))
            .and_then(|bytes| {
                let source = cull_cli::source_fingerprint(&bytes);
                decode_input(input, &bytes).map(|decoded| (decoded, source))
            });
        match decoded {
            Ok(((frame, raw), source)) => {
                isos.push(
                    raw.as_ref()
                        .and_then(|metadata| metadata.iso)
                        .map(|iso| iso.round().max(0.0) as u32),
                );
                frames.push(frame);
                inputs.push(input.clone());
                source_fingerprints.push(source);
            }
            Err(error) => {
                failed += 1;
                eprintln!("error: cull analysis `{}` failed: {error}", input.display());
                items.push(serde_json::json!({
                    "input": input,
                    "status": "failed",
                    "error": error.to_string(),
                }));
            }
        }
    }
    if frames.is_empty() {
        return Err(CliError::Message(format!(
            "cull analysis: no decodable inputs ({failed} failed)"
        )));
    }

    let sources: Vec<CullSourceInput<'_>> = frames
        .iter()
        .zip(isos.iter())
        .map(|(frame, iso)| CullSourceInput { frame, iso: *iso })
        .collect();
    let selection_analysis = analyze_selection(&sources, &config)?;

    for (index, analysis) in selection_analysis.images.iter().enumerate() {
        let input = inputs[index].clone();
        match cull_cli::persist_one(&input, &source_fingerprints[index], analysis, args.force) {
            Ok(item) => items.push(item),
            Err(error) => {
                failed += 1;
                eprintln!("error: cull persist `{}` failed: {error}", input.display());
                items.push(serde_json::json!({
                    "input": input,
                    "status": "failed",
                    "error": error.to_string(),
                }));
            }
        }
    }

    emit(
        args.json,
        serde_json::json!({
            "command": "cull",
            "action": "analyze",
            "items": items,
            "groups": selection_analysis.groups,
        }),
        "cull analysis",
    )?;
    if failed > 0 {
        return Err(CliError::Partial {
            command: "cull",
            failed,
        });
    }
    Ok(())
}
