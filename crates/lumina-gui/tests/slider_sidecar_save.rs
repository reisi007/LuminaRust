//! SIDECAR-SAVE-STRAND-39: the **debounced sidecar save** of a real slider
//! interaction, proven end to end in a headless `egui` harness.
//!
//! # The contract under test
//!
//! `GUI-SLIDER-SAVE-1`: a recipe edit made through a slider arms a debounced
//! commit (`pending_slider_commit` / `pending_history_step`) and the app's
//! per-frame scheduler (`render_schedule::schedule_render`) is the only place
//! that runs it — in its `!pointer_down` branch, once the 150 ms idle debounce
//! has elapsed. Zoom/pan (pure view state) arm no token and therefore never
//! write a sidecar.
//!
//! # What the task reported, and what is actually true
//!
//! The task entry claimed a *single-frame drag* loses the save because the
//! `!pointer_down` branch runs with `last_edit_time` from before the drag.
//! **Measured: that cannot happen.** `full_render_debounce_remaining` returns
//! `None` (commit now) for a stale/zero `last_edit_time`, so the debounce can
//! only *delay* a commit, never cancel it; and `pending_full_render` is
//! re-armed by the slider itself on every pointer-down frame of a drag
//! (`slider.rs`: `changed` is set unconditionally while `dragged()`), so the
//! release frame always finds the pending work. The two tests below that drag
//! a real rail pin exactly that.
//!
//! What *is* reachable — and what this target reproduces — is a **cancelled**
//! commit: a full render that does *not* go through `commit_pending_slider_save`
//! clears `pending_full_render`, and that flag is the gate of the scheduler's
//! only commit path. The Develop footer's `Render / Apply` button is such a
//! render (`render_action` → `LuminaApp::render`): clicking it inside the
//! 150 ms window leaves the armed token with no reachable commit path, and the
//! edit stays in memory while the sidecar keeps the old value — silently, with
//! no error and no badge.
//!
//! # Non-vacuity
//!
//! Every test first asserts that the gesture really changed the in-memory value
//! (a test that drags nothing would pass on any sidecar). Two production
//! mutations were measured against this target:
//!
//! | mutation | result |
//! |---|---|
//! | the re-arm block in `render_schedule.rs` no longer sets `pending_full_render` | **red** — only `a_render_apply_click_inside_the_debounce_window_does_not_strand_the_save`, with `memory 0 vs disk 0.6000000238418579`; the other three stay green |
//! | the `!pointer_down` branch no longer calls `commit_pending_slider_save` | **red** — all four (the three file assertions plus the "a valid render exists" premise, which no full render ever satisfies any more) |
//!
//! The first row is what shows the tests are not all testing one thing: the drag
//! save and the cancellation save are separate statements, and only the fix
//! separates them.
//!
//! # Scope
//!
//! Headless only: `egui_kittest::Harness` + the real `eframe::App::ui` +
//! a `tempfile::tempdir`. No window, no GPU, no human.

mod mask_local_editors_support;
mod mask_local_slider_support;
mod slider_sidecar_save_support;

use mask_local_editors_support::{
    click, close, only_rect, open_masking_panel, persisted_local_recipe, settle_persisted,
};
use mask_local_slider_support::{drag_slider, slider_rail, Row};
use slider_sidecar_save_support::{
    drag_then_hold, global_slider_rail, open_develop_basic, persisted_exposure, press_hold_release,
};

/// Frames the pointer stays down without moving after a value change, so the
/// scheduler's pointer-down branch runs once and starts the 150 ms edit clock.
/// One frame is the minimum; the tests do not tune it higher.
const HOLD_FRAMES: usize = 1;

/// The persisted local `dehaze`, treating an **absent** presence block as `0.0`.
///
/// The serializer drops a neutral block, so a gesture back to `0.0` lands as "no
/// presence edit at all". That is still proof that a save ran: these tests seed
/// a non-zero `dehaze` first, so only a *later* write can remove the block.
fn persisted_dehaze(dir: &tempfile::TempDir) -> f64 {
    persisted_local_recipe(dir)
        .presence
        .map_or(0.0, |block| f64::from(block.dehaze))
}

/// A multi-frame drag of the **global** Develop `Exposure` row that begins while
/// a valid render is already in place reaches the sidecar.
///
/// This is the state the task named as untested: every other interaction test
/// starts from a cold `LuminaApp`, so `render_key` is `None` and the scheduler's
/// pointer-down branch is always taken. Here `render_key` is `Some(..)` when the
/// drag begins — `render_hash_tooltip()` is the public readout of that
/// (`Some` iff `render_key.is_some()`) — and the save still lands.
#[test]
fn a_global_slider_drag_from_a_valid_render_reaches_the_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = open_develop_basic(&dir);
    assert!(
        harness.state().render_hash_tooltip().is_some(),
        "a valid render must exist when the drag begins, otherwise the \
         `render_key.is_none()` branch condition is never exercised"
    );

    let rail = global_slider_rail(&harness, "Exposure");
    drag_then_hold(&mut harness, rail, 0.5, 0.1, HOLD_FRAMES);

    let memory = harness
        .state()
        .recipe()
        .adjustments
        .get("exposure")
        .copied();
    assert!(
        memory.is_some_and(|value| value < 0.0),
        "the drag must have changed the global exposure, got {memory:?} — \
         without this the file assertion below would pass vacuously"
    );
    settle_persisted(&mut harness);
    let disk = persisted_exposure(&dir);
    assert!(
        close(memory.unwrap_or_default(), disk.unwrap_or_default()),
        "the debounced save must write the dragged value: memory {memory:?} vs disk {disk:?}"
    );
}

