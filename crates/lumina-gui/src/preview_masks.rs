//! GUI-REFACTOR-W2-20 S2.1: preview mask interaction and the preview
//! overlays, extracted verbatim from `lib.rs`.
//!
//! [`LuminaApp::handle_mask_tool_drag`] drives an interactive mask-tool drag
//! (brush sampling/upload, gradient/radial start-end) and
//! [`LuminaApp::gpu_upload_brush_tile`] keeps the VRAM brush plane in sync on
//! the GPU path. The overlay painters ([`LuminaApp::draw_mask_overlay`],
//! [`LuminaApp::draw_edit_pins`], [`LuminaApp::draw_lens_blur_overlay`])
//! render the CPU matte, the G-11 pins and the lens-blur rectangle on the
//! full-frame preview rect. Pure display and session state — no recipe/sidecar
//! changes. The `#[cfg(feature = "gpu")]` blocks move atomically.
//!
//! UX-LOOK-CROP-18 moved the interactive crop overlay to
//! `develop_geometry::crop_overlay` ([`LuminaApp::draw_crop_overlay`]); the
//! mask/pin/lens-blur painters stay here.
//!
//! The helpers called from `preview_draws::draw_preview` are `pub(crate)`;
//! `gpu_upload_brush_tile` stays private (only this module's drag calls it).

use super::*;
#[cfg(feature = "gpu")]
use log::trace;

