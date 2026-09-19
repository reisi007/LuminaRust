//! GUI-REFACTOR-W2-20 S2.2: the Tone Curve Develop section, extracted
//! verbatim from `lib.rs`.
//!
//! [`LuminaApp::draw_tone_curve`] paints the per-channel region sliders and the
//! interactive point-curve graph. The curve math (`tone_curve_regions`,
//! `build_tone_curve_points`, …) stays at the crate root because the headless
//! tests and recipe helpers share it. `pub(crate)` because `DEVELOP_SECTIONS`
//! references it.

// UX-LOOK-TONECURVE-18: the interactive point-curve graph (new logic in its own
// file per the file-size ratchet) while staying in this module boundary.
pub(crate) mod tone_curve_graph;

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
            // UX-LOOK-TONECURVE-18: interactive point-curve graph. A click on
            // the curve adds a control point, a drag moves it, a double-click
            // removes it (see `tone_curve_graph`). Edits reuse the existing
            // `curves` recipe block — no schema change, no migration.
            ui.separator();
            ui.label(Str::ToneCurvePoints.t());
            self.draw_tone_curve_graph(ui, channel);
        });
        if section_response.header_response.clicked() {
            self.set_section_open(SECTION_TONE_CURVE, !section_was_open);
        }
    }
}
