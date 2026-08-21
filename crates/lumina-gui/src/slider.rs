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

use eframe::egui;

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

/// Draw one Lightroom-style adjustment row.
///
/// `value` is the normative stored value (any [`Scalar`]); it is mutated in
/// place on a slider change (and on a label double-click reset).  The caller is
/// responsible for persisting `value` into the recipe based on the returned
/// [`SliderAction`].
pub fn lr_slider<T: Scalar>(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut T,
    spec: SliderSpec,
) -> SliderAction {
    let display_min = to_display(spec.range.0, spec.scale);
    let display_max = to_display(spec.range.1, spec.scale);
    let alt = ui.ctx().input(|i| i.modifiers.alt);
    let step = if alt {
        spec.fine_step
    } else {
        spec.coarse_step
    };

    let mut action = SliderAction::Nothing;
    ui.horizontal(|ui| {
        let label_response = ui.add(
            egui::Label::new(label)
                .truncate()
                .sense(egui::Sense::click()),
        );

        let mut display_value = to_display(value.to_f64(), spec.scale);
        let slider_response = ui.add(
            egui::Slider::new(&mut display_value, display_min..=display_max)
                .step_by(step)
                .show_value(false),
        );

        // The value read-out is interactive (so it feels alive) but a
        // double-click on it is explicitly a no-op: it must never reset.
        ui.add(
            egui::Label::new(format!("{display_value:.0}"))
                .sense(egui::Sense::click())
                .truncate(),
        );

        if label_response.double_clicked() {
            *value = T::from_f64(spec.default);
            action = SliderAction::ResetRequested;
            return;
        }
        if slider_response.changed() {
            let normative =
                from_display(display_value, spec.scale).clamp(spec.range.0, spec.range.1);
            *value = T::from_f64(normative);
            action = SliderAction::Changed;
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
    }
}
