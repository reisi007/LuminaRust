//! GUI-INT-MASKLOCAL-38: the mask-local **curve-graph** geometry and gestures
//! — locating the graph (a painted area, not an accesskit node) and driving the
//! gestures the shared `curve_graph_gesture` decoder turns pointer input into:
//! set (click on the drawn curve), move (drag) and delete (double click).
//! Every frame-time precondition the gestures need is documented at its helper.
//!
//! Used only by `mask_local_editors_wiring.rs`. The graph's *label* addressing
//! is not here but in `mask_local_label_support`: "Red" and "Reset" are
//! ambiguous in the Masking panel for reasons that have nothing to do with the
//! curve, and the colour target needs the same lookups.

use super::mask_local_editors_support::settle;
use eframe::egui::{Pos2, Rect};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use lumina_gui::LuminaApp;

/// Side length of the interactive curve graph
/// (`develop_tone::tone_curve_graph::TONE_CURVE_GRAPH_SIDE`). Mirrored on
/// purpose: the golden guard must not reuse the constant it verifies.
pub const CURVE_GRAPH_SIDE: f32 = 184.0;

/// Every unique `CURVE_GRAPH_SIDE` square in the frame, top to bottom.
///
/// The graph is a painted `Ui::interact` area, not an accesskit node, so it is
/// found by geometry. Only the *outermost* square counts: egui paints the
/// filled background **and** the border stroke at the same rect, and every
/// control point is a 4pt circle — so filtering on the exact side length
/// excludes the points and the dedup merges fill and stroke.
pub fn curve_graph_rects(harness: &Harness<'_, LuminaApp>) -> Vec<Rect> {
    let mut found: Vec<Rect> = harness
        .output()
        .shapes
        .iter()
        .map(|clipped| clipped.shape.visual_bounding_rect())
        .filter(|rect| {
            (rect.width() - CURVE_GRAPH_SIDE).abs() < 1.0
                && (rect.height() - CURVE_GRAPH_SIDE).abs() < 1.0
        })
        .collect();
    found.sort_by(|a, b| {
        a.min
            .y
            .total_cmp(&b.min.y)
            .then_with(|| a.min.x.total_cmp(&b.min.x))
    });
    found.dedup_by(|a, b| a.center().distance(b.center()) < 0.5);
    found
}

/// The mask-local curve graph rectangle: the panel's unique
/// `CURVE_GRAPH_SIDE` square among the frame's painted shapes.
pub fn curve_graph_rect(harness: &Harness<'_, LuminaApp>) -> Rect {
    let found = curve_graph_rects(harness);
    assert_eq!(
        found.len(),
        1,
        "the mask-local curve graph must be the panel's only {CURVE_GRAPH_SIDE}pt square, \
         got {found:?}"
    );
    found[0]
}

/// The global and the mask-local curve graph rect, `(global, local)`.
///
/// Which of the two painted squares is which is **not** assumed from the
/// section order: the mask-local block prints a readout line no global block
/// has (`local curves.<channel>: N pts, mid X`) directly below its own graph,
/// so the local graph is the square **immediately** above that unique text.
/// Deriving the discriminator from the editor's own output keeps this correct
/// even if the two Develop sections are ever reordered on screen.
pub fn global_and_local_graph_rects(harness: &Harness<'_, LuminaApp>) -> (Rect, Rect) {
    let readout_rect = harness
        .query_all_by_label_contains("local curves.")
        .next()
        .expect("the mask-local curve block must print its `local curves.` readout")
        .rect();
    let squares = curve_graph_rects(harness);
    assert_eq!(
        squares.len(),
        2,
        "opening Tone Curve next to Masking must paint exactly two curve graphs, got {squares:?}"
    );
    // The readout is below *both* graphs, so "above" alone cannot pick one:
    // the local graph is the *nearest* one above it.
    let mut above: Vec<Rect> = squares
        .iter()
        .copied()
        .filter(|rect| rect.max.y <= readout_rect.min.y)
        .collect();
    above.sort_by(|a, b| b.max.y.total_cmp(&a.max.y));
    let local = above
        .first()
        .copied()
        .unwrap_or_else(|| panic!("no curve graph above the mask-local readout in {squares:?}"));
    let global = squares
        .iter()
        .copied()
        .find(|rect| rect.center().distance(local.center()) > 1.0)
        .expect("the second curve graph");
    assert!(
        readout_rect.min.y - local.max.y <= readout_rect.min.y - global.max.y,
        "the local graph must be the one directly above its readout, got {local:?} / {global:?}"
    );
    (global, local)
}

