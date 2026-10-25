//! Optics and geometry: lens profile/correction, lens blur, crop/aspect, rotate,
//! mirror, perspective and upright.
//! The inherent `impl LuminaApp` block of `lib.rs`, moved verbatim
//! (FILE-SIZE-1000-ALL-EXCEPTIONS-100 — the last 1000-line Ratchet exception).
//! One inherent impl may be spread over several modules of the same crate, so
//! this is a pure structural move: every body below is byte-identical to its
//! `lib.rs` text. The only differences are this header, the `use super::*;`
//! glob and the `pub(crate)` tokens on the private helpers that a sibling
//! module calls (listed in `scripts/file_size_baseline.txt`).

use super::*;

impl LuminaApp {
    /// Remove the manual lens profile (keeps the coefficients). Arms one
    /// G-06 history step.
    pub fn clear_lens_profile(&mut self) {
        instrument_gui_action!(self, GuiAction::ClearLensProfile);
        if let Some(lens) = self.recipe.lens_correction.as_mut() {
            lens.profile = None;
        }
        self.mark_recipe_dirty("lens_correction.profile_clear", 0.0);
        self.pending_history_step = Some("lens_correction.profile_clear".into());
        info!("GUI interaction: clear_lens_profile");
    }

    /// Set one lens-correction field (`distortion_k1`…`ca_blue`) and record the
    /// save commit (GUI-SLIDER-SAVE-1). Unknown names are ignored loudly.
    /// Arms one G-06 history step.
    pub(crate) fn set_lens_correction_value(&mut self, field: &str, value: f64) {
        let mut lc = self
            .recipe
            .lens_correction
            .clone()
            .unwrap_or(LensCorrection {
                version: 1,
                profile: None,
                distortion_k1: None,
                distortion_k2: None,
                distortion_k3: None,
                vignette_c0: None,
                vignette_c1: None,
                vignette_c2: None,
                ca_red: None,
                ca_blue: None,
            });
        let slot = match field {
            "distortion_k1" => &mut lc.distortion_k1,
            "distortion_k2" => &mut lc.distortion_k2,
            "distortion_k3" => &mut lc.distortion_k3,
            "vignette_c0" => &mut lc.vignette_c0,
            "vignette_c1" => &mut lc.vignette_c1,
            "vignette_c2" => &mut lc.vignette_c2,
            "ca_red" => &mut lc.ca_red,
            "ca_blue" => &mut lc.ca_blue,
            _ => {
                warn!("set_lens_correction_value: unknown field {field}");
                return;
            }
        };
        *slot = Some(value as f32);
        self.recipe.lens_correction = Some(lc);
        self.mark_recipe_dirty(&format!("lens_correction.{field}"), value);
        self.pending_history_step = Some(format!("lens_correction.{field}"));
        info!("GUI interaction: set_lens_correction_value {field} -> {value}");
    }

    /// Current lens-blur stage (G-05), if the active virtual copy carries one.
    pub fn lens_blur(&self) -> Option<LensBlur> {
        self.recipe.lens_blur.clone()
    }

    /// User-visible Lensfun auto status (G-06): EXIF snapshot plus the
    /// manual profile state. Always visible, never implied; DB-backed
    /// profile matching itself happens at render time (CLI
    /// `--lensfun-status` resolves it on demand for one file).
    pub fn lensfun_auto_status_text(&self) -> String {
        #[cfg(not(feature = "lensfun"))]
        {
            "unavailable in this build (manual correction applies)".into()
        }
        #[cfg(feature = "lensfun")]
        {
            let exif = match &self.loaded_lens_identity {
                Some(identity) => {
                    let mut parts = Vec::new();
                    if let Some(make) = &identity.camera_make {
                        parts.push(make.clone());
                    }
                    if let Some(model) = &identity.camera_model {
                        parts.push(model.clone());
                    }
                    if let Some(lens) = &identity.lens {
                        parts.push(format!("({lens})"));
                    }
                    if parts.is_empty() {
                        "partial EXIF".into()
                    } else {
                        parts.join(" ")
                    }
                }
                None => "no EXIF".into(),
            };
            let manual = match self
                .recipe
                .lens_correction
                .as_ref()
                .and_then(|lens| lens.profile.as_deref())
            {
                Some(name) => format!("manual profile `{name}`"),
                None => "no manual profile".to_string(),
            };
            format!("{exif}; {manual}; auto applies at render when a system profile matches")
        }
    }

