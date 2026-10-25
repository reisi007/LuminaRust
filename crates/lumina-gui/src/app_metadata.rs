//! Metadata draft/history/embedded IPTC plus the metadata presets and the
//! selection-wide sync.
//! The inherent `impl LuminaApp` block of `lib.rs`, moved verbatim
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100 — the last 1000-line Ratchet exception).
//! One inherent impl may be spread over several modules of the same crate, so
//! this is a pure structural move: every body below is byte-identical to its
//! `lib.rs` text. The only differences are this header, the `use super::*;`
//! glob and the `pub(crate)` tokens on the private helpers that a sibling
//! module calls (listed in `scripts/file_size_baseline.txt`).

use super::*;

impl LuminaApp {
    /// Current draft values of the loaded document (empty without one).
    /// Read-only accessor for the panel and headless tests.
    pub fn metadata_draft(&self) -> BTreeMap<String, String> {
        self.document
            .as_ref()
            .map(|document| document.metadata.draft.clone())
            .unwrap_or_default()
    }

    /// Current metadata history of the loaded document, newest first (empty
    /// without one). Read-only accessor for the panel and headless tests.
    pub fn metadata_history(&self) -> Vec<MetadataHistoryEntry> {
        self.document
            .as_ref()
            .map(|document| document.metadata.history.clone())
            .unwrap_or_default()
    }

    /// Re-sync the draft text buffers from the loaded document when the
    /// lineage changed (other image, or a new `rev` after commit/batch).
    /// Buffers the user edited since the last sync (`meta_buffers_dirty`)
    /// keep their keystrokes — only missing fields are filled — so the
    /// commit (which creates the document first) never wipes the edit it
    /// is about to save. Call [`Self::resync_meta_buffers`] after
    /// operations that intentionally replace the document content.
    pub(crate) fn ensure_meta_buffers(&mut self) {
        let key = self
            .document
            .as_ref()
            .map(|document| (self.path.trim().to_string(), document.metadata.latest_rev()));
        if self.meta_buffers_key == key {
            return;
        }
        if self.meta_buffers_dirty {
            let draft = self.metadata_draft();
            for id in METADATA_FIELD_IDS {
                self.meta_buffers
                    .entry((*id).to_string())
                    .or_insert_with(|| draft.get(*id).cloned().unwrap_or_default());
            }
            self.meta_buffers_key = key;
            return;
        }
        self.resync_meta_buffers();
    }

    /// Rebuild every draft buffer from the loaded document, discarding
    /// unsaved keystrokes. Used after operations that replace the document
    /// content (clear, preset apply, batch/sync reload of the open image).
    fn resync_meta_buffers(&mut self) {
        let draft = self.metadata_draft();
        self.meta_buffers.clear();
        for id in METADATA_FIELD_IDS {
            self.meta_buffers.insert(
                (*id).to_string(),
                draft.get(*id).cloned().unwrap_or_default(),
            );
        }
        self.meta_buffers_key = self
            .document
            .as_ref()
            .map(|document| (self.path.trim().to_string(), document.metadata.latest_rev()));
        self.meta_buffers_dirty = false;
    }

    /// Set one draft text buffer (panel input + headless tests). Unknown
    /// IDs — including `keywords`, which stays the document `keywords`
    /// field — fail loudly before anything is stored.
    pub fn set_metadata_buffer(&mut self, field: &str, value: String) -> Result<(), GuiError> {
        validate_metadata_field_value(field, "").map_err(GuiError::from)?;
        self.ensure_meta_buffers();
        self.meta_buffers.insert(field.to_string(), value);
        self.meta_buffers_dirty = true;
        Ok(())
    }