/// The same drag without any second gesture is the control for the next test:
/// the save lands on its own, so the cancellation below is what breaks it.
#[test]
fn a_multi_frame_mask_local_drag_saves_without_a_second_gesture() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = open_masking_panel(&dir);
    let rail = slider_rail(&harness, ("Presence", 0), Row::Below, "Dehaze");
    press_hold_release(&mut harness, rail, 0.8, HOLD_FRAMES);

    let memory = harness.state().selected_mask_local_presence().unwrap().2;
    assert!(
        memory > 0.5,
        "the press must have changed the local dehaze, got {memory} — \
         without this the file assertion below would pass vacuously"
    );
    settle_persisted(&mut harness);
    let disk = persisted_dehaze(&dir);
    assert!(
        close(memory, disk),
        "the debounced save must write the dragged value: memory {memory} vs disk {disk}"
    );
}

/// **The reproduction.** A `Render / Apply` click inside the 150 ms debounce
/// window must not cancel the armed save.
///
/// `Render / Apply` is a plain re-render: it calls `LuminaApp::render`, which
/// clears `pending_full_render` without committing. The armed token therefore
/// has no reachable commit path any more, and the dragged value stays in memory
/// while the sidecar keeps the pre-drag value. Before the fix this test ends on
/// `memory 0 vs disk 0.6000000238418579`.
#[test]
fn a_render_apply_click_inside_the_debounce_window_does_not_strand_the_save() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = open_masking_panel(&dir);
    // A non-neutral starting value, so the press itself is a value change and
    // the debounce window really opens (a press that changes nothing commits
    // immediately, and there would be no window to cancel).
    harness
        .state_mut()
        .set_mask_local_presence("dehaze", 0.6)
        .unwrap();
    settle_persisted(&mut harness);
    let seeded = persisted_dehaze(&dir);
    assert!(
        close(seeded, 0.6),
        "the seeded dehaze must be 0.6, got {seeded}"
    );

    let rail = slider_rail(&harness, ("Presence", 0), Row::Below, "Dehaze");
    press_hold_release(&mut harness, rail, 0.5, HOLD_FRAMES);
    let memory = harness.state().selected_mask_local_presence().unwrap().2;
    assert!(
        (memory - seeded).abs() > 0.2,
        "the press must have changed the local dehaze away from {seeded}, got {memory}"
    );

    // Inside the window: the Develop footer's plain re-render.
    let render_apply = only_rect(&harness, "Render / Apply");
    click(&mut harness, render_apply);

    settle_persisted(&mut harness);
    let disk = persisted_dehaze(&dir);
    assert!(
        close(memory, disk),
        "a Render / Apply click inside the debounce window must not strand the \
         armed save: memory {memory} vs disk {disk}"
    );
}

/// The `last_edit_time == 0.0` clause: an edit with **no drag** commits
/// immediately. The block-reset control is one real click after a settled drag,
/// so the assertion also pins that the click's own commit is what persisted the
/// cleared block — a stranded token from the drag cannot be the reason it is on
/// disk.
#[test]
fn a_control_click_without_a_drag_saves_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let mut harness = open_masking_panel(&dir);
    let rail = slider_rail(&harness, ("Presence", 0), Row::Below, "Dehaze");
    drag_slider(&mut harness, rail, 0.8);
    settle_persisted(&mut harness);
    let dragged = harness.state().selected_mask_local_presence().unwrap().2;
    assert!(
        close(persisted_dehaze(&dir), dragged),
        "the drag must be on disk before the control click: memory {dragged} vs disk {}",
        persisted_dehaze(&dir)
    );

    // The block reset is a real control: one click, no drag.
    let reset = only_rect(&harness, "all local presence reset");
    click(&mut harness, reset);
    assert_eq!(
        harness.state().selected_mask_local_presence().unwrap(),
        (0.0, 0.0, 0.0)
    );
    settle_persisted(&mut harness);
    assert!(
        close(persisted_dehaze(&dir), 0.0),
        "the immediate commit must persist the cleared presence block, got {}",
        persisted_dehaze(&dir)
    );
    assert!(
        persisted_local_recipe(&dir).presence.is_none(),
        "a neutral presence block must be gone from the sidecar, got {:?}",
        persisted_local_recipe(&dir).presence
    );
}
