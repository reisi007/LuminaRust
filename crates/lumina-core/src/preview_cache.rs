//! Hybrid neighbor **preview cache** (PREVIEW-CACHE-FEATURE).
//!
//! This is the platform-neutral core of the GUI's scrolling-optimization stack
//! (`feature/quality/preview-cache.md`): the active image stays a full GPU
//! texture, while the neighbors in the navigation window are prepared as WebP
//! previews on Screen/1:1 resolution, on disk and in a small RAM LRU, so the
//! next image switch has no visible decode/render stall.
//!
//! What lives here (kept deliberately GPU-free and, except for the native disk
//! tier, file-system-free):
//!
//! - [`PreviewKey`]: the full cache identity derived from source content hash,
//!   decode/pipeline context, virtual copy + render key, and preview
//!   kind/resolution. Veraltung is measured **by the key**, never by timestamp.
//! - [`prefetch_window`]: the asymmetric **+4 / −2** window with the mandated
//!   priority order `+1 > +2 > −1 > +3 > −2 > +4`, no wrap-around at the order
//!   edges.
//! - [`LruPreviewCache`]: the RAM LRU (default 7 slots: active + 6 neighbors),
//!   byte-budgeted, with the **active entry pinned** (never evicted, promoted on
//!   every access).
//! - WebP encode/decode helpers ([`encode_webp_lossless`], [`decode_webp`]) that
//!   preserve the alpha channel (via `image` 0.25, already a dependency).
//!
//! Disk tier: persistent `.webp` files under `.lumina/previews/` (native only),
//! addressed by the digest of [`PreviewKey`]; a partial/corrupt file is a miss,
//! never a valid hit. This is a *performance* layer only — fully deletable,
//! never authoritative.
//!
//! Semantics (from the SOLL):
//! - A **hit** with a matching key serves pixels immediately (no decode).
//! - A **miss** is a visible "wird vorbereitet" state until the neighbor is
//!   ready — never a silently wrong / upscaled image.
//! - A **stale** entry (source content hash or render key changed) is reported
//!   as stale, never silently displayed.

use crate::ImageFrame;
use blake3::Hasher;
use std::collections::HashMap;

/// Resolution / preview-kind of a cached neighbor preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreviewKind {
    /// Aspekt-treu panel-fit (Screen) resolution — the default.
    Screen,
    /// 1:1 preview (inherited, opt-in folder option).
    OneToOne,
}

impl PreviewKind {
    pub const ALL: [PreviewKind; 2] = [Self::Screen, Self::OneToOne];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Screen => "screen",
            Self::OneToOne => "1:1",
        }
    }
}

/// Encoder parameters that participate in the cache key (a quality change must
/// not serve an old-quality entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PreviewEncode {
    pub lossless: bool,
    /// Only meaningful when `lossless == false`; 0..=100 (100 == lossless).
    pub quality: u8,
}

impl Default for PreviewEncode {
    fn default() -> Self {
        Self {
            lossless: true,
            quality: 100,
        }
    }
}

/// Full cache identity of one neighbor preview.
///
/// Every field is part of the digest; a change in **any** of them makes the
/// entry stale. This is the "Cache-Key: Content-Hash + Render-Key" from the
/// SOLL, plus the geometry/kind/encoder context so stale detection is
/// exact rather than heuristic.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PreviewKey {
    /// BLAKE3 content hash of the source bytes (the critical operation that may
    /// use the full hash, per Agents.md).
    pub source_content_hash: String,
    /// Decode version + geometry/ROI context of the RAW/raster decode.
    pub decode_context: String,
    /// Pipeline version at render time.
    pub pipeline_version: String,
    /// Stable virtual copy id.
    pub virtual_copy_id: String,
    /// Render-key / recipe hash — every recipe-dependent stage.
    pub render_key: String,
    /// Screen vs. 1:1 and target dimensions.
    pub kind: PreviewKind,
    pub width: u32,
    pub height: u32,
    /// Encoder/format parameters.
    pub encode: PreviewEncode,
}

