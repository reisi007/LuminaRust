use super::*;

/// LRPAR-G15-IPTC-S3: origin recorded for every CLI draft/history mutation
/// (S1 `validate_metadata_origin` accepts `cli`; no paths are ever written
/// into `metadata`, so no absolute path can leak into the sidecar).
const META_ORIGIN_CLI: &str = "cli";

/// LRPAR-G15-IPTC-S3: dispatches the `meta` subcommands (SOLL §8). All
/// draft/history mutations run over the same sidecar path (CAS, atomar,
/// loud); `inspect`/`history show` are read-only.
pub(crate) fn meta(args: MetaArgs) -> Result<(), CliError> {
    match args.command {
        MetaCommand::Inspect(inspect) => meta_inspect(inspect),
        MetaCommand::Draft(draft) => match draft.command {
            MetaDraftCommand::Set(set) => meta_draft_set(set),
            MetaDraftCommand::Clear(clear) => meta_draft_clear(clear),
        },
        MetaCommand::History(history) => match history.command {
            MetaHistoryCommand::Show(show) => meta_history_show(show),
            MetaHistoryCommand::Clear(clear) => meta_history_clear(clear),
        },
        MetaCommand::Preset(preset) => match preset.command {
            MetaPresetCommand::List(list) => meta_preset_list(list),
            MetaPresetCommand::Show(show) => meta_preset_show(show),
            MetaPresetCommand::Apply(apply) => meta_preset_apply(apply),
        },
        MetaCommand::Sync(sync) => meta_sync(sync),
        MetaCommand::Copy(copy) => meta_copy(copy),
        MetaCommand::Paste(paste) => meta_paste(paste),
    }
}

/// LRPAR-G15-IPTC-S3: reads the embedded IPTC of `bytes` when they are a
/// JPEG (IIM/XMP via `lumina-iptc`). Non-JPEG input (PNG/WebP/RAW/Raster)
/// yields `Ok(None)` — inspect reports this loudly as "nicht verfügbar".
/// Present-but-broken JPEG segments are a loud error, never a silent skip.
fn read_embedded_iptc(bytes: &[u8]) -> Result<Option<IptcMetadata>, CliError> {
    if bytes.len() < 2 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return Ok(None);
    }
    extract_metadata(bytes)
        .map(Some)
        .map_err(|error| CliError::Message(format!("embedded IPTC unreadable: {error}")))
}

/// LRPAR-G15-IPTC-S3: maps a registry field ID onto the embedded value.
/// `IptcMetadata` exposes no by-ID accessor, so the mapping lives here
/// (CLI-only, no second registry: IDs come from `METADATA_FIELD_IDS`).
fn embedded_field_value<'a>(meta: &'a IptcMetadata, id: &str) -> Option<&'a str> {
    match id {
        "title" => meta.title.as_deref(),
        "headline" => meta.headline.as_deref(),
        "description" => meta.description.as_deref(),
        "copyright_notice" => meta.copyright_notice.as_deref(),
        "creator" => meta.creator.as_deref(),
        "credit" => meta.credit.as_deref(),
        "source" => meta.source.as_deref(),
        "city" => meta.city.as_deref(),
        "state_province" => meta.state_province.as_deref(),
        "country" => meta.country.as_deref(),
        "date_created" => meta.date_created.as_deref(),
        _ => None,
    }
}

fn format_optional_value(value: Option<&str>) -> String {
    value.map_or("(absent)".to_string(), |v| format!("\"{v}\""))
}