    /// Commit the draft buffers of the loaded image (diffed against the
    /// document: changed values are set, emptied buffers remove their
    /// field). All-or-nothing with `origin = "gui"` through
    /// [`SidecarDocument::apply_metadata_draft`] + [`Self::save_sidecar`]
    /// (CAS, atomar). Returns `Ok(true)` on update, `Ok(false)` for
    /// idempotent no-ops. Loud on invalid values — nothing is written then.
    pub fn commit_metadata_draft(&mut self) -> Result<bool, GuiError> {
        instrument_gui_action!(self, GuiAction::CommitMetadataDraft);
        self.ensure_document_loaded()?;
        self.ensure_meta_buffers();
        let current = self.metadata_draft();
        let mut fields = BTreeMap::new();
        for id in METADATA_FIELD_IDS {
            let buffered = self.meta_buffers.get(*id).cloned().unwrap_or_default();
            let stored = current.get(*id);
            if buffered.is_empty() {
                if stored.is_some() {
                    fields.insert((*id).to_string(), String::new());
                }
            } else if stored.is_none_or(|value| value != &buffered) {
                fields.insert((*id).to_string(), buffered);
            }
        }
        for (field, value) in &fields {
            validate_metadata_field_value(field, value)?;
        }
        if fields.is_empty() {
            info!("metadata draft for {} unchanged", self.path.trim());
            self.status = Str::MetadataDraftUnchanged.t().into();
            return Ok(false);
        }
        let timestamp = now_rfc3339_utc();
        let changed = {
            let document = self.document.as_mut().expect("document was ensured");
            document.apply_metadata_draft(&fields, Self::META_ORIGIN_GUI, &timestamp)?
        };
        if !changed {
            info!("metadata draft for {} unchanged", self.path.trim());
            self.status = Str::MetadataDraftUnchanged.t().into();
            return Ok(false);
        }
        let rev = self
            .document
            .as_ref()
            .map(|document| document.metadata.latest_rev())
            .unwrap_or(0);
        self.meta_buffers_key = Some((self.path.trim().to_string(), rev));
        self.meta_buffers_dirty = false;
        self.save_sidecar();
        self.refresh_entry(&PathBuf::from(self.path.trim()));
        if self.error().is_none() {
            info!(
                "metadata draft for {} updated (rev {rev})",
                self.path.trim()
            );
            self.status = Str::MetadataDraftSaved.t().into();
        }
        Ok(true)
    }

    /// KITTEST-COVERAGE-STATES-1: copy the current draft buffer values
    /// (non-empty fields, as shown in the editor — including unsaved
    /// keystrokes) into the metadata panel's own session clipboard. Pure
    /// session state: never touches the sidecar. Returns the field count.
    pub fn copy_metadata_draft(&mut self) -> Result<usize, GuiError> {
        instrument_gui_action!(self, GuiAction::CopyMetadataDraft);
        self.ensure_document_loaded()?;
        self.ensure_meta_buffers();
        let copied: BTreeMap<String, String> = METADATA_FIELD_IDS
            .iter()
            .filter_map(|id| {
                let value = self.meta_buffers.get(*id).cloned().unwrap_or_default();
                (!value.trim().is_empty()).then(|| ((*id).to_string(), value))
            })
            .collect();
        let count = copied.len();
        self.meta_clipboard = Some(copied);
        info!("metadata copy for {} ({count} field(s))", self.path.trim());
        self.status = Str::MetadataCopiedPattern.format_arg(&count.to_string());
        Ok(count)
    }

    /// KITTEST-COVERAGE-STATES-1: paste the metadata clipboard onto the loaded
    /// image through the same commit path as a manual draft edit (origin
    /// `gui`, CAS + atomar). The pasted values are written into the editor
    /// buffers first so the panel shows them. Loud without a prior copy —
    /// never a silent no-op.
    pub fn paste_metadata_draft(&mut self) -> Result<bool, GuiError> {
        instrument_gui_action!(self, GuiAction::PasteMetadataDraft);
        let clipboard = self
            .meta_clipboard
            .clone()
            .ok_or_else(|| GuiError::Io(Str::MetadataNothingToPaste.t().to_string()))?;
        self.ensure_document_loaded()?;
        self.ensure_meta_buffers();
        for (field, value) in &clipboard {
            self.meta_buffers.insert(field.clone(), value.clone());
        }
        self.meta_buffers_dirty = true;
        let changed = self.commit_metadata_draft()?;
        info!(
            "metadata paste for {} ({} field(s), changed: {changed})",
            self.path.trim(),
            clipboard.len()
        );
        Ok(changed)
    }

