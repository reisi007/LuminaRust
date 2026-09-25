//! UX-LOOK-TONECURVE-18 (UXG-16): the interactive Tone Curve graph.
//!
//! Replaces the former per-point slider rows ("P0/P1 …") with a Lightroom-like
//! point-curve editor per channel (Master/Red/Green/Blue):
//!
//! * **Set:** a single click on the drawn curve adds a control point on the
//!   curve at the clicked input (the curve stays visually unchanged until it is
//!   dragged — Lightroom behavior). A click away from the curve is a no-op.
//! * **Drag:** dragging an interior point moves it; its input is clamped
//!   strictly between its neighbours (core requires ascending inputs), its
//!   output to the normative `0..=1` range.
//! * **Delete:** a double-click on an interior point removes it. The endpoints
//!   `(0,0)`/`(1,1)` are mandatory for the recipe/core validation and are never
//!   removable or draggable; attempts are refused loudly (status, no save).
//!
//! Every edit routes through the existing instrumented setters
//! (`add_curve_point`/`remove_curve_point`, and the new `move_curve_point`), so
//! points persist in the existing `curves` recipe block — no schema change and
//! no migration.
//!
//! The drawn spline mirrors `lumina-core::monotone_curve` (monotone cubic
//! Hermite / PCHIP). `display_sampler_matches_core_render` pins the display
//! sampler against the real `render_frame` output, so the graph can never show
//! a curve the renderer does not apply. The curve stage stays fully
//! GPU-evaluated (`lumina-gpu/src/stages.rs` has the same `monotone_curve`); the
//! GUI only reads it.

use super::*;
use log::{info, warn};

/// Side length (points) of the square curve graph.
pub(crate) const TONE_CURVE_GRAPH_SIDE: f32 = 184.0;
/// Radius (points) of a drawn control point.
const TONE_CURVE_POINT_RADIUS: f32 = 4.0;
/// Screen radius (points) that grabs/deletes a control point.
pub(crate) const TONE_CURVE_HIT_RADIUS: f32 = 10.0;
/// Screen distance (points) within which a click counts as "on the curve".
const TONE_CURVE_LINE_TOLERANCE: f32 = 8.0;
/// Minimum input gap between neighbouring control points (strictly ascending).
pub(crate) const TONE_CURVE_MIN_GAP: f32 = 0.005;
/// Polyline resolution used to stroke the spline.
const TONE_CURVE_SAMPLES: usize = 72;

/// Stable widget id of the per-channel curve graph: used by the F-100 audit
/// (`f100_audit.rs`) and the headless gesture tests to locate the widget.
pub(crate) fn tone_curve_graph_id(channel: &str) -> egui::Id {
    egui::Id::new("lumina.tone_curve_graph").with(channel)
}

/// Stable widget id of the **mask-local** curve graph (MASK-LOCAL-P1.2a).
///
/// A separate id is mandatory, not cosmetic: the local and the global graph
/// are painted in the same UI tree (Masking next to Tone Curve) and must not
/// share egui interaction state — a drag on one could otherwise move a point
/// of the other.
pub(crate) fn local_tone_curve_graph_id(channel: &str) -> egui::Id {
    egui::Id::new("lumina.local_tone_curve_graph").with(channel)
}

/// One decoded point-curve gesture. Both the global editor and the mask-local
/// editor (MASK-LOCAL-P1.2a) consume the *same* gesture decoder, so the two
/// graphs cannot drift apart in hit radius, minimum gap, mandatory endpoints
/// or the "click only counts on the curve" rule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CurveGesture {
    Move {
        index: usize,
        input: f32,
        output: f32,
    },
    Add {
        input: f32,
        output: f32,
    },
    Remove {
        index: usize,
    },
    /// A drag/click targeted a mandatory endpoint.
    EndpointRefused,
}

