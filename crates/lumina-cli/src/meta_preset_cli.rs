use super::*;

/// LRPAR-G15-IPTC-S4: resolves a `show` / `apply` preset spec (display name
/// against the user-global directory, or an explicit file path) and loads it.
/// Any failure aborts loudly (exit 1) before any target is touched.
fn load_cli_meta_preset(spec: &str) -> Result<(PathBuf, MetaPresetFile), CliError> {
    let path = resolve_meta_preset_path(spec, None).map_err(|error| {
        CliError::Message(format!("meta preset `{spec}` cannot be resolved: {error}"))
    })?;
    load_meta_preset_file(&path)
        .map(|preset| (path.clone(), preset))
        .map_err(|error| {
            CliError::Message(format!(
                "meta preset `{}` rejected: {error}",
                path.display()
            ))
        })
}

/// LRPAR-G15-IPTC-S4: `meta preset list` — lists the directory (explicit or
/// user-global) sorted by file name. Read-only; broken files surface as
/// failed entries with their reason (exit stays 0, nothing is hidden).
pub(super) fn meta_preset_list(args: MetaPresetListArgs) -> Result<(), CliError> {
    let dir = match args.dir {
        Some(dir) => dir,
        None => default_meta_presets_dir().ok_or_else(|| {
            CliError::Message(
                "meta-preset directory is unavailable: the platform configuration directory \
                 could not be determined; pass an explicit directory"
                    .into(),
            )
        })?,
    };
    let entries = scan_meta_presets_dir(&dir);
    let mut available_count = 0usize;
    let mut failed_count = 0usize;
    let mut lines = Vec::with_capacity(entries.len());
    let mut items = Vec::with_capacity(entries.len());
    for entry in &entries {
        match entry {
            MetaPresetEntry::Available { path, preset } => {
                available_count += 1;
                info!(
                    "meta preset list: `{}` available ({} field(s), {} placeholder(s))",
                    path.display(),
                    preset.fields.len(),
                    preset.placeholders.len()
                );
                lines.push(format!(
                    "{}: {} field(s), {} placeholder(s) [{}]",
                    preset.name,
                    preset.fields.len(),
                    preset.placeholders.len(),
                    path.display()
                ));
                items.push(serde_json::json!({
                    "path": path,
                    "status": "available",
                    "name": preset.name,
                    "fields": preset.fields.len(),
                    "placeholders": preset.placeholders.iter().map(|p| &p.name).collect::<Vec<_>>(),
                }));
            }
            MetaPresetEntry::Failed { path, error } => {
                failed_count += 1;
                info!("meta preset list: `{}` failed ({error})", path.display());
                lines.push(format!("{}: FAILED ({error})", path.display()));
                items.push(serde_json::json!({
                    "path": path,
                    "status": "failed",
                    "error": error,
                }));
            }
        }
    }
    info!(
        "meta preset list for `{}` ({} available, {} failed)",
        dir.display(),
        available_count,
        failed_count
    );
    let text = if lines.is_empty() {
        format!("presets in `{}`: (none)", dir.display())
    } else {
        format!(
            "presets in `{}` ({} available, {} failed):\n{}",
            dir.display(),
            available_count,
            failed_count,
            lines.join("\n")
        )
    };
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-preset-list",
            "dir": dir,
            "available": available_count,
            "failed": failed_count,
            "items": items,
            "status": "ok",
        }),
        &text,
    )
}

/// LRPAR-G15-IPTC-S4: `meta preset show` — displays one preset (fields plus
/// placeholder names with descriptions). Read-only.
pub(super) fn meta_preset_show(args: MetaPresetShowArgs) -> Result<(), CliError> {
    let (path, preset) = load_cli_meta_preset(&args.preset)?;
    info!(
        "meta preset show for `{}` ({} field(s), {} placeholder(s))",
        path.display(),
        preset.fields.len(),
        preset.placeholders.len()
    );
    let mut lines = vec![
        format!("name: {}", preset.name),
        format!("file: {}", path.display()),
    ];
    for (id, value) in &preset.fields {
        lines.push(format!("field {id}: \"{value}\""));
    }
    for placeholder in &preset.placeholders {
        lines.push(format!(
            "placeholder {{{}}}: {}",
            placeholder.name, placeholder.description
        ));
    }
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-preset-show",
            "preset": path,
            "name": preset.name,
            "fields": preset.fields,
            "placeholders": preset.placeholders,
            "status": "ok",
        }),
        &lines.join("\n"),
    )
}

