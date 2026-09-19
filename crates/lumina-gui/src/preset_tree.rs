//! UX-LOOK-HISTORY-18 (Release 1.0): the presets **group tree**.
//!
//! Presets stay single files in the user presets directory. The tree groups
//! them by their **relative sub-folder** (never an absolute path): a file in
//! `<presets>/Landscape/Golden Hour/Warm.lumina-preset.json` lives under the
//! `Landscape → Golden Hour` group. The relative folder is the only grouping
//! key; the group label shown in the UI is the folder name, so a persistent
//! recipe or preset never carries a machine path.
//!
//! Scanning is recursive with a depth limit ([`MAX_PRESET_DIR_DEPTH`]) so a
//! symlink cycle can never recurse forever. Every readable `.lumina-preset.json`
//! file is loaded and validated by the existing [`load_preset_file`] path —
//! broken files stay visible as failed leaves with their error text. An
//! unreadable sub-directory is reported as a failed node instead of being
//! skipped silently; an unreadable/missing root keeps the established
//! "missing directory = no presets" behavior.
//!
//! [`load_preset_file`]: crate::presets::load_preset_file

use super::*;
use crate::presets::{load_preset_file, PresetEntry, PRESET_FILE_SUFFIX};
use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

/// Hard recursion limit: a folder deeper than this never contributes entries
/// (the walk stops instead of following a possible symlink cycle).
pub(crate) const MAX_PRESET_DIR_DEPTH: usize = 16;

/// Recursively lists every preset file under `dir`, sorted by full path.
/// Missing root = empty (first run); an unreadable root or sub-directory is a
/// loud failed entry, never a silent drop.
pub(crate) fn scan_presets_recursive(dir: &Path) -> Vec<PresetEntry> {
    let mut files = Vec::new();
    let mut failures = Vec::new();
    match collect_files(dir, 0, &mut files, &mut failures) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            return vec![PresetEntry::Failed {
                path: dir.to_path_buf(),
                error: format!("presets directory unreadable: {error}"),
            }]
        }
    }
    files.sort();
    let mut entries: Vec<PresetEntry> = files
        .into_iter()
        .map(|path| match load_preset_file(&path) {
            Ok(preset) => PresetEntry::Available {
                path,
                preset: Box::new(preset),
            },
            Err(error) => PresetEntry::Failed {
                path,
                error: error.to_string(),
            },
        })
        .collect();
    entries.append(&mut failures);
    entries.sort_by(|a, b| entry_path(a).cmp(entry_path(b)));
    entries
}

fn collect_files(
    dir: &Path,
    depth: usize,
    files: &mut Vec<std::path::PathBuf>,
    failures: &mut Vec<PresetEntry>,
) -> std::io::Result<()> {
    if depth >= MAX_PRESET_DIR_DEPTH {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            if let Err(error) = collect_files(&path, depth + 1, files, failures) {
                failures.push(PresetEntry::Failed {
                    path,
                    error: format!("presets directory unreadable: {error}"),
                });
            }
        } else if file_type.is_file()
            && path
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|name| name.ends_with(PRESET_FILE_SUFFIX))
        {
            files.push(path);
        }
    }
    Ok(())
}

fn entry_path(entry: &PresetEntry) -> &Path {
    match entry {
        PresetEntry::Available { path, .. } | PresetEntry::Failed { path, .. } => path,
    }
}

#[derive(Default)]
struct GroupNode {
    dirs: BTreeMap<String, GroupNode>,
    entries: Vec<usize>,
}

/// Renders the group tree of `entries` (paths relative to `dir`) and returns
/// the index of a clicked available preset. Group headers are collapsible and
/// open by default; the label is the folder name only (no absolute path).
pub(crate) fn draw_preset_tree(
    ui: &mut egui::Ui,
    dir: &Path,
    entries: &[PresetEntry],
) -> Option<usize> {
    let root = build_tree(dir, entries);
    let mut clicked = None;
    for &index in &root.entries {
        paint_entry(ui, index, &entries[index], &mut clicked);
    }
    for (name, node) in &root.dirs {
        let salt = format!("preset_group::{name}");
        egui::CollapsingHeader::new(name)
            .id_salt(salt)
            .default_open(true)
            .show(ui, |ui| draw_node(ui, node, name, entries, &mut clicked));
    }
    clicked
}