    /// Clear draft fields (and/or `keywords`) on the loaded image, mirroring
    /// the CLI `meta draft clear --field` (one history entry with
    /// `origin = "gui"`, CAS + atomar). Unknown IDs fail loudly with
    /// all-or-nothing semantics; already-absent fields are an idempotent
    /// no-op. Returns `Ok(true)` on update.
    pub fn clear_metadata_fields(&mut self, fields: &[String]) -> Result<bool, GuiError> {
        instrument_gui_action!(self, GuiAction::ClearMetadataDraft);
        for id in fields {
            if id != "keywords" && !is_metadata_field(id) {
                return Err(GuiError::Io(
                    Str::MetadataUnknownFieldPattern.format_arg(id),
                ));
            }
        }
        self.ensure_document_loaded()?;
        let timestamp = now_rfc3339_utc();
        let removed = {
            let document = self.document.as_mut().expect("document was ensured");
            let mut removed = BTreeSet::new();
            for id in fields {
                if id == "keywords" {
                    if !document.keywords.is_empty() {
                        document.keywords.clear();
                        removed.insert("keywords".to_string());
                    }
                } else if document.metadata.draft.remove(id).is_some() {
                    removed.insert(id.clone());
                }
            }
            if removed.is_empty() {
                info!("metadata clear for {} unchanged", self.path.trim());
                self.status = Str::MetadataDraftUnchanged.t().into();
                return Ok(false);
            }
            let removed_list: Vec<String> = removed.into_iter().collect();
            let rev = document.metadata.latest_rev() + 1;
            document.metadata.history.insert(
                0,
                MetadataHistoryEntry {
                    rev,
                    timestamp,
                    origin: Self::META_ORIGIN_GUI.to_string(),
                    changed: removed_list,
                },
            );
            document
                .metadata
                .history
                .truncate(MAX_METADATA_HISTORY_ENTRIES);
            document.validate()?;
            document
                .metadata
                .history
                .first()
                .map(|entry| entry.changed.join(", "))
                .unwrap_or_default()
        };
        self.resync_meta_buffers();
        self.save_sidecar();
        self.refresh_entry(&PathBuf::from(self.path.trim()));
        if self.error().is_none() {
            info!("metadata clear for {} removed {removed}", self.path.trim());
            self.status = Str::MetadataDraftCleared.t().into();
        }
        Ok(true)
    }

    /// Clear the whole draft (`--all`, history is kept and gains one entry),
    /// mirroring the CLI. Idempotent no-op on an already-empty draft.
    pub fn clear_metadata_draft_all(&mut self) -> Result<bool, GuiError> {
        self.ensure_document_loaded()?;
        let timestamp = now_rfc3339_utc();
        let removed = {
            let document = self.document.as_mut().expect("document was ensured");
            if !document.clear_metadata_draft(Self::META_ORIGIN_GUI, &timestamp)? {
                info!("metadata clear for {} unchanged", self.path.trim());
                self.status = Str::MetadataDraftUnchanged.t().into();
                return Ok(false);
            }
            document
                .metadata
                .history
                .first()
                .map(|entry| entry.changed.join(", "))
                .unwrap_or_default()
        };
        self.resync_meta_buffers();
        self.save_sidecar();
        self.refresh_entry(&PathBuf::from(self.path.trim()));
        if self.error().is_none() {
            info!("metadata clear for {} removed {removed}", self.path.trim());
            self.status = Str::MetadataDraftCleared.t().into();
        }
        Ok(true)
    }

    /// Explicitly clear the whole metadata history (the only way to empty
    /// it; draft values are kept). Returns the number of removed entries.
    pub fn clear_metadata_history_gui(&mut self) -> Result<usize, GuiError> {
        instrument_gui_action!(self, GuiAction::ClearMetadataHistory);
        self.ensure_document_loaded()?;
        let removed = {
            let document = self.document.as_mut().expect("document was ensured");
            let removed = document.metadata.history.len();
            document.clear_metadata_history();
            document.validate()?;
            removed
        };
        if removed == 0 {
            self.status = Str::MetadataDraftUnchanged.t().into();
            return Ok(0);
        }
        self.save_sidecar();
        if self.error().is_none() {
            info!("metadata history for {} cleared", self.path.trim());
            self.status = Str::MetadataHistoryClearedPattern.format_arg(&removed.to_string());
        }
        Ok(removed)
    }

