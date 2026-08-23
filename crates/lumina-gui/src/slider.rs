//! Reusable Lightroom-style slider and its display scaling.
//!
//! The slider is a horizontal row: label (left), slider (centre), value
//! (right).  It encodes the F-100 interaction rules:
//!
//! * double-click on the **label** resets *only this* control to its documented
//!   default (never the whole recipe);
//! * double-click on the **value** does nothing — it must never reset the recipe;
//! * Alt/Option-Scroll over the row fine-adjusts (smaller step) while normal
//!   drag/scroll stays coarse.
//!
//! Internal adjustment domains stay normative (`-1..=1`, Exposure `-10..=10`,
//! Kelvin `1500..=12000`); the *displayed* value is scaled per [`DisplayScale`]
//! so that `-1..=1` reads as `-100..+100` Lightroom-style.  Storage and pipeline
//! validation always use the normative value.
//!
//! The control is drawn entirely by hand (track + handle) so the visuals can be
//! tuned exhaustively: a 4px rounded track whose contrast rises on hover and
//! turns accent while active, a 14px white handle with a 1px border, soft
//! shadow and a 24px hit area that scales to 1.1 on hover, a semibold label,
//! a right-aligned monospace value that is editable inline (via [`egui::DragValue`]),
//! tooltips, focus/keyboard accessibility and a grayed disabled state.

use eframe::egui;
use crate::theme::{ACCENT, DISABLED, HANDLE_BORDER, TRACK_HOVER, TRACK_IDLE};

/// How a normative value maps to its on-screen representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayScale {
    /// Identity: show the stored value directly (e.g. Exposure EV, Kelvin).
    Identity,
    /// Multiply by 100 for display: `-1..=1` is shown as `-100..+100`.
    Percent,
}

/// Map a normative value to its displayed value.
pub fn to_display(value: f64, scale: DisplayScale) -> f64 {
    match scale {
        DisplayScale::Identity => value,
        DisplayScale::Percent => value * 100.0,
    }
}

/// Map a displayed value back to the normative value.
pub fn from_display(display: f64, scale: DisplayScale) -> f64 {
    match scale {
        DisplayScale::Identity => display,
        DisplayScale::Percent => display / 100.0,
    }
}

/// What the user did with a [`lr_slider`] row this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderAction {
    /// The slider value changed; the caller should persist `value`.
    Changed,
    /// The label was double-clicked; the caller should reset *only this* control.
    ResetRequested,
    /// Nothing relevant happened.
    Nothing,
}

/// Static parameters of a slider row.
#[derive(Debug, Clone, Copy)]
pub struct SliderSpec {
    /// Normative (stored) value range as an inclusive `(min, max)` pair.
    pub range: (f64, f64),
    /// Documented default the label double-click restores.
    pub default: f64,
    /// Display scaling for the value/range.
    pub scale: DisplayScale,
    /// Coarse scroll/drag step (no Alt held).
    pub coarse_step: f64,
    /// Fine scroll step when Alt/Option is held.
    pub fine_step: f64,
    /// Optional unit suffix shown next to the value (e.g. `" K"` for Kelvin).
    ///
    /// Default is `None`.  Set with [`SliderSpec::unit`].
    pub unit: Option<&'static str>,
}

impl SliderSpec {
    /// Attach an optional unit suffix (`" K"`, `" %"`, …) to the value read-out.
    pub fn unit(mut self, unit: &'static str) -> Self {
        self.unit = Some(unit);
        self
    }
}

/// A scalar adjustment value.  Both `f32` (most stored recipe fields) and `f64`
/// (the flat `adjustments` map) are supported so a single slider implementation
/// drives every Lightroom-style control.
pub trait Scalar: Copy {
    fn to_f64(self) -> f64;
    fn from_f64(v: f64) -> Self;
}

impl Scalar for f64 {
    fn to_f64(self) -> f64 {
        self
    }
    fn from_f64(v: f64) -> Self {
        v
    }
}

impl Scalar for f32 {
    fn to_f64(self) -> f64 {
        self as f64
    }
    fn from_f64(v: f64) -> Self {
        v as f32
    }
}

