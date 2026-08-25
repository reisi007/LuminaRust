//! Tiling + LRU atlas + draft pyramid for GPU frame rendering.
//!
//! Splits a (potentially very large, e.g. 45 MP) source into 512² tiles kept
//! resident in VRAM via an LRU atlas, and derives the tile set covering a given
//! viewport/ROI through the draft (mip) pyramid. Parallel subagents fill in the
//! actual GPU upload/dispatch that consumes these tiles.
//!
//! # Edit generations (REVIEW-GPU-TILEVER-1)
//!
//! Every cached tile is stamped with the *edit generation* it was rendered
//! from (see [`TileKey::generation`]). A cache hit can therefore never return
//! pixels from a previous edit state: advancing the cache's generation via
//! [`TiledCache::set_generation`] drops all tiles of older generations, and any
//! leftover key from an old generation misses by construction because the
//! generation participates in the key's equality/hash identity.
//!
//! # Level selection (REVIEW-GPU-LEVELS-1)
//!
//! The pyramid level for a viewport is chosen **exclusively** through
//! [`DraftPyramid::level_for_zoom`] (which clamps to the pyramid's
//! `max_level`). There is deliberately no second, unclamped copy of that
//! formula: [`TiledCache::keys_for_viewport`] takes the pyramid as a parameter
//! so the ROI→tile expansion and the renderer can never disagree on the level.

use std::collections::{HashMap, VecDeque};
use wgpu::Texture;

/// Tile edge length in source pixels at pyramid level 0 (full resolution).
///
/// Kept identical to the platform-neutral [`lumina_core::mask_tiles::MASK_TILE_SIZE`]
/// so CPU-side dirty bookkeeping and GPU tile addressing always agree.
pub const TILE_SIZE: u32 = lumina_core::mask_tiles::MASK_TILE_SIZE;

/// A normalized region of interest in source-pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Viewport {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// Stable key for one cache tile: pyramid `level`, tile indices `(tx, ty)` and
/// the edit `generation` the tile was rendered from.
///
/// Two keys with equal `(level, tx, ty)` but different generations denote
/// **different** cache entries — a tile rendered before an edit must never
/// serve a lookup after it (REVIEW-GPU-TILEVER-1). Use [`TileKey::new`] for the
/// initial (unedited) state and [`TileKey::with_generation`] to stamp a
/// specific one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub level: u32,
    pub tx: u32,
    pub ty: u32,
    /// Edit generation this tile belongs to. `0` is the initial, unedited
    /// state; callers bump the counter whenever the rendered edit state changes
    /// (recipe edit, mask-stroke commit, …).
    pub generation: u64,
}

impl TileKey {
    /// The initial edit generation ([`TileKey::new`] stamps this value).
    pub const INITIAL_GENERATION: u64 = 0;

    /// Create a key in the initial (generation `0`) edit state.
    pub fn new(level: u32, tx: u32, ty: u32) -> Self {
        Self {
            level,
            tx,
            ty,
            generation: Self::INITIAL_GENERATION,
        }
    }

    /// Stamp this key with an explicit edit generation.
    pub fn with_generation(mut self, generation: u64) -> Self {
        self.generation = generation;
        self
    }
}

/// Key/LRU/edit-generation bookkeeping of [`TiledCache`].
///
/// Generic over the stored resource handle so the invalidation policy is fully
/// unit-testable without a real `wgpu::Device` (tests use `CacheCore<u8>`).
struct CacheCore<T> {
    capacity: usize,
    generation: u64,
    order: VecDeque<TileKey>,
    tiles: HashMap<TileKey, T>,
}

