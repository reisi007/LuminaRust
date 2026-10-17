//! Native on-disk preview tier, aus `lumina-core` extrahiert
//! (LUMINA-CACHE-EXTRACT-70 Teil 2).
//!
//! Dieses Modul ist der **zweite** native Disk-Tier, der in `lumina-cache`
//! liegt: RAM-LRU, Cache-Keys, Prefetch-Fenster und die WebP-Helfer bleiben in
//! `lumina-core` (`crates/lumina-core/src/preview_cache.rs`); nur dieser
//! persistente `.lumina/previews/*.webp`-Tier ist herausgezogen, damit
//! `lumina-core` im Produktionscode ohne Dateisystem auskommt. Der RAM-LRU +
//! Key + Window + Encode-Helfer obigen Core-Moduls bleiben voll portabel.
//!
//! Nichts hier ist autoritativ: Loeschen des Verzeichnisses loescht nur
//! Vorschauen, die erneut gerendert werden.

use lumina_core::preview_cache::decode_webp;
use lumina_core::{CoreError, ImageFrame};

/// Native on-disk preview tier rooted at `<folder>/.lumina/previews/`.
///
/// Addresses entries by the digest of
/// [`lumina_core::preview_cache::PreviewKey`]. Atomic write via write-to-temp +
/// rename; a file that does not decode (partial write, corrupt) is reported as a
/// miss — never a valid hit.
#[derive(Clone)]
pub struct PreviewDiskCache {
    previews_dir: std::path::PathBuf,
}

impl PreviewDiskCache {
    /// Root the disk tier at the given folder's `.lumina/previews` directory
    /// (created on demand).
    pub fn in_folder(folder: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let previews_dir = folder.as_ref().join(".lumina").join("previews");
        std::fs::create_dir_all(&previews_dir)?;
        Ok(Self { previews_dir })
    }

    fn file_path(&self, key_digest: &str) -> std::path::PathBuf {
        self.previews_dir.join(format!("{key_digest}.preview.webp"))
    }

    /// Store an encoded WebP atomically. Returns the number of bytes written.
    pub fn store(&self, key_digest: &str, webp: &[u8]) -> std::io::Result<usize> {
        let path = self.file_path(key_digest);
        let temporary = self
            .previews_dir
            .join(format!("{key_digest}.{}.tmp", std::process::id()));
        std::fs::write(&temporary, webp)?;
        std::fs::rename(&temporary, &path)?;
        Ok(webp.len())
    }

    /// Load an entry; a file that exists but fails to decode is a miss
    /// (never a valid hit).
    pub fn load(&self, key_digest: &str) -> Result<Option<ImageFrame>, CoreError> {
        let path = self.file_path(key_digest);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(None),
        };
        match decode_webp(&bytes) {
            Ok(frame) => Ok(Some(frame)),
            Err(_) => Ok(None),
        }
    }

    /// Remove files whose records/digests are not in `live_digests` (prune
    /// orphans, analogous to `DiskFolderCache::prune_orphans`).
    pub fn prune_orphans(&self, live_digests: &[String]) -> std::io::Result<usize> {
        let mut removed = 0;
        for item in std::fs::read_dir(&self.previews_dir)? {
            let item = item?;
            let Some(name) = item.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name.ends_with(".tmp") {
                // An interrupted atomic write: never a valid hit, remove it.
                std::fs::remove_file(item.path())?;
                removed += 1;
                continue;
            }
            if let Some(stem) = name.strip_suffix(".preview.webp") {
                if !live_digests.iter().any(|d| d == stem) {
                    std::fs::remove_file(item.path())?;
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }

    /// Delete the whole tier (complete deletability test).
    pub fn clear(&self) -> std::io::Result<()> {
        for item in std::fs::read_dir(&self.previews_dir)? {
            let item = item?;
            if item.path().is_file() {
                std::fs::remove_file(item.path())?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::preview_cache::{
        encode_webp_lossless, PreviewEncode, PreviewKey, PreviewKind,
    };

    fn frame(w: u32, h: u32, value: u8) -> ImageFrame {
        ImageFrame::new(w, h, vec![value; (w * h * 4) as usize]).unwrap()
    }

    fn key_of(content: &str, render: &str) -> PreviewKey {
        PreviewKey {
            source_content_hash: content.into(),
            decode_context: "decode-v1".into(),
            pipeline_version: "pipe-v1".into(),
            virtual_copy_id: "vc-original".into(),
            render_key: render.into(),
            kind: PreviewKind::Screen,
            width: 800,
            height: 600,
            encode: PreviewEncode::default(),
        }
    }

    // ---- Disk tier (native) ----

    #[test]
    fn disk_tier_roundtrip_prune_and_clear() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "lumina-preview-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                + (N.fetch_add(1, Ordering::Relaxed) as u128)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let disk = PreviewDiskCache::in_folder(&dir).unwrap();
        let key = key_of("content-a", "render-a");
        let digest = key.digest();
        let frame = frame(4, 4, 9);
        let webp = encode_webp_lossless(&frame).unwrap();
        assert_eq!(disk.store(&digest, &webp).unwrap(), webp.len());

        // Hit.
        assert_eq!(disk.load(&digest).unwrap().map(|f| f.pixels[0]), Some(9));

        // A non-existent digest is a miss.
        assert!(disk.load("missing").unwrap().is_none());

        // A partial write (interrupted temp) is a miss and is pruned.
        disk.prune_orphans(std::slice::from_ref(&digest)).unwrap();
        assert_eq!(
            disk.load(&digest).unwrap().map(|f| f.pixels[0]),
            Some(9),
            "live digest survives prune"
        );

        // Orphan pruning removes a missing digest.
        disk.prune_orphans(&[]).unwrap();
        assert!(disk.load(&digest).unwrap().is_none());

        // Completeness: store again then fully clear.
        disk.store(&digest, &webp).unwrap();
        assert!(disk.load(&digest).unwrap().is_some());
        disk.clear().unwrap();
        assert!(disk.load(&digest).unwrap().is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
