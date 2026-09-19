//! ROI/zoom/pan drag behaviour tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

#[test]
fn roi_from_zoom_follows_pan_and_reaches_borders() {
    use super::{LuminaApp, PREVIEW_ROI_MARGIN};
    // Landscape 4000×3000 source in an 800×600 pane: fit = 0.2.
    let (w, h) = (4000_u32, 3000_u32);
    let (pw, ph) = (800.0_f32, 600.0_f32);

    // Fit/zoom-out renders the whole frame.
    assert_eq!(
        LuminaApp::roi_from_zoom(w, h, 1.0, egui::Vec2::ZERO, pw, ph),
        None
    );
    assert_eq!(
        LuminaApp::roi_from_zoom(w, h, 0.5, egui::Vec2::ZERO, pw, ph),
        None
    );

    // Centered pan at 4×: ROI is centred and covers the visible window
    // (pane / (fit·zoom)) plus panning margin on every side.
    let roi = LuminaApp::roi_from_zoom(w, h, 4.0, egui::Vec2::ZERO, pw, ph).unwrap();
    let scale = 0.2_f64 * 4.0;
    let window_w = (800.0_f64 / scale) * PREVIEW_ROI_MARGIN;
    let window_h = (600.0_f64 / scale) * PREVIEW_ROI_MARGIN;
    assert_eq!(roi[2] as f64, window_w.floor());
    assert_eq!(roi[3] as f64, window_h.floor());
    assert!((roi[0] as f64 - (4000.0 - window_w) / 2.0).abs() <= 1.0);
    assert!((roi[1] as f64 - (3000.0 - window_h) / 2.0).abs() <= 1.0);

    // Dragging the image right/up (negative pan delta) moves the window
    // towards the bottom-right; far enough it clamps against that border
    // so the corner becomes reachable (REVIEW-GUI-PANROI-1).
    let br = LuminaApp::roi_from_zoom(w, h, 4.0, egui::vec2(-1200.0, -1200.0), pw, ph)
        .expect("panned ROI");
    assert_eq!(br[0] + br[2], w, "right border reachable");
    assert_eq!(br[1] + br[3], h, "bottom border reachable");

    // Dragging left/down clamps against the top-left border.
    let tl = LuminaApp::roi_from_zoom(w, h, 4.0, egui::vec2(1200.0, 1200.0), pw, ph).unwrap();
    assert_eq!(tl[0], 0, "left border reachable");
    assert_eq!(tl[1], 0, "top border reachable");

    // Extreme zoom stays inside bounds and never returns an empty rect.
    let roi = LuminaApp::roi_from_zoom(w, h, 32.0, egui::vec2(12345.0, -9999.0), pw, ph).unwrap();
    assert!(roi[2] >= 1 && roi[3] >= 1);
    assert!(roi[0] + roi[2] <= w && roi[1] + roi[3] <= h);

    // At fit-like zoom the window would cover the whole frame: whole-frame
    // render instead of a degenerate crop.
    assert_eq!(
        LuminaApp::roi_from_zoom(w, h, 1.01, egui::Vec2::ZERO, pw, ph),
        None
    );
}

