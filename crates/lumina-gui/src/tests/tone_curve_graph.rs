//! UX-LOOK-TONECURVE-18 (UXG-16): headless GUI tests for the interactive
//! tone-curve graph.
//!
//! Covers, per channel: set (click on the curve) / drag (move) / delete
//! (double-click), persistence and reload, loud clamping of out-of-range
//! moves, and the display-vs-render parity of the drawn spline against the
//! real `lumina_core::render_frame` curve stage. The graph has no text label,
//! so gestures are driven by egui pointer events at the registered widget id.

use super::*;
use crate::develop_tone::tone_curve_graph::{
    tone_curve_channel_points, tone_curve_display_output, tone_curve_graph_id,
};

/// Persistent headless context with an advancing clock (double-click
/// detection needs `time`).
struct GraphHarness {
    ctx: egui::Context,
    time: f64,
}

impl GraphHarness {
    fn new() -> Self {
        Self {
            ctx: egui::Context::default(),
            time: 0.0,
        }
    }

    fn run(
        &mut self,
        app: &mut LuminaApp,
        events: Vec<egui::Event>,
        draw: &mut dyn FnMut(&mut LuminaApp, &mut egui::Ui),
    ) {
        self.time += 1.0 / 60.0;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 4096.0));
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ui| draw(app, ui),
        );
        // No GPU renderer consumes the per-frame texture deltas headless.
        output.textures_delta.clear();
    }
}

