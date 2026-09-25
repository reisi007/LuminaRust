//! GUI-REFACTOR-W2-20 S2.6: the navigator rail, extracted verbatim from
//! `lib.rs`.
//!
//! [`LuminaApp::navigator_frame`] resolves the overview frame,
//! [`LuminaApp::navigator_zoomed_overview`] the thumbnail-grade zoomed
//! overview (cached by content hash), [`LuminaApp::draw_navigator_viewport`]
//! maps/paints the draggable viewport rectangle and [`LuminaApp::draw_navigator`]
//! the collapsible navigator panel. R4-UX-1 (2026-09-20) removed the duplicate
//! thumbnail rail from `draw_navigator`; R4-NAV-1 gated the drag-to-pan on a
//! genuinely magnified view. The externally called helpers are `pub(crate)`.

use super::*;
use log::{trace, warn};
use lumina_core::{render_frame_with_denoise, DenoiseStageInput, DenoiseStageStatus};

// R4-NAV-1: test-only capture of the navigator viewport rectangle painted by
// the last `draw_navigator_viewport` call on this thread. A pure test seam
// (like `timing::TIMING_LOG`) so a headless test can drive the real draw path
// and observe where the box actually lands — not only the pure helper. No
// production state, no `LuminaApp` field (lib.rs is under the file-size
// ratchet).
#[cfg(test)]
thread_local! {
    static LAST_NAV_VIEW_RECT: std::cell::Cell<Option<egui::Rect>> =
        const { std::cell::Cell::new(None) };
}

/// Test-only: the navigator viewport rectangle painted by the last
/// `draw_navigator_viewport` call on this thread (`None` before the first
/// paint). Read-only; the value is overwritten each paint.
#[cfg(test)]
pub(crate) fn last_navigator_view_rect() -> Option<egui::Rect> {
    LAST_NAV_VIEW_RECT.with(|cell| cell.get())
}

