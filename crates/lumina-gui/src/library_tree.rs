//! GUI-REFACTOR-W2-20 S2.3: the Library folder tree and keyword chips,
//! extracted verbatim from `lib.rs`.
//!
//! [`LuminaApp::draw_folder_tree`] renders the Lightroom-like lazily expanded
//! directory hierarchy with a depth-limited RAW count per node,
//! [`LuminaApp::draw_folder_node`] is one node and
//! [`LuminaApp::draw_keyword_chips`] the keyword chip row shared by the metadata
//! panel. No behaviour changes: the selection/rescan flow and the `trace!`s are
//! byte-identical. The externally called entry points are `pub(crate)`.

use super::*;
use log::trace;

impl LuminaApp {
    /// Lightroom-like Library folder tree (left panel): directory hierarchy
    /// rooted at `$HOME` (or two ancestors above the current directory when it
    /// lives outside the home tree), lazily expanded via `read_dir`, showing a
    /// depth-limited RAW count per node. Clicking a node selects the directory.
    pub(crate) fn draw_folder_tree(&mut self, ui: &mut egui::Ui) {
        ui.heading(Str::Folders.t());
        // Direct path entry stays available (replaces the old text browser's
        // address row) plus a manual rescan.
        // Button-first in a plain row so Open stays inside the panel
        // (GUI-VISION-1). A direction-changing `with_layout(right_to_left)`
        // must NOT be used here: the sub-layout claims all remaining
        // vertical panel space and pushes the folder tree below the fold
        // (B1 — the tree ScrollArea below then starts off-panel and no test
        // scroll can recover it). A plain horizontal row wraps its height to
        // the content.
        let mut open_clicked = false;
        ui.horizontal(|ui| {
            open_clicked = ui.button(Str::Open.t()).clicked();
            ui.text_edit_singleline(&mut self.directory);
        });
        if open_clicked {
            let target = self.directory.clone();
            self.set_directory(target);
        }
        if ui.button(Str::Refresh.t()).clicked() {
            self.list_directory();
        }
        ui.separator();
        let root = library_root(&self.directory);
        // The root itself is always visible/expanded.
        self.open_folders.insert(root.display().to_string());
        let mut select_target: Option<String> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            self.draw_folder_node(ui, &root, &root, 0, &mut select_target);
        });
        if let Some(path) = select_target {
            trace!("GUI interaction: folder select {}", path);
            self.set_directory(path);
        }
    }

    /// One folder-tree node: disclosure arrow + label with RAW count, then the
    /// lazily cached children when expanded.
    fn draw_folder_node(
        &mut self,
        ui: &mut egui::Ui,
        root: &Path,
        path: &Path,
        depth: usize,
        select_target: &mut Option<String>,
    ) {
        let path_str = path.display().to_string();
        // Depth-limited RAW count, computed once per folder and cached.
        if !self.folder_raw_counts.contains_key(&path_str) {
            let count = count_raw_files(path, FOLDER_SCAN_DEPTH);
            self.folder_raw_counts.insert(path_str.clone(), count);
        }
        let raw_count = self.folder_raw_counts[&path_str];
        let open = self.open_folders.contains(&path_str);
        ui.horizontal(|ui| {
            ui.add_space((depth * 14) as f32);
            // Disclosure toggle.
            let (arrow_rect, arrow_resp) =
                ui.allocate_exact_size(egui::vec2(12.0, 16.0), egui::Sense::click());
            ui.painter().text(
                arrow_rect.center(),
                egui::Align2::CENTER_CENTER,
                if open { "▾" } else { "▸" },
                egui::FontId::default(),
                ui.visuals().text_color(),
            );
            if arrow_resp.clicked() {
                if open {
                    self.open_folders.remove(&path_str);
                } else {
                    self.open_folders.insert(path_str.clone());
                }
            }
            let label = format!("{} ({})", folder_label(root, path), raw_count);
            if ui
                .selectable_label(self.directory == path_str, label)
                .clicked()
            {
                *select_target = Some(path_str.clone());
            }
        });
        if !open {
            return;
        }
        // Lazy children cache: fill via read_dir on first expansion.
        let children = match self.folder_children.get(&path_str) {
            Some(children) => children.clone(),
            None => {
                let children: Vec<String> = subdirectories(path)
                    .iter()
                    .map(|child| child.display().to_string())
                    .collect();
                self.folder_children
                    .insert(path_str.clone(), children.clone());
                children
            }
        };
        for child in children {
            self.draw_folder_node(ui, root, Path::new(&child), depth + 1, select_target);
        }
    }

    /// G-15 META-MVP (Slice 3) Library metadata drawer: keywords,
    /// collections, smart collections and the batch bar. Rendered inside the
    /// `\` drawer only, so the default grid (and its kittest goldens) stays
    /// pixel-identical when the drawer is closed. Every mutation goes
    /// through the `BatchOp` + `save_sidecar` paths (Sidecar-first, loud
    /// errors via `show_error`, `info!` in the mutators); display state
    /// (inputs, active filter, catalog) is session-only.
    /// Keyword chips of the loaded image (G-15 META-MVP, Slice 3 component,
    /// reused by the `\` drawer and the LRPAR-G15-IPTC-S8 right-column
    /// Metadata panel): add/remove run through `add_keyword`/`remove_keyword`
    /// (Slice-1 validation + CAS save, loud errors, `info!` logs).
    pub(crate) fn draw_keyword_chips(&mut self, ui: &mut egui::Ui) {
        let keywords = self.keywords();
        let mut remove: Option<String> = None;
        for keyword in &keywords {
            ui.horizontal(|ui| {
                ui.label(keyword);
                if ui.button("✕").clicked() {
                    remove = Some(keyword.clone());
                }
            });
        }
        if let Some(keyword) = remove {
            if let Err(error) = self.remove_keyword(&keyword) {
                self.show_error(error);
            }
        }
        ui.horizontal(|ui| {
            let mut input = self.keyword_input.clone();
            if ui
                .add(egui::TextEdit::singleline(&mut input).hint_text(Str::KeywordInputHint.t()))
                .changed()
            {
                self.keyword_input = input.clone();
            }
            if ui.button(Str::AddKeyword.t()).clicked() {
                let value = input.trim().to_string();
                if !value.is_empty() {
                    match self.add_keyword(&value) {
                        Ok(_) => self.keyword_input.clear(),
                        Err(error) => self.show_error(error),
                    }
                }
            }
        });
    }
}
