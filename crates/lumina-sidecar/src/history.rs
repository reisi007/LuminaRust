//! UX-LOOK-HISTORY-18 (Release 1.0): structured, human-readable edit-history
//! steps.
//!
//! The user decision (`feature/platform/lightroom-ux-parity.md` § SOLL-Entscheide
//! UX-LOOK-18) is: a history entry carries the control name plus the
//! old→new values as **structured fields in the sidecar**, so the GUI can render
//! `parameter from → to` instead of a machine id. The timestamp stays in the
//! existing [`HistoryEntry::recorded_at`] field.
//!
//! **Schema decision (documented, additive):** the structured step is persisted
//! under the
//! [`HISTORY_CHANGES_KEY`] extra key of the history entry as a JSON
//! `[HistoryChange]` array. It is additive: an entry without the key is a valid
//! legacy entry and reads as an empty change list — nothing is invented or
//! silently normalized. A *present* key must deserialize and validate as a
//! typed change list; anything else (wrong JSON type, missing/unknown fields,
//! empty parameter, control characters, over-long text, too many entries) is
//! rejected loudly with [`SidecarError::Invalid`] — no silent fallback.
//!
//! Why the existing `serde(flatten)` extra mechanism and not a new Rust field?
//! [`HistoryEntry`] is a shared domain struct constructed literally in
//! `lumina-core`, `lumina-cli` and several GUIs; a new struct field would force
//! edits in those out-of-scope crates. The extra channel is the sidecar's
//! documented additive-extension mechanism (`feature/architecture/sidecar.md`
//! § Persistenzregeln), keeps the change fully typed/validated here, and keeps
//! the JSON shape exactly the structured object the decision asks for:
//! `"changes": [{"parameter": "...", "from": "...", "to": "..."}]`.

use crate::{invalid, EditRecipe, Extras, MaskStateSnapshot, SidecarError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Extra key that carries the structured [`HistoryChange`] list of a history
/// entry. Absent = legacy entry (empty change list).
pub const HISTORY_CHANGES_KEY: &str = "changes";

/// Upper bound on changes stored per history step (a preset may touch several
/// controls; an unbounded list would be an injection/DoS vector).
pub const MAX_HISTORY_CHANGES: usize = 64;

/// Additive history key carrying the complete ordered mask-layer state before
/// the recorded edit.  Legacy entries may omit it; a present value is parsed
/// strictly and never silently treated as an empty snapshot.
pub const HISTORY_MASK_STATE_KEY: &str = "mask_state";

/// Character limit of the control/parameter name.
pub const MAX_HISTORY_CHANGE_PARAMETER_CHARS: usize = 128;

/// Character limit of the `from`/`to` value strings.
pub const MAX_HISTORY_CHANGE_VALUE_CHARS: usize = 256;

/// One structured control change of a history step: the control/slider name
/// plus the old and new display values. `from` may be empty (a control that was
/// unset before); `parameter` must not. Values are display strings, never
/// parsed back into geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryChange {
    pub parameter: String,
    pub from: String,
    pub to: String,
}

impl HistoryChange {
    /// Loud validation of one change (see the module docs).
    pub fn validate(&self) -> Result<(), SidecarError> {
        validate_change_text(
            "history change parameter",
            &self.parameter,
            MAX_HISTORY_CHANGE_PARAMETER_CHARS,
            false,
        )?;
        validate_change_text(
            "history change from",
            &self.from,
            MAX_HISTORY_CHANGE_VALUE_CHARS,
            true,
        )?;
        validate_change_text(
            "history change to",
            &self.to,
            MAX_HISTORY_CHANGE_VALUE_CHARS,
            true,
        )?;
        Ok(())
    }
}

fn validate_change_text(
    field: &str,
    value: &str,
    max_chars: usize,
    allow_empty: bool,
) -> Result<(), SidecarError> {
    if !allow_empty && value.trim().is_empty() {
        return invalid(format!("{field} must not be empty"));
    }
    if value.chars().any(char::is_control) {
        return invalid(format!("{field} must not contain control characters"));
    }
    if value.chars().count() > max_chars {
        return invalid(format!("{field} exceeds limit of {max_chars} characters"));
    }
    Ok(())
}

