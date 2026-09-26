//! GUI-INT-MASKLOCAL-38: **addressing the mask-local tone-curve editor** —
//! locating the graph (a painted area, not an accesskit node), disambiguating
//! its labels, and driving the gestures the shared `curve_graph_gesture`
//! decoder turns pointer input into: set (click on the drawn curve), move
//! (drag), delete (double click) and the two resets. Every frame-time
//! precondition the gestures need is documented at its helper.
//!
//! Used only by `mask_local_editors_wiring.rs`. The label lookups live
//! here rather than in the shared core because the curve block is the reason
//! they exist: it contributes a second "Red" (a curve channel next to the HSL
//! band) and a third "Reset" (per-channel next to the block-wide one) to a
//! panel that already repeats both labels several times.

use super::mask_local_editors_support::{nodes, settle};
use eframe::egui::{Pos2, Rect};
use egui_kittest::Harness;
use lumina_gui::LuminaApp;

/// Side length of the interactive curve graph
/// (`develop_tone::tone_curve_graph::TONE_CURVE_GRAPH_SIDE`). Mirrored on
/// purpose: the golden guard must not reuse the constant it verifies.
pub const CURVE_GRAPH_SIDE: f32 = 184.0;

/// The mask-local curve graph rectangle: the panel's unique
/// `CURVE_GRAPH_SIDE` square among the frame's painted shapes.
///
/// The graph is a painted `Ui::interact` area, not an accesskit node, so it is
/// found by geometry. Only the *outermost* square counts: egui paints the
/// filled background **and** the border stroke at the same rect, and every
/// control point is a 4pt circle — so filtering on the exact side length
/// excludes the points and the dedup merges fill and stroke.
pub fn curve_graph_rect(harness: &Harness<'_, LuminaApp>) -> eframe::egui::Rect {
    let mut found: Vec<eframe::egui::Rect> = harness
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
            .x
            .total_cmp(&b.min.x)
            .then_with(|| a.min.y.total_cmp(&b.min.y))
    });
    found.dedup_by(|a, b| a.center().distance(b.center()) < 0.5);
    assert_eq!(
        found.len(),
        1,
        "the mask-local curve graph must be the panel's only {CURVE_GRAPH_SIDE}pt square, \
         got {found:?}"
    );
    found[0]
}

/// The rect of the single occurrence of `label`, or `None`.
///
/// A slider contributes two accesskit nodes with the same label (the caption
/// and its numeric readout), so "present" must not mean "unique" here — the
/// anchor-based lookups below filter by direction, and the panel's caption
/// lookups belong to `mask_local_slider_support::slider_rail`.
pub fn find_rect(harness: &Harness<'_, LuminaApp>, label: &str) -> Option<Rect> {
    let found = nodes(harness, label);
    match found.len() {
        0 => None,
        1 => Some(found[0]),
        _ => panic!("expected at most one {label:?} node in the Masking panel, got {found:?}"),
    }
}

/// The nearest `label` node strictly **below** `anchor`.
///
/// Used for the curve block's channel selector: "Red" under the panel-unique
/// "Channel" caption is the curve channel, the HSL band of the same name sits
/// further down. `anchor` must be a panel-unique label.
pub fn below(harness: &Harness<'_, LuminaApp>, anchor: &str, label: &str) -> Rect {
    let anchor_y = find_rect(harness, anchor)
        .unwrap_or_else(|| panic!("anchor label {anchor:?} is not painted in the Masking panel"))
        .max
        .y;
    let found: Vec<Rect> = nodes(harness, label)
        .into_iter()
        .filter(|rect| rect.min.y >= anchor_y)
        .collect();
    let nearest = found
        .iter()
        .min_by(|a, b| {
            a.min
                .y
                .total_cmp(&b.min.y)
                .then_with(|| a.min.x.total_cmp(&b.min.x))
        })
        .copied();
    nearest.unwrap_or_else(|| panic!("no {label:?} node below {anchor:?} in the Masking panel"))
}

/// The nearest `label` node strictly **above** `anchor`. See [`below`].
///
/// Used for the per-channel reset: anchored on the panel-unique "all local
/// curves reset" button, the nearest "Reset" above it is the selected
/// channel's reset.
pub fn above(harness: &Harness<'_, LuminaApp>, anchor: &str, label: &str) -> Rect {
    let anchor_y = find_rect(harness, anchor)
        .unwrap_or_else(|| panic!("anchor label {anchor:?} is not painted in the Masking panel"))
        .min
        .y;
    let found: Vec<Rect> = nodes(harness, label)
        .into_iter()
        .filter(|rect| rect.max.y <= anchor_y)
        .collect();
    found
        .iter()
        .max_by(|a, b| {
            a.min
                .y
                .total_cmp(&b.min.y)
                .then_with(|| a.min.x.total_cmp(&b.min.x))
        })
        .copied()
        .unwrap_or_else(|| panic!("no {label:?} node above {anchor:?} in the Masking panel"))
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
