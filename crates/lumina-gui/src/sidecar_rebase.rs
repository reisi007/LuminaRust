//! SIDECAR-REBASE-1: conflict rebase for the GUI sidecar save paths.
//!
//! The compare-and-swap writer (`lumina_sidecar::save_sidecar_if_unchanged`)
//! refuses a save whose expected revision no longer matches the file. That is
//! the correct default (no silent overwrite), but an overtaking save — e.g. a
//! crop commit followed by the slider debounce, or a batch action on the
//! selected image — leaves the session on a stale revision, so *every*
//! following save keeps failing with `sidecar changed concurrently` and the
//! user sees a string of conflict dialogs while their edits are not persisted.
//!
//! This module rebases the losing save instead of dropping it: on `Conflict`
//! the current file is loaded and the local changes are applied onto it before
//! retrying. Retries are bounded ([`MAX_REBASE_ATTEMPTS`]); a writer that keeps
//! overtaking us is surfaced loudly, never silently overwritten or looped.
//!
//! ## Merge semantics (documented, field-selective)
//!
//! Only sidecar JSON is merged, never pixels, and there is no schema change.
//! The three-way merge works on the serialized document:
//!
//! * **Objects** (`recipe`, `adjustments`, `metadata`, `extras`, …) are merged
//!   recursively, key by key.
//! * **Arrays with a common identity key** (`id`, `mask_id` or `name` present
//!   on every element of base/local/current — e.g. `virtual_copies`, `history`,
//!   `mask_layers`) are merged per identity: a foreign writer's change to copy
//!   B survives while our change to copy A is applied.
//! * **All other arrays and all scalar leaves** are atomic. When only one side
//!   changed the value, the changed side wins. When *both* sides changed it
//!   divergently, the local (last-writer) value wins and the JSON path is
//!   recorded in [`RebaseOutcome::overwritten_fields`] so the caller can log it
//!   at `warn!` — a foreign value is never overwritten silently.
//!
//! Fields the local side did not touch are always taken from the current file,
//! so a concurrent edit is never lost wholesale. Section-only writes
//! ([`RebaseSection`] for the source-level `culling`/`face` sections) apply
//! exactly that one section onto the freshly loaded file, preserving every
//! other foreign field.

use super::*;
use log::{info, warn};
use lumina_sidecar::{SidecarDocument, SidecarError};
use std::path::Path;

// SIDECAR-REBASE-1: the pure JSON three-way merge (no IO, no GUI state) lives
// in its own file to keep this module within the file-size ratchet.
#[path = "sidecar_rebase_merge.rs"]
mod merge;

/// Number of automatic rebase retries after the first `Conflict`. A genuine
/// second writer that keeps writing during the retries still surfaces loudly
/// instead of looping forever.
pub(crate) const MAX_REBASE_ATTEMPTS: usize = 3;

/// The outcome of a successful (possibly rebased) save.
#[derive(Debug)]
pub(crate) struct RebaseOutcome {
    /// Revision the file carries after the write (the next CAS anchor).
    pub revision: String,
    /// The document that was actually written (the merged one when rebased).
    pub document: SidecarDocument,
    /// Whether at least one conflict had to be rebased away.
    pub rebased: bool,
    /// JSON paths where both the local and the on-disk side changed and the
    /// local (last writer) value won. Empty for a conflict-free save; logged
    /// by `finish_sidecar_save` at `warn!`.
    pub overwritten_fields: Vec<String>,
}

/// Which source-level section a section-only save replaces.
#[derive(Clone, Copy)]
pub(crate) enum RebaseSection {
    Culling,
    Face,
}

/// Test seam (SIDECAR-REBASE-1): a writer invoked before every CAS attempt, so
/// a test can overtake the save deterministically instead of racing a thread.
/// Per-thread, so parallel tests stay isolated; compiled out of production.
#[cfg(test)]
type ConflictHook = Box<dyn FnMut(usize)>;

