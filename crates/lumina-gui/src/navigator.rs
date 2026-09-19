//! GUI-REFACTOR-W2-20 S2.6: the navigator rail, extracted verbatim from
//! `lib.rs`.
//!
//! [`LuminaApp::navigator_frame`] resolves the overview frame,
//! [`LuminaApp::navigator_zoomed_overview`] the thumbnail-grade zoomed
//! overview (cached by content hash), [`LuminaApp::draw_navigator_viewport`]
//! maps/paints the draggable viewport rectangle and [`LuminaApp::draw_navigator`]
//! the collapsible rail. No behaviour changes: the ROI mapping, the `Custom`
//! pan pin and the `trace!`s are byte-identical. The externally called helpers
//! are `pub(crate)`.

use super::*;
use log::trace;

impl LuminaApp {
    /// Full-frame overview for the navigator (GUI-NAV-RECT-1): the viewport
    /// rectangle math maps full-source coordinates, so the overview image
    /// must be the full source too — never the ROI-cropped preview texture
    /// (at zoom the preview shows a crop; mapping full-source rect math onto
    /// a crop image doubles the error). At Fit the preview texture itself is
    /// full-frame and is reused verbatim; while zoomed a downscaled
    /// full-frame render with the current recipe is served from
    /// [`Self::navigator_zoomed_overview`] (thumbnail-grade, cached by
    /// source + recipe).
    pub(crate) fn navigator_frame(&self) -> Option<&ImageFrame> {
        self.original.as_ref()
    }

    /// Downscaled full-frame overview render with the current recipe for the
    /// zoomed navigator (GUI-NAV-RECT-1): the preview texture is an ROI crop
    /// while zoomed, so the overview renders the downscaled full source
    /// instead (masks stay out — full-resolution planes do not align with the
    /// downscaled source — exactly like the draft path). Returns the cached
    /// frame when source + recipe are unchanged, re-renders otherwise. `None`
    /// without a loaded source.
    fn navigator_zoomed_overview(&mut self) -> Option<ImageFrame> {
        let (width, height) = self
            .navigator_frame()
            .map(|frame| (frame.width, frame.height))?;
        let recipe_json = serde_json::to_vec(&self.recipe).ok()?;
        let digest = format!("blake3:{}", blake3::hash(&recipe_json).to_hex());
        let key = (self.path.clone(), width, height, digest);
        if self.navigator_overview_key.as_ref() == Some(&key) {
            return self.navigator_overview.clone();
        }
        let small = self
            .navigator_frame()
            .map(|frame| frame.downscale(NAVIGATOR_OVERVIEW_MAX_DIM))?;
        // G-06: Lensfun auto-corrector for the navigator overview (same
        // cached lookup as the preview render).
        #[cfg(feature = "lensfun")]
        self.ensure_lensfun_cache(small.width, small.height);
        #[cfg(feature = "lensfun")]
        let nav_lensfun = self.lensfun_render_ref();
        #[cfg(not(feature = "lensfun"))]
        let nav_lensfun = None;
        let context = RenderContext {
            recipe: &self.recipe,
            camera_white_balance: None,
            source_actions: &[],
            masks: None,
            lensfun: nav_lensfun,
            depth: None,
        };
        let frame = render_frame(&small, &context).map(|o| o.frame).ok()?;
        self.navigator_overview = Some(frame.clone());
        self.navigator_overview_key = Some(key);
        Some(frame)
    }