/// Map graph fractions (input right, output up) to a screen position.
fn graph_pos(graph: Rect, input: f32, output: f32) -> Pos2 {
    Pos2::new(
        graph.min.x + graph.width() * input.clamp(0.0, 1.0),
        graph.max.y - graph.height() * output.clamp(0.0, 1.0),
    )
}

/// Click the drawn curve at graph fraction `(input, output)`.
///
/// A click only inserts a point when it lands **on** the drawn curve (within
/// the shared `TONE_CURVE_LINE_TOLERANCE`), so the target must be a point of
/// the currently drawn spline — on an identity channel that is the diagonal.
pub fn click_curve_point(
    harness: &mut Harness<'_, LuminaApp>,
    graph: Rect,
    input: f32,
    output: f32,
) {
    let pos = graph_pos(graph, input, output);
    harness.hover_at(pos);
    harness.drag_at(pos);
    settle(harness, 1);
    harness.drop_at(pos);
    settle(harness, 2);
}

/// Screen position of graph fraction `(input, output)` in `graph`.
pub fn graph_pos_of(harness: &Harness<'_, LuminaApp>, input: f32, output: f32) -> Pos2 {
    graph_pos(curve_graph_rect(harness), input, output)
}

/// Frames that must elapse after a pointer gesture before a *new* click gesture
/// is a click in its own right.
///
/// `egui` keeps a click **chain** (`last_click_time` / `last_click_pos`) and
/// counts a release as a double/triple click when it lands within
/// `max_double_click_delay` (0.3 s) of the previous one. A curve edit is itself
/// two clicks (insert, then drag-release), so a double click issued right after
/// it is classified as a *triple* and never reaches `double_clicked()`. At
/// [`STEP_DT`] that window is ~18 frames; 20 clears it with a margin.
pub const CLICK_CHAIN_FRAMES: usize = 20;

/// Double-click at `pos`: the gesture `egui` folds into `double_clicked()`
/// (the shared curve decoder turns it into a curve-point removal).
///
/// Three frame-time conditions, all measured rather than assumed:
///
/// 1. the press and the release must sit in **separate** frames, so each frame
///    runs `ui.interact` and records its own release;
/// 2. the step size must be small enough that the two releases stay inside
///    `max_double_click_delay` — see [`STEP_DT`];
/// 3. the *preceding* click must already have left the chain window, otherwise
///    the pair is classified as a triple click — see [`CLICK_CHAIN_FRAMES`],
///    which the caller applies via [`break_click_chain`] before this helper.
pub fn double_click(harness: &mut Harness<'_, LuminaApp>, pos: Pos2) {
    for _ in 0..2 {
        harness.hover_at(pos);
        harness.drag_at(pos);
        harness.step();
        harness.drop_at(pos);
        harness.step();
    }
}

/// Let `egui`'s click chain expire so the next click gesture is counted from
/// one (see [`CLICK_CHAIN_FRAMES`]).
pub fn break_click_chain(harness: &mut Harness<'_, LuminaApp>) {
    settle(harness, CLICK_CHAIN_FRAMES);
}

/// Drag the control point at graph fraction `(input, output)` to
/// `(to_input, to_output)`.
///
/// A point dropped back **onto** the identity diagonal is a legal but
/// pixel-neutral curve (`curve_points_are_identity` is purely `input == output`),
/// so a gesture that must be observable in the render identity has to move the
/// point off the diagonal. Hence this two-step helper: click to insert on the
/// drawn curve, then drag the inserted point away from it.
pub fn edit_curve_point(
    harness: &mut Harness<'_, LuminaApp>,
    graph: Rect,
    input: f32,
    output: f32,
    to_input: f32,
    to_output: f32,
) {
    click_curve_point(harness, graph, input, output);
    let from = graph_pos(graph, input, output);
    let to = graph_pos(graph, to_input, to_output);
    harness.hover_at(from);
    harness.drag_at(from);
    settle(harness, 1);
    harness.hover_at(to);
    settle(harness, 1);
    harness.hover_at(to);
    settle(harness, 1);
    harness.drop_at(to);
    settle(harness, 2);
}
