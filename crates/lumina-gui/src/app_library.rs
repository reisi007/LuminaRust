//! Library module state: crop mode, filter bar, compare/survey views, split view,
//! fullscreen, stacks, keywords, collections and the metadata batch.
//! The inherent `impl LuminaApp` block of `lib.rs`, moved verbatim
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100 — the last 1000-line Ratchet exception).
//! One inherent impl may be spread over several modules of the same crate, so
//! this is a pure structural move: every body below is byte-identical to its
//! `lib.rs` text. The only differences are this header, the `use super::*;`
//! glob and the `pub(crate)` tokens on the private helpers that a sibling
//! module calls (listed in `scripts/file_size_baseline.txt`).

use super::*;

impl LuminaApp {
    /// Toggle the crop-mode badge (`R`, Welle 2). Display-only: while armed,
    /// the preview header advertises the mode; edits stay in the Geometry
    /// Crop controls. Never mutates the recipe.
    pub fn toggle_crop_mode(&mut self) {
        instrument_gui_action!(self, GuiAction::ToggleCropMode);
        self.crop_mode = !self.crop_mode;
        // R5-TOOLFLOW-1 (User-Entscheid 2026-09-20): arming the geometry tool
        // disarms the source-coordinate tools (mask / WB / red-eye / spot)
        // instead of leaving them armed-but-refused. The reverse direction
        // (geometry -> mask/WB) commits the crop draft first, see
        // `commit_outgoing_tool_for_switch`.
        if self.crop_mode {
            self.set_mask_tool(MaskTool::None);
            self.cancel_armed_preview_tools();
        }
        // UX-LOOK-CROP-18b: arming/leaving crop mode changes the preview
        // texture (full frame vs. committed crop) — invalidate.
        self.mark_dirty();
        info!("GUI interaction: toggle_crop_mode -> {}", self.crop_mode);
        self.set_crop_mode_status();
    }

    /// Toggle the Library filter drawer (`\`, Welle 3, LR-13 light).
    /// Display-only: shows/hides the text filter + Quick Develop sliders in
    /// the Library grid. Never mutates the recipe.
    pub fn toggle_filter_bar(&mut self) {
        instrument_gui_action!(self, GuiAction::ToggleFilterBar);
        self.filter_bar_visible = !self.filter_bar_visible;
        info!(
            "GUI interaction: toggle_filter_bar -> {}",
            self.filter_bar_visible
        );
        self.status = if self.filter_bar_visible {
            Str::FilterShown.t().into()
        } else {
            Str::FilterHidden.t().into()
        };
    }

    /// Set the Library text filter query (Welle 3, LR-13 light). Display-only;
    /// matched by [`library_filter_matches`] against the scanned entry
    /// metadata. Never mutates the recipe.
    pub fn set_library_filter(&mut self, query: impl Into<String>) {
        self.library_filter = query.into();
        trace!(
            "GUI interaction: set_library_filter {:?}",
            self.library_filter
        );
    }

    /// Active compare/survey proxy mode (Welle 3, LR-20 light). Read-only
    /// accessor for badges and headless tests.
    pub fn compare_mode(&self) -> Option<CompareMode> {
        self.compare_mode
    }

    /// Toggle a compare/survey view (Welle 3, LR-20 light). `Compare` (`C`)
    /// reuses the existing Before/After path (full-frame Before proxy, never
    /// a recipe mutation); `Survey` (`N`) jumps to the Library grid (survey
    /// proxy over the file-browser entries) and clears Before/After. A repeat
    /// press leaves the view. Never mutates the recipe.
    pub fn toggle_compare_mode(&mut self, mode: CompareMode) {
        instrument_gui_action!(self, GuiAction::ToggleCompareMode);
        trace!("GUI interaction: toggle_compare_mode {:?}", mode);
        match mode {
            CompareMode::Compare => {
                if self.compare_mode == Some(CompareMode::Compare) && self.before_after {
                    self.compare_mode = None;
                    self.before_after = false;
                    self.status = Str::CompareOff.t().into();
                } else {
                    self.compare_mode = Some(CompareMode::Compare);
                    self.before_after = true;
                    self.status = Str::CompareOnPattern.format_arg(Str::CompareModeCompare.t());
                }
            }
            CompareMode::Survey => {
                if self.compare_mode == Some(CompareMode::Survey) {
                    self.compare_mode = None;
                    self.library_view = LibraryView::Grid;
                    self.status = Str::CompareOff.t().into();
                } else {
                    self.compare_mode = Some(CompareMode::Survey);
                    self.before_after = false;
                    self.set_module(Module::Library);
                    self.library_view = LibraryView::Survey;
                    self.status = Str::SurveyOn.t().into();
                }
            }
        }
    }

