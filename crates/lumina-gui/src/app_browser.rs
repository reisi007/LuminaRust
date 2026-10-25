//! Folder listing/picker and the file-browser scan, plus the filmstrip selection
//! clicks.
//! The inherent `impl LuminaApp` block of `lib.rs`, moved verbatim
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100 — the last 1000-line Ratchet exception).
//! One inherent impl may be spread over several modules of the same crate, so
//! this is a pure structural move: every body below is byte-identical to its
//! `lib.rs` text. The only differences are this header, the `use super::*;`
//! glob and the `pub(crate)` tokens on the private helpers that a sibling
//! module calls (listed in `scripts/file_size_baseline.txt`).

use super::*;

impl LuminaApp {
    /// Navigate to `directory` (folder tree click, `Open`, startup workdir).
    /// R4-LIB-1(a): the grid aggregates subfolder images (F-100), so every
    /// navigation path uses the recursive listing (depth-limited; `.lumina/`
    /// excluded) — the former flat tree-click listing hid subfolder images.
    pub fn set_directory(&mut self, directory: impl Into<String>) {
        self.directory = directory.into();
        info!("directory set: {}", self.directory);
        // LRPAR-G01-BASIC: keep the folder-inherited reset-sliders flag in
        // sync on explicit navigation (same refresh as `open_file`).
        let folder = PathBuf::from(self.directory.trim());
        self.refresh_reset_sliders_flag(&folder);
        self.list_directory();
    }

    /// Current working directory (read-only accessor for the `main()` startup
    /// wiring and headless tests; mirrors [`Self::set_directory`]).
    pub fn directory(&self) -> &str {
        &self.directory
    }

    /// UX-SLICE-2 (F2): install a deterministic folder-picker seam for headless
    /// tests (a real `rfd` dialog needs a display server). Production leaves
    /// this unset and [`Self::open_folder`] opens the native dialog.
    pub fn set_folder_picker(&mut self, picker: impl FnMut() -> Option<PathBuf> + 'static) {
        self.folder_picker = Some(Box::new(picker));
    }

    /// UX-SLICE-2 (F2): the truthful "Open Folder" action — pick a directory
    /// through the native dialog (or the injected test seam) and re-list it.
    /// A cancelled dialog is a deliberate no-op: the previous directory and its
    /// state stay untouched, and no status/error is fabricated.
    pub fn open_folder(&mut self) {
        let picked = match self.folder_picker.as_mut() {
            Some(picker) => picker(),
            None => rfd::FileDialog::new().pick_folder(),
        };
        let Some(folder) = picked else {
            trace!("GUI interaction: open folder cancelled");
            return;
        };
        info!("open folder: {}", folder.display());
        self.set_directory(folder.display().to_string());
    }

    /// Recursive aggregation (F-100 Library): images of the chosen folder
    /// *including* subfolders, symlink-/loop-safe via a canonical visited
    /// set, depth-limited by `FOLDER_SCAN_DEPTH`. Each entry carries its
    /// relative subfolder in [`FileBrowserEntry::folder`] for the grid path
    /// badge. The RAW-only grid decision is unchanged — only aggregation.
    pub fn list_directory(&mut self) {
        self.request_scan(true);
    }

    /// Flat single-folder listing behind [`Self::set_directory`].
    pub fn list_directory_flat(&mut self) {
        self.request_scan(false);
    }

    /// R2-MODSWITCH-1 F8: request a folder scan. Production runs it on the
    /// worker thread ([`Self::begin_scan`], drained by [`Self::poll_scan`]) so
    /// the UI thread never blocks. Headless tests have no event loop, so under
    /// `cfg(test)` the same scan engine runs synchronously
    /// ([`Self::scan_directory_blocking`]) — a test seam, not a production
    /// fallback. The async worker path itself is covered by the dedicated
    /// `begin_scan`/`poll_scan` tests (`tests/library_scan_async.rs`).
    fn request_scan(&mut self, recursive: bool) {
        #[cfg(test)]
        self.scan_directory_blocking(recursive);
        #[cfg(not(test))]
        {
            let _ = self.begin_scan(recursive);
        }
    }