impl PreviewKey {
    /// Stable cache-key digest. Used both as the RAM-LRU key and (via
    /// [`PreviewKey::disk_name`]) the on-disk file stem.
    #[must_use]
    pub fn digest(&self) -> String {
        let mut hasher = Hasher::new();
        hasher.update(b"preview");
        hasher.update(&[0]);
        for value in [
            &self.source_content_hash,
            &self.decode_context,
            &self.pipeline_version,
            &self.virtual_copy_id,
            &self.render_key,
        ] {
            hasher.update(value.as_bytes());
            hasher.update(&[0]);
        }
        hasher.update(self.kind.as_str().as_bytes());
        hasher.update(&[0]);
        hasher.update(&self.width.to_le_bytes());
        hasher.update(&self.height.to_le_bytes());
        hasher.update(&[if self.encode.lossless { 1u8 } else { 0u8 }]);
        hasher.update(&self.encode.quality.to_le_bytes());
        hasher.finalize().to_hex().to_string()
    }
}

/// One planned neighbor slot of the asymmetric prefetch window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefetchSlot {
    /// Index into the ordered source list (0-based), always clamped to the
    /// list bounds — there is never a wrap-around.
    pub index: usize,
    /// Relative offset from the active image (negative = backwards).
    pub offset: i64,
    /// Priority rank: 0 = highest (+1), 5 = lowest (+4). This is the mandated
    /// order `+1 > +2 > −1 > +3 > −2 > +4`.
    pub priority: u8,
}

/// Compute the asymmetric +4 / −2 prefetch window around `active` in a folder
/// of `count` images.
///
/// The returned slots are already sorted by priority (highest first). Offsets
/// that fall outside `0..count` are skipped (no wrap-around). The window never
/// yields more than 6 neighbors — the RAM LRU then holds active + 6 = 7.
#[must_use]
pub fn prefetch_window(active: usize, count: usize) -> Vec<PrefetchSlot> {
    // Priority (mandated): +1, +2, −1, +3, −2, +4.
    const OFFSETS: [(i64, u8); 6] = [(1, 0), (2, 1), (-1, 2), (3, 3), (-2, 4), (4, 5)];
    let mut slots = Vec::with_capacity(6);
    for (offset, priority) in OFFSETS {
        let index = active as i64 + offset;
        if index >= 0 && (index as usize) < count {
            let index = index as usize;
            // De-duplicate: an extremely small count could in theory alias two
            // offsets to the same index? With distinct offsets that cannot
            // happen (offsets differ), so no dedup is needed.
            slots.push(PrefetchSlot {
                index,
                offset,
                priority,
            });
        }
    }
    slots
}

/// RAM LRU of decoded neighbor preview frames.
///
/// - Max `slots` (default 7: active + 6 neighbors), additionally byte-budgeted.
/// - The **active** entry is pinned: [`Self::get`] marks it most-recently-used
///   and eviction never removes it.
/// - On a miss, the caller renders lazily (see SOLL: a miss is a visible
///   preparation state, not a silent fallback).
#[derive(Debug)]
pub struct LruPreviewCache {
    entries: HashMap<String, Slot>,
    max_slots: usize,
    max_bytes: usize,
    used_bytes: usize,
    clock: u64,
}

#[derive(Debug)]
struct Slot {
    frame: ImageFrame,
    stamp: u64,
    active: bool,
}

impl Default for LruPreviewCache {
    fn default() -> Self {
        // 7 slots (active + 6 neighbors). Byte budget far below the 8 GB GUI
        // RAM+VRAM budget: 7 24 MP RGBA8 frames ≈ 672 MB; a modest ceiling of
        // 1.5 GiB keeps any reasonable neighbor set under the documented budget
        // while still blocking pathological growth.
        Self::new(7, 1_500_000_000)
    }
}