    /// User-visible lens-blur depth status (G-05): `off`, `heuristic active`
    /// or `missing depth artifact`. The GUI never resolves external depth
    /// files (no depth format in v1), so a referenced artifact reports
    /// `missing` until a loader exists — identical to the CLI contract.
    /// DEPTH-PLUMBING-1 (Entscheid 2026-09-16): runtime binding of external
    /// depth is deliberately Post-MVP; v1 persists/reports the reserved
    /// reference and a set reference aborts the render loudly.
    /// (`feature/architecture/pipeline.md` § „External-Depth-Bindung“.)
    pub fn lens_blur_status_text(&self) -> String {
        lumina_core::lens_blur_status(self.recipe.lens_blur.as_ref(), false).into()
    }

    /// Focus-rectangle overlay rect (G-05) in preview-image coordinates:
    /// the normalized recipe rect mapped into `img_rect`. `None` when no
    /// enabled stage with a valid rect exists. Pure helper so the mapping is
    /// unit-testable headless.
    pub fn lens_blur_focus_overlay(
        img_rect: egui::Rect,
        blur: Option<&LensBlur>,
    ) -> Option<egui::Rect> {
        let b = blur.filter(|b| b.enabled)?;
        let r = &b.focus_rect;
        if !(0.0..=1.0).contains(&r.x)
            || !(0.0..=1.0).contains(&r.y)
            || r.width <= 0.0
            || r.height <= 0.0
            || r.x + r.width > 1.0
            || r.y + r.height > 1.0
        {
            return None;
        }
        let min = img_rect.min + egui::vec2(r.x * img_rect.width(), r.y * img_rect.height());
        let max = img_rect.min
            + egui::vec2(
                (r.x + r.width) * img_rect.width(),
                (r.y + r.height) * img_rect.height(),
            );
        Some(egui::Rect::from_min_max(min, max))
    }