/// LRPAR-G15-IPTC-S4: splits a `--var` value at the first `=` into
/// `(name, value)`. A missing `=` is a loud error before anything is mutated.
fn split_var_assignment(value: &str) -> Result<(String, String), CliError> {
    value.split_once('=').map_or_else(
        || {
            Err(CliError::Message(format!(
                "invalid variable assignment `{value}`: expected `NAME=VALUE`"
            )))
        },
        |(name, var_value)| Ok((name.to_string(), var_value.to_string())),
    )
}

/// LRPAR-G15-IPTC-S4: applies the resolved draft to one target: load, mutate
/// a clone via `apply_metadata_draft` (`origin = "preset:<name>"`), validate,
/// CAS + atomic save. Returns `Ok(true)` on update and `Ok(false)` for
/// idempotent no-ops (no history entry, no write). A missing sidecar is a
/// loud per-target error ("zuerst importieren"), never a silent creation.
fn apply_meta_preset_to_target(
    target: &Path,
    resolved: &BTreeMap<String, String>,
    origin: &str,
) -> Result<bool, CliError> {
    let sidecar = sidecar_path_for(target);
    let document = match load_sidecar(&sidecar) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            return Err(CliError::Message(format!(
                "no sidecar for `{}`; run `import` first",
                target.display()
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let original_bytes = fs::read(target).map_err(|error| io_error(target, error))?;
    let expected = document_revision(&document)?;
    let timestamp = now_rfc3339_utc();
    let mut candidate = document.clone();
    if !candidate.apply_metadata_draft(resolved, origin, &timestamp)? {
        debug_assert_eq!(
            fs::read(target).map_err(|error| io_error(target, error))?,
            original_bytes
        );
        return Ok(false);
    }
    save_sidecar_if_unchanged(&sidecar, &candidate, Some(&expected))?;
    // The original image is never modified by a metadata command.
    debug_assert_eq!(
        fs::read(target).map_err(|error| io_error(target, error))?,
        original_bytes
    );
    Ok(true)
}

/// LRPAR-G15-IPTC-S4: `meta preset apply` — renders the preset upfront (all
/// placeholder variables required; missing/unknown variables and limit
/// violations abort everything with exit 1, nothing written), then applies
/// the rendered draft per target in isolation (updated / unchanged / failed;
/// exit 3 on partial failure). Idempotent re-application reports `unchanged`
/// without a history entry.
pub(super) fn meta_preset_apply(args: MetaPresetApplyArgs) -> Result<(), CliError> {
    let (path, preset) = load_cli_meta_preset(&args.preset)?;
    let mut vars = BTreeMap::new();
    for assignment in &args.var {
        let (name, value) = split_var_assignment(assignment)?;
        if vars.insert(name.clone(), value).is_some() {
            return Err(CliError::Message(format!(
                "duplicate variable `{name}` (each `--var` name may be given once)"
            )));
        }
    }
    let resolved =
        render_meta_preset(&preset, &vars, &path.display().to_string()).map_err(|error| {
            CliError::Message(format!(
                "meta preset apply for `{}` rejected: {error}",
                path.display()
            ))
        })?;
    let origin = format!("preset:{}", preset.name);
    info!(
        "meta preset apply `{}` to {} target(s) ({} field(s))",
        preset.name,
        args.target.len(),
        resolved.len()
    );
    let mut updated_count = 0usize;
    let mut unchanged_count = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut items = Vec::with_capacity(args.target.len());
    for target in &args.target {
        match apply_meta_preset_to_target(target, &resolved, &origin) {
            Ok(true) => {
                info!(
                    "meta preset apply: `{}` updated (preset `{}`)",
                    target.display(),
                    preset.name
                );
                updated_count += 1;
                items.push(serde_json::json!({"target": target, "status": "updated"}));
            }
            Ok(false) => {
                info!(
                    "meta preset apply: `{}` unchanged (preset `{}` already applied)",
                    target.display(),
                    preset.name
                );
                unchanged_count += 1;
                items.push(serde_json::json!({"target": target, "status": "unchanged"}));
            }
            Err(error) => {
                let message = format!("{}: {error}", target.display());
                eprintln!("error: meta preset apply: {message}");
                info!("meta preset apply: `{}` failed", target.display());
                failures.push(message.clone());
                items.push(
                    serde_json::json!({"target": target, "status": "failed", "error": message}),
                );
            }
        }
    }
    let failed = failures.len();
    let text = format!(
        "meta preset apply: {updated_count} updated, {unchanged_count} unchanged, {failed} failed (preset `{}`)",
        preset.name
    );
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-preset-apply",
            "preset": path,
            "name": preset.name,
            "updated": updated_count,
            "unchanged": unchanged_count,
            "failed": failed,
            "errors": failures,
            "items": items,
            "status": if failed == 0 { "ok" } else { "partial" },
        }),
        &text,
    )?;
    info!("{text}");
    if failed != 0 {
        return Err(CliError::BatchPartial { failed });
    }
    Ok(())
}