    /// Navigator viewport overview (GUI-PREVIEW-NAV-1): the full image with the
    /// currently visible Develop working-area rectangle. Dragging the rectangle
    /// pans (`preview_pan` + `mark_dirty`); panning pins the mode to `Custom`
    /// because absolute modes re-centre every frame (see [`Self::sync_zoom`]).
    pub(crate) fn draw_navigator_viewport(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let (src_w, src_h) = self.image_dims().unwrap_or((1, 1));
        if src_w == 0 || src_h == 0 {
            ui.label(Str::NotCurrent.t());
            return;
        }
        // GUI-NAV-RECT-1: overview texture from the FULL source (see
        // `navigator_frame`), refreshed only on source change. At Fit the
        // preview texture itself is full-frame and is reused verbatim (no
        // extra render, pixel-identical); while zoomed the cached downscaled
        // full-frame render stands in so rect math meets full-frame pixels.
        if self.preview_roi.is_none() {
            let key = (self.path.clone(), src_w, src_h);
            match self.texture.clone() {
                Some(texture) => {
                    self.navigator_texture = Some(texture);
                    self.navigator_texture_key = Some(key);
                }
                None => {
                    ui.label(Str::NotCurrent.t());
                    return;
                }
            }
        } else {
            let key = (self.path.clone(), src_w, src_h);
            let texture = self.navigator_zoomed_overview().map(|frame| {
                let size = [frame.width as usize, frame.height as usize];
                let image = egui::ColorImage::from_rgba_unmultiplied(size, &frame.pixels);
                ctx.load_texture("lumina-navigator", image, egui::TextureOptions::LINEAR)
            });
            match texture {
                Some(texture) => {
                    self.navigator_texture = Some(texture);
                    self.navigator_texture_key = Some(key);
                }
                None => {
                    ui.label(Str::NotCurrent.t());
                    return;
                }
            }
        }
        let texture = self
            .navigator_texture
            .clone()
            .expect("navigator texture set above");
        // Aspect-fitted overview (no letterboxing, so the navigator scale is
        // uniform on both axes and the drag mapping stays exact).
        let avail_w = ui.available_width().max(40.0);
        let height = (avail_w * src_h as f32 / src_w as f32).max(40.0);
        let size = egui::vec2(avail_w, height);
        let (nav_rect, response) = ui.allocate_exact_size(size, egui::Sense::drag());
        ui.put(nav_rect, egui::Image::from_texture(&texture).max_size(size));
        let scale = self.preview_effective_scale.max(1e-6);
        let view = Self::navigator_viewport_rect(
            nav_rect,
            src_w as f32,
            src_h as f32,
            self.preview_pane_w,
            self.preview_pane_h,
            scale,
            self.preview_pan,
        );
        ui.painter().rect_stroke(
            view,
            1.0_f32,
            egui::Stroke::new(2.0_f32, crate::theme::ACCENT),
            egui::StrokeKind::Middle,
        );
        let drag = response.drag_delta();
        if drag != egui::Vec2::ZERO {
            let nav_scale = (nav_rect.width() / src_w as f32).max(1e-6);
            self.preview_pan =
                Self::pan_for_navigator_drag(self.preview_pan, drag, nav_scale, scale);
            self.zoom_mode = ZoomMode::Custom;
            trace!("GUI interaction: navigator viewport drag");
            self.mark_dirty();
        }
        if self.preview_is_draft {
            ui.colored_label(egui::Color32::YELLOW, Str::Draft.t());
        }
    }

