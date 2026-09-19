//! R2-MODSWITCH-1 F7: folder-scoped, metadata-only preview-cache probing for
//! the thumbnail scheduler.
//!
//! The module-switch latency came from [`crate::filmstrip_frame`]'s former
//! synchronous cache-hit path: every visible cell built a fresh
//! `DiskFolderCache` (`create_dir_all`), walked the folder-settings ancestor
//! chain, opened and read the stored PNG, decoded it and uploaded the texture —
//! all on the UI thread, once per cell. This module replaces that with a
//! per-folder metadata index:
//!
//! * [`PreviewIndexCache::probe`] builds one [`PreviewIndex`] per image folder
//!   (one `create_dir_all`, one `effective_settings` read, one `read_dir` of
//!   the previews directory) and reuses it for every later cell and frame.
//! * `has_standard_preview` is then pure memory: no settings chain, no file
//!   read, no decode. The expensive decode moves to the thumbnail worker
//!   (`crate::thumb_worker`).
//! * The effective folder settings are memoized inside the index. A settings
//!   write through [`crate::LuminaApp::set_reset_sliders_automatically`] (or a
//!   directory change) invalidates the memo explicitly, so a changed folder
//!   option can never be served silently from a stale index.
//!
//! The preview path layout (`<folder>/.lumina/previews/<blake3>-<kind>.bin`)
//! mirrors `lumina_core::cache::disk`'s private `entry_paths`. The
//! `preview_index_layout_matches_core_store` test pins the two together by
//! writing through the core API and probing through this index, so a core layout
//! change fails the suite loudly instead of silently turning every thumbnail
//! into a cold render.

use lumina_core::cache::disk::DiskFolderCache;
use lumina_core::cache::{preview_cache_key, PreviewKind};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Preview directory below a folder's `.lumina` cache root (mirrors core).
const PREVIEWS_DIR: &str = "previews";

/// Virtual copy whose standard preview the filmstrip/Library cells display
/// (the same identity `ensure_thumbnail` has always probed).
pub(crate) const THUMB_VIRTUAL_COPY: &str = "vc-original";

/// Metadata-only snapshot of one image folder's preview cache.
struct PreviewIndex {
    /// Folder cache handle, reused by the worker jobs so it is built once per
    /// folder instead of once per cell. `None` when the folder cache could not
    /// be opened (then every probe is a miss and the worker renders fresh).
    cache: Option<DiskFolderCache>,
    /// Memoized effective `standard_preview` folder option.
    allows_standard: bool,
    /// Stems (`<blake3>-standard`) of the `.bin` previews present on disk.
    stems: BTreeSet<String>,
}

impl PreviewIndex {
    fn empty() -> Self {
        Self {
            cache: None,
            allows_standard: false,
            stems: BTreeSet::new(),
        }
    }

    fn build(folder: &Path) -> Self {
        let Ok(cache) = DiskFolderCache::in_folder(folder) else {
            return Self::empty();
        };
        // One ancestor-chain settings read per folder (memoized for the frames
        // that follow; invalidated on a settings write).
        let allows_standard = cache
            .effective_settings()
            .map(|settings| settings.allows(PreviewKind::Standard))
            .unwrap_or(false);
        let mut stems = BTreeSet::new();
        // One metadata-only directory listing; no preview file is opened here.
        if let Ok(entries) = std::fs::read_dir(cache.path().join(PREVIEWS_DIR)) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("bin") {
                    continue;
                }
                if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                    stems.insert(stem.to_owned());
                }
            }
        }
        Self {
            cache: Some(cache),
            allows_standard,
            stems,
        }
    }

    /// Metadata-only hit check. The settings gate is applied here exactly like
    /// `DiskFolderCache::load_preview` does, so a disabled option can never be
    /// bypassed by the index.
    fn has_standard_preview(&self, source: &str) -> bool {
        self.allows_standard && self.stems.contains(&standard_stem(source))
    }
}

/// Stable on-disk stem of the standard preview for one source
/// (`vc-original` copy). Mirrors `DiskFolderCache::entry_paths`.
fn standard_stem(source: &str) -> String {
    let id = blake3::hash(preview_cache_key(source, THUMB_VIRTUAL_COPY).as_bytes()).to_hex();
    format!("{id}-{}", PreviewKind::Standard.as_str())
}

/// Per-folder metadata index, memoized across frames and invalidated visibly on
/// a directory switch or a folder-settings write.
#[derive(Default)]
pub(crate) struct PreviewIndexCache {
    /// Browsed root the indices belong to; a change drops them all.
    directory: Option<String>,
    folders: BTreeMap<PathBuf, PreviewIndex>,
    /// Test-only build counter: proves the index is built once per folder, not
    /// once per cell.
    #[cfg(test)]
    builds: u32,
}

/// Result of one scheduler probe: the folder cache handle to reuse in the
/// worker job plus the metadata-only cache-hit bit.
pub(crate) struct ThumbProbe {
    pub(crate) cache: Option<DiskFolderCache>,
    pub(crate) cached: bool,
}

