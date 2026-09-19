//! GUI-FILMSTRIP-SYNC-1 / LRPAR-G08-PREVIOUS — filmstrip selection actions.
//!
//! The three user-visible, recipe-mutating filmstrip buttons (Lightroom
//! "Sync Settings", "Match Total Exposures" and "Previous Image") and their
//! CAS-sidecar helpers.
//!
//! Extracted verbatim from `lib.rs` (GUI-INSTRDBG-17c-Rework F-1) so the
//! file-size ratchet keeps the GUI root within its committed baseline while the
//! three buttons gain their `GuiAction` instrumentation. Each handler carries
//! `instrument_gui_action!` as its first statement (debug-only via
//! [`crate::GuiActionTimer`]); persistence, the loud per-image failure report
//! and the `preview_generation` bumps are unchanged. `decode_selection_frame`,
//! `selection_source_identity`, `default_copy_mut`, `previous_history_extras`
//! and `is_raw_name` stay at the crate root because `merge_gui` shares them.

use super::*;
use crate::sidecar_rebase::save_rebased_unit;
use log::{error, info};

impl LuminaApp {
    /// GUI-FILMSTRIP-SYNC-1: apply the active copy's recipe to every selected
    /// image (Lightroom "Sync Settings"). Each target keeps its own sidecar
    /// (created when missing) written via CAS; per-image failures are loud
    /// (`error!` + report entry) and never abort the remaining targets. Every
    /// applied image logs `info!` and bumps `preview_generation`.
    pub fn sync_settings_to_selection(&mut self) -> SelectionSyncReport {
        instrument_gui_action!(self, GuiAction::SyncSettingsToSelection);
        let targets: Vec<String> = self.filmstrip_selection.iter().cloned().collect();
        let mut report = SelectionSyncReport::default();
        if targets.is_empty() {
            self.status = "No images selected".into();
            return report;
        }
        let recipe = self.recipe.clone();
        for (index, target) in targets.iter().enumerate() {
            match self.apply_recipe_to_path(
                target,
                &recipe,
                &format!("sync-{index}"),
                BTreeMap::new(),
            ) {
                Ok(()) => {
                    info!("sync settings: {target} updated");
                    self.preview_generation += 1;
                    self.refresh_entry(Path::new(target));
                    report.applied.push(target.clone());
                }
                Err(message) => {
                    error!("sync settings failed for {target}: {message}");
                    report.failed.push((target.clone(), message));
                }
            }
        }
        if report.failed.is_empty() {
            self.status = format!("Synced settings to {} image(s)", report.applied.len());
        } else {
            let joined = report
                .failed
                .iter()
                .map(|(path, message)| format!("{path}: {message}"))
                .collect::<Vec<_>>()
                .join("; ");
            self.show_error(format!(
                "Sync failed for {} image(s): {joined}",
                report.failed.len()
            ));
        }
        report
    }

    /// GUI-FILMSTRIP-SYNC-1: equalize exposure over the selection (Lightroom
    /// "Match Total Exposures"). Each selected image is measured with Core's
    /// [`analyze_tone`](lumina_core::analyze_tone); the selection median of
    /// those means is the common target, and each image receives its own Core
    /// [`match_total_exposure`](lumina_core::match_total_exposure) delta on
    /// top of its current exposure (read-only Core use — no Core change).
    /// Persistence, logging and `preview_generation` behave like
    /// [`Self::sync_settings_to_selection`].
    pub fn match_exposures_of_selection(&mut self) -> SelectionSyncReport {
        instrument_gui_action!(self, GuiAction::MatchExposuresOfSelection);
        let targets: Vec<String> = self.filmstrip_selection.iter().cloned().collect();
        let mut report = SelectionSyncReport::default();
        if targets.is_empty() {
            self.status = "No images selected".into();
            return report;
        }
        // Pass 1 (measure): decode every target and read its mean luminance.
        // A decode failure is a loud per-image entry, never an abort.
        let mut measured: Vec<(String, ImageFrame, f64)> = Vec::new();
        for target in &targets {
            match decode_selection_frame(Path::new(target)) {
                Ok((_, frame, _)) => {
                    let mean = analyze_tone(&frame).mean;
                    measured.push((target.clone(), frame, mean));
                }
                Err(message) => {
                    error!("match exposures: cannot decode {target}: {message}");
                    report.failed.push((target.clone(), message));
                }
            }
        }
        if measured.is_empty() {
            self.show_error("Match exposures: no selectable image could be decoded");
            return report;
        }
        let mut means: Vec<f64> = measured.iter().map(|(_, _, mean)| *mean).collect();
        means.sort_by(f64::total_cmp);
        let middle = means.len() / 2;
        let median = if means.len() % 2 == 1 {
            means[middle]
        } else {
            (means[middle - 1] + means[middle]) / 2.0
        }
        .clamp(0.0, 1.0);
        // Pass 2 (apply): one Core delta per image against the median.
        for (index, (target, frame, _)) in measured.iter().enumerate() {
            let delta = match lumina_core::match_total_exposure(frame, median) {
                Ok(delta) => delta,
                Err(error) => {
                    let message = error.to_string();
                    error!("match exposures failed for {target}: {message}");
                    report.failed.push((target.clone(), message));
                    continue;
                }
            };
            match self.apply_match_delta_to_path(target, delta, median, &format!("match-{index}")) {
                Ok((old, new)) => {
                    info!(
                        "match exposures: {target} exposure {old:+.3} -> {new:+.3} (median luminance {median:.4})"
                    );
                    self.preview_generation += 1;
                    self.refresh_entry(Path::new(target));
                    report.applied.push(target.clone());
                }
                Err(message) => {
                    error!("match exposures failed for {target}: {message}");
                    report.failed.push((target.clone(), message));
                }
            }
        }
        if report.failed.is_empty() {
            self.status = format!(
                "Matched exposures of {} image(s) to median luminance {median:.4}",
                report.applied.len()
            );
        } else {
            let joined = report
                .failed
                .iter()
                .map(|(path, message)| format!("{path}: {message}"))
                .collect::<Vec<_>>()
                .join("; ");
            self.show_error(format!(
                "Match exposures failed for {} image(s): {joined}",
                report.failed.len()
            ));
        }
        report
    }