    /// Left thumbnail navigator rail (Lightroom-like). Reuses the filmstrip
    /// [`Self::ensure_thumbnail`] / [`ThumbnailManager`] pipeline — no duplicate
    /// thumbnail generation — shows a vertical scroll of directory entries,
    /// highlights the active image and opens an entry on click.
    pub(crate) fn draw_navigator(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(Str::Navigator.t());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("‹").clicked() {
                    self.navigator_open = false;
                    trace!("GUI interaction: navigator collapse");
                }
            });
        });
        ui.separator();
        self.draw_navigator_viewport(ctx, ui);
        ui.separator();
        // RAW-only: mirror the filmstrip filter so the left navigator rail shows
        // only RAW entries (jpg/png/webp are excluded from the Develop preview).
        // GUI-FILMSTRIP-DUP-1: one shared index source — each image once.
        // GUI-SCROLL-200-1: index view + `show_rows` — one fixed-height row per
        // entry, only the visible window is laid out and scheduled.
        let raw_indices: Vec<usize> = self.raw_entry_indices();
        let count = raw_indices.len();
        // UX-SLICE-2 (F3): honest empty hint — the same text as the filmstrip
        // ("No images in this folder") instead of "Click a thumbnail to open it"
        // when the rail carries no entries.
        ui.label(if count == 0 {
            Str::FilmstripEmpty.t()
        } else {
            Str::FilmstripHint.t()
        });
        const CELL_W: f32 = 120.0;
        const CELL_H: f32 = 90.0;
        let active_path = self.path.clone();
        let visible_rows = egui::ScrollArea::vertical()
            .show_rows(ui, CELL_H, count, |ui, rows: std::ops::Range<usize>| {
                for i in rows.clone() {
                    // Reuse the filmstrip thumbnail pipeline (no duplicate
                    // generation): ensure_thumbnail populates the shared
                    // ThumbnailManager entry.
                    let entry = self.entries[raw_indices[i]].clone();
                    self.ensure_thumbnail(ctx, &entry);
                    let tex = self.thumbnails.get(&entry.thumb_key).cloned();
                    let placeholder_label = self.thumbnail_placeholder_label(&entry);
                    let active = active_path == entry.path.display().to_string();
                    let (cell, resp) =
                        ui.allocate_exact_size(egui::vec2(CELL_W, CELL_H), egui::Sense::click());
                    if let Some(texture) = tex {
                        ui.put(
                            cell,
                            egui::Image::from_texture(&texture).max_size(cell.size()),
                        );
                    } else {
                        ui.painter()
                            .rect_filled(cell, 2.0, egui::Color32::from_gray(40));
                        ui.put(cell, egui::Label::new(placeholder_label));
                    }
                    if active {
                        ui.painter().rect_stroke(
                            cell,
                            2.0_f32,
                            egui::Stroke::new(2.0_f32, crate::theme::ACCENT),
                            egui::StrokeKind::Middle,
                        );
                    }
                    // PREVIEW-CACHE-FEATURE (A2): visible per-cell neighbor-preview
                    // state („wird vorbereitet / Veraltet / Fehler"), never only in
                    // logs. The thumb_key is the canonical path used as the probe id.
                    if let Some((text, color)) = self.neighbor_preview_badge(&entry.thumb_key) {
                        let corner_max = CELL_W.min(CELL_H) * 0.5;
                        let badge_w = corner_max + text.len() as f32 * 5.5 + 8.0;
                        let badge_h = corner_max + 8.0;
                        let badge_rect = egui::Rect::from_min_size(
                            egui::pos2(cell.min.x + 2.0, cell.min.y + 2.0),
                            egui::vec2(badge_w, badge_h),
                        );
                        ui.painter().rect_filled(badge_rect, 3.0, color);
                        ui.painter().text(
                            badge_rect.min + egui::vec2(5.0, 5.0),
                            egui::Align2::LEFT_TOP,
                            text,
                            egui::FontId::proportional(10.0),
                            egui::Color32::WHITE,
                        );
                    }
                    if resp.clicked() {
                        // GUI-FILMSTRIP-DUP-1: the rail shares the filmstrip
                        // selection — clicking here selects AND opens, exactly
                        // like a filmstrip click, so all views stay in sync.
                        trace!("GUI interaction: navigator open {}", entry.path.display());
                        self.handle_filmstrip_click(entry.path.display().to_string(), false, false);
                    }
                }
                rows
            })
            .inner;
        // GUI-SCROLL-200-1: visible-first scheduling + bounded prefetch for the
        // rail as well; show_rows covers the drawing side.
        let window = visible_rows.start..visible_rows.end.min(count);
        self.frame_thumb_enqueued += self.ensure_thumbnail_priority(ctx, &raw_indices, window);
    }
}