impl LuminaApp {
    /// Drive an interactive mask-tool drag on the preview widget. For Brush,
    /// paint the live source-pixel cursor and route a pin click to selection
    /// before any dab is accumulated.
    pub(crate) fn handle_mask_tool_drag(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: egui::Rect,
        scale: f32,
    ) {
        if self.wb_pick_mode
            || self.red_eye_pick_mode
            || self.spot_tool != SpotTool::None
            || self.crop_mode
        {
            return;
        }
        if self.mask_tool == MaskTool::None {
            // Pins remain selectable even while no paint tool is armed, except
            // when another image-click picker (Spot/Red-eye) owns the gesture.
            if self.spot_tool == SpotTool::None && !self.red_eye_pick_mode && response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let full = self.image_dims().unwrap_or((1, 1));
                    let roi = self.preview_roi.map(|roi| {
                        Self::roi_in_full_pixels(roi, full.0, full.1, self.preview_render_src)
                    });
                    let (nx, ny) = Self::to_normalized(pos, rect, roi, full);
                    if let Some(mask_id) = self.mask_pin_hit_at(nx, ny, scale) {
                        if let Err(error) = self.select_mask(&mask_id) {
                            self.show_error(error);
                        }
                    }
                }
            }
            return;
        }
        if self.brush_cursor_allowed(ui) {
            if let Some(pos) = response.hover_pos().filter(|pos| rect.contains(*pos)) {
                let radius = self.brush_cursor_radius(scale);
                ui.painter().circle_stroke(
                    pos,
                    radius,
                    egui::Stroke::new(1.5_f32, crate::theme::ACCENT),
                );
            }
        }
        // R5-TOOLFLOW-1 (User-Entscheid 2026-09-20): the former geometry
        // defense-in-depth refusal is replaced by the tool switch committing
        // the active crop/straighten draft; drawn prompts map through the
        // current `to_normalized`/ROI path.
        let Some(pos) = response.interact_pointer_pos() else {
            return;
        };
        // GUI-DRAFT-JUMP-1: map through the full-space ROI so mask prompts
        // land on the same source pixels on both render paths.
        let full = self.image_dims().unwrap_or((1, 1));
        let roi = self
            .preview_roi
            .map(|r| Self::roi_in_full_pixels(r, full.0, full.1, self.preview_render_src));
        let (nx, ny) = Self::to_normalized(pos, rect, roi, full);
        if let Some(mask_id) = self.mask_pin_hit_at(nx, ny, scale) {
            if response.drag_started() || response.clicked() {
                if let Err(error) = self.select_mask(&mask_id) {
                    self.show_error(error);
                }
                self.drag_current = Some(Point2 { x: nx, y: ny });
                self.pending_brush_marks.clear();
                self.drawing = false;
                return;
            }
        }
        if response.drag_started() {
            if self.mask_tool == MaskTool::Brush {
                if let Err(error) = self.ensure_selected_mask() {
                    self.show_error(error);
                    return;
                }
            }
            self.drawing = true;
            self.drag_start = Some(Point2 { x: nx, y: ny });
            self.drag_current = Some(Point2 { x: nx, y: ny });
            if self.mask_tool == MaskTool::Brush {
                self.pending_brush_marks.clear();
                self.pending_brush_marks.push(self.brush_mark_at(nx, ny));
                #[cfg(feature = "gpu")]
                self.gpu_upload_brush_tile(nx, ny);
            }
        } else if response.dragged() {
            self.drag_current = Some(Point2 { x: nx, y: ny });
            if self.mask_tool == MaskTool::Brush {
                if let Some(last) = self.pending_brush_marks.last() {
                    let dist = ((last.x - nx).powi(2) + (last.y - ny).powi(2)).sqrt();
                    if dist > self.brush_radius * 0.5 {
                        self.pending_brush_marks.push(self.brush_mark_at(nx, ny));
                        #[cfg(feature = "gpu")]
                        self.gpu_upload_brush_tile(nx, ny);
                    }
                }
            }
        }
        if response.drag_stopped() && self.drawing {
            self.finish_drawing();
        }
    }

    /// Screen radius for the live brush cursor. The source radius is relative
    /// to the shorter image axis, so this remains a true circle under fit/zoom.
    pub(crate) fn brush_cursor_radius(&self, scale: f32) -> f32 {
        if !scale.is_finite() || scale <= 0.0 {
            return 0.0;
        }
        let (width, height) = self.image_dims().unwrap_or((1, 1));
        (self.brush_radius * width.min(height) as f32 * scale).clamp(2.0, 4000.0)
    }

    /// Hit-test visible mask prompt pins. The 12-point screen tolerance matches
    /// Spot pins, but interaction honors the exact G-11 visibility gate used by
    /// the painter: a hidden pin is neither painted nor clickable. More than one
    /// pin in the tolerance is deliberately ambiguous and selects nothing.
    pub(crate) fn mask_pin_hit_at(&self, nx: f32, ny: f32, scale: f32) -> Option<String> {
        if !self.pins_visible() || !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        let (width, height) = self.image_dims().unwrap_or((1, 1));
        let tolerance_px = 12.0 / scale;
        let mut hits = Vec::new();
        for pin in self
            .visible_edit_pins()
            .into_iter()
            .filter(|pin| pin.kind == EditPinKind::Mask)
        {
            let dx = (f64::from(nx) - f64::from(pin.pos.0)) * f64::from(width);
            let dy = (f64::from(ny) - f64::from(pin.pos.1)) * f64::from(height);
            if (dx * dx + dy * dy).sqrt() as f32 <= tolerance_px {
                if let Some(id) = pin.id.strip_prefix("mask:") {
                    hits.push(id.to_owned());
                }
            }
        }
        (hits.len() == 1).then(|| hits.remove(0))
    }

    /// Draw the currently relevant mask as a translucent overlay on the preview:
    /// the in-progress drag (live) or the selected mask's saved prompt. The
    /// F-079 geometric rasterizer produces the matte; it is painted as a
    /// translucent tint over the source rect so the user sees exactly what the
    /// pipeline will evaluate.
    ///
    /// GUI-WGPU-PRESENT-1: when the preview is presented readback-free from
    /// VRAM, the overlay shader *already* composites the VRAM mask plane into
    /// the presented texture — painting the CPU tint here would double-tint.
    /// The CPU overlay therefore only runs for content that lives exclusively
    /// on the CPU side:
    ///
    /// - gradient/radial prompts while drawing (never stamped into VRAM tiles),
    /// - any overlay when the frame was CPU-presented.
    ///
    /// Live brush strokes and pipeline-evaluated planes
    /// (`vram_mask_is_evaluated`, pushed after each full render via
    /// `combine_mask_planes` + `upload_mask_plane`) are shown by the GPU
    /// composite instead — same tint strength, same u16 coverage domain.
    pub(crate) fn draw_mask_overlay(&mut self, ui: &mut egui::Ui, full_rect: egui::Rect) {
        // R5-MASKVIS-25: the view/mode gate must run before the GPU-present
        // early return as well as before the CPU rasterizer. Otherwise a
        // combined VRAM mask could remain painted while the Masking section is
        // closed or while pins-only mode is selected.
        if !self.mask_overlay_allowed() {
            return;
        }
        #[cfg(feature = "gpu")]
        if self.gpu_present_frame.is_some() {
            let live_brush_in_vram = self.drawing && self.mask_tool == MaskTool::Brush;
            if live_brush_in_vram || self.vram_mask_is_evaluated {
                trace!(
                    "draw_mask_overlay: gpu present composites the vram mask \
                     (live_brush={live_brush_in_vram}, evaluated={})",
                    self.vram_mask_is_evaluated
                );
                return;
            }
            // Gradient/radial prompts have no VRAM representation — fall
            // through to the CPU overlay below.
        }
        let Some(prompt) = self.effective_overlay_prompt() else {
            return;
        };
        // G-03: the Show switch and the mask's own eye are part of the single
        // `mask_overlay_allowed` gate above; no second mutable draw decision.
        let (w, h) = self.image_dims().unwrap_or((1, 1));
        // Cap the rasterization so live drags stay smooth on large sources.
        let max_dim = 1024u32;
        let (rw, rh) = if w.max(h) > max_dim {
            let s = max_dim as f32 / w.max(h) as f32;
            (
                (w as f32 * s).round().max(1.0) as u32,
                (h as f32 * s).round().max(1.0) as u32,
            )
        } else {
            (w, h)
        };
        // G-03: range stages evaluate against (downscaled) source pixels;
        // geometry prompts rasterize as before. Either failure hides the
        // overlay instead of painting a wrong matte.
        let plane = if range_masks::is_range_prompt(Some(&prompt)) {
            let Some(original) = self.original.as_ref() else {
                return;
            };
            let (pixels, sw, sh) =
                downscale_rgba(&original.pixels, original.width, original.height, max_dim);
            let small = lumina_core::ImageFrame {
                width: sw,
                height: sh,
                pixels,
            };
            match range_masks::evaluate_range_prompt(&small, &prompt) {
                Ok(plane) => plane,
                Err(_) => return,
            }
        } else {
            let Ok(plane) = rasterize_prompt(&prompt, rw, rh) else {
                return;
            };
            plane
        };
        let [tr, tg, tb] = self.overlay_color;
        let mut pixels = vec![0u8; plane.values.len() * 4];
        for (i, value) in plane.values.iter().enumerate() {
            let alpha = (*value as f32 / u16::MAX as f32 * 0.45 * 255.0) as u8;
            pixels[i * 4..i * 4 + 4].copy_from_slice(&[tr, tg, tb, alpha]);
        }
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [plane.width as usize, plane.height as usize],
            &pixels,
        );
        // KITTEST-COVERAGE-OVERLAYS-1: keep the overlay texture alive across
        // frames (`self.mask_overlay_texture`). A per-frame `load_texture` into
        // a local handle dropped the texture at the end of this same frame: egui
        // emitted `set` and `free` in one `TexturesDelta`, the backend freed it
        // before painting, and the matte was invisible in the golden (and on
        // screen under a real backend). Re-using the retained handle via `set`
        // avoids the per-frame alloc/free and matches the retained preview/
        // navigator/thumbnail textures.
        let texture_id = if let Some(handle) = self.mask_overlay_texture.as_mut() {
            handle.set(image, egui::TextureOptions::NEAREST);
            handle.id()
        } else {
            let handle =
                ui.ctx()
                    .load_texture("lumina-mask-overlay", image, egui::TextureOptions::NEAREST);
            let id = handle.id();
            self.mask_overlay_texture = Some(handle);
            id
        };
        ui.painter().image(
            texture_id,
            full_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }

    /// Paint the [`Self::visible_edit_pins`] list (G-11) onto the preview:
    /// numbered circles at the normalized pin anchors mapped through the
    /// full-source rect. Selected pins use the accent fill. No-op while the
    /// pin mode hides pins or no pins exist, so default states render exactly
    /// as before (no golden churn).
    pub(crate) fn draw_edit_pins(&self, ui: &mut egui::Ui, full_rect: egui::Rect) {
        let pins = self.visible_edit_pins();
        if pins.is_empty() {
            return;
        }
        let painter = ui.painter();
        for pin in &pins {
            let pos = egui::pos2(
                full_rect.min.x + pin.pos.0 * full_rect.width(),
                full_rect.min.y + pin.pos.1 * full_rect.height(),
            );
            if !full_rect.contains(pos) {
                continue;
            }
            let fill = if pin.selected {
                crate::theme::ACCENT
            } else {
                egui::Color32::from_gray(30)
            };
            painter.circle_filled(pos, 9.0, fill);
            painter.circle_stroke(pos, 9.0, egui::Stroke::new(1.5, egui::Color32::WHITE));
            painter.text(
                pos,
                egui::Align2::CENTER_CENTER,
                pin.label.clone(),
                egui::FontId::proportional(10.0),
                egui::Color32::WHITE,
            );
        }
    }

    /// Focus-rectangle overlay (G-05): paints the enabled lens-blur focus
    /// rect as an accent stroke over the full-frame preview rect. Pure
    /// display (never recipe/sidecar); the mapping is covered headless via
    /// [`Self::lens_blur_focus_overlay`].
    pub(crate) fn draw_lens_blur_overlay(&self, ui: &mut egui::Ui, full_rect: egui::Rect) {
        let Some(rect) = Self::lens_blur_focus_overlay(full_rect, self.recipe.lens_blur.as_ref())
        else {
            return;
        };
        ui.painter().rect_stroke(
            rect,
            1.0_f32,
            egui::Stroke::new(2.0_f32, crate::theme::ACCENT),
            egui::StrokeKind::Middle,
        );
    }
}
