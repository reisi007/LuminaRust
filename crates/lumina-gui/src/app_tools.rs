//! Spot-removal tool state (visualize/detect/distract/regenerate) and the
//! viewport math shared by preview, navigator and zoom.
//! The inherent `impl LuminaApp` block of `lib.rs`, moved verbatim
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100 — the last 1000-line Ratchet exception).
//! One inherent impl may be spread over several modules of the same crate, so
//! this is a pure structural move: every body below is byte-identical to its
//! `lib.rs` text. The only differences are this header, the `use super::*;`
//! glob and the `pub(crate)` tokens on the private helpers that a sibling
//! module calls (listed in `scripts/file_size_baseline.txt`).

use super::*;

impl LuminaApp {
    pub fn set_spot_tool(&mut self, tool: SpotTool) {
        instrument_gui_action!(self, GuiAction::SetSpotTool);
        self.spot_tool = tool;
        if tool != SpotTool::None {
            self.mask_tool = MaskTool::None;
            self.wb_pick_mode = false;
            self.local_wb_pick_mode = false;
            self.red_eye_pick_mode = false;
            self.clear_mask_gesture();
        }
    }
    pub fn spot_tool(&self) -> SpotTool {
        self.spot_tool
    }
    pub fn set_spot_mode(&mut self, mode: SpotMode) {
        instrument_gui_action!(self, GuiAction::SetSpotMode);
        self.spot_mode = mode;
    }
    pub fn spot_mode(&self) -> SpotMode {
        self.spot_mode
    }
    // ---- LRPAR-G04-REMOVE (G-04 Remove-Parität) ---------------------------

    /// Recipe-backed visualize threshold (`None` = off). Read-only accessor
    /// for the panel slider and headless tests.
    pub fn spot_visualize_threshold(&self) -> Option<f32> {
        self.recipe.spot_visualize_threshold()
    }

    /// Set (`Some(0..=1)`) or clear (`None`) the visualize threshold (G-04).
    /// Recipe-backed: persists through the debounced slider-save path
    /// ([`Self::commit_pending_slider_save`], `info!`-logged), so headless
    /// tests drive it without a timer. Loud on out-of-range values.
    pub fn set_spot_visualize(&mut self, threshold: Option<f32>) -> Result<(), GuiError> {
        if let Some(t) = threshold {
            if !t.is_finite() || !(0.0..=1.0).contains(&t) {
                return Err(GuiError::Io("Visualize threshold must be 0..=1".into()));
            }
        }
        self.recipe
            .set_spot_visualize_threshold(threshold)
            .map_err(|error| GuiError::Io(error.to_string()))?;
        match threshold {
            Some(t) => self.mark_recipe_dirty("spot.visualize", f64::from(t)),
            None => self.mark_recipe_dirty("spot.visualize", -1.0),
        }
        info!("GUI interaction: set_spot_visualize -> {threshold:?}");
        Ok(())
    }