/// Output of the hand-drawn track widget for one frame.
struct TrackOutcome {
    /// New display-space value (already in `[display_min, display_max]`).
    value: f64,
    /// Whether the value changed because of this widget this frame.
    changed: bool,
}

/// Geometry constants for the hand-drawn slider.
const TRACK_THICKNESS: f32 = 4.0;
/// Total interactive height of the row's slider segment (the hit area).
const TRACK_HIT_HEIGHT: f32 = 24.0;
/// Diameter of the handle (px).
const HANDLE_DIAMETER: f32 = 14.0;
/// Hover scale applied to the handle radius for a subtle "lift".
const HANDLE_HOVER_SCALE: f32 = 1.1;
/// Fixed width (px) reserved for the left-hand label so every row aligns and
/// the slider width can be computed deterministically.
const LABEL_WIDTH: f32 = 110.0;
/// Fixed width (px) reserved for the right-aligned value read-out.
const VALUE_WIDTH: f32 = 44.0;
/// Horizontal gap (px) between the label, track and value boxes.
const ROW_SPACING: f32 = 8.0;
/// Minimum / maximum width (px) the slider track may occupy.  The cap keeps a
/// wide (resizable) panel from stretching the track across the whole window.
const SLIDER_MIN_W: f32 = 40.0;
const SLIDER_MAX_W: f32 = 240.0;