/// Reset the test-only navigator viewport capture.
#[cfg(test)]
pub(crate) fn clear_last_navigator_view_rect() {
    LAST_NAV_VIEW_RECT.with(|cell| cell.set(None));
}

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
    ///
    /// R3-DENOISE-1 (B4): the render uses the **session denoise policy** like
    /// the active preview / neighbor worker ([`Self::denoise_policy`]). The
    /// former `render_frame` default (`Strict`) hard-failed for any active
    /// `denoise_ai` while the session policy was `Warn`, and the `.ok()?`
    /// swallowed it into a silent "not current" navigator — a silent path split.
    /// The downscaled stand-in cannot blend the full-frame artifact, so the
    /// stage is resolved as non-ready; `Warn` falls back visibly, `Strict`
    /// aborts. A genuine failure is logged once per (source, recipe) key and
    /// remembered so it cannot spam per frame.
    pub(crate) fn navigator_zoomed_overview(&mut self) -> Option<ImageFrame> {
        if let Some(reason) = self.local_adjustment_route_reason() {
            warn!("navigator stand-in refused: {reason}");
            self.status = reason;
            self.navigator_overview = None;
            self.navigator_overview_key = None;
            return None;
        }
        let source = self.navigator_frame()?;
        let (width, height) = (source.width, source.height);
        // GUI-SRCACC-1: the zoomed overview is a stand-in, not an exemption.
        // Resolve and apply repair regions at full source geometry before the
        // overview downscale; resolver/composite errors are logged and yield
        // the existing visible `Not current` state, never a raw approximation.
        let source_actions = match self.resolve_current_source_actions(source) {
            Ok(actions) => actions,
            Err(error) => {
                warn!("navigator source-action resolution failed: {error}");
                self.navigator_overview = None;
                self.navigator_overview_key = None;
                return None;
            }
        };
        let recipe_json = serde_json::to_vec(&self.recipe).ok()?;
        let source_content_identity = self
            .source_bytes
            .as_ref()
            .map(|bytes| format!("blake3:{}:{}", blake3::hash(bytes).to_hex(), bytes.len()))
            .unwrap_or_else(|| "blake3:unknown:0".to_owned());
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"source-content\0");
        hasher.update(source_content_identity.as_bytes());
        hasher.update(b"\0recipe\0");
        hasher.update(&recipe_json);
        for identity in source_actions.identities() {
            hasher.update(&[0]);
            hasher.update(identity.id.as_bytes());
            hasher.update(&[0]);
            hasher.update(identity.checksum.as_bytes());
        }
        let digest = format!("blake3:{}", hasher.finalize().to_hex());
        let key = (self.path.clone(), width, height, digest);
        if self.navigator_overview_key.as_ref() == Some(&key) {
            return self.navigator_overview.clone();
        }
        let small = if source_actions.is_empty() {
            source.downscale(NAVIGATOR_OVERVIEW_MAX_DIM)
        } else {
            let mut source_work = StageWork::default();
            match prepare_source_base(source, source_actions.artifacts(), &mut source_work) {
                Ok(frame) => frame.downscale(NAVIGATOR_OVERVIEW_MAX_DIM),
                Err(error) => {
                    warn!("navigator source-action composition failed: {error}");
                    self.navigator_overview = None;
                    self.navigator_overview_key = None;
                    return None;
                }
            }
        };
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
        let denoise = DenoiseStageInput::non_ready(
            DenoiseStageStatus::Unavailable,
            "navigator overview stand-in: the full-frame denoise artifact is \
             applied only to the active full-resolution render",
        )
        .with_policy(self.denoise_policy());
        let frame = match render_frame_with_denoise(&small, &context, &denoise) {
            Ok(output) => output.frame,
            Err(error) => {
                warn!("navigator overview render failed: {error}");
                // Remember the failure under this key so a persistent error
                // (e.g. an explicit `Strict` policy) logs once, not per frame.
                self.navigator_overview = None;
                self.navigator_overview_key = Some(key);
                return None;
            }
        };
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
        #[cfg(test)]
        LAST_NAV_VIEW_RECT.with(|cell| cell.set(Some(view)));
        let drag = response.drag_delta();
        // R4-NAV-1: only a genuinely magnified, overflowing view has a window
        // to move. At Fit (or zoom <= 1) the viewport rectangle IS the whole
        // frame, so a drag must neither move `preview_pan` nor pin `Custom` —
        // otherwise the state silently changes while the box stays glued (the
        // reported "box does not follow the drag"). Same gate as the preview
        // hand-tool pan (`pan_gesture_pins_custom`).
        if drag != egui::Vec2::ZERO
            && Self::navigator_drag_pans_preview(
                self.preview_zoom,
                src_w as f32,
                src_h as f32,
                scale,
                self.preview_pane_w,
                self.preview_pane_h,
            )
        {
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

    /// R4-NAV-1: whether a navigator drag may pan the preview. Only a
    /// genuinely magnified view (`zoom > 1`) whose full-source draw overflows
    /// the pane has an off-screen window to move — exactly the preview
    /// hand-tool gate ([`Self::pan_gesture_pins_custom`]). `scale` is the
    /// on-screen preview scale (screen points per source pixel) and
    /// `src_w * scale` / `src_h * scale` the full-source draw size at that
    /// scale. Pure helper so the gate is headless-testable without a pane.
    pub(crate) fn navigator_drag_pans_preview(
        zoom: f32,
        src_w: f32,
        src_h: f32,
        scale: f32,
        pane_w: f32,
        pane_h: f32,
    ) -> bool {
        Self::pan_gesture_pins_custom(zoom, src_w * scale, src_h * scale, pane_w, pane_h)
    }

    /// Left navigator panel (Lightroom-like): the overview with the draggable
    /// viewport rectangle.
    ///
    /// R4-UX-1 (User-Entscheid 2026-09-20): the former thumbnail rail below the
    /// viewport is removed. It duplicated the same directory images as the
    /// bottom filmstrip ("Click a thumbnail to open it"), which F-100 already
    /// designates as the single selection surface in all three modules; the
    /// left panel now carries only the Navigator viewport (plus
    /// Presets/Snapshots/History in Develop). No selection path is lost — the
    /// filmstrip keeps click/⌘-click/⇧-click and its neighbor-preview badges.
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
    }
}