impl LruPreviewCache {
    #[must_use]
    pub fn new(max_slots: usize, max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_slots,
            max_bytes,
            used_bytes: 0,
            clock: 0,
        }
    }

    /// Returns a private copy of the cached frame, marking it most-recently
    /// used; the (transient) active entry is promoted on every access.
    /// A `None` result is a miss. The caller owns the copy.
    pub fn get(&mut self, key: &str) -> Option<ImageFrame> {
        let slot = self.entries.get_mut(key)?;
        self.clock += 1;
        slot.stamp = self.clock;
        Some(slot.frame.clone())
    }

    /// Promote an entry without returning its pixels (used when the active
    /// image is refreshed and its key changes but the slot should stay warm).
    pub fn touch(&mut self, key: &str) {
        if let Some(slot) = self.entries.get_mut(key) {
            self.clock += 1;
            slot.stamp = self.clock;
        }
    }

    /// Mark the entry for `key` as the active one (pinned). This is called when
    /// a neighbor becomes the current image: its RAM entry is promoted and must
    /// never be evicted.
    pub fn set_active(&mut self, key: &str) {
        if let Some(slot) = self.entries.get_mut(key) {
            self.clock += 1;
            slot.stamp = self.clock;
            slot.active = true;
        }
    }

    /// No-op guard that only bumps recency for a matching active key, used to
    /// keep the "active never evicted" invariant observable in tests.
    pub fn is_active(&self, key: &str) -> bool {
        self.entries.get(key).is_some_and(|slot| slot.active)
    }

    /// Insert (or replace) a decoded frame under `key`, preserving the pinned
    /// active flag if the key is already active. Evicts the LRU **non-active**
    /// entry until the slot and byte budgets fit. Returns `false` when the
    /// frame alone exceeds the byte budget (documented capacity limit, not a
    /// silent fallback).
    pub fn insert(&mut self, key: impl Into<String>, frame: ImageFrame) -> bool {
        let bytes = frame.pixels.len();
        if bytes > self.max_bytes {
            return false;
        }
        let key = key.into();
        let was_active = self.entries.get(&key).is_some_and(|s| s.active);
        self.remove(&key);
        self.clock += 1;
        // Enforce slot count first (respecting the pinned active entry).
        while self.entries.len() >= self.max_slots {
            if !self.evict_lru() {
                break;
            }
        }
        while self.used_bytes + bytes > self.max_bytes {
            if !self.evict_lru() {
                break;
            }
        }
        self.used_bytes += bytes;
        self.entries.insert(
            key,
            Slot {
                frame,
                stamp: self.clock,
                active: was_active,
            },
        );
        true
    }

    /// Evict the least-recently-used **non-active** entry. Returns `false` when
    /// nothing can be evicted (empty, or only the pinned active entry remains).
    fn evict_lru(&mut self) -> bool {
        let lru = self
            .entries
            .iter()
            .filter(|(_, slot)| !slot.active)
            .min_by_key(|(_, slot)| slot.stamp)
            .map(|(key, _)| key.clone());
        match lru {
            Some(key) => {
                self.remove(&key);
                true
            }
            None => false,
        }
    }

    fn remove(&mut self, key: &str) {
        if let Some(slot) = self.entries.remove(key) {
            self.used_bytes -= slot.frame.pixels.len();
        }
    }

    /// Remove `key` from the cache, releasing its byte budget. Used by the GUI
    /// controller to drop a stale preview whose key no longer matches the
    /// current source/recipe (A3) — the stale frame is never served again.
    pub fn remove_entry(&mut self, key: &str) {
        self.remove(key);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    #[must_use]
    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    #[must_use]
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.used_bytes = 0;
    }
}