/// Draw the slider track + handle and resolve pointer / keyboard interaction.
///
/// `display_value` is the current display-space value.  Returns the possibly
/// updated value and whether it changed.  `display_step` is the coarse step in
/// display units (used for keyboard nudging); `alt` enables the fine step.
/// `slider_w` is the already-constrained track width (see [`lr_slider`]) so the
/// widget never expands to fill its parent.
fn draw_track(
    ui: &mut egui::Ui,
    id: egui::Id,
    display_min: f64,
    display_max: f64,
    display_value: f64,
    display_step: f64,
    alt: bool,
    enabled: bool,
    slider_w: f32,
) -> TrackOutcome {
    let span = display_max - display_min;
    let desired = egui::vec2(slider_w.clamp(SLIDER_MIN_W, SLIDER_MAX_W), TRACK_HIT_HEIGHT);
    let (_, rect) = ui.allocate_space(desired);
    let mut response = ui.interact(rect, id, egui::Sense::click_and_drag());

    // Clicking the row focuses it so arrow keys can nudge the value.
    if response.clicked() && enabled {
        response.request_focus();
    }
    let focused = response.has_focus() && enabled;

    let pointer = ui.ctx().input(|i| i.pointer.interact_pos());
    let hover = enabled && (response.hovered() || pointer.is_some_and(|p| rect.contains(p)));

    let mut value = display_value;
    let mut changed = false;

    if enabled {
        let frac_at = |p: egui::Pos2| -> f64 {
            if span <= 0.0 {
                0.0
            } else {
                let left = rect.left() as f64;
                let width = rect.width() as f64;
                ((p.x as f64 - left) / width).clamp(0.0, 1.0)
            }
        };

        // Pointer: jump on click, follow on drag (even slightly outside the band).
        if let Some(pos) = pointer {
            if response.dragged() || (response.clicked() && rect.contains(pos)) {
                value = display_min + frac_at(pos) * span;
                changed = true;
            }
        }

        // Keyboard: arrow keys nudge by the (fine while Alt held) step.
        if focused {
            let step = if alt { display_step / 10.0 } else { display_step };
            let mut nudged = false;
            for (key, dir) in [
                (egui::Key::ArrowRight, 1.0),
                (egui::Key::ArrowUp, 1.0),
                (egui::Key::ArrowLeft, -1.0),
                (egui::Key::ArrowDown, -1.0),
            ] {
                if ui
                    .ctx()
                    .input_mut(|i| i.consume_key(egui::Modifiers::NONE, key))
                {
                    value = (value + dir * step).clamp(display_min, display_max);
                    nudged = true;
                }
            }
            changed |= nudged;
        }
    } else {
        value = display_value;
    }

    // ----- Painting -----
    let cy = rect.center().y;
    let track_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left(), cy - TRACK_THICKNESS / 2.0_f32),
        egui::pos2(rect.right(), cy + TRACK_THICKNESS / 2.0_f32),
    );
    let rounding = egui::CornerRadius::from(TRACK_THICKNESS as u8 / 2);

    let frac = if span <= 0.0 {
        0.0
    } else {
        ((value - display_min) / span).clamp(0.0, 1.0)
    };

    // Track background colour rises in contrast with interaction.
    let (bg, fill) = if !enabled {
        (DISABLED, DISABLED)
    } else if response.dragged() {
        (ACCENT, ACCENT)
    } else if hover {
        (TRACK_HOVER, ACCENT)
    } else {
        (TRACK_IDLE, ACCENT)
    };

    let painter = ui.painter();
    // Unfilled track (rounded ends).
    painter.rect_filled(track_rect, rounding, bg);
    // Filled portion up to the handle.
    if enabled && frac > 0.0 {
        let fill_right = (track_rect.left() as f64 + frac * track_rect.width() as f64) as f32;
        let fill_rect = egui::Rect::from_min_max(
            track_rect.min,
            egui::pos2(fill_right, track_rect.max.y),
        );
        if fill_rect.width() > 0.5_f32 {
            painter.rect_filled(fill_rect, rounding, fill);
        }
    }

    // Focus ring.
    if focused {
        painter.rect_stroke(
            track_rect.expand(3.0_f32),
            rounding,
            egui::Stroke::new(1.0_f32, ACCENT),
            egui::StrokeKind::Middle,
        );
    }

    // ---- Handle ----
    let handle_radius =
        (HANDLE_DIAMETER / 2.0_f32) * if hover { HANDLE_HOVER_SCALE } else { 1.0_f32 };
    let handle_x = if span <= 0.0 {
        (track_rect.left() as f64 + handle_radius as f64) as f32
    } else {
        let left = track_rect.left() as f64;
        let width = track_rect.width() as f64;
        (left + handle_radius as f64 + frac * (width - 2.0_f64 * handle_radius as f64)) as f32
    };
    let handle_center = egui::pos2(handle_x, cy);

    if enabled {
        // Soft drop shadow.
        painter.circle_filled(
            handle_center + egui::vec2(0.0_f32, 1.0_f32),
            handle_radius + 1.0_f32,
            egui::Color32::from_black_alpha(70),
        );
        painter.circle_filled(handle_center, handle_radius, egui::Color32::WHITE);
        painter.circle_stroke(
            handle_center,
            handle_radius,
            egui::Stroke::new(1.0_f32, HANDLE_BORDER),
        );
    } else {
        painter.circle_filled(handle_center, handle_radius, egui::Color32::from_gray(0x66));
        painter.circle_stroke(
            handle_center,
            handle_radius,
            egui::Stroke::new(1.0_f32, DISABLED),
        );
    }

    TrackOutcome { value, changed }
}

