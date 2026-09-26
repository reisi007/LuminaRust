//! MASK-LOCAL-P1.2a headless GUI tests for the mask-local tone-curve *graph*.
//!
//! Split from `mask_local_curves.rs` (file-size ratchet): this half drives the
//! real egui widget with pointer events, so a gesture in the Masking section is
//! proven to write to the selected mask layer and to nothing else.

use super::mask_local::local_app;
use super::mask_local_curves::{
    graph_pos, local_draw, local_graph_rect, local_point, press, LocalCurveHarness,
};
use super::*;
use crate::develop_tone::tone_curve_graph::tone_curve_display_output;

/// A real click on the local graph adds a point to the *mask layer*, and the
/// global curve graph next to it stays empty.
#[test]
fn a_click_on_the_local_graph_edits_the_mask_layer_only() {
    let (directory, mut app, _source) = local_app();
    let _ = directory;
    let mut harness = LocalCurveHarness::new();
    let rect = local_graph_rect(&mut harness, &mut app, "master", &mut local_draw());

    // The identity curve passes through the middle of the graph.
    let target = graph_pos(rect, 0.5, 0.5);
    harness.run(
        &mut app,
        vec![egui::Event::PointerMoved(target), press(target, true)],
        &mut local_draw(),
    );
    harness.run(
        &mut app,
        vec![egui::Event::PointerMoved(target), press(target, false)],
        &mut local_draw(),
    );
    harness.run(&mut app, vec![], &mut local_draw());

    let points = app.selected_mask_local_curve("master").unwrap();
    assert_eq!(points.len(), 3, "a click on the curve must add a point");
    assert!((points[1].input - 0.5).abs() < 0.02, "{points:?}");
    // The drawn spline really is the stored curve: the display sampler agrees
    // with the rendered tone stage's input mapping.
    assert!((tone_curve_display_output(&points, 0.5) - points[1].output).abs() < 1e-3);
    // Nothing global moved.
    assert!(app.recipe().curves.is_none());
    assert!(app.recipe().adjustments.is_empty());
    // And the edit armed the local transaction.
    assert_eq!(
        app.pending_slider_commit.as_ref().map(|(k, _)| k.as_str()),
        Some("mask.local.curves.master")
    );
}

/// Dragging an interior point moves it; the mandatory endpoints stay put.
#[test]
fn dragging_the_local_graph_moves_an_interior_point_only() {
    let (_directory, mut app, _source) = local_app();
    app.set_mask_local_curve_channel(
        "master",
        vec![
            local_point(0.0, 0.0),
            local_point(0.5, 0.5),
            local_point(1.0, 1.0),
        ],
    )
    .unwrap();
    let mut harness = LocalCurveHarness::new();
    let rect = local_graph_rect(&mut harness, &mut app, "master", &mut local_draw());

    let grab = graph_pos(rect, 0.5, 0.5);
    let drop = graph_pos(rect, 0.5, 0.8);
    harness.run(
        &mut app,
        vec![egui::Event::PointerMoved(grab), press(grab, true)],
        &mut local_draw(),
    );
    // Two move frames: the first crosses egui's drag threshold, the second
    // carries the pointer to the target.
    harness.run(
        &mut app,
        vec![egui::Event::PointerMoved(drop)],
        &mut local_draw(),
    );
    harness.run(
        &mut app,
        vec![egui::Event::PointerMoved(drop)],
        &mut local_draw(),
    );
    harness.run(&mut app, vec![press(drop, false)], &mut local_draw());
    harness.run(&mut app, vec![], &mut local_draw());

    let points = app.selected_mask_local_curve("master").unwrap();
    assert_eq!(points.len(), 3);
    assert!((points[1].output - 0.8).abs() < 0.02, "{points:?}");
    assert_eq!(points[0], local_point(0.0, 0.0));
    assert_eq!(points[2], local_point(1.0, 1.0));
    assert!(app.recipe().curves.is_none());
}

/// The local editor refuses the mandatory endpoints loudly, exactly like the
/// global one, and a click on an endpoint is a no-op (never an inserted point).
#[test]
fn the_local_graph_refuses_to_move_an_endpoint() {
    let (_directory, mut app, _source) = local_app();
    let mut harness = LocalCurveHarness::new();
    let rect = local_graph_rect(&mut harness, &mut app, "master", &mut local_draw());
    let corner = graph_pos(rect, 0.0, 0.0);
    // A plain click on the endpoint inserts nothing.
    harness.run(
        &mut app,
        vec![egui::Event::PointerMoved(corner), press(corner, true)],
        &mut local_draw(),
    );
    harness.run(
        &mut app,
        vec![egui::Event::PointerMoved(corner), press(corner, false)],
        &mut local_draw(),
    );
    assert_eq!(app.selected_mask_local_curve("master").unwrap().len(), 2);
    assert!(!app.has_mask_local_curves().unwrap());

    // Dragging the endpoint is refused loudly, and the curve is untouched.
    let away = graph_pos(rect, 0.4, 0.4);
    harness.run(
        &mut app,
        vec![egui::Event::PointerMoved(corner), press(corner, true)],
        &mut local_draw(),
    );
    harness.run(
        &mut app,
        vec![egui::Event::PointerMoved(away)],
        &mut local_draw(),
    );
    harness.run(
        &mut app,
        vec![egui::Event::PointerMoved(away)],
        &mut local_draw(),
    );
    harness.run(&mut app, vec![press(away, false)], &mut local_draw());
    assert!(app.status().contains("endpoints"), "{}", app.status());
    assert_eq!(
        app.selected_mask_local_curve("master").unwrap(),
        vec![local_point(0.0, 0.0), local_point(1.0, 1.0)]
    );
    assert!(app.pending_mask_state_before.is_none());
}
