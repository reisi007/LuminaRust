//! R2-MODSWITCH-1 F8 (Release 1.0): the asynchronous folder scan.
//!
//! The R2-MODSWITCH-1 / R3-SWITCH-1 measurements showed `list_directory`
//! blocking the UI thread: `scan_entry` reads every sidecar and every RAW's
//! metadata (`lumina_raw::read_metadata`) and hashes the source, so a folder
//! with many large RAWs stalls the frame that opens it. This module moves the
//! scan onto a dedicated worker thread — the same pattern PERF-GUI-7 already
//! uses for the decode (`begin_load_path`/`poll_decode`) — and drains the
//! finished listing on the main thread.
//!
//! **No new pool.** A folder scan is one short-lived thread per requested
//! directory (the decode path does exactly this); the thumbnail/neighbor pools
//! are sized for streaming many independent cells and would only queue the
//! scan behind them. Coalescing is explicit: starting a new scan drops the
//! superseded in-flight request (latest-wins), so rapid tree clicks never pile
//! up stale listings.
//!
//! Load state is visible: while a scan is in flight the status line reads the
//! `scanning` progress string (`Str::ScanningFolder`), and the completed scan
//! is applied atomically by `apply_listing` — no silent stall, no empty grid
//! flash. A scan whose root disappeared surfaces the existing loud
//! "directory not readable" status.

use super::*;
use log::{info, trace};

/// Visible, non-blocking scan progress line (status). Constant instead of a
/// `Str` variant: `i18n.rs` is at its committed size ceiling (Ratchet), same
/// precedent as `WARMUP_PROGRESS` (R3-WARMUP-1) and the R2-JANK-1 F4 pending
/// label.
pub(crate) const SCAN_PROGRESS: &str = "Scanning folder…";

/// One scan request handed to the worker thread. `recursive` selects the
/// F-100 aggregation (include subfolders, depth-limited) over the flat listing.
pub(crate) struct ScanRequest {
    pub(crate) directory: PathBuf,
    pub(crate) recursive: bool,
    /// Generation tag so a superseded result can be dropped explicitly instead
    /// of racing into `apply_listing` (latest-wins).
    pub(crate) generation: u64,
}

/// A finished folder scan: the scanned root, the entries, and the generation.
pub(crate) struct ScanResult {
    pub(crate) directory: PathBuf,
    pub(crate) entries: Vec<FileBrowserEntry>,
    pub(crate) generation: u64,
}

/// Run one scan to completion (worker-thread entry point). Pure read-only; the
/// caller owns all state application.
pub(crate) fn run_scan(request: &ScanRequest) -> ScanResult {
    let mut entries = Vec::new();
    if request.recursive {
        collect_entries_recursive(&request.directory, &mut entries);
    } else {
        collect_entries_flat(&request.directory, &mut entries);
    }
    ScanResult {
        directory: request.directory.clone(),
        entries,
        generation: request.generation,
    }
}

/// Spawn one short-lived worker thread that performs the scan and sends the
/// result back over an unbounded channel.
pub(crate) fn spawn_scan(request: ScanRequest) -> mpsc::Receiver<ScanResult> {
    let (tx, rx) = mpsc::channel();
    let recursive = request.recursive;
    let directory = request.directory.clone();
    std::thread::spawn(move || {
        let result = run_scan(&request);
        // A closed receiver (app shutting down) drops the result silently —
        // there is no UI left to update, so this is not a data-loss path.
        let _ = tx.send(result);
    });
    trace!(
        "GUI scan worker: started for {} (recursive={})",
        directory.display(),
        recursive
    );
    rx
}

impl LuminaApp {
    /// R2-MODSWITCH-1 F8: start an asynchronous scan of `self.directory` and
    /// show the visible loading status. Supersedes any in-flight scan
    /// (latest-wins). Returns the generation just started.
    pub(crate) fn begin_scan(&mut self, recursive: bool) -> u64 {
        let directory = PathBuf::from(self.directory.trim());
        self.scan_generation += 1;
        let request = ScanRequest {
            directory: directory.clone(),
            recursive,
            generation: self.scan_generation,
        };
        self.scan_rx = Some(spawn_scan(request));
        self.scan_pending = true;
        self.status = format!("{SCAN_PROGRESS} {}", directory.display());
        info!("scanning folder: {}", directory.display());
        self.scan_generation
    }

