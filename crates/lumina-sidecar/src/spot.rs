//! Shared spot-removal identity, entry normalization, and artifact status.
//!
//! The recipe has two historical views of a spot operation: the typed
//! `spot_removals` mirror and the geometry-carrying `extras` view.  Keeping the
//! identity/status decisions here prevents the CLI and the GUI from drifting
//! (especially for a typed-only generative entry, which has no geometry but is
//! still a selectable recipe operation).

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use crate::{ArtifactStatus, EditRecipe, GenerativeArtifactRef, SpotRemoval};

/// Stable prefix used for IDs synthesized for older typed-only generative
/// entries.  Explicit IDs remain unchanged.
pub const SPOT_REMOVAL_ID_PREFIX: &str = "spot-";

/// Derive a deterministic ID from a spot entry after removing its existing ID.
/// The complete entry (including additive fields such as prompt/seed) is
/// hashed, so two otherwise-identical entries can still be rejected as a
/// duplicate rather than silently acquiring the same target identity.
#[must_use]
pub fn stable_spot_id(value: &Value) -> String {
    let mut object = value.as_object().cloned().unwrap_or_default();
    object.remove("id");
    let bytes = serde_json::to_vec(&Value::Object(object))
        .unwrap_or_else(|_| b"spot-removal-invalid".to_vec());
    format!("{SPOT_REMOVAL_ID_PREFIX}{}", blake3::hash(&bytes).to_hex())
}

/// Add the compatibility ID to a generative entry which predates the explicit
/// `id` field.  Heuristic entries are left untouched: their geometry contract
/// still rejects an absent ID loudly rather than inventing geometry.
pub(crate) fn normalize_spot_removal_value(value: &mut Value) {
    let Some(object) = value.as_object() else {
        return;
    };
    let generative = object
        .get("mode")
        .and_then(Value::as_str)
        .is_some_and(|mode| mode == "generative");
    if !generative {
        return;
    }
    let missing = match object.get("id") {
        None => true,
        Some(Value::String(id)) => id.is_empty(),
        Some(Value::Null) => true,
        Some(_) => false,
    };
    if missing {
        let id = stable_spot_id(value);
        if let Some(object) = value.as_object_mut() {
            object.insert("id".into(), Value::String(id));
        }
    }
}

/// Normalize every entry in a raw `spot_removals` value in place.
pub(crate) fn normalize_spot_removal_array(value: &mut Value) {
    if let Some(entries) = value.as_array_mut() {
        for entry in entries {
            normalize_spot_removal_value(entry);
        }
    }
}

/// Fill compatibility IDs in a typed vector before serialization.  This keeps
/// old in-memory producers (which predate the field) lossless without changing
/// the wire values of explicit IDs.
pub(crate) fn normalize_typed_spot_removals(values: &mut [SpotRemoval]) {
    for spot in values {
        if spot.mode != crate::SpotRemovalMode::Generative || !spot.id.is_empty() {
            continue;
        }
        if let Ok(mut value) = serde_json::to_value(&*spot) {
            normalize_spot_removal_value(&mut value);
            if let Some(id) = value.get("id").and_then(Value::as_str) {
                spot.id = id.to_owned();
            }
        }
    }
}

/// Return all spot operations in the geometry-carrying view.  A typed-only
/// recipe is a valid generative operation, so it is represented rather than
/// silently dropped when no extras mirror exists yet.
#[must_use]
pub fn spot_removal_entries(recipe: &EditRecipe) -> Vec<Value> {
    if let Some(raw) = recipe.extras.get("spot_removals") {
        if let Ok(mut entries) = serde_json::from_value::<Vec<Value>>(raw.clone()) {
            for entry in &mut entries {
                normalize_spot_removal_value(entry);
            }
            return entries;
        }
    }
    recipe
        .spot_removals
        .iter()
        .filter_map(|spot| {
            let mut value = serde_json::to_value(spot).ok()?;
            normalize_spot_removal_value(&mut value);
            Some(value)
        })
        .collect()
}

