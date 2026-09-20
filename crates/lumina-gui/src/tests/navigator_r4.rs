//! R4-NAV-1 (F-103-N6 Runde 4) headless tests: the navigator viewport drag on
//! the **real draw path** (not only the pure rect math the coverage wave
//! pinned) and the R4-UX-1 removal of the duplicate thumbnail rail.
//!
//! `tests/navigator.rs` is exactly at the 500-line ratchet, so the new cases
//! live here (new file, `<= 500` lines).

use super::*;
use crate::navigator::{clear_last_navigator_view_rect, last_navigator_view_rect};

/// One persistent `egui::Context` across frames, driving the real
/// `draw_navigator` (and therefore `draw_navigator_viewport`) so the drag
/// response, the pan pin and the painted viewport rectangle are exercised
/// end-to-end — the seam the coverage wave noted was missing (it only drove
/// `navigator_viewport_rect` by hand).
struct NavHarness {
    ctx: egui::Context,
    time: f64,
    screen: egui::Rect,
}

impl NavHarness {
    fn new() -> Self {
        Self {
            ctx: egui::Context::default(),
            time: 0.0,
            screen: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
        }
    }

    fn pass(&mut self, app: &mut LuminaApp, events: Vec<egui::Event>) {
        self.time += 1.0 / 60.0;
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(self.screen),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ui| {
                let ctx = ui.ctx().clone();
                app.draw_navigator(&ctx, ui);
            },
        );
        output.textures_delta.clear();
    }
}

/// A source-backed app with a preview texture but no ROI crop, so the navigator
/// reuses the full-frame texture (the Fit branch) and only the geometry fields
/// under test matter.
fn nav_app(harness: &mut NavHarness) -> LuminaApp {
    let (png, frame) = synthetic_gradient_png();
    let mut app = LuminaApp::new(harness.ctx.clone());
    app.load_bytes(png, "gradient.png").unwrap();
    app.texture = Some(harness.ctx.load_texture(
        "preview",
        egui::ColorImage::filled(
            [frame.width as usize, frame.height as usize],
            egui::Color32::GRAY,
        ),
        egui::TextureOptions::LINEAR,
    ));
    app
}

fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

/// R4-NAV-1: the eligibility gate — a navigator drag may only pan a genuinely
/// magnified, overflowing view, exactly like the preview hand-tool pan. At Fit
/// (zoom 1.0) or a fully visible draw rect there is nothing to pan.
#[test]
fn navigator_drag_pan_gate_requires_magnified_overflow() {
    // Fit / zoomed out: never, however the draw rect is sized.
    assert!(!LuminaApp::navigator_drag_pans_preview(
        1.0, 600.0, 400.0, 2.0, 800.0, 600.0
    ));
    assert!(!LuminaApp::navigator_drag_pans_preview(
        0.5, 600.0, 400.0, 2.0, 800.0, 600.0
    ));
    // Magnified but the draw fits the pane: nothing to move.
    assert!(!LuminaApp::navigator_drag_pans_preview(
        2.0, 600.0, 400.0, 1.0, 800.0, 600.0
    ));
    // Magnified and overflowing (fit-by-width): panning is meaningful.
    assert!(LuminaApp::navigator_drag_pans_preview(
        2.0, 600.0, 400.0, 2.0, 800.0, 600.0
    ));
    // Portrait fit-by-height: the vertical overflow alone arms the pan.
    assert!(LuminaApp::navigator_drag_pans_preview(
        2.0, 400.0, 600.0, 2.0, 800.0, 600.0
    ));
}

/// R4-NAV-1 (real draw path): at a magnified view the navigator drag moves the
/// visible window — the painted viewport rectangle follows the cursor in
/// navigator points and the paired render ROI follows in source pixels.
#[test]
fn navigator_viewport_drag_moves_box_and_roi_at_zoom() {
    let mut harness = NavHarness::new();
    let mut app = nav_app(&mut harness);
    // Zoomed geometry: the 64×40 source at fit 12.5, zoom 2 → scale 25
    // (64·25 = 1600 px wide, far wider than the 800 px pane) → a small, movable
    // viewport rectangle.
    app.zoom_mode = ZoomMode::Custom;
    app.preview_zoom = 2.0;
    app.preview_pan = egui::Vec2::ZERO;
    app.preview_effective_scale = 25.0;
    app.preview_pane_w = 800.0;
    app.preview_pane_h = 600.0;
    clear_last_navigator_view_rect();

    harness.pass(&mut app, vec![]);
    let before = last_navigator_view_rect().expect("navigator must paint its viewport");
    assert!(
        before.width() < harness.screen.width(),
        "a zoomed view must show a sub-frame viewport, got {before:?}"
    );

    let start = before.center();
    let drag = egui::vec2(24.0, -12.0);
    let end = start + drag;
    harness.pass(&mut app, vec![egui::Event::PointerMoved(start)]);
    harness.pass(
        &mut app,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
    );
    harness.pass(&mut app, vec![egui::Event::PointerMoved(end)]);
    harness.pass(&mut app, vec![pointer(end, false)]);
    // One settle frame paints the post-drag geometry.
    harness.pass(&mut app, vec![]);

    assert_ne!(
        app.preview_pan,
        egui::Vec2::ZERO,
        "a magnified navigator drag must move the pan"
    );
    assert_eq!(app.zoom_mode, ZoomMode::Custom);
    let after = last_navigator_view_rect().expect("navigator repaints after the drag");
    assert!(
        (after.center().x - before.center().x - drag.x).abs() < 1.0
            && (after.center().y - before.center().y - drag.y).abs() < 1.0,
        "the viewport rectangle must follow the drag: {before:?} -> {after:?} (drag {drag:?})"
    );

    // The render ROI follows the same window (same source-pixel centre shift).
    let roi_before = LuminaApp::roi_from_zoom(64, 40, 2.0, egui::Vec2::ZERO, 800.0, 600.0).unwrap();
    let roi_after = LuminaApp::roi_from_zoom(64, 40, 2.0, app.preview_pan, 800.0, 600.0).unwrap();
    let center = |roi: [u32; 4]| {
        (
            roi[0] as f32 + roi[2] as f32 / 2.0,
            roi[1] as f32 + roi[3] as f32 / 2.0,
        )
    };
    assert_ne!(
        center(roi_before),
        center(roi_after),
        "the render ROI must move with the navigator window"
    );
}

