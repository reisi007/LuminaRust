//! UX-LOOK-CROP-18 (UXG-01): interactive on-canvas crop overlay.
//!
//! The Crop tool (armed by `R` or the Crop toolbar icon) becomes interactive:
//! four corner handles resize, dragging inside the frame moves it, a thirds
//! grid and a darkening mask outside the crop frame the shot live on the
//! preview.
//!
//! SOLL (`feature/platform/lightroom-ux-parity.md` § UX-LOOK-18): the dragged
//! rectangle is a **session-only draft** held in `egui` temp memory — never the
//! recipe and never the sidecar. `Enter` commits the draft through the existing
//! [`LuminaApp::set_crop_free`] path (one debounced sidecar write, one geometry
//! history step); `Esc` discards it. The recipe is therefore written only on
//! commit, and the recipe semantics stay unchanged beyond that documented
//! commit (`Rezept-Semantik außer dem dokumentierten Commit unverändert`).
//!
//! Leaving crop mode without `Enter` (e.g. `R` toggled off) discards the draft
//! loudly instead of carrying an uncommitted rectangle into the next session.
//! Handles are free-rect handles: an interactive commit writes a
//! [`Crop::Free`] rectangle; the aspect-preset selector in the Geometry panel
//! stays the way to request a locked aspect.
//!
//! The unarmed overlay (a recipe crop painted as a white stroke, `OverlayMode`
//! `Always`) is preserved byte-for-byte — only the armed crop mode adds the
//! interactive chrome.

use super::*;
use log::{info, warn};

/// Normalized (`0..=1`) free-crop rectangle of the session draft.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CropDraft {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Minimum normalized extent a corner drag may leave (never a degenerate rect).
const CROP_MIN_EXTENT: f32 = 0.02;
/// Screen (points) side length of one painted corner handle.
const CROP_HANDLE_SIZE: f32 = 8.0;
/// Screen (points) radius within which a press grabs a corner handle.
const CROP_HANDLE_HIT: f32 = 14.0;

/// One of the four draggable crop corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CropCorner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// What an in-flight crop drag does.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CropDragKind {
    Move,
    Corner(CropCorner),
}

/// In-flight crop gesture, kept in `egui` temp memory across frames.
#[derive(Debug, Clone, Copy)]
struct CropDrag {
    kind: CropDragKind,
    /// Effective normalized rectangle when the gesture started.
    start: CropDraft,
    /// Normalized pointer position when the gesture started.
    origin: (f32, f32),
}

fn draft_id() -> egui::Id {
    egui::Id::new("lumina.crop_overlay.draft")
}

fn drag_id() -> egui::Id {
    egui::Id::new("lumina.crop_overlay.drag")
}

/// Stable widget id of the interactive crop region. Used by headless gesture
/// tests to locate the region (the overlay paints no text label).
pub(crate) fn crop_overlay_id() -> egui::Id {
    egui::Id::new("lumina.crop_overlay.region")
}

/// The session-only crop draft, if one is active.
pub(crate) fn crop_draft(ctx: &egui::Context) -> Option<CropDraft> {
    ctx.data(|data| data.get_temp(draft_id()))
}

/// Store (`Some`) or clear (`None`) the crop draft. Returns whether a value was
/// present before/removed: the `None` case is the cancel primitive.
fn set_crop_draft(ctx: &egui::Context, draft: Option<CropDraft>) -> bool {
    ctx.data_mut(|data| match draft {
        Some(value) => {
            data.insert_temp(draft_id(), value);
            true
        }
        None => {
            let had = data.get_temp::<CropDraft>(draft_id()).is_some();
            data.remove::<CropDraft>(draft_id());
            had
        }
    })
}

/// Normalized screen fraction of `pos` inside `full_rect`, clamped to `0..=1`.
fn pointer_fraction(full_rect: egui::Rect, pos: egui::Pos2) -> (f32, f32) {
    (
        ((pos.x - full_rect.min.x) / full_rect.width().max(1e-6)).clamp(0.0, 1.0),
        ((pos.y - full_rect.min.y) / full_rect.height().max(1e-6)).clamp(0.0, 1.0),
    )
}