    /// Active Library view (G-09, LRPAR-G09-LIB). Read-only accessor for
    /// badges and headless tests.
    pub fn library_view(&self) -> LibraryView {
        self.library_view
    }

    /// Set the Library view deterministically (G-09). Display-only like
    /// [`Self::toggle_compare_mode`]: `Compare` holds the Before image via
    /// the existing `before_after` path, `Survey`/`Grid`/`Loupe` clear it;
    /// `Survey` and `Grid` and `Loupe` all live in the Library module, so
    /// they switch `active_module` there. Never mutates the recipe.
    pub fn set_library_view(&mut self, view: LibraryView) {
        instrument_gui_action!(self, GuiAction::SetLibraryView);
        trace!("GUI interaction: set_library_view {:?}", view);
        self.library_view = view;
        match view {
            LibraryView::Grid | LibraryView::Loupe | LibraryView::Survey => {
                self.set_module(Module::Library);
                self.before_after = false;
                self.compare_mode = match view {
                    LibraryView::Survey => Some(CompareMode::Survey),
                    _ => None,
                };
                self.status = match view {
                    LibraryView::Survey => Str::SurveyOn.t().into(),
                    LibraryView::Loupe => Str::LoupeOn.t().into(),
                    _ => Str::LibraryGridOn.t().into(),
                };
            }
            LibraryView::Compare => {
                self.set_module(Module::Library);
                self.compare_mode = Some(CompareMode::Compare);
                self.before_after = true;
                self.status = Str::CompareOnPattern.format_arg(Str::CompareModeCompare.t());
            }
            LibraryView::People => {
                self.set_module(Module::Library);
                self.before_after = false;
                self.compare_mode = None;
                self.status = Str::FacePeople.t().into();
            }
        }
    }

    /// Toggle the split Before/After marker (`Shift+Y`, Welle 3, LR-09
    /// light). Display-only: enabling also holds the Before image via the
    /// existing `before_after` path (full-frame Before proxy — a true
    /// side-by-side split render is documented follow-up work, see
    /// `feature/platform/cli-gui-wasm.md`). Never mutates the recipe.
    pub fn toggle_split_view(&mut self) {
        instrument_gui_action!(self, GuiAction::ToggleSplitView);
        self.before_after_split = !self.before_after_split;
        if self.before_after_split {
            self.before_after = true;
        }
        info!(
            "GUI interaction: toggle_split_view -> {}",
            self.before_after_split
        );
        self.status = if self.before_after_split {
            Str::SplitViewOn.t().into()
        } else {
            Str::SplitViewOff.t().into()
        };
    }

    /// Toggle the fullscreen preview (`F`, Welle 3). Display-only: hides the
    /// same chrome as lights-out (see [`Self::chrome_hidden`]) and settles
    /// the zoom on Fit when enabling, so the previous `F`-zoom-to-fit
    /// behaviour is preserved on entry. Never mutates the recipe.
    pub fn toggle_fullscreen(&mut self) {
        instrument_gui_action!(self, GuiAction::ToggleFullscreen);
        self.fullscreen = !self.fullscreen;
        info!("GUI interaction: toggle_fullscreen -> {}", self.fullscreen);
        if self.fullscreen {
            self.set_zoom_mode(ZoomMode::Fit);
        }
        self.status = if self.fullscreen {
            Str::FullscreenOn.t().into()
        } else {
            Str::FullscreenOff.t().into()
        };
    }

    /// Set the fullscreen preview deterministically (F-100 Startverhalten,
    /// `--fullscreen` CLI flag). Display-only like [`Self::toggle_fullscreen`]:
    /// hides the same chrome as lights-out (see [`Self::chrome_hidden`]) and
    /// settles the zoom on Fit when enabling. Never mutates the recipe.
    /// No-op when already in the requested state (so a default `false` at
    /// startup leaves the status line untouched).
    pub fn set_fullscreen(&mut self, enabled: bool) {
        if self.fullscreen == enabled {
            return;
        }
        trace!("GUI interaction: set_fullscreen -> {enabled}");
        self.fullscreen = enabled;
        if enabled {
            self.set_zoom_mode(ZoomMode::Fit);
        }
        self.status = if enabled {
            Str::FullscreenOn.t().into()
        } else {
            Str::FullscreenOff.t().into()
        };
    }

