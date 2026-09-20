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

/// R4-LIB-1: per-folder tree info. `raw_count` is the depth-limited RAW count
/// shown in the node label; `has_images` is true when any **supported** image
/// (RAW or raster — the Library grid's set) exists in the folder or its
/// depth-limited descendants, so the tree hides folders that would list
/// nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FolderTreeInfo {
    pub(crate) raw_count: usize,
    pub(crate) has_images: bool,
}

/// Depth-limited walk behind [`FolderTreeInfo`]. `.lumina/` is never descended
/// (R4-LIB-1d); symlink-/loop-safe via a canonical visited set (same
/// convention as the recursive listing scan).
fn folder_tree_info(path: &Path) -> FolderTreeInfo {
    folder_tree_info_at_depth(path, FOLDER_SCAN_DEPTH)
}

/// [`folder_tree_info`] at an explicit depth (test seam for the depth-limit
/// assertions; production always uses `FOLDER_SCAN_DEPTH`).
pub(crate) fn folder_tree_info_at_depth(dir: &Path, remaining_depth: usize) -> FolderTreeInfo {
    let mut visited = std::collections::HashSet::new();
    folder_tree_info_inner(dir, remaining_depth, &mut visited)
}

fn folder_tree_info_inner(
    dir: &Path,
    remaining_depth: usize,
    visited: &mut std::collections::HashSet<PathBuf>,
) -> FolderTreeInfo {
    if remaining_depth == 0 {
        return FolderTreeInfo::default();
    }
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    if !visited.insert(canonical) {
        return FolderTreeInfo::default();
    }
    let mut info = FolderTreeInfo::default();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return info;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // R4-LIB-1(d): the deletable cache directory is not content.
            if path.file_name().and_then(|name| name.to_str()) == Some(".lumina") {
                continue;
            }
            let sub = folder_tree_info_inner(&path, remaining_depth - 1, visited);
            info.raw_count += sub.raw_count;
            info.has_images |= sub.has_images;
        } else if is_supported_image(&path) {
            info.has_images = true;
            if is_raw_name(&path.display().to_string()) {
                info.raw_count += 1;
            }
        }
    }
    info
}

/// R4-LIB-1(b): clickable breadcrumb segments of `directory` as `(label,
/// absolute target)`. The last segment is the current folder; earlier segments
/// navigate up. Pure lexical path logic (no I/O), unit-testable headless.
pub(crate) fn library_breadcrumb(directory: &str) -> Vec<(String, String)> {
    let path = PathBuf::from(directory.trim());
    let mut segments = Vec::new();
    let mut accumulated = PathBuf::new();
    for component in path.components() {
        accumulated.push(component.as_os_str());
        let label = match component {
            std::path::Component::RootDir => "/".to_string(),
            _ => component.as_os_str().to_string_lossy().into_owned(),
        };
        segments.push((label, accumulated.display().to_string()));
    }
    segments
}

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

    /// R4-LIB-1/R4-SWITCH-2: cached [`FolderTreeInfo`] for `path`. The first
    /// access performs the synchronous depth-limited walk (traced via
    /// `folder_scan_line`), later frames reuse the cache.
    fn folder_info_cached(&mut self, path: &Path) -> FolderTreeInfo {
        let key = path.display().to_string();
        if let Some(info) = self.folder_raw_counts.get(&key) {
            return *info;
        }
        let stopwatch = crate::timing::Stopwatch::now();
        let info = folder_tree_info(path);
        crate::timing::emit(|| {
            crate::timing::folder_scan_line(path, info.raw_count, stopwatch.elapsed_ms())
        });
        self.folder_raw_counts.insert(key, info);
        info
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
        // Depth-limited node info, computed once per folder and cached.
        // R4-SWITCH-2: this synchronous walk was the uninstrumented UI block
        // behind the first Library paint; time it so the trace names it.
        let info = self.folder_info_cached(path);
        let raw_count = info.raw_count;
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
        // R4-LIB-1(c/d): a child is kept only when its depth-limited subtree
        // actually carries a supported image (empty folders would list nothing)
        // and never when it is the deletable `.lumina/` cache directory.
        let children = match self.folder_children.get(&path_str) {
            Some(children) => children.clone(),
            None => {
                let mut children: Vec<String> = Vec::new();
                for child in subdirectories(path) {
                    // R4-LIB-1(d): the deletable cache directory is never a node.
                    if child.file_name().and_then(|name| name.to_str()) == Some(".lumina") {
                        continue;
                    }
                    if self.folder_info_cached(&child).has_images {
                        children.push(child.display().to_string());
                    }
                }
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
