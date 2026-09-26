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

/// Number of intermediate pointer positions a [`drag_slider`] walks through.
pub const DRAG_STEPS: usize = 3;

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
/// Three measured properties, all of them load-bearing:
///
/// 1. **Multi-frame.** A slider is a drag widget, not a click widget: a
///    same-frame press+release never moves it. The press and the release
///    therefore sit in separate frames, so each frame runs `ui.interact` and
///    records its own event.
/// 2. **Distinct intermediate positions.** The pointer walks from the rail
///    position the handle currently sits at through [`DRAG_STEPS`] further
///    positions to the target, one frame each. This is what a real drag looks
///    like, and it is also what the app's save path requires: the debounced
///    sidecar commit is armed by `mark_recipe_dirty` on *each* value change and
///    the drag branch of the scheduler sets `last_edit_time` per drag frame. A
///    gesture that parks the pointer and then releases changes the value in a
///    single frame; `render_schedule.rs` then takes the `!pointer_down` branch
///    with `last_edit_time` still at its pre-drag value, and the armed save is
///    only flushed by a *later*, unrelated edit. Measured, not assumed: with a
///    parked pointer the value lands in memory but never reaches the sidecar.
/// 3. **A final position-changing frame before the release**, so the last
///    `mark_recipe_dirty` happens while the pointer is still down and the
///    release frame takes the debounce branch.
pub fn drag_slider(harness: &mut Harness<'_, LuminaApp>, rail: Rect, fraction: f32) {
    let y = rail.center().y;
    let at = |f: f32| Pos2::new(rail.min.x + rail.width() * f.clamp(0.0, 1.0), y);
    // Press where the handle already is (the middle of the rail is the neutral
    // value of every slider these editors offer), then walk to the target.
    let from = 0.5;
    let to = fraction.clamp(0.0, 1.0);
    harness.hover_at(at(from));
    harness.drag_at(at(from));
    harness.step();
    for step in 1..=DRAG_STEPS {
        let position = from + (to - from) * (step as f32) / (DRAG_STEPS as f32 + 1.0);
        harness.hover_at(at(position));
        harness.step();
    }
    harness.hover_at(at(to));
    harness.step();
    harness.drop_at(at(to));
    settle(harness, 2);
}