impl<T> CacheCore<T> {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            generation: TileKey::INITIAL_GENERATION,
            order: VecDeque::new(),
            tiles: HashMap::new(),
        }
    }

    fn generation(&self) -> u64 {
        self.generation
    }

    /// Advance the edit generation and purge every tile that was rendered from
    /// a different one. Returns the number of purged entries so callers can
    /// surface the invalidation instead of swallowing it.
    fn set_generation(&mut self, generation: u64) -> usize {
        self.generation = generation;
        let stale: Vec<TileKey> = self
            .tiles
            .keys()
            .copied()
            .filter(|key| key.generation != generation)
            .collect();
        for key in &stale {
            self.tiles.remove(key);
        }
        self.order.retain(|key| key.generation == generation);
        stale.len()
    }

    fn contains(&self, key: &TileKey) -> bool {
        self.tiles.contains_key(key)
    }

    fn get(&self, key: &TileKey) -> Option<&T> {
        self.tiles.get(key)
    }

    fn insert(&mut self, key: TileKey, value: T) {
        if let Some(old) = self.tiles.insert(key, value) {
            drop(old);
        }
        self.order.retain(|k| *k != key);
        self.order.push_front(key);
        self.evict();
    }

    fn touch(&mut self, key: &TileKey) {
        if self.tiles.contains_key(key) {
            self.order.retain(|k| k != key);
            self.order.push_front(*key);
        }
    }

    fn len(&self) -> usize {
        self.tiles.len()
    }

    fn evict(&mut self) {
        while self.order.len() > self.capacity {
            if let Some(evict_key) = self.order.pop_back() {
                self.tiles.remove(&evict_key);
            } else {
                break;
            }
        }
    }
}

/// LRU atlas of VRAM tile textures.
///
/// Keeps at most `capacity` decoded/base tiles resident; least-recently-used
/// entries are evicted (their `wgpu::Texture` is dropped) when the limit is
/// exceeded. The cache owns the VRAM handles, so callers must not retain texture
/// references across eviction.
///
/// # Edit-generation contract (REVIEW-GPU-TILEVER-1)
///
/// The cache carries an edit generation (initially
/// [`TileKey::INITIAL_GENERATION`]). Callers must bump it with
/// [`TiledCache::set_generation`] whenever the rendered edit state changes; that
/// call drops every tile from a different generation instead of silently
/// re-serving it. Keys produced by [`TiledCache::keys_for_viewport`] are stamped
/// with the current generation automatically; hand-built keys can be stamped via
/// [`TileKey::with_generation`]. Because the generation is part of the hash key,
/// a stale lookup can never alias a fresh entry even between purge points.
pub struct TiledCache {
    core: CacheCore<Texture>,
}

impl TiledCache {
    /// Create an empty cache that retains at most `capacity` tiles.
    pub fn new(capacity: usize) -> Self {
        Self {
            core: CacheCore::new(capacity),
        }
    }

    /// The edit generation newly rendered tiles belong to.
    pub fn generation(&self) -> u64 {
        self.core.generation()
    }

    /// Advance the edit generation to `generation` and drop every cached tile
    /// that was rendered from a different one.
    ///
    /// Returns the number of purged entries so callers can log/report the
    /// invalidation loudly rather than leaving it implicit (Agents.md: no silent
    /// fallbacks).
    pub fn set_generation(&mut self, generation: u64) -> usize {
        self.core.set_generation(generation)
    }

    /// Number of resident tiles.
    pub fn len(&self) -> usize {
        self.core.len()
    }

    /// Whether no tile is resident.
    pub fn is_empty(&self) -> bool {
        self.core.len() == 0
    }

    pub fn contains(&self, key: &TileKey) -> bool {
        self.core.contains(key)
    }

    pub fn get(&self, key: &TileKey) -> Option<&Texture> {
        self.core.get(key)
    }

    /// Insert (or replace) a tile texture and mark it most-recently-used.
    /// Evicts the LRU entry if `capacity` is exceeded.
    ///
    /// The caller is responsible for stamping `key` with the cache's current
    /// [`TiledCache::generation`] (keys from [`Self::keys_for_viewport`] already
    /// carry it); inserting an outdated generation would reintroduce exactly the
    /// staleness this cache exists to prevent.
    pub fn insert(&mut self, key: TileKey, texture: Texture) {
        self.core.insert(key, texture);
    }

    /// Mark a key as recently used (move to front) without inserting.
    pub fn touch(&mut self, key: &TileKey) {
        self.core.touch(key);
    }