    /// Embedded IPTC of the loaded image (JPEG IIM/XMP, read-only).
    /// Non-JPEG sources yield `Ok(None)` ("nicht verfügbar"); present-but-
    /// broken JPEG segments are a loud error, never a silent skip.
    pub fn embedded_metadata(&self) -> Result<Option<IptcMetadata>, GuiError> {
        let path = self.path.trim().to_string();
        if path.is_empty() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path)
            .map_err(|error| GuiError::Io(format!("cannot read `{path}`: {error}")))?;
        if bytes.len() < 2 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
            return Ok(None);
        }
        extract_metadata(&bytes).map(Some).map_err(|error| {
            GuiError::Io(Str::MetadataEmbeddedUnreadablePattern.format_arg(&error.to_string()))
        })
    }

    /// Cached [`Self::embedded_metadata`] for the panel: the file is only
    /// re-read when `(path, length, mtime)` changed, so hovering the panel
    /// never pays per-frame file IO. Failures stay visible as `Err` text.
    pub(crate) fn embedded_cached(&mut self) -> Result<Option<IptcMetadata>, String> {
        let path = self.path.trim().to_string();
        let fingerprint = std::fs::metadata(&path)
            .ok()
            .map(|meta| (meta.len(), meta.modified().ok()));
        let fresh = match (&self.meta_embedded_cache, &fingerprint) {
            (
                Some(EmbeddedCache {
                    path: cached_path,
                    len,
                    mtime,
                    ..
                }),
                Some((current_len, current_mtime)),
            ) if cached_path == &path && len == current_len && mtime == current_mtime => true,
            (None, None) if path.is_empty() => true,
            _ => false,
        };
        if !fresh {
            let result = self.embedded_metadata().map_err(|error| error.to_string());
            let (len, mtime) = fingerprint.unwrap_or((0, None));
            self.meta_embedded_cache = Some(EmbeddedCache {
                path,
                len,
                mtime,
                result: result.clone(),
            });
            return result;
        }
        self.meta_embedded_cache
            .as_ref()
            .map(|cache| cache.result.clone())
            .unwrap_or(Ok(None))
    }

    /// Override the user-global meta-presets directory (headless tests).
    /// `None` restores the production default.
    pub fn set_meta_presets_dir(&mut self, dir: Option<PathBuf>) {
        self.meta_presets_dir_override = dir;
    }

    /// Effective meta-presets directory: the test override or the
    /// user-global directory shared with edit presets (SOLL §5). `None`
    /// means the platform config base is unavailable (loud, no fallback).
    fn meta_presets_dir(&self) -> Option<PathBuf> {
        self.meta_presets_dir_override
            .clone()
            .or_else(default_meta_presets_dir)
    }

    /// Re-scan the meta-presets directory. A missing directory means "no
    /// presets saved yet" (not an error); broken files stay visible as
    /// failed entries — never skipped silently.
    pub fn refresh_meta_presets(&mut self) {
        match self.meta_presets_dir() {
            Some(dir) => {
                self.meta_preset_entries = scan_meta_presets_dir(&dir);
            }
            None => {
                self.meta_preset_entries.clear();
                self.status = Str::MetadataNoPresetDir.t().into();
            }
        }
    }

    /// Display names of the available (valid) meta presets, sorted.
    pub fn meta_preset_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for entry in &self.meta_preset_entries {
            if let MetaPresetEntry::Available { preset, .. } = entry {
                names.push(preset.name.clone());
            }
        }
        names.sort();
        names
    }

    /// Declared placeholders `(name, description)` of one preset.
    pub fn meta_preset_placeholders(&self, spec: &str) -> Result<Vec<(String, String)>, GuiError> {
        let (_, preset) = self.load_gui_meta_preset(spec)?;
        Ok(preset
            .placeholders
            .iter()
            .map(|placeholder| (placeholder.name.clone(), placeholder.description.clone()))
            .collect())
    }

    /// Resolve a preset spec (display name against the effective directory,
    /// or an explicit file path — same resolution as the CLI) and load it.
    /// `keywords` in `fields` and every other deviation fail loudly here,
    /// before any target is touched.
    fn load_gui_meta_preset(&self, spec: &str) -> Result<(PathBuf, MetaPresetFile), GuiError> {
        let explicit = self.meta_presets_dir_override.clone();
        let path = resolve_meta_preset_path(spec, explicit.as_deref()).map_err(|error| {
            GuiError::Io(format!("meta preset `{spec}` cannot be resolved: {error}"))
        })?;
        load_meta_preset_file(&path)
            .map(|preset| (path.clone(), preset))
            .map_err(|error| {
                GuiError::Io(format!(
                    "meta preset `{}` rejected: {error}",
                    path.display()
                ))
            })
    }

    /// Apply a meta preset to the loaded image: render upfront (every
    /// placeholder variable required; missing/unknown variables and limit
    /// violations abort with nothing written), then mutate via
    /// [`SidecarDocument::apply_metadata_draft`] (`origin =
    /// "preset:<name>"`) + [`Self::save_sidecar`]. Idempotent
    /// re-application reports `Ok(false)` without a history entry.
    pub fn apply_meta_preset_loaded(
        &mut self,
        spec: &str,
        vars: &BTreeMap<String, String>,
    ) -> Result<bool, GuiError> {
        instrument_gui_action!(self, GuiAction::ApplyMetaPreset);
        let (path, preset) = self.load_gui_meta_preset(spec)?;
        let resolved = render_meta_preset(&preset, vars, &path.display().to_string())
            .map_err(|error| GuiError::Io(format!("meta preset apply rejected: {error}")))?;
        self.ensure_document_loaded()?;
        let origin = format!("preset:{}", preset.name);
        let timestamp = now_rfc3339_utc();
        let changed = {
            let document = self.document.as_mut().expect("document was ensured");
            document.apply_metadata_draft(&resolved, &origin, &timestamp)?
        };
        if !changed {
            info!(
                "meta preset `{}` unchanged for {}",
                preset.name,
                self.path.trim()
            );
            self.status = Str::MetadataPresetUnchangedPattern.format_arg(&preset.name);
            return Ok(false);
        }
        self.resync_meta_buffers();
        self.save_sidecar();
        self.refresh_entry(&PathBuf::from(self.path.trim()));
        if self.error().is_none() {
            info!(
                "meta preset `{}` applied to {}",
                preset.name,
                self.path.trim()
            );
            self.status = Str::MetadataPresetAppliedPattern.format_arg(&preset.name);
        }
        Ok(true)
    }

    /// Apply a meta preset to every image of the filmstrip selection (empty
    /// selection falls back to the loaded image). The preset renders once
    /// upfront — a render failure aborts everything with nothing written
    /// (loud). Per target: CAS + atomic save; failures are loud per image
    /// (`error!` + report entry) and never abort the rest (Stapel-Muster,
    /// `metadata.md` §4).
    pub fn apply_meta_preset_to_selection(
        &mut self,
        spec: &str,
        vars: &BTreeMap<String, String>,
    ) -> SelectionSyncReport {
        let report = SelectionSyncReport::default();
        let (path, preset) = match self.load_gui_meta_preset(spec) {
            Ok(loaded) => loaded,
            Err(error) => {
                self.show_error(error);
                return report;
            }
        };
        let resolved = match render_meta_preset(&preset, vars, &path.display().to_string()) {
            Ok(resolved) => resolved,
            Err(error) => {
                self.show_error(format!("meta preset apply rejected: {error}"));
                return report;
            }
        };
        let origin = format!("preset:{}", preset.name);
        self.apply_resolved_to_selection(&resolved, &origin, "preset", &preset.name)
    }

    /// Field-selective metadata sync from the loaded image onto the
    /// filmstrip selection (empty selection falls back to the loaded image
    /// itself, like [`Self::apply_metadata_batch`]). Mirror semantics per
    /// target (SOLL §6): a source-absent field is removed on the target,
    /// `keywords` are replaced wholesale; non-selected fields stay
    /// untouched. `fields` must be non-empty (registry IDs, `keywords`
    /// allowed) — no silent transfer-all. A missing sidecar (source or
    /// target) is a loud error ("first open/import the image"), never a
    /// silent creation. Recipes, masks and edit history are never touched.
    pub fn sync_metadata_to_selection(&mut self, fields: &BTreeSet<String>) -> SelectionSyncReport {
        instrument_gui_action!(self, GuiAction::SyncMetadata);
        let report = SelectionSyncReport::default();
        if fields.is_empty() {
            self.show_error(Str::MetadataSyncNeedsFields.t());
            return report;
        }
        for id in fields {
            if id != "keywords" && !is_metadata_field(id) {
                self.show_error(Str::MetadataUnknownFieldPattern.format_arg(id));
                return report;
            }
        }
        let source = self.path.trim().to_string();
        if source.is_empty() {
            self.show_error(Str::NoImageLoaded.t());
            return report;
        }
        let source_sidecar = sidecar_path_for(Path::new(&source));
        let source_document = match load_sidecar(&source_sidecar) {
            Ok(document) => document,
            Err(_) => {
                self.show_error(Str::MetadataNoSidecarPattern.format_arg(&source));
                return report;
            }
        };
        let source_name = Path::new(&source)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty());
        let Some(source_name) = source_name else {
            self.show_error(Str::NoImageLoaded.t());
            return report;
        };
        let origin = format!("sync:{source_name}");
        let source_draft = source_document.metadata.draft.clone();
        let source_keywords = source_document.keywords.clone();
        let mut targets: Vec<String> = self.filmstrip_selection.iter().cloned().collect();
        if targets.is_empty() {
            targets.push(source.clone());
        }
        let mut report = SelectionSyncReport::default();
        for target in &targets {
            match Self::apply_meta_sync_to_target(
                Path::new(target),
                &source_draft,
                &source_keywords,
                fields,
                &origin,
            ) {
                Ok(true) => {
                    info!("metadata sync: `{target}` updated (from `{source_name}`)");
                    self.refresh_entry(Path::new(target));
                    if self.path.trim() == target.as_str() {
                        self.sidecar_revision = None;
                        self.reload_document_for_batch_target(Path::new(target));
                        self.resync_meta_buffers();
                    }
                    report.applied.push(target.clone());
                }
                Ok(false) => {
                    info!("metadata sync: `{target}` unchanged");
                    report.applied.push(target.clone());
                }
                Err(message) => {
                    error!("metadata sync failed for {target}: {message}");
                    report.failed.push((target.clone(), message));
                }
            }
        }
        if report.failed.is_empty() {
            self.status =
                Str::MetadataSyncAppliedPattern.format_arg(&report.applied.len().to_string());
        } else {
            let joined = report
                .failed
                .iter()
                .map(|(path, message)| format!("{path}: {message}"))
                .collect::<Vec<_>>()
                .join("; ");
            self.show_error(
                Str::MetadataSyncFailedPattern
                    .t()
                    .replacen("{}", &report.failed.len().to_string(), 1)
                    .replacen("{}", &joined, 1),
            );
        }
        report
    }

    /// Shared per-target loop for a pre-rendered draft (`preset:<name>`
    /// origin): CAS + atomic save per file, failures never abort the rest.
    /// `kind`/`name` only feed the status line and `info!` logs.
    fn apply_resolved_to_selection(
        &mut self,
        resolved: &BTreeMap<String, String>,
        origin: &str,
        kind: &str,
        name: &str,
    ) -> SelectionSyncReport {
        let mut targets: Vec<String> = self.filmstrip_selection.iter().cloned().collect();
        if targets.is_empty() && !self.path.trim().is_empty() {
            targets.push(self.path.trim().to_string());
        }
        let mut report = SelectionSyncReport::default();
        if targets.is_empty() {
            self.status = Str::NoImagesSelected.t().into();
            return report;
        }
        for target in &targets {
            match Self::apply_preset_to_target(Path::new(target), resolved, origin) {
                Ok(true) => {
                    info!("metadata {kind} `{name}`: `{target}` updated");
                    self.refresh_entry(Path::new(target));
                    if self.path.trim() == target.as_str() {
                        self.sidecar_revision = None;
                        self.reload_document_for_batch_target(Path::new(target));
                        self.resync_meta_buffers();
                    }
                    report.applied.push(target.clone());
                }
                Ok(false) => {
                    info!("metadata {kind} `{name}`: `{target}` unchanged");
                    report.applied.push(target.clone());
                }
                Err(message) => {
                    error!("metadata {kind} `{name}` failed for {target}: {message}");
                    report.failed.push((target.clone(), message));
                }
            }
        }
        if report.failed.is_empty() {
            self.status = Str::MetadataPresetBatchPattern
                .t()
                .replacen("{}", name, 1)
                .replacen("{}", &report.applied.len().to_string(), 1);
        } else {
            let joined = report
                .failed
                .iter()
                .map(|(path, message)| format!("{path}: {message}"))
                .collect::<Vec<_>>()
                .join("; ");
            self.show_error(
                Str::MetadataSyncFailedPattern
                    .t()
                    .replacen("{}", &report.failed.len().to_string(), 1)
                    .replacen("{}", &joined, 1),
            );
        }
        report
    }

    /// One atomic preset step for `target` (mirrors the CLI
    /// `apply_meta_preset_to_target`): load, mutate a clone via
    /// `apply_metadata_draft`, validate, CAS + atomic save. A missing
    /// sidecar is a loud per-target error, never a silent creation.
    fn apply_preset_to_target(
        target: &Path,
        resolved: &BTreeMap<String, String>,
        origin: &str,
    ) -> Result<bool, String> {
        let sidecar = sidecar_path_for(target);
        let document = match load_sidecar(&sidecar) {
            Ok(document) => document,
            Err(lumina_sidecar::SidecarError::Missing(_)) => {
                return Err(format!(
                    "no sidecar for `{}`; run `import` first",
                    target.display()
                ));
            }
            Err(error) => return Err(error.to_string()),
        };
        let expected = document_revision(&document).map_err(|error| error.to_string())?;
        let timestamp = now_rfc3339_utc();
        let mut candidate = document.clone();
        if !candidate
            .apply_metadata_draft(resolved, origin, &timestamp)
            .map_err(|error| error.to_string())?
        {
            return Ok(false);
        }
        sidecar_rebase::save_rebased_unit(&sidecar, &document, &candidate, Some(&expected))
            .map_err(|error| error.to_string())?;
        Ok(true)
    }

    /// One atomic sync step for `target` (mirrors the CLI
    /// `apply_meta_sync_to_target`): mirror the selected source fields onto
    /// a clone (source-absent fields removed, `keywords` replaced
    /// wholesale), exactly one history entry (`origin`), validate, CAS +
    /// atomic save. Idempotent no-ops report `Ok(false)` without a history
    /// entry or write.
    fn apply_meta_sync_to_target(
        target: &Path,
        source_draft: &BTreeMap<String, String>,
        source_keywords: &[String],
        fields: &BTreeSet<String>,
        origin: &str,
    ) -> Result<bool, String> {
        let sidecar = sidecar_path_for(target);
        let document = match load_sidecar(&sidecar) {
            Ok(document) => document,
            Err(lumina_sidecar::SidecarError::Missing(_)) => {
                return Err(format!(
                    "no sidecar for `{}`; run `import` first",
                    target.display()
                ));
            }
            Err(error) => return Err(error.to_string()),
        };
        let expected = document_revision(&document).map_err(|error| error.to_string())?;
        let timestamp = now_rfc3339_utc();
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
                timestamp,
                origin: origin.to_string(),
                changed: changed_list,
            },
        );
        candidate
            .metadata
            .history
            .truncate(MAX_METADATA_HISTORY_ENTRIES);
        candidate.validate().map_err(|error| error.to_string())?;
        sidecar_rebase::save_rebased_unit(&sidecar, &document, &candidate, Some(&expected))
            .map_err(|error| error.to_string())?;
        Ok(true)
    }
}