/// Draw one Lightroom-style adjustment row.
///
/// `value` is the normative stored value (any [`Scalar`]); it is mutated in
/// place on a slider change (and on a label double-click reset).  The caller is
/// responsible for persisting `value` into the recipe based on the returned
/// [`SliderAction`].
///
/// The row is grayed and non-interactive when the surrounding UI is disabled
/// (e.g. no image is loaded).
pub fn lr_slider<T: Scalar>(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut T,
    spec: SliderSpec,
) -> SliderAction {
    let display_min = to_display(spec.range.0, spec.scale);
    let display_max = to_display(spec.range.1, spec.scale);
    let mods = ui.ctx().input(|i| i.modifiers);
    let step = if mods.alt {
        spec.fine_step
    } else {
        spec.coarse_step
    };
    let display_step = to_display(step, spec.scale);
    // Show enough decimals for the smallest relevant step.
    let decimals = if display_step >= 1.0 {
        0
    } else if display_step >= 0.1 {
        1
    } else {
        2
    };
    let enabled = ui.is_enabled();
    let id = ui.auto_id_with(label);

    let mut action = SliderAction::Nothing;
    ui.horizontal(|ui| {
        ui.set_min_height(28.0_f32);
        // Capture the full row width *before* any child is placed so the track
        // width can be computed deterministically (egui's `available_width`
        // otherwise shrinks as the cursor advances past the label).
        let row_width = ui.available_width();
        ui.add_space(ROW_SPACING);

        // ----- Label (left, semibold 12px, fixed width so rows align) -----
        let label_response = ui.add_sized(
            egui::vec2(LABEL_WIDTH, 16.0_f32),
            egui::Label::new(egui::RichText::new(label).strong().size(12.0))
                .truncate()
                .sense(egui::Sense::click()),
        );
        let label_double_clicked = label_response.double_clicked();
        label_response.on_hover_text("Double-click to reset to default");

        ui.add_space(ROW_SPACING);

        // ----- Slider track (centre, width = remaining space, capped) -----
        // `row_width` is the full row width; subtract the fixed label, value and
        // the gaps so the track fits exactly and never grows to fill the window.
        // The clamp keeps it sane in very narrow/wide (resizable) panels.
        let slider_w = (row_width - LABEL_WIDTH - VALUE_WIDTH - 3.0_f32 * ROW_SPACING)
            .clamp(SLIDER_MIN_W, SLIDER_MAX_W);
        let mut display_value =
            to_display(value.to_f64(), spec.scale).clamp(display_min, display_max);
        let track = draw_track(
            ui,
            id,
            display_min,
            display_max,
            display_value,
            display_step,
            mods.alt,
            enabled,
            slider_w,
        );
        if track.changed {
            display_value = track.value;
            *value = T::from_f64(
                from_display(display_value, spec.scale).clamp(spec.range.0, spec.range.1),
            );
            action = SliderAction::Changed;
        }

        ui.add_space(ROW_SPACING);

        // ----- Value (right, mono 11px, editable, fixed width) -----
        let mut edit_value = display_value;
        let prev_text_style = ui.style().drag_value_text_style.clone();
        ui.style_mut().drag_value_text_style = egui::TextStyle::Monospace;
        let mut dv = egui::DragValue::new(&mut edit_value)
            .speed(display_step.max(1e-4))
            .range(display_min..=display_max)
            .min_decimals(0)
            .max_decimals(decimals);
        if let Some(unit) = spec.unit {
            dv = dv.suffix(unit);
        }
        let value_response = ui
            .allocate_ui_with_layout(
                egui::vec2(VALUE_WIDTH, 18.0_f32),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| ui.add_enabled(enabled, dv),
            )
            .inner;
        let value_changed = value_response.changed();
        value_response.on_hover_text("Click and drag, or type, to edit");
        ui.style_mut().drag_value_text_style = prev_text_style;
        if value_changed {
            let clamped = edit_value.clamp(display_min, display_max);
            *value = T::from_f64(
                from_display(clamped, spec.scale).clamp(spec.range.0, spec.range.1),
            );
            action = SliderAction::Changed;
        }

        // ----- Label double-click resets only this control -----
        if label_double_clicked {
            *value = T::from_f64(spec.default);
            action = SliderAction::ResetRequested;
        }
    });
    action
}

/// Convenience constructor for a `-1..=1` adjustment shown as `-100..+100`.
pub fn percent_spec(range: std::ops::RangeInclusive<f64>, default: f64) -> SliderSpec {
    SliderSpec {
        range: (*range.start(), *range.end()),
        default,
        scale: DisplayScale::Percent,
        coarse_step: 0.01,
        fine_step: 0.001,
        unit: None,
    }
}

/// Convenience constructor for an identity-domain adjustment (Exposure / Kelvin).
pub fn identity_spec(range: std::ops::RangeInclusive<f64>, default: f64, step: f64) -> SliderSpec {
    SliderSpec {
        range: (*range.start(), *range.end()),
        default,
        scale: DisplayScale::Identity,
        coarse_step: step,
        fine_step: step / 10.0,
        unit: None,
    }
}