    /// Compute the set of tile keys covering `viewport` at the given `zoom`.
    ///
    /// `zoom > 1` means zoomed in (source shown larger than the viewport) →
    /// full-res tiles (level 0). `zoom < 1` means zoomed out → a lower-resolution
    /// pyramid level is selected so each tile still covers `TILE_SIZE` on-screen
    /// pixels. This is the ROI → tile-set expansion used by
    /// [`super::GpuContext::render_draft`].
    ///
    /// REVIEW-GPU-LEVELS-1: `pyramid` is an explicit parameter and the level is
    /// taken verbatim from [`DraftPyramid::level_for_zoom`], so the selection —
    /// including its clamp against the pyramid's `max_level` — has a single
    /// source of truth shared with the renderer. Previously this method
    /// duplicated the formula unclamped and could address levels beyond
    /// `max_level` for extreme zoom-outs.
    ///
    /// Every returned key is stamped with the cache's current edit generation,
    /// so the result can be fed straight into [`Self::contains`]/[`Self::get`]
    /// without stale hits.
    pub fn keys_for_viewport(
        &self,
        pyramid: &DraftPyramid,
        viewport: &Viewport,
        zoom: f32,
    ) -> Vec<TileKey> {
        let level = pyramid.level_for_zoom(zoom);
        let zoom = zoom.max(1e-3);
        let source_w = viewport.width / zoom;
        let source_h = viewport.height / zoom;
        let tile_px = (TILE_SIZE << level) as f32;
        let x0 = (viewport.x / tile_px).floor() as i64;
        let y0 = (viewport.y / tile_px).floor() as i64;
        let x1 = ((viewport.x + source_w) / tile_px).ceil() as i64;
        let y1 = ((viewport.y + source_h) / tile_px).ceil() as i64;
        let mut keys = Vec::new();
        for ty in y0..y1 {
            for tx in x0..x1 {
                if tx < 0 || ty < 0 {
                    continue;
                }
                keys.push(
                    TileKey::new(level, tx as u32, ty as u32)
                        .with_generation(self.core.generation()),
                );
            }
        }
        keys
    }
}

/// Draft (mip) pyramid descriptor for fast interactive preview.
///
/// Each level `l` is the source downsampled by `2^l`. The renderer picks the
/// coarsest level that still covers the viewport (see
/// [`TiledCache::keys_for_viewport`]) while a slider is dragged, then swaps to
/// full-res on commit.
#[derive(Debug, Clone, Copy)]
pub struct DraftPyramid {
    pub base_width: u32,
    pub base_height: u32,
    pub max_level: u32,
}

impl DraftPyramid {
    pub fn new(base_width: u32, base_height: u32) -> Self {
        let max_level = (base_width.max(base_height) as f32).log2().floor().max(0.0) as u32;
        Self {
            base_width,
            base_height,
            max_level,
        }
    }

    /// Dimensions of pyramid level `l` (clamped to >= 1px).
    pub fn level_dimensions(&self, level: u32) -> (u32, u32) {
        let shift = level.min(self.max_level);
        (
            (self.base_width >> shift).max(1),
            (self.base_height >> shift).max(1),
        )
    }

    /// Coarsest pyramid level whose on-screen coverage still satisfies `zoom`.
    ///
    /// This is the **single source of truth** for level selection
    /// (REVIEW-GPU-LEVELS-1): the raw `floor(log2(1/zoom))` estimate is clamped
    /// to `max_level`, and every consumer — including
    /// [`TiledCache::keys_for_viewport`] — must go through this method instead of
    /// re-deriving (and possibly unclamping) the formula.
    pub fn level_for_zoom(&self, zoom: f32) -> u32 {
        let zoom = zoom.max(1e-3);
        (((1.0 / zoom).max(1.0)).log2().floor().max(0.0) as u32).min(self.max_level)
    }
}

