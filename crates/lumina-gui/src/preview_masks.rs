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
#[cfg(feature = "gpu")]
use log::warn;

impl LuminaApp {
    /// Drive an interactive mask-tool drag on the preview widget.
    pub(crate) fn handle_mask_tool_drag(&mut self, response: &egui::Response, rect: egui::Rect) {
        if self.mask_tool == MaskTool::None || self.wb_pick_mode {
            return;
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
        if response.drag_started() {
            self.drawing = true;
            self.drag_start = Some(Point2 { x: nx, y: ny });
            self.drag_current = Some(Point2 { x: nx, y: ny });
            if self.mask_tool == MaskTool::Brush {
                self.pending_brush_marks.clear();
                self.pending_brush_marks.push(BrushMark {
                    x: nx,
                    y: ny,
                    radius: self.brush_radius,
                    sign: if self.brush_eraser {
                        BrushMarkSign::Negative
                    } else {
                        BrushMarkSign::Positive
                    },
                });
                #[cfg(feature = "gpu")]
                self.gpu_upload_brush_tile(nx, ny);
            }
        } else if response.dragged() {
            self.drag_current = Some(Point2 { x: nx, y: ny });
            if self.mask_tool == MaskTool::Brush {
                if let Some(last) = self.pending_brush_marks.last() {
                    let dist = ((last.x - nx).powi(2) + (last.y - ny).powi(2)).sqrt();
                    if dist > self.brush_radius * 0.5 {
                        self.pending_brush_marks.push(BrushMark {
                            x: nx,
                            y: ny,
                            radius: self.brush_radius,
                            sign: if self.brush_eraser {
                                BrushMarkSign::Negative
                            } else {
                                BrushMarkSign::Positive
                            },
                        });
                        #[cfg(feature = "gpu")]
                        self.gpu_upload_brush_tile(nx, ny);
                    }
                }
            }
        }
        if response.drag_stopped() {
            self.finish_drawing();
        }
    }

    #[cfg(feature = "gpu")]
    fn gpu_upload_brush_tile(&mut self, nx: f32, ny: f32) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        if !gpu.is_available() {
            return;
        }
        let Ok((w, h)) = self.image_dims() else {
            return;
        };
        // Ensure the persistent R16 plane exists and matches the current source dims.
        let dims_changed = self.brush_mask_plane_dims != Some((w, h));
        if dims_changed || self.brush_mask_plane.is_none() {
            let len = (w as usize).saturating_mul(h as usize);
            self.brush_mask_plane = Some(vec![0u16; len]);
            self.brush_mask_plane_dims = Some((w, h));
            // Also ensure VRAM mask texture is sized for this source; stale
            // dimensions are handled lazily in `ensure_vram` at render time.
            if let Err(e) = gpu.ensure_vram(w, h) {
                warn!("gpu ensure_vram({}x{}) failed: {}", w, h, e);
            }
        }
        let Some(plane) = self.brush_mask_plane.as_mut() else {
            return;
        };
        let sign = if self.brush_eraser {
            lumina_sidecar::BrushMarkSign::Negative
        } else {
            lumina_sidecar::BrushMarkSign::Positive
        };
        let tiles = lumina_gpu::tiling::dirty_tiles_for_brush_mark(nx, ny, self.brush_radius, w, h);
        // Persistent plane: stamp once, then upload only dirty 512² tiles.
        // `stamp_brush_mark` is the canonical per-pixel kernel from
        // `lumina_core::mask_tiles` (byte-identical to `rasterize_prompt` Brush).
        lumina_core::mask_tiles::stamp_brush_mark(plane, w, h, nx, ny, self.brush_radius, sign);
        for tile in tiles {
            let x0 = tile.tx * lumina_gpu::tiling::TILE_SIZE;
            let y0 = tile.ty * lumina_gpu::tiling::TILE_SIZE;
            let tw = (lumina_gpu::tiling::TILE_SIZE)
                .min(w.saturating_sub(x0))
                .max(1);
            let th = (lumina_gpu::tiling::TILE_SIZE)
                .min(h.saturating_sub(y0))
                .max(1);
            // Extract this tile's u16 row-major subregion from the persistent plane
            // and upload as u8 LE bytes via `bytemuck::cast_slice` (no per-pixel copy).
            let mut tile_u16 = Vec::with_capacity((tw * th) as usize);
            for row in 0..th {
                let src_y = y0 + row;
                let src_start = (src_y * w + x0) as usize;
                let src_end = src_start + tw as usize;
                if src_end <= plane.len() {
                    tile_u16.extend_from_slice(&plane[src_start..src_end]);
                }
            }
            if tile_u16.len() != (tw * th) as usize {
                warn!(
                    "brush tile slice length mismatch {} vs {}x{}",
                    tile_u16.len(),
                    tw,
                    th
                );
                continue;
            }
            let tile_bytes: &[u8] = bytemuck::cast_slice(&tile_u16);
            if let Err(e) = gpu.upload_mask_tile(x0, y0, tw, th, tile_bytes) {
                warn!(
                    "gpu_upload_brush_tile upload failed at tile {}x{} ({}x{}): {}",
                    x0, y0, tw, th, e
                );
            } else {
                trace!(
                    "gpu_upload_brush_tile stamped ({:.3},{:.3}) r={:.3} -> tile ({},{}) {}x{}",
                    nx,
                    ny,
                    self.brush_radius,
                    x0,
                    y0,
                    tw,
                    th
                );
            }
        }
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
        // G-03: the Show switch and the mask's own eye gate the overlay on top
        // of the G-11 mode (`effective_overlay_prompt` already applied it).
        if !self.mask_overlay_allowed() {
            return;
        }
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