    /// Whether the fullscreen working view is armed (read-only accessor for
    /// the `main()` startup wiring and headless tests; mirrors
    /// [`Self::set_fullscreen`]).
    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// Current stack-group proxy id of the active virtual copy (Welle 3,
    /// LR-17 light), read tolerantly via [`stack_id_of`]. Returns `None`
    /// when no document is loaded. Read-only accessor for headless tests.
    pub fn stack_group_id(&self) -> Option<String> {
        self.document.as_ref().and_then(|document| {
            document
                .virtual_copies
                .iter()
                .find(|copy| copy.id == self.virtual_copy_id)
                .and_then(|copy| stack_id_of(&copy.extras))
        })
    }

    /// Toggle stack-group membership of the active virtual copy (`Cmd/Ctrl+G`,
    /// Welle 3, LR-17 light). Grouping proxy without a schema change: the
    /// first press mints a `stack-<n>` id (unique across the loaded
    /// document's copies) into the copy's `extras["stack_group"]`, the second
    /// press removes it again. Persists through [`Self::save_sidecar`].
    pub fn toggle_stack_group(&mut self) -> Result<Option<String>, GuiError> {
        instrument_gui_action!(self, GuiAction::ToggleStackGroup);
        self.ensure_document_loaded()?;
        if stack_id_of(&self.active_copy_mut()?.extras).is_some() {
            self.active_copy_mut()?.extras.remove("stack_group");
            self.save_sidecar();
            self.status = Str::StackUngrouped.t().into();
            info!("GUI interaction: toggle_stack_group -> ungrouped");
            return Ok(None);
        }
        let mut counter = 0usize;
        for copy in &self
            .document
            .as_ref()
            .expect("document was ensured")
            .virtual_copies
        {
            if let Some(id) = stack_id_of(&copy.extras) {
                if let Some(n) = id
                    .strip_prefix("stack-")
                    .and_then(|rest| rest.parse::<usize>().ok())
                {
                    counter = counter.max(n);
                }
            }
        }
        let new_id = loop {
            counter += 1;
            let candidate = format!("stack-{counter}");
            let taken = self
                .document
                .as_ref()
                .expect("document was ensured")
                .virtual_copies
                .iter()
                .any(|copy| stack_id_of(&copy.extras).as_deref() == Some(&candidate));
            if !taken {
                break candidate;
            }
        };
        self.active_copy_mut()?
            .extras
            .insert("stack_group".into(), Value::String(new_id.clone()));
        self.save_sidecar();
        self.status = Str::StackGroupedPattern.format_arg(&new_id);
        info!("GUI interaction: toggle_stack_group -> {new_id}");
        Ok(Some(new_id))
    }

    /// Source-level keywords of the loaded document (G-15 META-MVP, Slice 3).
    /// Empty without a loaded document. Read-only accessor for the Library
    /// panel and headless tests.
    pub fn keywords(&self) -> Vec<String> {
        self.document
            .as_ref()
            .map(|document| document.keywords.clone())
            .unwrap_or_default()
    }

    /// Add a keyword to the loaded image (G-15 META-MVP, Slice 3) via the
    /// Slice-1 `BatchOp::AddKeyword` language + [`Self::save_sidecar`]
    /// (CAS, atomar). Idempotent: an existing keyword succeeds unchanged.
    /// Invalid keywords fail loudly, never silently normalised.
    pub fn add_keyword(&mut self, keyword: &str) -> Result<bool, GuiError> {
        instrument_gui_action!(self, GuiAction::AddKeyword);
        self.ensure_document_loaded()?;
        let Some(document) = &mut self.document else {
            return Err(GuiError::Io(Str::NoSidecarLoaded.t().to_string()));
        };
        let changed = apply_batch_op(
            document,
            &BatchOp::AddKeyword {
                keyword: keyword.to_string(),
            },
        )?;
        if changed {
            self.save_sidecar();
            self.refresh_entry(&PathBuf::from(self.path.trim()));
            info!("keyword `{keyword}` added to {}", self.path.trim());
            self.status = Str::KeywordAddedPattern.format_arg(keyword);
        } else {
            info!(
                "keyword `{keyword}` already present on {}",
                self.path.trim()
            );
            self.status = Str::KeywordUnchangedPattern.format_arg(keyword);
        }
        Ok(changed)
    }