/// One persisted edit-history step of a virtual copy. Moved here (from the
/// crate root) unchanged so the structured change API lives next to its schema;
/// the type is re-exported at the crate root, so every existing path keeps
/// working.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub recipe: EditRecipe,
    pub recorded_at: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extras: Extras,
}

impl HistoryEntry {
    /// Parses the structured change list persisted under [`HISTORY_CHANGES_KEY`].
    ///
    /// * Absent key → `Ok(vec![])` (legacy entry; additive extension).
    /// * Present key → strict typed parse; any deviation is a loud
    ///   [`SidecarError::Invalid`]. The value is never dropped, trimmed or
    ///   coerced.
    pub fn changes(&self) -> Result<Vec<HistoryChange>, SidecarError> {
        let Some(value) = self.extras.get(HISTORY_CHANGES_KEY) else {
            return Ok(Vec::new());
        };
        let changes: Vec<HistoryChange> =
            serde_json::from_value(value.clone()).map_err(|error| {
                SidecarError::Invalid(format!(
                    "history entry `{}` has invalid `{HISTORY_CHANGES_KEY}`: {error}",
                    self.id
                ))
            })?;
        if changes.len() > MAX_HISTORY_CHANGES {
            return Err(SidecarError::Invalid(format!(
                "history entry `{}` has more than {MAX_HISTORY_CHANGES} changes",
                self.id
            )));
        }
        for change in &changes {
            change.validate()?;
        }
        Ok(changes)
    }

    /// Stores the structured change list under [`HISTORY_CHANGES_KEY`] after
    /// validating it. An empty list removes the key (the entry serializes back
    /// as a legacy entry — no empty `changes` array is written).
    pub fn set_changes(&mut self, changes: Vec<HistoryChange>) -> Result<(), SidecarError> {
        if changes.len() > MAX_HISTORY_CHANGES {
            return invalid(format!(
                "history entry `{}` has more than {MAX_HISTORY_CHANGES} changes",
                self.id
            ));
        }
        for change in &changes {
            change.validate()?;
        }
        if changes.is_empty() {
            self.extras.remove(HISTORY_CHANGES_KEY);
            return Ok(());
        }
        let value = serde_json::to_value(&changes).map_err(|error| {
            SidecarError::Json(format!("cannot encode history changes: {error}"))
        })?;
        self.extras.insert(HISTORY_CHANGES_KEY.to_string(), value);
        Ok(())
    }

    /// Parse the complete mask-layer snapshot stored before a history edit.
    /// `None` is the valid legacy representation for entries that predate
    /// MASK-LOCAL-P0; v1 snapshots migrate to the current typed version and a
    /// present but malformed snapshot is always rejected.
    pub fn mask_state(&self) -> Result<Option<MaskStateSnapshot>, SidecarError> {
        let Some(value) = self.extras.get(HISTORY_MASK_STATE_KEY) else {
            return Ok(None);
        };
        let snapshot: MaskStateSnapshot =
            serde_json::from_value(value.clone()).map_err(|error| {
                SidecarError::Invalid(format!(
                    "history entry `{}` has invalid `{HISTORY_MASK_STATE_KEY}`: {error}",
                    self.id
                ))
            })?;
        snapshot.validate()?;
        Ok(Some(snapshot))
    }

    /// Store a complete mask-layer snapshot in this history entry.  This is
    /// intentionally additive: old history rows remain readable, while every
    /// new local-adjustment step has enough state for an exact restore.
    pub fn set_mask_state(&mut self, mut snapshot: MaskStateSnapshot) -> Result<(), SidecarError> {
        for layer in &mut snapshot.layers {
            layer.normalize_local_adjustments()?;
        }
        snapshot.validate()?;
        let value = serde_json::to_value(snapshot).map_err(|error| {
            SidecarError::Json(format!("cannot encode history mask state: {error}"))
        })?;
        self.extras
            .insert(HISTORY_MASK_STATE_KEY.to_string(), value);
        Ok(())
    }
}

/// Loud validation hook called by [`crate::SidecarDocument::validate`] for
/// every persisted history entry of a virtual copy.
pub fn validate_history_entry(entry: &HistoryEntry) -> Result<(), SidecarError> {
    entry.changes().map(|_| ())?;
    entry.mask_state().map(|_| ())
}
