//! GUI-REFACTOR-W2-20 S2.2: the Effects Develop section, extracted verbatim
//! from `lib.rs`.
//!
//! [`LuminaApp::draw_effects`] paints the vignette/grain controls (recipe fields
//! via `set_effects_value`). `pub(crate)` because `DEVELOP_SECTIONS` references
//! it.

use super::*;

impl LuminaApp {
    pub(crate) fn draw_effects(&mut self, ui: &mut egui::Ui) {
        // G-11 solo: see `draw_basic`.
        let section_was_open = self.section_open[SECTION_EFFECTS];
        let section_header =
            egui::CollapsingHeader::new(Str::Effects.t()).open(Some(section_was_open));
        let section_response = section_header.show(ui, |ui| {
            self.draw_section_prev_reset(ui, SECTION_EFFECTS);
            ui.label(Str::Vignette.t());
            let mut effects = self.recipe.effects.clone().unwrap_or(Effects {
                vignette: Some(Vignette {
                    version: 1,
                    amount: 0.0,
                    midpoint: 0.5,
                    roundness: 0.0,
                    feather: 0.0,
                }),
                grain: Some(Grain {
                    version: 1,
                    amount: 0.0,
                    size: 0.0,
                    roughness: 0.0,
                    seed: 0,
                }),
            });
            if let Some(v) = &mut effects.vignette {
                let mut amount = v.amount;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Amount.t(),
                        &mut amount,
                        percent_spec(-1.0..=1.0, 0.0)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    // GUI-SLIDER-SAVE-1: effects sliders commit through
                    // `set_effects_value` (save at debounce).
                    self.set_effects_value("vignette", "amount", f64::from(amount));
                }
                let mut midpoint = v.midpoint;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Midpoint.t(),
                        &mut midpoint,
                        identity_spec(0.0..=1.0, 0.5, 0.01)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    self.set_effects_value("vignette", "midpoint", f64::from(midpoint));
                }
                let mut roundness = v.roundness;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Roundness.t(),
                        &mut roundness,
                        percent_spec(-1.0..=1.0, 0.0)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    self.set_effects_value("vignette", "roundness", f64::from(roundness));
                }
                let mut feather = v.feather;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Feather.t(),
                        &mut feather,
                        identity_spec(0.0..=1.0, 0.0, 0.01)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    self.set_effects_value("vignette", "feather", f64::from(feather));
                }
            }
            ui.label(Str::Grain.t());
            if let Some(g) = &mut effects.grain {
                let mut amount = g.amount;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Amount.t(),
                        &mut amount,
                        identity_spec(0.0..=1.0, 0.0, 0.01)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    self.set_effects_value("grain", "amount", f64::from(amount));
                }
                let mut size = g.size;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Size.t(),
                        &mut size,
                        identity_spec(0.0..=1.0, 0.0, 0.01)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    self.set_effects_value("grain", "size", f64::from(size));
                }
                let mut roughness = g.roughness;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Roughness.t(),
                        &mut roughness,
                        identity_spec(0.0..=1.0, 0.0, 0.01)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    self.set_effects_value("grain", "roughness", f64::from(roughness));
                }
                let mut seed = g.seed as f64;
                if matches!(
                    lr_slider(
                        ui,
                        Str::Seed.t(),
                        &mut seed,
                        identity_spec(0.0..=1_000_000.0, 0.0, 1.0)
                    ),
                    SliderAction::Changed | SliderAction::ResetRequested
                ) {
                    self.set_effects_value("grain", "seed", seed);
                }
            }
            // `effects` is only a slider binding buffer now: every arm above
            // commits through `set_effects_value` (GUI-SLIDER-SAVE-1).
        });
        if section_response.header_response.clicked() {
            self.set_section_open(SECTION_EFFECTS, !section_was_open);
        }
    }
}