/// Decode the pointer gestures of one curve graph into intent.
///
/// `memory_id` is the caller's per-graph drag slot, so two graphs on screen
/// never share the "currently dragged point" state.
pub(crate) fn curve_graph_gesture(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    points: &[CurvePoint],
    memory_id: egui::Id,
) -> Vec<CurveGesture> {
    let mut gestures = Vec::new();
    if response.drag_started() {
        // Grab the point under the *press origin*: by the time a drag is
        // recognized the pointer has already moved past the point.
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .or_else(|| response.interact_pointer_pos());
        if let Some(pos) = origin {
            match nearest_point_index(points, rect, pos, TONE_CURVE_HIT_RADIUS) {
                Some(index) if index > 0 && index + 1 < points.len() => {
                    ui.memory_mut(|m| m.data.insert_temp(memory_id, index));
                }
                Some(_) => gestures.push(CurveGesture::EndpointRefused),
                None => {}
            }
        }
    }
    if response.dragged() {
        if let Some(index) = ui.memory(|m| m.data.get_temp::<usize>(memory_id)) {
            if let (Some(pos), Some(point)) = (response.interact_pointer_pos(), points.get(index)) {
                let (input, output) = graph_to_curve(rect, pos);
                let (input, output) = clamped_point_move(points, index, input, output);
                if (input - point.input).abs() > f32::EPSILON
                    || (output - point.output).abs() > f32::EPSILON
                {
                    gestures.push(CurveGesture::Move {
                        index,
                        input,
                        output,
                    });
                }
            }
        }
    }
    if response.drag_stopped() {
        ui.memory_mut(|m| m.data.remove::<usize>(memory_id));
    }
    if response.double_clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if let Some(index) = nearest_point_index(points, rect, pos, TONE_CURVE_HIT_RADIUS) {
                gestures.push(CurveGesture::Remove { index });
            }
        }
    } else if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if nearest_point_index(points, rect, pos, TONE_CURVE_HIT_RADIUS).is_none() {
                let (input, _) = graph_to_curve(rect, pos);
                let on_curve = tone_curve_display_output(points, input);
                if curve_to_graph(rect, input, on_curve).distance(pos) <= TONE_CURVE_LINE_TOLERANCE
                {
                    gestures.push(CurveGesture::Add {
                        input,
                        output: on_curve,
                    });
                }
            }
        }
    }
    gestures
}

/// The stored control points of one curve channel. A channel without an
/// explicit list (or with a malformed shorter-than-two list) reads as the
/// two-point identity — the same read-back the panel used before.
pub(crate) fn tone_curve_channel_points(recipe: &EditRecipe, channel: &str) -> Vec<CurvePoint> {
    let stored = match channel {
        "red" => recipe.curves.as_ref().and_then(|c| c.channels.red.clone()),
        "green" => recipe
            .curves
            .as_ref()
            .and_then(|c| c.channels.green.clone()),
        "blue" => recipe.curves.as_ref().and_then(|c| c.channels.blue.clone()),
        _ => recipe.curves.as_ref().map(|c| c.master.clone()),
    };
    stored
        .filter(|points| points.len() >= 2)
        .unwrap_or_else(crate::identity_curve_points)
}

/// Display mirror of `lumina-core::monotone_curve` (monotone cubic Hermite,
/// PCHIP) for one channel's control points. Kept in exact sync with the core
/// math; `display_sampler_matches_core_render` pins it against `render_frame`.
pub(crate) fn tone_curve_display_output(points: &[CurvePoint], x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if points.len() < 2 {
        return x;
    }
    let i = points
        .windows(2)
        .position(|w| x <= w[1].input)
        .unwrap_or(points.len() - 2);
    let (a, b) = (&points[i], &points[i + 1]);
    let h = b.input - a.input;
    let t = ((x - a.input) / h).clamp(0.0, 1.0);
    let slope = |j: usize| {
        if j == 0 {
            (points[1].output - points[0].output) / (points[1].input - points[0].input)
        } else if j + 1 == points.len() {
            (points[j].output - points[j - 1].output) / (points[j].input - points[j - 1].input)
        } else {
            (points[j + 1].output - points[j - 1].output)
                / (points[j + 1].input - points[j - 1].input)
        }
    };
    let (m0, m1) = (slope(i), slope(i + 1));
    let d = (b.output - a.output) / h;
    let (m0, m1) = if d == 0.0 {
        (0.0, 0.0)
    } else {
        let lo = 0.0f32.min(3.0 * d);
        let hi = 0.0f32.max(3.0 * d);
        (m0.clamp(lo, hi), m1.clamp(lo, hi))
    };
    let t2 = t * t;
    let t3 = t2 * t;
    ((2.0 * t3 - 3.0 * t2 + 1.0) * a.output
        + (t3 - 2.0 * t2 + t) * h * m0
        + (-2.0 * t3 + 3.0 * t2) * b.output
        + (t3 - t2) * h * m1)
        .clamp(0.0, 1.0)
}

/// Map a control point to the graph rectangle (`output` grows upwards).
pub(crate) fn curve_to_graph(rect: egui::Rect, input: f32, output: f32) -> egui::Pos2 {
    egui::pos2(
        rect.left() + input.clamp(0.0, 1.0) * rect.width(),
        rect.bottom() - output.clamp(0.0, 1.0) * rect.height(),
    )
}