/// R4-NAV-1 (regression, real draw path): at Fit the viewport rectangle IS the
/// whole frame, so a drag must not silently move `preview_pan` nor pin `Custom`
/// — that was the reported "box does not follow the drag" state change. The
/// box stays the full navigator rect.
#[test]
fn navigator_viewport_drag_is_inert_at_fit() {
    let mut harness = NavHarness::new();
    let mut app = nav_app(&mut harness);
    app.zoom_mode = ZoomMode::Fit;
    app.preview_zoom = 1.0;
    app.preview_pan = egui::Vec2::ZERO;
    app.preview_effective_scale = 1.0;
    app.preview_pane_w = 800.0;
    app.preview_pane_h = 600.0;
    clear_last_navigator_view_rect();

    harness.pass(&mut app, vec![]);
    let before = last_navigator_view_rect().expect("navigator must paint");
    // At Fit the whole frame is visible: the rect spans the navigator width.
    assert!(
        (before.width() - harness.screen.width()).abs() < 1.0,
        "Fit must show the full frame, got {before:?}"
    );

    let start = before.center();
    harness.pass(&mut app, vec![egui::Event::PointerMoved(start)]);
    harness.pass(
        &mut app,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
    );
    harness.pass(
        &mut app,
        vec![egui::Event::PointerMoved(start + egui::vec2(30.0, 20.0))],
    );
    harness.pass(
        &mut app,
        vec![pointer(start + egui::vec2(30.0, 20.0), false)],
    );
    harness.pass(&mut app, vec![]);

    assert_eq!(
        app.preview_pan,
        egui::Vec2::ZERO,
        "a Fit drag must not move the pan (nothing to pan)"
    );
    assert_eq!(
        app.zoom_mode,
        ZoomMode::Fit,
        "a Fit drag must not pin Custom"
    );
    let after = last_navigator_view_rect().expect("navigator repaints");
    assert_eq!(before, after, "the Fit viewport rectangle is static");
}

/// R5-FIX-WELLE-20 (R4-NAV-1 again): the navigator box drag must move the real
/// view, not just the pure helper. This drives the **full frame path** — the
/// left-rail navigator *and* `draw_preview` in the same persistent
/// `egui::Context` — so the pan clamp that the R4 test never exercised runs.
///
/// Root causes pinned here (both made the box "not follow" while the R4 gate
/// was green):
/// 1. `draw_preview` clamped the pan against the on-screen size of the painted
///    ROI-crop texture (only `PREVIEW_ROI_MARGIN`× the pane), capping every pan
///    at ~15 % of a pane. The drag below maps to ≈ −257 px; the old clamp would
///    stop at ≈ −89 px, so the `> 150` bound is the mutation catch.
/// 2. The crop already encodes the pan, yet `draw_preview` offset the texture by
///    the live pan again, so the drawn view moved 2× the cursor. The
///    view-centre check fails under that double count.
struct FullNavHarness {
    ctx: egui::Context,
    time: f64,
    screen: egui::Rect,
}

impl FullNavHarness {
    fn new() -> Self {
        Self {
            ctx: egui::Context::default(),
            time: 0.0,
            screen: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
        }
    }

    fn frame(&mut self, app: &mut LuminaApp, events: Vec<egui::Event>) {
        self.time += 1.0 / 60.0;
        let ctx = self.ctx.clone();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(self.screen),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ui| {
                egui::Panel::left("nav_full")
                    .resizable(false)
                    .default_size(200.0)
                    .show(ui, |ui| {
                        let ctx = ui.ctx().clone();
                        app.draw_navigator(&ctx, ui);
                    });
                egui::CentralPanel::default().show(ui, |ui| {
                    let ctx = ui.ctx().clone();
                    app.update_texture(&ctx);
                    app.draw_preview(ui);
                });
            },
        );
        output.textures_delta.clear();
    }
}

