//! LRPAR-G09-SORT-09: the portable, atomic per-folder sort-order file.
//!
//! The custom Library order of a folder lives in the folder's deletable cache
//! directory as `.lumina/lumina-sort.json` (`format = "lumina-folder-sort"`,
//! `version = 1`). `.lumina/` is the established Lumina cache folder (previews,
//! settings, index) and is already git-ignored, so the sort order is ignored
//! too. It is portable: `order` is a list of **relative** names w.r.t. the
//! listed folder (`file.ext` or `subdir/file.ext`, `/`-separated) — never
//! absolute paths, never `.`/`..`, never array positions. Moving the whole
//! folder bundle keeps it valid.
//!
//! Writes are atomic (`NamedTempFile` + `persist`, the same contract as sidecar
//! writes) and only happen on an explicit sort change or drag-drop reorder.
//! A corrupt/higher-version/unsafe file is a loud error at the caller — this
//! module never silently normalizes.
//!
//! A pre-2026-09-20 file written directly next to the images is migrated once,
//! on the first listing that only finds the legacy file: its content is read,
//! validated, re-written to `.lumina/lumina-sort.json` and the legacy file is
//! removed (loud `info!`). Nothing is silently dropped and no second copy is
//! silently kept behind.

use std::path::{Component, Path, PathBuf};

/// Folder (inside the listed directory) holding Lumina's deletable cache; it is
/// already git-ignored, so the sort-order file inside it is too.
pub(crate) const SORT_ORDER_DIR: &str = ".lumina";
/// File name of the folder sort-order file (inside [`.lumina/`]).
pub const SORT_ORDER_FILE: &str = "lumina-sort.json";
/// `format` marker of the folder sort-order file.
pub const SORT_ORDER_FORMAT: &str = "lumina-folder-sort";
/// Schema version of the folder sort-order file.
pub const SORT_ORDER_VERSION: u32 = 1;
/// Defensive upper bound on the custom-order length (portable file, untrusted
/// input): a larger list is rejected loudly instead of allocating unbounded.
const SORT_ORDER_MAX_ENTRIES: usize = 100_000;
/// Defensive upper bound on a single relative-name entry.
const SORT_ORDER_MAX_NAME_BYTES: usize = 512;

/// On-disk document of the folder sort order. `mode` is the last chosen sort
/// mode (one of `LibrarySort::name`) and `order` the custom arrangement.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FolderSortOrder {
    pub format: String,
    pub version: u32,
    pub mode: String,
    #[serde(default)]
    pub order: Vec<String>,
}

/// Path of the folder sort-order file for a listed directory:
/// `<directory>/.lumina/lumina-sort.json`.
pub(crate) fn sort_order_path_for(directory: &Path) -> PathBuf {
    directory.join(SORT_ORDER_DIR).join(SORT_ORDER_FILE)
}

/// Legacy (pre-2026-09-20) location directly next to the images; only read to
/// migrate it once into [`sort_order_path_for`].
pub(crate) fn legacy_sort_order_path_for(directory: &Path) -> PathBuf {
    directory.join(SORT_ORDER_FILE)
}

/// True when `entry` is a portable relative name: non-empty, bounded, not
/// absolute, no `.`/`..`/root/prefix components and no Windows separator.
pub(crate) fn order_entry_is_portable(entry: &str) -> bool {
    if entry.is_empty() || entry.len() > SORT_ORDER_MAX_NAME_BYTES {
        return false;
    }
    if entry.contains('\\') {
        return false;
    }
    let path = Path::new(entry);
    if path.is_absolute() {
        return false;
    }
    path.components()
        .all(|component| matches!(component, Component::Normal(_)))
}

impl FolderSortOrder {
    /// Builds a validated document; an unsafe custom entry is rejected loudly.
    pub fn new(mode: &str, order: Vec<String>) -> Result<Self, String> {
        let document = Self {
            format: SORT_ORDER_FORMAT.to_string(),
            version: SORT_ORDER_VERSION,
            mode: mode.to_string(),
            order,
        };
        document.validate()?;
        Ok(document)
    }