/// LRPAR-G15-IPTC-S3: `meta inspect` — embedded IPTC (JPEG) or a loud
/// "nicht verfügbar", the draft overlay per field (draft vs. embedded),
/// keywords (draft vs. embedded) and the history length. Read-only.
fn meta_inspect(args: MetaInspectArgs) -> Result<(), CliError> {
    let (_, document) = require_sidecar(&args.path)?;
    let bytes = fs::read(&args.path).map_err(|error| io_error(&args.path, error))?;
    let embedded = read_embedded_iptc(&bytes)?;
    let history_len = document.metadata.history.len();
    let latest_rev = document.metadata.latest_rev();
    info!(
        "meta inspect for `{}` (embedded: {}, {} draft field(s), history: {} entries)",
        args.path.display(),
        if embedded.is_some() {
            "verfügbar"
        } else {
            "nicht verfügbar"
        },
        document.metadata.draft.len(),
        history_len
    );
    let mut draft_json = serde_json::Map::new();
    let mut embedded_json = serde_json::Map::new();
    let mut lines = Vec::with_capacity(METADATA_FIELD_IDS.len() + 3);
    lines.push(if embedded.is_some() {
        "embedded: verfügbar (JPEG, IIM/XMP)".to_string()
    } else {
        "embedded: nicht verfügbar (kein JPEG / kein IIM/XMP)".to_string()
    });
    for id in METADATA_FIELD_IDS {
        let draft = document.metadata.get(id);
        let embedded_value = embedded
            .as_ref()
            .and_then(|meta| embedded_field_value(meta, id));
        if let Some(value) = draft {
            draft_json.insert(
                (*id).to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
        if let Some(value) = embedded_value {
            embedded_json.insert(
                (*id).to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
        lines.push(format!(
            "{id}: draft={} embedded={}",
            format_optional_value(draft),
            format_optional_value(embedded_value)
        ));
    }
    let embedded_keywords: Vec<String> = embedded
        .as_ref()
        .map_or_else(Vec::new, |meta| meta.keywords.clone());
    lines.push(format!(
        "keywords: draft=[{}] embedded=[{}]",
        document.keywords.join(", "),
        embedded_keywords.join(", ")
    ));
    lines.push(format!(
        "history: {history_len} entries (latest rev {latest_rev})"
    ));
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-inspect",
            "input": args.path,
            "embedded_available": embedded.is_some(),
            "draft": draft_json,
            "embedded": embedded_json,
            "keywords": document.keywords,
            "keywords_embedded": embedded_keywords,
            "history_len": history_len,
            "history_latest_rev": latest_rev,
            "status": "ok",
        }),
        &lines.join("\n"),
    )
}

/// LRPAR-G15-IPTC-S3: splits a `--field` value at the first `=` into
/// `(id, value)`. A missing `=` is a loud error before anything is mutated.
fn split_field_assignment(value: &str) -> Result<(String, String), CliError> {
    value.split_once('=').map_or_else(
        || {
            Err(CliError::Message(format!(
                "invalid field assignment `{value}`: expected `ID=VALUE`"
            )))
        },
        |(id, field_value)| Ok((id.to_string(), field_value.to_string())),
    )
}

/// LRPAR-G15-IPTC-S3: `meta draft set` — all-or-nothing per call. Every
/// assignment is parsed and every draft value is validated (S1
/// `validate_metadata_field_value`, unknown IDs included) BEFORE any
/// mutation; draft + routed `keywords` land in ONE history entry
/// (`origin = "cli"`) on a clone, the clone is fully validated and the
/// write goes through CAS (`save_sidecar_if_unchanged`, atomar). Conflict =
/// loud error, never silent last-write-wins.
fn meta_draft_set(args: MetaDraftSetArgs) -> Result<(), CliError> {
    let (path, document) = require_sidecar(&args.path)?;
    let original_bytes = fs::read(&args.path).map_err(|error| io_error(&args.path, error))?;
    let mut draft_fields: Vec<(String, String)> = Vec::new();
    let mut keyword_entries: Vec<String> = Vec::new();
    for assignment in &args.field {
        let (id, value) = split_field_assignment(assignment)?;
        if id == "keywords" {
            keyword_entries.push(value);
        } else {
            draft_fields.push((id, value));
        }
    }
    for (field, value) in &draft_fields {
        validate_metadata_field_value(field, value).map_err(|error| {
            CliError::Message(format!(
                "meta draft set for `{}` rejected: {error}",
                args.path.display()
            ))
        })?;
    }
    let mut candidate = document.clone();
    let mut changed = BTreeSet::new();
    for (field, value) in &draft_fields {
        if value.is_empty() || value.trim().is_empty() {
            if candidate.metadata.draft.remove(field).is_some() {
                changed.insert(field.clone());
            }
        } else if candidate.metadata.draft.get(field).map(String::as_str) != Some(value.as_str()) {
            candidate
                .metadata
                .draft
                .insert(field.clone(), value.clone());
            changed.insert(field.clone());
        }
    }
    if !keyword_entries.is_empty() {
        let sole_empty = keyword_entries.len() == 1
            && (keyword_entries[0].is_empty() || keyword_entries[0].trim().is_empty());
        if !sole_empty {
            for entry in &keyword_entries {
                if entry.is_empty() || entry.trim() != entry {
                    return Err(CliError::Message(format!(
                        "meta draft set for `{}` rejected: keyword must be non-empty and without leading/trailing whitespace (use `meta draft clear --field keywords` to empty the list)",
                        args.path.display()
                    )));
                }
            }
        }
        let next: Vec<String> = if sole_empty {
            Vec::new()
        } else {
            keyword_entries.clone()
        };
        if next != candidate.keywords {
            candidate.keywords = next;
            changed.insert("keywords".to_string());
        }
    }
    let status_text = if changed.is_empty() {
        info!(
            "meta draft set for `{}` unchanged (idempotent no-op)",
            args.path.display()
        );
        format!(
            "meta draft set: unchanged ({} assignment(s))",
            args.field.len()
        )
    } else {
        let changed_list: Vec<String> = changed.into_iter().collect();
        let timestamp = now_rfc3339_utc();
        let rev = candidate.metadata.latest_rev() + 1;
        candidate.metadata.history.insert(
            0,
            MetadataHistoryEntry {
                rev,
                timestamp,
                origin: META_ORIGIN_CLI.to_string(),
                changed: changed_list.clone(),
            },
        );
        candidate
            .metadata
            .history
            .truncate(MAX_METADATA_HISTORY_ENTRIES);
        candidate.validate().map_err(|error| {
            CliError::Message(format!(
                "meta draft set for `{}` rejected: {error}",
                args.path.display()
            ))
        })?;
        let expected = document_revision(&document)?;
        let saved_rev = save_sidecar_if_unchanged(&path, &candidate, Some(&expected))?;
        debug_assert!(!saved_rev.is_empty());
        info!(
            "meta draft set for `{}` updated (rev {rev}, changed: {})",
            args.path.display(),
            changed_list.join(", ")
        );
        format!(
            "meta draft set: updated rev {rev} (changed: {})",
            changed_list.join(", ")
        )
    };
    // The original image is never modified by a metadata command.
    debug_assert_eq!(
        fs::read(&args.path).map_err(|error| io_error(&args.path, error))?,
        original_bytes
    );
    let reloaded = load_sidecar(&path)?;
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-draft-set",
            "input": args.path,
            "draft": reloaded.metadata.draft,
            "keywords": reloaded.keywords,
            "history_len": reloaded.metadata.history.len(),
            "history_latest_rev": reloaded.metadata.latest_rev(),
            "status": "ok",
        }),
        &status_text,
    )
}

