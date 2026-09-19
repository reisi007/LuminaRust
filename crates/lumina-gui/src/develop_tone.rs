//! GUI-REFACTOR-W2-20 S2.2: the Tone Curve Develop section, extracted
//! verbatim from `lib.rs`.
//!
//! [`LuminaApp::draw_tone_curve`] paints the point/parametric region editors and
//! the per-channel region sliders. The curve math (`tone_curve_regions`,
//! `build_tone_curve_points`, …) stays at the crate root because the headless
//! tests and recipe helpers share it. `pub(crate)` because `DEVELOP_SECTIONS`
//! references it.

use super::*;
use log::info;

impl LuminaApp {
    pub(crate) fn draw_tone_curve(&mut self, ui: &mut egui::Ui) {
        // G-11 solo: see `draw_basic`.
        let section_was_open = self.section_open[SECTION_TONE_CURVE];
        let section_header =
            egui::CollapsingHeader::new(Str::ToneCurve.t()).open(Some(section_was_open));
        let section_response = section_header.show(ui, |ui| {
            self.draw_section_prev_reset(ui, SECTION_TONE_CURVE);
            // G-02: channel selector (Master/Red/Green/Blue). Display-only
            // session state; the parametric sliders and the point editor
            // below bind to the selected channel.
            ui.label(Str::ToneCurveChannel.t());
            let channels = [
                (0usize, Str::ToneCurveChannelMaster),
                (1, Str::ToneCurveChannelRed),
                (2, Str::ToneCurveChannelGreen),
                (3, Str::ToneCurveChannelBlue),
            ];
            let mut selected = self.tone_curve_channel;
            ui.horizontal(|ui| {
                for (index, label) in channels {
                    if ui.selectable_label(selected == index, label.t()).clicked() {
                        selected = index;
                    }
                }
            });
            if selected != self.tone_curve_channel {
                self.tone_curve_channel = selected;
                info!("GUI interaction: tone_curve_channel {selected}");
            }
            let channel = match self.tone_curve_channel {
                1 => "red",
                2 => "green",
                3 => "blue",
                _ => "master",
            };
            ui.label(Str::CurveRegions.t());
            let (mut s, mut d, mut l, mut h) = tone_curve_channel_regions(&self.recipe, channel);
            let spec = percent_spec(-1.0..=1.0, 0.0);
            // GUI-SLIDER-SAVE-1: each region slider commits through
            // `set_tone_curve_channel_region` (save at debounce); the locals
            // are only slider binding buffers.
            for (val, label) in [
                (&mut s, Str::ToneCurveShadows),
                (&mut d, Str::ToneCurveDarks),
                (&mut l, Str::ToneCurveLights),
                (&mut h, Str::ToneCurveHighlights),
            ] {
                if matches!(
                    lr_slider(ui, label.t(), val, spec),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    let region = match label {
                        Str::ToneCurveShadows => "shadows",
                        Str::ToneCurveDarks => "darks",
                        Str::ToneCurveLights => "lights",
                        Str::ToneCurveHighlights => "highlights",
                        _ => continue,
                    };
                    self.set_tone_curve_channel_region(channel, region, *val);
                }
            }
            // G-02: free point editor for the selected channel. Editing a
            // point replaces the parametric 4-point list (Last-Write-Wins je
            // Kanal); invalid edits are refused loudly by the setters.
            ui.separator();
            ui.label(Str::ToneCurvePoints.t());
            let points: Vec<CurvePoint> = match channel {
                "red" => self
                    .recipe
                    .curves
                    .as_ref()
                    .and_then(|c| c.channels.red.clone())
                    .unwrap_or_else(identity_curve_points),
                "green" => self
                    .recipe
                    .curves
                    .as_ref()
                    .and_then(|c| c.channels.green.clone())
                    .unwrap_or_else(identity_curve_points),
                "blue" => self
                    .recipe
                    .curves
                    .as_ref()
                    .and_then(|c| c.channels.blue.clone())
                    .unwrap_or_else(identity_curve_points),
                _ => self
                    .recipe
                    .curves
                    .as_ref()
                    .map(|c| c.master.clone())
                    .unwrap_or_else(identity_curve_points),
            };
            let point_count = points.len();
            for (index, point) in points.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("P{index} ({:.2})", point.input));
                    let mut output = point.output;
                    if matches!(
                        lr_slider(
                            ui,
                            &Str::ToneCurvePointOutput.format_arg(&index.to_string()),
                            &mut output,
                            identity_spec(0.0..=1.0, 0.0, 0.01)
                        ),
                        SliderAction::Changed | SliderAction::ResetRequested
                    ) {
                        self.set_curve_point(channel, index, "output", f64::from(output));
                    }
                    if index > 0
                        && index + 1 < point_count
                        && ui.button(Str::ToneCurveRemovePoint.t()).clicked()
                    {
                        self.remove_curve_point(channel, index);
                    }
                });
            }
            // New-point editor: one control per row. Two side-by-side
            // `lr_slider`s forced the resizable right panel to ~540px min
            // width (squeezing the preview to ~100px at 1024x720) — stack
            // them instead. Same setters, same behavior, layout only.
            let mut input = self.tone_curve_new_input;
            let mut output = self.tone_curve_new_output;
            let changed_input = matches!(
                lr_slider(
                    ui,
                    &Str::ToneCurvePointInput.format_arg(""),
                    &mut input,
                    identity_spec(0.0..=1.0, 0.5, 0.01)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            );
            let changed_output = matches!(
                lr_slider(
                    ui,
                    &Str::ToneCurvePointOutput.format_arg(""),
                    &mut output,
                    identity_spec(0.0..=1.0, 0.5, 0.01)
                ),
                SliderAction::Changed | SliderAction::ResetRequested
            );
            if changed_input {
                self.tone_curve_new_input = input;
            }
            if changed_output {
                self.tone_curve_new_output = output;
            }
            if ui.button(Str::ToneCurveAddPoint.t()).clicked() {
                self.add_curve_point(
                    channel,
                    f64::from(self.tone_curve_new_input),
                    f64::from(self.tone_curve_new_output),
                );
            }
        });
        if section_response.header_response.clicked() {
            self.set_section_open(SECTION_TONE_CURVE, !section_was_open);
        }
    }
}
