//! GUI-REFACTOR-W2-20 S2.2: the Color / Color Grading / Point Color Develop
//! sections, extracted verbatim from `lib.rs`.
//!
//! [`LuminaApp::draw_color`] paints saturation/HSL plus the Color Grading wheels,
//! [`LuminaApp::color_grading_range_slider`] is its range slider helper and
//! [`LuminaApp::draw_point_color`] the Point Color picker/table. Pure UI writes
//! through `set_adjustment`/`set_hsl_value` unchanged. `draw_color`/
//! `draw_point_color` are `pub(crate)` (`DEVELOP_SECTIONS` and
//! `develop_scroll_content`); the range helper stays private to this module.

use super::*;

impl LuminaApp {
    pub(crate) fn draw_color(&mut self, ui: &mut egui::Ui) {
        // G-11 solo: see `draw_basic`.
        let section_was_open = self.section_open[SECTION_COLOR];
        let section_header =
            egui::CollapsingHeader::new(Str::Color.t()).open(Some(section_was_open));
        let section_response = section_header.show(ui, |ui| {
            self.draw_section_prev_reset(ui, SECTION_COLOR);
            ui.label(Str::HslMixer.t());
            // GUI-SLIDER-SAVE-1: mixer sliders commit through `set_hsl_value`
            // (save at debounce); `hsl` is only a slider binding buffer.
            let mut hsl = self.recipe.hsl.clone().unwrap_or_default();
            let spec = percent_spec(-1.0..=1.0, 0.0);
            let channels = [
                (Str::HslRed, "red"),
                (Str::HslOrange, "orange"),
                (Str::HslYellow, "yellow"),
                (Str::HslGreen, "green"),
                (Str::HslCyan, "cyan"),
                (Str::HslBlue, "blue"),
                (Str::HslViolet, "violet"),
                (Str::HslMagenta, "magenta"),
            ];
            for (label, key) in channels {
                ui.label(label.t());
                let slot = hsl_channel_mut(&mut hsl, key);
                for (field, label) in [
                    (&mut slot.hue, Str::Hue),
                    (&mut slot.saturation, Str::Saturation),
                    (&mut slot.luminance, Str::Luminance),
                ] {
                    if matches!(
                        lr_slider(ui, label.t(), field, spec),
                        SliderAction::Changed | SliderAction::ResetRequested
                    ) {
                        // GUI-SLIDER-SAVE-1: each mixer slider commits through
                        // `set_hsl_value` (save at debounce); the locals are
                        // only slider binding buffers.
                        let field_name = match label {
                            Str::Hue => "hue",
                            Str::Saturation => "saturation",
                            Str::Luminance => "luminance",
                            _ => continue,
                        };
                        self.set_hsl_value(key, field_name, f64::from(*field));
                    }
                }
            }
            ui.separator();
            // G-02 Point Color group (F-090b): targeted entries after the HSL
            // mixer, before Color Grading (F-100 order: curve → HSL →
            // Point Color → grading → presence → vibrance/saturation).
            self.draw_point_color(ui);
            ui.separator();
            ui.label(Str::ColorGrading.t());
            // GUI-SLIDER-SAVE-1: grading sliders commit through the
            // `set_color_grading_*` setters (save at debounce); `cg` is only a
            // slider binding buffer.
            let mut cg = self
                .recipe
                .color_grading
                .clone()
                .unwrap_or_else(ColorGrading::neutral);
            for (range, range_name, label) in [
                (&mut cg.shadows, "shadows", Str::GradingShadows),
                (&mut cg.midtones, "midtones", Str::GradingMidtones),
                (&mut cg.highlights, "highlights", Str::GradingHighlights),
            ] {
                self.color_grading_range_slider(ui, range_name, range, label);
            }
            let mut balance = cg.balance;
            if matches!(
                lr_slider(
                    ui,
                    Str::GradingBalance.t(),
                    &mut balance,
                    percent_spec(-1.0..=1.0, 0.0),
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                self.set_color_grading_balance(f64::from(balance));
            }
            // G-02 Feinschliff: global blending slider (0..=1, default 0.5).
            let mut blending = cg.blending;
            if matches!(
                lr_slider(
                    ui,
                    Str::GradingBlending.t(),
                    &mut blending,
                    identity_spec(0.0..=1.0, 0.5, 0.01)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                self.set_color_grading_blending(f64::from(blending));
            }

            ui.separator();

            ui.separator();
            // F-100 (F-094): Presence (Texture, Clarity, Dehaze) belongs to the
            // Color section and is ordered *before* Vibrance/Saturation (F-092)
            // and before Sharpening / Noise Reduction / Vignette. We render it as
            // its own labeled group here — between Color Grading and Vibrance/
            // Saturation — so the normative F-100 control order is visible.
            // (F-103-N7 allowed either `draw_effects` above Vignette/Grain or an
            // own group; the F-100 ordering requires it here, ahead of
            // Vibrance/Saturation, hence this dedicated group.)
            ui.label(Str::Presence.t());
            // GUI-SLIDER-SAVE-1: presence sliders commit through the shared
            // `set_presence` path (save at debounce); `presence` is only a
            // slider binding buffer.
            let mut presence = self.recipe.presence.unwrap_or(Presence {
                version: 1,
                texture: 0.0,
                clarity: 0.0,
                dehaze: 0.0,
            });
            let spec = percent_spec(-1.0..=1.0, 0.0);
            for (label, field) in [
                (Str::Texture, &mut presence.texture),
                (Str::Clarity, &mut presence.clarity),
                (Str::Dehaze, &mut presence.dehaze),
            ] {
                if matches!(
                    lr_slider(ui, label.t(), field, spec),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    let name = match label {
                        Str::Texture => "texture",
                        Str::Clarity => "clarity",
                        Str::Dehaze => "dehaze",
                        _ => continue,
                    };
                    self.set_presence(name, f64::from(*field));
                }
            }

            ui.separator();
            // F-100 (F-092): Dynamics/Saturation — Vibrance then Saturation, both
            // flat adjustments on the `-1..=1` domain shown as `-100..+100`.
            self.adjustment_slider(
                ui,
                "vibrance",
                Str::Vibrance.t(),
                percent_spec(-1.0..=1.0, 0.0),
            );
            self.adjustment_slider(
                ui,
                "saturation",
                Str::Saturation.t(),
                percent_spec(-1.0..=1.0, 0.0),
            );
        });
        if section_response.header_response.clicked() {
            self.set_section_open(SECTION_COLOR, !section_was_open);
        }
    }

    /// One Lightroom color-grading range (hue + saturation + luminance
    /// sliders, G-02 Feinschliff) bound to the `set_color_grading_value`
    /// commit path (GUI-SLIDER-SAVE-1). `range` is only a slider binding
    /// buffer; the setter re-reads the recipe.
    fn color_grading_range_slider(
        &mut self,
        ui: &mut egui::Ui,
        range_name: &str,
        range: &mut ColorGradingRange,
        label: Str,
    ) {
        let mut hue = range.hue_degrees;
        if matches!(
            lr_slider(
                ui,
                &Str::HuePattern.format_arg(label.t()),
                &mut hue,
                identity_spec(0.0..=360.0, 0.0, 1.0)
            ),
            SliderAction::Changed | SliderAction::ResetRequested
        ) {
            self.set_color_grading_value(range_name, "hue_degrees", f64::from(hue));
        }
        let mut sat = range.saturation;
        if matches!(
            lr_slider(
                ui,
                &Str::SatPattern.format_arg(label.t()),
                &mut sat,
                identity_spec(0.0..=1.0, 0.0, 0.01)
            ),
            SliderAction::Changed | SliderAction::ResetRequested
        ) {
            self.set_color_grading_value(range_name, "saturation", f64::from(sat));
        }
        // G-02 Feinschliff: per-range luminance.
        let mut lum = range.luminance;
        if matches!(
            lr_slider(
                ui,
                &Str::LumPattern.format_arg(label.t()),
                &mut lum,
                percent_spec(-1.0..=1.0, 0.0)
            ),
            SliderAction::Changed | SliderAction::ResetRequested
        ) {
            self.set_color_grading_value(range_name, "luminance", f64::from(lum));
        }
    }

    /// G-02 Point Color group (F-090b) inside the Color section: entry list
    /// with stable ids, per-entry sliders and add/remove buttons. All edits
    /// commit through the `add/remove/set_point_color_*` setters (save at
    /// debounce); the locals are only binding buffers.
    pub(crate) fn draw_point_color(&mut self, ui: &mut egui::Ui) {
        ui.label(Str::PointColor.t());
        let entries: Vec<PointColorEntry> = self
            .recipe
            .point_color
            .as_ref()
            .map(|block| block.entries.clone())
            .unwrap_or_default();
        for entry in &entries {
            ui.horizontal(|ui| {
                ui.label(entry.id.clone());
                if ui.button(Str::PointColorRemove.t()).clicked() {
                    self.remove_point_color(&entry.id.clone());
                }
            });
            let mut center = entry.hue_center;
            if matches!(
                lr_slider(
                    ui,
                    Str::PointColorHueCenter.t(),
                    &mut center,
                    identity_spec(0.0..=360.0, 0.0, 1.0)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                self.set_point_color_value(&entry.id, "hue_center", f64::from(center));
            }
            let mut range = entry.hue_range;
            if matches!(
                lr_slider(
                    ui,
                    Str::PointColorRange.t(),
                    &mut range,
                    identity_spec(0.0..=180.0, 30.0, 1.0)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            ) {
                self.set_point_color_value(&entry.id, "hue_range", f64::from(range));
            }
            let spec = percent_spec(-1.0..=1.0, 0.0);
            for (mut value, field, label) in [
                (entry.hue_shift, "hue_shift", Str::PointColorHueShift.t()),
                (
                    entry.saturation_shift,
                    "saturation_shift",
                    Str::PointColorSatShift.t(),
                ),
                (
                    entry.luminance_shift,
                    "luminance_shift",
                    Str::PointColorLumShift.t(),
                ),
            ] {
                if matches!(
                    lr_slider(ui, label, &mut value, spec),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    let id = entry.id.clone();
                    self.set_point_color_value(&id, field, f64::from(value));
                }
            }
        }
        if entries.len() < 8 && ui.button(Str::PointColorAdd.t()).clicked() {
            self.add_point_color();
        }
    }
}