    /// Remove a keyword from the loaded image (G-15 META-MVP, Slice 3),
    /// mirroring [`Self::add_keyword`]. Absent keywords succeed unchanged.
    pub fn remove_keyword(&mut self, keyword: &str) -> Result<bool, GuiError> {
        instrument_gui_action!(self, GuiAction::RemoveKeyword);
        self.ensure_document_loaded()?;
        let Some(document) = &mut self.document else {
            return Err(GuiError::Io(Str::NoSidecarLoaded.t().to_string()));
        };
        let changed = apply_batch_op(
            document,
            &BatchOp::RemoveKeyword {
                keyword: keyword.to_string(),
            },
        )?;
        if changed {
            self.save_sidecar();
            self.refresh_entry(&PathBuf::from(self.path.trim()));
            info!("keyword `{keyword}` removed from {}", self.path.trim());
            self.status = Str::KeywordRemovedPattern.format_arg(keyword);
        } else {
            info!("keyword `{keyword}` absent on {}", self.path.trim());
            self.status = Str::KeywordUnchangedPattern.format_arg(keyword);
        }
        Ok(changed)
    }

    /// Source-level static collection memberships of the loaded document
    /// (G-15 META-MVP, Slice 3). Empty without a loaded document.
    pub fn collections(&self) -> Vec<CollectionMembership> {
        self.document
            .as_ref()
            .map(|document| document.collections.clone())
            .unwrap_or_default()
    }

    /// Split a collection assignment `id=name` at the first `=` (CLI-compat,
    /// G-15 META-MVP Slice 2 `split_collection_assignment`). A missing `=`
    /// is a loud error, never a silent `id == name`. Pure helper shared by
    /// the panel and headless tests.
    pub fn split_collection_assignment(value: &str) -> Result<(String, String), GuiError> {
        value.split_once('=').map_or_else(
            || {
                Err(GuiError::Io(
                    Str::InvalidCollectionAssignment.format_arg(value),
                ))
            },
            |(id, name)| Ok((id.to_string(), name.to_string())),
        )
    }

    /// Add the loaded image to a static collection (G-15 META-MVP, Slice 3)
    /// via `BatchOp::AddToCollection` + [`Self::save_sidecar`]. An existing
    /// `id` refreshes the display `name` (rename, like the CLI). Loud
    /// validation, `info!` logging, entry refresh — like [`Self::add_keyword`].
    pub fn add_to_collection(&mut self, id: &str, name: &str) -> Result<bool, GuiError> {
        self.ensure_document_loaded()?;
        let Some(document) = &mut self.document else {
            return Err(GuiError::Io(Str::NoSidecarLoaded.t().to_string()));
        };
        let changed = apply_batch_op(
            document,
            &BatchOp::AddToCollection {
                id: id.to_string(),
                name: name.to_string(),
            },
        )?;
        if changed {
            self.save_sidecar();
            self.refresh_entry(&PathBuf::from(self.path.trim()));
            info!("collection `{id}` ({name}) joined by {}", self.path.trim());
            self.status = Str::CollectionJoinedPattern.format_arg(name);
        } else {
            info!("collection `{id}` unchanged for {}", self.path.trim());
            self.status = Str::CollectionUnchangedPattern.format_arg(name);
        }
        Ok(changed)
    }

    /// Remove the loaded image from a static collection (G-15 META-MVP,
    /// Slice 3), mirroring [`Self::add_to_collection`].
    pub fn remove_from_collection(&mut self, id: &str) -> Result<bool, GuiError> {
        self.ensure_document_loaded()?;
        let Some(document) = &mut self.document else {
            return Err(GuiError::Io(Str::NoSidecarLoaded.t().to_string()));
        };
        let changed = apply_batch_op(
            document,
            &BatchOp::RemoveFromCollection { id: id.to_string() },
        )?;
        if changed {
            self.save_sidecar();
            self.refresh_entry(&PathBuf::from(self.path.trim()));
            info!("collection `{id}` left by {}", self.path.trim());
            self.status = Str::CollectionLeftPattern.format_arg(id);
        } else {
            info!("collection `{id}` absent on {}", self.path.trim());
            self.status = Str::CollectionUnchangedPattern.format_arg(id);
        }
        Ok(changed)
    }

