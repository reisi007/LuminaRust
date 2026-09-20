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
//! R4-RECT-1 (2026-09-20): the former unarmed white crop stroke is removed.
//! Outside crop mode the preview pixels already carry the committed crop (the
//! render applies it); painting its normalized rect onto the *full-source*
//! canvas drew a white frame that reached past the cropped image with no
//! interactive meaning — the reported rectangle over the preview at Fit. Only
//! the armed crop tool paints a crop frame (its preview is the full frame, so
//! the rect maps correctly).

use super::*;
use log::{info, warn};

// UX-LOOK-CROP-18b: the crop bar's session rotation draft, the fingerprinted
// Auto-Level stash and the bar layout/gesture helpers (own file, same module).
use super::crop_geometry::{
    draft_screen_rect, moved_rect, nearest_corner, outside_regions, pointer_fraction, resized_rect,
    CropDraft, CropDrag, CropDragKind, CROP_HANDLE_HIT, CROP_HANDLE_SIZE,
};
use super::crop_rotation::{
    crop_auto_level_stash, crop_bar_rect, crop_rotation_draft, set_crop_auto_level_stash,
    set_crop_rotation_draft,
};

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
    /// grid, corner handles, drag gesture) and the crop bar (straighten +
    /// Auto-Level, UX-LOOK-CROP-18b) are added. Pure display/session state —
    /// the recipe is only touched on `Enter`.
    pub(crate) fn draw_crop_overlay(
        &mut self,
        ui: &mut egui::Ui,
        pane: egui::Rect,
        full_rect: egui::Rect,
    ) {
        // R4-RECT-1: only the armed crop tool paints a crop frame. An unarmed
        // committed crop is already applied to the preview pixels; the former
        // full-source white stroke reached past the cropped image and was not
        // interactive (no picker/crop gesture), so it must not be painted.
        if !self.crop_mode {
            return;
        }
        // Armed crop mode (UX-LOOK-CROP-18): the effective rectangle is the
        // session draft, otherwise the recipe crop, otherwise the full frame.
        // UX-LOOK-CROP-18b: `full_rect` is the *full-frame* canvas because the
        // crop-mode preview renders the full frame (see `crop_display.rs`), so
        // a committed crop stays re-editable (shrink AND grow to full frame).
        //
        // The toggle invalidates the preview (full frame vs. committed crop),
        // and the resulting render resets the status line to "Preview current";
        // keep the crop-mode hint visible without clobbering real feedback
        // (errors, Auto-Level status), which are never `PreviewCurrent`.
        if self.status == Str::PreviewCurrent.t() {
            self.status = Str::CropModeOn.t().into();
        }
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
        let bar = crop_bar_rect(pane);
        if self.mask_tool != MaskTool::None {
            // A masking/retouch tool owns the pointer; the crop frame stays a
            // pure display (no gesture, no silent reassignment). The crop bar
            // widgets still handle their own pointer.
            self.draw_crop_bar(ui, bar, &ctx);
            return;
        }
        let response = ui.interact(full_rect, crop_overlay_id(), egui::Sense::drag());
        self.crop_drag_interaction(ui, &ctx, full_rect, &rect, &response, bar);
        self.draw_crop_bar(ui, bar, &ctx);
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
    /// stays untouched until `Enter`. `bar` is the crop-bar strip: a press that
    /// starts there belongs to the bar controls, never to the crop gesture.
    fn crop_drag_interaction(
        &self,
        ui: &egui::Ui,
        ctx: &egui::Context,
        full_rect: egui::Rect,
        rect: &egui::Rect,
        response: &egui::Response,
        bar: egui::Rect,
    ) {
        let id = drag_id();
        if response.drag_started() {
            // Grab from the press origin: by the time a drag is recognized the
            // pointer has already moved past the corner.
            let origin = ui
                .input(|input| input.pointer.press_origin())
                .or_else(|| response.interact_pointer_pos());
            let Some(pos) = origin else { return };
            if bar.contains(pos) {
                // The press belongs to the crop-bar slider/button.
                return;
            }
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

    /// `Enter` commit (UX-LOOK-CROP-18): write the session drafts through the
    /// existing setters, which arm the debounced sidecar write and one geometry
    /// history step (crop + rotation + a stashed Auto-Level analysis run before
    /// the single save, so they coalesce into one step). Returns whether
    /// anything was committed.
    ///
    /// UX-LOOK-CROP-18b adds the session rotation draft and the Auto-Level
    /// analysis stash to the same commit; a rotation that already matches the
    /// recipe is not re-written (keeps a crop-only commit labelled
    /// `geometry.crop_free`).
    pub fn commit_crop_edit(&mut self, ctx: &egui::Context) -> bool {
        let draft = crop_draft(ctx);
        let rotation = crop_rotation_draft(ctx);
        let auto = crop_auto_level_stash(ctx);
        if draft.is_none() && rotation.is_none() && auto.is_none() {
            return false;
        }
        if let Some(stash) = &auto {
            self.persist_auto_level_analysis(stash);
        }
        if let Some(degrees) = rotation {
            let current = self.committed_rotation_degrees();
            if (f64::from(degrees) - current).abs() > 1e-4 {
                self.set_straighten(f64::from(degrees));
            }
        }
        if let Some(draft) = draft {
            if let Err(error) = self.set_crop_free(
                f64::from(draft.x),
                f64::from(draft.y),
                f64::from(draft.width),
                f64::from(draft.height),
            ) {
                self.show_error(error);
                return false;
            }
        }
        set_crop_draft(ctx, None);
        set_crop_rotation_draft(ctx, None);
        set_crop_auto_level_stash(ctx, None);
        info!(
            "GUI interaction: commit_crop_edit -> crop={draft:?} rotation={rotation:?} \
             auto_level={}",
            auto.is_some()
        );
        true
    }

    /// `Esc` cancel / crop-mode exit (UX-LOOK-CROP-18): drop the session drafts
    /// (crop rectangle, rotation, Auto-Level stash), leaving the recipe exactly
    /// as before the gesture. Returns whether anything was discarded.
    pub fn cancel_crop_edit(&mut self, ctx: &egui::Context) -> bool {
        let had = set_crop_draft(ctx, None)
            | set_crop_rotation_draft(ctx, None)
            | set_crop_auto_level_stash(ctx, None);
        if !had {
            return false;
        }
        info!("GUI interaction: cancel_crop_edit -> interactive crop/rotation draft discarded");
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
