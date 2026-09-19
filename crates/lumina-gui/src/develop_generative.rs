//! GUI-REFACTOR-W2-20 S2.2: the Generative Expand Develop block, extracted
//! verbatim from `lib.rs`.
//!
//! [`LuminaApp::draw_generative_expand`] paints the canvas/auto-fill controls and
//! the explicit generate action. `pub(crate)` because `develop_scroll_content`
//! calls it.

use super::*;

impl LuminaApp {
    pub(crate) fn draw_generative_expand(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Generative Expand", |ui| {
            let mut expand = self
                .recipe
                .generative_edit
                .as_ref()
                .is_some_and(|ge| ge.effective_expand());
            let mut auto_fill = self
                .recipe
                .generative_edit
                .as_ref()
                .and_then(|ge| ge.auto_fill_transparent)
                .unwrap_or(false);
            ui.label(Str::ExpandHint.t());
            if ui
                .checkbox(&mut expand, Str::ExpandBeyondImage.t())
                .changed()
            {
                if let Err(e) = self.set_expand_beyond_image(expand) {
                    self.show_error(e);
                }
            }
            if ui
                .checkbox(&mut auto_fill, Str::AutoFillTransparent.t())
                .changed()
            {
                if let Err(e) = self.set_auto_fill_transparent(auto_fill) {
                    self.show_error(e);
                }
            }
            if expand {
                let ge = self
                    .recipe
                    .generative_edit
                    .clone()
                    .unwrap_or(GenerativeEdit {
                        version: 1,
                        canvas: None,
                        artifact: None,
                        keep_generative_content: None,
                        auto_fill_transparent: None,
                        expand_beyond_image: Some(true),
                        seed: None,
                        prompt: None,
                        extras: Default::default(),
                    });
                if let Some(canvas) = ge.canvas.clone() {
                    ui.label(format!(
                        "{}: {}x{} offset ({},{}) ",
                        Str::ExpandCanvasLabel.t(),
                        canvas.output_width,
                        canvas.output_height,
                        canvas.source_offset_x,
                        canvas.source_offset_y
                    ));
                    let mut w = canvas.output_width as f32;
                    let mut h = canvas.output_height as f32;
                    let mut ox = canvas.source_offset_x as f32;
                    let mut oy = canvas.source_offset_y as f32;
                    let mut changed = false;
                    if ui
                        .add(
                            egui::DragValue::new(&mut w)
                                .speed(1.0)
                                .range(1.0..=8192.0)
                                .prefix("W "),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    if ui
                        .add(
                            egui::DragValue::new(&mut h)
                                .speed(1.0)
                                .range(1.0..=8192.0)
                                .prefix("H "),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    if ui
                        .add(
                            egui::DragValue::new(&mut ox)
                                .speed(1.0)
                                .range(-4096.0..=4096.0)
                                .prefix("X "),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    if ui
                        .add(
                            egui::DragValue::new(&mut oy)
                                .speed(1.0)
                                .range(-4096.0..=4096.0)
                                .prefix("Y "),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    if changed {
                        let new_canvas = lumina_sidecar::GenerativeCanvas {
                            output_width: w as u32,
                            output_height: h as u32,
                            source_offset_x: ox as i32,
                            source_offset_y: oy as i32,
                            extras: Default::default(),
                        };
                        if let Err(e) = self.set_expand_canvas(new_canvas) {
                            self.show_error(e);
                        }
                    }
                    if ui.button("Apply Frame (drag) → Canvas").clicked() {
                        let src_w = self.original.as_ref().map(|f| f.width).unwrap_or(8);
                        let src_h = self.original.as_ref().map(|f| f.height).unwrap_or(8);
                        let new_canvas = lumina_sidecar::GenerativeCanvas {
                            output_width: src_w + 4,
                            output_height: src_h + 4,
                            source_offset_x: 2,
                            source_offset_y: 2,
                            extras: Default::default(),
                        };
                        // GEN-ONNX-1 Welle 2b (F4): a failed canvas apply (e.g.
                        // the active expand has no artifact yet) must surface,
                        // never be discarded.
                        if let Err(e) = self.set_expand_canvas(new_canvas) {
                            self.show_error(e);
                        }
                    }
                } else {
                    ui.label("Canvas not set — use frame drag to define.");
                    if ui.button("Set default 12x12 canvas (8→12)").clicked() {
                        let src_w = self.original.as_ref().map(|f| f.width).unwrap_or(8);
                        let src_h = self.original.as_ref().map(|f| f.height).unwrap_or(8);
                        let new_canvas = lumina_sidecar::GenerativeCanvas {
                            output_width: src_w + 4,
                            output_height: src_h + 4,
                            source_offset_x: 2,
                            source_offset_y: 2,
                            extras: Default::default(),
                        };
                        // GEN-ONNX-1 Welle 2b (F4): loud, never swallowed.
                        if let Err(e) = self.set_expand_canvas(new_canvas) {
                            self.show_error(e);
                        }
                    }
                }
                ui.colored_label(egui::Color32::YELLOW, Str::ExpandDragFrameHint.t());
            } else {
                ui.label(Str::ExpandCropToImage.t());
            }
            // GEN-ONNX-1 Welle 2b: the explicit generation action (visible for
            // either active role). It produces the deterministic fixture canvas,
            // persists it into the sidecar bundle and links it in the recipe; a
            // missing model/artifact shows here as a loud error, never a faked
            // preview.
            if self.generative_stage_active() {
                if ui.button(Str::GenerateCanvas.t()).clicked() {
                    if let Err(e) = self.generate_generative_canvas() {
                        self.show_error(e);
                    }
                }
                ui.label(self.generative_status_text());
            }
        });
    }
}
