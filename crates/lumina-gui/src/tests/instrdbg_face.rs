//! GUI-INSTRDBG-17c-Rest: click test for the last user-visible, recipe-/mask-
//! mutating button — the Library People view's per-face "Use as mask" Develop
//! bridge (`LuminaApp::create_face_mask`). Clicked on the real People view and
//! must (a) route through the instrumented command to exactly one debug action
//! line and (b) prove the persisted mask-definition consequence in the recipe
//! and the sidecar.
#![cfg(debug_assertions)]

use super::*;

/// Click `labels` in order on one persistent headless context and return the
/// debug action lines captured for that click. The capture is drained first,
/// so the `seed_face` preparation can never masquerade as the button's line.
fn click_action_lines(
    app: &mut LuminaApp,
    height: f32,
    labels: &[&str],
    draw: impl FnMut(&mut LuminaApp, &mut egui::Ui),
) -> Vec<String> {
    let _ = take_gui_action_log();
    headless_click_labels_sized(app, height, labels, draw);
    take_gui_action_log()
}

fn assert_single_action_line(lines: &[String], action: GuiAction) {
    assert_eq!(
        lines.len(),
        1,
        "{action:?}: expected one line, got {lines:?}"
    );
    let expected = format!("action={} ", action.name());
    assert!(
        lines[0].starts_with(&expected),
        "{action:?}: expected {expected:?}, got {:?}",
        lines[0]
    );
}

/// Paint the real Library surface in the People view (`draw_library_grid`
/// dispatches to `draw_library_people` for `LibraryView::People`).
fn draw_people_view(app: &mut LuminaApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    app.draw_library_grid(&ctx, ui);
}

#[test]
fn f100_people_use_as_mask_button_logs_one_line_and_persists_mask() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    // A persisted, valid face analysis: the exact embedding records the
    // references checksum, so `face_view_status()` is `Valid` (not staged).
    crate::face_gui::tests::seed_face(&mut app);
    app.set_library_view(LibraryView::People);
    app.people_selected_cluster = "cluster-a".into();

    let lines = click_action_lines(
        &mut app,
        4096.0,
        &[Str::FaceUseAsMask.t()],
        draw_people_view,
    );
    assert_single_action_line(&lines, GuiAction::CreateFaceMask);

    // The button created a deterministic mask definition in the active virtual
    // copy (recipe/mask mutation, never a display-only action).
    let created = app.document.as_ref().unwrap().virtual_copies[0]
        .mask_library
        .iter()
        .find(|mask| mask.prompt.is_some())
        .cloned()
        .expect("the button must add a face-box mask definition");
    assert!(created.id.starts_with("mask-"));

    // Persisted through the shared sidecar writer: a reopen restores it.
    let persisted =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert!(
        persisted.virtual_copies[0]
            .mask_library
            .iter()
            .any(|mask| mask.id == created.id),
        "the created mask must be in the persisted sidecar"
    );
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert!(reopened.document.as_ref().unwrap().virtual_copies[0]
        .mask_library
        .iter()
        .any(|mask| mask.id == created.id));
}
