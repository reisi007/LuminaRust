//! GUI-LIB-SPLIT-65 Slice 14: Dateisystem- und Dateiformat-Erkennungshelfer.
//!
//! Pure path/format predicates and directory readers shared by the Library
//! scan, the file browser, the filmstrip and the export panel. Extracted
//! verbatim from `lib.rs` (file-size ratchet) so the crate root stays within
//! its committed baseline.

use super::*;
use std::path::{Path, PathBuf};

/// True when `path` lies inside a `.lumina/` cache directory (exact
/// directory name `.lumina`, any level). Pure lexical path logic, no I/O:
/// `.lumina/` holds only deletable cache and settings (F-100 Library,
/// GUI-LIBRARY-LUMINA-DIR-1), so the Library scan must never surface files
/// below it as images — flat or recursive, on every level.
pub(crate) fn is_lumina_cache_path(path: &Path) -> bool {
    path.components().any(
        |component| matches!(component, std::path::Component::Normal(name) if name == ".lumina"),
    )
}

pub(crate) fn is_supported_image(path: &Path) -> bool {
    // The file browser lists all editable formats; the filmstrip display applies
    // its own RAW-only filter (see `draw_filmstrip`). v1: PNG/JPEG/WebP plus the
    // RAW extensions.
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("png" | "jpg" | "jpeg" | "webp") => true,
        _ => is_raw_name(&path.display().to_string()),
    }
}

/// Human-readable label for an [`ImageFileFormat`] used by the Export panel.
pub(crate) fn format_label(format: ImageFileFormat) -> &'static str {
    match format {
        ImageFileFormat::Png => "PNG",
        ImageFileFormat::Jpeg => "JPEG",
        ImageFileFormat::WebP => "WebP",
    }
}

pub(crate) fn is_raw_name(name: &str) -> bool {
    // Single source of truth: delegate to `lumina_raw::is_raw_extension`
    // (RAW_EXTENSIONS lives there). R2-CLI-01 already consolidated the same
    // list for the CLI path; keeping one canonical extension list prevents the
    // two crates from drifting and re-introducing the silent 9-of-18 skip the
    // review flagged. Matching stays ASCII-case-insensitive.
    std::path::Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(lumina_raw::is_raw_extension)
}

/// How many directory levels the RAW-count scan descends at most. Keeps the
/// per-folder count cheap even under large trees.
pub(crate) const FOLDER_SCAN_DEPTH: usize = 3;

/// Immediate subdirectories of `dir`, sorted; empty when unreadable so a
/// permission error degrades to "no children" instead of a broken node.
pub(crate) fn subdirectories(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    dirs
}