    /// Loud validation: `format`, `version` and every `order` entry. Never a
    /// silent normalization (unknown fields are ignored by serde, malformed
    /// values are refused).
    pub fn validate(&self) -> Result<(), String> {
        if self.format != SORT_ORDER_FORMAT {
            return Err(format!(
                "unexpected sort-order format `{}` (expected `{SORT_ORDER_FORMAT}`)",
                self.format
            ));
        }
        if self.version != SORT_ORDER_VERSION {
            return Err(format!(
                "unsupported sort-order version {} (this build supports {SORT_ORDER_VERSION})",
                self.version
            ));
        }
        if self.order.len() > SORT_ORDER_MAX_ENTRIES {
            return Err(format!(
                "sort order has {} entries (limit {SORT_ORDER_MAX_ENTRIES})",
                self.order.len()
            ));
        }
        for entry in &self.order {
            if !order_entry_is_portable(entry) {
                return Err(format!(
                    "sort order contains a non-portable entry `{entry}` \
                     (relative name, no absolute path, no `.`/`..`)"
                ));
            }
        }
        Ok(())
    }
}

/// Reads and validates a sort-order file at `path` (loud `Err` on invalid
/// content, never a silent normalization).
fn read_sort_order(path: &Path) -> Result<FolderSortOrder, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("cannot read `{}`: {error}", path.display()))?;
    let document: FolderSortOrder = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid sort order in `{}`: {error}", path.display()))?;
    document
        .validate()
        .map_err(|error| format!("invalid sort order in `{}`: {error}", path.display()))?;
    Ok(document)
}

/// Ensures `<directory>/.lumina/` exists, then writes `bytes` atomically to
/// `<directory>/.lumina/lumina-sort.json`.
fn write_sort_order_atomically(directory: &Path, bytes: &[u8]) -> Result<(), String> {
    let cache_dir = directory.join(SORT_ORDER_DIR);
    std::fs::create_dir_all(&cache_dir)
        .map_err(|error| format!("cannot create `{}`: {error}", cache_dir.display()))?;
    let path = cache_dir.join(SORT_ORDER_FILE);
    lumina_sidecar::write_atomically(&path, bytes)
        .map_err(|error| format!("cannot write `{}`: {error}", path.display()))
}

/// Loads the folder sort order, if present. The active file is
/// `.lumina/lumina-sort.json`; a legacy file next to the images is migrated
/// once (see [`migrate_legacy_sort_order`]). Missing file → `Ok(None)`;
/// unreadable/invalid content → a loud `Err` (the caller reports it visibly and
/// falls back to the default mode, never a silent ignore).
pub(crate) fn load_folder_sort_order(directory: &Path) -> Result<Option<FolderSortOrder>, String> {
    let path = sort_order_path_for(directory);
    if path.is_file() {
        let legacy = legacy_sort_order_path_for(directory);
        if legacy.is_file() {
            log::warn!(
                "library sort: ignoring legacy `{}` (the active file is `{}`)",
                legacy.display(),
                path.display()
            );
        }
        return read_sort_order(&path).map(Some);
    }
    let legacy = legacy_sort_order_path_for(directory);
    if legacy.is_file() {
        return migrate_legacy_sort_order(directory, &legacy, &path);
    }
    Ok(None)
}

/// One-time migration of the legacy sort file next to the images into the
/// `.lumina/` cache folder: read + validate, write to the new path, then remove
/// the legacy file (loud `info!`). A failed write keeps the legacy file so
/// nothing is lost; the next listing retries.
fn migrate_legacy_sort_order(
    directory: &Path,
    legacy: &Path,
    path: &Path,
) -> Result<Option<FolderSortOrder>, String> {
    let document = read_sort_order(legacy)?;
    let bytes = serde_json::to_vec_pretty(&document)
        .map_err(|error| format!("cannot encode sort order: {error}"))?;
    if let Err(error) = write_sort_order_atomically(directory, &bytes) {
        log::error!(
            "library sort: cannot migrate legacy `{}` to `{}`: {error}",
            legacy.display(),
            path.display()
        );
        // The value is still valid and applied; the legacy file stays for a
        // later retry — nothing is dropped silently.
        return Ok(Some(document));
    }
    match std::fs::remove_file(legacy) {
        Ok(()) => log::info!(
            "library sort: migrated legacy `{}` to `{}`",
            legacy.display(),
            path.display()
        ),
        Err(error) => log::warn!(
            "library sort: migrated `{}` but cannot remove legacy `{}`: {error}",
            path.display(),
            legacy.display()
        ),
    }
    Ok(Some(document))
}

/// Writes the folder sort order atomically to `.lumina/lumina-sort.json`
/// (creating `.lumina/` when needed).
pub(crate) fn save_folder_sort_order(
    directory: &Path,
    mode: &str,
    order: &[String],
) -> Result<(), String> {
    let document = FolderSortOrder::new(mode, order.to_vec())?;
    let bytes = serde_json::to_vec_pretty(&document)
        .map_err(|error| format!("cannot encode sort order: {error}"))?;
    write_sort_order_atomically(directory, &bytes)
}
