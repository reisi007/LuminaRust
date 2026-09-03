//! Native on-disk half of the folder cache.
//!
//! Nothing in this module is authoritative: deleting its directory only causes
//! previews to be rendered again; recipes remain in their sidecars.

use super::{preview_cache_key, FolderCacheSettings, PreviewKind};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;

const SETTINGS: &str = "settings.json";
const PREVIEWS: &str = "previews";

#[derive(Debug, Error)]
pub enum DiskCacheError {
    #[error("cache I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("invalid cache settings: {0}")]
    Settings(#[from] serde_json::Error),
}

#[derive(Debug, Serialize, Deserialize)]
struct PreviewRecord {
    source: String,
    virtual_copy: String,
    kind: PreviewKind,
}

/// A cache rooted at the `.lumina` directory belonging to an image folder.
#[derive(Debug, Clone)]
pub struct DiskFolderCache {
    image_folder: PathBuf,
    root: PathBuf,
}

impl DiskFolderCache {
    /// Derive the cache from an image path (the image itself is never touched).
    pub fn for_image(image: impl AsRef<Path>) -> Result<Self, DiskCacheError> {
        let image = image.as_ref();
        let folder = image.parent().unwrap_or_else(|| Path::new("."));
        Self::in_folder(folder)
    }

    pub fn in_folder(folder: impl AsRef<Path>) -> Result<Self, DiskCacheError> {
        let image_folder = folder.as_ref().to_path_buf();
        let root = image_folder.join(".lumina");
        fs::create_dir_all(root.join(PREVIEWS))?;
        Ok(Self { image_folder, root })
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    pub fn settings_path(&self) -> PathBuf {
        self.root.join(SETTINGS)
    }

    /// Read the nearest settings chain, from filesystem root to this folder.
    /// A setting file may contain either or both fields; absent fields inherit.
    pub fn effective_settings(&self) -> Result<FolderCacheSettings, DiskCacheError> {
        let mut effective = FolderCacheSettings::default();
        let mut folders = Vec::new();
        let mut current = Some(self.image_folder.as_path());
        while let Some(folder) = current {
            folders.push(folder.to_path_buf());
            current = folder.parent();
        }
        folders.reverse();
        for folder in folders {
            let path = folder.join(".lumina").join(SETTINGS);
            if path.exists() {
                let value: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
                // Merge only fields present in this file, rather than making
                // serde defaults accidentally erase an inherited option.
                if let Some(v) = value.get("standard_preview").and_then(|v| v.as_bool()) {
                    effective.standard_preview = v;
                }
                if let Some(v) = value.get("one_to_one_preview").and_then(|v| v.as_bool()) {
                    effective.one_to_one_preview = v;
                }
            }
        }
        Ok(effective)
    }

    /// Atomically replace this folder's settings (parents are not modified).
    pub fn save_settings(&self, settings: &FolderCacheSettings) -> Result<(), DiskCacheError> {
        fs::create_dir_all(&self.root)?;
        let bytes = serde_json::to_vec_pretty(settings)?;
        atomic_write(&self.settings_path(), &bytes)
    }

    fn entry_paths(
        &self,
        source: &str,
        virtual_copy: &str,
        kind: PreviewKind,
    ) -> (PathBuf, PathBuf) {
        let id = blake3::hash(preview_cache_key(source, virtual_copy).as_bytes()).to_hex();
        let stem = format!("{id}-{}", kind.as_str());
        (
            self.root.join(PREVIEWS).join(format!("{stem}.json")),
            self.root.join(PREVIEWS).join(format!("{stem}.bin")),
        )
    }

    /// Store a preview only when enabled by the inherited folder settings.
    pub fn store_preview(
        &self,
        source: &str,
        virtual_copy: &str,
        kind: PreviewKind,
        bytes: &[u8],
    ) -> Result<bool, DiskCacheError> {
        if !self.effective_settings()?.allows(kind) {
            return Ok(false);
        }
        let (meta, data) = self.entry_paths(source, virtual_copy, kind);
        atomic_write(&data, bytes)?;
        let record = serde_json::to_vec(&PreviewRecord {
            source: source.into(),
            virtual_copy: virtual_copy.into(),
            kind,
        })?;
        atomic_write(&meta, &record)?;
        Ok(true)
    }

    pub fn load_preview(
        &self,
        source: &str,
        virtual_copy: &str,
        kind: PreviewKind,
    ) -> Result<Option<Vec<u8>>, DiskCacheError> {
        if !self.effective_settings()?.allows(kind) {
            return Ok(None);
        }
        let (meta, data) = self.entry_paths(source, virtual_copy, kind);
        if !meta.is_file() || !data.is_file() {
            return Ok(None);
        }
        Ok(Some(fs::read(data)?))
    }

    /// Delete entries whose recorded source is not in the current directory scan.
    pub fn prune_orphans(&self, live_sources: &[String]) -> Result<usize, DiskCacheError> {
        let directory = self.root.join(PREVIEWS);
        let mut removed = 0;
        for item in fs::read_dir(&directory)? {
            let path = item?.path();
            if path.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let record: Result<PreviewRecord, _> = serde_json::from_slice(&fs::read(&path)?);
            let orphan = record
                .as_ref()
                .map(|r| !live_sources.iter().any(|s| s == &r.source))
                .unwrap_or(true);
            if orphan {
                fs::remove_file(&path)?;
                let bin = path.with_extension("bin");
                if bin.exists() {
                    fs::remove_file(bin)?;
                }
                removed += 1;
            }
        }
        Ok(removed)
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), DiskCacheError> {
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    fn folder() -> PathBuf {
        std::env::temp_dir().join(format!(
            "lumina-cache-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                .wrapping_add(u128::from(NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed)))
        ))
    }
    fn setup() -> (PathBuf, DiskFolderCache) {
        let path = folder();
        fs::create_dir_all(&path).unwrap();
        let cache = DiskFolderCache::in_folder(&path).unwrap();
        (path, cache)
    }

    #[test]
    fn settings_roundtrip_and_parent_inheritance() {
        let (path, cache) = setup();
        cache
            .save_settings(&FolderCacheSettings {
                standard_preview: false,
                one_to_one_preview: true,
            })
            .unwrap();
        assert!(cache.effective_settings().unwrap().one_to_one_preview);
        let child = path.join("child");
        fs::create_dir(&child).unwrap();
        let child_cache = DiskFolderCache::in_folder(&child).unwrap();
        child_cache
            .save_settings(&FolderCacheSettings {
                standard_preview: true,
                one_to_one_preview: false,
            })
            .unwrap();
        assert_eq!(
            child_cache.effective_settings().unwrap(),
            FolderCacheSettings::default()
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn settings_control_standard_and_one_to_one() {
        let (path, cache) = setup();
        assert!(cache
            .store_preview("a.raw", "main", PreviewKind::Standard, b"s")
            .unwrap());
        assert!(!cache
            .store_preview("a.raw", "main", PreviewKind::OneToOne, b"1")
            .unwrap());
        cache
            .save_settings(&FolderCacheSettings {
                one_to_one_preview: true,
                ..Default::default()
            })
            .unwrap();
        assert!(cache
            .store_preview("a.raw", "main", PreviewKind::OneToOne, b"1")
            .unwrap());
        assert_eq!(
            cache
                .load_preview("a.raw", "main", PreviewKind::OneToOne)
                .unwrap(),
            Some(b"1".to_vec())
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn partial_child_settings_inherit_the_other_parent_field() {
        let (path, parent) = setup();
        parent
            .save_settings(&FolderCacheSettings {
                standard_preview: false,
                one_to_one_preview: true,
            })
            .unwrap();
        let child_path = path.join("child");
        let child = DiskFolderCache::in_folder(&child_path).unwrap();
        fs::write(child.settings_path(), br#"{"standard_preview":true}"#).unwrap();

        assert_eq!(
            child.effective_settings().unwrap(),
            FolderCacheSettings {
                standard_preview: true,
                one_to_one_preview: true,
            }
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn prune_removes_orphans_and_cache_is_non_authoritative() {
        let (path, cache) = setup();
        cache
            .store_preview("gone.raw", "main", PreviewKind::Standard, b"x")
            .unwrap();
        assert_eq!(cache.prune_orphans(&[]).unwrap(), 1);
        assert!(cache
            .load_preview("gone.raw", "main", PreviewKind::Standard)
            .unwrap()
            .is_none());
        fs::remove_dir_all(path).unwrap();
    }
}
