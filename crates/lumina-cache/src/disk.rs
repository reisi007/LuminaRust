//! Native on-disk half of the folder cache.
//!
//! Nothing in this module is authoritative: deleting its directory only causes
//! previews to be rendered again; recipes remain in their sidecars.
//!
//! LUMINA-CACHE-EXTRACT-70: dieser Tier liegt im Crate `lumina-cache`, außerhalb
//! von `lumina-core`, damit `lumina-core` im Produktionscode ohne Dateisystem
//! kompiliert (Architektur-Review-Befund R4). Q4: das Pfad-Layout ist hier
//! öffentlich nutzbar, damit Frontends es aufrufen statt es zu spiegeln.

use lumina_core::cache::{preview_cache_key, FolderCacheSettings, PreviewKind};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use thiserror::Error;

const SETTINGS: &str = "settings.json";

/// Hard upper bound for one `.lumina/settings.json` file (64 KiB).
///
/// SEC-4: the settings file is read from **every** ancestor directory of the
/// image folder, so on a shared mount it can be controlled from outside the
/// user's reach. The largest payload the app itself ever writes is
/// `serde_json::to_vec_pretty(FolderCacheSettings)` — three boolean fields,
/// measured 101 bytes (`save_settings`). 64 KiB is ~650 times that payload
/// and keeps the parsed JSON tree in the low megabytes even in the worst
/// case (many tiny keys).
pub const MAX_SETTINGS_BYTES: u64 = 64 * 1024;

/// Unterverzeichnis des `.lumina`-Cachewurzelverzeichnisses, das die Preview-
/// Einträge trägt. Öffentlich, weil die Frontends dasselbe Verzeichnis listen.
pub const PREVIEWS_DIR: &str = "previews";

