//! UX-LOOK-CROP-18b: headless helpers to drive the crop bar's controls through
//! the **real pointer path** (F1 rework). Kept in its own file so
//! `crop_session.rs` stays below the file-size ratchet.
//!
//! The slider track is located from the painted frame (the widget id is not
//! reachable from outside `lr_slider`): the bar background is 36px tall, the
//! handle is a circle and the track is the only ~4px-tall rect, so the widest
//! such rect is the track. The Auto button is located via its painted `Auto`
//! label (the shared `headless_click_labels_sized_frame` helper).

use super::*;
use crate::develop_geometry::crop_rotation::crop_bar_rect;
use crop_overlay_support::button;

/// A fixed crop-bar strip inside the headless screen, produced by the
/// production `crop_bar_rect` so the tested layout is the real one.
pub(super) fn test_bar() -> egui::Rect {
    crop_bar_rect(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(900.0, 500.0),
    ))
}

/// The widest ~4px-tall painted rect of the bar is the slider track.
fn track_rect(shapes: &[egui::epaint::ClippedShape]) -> Option<egui::Rect> {
    shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect) if (rect.rect.height() - 4.0).abs() < 1.0 => Some(rect.rect),
            _ => None,
        })
        .max_by(|a, b| a.width().total_cmp(&b.width()))
}

/// Drive a **real pointer drag** on the crop bar's Straighten track (hit-test +
/// `Sense::click_and_drag` path, never the setter) and return the persistent
/// context so the resulting session draft can be read back.
pub(super) fn drag_straighten_slider(app: &mut LuminaApp, target_frac: f32) -> egui::Context {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 720.0));
    let bar = crop_bar_rect(screen);
    let mut time = 0.0_f64;
    let mut run = |app: &mut LuminaApp, events: Vec<egui::Event>| {
        time += 1.0 / 60.0;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| {
                let c = ui.ctx().clone();
                app.draw_crop_bar(ui, bar, &c);
            },
        );
        output.textures_delta.clear();
        output.shapes
    };
    let shapes = run(app, vec![]);
    let track = track_rect(&shapes).expect("the straighten track must paint");
    let y = track.center().y;
    let start = egui::pos2(track.left() + 0.5 * track.width(), y);
    let end = egui::pos2(track.left() + target_frac * track.width(), y);
    run(
        app,
        vec![egui::Event::PointerMoved(start), button(start, true)],
    );
    run(app, vec![egui::Event::PointerMoved(end)]);
    run(
        app,
        vec![egui::Event::PointerMoved(end), button(end, false)],
    );
    ctx
}

/// Draw only the crop bar in the shared click helper's persistent context and
/// return that context, so a real `Auto` button click can be inspected.
pub(super) fn click_auto_button(app: &mut LuminaApp) -> egui::Context {
    let (_shapes, ctx) =
        headless_click_labels_sized_frame(app, 720.0, &[Str::Auto.t()], |app, ui| {
            let c = ui.ctx().clone();
            app.draw_crop_bar(ui, test_bar(), &c);
        });
    ctx
}