    pub(crate) fn apply_listing(
        &mut self,
        directory: std::path::PathBuf,
        mut entries: Vec<FileBrowserEntry>,
        generation: u64,
    ) {
        debug!("listing directory: {}", directory.display());
        let pending_matches = self.pending_directory_open.as_ref().is_some_and(|pending| {
            pending.directory == directory.to_string_lossy()
                && pending.scan_generation == Some(generation)
        });
        let pending_active_path = pending_matches
            .then(|| self.pending_directory_open.take())
            .flatten()
            .map(|pending| pending.active_path);
        if !pending_matches
            && self
                .pending_directory_open
                .as_ref()
                .is_some_and(|pending| pending.directory == directory.to_string_lossy())
        {
            self.pending_directory_open = None;
        }
        // REVIEW-GUI-THUMB-1: drop cached thumbnails of a previous folder so
        // they neither resurface nor accumulate unboundedly across a session.
        self.thumbnails
            .ensure_directory(&directory.to_string_lossy());
        // R2-MODSWITCH-1 F7: same for the metadata-only preview index.
        self.thumbnail_cache
            .ensure_directory(&directory.to_string_lossy());
        // PREVIEW-CACHE-FEATURE: a *directory change* invalidates the neighbor
        // cache state (RAM LRU, in-flight, failures) — stale entries of another
        // folder must neither resurface nor ever be shown. Relisting the same
        // directory during navigation keeps the warm LRU so a change-of-active
        // is served as an instant cache hit (A1).
        if let Some(ctrl) = self.preview_ctrl.as_mut() {
            ctrl.ensure_directory(self.directory.trim());
        }
        match std::fs::read_dir(&directory) {
            Ok(_) => {
                // LRPAR-G09-SORT-09: restore the folder's persisted sort mode +
                // custom order (loud on a corrupt file; defaults Name/empty).
                let sort_error = self.load_library_sort_for(&directory);
                crate::library_sort::sort_entries_in(
                    &directory,
                    &mut entries,
                    self.library_sort,
                    &self.library_sort_order,
                );
                // GUI-STARTUP-SELECTION-1: remember the grid position of a
                // single selection so a rescan that prunes it (e.g. the
                // selected file was deleted on disk) can fall back to its
                // successor instead of going empty while images remain.
                let removed_index = if self.filmstrip_selection.len() == 1 {
                    let selected = self
                        .filmstrip_selection
                        .iter()
                        .next()
                        .expect("single selection has one element");
                    self.entries
                        .iter()
                        .map(|e| e.path.display().to_string())
                        .position(|path| &path == selected)
                } else {
                    None
                };
                self.entries = entries;
                self.status = Str::ImagesInDirectory.format_arg(&self.entries.len().to_string());
                // LRPAR-G09-SORT-09: a corrupt sort-order file is surfaced
                // visibly here (it was logged loudly in `load_library_sort_for`)
                // instead of silently falling back to `Name`.
                if let Some(message) = sort_error {
                    self.status = message;
                }
                self.stabilize_selection(removed_index);
                // PERF-GUI-6: when no specific file was requested (e.g. the user
                // picked a directory, not a single image) and nothing is loaded
                // yet, auto-load the first grid entry so the Develop module
                // shows an image immediately — no manual click required.
                //
                // GUI-STARTUP-SELECTION-1 (F-100 Startverhalten): this covers
                // ALL supported formats (not just RAW — `entries` only ever
                // holds `is_supported_image` paths) and selects exactly like a
                // plain click (single selection + anchor) so selection and the
                // loading path can never desync. Decode failures surface loudly
                // through `finish_decode`/`show_error` — never a silent
                // fallback.
                //
                // Robustness guards:
                // * `!self.auto_load_attempted` — run the auto-load at most once
                //   per session so rescanning the directory never restarts a
                //   decode that is already in flight.
                // * `self.decode_rx.is_none()` — a decode is already pending
                //   (async), so we must not start a second one; `original` stays
                //   `None` until the in-flight decode's `finish_decode` runs.
                // If no entry exists yet we deliberately leave
                // `auto_load_attempted` unset so a later, now-populated scan can
                // still auto-load.
                if !self.auto_load_attempted
                    && self.path.is_empty()
                    && self.original.is_none()
                    && self.decode_rx.is_none()
                    && !self.entries.is_empty()
                {
                    let first = self.entries[0].path.display().to_string();
                    debug!("auto-loading first entry after list_directory: {}", first);
                    self.filmstrip_selection = BTreeSet::from([first.clone()]);
                    self.filmstrip_anchor = Some(first.clone());
                    self.begin_load_path(first);
                    self.auto_load_attempted = true;
                }
                if let Some(active_path) = pending_active_path {
                    self.schedule_neighbor_previews(&active_path);
                }
            }
            Err(error) => {
                self.entries.clear();
                self.status = Str::DirectoryNotReadable.format_arg(&error.to_string());
            }
        }
    }