fn press(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

/// Draw the Tone Curve section once and return the graph rect of `channel`
/// plus the harness to continue interacting on the same context.
fn graph_rect(
    harness: &mut GraphHarness,
    app: &mut LuminaApp,
    channel: &str,
    draw: &mut dyn FnMut(&mut LuminaApp, &mut egui::Ui),
) -> egui::Rect {
    harness.run(app, vec![], draw);
    harness
        .ctx
        .read_response(tone_curve_graph_id(channel))
        .unwrap_or_else(|| panic!("tone curve graph for {channel:?} must be painted"))
        .rect
}

/// Click (or double-click) `at` (graph fractions, y up) on the `channel` graph.
fn graph_click_at(
    app: &mut LuminaApp,
    channel_index: usize,
    channel: &str,
    at: (f32, f32),
    double: bool,
) {
    app.tone_curve_channel = channel_index;
    app.set_section_open(SECTION_TONE_CURVE, true);
    let mut harness = GraphHarness::new();
    let mut draw = |app: &mut LuminaApp, ui: &mut egui::Ui| app.draw_tone_curve(ui);
    let rect = graph_rect(&mut harness, app, channel, &mut draw);
    let pos = egui::pos2(
        rect.left() + at.0 * rect.width(),
        rect.bottom() - at.1 * rect.height(),
    );
    let clicks = if double { 2 } else { 1 };
    for _ in 0..clicks {
        harness.run(
            app,
            vec![egui::Event::PointerMoved(pos), press(pos, true)],
            &mut draw,
        );
        harness.run(
            app,
            vec![egui::Event::PointerMoved(pos), press(pos, false)],
            &mut draw,
        );
    }
    harness.run(app, vec![], &mut draw);
}

/// [`graph_click_at`] on the graph centre (the identity curve's midpoint),
/// capturing the debug action log for the click→instrumentation wiring.
#[cfg(debug_assertions)]
fn graph_click_logged(
    app: &mut LuminaApp,
    channel_index: usize,
    channel: &str,
    double: bool,
) -> Vec<String> {
    let _ = take_gui_action_log();
    graph_click_at(app, channel_index, channel, (0.5, 0.5), double);
    take_gui_action_log()
}

/// Set up the graph for `channel` (no click), then add a point at the centre
/// of the identity curve and drag it to `to` (graph fractions, y up).
fn graph_add_then_drag(app: &mut LuminaApp, channel_index: usize, channel: &str, to: (f32, f32)) {
    app.tone_curve_channel = channel_index;
    app.set_section_open(SECTION_TONE_CURVE, true);
    let mut harness = GraphHarness::new();
    let mut draw = |app: &mut LuminaApp, ui: &mut egui::Ui| app.draw_tone_curve(ui);
    let rect = graph_rect(&mut harness, app, channel, &mut draw);
    let center = rect.center();
    // Click on the identity curve (passes through the centre) to add a point.
    harness.run(
        app,
        vec![egui::Event::PointerMoved(center), press(center, true)],
        &mut draw,
    );
    harness.run(
        app,
        vec![egui::Event::PointerMoved(center), press(center, false)],
        &mut draw,
    );
    // Press the new point, move past the drag threshold to `to`, release.
    let target = egui::pos2(
        rect.left() + to.0 * rect.width(),
        rect.bottom() - to.1 * rect.height(),
    );
    harness.run(
        app,
        vec![egui::Event::PointerMoved(center), press(center, true)],
        &mut draw,
    );
    harness.run(app, vec![egui::Event::PointerMoved(target)], &mut draw);
    harness.run(app, vec![egui::Event::PointerMoved(target)], &mut draw);
    harness.run(app, vec![press(target, false)], &mut draw);
    harness.run(app, vec![], &mut draw);
}

/// The stored point list of `channel` (identity fallback included).
fn stored_points(app: &LuminaApp, channel: &str) -> Vec<CurvePoint> {
    tone_curve_channel_points(app.recipe(), channel)
}

#[cfg(debug_assertions)]
#[test]
fn graph_gesture_add_logs_curve_action() {
    let (_directory, mut app) = persistent_app();
    let lines = graph_click_logged(&mut app, 0, "master", false);
    assert_eq!(lines.len(), 1, "one add gesture, got {lines:?}");
    assert!(
        lines[0].starts_with(&format!("action={} ", GuiAction::AddCurvePoint.name())),
        "got {:?}",
        lines[0]
    );
}

#[cfg(debug_assertions)]
#[test]
fn graph_gesture_remove_logs_curve_action() {
    let (_directory, mut app) = persistent_app();
    app.add_curve_point("master", 0.5, 0.5);
    let lines = graph_click_logged(&mut app, 0, "master", true);
    assert_eq!(lines.len(), 1, "one double-click remove, got {lines:?}");
    assert!(
        lines[0].starts_with(&format!("action={} ", GuiAction::RemoveCurvePoint.name())),
        "got {:?}",
        lines[0]
    );
}

/// Set / drag / delete on Master, with persistence through the sidecar and a
/// reload roundtrip (recipe only — no schema change).
#[test]
fn graph_set_drag_delete_persists_and_reloads() {
    let (directory, mut app) = persistent_app();
    let source = directory.path().join("photo.png");
    // Set: click on the identity curve adds one interior point at (0.5, 0.5).
    graph_add_then_drag(&mut app, 0, "master", (0.5, 0.75));
    let points = stored_points(&app, "master");
    assert_eq!(points.len(), 3, "one interior point added: {points:?}");
    assert!(
        (points[1].input - 0.5).abs() < 1e-3,
        "drag keeps the input: {points:?}"
    );
    assert!(
        (points[1].output - 0.75).abs() < 1e-3,
        "drag sets the output: {points:?}"
    );
    // Persist + reload.
    let document = commit_and_load_doc(&mut app, &source);
    let persisted = document.virtual_copies[0]
        .recipe
        .curves
        .clone()
        .expect("curves persisted")
        .master;
    assert_eq!(persisted.len(), 3, "persisted: {persisted:?}");
    assert!((persisted[1].output - 0.75).abs() < 1e-3);
    let reopened = reopen_app(&source);
    let reloaded = stored_points(&reopened, "master");
    assert_eq!(reloaded.len(), 3, "reloaded: {reloaded:?}");
    assert!((reloaded[1].output - 0.75).abs() < 1e-3);
    // Delete: double-click the (dragged) interior point at its actual
    // location (the curve is no longer identity after the reload). The
    // click→`RemoveCurvePoint` log wiring is pinned by
    // `graph_gesture_remove_logs_curve_action` (debug-only).
    graph_click_at(&mut app, 0, "master", (0.5, 0.75), true);
    let after = stored_points(&app, "master");
    assert_eq!(after.len(), 2, "interior point removed: {after:?}");
}

/// Set/delete works independently per channel (Master/Red/Green/Blue).
#[test]
fn graph_edits_are_per_channel() {
    let (_directory, mut app) = persistent_app();
    const CHANNELS: [&str; 4] = ["master", "red", "green", "blue"];
    for (index, channel) in [(1usize, "red"), (2, "green"), (3, "blue")] {
        let before: Vec<(&str, usize)> = CHANNELS
            .iter()
            .map(|name| (*name, stored_points(&app, name).len()))
            .collect();
        graph_add_then_drag(&mut app, index, channel, (0.5, 0.6));
        for (name, len) in before {
            let after = stored_points(&app, name).len();
            if name == channel {
                assert_eq!(after, len + 1, "{channel} gained exactly one point");
            } else {
                assert_eq!(
                    after, len,
                    "{name} must be unchanged while editing {channel}"
                );
            }
        }
    }
    assert_eq!(stored_points(&app, "master").len(), 2);
}

/// Out-of-range moves are clamped (never silently swallowed) and endpoint
/// moves are refused loudly.
#[test]
fn graph_moves_are_clamped_or_refused_loudly() {
    let (_directory, mut app) = persistent_app();
    app.add_curve_point("red", 0.5, 0.5);
    // Far past the normative range: clamped into the point's neighbour gap and
    // the 0..=1 output range, then saved.
    app.move_curve_point("red", 1, 5.0, -3.0);
    let red = stored_points(&app, "red");
    let point = red[1];
    assert!(
        point.input > 0.0 && point.input < 1.0,
        "input clamped into (0,1): {point:?}"
    );
    assert_eq!(point.output, 0.0, "output clamped to 0: {point:?}");
    assert!(
        app.pending_slider_commit.is_some(),
        "a clamped move is still a committed edit"
    );
    // Endpoints are mandatory: a move is refused loudly and the point list is
    // unchanged.
    let before = stored_points(&app, "red");
    app.move_curve_point("red", 0, 0.4, 0.4);
    assert!(
        app.status.contains("mandatory"),
        "endpoint refusal must be loud: {}",
        app.status
    );
    assert_eq!(
        stored_points(&app, "red"),
        before,
        "refused move left no trace"
    );
    // Unknown channels warn without a commit (no silent fallback).
    app.move_curve_point("bogus", 1, 0.5, 0.5);
    // Neighbours closer than twice the minimum gap: refuse loudly instead of
    // panicking in `f32::clamp`.
    for input in [0.5_f64, 0.505, 0.508] {
        app.add_curve_point("green", input, 0.5);
    }
    let green_before = stored_points(&app, "green");
    app.move_curve_point("green", 2, 0.6, 0.5);
    assert!(
        app.status.contains("minimum input gap"),
        "dense-neighbour refusal must be loud: {}",
        app.status
    );
    assert_eq!(
        stored_points(&app, "green"),
        green_before,
        "dense-neighbour refusal left no trace"
    );
}

/// Headless paint guard: the graph widget paints the sampled spline plus one
/// circle per control point (a non-identity curve must actually change the
/// drawn geometry). Complements the pixel golden in `kittest_snapshots`.
#[test]
fn graph_paints_curve_and_control_points() {
    let (_directory, mut app) = persistent_app();
    app.add_curve_point("master", 0.5, 0.8);
    app.set_section_open(SECTION_TONE_CURVE, true);
    let (shapes, ctx) = headless_frame_sized(&mut app, 8000.0, |app, ui| app.draw_tone_curve(ui));
    let rect = ctx
        .read_response(tone_curve_graph_id("master"))
        .expect("graph painted")
        .rect;
    let mut spline_samples = 0usize;
    let mut circles = 0usize;
    for clipped in &shapes {
        match &clipped.shape {
            egui::Shape::Path(path) if !path.points.is_empty() => {
                if path.points.iter().all(|p| rect.expand(1.0).contains(*p)) {
                    spline_samples = spline_samples.max(path.points.len());
                }
            }
            egui::Shape::Circle(circle) if rect.expand(1.0).contains(circle.center) => {
                circles += 1;
            }
            _ => {}
        }
    }
    assert!(
        spline_samples > 2,
        "the drawn spline must be a sampled polyline, got {spline_samples} points"
    );
    assert!(
        circles >= 3,
        "two endpoints + one interior control point must be painted, got {circles}"
    );
}

/// The display sampler must equal what `render_frame` actually applies: a
/// grayscale ramp through a master-only curve maps each pixel to the same
/// `monotone_curve` value the graph draws.
#[test]
fn display_sampler_matches_core_render() {
    let points = vec![
        CurvePoint {
            input: 0.0,
            output: 0.0,
        },
        CurvePoint {
            input: 0.25,
            output: 0.10,
        },
        CurvePoint {
            input: 0.5,
            output: 0.6,
        },
        CurvePoint {
            input: 1.0,
            output: 1.0,
        },
    ];
    let mut pixels = Vec::with_capacity(256 * 4);
    for v in 0u16..=255 {
        let v = v as u8;
        pixels.extend_from_slice(&[v, v, v, 255]);
    }
    let ramp = ImageFrame::new(256, 1, pixels).unwrap();
    let mut recipe = EditRecipe::default();
    recipe.curves = Some(Curves {
        version: 1,
        master: points.clone(),
        channels: CurveChannels::default(),
    });
    let context = RenderContext {
        recipe: &recipe,
        camera_white_balance: None,
        source_actions: &[],
        masks: None,
        lensfun: None,
        depth: None,
    };
    let rendered = lumina_core::render_frame(&ramp, &context).unwrap().frame;
    for v in 0u16..=255 {
        let expected =
            (tone_curve_display_output(&points, v as f32 / 255.0) * 255.0).round() as i32;
        let actual = i32::from(rendered.pixels[(v as usize) * 4]);
        assert!(
            (expected - actual).abs() <= 1,
            "display sampler diverged from render at {v}: display {expected}, render {actual}"
        );
    }
    // A non-identity curve must actually differ from the ramp (non-vacuous).
    assert_ne!(rendered.pixels[64 * 4], 64, "curve must change midtones");
}