/// Encode an RGBA8 frame to lossless WebP, preserving the alpha channel.
pub fn encode_webp_lossless(frame: &ImageFrame) -> Result<Vec<u8>, crate::CoreError> {
    use image::codecs::webp::WebPEncoder;
    use image::{ColorType, ImageEncoder};
    use std::io::Cursor;
    let mut out = Cursor::new(Vec::new());
    WebPEncoder::new_lossless(&mut out)
        .write_image(
            &frame.pixels,
            frame.width,
            frame.height,
            ColorType::Rgba8.into(),
        )
        .map_err(|e| crate::CoreError::Encode(e.to_string()))?;
    Ok(out.into_inner())
}

/// Decode a WebP buffer into an RGBA8 frame (alpha preserved).
pub fn decode_webp(bytes: &[u8]) -> Result<ImageFrame, crate::CoreError> {
    ImageFrame::decode(bytes)
}

// ---------------------------------------------------------------------------
// Native disk tier (`.lumina/previews/*.webp`). It touches the file system;
// the RAM LRU + key + window + encode helpers above stay fully portable.
// ---------------------------------------------------------------------------

/// Native on-disk preview tier rooted at `<folder>/.lumina/previews/`.
///
/// Addresses entries by the digest of [`PreviewKey`]. Atomic write via
/// write-to-temp + rename; a file that does not decode (partial write, corrupt)
/// is reported as a miss — never a valid hit.
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
    pub fn load(&self, key_digest: &str) -> Result<Option<ImageFrame>, crate::CoreError> {
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

    // ---- Prefetch window (+4 / −2, no wrap) ----

    #[test]
    fn prefetch_window_is_asymmetric_with_priority_order() {
        // Active image 10 of, say, 40 → full +4/−2 window.
        let slots = prefetch_window(10, 40);
        let indices: Vec<(i64, usize)> = slots.iter().map(|s| (s.offset, s.index)).collect();
        assert_eq!(
            indices,
            vec![(1, 11), (2, 12), (-1, 9), (3, 13), (-2, 8), (4, 14)]
        );
        // Priority rank matches the mandated order.
        let prio: Vec<u8> = slots.iter().map(|s| s.priority).collect();
        assert_eq!(prio, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn prefetch_window_no_wrap_at_start_and_end() {
        // At index 0 there is nothing backwards: only +1..+4 survive.
        let start = prefetch_window(0, 40);
        assert_eq!(
            start.iter().map(|s| s.offset).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        // At the last index only the two backwards neighbors survive (+4…+2 fall
        // off the end) — still no wrap-around to the start.
        let end = prefetch_window(39, 40);
        assert_eq!(
            end.iter().map(|s| s.offset).collect::<Vec<_>>(),
            vec![-1, -2]
        );
        // A window near the start still yields fewer than 6 neighbors.
        let near = prefetch_window(0, 2);
        assert_eq!(near.iter().map(|s| s.offset).collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn prefetch_window_respects_priority_across_single_edge() {
        // active=3, count=8: +1,+2,-1,+3 available, -2,+4 within range too.
        let slots = prefetch_window(3, 8);
        let offsets: Vec<i64> = slots.iter().map(|s| s.offset).collect();
        assert_eq!(offsets, vec![1, 2, -1, 3, -2, 4]);
        // Highest priority first regardless of direction.
        assert_eq!(slots[0].offset, 1);
        assert_eq!(slots[1].offset, 2);
        assert_eq!(slots[2].offset, -1);
    }

    // ---- Cache key (hit / miss / stale) ----

    #[test]
    fn key_digest_differs_on_content_and_render() {
        let a = key_of("content-a", "render-a");
        let a2 = key_of("content-a", "render-a");
        assert_eq!(a.digest(), a2.digest(), "identical keys are a hit");
        assert_ne!(
            a.digest(),
            key_of("content-b", "render-a").digest(),
            "source change → stale (different key)"
        );
        assert_ne!(
            a.digest(),
            key_of("content-a", "render-b").digest(),
            "recipe/render change → stale"
        );
        // Kind/resolution participates.
        let mut b = key_of("content-a", "render-a");
        b.kind = PreviewKind::OneToOne;
        assert_ne!(a.digest(), b.digest(), "kind/resolution change → miss");
    }

    // ---- RAM LRU (eviction, byte budget, active pin) ----

    #[test]
    fn lru_evicts_least_recently_used() {
        let mut cache = LruPreviewCache::new(3, 10_000);
        cache.insert("a", frame(2, 2, 1));
        cache.insert("b", frame(2, 2, 2));
        cache.insert("c", frame(2, 2, 3));
        assert_eq!(cache.len(), 3);
        // Touch "a" so "b" becomes LRU.
        assert!(cache.get("a").is_some());
        cache.insert("d", frame(2, 2, 4));
        assert_eq!(cache.len(), 3);
        assert!(!cache.contains("b"), "'b' must be evicted first");
        assert!(cache.contains("a"));
        assert!(cache.contains("c"));
        assert!(cache.contains("d"));
    }

    #[test]
    fn lru_respects_max_slots_and_byte_budget() {
        let mut cache = LruPreviewCache::new(2, 200);
        cache.insert("a", frame(4, 4, 1)); // 64 bytes
        cache.insert("b", frame(4, 4, 2)); // 64 bytes
        cache.insert("c", frame(4, 4, 3)); // evicts LRU → 2 entries
        assert_eq!(cache.len(), 2);
        assert!(!cache.contains("a"));
        assert!(cache.contains("b"));
        assert!(cache.contains("c"));
        // Oversized entry refused without storing.
        assert!(!cache.insert("huge", frame(10, 10, 0))); // 400 > 200
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn active_entry_is_never_evicted() {
        let mut cache = LruPreviewCache::new(2, 100_000);
        cache.insert("active", frame(2, 2, 7));
        cache.insert("n1", frame(2, 2, 8));
        cache.set_active("active");
        // Fill past capacity; only the non-active entry can be evicted.
        cache.insert("n2", frame(2, 2, 9));
        cache.insert("n3", frame(2, 2, 10));
        assert!(cache.contains("active"), "pinned active must survive");
        assert!(cache.is_active("active"));
        assert_eq!(cache.len(), 2, "capacity is 2: active + 1 non-active");
        assert!(!cache.contains("n1"));
        assert!(!cache.contains("n2"));
        assert!(cache.contains("n3"));
    }

    #[test]
    fn replacing_active_key_keeps_it_pinned() {
        let mut cache = LruPreviewCache::new(2, 100_000);
        cache.insert("active", frame(2, 2, 7));
        cache.set_active("active");
        cache.insert("active", frame(2, 2, 99));
        assert!(cache.is_active("active"));
        assert_eq!(cache.get("active").unwrap().pixels[0], 99);
    }

    // ---- WebP roundtrip (alpha preserved) ----

    #[test]
    fn webp_roundtrip_preserves_alpha_semitransparency() {
        let mut pixels = vec![0u8; 4 * 4 * 4];
        // Give every pixel a distinct RGBA, some semitransparent.
        for (i, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            px.copy_from_slice(&[i as u8, (i * 7) as u8, (i * 13) as u8, (i * 3) as u8]);
        }
        let frame = ImageFrame::new(4, 4, pixels).unwrap();
        let webp = encode_webp_lossless(&frame).unwrap();
        let decoded = decode_webp(&webp).unwrap();
        assert_eq!((decoded.width, decoded.height), (4, 4));
        // Lossless WebP is exact on RGBA8.
        assert_eq!(
            decoded.pixels, frame.pixels,
            "alpha channel preserved exactly"
        );
        // Spot-check a semitransparent pixel.
        assert_eq!(decoded.pixels[7], frame.pixels[7]);
    }

    #[test]
    fn decode_webp_rejects_garbage() {
        assert!(decode_webp(b"not a real webp").is_err());
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