    /// GUI-STARTUP-SELECTION-1 (F-100 Startverhalten): keep the selection
    /// non-empty while images exist. Prunes paths that no longer list
    /// (deleted/moved on disk), then — only when the prune emptied the
    /// selection — re-selects: the still-listed loaded image first (path vs.
    /// selection consistency, without triggering a resync decode), otherwise
    /// the successor at the removed grid position (clamped to the new last
    /// entry), otherwise the first grid entry. Clears everything only when no
    /// entries remain. Never starts a decode itself; loading stays with the
    /// auto-load in [`Self::apply_listing`] and the explicit
    /// [`Self::open_file`] path.
    fn stabilize_selection(&mut self, removed_index: Option<usize>) {
        let live: BTreeSet<String> = self
            .entries
            .iter()
            .map(|entry| entry.path.display().to_string())
            .collect();
        self.filmstrip_selection.retain(|path| live.contains(path));
        if self
            .filmstrip_anchor
            .as_ref()
            .is_some_and(|anchor| !live.contains(anchor))
        {
            self.filmstrip_anchor = None;
        }
        if self.entries.is_empty() {
            self.filmstrip_selection.clear();
            self.filmstrip_anchor = None;
            return;
        }
        if !self.filmstrip_selection.is_empty() {
            return;
        }
        if !self.path.is_empty() && live.contains(&self.path) {
            self.filmstrip_anchor = Some(self.path.clone());
            self.filmstrip_selection.insert(self.path.clone());
            return;
        }
        let index = removed_index.unwrap_or(0).min(self.entries.len() - 1);
        let target = self.entries[index].path.display().to_string();
        self.filmstrip_anchor = Some(target.clone());
        self.filmstrip_selection.insert(target);
    }

    /// Re-scan a single file into `self.entries` (in place, order-preserving).
    /// Used after a save so the browser reflects the new sidecar state
    /// without a full directory rescan — `list_directory` re-reads and
    /// re-hashes *every* source file via `source_status`, which stalls the UI
    /// on folders with large RAWs on every slider-commit save (GUI-VIEW-2,
    /// N6 Develop→Library/save stall class).
    pub(crate) fn refresh_entry(&mut self, path: &Path) {
        let Some(mut scanned) = library_scan::scan_entry(path) else {
            return;
        };
        // VIEW-2 single-file refresh: keep the subfolder badge consistent
        // with a full listing (no rescan here — that is the point).
        scanned.folder = folder_badge(&PathBuf::from(self.directory.trim()), path);
        if let Some(slot) = self.entries.iter_mut().find(|e| e.path == scanned.path) {
            *slot = scanned;
        } else {
            self.entries.push(scanned);
            // LRPAR-G09-SORT-09: keep the active sort order for a new entry.
            self.sort_entries_now();
        }
    }