    /// Previous reference path (read-only accessor for headless tests).
    pub fn previous_source_path(&self) -> Option<&str> {
        self.previous_reference
            .as_ref()
            .map(|reference| reference.path.as_str())
    }

    /// LRPAR-G08-PREVIOUS: apply the Previous reference (the image edited
    /// immediately before the current one, Lightroom "Previous") to the
    /// filmstrip selection — the same full-recipe Sync mechanism as
    /// [`Self::sync_settings_to_selection`] (each target keeps its own
    /// sidecar written via CAS, one `previous-{index}` history step each,
    /// per-image failures loud via `error!` + report entry, never aborting
    /// the rest). With an empty selection the currently loaded image is the
    /// single target (Previous on the active photo); with neither selection
    /// nor loaded image — or without any reference — the call is a loud
    /// no-op (empty report + visible error, no sidecar write). Every applied
    /// image logs `info!` and bumps `preview_generation`. Unlike Sync, the
    /// currently loaded target additionally adopts the reference in memory
    /// (recipe + document + baseline + re-render) so preview and sidecar
    /// stay consistent.
    pub fn apply_previous_to_selection(&mut self) -> SelectionSyncReport {
        instrument_gui_action!(self, GuiAction::ApplyPreviousToSelection);
        let mut report = SelectionSyncReport::default();
        let Some(reference) = self.previous_reference.clone() else {
            self.show_error("Previous unavailable: no previously edited image in this session");
            return report;
        };
        let mut targets: Vec<String> = self.filmstrip_selection.iter().cloned().collect();
        if targets.is_empty() {
            if self.original.is_none() || self.path.trim().is_empty() {
                self.show_error("Previous unavailable: no image loaded");
                return report;
            }
            targets.push(self.path.clone());
        }
        let reference_name = Path::new(&reference.path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&reference.path)
            .to_string();
        for (index, target) in targets.iter().enumerate() {
            let history_id = format!("previous-{index}");
            let history_extras = previous_history_extras(&reference.path);
            let generation_before = self.preview_generation;
            let applied = if *target == self.path && self.original.is_some() {
                self.apply_previous_to_current(&reference.recipe, &history_id, history_extras)
            } else {
                self.apply_recipe_to_path(target, &reference.recipe, &history_id, history_extras)
            };
            match applied {
                Ok(()) => {
                    info!(
                        "previous settings: {target} updated from {}",
                        reference.path
                    );
                    // Exactly one generation step per applied image: the
                    // current-image path re-rendered above (which bumps
                    // itself), the file-only path did not.
                    if self.preview_generation == generation_before {
                        self.preview_generation += 1;
                    }
                    self.refresh_entry(Path::new(target));
                    report.applied.push(target.clone());
                }
                Err(message) => {
                    error!("previous settings failed for {target}: {message}");
                    report.failed.push((target.clone(), message));
                }
            }
        }
        if report.failed.is_empty() {
            self.status = format!(
                "Applied previous settings from {reference_name} to {} image(s)",
                report.applied.len()
            );
        } else {
            let joined = report
                .failed
                .iter()
                .map(|(path, message)| format!("{path}: {message}"))
                .collect::<Vec<_>>()
                .join("; ");
            self.show_error(format!(
                "Previous failed for {} image(s): {joined}",
                report.failed.len()
            ));
        }
        report
    }