    /// Crop-rectangle overlay rect (G-06) in preview-image coordinates: the
    /// normalized recipe crop mapped into `img_rect`. Aspect presets resolve
    /// to the same centered rectangle as the core `crop_rect` (needs the
    /// source dimensions); free rects map directly. `None` when no crop is
    /// set or the rect is invalid. Pure helper so the mapping is
    /// unit-testable headless.
    pub fn crop_overlay_rect(
        img_rect: egui::Rect,
        crop: Option<&Crop>,
        src_w: u32,
        src_h: u32,
    ) -> Option<egui::Rect> {
        let (x, y, w, h) = match crop? {
            Crop::Free {
                x,
                y,
                width,
                height,
            } => (*x, *y, *width, *height),
            Crop::Aspect { preset } => {
                if src_w == 0 || src_h == 0 {
                    return None;
                }
                let ratio = match preset {
                    AspectPreset::Original => f64::from(src_w) / f64::from(src_h),
                    AspectPreset::OneToOne => 1.0,
                    AspectPreset::FourToFive => 4.0 / 5.0,
                    AspectPreset::FiveToFour => 5.0 / 4.0,
                    AspectPreset::ThreeToTwo => 3.0 / 2.0,
                    AspectPreset::TwoToThree => 2.0 / 3.0,
                    AspectPreset::FourToThree => 4.0 / 3.0,
                    AspectPreset::ThreeToFour => 3.0 / 4.0,
                    AspectPreset::SixteenToNine => 16.0 / 9.0,
                    AspectPreset::NineToSixteen => 9.0 / 16.0,
                };
                let source_ratio = f64::from(src_w) / f64::from(src_h);
                if source_ratio > ratio {
                    (
                        ((1.0 - ratio / source_ratio) / 2.0) as f32,
                        0.0,
                        (ratio / source_ratio) as f32,
                        1.0,
                    )
                } else {
                    (
                        0.0,
                        ((1.0 - source_ratio / ratio) / 2.0) as f32,
                        1.0,
                        (source_ratio / ratio) as f32,
                    )
                }
            }
        };
        if !x.is_finite() || !y.is_finite() || !w.is_finite() || !h.is_finite() {
            return None;
        }
        if !(0.0..=1.0).contains(&x)
            || !(0.0..=1.0).contains(&y)
            || w <= 0.0
            || h <= 0.0
            || x + w > 1.0
            || y + h > 1.0
        {
            return None;
        }
        let min = img_rect.min + egui::vec2(x * img_rect.width(), y * img_rect.height());
        let max =
            img_rect.min + egui::vec2((x + w) * img_rect.width(), (y + h) * img_rect.height());
        Some(egui::Rect::from_min_max(min, max))
    }
    /// stage with centered defaults when none exists (same defaults as the
    /// CLI `lens-blur` command: touching lens blur enables it).
    fn lens_blur_mut(&mut self) -> &mut LensBlur {
        self.recipe.lens_blur.get_or_insert(LensBlur {
            version: 1,
            enabled: true,
            focus_rect: FocusRect {
                x: 0.25,
                y: 0.25,
                width: 0.5,
                height: 0.5,
            },
            focal_near: 0.0,
            focal_far: 0.2,
            blur_amount: 0.5,
            bokeh: BokehShape::Round,
            depth_artifact: None,
        })
    }

    /// Enable/disable the lens-blur stage (G-05). Values are kept, so
    /// disabling renders identity and re-enabling restores the look.
    /// Recipe-backed: persists through the debounced slider-save path
    /// ([`Self::commit_pending_slider_save`], `info!`-logged).
    pub fn set_lens_blur_enabled(&mut self, enabled: bool) {
        instrument_gui_action!(self, GuiAction::SetLensBlurEnabled);
        self.lens_blur_mut().enabled = enabled;
        self.mark_recipe_dirty("lens_blur.enabled", f64::from(enabled as u8));
        info!("GUI interaction: set_lens_blur_enabled -> {enabled}");
    }

    /// Set the blur strength `0..=1` (G-05, 0 is identity). Loud on
    /// out-of-range/non-finite values; the recipe is untouched then.
    pub fn set_lens_blur_amount(&mut self, amount: f64) -> Result<(), GuiError> {
        if !amount.is_finite() || !(0.0..=1.0).contains(&amount) {
            return Err(GuiError::Io(format!(
                "Lens blur amount must be 0..=1, got {amount}"
            )));
        }
        self.lens_blur_mut().blur_amount = amount as f32;
        self.mark_recipe_dirty("lens_blur.blur_amount", amount);
        info!("GUI interaction: set_lens_blur_amount -> {amount}");
        Ok(())
    }

    /// Set the sharp depth band `[near, far]` in `0..=1` (G-05). Loud when
    /// either edge is out of range or `near > far`; the recipe is untouched
    /// then.
    pub fn set_lens_blur_focal(&mut self, near: f64, far: f64) -> Result<(), GuiError> {
        for (name, v) in [("focal_near", near), ("focal_far", far)] {
            if !v.is_finite() || !(0.0..=1.0).contains(&v) {
                return Err(GuiError::Io(format!(
                    "Lens blur {name} must be 0..=1, got {v}"
                )));
            }
        }
        if near > far {
            return Err(GuiError::Io(format!(
                "Lens blur focal range must satisfy near <= far, got {near} > {far}"
            )));
        }
        let blur = self.lens_blur_mut();
        blur.focal_near = near as f32;
        blur.focal_far = far as f32;
        self.mark_recipe_dirty("lens_blur.focal_near", near);
        self.mark_recipe_dirty("lens_blur.focal_far", far);
        info!("GUI interaction: set_lens_blur_focal -> {near}..={far}");
        Ok(())
    }

