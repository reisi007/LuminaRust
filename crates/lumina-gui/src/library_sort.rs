//! LRPAR-G09-SORT-09: Library sort order (Name / Capture Date / Custom) for
//! Grid and Filmstrip, plus the portable custom order.
//!
//! The sort is pure display order: it never touches a recipe, a sidecar or an
//! original. It reorders `LuminaApp::entries`, the single source shared by the
//! grid, the filmstrip, the navigator and the Library keyboard navigation
//! ([`Self::raw_entry_indices`]). The custom arrangement is persisted portably
//! by [`crate::library_sort_file`] as `.lumina/lumina-sort.json` in the
//! folder's deletable cache directory.
//!
//! Stacks (LRPAR-G15-STACK-15) sort as a unit: a collapsed stack shows its
//! cover at the cover's sorted position, and the custom order keeps all members
//! contiguous so the unit can never be split. A drag-drop on any member moves
//! the whole stack.

use super::*;
use crate::library_sort_file::{
    load_folder_sort_order, save_folder_sort_order, sort_order_path_for,
};
use log::{error, info};
use std::cmp::Ordering;
use std::path::{Component, Path, PathBuf};

/// LRPAR-G09-SORT-09: the three — and only three — Library sort modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibrarySort {
    /// File name, lexicographic (the default).
    Name,
    /// EXIF capture date ascending (entries without a timestamp sort last).
    CaptureDate,
    /// Manual order persisted in the folder's `.lumina/lumina-sort.json`.
    Custom,
}

impl LibrarySort {
    /// Stable machine name used in `lumina-sort.json`, logs and tests.
    pub const fn name(self) -> &'static str {
        match self {
            LibrarySort::Name => "name",
            LibrarySort::CaptureDate => "capture_date",
            LibrarySort::Custom => "custom",
        }
    }

    /// Parses the machine name; an unknown value is `None` (loudly rejected by
    /// the caller, never silently defaulted).
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "name" => Some(LibrarySort::Name),
            "capture_date" => Some(LibrarySort::CaptureDate),
            "custom" => Some(LibrarySort::Custom),
            _ => None,
        }
    }
}

/// English label of a sort mode (button + status text).
pub(crate) fn sort_label(sort: LibrarySort) -> &'static str {
    match sort {
        LibrarySort::Name => Str::LibrarySortName.t(),
        LibrarySort::CaptureDate => Str::LibrarySortDate.t(),
        LibrarySort::Custom => Str::LibrarySortCustom.t(),
    }
}

/// Stable egui id of a Library grid cell, so headless tests can locate a cell
/// (and drive the drag-drop reorder) by entry instead of by layout position.
pub(crate) fn library_cell_id(thumb_key: &str) -> egui::Id {
    egui::Id::new(("lumina-library-cell", thumb_key))
}