/// REVIEW-GUI-PANROI-1 follow-up: the hand tool must drive the ROI
/// re-render pipeline. A pan drag invalidates the render key and sets
/// `pending_full_render`, so the PERF-GUI-3/4 hot path renders a cheap
/// draft from the new offset while the pointer stays down, and the
/// debounced full render honours the FINAL pan (borders reachable).
#[test]
fn pan_drag_schedules_draft_and_final_roi_render() {
    let ctx = egui::Context::default();
    let mut app = LuminaApp::new(ctx.clone());
    // 200×150 source in an ~800×600 pane → base fit 4.0; at zoom 2 the
    // drawn image overflows the pane, so panning is eligible.
    app.load_bytes(
        ImageFrame::new(200, 150, [128_u8, 128, 128, 255].repeat(200 * 150))
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap(),
        "pan.png",
    )
    .unwrap();
    app.render().unwrap();
    // Viewport state as cached by previous frames of `draw_preview`.
    app.preview_zoom = 2.0;
    app.zoom_mode = ZoomMode::Custom;
    app.preview_base_fit_scale = 4.0;
    app.preview_src_w = 200.0;
    app.preview_src_h = 150.0;
    // `draw_preview` needs an existing preview texture to draw at all; it
    // derives the on-screen draw size from the texture dimensions, so it
    // must match a rendered full preview (200×150), not a tiny placeholder.
    app.texture = Some(ctx.load_texture(
        "preview",
        egui::ColorImage::filled([200, 150], egui::Color32::BLACK),
        egui::TextureOptions::LINEAR,
    ));

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let pass = |app: &mut LuminaApp, events: Vec<egui::Event>, time: f64| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.draw_preview(ui));
            },
        );
        // No GPU renderer consumes the per-frame texture deltas in these
        // headless tests; dropping them would trip epaint's
        // "unapplied deltas" debug assertion.
        output.textures_delta.clear();
    };

    // Pass 1: pointer-down inside the pane pans nothing yet.
    let start = screen.center();
    // Warm-up pass so the preview widget exists before the press is
    // hit-tested (egui registers interactions one frame after layout).
    pass(&mut app, vec![egui::Event::PointerMoved(start)], 0.9);
    pass(
        &mut app,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
        1.0,
    );
    assert!(!app.pending_full_render, "press alone must not re-render");

    // Pass 2: drag right — the pan changes, so a re-render must be armed.
    pass(
        &mut app,
        vec![egui::Event::PointerMoved(start + egui::vec2(60.0, 0.0))],
        1.1,
    );
    assert!(app.preview_pan.x > 0.0, "drag must move the pan");
    assert_eq!(app.zoom_mode, ZoomMode::Custom);
    assert!(
        app.pending_full_render,
        "pan change must schedule the full re-render"
    );
    assert!(
        app.render_key.is_none(),
        "pan change must invalidate the render key so the draft hot path fires"
    );

    // Pass 3: pointer release — the pending full render survives until the
    // debounce commits it with the FINAL pan offset.
    pass(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: start + egui::vec2(60.0, 0.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
        1.2,
    );
    assert!(
        app.pending_full_render,
        "released pan must still await its full render"
    );

    // The debounced full render consumes the pending flag and derives the
    // ROI from the post-drag pan.
    let (pw, ph) = (app.preview_pane_w, app.preview_pane_h);
    let pan = app.preview_pan;
    app.render_full([800, 600], None).unwrap();
    assert!(!app.pending_full_render);
    assert_eq!(
        app.preview_roi,
        LuminaApp::roi_from_zoom(
            app.original.as_ref().map(|o| o.width).unwrap_or_default(),
            app.original.as_ref().map(|o| o.height).unwrap_or_default(),
            2.0,
            pan,
            pw,
            ph,
        ),
        "full render must crop exactly the panned visible window"
    );
}

/// GUI-SCROLL-200-1 (single view): scroll-wheel zoom over the preview must
/// stay fluid — it only *arms* the debounced re-render pipeline
/// (`mark_dirty`); no synchronous full decode/full render may run inside
/// the frame. A synchronous render would have consumed
/// `pending_full_render` and produced a fresh `render_key` in the same
/// pass.
#[test]
/// GUI-PREVIEW-NAV-1: the scroll wheel without a modifier must never zoom
/// (and never switch the mode to `Custom`); with Ctrl held it zooms around
/// the cursor and arms the debounced full render without rendering
/// synchronously. Replaces the pre-zoom-gating assertion that any wheel
/// event zooms.
fn scroll_wheel_zoom_arms_debounce_without_synchronous_render() {
    let ctx = egui::Context::default();
    let mut app = LuminaApp::new(ctx.clone());
    app.load_bytes(
        ImageFrame::new(200, 150, [128_u8, 128, 128, 255].repeat(200 * 150))
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap(),
        "zoom.png",
    )
    .unwrap();
    app.render().unwrap();
    app.texture = Some(ctx.load_texture(
        "preview",
        egui::ColorImage::filled([200, 150], egui::Color32::BLACK),
        egui::TextureOptions::LINEAR,
    ));
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let pointer = screen.center();
    // Warm-up so the preview widget exists and hit-testing sees the cursor.
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(0.9),
            events: vec![egui::Event::PointerMoved(pointer)],
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| app.draw_preview(ui));
        },
    );
    output.textures_delta.clear();
    // Wheel WITHOUT a modifier: no zoom, no mode change, no render armed
    // (GUI-PREVIEW-NAV-1 — the image fits the pane here, so there is
    // nothing to pan either).
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(1.0),
            events: vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 120.0),
                phase: egui::TouchPhase::Move,
                modifiers: Default::default(),
            }],
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| app.draw_preview(ui));
        },
    );
    output.textures_delta.clear();
    assert_eq!(app.preview_zoom, 1.0, "modifier-free wheel must not zoom");
    assert_eq!(
        app.zoom_mode,
        ZoomMode::Fit,
        "modifier-free wheel must not switch to Custom"
    );
    assert!(
        !app.pending_full_render,
        "modifier-free wheel at fit arms no render"
    );
    // Wheel WITH Ctrl held: zoom around the cursor, pin Custom, arm the
    // debounced full render — without rendering synchronously.
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(1.1),
            events: vec![
                egui::Event::PointerMoved(pointer),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 120.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::CTRL,
                },
            ],
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| app.draw_preview(ui));
        },
    );
    output.textures_delta.clear();
    assert!(app.preview_zoom > 1.0, "wheel must zoom in");
    assert_eq!(app.zoom_mode, ZoomMode::Custom);
    assert!(
        app.pending_full_render,
        "zoom must arm the debounced full render"
    );
    assert!(
        app.render_key.is_none(),
        "no synchronous full render may run during the wheel frame"
    );
}

