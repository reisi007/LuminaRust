//! `lumina_batch_sync_metadata` — field-selective draft sync from one source
//! image to many targets (path-based per-path report, SOLL §6 mirror semantics).

use crate::error::McpError;
use crate::tools::meta_common::{expected_revision, map_sidecar_error, require_sidecar_for};
use crate::util::get_str;
use crate::Server;
use lumina_sidecar::{
    is_metadata_field, now_rfc3339_utc, save_sidecar_if_unchanged, MetadataHistoryEntry,
    MAX_METADATA_HISTORY_ENTRIES,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const NAME: &str = "lumina_batch_sync_metadata";
pub const DESCRIPTION: &str = "Copy selected draft fields (plus keywords) from one source image \
to several target images by path. `fields` is required — there is no silent transfer-all. Mirror \
semantics: a selected field absent in the source is removed on the targets; keywords are replaced \
wholesale. Per-target CAS + atomic save; errors never abort the series. Per-path report with \
updated/unchanged/failed.";

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "source": { "type": "string", "description": "Source image path whose draft is mirrored." },
            "targets": {
                "type": "array",
                "description": "Target image paths.",
                "items": { "type": "string" },
                "minItems": 1
            },
            "fields": {
                "type": "array",
                "description": "Field ids to transfer (registry ids, `keywords` allowed). Required.",
                "items": { "type": "string" },
                "minItems": 1
            }
        },
        "required": ["source", "targets", "fields"]
    })
}

pub fn run(_server: &mut Server, args: &Value) -> Result<Value, McpError> {
    let source_str = get_str(args, "source")?;
    let source = Path::new(source_str);
    if !source.exists() {
        return Err(McpError::FileNotFound(source_str.to_string()));
    }
    let targets = parse_string_array(args, "targets")?;
    let fields = parse_string_array(args, "fields")?;
    if fields.iter().all(String::is_empty) {
        return Err(McpError::InvalidParams(
            "`fields` is required (registry field ids, `keywords` allowed); refusing a silent transfer-all".into(),
        ));
    }
    let mut selected = BTreeSet::new();
    for id in &fields {
        if id != "keywords" && !is_metadata_field(id) {
            return Err(McpError::InvalidParams(format!(
                "unknown metadata field `{id}`"
            )));
        }
        selected.insert(id.clone());
    }

    let (_source_sidecar, source_document) = require_sidecar_for(source)?;
    let source_draft = source_document.metadata.draft.clone();
    let source_keywords = source_document.keywords.clone();
    let source_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| McpError::InvalidParams("source has no file name".into()))?;
    let origin = format!("sync:{source_name}");
    let field_list: Vec<String> = selected.iter().cloned().collect();
    log::info!(
        "lumina_batch_sync_metadata from `{source_name}` to {} target(s) ({} field(s): {})",
        targets.len(),
        field_list.len(),
        field_list.join(", ")
    );

    let mut updated_count = 0usize;
    let mut unchanged_count = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut items = Vec::with_capacity(targets.len());
    for target in &targets {
        match apply_sync_to_target(
            Path::new(target),
            &source_draft,
            &source_keywords,
            &selected,
            &origin,
        ) {
            Ok(true) => {
                log::info!("lumina_batch_sync_metadata: `{target}` updated (from `{source_name}`)");
                updated_count += 1;
                items.push(json!({"path": target, "status": "updated"}));
            }
            Ok(false) => {
                log::info!("lumina_batch_sync_metadata: `{target}` unchanged");
                unchanged_count += 1;
                items.push(json!({"path": target, "status": "unchanged"}));
            }
            Err(error) => {
                let message = format!("{target}: {}", error.message());
                log::info!(
                    "lumina_batch_sync_metadata: `{target}` failed ({})",
                    error.name()
                );
                failures.push(message.clone());
                items.push(json!({
                    "path": target,
                    "status": "failed",
                    "error_name": error.name(),
                    "error": message,
                }));
            }
        }
    }
    let failed = failures.len();
    log::info!(
        "lumina_batch_sync_metadata: {updated_count} updated, {unchanged_count} unchanged, {failed} failed"
    );
    Ok(json!({
        "source": source_str,
        "fields": field_list,
        "updated": updated_count,
        "unchanged": unchanged_count,
        "failed": failed,
        "errors": failures,
        "items": items,
        "status": if failed == 0 { "ok" } else { "partial" },
    }))
}

fn parse_string_array(args: &Value, key: &str) -> Result<Vec<String>, McpError> {
    let array = args.get(key).and_then(Value::as_array).ok_or_else(|| {
        McpError::InvalidParams(format!("missing `{key}` array (at least one entry)"))
    })?;
    if array.is_empty() {
        return Err(McpError::InvalidParams(format!(
            "`{key}` must contain at least one entry"
        )));
    }
    let mut out = Vec::with_capacity(array.len());
    for entry in array {
        let text = entry.as_str().ok_or_else(|| {
            McpError::InvalidParams(format!("`{key}` entries must be strings (got `{entry}`)"))
        })?;
        out.push(text.to_string());
    }
    Ok(out)
}

/// Mirrors the selected source fields onto one target (CLI
/// `apply_meta_sync_to_target` parity, S5): draft values copied,
/// source-absent fields removed, `keywords` replaced wholesale. Exactly one
/// history entry (`origin = "sync:<source-file-name>"`) per updating call;
/// `Ok(false)` for idempotent no-ops. Recipes, masks and per-copy edit
/// history are never touched.
fn apply_sync_to_target(
    target: &Path,
    source_draft: &BTreeMap<String, String>,
    source_keywords: &[String],
    fields: &BTreeSet<String>,
    origin: &str,
) -> Result<bool, McpError> {
    if !target.exists() {
        return Err(McpError::FileNotFound(target.display().to_string()));
    }
    let (sidecar_path, document) = require_sidecar_for(target)?;
    let expected = expected_revision(&document)?;
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
        return Ok(false);
    }
    let changed_list: Vec<String> = changed.into_iter().collect();
    let rev = candidate.metadata.latest_rev() + 1;
    candidate.metadata.history.insert(
        0,
        MetadataHistoryEntry {
            rev,
            timestamp: now_rfc3339_utc(),
            origin: origin.to_string(),
            changed: changed_list,
        },
    );
    candidate
        .metadata
        .history
        .truncate(MAX_METADATA_HISTORY_ENTRIES);
    candidate
        .validate()
        .map_err(|error| McpError::Sidecar(format!("{error}")))?;
    save_sidecar_if_unchanged(&sidecar_path, &candidate, Some(&expected))
        .map_err(map_sidecar_error)?;
    Ok(true)
}