impl PreviewIndexCache {
    /// Drop every index when the browsed directory changes; relisting the same
    /// directory keeps the warm indices (mirrors `ThumbnailManager`).
    pub(crate) fn ensure_directory(&mut self, directory: &str) {
        if self.directory.as_deref() != Some(directory) {
            self.clear();
            self.directory = Some(directory.to_owned());
        }
    }

    /// Invalidate one folder's memoized index after its settings changed.
    pub(crate) fn invalidate_folder(&mut self, folder: &Path) {
        self.folders.remove(folder);
    }

    /// Drop every memoized index.
    pub(crate) fn clear(&mut self) {
        self.folders.clear();
    }

    /// Metadata-only probe for one entry. Builds the folder index on first use
    /// (one `create_dir_all` + one settings read + one `read_dir`) and reuses it
    /// for every later cell/frame.
    pub(crate) fn probe(&mut self, folder: &Path, source: &str) -> ThumbProbe {
        let index = self.index_for(folder);
        ThumbProbe {
            cache: index.cache.clone(),
            cached: index.has_standard_preview(source),
        }
    }

    fn index_for(&mut self, folder: &Path) -> &PreviewIndex {
        if !self.folders.contains_key(folder) {
            #[cfg(test)]
            {
                self.builds += 1;
            }
            let index = PreviewIndex::build(folder);
            self.folders.insert(folder.to_path_buf(), index);
        }
        self.folders.get(folder).expect("index inserted above")
    }

    /// Test-only: how many folder indices were built.
    #[cfg(test)]
    pub(crate) fn builds(&self) -> u32 {
        self.builds
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::cache::FolderCacheSettings;

    fn folder() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    /// The metadata index must agree with the core writer: a preview stored
    /// through `DiskFolderCache::store_preview` is a probe hit, and the index's
    /// derived stem is exactly the file core wrote. This pins the mirrored
    /// layout so a core change fails here instead of silently degrading every
    /// thumbnail to a cold render.
    #[test]
    fn preview_index_layout_matches_core_store() {
        let dir = folder();
        let source = "photo.arw";
        let cache = DiskFolderCache::in_folder(dir.path()).unwrap();
        let mut index = PreviewIndexCache::default();
        assert!(
            !index.probe(dir.path(), source).cached,
            "no preview stored yet"
        );

        cache
            .store_preview(source, THUMB_VIRTUAL_COPY, PreviewKind::Standard, b"png")
            .unwrap();
        // The derived stem must equal the file core actually created.
        let entry = std::fs::read_dir(cache.path().join(PREVIEWS_DIR))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .find(|name| name.ends_with("-standard.bin"))
            .expect("core wrote a standard preview");
        assert_eq!(entry, format!("{}.bin", standard_stem(source)));

        let mut index = PreviewIndexCache::default();
        assert!(
            index.probe(dir.path(), source).cached,
            "the metadata index must see the preview core stored"
        );
    }

    /// One index per folder, reused across cells and frames — never rebuilt per
    /// cell (the former per-cell cost).
    #[test]
    fn index_is_built_once_per_folder_not_per_cell() {
        let dir = folder();
        let mut index = PreviewIndexCache::default();
        for name in ["a.arw", "b.arw", "c.arw"] {
            index.probe(dir.path(), name);
        }
        assert_eq!(index.builds(), 1, "three cells in one folder = one build");
        index.probe(dir.path(), "d.arw");
        assert_eq!(index.builds(), 1, "later frames reuse the warm index");

        let other = folder();
        index.probe(other.path(), "e.arw");
        assert_eq!(index.builds(), 2, "a new folder gets its own index");
    }

    /// A settings write that disables standard previews is invisible until the
    /// index is invalidated (memoization), then takes effect (visible
    /// invalidation) — never a silent stale gate.
    #[test]
    fn settings_memo_invalidates_visibly() {
        let dir = folder();
        let cache = DiskFolderCache::in_folder(dir.path()).unwrap();
        cache
            .store_preview("photo.arw", THUMB_VIRTUAL_COPY, PreviewKind::Standard, b"x")
            .unwrap();
        let mut index = PreviewIndexCache::default();
        assert!(index.probe(dir.path(), "photo.arw").cached);

        cache
            .save_settings(&FolderCacheSettings {
                standard_preview: false,
                one_to_one_preview: false,
                reset_sliders_automatically: false,
            })
            .unwrap();
        assert!(
            index.probe(dir.path(), "photo.arw").cached,
            "the memoized index must not re-read settings on every probe"
        );
        index.invalidate_folder(dir.path());
        assert!(
            !index.probe(dir.path(), "photo.arw").cached,
            "after invalidation the disabled option must take effect visibly"
        );
    }

    /// The browsed directory change drops every warm index.
    #[test]
    fn directory_change_clears_the_index() {
        let dir = folder();
        let mut index = PreviewIndexCache::default();
        index.probe(dir.path(), "a.arw");
        assert_eq!(index.builds(), 1);
        index.ensure_directory("/somewhere/else");
        assert_eq!(
            index.folders.len(),
            0,
            "a directory switch drops the indices"
        );
    }
}