    /// Static collections aggregated from the scanned entries (G-15 META-MVP,
    /// Slice 3): `(id, name, member_count)`, sorted by name. Sidecar-first —
    /// rebuilt from the scan on every call, never a second store. A
    /// divergent `id → name` (renamed in some sidecars only) keeps the
    /// first-seen name; renaming across all sidecars is a batch operation
    /// (see [`Self::apply_metadata_batch`]).
    pub fn static_collections(&self) -> Vec<(String, String, usize)> {
        let mut aggregated: BTreeMap<String, (String, usize)> = BTreeMap::new();
        for entry in &self.entries {
            for membership in &entry.collections {
                aggregated
                    .entry(membership.id.clone())
                    .and_modify(|(_, count)| *count += 1)
                    .or_insert_with(|| (membership.name.clone(), 1));
            }
        }
        let mut out: Vec<(String, String, usize)> = aggregated
            .into_iter()
            .map(|(id, (name, count))| (id, name, count))
            .collect();
        out.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        out
    }

    /// Select the Library collection filter (G-15 META-MVP, Slice 3):
    /// display-only, never persisted. `None` shows all images.
    pub fn set_active_collection(&mut self, filter: Option<CollectionFilter>) {
        trace!("GUI interaction: set_active_collection {filter:?}");
        self.active_collection = filter;
    }