    /// GUI-FILMSTRIP-SYNC-1: pure filmstrip click semantics (Lightroom-like),
    /// headless-testable without an [`egui::Context`].
    ///
    /// * plain click → the selection is exactly `clicked`, anchor becomes `clicked`;
    /// * `toggle` (Cmd/Ctrl-Click) → `clicked` is added or removed, anchor becomes `clicked`;
    /// * `range` (Shift-Click) → the inclusive span from the anchor (or `clicked`
    ///   when there is no usable anchor) to `clicked` over `order` is added to
    ///   the selection; the anchor is kept so repeated Shift-Clicks extend from
    ///   the same origin.
    ///
    /// Clicking a path that is not in `order` leaves selection and anchor unchanged.
    pub fn apply_filmstrip_click(
        order: &[String],
        selection: &BTreeSet<String>,
        anchor: Option<&str>,
        clicked: &str,
        toggle: bool,
        range: bool,
    ) -> (BTreeSet<String>, Option<String>) {
        let end = order.iter().position(|path| path == clicked);
        let Some(end) = end else {
            return (selection.clone(), anchor.map(str::to_string));
        };
        if range {
            let start = anchor
                .and_then(|known| order.iter().position(|path| path == known))
                .unwrap_or(end);
            let (low, high) = if start <= end {
                (start, end)
            } else {
                (end, start)
            };
            let mut next = selection.clone();
            for path in &order[low..=high] {
                next.insert(path.clone());
            }
            return (next, anchor.map(str::to_string));
        }
        if toggle {
            let mut next = selection.clone();
            if !next.remove(clicked) {
                next.insert(clicked.to_string());
            }
            return (next, Some(clicked.to_string()));
        }
        (
            BTreeSet::from([clicked.to_string()]),
            Some(clicked.to_string()),
        )
    }

    /// Currently selected filmstrip paths, sorted.
    pub fn filmstrip_selection(&self) -> Vec<String> {
        self.filmstrip_selection.iter().cloned().collect()
    }

    /// GUI-FILMSTRIP-SYNC-1: update the multi-selection for a click WITHOUT
    /// opening the image (Library grid single-click selects; opening stays
    /// on double-click). Selection bookkeeping is shared with
    /// [`Self::handle_filmstrip_click`] so every entry point syncs
    /// identically (GUI-FILMSTRIP-DUP-1).
    pub fn select_filmstrip_path(&mut self, path: String, toggle: bool, range: bool) {
        let order = self.filmstrip_order();
        let (next, anchor) = Self::apply_filmstrip_click(
            &order,
            &self.filmstrip_selection,
            self.filmstrip_anchor.as_deref(),
            &path,
            toggle,
            range,
        );
        // LRPAR-G15-STACK-15: a stack selects (and deselects) as one unit.
        let next = self.apply_stack_selection(next, &path, toggle);
        self.filmstrip_selection = next;
        self.filmstrip_anchor = anchor;
        trace!(
            "GUI interaction: filmstrip select {} (toggle={toggle}, range={range}, selected={})",
            path,
            self.filmstrip_selection.len()
        );
    }

    /// GUI-FILMSTRIP-SYNC-1: update the multi-selection for a filmstrip click
    /// and open the clicked image. Selection bookkeeping is synchronous (it
    /// never waits for the background decode started by [`Self::open_file`]).
    pub fn handle_filmstrip_click(&mut self, path: String, toggle: bool, range: bool) {
        self.select_filmstrip_path(path.clone(), toggle, range);
        trace!(
            "GUI interaction: filmstrip click {} (selected={})",
            path,
            self.filmstrip_selection.len()
        );
        self.open_file(path);
    }
}