/// `/`-joined relative key of `path` w.r.t. the listed `root` (portable custom
/// order member: `file.ext` or `subdir/file.ext`, never absolute).
pub(crate) fn relative_key(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Total order of two entries for `mode`. Deterministic for equal keys
/// (file name tie-break); unknown timestamps and unknown custom entries sort
/// after their known counterparts.
fn compare_entries(
    root: &Path,
    a: &FileBrowserEntry,
    b: &FileBrowserEntry,
    mode: LibrarySort,
    order: &BTreeMap<String, usize>,
) -> Ordering {
    match mode {
        LibrarySort::Name => a.name.cmp(&b.name),
        LibrarySort::CaptureDate => match (a.capture_timestamp, b.capture_timestamp) {
            (Some(left), Some(right)) => left.cmp(&right).then_with(|| a.name.cmp(&b.name)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => a.name.cmp(&b.name),
        },
        LibrarySort::Custom => {
            let key_a = relative_key(root, &a.path);
            let key_b = relative_key(root, &b.path);
            match (order.get(&key_a), order.get(&key_b)) {
                (Some(left), Some(right)) => left.cmp(right).then_with(|| key_a.cmp(&key_b)),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => key_a.cmp(&key_b),
            }
        }
    }
}

/// Sorts `entries` in place for `mode`/`order` (pure; shared by the listing
/// path and the explicit sort actions).
pub(crate) fn sort_entries_in(
    root: &Path,
    entries: &mut [FileBrowserEntry],
    mode: LibrarySort,
    order: &[String],
) {
    let index: BTreeMap<String, usize> = order
        .iter()
        .enumerate()
        .map(|(position, key)| (key.clone(), position))
        .collect();
    entries.sort_by(|a, b| compare_entries(root, a, b, mode, &index));
}

impl LuminaApp {
    /// Active Library sort mode (LRPAR-G09-SORT-09).
    pub fn library_sort(&self) -> LibrarySort {
        self.library_sort
    }

    /// Current custom order (relative keys) — the persisted arrangement.
    pub fn library_sort_order(&self) -> &[String] {
        &self.library_sort_order
    }

    /// Selects a sort mode, persists mode + order to the folder's
    /// `.lumina/lumina-sort.json` and re-sorts the display order. Display-only:
    /// recipe/sidecar are never touched. A failed write is returned loudly for
    /// the caller to surface (`show_error`).
    pub fn set_library_sort(&mut self, sort: LibrarySort) -> Result<(), String> {
        instrument_gui_action!(self, GuiAction::SetLibrarySort);
        self.library_sort = sort;
        if sort == LibrarySort::Custom {
            self.seed_custom_order();
        }
        info!("library sort: {}", sort.name());
        self.status = sort_label(sort).to_string();
        let persisted = self.persist_library_sort();
        self.sort_entries_now();
        persisted
    }

    /// Drag-drop reorder: moves `dragged` (its whole stack, when stacked)
    /// before `target`, switches to `Custom`, persists and re-sorts. Returns
    /// loudly on a failed persistence write.
    pub fn reorder_library_entry(&mut self, dragged: &str, target: &str) -> Result<(), String> {
        let root = PathBuf::from(self.directory.trim());
        let target_key = relative_key(&root, Path::new(target));
        let unit = self.sort_unit_keys(dragged);
        if unit.is_empty() || unit.contains(&target_key) {
            return Ok(());
        }
        let mut order = self.current_order_keys();
        order.retain(|key| !unit.contains(key));
        // Insert before the target's whole unit (a stack target must never be
        // split by dropping inside it), falling back to the end.
        let target_unit = self.sort_unit_keys(target);
        let insert_at = order
            .iter()
            .position(|key| target_unit.contains(key))
            .unwrap_or(order.len());
        for (offset, key) in unit.iter().enumerate() {
            order.insert(insert_at + offset, key.clone());
        }
        self.library_sort_order = order;
        self.library_sort = LibrarySort::Custom;
        let name = Path::new(dragged)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(dragged);
        info!("library reorder: `{dragged}` before `{target}` (custom)");
        self.status = Str::LibrarySortReorderedPattern.format_arg(name);
        let persisted = self.persist_library_sort();
        self.sort_entries_now();
        persisted
    }

    /// Re-sorts `entries` for the active mode/order.
    pub(crate) fn sort_entries_now(&mut self) {
        let root = PathBuf::from(self.directory.trim());
        sort_entries_in(
            &root,
            &mut self.entries,
            self.library_sort,
            &self.library_sort_order,
        );
    }

    /// All current display-order keys (RAW entries in `self.entries` order,
    /// including members hidden behind a collapsed stack). Stack members are
    /// kept contiguous (the unit's first occurrence pulls the rest right after
    /// it), so a collapsed stack stays one block in the custom order.
    fn current_order_keys(&self) -> Vec<String> {
        let root = PathBuf::from(self.directory.trim());
        let mut emitted: BTreeSet<String> = BTreeSet::new();
        let mut out: Vec<String> = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            if !is_raw_name(&entry.name) {
                continue;
            }
            let key = relative_key(&root, &entry.path);
            if !emitted.insert(key.clone()) {
                continue;
            }
            out.push(key);
            if entry.stack.is_some() {
                for member in self.stack_present_paths(index) {
                    let member_key = relative_key(&root, Path::new(&member));
                    if emitted.insert(member_key.clone()) {
                        out.push(member_key);
                    }
                }
            }
        }
        out
    }

    /// Fills an empty custom order from the current display order; an existing
    /// arrangement is kept.
    fn seed_custom_order(&mut self) {
        if self.library_sort_order.is_empty() {
            self.library_sort_order = self.current_order_keys();
        }
    }

    /// Relative keys of the drag unit for `path`: the whole stack (all present
    /// members) or just the entry itself. Empty when the path is not listed.
    fn sort_unit_keys(&self, path: &str) -> Vec<String> {
        let root = PathBuf::from(self.directory.trim());
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.path.display().to_string() == path)
        else {
            return Vec::new();
        };
        self.stack_present_paths(index)
            .iter()
            .map(|member| relative_key(&root, Path::new(member)))
            .collect()
    }

    /// Writes mode + order atomically. An empty directory (in-memory session)
    /// is a deliberate no-op; a non-directory is a loud error.
    fn persist_library_sort(&self) -> Result<(), String> {
        if self.directory.trim().is_empty() {
            return Ok(());
        }
        let directory = PathBuf::from(self.directory.trim());
        if !directory.is_dir() {
            return Err(format!(
                "cannot write sort order: `{}` is not a directory",
                directory.display()
            ));
        }
        save_folder_sort_order(
            &directory,
            self.library_sort.name(),
            &self.library_sort_order,
        )
    }

    /// Loads the folder's `.lumina/lumina-sort.json` into the session state (mode +
    /// order). Returns a loud error message when the file exists but is not
    /// usable; the caller surfaces it visibly and the defaults (`Name`, empty
    /// order) apply — never a silent ignore.
    pub(crate) fn load_library_sort_for(&mut self, directory: &Path) -> Option<String> {
        match load_folder_sort_order(directory) {
            Ok(Some(document)) => match LibrarySort::from_name(&document.mode) {
                Some(mode) => {
                    self.library_sort = mode;
                    self.library_sort_order = document.order;
                    None
                }
                None => {
                    let message = format!(
                        "unknown sort mode `{}` in `{}`",
                        document.mode,
                        sort_order_path_for(directory).display()
                    );
                    error!("{message}");
                    self.library_sort = LibrarySort::Name;
                    self.library_sort_order = Vec::new();
                    Some(message)
                }
            },
            Ok(None) => {
                self.library_sort = LibrarySort::Name;
                self.library_sort_order = Vec::new();
                None
            }
            Err(message) => {
                error!("{message}");
                self.library_sort = LibrarySort::Name;
                self.library_sort_order = Vec::new();
                Some(message)
            }
        }
    }

    /// Display-string paths of the filmstrip entries in strip order (the same
    /// RAW-only order [`Self::draw_filmstrip`] renders).
    pub(crate) fn filmstrip_order(&self) -> Vec<String> {
        self.raw_entry_indices()
            .iter()
            .map(|&index| self.entries[index].path.display().to_string())
            .collect()
    }

    /// Indices of the RAW entries in display order (GUI-FILMSTRIP-DUP-1):
    /// the single source behind the filmstrip, the navigator rail and the
    /// Library grid — every image appears exactly once per view, and every
    /// view shares the same selection bookkeeping. A duplicated source path
    /// (e.g. listed twice after overlapping rescans) collapses to its first
    /// occurrence so no view ever shows the same image twice.
    pub(crate) fn raw_entry_indices(&self) -> Vec<usize> {
        let mut seen = BTreeSet::new();
        let indices: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| is_raw_name(&entry.name))
            .filter(|(_, entry)| seen.insert(entry.path.display().to_string()))
            .map(|(index, _)| index)
            .collect();
        // LRPAR-G15-STACK-15: a collapsed stack contributes only its cover.
        self.collapse_stacked_indices(indices)
    }

    /// Filtered Library raster order behind every G-09 view: the RAW-only
    /// display order ([`Self::raw_entry_indices`]) narrowed by the active
    /// collection view and the `\` query. Pure read over `&self`, shared by
    /// grid painting and keyboard navigation so both see the same list.
    pub fn filtered_library_order(&self) -> Vec<usize> {
        let query = self.library_filter.clone();
        let active_collection = self.active_collection.clone();
        let smart_catalog = self.smart_catalog.clone();
        self.raw_entry_indices()
            .into_iter()
            .filter(|&entry_idx| {
                let entry = &self.entries[entry_idx];
                if let Some(filter) = &active_collection {
                    if !collection_filter_matches_entry(entry, filter, &smart_catalog) {
                        return false;
                    }
                }
                library_entry_matches(entry, &query)
            })
            .collect()
    }
}