#[test]
fn navigator_drag_moves_view_past_the_roi_crop_margin() {
    let mut harness = FullNavHarness::new();
    let mut app = LuminaApp::new(harness.ctx.clone());
    let (png, _frame) = synthetic_gradient_png();
    app.load_bytes(png, "gradient.png").unwrap();
    app.texture = Some(harness.ctx.load_texture(
        "preview",
        egui::ColorImage::filled([64, 40], egui::Color32::GRAY),
        egui::TextureOptions::LINEAR,
    ));
    // Warm-up caches the pane geometry + base fit; then zoom and render the ROI
    // crop that the navigator drag must survive.
    harness.frame(&mut app, vec![]);
    app.zoom_mode = ZoomMode::Custom;
    app.preview_zoom = 8.0;
    app.preview_pan = egui::Vec2::ZERO;
    app.mark_dirty();
    app.render_full([800, 600], None).unwrap();
    harness.frame(&mut app, vec![]);
    assert!(
        app.preview_roi.is_some(),
        "zoomed view must render an ROI crop for this test"
    );

    let before_box = last_navigator_view_rect().expect("navigator paints its viewport");
    let start = before_box.center();
    let drag = egui::vec2(40.0, -20.0);
    let end = start + drag;
    harness.frame(&mut app, vec![egui::Event::PointerMoved(start)]);
    harness.frame(
        &mut app,
        vec![egui::Event::PointerMoved(start), pointer(start, true)],
    );
    harness.frame(&mut app, vec![egui::Event::PointerMoved(end)]);
    harness.frame(&mut app, vec![pointer(end, false)]);
    harness.frame(&mut app, vec![]);

    // 1. The pan survived the downstream clamp: the old ROI-crop clamp capped
    //    it at ~15 % of the pane (~89 px here); the drag maps to ≈ −257 px.
    assert!(
        app.preview_pan.x < -150.0 && app.preview_pan.y > 70.0,
        "navigator drag must pan past the ROI-crop margin, got {:?}",
        app.preview_pan
    );
    assert_eq!(app.zoom_mode, ZoomMode::Custom);

    // 2. The painted box followed the cursor by the drag (navigator points).
    let after_box = last_navigator_view_rect().expect("navigator repaints");
    assert!(
        (after_box.center().x - before_box.center().x - drag.x).abs() < 2.0
            && (after_box.center().y - before_box.center().y - drag.y).abs() < 2.0,
        "the box must follow the drag: {before_box:?} -> {after_box:?} (drag {drag:?})"
    );

    // Simulate the debounced full render the navigator's `mark_dirty` arms: the
    // crop re-centres on the new pan, so the settled view is compared (the
    // mouse-down frames necessarily place a stale crop).
    app.render_full([800, 600], None).unwrap();
    harness.frame(&mut app, vec![]);

    // 3. The visible view and the box agree: the source under the pane centre is
    //    exactly `w/2 - pan/scale` (no 2× placement). The old double count would
    //    report `w/2 - 2·pan/scale` here.
    let full = app.image_dims().unwrap();
    let pane = app.preview_pane_rect().expect("preview pane recorded");
    let rect = app.preview_screen_rect().expect("preview painted");
    let roi = app
        .preview_roi
        .map(|r| LuminaApp::roi_in_full_pixels(r, full.0, full.1, app.preview_render_src));
    let (view_x, view_y) = LuminaApp::to_normalized(pane.center(), rect, roi, full);
    let expected_x = 0.5 - app.preview_pan.x / app.preview_effective_scale / full.0 as f32;
    let expected_y = 0.5 - app.preview_pan.y / app.preview_effective_scale / full.1 as f32;
    assert!(
        (view_x - expected_x).abs() < 0.02 && (view_y - expected_y).abs() < 0.02,
        "view centre ({view_x},{view_y}) must match the box window ({expected_x},{expected_y})"
    );
}

/// R4-UX-1: the navigator no longer paints the duplicate thumbnail rail (its
/// "Click a thumbnail to open it" / "No images in this folder" hint and the
/// per-cell thumbnails). The bottom filmstrip stays the single selection
/// surface. The rail painted one of the two hints whenever it was drawn — with
/// or without entries — so their absence on the navigator surface is the
/// removal proof.
#[test]
fn navigator_has_no_duplicate_thumbnail_rail() {
    let mut harness = NavHarness::new();
    let mut app = nav_app(&mut harness);
    let ctx = harness.ctx.clone();
    let screen = harness.screen;
    harness.time += 1.0 / 60.0;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(harness.time),
            events: vec![],
            ..Default::default()
        },
        |ui| {
            let ctx = ui.ctx().clone();
            app.draw_navigator(&ctx, ui);
        },
    );
    output.textures_delta.clear();
    assert!(
        !text_contains(&output.shapes, Str::FilmstripHint.t())
            && !text_contains(&output.shapes, Str::FilmstripEmpty.t()),
        "the removed rail hint must not be painted by the navigator"
    );
    // The Navigator heading and the viewport rectangle still paint.
    assert!(text_contains(&output.shapes, Str::Navigator.t()));
    assert!(last_navigator_view_rect().is_some());
}