/// Inverse of [`curve_to_graph`], clamped to the normative `0..=1` square.
pub(crate) fn graph_to_curve(rect: egui::Rect, pos: egui::Pos2) -> (f32, f32) {
    (
        ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
        ((rect.bottom() - pos.y) / rect.height()).clamp(0.0, 1.0),
    )
}

/// Index of the control point within `max_dist` screen points of `pos`.
pub(crate) fn nearest_point_index(
    points: &[CurvePoint],
    rect: egui::Rect,
    pos: egui::Pos2,
    max_dist: f32,
) -> Option<usize> {
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            (
                index,
                curve_to_graph(rect, point.input, point.output).distance(pos),
            )
        })
        .filter(|(_, dist)| *dist <= max_dist)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

/// Clamp a dragged interior point to the strictly-ascending input range between
/// its neighbours and to the normative `0..=1` output range.
pub(crate) fn clamped_point_move(
    points: &[CurvePoint],
    index: usize,
    input: f32,
    output: f32,
) -> (f32, f32) {
    let lower = points[index - 1].input + TONE_CURVE_MIN_GAP;
    let upper = points[index + 1].input - TONE_CURVE_MIN_GAP;
    // `f32::clamp` panics when the neighbours are closer than the minimum gap;
    // fall back to `lower` and let `move_curve_point` refuse loudly.
    let input = if lower <= upper {
        input.clamp(lower, upper)
    } else {
        lower
    };
    (input, output.clamp(0.0, 1.0))
}

/// Curve color: the selected channel's hue, so R/G/B edits are readable at a
/// glance (Master keeps the single theme accent).
fn curve_color(channel: &str) -> egui::Color32 {
    match channel {
        "red" => egui::Color32::from_rgb(0xd9, 0x5a, 0x5a),
        "green" => egui::Color32::from_rgb(0x5a, 0xc9, 0x6a),
        "blue" => egui::Color32::from_rgb(0x5a, 0x8a, 0xe0),
        _ => theme::ACCENT,
    }
}

/// Paint the graph chrome, the spline and the control points. `active`/`hover`
/// highlight the currently dragged / under-pointer point.
pub(crate) fn paint_tone_curve_graph(
    painter: egui::Painter,
    rect: egui::Rect,
    points: &[CurvePoint],
    channel: &str,
    active: Option<usize>,
    hover: Option<usize>,
) {
    let color = curve_color(channel);
    painter.rect_filled(rect, 3.0, theme::WORKING);
    painter.rect_stroke(
        rect,
        3.0,
        egui::Stroke::new(1.0, theme::SEPARATOR),
        egui::StrokeKind::Inside,
    );
    let grid = egui::Stroke::new(1.0, theme::SEPARATOR.gamma_multiply(0.55));
    for step in 1..4 {
        let f = step as f32 / 4.0;
        let x = rect.left() + f * rect.width();
        let y = rect.bottom() - f * rect.height();
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            grid,
        );
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            grid,
        );
    }
    painter.line_segment(
        [
            curve_to_graph(rect, 0.0, 0.0),
            curve_to_graph(rect, 1.0, 1.0),
        ],
        egui::Stroke::new(1.0, theme::SEPARATOR),
    );
    let poly: Vec<egui::Pos2> = (0..=TONE_CURVE_SAMPLES)
        .map(|i| {
            let x = i as f32 / TONE_CURVE_SAMPLES as f32;
            curve_to_graph(rect, x, tone_curve_display_output(points, x))
        })
        .collect();
    painter.add(egui::Shape::line(poly, egui::Stroke::new(1.75, color)));
    for (index, point) in points.iter().enumerate() {
        let center = curve_to_graph(rect, point.input, point.output);
        let highlighted = active == Some(index) || hover == Some(index);
        let radius = if highlighted {
            TONE_CURVE_POINT_RADIUS + 1.5
        } else {
            TONE_CURVE_POINT_RADIUS
        };
        let fill = if highlighted {
            egui::Color32::WHITE
        } else {
            color
        };
        painter.circle_filled(center, radius, fill);
        painter.circle_stroke(
            center,
            radius,
            egui::Stroke::new(1.0, egui::Color32::from_gray(0x20)),
        );
    }
}

