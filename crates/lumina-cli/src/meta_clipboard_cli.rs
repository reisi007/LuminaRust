use super::*;
use crate::meta_cli::META_ORIGIN_CLI;

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
pub(super) fn meta_copy(args: MetaCopyArgs) -> Result<(), CliError> {
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
pub(super) fn meta_paste(args: MetaPasteArgs) -> Result<(), CliError> {
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
