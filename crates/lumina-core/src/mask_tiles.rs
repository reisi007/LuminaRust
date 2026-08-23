//! 512² mask tile bookkeeping for the interactive brush path (GUI-60FPS-1).
//!
//! Pure, platform-neutral logic shared by the CPU fallback and the GPU present
//! path (`lumina-gpu`). Two pieces:
//!
//! * [`MaskTileGrid`] — per-512²-tile dirty flags. A brush mark only dirties
//!   the tiles its disc touches, so only those tiles are re-stamped/re-uploaded
//!   each frame (the rest of the mask plane is left untouched).
//! * [`stamp_brush_mark`] — single-mark incremental rasterizer whose per-pixel
//!   semantics are byte-identical to the reference brush kernel in
//!   [`crate::masks::rasterize_prompt`] (`MaskPrompt::Brush`): a pixel belongs
//!   to a mark iff its centre `(x+0.5)/w, (y+0.5)/h` lies within the mark's
//!   normalized radius; marks applied in order override earlier ones.
//!
//! No GPU, filesystem or GUI dependency in this module (Agents.md: core stays
//! platform-neutral).

use lumina_sidecar::BrushMarkSign;

/// Tile edge length in pixels for the mask overlay grid. Must stay identical
/// to `lumina_gpu::tiling::TILE_SIZE` (which re-exports this constant).
pub const MASK_TILE_SIZE: u32 = 512;

/// One tile coordinate in the mask tile grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileCoord {
    pub tx: u32,
    pub ty: u32,
}

impl TileCoord {
    /// Pixel bounds of the tile inside a `width × height` plane, clamped so the
    /// right/bottom border tiles never exceed the plane. Returns
    /// `(x0, y0, w, h)` with `w, h >= 1`.
    pub fn bounds(&self, width: u32, height: u32) -> (u32, u32, u32, u32) {
        let x0 = self.tx.min(tiles_across(width) - 1) * MASK_TILE_SIZE;
        let y0 = self.ty.min(tiles_across(height) - 1) * MASK_TILE_SIZE;
        let w = MASK_TILE_SIZE.min(width - x0).max(1);
        let h = MASK_TILE_SIZE.min(height - y0).max(1);
        (x0, y0, w, h)
    }
}

/// Number of tiles needed to cover `length` pixels (>= 1).
pub fn tiles_across(length: u32) -> u32 {
    length.div_ceil(MASK_TILE_SIZE).max(1)
}

/// Per-tile dirty flags for one mask plane of `width × height` pixels.
///
/// The grid itself owns no pixel data; it only tracks *which* 512² tiles must
/// be re-rendered / re-uploaded after brush activity. `take_dirty` drains the
/// set (used by the frame loop: stamp → upload → clear).
#[derive(Debug, Clone)]
pub struct MaskTileGrid {
    width: u32,
    height: u32,
    tiles_x: u32,
    tiles_y: u32,
    dirty: std::collections::BTreeSet<TileCoord>,
}

impl MaskTileGrid {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            tiles_x: tiles_across(width.max(1)),
            tiles_y: tiles_across(height.max(1)),
            dirty: std::collections::BTreeSet::new(),
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn tiles_x(&self) -> u32 {
        self.tiles_x
    }

    pub fn tiles_y(&self) -> u32 {
        self.tiles_y
    }

    pub fn dirty_count(&self) -> usize {
        self.dirty.len()
    }

    pub fn is_dirty(&self, tile: TileCoord) -> bool {
        self.dirty.contains(&tile)
    }