#[test]
fn sync_zoom_derives_absolute_modes_from_uncropped_source_fit() {
    // REVIEW-GUI-ZOOMLOOP-1 regression: absolute zoom modes must derive
    // from the fit of the pane against the UN-CROPPED source dimensions,
    // never from the currently displayed (ROI-cropped) texture — otherwise
    // 100%/200%/Fit-Width oscillate frame-by-frame once zoom > 1.
    let mut app = new_app();
    // 4000×3000 source in an 800×600 pane → base fit 0.2.
    app.preview_base_fit_scale = 0.2;
    app.preview_src_w = 4000.0;
    app.preview_src_h = 3000.0;
    app.preview_pane_w = 800.0;
    app.preview_pane_h = 600.0;

    // One-to-one: one source pixel per screen point → 1/fit.
    app.zoom_mode = ZoomMode::OneToOne;
    app.preview_zoom = 42.0; // stale value from a previous frame must not matter
    app.sync_zoom();
    assert!((app.preview_zoom - 5.0).abs() < 1e-5);

    // 200% likewise: 2/fit.
    app.zoom_mode = ZoomMode::TwoHundred;
    app.sync_zoom();
    assert!((app.preview_zoom - 10.0).abs() < 1e-5);

    // Fit-width: pane/source ratio relative to base fit (width-limited
    // here, so identical to fit).
    app.zoom_mode = ZoomMode::FitWidth;
    app.sync_zoom();
    assert!((app.preview_zoom - 1.0).abs() < 1e-5);

    // Stability across frames: re-deriving after a simulated render (the
    // texture changed, the cached base geometry did not) yields the exact
    // same value — no oscillation.
    app.zoom_mode = ZoomMode::OneToOne;
    app.sync_zoom();
    let first = app.preview_zoom;
    app.preview_base_fit_scale = 0.2; // unchanged by draw_preview by design
    app.sync_zoom();
    assert_eq!(app.preview_zoom, first);
}

/// GUI-PREVIEW-NAV-1: the fractional zoom steps resolve to a fraction of
/// the pane fit (Fit stays the default 1.0). Same geometry as the
/// `sync_zoom` regression test above: 4000×3000 source in 800×600 → fit 0.2.
#[test]
fn zoom_fraction_modes_derive_from_fit() {
    let mut app = new_app();
    app.preview_base_fit_scale = 0.2;
    app.preview_src_w = 4000.0;
    app.preview_src_h = 3000.0;
    app.preview_pane_w = 800.0;
    app.preview_pane_h = 600.0;

    // Fit (default) is always 1.0.
    app.zoom_mode = ZoomMode::Fit;
    app.sync_zoom();
    assert_eq!(app.preview_zoom, 1.0);

    // 25 % / 50 % / 75 % effective scale → fraction of fit.
    app.zoom_mode = ZoomMode::Quarter;
    app.sync_zoom();
    assert!((app.preview_zoom - 1.25).abs() < 1e-5);
    app.zoom_mode = ZoomMode::Half;
    app.sync_zoom();
    assert!((app.preview_zoom - 2.5).abs() < 1e-5);
    app.zoom_mode = ZoomMode::ThreeQuarter;
    app.sync_zoom();
    assert!((app.preview_zoom - 3.75).abs() < 1e-5);

    // Fractional steps map onto the zoom factor; near-fit steps still
    // cover the whole frame (whole-frame render, no degenerate crop),
    // while 50 %/75 % produce a real ROI crop (zoomed render).
    assert_eq!(
        LuminaApp::roi_from_zoom(4000, 3000, 1.25, egui::Vec2::ZERO, 800.0, 600.0),
        None,
        "25 % step still covers the frame"
    );
    assert!(
        LuminaApp::roi_from_zoom(4000, 3000, 2.5, egui::Vec2::ZERO, 800.0, 600.0).is_some(),
        "50 % step must crop"
    );
    assert!(
        LuminaApp::roi_from_zoom(4000, 3000, 3.75, egui::Vec2::ZERO, 800.0, 600.0).is_some(),
        "75 % step must crop"
    );
    assert_eq!(
        LuminaApp::roi_from_zoom(4000, 3000, 1.0, egui::Vec2::ZERO, 800.0, 600.0),
        None
    );
}
