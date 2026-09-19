//! GUI-REFACTOR-W2-20 S2.2: the Optics Develop section, extracted verbatim
//! from `lib.rs`.
//!
//! [`LuminaApp::draw_optics`] paints lens-profile/lens-blur/perspective controls;
//! [`LuminaApp::lens_profile_status`] formats the lens-correction status pair.
//! `lens_profile_status` is `pub(crate)` because the headless lensprofile tests
//! call it; `draw_optics` because `DEVELOP_SECTIONS` references it.

use super::*;

impl LuminaApp {
    /// Visible lens-profile status (GUI-OPTICS-1): the profile name when the
    /// recipe carries one, otherwise the explicit inactive notice — a missing
    /// profile is an inactive automatic correction, never a silent one.
    /// Returns `(text, has_profile)`. Pure helper so the status wording is
    /// unit-testable headless.
    pub(crate) fn lens_profile_status(lens: &Option<LensCorrection>) -> (String, bool) {
        match lens.as_ref().and_then(|lens| lens.profile.as_deref()) {
            Some(name) if !name.is_empty() => (Str::OpticsProfilePattern.format_arg(name), true),
            _ => (Str::OpticsProfileNone.t().to_string(), false),
        }
    }

    pub(crate) fn draw_optics(&mut self, ui: &mut egui::Ui) {
        // G-11 solo: see `draw_basic`.
        let section_was_open = self.section_open[SECTION_OPTICS];
        let section_header =
            egui::CollapsingHeader::new(Str::Optics.t()).open(Some(section_was_open));
        let section_response = section_header.show(ui, |ui| {
            self.draw_section_prev_reset(ui, SECTION_OPTICS);
            // G-06 (LRPAR-G06-GEO): the manual lens model is pure
            // core-pipeline state — always settable, never behind a
            // Lensfun feature gate (like geometry: F-098 manual is
            // core-only). The Lensfun *auto* correction keeps its own
            // status line below; without the feature it names the
            // missing capability instead of hiding the sliders.
            ui.label(Str::LensCorrection.t());
            // GUI-OPTICS-1: the profile status is always visible (name or
            // "no profile — correction inactive"), never implied.
            let (status, _) = Self::lens_profile_status(&self.recipe.lens_correction);
            ui.label(status);
            // G-06: manual profile picker (Core whitelist; anything else
            // is rejected loudly by `set_lens_profile`, never guessed).
            // Owned snapshot (no recipe borrow across the setter calls).
            let current_profile = self
                .recipe
                .lens_correction
                .as_ref()
                .and_then(|lens| lens.profile.clone())
                .unwrap_or_else(|| "none".into());
            egui::ComboBox::from_label(Str::LensProfile.t())
                .selected_text(&current_profile)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(current_profile == "none", "none")
                        .clicked()
                        && current_profile != "none"
                    {
                        self.clear_lens_profile();
                    }
                    for name in ["wide-light", "tele-light", "standard-neutral"] {
                        if ui.selectable_label(current_profile == name, name).clicked()
                            && current_profile != name
                        {
                            if let Err(error) = self.set_lens_profile(name) {
                                self.show_error(error);
                            }
                        }
                    }
                });
            ui.label(Str::LensfunAutoPattern.format_arg(&self.lensfun_auto_status_text()));
            {
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
                // GUI-OPTICS-1: every manual parameter is always settable.
                // The previous build rendered sliders only for `Some` values
                // and a bare "(unset)" label otherwise, so a fresh recipe
                // could never receive a correction from this panel at all
                // (the reported "no effect"). Each slider binds
                // `current.unwrap_or(0.0)` — the identity for all eight
                // params — and nothing is written until the user moves or
                // resets it, so `None` stays `None`.
                // GUI-SLIDER-SAVE-1: optics sliders commit through
                // `set_lens_correction_value` (save at debounce); `lc` is only
                // a slider binding buffer.
                ui.label(Str::OpticsDistortionGroup.t())
                    .on_hover_text(Str::OpticsDistortionHint.t());
                for (field, name, label, spec) in [
                    (
                        &mut lc.distortion_k1,
                        "distortion_k1",
                        Str::DistortionK1,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut lc.distortion_k2,
                        "distortion_k2",
                        Str::DistortionK2,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut lc.distortion_k3,
                        "distortion_k3",
                        Str::DistortionK3,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                ] {
                    let mut v = field.as_ref().copied().unwrap_or(0.0);
                    if matches!(
                        lr_slider(ui, label.t(), &mut v, spec),
                        SliderAction::Changed | SliderAction::ResetRequested
                    ) {
                        self.set_lens_correction_value(name, f64::from(v));
                    }
                }
                ui.label(Str::OpticsVignetteGroup.t())
                    .on_hover_text(Str::OpticsVignetteHint.t());
                for (field, name, label, spec) in [
                    (
                        &mut lc.vignette_c0,
                        "vignette_c0",
                        Str::VignetteC0,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut lc.vignette_c1,
                        "vignette_c1",
                        Str::VignetteC1,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                    (
                        &mut lc.vignette_c2,
                        "vignette_c2",
                        Str::VignetteC2,
                        percent_spec(-1.0..=1.0, 0.0),
                    ),
                ] {
                    let mut v = field.as_ref().copied().unwrap_or(0.0);
                    if matches!(
                        lr_slider(ui, label.t(), &mut v, spec),
                        SliderAction::Changed | SliderAction::ResetRequested
                    ) {
                        self.set_lens_correction_value(name, f64::from(v));
                    }
                }
                ui.label(Str::OpticsCaGroup.t())
                    .on_hover_text(Str::OpticsCaHint.t());
                for (field, name, label, spec) in [
                    (
                        &mut lc.ca_red,
                        "ca_red",
                        Str::ChromaticRed,
                        identity_spec(-0.05..=0.05, 0.0, 0.001),
                    ),
                    (
                        &mut lc.ca_blue,
                        "ca_blue",
                        Str::ChromaticBlue,
                        identity_spec(-0.05..=0.05, 0.0, 0.001),
                    ),
                ] {
                    let mut v = field.as_ref().copied().unwrap_or(0.0);
                    if matches!(
                        lr_slider(ui, label.t(), &mut v, spec),
                        SliderAction::Changed | SliderAction::ResetRequested
                    ) {
                        self.set_lens_correction_value(name, f64::from(v));
                    }
                }
            }
            // G-05 Lens Blur: optics-adjacent subgroup (own collapsible group
            // inside Optics so the 8-section F-100 order stays intact). All
            // edits commit through the `set_lens_blur_*` setters (save at
            // debounce); locals are only control binding buffers.
            ui.collapsing(Str::LensBlur.t(), |ui| {
                ui.label(Str::LensBlurHint.t());
                let mut enabled = self.recipe.lens_blur.as_ref().is_some_and(|b| b.enabled);
                if ui.checkbox(&mut enabled, Str::LensBlurEnable.t()).changed() {
                    self.set_lens_blur_enabled(enabled);
                }
                let current = self.recipe.lens_blur.clone().unwrap_or(LensBlur {
                    version: 1,
                    enabled: false,
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
                });
                let mut amount = current.blur_amount;
                if matches!(
                    lr_slider(
                        ui,
                        Str::LensBlurAmount.t(),
                        &mut amount,
                        percent_spec(0.0..=1.0, 0.5)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    if let Err(error) = self.set_lens_blur_amount(f64::from(amount)) {
                        self.status = error.to_string();
                    }
                }
                let mut near = current.focal_near;
                if matches!(
                    lr_slider(
                        ui,
                        Str::LensBlurFocalNear.t(),
                        &mut near,
                        percent_spec(0.0..=1.0, 0.0)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    if let Err(error) =
                        self.set_lens_blur_focal(f64::from(near), f64::from(current.focal_far))
                    {
                        self.status = error.to_string();
                    }
                }
                let mut far = current.focal_far;
                if matches!(
                    lr_slider(
                        ui,
                        Str::LensBlurFocalFar.t(),
                        &mut far,
                        percent_spec(0.0..=1.0, 0.2)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    if let Err(error) =
                        self.set_lens_blur_focal(f64::from(current.focal_near), f64::from(far))
                    {
                        self.status = error.to_string();
                    }
                }
                ui.label(Str::LensBlurBokeh.t());
                for (shape, label) in [
                    (BokehShape::Round, Str::LensBlurBokehRound),
                    (BokehShape::Elliptical, Str::LensBlurBokehElliptical),
                    (BokehShape::Hexagonal, Str::LensBlurBokehHexagonal),
                ] {
                    if ui.radio(current.bokeh == shape, label.t()).clicked() {
                        self.set_lens_blur_bokeh(shape);
                    }
                }
                // Focus rectangle: four normalized sliders (x, y, w, h).
                ui.label(Str::LensBlurFocusRect.t());
                let mut rect = [
                    current.focus_rect.x,
                    current.focus_rect.y,
                    current.focus_rect.width,
                    current.focus_rect.height,
                ];
                let mut rect_changed = false;
                for (i, default) in [0.25, 0.25, 0.5, 0.5].into_iter().enumerate() {
                    let mut v = rect[i];
                    if matches!(
                        lr_slider(
                            ui,
                            ["x", "y", "w", "h"][i],
                            &mut v,
                            percent_spec(0.0..=1.0, default)
                        ),
                        SliderAction::Changed | SliderAction::ResetRequested
                    ) {
                        rect[i] = v;
                        rect_changed = true;
                    }
                }
                if rect_changed {
                    if let Err(error) = self.set_lens_blur_focus_rect(
                        f64::from(rect[0]),
                        f64::from(rect[1]),
                        f64::from(rect[2]),
                        f64::from(rect[3]),
                    ) {
                        self.status = error.to_string();
                    }
                }
                // Depth status is always visible (never implied): off,
                // heuristic active, or missing depth artifact (loud).
                let status = self.lens_blur_status_text();
                ui.label(Str::LensBlurStatusPattern.format_arg(&status));
            });
        });
        if section_response.header_response.clicked() {
            self.set_section_open(SECTION_OPTICS, !section_was_open);
        }
    }
}
