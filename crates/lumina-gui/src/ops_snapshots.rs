//! GUI-REFACTOR-W2-20 S2.8: named snapshot operations, extracted verbatim
//! from `lib.rs`.
//!
//! [`LuminaApp::snapshots`] lists them, [`LuminaApp::create_snapshot`] freezes
//! the session recipe under a stable id, [`LuminaApp::create_snapshot_auto`] the
//! auto-named `Cmd/Ctrl+Alt+S` variant and [`LuminaApp::restore_snapshot`]
//! adopts a frozen recipe. Public methods keep `pub`; `create_snapshot_auto` is
//! `pub(crate)` because the history panel and headless tests call it.

use super::*;
use log::info;

impl LuminaApp {
    /// Named snapshot list of the active virtual copy (Welle 3, LR-12
    /// light): `(entry id, snapshot name)` for history entries carrying the
    /// `extras["snapshot"] = true` marker — or, tolerantly, the
    /// `snapshot-<n>` id naming for entries written without the marker. The
    /// name falls back to the entry id when no `snapshot_name` is stored.
    /// Plain history entries are skipped. Empty without a loaded document.
    pub fn snapshots(&self) -> Vec<(String, String)> {
        self.document
            .as_ref()
            .and_then(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
            })
            .map(|copy| {
                copy.history
                    .iter()
                    .filter(|entry| {
                        entry.extras.get("snapshot").and_then(Value::as_bool) == Some(true)
                            || entry.id.starts_with("snapshot-")
                    })
                    .map(|entry| {
                        let name = entry
                            .extras
                            .get("snapshot_name")
                            .and_then(Value::as_str)
                            .unwrap_or(&entry.id)
                            .to_string();
                        (entry.id.clone(), name)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// GUI-CLICK-ALL-17 + GUI-INSTRDBG-17: the shared auto-named snapshot
    /// action behind the `Cmd/Ctrl+Alt+S` shortcut and the History-section
    /// Snapshot button. Single path, so button and keyboard cannot diverge.
    pub(crate) fn create_snapshot_auto(&mut self) {
        let name = Str::SnapshotNamePattern.format_arg(&(self.snapshots().len() + 1).to_string());
        if let Err(error) = self.create_snapshot(name).map(|_| ()) {
            self.show_error(error);
        }
    }

    /// Freeze the session recipe as a named snapshot (`Cmd/Ctrl+Alt+S`,
    /// Welle 3, LR-12 light). Snapshots are history entries with an
    /// `extras["snapshot"]` marker — unlike plain history they are named and
    /// meant to be kept. Persists through [`Self::save_sidecar`]; an empty
    /// name fails loudly, never silently.
    pub fn create_snapshot(&mut self, name: impl Into<String>) -> Result<String, GuiError> {
        instrument_gui_action!(self, GuiAction::CreateSnapshot);
        let name = name.into();
        if name.trim().is_empty() {
            return Err(GuiError::Io(Str::InvalidSnapshotName.t().to_string()));
        }
        self.ensure_document_loaded()?;
        let new_id = {
            let document = self.document.as_ref().expect("document was ensured");
            let copy = document
                .virtual_copies
                .iter()
                .find(|copy| copy.id == self.virtual_copy_id)
                .ok_or_else(|| GuiError::Io(Str::VirtualCopyNotFound.t().to_string()))?;
            let mut counter = copy.history.len();
            loop {
                counter += 1;
                let candidate = format!("snapshot-{counter}");
                if !copy.history.iter().any(|entry| entry.id == candidate) {
                    break candidate;
                }
            }
        };
        let mut extras = BTreeMap::new();
        extras.insert("snapshot".into(), Value::Bool(true));
        extras.insert("snapshot_name".into(), Value::String(name.clone()));
        let frozen = self.recipe.clone();
        let timestamp = self.history_timestamp();
        self.active_copy_mut()?.history.push(HistoryEntry {
            id: new_id.clone(),
            recipe: frozen,
            recorded_at: Some(timestamp),
            extras,
        });
        self.save_sidecar();
        // `save_sidecar` overwrites the status ("Sidecar saved"); restore the
        // snapshot message so the freeze stays visible.
        self.status = Str::SnapshotCreatedPattern.format_arg(&name);
        info!("GUI interaction: create_snapshot -> {new_id} ({name})");
        Ok(new_id)
    }

    /// Restore a named snapshot (Welle 3, LR-12 light): adopts the frozen
    /// recipe into the session recipe and re-renders, like
    /// [`Self::restore_history`]. Accepts the `extras["snapshot"]` marker or
    /// — tolerantly — the `snapshot-<n>` id naming; anything else fails
    /// loudly as [`Str::NotSnapshot`] instead of restoring plain history
    /// silently.
    pub fn restore_snapshot(&mut self, entry_id: &str) -> Result<(), GuiError> {
        let is_snapshot = self
            .document
            .as_ref()
            .and_then(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
            })
            .and_then(|copy| copy.history.iter().find(|entry| entry.id == entry_id))
            .is_some_and(|entry| {
                entry.extras.get("snapshot").and_then(Value::as_bool) == Some(true)
                    || entry.id.starts_with("snapshot-")
            });
        if !is_snapshot {
            return Err(GuiError::Io(Str::NotSnapshot.t().to_string()));
        }
        self.restore_history(entry_id)
    }
}