    /// R2-MODSWITCH-1 F8: drain a completed scan (non-blocking) and apply it
    /// atomically through [`Self::apply_listing`]. A superseded result (the user
    /// clicked elsewhere meanwhile) is dropped loudly at `trace!`, never merged.
    /// Returns whether a result was applied this call.
    pub(crate) fn poll_scan(&mut self) -> bool {
        let Some(rx) = &self.scan_rx else {
            return false;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // The worker died without sending: surface the stall loudly
                // instead of leaving the grid pending forever.
                self.scan_rx = None;
                self.scan_pending = false;
                self.status = Str::DirectoryNotReadable
                    .format_arg("scan worker disconnected before returning a listing");
                log::warn!("GUI scan worker disconnected without a listing");
                return false;
            }
        };
        self.scan_rx = None;
        self.scan_pending = false;
        if result.generation != self.scan_generation {
            trace!(
                "GUI scan worker: dropping superseded listing for {} (gen {} != {})",
                result.directory.display(),
                result.generation,
                self.scan_generation
            );
            return false;
        }
        self.apply_listing(result.directory, result.entries);
        true
    }

    /// R2-MODSWITCH-1 F8: a scan is currently pending (visible loading state).
    pub fn scan_pending(&self) -> bool {
        self.scan_pending
    }

    /// PERF-GUI-7: a background decode is currently in flight. Read-only
    /// diagnostic (harness/headless): lets a snapshot settle both async
    /// background paths (scan + decode) before asserting pixels.
    pub fn decode_pending(&self) -> bool {
        self.decode_rx.is_some()
    }

    /// Headless/programmatic synchronous scan: for callers without an event
    /// loop (tests, non-UI scripts) that need the entries immediately. Runs the
    /// exact same scan engine as the worker (`run_scan`) so the two paths cannot
    /// drift; production UI paths use [`Self::begin_scan`]/[`Self::poll_scan`].
    #[cfg(test)]
    pub(crate) fn scan_directory_blocking(&mut self, recursive: bool) {
        let directory = PathBuf::from(self.directory.trim());
        let request = ScanRequest {
            directory: directory.clone(),
            recursive,
            generation: self.scan_generation,
        };
        let result = run_scan(&request);
        self.scan_pending = false;
        self.apply_listing(result.directory, result.entries);
    }
}

/// Flat (single-folder) scan used by [`crate::LuminaApp::list_directory_flat`].
pub(crate) fn collect_entries_flat(directory: &Path, out: &mut Vec<FileBrowserEntry>) {
    scan_single_dir(directory, directory, out);
}

/// Recursive aggregation used by [`crate::LuminaApp::list_directory`].
pub(crate) fn collect_entries_recursive(root: &Path, out: &mut Vec<FileBrowserEntry>) {
    let mut visited = std::collections::HashSet::new();
    scan_dir_recursive(root, root, FOLDER_SCAN_DEPTH, &mut visited, out);
}