/// Nearest corner of `crop` within `max_dist` screen points of `pos`.
fn nearest_corner(crop: egui::Rect, pos: egui::Pos2, max_dist: f32) -> Option<CropCorner> {
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
fn moved_rect(start: CropDraft, dx: f32, dy: f32) -> CropDraft {
    CropDraft {
        x: (start.x + dx).clamp(0.0, (1.0 - start.width).max(0.0)),
        y: (start.y + dy).clamp(0.0, (1.0 - start.height).max(0.0)),
        ..start
    }
}

/// Resize `start` by dragging `corner` to the normalized point `to`; keeps the
/// opposite corner anchored and enforces [`CROP_MIN_EXTENT`].
fn resized_rect(start: CropDraft, corner: CropCorner, to: (f32, f32)) -> CropDraft {
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
fn draft_screen_rect(full_rect: egui::Rect, draft: CropDraft) -> egui::Rect {
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
fn outside_regions(full: egui::Rect, crop: egui::Rect) -> [egui::Rect; 4] {
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

impl LuminaApp {
    /// Effective normalized crop rectangle for the overlay: the active session
    /// draft when a gesture/edit is in flight, otherwise the recipe crop (free
    /// rect directly, aspect preset centred like the core crop), otherwise the
    /// full frame.
    fn effective_crop_draft(&self, ctx: &egui::Context) -> CropDraft {
        if let Some(draft) = crop_draft(ctx) {
            return draft;
        }
        let crop = self.recipe.geometry.as_ref().and_then(|g| g.crop.as_ref());
        let (src_w, src_h) = self.image_dims().unwrap_or((0, 0));
        let unit = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        match Self::crop_overlay_rect(unit, crop, src_w, src_h) {
            Some(rect) => CropDraft {
                x: rect.min.x,
                y: rect.min.y,
                width: rect.width(),
                height: rect.height(),
            },
            None => CropDraft {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
        }
    }

    /// Crop-rectangle overlay: the recipe crop paints as a white stroke, and
    /// while crop mode is armed the interactive chrome (darkening, thirds
    /// grid, corner handles, drag gesture) is added. Pure display/session
    /// state — the recipe is only touched on `Enter`.
    pub(crate) fn draw_crop_overlay(&self, ui: &mut egui::Ui, full_rect: egui::Rect) {
        if !self.overlay_visible() && !self.crop_mode {
            return;
        }
        if !self.crop_mode {
            // Historical behaviour (unarmed crop, `OverlayMode::Always`): only
            // an actual recipe crop paints, as a white stroke.
            let crop = self.recipe.geometry.as_ref().and_then(|g| g.crop.as_ref());
            let (src_w, src_h) = self.image_dims().unwrap_or((0, 0));
            let Some(rect) = Self::crop_overlay_rect(full_rect, crop, src_w, src_h) else {
                return;
            };
            ui.painter().rect_stroke(
                rect,
                1.0_f32,
                egui::Stroke::new(1.5_f32, egui::Color32::WHITE),
                egui::StrokeKind::Middle,
            );
            return;
        }
        // Armed crop mode (UX-LOOK-CROP-18): the effective rectangle is the
        // session draft, otherwise the recipe crop, otherwise the full frame.
        let ctx = ui.ctx().clone();
        let draft = self.effective_crop_draft(&ctx);
        let rect = draft_screen_rect(full_rect, draft);
        ui.painter().rect_stroke(
            rect,
            1.0_f32,
            egui::Stroke::new(1.5_f32, egui::Color32::WHITE),
            egui::StrokeKind::Middle,
        );
        self.paint_crop_mode_chrome(ui, full_rect, rect);
        if self.mask_tool != MaskTool::None {
            // A masking/retouch tool owns the pointer; the crop frame stays a
            // pure display (no gesture, no silent reassignment).
            return;
        }
        let response = ui.interact(full_rect, crop_overlay_id(), egui::Sense::drag());
        self.crop_drag_interaction(ui, &ctx, full_rect, &rect, &response);
    }

    /// Paint the armed crop-mode chrome: darkening outside the crop, the
    /// thirds grid and the four corner handles.
    fn paint_crop_mode_chrome(&self, ui: &egui::Ui, full_rect: egui::Rect, rect: egui::Rect) {
        let painter = ui.painter();
        let shade = egui::Color32::from_black_alpha(150);
        for region in outside_regions(full_rect, rect) {
            if region.width().is_finite()
                && region.height().is_finite()
                && region.width() > 0.0
                && region.height() > 0.0
            {
                painter.rect_filled(region, 0.0, shade);
            }
        }
        let grid = egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(120));
        for step in 1..3 {
            let fraction = step as f32 / 3.0;
            let x = rect.left() + fraction * rect.width();
            let y = rect.top() + fraction * rect.height();
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                grid,
            );
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                grid,
            );
        }
        for corner in [
            rect.left_top(),
            rect.right_top(),
            rect.left_bottom(),
            rect.right_bottom(),
        ] {
            let handle = egui::Rect::from_center_size(
                corner,
                egui::vec2(CROP_HANDLE_SIZE, CROP_HANDLE_SIZE),
            );
            painter.rect_filled(handle, 1.0_f32, egui::Color32::WHITE);
            painter.rect_stroke(
                handle,
                1.0_f32,
                egui::Stroke::new(1.0_f32, egui::Color32::from_gray(30)),
                egui::StrokeKind::Inside,
            );
        }
        // Cursor affordance (UXG-14): a crosshair over the armed crop region
        // only, so hovering a panel keeps that panel's cursor.
        if let Some(pos) = ui.ctx().pointer_hover_pos() {
            if full_rect.contains(pos) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
        }
    }

    /// Gesture handling for the armed crop overlay: press a corner to resize,
    /// press inside to move. The draft lives in `egui` temp memory; the recipe
    /// stays untouched until `Enter`.
    fn crop_drag_interaction(
        &self,
        ui: &egui::Ui,
        ctx: &egui::Context,
        full_rect: egui::Rect,
        rect: &egui::Rect,
        response: &egui::Response,
    ) {
        let id = drag_id();
        if response.drag_started() {
            // Grab from the press origin: by the time a drag is recognized the
            // pointer has already moved past the corner.
            let origin = ui
                .input(|input| input.pointer.press_origin())
                .or_else(|| response.interact_pointer_pos());
            let Some(pos) = origin else { return };
            let kind = match nearest_corner(*rect, pos, CROP_HANDLE_HIT) {
                Some(corner) => CropDragKind::Corner(corner),
                None if rect.contains(pos) => CropDragKind::Move,
                // A press outside the crop frame is not a crop gesture.
                None => return,
            };
            let drag = CropDrag {
                kind,
                start: self.effective_crop_draft(ctx),
                origin: pointer_fraction(full_rect, pos),
            };
            ctx.data_mut(|data| data.insert_temp(id, drag));
        }
        if response.dragged() {
            let drag = ctx.data(|data| data.get_temp::<CropDrag>(id));
            if let (Some(drag), Some(pos)) = (drag, response.interact_pointer_pos()) {
                let now = pointer_fraction(full_rect, pos);
                let next = match drag.kind {
                    CropDragKind::Move => {
                        moved_rect(drag.start, now.0 - drag.origin.0, now.1 - drag.origin.1)
                    }
                    CropDragKind::Corner(corner) => resized_rect(drag.start, corner, now),
                };
                set_crop_draft(ctx, Some(next));
                ctx.request_repaint();
            }
        }
        if response.drag_stopped() {
            ctx.data_mut(|data| data.remove::<CropDrag>(id));
        }
    }

    /// `Enter` commit (UX-LOOK-CROP-18): write the session draft through the
    /// existing free-crop setter, which arms the debounced sidecar write and
    /// one geometry history step. Returns whether a draft was committed.
    pub fn commit_crop_edit(&mut self, ctx: &egui::Context) -> bool {
        let Some(draft) = crop_draft(ctx) else {
            return false;
        };
        if let Err(error) = self.set_crop_free(
            f64::from(draft.x),
            f64::from(draft.y),
            f64::from(draft.width),
            f64::from(draft.height),
        ) {
            self.show_error(error);
            return false;
        }
        set_crop_draft(ctx, None);
        info!(
            "GUI interaction: commit_crop_edit -> x={:.4} y={:.4} w={:.4} h={:.4}",
            draft.x, draft.y, draft.width, draft.height
        );
        true
    }

    /// `Esc` cancel / crop-mode exit (UX-LOOK-CROP-18): drop the session draft,
    /// leaving the recipe exactly as before the gesture. Returns whether a
    /// draft was discarded.
    pub fn cancel_crop_edit(&mut self, ctx: &egui::Context) -> bool {
        if !set_crop_draft(ctx, None) {
            return false;
        }
        info!("GUI interaction: cancel_crop_edit -> interactive crop draft discarded");
        true
    }

    /// `Enter`/`Esc` wiring for the interactive crop tool. `Enter` commits and
    /// `Esc` discards the session draft while crop mode is armed in Develop;
    /// every other key state falls through to the existing escape handling.
    /// Replaces the bare [`Self::handle_escape_shortcut`] call in the app
    /// frame (the method itself keeps its armed-picker contract).
    pub(crate) fn handle_crop_shortcuts(&mut self, ctx: &egui::Context) {
        if self.active_module == Module::Develop && !ctx.egui_wants_keyboard_input() {
            if self.crop_mode {
                if ctx.input(|input| input.key_pressed(egui::Key::Enter))
                    && self.commit_crop_edit(ctx)
                {
                    return;
                }
                if ctx.input(|input| input.key_pressed(egui::Key::Escape))
                    && self.cancel_crop_edit(ctx)
                {
                    self.status = Str::Cancel.t().into();
                    return;
                }
            } else if self.cancel_crop_edit(ctx) {
                // Crop mode left without Enter (`R` toggled off): the draft is
                // discarded loudly, never carried silently into a later session.
                warn!("crop overlay: crop mode left with an uncommitted draft — discarded");
                return;
            }
        }
        self.handle_escape_shortcut(ctx);
    }
}