    /// Mark every tile intersecting the pixel-space rectangle
    /// `[x0, x0+w) × [y0, y0+h)` as dirty. Out-of-plane parts are ignored.
    pub fn mark_region_px(&mut self, x0: i64, y0: i64, w: i64, h: i64) {
        if w <= 0 || h <= 0 {
            return;
        }
        let x_end = x0 + w - 1; // inclusive last covered pixel column
        let y_end = y0 + h - 1;
        // Fully outside the plane (left/top or right/bottom) → nothing to do.
        if x_end < 0 || y_end < 0 || x0 >= self.width as i64 || y0 >= self.height as i64 {
            return;
        }
        let clamp_tx = |v: i64| -> u32 { (v.max(0) as u32 / MASK_TILE_SIZE).min(self.tiles_x - 1) };
        let clamp_ty = |v: i64| -> u32 { (v.max(0) as u32 / MASK_TILE_SIZE).min(self.tiles_y - 1) };
        let tx0 = clamp_tx(x0);
        let ty0 = clamp_ty(y0);
        let tx1 = clamp_tx(x_end);
        let ty1 = clamp_ty(y_end);
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                self.dirty.insert(TileCoord { tx, ty });
            }
        }
    }

    /// Dirty the tiles touched by a single brush mark (normalized coordinates,
    /// radius in normalized units — same domain as [`lumina_sidecar::BrushMark`]).
    ///
    /// The bounding square `[x-r, x+r] × [y-r, y+r]` fully contains the mark's
    /// disc, so over-dirtying neighbouring tiles is safe (they are re-stamped
    /// without visual change).
    pub fn mark_brush_mark(&mut self, x: f32, y: f32, radius: f32) {
        if !x.is_finite() || !y.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return;
        }
        let (fw, fh) = (self.width as f32, self.height as f32);
        let px = x * fw;
        let py = y * fh;
        let pr = radius * fw.max(fh);
        self.mark_region_px(
            (px - pr).floor() as i64,
            (py - pr).floor() as i64,
            (2.0 * pr).ceil() as i64,
            (2.0 * pr).ceil() as i64,
        );
    }

    /// Drain the dirty set: returns all dirty tiles in row-major order and
    /// clears the flags.
    pub fn take_dirty(&mut self) -> Vec<TileCoord> {
        let drained: Vec<TileCoord> = self.dirty.iter().copied().collect();
        self.dirty.clear();
        drained
    }

    /// Mark every tile dirty (e.g. after a full-plane invalidation such as an
    /// image or resolution change).
    pub fn mark_all(&mut self) {
        for ty in 0..self.tiles_y {
            for tx in 0..self.tiles_x {
                self.dirty.insert(TileCoord { tx, ty });
            }
        }
    }

    pub fn clear(&mut self) {
        self.dirty.clear();
    }
}

