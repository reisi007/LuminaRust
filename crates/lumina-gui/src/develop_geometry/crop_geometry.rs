//! UX-LOOK-CROP-18b (UXG-01): pure math of the interactive crop overlay.
//!
//! Extracted from `crop_overlay.rs` (file-size ratchet) so the session-state
//! and draw code stays below the 500-line limit. Everything here is pure
//! geometry — no recipe, no session storage, no painting — so it is trivially
//! headless-testable. The session draft storage itself stays in
//! `crop_overlay.rs`.

use super::*;

/// Normalized (`0..=1`) free-crop rectangle of the session draft.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CropDraft {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

/// Minimum normalized extent a corner drag may leave (never a degenerate rect).
pub(crate) const CROP_MIN_EXTENT: f32 = 0.02;
/// Screen (points) side length of one painted corner handle.
pub(crate) const CROP_HANDLE_SIZE: f32 = 8.0;
/// Screen (points) radius within which a press grabs a corner handle.
pub(crate) const CROP_HANDLE_HIT: f32 = 14.0;

/// One of the four draggable crop corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CropCorner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// What an in-flight crop drag does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CropDragKind {
    Move,
    Corner(CropCorner),
}

/// In-flight crop gesture, kept in `egui` temp memory across frames.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CropDrag {
    pub(crate) kind: CropDragKind,
    /// Effective normalized rectangle when the gesture started.
    pub(crate) start: CropDraft,
    /// Normalized pointer position when the gesture started.
    pub(crate) origin: (f32, f32),
}

/// Normalized screen fraction of `pos` inside `full_rect`, clamped to `0..=1`.
pub(crate) fn pointer_fraction(full_rect: egui::Rect, pos: egui::Pos2) -> (f32, f32) {
    (
        ((pos.x - full_rect.min.x) / full_rect.width().max(1e-6)).clamp(0.0, 1.0),
        ((pos.y - full_rect.min.y) / full_rect.height().max(1e-6)).clamp(0.0, 1.0),
    )
}

/// Nearest corner of `crop` within `max_dist` screen points of `pos`.
pub(crate) fn nearest_corner(
    crop: egui::Rect,
    pos: egui::Pos2,
    max_dist: f32,
) -> Option<CropCorner> {
    [
        (CropCorner::TopLeft, crop.left_top()),
        (CropCorner::TopRight, crop.right_top()),
        (CropCorner::BottomLeft, crop.left_bottom()),
        (CropCorner::BottomRight, crop.right_bottom()),
    ]
    .into_iter()
    .map(|(corner, point)| (corner, point.distance(pos)))
    .filter(|(_, distance)| *distance <= max_dist)
    .min_by(|a, b| a.1.total_cmp(&b.1))
    .map(|(corner, _)| corner)
}

/// Move `start` by a normalized delta, clamped so the rectangle stays inside
/// the frame.
pub(crate) fn moved_rect(start: CropDraft, dx: f32, dy: f32) -> CropDraft {
    CropDraft {
        x: (start.x + dx).clamp(0.0, (1.0 - start.width).max(0.0)),
        y: (start.y + dy).clamp(0.0, (1.0 - start.height).max(0.0)),
        ..start
    }
}

/// Resize `start` by dragging `corner` to the normalized point `to`; keeps the
/// opposite corner anchored and enforces [`CROP_MIN_EXTENT`].
pub(crate) fn resized_rect(start: CropDraft, corner: CropCorner, to: (f32, f32)) -> CropDraft {
    let fx = to.0.clamp(0.0, 1.0);
    let fy = to.1.clamp(0.0, 1.0);
    let mut x0 = start.x;
    let mut y0 = start.y;
    let mut x1 = start.x + start.width;
    let mut y1 = start.y + start.height;
    match corner {
        CropCorner::TopLeft => {
            x0 = fx;
            y0 = fy;
        }
        CropCorner::TopRight => {
            x1 = fx;
            y0 = fy;
        }
        CropCorner::BottomLeft => {
            x0 = fx;
            y1 = fy;
        }
        CropCorner::BottomRight => {
            x1 = fx;
            y1 = fy;
        }
    }
    // Keep the dragged edge on the far side of the anchored one (a crossing
    // drag stops at the minimum extent instead of inverting the rect).
    match corner {
        CropCorner::TopLeft | CropCorner::BottomLeft => x0 = x0.min(x1 - CROP_MIN_EXTENT),
        CropCorner::TopRight | CropCorner::BottomRight => x1 = x1.max(x0 + CROP_MIN_EXTENT),
    }
    match corner {
        CropCorner::TopLeft | CropCorner::TopRight => y0 = y0.min(y1 - CROP_MIN_EXTENT),
        CropCorner::BottomLeft | CropCorner::BottomRight => y1 = y1.max(y0 + CROP_MIN_EXTENT),
    }
    // Final clamp into the frame (the anchored side may sit near an edge, so
    // the minimum extent wins over the boundary).
    x0 = x0.clamp(0.0, 1.0 - CROP_MIN_EXTENT);
    y0 = y0.clamp(0.0, 1.0 - CROP_MIN_EXTENT);
    x1 = x1.clamp(x0 + CROP_MIN_EXTENT, 1.0);
    y1 = y1.clamp(y0 + CROP_MIN_EXTENT, 1.0);
    CropDraft {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

/// Map the normalized draft onto the full-frame preview canvas.
pub(crate) fn draft_screen_rect(full_rect: egui::Rect, draft: CropDraft) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(
            full_rect.min.x + draft.x * full_rect.width(),
            full_rect.min.y + draft.y * full_rect.height(),
        ),
        egui::vec2(
            draft.width * full_rect.width(),
            draft.height * full_rect.height(),
        ),
    )
}

/// The four rectangles covering `full` outside `crop` (the darkening bands).
pub(crate) fn outside_regions(full: egui::Rect, crop: egui::Rect) -> [egui::Rect; 4] {
    let c = crop.intersect(full);
    [
        egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, c.min.y)),
        egui::Rect::from_min_max(egui::pos2(full.min.x, c.max.y), full.max),
        egui::Rect::from_min_max(
            egui::pos2(full.min.x, c.min.y),
            egui::pos2(c.min.x, c.max.y),
        ),
        egui::Rect::from_min_max(
            egui::pos2(c.max.x, c.min.y),
            egui::pos2(full.max.x, c.max.y),
        ),
    ]
}