    /// GUI selection batch adjustment (exposure/contrast/highlights/shadows):
    /// sets `key = value` on the default copy of every selected sidecar and
    /// persists each atomically. UX-LOOK-HISTORY-18: the appended history step
    /// carries the structured control change (name + old→new) and a timestamp.
    pub fn apply_adjustment_to_selection(
        paths: &[std::path::PathBuf],
        key: &str,
        value: f64,
    ) -> Result<usize, GuiError> {
        if !matches!(key, "exposure" | "contrast" | "highlights" | "shadows") {
            return Err(GuiError::Io(Str::UnknownAdjustment.format_arg(key)));
        }
        let mut changed = 0;
        for path in paths {
            let sidecar_path = lumina_sidecar::sidecar_path_for(path);
            let mut document = lumina_sidecar::load_sidecar(&sidecar_path)?;
            // SIDECAR-REBASE-1: CAS instead of the former blind plain save — a
            // concurrent change to this target rebases our adjustment onto the
            // current file instead of being overwritten or dropped.
            let base = document.clone();
            let expected = lumina_sidecar::document_revision(&base)?;
            let Some(copy) = document
                .virtual_copies
                .iter_mut()
                .find(|copy| copy.is_default)
            else {
                continue;
            };
            let before = copy.recipe.clone();
            copy.recipe.adjustments.insert(key.into(), value);
            let mut entry = HistoryEntry {
                id: format!("selection-{changed}"),
                recipe: copy.recipe.clone(),
                recorded_at: Some(history_changes::now_rfc3339()),
                extras: BTreeMap::new(),
            };
            if let Err(error) =
                entry.set_changes(history_changes::recipe_changes(&before, &copy.recipe))
            {
                error!("selection history changes rejected: {error}");
            }
            copy.history.push(entry);
            save_rebased_unit(&sidecar_path, &base, &document, Some(&expected))?;
            changed += 1;
        }
        Ok(changed)
    }

