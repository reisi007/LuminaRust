//! LRPAR-G09-SORT-09: the portable, atomic per-folder sort-order file.
//!
//! The custom Library order of a folder lives directly next to the images in
//! `lumina-sort.json` (`format = "lumina-folder-sort"`, `version = 1`). It is
//! portable: `order` is a list of **relative** names w.r.t. the listed folder
//! (`file.ext` or `subdir/file.ext`, `/`-separated) — never absolute paths,
//! never `.`/`..`, never array positions. Moving the whole folder bundle keeps
//! it valid.
//!
//! Writes are atomic (`NamedTempFile` + `persist`, the same contract as sidecar
//! writes) and only happen on an explicit sort change or drag-drop reorder.
//! A corrupt/higher-version/unsafe file is a loud error at the caller — this
//! module never silently normalizes.

use std::path::{Component, Path, PathBuf};

/// File name of the folder sort-order file (next to the images).
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

/// Path of the folder sort-order file for a listed directory.
pub(crate) fn sort_order_path_for(directory: &Path) -> PathBuf {
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

/// Loads the folder sort order, if present. Missing file → `Ok(None)`;
/// unreadable/invalid content → a loud `Err` (the caller reports it visibly and
/// falls back to the default mode, never a silent ignore).
pub(crate) fn load_folder_sort_order(directory: &Path) -> Result<Option<FolderSortOrder>, String> {
    let path = sort_order_path_for(directory);
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path)
        .map_err(|error| format!("cannot read `{}`: {error}", path.display()))?;
    let document: FolderSortOrder = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid sort order in `{}`: {error}", path.display()))?;
    document
        .validate()
        .map_err(|error| format!("invalid sort order in `{}`: {error}", path.display()))?;
    Ok(Some(document))
}

/// Writes the folder sort order atomically (`NamedTempFile` + `persist`).
pub(crate) fn save_folder_sort_order(
    directory: &Path,
    mode: &str,
    order: &[String],
) -> Result<(), String> {
    let document = FolderSortOrder::new(mode, order.to_vec())?;
    let bytes = serde_json::to_vec_pretty(&document)
        .map_err(|error| format!("cannot encode sort order: {error}"))?;
    let path = sort_order_path_for(directory);
    lumina_sidecar::write_atomically(&path, &bytes)
        .map_err(|error| format!("cannot write `{}`: {error}", path.display()))
}