#[derive(Debug, Error)]
pub enum DiskCacheError {
    #[error("cache I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("invalid cache settings: {0}")]
    Settings(#[from] serde_json::Error),
    /// SEC-4: a settings file above [`MAX_SETTINGS_BYTES`] is refused. Its
    /// content is never read into memory and never used; `size` is the number
    /// of bytes the bounded read pulled before the refusal.
    #[error(
        "invalid cache settings: {path} is larger than the {limit}-byte limit ({size} bytes read)"
    )]
    SettingsTooLarge {
        path: PathBuf,
        limit: u64,
        size: u64,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct PreviewRecord {
    source: String,
    virtual_copy: String,
    kind: PreviewKind,
    /// Exact source bytes that authorized this preview. `None` is a legacy
    /// filename-only record and must never satisfy an identified lookup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_content_hash: Option<String>,
}

/// A cache rooted at the `.lumina` directory belonging to an image folder.
#[derive(Debug, Clone)]
pub struct DiskFolderCache {
    image_folder: PathBuf,
    root: PathBuf,
}

/// Shared on-disk layout of one preview entry: the stem of the `.json` metadata
/// and the `.bin` data file below `PREVIEWS_DIR`, derived from source, virtual
/// copy and kind. Public (Q4 of LUMINA-CACHE-EXTRACT-70) so native frontends
/// index that directory through this function instead of mirroring the layout;
/// the GUI thumbnail index is its only other reader.
pub fn entry_stem(source: &str, virtual_copy: &str, kind: PreviewKind) -> String {
    let id = blake3::hash(preview_cache_key(source, virtual_copy).as_bytes()).to_hex();
    format!("{id}-{}", kind.as_str())
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
        fs::create_dir_all(root.join(PREVIEWS_DIR))?;
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
    ///
    /// SEC-4: every level is read **bounded** (at most
    /// [`MAX_SETTINGS_BYTES`]); a file above the limit is refused with
    /// [`DiskCacheError::SettingsTooLarge`] and its content is never used.
    /// The refusal **propagates** instead of silently falling back to the
    /// defaults: an unusable level would otherwise change the effective
    /// configuration without a trace, and the defaults (`standard_preview`
    /// on) are not what such a file states. Callers therefore see a loud
    /// error; the cache degrades to "no preview caching" (fail closed), not
    /// to "different settings".
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
                let value: serde_json::Value =
                    serde_json::from_slice(&read_settings_bytes(&path)?)?;
                // Merge only fields present in this file, rather than making
                // serde defaults accidentally erase an inherited option.
                if let Some(v) = value.get("standard_preview").and_then(|v| v.as_bool()) {
                    effective.standard_preview = v;
                }
                if let Some(v) = value.get("one_to_one_preview").and_then(|v| v.as_bool()) {
                    effective.one_to_one_preview = v;
                }
                // LRPAR-G01-BASIC: same field-wise inheritance as the preview
                // flags — absent keeps the parent value, never resets it.
                if let Some(v) = value
                    .get("reset_sliders_automatically")
                    .and_then(|v| v.as_bool())
                {
                    effective.reset_sliders_automatically = v;
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

    /// Full path pair of one preview entry under a cache `root`. Associated
    /// (handle-free) form of [`entry_stem`], callable without a
    /// [`DiskFolderCache`] handle so frontends can name the entry core wrote.
    pub fn entry_paths(
        root: &Path,
        source: &str,
        virtual_copy: &str,
        kind: PreviewKind,
    ) -> (PathBuf, PathBuf) {
        let stem = entry_stem(source, virtual_copy, kind);
        (
            root.join(PREVIEWS_DIR).join(format!("{stem}.json")),
            root.join(PREVIEWS_DIR).join(format!("{stem}.bin")),
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
        self.store_preview_record(source, virtual_copy, kind, None, bytes)
    }

    /// Store a preview authorized by exact source-content identity.
    pub fn store_preview_with_source_hash(
        &self,
        source: &str,
        virtual_copy: &str,
        kind: PreviewKind,
        source_content_hash: &str,
        bytes: &[u8],
    ) -> Result<bool, DiskCacheError> {
        self.store_preview_record(
            source,
            virtual_copy,
            kind,
            Some(source_content_hash.to_owned()),
            bytes,
        )
    }

    fn store_preview_record(
        &self,
        source: &str,
        virtual_copy: &str,
        kind: PreviewKind,
        source_content_hash: Option<String>,
        bytes: &[u8],
    ) -> Result<bool, DiskCacheError> {
        if !self.effective_settings()?.allows(kind) {
            return Ok(false);
        }
        let (meta, data) = Self::entry_paths(&self.root, source, virtual_copy, kind);
        atomic_write(&data, bytes)?;
        let record = serde_json::to_vec(&PreviewRecord {
            source: source.into(),
            virtual_copy: virtual_copy.into(),
            kind,
            source_content_hash,
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
        let (meta, data) = Self::entry_paths(&self.root, source, virtual_copy, kind);
        if !meta.is_file() || !data.is_file() {
            return Ok(None);
        }
        Ok(Some(fs::read(data)?))
    }

    /// Load only a preview whose persisted record names the exact expected
    /// source bytes. Legacy filename-only and mismatched records are misses.
    pub fn load_preview_with_source_hash(
        &self,
        source: &str,
        virtual_copy: &str,
        kind: PreviewKind,
        source_content_hash: &str,
    ) -> Result<Option<Vec<u8>>, DiskCacheError> {
        if !self.effective_settings()?.allows(kind) {
            return Ok(None);
        }
        let (meta, data) = Self::entry_paths(&self.root, source, virtual_copy, kind);
        if !meta.is_file() || !data.is_file() {
            return Ok(None);
        }
        let record: PreviewRecord = serde_json::from_slice(&fs::read(meta)?)?;
        if record.source != source
            || record.virtual_copy != virtual_copy
            || record.kind != kind
            || record.source_content_hash.as_deref() != Some(source_content_hash)
        {
            return Ok(None);
        }
        Ok(Some(fs::read(data)?))
    }

    /// Delete entries whose recorded source is not in the current directory scan.
    pub fn prune_orphans(&self, live_sources: &[String]) -> Result<usize, DiskCacheError> {
        let directory = self.root.join(PREVIEWS_DIR);
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

/// Read one settings file, bounded by [`MAX_SETTINGS_BYTES`].
///
/// SEC-4: the bound is enforced by the **reader**, not by a
/// `metadata().len()` pre-check — the file may grow between the check and
/// the read, so only a limited read can bound the memory. At most
/// `MAX_SETTINGS_BYTES + 1` bytes are pulled from disk; one byte more than
/// allowed is enough to prove that the file is too large, and that byte is
/// never kept. A file above the limit is refused with
/// [`DiskCacheError::SettingsTooLarge`] — its content is not used.
fn read_settings_bytes(path: &Path) -> Result<Vec<u8>, DiskCacheError> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_SETTINGS_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SETTINGS_BYTES {
        return Err(DiskCacheError::SettingsTooLarge {
            path: path.to_path_buf(),
            limit: MAX_SETTINGS_BYTES,
            size: bytes.len() as u64,
        });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
