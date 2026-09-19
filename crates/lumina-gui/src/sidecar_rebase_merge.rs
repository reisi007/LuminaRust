//! SIDECAR-REBASE-1: pure JSON three-way merge for the rebase helpers.
//!
//! Split out of `sidecar_rebase.rs` (file-size ratchet) — it holds no IO and
//! no GUI state, only the deterministic merge described in that module's
//! header (objects recursively, identity-keyed arrays per item, atomic
//! scalars/arrays with the local last writer winning). `merge_documents`
//! revalidates the result through [`SidecarDocument::from_json`], so a merge
//! that would produce an invalid sidecar fails loudly instead of persisting.

use lumina_sidecar::{SidecarDocument, SidecarError};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Three-way merge of the serialized documents. Returns the merged document
/// plus the JSON paths where the local (last-writer) value overrode a
/// divergent concurrent change.
pub(crate) fn merge_documents(
    base: &SidecarDocument,
    local: &SidecarDocument,
    current: &SidecarDocument,
) -> Result<(SidecarDocument, Vec<String>), SidecarError> {
    let base = serde_json::to_value(base).map_err(json_error)?;
    let local = serde_json::to_value(local).map_err(json_error)?;
    let current = serde_json::to_value(current).map_err(json_error)?;
    let mut overwritten = Vec::new();
    let mut path = Vec::new();
    let merged = merge_value(&base, &local, &current, &mut path, &mut overwritten);
    let json = serde_json::to_string(&merged).map_err(json_error)?;
    let document = SidecarDocument::from_json(&json)?;
    Ok((document, overwritten))
}

fn json_error(error: serde_json::Error) -> SidecarError {
    SidecarError::Json(error.to_string())
}

/// Recursive three-way merge of one JSON value. `base` is the common ancestor,
/// `local` the last writer's state, `current` the file on disk.
fn merge_value(
    base: &Value,
    local: &Value,
    current: &Value,
    path: &mut Vec<String>,
    overwritten: &mut Vec<String>,
) -> Value {
    if local == base {
        return current.clone();
    }
    if current == base || local == current {
        return local.clone();
    }
    match (base, local, current) {
        (Value::Object(base), Value::Object(local), Value::Object(current)) => {
            merge_objects(base, local, current, path, overwritten)
        }
        (Value::Array(base_arr), Value::Array(local_arr), Value::Array(current_arr)) => {
            merge_keyed_arrays(base_arr, local_arr, current_arr, path, overwritten)
                .unwrap_or_else(|| conflict_local(local, path, overwritten))
        }
        _ => conflict_local(local, path, overwritten),
    }
}

/// Both sides changed a leaf/atomic value divergently: the local (last writer)
/// value wins, recorded so the caller can log it.
fn conflict_local(local: &Value, path: &[String], overwritten: &mut Vec<String>) -> Value {
    overwritten.push(format_path(path));
    local.clone()
}

fn merge_objects(
    base: &Map<String, Value>,
    local: &Map<String, Value>,
    current: &Map<String, Value>,
    path: &mut Vec<String>,
    overwritten: &mut Vec<String>,
) -> Value {
    let mut keys: BTreeSet<&String> = BTreeSet::new();
    keys.extend(base.keys());
    keys.extend(local.keys());
    keys.extend(current.keys());
    let mut merged = Map::new();
    for key in keys {
        path.push(key.clone());
        let value = merge_opt(
            base.get(key),
            local.get(key),
            current.get(key),
            path,
            overwritten,
        );
        path.pop();
        if let Some(value) = value {
            merged.insert(key.clone(), value);
        }
    }
    Value::Object(merged)
}

/// Merge one possibly-absent object key. `None` means the key is absent.
fn merge_opt(
    base: Option<&Value>,
    local: Option<&Value>,
    current: Option<&Value>,
    path: &mut Vec<String>,
    overwritten: &mut Vec<String>,
) -> Option<Value> {
    if local == base {
        return current.cloned();
    }
    if current == base || local == current {
        return local.cloned();
    }
    match (base, local, current) {
        (Some(base), Some(local), Some(current)) => {
            Some(merge_value(base, local, current, path, overwritten))
        }
        // Both sides touched a previously-absent key: last writer wins.
        (None, Some(local), _) => Some(conflict_local(local, path, overwritten)),
        // We changed the value, the foreign writer removed it.
        (Some(_), Some(local), None) => Some(conflict_local(local, path, overwritten)),
        // We removed the key while the foreign writer changed it: our removal
        // is the last write.
        (_, None, _) => {
            overwritten.push(format_path(path));
            None
        }
    }
}

/// Merge three arrays by a common object identity key. Returns `None` when no
/// such key exists (the caller then treats the array atomically).
fn merge_keyed_arrays(
    base: &[Value],
    local: &[Value],
    current: &[Value],
    path: &mut Vec<String>,
    overwritten: &mut Vec<String>,
) -> Option<Value> {
    let key = identity_key(base, local, current)?;
    let (base, _base_order) = index_by_key(base, &key)?;
    let (local, local_order) = index_by_key(local, &key)?;
    let (current, current_order) = index_by_key(current, &key)?;
    // Preserve the on-disk order, then append local-only additions in local
    // order. Removed entries drop out through the per-key merge.
    let mut order: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for id in current_order.into_iter().chain(local_order) {
        if seen.insert(id.clone()) {
            order.push(id);
        }
    }
    let mut merged = Vec::new();
    for id in order {
        path.push(format!("{key}={id}"));
        let value = merge_opt(
            base.get(&id),
            local.get(&id),
            current.get(&id),
            path,
            overwritten,
        );
        path.pop();
        if let Some(value) = value {
            merged.push(value);
        }
    }
    Some(Value::Array(merged))
}

/// The shared identity key present on every element of all three arrays, if
/// any. Only stable, sidecar-defined keys are considered.
fn identity_key(base: &[Value], local: &[Value], current: &[Value]) -> Option<String> {
    ["id", "mask_id", "name"].iter().find_map(|candidate| {
        let all_present = base
            .iter()
            .chain(local.iter())
            .chain(current.iter())
            .all(|item| {
                item.as_object()
                    .and_then(|obj| obj.get(*candidate))
                    .is_some()
            });
        all_present.then(|| (*candidate).to_string())
    })
}

/// Key-index one array, alongside its original order (for deterministic merge
/// output ordering).
fn index_by_key(values: &[Value], key: &str) -> Option<(BTreeMap<String, Value>, Vec<String>)> {
    let mut index = BTreeMap::new();
    let mut order = Vec::new();
    for value in values {
        let id = key_of(value, key)?;
        if index.insert(id.clone(), value.clone()).is_none() {
            order.push(id);
        }
    }
    Some((index, order))
}

fn key_of(value: &Value, key: &str) -> Option<String> {
    let raw = value.as_object()?.get(key)?;
    raw.as_str()
        .map(str::to_string)
        .or_else(|| raw.as_u64().map(|n| n.to_string()))
}

fn format_path(path: &[String]) -> String {
    if path.is_empty() {
        return "<root>".to_string();
    }
    path.join(".")
}
