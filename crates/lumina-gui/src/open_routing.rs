//! File-open routing policies shared by dialogs, filmstrip actions, and native drops.
//!
//! Ordinary opens navigate immediately. Native path drops reuse the same
//! asynchronous decode/sidecar lineage, but defer target-directory adoption
//! until that decode succeeds so a failed drop cannot strand navigation away
//! from the still-loaded source.

use std::path::Path;

use super::{DirectoryOpenPolicy, LuminaApp};

impl LuminaApp {
    /// Open a file through the ordinary immediate-navigation policy.
    pub fn open_file(&mut self, path: impl Into<String>) {
        let path = path.into();
        log::trace!("GUI interaction: open_file {}", path);
        // REVIEW-GUI-PATHDESYNC-1: `self.path` is NOT committed here. Adopting
        // B before `finish_decode` would let Save/Export/mask writes pair A's
        // still-loaded state with B's path, and a failed decode would leave that
        // phantom target behind. The path is committed only on decode success.
        // GUI-SIDECAR-READ-1: flush an armed commit to A before switching, or
        // `apply_decoded_frame` would discard the pending edit.
        self.flush_pending_edit();
        self.prepare_open_directory(&path);
        self.begin_load_path(path);
    }

    /// Native path-drop variant of [`Self::open_file`].
    pub(super) fn open_file_deferred(&mut self, path: impl Into<String>) {
        let path = path.into();
        log::trace!("GUI interaction: deferred open_file {}", path);
        // Editing still flushes to A, but directory/listing/selection remain
        // untouched until B has decoded successfully.
        self.flush_pending_edit();
        self.begin_load_path_deferred(path);
    }

    /// Adopt the target directory only for a successful deferred decode.
    pub(super) fn adopt_directory_after_decode(&mut self, path: &str, policy: DirectoryOpenPolicy) {
        if policy != DirectoryOpenPolicy::Deferred {
            return;
        }
        self.prepare_open_directory(path);
        // A native drop is a single explicit target, not an extension of a
        // multi-selection. Keep filmstrip state aligned with the loaded path.
        self.filmstrip_selection.clear();
        self.filmstrip_selection.insert(path.to_string());
        self.filmstrip_anchor = Some(path.to_string());
    }

    /// Populate the file browser with the directory containing `path`.
    fn prepare_open_directory(&mut self, path: &str) {
        // Populate the file browser with the directory containing the opened
        // file. GUI-VIEW-2: rescan only when actually navigating (new directory
        // or no entries yet). A same-folder switch (filmstrip clicks) reuses the
        // live entries — our own saves keep them fresh via `refresh_entry` —
        // instead of re-reading + re-hashing every source (the N6 stall:
        // ~224 ms per switch with hashed sidecars). External folder changes
        // still surface via Open/Refresh/`set_directory` rescans.
        if let Some(parent) = Path::new(path).parent() {
            let directory = parent.display().to_string();
            // LRPAR-G01-BASIC: the reset-sliders flag is folder-inherited —
            // refresh it for the target folder on every open (no-op without a
            // settings file).
            self.refresh_reset_sliders_flag(parent);
            if directory != self.directory || self.entries.is_empty() {
                self.directory = directory;
                // GUI-STARTUP-SELECTION-1: an explicit open discharges startup
                // auto-load, so its scan can neither start a second decode nor
                // select a different first entry. Seeding also preserves an
                // existing multi-selection; ordinary filmstrip callers have
                // already selected their target before reaching this method.
                if !self.filmstrip_selection.contains(path) {
                    self.filmstrip_selection.insert(path.to_string());
                    self.filmstrip_anchor = Some(path.to_string());
                }
                self.auto_load_attempted = true;
                self.list_directory();
            } else {
                self.directory = directory;
            }
        }
    }
}