    /// Apply one [`BatchOp`] to every image of the filmstrip selection
    /// (G-15 META-MVP, Slice 3 — Stapel-Vollfunktion). An empty selection
    /// falls back to the loaded image so the action is never a silent
    /// no-op. Per file: `load_sidecar` → `apply_batch_op` → CAS+rebase save
    /// (`validate` runs inside the write). `SetRating`/`SetFlag` with an empty `copy_id`
    /// (see [`parse_metadata_batch_op`]) resolve against the target's
    /// default copy. Failures are loud per image (`error!` + report entry)
    /// and never abort the rest — same pattern as
    /// [`Self::sync_settings_to_selection`]. Recipes, masks and history are
    /// never touched; the original bytes are never read, let alone written.
    pub fn apply_metadata_batch(&mut self, op: &BatchOp) -> SelectionSyncReport {
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
            match Self::apply_metadata_op_to_path(Path::new(target), op) {
                Ok(true) => {
                    info!("batch-meta: `{target}` updated");
                    self.refresh_entry(Path::new(target));
                    // Keep the loaded document in sync when the batch touched
                    // the open image (otherwise the panel shows stale data).
                    if self.path.trim() == target.as_str() {
                        self.sidecar_revision = None;
                        self.reload_document_for_batch_target(Path::new(target));
                    }
                    report.applied.push(target.clone());
                }
                Ok(false) => {
                    info!("batch-meta: `{target}` unchanged");
                    report.applied.push(target.clone());
                }
                Err(message) => {
                    error!("batch-meta failed for {target}: {message}");
                    report.failed.push((target.clone(), message));
                }
            }
        }
        if report.failed.is_empty() {
            self.status = Str::BatchAppliedPattern.format_arg(&report.applied.len().to_string());
        } else {
            let joined = report
                .failed
                .iter()
                .map(|(path, message)| format!("{path}: {message}"))
                .collect::<Vec<_>>()
                .join("; ");
            self.show_error(format!(
                "Batch failed for {} image(s): {joined}",
                report.failed.len()
            ));
        }
        report
    }

    /// One atomic batch step for `path` (G-15 META-MVP, Slice 3); returns
    /// `Ok(changed)`. Missing sidecar/invented identity stays a loud error.
    fn apply_metadata_op_to_path(path: &Path, op: &BatchOp) -> Result<bool, String> {
        let sidecar_path = lumina_sidecar::sidecar_path_for(path);
        let mut document = lumina_sidecar::load_sidecar(&sidecar_path)
            .map_err(|error| format!("{}: {error}", sidecar_path.display()))?;
        // Resolve selector ops (empty `copy_id` from the batch bar) against
        // the target's default copy — loudly when the document is empty.
        let resolved = match op {
            BatchOp::SetRating { copy_id, rating } if copy_id.is_empty() => {
                let id = default_copy_id(&document).ok_or_else(|| {
                    format!("{}: no virtual copy for set_rating", sidecar_path.display())
                })?;
                BatchOp::SetRating {
                    copy_id: id,
                    rating: *rating,
                }
            }
            BatchOp::SetFlag { copy_id, flag } if copy_id.is_empty() => {
                let id = default_copy_id(&document).ok_or_else(|| {
                    format!("{}: no virtual copy for set_flag", sidecar_path.display())
                })?;
                BatchOp::SetFlag {
                    copy_id: id,
                    flag: *flag,
                }
            }
            _ => op.clone(),
        };
        let base = document.clone();
        let changed =
            apply_batch_op(&mut document, &resolved).map_err(|error| error.to_string())?;
        if changed {
            let expected =
                lumina_sidecar::document_revision(&base).map_err(|error| error.to_string())?;
            sidecar_rebase::save_rebased_unit(&sidecar_path, &base, &document, Some(&expected))
                .map_err(|error| error.to_string())?;
        }
        Ok(changed)
    }

    /// Re-read the loaded document after a batch touched the open image, so
    /// the panel never shows stale keywords/collections/rating. The session
    /// recipe follows the reloaded active copy; failures stay visible via
    /// `show_error` (the in-memory lineage is kept).
    pub(crate) fn reload_document_for_batch_target(&mut self, path: &Path) {
        let sidecar_path = lumina_sidecar::sidecar_path_for(path);
        match lumina_sidecar::load_sidecar(&sidecar_path) {
            Ok(document) => {
                if let Some(copy) = document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == self.virtual_copy_id)
                {
                    self.recipe = copy.recipe.clone();
                }
                // REVIEW-GUI-N1: re-anchor the CAS revision to the reloaded
                // lineage so the next `save_sidecar` compares against the
                // batch-written file instead of conflicting with it.
                self.sidecar_revision = lumina_sidecar::document_revision(&document).ok();
                self.document = Some(document);
            }
            Err(error) => self.show_error(error),
        }
    }

    /// Load a portable smart-collection catalog (G-15 META-MVP, Slice 3) in
    /// the CLI-identical format
    /// (`{"format":"lumina-smart-catalog","version":1,"collections":[...]}`).
    /// Every deviation (unreadable file, invalid JSON, wrong format/version,
    /// invalid definition) is a loud error — never a silent empty catalog.
    pub fn load_smart_catalog(&mut self, path: &str) -> Result<usize, GuiError> {
        let text = std::fs::read_to_string(path).map_err(|error| {
            GuiError::Io(format!("cannot read smart catalog `{path}`: {error}"))
        })?;
        let catalog: SmartCatalogFile = serde_json::from_str(&text)
            .map_err(|error| GuiError::Io(format!("invalid smart catalog `{path}`: {error}")))?;
        if catalog.format != SMART_CATALOG_FORMAT {
            return Err(GuiError::Io(
                Str::InvalidSmartCatalogFormat.format_arg(&catalog.format),
            ));
        }
        if catalog.version != SMART_COLLECTION_VERSION {
            return Err(GuiError::Io(
                Str::InvalidSmartCatalogVersion.format_arg(&catalog.version.to_string()),
            ));
        }
        for def in &catalog.collections {
            validate_smart_collection_def(def).map_err(GuiError::from)?;
        }
        let count = catalog.collections.len();
        self.smart_catalog = catalog.collections;
        self.smart_catalog_path = path.to_string();
        info!("smart catalog `{path}` loaded ({count} collection(s))");
        self.status = Str::SmartCatalogLoadedPattern.format_arg(&count.to_string());
        Ok(count)
    }

    /// Save the session smart catalog to `path` (explicit user action only,
    /// G-15 META-MVP Slice 3). The file carries versioned rule data only —
    /// never absolute paths. Atomic write via `write_atomically`.
    pub fn save_smart_catalog(&mut self, path: &str) -> Result<(), GuiError> {
        for def in &self.smart_catalog {
            validate_smart_collection_def(def).map_err(GuiError::from)?;
        }
        let catalog = SmartCatalogFile {
            format: SMART_CATALOG_FORMAT.to_string(),
            version: SMART_COLLECTION_VERSION,
            collections: self.smart_catalog.clone(),
        };
        let text = serde_json::to_string_pretty(&catalog)
            .map_err(|error| GuiError::Io(format!("cannot encode smart catalog: {error}")))?;
        lumina_sidecar::write_atomically(Path::new(path), text.as_bytes())?;
        self.smart_catalog_path = path.to_string();
        info!(
            "smart catalog `{path}` saved ({} collection(s))",
            self.smart_catalog.len()
        );
        self.status =
            Str::SmartCatalogSavedPattern.format_arg(&self.smart_catalog.len().to_string());
        Ok(())
    }

    /// Create a smart collection from the caller-held rule stack (G-15
    /// META-MVP, Slice 3). Requires exactly one combined rule on the stack
    /// (see [`combine_smart_rules`]); `id`/`name` validate like static
    /// collections (non-empty, no surrounding whitespace, `id` without
    /// `/`, `\`, `:`). Duplicate `id`s and invalid rules fail loudly.
    pub fn create_smart_collection(&mut self, id: &str, name: &str) -> Result<(), GuiError> {
        if self.smart_rule_stack.len() != 1 {
            return Err(GuiError::Io(Str::SmartNeedsSingleRule.t().to_string()));
        }
        if self.smart_catalog.iter().any(|def| def.id == id) {
            return Err(GuiError::Io(Str::SmartDuplicateId.format_arg(id)));
        }
        let def = SmartCollectionDef {
            version: SMART_COLLECTION_VERSION,
            id: id.to_string(),
            name: name.to_string(),
            rule: self
                .smart_rule_stack
                .pop()
                .expect("stack length was checked"),
        };
        validate_smart_collection_def(&def).map_err(GuiError::from)?;
        self.smart_catalog.push(def);
        info!("smart collection `{id}` created");
        self.status = Str::SmartCreatedPattern.format_arg(id);
        Ok(())
    }

    /// Delete a smart collection by `id` (G-15 META-MVP, Slice 3). Unknown
    /// ids fail loudly; an active filter on the deleted id is cleared so the
    /// grid never filters on a ghost.
    pub fn delete_smart_collection(&mut self, id: &str) -> Result<(), GuiError> {
        let before = self.smart_catalog.len();
        self.smart_catalog.retain(|def| def.id != id);
        if self.smart_catalog.len() == before {
            return Err(GuiError::Io(Str::SmartUnknownId.format_arg(id)));
        }
        if self.active_collection == Some(CollectionFilter::Smart { id: id.to_string() }) {
            self.active_collection = None;
        }
        info!("smart collection `{id}` deleted");
        self.status = Str::SmartDeletedPattern.format_arg(id);
        Ok(())
    }

    /// Push one [`build_smart_rule`] rule onto the composition stack (G-15
    /// META-MVP, Slice 3). Loud on unknown kind / bad value.
    pub fn push_smart_rule(&mut self, kind: &str, value: &str) -> Result<(), GuiError> {
        let rule = build_smart_rule(kind, value)
            .map_err(|message| GuiError::Io(format!("invalid smart rule: {message}")))?;
        self.smart_rule_stack.push(rule);
        trace!("GUI interaction: push_smart_rule {kind}");
        Ok(())
    }

    /// Combine the rule stack with `and`/`or`/`not` (see
    /// [`combine_smart_rules`]). Loud on underflow / unknown op.
    pub fn combine_smart_stack(&mut self, op: &str) -> Result<(), GuiError> {
        combine_smart_rules(&mut self.smart_rule_stack, op)
            .map_err(|message| GuiError::Io(format!("invalid smart-rule combine: {message}")))?;
        trace!("GUI interaction: combine_smart_stack {op}");
        Ok(())
    }

    // ---- LRPAR-G15-IPTC-S8: Library Metadata panel -----------------------
    //
    // Every mutation below travels the same sidecar path as the CLI (`meta
    // draft`, `meta preset apply`, `meta sync`): load → mutate a clone with
    // the S1 helpers (`apply_metadata_draft`, history handling) → validate →
    // CAS + atomic rebase (`save_rebased`): concurrent changes merge
    // field-selectively; a persistent conflict is a loud error, never silent
    // last-write-wins. `keywords` in preset `fields` stays loudly rejected
    // (S4 semantics); an empty draft value removes the field (S1 semantics).
    // No second metadata logic lives in the GUI.

    /// Origin marker for history entries written through this panel.
    pub(crate) const META_ORIGIN_GUI: &'static str = "gui";
}
