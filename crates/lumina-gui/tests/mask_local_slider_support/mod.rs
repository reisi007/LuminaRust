//! GUI-INT-MASKLOCAL-38: the **slider gestures** of the mask-local editors,
//! used only by `mask_local_editors.rs`.
//!
//! One cohesive unit: how a real `egui::Slider` is addressed unambiguously in
//! the Masking panel and how a multi-frame drag drives it. Every measured
//! property of the drag is documented at [`drag_slider`].
//!
//! Extracted from `mask_local_editors_support` (the shared core) because the
//! tone-curve test target does not drag sliders — keeping a helper in the core
//! that only one consumer calls would be dead code, and `Agents.md` forbids
//! closing a `-D warnings` gate with an `allow` attribute.

use super::mask_local_editors_support::{nodes, settle};
use eframe::egui::{Pos2, Rect};
use egui_kittest::Harness;
use lumina_gui::LuminaApp;

/// Horizontal inset keeping a 0.0/1.0 press strictly inside the slider.
const RAIL_INSET: f32 = 3.0;

/// Which vertical direction a looked-up widget lies in relative to its anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// The widget's row starts below the anchor's bottom edge.
    Below,
}

/// One node of `label`, addressed unambiguously inside the Masking panel.
///
/// `label` alone is often ambiguous: "Saturation" is the HSL, the vibrance
/// *and* the grading slider, "Hue" the HSL and the grading one, "Luminance" the
/// HSL, grading *and* noise-reduction one, and "Detail" is both the open
/// Develop section header, the mask-local block caption *and* the sharpening
/// detail slider. Rather than a fragile magic offset, the caller names the
/// `occurrence` explicitly (top-to-bottom, left-to-right paint order), and the
/// lookup fails loudly when the panel no longer has that many nodes.
pub fn occurrence(harness: &Harness<'_, LuminaApp>, label: &str, index: usize) -> Rect {
    let mut found = nodes(harness, label);
    assert!(
        !found.is_empty(),
        "label {label:?} is not painted in the Masking panel"
    );
    found.sort_by(|a, b| {
        a.min
            .y
            .total_cmp(&b.min.y)
            .then_with(|| a.min.x.total_cmp(&b.min.x))
    });
    assert!(
        found.len() > index,
        "expected at least {} occurrences of {label:?} in the Masking panel, got {found:?}",
        index + 1
    );
    found[index]
}

/// The draggable rail of the `caption` slider, relative to the `anchor`
/// occurrence and the given vertical direction.
///
/// # Why the caption rect, and not caption..readout
///
/// egui's `Slider` occupies exactly the caption's horizontal span; its numeric
/// readout is a **separate** `DragValue` widget to the right, *outside* the
/// slider's own response rect. A press between caption and readout therefore
/// misses the slider entirely and lands on nothing — measured: it drives the
/// value to the range minimum regardless of the x position. The caption rect is
/// inset by [`RAIL_INSET`] so a fraction of 0.0 / 1.0 still lands strictly
/// inside the widget instead of on its border stroke.
pub fn slider_rail(
    harness: &Harness<'_, LuminaApp>,
    anchor: (&str, usize),
    direction: Row,
    caption: &str,
) -> Rect {
    let anchor_rect = occurrence(harness, anchor.0, anchor.1);
    // Group the caption's nodes into rows (a slider contributes a caption and a
    // readout), then keep the first row on the requested side of the anchor.
    let mut rows: Vec<Vec<Rect>> = Vec::new();
    for rect in nodes(harness, caption) {
        match rows
            .iter_mut()
            .find(|row| (row[0].center().y - rect.center().y).abs() < 2.0)
        {
            Some(row) => row.push(rect),
            None => rows.push(vec![rect]),
        }
    }
    let mut candidates: Vec<&Vec<Rect>> = rows
        .iter()
        .filter(|row| match direction {
            Row::Below => row[0].min.y >= anchor_rect.max.y,
        })
        .collect();
    candidates.sort_by(|a, b| a[0].min.y.total_cmp(&b[0].min.y));
    let row = candidates.first().copied().unwrap_or_else(|| {
        panic!("no {caption:?} slider row {direction:?} the {anchor:?} anchor in the Masking panel")
    });
    assert_eq!(
        row.len(),
        2,
        "the {caption:?} slider must contribute a caption and a numeric readout, got {row:?}"
    );
    let (caption_rect, readout) = (row[0], row[1]);
    assert!(
        caption_rect.min.x < readout.min.x,
        "the {caption:?} caption must sit left of its readout, got {caption_rect:?} / {readout:?}"
    );
    let min = caption_rect.min.x + RAIL_INSET;
    let max = (caption_rect.max.x - RAIL_INSET).max(min);
    Rect::from_min_max(
        Pos2::new(min, caption_rect.center().y),
        Pos2::new(max, caption_rect.center().y),
    )
}

/// Drag a real `egui::Slider` from its current value to `fraction` of `rail`.
///
/// The press, the move and the release sit in **three separate frames**, and
/// that is the whole requirement. Measured, not assumed:
///
/// * a same-frame press+release never moves an egui slider at all — the widget
///   needs a `dragged()` event, which only a *frame* between press and release
///   produces; and
/// * the value change has to happen while the pointer is still down, so the
///   debounced save arms (`mark_recipe_dirty` → `pending_slider_commit`) and
///   `schedule_render` refreshes `last_edit_time` for that drag.
///
/// # Why there are no intermediate pointer positions
///
/// An earlier version walked `DRAG_STEPS` intermediate positions and documented
/// that as *required*: the claim was that a drag which changes its value in a
/// single frame strands the debounced save (`SIDECAR-SAVE-STRAND-39`). That
/// justification is **measured to be false** — every interaction test of this
/// feature passes unchanged with 0, 1, 2 and 3 intermediate positions. What
/// matters is the frame separation, not the number of steps.
///
/// The workaround is therefore gone on purpose. `SIDECAR-SAVE-STRAND-39`
/// requires that its reproduction must not be explained by workaround logic
/// ("sonst ist die Abdeckung vakuos"), and a helper that drags through five
/// positions to dodge a bug is exactly that.
pub fn drag_slider(harness: &mut Harness<'_, LuminaApp>, rail: Rect, fraction: f32) {
    let y = rail.center().y;
    let at = |f: f32| Pos2::new(rail.min.x + rail.width() * f.clamp(0.0, 1.0), y);
    // Press where the handle already is (the middle of the rail is the neutral
    // value of every slider these editors offer), move to the target, release —
    // one frame each.
    let from = 0.5;
    let to = fraction.clamp(0.0, 1.0);
    harness.hover_at(at(from));
    harness.drag_at(at(from));
    harness.step();
    harness.hover_at(at(to));
    harness.step();
    harness.drop_at(at(to));
    settle(harness, 2);
}
