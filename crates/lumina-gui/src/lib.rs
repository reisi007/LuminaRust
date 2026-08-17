//! Shared eframe application for the native and browser MVP.

use eframe::egui;
use lumina_core::ImageFrame;
use lumina_sidecar::EditRecipe;
#[cfg(not(target_arch = "wasm32"))]
use lumina_sidecar::SidecarDocument;
#[cfg(not(target_arch = "wasm32"))]
use lumina_sidecar::{DecodeFingerprint, GeometryFingerprint, SourceIdentity};
#[cfg(not(target_arch = "wasm32"))]
use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum GuiError {
    #[error("{0}")]
    Core(#[from] lumina_core::CoreError),
    #[error("{0}")]
    Sidecar(#[from] lumina_sidecar::SidecarError),
    #[error("{0}")]
    Io(String),
}

pub struct LuminaApp {
    original: Option<ImageFrame>,
    preview: Option<ImageFrame>,
    source_bytes: Option<Vec<u8>>,
    source_name: String,
    #[cfg(not(target_arch = "wasm32"))]
    path: String,
    recipe: EditRecipe,
    texture: Option<egui::TextureHandle>,
    status: String,
    error: Option<String>,
}

impl LuminaApp {
    pub fn new(_ctx: egui::Context) -> Self {
        Self {
            original: None,
            preview: None,
            source_bytes: None,
            source_name: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            path: String::new(),
            recipe: EditRecipe::default(),
            texture: None,
            status: "Bereit für ein PNG, JPEG oder WebP".into(),
            error: None,
        }
    }

    pub fn recipe(&self) -> &EditRecipe {
        &self.recipe
    }
    pub fn status(&self) -> &str {
        &self.status
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn preview(&self) -> Option<&ImageFrame> {
        self.preview.as_ref()
    }

    pub fn load_bytes(&mut self, bytes: Vec<u8>, name: impl Into<String>) -> Result<(), GuiError> {
        let frame = ImageFrame::decode(&bytes)?;
        self.source_name = name.into();
        self.source_bytes = Some(bytes);
        self.original = Some(frame);
        self.recipe = EditRecipe::default();
        self.error = None;
        self.status = format!("Geladen: {}", self.source_name);
        self.render()
    }

    pub fn set_adjustment(&mut self, name: &str, value: f64) {
        self.recipe.adjustments.insert(name.into(), value);
        self.error = None;
    }

    pub fn reset(&mut self) {
        self.recipe = EditRecipe::default();
        if self.original.is_some() {
            let _ = self.render();
        }
    }

    pub fn render(&mut self) -> Result<(), GuiError> {
        let Some(original) = &self.original else {
            self.status = "Kein Bild geladen".into();
            return Ok(());
        };
        let mut preview = original.clone();
        preview.apply_recipe(&self.recipe)?;
        self.preview = Some(preview);
        self.error = None;
        self.status = "Vorschau aktuell".into();
        Ok(())
    }

    fn show_error(&mut self, error: impl ToString) {
        let message = error.to_string();
        self.status = "Fehler".into();
        self.error = Some(message);
    }

    fn update_texture(&mut self, ctx: &egui::Context) {
        if let Some(frame) = &self.preview {
            let size = [frame.width as usize, frame.height as usize];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, &frame.pixels);
            self.texture =
                Some(ctx.load_texture("lumina-preview", image, egui::TextureOptions::LINEAR));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn source_identity(&self, frame: &ImageFrame) -> SourceIdentity {
        SourceIdentity {
            relative_name: if self.source_name.is_empty() {
                "dropped-image".into()
            } else {
                self.source_name.clone()
            },
            content_hash: self
                .source_bytes
                .as_ref()
                .map(|bytes| format!("blake3:{}", blake3::hash(bytes).to_hex()))
                .unwrap_or_else(|| "blake3:unknown".into()),
            byte_length: self
                .source_bytes
                .as_ref()
                .map_or(0, |bytes| bytes.len() as u64),
            modified_at: None,
            raw_format: Path::new(&self.source_name)
                .extension()
                .and_then(|v| v.to_str())
                .unwrap_or("raster")
                .to_ascii_uppercase(),
            orientation: 1,
            decode_fingerprint: DecodeFingerprint {
                decoder: "image".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                parameters: BTreeMap::new(),
                extras: BTreeMap::new(),
            },
            geometry_fingerprint: GeometryFingerprint {
                width: frame.width,
                height: frame.height,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: BTreeMap::new(),
            },
            extras: BTreeMap::new(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_path(&mut self) {
        let path = std::path::PathBuf::from(self.path.trim());
        match std::fs::read(&path) {
            Ok(bytes) => {
                if let Err(error) = self.load_bytes(
                    bytes,
                    path.file_name().and_then(|v| v.to_str()).unwrap_or("image"),
                ) {
                    self.show_error(error);
                }
            }
            Err(error) => self.show_error(GuiError::Io(format!("{}: {}", path.display(), error))),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_sidecar(&mut self) {
        if self.path.trim().is_empty() {
            self.show_error("Zum Speichern muss das Bild über einen lokalen Pfad geladen werden");
            return;
        }
        let path = std::path::PathBuf::from(self.path.trim());
        let Some(frame) = &self.original else {
            self.show_error("Kein Bild geladen");
            return;
        };
        let document = SidecarDocument::new(self.source_identity(frame), "raster-mvp-1");
        let mut document = document;
        document.virtual_copies[0].recipe = self.recipe.clone();
        if let Err(error) =
            lumina_sidecar::save_sidecar(&lumina_sidecar::sidecar_path_for(&path), &document)
        {
            self.show_error(error);
        } else {
            self.status = "Sidecar gespeichert".into();
        }
    }

    fn draw_preview(&mut self, ui: &mut egui::Ui) {
        if let Some(texture) = &self.texture {
            let available = ui.available_size();
            let image = egui::Image::from_texture(texture).fit_to_fraction(available);
            ui.add(image);
        } else {
            ui.centered_and_justified(|ui| {
                ui.label("Bild hierher ziehen oder einen Pfad laden");
            });
        }
    }
}

impl eframe::App for LuminaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        for file in ctx.input(|input| input.raw.dropped_files.clone()) {
            if let Some(bytes) = file.bytes {
                if let Err(error) = self.load_bytes(bytes.to_vec(), file.name) {
                    self.show_error(error);
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(path) = file.path {
                self.path = path.display().to_string();
                self.load_path();
            }
        }
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Lumina");
                ui.separator();
                ui.label(&self.status);
            });
            if let Some(error) = &self.error {
                ui.colored_label(egui::Color32::RED, error);
            }
        });
        egui::SidePanel::left("controls")
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("Entwicklung");
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.path);
                        if ui.button("Load").clicked() {
                            self.load_path();
                        }
                    });
                    if ui.button("Datei auswählen").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_file() {
                            self.path = path.display().to_string();
                            self.load_path();
                        }
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    ui.label("Web: Bild per Drag-and-drop laden");
                }
                let mut exposure = self
                    .recipe
                    .adjustments
                    .get("exposure")
                    .copied()
                    .unwrap_or(0.0);
                let mut contrast = self
                    .recipe
                    .adjustments
                    .get("contrast")
                    .copied()
                    .unwrap_or(0.0);
                if ui
                    .add(egui::Slider::new(&mut exposure, -10.0..=10.0).text("Exposure"))
                    .changed()
                {
                    self.set_adjustment("exposure", exposure);
                }
                if ui
                    .add(egui::Slider::new(&mut contrast, -1.0..=1.0).text("Contrast"))
                    .changed()
                {
                    self.set_adjustment("contrast", contrast);
                }
                ui.horizontal(|ui| {
                    if ui.button("Reset").clicked() {
                        self.reset();
                    }
                    if ui.button("Render / Apply").clicked() {
                        if let Err(error) = self.render() {
                            self.show_error(error);
                        }
                    }
                });
                #[cfg(not(target_arch = "wasm32"))]
                if ui.button("Save Recipe / Sidecar").clicked() {
                    self.save_sidecar();
                }
                #[cfg(target_arch = "wasm32")]
                {
                    ui.label("Browser-Dateispeichern ist im MVP noch nicht implementiert.");
                }
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            self.update_texture(ctx);
            self.draw_preview(ui);
        });
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() -> Result<(), wasm_bindgen::JsValue> {
    use wasm_bindgen::JsCast;
    let canvas = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("lumina_canvas"))
        .and_then(|element| element.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        .ok_or_else(|| wasm_bindgen::JsValue::from_str("Lumina canvas was not found"))?;
    wasm_bindgen_futures::spawn_local(async {
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(LuminaApp::new(cc.egui_ctx.clone())))),
            )
            .await;
        if let Err(error) = result {
            web_sys::console::error_1(&error);
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumina_core::ImageFileFormat;
    fn app() -> LuminaApp {
        LuminaApp::new(egui::Context::default())
    }
    fn png() -> Vec<u8> {
        ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap()
    }
    #[test]
    fn recipe_change_and_render() {
        let mut app = app();
        app.load_bytes(png(), "test.png").unwrap();
        app.set_adjustment("exposure", 1.0);
        app.render().unwrap();
        assert_eq!(app.recipe().adjustments["exposure"], 1.0);
        assert_eq!(app.preview().unwrap().pixels[0], 20);
    }
    #[test]
    fn reset_restores_original_preview() {
        let mut app = app();
        app.load_bytes(png(), "test.png").unwrap();
        app.set_adjustment("contrast", 1.0);
        app.render().unwrap();
        app.reset();
        assert!(app.recipe().adjustments.is_empty());
        assert_eq!(app.preview().unwrap().pixels[0], 10);
    }
    #[test]
    fn decode_error_is_visible() {
        let mut app = app();
        let result = app.load_bytes(vec![1, 2, 3], "bad.png");
        assert!(result.is_err());
        app.show_error(result.unwrap_err());
        assert_eq!(app.status(), "Fehler");
        assert!(app.error().is_some());
    }
}