/// Scan one directory level: supported images plus orphan sidecars whose
/// source file is missing. Directory entries are skipped here (the recursive
/// driver descends into them separately); every entry gets its subfolder badge
/// relative to `root` (`""` for top-level files).
fn scan_single_dir(root: &Path, dir: &Path, out: &mut Vec<FileBrowserEntry>) {
    if let Ok(dir_entries) = std::fs::read_dir(dir) {
        for entry in dir_entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            if let Some(mut scanned) = scan_entry(&path) {
                scanned.folder = folder_badge(root, &path);
                out.push(scanned);
            }
        }
    }
    // Also pick up orphan sidecars whose source file is missing.
    // After deleting the source, read_dir won't list it, but the
    // .lumina.json sidecar still exists on disk.
    if let Ok(sidecar_entries) = std::fs::read_dir(dir) {
        for entry in sidecar_entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with(".lumina.json") {
                    if let Some(source_name) = name.strip_suffix(".lumina.json") {
                        let source_path = dir.join(source_name);
                        if !out.iter().any(|e| e.path == source_path) {
                            if let Some(mut scanned) = scan_entry(&source_path) {
                                scanned.folder = folder_badge(root, &source_path);
                                out.push(scanned);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Recursive driver behind [`collect_entries_recursive`]: depth-limited,
/// symlink-/loop-safe via canonical `visited` paths. `remaining_depth == 0`
/// scans nothing (same convention as `library_tree::folder_tree_info`).
fn scan_dir_recursive(
    root: &Path,
    dir: &Path,
    remaining_depth: usize,
    visited: &mut std::collections::HashSet<PathBuf>,
    out: &mut Vec<FileBrowserEntry>,
) {
    if remaining_depth == 0 {
        return;
    }
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    if !visited.insert(canonical) {
        return;
    }
    scan_single_dir(root, dir, out);
    let mut subdirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                // GUI-LIBRARY-LUMINA-DIR-1: never descend into `.lumina/`
                // cache directories (exact name, every level) — belt and
                // braces next to the `scan_entry` guard, so the cache is
                // not even walked (and costs no scan depth).
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_none_or(|name| name != ".lumina")
                })
                .collect()
        })
        .unwrap_or_default();
    subdirs.sort();
    for sub in subdirs {
        scan_dir_recursive(root, &sub, remaining_depth - 1, visited, out);
    }
}

/// Scan one source path into a grid entry (images only). Moved from `lib.rs`
/// verbatim (file-size ratchet + cohesive with the scan driver).
pub(crate) fn scan_entry(path: &Path) -> Option<FileBrowserEntry> {
    // GUI-LIBRARY-LUMINA-DIR-1: `.lumina/` is deletable cache, never
    // library content — its files must not land in the grid, Sync/Match,
    // or sidecar writes. The guard lives here (not only in the recursive
    // driver) so flat listings, direct `.lumina/` navigation,
    // single-file refreshes, and orphan-sidecar derivations stay clean.
    if is_lumina_cache_path(path) {
        return None;
    }
    if !is_supported_image(path) {
        return None;
    }
    let sidecar_path = lumina_sidecar::sidecar_path_for(path);
    let has_sidecar = sidecar_path.is_file();
    let mut virtual_copies = 0usize;
    let mut missing_models = 0usize;
    // LR-01: the grid badge shows the default copy's rating/flag — the
    // canonical per-image organization state.
    let mut rating = 0u8;
    let mut flag = lumina_sidecar::Flag::Unflagged;
    let mut color_label = 0u8;
    // G-15 META-MVP (Slice 3): source-level keywords + static collection
    // memberships for the extended Library filter / smart evaluation.
    let mut keywords = Vec::new();
    let mut collections = Vec::new();
    let mut culling_section: Option<lumina_sidecar::CullingSection> = None;
    let mut face_persons = Vec::new();
    // LRPAR-G15-STACK-15: source-level image-stack membership.
    let mut stack = None;
    let source_status = if path.is_file() {
        match lumina_sidecar::load_sidecar(&sidecar_path) {
            Ok(document) => {
                keywords = document.keywords.clone();
                collections = document.collections.clone();
                culling_section = document.culling.clone();
                stack = document.stack.clone();
                face_persons = document
                    .face
                    .as_ref()
                    .map(|analysis| {
                        analysis
                            .persons
                            .iter()
                            .map(|person| person.name.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                virtual_copies = document.virtual_copies.len();
                if let Some(default) = document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.is_default)
                    .or_else(|| document.virtual_copies.first())
                {
                    rating = default.rating;
                    flag = default.flag;
                    color_label = color_label_of(&default.extras);
                }
                let bundle_root = path.parent().unwrap_or_else(|| Path::new("."));
                for copy in &document.virtual_copies {
                    for mask in &copy.mask_library {
                        let artifact_missing = mask.artifact.as_ref().is_some_and(|artifact| {
                            lumina_sidecar::artifact_status(bundle_root, artifact)
                                != ArtifactStatus::Available
                        });
                        if matches!(
                            mask.status,
                            MaskStatus::Missing
                                | MaskStatus::Pending
                                | MaskStatus::Stale
                                | MaskStatus::Corrupt
                        ) || artifact_missing
                        {
                            missing_models += 1;
                        }
                    }
                }
                lumina_sidecar::source_status(path, &document.source)
                    .unwrap_or(SourceStatus::Unchanged)
            }
            Err(_) => SourceStatus::Unchanged,
        }
    } else {
        SourceStatus::Missing
    };
    let cull_badge = cull_gui::scan_cull_badge(culling_section.as_ref(), source_status);
    let conflict = has_sidecar && !matches!(source_status, SourceStatus::Unchanged);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();
    // G-15 META-MVP (Slice 3): EXIF snapshot for the extended Library
    // filter — best effort, never a scan failure. `read_metadata` is a
    // metadata-only probe (no full decode); unreadable sources simply
    // carry `None` (the corresponding predicates then match nothing).
    let (camera, iso, focal_length, capture_timestamp) = match lumina_raw::read_metadata(path) {
        Ok(metadata) => {
            let camera = match (&metadata.camera_make, &metadata.camera_model) {
                (Some(make), Some(model)) => Some(format!("{make} {model}")),
                (Some(make), None) => Some(make.clone()),
                (None, Some(model)) => Some(model.clone()),
                (None, None) => None,
            };
            (
                camera,
                metadata.iso,
                metadata.focal_length,
                metadata.timestamp,
            )
        }
        Err(_) => (None, None, None, None),
    };
    Some(FileBrowserEntry {
        path: path.to_path_buf(),
        name,
        thumb_key: thumbnail_key(path),
        has_sidecar,
        source_status,
        conflict,
        virtual_copies,
        missing_models,
        rating,
        flag,
        color_label,
        keywords,
        collections,
        camera,
        iso,
        focal_length,
        capture_timestamp,
        folder: String::new(),
        cull_badge,
        face_persons,
        stack,
    })
}
