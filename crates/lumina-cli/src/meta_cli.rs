use super::*;

/// LRPAR-G15-IPTC-S3: origin recorded for every CLI draft/history mutation
/// (S1 `validate_metadata_origin` accepts `cli`; no paths are ever written
/// into `metadata`, so no absolute path can leak into the sidecar).
pub(crate) const META_ORIGIN_CLI: &str = "cli";

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