#[cfg(test)]
thread_local! {
    static CONFLICT_HOOK: std::cell::RefCell<Option<ConflictHook>> =
        std::cell::RefCell::new(None);
}

/// Install (or clear with `None`) the [`save_rebased`] conflict hook.
#[cfg(test)]
pub(crate) fn set_conflict_hook(hook: Option<ConflictHook>) {
    CONFLICT_HOOK.with(|slot| *slot.borrow_mut() = hook);
}

fn run_before_attempt(attempt: usize) {
    #[cfg(test)]
    CONFLICT_HOOK.with(|slot| {
        if let Some(hook) = slot.borrow_mut().as_mut() {
            hook(attempt);
        }
    });
    #[cfg(not(test))]
    let _ = attempt;
}

/// CAS save with rebase: apply the local changes onto the current file on a
/// revision conflict and retry, up to `max_rebase_attempts` retries.
///
/// A fresh document (`expected_revision == None`) is never rebased: if a file
/// appeared concurrently there is no common ancestor, so the conflict stays
/// loud instead of overwriting an unrelated foreign sidecar.
pub(crate) fn save_rebased(
    path: &Path,
    base: &SidecarDocument,
    local: &SidecarDocument,
    expected_revision: Option<&str>,
    max_rebase_attempts: usize,
) -> Result<RebaseOutcome, SidecarError> {
    save_rebased_inner(
        path,
        base,
        local,
        expected_revision,
        max_rebase_attempts,
        run_before_attempt,
    )
}

