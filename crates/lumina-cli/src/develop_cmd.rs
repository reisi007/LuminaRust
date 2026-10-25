//! Round-trip handlers: `import`, `develop`, `render`, `export`, `validate`,
//! `keywords`, `batch-meta`, `previous` and the `--migrate` sidecar delegation.
//!
//! Extracted verbatim from `main.rs` for the file-size ratchet
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100, round-trip slice). Pure move: bodies are
//! byte-identical; only the `pub(super)` handler tokens, the helper `use` lines
//! and this header are new.

use super::*;
pub(super) fn import_file(args: ImportArgs) -> Result<(), CliError> {
    let bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
    let (frame, raw) = decode_input(&args.input, &bytes)?;
    let path = sidecar_path_for(&args.input);
    if args.migrate && path.exists() {
        migrate_sidecar(&path)?;
    } else if path.exists() {
        let document = load_sidecar(&path)?;
        // REVIEW-CLI-N7: mirror `process_selected`'s source-change detection.
        // Import must not silently bless a sidecar whose edits belong to
        // different file contents — reproducibility over convenience. The
        // mismatch is a loud error; the sidecar keeps guarding the OLD
        // contents until it is consciously removed or migrated.
        let current_identity = source_identity(&args.input, &bytes, &frame, raw.as_ref())?;
        if document.source.content_hash != current_identity.content_hash
            || document.source.byte_length != current_identity.byte_length
        {
            return Err(CliError::Message(format!(
                "source changed since sidecar was written: `{}`; remove or rename the sidecar to re-import consciously",
                args.input.display()
            )));
        }
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

/// R2-CLI-09: validates an adjustment value BEFORE it is inserted into a
/// recipe, mirroring the MCP `lumina_edit` contract and the sidecar
/// save-time validator (same ranges). Previously `develop --exposure 999`
/// was accepted at insert time and only rejected later with a generic
/// save-time error that did not name the allowed range.
fn validate_adjustment_range(name: &str, value: f64) -> Result<(), CliError> {
    let (minimum, maximum) = match name {
        "exposure" => (-10.0, 10.0),
        _ => (-1.0, 1.0),
    };
    if !value.is_finite() || !(minimum..=maximum).contains(&value) {
        return Err(CliError::Message(format!(
            "invalid adjustment `{name}`: value {value} outside allowed range {minimum}..={maximum}"
        )));
    }
    Ok(())
}

pub(super) fn develop(args: DevelopArgs) -> Result<(), CliError> {
    // R2-CLI-09: fail BEFORE the sidecar is loaded or mutated so an invalid
    // value can never produce a half-applied develop run.
    if let Some(value) = args.exposure {
        validate_adjustment_range("exposure", value)?;
    }
    if let Some(value) = args.contrast {
        validate_adjustment_range("contrast", value)?;
    }
    // LRPAR-G01-BASIC: fail BEFORE the sidecar is loaded or mutated so an
    // invalid treatment/profile can never produce a half-applied develop run
    // (same R2-CLI-09 discipline as the numeric ranges above).
    if let Some(treatment) = &args.treatment {
        if treatment != lumina_sidecar::TREATMENT_COLOR && treatment != lumina_sidecar::TREATMENT_BW
        {
            return Err(CliError::Message(format!(
                "unknown treatment `{treatment}` (expected `color` or `bw`)"
            )));
        }
    }
    if let Some(profile) = &args.profile {
        if !lumina_sidecar::DEVELOP_PROFILES.contains(&profile.as_str()) {
            return Err(CliError::Message(format!(
                "unknown develop profile `{profile}` (expected one of {})",
                lumina_sidecar::DEVELOP_PROFILES.join("|")
            )));
        }
    }
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
    // LRPAR-G01-BASIC: shared sidecar mutation paths (same stasch/whitelist
    // semantics as the GUI Treatment selector and Profile dropdown).
    if let Some(treatment) = &args.treatment {
        copy.recipe
            .apply_treatment(treatment)
            .map_err(|error| CliError::Message(error.to_string()))?;
    }
    if let Some(profile) = &args.profile {
        copy.recipe
            .apply_develop_profile(profile)
            .map_err(|error| CliError::Message(error.to_string()))?;
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

pub(super) fn render(args: FileArgs) -> Result<(), CliError> {
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
            whites: None,
            blacks: None,
            highlights: None,
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
            write_metadata: false,
        },
        args.quality,
        args.virtual_copy.as_deref(),
        args.mask_policy.to_policy(),
        &mut mask_warnings,
    )?;
    emit(
        args.json,
        serde_json::json!({"command":"render", "output":output, "format":args.format, "status":"ok", "mask_warnings":mask_warnings}),
        "rendered",
    )
}

pub(super) fn export(args: ExportArgs) -> Result<(), CliError> {
    validate_format(&args.format)?;
    validate_quality(args.quality)?;
    if args.migrate {
        migrate_sidecar(&sidecar_path_for(&args.input))?;
    }
    // REVIEW-CLI-EXPORTMASK-1: stale-mask behaviour is harmonized across the
    // render-capable subcommands — the default is warn-and-continue (identical
    // to render/batch/process); aborting is reserved for an explicit
    // `--mask-policy strict`. The preflight deliberately runs before decoding
    // and writing so a strict abort leaves no half-written artifacts.
    preflight_masks(
        &args.input,
        args.virtual_copy.as_deref(),
        args.update_masks,
        args.mask_policy.to_policy(),
    )?;
    if args.update_masks {
        // Persist the one-shot refresh request (same channel develop/batch
        // use) so THIS export's render re-infers; `process_selected` consumes
        // and removes it again. Without an inference engine the render itself
        // fails loudly instead of pretending a stale mask was refreshed.
        mark_masks_pending_refresh(&args.input, args.virtual_copy.as_deref())?;
    }
    let output = args.output.with_extension(format_extension(&args.format));
    let mut mask_warnings = Vec::new();
    let metadata_written = process_selected(
        ProcessArgs {
            input: args.input.clone(),
            output: output.clone(),
            preset: None,
            exposure: None,
            contrast: None,
            whites: None,
            blacks: None,
            highlights: None,
            shadows: None,
            auto_tone: false,
            match_total_exposure: false,
            target_luminance: 0.5,
            write_metadata: args.write_metadata,
        },
        args.quality,
        args.virtual_copy.as_deref(),
        args.mask_policy.to_policy(),
        &mut mask_warnings,
    )?;
    // LRPAR-G15-IPTC-S6: the bake-in outcome is additive in `--json` output —
    // absent without the flag (exactly today's payload).
    let mut payload = serde_json::json!({"command":"export", "output":output, "quality":args.quality, "status":"ok", "mask_warnings":mask_warnings});
    if let Some(written) = metadata_written {
        payload["metadata_written"] = written.json();
    }
    emit(args.json, payload, "exported")
}

pub(super) fn validate(args: IndexArgs) -> Result<(), CliError> {
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

pub(super) fn keywords(args: KeywordsArgs) -> Result<(), CliError> {
    let (path, mut document) = require_sidecar(&args.input)?;
    let original_bytes = fs::read(&args.input).map_err(|error| io_error(&args.input, error))?;
    let mut ops = Vec::with_capacity(args.add.len() + args.remove.len());
    for keyword in &args.add {
        ops.push(BatchOp::AddKeyword {
            keyword: keyword.clone(),
        });
    }
    for keyword in &args.remove {
        ops.push(BatchOp::RemoveKeyword {
            keyword: keyword.clone(),
        });
    }
    let mut changed = false;
    for op in &ops {
        changed |= apply_batch_op(&mut document, op).map_err(|error| {
            CliError::Message(format!(
                "keywords for `{}` rejected: {error}",
                args.input.display()
            ))
        })?;
    }
    if changed {
        document.validate()?;
        save_sidecar(&path, &document)?;
        info!(
            "keywords for `{}` updated ({} operation(s), {} keyword(s))",
            args.input.display(),
            ops.len(),
            document.keywords.len()
        );
    } else if ops.is_empty() {
        info!("keywords for `{}` listed", args.input.display());
    } else {
        info!(
            "keywords for `{}` unchanged (idempotent no-op)",
            args.input.display()
        );
    }
    // The original image is never modified by a metadata command.
    debug_assert_eq!(
        fs::read(&args.input).map_err(|error| io_error(&args.input, error))?,
        original_bytes
    );
    emit(
        args.json,
        serde_json::json!({"command":"keywords", "input":args.input, "keywords":document.keywords, "changed":changed, "status":"ok"}),
        &format_keywords_text(&document.keywords),
    )
}

fn format_keywords_text(keywords: &[String]) -> String {
    if keywords.is_empty() {
        "keywords: (none)".into()
    } else {
        format!("keywords: {}", keywords.join(", "))
    }
}

/// Parses the single `BatchOp` of `batch-meta` from exactly one of `--op` /
/// `--op-file`. Anything else (neither, both, invalid JSON, unknown variant)
/// is a loud error; the operation language itself is owned by
/// `lumina-sidecar`.
fn parse_batch_op(args: &BatchMetaArgs) -> Result<BatchOp, CliError> {
    match (&args.op, &args.op_file) {
        (Some(_), Some(_)) => Err(CliError::Message(
            "`batch-meta` accepts exactly one of `--op` / `--op-file`".into(),
        )),
        (None, None) => Err(CliError::Message(
            "`batch-meta` requires one of `--op` / `--op-file`".into(),
        )),
        (Some(text), None) => serde_json::from_str(text)
            .map_err(|error| CliError::Message(format!("invalid batch operation JSON: {error}"))),
        (None, Some(path)) => {
            let text = fs::read_to_string(path).map_err(|error| io_error(path, error))?;
            serde_json::from_str(&text).map_err(|error| {
                CliError::Message(format!("invalid batch operation JSON: {error}"))
            })
        }
    }
}

pub(super) fn batch_meta(args: BatchMetaArgs) -> Result<(), CliError> {
    let op = parse_batch_op(&args)?;
    let targets = collect_target_sidecars(&args.input)?;
    if targets.is_empty() {
        return Err(CliError::Message(format!(
            "no sidecars found under `{}`",
            args.input.display()
        )));
    }
    let mut changed_count = 0usize;
    let mut unchanged_count = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut items = Vec::with_capacity(targets.len());
    for sidecar in &targets {
        match load_sidecar(sidecar).map_err(CliError::from) {
            Err(error) => {
                let message = format!("{}: {error}", sidecar.display());
                eprintln!("error: batch-meta: {message}");
                info!("batch-meta: `{}` failed", sidecar.display());
                failures.push(message);
                items.push(serde_json::json!({"sidecar":sidecar, "status":"failed"}));
            }
            Ok(mut document) => match apply_batch_op(&mut document, &op) {
                Err(error) => {
                    let message = format!("{}: {error}", sidecar.display());
                    eprintln!("error: batch-meta: {message}");
                    info!("batch-meta: `{}` failed", sidecar.display());
                    failures.push(message);
                    items.push(serde_json::json!({"sidecar":sidecar, "status":"failed"}));
                }
                Ok(false) => {
                    info!("batch-meta: `{}` unchanged", sidecar.display());
                    unchanged_count += 1;
                    items.push(
                        serde_json::json!({"sidecar":sidecar, "status":"ok", "changed":false}),
                    );
                }
                Ok(true) => match document
                    .validate()
                    .map_err(CliError::from)
                    .and_then(|()| save_sidecar(sidecar, &document).map_err(CliError::from))
                {
                    Err(error) => {
                        let message = format!("{}: {error}", sidecar.display());
                        eprintln!("error: batch-meta: {message}");
                        info!("batch-meta: `{}` failed", sidecar.display());
                        failures.push(message);
                        items.push(serde_json::json!({"sidecar":sidecar, "status":"failed"}));
                    }
                    Ok(()) => {
                        info!("batch-meta: `{}` updated", sidecar.display());
                        changed_count += 1;
                        items.push(
                            serde_json::json!({"sidecar":sidecar, "status":"ok", "changed":true}),
                        );
                    }
                },
            },
        }
    }
    let failed = failures.len();
    let text = format!(
        "batch-meta: {changed_count} changed, {unchanged_count} unchanged, {failed} failed"
    );
    emit(
        args.json,
        serde_json::json!({"command":"batch-meta", "input":args.input, "op":op, "changed":changed_count, "unchanged":unchanged_count, "failed":failed, "errors":failures, "items":items, "status": if failed == 0 { "ok" } else { "partial" }}),
        &text,
    )?;
    info!("{text}");
    if failed != 0 {
        return Err(CliError::BatchPartial { failed });
    }
    Ok(())
}

/// G-08 Previous-Übernahme (LRPAR-G08-PREVIOUS): copy the full recipe of one
/// reference image onto N target sidecars — the same full-recipe Sync
/// mechanism the GUI uses (no second mechanism, no subset selection). The
/// reference is loaded first and loudly aborts everything (exit 1) when its
/// sidecar is missing/invalid or the copy id is unknown, so no target is
/// touched without a valid source. Each target is then handled in isolation:
/// load, assign recipe, one `previous` history step, validate, atomic save.
/// A missing/invalid target sidecar, an unknown target copy, or non-neutral
/// local-mask state on that target marks only its item as `failed` (stderr
/// line + `info!` log); the remaining targets still run. Exit `0` on full
/// success, `3` on partial failure (analog `batch` / `batch-meta`), `1` on
/// hard errors. The original images are never modified; history `extras` carry
/// only the reference file name, never paths.
///
/// MASK-LOCAL-P0/P1.1: `previous` is a **recipe-only** cross-image transfer.
/// A non-neutral local mask state on the *reference* copy aborts the whole
/// command loudly (exit 1, no target touched); a non-neutral local mask state
/// on a *target* copy fails only that target (exit 3, target bytes unchanged).
/// Mask layers and their P0/P1.1 deltas are never transferred automatically —
/// an explicit full-look/mask copy is a separate, later action and is not part
/// of P1.1.
pub(super) fn previous(args: PreviousArgs) -> Result<(), CliError> {
    if args.to.is_empty() {
        return Err(CliError::Message(
            "`previous` requires at least one `--to` target".into(),
        ));
    }
    let from_sidecar = sidecar_path_for(&args.from);
    let from_document = load_sidecar(&from_sidecar).map_err(CliError::from)?;
    let from_id = args.from_copy.as_deref().unwrap_or("vc-original");
    let reference_copy = from_document
        .virtual_copies
        .iter()
        .find(|copy| copy.id == from_id)
        .ok_or_else(|| {
            CliError::Message(format!(
                "unknown virtual copy `{from_id}` in `{}`",
                from_sidecar.display()
            ))
        })?;
    let local_layers = mask_local::non_neutral_local_layer_ids(reference_copy)?;
    if !local_layers.is_empty() {
        return Err(CliError::Message(format!(
            "previous refused recipe-only transfer from copy `{from_id}`: non-neutral local mask adjustments exist on layer(s) {}; cross-image mask state transfer is unsafe",
            local_layers.join(", ")
        )));
    }
    let reference = reference_copy.recipe.clone();
    info!(
        "previous: reference `{}` copy `{from_id}`",
        args.from.display()
    );
    let to_id = args.to_copy.as_deref().unwrap_or("vc-original");
    let mut applied_count = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut items = Vec::with_capacity(args.to.len());
    for target in &args.to {
        match mask_local::apply_previous_to_target(target, to_id, &reference, &args.from) {
            Ok(()) => {
                info!(
                    "previous: `{}` updated from `{}`",
                    target.display(),
                    args.from.display()
                );
                applied_count += 1;
                items.push(serde_json::json!({"target":target, "status":"ok"}));
            }
            Err(error) => {
                let message = format!("{}: {error}", target.display());
                eprintln!("error: previous: {message}");
                info!("previous: `{}` failed", target.display());
                failures.push(message);
                items.push(serde_json::json!({"target":target, "status":"failed"}));
            }
        }
    }
    let failed = failures.len();
    let text = format!(
        "previous: {applied_count} applied, {failed} failed (reference `{}`)",
        args.from.display()
    );
    emit(
        args.json,
        serde_json::json!({"command":"previous", "from":args.from, "from_copy":from_id, "to_copy":to_id, "applied":applied_count, "failed":failed, "errors":failures, "items":items, "status": if failed == 0 { "ok" } else { "partial" }}),
        &text,
    )?;
    info!("{text}");
    if failed != 0 {
        return Err(CliError::BatchPartial { failed });
    }
    Ok(())
}
/// F-019: the CLI `--migrate` flag delegates to the library-level
/// `lumina_sidecar::migrate_sidecar_file` so every migration takes the
/// per-sidecar write lock and writes a `.bak` backup before the atomic
/// replace. There is no silent in-place rewrite: a locked or failing
/// migration is an explicit error (Agents.md: no silent fallbacks).
fn migrate_sidecar(path: &Path) -> Result<(), CliError> {
    lumina_sidecar::migrate_sidecar_file(path)?;
    Ok(())
}
