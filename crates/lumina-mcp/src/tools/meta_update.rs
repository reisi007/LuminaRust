//! `lumina_update_metadata_draft` — mutate the draft of one image (path-based,
//! write-through per CAS).

use crate::error::McpError;
use crate::tools::meta_common::{
    expected_revision, map_sidecar_error, require_sidecar_for, META_ORIGIN_MCP,
};
use crate::util::get_str;
use crate::Server;
use lumina_sidecar::{
    is_metadata_field, now_rfc3339_utc, save_sidecar_if_unchanged, validate_metadata_field_value,
    SidecarDocument, MAX_METADATA_HISTORY_ENTRIES,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const NAME: &str = "lumina_update_metadata_draft";
pub const DESCRIPTION: &str = "Update the IPTC metadata draft of an image by path: set draft \
fields via `fields` (empty values remove the field) and/or remove fields via `clear_fields`. \
Write-through per compare-and-swap; a concurrently modified sidecar surfaces as SidecarConflict \
(-32010, analog lumina_edit). Exactly one history entry (origin `mcp`) per updating call; \
idempotent no-ops write nothing.";

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Path to the source image." },
            "fields": {
                "type": "object",
                "description": "Draft field id → value (registry ids; empty values remove the field).",
                "additionalProperties": { "type": "string" }
            },
            "clear_fields": {
                "type": "array",
                "description": "Draft field ids to remove.",
                "items": { "type": "string" }
            }
        },
        "required": ["path"]
    })
}

pub fn run(_server: &mut Server, args: &Value) -> Result<Value, McpError> {
    let path_str = get_str(args, "path")?;
    let path = Path::new(path_str);
    if !path.exists() {
        return Err(McpError::FileNotFound(path_str.to_string()));
    }
    let fields = parse_fields(args)?;
    let clear_fields = parse_clear_fields(args)?;

    let (_sidecar_path, document) = require_sidecar_for(path)?;
    let sidecar_path = lumina_sidecar::sidecar_path_for(path);
    let rev = write_metadata_update(&sidecar_path, &document, &fields, &clear_fields)?;
    log::info!("lumina_update_metadata_draft for `{path_str}` (rev {rev})");
    Ok(json!({ "ok": true, "rev": rev }))
}

/// Parses the optional `fields` object (string values only; `null` = absent).
fn parse_fields(args: &Value) -> Result<BTreeMap<String, String>, McpError> {
    let Some(value) = args.get("fields").filter(|value| !value.is_null()) else {
        return Ok(BTreeMap::new());
    };
    let object = value.as_object().ok_or_else(|| {
        McpError::InvalidParams("`fields` must be an object of field id → string value".into())
    })?;
    let mut fields = BTreeMap::new();
    for (key, field_value) in object {
        let text = field_value.as_str().ok_or_else(|| {
            McpError::InvalidParams(format!(
                "field `{key}` must have a string value (got `{field_value}`)"
            ))
        })?;
        fields.insert(key.clone(), text.to_string());
    }
    Ok(fields)
}

/// Parses the optional `clear_fields` array (string ids only; `null` = absent).
fn parse_clear_fields(args: &Value) -> Result<Vec<String>, McpError> {
    let Some(value) = args.get("clear_fields").filter(|value| !value.is_null()) else {
        return Ok(Vec::new());
    };
    let array = value.as_array().ok_or_else(|| {
        McpError::InvalidParams("`clear_fields` must be an array of field ids".into())
    })?;
    let mut ids = Vec::with_capacity(array.len());
    for entry in array {
        let id = entry.as_str().ok_or_else(|| {
            McpError::InvalidParams(format!(
                "`clear_fields` entries must be strings (got `{entry}`)"
            ))
        })?;
        ids.push(id.to_string());
    }
    Ok(ids)
}

/// Validates field ids and values upfront (all-or-nothing per call, before any
/// IO): registry ids only — `keywords` is rejected with its routing hint —
/// values against the S1 registry limits. A field listed in both `fields` and
/// `clear_fields` is a loud contradiction, not a silent precedence.
fn validate_update_args(
    fields: &BTreeMap<String, String>,
    clear_fields: &[String],
) -> Result<(), McpError> {
    for id in fields.keys().chain(clear_fields.iter()) {
        if id == "keywords" {
            return Err(McpError::InvalidParams(
                "keywords is not a metadata draft field; keywords stay the document \
                 `keywords` field and are carried by sync, not by draft updates"
                    .into(),
            ));
        }
        if !is_metadata_field(id) {
            return Err(McpError::InvalidParams(format!(
                "unknown metadata field `{id}`"
            )));
        }
    }
    for id in clear_fields {
        if fields.contains_key(id) {
            return Err(McpError::InvalidParams(format!(
                "field `{id}` is listed in both `fields` and `clear_fields`; refusing a silent precedence"
            )));
        }
    }
    for (field, value) in fields {
        validate_metadata_field_value(field, value).map_err(|error| {
            McpError::InvalidParams(format!("field `{field}` rejected: {error}"))
        })?;
    }
    Ok(())
}

/// Applies a validated update to `document` (a fresh load by the caller) and
/// persists it per compare-and-swap against its own revision. Returns the
/// latest history `rev` afterwards (`Ok` with no write for idempotent no-ops).
pub fn write_metadata_update(
    sidecar_path: &Path,
    document: &SidecarDocument,
    fields: &BTreeMap<String, String>,
    clear_fields: &[String],
) -> Result<u64, McpError> {
    let expected = expected_revision(document)?;
    write_metadata_update_expected(sidecar_path, document, fields, clear_fields, &expected)
}

/// [`write_metadata_update`] against an explicit CAS expectation. A stale
/// expectation surfaces as [`McpError::SidecarConflict`] (`-32010`) instead of
/// a silent lost update. `pub` so the conflict path stays deterministically
/// testable (a path-based live call cannot interleave an external writer
/// between its own read and write on demand).
pub fn write_metadata_update_expected(
    sidecar_path: &Path,
    document: &SidecarDocument,
    fields: &BTreeMap<String, String>,
    clear_fields: &[String],
    expected_revision: &str,
) -> Result<u64, McpError> {
    validate_update_args(fields, clear_fields)?;
    let mut candidate = document.clone();
    let mut changed = BTreeSet::new();
    for (field, value) in fields {
        let next = if value.is_empty() || value.trim().is_empty() {
            None
        } else {
            Some(value.as_str())
        };
        if candidate.metadata.draft.get(field).map(String::as_str) != next {
            match next {
                Some(text) => {
                    candidate
                        .metadata
                        .draft
                        .insert(field.clone(), text.to_string());
                }
                None => {
                    candidate.metadata.draft.remove(field);
                }
            }
            changed.insert(field.clone());
        }
    }
    for id in clear_fields {
        if candidate.metadata.draft.remove(id).is_some() {
            changed.insert(id.clone());
        }
    }
    if changed.is_empty() {
        return Ok(candidate.metadata.latest_rev());
    }
    let changed_list: Vec<String> = changed.into_iter().collect();
    let rev = candidate.metadata.latest_rev() + 1;
    candidate.metadata.history.insert(
        0,
        lumina_sidecar::MetadataHistoryEntry {
            rev,
            timestamp: now_rfc3339_utc(),
            origin: META_ORIGIN_MCP.to_string(),
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
    save_sidecar_if_unchanged(sidecar_path, &candidate, Some(expected_revision))
        .map_err(map_sidecar_error)?;
    Ok(rev)
}
