//! PERF-GUI-1: in-RAM stage-frame cache for interactive rendering.
//!
//! [`StageFrameCache`] holds prepared pipeline-stage products (currently the
//! demosaiced/pre-adjustment **base** frame produced by
//! [`crate::render::prepare_source_base`]) keyed by their stage digest
//! ([`crate::cache::CacheStage::Base`] via
//! `RenderKey::stage_digest`). It is a pure performance layer for the
//! interactive GUI path:
//!
//! - Entries are process-local `ImageFrame`s (no serde, no checksum-on-read);
//!   integrity is guaranteed by construction because only this module writes
//!   them.
//! - Keys are full identity digests, so a hit can never serve pixels from a
//!   different source/decoder/ROI/source-action context. There is therefore no
//!   "stale" state and no fallback semantics: on a miss the caller simply
//!   rebuilds the stage from its inputs.
//! - The cache is byte-budgeted with deterministic LRU eviction; an entry
//!   larger than the whole budget is refused (`insert` returns `false`) and
//!   rendering continues correctly without it — that is a documented capacity
//!   limit, not a silent fallback.
//!
//! Platform-neutral by design (plain RAM), so the wasm32 build keeps
//! compiling. A GPU/VRAM variant is deliberately out of scope here
//! (`lumina-core` must stay GPU-free; see GPU-STAGE-1 / Agents.md).

use crate::ImageFrame;
use std::collections::HashMap;

#[derive(Debug)]
struct Slot {
    frame: ImageFrame,
    stamp: u64,
}

/// Byte-budgeted LRU cache mapping stage digests to RGBA8 frames.
///
/// Clone-on-read: [`Self::get`] hands out an owned copy so the caller can
/// mutate the frame in place (the adjustment passes) without ever poisoning
/// the cached base.
#[derive(Debug)]
pub struct StageFrameCache {
    entries: HashMap<String, Slot>,
    max_bytes: usize,
    used_bytes: usize,
    clock: u64,
}

impl StageFrameCache {
    /// An empty cache holding at most `max_bytes` pixel bytes.
    #[must_use]
    pub fn new(max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_bytes,
            used_bytes: 0,
            clock: 0,
        }
    }

    /// Returns a private copy of the cached frame and marks it most recently
    /// used. Mutating the returned frame cannot affect the cache.
    pub fn get(&mut self, key: &str) -> Option<ImageFrame> {
        let slot = self.entries.get_mut(key)?;
        self.clock += 1;
        slot.stamp = self.clock;
        Some(slot.frame.clone())
    }

    /// Non-bumping readback for diagnostics/tests.
    #[must_use]
    pub fn peek(&self, key: &str) -> Option<&ImageFrame> {
        self.entries.get(key).map(|slot| &slot.frame)
    }

    /// Inserts (or replaces) an entry, evicting least-recently-used entries
    /// until the budget fits. Returns `false` — without storing anything —
    /// when `frame` alone exceeds the configured budget.
    pub fn insert(&mut self, key: impl Into<String>, frame: ImageFrame) -> bool {
        let bytes = frame.pixels.len();
        if bytes > self.max_bytes {
            return false;
        }
        let key = key.into();
        if let Some(previous) = self.entries.remove(&key) {
            self.used_bytes -= previous.frame.pixels.len();
        }
        self.clock += 1;
        while self.used_bytes + bytes > self.max_bytes {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, slot)| slot.stamp)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(evicted) = self.entries.remove(&oldest) {
                self.used_bytes -= evicted.frame.pixels.len();
            }
        }
        self.used_bytes += bytes;
        self.entries.insert(
            key,
            Slot {
                frame,
                stamp: self.clock,
            },
        );
        true
    }

    /// Drops every entry (new source / decode identity).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.used_bytes = 0;
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
    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    #[must_use]
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u32, height: u32, value: u8) -> ImageFrame {
        ImageFrame::new(
            width,
            height,
            vec![value; width as usize * height as usize * 4],
        )
        .unwrap()
    }

    #[test]
    fn hit_returns_independent_copy_and_miss_returns_none() {
        let mut cache = StageFrameCache::new(1024);
        assert!(cache.get("a").is_none());
        assert!(cache.insert("a", frame(4, 4, 7)));
        let mut copy = cache.get("a").unwrap();
        assert_eq!(copy.pixels, vec![7; 64]);
        // A mutated clone must never poison the cached base.
        copy.pixels[0] = 255;
        assert_eq!(cache.peek("a").unwrap().pixels[0], 7);
    }

    #[test]
    fn eviction_is_least_recently_used_and_byte_accounted() {
        let mut cache = StageFrameCache::new(3 * 16);
        assert!(cache.insert("a", frame(2, 2, 1)));
        assert!(cache.insert("b", frame(2, 2, 2)));
        assert!(cache.insert("c", frame(2, 2, 3)));
        assert_eq!(cache.len(), 3);
        // Touch "a" so "b" becomes the LRU entry.
        assert!(cache.get("a").is_some());
        assert!(cache.insert("d", frame(2, 2, 4)));
        assert!(cache.peek("b").is_none(), "'b' must be evicted first");
        assert!(cache.peek("a").is_some());
        assert_eq!(cache.used_bytes(), 3 * 16);

        // Replacing an existing key does not double-account.
        assert!(cache.insert("a", frame(2, 2, 9)));
        assert_eq!(cache.used_bytes(), 3 * 16);
        assert_eq!(cache.peek("a").unwrap().pixels[0], 9);
    }

    #[test]
    fn oversized_entries_are_refused_without_evicting() {
        let mut cache = StageFrameCache::new(16);
        assert!(cache.insert("small", frame(2, 2, 1)));
        assert!(!cache.insert("huge", frame(16, 16, 0)));
        assert!(cache.peek("small").is_some());
        assert_eq!(cache.used_bytes(), 16);
    }

    #[test]
    fn clear_drops_everything() {
        let mut cache = StageFrameCache::new(128);
        assert!(cache.insert("a", frame(2, 2, 1)));
        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.used_bytes(), 0);
        assert!(cache.get("a").is_none());
    }
}