/// LRPAR-G15-IPTC-S3: `meta draft clear` — exactly one of `--field` /
/// `--all`. Unknown IDs abort loudly with all-or-nothing semantics; the
/// history is kept and gains one entry listing the removed IDs (CAS, atomar).
fn meta_draft_clear(args: MetaDraftClearArgs) -> Result<(), CliError> {
    if args.all == !args.field.is_empty() {
        return Err(CliError::Message(
            "`meta draft clear` requires exactly one of `--field <id,…>` / `--all`".into(),
        ));
    }
    let (path, document) = require_sidecar(&args.path)?;
    let original_bytes = fs::read(&args.path).map_err(|error| io_error(&args.path, error))?;
    let expected = document_revision(&document)?;
    let timestamp = now_rfc3339_utc();
    let mut candidate = document.clone();
    let status_text;
    if args.all {
        if !candidate.clear_metadata_draft(META_ORIGIN_CLI, &timestamp)? {
            info!(
                "meta draft clear for `{}` unchanged (draft already empty)",
                args.path.display()
            );
            status_text = "meta draft clear: unchanged (draft already empty)".to_string();
            debug_assert_eq!(
                fs::read(&args.path).map_err(|error| io_error(&args.path, error))?,
                original_bytes
            );
            let reloaded = load_sidecar(&path)?;
            return emit(
                args.json,
                serde_json::json!({
                    "command": "meta-draft-clear",
                    "input": args.path,
                    "draft": reloaded.metadata.draft,
                    "keywords": reloaded.keywords,
                    "history_len": reloaded.metadata.history.len(),
                    "history_latest_rev": reloaded.metadata.latest_rev(),
                    "status": "ok",
                }),
                &status_text,
            );
        }
    } else {
        for id in &args.field {
            if id != "keywords" && !is_metadata_field(id) {
                return Err(CliError::Message(format!(
                    "meta draft clear for `{}` rejected: unknown metadata field `{id}`",
                    args.path.display()
                )));
            }
        }
        let mut removed = BTreeSet::new();
        for id in &args.field {
            if id == "keywords" {
                if !candidate.keywords.is_empty() {
                    candidate.keywords.clear();
                    removed.insert("keywords".to_string());
                }
            } else if candidate.metadata.draft.remove(id).is_some() {
                removed.insert(id.clone());
            }
        }
        if removed.is_empty() {
            info!(
                "meta draft clear for `{}` unchanged (fields already absent)",
                args.path.display()
            );
            status_text = "meta draft clear: unchanged (fields already absent)".to_string();
            debug_assert_eq!(
                fs::read(&args.path).map_err(|error| io_error(&args.path, error))?,
                original_bytes
            );
            let reloaded = load_sidecar(&path)?;
            return emit(
                args.json,
                serde_json::json!({
                    "command": "meta-draft-clear",
                    "input": args.path,
                    "draft": reloaded.metadata.draft,
                    "keywords": reloaded.keywords,
                    "history_len": reloaded.metadata.history.len(),
                    "history_latest_rev": reloaded.metadata.latest_rev(),
                    "status": "ok",
                }),
                &status_text,
            );
        }
        let removed_list: Vec<String> = removed.into_iter().collect();
        let rev = candidate.metadata.latest_rev() + 1;
        candidate.metadata.history.insert(
            0,
            MetadataHistoryEntry {
                rev,
                timestamp,
                origin: META_ORIGIN_CLI.to_string(),
                changed: removed_list,
            },
        );
        candidate
            .metadata
            .history
            .truncate(MAX_METADATA_HISTORY_ENTRIES);
        candidate.validate()?;
    }
    let rev = candidate.metadata.latest_rev();
    let changed_list = candidate
        .metadata
        .history
        .first()
        .map_or_else(Vec::new, |entry| entry.changed.clone());
    save_sidecar_if_unchanged(&path, &candidate, Some(&expected))?;
    info!(
        "meta draft clear for `{}` updated (rev {rev}, removed: {})",
        args.path.display(),
        changed_list.join(", ")
    );
    status_text = format!(
        "meta draft clear: updated rev {rev} (removed: {})",
        changed_list.join(", ")
    );
    // The original image is never modified by a metadata command.
    debug_assert_eq!(
        fs::read(&args.path).map_err(|error| io_error(&args.path, error))?,
        original_bytes
    );
    let reloaded = load_sidecar(&path)?;
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-draft-clear",
            "input": args.path,
            "draft": reloaded.metadata.draft,
            "keywords": reloaded.keywords,
            "history_len": reloaded.metadata.history.len(),
            "history_latest_rev": reloaded.metadata.latest_rev(),
            "status": "ok",
        }),
        &status_text,
    )
}