/// Compute which 512² mask tiles a normalized brush mark touches and which are dirty.
///
/// `mark` is in normalized `0..=1` source space with `radius` also normalized.
/// Returns the set of `TileKey`s intersecting the mark's bounding square.
pub fn dirty_tiles_for_brush_mark(
    mark_x: f32,
    mark_y: f32,
    radius: f32,
    image_width: u32,
    image_height: u32,
) -> Vec<TileKey> {
    let rw = (radius * image_width as f32).ceil().max(1.0) as u32;
    let rh = (radius * image_height as f32).ceil().max(1.0) as u32;
    let cx = (mark_x * image_width as f32) as i32;
    let cy = (mark_y * image_height as f32) as i32;
    let x0 = (cx - rw as i32).max(0) as u32;
    let y0 = (cy - rh as i32).max(0) as u32;
    let x1 = ((cx + rw as i32).min(image_width as i32 - 1).max(0) as u32) + 1;
    let y1 = ((cy + rh as i32).min(image_height as i32 - 1).max(0) as u32) + 1;
    let tx0 = x0 / TILE_SIZE;
    let ty0 = y0 / TILE_SIZE;
    let tx1 = (x1 - 1) / TILE_SIZE;
    let ty1 = (y1 - 1) / TILE_SIZE;
    let mut out = Vec::new();
    for ty in ty0..=ty1 {
        for tx in tx0..=tx1 {
            // Mask tiles live in source space (level 0) and are invalidated by
            // the explicit mask-upload path, so they stay in the initial edit
            // generation.
            out.push(TileKey::new(0, tx, ty));
        }
    }
    out
}