    /// Write the Previous `reference` recipe into the currently loaded image:
    /// same disk write as [`Self::apply_recipe_to_path`] (CAS sidecar +
    /// history step), then adopt the persisted state in memory (recipe +
    /// document + revision + Previous baseline) and re-render, so the visible
    /// preview matches the sidecar. A save that did not land is a loud
    /// per-target failure, never a silent divergence.
    fn apply_previous_to_current(
        &mut self,
        recipe: &EditRecipe,
        history_id: &str,
        history_extras: BTreeMap<String, Value>,
    ) -> Result<(), String> {
        self.ensure_document_loaded()
            .map_err(|error| error.to_string())?;
        self.recipe = recipe.clone();
        let id = self.virtual_copy_id.clone();
        let timestamp = self.history_timestamp();
        let document = self
            .document
            .as_mut()
            .ok_or_else(|| "no sidecar document loaded".to_string())?;
        let copy = document
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == id)
            .ok_or_else(|| "sidecar has no virtual copies".to_string())?;
        let before = copy.recipe.clone();
        copy.recipe = recipe.clone();
        let mut entry = HistoryEntry {
            id: history_id.into(),
            recipe: recipe.clone(),
            recorded_at: Some(timestamp),
            extras: history_extras,
        };
        if let Err(error) = entry.set_changes(history_changes::recipe_changes(&before, recipe)) {
            error!("previous history changes rejected: {error}");
        }
        copy.history.push(entry);
        self.mark_dirty();
        self.save_sidecar();
        self.render().map_err(|error| error.to_string())?;
        // Reload anchor: the sidecar on disk is the truth — confirm the write
        // landed and adopt it, so a failed save can never leave preview and
        // sidecar silently diverged.
        let sidecar_path = lumina_sidecar::sidecar_path_for(Path::new(&self.path));
        let document =
            lumina_sidecar::load_sidecar(&sidecar_path).map_err(|error| error.to_string())?;
        let revision =
            lumina_sidecar::document_revision(&document).map_err(|error| error.to_string())?;
        let persisted = document
            .virtual_copies
            .iter()
            .find(|copy| copy.id == self.virtual_copy_id)
            .ok_or_else(|| "sidecar has no virtual copies".to_string())?;
        if persisted.recipe != *recipe {
            return Err("sidecar save did not persist the previous recipe".to_string());
        }
        self.recipe = persisted.recipe.clone();
        self.sidecar_revision = Some(revision);
        self.document = Some(document);
        self.capture_section_baselines();
        Ok(())
    }

    /// Write `recipe` into the default copy of `target`'s sidecar (creating
    /// the sidecar when missing) through the CAS API. The source is decoded
    /// first so a missing/unreadable image fails loudly before any write.
    fn apply_recipe_to_path(
        &self,
        target: &str,
        recipe: &EditRecipe,
        history_id: &str,
        history_extras: BTreeMap<String, Value>,
    ) -> Result<(), String> {
        let path = PathBuf::from(target);
        let sidecar_path = lumina_sidecar::sidecar_path_for(&path);
        let (bytes, frame, orientation) = decode_selection_frame(&path)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(target);
        let mut document = if sidecar_path.exists() {
            lumina_sidecar::load_sidecar(&sidecar_path).map_err(|error| error.to_string())?
        } else {
            SidecarDocument::new(
                selection_source_identity(name, &bytes, &frame, orientation, is_raw_name(name)),
                "raster-mvp-1",
            )
        };
        let expected =
            lumina_sidecar::document_revision(&document).map_err(|error| error.to_string())?;
        // CAS against the revision just read: an external modification between
        // our load and this write surfaces as a loud conflict instead of being
        // silently overwritten. A missing file expects `None` (fresh lineage).
        let expected_revision = if sidecar_path.exists() {
            Some(expected)
        } else {
            None
        };
        let base = document.clone();
        let copy = default_copy_mut(&mut document)
            .ok_or_else(|| "sidecar has no virtual copies".to_string())?;
        let before = copy.recipe.clone();
        copy.recipe = recipe.clone();
        let mut entry = HistoryEntry {
            id: history_id.into(),
            recipe: recipe.clone(),
            recorded_at: Some(self.history_timestamp()),
            extras: history_extras,
        };
        if let Err(error) = entry.set_changes(history_changes::recipe_changes(&before, recipe)) {
            error!("sync history changes rejected: {error}");
        }
        copy.history.push(entry);
        save_rebased_unit(
            &sidecar_path,
            &base,
            &document,
            expected_revision.as_deref(),
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Add `delta` to the current exposure of `target`'s default copy and tag
    /// the match state (`target_luminance` = selection median). Returns
    /// `(old_exposure, new_exposure)` for the per-image `info!` log.
    fn apply_match_delta_to_path(
        &self,
        target: &str,
        delta: f64,
        median: f64,
        history_id: &str,
    ) -> Result<(f64, f64), String> {
        let path = PathBuf::from(target);
        let sidecar_path = lumina_sidecar::sidecar_path_for(&path);
        let (bytes, frame, orientation) = decode_selection_frame(&path)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(target);
        let mut document = if sidecar_path.exists() {
            lumina_sidecar::load_sidecar(&sidecar_path).map_err(|error| error.to_string())?
        } else {
            SidecarDocument::new(
                selection_source_identity(name, &bytes, &frame, orientation, is_raw_name(name)),
                "raster-mvp-1",
            )
        };
        let expected =
            lumina_sidecar::document_revision(&document).map_err(|error| error.to_string())?;
        let expected_revision = if sidecar_path.exists() {
            Some(expected)
        } else {
            None
        };
        let base = document.clone();
        let copy = default_copy_mut(&mut document)
            .ok_or_else(|| "sidecar has no virtual copies".to_string())?;
        let before = copy.recipe.clone();
        let old = copy
            .recipe
            .adjustments
            .get("exposure")
            .copied()
            .unwrap_or(0.0);
        let new = old + delta;
        copy.recipe.adjustments.insert("exposure".into(), new);
        copy.recipe.auto_features.match_total_exposure = true;
        copy.recipe.auto_features.target_luminance = median;
        copy.recipe.auto_features.matched_exposure = Some(delta);
        let mut entry = HistoryEntry {
            id: history_id.into(),
            recipe: copy.recipe.clone(),
            recorded_at: Some(self.history_timestamp()),
            extras: BTreeMap::new(),
        };
        if let Err(error) =
            entry.set_changes(history_changes::recipe_changes(&before, &copy.recipe))
        {
            error!("match history changes rejected: {error}");
        }
        copy.history.push(entry);
        save_rebased_unit(
            &sidecar_path,
            &base,
            &document,
            expected_revision.as_deref(),
        )
        .map_err(|error| error.to_string())?;
        Ok((old, new))
    }
}