/// Atomically update both recipe views from normalized raw entries.  The raw
/// extras retain additive geometry/seed fields; the typed mirror retains the
/// stable ID/version/mode/artifact contract used by hashing and validation.
pub fn set_spot_removal_entries(recipe: &mut EditRecipe, entries: &[Value]) -> Result<(), String> {
    if entries.is_empty() {
        recipe.extras.remove("spot_removals");
        recipe.spot_removals.clear();
        return Ok(());
    }
    let mut normalized = entries.to_vec();
    for entry in &mut normalized {
        normalize_spot_removal_value(entry);
    }
    let typed = normalized
        .iter()
        .map(|entry| serde_json::from_value::<SpotRemoval>(entry.clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("spot_removals entry is invalid: {error}"))?;
    let value = serde_json::to_value(&normalized)
        .map_err(|error| format!("spot_removals cannot be serialized: {error}"))?;
    recipe.extras.insert("spot_removals".into(), value);
    recipe.spot_removals = typed;
    Ok(())
}

/// Effective status shown for one operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotRemovalStatus {
    Valid,
    Stale,
    Missing,
    Corrupt,
}

impl SpotRemovalStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Stale => "stale",
            Self::Missing => "missing",
            Self::Corrupt => "corrupt",
        }
    }
}

fn stored_status(entry: &Value) -> SpotRemovalStatus {
    match entry.get("status").and_then(Value::as_str) {
        Some("stale") => SpotRemovalStatus::Stale,
        Some("missing") => SpotRemovalStatus::Missing,
        Some("corrupt") => SpotRemovalStatus::Corrupt,
        _ => SpotRemovalStatus::Valid,
    }
}

/// Verify the referenced generative record as well as its container. This is
/// shared by canvas and spot links; the caller-specific status adds kind
/// separation where needed.
pub(crate) fn verified_generative_artifact_status(
    link: &GenerativeArtifactRef,
    bundle_root: &Path,
) -> ArtifactStatus {
    let base = crate::artifact_status(bundle_root, &link.as_artifact_reference());
    if base != ArtifactStatus::Available {
        return base;
    }
    #[cfg(feature = "zdata")]
    {
        let container = match crate::load_zdata(&bundle_root.join(&link.relative_path)) {
            Ok(container) => container,
            Err(_) => return ArtifactStatus::Corrupt,
        };
        let records = match container.decode_all() {
            Ok(records) => records,
            Err(_) => return ArtifactStatus::Corrupt,
        };
        for record in records {
            let (id, width, height, checksum) = match record {
                crate::RecordSpec::GenerativeCanvas(value) => {
                    let checksum = value.checksum();
                    (value.id, value.width, value.height, checksum)
                }
                crate::RecordSpec::SpotHealGenerative(value) => {
                    let checksum = value.checksum();
                    (value.id, value.width, value.height, checksum)
                }
                _ => continue,
            };
            if id == link.id {
                let checksum_matches = checksum == link.checksum
                    || link.checksum.strip_prefix("blake3:") == Some(checksum.as_str());
                return if width == link.width && height == link.height && checksum_matches {
                    ArtifactStatus::Available
                } else {
                    ArtifactStatus::Corrupt
                };
            }
        }
        ArtifactStatus::Corrupt
    }
    #[cfg(not(feature = "zdata"))]
    {
        let _ = link;
        ArtifactStatus::Available
    }
}

/// Check a generative spot reference against the actual `kind = 3` bundle
/// record.  The generic artifact checker verifies the container; this method
/// additionally proves that the referenced record exists, has the declared
/// dimensions/channels/version, and carries the exact declared checksum.
impl GenerativeArtifactRef {
    #[must_use]
    pub fn spot_heal_artifact_status(&self, bundle_root: &Path) -> ArtifactStatus {
        let base = self.artifact_status(bundle_root);
        if base != ArtifactStatus::Available {
            return base;
        }
        if self.id.trim().is_empty()
            || !self.format.contains("zdata")
            || self.channels != "rgba8"
            || self.data_version != "1"
            || self.width == 0
            || self.height == 0
        {
            return ArtifactStatus::Corrupt;
        }

        #[cfg(feature = "zdata")]
        {
            let path = bundle_root.join(&self.relative_path);
            let container = match crate::load_zdata(&path) {
                Ok(container) => container,
                Err(_) => return ArtifactStatus::Corrupt,
            };
            let record = match container.spot_heal_generative(&self.id) {
                Ok(record) => record,
                Err(_) => return ArtifactStatus::Corrupt,
            };
            let actual_checksum = record.checksum();
            let checksum_matches = actual_checksum == self.checksum
                || self.checksum.strip_prefix("blake3:") == Some(actual_checksum.as_str());
            if record.width == self.width && record.height == self.height && checksum_matches {
                ArtifactStatus::Available
            } else {
                ArtifactStatus::Corrupt
            }
        }
        #[cfg(not(feature = "zdata"))]
        {
            // The no-codec build can perform the structural file check only;
            // the GUI/CLI enable zdata, and the limitation is explicit here.
            ArtifactStatus::Available
        }
    }
}