/// LRPAR-G15-IPTC-S5: mirrors the selected source fields onto one target:
/// load, mutate a clone (draft values copied, source-absent fields removed,
/// `keywords` replaced wholesale), exactly one history entry
/// (`origin = "sync:<source-file-name>"`), validate, CAS + atomic save.
/// Returns `Ok(true)` on update and `Ok(false)` for idempotent no-ops (no
/// history entry, no write). A missing sidecar is a loud per-target error
/// ("zuerst importieren" — "run `import` first"), never a silent creation.
/// Recipes, masks and per-copy edit history are never touched.
fn apply_meta_sync_to_target(
    target: &Path,
    source_draft: &BTreeMap<String, String>,
    source_keywords: &[String],
    fields: &BTreeSet<String>,
    origin: &str,
) -> Result<bool, CliError> {
    let sidecar = sidecar_path_for(target);
    let document = match load_sidecar(&sidecar) {
        Ok(document) => document,
        Err(lumina_sidecar::SidecarError::Missing(_)) => {
            return Err(CliError::Message(format!(
                "no sidecar for `{}`; run `import` first",
                target.display()
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let original_bytes = fs::read(target).map_err(|error| io_error(target, error))?;
    let expected = document_revision(&document)?;
    let timestamp = now_rfc3339_utc();
    let mut candidate = document.clone();
    let mut changed = BTreeSet::new();
    for id in fields {
        if id == "keywords" {
            if candidate.keywords != source_keywords {
                candidate.keywords = source_keywords.to_vec();
                changed.insert(id.clone());
            }
        } else if let Some(value) = source_draft.get(id) {
            if candidate.metadata.draft.get(id).map(String::as_str) != Some(value.as_str()) {
                candidate.metadata.draft.insert(id.clone(), value.clone());
                changed.insert(id.clone());
            }
        } else if candidate.metadata.draft.remove(id).is_some() {
            changed.insert(id.clone());
        }
    }
    if changed.is_empty() {
        debug_assert_eq!(
            fs::read(target).map_err(|error| io_error(target, error))?,
            original_bytes
        );
        return Ok(false);
    }
    let changed_list: Vec<String> = changed.into_iter().collect();
    let rev = candidate.metadata.latest_rev() + 1;
    candidate.metadata.history.insert(
        0,
        MetadataHistoryEntry {
            rev,
            timestamp,
            origin: origin.to_string(),
            changed: changed_list.clone(),
        },
    );
    candidate
        .metadata
        .history
        .truncate(MAX_METADATA_HISTORY_ENTRIES);
    candidate.validate()?;
    save_sidecar_if_unchanged(&sidecar, &candidate, Some(&expected))?;
    // The original image is never modified by a metadata command.
    debug_assert_eq!(
        fs::read(target).map_err(|error| io_error(target, error))?,
        original_bytes
    );
    Ok(true)
}

/// LRPAR-G15-IPTC-S5: `meta sync` — validates `--fields` upfront (empty list
/// and unknown IDs abort everything with exit 1, nothing written), snapshots
/// the source draft + keywords once, then mirrors the selection per target in
/// isolation (updated / unchanged / failed; exit 3 on partial failure).
/// A missing source sidecar aborts everything with exit 1; a missing target
/// sidecar fails only its own item. Idempotent re-application reports
/// `unchanged` without a history entry.
pub(super) fn meta_sync(args: MetaSyncArgs) -> Result<(), CliError> {
    if args.fields.iter().all(|field| field.is_empty()) {
        return Err(CliError::Message(
            "`meta sync` requires `--fields <id,…>` (registry field IDs, `keywords` allowed); refusing a silent transfer-all".into(),
        ));
    }
    let mut fields = BTreeSet::new();
    for id in &args.fields {
        if id != "keywords" && !is_metadata_field(id) {
            return Err(CliError::Message(format!(
                "meta sync for `{}` rejected: unknown metadata field `{id}`",
                args.source.display()
            )));
        }
        fields.insert(id.clone());
    }
    let (_, source_document) = require_sidecar(&args.source)?;
    let source_draft = source_document.metadata.draft.clone();
    let source_keywords = source_document.keywords.clone();
    let source_name = args
        .source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            CliError::Message(format!(
                "meta sync for `{}` rejected: source has no file name",
                args.source.display()
            ))
        })?;
    let origin = format!("sync:{source_name}");
    let field_list: Vec<String> = fields.iter().cloned().collect();
    info!(
        "meta sync from `{source_name}` to {} target(s) ({} field(s): {})",
        args.target.len(),
        field_list.len(),
        field_list.join(", ")
    );
    let mut updated_count = 0usize;
    let mut unchanged_count = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut items = Vec::with_capacity(args.target.len());
    for target in &args.target {
        match apply_meta_sync_to_target(target, &source_draft, &source_keywords, &fields, &origin) {
            Ok(true) => {
                info!(
                    "meta sync: `{}` updated (from `{source_name}`)",
                    target.display()
                );
                updated_count += 1;
                items.push(serde_json::json!({"target": target, "status": "updated"}));
            }
            Ok(false) => {
                info!(
                    "meta sync: `{}` unchanged (already in sync with `{source_name}`)",
                    target.display()
                );
                unchanged_count += 1;
                items.push(serde_json::json!({"target": target, "status": "unchanged"}));
            }
            Err(error) => {
                let message = format!("{}: {error}", target.display());
                eprintln!("error: meta sync: {message}");
                info!("meta sync: `{}` failed", target.display());
                failures.push(message.clone());
                items.push(
                    serde_json::json!({"target": target, "status": "failed", "error": message}),
                );
            }
        }
    }
    let failed = failures.len();
    let text = format!(
        "meta sync: {updated_count} updated, {unchanged_count} unchanged, {failed} failed (from `{source_name}`)"
    );
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-sync",
            "source": args.source,
            "source_name": source_name,
            "fields": field_list,
            "origin": origin,
            "updated": updated_count,
            "unchanged": unchanged_count,
            "failed": failed,
            "errors": failures,
            "items": items,
            "status": if failed == 0 { "ok" } else { "partial" },
        }),
        &text,
    )?;
    info!("{text}");
    if failed != 0 {
        return Err(CliError::BatchPartial { failed });
    }
    Ok(())
}
