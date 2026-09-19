//! GUI-REFACTOR-W2-20 S2.2: the Spot Heal Develop block, extracted verbatim
//! from `lib.rs`.
//!
//! [`LuminaApp::draw_spot_heal`] paints the spot/red-eye tools, candidate lists
//! and generative healing controls. `pub(crate)` because `develop_scroll_content`
//! calls it.

use super::*;

impl LuminaApp {
    pub(crate) fn draw_spot_heal(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Dust Removal (Q)", |ui| {
            let spot_armed = self.spot_tool != SpotTool::None;
            ui.horizontal(|ui| {
                if ui.selectable_label(spot_armed, "Heal (Q)").clicked() { let next = if spot_armed { SpotTool::None } else { SpotTool::Heal }; self.set_spot_tool(next); }
                if ui.selectable_label(self.spot_mode == SpotMode::Heuristic, "Quick").clicked() { self.set_spot_mode(SpotMode::Heuristic); }
                if ui.selectable_label(self.spot_mode == SpotMode::Generative, "Generative").clicked() { self.set_spot_mode(SpotMode::Generative); }
            });
            if self.spot_mode == SpotMode::Generative { ui.colored_label(egui::Color32::YELLOW, "Generative inpaint requires model inpaint-heal-xl (lumina-onnx, BLAKE3 .lumina.zdata kind=spot_heal_generative). Missing → stale."); }
            // G-04: tool-overlay modes (G-11 session state, never recipe).
            ui.horizontal(|ui| {
                ui.label("Tool overlay:");
                let mode = self.overlay_mode;
                if ui.selectable_label(mode == OverlayMode::Always, "Always").clicked() { self.set_overlay_mode(OverlayMode::Always); }
                if ui.selectable_label(mode == OverlayMode::Auto, "Auto").clicked() { self.set_overlay_mode(OverlayMode::Auto); }
                if ui.selectable_label(mode == OverlayMode::Never, "Never").clicked() { self.set_overlay_mode(OverlayMode::Never); }
            });
            let mut radius = self.spot_radius; if ui.add(egui::Slider::new(&mut radius, 1.0..=512.0).text("Radius")).changed() { self.set_spot_radius(radius); }
            let mut feather = self.spot_feather; if ui.add(egui::Slider::new(&mut feather, 0.0..=1.0).text("Feather")).changed() { self.set_spot_feather(feather); }
            let mut opacity = self.spot_opacity; if ui.add(egui::Slider::new(&mut opacity, 0.0..=1.0).text("Opacity")).changed() { self.set_spot_opacity(opacity); }
            // G-04: visualize-spots slider (recipe-persisted, deterministic).
            {
                let current = self.spot_visualize_threshold();
                let mut value = current.unwrap_or(0.5);
                let changed = ui.add(egui::Slider::new(&mut value, 0.0..=1.0).text("Visualize spots")).changed();
                if changed {
                    if let Err(error) = self.set_spot_visualize(Some(value)) { self.show_error(error); }
                }
                ui.horizontal(|ui| {
                    ui.label(if current.is_some() { format!("Visualize: {:.2}", current.unwrap_or(0.0)) } else { "Visualize: off".into() });
                    if ui.button("Visualize off").clicked() {
                        if let Err(error) = self.clear_spot_visualize() { self.show_error(error); }
                    }
                });
            }
            // G-04: detect objects (heuristic stage 1, explicit apply only).
            {
                let mut threshold = self.spot_detect_threshold;
                if ui.add(egui::Slider::new(&mut threshold, 0.0..=1.0).text("Detect threshold")).changed() {
                    if let Err(error) = self.set_spot_detect_threshold(threshold) { self.show_error(error); }
                }
                ui.horizontal(|ui| {
                    if ui.button("Detect objects").clicked() {
                        if let Err(error) = self.detect_spot_candidates().map(|_| ()) { self.show_error(error); }
                    }
                    if ui.button("Apply detected").clicked() {
                        if let Err(error) = self.apply_detected_spot_objects().map(|_| ()) { self.show_error(error); }
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
                changed |= ui.checkbox(&mut setting.reflections, "Reflections").changed();
                changed |= ui.checkbox(&mut setting.people, "People").changed();
                changed |= ui.checkbox(&mut setting.dust, "Dust").changed();
                changed |= ui.checkbox(&mut setting.auto_mode, "Auto (list only, never auto-apply)").changed();
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
                        if let Err(error) = self.regenerate_spot_variant(&target).map(|_| ()) { self.show_error(error); }
                    }
                });
                if !self.spot_gen_status.is_empty() {
                    ui.label(&self.spot_gen_status);
                }
            }
            if ui.button("Clear spots").clicked() { self.clear_spot_heals(); }
            let spots: Vec<serde_json::Value> = self.recipe.extras.get("spot_removals").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
            for spot in &spots { let id = spot.get("id").and_then(|v| v.as_str()).unwrap_or("?"); let status = spot.get("status").and_then(|v| v.as_str()).unwrap_or("valid"); ui.label(format!("spot {id}: {status}")); }
            ui.label(Str::SpotOverlayHint.t());
            ui.label("SpotHeal → Lens → Perspective → Crop (quick heuristic instant, native desktop-only, no zdata; generative local ONNX Box/Pinsel/Prompt/Seed artifact kind=spot_heal_generative)");
        });
    }
}