/// Resolve the visible status of a spot operation from its reference.  Missing
/// and null links are `missing`; a missing path is `missing`; a bad checksum,
/// record kind/id, or damaged bundle is `corrupt`; a present identity mismatch
/// is `stale`.  Heuristic entries retain their persisted status because they
/// have no artifact reference.
#[must_use]
pub fn spot_removal_status(entry: &Value, bundle_root: &Path) -> SpotRemovalStatus {
    spot_removal_status_for_identity(entry, bundle_root, None)
}

/// Identity-aware variant used by callers that can prove the current operation
/// identity.  With no current identity, a valid reference is not guessed stale;
/// an explicitly persisted stale status is still retained.
#[must_use]
pub fn spot_removal_status_for_identity(
    entry: &Value,
    bundle_root: &Path,
    current_identity: Option<&str>,
) -> SpotRemovalStatus {
    let mode = entry
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("heuristic");
    if mode != "generative" {
        return stored_status(entry);
    }
    let Some(artifact) = entry.get("artifact") else {
        return SpotRemovalStatus::Missing;
    };
    if artifact.is_null() {
        return SpotRemovalStatus::Missing;
    }
    let link: GenerativeArtifactRef = match serde_json::from_value(artifact.clone()) {
        Ok(link) => link,
        Err(_) => return SpotRemovalStatus::Corrupt,
    };
    let status = match link.spot_heal_artifact_status(bundle_root) {
        ArtifactStatus::Missing => return SpotRemovalStatus::Missing,
        ArtifactStatus::Corrupt => return SpotRemovalStatus::Corrupt,
        ArtifactStatus::Available => SpotRemovalStatus::Valid,
    };
    if let Some(current) = current_identity {
        if link.identity() != Some(current) {
            return SpotRemovalStatus::Stale;
        }
    } else if let Some(stored) = match stored_status(entry) {
        SpotRemovalStatus::Valid => None,
        other => Some(other),
    } {
        return stored;
    }
    status
}

/// Validate uniqueness for a list of raw entries without allocating a second
/// domain representation.  Empty IDs are left to the regular field validator.
#[must_use]
pub(crate) fn duplicate_spot_id(entries: &[Value]) -> Option<String> {
    let mut seen = BTreeSet::new();
    for entry in entries {
        let Some(id) = entry.get("id").and_then(Value::as_str) else {
            continue;
        };
        if !seen.insert(id) {
            return Some(id.to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_only_generative_entry_gets_a_stable_id() {
        let value = serde_json::json!({
            "version": 1,
            "mode": "generative",
            "artifact": Value::Null,
            "prompt": "remove dust"
        });
        let mut first = value.clone();
        normalize_spot_removal_value(&mut first);
        let mut second = value;
        normalize_spot_removal_value(&mut second);
        assert_eq!(first, second);
        assert!(first["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("spot-")));
    }

    #[test]
    fn typed_only_view_materializes_an_empty_generative_id() {
        let mut recipe = EditRecipe::default();
        recipe.spot_removals.push(SpotRemoval {
            id: String::new(),
            version: crate::SPOT_REMOVAL_VERSION,
            mode: crate::SpotRemovalMode::Generative,
            artifact: None,
        });
        let entries = spot_removal_entries(&recipe);
        assert_eq!(entries.len(), 1);
        assert!(entries[0]["id"]
            .as_str()
            .is_some_and(|id| id.starts_with(SPOT_REMOVAL_ID_PREFIX)));
    }

    #[test]
    fn duplicate_ids_are_reported() {
        let entries = vec![
            serde_json::json!({"id":"same","mode":"heuristic"}),
            serde_json::json!({"id":"same","mode":"generative"}),
        ];
        assert_eq!(duplicate_spot_id(&entries).as_deref(), Some("same"));
    }
}