impl LuminaApp {
    /// UX-LOOK-TONECURVE-18: draw and drive the interactive point-curve graph
    /// for `channel`. See the module docs for the gesture set. Endpoints are
    /// fixed; every edit persists through the existing `curves` recipe block.
    pub(crate) fn draw_tone_curve_graph(&mut self, ui: &mut egui::Ui, channel: &str) {
        let points = tone_curve_channel_points(&self.recipe, channel);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(TONE_CURVE_GRAPH_SIDE, TONE_CURVE_GRAPH_SIDE),
            egui::Sense::hover(),
        );
        let response = ui
            .interact(
                rect,
                tone_curve_graph_id(channel),
                egui::Sense::click_and_drag(),
            )
            .on_hover_cursor(egui::CursorIcon::Crosshair);
        let memory_id = tone_curve_graph_id(channel).with("drag");
        let active = ui.memory(|m| m.data.get_temp::<usize>(memory_id));
        let hover = response
            .hover_pos()
            .and_then(|pos| nearest_point_index(&points, rect, pos, TONE_CURVE_HIT_RADIUS));
        paint_tone_curve_graph(ui.painter_at(rect), rect, &points, channel, active, hover);
        self.tone_curve_graph_interaction(ui, channel, rect, &response, &points);
    }

    /// Gesture handling for [`Self::draw_tone_curve_graph`]. Kept separate so
    /// the draw path stays a pure paint plus one interaction call.
    fn tone_curve_graph_interaction(
        &mut self,
        ui: &mut egui::Ui,
        channel: &str,
        rect: egui::Rect,
        response: &egui::Response,
        points: &[CurvePoint],
    ) {
        let memory_id = tone_curve_graph_id(channel).with("drag");
        for gesture in curve_graph_gesture(ui, rect, response, points, memory_id) {
            match gesture {
                CurveGesture::Move {
                    index,
                    input,
                    output,
                } => self.move_curve_point(channel, index, f64::from(input), f64::from(output)),
                CurveGesture::Add { input, output } => {
                    self.add_curve_point(channel, f64::from(input), f64::from(output));
                }
                CurveGesture::Remove { index } => self.remove_curve_point(channel, index),
                CurveGesture::EndpointRefused => {
                    // Endpoints are mandatory: refuse loudly, never silently.
                    self.status = Str::ToneCurveInvalidPattern
                        .format_arg("endpoints (0,0)/(1,1) are mandatory");
                    warn!("tone_curve_graph: endpoints (0,0)/(1,1) are fixed");
                }
            }
        }
    }

    /// Move one interior control point to `input`/`output` (UX-LOOK-TONECURVE-18)
    /// and record the save commit. The input is clamped between the neighbouring
    /// points (the core requires strictly ascending inputs) and the output to
    /// `0..=1`; endpoints are refused loudly. Persists through the existing
    /// `curves` block (no schema change).
    pub(crate) fn move_curve_point(
        &mut self,
        channel: &str,
        index: usize,
        input: f64,
        output: f64,
    ) {
        if !matches!(channel, "master" | "red" | "green" | "blue") {
            warn!("move_curve_point: unknown channel {channel}");
            return;
        }
        let mut candidate = self.recipe.curves.clone().unwrap_or_else(|| Curves {
            version: 1,
            master: crate::identity_curve_points(),
            channels: CurveChannels::default(),
        });
        let slot: &mut Vec<CurvePoint> = match channel {
            "master" => &mut candidate.master,
            "red" => candidate
                .channels
                .red
                .get_or_insert_with(crate::identity_curve_points),
            "green" => candidate
                .channels
                .green
                .get_or_insert_with(crate::identity_curve_points),
            "blue" => candidate
                .channels
                .blue
                .get_or_insert_with(crate::identity_curve_points),
            _ => unreachable!(),
        };
        if index == 0 || index + 1 >= slot.len() {
            self.status =
                Str::ToneCurveInvalidPattern.format_arg("endpoints (0,0)/(1,1) are mandatory");
            warn!("move_curve_point: {channel}[{index}] is an endpoint");
            return;
        }
        let lower = slot[index - 1].input + TONE_CURVE_MIN_GAP;
        let upper = slot[index + 1].input - TONE_CURVE_MIN_GAP;
        if lower > upper {
            self.status = Str::ToneCurveInvalidPattern
                .format_arg("neighbouring points are closer than the minimum input gap");
            warn!("move_curve_point: {channel}[{index}] neighbours are too close to move");
            return;
        }
        let input = (input as f32).clamp(lower, upper);
        let output = (output as f32).clamp(0.0, 1.0);
        slot[index].input = input;
        slot[index].output = output;
        if let Some(reason) = Self::validate_curve_points(slot) {
            self.status = Str::ToneCurveInvalidPattern.format_arg(&reason);
            warn!("move_curve_point: {channel}[{index}] refused ({reason})");
            return;
        }
        self.recipe.curves = Some(candidate);
        info!("GUI interaction: curves.{channel} move point {index} to ({input:.3},{output:.3})");
        self.mark_recipe_dirty(
            &format!("curves.{channel}.points[{index}]"),
            f64::from(input),
        );
    }
}