/// LRPAR-G15-IPTC-S3: `meta history show` — lists entries newest-first,
/// optionally limited. Read-only.
fn meta_history_show(args: MetaHistoryShowArgs) -> Result<(), CliError> {
    let (_, document) = require_sidecar(&args.path)?;
    let total = document.metadata.history.len();
    let mut entries = document.metadata.history.clone();
    if let Some(limit) = args.limit {
        entries.truncate(limit);
    }
    info!(
        "meta history show for `{}` ({} of {total} entries)",
        args.path.display(),
        entries.len()
    );
    let lines: Vec<String> = entries
        .iter()
        .map(|entry| {
            format!(
                "rev {} | {} | {} | {}",
                entry.rev,
                entry.timestamp,
                entry.origin,
                entry.changed.join(", ")
            )
        })
        .collect();
    let text = if lines.is_empty() {
        "history: (empty)".to_string()
    } else {
        format!(
            "history ({} of {total}):\n{}",
            entries.len(),
            lines.join("\n")
        )
    };
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-history-show",
            "input": args.path,
            "history": entries,
            "shown": entries.len(),
            "total": total,
            "status": "ok",
        }),
        &text,
    )
}

/// LRPAR-G15-IPTC-S3: `meta history clear` — explicitly clears the whole
/// history (the only way to empty it; no undo). Draft values are kept.
/// CAS + atomar; clearing an empty history is an idempotent no-op.
fn meta_history_clear(args: MetaHistoryClearArgs) -> Result<(), CliError> {
    let (path, mut document) = require_sidecar(&args.path)?;
    let original_bytes = fs::read(&args.path).map_err(|error| io_error(&args.path, error))?;
    let status_text = if document.metadata.history.is_empty() {
        info!(
            "meta history clear for `{}` unchanged (history already empty)",
            args.path.display()
        );
        "meta history clear: unchanged (history already empty)".to_string()
    } else {
        let removed = document.metadata.history.len();
        let expected = document_revision(&document)?;
        document.clear_metadata_history();
        document.validate()?;
        save_sidecar_if_unchanged(&path, &document, Some(&expected))?;
        info!(
            "meta history clear for `{}` removed {removed} entries (draft kept)",
            args.path.display()
        );
        format!("meta history clear: removed {removed} entries (draft kept)")
    };
    // The original image is never modified by a metadata command.
    debug_assert_eq!(
        fs::read(&args.path).map_err(|error| io_error(&args.path, error))?,
        original_bytes
    );
    let reloaded = load_sidecar(&path)?;
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-history-clear",
            "input": args.path,
            "draft": reloaded.metadata.draft,
            "keywords": reloaded.keywords,
            "history_len": reloaded.metadata.history.len(),
            "status": "ok",
        }),
        &status_text,
    )
}

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
fn meta_preset_list(args: MetaPresetListArgs) -> Result<(), CliError> {
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
fn meta_preset_show(args: MetaPresetShowArgs) -> Result<(), CliError> {
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
fn meta_preset_apply(args: MetaPresetApplyArgs) -> Result<(), CliError> {
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
fn meta_sync(args: MetaSyncArgs) -> Result<(), CliError> {
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

/// META-COPYPASTE-1: CLI meta clipboard file format marker + version (SOLL §8).
pub(crate) const META_CLIPBOARD_FORMAT: &str = "lumina-meta-clipboard";
pub(crate) const META_CLIPBOARD_VERSION: u8 = 1;

/// META-COPYPASTE-1: default clipboard path in the OS temp directory. Explicit
/// and ephemeral on purpose — never a CWD dotfile that could be committed or
/// silently shared between checkouts (SOLL §8).
pub(crate) fn default_meta_clipboard_path() -> PathBuf {
    std::env::temp_dir().join("lumina-meta-clipboard.json")
}

/// META-COPYPASTE-1: the versioned, portable CLI clipboard file. Holds only
/// non-empty values (a clipboard can never encode a deletion). `source` is the
/// source file name only — never a path.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct MetaClipboardFile {
    pub(crate) format: String,
    pub(crate) version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) fields: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) keywords: Vec<String>,
}

impl MetaClipboardFile {
    /// Field IDs stored in this clipboard (draft IDs plus `keywords` when a
    /// non-empty keyword list was captured).
    pub(crate) fn stored_ids(&self) -> BTreeSet<String> {
        let mut ids: BTreeSet<String> = self.fields.keys().cloned().collect();
        if !self.keywords.is_empty() {
            ids.insert("keywords".to_string());
        }
        ids
    }
}

/// META-COPYPASTE-1: validates a `--fields` list (registry IDs from SOLL §4,
/// `keywords` allowed) into a deduplicated set. Empty entries and unknown IDs
/// are loud errors before anything is read or written.
pub(crate) fn parse_meta_fields(
    fields: &[String],
    context: &str,
) -> Result<BTreeSet<String>, CliError> {
    if fields.iter().any(|id| id.is_empty()) {
        return Err(CliError::Message(format!(
            "{context} rejected: empty metadata field ID in `--fields`"
        )));
    }
    let mut set = BTreeSet::new();
    for id in fields {
        if id != "keywords" && !is_metadata_field(id) {
            return Err(CliError::Message(format!(
                "{context} rejected: unknown metadata field `{id}`"
            )));
        }
        set.insert(id.clone());
    }
    Ok(set)
}

/// META-COPYPASTE-1: loads and validates a clipboard file. Every deviation
/// (missing/unreadable file, invalid JSON, wrong format/version, unknown or
/// empty field values, invalid keywords) is a loud error — there is no silent
/// fallback to an empty clipboard. Empty values are rejected because paste is
/// additive and an empty value would otherwise mean "remove the field".
pub(crate) fn load_meta_clipboard(path: &Path) -> Result<MetaClipboardFile, CliError> {
    let json = fs::read_to_string(path).map_err(|error| io_error(path, error))?;
    let clipboard: MetaClipboardFile = serde_json::from_str(&json).map_err(|error| {
        CliError::Message(format!(
            "invalid metadata clipboard `{}`: {error}",
            path.display()
        ))
    })?;
    if clipboard.format != META_CLIPBOARD_FORMAT {
        return Err(CliError::Message(format!(
            "invalid metadata clipboard `{}`: expected format \"{META_CLIPBOARD_FORMAT}\", got \"{}\"",
            path.display(),
            clipboard.format
        )));
    }
    if clipboard.version != META_CLIPBOARD_VERSION {
        return Err(CliError::Message(format!(
            "unsupported metadata clipboard version {} in `{}` (expected {META_CLIPBOARD_VERSION})",
            clipboard.version,
            path.display()
        )));
    }
    for (id, value) in &clipboard.fields {
        if id == "keywords" {
            return Err(CliError::Message(format!(
                "invalid metadata clipboard `{}`: `keywords` must not appear inside `fields`",
                path.display()
            )));
        }
        validate_metadata_field_value(id, value).map_err(|error| {
            CliError::Message(format!(
                "invalid metadata clipboard `{}`: {error}",
                path.display()
            ))
        })?;
        if value.is_empty() || value.trim().is_empty() {
            return Err(CliError::Message(format!(
                "invalid metadata clipboard `{}`: field `{id}` is empty (paste never deletes fields)",
                path.display()
            )));
        }
    }
    if clipboard.keywords.len() > MAX_KEYWORDS_PER_DOCUMENT {
        return Err(CliError::Message(format!(
            "invalid metadata clipboard `{}`: keyword list exceeds limit of {MAX_KEYWORDS_PER_DOCUMENT}",
            path.display()
        )));
    }
    for keyword in &clipboard.keywords {
        if keyword.is_empty() || keyword.trim() != keyword {
            return Err(CliError::Message(format!(
                "invalid metadata clipboard `{}`: keyword must be non-empty and without leading/trailing whitespace",
                path.display()
            )));
        }
        if keyword.chars().count() > MAX_KEYWORD_CHARS {
            return Err(CliError::Message(format!(
                "invalid metadata clipboard `{}`: keyword exceeds limit of {MAX_KEYWORD_CHARS} characters",
                path.display()
            )));
        }
    }
    Ok(clipboard)
}

/// META-COPYPASTE-1: `meta copy` — writes the selected non-empty draft fields
/// (+ non-empty keywords) of `path` into an explicit clipboard file. The
/// sidecar is read-only here; no value is normalized or removed. Missing
/// source sidecar and unknown `--fields` IDs are loud (exit 1). An empty
/// selection still writes the file and is reported as `empty` (exit 0) — it is
/// never pasted silently.
fn meta_copy(args: MetaCopyArgs) -> Result<(), CliError> {
    let context = format!("meta copy for `{}`", args.path.display());
    let selection = parse_meta_fields(&args.fields, &context)?;
    let (_, document) = require_sidecar(&args.path)?;
    let out = args.out.clone().unwrap_or_else(default_meta_clipboard_path);
    // META-COPYPASTE-2: the clipboard file must never clobber the original
    // source or its Lumina bundle (`<input>.lumina.json`/`.lumina.zdata`,
    // including hard links) — same non-destructive guard as the export paths.
    reject_protected_output(&args.path, &out)?;
    let source = args
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            CliError::Message(format!(
                "meta copy for `{}` rejected: source has no file name",
                args.path.display()
            ))
        })?;
    let copy_all = selection.is_empty();
    let mut fields = BTreeMap::new();
    for (id, value) in &document.metadata.draft {
        if (copy_all || selection.contains(id)) && !value.trim().is_empty() {
            fields.insert(id.clone(), value.clone());
        }
    }
    let keywords = if copy_all || selection.contains("keywords") {
        document.keywords.clone()
    } else {
        Vec::new()
    };
    let field_ids: Vec<String> = fields.keys().cloned().collect();
    let clipboard = MetaClipboardFile {
        format: META_CLIPBOARD_FORMAT.to_string(),
        version: META_CLIPBOARD_VERSION,
        source: Some(source.clone()),
        fields,
        keywords,
    };
    let json = serde_json::to_string_pretty(&clipboard).map_err(|error| {
        CliError::Message(format!("cannot serialize metadata clipboard: {error}"))
    })?;
    fs::write(&out, json).map_err(|error| io_error(&out, error))?;
    let empty = field_ids.is_empty() && clipboard.keywords.is_empty();
    if empty {
        info!(
            "meta copy for `{}` wrote an empty clipboard to `{}` (no non-empty metadata selected)",
            args.path.display(),
            out.display()
        );
    } else {
        info!(
            "meta copy for `{}` wrote {} field(s) to `{}` (source `{source}`)",
            args.path.display(),
            field_ids.len() + usize::from(!clipboard.keywords.is_empty()),
            out.display()
        );
    }
    let text = if empty {
        format!(
            "meta copy: clipboard `{}` is empty (no non-empty metadata to copy)",
            out.display()
        )
    } else {
        format!(
            "meta copy: wrote {} field(s) to `{}` (source `{source}`)",
            field_ids.len() + usize::from(!clipboard.keywords.is_empty()),
            out.display()
        )
    };
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-copy",
            "input": args.path,
            "clipboard": out,
            "source": source,
            "fields": field_ids,
            "keywords": clipboard.keywords,
            "status": if empty { "empty" } else { "ok" },
        }),
        &text,
    )
}