/// Return the union of dirty tiles for a slice of brush marks.
pub fn dirty_tiles_for_marks(
    marks: &[(f32, f32, f32)],
    image_width: u32,
    image_height: u32,
) -> Vec<TileKey> {
    let mut seen = std::collections::BTreeSet::new();
    for (x, y, r) in marks {
        for k in dirty_tiles_for_brush_mark(*x, *y, *r, image_width, image_height) {
            seen.insert((k.level, k.tx, k.ty));
        }
    }
    seen.into_iter()
        .map(|(level, tx, ty)| TileKey::new(level, tx, ty))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REVIEW-GPU-TILEVER-1: the edit generation is part of a key's identity —
    /// same tile coordinates before/after an edit are different entries.
    #[test]
    fn tile_key_distinguishes_edit_generations() {
        let initial = TileKey::new(2, 3, 4);
        assert_eq!(initial.generation, TileKey::INITIAL_GENERATION);

        let edited = initial.with_generation(1);
        assert_ne!(initial, edited);
        assert_eq!(edited.generation, 1);
        assert_eq!(edited.with_generation(TileKey::INITIAL_GENERATION), initial);

        // Hash identity follows equality: both keys can coexist in one map.
        let mut map = HashMap::new();
        map.insert(initial, "before");
        map.insert(edited, "after");
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&initial), Some(&"before"));
        assert_eq!(map.get(&edited), Some(&"after"));
    }

    /// REVIEW-GPU-TILEVER-1: a cache lookup must never cross an edit boundary —
    /// same `(level, tx, ty)` at a different generation misses by construction.
    #[test]
    fn cache_lookup_never_crosses_edit_generations() {
        let mut core: CacheCore<u8> = CacheCore::new(8);
        let old = TileKey::new(0, 1, 1);
        core.insert(old, 42);
        assert!(core.contains(&old));

        let purged = core.set_generation(1);
        assert_eq!(purged, 1, "the stale tile must be reported as purged");
        assert_eq!(core.len(), 0);
        assert!(!core.contains(&old));
        assert!(core.get(&old).is_none());

        // Re-render the same coordinates at the new generation: the fresh entry
        // hits while the stale key still misses (generation is hashed).
        let fresh = TileKey::new(0, 1, 1).with_generation(1);
        core.insert(fresh, 7);
        assert_eq!(core.get(&fresh), Some(&7));
        assert!(!core.contains(&old));
    }

    /// Purging keeps tiles that already belong to the new generation.
    #[test]
    fn set_generation_keeps_current_generation_tiles() {
        let mut core: CacheCore<u8> = CacheCore::new(8);
        core.insert(TileKey::new(1, 0, 0).with_generation(3), 1);
        core.insert(TileKey::new(1, 1, 0).with_generation(3), 2);
        core.insert(TileKey::new(1, 2, 0).with_generation(9), 3);

        let purged = core.set_generation(3);
        assert_eq!(purged, 1);
        assert_eq!(core.generation(), 3);
        assert_eq!(core.len(), 2);
        assert!(core.contains(&TileKey::new(1, 0, 0).with_generation(3)));
        assert!(core.contains(&TileKey::new(1, 1, 0).with_generation(3)));
        assert!(!core.contains(&TileKey::new(1, 2, 0).with_generation(9)));
    }

    /// The LRU capacity stays enforced across generation bumps.
    #[test]
    fn lru_capacity_is_respected_across_generations() {
        let mut core: CacheCore<u8> = CacheCore::new(2);
        core.insert(TileKey::new(0, 0, 0), 10);
        core.insert(TileKey::new(0, 1, 0), 11);
        core.touch(&TileKey::new(0, 0, 0)); // make (0,1,0) the LRU entry
        core.insert(TileKey::new(0, 2, 0), 12);
        assert_eq!(core.len(), 2);
        assert!(core.contains(&TileKey::new(0, 0, 0)));
        assert!(!core.contains(&TileKey::new(0, 1, 0)));

        // A generation bump purges; afterwards insertion refills up to the cap.
        core.set_generation(1);
        assert_eq!(core.len(), 0);
        for tx in 0..3u32 {
            core.insert(TileKey::new(0, tx, 0).with_generation(1), tx as u8);
        }
        assert_eq!(core.len(), 2);
    }

    /// REVIEW-GPU-LEVELS-1: the viewport expansion uses `level_for_zoom`
    /// verbatim — clamped to `max_level` — and stamps the cache's generation.
    #[test]
    fn keys_for_viewport_clamps_level_and_stamps_generation() {
        let pyramid = DraftPyramid::new(128, 128);
        assert_eq!(pyramid.max_level, 7);

        let mut cache = TiledCache::new(4);
        cache.set_generation(6);
        let viewport = Viewport::new(0.0, 0.0, 100.0, 100.0);

        // Extreme zoom-out: the previously duplicated unclamped formula would
        // have selected floor(log2(1000)) = 9 > max_level.
        let keys = cache.keys_for_viewport(&pyramid, &viewport, 1e-3);
        assert!(!keys.is_empty());
        for key in &keys {
            assert_eq!(key.level, pyramid.level_for_zoom(1e-3));
            assert!(key.level <= pyramid.max_level);
            assert_eq!(key.generation, 6);
        }

        // Sweep: expansion and renderer level always agree, and deeper zoom-out
        // never selects a *higher* (coarser) level than an earlier one.
        let mut prev_level = pyramid.max_level;
        for &zoom in &[0.001f32, 0.01, 0.05, 0.125, 0.25, 0.5, 1.0, 2.0, 8.0] {
            let keys = cache.keys_for_viewport(&pyramid, &viewport, zoom);
            assert!(!keys.is_empty(), "zoom {zoom} must cover the viewport");
            let level = keys[0].level;
            assert!(
                keys.iter().all(|k| k.level == level),
                "all keys of one viewport share the pyramid level"
            );
            assert_eq!(
                level,
                pyramid.level_for_zoom(zoom),
                "expansion and level_for_zoom must agree at zoom {zoom}"
            );
            assert!(level <= prev_level, "level must be non-increasing in zoom");
            prev_level = level;
            for key in &keys {
                assert_eq!(key.generation, 6, "keys carry the cache generation");
            }
        }
    }

    /// Brush-mark tiles stay level-0/initial-generation keys (mask uploads are
    /// invalidated explicitly, not via the render-generation counter).
    #[test]
    fn dirty_tiles_use_initial_generation_at_level_zero() {
        let tiles = dirty_tiles_for_brush_mark(0.5, 0.5, 0.1, 1024, 1024);
        assert!(!tiles.is_empty());
        for key in &tiles {
            assert_eq!(key.level, 0);
            assert_eq!(key.generation, TileKey::INITIAL_GENERATION);
        }

        let marks = vec![(0.25f32, 0.75f32, 0.05f32), (0.9, 0.1, 0.2)];
        let union = dirty_tiles_for_marks(&marks, 512, 512);
        assert!(!union.is_empty());
        assert!(union.windows(2).all(|pair| pair[0] != pair[1]));
        for key in &union {
            assert_eq!(key.level, 0);
            assert_eq!(key.generation, TileKey::INITIAL_GENERATION);
        }
    }
}
