//! GUI-REFACTOR-W2-20 S2.1: the preview/canvas draw path, extracted
//! verbatim from `lib.rs`.
//!
//! [`LuminaApp::draw_preview`] paints the committed (or draft) frame into the
//! preview pane: object-contain fit, zoom/pan geometry, ROI-crop placement, the
//! Ctrl/Cmd wheel zoom and modifier-free wheel pan, the WB/red-eye pickers and
//! the mask-tool drag. [`LuminaApp::to_normalized`] is the pointer→source
//! mapping shared by the pickers/mask tools, [`LuminaApp::current_overlay_prompt`]
//! resolves the live/saved overlay prompt. No behaviour changes: every rect,
//! clamp, gesture and trace is byte-identical to the inlined sequence this
//! extraction replaces.
//!
//! `draw_preview`/`current_overlay_prompt` are `pub(crate)` because the preview
//! area/side-panel draw paths in `lib.rs` call them; `to_normalized` is
//! `pub(crate)` for the headless mapping test.

use super::*;
use log::trace;

// R5-FIX-WELLE-20 / R4-NAV-1: pan-clamp + ROI placement geometry (new file so
// neither this 500-line-ratcheted module nor `lib.rs` has to grow).
mod preview_geometry;
mod preview_mapping;

impl LuminaApp {
    pub(crate) fn draw_preview(&mut self, ui: &mut egui::Ui) {
        // Clone the texture handle so the borrow of `self` does not outlive the
        // block — several helpers below (`handle_mask_tool_drag`,
        // `draw_mask_overlay`, `pick_white_balance_at`, `mark_dirty`) need
        // `&mut self` and would otherwise conflict with `&self.texture`.
        //
        // GUI-WGPU-PRESENT-1: when the frame was presented readback-free from
        // VRAM (`gpu_present_frame`), the painted image is that registered user
        // texture instead of the CPU `ColorImage`; its size (full frame) feeds
        // the same geometry math. The CPU handle stays the fallback and is
        // always present once any CPU render ran (warm-up before the very first
        // render still shows the empty-state label).
        // KITTEST-PARITY-PATHS-1: the geometry readouts describe exactly the
        // frame being painted; clear them so the empty state never reports a
        // stale rect from a previous preview.
        self.preview_screen_rect = None;
        self.preview_pane_rect = None;
        self.overlay_full_rect = None;
        if let Some(texture) = self.texture.clone() {
            #[cfg(feature = "gpu")]
            let gpu_present = self.gpu_present_frame;
            #[cfg(not(feature = "gpu"))]
            #[allow(clippy::infallible_destructuring_match)]
            let gpu_present: Option<(egui::TextureId, [usize; 2])> = None;
            // The preview pane is laid out somewhere inside the window, so its
            // origin is not (0,0). `available_size()` returns only the
            // dimensions, so building the draw rect relative to (0,0) places the
            // image at the window origin instead of centering it in the pane,
            // producing the misalignment/clipping seen in the screenshot.
            //
            // Use the true available rectangle (with the correct origin) and
            // center the image inside it.
            let pane = ui.available_rect_before_wrap();
            let (tw, th) = match gpu_present {
                Some((_, size)) => (size[0] as f32, size[1] as f32),
                None => {
                    let size = texture.size();
                    (size[0] as f32, size[1] as f32)
                }
            };
            // Un-cropped source dimensions backing the texture (the texture
            // itself is an ROI crop at zoom > 1, so its own fit scale depends
            // on the zoom and must never feed back into the zoom derivation —
            // REVIEW-GUI-ZOOMLOOP-1).
            let (src_w, src_h) = self
                .original
                .as_ref()
                .map(|o| (o.width as f32, o.height as f32))
                .or_else(|| {
                    self.draft_original
                        .as_ref()
                        .map(|d| (d.width as f32, d.height as f32))
                })
                .unwrap_or((tw, th));
            if src_w > 0.0 && src_h > 0.0 {
                // Object-contain fit of the pane against the un-cropped source
                // (not capped, so small images fill the pane / large images are
                // downscaled, Lightroom-like).
                self.preview_base_fit_scale = (pane.width() / src_w).min(pane.height() / src_h);
                self.preview_src_w = src_w;
                self.preview_src_h = src_h;
            }
            // Cache geometry so the next frame's sync_zoom() derives absolute
            // zoom modes (100% / 200% / Fit Width) from stable, un-cropped
            // values.
            self.preview_pane_w = pane.width();
            self.preview_pane_h = pane.height();

            // On-screen scale in screen points per FULL-source pixel. The
            // ROI-cropped texture is drawn at this same scale; `roi_from_zoom`
            // sizes the crop to fill the pane exactly at it. A draft texture
            // lives in downscaled render-source space (GUI-DRAFT-JUMP-1), so
            // its dims are scaled back into full-source geometry first —
            // otherwise the draft draws too small and jumps on mouse-up when
            // the full frame swaps in.
            let mut scale = self.preview_base_fit_scale * self.preview_zoom;
            let (tex_w, tex_h) =
                Self::preview_draw_dims(tw, th, src_w, src_h, self.preview_render_src);
            let mut draw = egui::vec2(tex_w * scale, tex_h * scale);
            // GUI-FIT-1: pan is only meaningful in `Custom`. Absolute modes
            // re-centre every frame (`sync_zoom`), so a stale pan offset must
            // never shift the placement here — panning in Fit is a no-op.
            let eff_pan = if self.zoom_mode == ZoomMode::Custom {
                self.preview_pan
            } else {
                egui::Vec2::ZERO
            };
            // R4-NAV-1: an ROI crop already encodes the pan in its window
            // (`roi_from_zoom` centres it on `w/2 - pan/scale`), so it is
            // placed by `live_pan - rendered_pan` — adding the live pan on top
            // of the crop (the old code) moved the image 2× the cursor and
            // made the navigator box disagree with the view. Without a crop
            // `rendered_pan` is zero and this is the historical `+pan`.
            let (full_u_w, full_u_h) = self.image_dims().unwrap_or((src_w as u32, src_h as u32));
            let rendered_pan = Self::roi_rendered_pan(
                self.preview_roi,
                full_u_w,
                full_u_h,
                self.preview_render_src,
                scale,
            );
            let mut center = pane.center() + eff_pan - rendered_pan;
            let mut rect = egui::Rect::from_center_size(center, draw);

            // Scroll-wheel behaviour (GUI-PREVIEW-NAV-1, Lightroom-like): the
            // wheel zooms around the cursor ONLY while Ctrl/Cmd is held (then
            // the mode pins to `Custom`, like `zoom_step`); without a modifier
            // the wheel pans the zoomed image and never touches the zoom, so
            // `Custom` can never arise by accident. egui 0.36 removed
            // `InputState::raw_scroll_delta`; the raw per-frame wheel delta is
            // summed from the `MouseWheel` events. Only handled while the
            // pointer hovers the preview so other scroll areas are unaffected.
            //
            // The modifier is read from the wheel events themselves as well as
            // the global input state: a held Ctrl is delivered as key state in
            // live frames, while synthetic/headless frames may carry it only
            // on the event.
            let (wheel, wheel_zoom) = ui.input(|i| {
                let mut delta = egui::Vec2::ZERO;
                let mut zoom = Self::wants_wheel_zoom(&i.modifiers);
                for event in i.raw.events.iter() {
                    if let egui::Event::MouseWheel {
                        delta: event_delta,
                        modifiers,
                        ..
                    } = event
                    {
                        delta += *event_delta;
                        zoom = zoom || Self::wants_wheel_zoom(modifiers);
                    }
                }
                (delta, zoom)
            });
            let pointer = ui.input(|i| i.pointer.interact_pos());
            if wheel != egui::Vec2::ZERO {
                // GUI-VIEW-2 (Scroll-Bleed): the wheel acts only when the
                // pointer is over the preview *pane* — the image rect can
                // extend under the side panels when zoomed (it is painted
                // clipped below), and without the pane gate a wheel over the
                // Basic panel would pan/zoom the image behind it.
                if let Some(p) = pointer {
                    if pane.contains(p) && rect.contains(p) {
                        if wheel_zoom {
                            let srect_w = rect.width().max(1e-6);
                            let srect_h = rect.height().max(1e-6);
                            let fx = ((p.x - rect.min.x) / srect_w).clamp(0.0, 1.0);
                            let fy = ((p.y - rect.min.y) / srect_h).clamp(0.0, 1.0);
                            let factor = if wheel.y > 0.0 { 1.1 } else { 1.0 / 1.1 };
                            self.preview_zoom = (self.preview_zoom * factor).clamp(0.05, 32.0);
                            self.zoom_mode = ZoomMode::Custom;
                            let new_scale = self.preview_base_fit_scale * self.preview_zoom;
                            let new_draw = egui::vec2(tex_w * new_scale, tex_h * new_scale);
                            let new_center =
                                p - egui::vec2(fx * new_draw.x, fy * new_draw.y) + new_draw / 2.0;
                            self.preview_pan = new_center - pane.center();
                            // Recompute for the placement below.
                            scale = new_scale;
                            draw = new_draw;
                            center = new_center - rendered_pan;
                            rect = egui::Rect::from_center_size(center, draw);
                            self.mark_dirty();
                        } else if Self::pan_gesture_pins_custom(
                            self.preview_zoom,
                            draw.x,
                            draw.y,
                            pane.width(),
                            pane.height(),
                        ) {
                            // Modifier-free wheel pans the zoomed image (the
                            // clamp below keeps it covering the pane). Panning
                            // only persists in `Custom` (see `sync_zoom`), so
                            // the mode follows — the zoom factor itself is
                            // untouched, never an accidental zoom.
                            // GUI-ZOOM-CUSTOM-1: at Fit there is nothing to
                            // pan — the gate above keeps the mode Fit.
                            self.zoom_mode = ZoomMode::Custom;
                            center += wheel;
                            self.preview_pan += wheel;
                            // Recompute for the placement below.
                            rect = egui::Rect::from_center_size(center, draw);
                            self.mark_dirty();
                        }
                    }
                }
            }

            // Whether the (zoomed) image overflows the pane on either axis — only
            // then is panning meaningful. GUI-ZOOM-CUSTOM-1: panning (and the
            // `Custom` pin) additionally requires an actual magnification
            // (`preview_zoom > 1.0`) — at Fit a stale/oversized texture must
            // never flip the readout to `Custom`.
            let pan_eligible = Self::pan_gesture_pins_custom(
                self.preview_zoom,
                draw.x,
                draw.y,
                pane.width(),
                pane.height(),
            );

            // Resolve one effective preview gesture owner. The arming setters
            // clear competitors, and this defensive gate also keeps a stale
            // flag from making two handlers consume the same click.
            let owner_free = !self.crop_mode;
            let pick = (self.wb_pick_mode || self.local_wb_pick_mode)
                && !self.red_eye_pick_mode
                && self.mask_tool == MaskTool::None
                && self.spot_tool == SpotTool::None
                && owner_free;
            let red_eye_pick = self.red_eye_pick_mode
                && !self.wb_pick_mode
                && !self.local_wb_pick_mode
                && self.mask_tool == MaskTool::None
                && self.spot_tool == SpotTool::None
                && owner_free;
            let armed = self.mask_tool != MaskTool::None
                && self.spot_tool == SpotTool::None
                && !self.wb_pick_mode
                && !self.local_wb_pick_mode
                && !self.red_eye_pick_mode
                && owner_free;
            let spot_armed = self.spot_tool != SpotTool::None
                && self.mask_tool == MaskTool::None
                && !self.wb_pick_mode
                && !self.local_wb_pick_mode
                && !self.red_eye_pick_mode
                && owner_free;

            // A mask tool arms the preview for a drag gesture; the armed spot
            // tool, the WB eyedropper and the red-eye region picker keep a
            // plain click; otherwise a zoomed image drags to pan (hand tool).
            // Pan never conflicts with an armed tool or either picker.
            let sense = if armed {
                egui::Sense::drag()
            } else if spot_armed || pick || red_eye_pick {
                egui::Sense::click()
            } else if pan_eligible && !self.crop_mode {
                egui::Sense::drag()
            } else {
                egui::Sense::click()
            };
            let response = ui.allocate_rect(rect, sense);

            // Pan while zoomed (only when no tool and not picking, and
            // never while the interactive crop tool owns the pointer).
            if !armed && !spot_armed && !pick && !red_eye_pick && pan_eligible && !self.crop_mode {
                let delta = response.drag_delta();
                if delta != egui::Vec2::ZERO {
                    if response.drag_started() {
                        self.zoom_mode = ZoomMode::Custom;
                        trace!("GUI interaction: preview pan start");
                    }
                    center += delta;
                    // REVIEW-GUI-PANROI-1: a pan moves the visible window
                    // inside the source, so the ROI-cropped texture must be
                    // re-derived from the new offset. Marking dirty here arms
                    // the PERF-GUI-3/4 hot path: while the pointer stays down
                    // the next frame renders a cheap draft from the new pan
                    // (coalesced to one draft per moved frame), and once the
                    // pointer is released the debounced full render commits
                    // the final ROI — including the clamped borders that
                    // `preview_pan` alone could never reach before.
                    self.mark_dirty();
                }
            }

            // R5-FIX-WELLE-20 / R4-NAV-1: clamp the pan so the FULL source image
            // always covers the pane (no empty gutters); a not-magnified or
            // smaller-than-pane view stays centred. The old inline clamp used
            // the painted texture's size, which at zoom is an ROI crop only
            // `PREVIEW_ROI_MARGIN`× the pane — that capped every pan at ~15 % of
            // a pane, so the navigator box barely moved. The crop itself always
            // covers the pane (it is re-derived from the clamped pan), so the
            // guard is purely about the image bounds.
            let desired_pan = center - pane.center() + rendered_pan;
            let clamped = Self::clamp_preview_pan(
                desired_pan,
                self.preview_zoom,
                src_w,
                src_h,
                scale,
                pane.width(),
                pane.height(),
            );
            self.preview_pan = if self.zoom_mode == ZoomMode::Custom {
                clamped
            } else {
                // GUI-ZOOM-CUSTOM-1 / GUI-NAV-RECT-1: pan is only meaningful
                // in `Custom`. Absolute modes re-centre every frame
                // (`sync_zoom`), so a stale offset (e.g. an oversized
                // texture right after an image switch) must never leak into
                // the ROI crop or the navigator rectangle — Fit stays
                // centred with zero pan.
                egui::Vec2::ZERO
            };
            center = pane.center() + self.preview_pan - rendered_pan;
            let rect = egui::Rect::from_center_size(center, draw);
            self.preview_effective_scale = scale;
            // KITTEST-PARITY-PATHS-1: record the painted preview quad and the
            // pane it was fitted into (path-independent geometry anchor).
            self.preview_screen_rect = Some(rect);
            self.preview_pane_rect = Some(pane);

            // GUI-VIEW-2 (Overlap): the zoomed image rect can extend beyond
            // the pane (toolbar/filmstrip/panel territory) — constrain all
            // preview painting to the pane and restore the clip afterwards.
            let previous_clip = ui.clip_rect();
            ui.set_clip_rect(previous_clip.intersect(pane));
            // GUI-WGPU-PRESENT-1: the GPU-presented frame is a registered
            // user texture — draw it via the painter directly (identical rect,
            // full UVs). Otherwise the historical CPU `Image` widget.
            if let Some((present_id, _)) = gpu_present {
                ui.painter().image(
                    present_id,
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            } else {
                // GUI-PREVIEW-SCALE-1: draw the CPU texture through the painter
                // with the same fitted `rect` and full UVs as the GPU-present
                // path above. `ui.put(rect, Image::from_texture(..))` left the
                // image at its native texel size in points: `Image::new`
                // derives `ImageFit::Exact(tex.size)` for a texture source and
                // `Ui::put` only supplies `max_rect`, so `Exact` ignored the
                // available size. The CPU fallback therefore drew tiny (e.g.
                // the 4x3 sample) while the overlays mapped the fitted
                // `full_rect` — diverging visibly from the GPU path, which
                // already paints via `painter().image`. A direct painter blit
                // keeps zoom/pan/ROI (`preview_roi`, `preview_render_src`)
                // consistent: `rect` is the ROI-crop/draft-adjusted fit rect on
                // both paths and the texture is already the crop, so full UVs
                // map it 1:1.
                ui.painter().image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }

            // WB eyedropper needs the source-coordinate mapping, which is part
            // of the desktop capability set.
            if pick && response.clicked() {
                // R5-TOOLFLOW-1 (User-Entscheid 2026-09-20): the former geometry
                // hard-lock is replaced by the tool switch committing the active
                // crop/straighten draft; the pick is no longer refused.
                if let Some(pos) = response.interact_pointer_pos() {
                    if self.local_wb_pick_mode {
                        // The local picker samples the effective post-global
                        // stage in display/output coordinates. It deliberately
                        // does not reuse the global raw-source mapping.
                        let nx = ((pos.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0);
                        let ny = ((pos.y - rect.min.y) / rect.height().max(1.0)).clamp(0.0, 1.0);
                        if let Err(error) = self.pick_local_wb_at(nx as f64, ny as f64) {
                            self.show_error(error);
                        }
                    } else {
                        let full = self.image_dims().unwrap_or((1, 1));
                        // GUI-DRAFT-JUMP-1: map through the full-space ROI so
                        // the global pick lands on the same source pixel on
                        // both paths.
                        let roi = self.preview_roi.map(|r| {
                            Self::roi_in_full_pixels(r, full.0, full.1, self.preview_render_src)
                        });
                        let (nx, ny) = Self::to_normalized(pos, rect, roi, full);
                        self.pick_white_balance_at(nx as f64, ny as f64);
                    }
                }
            }
            if pick {
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(2.0_f32, crate::theme::ACCENT),
                    egui::StrokeKind::Middle,
                );
            }
            // G-14 red-eye region picker: a click marks a pupil at the clicked
            // source position (normalized). The overlays/region list live in the
            // Detail panel; geometry that blocks source mapping is refused
            // visibly, exactly like the WB eyedropper.
            if red_eye_pick {
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(2.0_f32, crate::theme::ACCENT),
                    egui::StrokeKind::Middle,
                );
                if response.clicked() {
                    // R5-TOOLFLOW-1 (User-Entscheid 2026-09-20): red-eye is now
                    // freed like masks/WB — the tool switch commits the active
                    // crop/straighten draft, so the pick is no longer refused
                    // with the stale geometry lock.
                    if let Some(pos) = response.interact_pointer_pos() {
                        let full = self.image_dims().unwrap_or((1, 1));
                        let roi = self.preview_roi.map(|r| {
                            Self::roi_in_full_pixels(r, full.0, full.1, self.preview_render_src)
                        });
                        let (nx, ny) = Self::to_normalized(pos, rect, roi, full);
                        if let Err(error) = self.add_red_eye_region(nx, ny) {
                            self.show_error(error);
                        }
                        // One click marks one region; the picker stays armed so
                        // several pupils can be marked in a row.
                    }
                }
            }
            self.handle_mask_tool_drag(ui, &response, rect, scale);
            // R5-DUST-23: the armed spot tool dabs on click and paints the
            // live-size circle cursor (the sidebar panel never wired a pointer
            // handler, so the tool did nothing on the image).
            self.handle_spot_tool_interaction(ui, &response, rect, scale);
            // Mask overlay is painted over the full-frame rect (accounting for the
            // current ROI crop) so it lines up with the zoomed/panned view.
            {
                let (full_w, full_h) = self.image_dims().unwrap_or((1, 1));
                // GUI-DRAFT-JUMP-1: the recorded ROI lives in render-source
                // pixels; scale it into full-source space so the overlay
                // lines up with the zoomed/panned view on both paths.
                let roi = self.preview_roi.map_or([0, 0, full_w, full_h], |r| {
                    Self::roi_in_full_pixels(r, full_w, full_h, self.preview_render_src)
                });
                let from_min = egui::pos2(
                    rect.min.x - roi[0] as f32 * scale,
                    rect.min.y - roi[1] as f32 * scale,
                );
                let full_rect = egui::Rect::from_min_size(
                    from_min,
                    egui::vec2(full_w as f32 * scale, full_h as f32 * scale),
                );
                // KITTEST-PARITY-PATHS-1: the overlay canvas the painter helpers
                // below (and the crop/focus/pin mapping) actually use.
                self.overlay_full_rect = Some(full_rect);
                self.draw_mask_overlay(ui, full_rect);
                self.draw_edit_pins(ui, full_rect);
                self.draw_lens_blur_overlay(ui, full_rect);
                self.draw_crop_overlay(ui, pane, full_rect);
            }
            ui.set_clip_rect(previous_clip);
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(Str::NoImage.t());
            });
        }
    }

    /// The prompt to display in the overlay: the live in-progress gesture while
    /// drawing, otherwise the selected mask's saved prompt (if any).
    pub(crate) fn current_overlay_prompt(&self) -> Option<MaskPrompt> {
        if self.drawing && self.mask_tool != MaskTool::None {
            let (start, end) = (self.drag_start?, self.drag_current?);
            return match self.mask_tool {
                MaskTool::Brush => self.pending_brush_overlay_prompt(),
                MaskTool::LinearGradient => Some(Self::gradient_prompt_from_drag(start, end)),
                MaskTool::Radial => Some(Self::ellipse_prompt_from_drag(start, end)),
                MaskTool::None => None,
            };
        }
        self.selected_mask_prompt()
    }
}