    /// Set the bokeh kernel shape (G-05). All three shapes are deterministic
    /// integer kernels (no randomness).
    pub fn set_lens_blur_bokeh(&mut self, bokeh: BokehShape) {
        instrument_gui_action!(self, GuiAction::SetLensBlurBokeh);
        self.lens_blur_mut().bokeh = bokeh;
        self.mark_recipe_dirty(
            "lens_blur.bokeh",
            match bokeh {
                BokehShape::Round => 0.0,
                BokehShape::Elliptical => 1.0,
                BokehShape::Hexagonal => 2.0,
            },
        );
        info!("GUI interaction: set_lens_blur_bokeh -> {bokeh:?}");
    }

    /// Set the focus rectangle in normalized `0..=1` coordinates (G-05).
    /// Loud on degenerate/out-of-bounds rectangles; the recipe is untouched
    /// then (the save-time validator rejects them too — never a silent
    /// reinterpretation).
    pub fn set_lens_blur_focus_rect(
        &mut self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) -> Result<(), GuiError> {
        for (name, v) in [("x", x), ("y", y), ("width", width), ("height", height)] {
            if !v.is_finite() {
                return Err(GuiError::Io(format!(
                    "Lens blur focus rect {name} must be finite, got {v}"
                )));
            }
        }
        if !(0.0..=1.0).contains(&x)
            || !(0.0..=1.0).contains(&y)
            || width <= 0.0
            || height <= 0.0
            || x + width > 1.0
            || y + height > 1.0
        {
            return Err(GuiError::Io(format!(
                "Lens blur focus rect must be a positive area inside 0..=1, got {x},{y},{width},{height}"
            )));
        }
        self.lens_blur_mut().focus_rect = FocusRect {
            x: x as f32,
            y: y as f32,
            width: width as f32,
            height: height as f32,
        };
        self.mark_recipe_dirty("lens_blur.focus_rect", x + y + width + height);
        info!("GUI interaction: set_lens_blur_focus_rect -> {x},{y},{width},{height}");
        Ok(())
    }

    /// Remove the whole lens-blur stage (G-05, back to identity).
    pub fn clear_lens_blur(&mut self) {
        self.recipe.lens_blur = None;
        self.mark_recipe_dirty("lens_blur.clear", 0.0);
        info!("GUI interaction: clear_lens_blur");
    }