fn build_tree(dir: &Path, entries: &[PresetEntry]) -> GroupNode {
    let mut root = GroupNode::default();
    for (index, entry) in entries.iter().enumerate() {
        let mut node = &mut root;
        for component in relative_components(dir, entry_path(entry)) {
            node = node.dirs.entry(component).or_default();
        }
        node.entries.push(index);
    }
    root
}

fn draw_node(
    ui: &mut egui::Ui,
    node: &GroupNode,
    prefix: &str,
    entries: &[PresetEntry],
    clicked: &mut Option<usize>,
) {
    for &index in &node.entries {
        paint_entry(ui, index, &entries[index], clicked);
    }
    for (name, child) in &node.dirs {
        let child_prefix = format!("{prefix}/{name}");
        egui::CollapsingHeader::new(name)
            .id_salt(format!("preset_group::{child_prefix}"))
            .default_open(true)
            .show(ui, |ui| {
                draw_node(ui, child, &child_prefix, entries, clicked)
            });
    }
}

fn paint_entry(ui: &mut egui::Ui, index: usize, entry: &PresetEntry, clicked: &mut Option<usize>) {
    match entry {
        PresetEntry::Available { preset, .. } => {
            if ui.selectable_label(false, &preset.name).clicked() {
                *clicked = Some(index);
            }
        }
        PresetEntry::Failed { path, error } => {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            ui.colored_label(egui::Color32::LIGHT_RED, format!("{name}: {error}"));
        }
    }
}

/// Relative folder components of `path` under `dir` (empty = directly in
/// `dir`). Absolute paths never leak into a group label.
fn relative_components(dir: &Path, path: &Path) -> Vec<String> {
    let Ok(relative) = path.strip_prefix(dir) else {
        return Vec::new();
    };
    let Some(parent) = relative.parent() else {
        return Vec::new();
    };
    parent
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(name) => name.to_str().map(str::to_string),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn write_preset(dir: &Path, relative: &str, name: &str) {
        let path = dir.join(relative);
        fs::create_dir_all(&path).unwrap();
        let preset = lumina_sidecar::Preset {
            id: format!("preset-{name}"),
            name: name.into(),
            recipe: lumina_sidecar::EditRecipe::default(),
            extras: Default::default(),
        };
        crate::presets::save_preset_file(&path, &preset, true).unwrap();
    }

    #[test]
    fn scan_recurses_and_keeps_relative_order() {
        let directory = tempfile::tempdir().unwrap();
        write_preset(directory.path(), "", "Root");
        write_preset(directory.path(), "Landscape", "Wide");
        write_preset(directory.path(), "Landscape/Golden Hour", "Warm");
        let entries = scan_presets_recursive(directory.path());
        assert_eq!(entries.len(), 3);
        let groups: Vec<Vec<String>> = entries
            .iter()
            .map(|entry| relative_components(directory.path(), entry_path(entry)))
            .collect();
        assert_eq!(
            groups,
            vec![
                vec!["Landscape".to_string(), "Golden Hour".to_string()],
                vec!["Landscape".to_string()],
                Vec::new(),
            ],
            "paths sort before grouping"
        );
    }

    #[test]
    fn missing_directory_is_empty_and_not_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("does-not-exist");
        assert!(scan_presets_recursive(&missing).is_empty());
    }

    #[test]
    fn broken_file_in_subfolder_stays_visible() {
        let directory = tempfile::tempdir().unwrap();
        write_preset(directory.path(), "Group", "Good");
        fs::write(
            directory.path().join("Group/Broken.lumina-preset.json"),
            b"{ broken",
        )
        .unwrap();
        let entries = scan_presets_recursive(directory.path());
        assert_eq!(entries.len(), 2);
        let failed: Vec<&PathBuf> = entries
            .iter()
            .filter_map(|entry| match entry {
                PresetEntry::Failed { path, .. } => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(failed.len(), 1);
    }
}
