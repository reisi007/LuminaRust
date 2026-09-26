//! GUI-INT-MASKLOCAL-38: **directional label lookups** for the Masking panel —
//! how a label that the panel repeats is addressed without a magic offset.
//!
//! Used by `mask_local_editors_wiring.rs` (the curve block's channel "Red" and
//! its per-channel "Reset") and by `mask_local_color_controls.rs` (the
//! per-band and per-range "Reset"). It is a separate module from
//! `mask_local_curve_graph_support` because its reason for existing is *not*
//! the curve: the Masking panel repeats short labels across unrelated blocks —
//! "Red" is an HSL band **and** a curve channel, "Saturation" is the HSL, the
//! vibrance **and** the grading slider, "Hue" is the HSL and the grading one,
//! "Luminance" is HSL, grading **and** noise reduction, "Detail" is the Develop
//! section header, the mask-local block caption **and** the sharpening slider,
//! and "Reset" appears half a dozen times. A bare "first match" query would
//! silently address the wrong widget; these helpers anchor on a panel-unique
//! label and take the nearest match in one direction.

use super::mask_local_editors_support::nodes;
use eframe::egui::Rect;
use egui_kittest::Harness;
use lumina_gui::LuminaApp;

/// The rect of the single occurrence of `label`, or `None`.
///
/// A slider contributes two accesskit nodes with the same label (the caption
/// and its numeric readout), so "present" must not mean "unique" here. For a
/// caption *and* its rail together, see
/// `mask_local_slider_support::slider_rail`.
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
/// Ties are broken left-to-right, so a horizontal row (`Shadows`/`Midtones`/
/// `Highlights`) resolves to its first button. `anchor` must be a panel-unique
/// label; the lookup fails loudly when the anchor or the target is absent, so a
/// renamed or moved control breaks the test instead of silently moving the
/// gesture somewhere else.
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
