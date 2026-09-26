//! SIDECAR-SAVE-STRAND-39: the **debounced slider save** of a real headless
//! interaction, plus the Develop-panel addressing its tests need.
//!
//! One cohesive unit: how the global Develop slider rows are located, how a
//! pointer gesture is driven on a real `egui` slider, and how the persisted
//! recipe is read back from the sidecar the debounced commit actually wrote.
//!
//! The gestures here are **not** workarounds. `SIDECAR-SAVE-STRAND-39` forbids
//! explaining a reproduction with drag mechanics (the removed `DRAG_STEPS`
//! intermediate positions in `mask_local_slider_support::drag_slider` were
//! exactly that, and were removed because the premise was measured false).
//! Every gesture below is the shortest real pointer sequence that reaches the
//! state under test, and each one documents which scheduler state it produces:
//!
//! * [`press_hold_release`] — press on the rail, hold `hold` frames **without
//!   moving**, release. The press is the jump-to-click value change; the hold
//!   frame is what lets `schedule_render`'s pointer-down branch run once, which
//!   is what starts the 150 ms edit clock. Without the hold frame the release
//!   frame still sees `last_edit_time == 0.0` and commits *immediately*.
//! * [`drag_then_hold`] — press on the rail, move to the target in one frame,
//!   hold, release: a real multi-frame drag whose value change happens while
//!   the pointer is down.
//!
//! Neither walks intermediate positions, and neither waits for a "helpful"
//! number of frames: the hold count is a parameter and the tests state the
//! number they need.

use super::mask_local_editors_support::{nodes, open_masking_panel, settle, smoke_png};
use eframe::egui::{Pos2, Rect};
use egui_kittest::Harness;
use lumina_gui::{LuminaApp, SECTION_BASIC, SECTION_MASKING};

/// Frames to let a section open/close settle after [`open_develop_basic`].
const LAYOUT_FRAMES: usize = 6;

/// The `lr_slider` label column is fixed at this width and the row puts one
/// `ROW_SPACING` gap after it plus egui's own item spacing, so the track always
/// starts this far right of the caption node (`slider.rs`).
const ROW_SPACING: f32 = 8.0;
/// The widest legal `lr_slider` track; the real one is
/// `(row width - LABEL_WIDTH - VALUE_WIDTH - 3*ROW_SPACING)` clamped to
/// `TRACK_MIN_W..=TRACK_MAX_W`.
const TRACK_MAX_W: f32 = 240.0;

/// Harness with the Develop **Basic** section open and the Masking section
/// closed, so the global `Exposure` row is painted and clickable.
///
/// The Masking section is closed on purpose: with both open, the panel content
/// grows past its clip rect and the mask-local rows stop being interactive,
/// which would silently swallow a gesture. `open_masking_panel` is the shared
/// constructor (fixture file, decode drain, one selected mask); this only
/// re-arranges the sections afterwards.
pub fn open_develop_basic(dir: &tempfile::TempDir) -> Harness<'static, LuminaApp> {
    let mut harness = open_masking_panel(dir);
    harness.state_mut().set_section_open(SECTION_MASKING, false);
    harness.state_mut().set_section_open(SECTION_BASIC, true);
    settle(&mut harness, LAYOUT_FRAMES);
    harness
}

/// The rail of a global Develop adjustment row (`lr_slider`), derived from its
/// caption node.
///
/// The returned rect is the *widest* legal track, so a fraction of it is always
/// a legal point of a *narrower* real track: fraction `f` addresses the real
/// track at `min(f * TRACK_MAX_W, TRACK_W)` — never left of its start, never
/// right of its end. Callers therefore address the track without knowing the
/// panel width, and every test asserts that the gesture really changed the
/// value, so a geometry change fails loudly instead of passing vacuously.
pub fn global_slider_rail(harness: &Harness<'_, LuminaApp>, caption: &str) -> Rect {
    let mut found = nodes(harness, caption);
    assert!(
        !found.is_empty(),
        "the Develop panel must paint a {caption:?} adjustment row"
    );
    found.sort_by(|a, b| {
        a.min
            .y
            .total_cmp(&b.min.y)
            .then_with(|| a.min.x.total_cmp(&b.min.x))
    });
    let label = found[0];
    let left = label.max.x + 2.0 * ROW_SPACING;
    Rect::from_min_max(
        Pos2::new(left, label.center().y),
        Pos2::new(left + TRACK_MAX_W, label.center().y),
    )
}

/// Press on `rail` at `fraction`, hold `hold` frames without moving, release.
///
/// The jump-to-click value change happens on the press frame; the hold frames
/// are where `schedule_render`'s pointer-down branch runs and starts the 150 ms
/// edit clock (`last_edit_time`). See the module docs.
pub fn press_hold_release(
    harness: &mut Harness<'_, LuminaApp>,
    rail: Rect,
    fraction: f32,
    hold: usize,
) {
    let at = point_at(rail, fraction);
    harness.hover_at(at);
    harness.step();
    harness.drag_at(at);
    harness.step();
    for _ in 0..hold {
        harness.step();
    }
    harness.drop_at(at);
    harness.step();
}

/// A real multi-frame drag: press at `from`, move to `to` in the next frame,
/// hold `hold` frames there, release at `to`.
pub fn drag_then_hold(
    harness: &mut Harness<'_, LuminaApp>,
    rail: Rect,
    from: f32,
    to: f32,
    hold: usize,
) {
    let start = point_at(rail, from);
    let end = point_at(rail, to);
    harness.hover_at(start);
    harness.step();
    harness.drag_at(start);
    harness.step();
    harness.hover_at(end);
    harness.step();
    for _ in 0..hold {
        harness.step();
    }
    harness.drop_at(end);
    harness.step();
}

fn point_at(rail: Rect, fraction: f32) -> Pos2 {
    Pos2::new(
        rail.min.x + rail.width() * fraction.clamp(0.0, 1.0),
        rail.center().y,
    )
}

/// The persisted `exposure` of the active virtual copy, or `None` when the
/// sidecar carries no such key. Panics when the sidecar is missing or
/// unreadable — a debounced save that never wrote is a failure, not `None`.
pub fn persisted_exposure(dir: &tempfile::TempDir) -> Option<f64> {
    load_fixture_sidecar(dir).virtual_copies[0]
        .recipe
        .adjustments
        .get("exposure")
        .copied()
}

fn load_fixture_sidecar(dir: &tempfile::TempDir) -> lumina_sidecar::SidecarDocument {
    let path = lumina_sidecar::sidecar_path_for(&smoke_png(dir));
    lumina_sidecar::load_sidecar(&path).unwrap_or_else(|error| {
        panic!(
            "the debounced save must have written {}: {error}",
            path.display()
        )
    })
}
