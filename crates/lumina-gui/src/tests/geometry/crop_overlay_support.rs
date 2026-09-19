//! UX-LOOK-CROP-18 (UXG-01) headless test support: the shared preview harness
//! and the tempdir-backed app/gesture helpers. Split out so
//! `crop_overlay.rs` stays under the file-size ratchet (new files stay
//! `<= 500` lines).

use super::*;

/// Persistent headless preview canvas with an advancing clock, driving the
/// real `draw_preview` path and `handle_crop_shortcuts` (key events).
pub(super) struct CropHarness {
    pub(super) ctx: egui::Context,
    pub(super) time: f64,
    pub(super) screen: egui::Rect,
}

impl CropHarness {
    pub(super) fn new() -> Self {
        Self {
            ctx: egui::Context::default(),
            time: 0.0,
            screen: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
        }
    }

    pub(super) fn pass(
        &mut self,
        app: &mut LuminaApp,
        events: Vec<egui::Event>,
    ) -> Vec<egui::epaint::ClippedShape> {
        self.time += 1.0 / 60.0;
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(self.screen),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.draw_preview(ui));
            },
        );
        output.textures_delta.clear();
        output.shapes
    }

    /// One pass that first runs the crop key handler (real key events) and then
    /// paints the preview, so the commit/discard wiring is exercised end-to-end.
    pub(super) fn key_pass(
        &mut self,
        app: &mut LuminaApp,
        key: egui::Key,
    ) -> Vec<egui::epaint::ClippedShape> {
        self.time += 1.0 / 60.0;
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(self.screen),
                time: Some(self.time),
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }],
                ..Default::default()
            },
            |ui| {
                app.handle_crop_shortcuts(ui.ctx());
                egui::CentralPanel::default().show(ui, |ui| app.draw_preview(ui));
            },
        );
        output.textures_delta.clear();
        output.shapes
    }
}

pub(super) fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

/// App on a real (tempdir) source with a preview texture and crop mode armed,
/// so gestures, persistence and sidecar reload all run the production paths.
pub(super) fn crop_app(ctx: &egui::Context) -> (tempfile::TempDir, LuminaApp) {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = LuminaApp::new(ctx.clone());
    open_and_decode(&mut app, source.display().to_string());
    app.ensure_document_loaded().unwrap();
    app.texture = Some(ctx.load_texture(
        "preview",
        egui::ColorImage::filled([2, 1], egui::Color32::BLACK),
        egui::TextureOptions::LINEAR,
    ));
    app.toggle_crop_mode();
    (directory, app)
}

/// Drag the overlay corner near `from` to normalized `to` (fractions of the
/// full-frame canvas).
pub(super) fn drag_corner(
    harness: &mut CropHarness,
    app: &mut LuminaApp,
    from_fraction: (f32, f32),
    to_fraction: (f32, f32),
) {
    // Warm up so the widget is laid out and hit-tested, then read the canvas.
    harness.pass(
        app,
        vec![egui::Event::PointerMoved(harness.screen.center())],
    );
    let full = app
        .overlay_full_rect()
        .expect("the preview overlay canvas must be recorded");
    let from = egui::pos2(
        full.min.x + from_fraction.0 * full.width(),
        full.min.y + from_fraction.1 * full.height(),
    );
    let to = egui::pos2(
        full.min.x + to_fraction.0 * full.width(),
        full.min.y + to_fraction.1 * full.height(),
    );
    harness.pass(app, vec![egui::Event::PointerMoved(from)]);
    harness.pass(
        app,
        vec![egui::Event::PointerMoved(from), button(from, true)],
    );
    harness.pass(app, vec![egui::Event::PointerMoved(to)]);
    harness.pass(app, vec![button(to, false)]);
}
