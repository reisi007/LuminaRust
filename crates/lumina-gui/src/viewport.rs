//! GUI-SCROLL-200-1: pure virtualization and thumbnail-priority helpers.
//!
//! These helpers let the Library grid, the filmstrip and the Develop navigator
//! rail touch **only** the cells that are (or will shortly be) visible:
//!
//! 1. [`visible_cell_range`] converts a scroll-area viewport (in content
//!    pixels) into the range of fixed-size cells that intersect it.
//! 2. [`buffered_range`] widens such a range by a small ring of cells so a
//!    fast scroll never shows empty placeholders at the edges.
//! 3. [`prefetch_order`] lists the *off-screen* indices sorted by distance to
//!    the visible window, so background thumbnail jobs can be enqueued
//!    nearest-first with a hard per-frame budget instead of running an O(n)
//!    loop over every entry every frame.
//!
//! All functions are pure and platform-independent so they can be unit-tested
//! headless. Nothing here touches egui state, file systems or sidecars.

/// Ring of extra cells around the visible window whose thumbnails are treated
/// as visible (enqueued immediately, no budget). Kept small on purpose: it is
/// measured in cells, not rows, and must not re-create an unbounded queue.
pub const VISIBLE_BUFFER_CELLS: usize = 8;

/// Maximum off-screen (prefetch) entries probed/enqueued per frame. This bounds
/// the worst-case per-frame disk-cache probes (`DiskFolderCache`) on the UI
/// thread; at 60 fps a 200-image folder is fully prefetched in under two
/// seconds of idle time.
pub const PREFETCH_BUDGET_PER_FRAME: usize = 4;

/// Range of cell indices whose thumbnails count as "visible" for scheduling.
pub type CellWindow = std::ops::Range<usize>;

/// Convert a scroll viewport into the range of fixed-size cells that intersect
/// it.
///
/// * `viewport_min_px` — left/top edge of the viewport in content coordinates
///   (as reported by `egui::ScrollArea::show_viewport`, where `0` means fully
///   scrolled to the start).
/// * `viewport_len_px` — width/height of the viewport in content pixels.
/// * `step_px` — distance between consecutive cell origins (cell size plus one
///   item-spacing gap). Must be positive for meaningful output.
///
/// The returned range is always clamped to `0..count`; degenerate inputs
/// (zero cells, non-positive step/viewport) yield an empty range.
pub fn visible_cell_range(
    viewport_min_px: f32,
    viewport_len_px: f32,
    step_px: f32,
    count: usize,
) -> CellWindow {
    if count == 0 || !step_px.is_finite() || step_px <= 0.0 || viewport_len_px <= 0.0 {
        return 0..0;
    }
    // Float-to-int casts saturate in Rust, but clamp explicitly against
    // negative scroll positions so the intent stays obvious.
    let first = ((viewport_min_px / step_px).floor().max(0.0)) as usize;
    let last_excl = (((viewport_min_px + viewport_len_px) / step_px)
        .ceil()
        .max(0.0)) as usize;
    let first = first.min(count);
    let last = last_excl.max(first).min(count);
    first..last
}

/// Widen a cell window by `buffer_cells` on both sides, clamped to `0..count`.
pub fn buffered_range(visible: CellWindow, count: usize, buffer_cells: usize) -> CellWindow {
    let start = visible.start.saturating_sub(buffer_cells);
    let end = visible.end.saturating_add(buffer_cells).min(count);
    start..end.max(start)
}