    /// Set the geometry rotation and record the save commit
    /// (GUI-SLIDER-SAVE-1). Public so headless/integration harnesses and
    /// future shortcuts drive the same path as the Geometry slider
    /// (GUI-ROTATE-1: one wired path, no shadow state). Arms one G-06
    /// history step consumed at save time.
    pub fn set_geometry_rotation(&mut self, degrees: f64) {
        let mut geo = self.recipe.geometry.clone().unwrap_or(Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        });
        geo.rotation_degrees = degrees as f32;
        self.recipe.geometry = Some(geo);
        self.mark_recipe_dirty("geometry.rotation_degrees", degrees);
        self.pending_history_step = Some("geometry.rotation".into());
        info!("GUI interaction: set_geometry_rotation -> {degrees}");
    }

    /// Straighten angle (G-06, LRPAR-G06-GEO): documented alias of
    /// [`Self::set_geometry_rotation`] — same field
    /// (`geometry.rotation_degrees`), same validation, same render effect.
    /// Commits through the rotation path so slider, button and straighten
    /// control can never diverge; the history step is labelled
    /// `geometry.straighten`.
    pub fn set_straighten(&mut self, degrees: f64) {
        self.set_geometry_rotation(degrees);
        self.pending_history_step = Some("geometry.straighten".into());
        info!("GUI interaction: set_straighten -> {degrees}");
    }

    /// Set the crop to an aspect preset (G-06). Unknown names are rejected
    /// loudly without touching the recipe. Arms one G-06 history step.
    pub fn set_crop_aspect(&mut self, preset: &str) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetCropAspect);
        let parsed = match preset {
            "original" => AspectPreset::Original,
            "1:1" => AspectPreset::OneToOne,
            "4:5" => AspectPreset::FourToFive,
            "5:4" => AspectPreset::FiveToFour,
            "3:2" => AspectPreset::ThreeToTwo,
            "2:3" => AspectPreset::TwoToThree,
            "4:3" => AspectPreset::FourToThree,
            "3:4" => AspectPreset::ThreeToFour,
            "16:9" => AspectPreset::SixteenToNine,
            "9:16" => AspectPreset::NineToSixteen,
            _ => {
                return Err(GuiError::Io(format!(
                    "Unknown aspect preset `{preset}` (expected original|1:1|4:5|5:4|3:2|2:3|4:3|3:4|16:9|9:16)"
                )));
            }
        };
        let mut geo = self.recipe.geometry.clone().unwrap_or(Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        });
        geo.crop = Some(Crop::Aspect { preset: parsed });
        self.recipe.geometry = Some(geo);
        self.mark_recipe_dirty("geometry.crop_aspect", 0.0);
        self.pending_history_step = Some("geometry.crop_aspect".into());
        info!("GUI interaction: set_crop_aspect -> {preset}");
        Ok(())
    }

    /// Set a free crop rectangle in normalized `0..=1` coordinates (G-06).
    /// Non-finite inputs are rejected loudly without touching the recipe;
    /// deeper rect validation (positive area, frame bounds) runs at save
    /// time via the sidecar validator. Arms one G-06 history step.
    pub fn set_crop_free(
        &mut self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) -> Result<(), GuiError> {
        for (name, value) in [("x", x), ("y", y), ("width", width), ("height", height)] {
            if !value.is_finite() {
                return Err(GuiError::Io(format!(
                    "Crop rect {name} must be finite, got {value}"
                )));
            }
        }
        let mut geo = self.recipe.geometry.clone().unwrap_or(Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        });
        geo.crop = Some(Crop::Free {
            x: x as f32,
            y: y as f32,
            width: width as f32,
            height: height as f32,
        });
        self.recipe.geometry = Some(geo);
        self.mark_recipe_dirty("geometry.crop_free", width);
        self.pending_history_step = Some("geometry.crop_free".into());
        info!("GUI interaction: set_crop_free -> {x},{y},{width},{height}");
        Ok(())
    }

    /// Remove the crop (back to the full frame, keeps rotation/mirrors).
    /// Arms one G-06 history step.
    pub fn clear_crop(&mut self) {
        instrument_gui_action!(self, GuiAction::ClearCrop);
        if let Some(geo) = self.recipe.geometry.as_mut() {
            geo.crop = None;
        }
        self.mark_recipe_dirty("geometry.crop_clear", 0.0);
        self.pending_history_step = Some("geometry.crop_clear".into());
        info!("GUI interaction: clear_crop");
    }

    /// Set the manual lens profile by name (G-06). Only the Core whitelist
    /// is accepted; anything else is rejected loudly without touching the
    /// recipe (the sidecar validator is the second gate at save time).
    pub fn set_lens_profile(&mut self, profile: &str) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetLensProfile);
        if !matches!(profile, "wide-light" | "tele-light" | "standard-neutral") {
            return Err(GuiError::Io(format!(
                "Unknown lens profile `{profile}` (expected wide-light|tele-light|standard-neutral)"
            )));
        }
        let mut lc = self
            .recipe
            .lens_correction
            .clone()
            .unwrap_or(LensCorrection {
                version: 1,
                profile: None,
                distortion_k1: None,
                distortion_k2: None,
                distortion_k3: None,
                vignette_c0: None,
                vignette_c1: None,
                vignette_c2: None,
                ca_red: None,
                ca_blue: None,
            });
        lc.profile = Some(profile.into());
        self.recipe.lens_correction = Some(lc);
        self.mark_recipe_dirty("lens_correction.profile", 0.0);
        self.pending_history_step = Some("lens_correction.profile".into());
        info!("GUI interaction: set_lens_profile -> {profile}");
        Ok(())
    }

    /// Rotate by a relative step in degrees (GUI-ROTATE-1: the ±90° quick
    /// buttons). Normalizes into `(-180.0, 180.0]` and commits through
    /// [`Self::set_geometry_rotation`] so button, slider and (future)
    /// shortcut share one save path.
    pub fn rotate_step(&mut self, delta_degrees: f64) {
        instrument_gui_action!(self, GuiAction::RotateStep);
        let current = self
            .recipe
            .geometry
            .as_ref()
            .map(|g| f64::from(g.rotation_degrees))
            .unwrap_or(0.0);
        let mut next = (current + delta_degrees) % 360.0;
        if next <= -180.0 {
            next += 360.0;
        } else if next > 180.0 {
            next -= 360.0;
        }
        info!("GUI interaction: rotate_step {delta_degrees:+} -> {next}");
        self.set_geometry_rotation(next);
    }

    /// Set the geometry mirror flags and record the save commit
    /// (GUI-SLIDER-SAVE-1). Public for the same reason as
    /// [`Self::set_geometry_rotation`]. Arms one G-06 history step.
    pub fn set_geometry_mirror(&mut self, horizontal: bool, vertical: bool) {
        instrument_gui_action!(self, GuiAction::SetGeometryMirror);
        let mut geo = self.recipe.geometry.clone().unwrap_or(Geometry {
            version: 1,
            crop: None,
            rotation_degrees: 0.0,
            mirror_horizontal: false,
            mirror_vertical: false,
        });
        geo.mirror_horizontal = horizontal;
        geo.mirror_vertical = vertical;
        self.recipe.geometry = Some(geo);
        self.mark_recipe_dirty(
            "geometry.mirror",
            f64::from(u8::from(horizontal) * 2 + u8::from(vertical)),
        );
        self.pending_history_step = Some("geometry.mirror".into());
        info!("GUI interaction: set_geometry_mirror -> h={horizontal} v={vertical}");
    }

    /// Set one perspective field (`vertical`/`horizontal`/`rotation`/`scale`/
    /// `aspect_ratio`/`shift_x`/`shift_y`) and record the save commit
    /// (GUI-SLIDER-SAVE-1). Unknown names are ignored loudly. Arms one G-06
    /// history step.
    pub(crate) fn set_perspective_value(&mut self, field: &str, value: f64) {
        let mut persp = self.recipe.perspective.unwrap_or(Perspective {
            version: 1,
            vertical: 0.0,
            horizontal: 0.0,
            rotation: 0.0,
            scale: 1.0,
            aspect_ratio: 1.0,
            shift_x: 0.0,
            shift_y: 0.0,
        });
        match field {
            "vertical" => persp.vertical = value as f32,
            "horizontal" => persp.horizontal = value as f32,
            "rotation" => persp.rotation = value as f32,
            "scale" => persp.scale = value as f32,
            "aspect_ratio" => persp.aspect_ratio = value as f32,
            "shift_x" => persp.shift_x = value as f32,
            "shift_y" => persp.shift_y = value as f32,
            _ => {
                warn!("set_perspective_value: unknown field {field}");
                return;
            }
        }
        self.recipe.perspective = Some(persp);
        self.mark_recipe_dirty(&format!("perspective.{field}"), value);
        self.pending_history_step = Some(format!("perspective.{field}"));
        info!("GUI interaction: set_perspective_value {field} -> {value}");
    }

    /// LRPAR-G06-UPRIGHT-15: run the deterministic `upright-lines-v1` analysis
    /// on the currently loaded source frame and persist it, bound to the source
    /// identity fingerprint. Enables the stage (the analysis is meant to be
    /// applied); the manual perspective stays persisted and returns on disable.
    pub fn analyze_upright_now(&mut self) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::AnalyzeUpright);
        let Some(frame) = self.original.clone() else {
            return Ok(());
        };
        let source_hash = self.resolved_source_hash();
        let fingerprint = upright_input_fingerprint(
            &source_hash,
            frame.width(),
            frame.height(),
            self.raw_orientation,
        );
        let suggestion = analyze_upright(&frame);
        let current = self
            .recipe
            .upright
            .as_ref()
            .map(|stage| stage.enabled)
            .unwrap_or(true);
        self.recipe.upright = Some(Upright {
            version: 1,
            enabled: current,
            analysis: Some(upright_analysis(suggestion, fingerprint)),
        });
        self.mark_recipe_dirty("upright.analyze", f64::from(suggestion.confidence));
        self.pending_history_step = Some("upright.analyze".into());
        info!(
            "GUI interaction: analyze_upright_now -> lines={} confidence={:.3} \
             vertical={:.3} horizontal={:.3} rotation={:.3}",
            suggestion.line_count,
            suggestion.confidence,
            suggestion.vertical,
            suggestion.horizontal,
            suggestion.rotation
        );
        Ok(())
    }

    /// LRPAR-G06-UPRIGHT-15: apply or stop applying the persisted analysis.
    /// Enabling without an analysis is refused loudly (no silent identity
    /// render); the manual perspective is authoritative while disabled.
    pub fn set_upright_enabled(&mut self, enabled: bool) -> Result<(), GuiError> {
        instrument_gui_action!(self, GuiAction::SetUprightEnabled);
        let stage = self.recipe.upright.clone().unwrap_or(Upright {
            version: 1,
            enabled: false,
            analysis: None,
        });
        if enabled && stage.analysis.is_none() {
            return Err(GuiError::Io(
                "no persisted upright analysis; run Analyze first".into(),
            ));
        }
        let mut stage = stage;
        stage.enabled = enabled;
        self.recipe.upright = Some(stage);
        self.mark_recipe_dirty("upright.enabled", f64::from(u8::from(enabled)));
        self.pending_history_step = Some("upright.enabled".into());
        info!("GUI interaction: set_upright_enabled -> {enabled}");
        Ok(())
    }

    /// LRPAR-G06-UPRIGHT-15: remove the whole upright stage (the manual
    /// perspective, if any, becomes authoritative again).
    pub fn clear_upright(&mut self) {
        instrument_gui_action!(self, GuiAction::ClearUpright);
        self.recipe.upright = None;
        self.mark_recipe_dirty("upright.clear", 0.0);
        self.pending_history_step = Some("upright.clear".into());
        info!("GUI interaction: clear_upright");
    }

    /// LRPAR-G06-UPRIGHT-15: `fresh`/`stale`/`none` status of the persisted
    /// analysis for the currently loaded source (visible, never a silent
    /// recompute). Pure read; the source hash is memoized.
    pub fn upright_status(&mut self) -> &'static str {
        let Some(persisted) = self
            .recipe
            .upright
            .as_ref()
            .and_then(|stage| stage.analysis.as_ref())
            .map(|analysis| analysis.fingerprint.input_fingerprint.clone())
        else {
            return "none";
        };
        let Some((width, height)) = self
            .original
            .as_ref()
            .map(|frame| (frame.width(), frame.height()))
        else {
            return "none";
        };
        let source_hash = self.resolved_source_hash();
        let current = upright_input_fingerprint(&source_hash, width, height, self.raw_orientation);
        if persisted == current {
            "fresh"
        } else {
            "stale"
        }
    }
}