/// Stamp a single brush mark into `values` (row-major `u16`, `width × height`)
/// using exactly the same pixel test as the [`MaskPrompt::Brush`] branch of
/// [`crate::masks::rasterize_prompt`]: later marks override earlier ones, a
/// negative mark paints 0, a positive mark paints `u16::MAX`, and a pixel is
/// covered by the mark when its centre lies within the normalized radius.
///
/// Only the pixels inside the mark's disc are touched, so callers can update a
/// persistent mask plane incrementally instead of re-rasterizing the whole
/// prompt per frame.
///
/// Returns the pixel-space bounding box actually written
/// `(x0, y0, w, h)` (empty box `(0, 0, 0, 0)` for non-finite/degenerate input).
pub fn stamp_brush_mark(
    values: &mut [u16],
    width: u32,
    height: u32,
    x: f32,
    y: f32,
    radius: f32,
    sign: BrushMarkSign,
) -> (u32, u32, u32, u32) {
    let empty = (0u32, 0u32, 0u32, 0u32);
    if !x.is_finite() || !y.is_finite() || !radius.is_finite() || radius <= 0.0 {
        return empty;
    }
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || values.len() < w.saturating_mul(h) {
        return empty;
    }
    let (fw, fh) = (width as f32, height as f32);
    // Pixel-space bounding box of the disc (clamped to the plane).
    let cx = x * fw;
    let cy = y * fh;
    // The disc is defined in normalized space; its aspect on screen follows the
    // larger dimension so the test below uses per-axis radii derived from the
    // same normalization the reference kernel performs implicitly via nx/ny.
    let rx = radius * fw;
    let ry = radius * fh;
    let x0 = ((cx - rx).floor().max(0.0)) as usize;
    let y0 = ((cy - ry).floor().max(0.0)) as usize;
    let x1 = ((cx + rx).ceil().min(fw)) as usize;
    let y1 = ((cy + ry).ceil().min(fh)) as usize;
    if x0 >= x1 || y0 >= y1 {
        return empty;
    }
    let value = if matches!(sign, BrushMarkSign::Positive) {
        u16::MAX
    } else {
        0
    };
    let r_sq = radius * radius;
    for py in y0..y1 {
        let ny = (py as f32 + 0.5) / fh;
        for px in x0..x1 {
            let nx = (px as f32 + 0.5) / fw;
            let ddx = nx - x;
            let ddy = ny - y;
            if ddx * ddx + ddy * ddy <= r_sq {
                values[py * w + px] = value;
            }
        }
    }
    (x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_sidecar::{BrushMark, MaskPrompt};

    fn mark(x: f32, y: f32, radius: f32, sign: BrushMarkSign) -> BrushMark {
        BrushMark { x, y, radius, sign }
    }

    #[test]
    fn tiles_across_covers_edges() {
        assert_eq!(tiles_across(1), 1);
        assert_eq!(tiles_across(512), 1);
        assert_eq!(tiles_across(513), 2);
        assert_eq!(tiles_across(1024), 2);
    }

    #[test]
    fn tile_bounds_clamp_to_plane() {
        // 600×300 plane → 2×1 tiles; the border tile is only 88px wide.
        let grid = MaskTileGrid::new(600, 300);
        assert_eq!((grid.tiles_x(), grid.tiles_y()), (2, 1));
        let (x, y, w, h) = TileCoord { tx: 1, ty: 0 }.bounds(600, 300);
        assert_eq!((x, y, w, h), (512, 0, 88, 300));
    }

    #[test]
    fn brush_mark_dirties_expecting_tiles() {
        let mut grid = MaskTileGrid::new(1024, 1024);
        // Centre mark touches only tile (0,0)'s neighbourhood? At (100,100) px
        // with radius 50 px it stays inside tile (0,0).
        grid.mark_brush_mark(100.0 / 1024.0, 100.0 / 1024.0, 50.0 / 1024.0);
        assert_eq!(grid.dirty_count(), 1);
        assert!(grid.is_dirty(TileCoord { tx: 0, ty: 0 }));

        // A mark spanning the 512 boundary must dirty both columns.
        grid.clear();
        grid.mark_brush_mark(512.0 / 1024.0, 100.0 / 1024.0, 64.0 / 1024.0);
        assert!(grid.is_dirty(TileCoord { tx: 0, ty: 0 }));
        assert!(grid.is_dirty(TileCoord { tx: 1, ty: 0 }));
        assert_eq!(grid.dirty_count(), 2);

        // take_dirty drains and clears.
        let drained = grid.take_dirty();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0], TileCoord { tx: 0, ty: 0 }); // row-major order
        assert_eq!(grid.dirty_count(), 0);
        assert_eq!(grid.take_dirty(), Vec::new());
    }

    #[test]
    fn degenerate_brush_mark_dirties_nothing() {
        let mut grid = MaskTileGrid::new(512, 512);
        grid.mark_brush_mark(f32::NAN, 0.5, 0.1);
        grid.mark_brush_mark(0.5, 0.5, 0.0);
        grid.mark_brush_mark(0.5, 0.5, -1.0);
        assert_eq!(grid.dirty_count(), 0);
    }

    #[test]
    fn mark_all_and_out_of_plane_regions() {
        let mut grid = MaskTileGrid::new(600, 300);
        grid.mark_all();
        assert_eq!(grid.dirty_count(), 2);
        // Fully out-of-plane region dirties nothing (and does not panic).
        grid.take_dirty();
        grid.mark_region_px(-10_000, -10_000, 5_000, 5_000);
        assert_eq!(grid.dirty_count(), 0);
        // A region overlapping the plane from the left dirties only the
        // intersecting tiles (clamped, no panic); it spans both columns.
        grid.mark_region_px(-100, -100, 700, 700);
        assert_eq!(grid.dirty_count(), 2);
    }

    /// The incremental single-mark stamp must produce byte-identical results
    /// to the reference full-plane brush rasterizer when replaying the same
    /// marks in the same order onto a zeroed plane.
    #[test]
    fn incremental_stamp_matches_reference_kernel() {
        let marks = [
            mark(0.30, 0.30, 0.12, BrushMarkSign::Positive),
            mark(0.38, 0.34, 0.10, BrushMarkSign::Positive),
            mark(0.34, 0.32, 0.05, BrushMarkSign::Negative),
            mark(0.90, 0.85, 0.20, BrushMarkSign::Positive),
        ];
        let (w, h) = (320u32, 240u32);

        let prompt = MaskPrompt::Brush {
            marks: marks.to_vec(),
            resolution: (w, h),
            transformation: lumina_sidecar::PromptTransform::default(),
        };
        let reference = crate::masks::rasterize_prompt(&prompt, w, h).unwrap();

        let mut incremental = vec![0u16; (w * h) as usize];
        let mut total_bbox = (0u32, 0u32, 0u32, 0u32);
        for m in &marks {
            let bbox = stamp_brush_mark(&mut incremental, w, h, m.x, m.y, m.radius, m.sign);
            // Union of bboxes for coverage sanity (not asserted precisely).
            total_bbox.2 += bbox.2;
            total_bbox.3 += bbox.3;
        }
        let _ = total_bbox;

        assert_eq!(
            reference.values, incremental,
            "incremental stamp diverges from the reference brush kernel"
        );
    }

    #[test]
    fn stamp_returns_empty_for_degenerate_input() {
        let mut values = vec![0u16; 16];
        assert_eq!(
            stamp_brush_mark(
                &mut values,
                4,
                4,
                f32::INFINITY,
                0.5,
                0.1,
                BrushMarkSign::Positive
            ),
            (0, 0, 0, 0)
        );
        assert_eq!(
            stamp_brush_mark(&mut values, 4, 4, 0.5, 0.5, -0.1, BrushMarkSign::Positive),
            (0, 0, 0, 0)
        );
        // Buffer too small → no panic, empty result.
        assert_eq!(
            stamp_brush_mark(
                &mut values[..2],
                4,
                4,
                0.5,
                0.5,
                0.1,
                BrushMarkSign::Positive
            ),
            (0, 0, 0, 0)
        );
    }
}