/// META-COPYPASTE-1: applies the selected clipboard fields to one target. Like
/// `meta sync` this is one CAS + atomic write with a single history entry, but
/// purely additive: clipboard values overwrite their field, nothing else is
/// ever removed (no mirror semantics). `Ok(false)` = idempotent no-op.
fn apply_meta_paste_to_target(
    target: &Path,
    clipboard: &MetaClipboardFile,
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
            if candidate.keywords != clipboard.keywords {
                candidate.keywords = clipboard.keywords.clone();
                changed.insert(id.clone());
            }
        } else if let Some(value) = clipboard.fields.get(id) {
            // Non-empty by construction (validated on load) — a pure
            // overwrite; it can never remove another field.
            if candidate.metadata.draft.get(id).map(String::as_str) != Some(value.as_str()) {
                candidate.metadata.draft.insert(id.clone(), value.clone());
                changed.insert(id.clone());
            }
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

/// META-COPYPASTE-1: `meta paste` — loads/validates the clipboard upfront
/// (missing/invalid file, empty selection and unknown or not-stored `--fields`
/// IDs abort everything with exit 1, nothing written), then applies the
/// selection per target in isolation (updated / unchanged / failed; exit 3 on
/// partial failure). Additive only: no other target field is removed; a
/// selected `keywords` replaces the target list as a whole. A missing target
/// sidecar fails only its own item, never a silent creation.
fn meta_paste(args: MetaPasteArgs) -> Result<(), CliError> {
    let clipboard_path = args
        .clipboard
        .clone()
        .unwrap_or_else(default_meta_clipboard_path);
    let clipboard = load_meta_clipboard(&clipboard_path)?;
    let context = format!("meta paste from `{}`", clipboard_path.display());
    let requested = parse_meta_fields(&args.fields, &context)?;
    let stored = clipboard.stored_ids();
    let fields = if requested.is_empty() {
        stored
    } else {
        for id in &requested {
            if !stored.contains(id) {
                return Err(CliError::Message(format!(
                    "{context} rejected: field `{id}` is not present in the clipboard"
                )));
            }
        }
        requested
    };
    if fields.is_empty() {
        return Err(CliError::Message(format!(
            "metadata clipboard `{}` is empty; run `meta copy` first",
            clipboard_path.display()
        )));
    }
    let field_list: Vec<String> = fields.iter().cloned().collect();
    let origin = META_ORIGIN_CLI.to_string();
    let source = clipboard
        .source
        .clone()
        .unwrap_or_else(|| "(unbekannt)".to_string());
    info!(
        "meta paste from `{}` (source `{source}`) to {} target(s) ({} field(s): {})",
        clipboard_path.display(),
        args.target.len(),
        field_list.len(),
        field_list.join(", ")
    );
    let mut updated_count = 0usize;
    let mut unchanged_count = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut items = Vec::with_capacity(args.target.len());
    for target in &args.target {
        match apply_meta_paste_to_target(target, &clipboard, &fields, &origin) {
            Ok(true) => {
                info!(
                    "meta paste: `{}` updated (from clipboard `{}`)",
                    target.display(),
                    clipboard_path.display()
                );
                updated_count += 1;
                items.push(serde_json::json!({"target": target, "status": "updated"}));
            }
            Ok(false) => {
                info!(
                    "meta paste: `{}` unchanged (already matches the clipboard)",
                    target.display()
                );
                unchanged_count += 1;
                items.push(serde_json::json!({"target": target, "status": "unchanged"}));
            }
            Err(error) => {
                let message = format!("{}: {error}", target.display());
                eprintln!("error: meta paste: {message}");
                info!("meta paste: `{}` failed", target.display());
                failures.push(message.clone());
                items.push(
                    serde_json::json!({"target": target, "status": "failed", "error": message}),
                );
            }
        }
    }
    let failed = failures.len();
    let text = format!(
        "meta paste: {updated_count} updated, {unchanged_count} unchanged, {failed} failed (from `{}`)",
        clipboard_path.display()
    );
    emit(
        args.json,
        serde_json::json!({
            "command": "meta-paste",
            "clipboard": clipboard_path,
            "source": source,
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
