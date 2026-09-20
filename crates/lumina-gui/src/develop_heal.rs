//! R5-DUST-23: the Spot-Heal tool options, painted as a toolbar-near strip
//! over the image (the former sidebar "Dust Removal (Q)" section is gone).
//!
//! [`LuminaApp::draw_spot_tool_options`] is called by `draw_preview_area` while
//! the Heal tool is armed (toolbar icon / `Q`). The primary controls (mode and
//! the Size/Feather/Opacity tool settings) stay visible with their label in the
//! narrow center pane at the reference 1024×720 viewport (two compact rows, no
//! horizontal overflow into the histogram panel); the rarely used G-04 extras
//! (visualize, detect, distraction, generative variants, clear) live in the
//! "Remove options" group so the strip stays compact over the image. Every G-04
//! control keeps its instrumented clickable button.

use super::*;

impl LuminaApp {
    pub(crate) fn draw_spot_tool_options(&mut self, ui: &mut egui::Ui) {
        // R5-DUST-23 (B1): the tool settings must stay inside the center panel
        // at 1024×720 (a single wrapped row overflowed and the histogram panel
        // clipped Feather/Opacity). Two compact rows with an explicit label at
        // every slider keep Size/Feather/Opacity visible and labelled; each row
        // stays well below the narrowest center pane.
        let mut size = self.spot_radius;
        let mut feather = self.spot_feather;
        ui.horizontal(|ui| {
            ui.strong("Spot Heal (Q)");
            if ui
                .selectable_label(self.spot_mode == SpotMode::Heuristic, "Quick")
                .clicked()
            {
                self.set_spot_mode(SpotMode::Heuristic);
            }
            if ui
                .selectable_label(self.spot_mode == SpotMode::Generative, "Generative")
                .clicked()
            {
                self.set_spot_mode(SpotMode::Generative);
            }
            ui.label("Size");
            if ui
                .add(
                    egui::Slider::new(&mut size, 1.0..=512.0)
                        .show_value(false)
                        .fixed_decimals(0),
                )
                .on_hover_text("Spot size in source pixels ([ / ])")
                .changed()
            {
                self.set_spot_radius(size);
            }
            ui.label(format!("{:.0} px", self.spot_radius));
        });
        ui.horizontal(|ui| {
            ui.label("Feather");
            if ui
                .add(egui::Slider::new(&mut feather, 0.0..=1.0).show_value(false))
                .changed()
            {
                self.set_spot_feather(feather);
            }
            ui.label(format!("{:.2}", self.spot_feather));
            let mut opacity = self.spot_opacity;
            ui.label("Opacity");
            if ui
                .add(egui::Slider::new(&mut opacity, 0.0..=1.0).show_value(false))
                .changed()
            {
                self.set_spot_opacity(opacity);
            }
            ui.label(format!("{:.2}", self.spot_opacity));
        });
        if self.spot_mode == SpotMode::Generative {
            ui.colored_label(egui::Color32::YELLOW, "Generative inpaint requires model inpaint-heal-xl (lumina-onnx, BLAKE3 .lumina.zdata kind=spot_heal_generative). Missing → stale.");
        }
        ui.collapsing("Remove options", |ui| {
            // G-04: tool-overlay modes (G-11 session state, never recipe).
            ui.horizontal(|ui| {
                ui.label("Tool overlay:");
                let mode = self.overlay_mode;
                if ui
                    .selectable_label(mode == OverlayMode::Always, "Always")
                    .clicked()
                {
                    self.set_overlay_mode(OverlayMode::Always);
                }
                if ui
                    .selectable_label(mode == OverlayMode::Auto, "Auto")
                    .clicked()
                {
                    self.set_overlay_mode(OverlayMode::Auto);
                }
                if ui
                    .selectable_label(mode == OverlayMode::Never, "Never")
                    .clicked()
                {
                    self.set_overlay_mode(OverlayMode::Never);
                }
            });
            // G-04: visualize-spots slider (recipe-persisted, deterministic).
            {
                let current = self.spot_visualize_threshold();
                let mut value = current.unwrap_or(0.5);
                let changed = ui
                    .add(egui::Slider::new(&mut value, 0.0..=1.0).text("Visualize spots"))
                    .changed();
                if changed {
                    if let Err(error) = self.set_spot_visualize(Some(value)) {
                        self.show_error(error);
                    }
                }
                ui.horizontal(|ui| {
                    ui.label(if current.is_some() {
                        format!("Visualize: {:.2}", current.unwrap_or(0.0))
                    } else {
                        "Visualize: off".into()
                    });
                    if ui.button("Visualize off").clicked() {
                        if let Err(error) = self.clear_spot_visualize() {
                            self.show_error(error);
                        }
                    }
                });
            }
            // G-04: detect objects (heuristic stage 1, explicit apply only).
            {
                let mut threshold = self.spot_detect_threshold;
                if ui
                    .add(egui::Slider::new(&mut threshold, 0.0..=1.0).text("Detect threshold"))
                    .changed()
                {
                    if let Err(error) = self.set_spot_detect_threshold(threshold) {
                        self.show_error(error);
                    }
                }
                ui.horizontal(|ui| {
                    if ui.button("Detect objects").clicked() {
                        if let Err(error) = self.detect_spot_candidates().map(|_| ()) {
                            self.show_error(error);
                        }
                    }
                    if ui.button("Apply detected").clicked() {
                        if let Err(error) = self.apply_detected_spot_objects().map(|_| ()) {
                            self.show_error(error);
                        }
                    }
                });
                if !self.spot_detect_status.is_empty() {
                    ui.label(&self.spot_detect_status);
                }
            }
            // G-04: distraction removal switches (recipe-persisted, auto lists only).
            {
                let mut setting = self.spot_distraction();
                let mut changed = false;
                changed |= ui
                    .checkbox(&mut setting.reflections, "Reflections")
                    .changed();
                changed |= ui.checkbox(&mut setting.people, "People").changed();
                changed |= ui.checkbox(&mut setting.dust, "Dust").changed();
                changed |= ui
                    .checkbox(&mut setting.auto_mode, "Auto (list only, never auto-apply)")
                    .changed();
                if changed {
                    self.set_spot_distraction(setting);
                }
                for (kind, text) in self.distraction_status() {
                    ui.label(format!("{kind}: {text}"));
                }
            }
            // G-04: generative variant regeneration (explicit, deterministic).
            {
                ui.text_edit_singleline(&mut self.spot_gen_prompt);
                let mut seed = self.spot_gen_seed;
                let mut variant = self.spot_gen_variant;
                ui.horizontal(|ui| {
                    ui.label("Seed:");
                    ui.add(egui::DragValue::new(&mut seed));
                    ui.label("Variant:");
                    ui.add(egui::DragValue::new(&mut variant));
                });
                if seed != self.spot_gen_seed || variant != self.spot_gen_variant {
                    let prompt = self.spot_gen_prompt.clone();
                    self.set_spot_gen_inputs(prompt, seed, variant);
                }
                ui.horizontal(|ui| {
                    ui.label("Spot id:");
                    ui.text_edit_singleline(&mut self.spot_gen_target);
                    let target = self.spot_gen_target.trim().to_string();
                    if ui.button("Regenerate variant").clicked() && !target.is_empty() {
                        if let Err(error) = self.regenerate_spot_variant(&target).map(|_| ()) {
                            self.show_error(error);
                        }
                    }
                });
                if !self.spot_gen_status.is_empty() {
                    ui.label(&self.spot_gen_status);
                }
            }
            if ui.button("Clear spots").clicked() {
                self.clear_spot_heals();
            }
            let spots: Vec<serde_json::Value> = self
                .recipe
                .extras
                .get("spot_removals")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            for spot in &spots {
                let id = spot.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                let status = spot
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("valid");
                ui.label(format!("spot {id}: {status}"));
            }
            ui.label(Str::SpotOverlayHint.t());
            ui.label("Click the image to heal a spot.");
        });
    }
}