/// Off-screen indices, nearest to the visible window first.
///
/// The walk alternates between the right and the left edge of the window, so
/// cells adjacent to either edge are prefetched before distant ones. When both
/// a left and a right candidate are equidistant, the right one comes first
/// (documented tie-break; scrolling forward is the common case).
///
/// An empty visible window (`start == end`) prefetches from its position
/// outward — e.g. `0..0` yields `0, 1, 2, …`.
pub fn prefetch_order(count: usize, visible: CellWindow) -> Vec<usize> {
    let mut order = Vec::new();
    // `left` is the next candidate below the window (exclusive),
    // `right` the next candidate above it.
    let mut left = visible.start.min(count);
    let mut right = visible.end.max(left).min(count);
    while left > 0 || right < count {
        if right < count {
            order.push(right);
            right += 1;
        }
        if left > 0 {
            left -= 1;
            order.push(left);
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_cell_range_at_scroll_start() {
        assert_eq!(visible_cell_range(0.0, 500.0, 100.0, 200), 0..5);
    }

    #[test]
    fn visible_cell_range_mid_scroll() {
        // Viewport [950, 1450) intersects cells 9..15 with a 100 px step.
        assert_eq!(visible_cell_range(950.0, 500.0, 100.0, 200), 9..15);
    }

    #[test]
    fn visible_cell_range_clamps_at_end() {
        // A 200-cell strip with 110 px cells + 8 px spacing: scrolling far past
        // the end must clamp to the real range, not run past `count`.
        let count = 200;
        let step = 118.0;
        let range = visible_cell_range(199.0 * step, 400.0, step, count);
        assert_eq!(range.end, count);
        assert!(range.start < count);
    }

    #[test]
    fn visible_cell_range_degenerate_inputs_yield_empty() {
        assert_eq!(visible_cell_range(0.0, 500.0, 100.0, 0), 0..0);
        assert_eq!(visible_cell_range(0.0, 0.0, 100.0, 10), 0..0);
        assert_eq!(visible_cell_range(0.0, 500.0, 0.0, 10), 0..0);
        assert_eq!(visible_cell_range(0.0, 500.0, f32::NAN, 10), 0..0);
    }

    #[test]
    fn buffered_range_stays_inside_bounds() {
        assert_eq!(buffered_range(0..5, 200, 8), 0..13);
        assert_eq!(buffered_range(190..200, 200, 8), 182..200);
        assert_eq!(buffered_range(50..60, 55, 8), 42..55);
        // Buffer larger than the collection clamps to everything.
        assert_eq!(buffered_range(2..3, 5, 8), 0..5);
    }

    #[test]
    fn buffered_range_keeps_empty_window_empty_only_when_count_is_zero() {
        // An empty window in a non-empty folder still widens into a ring —
        // this keeps the filmstrip prefetched even while no cell is laid out.
        assert_eq!(buffered_range(3..3, 10, 1), 2..4);
        assert_eq!(buffered_range(0..0, 0, 8), 0..0);
    }

    #[test]
    fn prefetch_order_alternates_nearest_first() {
        // Window 4..6 in ten entries: right neighbour, then left neighbour,
        // then walking outward alternately (right wins the documented tie).
        assert_eq!(
            prefetch_order(10, 4..6),
            vec![6, 3, 7, 2, 8, 1, 9, 0],
            "off-screen cells must be ordered strictly by distance to the window"
        );
    }

    #[test]
    fn prefetch_order_from_start_and_end() {
        assert_eq!(prefetch_order(4, 0..0), vec![0, 1, 2, 3]);
        assert_eq!(prefetch_order(4, 4..4), vec![3, 2, 1, 0]);
    }

    #[test]
    fn prefetch_order_fully_visible_yields_nothing() {
        assert!(prefetch_order(5, 0..5).is_empty());
        assert!(prefetch_order(0, 0..0).is_empty());
    }

    /// End-to-end property of the three helpers combined: for any scroll
    /// offset over a 200-cell strip the scheduled set is exactly
    /// "buffered window ∪ nearest off-screen candidates", never all cells.
    #[test]
    fn scroll_scheduling_is_bounded_for_200_cells() {
        let count = 200;
        let step = 118.0;
        let viewport_len = 600.0;
        for cell in 0..count {
            let vis = visible_cell_range(cell as f32 * step, viewport_len, step, count);
            let buffered = buffered_range(vis.clone(), count, VISIBLE_BUFFER_CELLS);
            // Immediate work is bounded by the buffer ring, not by the folder.
            assert!(
                buffered.len()
                    <= 2 * VISIBLE_BUFFER_CELLS + viewport_len as usize / step as usize + 2
            );
            // Prefetch candidates are exactly the complement of the buffered
            // window, in near-first order; only PREFETCH_BUDGET_PER_FRAME of
            // them are touched/frame.
            let order = prefetch_order(count, buffered.clone());
            assert_eq!(order.len(), count - buffered.len());
            for (pos, &idx) in order.iter().enumerate() {
                assert!(idx < count, "index out of bounds");
                assert!(!buffered.contains(&idx));
                if pos > 0 {
                    let prev_dist = distance(order[pos - 1], &buffered);
                    let dist = distance(idx, &buffered);
                    assert!(dist >= prev_dist, "prefetch must be nearest-first");
                }
            }
        }
    }

    fn distance(index: usize, window: &CellWindow) -> usize {
        if index < window.start {
            window.start - index
        } else if index >= window.end {
            index + 1 - window.end
        } else {
            0
        }
    }
}