    /// GUI-INSTRDBG-17b-REST: the "Visualize off" button command. Instrumented
    /// separately from [`Self::set_spot_visualize`] because the visualize
    /// *slider* commits through the same setter and a slider drag is not a
    /// button action (same split as `save_recipe_action`).
    pub(crate) fn clear_spot_visualize(&mut self) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::ClearSpotVisualize);
        self.set_spot_visualize(None)
    }

    /// Active visualize threshold for the preview gate (G04-FOLLOWUP-1):
    /// the recipe threshold ANDed with the G-11 overlay mode (`Always` = on
    /// as soon as the threshold is set, `Never` = off, `Auto` = only while a
    /// mask/spot tool is armed or a drag runs). Single gate for `render_from`
    /// (preview only — render/export/CLI stay untinted); headless-testable
    /// without pixels.
    pub fn spot_visualize_overlay_threshold(&self) -> Option<f32> {
        let threshold = self.recipe.spot_visualize_threshold()?;
        if self.overlay_visible() {
            Some(threshold)
        } else {
            None
        }
    }

    /// Effective heuristic detect threshold (G04-FOLLOWUP-1): the recipe
    /// visualize threshold when set, else the session input (default 0.5).
    /// The recipe value wins while set (it is persisted user intent); the
    /// session slider — synced from the recipe on load — applies after
    /// Clear or when no recipe value was ever set.
    pub fn spot_detect_effective_threshold(&self) -> f32 {
        self.recipe
            .spot_visualize_threshold()
            .unwrap_or(self.spot_detect_threshold)
    }

    /// Detection threshold input for heuristic Detect-Objects (`0..=1`).
    /// Session display state (never recipe); loud on bad values.
    pub fn set_spot_detect_threshold(&mut self, threshold: f32) -> Result<(), GuiError> {
        if !threshold.is_finite() || !(0.0..=1.0).contains(&threshold) {
            return Err(GuiError::Io("Detect threshold must be 0..=1".into()));
        }
        self.spot_detect_threshold = threshold;
        info!("GUI interaction: set_spot_detect_threshold -> {threshold}");
        Ok(())
    }

    /// Heuristic Detect-Objects (G-04, stage 1, no model): lists candidates on
    /// the loaded frame without persisting anything. The outcome text lands in
    /// `spot_detect_status` (visible, never silent); the candidates are
    /// returned for an explicit [`Self::apply_detected_spots`].
    pub fn detect_spot_candidates(&mut self) -> Result<Vec<DetectedSpot>, GuiError> {
        instrument_gui_action!(self, GuiAction::DetectSpotCandidates);
        let frame = self
            .original
            .as_ref()
            .ok_or_else(|| GuiError::Io(Str::NoImageLoaded.t().to_string()))?
            .clone();
        // G04-FOLLOWUP-1: the recipe visualize threshold is the default
        // when set (else the session input); the used value lands in the
        // status line so it is never silent.
        let threshold = self.spot_detect_effective_threshold();
        let candidates = detect_spots_heuristic(&frame, threshold, 32)?;
        info!(
            "GUI interaction: detect_spot_candidates -> {} candidate(s) at threshold {}",
            candidates.len(),
            threshold
        );
        self.spot_detect_status = format!(
            "Detected {} candidate(s) at threshold {:.2} (not applied — use Apply)",
            candidates.len(),
            threshold
        );
        Ok(candidates)
    }

    /// Persist detected candidates as heuristic spots (G-04, explicit only):
    /// one atomic recipe update + save + render. Never called implicitly —
    /// the panel wires it to an "Apply detected" button, `auto` modes only
    /// list.
    pub fn apply_detected_spots(&mut self, candidates: &[DetectedSpot]) -> Result<usize, GuiError> {
        if candidates.is_empty() {
            self.spot_detect_status = "No candidates to apply".into();
            return Ok(0);
        }
        let mut spots = self.spot_entries();
        for candidate in candidates {
            if !candidate.x.is_finite()
                || !candidate.y.is_finite()
                || !(0.0..=1.0).contains(&candidate.x)
                || !(0.0..=1.0).contains(&candidate.y)
            {
                return Err(GuiError::Io(
                    "Detected candidate has invalid coordinates".into(),
                ));
            }
            let id = format!(
                "spot-{}",
                blake3::hash(
                    format!(
                        "{:.6},{:.6},{:.2}",
                        candidate.x, candidate.y, candidate.radius
                    )
                    .as_bytes()
                )
                .to_hex()
            );
            if spots
                .iter()
                .any(|entry| entry.get("id").and_then(|value| value.as_str()) == Some(id.as_str()))
            {
                return Err(GuiError::Io(format!(
                    "Spot `{id}` already exists; IDs must be unique"
                )));
            }
            spots.push(serde_json::json!({
                "id": id, "version": 1, "mode": "heuristic",
                "center_x": candidate.x, "center_y": candidate.y,
                "radius": candidate.radius.clamp(1.0, 512.0),
                "feather": 0.0, "offset_dx": 0.05, "offset_dy": 0.0,
                "opacity": 1.0, "status": "valid",
            }));
        }
        let applied = candidates.len();
        self.store_spot_entries(&spots)?;
        self.mark_dirty();
        self.save_sidecar();
        // F4/F7: no swallowed render error on the spot-apply path — surface it
        // visibly (the applied spots are already persisted).
        if let Err(error) = self.render() {
            self.show_error(error);
        }
        info!("GUI interaction: apply_detected_spots -> {applied} spot(s)");
        self.spot_detect_status = format!("Applied {applied} detected spot(s)");
        Ok(applied)
    }

    /// GUI-INSTRDBG-17b-REST: the "Apply detected" button command. One outer
    /// instrumented action around the detector + persistence: the nested
    /// `detect_spot_candidates` is part of this action and must not add a
    /// second log line (the depth guard suppresses it).
    pub(crate) fn apply_detected_spot_objects(&mut self) -> Result<usize, GuiError> {
        instrument_gui_action!(self, GuiAction::ApplyDetectedSpots);
        let candidates = self.detect_spot_candidates()?;
        self.apply_detected_spots(&candidates)
    }

    /// Recipe-backed distraction switches (G-04). `auto_mode` only lists —
    /// applying stays explicit via [`Self::apply_detected_spots`] (the panel
    /// Apply button); enabling `auto` never persists spots by itself.
    pub fn spot_distraction(&self) -> SpotDistraction {
        self.recipe.spot_distraction()
    }

    pub fn set_spot_distraction(&mut self, setting: SpotDistraction) {
        instrument_gui_action!(self, GuiAction::SetSpotDistraction);
        self.recipe.set_spot_distraction(setting);
        self.mark_recipe_dirty(
            "spot.distraction",
            f64::from(setting.reflections as u8)
                + 2.0 * f64::from(setting.people as u8)
                + 4.0 * f64::from(setting.dust as u8)
                + 8.0 * f64::from(setting.auto_mode as u8),
        );
        info!("GUI interaction: set_spot_distraction -> {setting:?}");
    }

    /// Visible distraction status (G-04): per-kind outcome of
    /// [`distraction_candidates`] on the loaded frame — heuristic dust
    /// candidates or an explicit `needs model (F-078 gate)` marker for
    /// reflections/people. Never a silent substitute.
    pub fn distraction_status(&self) -> Vec<(String, String)> {
        let Some(frame) = self.original.as_ref() else {
            return vec![("none".into(), "no image loaded".into())];
        };
        let setting = self.recipe.spot_distraction();
        let core_setting = DistractionSetting {
            reflections: setting.reflections,
            people: setting.people,
            dust: setting.dust,
            auto_mode: setting.auto_mode,
        };
        let threshold = self.spot_detect_effective_threshold();
        let Ok(outcomes) = distraction_candidates(frame, core_setting, threshold, 32) else {
            return vec![("error".into(), "invalid threshold".into())];
        };
        outcomes
            .into_iter()
            .map(|(kind, status)| {
                let name = match kind {
                    DistractionKind::Reflections => "reflections",
                    DistractionKind::People => "people",
                    DistractionKind::Dust => "dust",
                }
                .to_string();
                let text = match status {
                    DistractionStatus::Ready(spots) => format!("{} candidate(s)", spots.len()),
                    DistractionStatus::NeedsModel { reason, .. } => {
                        format!("needs model (F-078 gate): {reason}")
                    }
                };
                (name, text)
            })
            .collect()
    }

    /// Session inputs for generative variant regeneration (G-04). Display
    /// state (never recipe until Regenerate); loud on empty prompts.
    pub fn set_spot_gen_inputs(&mut self, prompt: String, seed: u64, variant: u64) {
        self.spot_gen_prompt = prompt;
        self.spot_gen_seed = seed;
        self.spot_gen_variant = variant;
        info!("GUI interaction: set_spot_gen_inputs seed={seed} variant={variant}");
    }

    /// Regenerate a generative spot variant (G-04, explicit only): sets
    /// `seed = variant_seed(base, variant)` (+ `variant`, `prompt`) on the
    /// named extras entry, then saves. Heuristic entries and unknown ids fail
    /// loudly — a variant never silently retargets another spot.
    pub fn regenerate_spot_variant(&mut self, spot_id: &str) -> Result<u64, GuiError> {
        instrument_gui_action!(self, GuiAction::RegenerateSpotVariant);
        let derived = generative_variant_seed(self.spot_gen_seed, self.spot_gen_variant);
        let mut spots = self.spot_entries();
        let matches: Vec<usize> = spots
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                (entry.get("id").and_then(|v| v.as_str()) == Some(spot_id)).then_some(index)
            })
            .collect();
        let index = match matches.as_slice() {
            [] => return Err(GuiError::Io(format!("Unknown spot `{spot_id}`"))),
            [index] => *index,
            _ => {
                return Err(GuiError::Io(format!(
                    "Spot `{spot_id}` is ambiguous: duplicate ids are not valid targets"
                )))
            }
        };
        let mode = spots[index]
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("heuristic");
        if mode != "generative" {
            return Err(GuiError::Io(format!(
                "Spot `{spot_id}` is not generative (mode `{mode}`)"
            )));
        }
        spots[index]["seed"] = serde_json::json!(derived);
        spots[index]["variant"] = serde_json::json!(self.spot_gen_variant);
        spots[index]["base_seed"] = serde_json::json!(self.spot_gen_seed);
        if !self.spot_gen_prompt.is_empty() {
            spots[index]["prompt"] = serde_json::json!(self.spot_gen_prompt);
        }
        self.store_spot_entries(&spots)?;
        self.mark_dirty();
        self.save_sidecar();
        // F4/F7: no swallowed render error on the spot-variant path — surface
        // it visibly (the regenerated seed is already persisted).
        if let Err(error) = self.render() {
            self.show_error(error);
        }
        info!("GUI interaction: regenerate_spot_variant {spot_id} -> seed {derived}");
        self.spot_gen_status = format!(
            "Regenerated `{spot_id}` variant {} (seed {derived})",
            self.spot_gen_variant
        );
        Ok(derived)
    }

    /// Compute the source ROI `(x, y, w, h)` that is visible in the preview
    /// pane for a zoom factor `> 1.0` (PERF-GUI-5, REVIEW-GUI-PANROI-1).
    ///
    /// The visible window follows the pan offset: the drawn image centre sits
    /// at `pane.center() + pan`, so the source point currently behind the pane
    /// centre is the image centre shifted by `-pan / scale` where
    /// `scale = fit(w, h) * zoom` is the on-screen scale (screen points per
    /// source pixel). The returned rect is that window expanded by
    /// [`PREVIEW_ROI_MARGIN`] so the hand tool always has off-screen content
    /// to drag without an immediate re-render, and it is clamped to the image
    /// bounds — which is what makes borders/corners reachable at any zoom.
    ///
    /// Returns `None` at fit/zoom-out or when the window already covers the
    /// whole frame, so the entire image is rendered.
    pub(crate) fn roi_from_zoom(
        w: u32,
        h: u32,
        zoom: f32,
        pan: egui::Vec2,
        pane_w: f32,
        pane_h: f32,
    ) -> Option<[u32; 4]> {
        if zoom <= 1.0 || w == 0 || h == 0 {
            return None;
        }
        let (w, h) = (f64::from(w), f64::from(h));
        let (pane_w, pane_h) = (f64::from(pane_w), f64::from(pane_h));
        let fit = (pane_w / w).min(pane_h / h);
        if fit <= 0.0 {
            return None;
        }
        let scale = f64::from(zoom) * fit;
        // Visible window in source pixels, with margin for panning headroom.
        let vw = pane_w / scale * PREVIEW_ROI_MARGIN;
        let vh = pane_h / scale * PREVIEW_ROI_MARGIN;
        if vw >= w || vh >= h {
            return None;
        }
        let zw = (vw.floor() as u32).clamp(1, w as u32);
        let zh = (vh.floor() as u32).clamp(1, h as u32);
        // Source point under the pane centre (window centre), clamped so the
        // window never leaves the frame.
        let cx = w / 2.0 - f64::from(pan.x) / scale;
        let cy = h / 2.0 - f64::from(pan.y) / scale;
        let x = ((cx - vw / 2.0).floor() as i64).clamp(0, (w as u32 - zw) as i64) as u32;
        let y = ((cy - vh / 2.0).floor() as i64).clamp(0, (h as u32 - zh) as i64) as u32;
        Some([x, y, zw, zh])
    }

    /// Effective texture dimensions in *full-source* pixels (GUI-DRAFT-JUMP-1).
    ///
    /// A draft texture lives in downscaled render-source space, so drawing it
    /// at `tex_dims · scale` (full-source scale) comes out too small; scaling
    /// by `full/render_src` per axis restores the exact on-screen size of the
    /// equivalent full render. Full renders (`render_src == full`) and the
    /// `None` legacy state pass the dimensions through unchanged. Pure helper
    /// so the draft-vs-full placement roundtrip is unit-testable headless.
    pub(crate) fn preview_draw_dims(
        tex_w: f32,
        tex_h: f32,
        full_w: f32,
        full_h: f32,
        render_src: Option<(u32, u32)>,
    ) -> (f32, f32) {
        match render_src {
            Some((sw, sh)) if sw > 0 && sh > 0 && full_w > 0.0 && full_h > 0.0 => {
                (tex_w * full_w / sw as f32, tex_h * full_h / sh as f32)
            }
            _ => (tex_w, tex_h),
        }
    }

    /// Translate a render-source-space ROI into full-source pixels
    /// (GUI-DRAFT-JUMP-1): the inverse of the draft downscale, so pointer→
    /// source mapping and the mask overlay consume full-space rects regardless
    /// of which path rendered the texture. A `None`/degenerate source passes
    /// the rect through; results are clamped to the full frame. Pure helper
    /// for the headless draft-vs-full geometry test.
    pub(crate) fn roi_in_full_pixels(
        roi: [u32; 4],
        full_w: u32,
        full_h: u32,
        render_src: Option<(u32, u32)>,
    ) -> [u32; 4] {
        match render_src {
            Some((sw, sh)) if sw > 0 && sh > 0 && full_w > 0 && full_h > 0 => {
                let sx = full_w as f64 / sw as f64;
                let sy = full_h as f64 / sh as f64;
                let x = ((roi[0] as f64 * sx).round() as u32).min(full_w - 1);
                let y = ((roi[1] as f64 * sy).round() as u32).min(full_h - 1);
                let w = ((roi[2] as f64 * sx).round() as u32).clamp(1, full_w - x);
                let h = ((roi[3] as f64 * sy).round() as u32).clamp(1, full_h - y);
                [x, y, w, h]
            }
            _ => roi,
        }
    }

    /// Whether a pan gesture (modifier-free wheel over the preview, hand-tool
    /// drag) may pin the zoom mode to `Custom` (GUI-ZOOM-CUSTOM-1, F-100):
    /// only when actually zoomed in (`zoom > 1.0`) AND the drawn image
    /// overflows the pane. At Fit (or zoomed out) there is nothing to pan,
    /// so the gesture must never flip the readout to `Custom` — `Custom`
    /// arises solely from explicit zoom/pan of a magnified view. Pure helper
    /// so the Fit-guard is unit-testable headless.
    pub(crate) fn pan_gesture_pins_custom(
        zoom: f32,
        draw_w: f32,
        draw_h: f32,
        pane_w: f32,
        pane_h: f32,
    ) -> bool {
        zoom > 1.0 && (draw_w > pane_w + 0.5 || draw_h > pane_h + 0.5)
    }

    /// Open or collapse the navigator rail (GUI-PREVIEW-NAV-1). Pure view
    /// state — never touches the recipe or the sidecar.
    pub fn set_navigator_open(&mut self, open: bool) {
        trace!("GUI interaction: set_navigator_open {}", open);
        self.navigator_open = open;
    }

    /// Switch the preview zoom mode. Non-`Custom` modes re-derive `preview_zoom`
    /// from the current pane each frame (so they survive resizes); switching
    /// always re-centres the pan and triggers a re-render so the ROI crop
    /// matches the on-screen zoom. The re-render replaces the stale texture
    /// (GUI-FIT-1 texture-ROI identity: a Custom crop texture is never valid
    /// under Fit — `mark_dirty` arms its replacement, and `draw_preview`
    /// neutralizes any stale pan in non-`Custom` modes until it lands).
    pub fn set_zoom_mode(&mut self, mode: ZoomMode) {
        instrument_gui_action!(self, GuiAction::SetZoomMode);
        trace!("GUI interaction: set_zoom_mode {:?}", mode);
        self.zoom_mode = mode;
        self.preview_pan = egui::Vec2::ZERO;
        self.mark_dirty();
        // PREVIEW-CACHE-FEATURE (A6): a zoom-mode change to 1:1 (or back to Fit)
        // changes the neighbor preview kind/resolution; the current source's
        // cached neighbors no longer match the new key, so the +4/−2 window is
        // re-planned at the new resolution. Only native (the neighbor cache is a
        // native capability); no-op while nothing is loaded.
        if !self.path.is_empty() && self.original.is_some() {
            let active = self.path.clone();
            self.schedule_neighbor_previews(&active);
        }
    }

    /// Scroll-wheel / keyboard continuous zoom (relative-to-fit multiplier).
    /// Pins the mode to `Custom` so the next frame does not re-derive
    /// `preview_zoom`.
    pub fn zoom_step(&mut self, factor: f32) {
        let next = (self.preview_zoom * factor).clamp(0.05, 32.0);
        trace!(
            "GUI interaction: zoom_step factor={:.3} -> {:.3}",
            factor,
            next
        );
        self.preview_zoom = next;
        self.zoom_mode = ZoomMode::Custom;
        self.mark_dirty();
    }

    /// Re-derive `preview_zoom` (and reset pan) for non-`Custom` modes using
    /// the pane geometry and **un-cropped source dimensions** cached by the
    /// previous [`Self::draw_preview`] (REVIEW-GUI-ZOOMLOOP-1: deriving from
    /// the ROI-cropped texture's fit scale made 100%/200%/Fit-Width wrong and
    /// oscillate frame-by-frame). Called once per frame before the render logic
    /// so the ROI crop matches the on-screen zoom even on the frame a mode
    /// button/shortcut is pressed.
    pub(crate) fn sync_zoom(&mut self) {
        use ZoomMode::*;
        if self.zoom_mode == Custom {
            return;
        }
        // Fit of the pane against the un-cropped source, not the current
        // (possibly ROI-cropped) texture.
        let fit = self.preview_base_fit_scale.max(1e-6);
        let src_w = self.preview_src_w.max(1.0);
        self.preview_pan = egui::Vec2::ZERO;
        self.preview_zoom = match self.zoom_mode {
            Fit => 1.0,
            Quarter => 0.25 / fit,
            Half => 0.5 / fit,
            ThreeQuarter => 0.75 / fit,
            OneToOne => 1.0 / fit,
            TwoHundred => 2.0 / fit,
            FitWidth => (self.preview_pane_w / src_w) / fit,
            Custom => 1.0,
        };
    }

    /// Viewport rectangle for the navigator overview (GUI-PREVIEW-NAV-1): the
    /// currently visible Develop working area mapped into `nav_rect`, which
    /// shows the whole source (`src_w × src_h`) contain-fitted without
    /// letterboxing (the caller sizes it to the source aspect).
    ///
    /// `scale` is the on-screen preview scale (screen points per source pixel)
    /// and `pan` the preview pan offset. At fit (or degenerate geometry) the
    /// whole frame is visible and the returned rect equals `nav_rect`. Pure
    /// helper so the pan-rectangle roundtrip is unit-testable headless.
    pub(crate) fn navigator_viewport_rect(
        nav_rect: egui::Rect,
        src_w: f32,
        src_h: f32,
        pane_w: f32,
        pane_h: f32,
        scale: f32,
        pan: egui::Vec2,
    ) -> egui::Rect {
        if src_w <= 0.0 || src_h <= 0.0 || scale <= 0.0 || pane_w <= 0.0 || pane_h <= 0.0 {
            return nav_rect;
        }
        // Visible window in source pixels, centred on the source point behind
        // the pane centre (mirrors `roi_from_zoom` without the pan margin).
        let vw = (pane_w / scale).min(src_w);
        let vh = (pane_h / scale).min(src_h);
        if vw >= src_w && vh >= src_h {
            return nav_rect;
        }
        let cx = (src_w / 2.0 - pan.x / scale).clamp(vw / 2.0, src_w - vw / 2.0);
        let cy = (src_h / 2.0 - pan.y / scale).clamp(vh / 2.0, src_h - vh / 2.0);
        let to_nav_x = |x: f32| nav_rect.min.x + x / src_w * nav_rect.width();
        let to_nav_y = |y: f32| nav_rect.min.y + y / src_h * nav_rect.height();
        egui::Rect::from_min_max(
            egui::pos2(to_nav_x(cx - vw / 2.0), to_nav_y(cy - vh / 2.0)),
            egui::pos2(to_nav_x(cx + vw / 2.0), to_nav_y(cy + vh / 2.0)),
        )
    }

    /// Map a navigator drag onto the preview pan offset (GUI-PREVIEW-NAV-1):
    /// dragging the viewport rectangle by `drag_nav` (navigator points) moves
    /// the visible window with the cursor. `nav_scale` is navigator points per
    /// source pixel, `preview_scale` the on-screen preview scale. Pure helper
    /// for the headless pan-rectangle roundtrip test.
    pub(crate) fn pan_for_navigator_drag(
        pan: egui::Vec2,
        drag_nav: egui::Vec2,
        nav_scale: f32,
        preview_scale: f32,
    ) -> egui::Vec2 {
        if nav_scale <= 0.0 || preview_scale <= 0.0 {
            return pan;
        }
        pan - drag_nav * (preview_scale / nav_scale)
    }

    /// Composite zdata tile-record id shared with the CLI (REVIEW-CLI-N1):
    /// `"{copy_id}/{mask_id}"`. The field order mirrors the
    /// `(copy_id, mask_id)` planes key of `MaskContext`.
    pub(crate) fn zdata_tile_record_id(copy_id: &str, mask_id: &str) -> String {
        format!("{copy_id}/{mask_id}")
    }
}
