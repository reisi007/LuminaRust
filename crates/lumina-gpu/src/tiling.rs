//! Tiling + LRU atlas + draft pyramid for GPU frame rendering.
//!
//! Splits a (potentially very large, e.g. 45 MP) source into 512² tiles kept
//! resident in VRAM via an LRU atlas, and derives the tile set covering a given
//! viewport/ROI through the draft (mip) pyramid. Parallel subagents fill in the
//! actual GPU upload/dispatch that consumes these tiles.

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

/// Stable key for one cache tile: pyramid `level` and tile indices `(tx, ty)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub level: u32,
    pub tx: u32,
    pub ty: u32,
}

/// LRU atlas of VRAM tile textures.
///
/// Keeps at most `capacity` decoded/base tiles resident; least-recently-used
/// entries are evicted (their `wgpu::Texture` is dropped) when the limit is
/// exceeded. The cache owns the VRAM handles, so callers must not retain texture
/// references across eviction.
pub struct TiledCache {
    capacity: usize,
    order: VecDeque<TileKey>,
    tiles: HashMap<TileKey, Texture>,
}

impl TiledCache {
    /// Create an empty cache that retains at most `capacity` tiles.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            order: VecDeque::new(),
            tiles: HashMap::new(),
        }
    }

    pub fn contains(&self, key: &TileKey) -> bool {
        self.tiles.contains_key(key)
    }

    pub fn get(&self, key: &TileKey) -> Option<&Texture> {
        self.tiles.get(key)
    }

    /// Insert (or replace) a tile texture and mark it most-recently-used.
    /// Evicts the LRU entry if `capacity` is exceeded.
    pub fn insert(&mut self, key: TileKey, texture: Texture) {
        if let Some(old) = self.tiles.insert(key, texture) {
            drop(old);
        }
        self.order.retain(|k| *k != key);
        self.order.push_front(key);
        self.evict();
    }

    /// Mark a key as recently used (move to front) without inserting.
    pub fn touch(&mut self, key: &TileKey) {
        if self.tiles.contains_key(key) {
            self.order.retain(|k| k != key);
            self.order.push_front(*key);
        }
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

    /// Compute the set of tile keys covering `viewport` at the given `zoom`.
    ///
    /// `zoom > 1` means zoomed in (source shown larger than the viewport) →
    /// full-res tiles (level 0). `zoom < 1` means zoomed out → a lower-resolution
    /// pyramid level is selected so each tile still covers `TILE_SIZE` on-screen
    /// pixels. This is the ROI → tile-set expansion used by
    /// [`super::GpuContext::render_draft`].
    pub fn keys_for_viewport(&self, viewport: &Viewport, zoom: f32) -> Vec<TileKey> {
        let zoom = zoom.max(1e-3);
        let source_w = viewport.width / zoom;
        let source_h = viewport.height / zoom;
        // Coarsest pyramid level whose tiles are still ~TILE_SIZE on screen.
        let level = ((1.0 / zoom).max(1.0)).log2().floor().max(0.0) as u32;
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
                keys.push(TileKey {
                    level,
                    tx: tx as u32,
                    ty: ty as u32,
                });
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
            out.push(TileKey { level: 0, tx, ty });
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
        .map(|(level, tx, ty)| TileKey { level, tx, ty })
        .collect()
}