fn save_rebased_inner<B: FnMut(usize)>(
    path: &Path,
    base: &SidecarDocument,
    local: &SidecarDocument,
    expected_revision: Option<&str>,
    max_rebase_attempts: usize,
    mut before_attempt: B,
) -> Result<RebaseOutcome, SidecarError> {
    let mut candidate = local.clone();
    let mut expected = expected_revision.map(str::to_string);
    let mut rebased = false;
    let mut overwritten_fields = Vec::new();
    let mut retries = 0usize;
    loop {
        before_attempt(retries);
        match lumina_sidecar::save_sidecar_if_unchanged(path, &candidate, expected.as_deref()) {
            Ok(revision) => {
                return Ok(RebaseOutcome {
                    revision,
                    document: candidate,
                    rebased,
                    overwritten_fields,
                });
            }
            Err(SidecarError::Conflict(message)) => {
                if expected.is_none() {
                    return Err(SidecarError::Conflict(message));
                }
                if retries >= max_rebase_attempts {
                    return Err(SidecarError::Conflict(format!(
                        "concurrent sidecar change still present after {max_rebase_attempts} \
                         rebase attempt(s): {message}"
                    )));
                }
                let disk = load_sidecar(path)?;
                let disk_revision = document_revision(&disk)?;
                let (merged, mut fields) = merge::merge_documents(base, local, &disk)?;
                overwritten_fields.append(&mut fields);
                candidate = merged;
                expected = Some(disk_revision);
                rebased = true;
                retries += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

/// [`save_rebased`] for callers that only need success/failure; logs a rebase loudly.
pub(crate) fn save_rebased_unit(
    path: &Path,
    base: &SidecarDocument,
    local: &SidecarDocument,
    expected_revision: Option<&str>,
) -> Result<(), SidecarError> {
    let outcome = save_rebased(path, base, local, expected_revision, MAX_REBASE_ATTEMPTS)?;
    log_rebase_outcome(path, &outcome);
    Ok(())
}

/// Log a rebase (`info!`) and any overwritten field (`warn!`); a clean save logs nothing.
fn log_rebase_outcome(path: &Path, outcome: &RebaseOutcome) {
    if !outcome.rebased {
        return;
    }
    if outcome.overwritten_fields.is_empty() {
        info!(
            "sidecar rebased after concurrent change: {}",
            path.display()
        );
    } else {
        warn!(
            "sidecar rebased for {}; concurrent changes overwritten at: {}",
            path.display(),
            outcome.overwritten_fields.join(", ")
        );
    }
}

/// CAS save for a source-level section (`culling`/`face`): on a revision
/// conflict the freshly loaded file gets exactly that section replaced and is
/// retried, so foreign changes to every other field survive. The local side
/// never needs a full base document — the section is the whole local delta.
pub(crate) fn save_section_rebased(
    path: &Path,
    local: &SidecarDocument,
    expected_revision: Option<&str>,
    max_rebase_attempts: usize,
    section: RebaseSection,
) -> Result<RebaseOutcome, SidecarError> {
    let mut candidate = local.clone();
    let mut expected = expected_revision.map(str::to_string);
    let mut rebased = false;
    let mut retries = 0usize;
    loop {
        match lumina_sidecar::save_sidecar_if_unchanged(path, &candidate, expected.as_deref()) {
            Ok(revision) => {
                return Ok(RebaseOutcome {
                    revision,
                    document: candidate,
                    rebased,
                    overwritten_fields: Vec::new(),
                });
            }
            Err(SidecarError::Conflict(message)) => {
                if expected.is_none() || retries >= max_rebase_attempts {
                    return Err(SidecarError::Conflict(format!(
                        "concurrent sidecar change still present after {retries} \
                         rebase attempt(s): {message}"
                    )));
                }
                let disk = load_sidecar(path)?;
                let disk_revision = document_revision(&disk)?;
                let mut merged = disk;
                match section {
                    RebaseSection::Culling => merged.culling = local.culling.clone(),
                    RebaseSection::Face => merged.face = local.face.clone(),
                }
                candidate = merged;
                expected = Some(disk_revision);
                rebased = true;
                retries += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

impl LuminaApp {
    /// SIDECAR-REBASE-1: persist a source-level section (`culling`/`face`)
    /// through the CAS writer with rebase. On success the merged document and
    /// its revision are adopted; on failure the (unsaved) local document stays
    /// in `self.document` so the edit is not lost.
    pub(crate) fn save_section_with_rebase(
        &mut self,
        source_path: &str,
        local: SidecarDocument,
        section: RebaseSection,
    ) -> Result<String, SidecarError> {
        let expected = self.sidecar_revision.clone();
        let sidecar = sidecar_path_for(Path::new(source_path));
        match save_section_rebased(
            &sidecar,
            &local,
            expected.as_deref(),
            MAX_REBASE_ATTEMPTS,
            section,
        ) {
            Ok(saved) => {
                log_rebase_outcome(Path::new(source_path), &saved);
                self.sidecar_revision = Some(saved.revision.clone());
                self.document = Some(saved.document);
                Ok(saved.revision)
            }
            Err(error) => {
                self.document = Some(local);
                Err(error)
            }
        }
    }

    /// SIDECAR-REBASE-1: adopt a successful [`save_rebased`] outcome: set the
    /// saved status/revision/document, log a rebase (and any overwritten
    /// concurrent field) loudly, and — when a rebase changed the active
    /// recipe — re-render so preview and sidecar stay consistent.
    pub(crate) fn finish_sidecar_save(&mut self, source_path: &Path, saved: RebaseOutcome) {
        log_rebase_outcome(source_path, &saved);
        self.status = Str::SidecarSaved.t().into();
        self.sidecar_revision = Some(saved.revision);
        let mut rerender = false;
        if saved.rebased {
            // Adopt merged recipe values so a later save cannot revert fields a
            // foreign writer changed (the session would otherwise keep writing
            // its stale copy over the merged state).
            if let Some(copy) = saved
                .document
                .virtual_copies
                .iter()
                .find(|copy| copy.id == self.virtual_copy_id)
            {
                if self.recipe != copy.recipe {
                    self.recipe = copy.recipe.clone();
                    rerender = true;
                }
            }
        }
        self.document = Some(saved.document);
        self.capture_section_baselines();
        self.refresh_entry(source_path);
        if rerender {
            if let Err(error) = self.render() {
                self.show_error(error);
            } else {
                self.status = Str::SidecarSaved.t().into();
            }
        }
    }
}
