//! GUI-INSTRDBG-17c: headless click tests for the last user-visible buttons
//! (WB eyedropper, Point Color add/remove, Spot-distraction checkboxes,
//! Red-Eye region-pick toggle, Presets refresh and the generative canvas
//! buttons). Every button is clicked on its real panel and must route through
//! its instrumented command to exactly one debug action line.
#![cfg(debug_assertions)]

use super::*;

/// Click `labels` in order on one persistent headless context and return the
/// debug action lines captured for that click. The capture is drained first,
/// so preparation setters can never masquerade as the button's line.
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

#[test]
fn f100_spot_distraction_checkbox_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    let lines = click_action_lines(
        &mut app,
        6000.0,
        &["Dust Removal (Q)", "Dust"],
        |app, ui| app.draw_spot_heal(ui),
    );
    assert_single_action_line(&lines, GuiAction::SetSpotDistraction);
    assert!(app.spot_distraction().dust, "the checkbox must persist");
}

#[test]
fn f100_detail_red_eye_pick_toggle_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    app.set_section_open(SECTION_DETAIL, true);
    let lines = click_action_lines(&mut app, 8000.0, &[Str::RedEyePickMode.t()], |app, ui| {
        app.draw_detail(ui)
    });
    assert_single_action_line(&lines, GuiAction::SetRedEyePickMode);
    assert!(app.red_eye_pick_mode, "the toggle must arm the picker");
}

#[test]
fn f100_presets_refresh_button_logs_one_line() {
    let (directory, mut app) = persistent_app();
    let presets = directory.path().join("presets");
    std::fs::create_dir_all(&presets).unwrap();
    app.presets_dir = Some(presets);
    let lines = click_action_lines(
        &mut app,
        720.0,
        &[Str::PresetsSection.t(), Str::Refresh.t()],
        |app, ui| app.draw_presets_section(ui),
    );
    assert_single_action_line(&lines, GuiAction::ReloadPresetEntries);
}

#[test]
fn f100_basic_wb_eyedropper_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    app.set_section_open(SECTION_BASIC, true);
    let lines = click_action_lines(&mut app, 6000.0, &[Str::WbEyedropper.t()], |app, ui| {
        app.draw_basic(ui)
    });
    assert_single_action_line(&lines, GuiAction::ArmWbEyedropper);
    assert!(app.wb_pick_mode, "the button must arm the eyedropper");
}

#[test]
fn f100_color_point_color_add_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    app.set_section_open(SECTION_COLOR, true);
    let lines = click_action_lines(&mut app, 8000.0, &[Str::PointColorAdd.t()], |app, ui| {
        app.draw_color(ui)
    });
    assert_single_action_line(&lines, GuiAction::AddPointColor);
}

#[test]
fn f100_color_point_color_remove_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    app.add_point_color();
    app.set_section_open(SECTION_COLOR, true);
    let lines = click_action_lines(&mut app, 8000.0, &[Str::PointColorRemove.t()], |app, ui| {
        app.draw_color(ui)
    });
    assert_single_action_line(&lines, GuiAction::RemovePointColor);
}

#[test]
fn f100_generative_apply_canvas_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    let _ = app.set_expand_beyond_image(true);
    let lines = click_action_lines(
        &mut app,
        2000.0,
        &["Generative Expand", "Apply Frame (drag) → Canvas"],
        |app, ui| app.draw_generative_expand(ui),
    );
    assert_single_action_line(&lines, GuiAction::SetExpandCanvas);
}

#[test]
fn f100_generative_generate_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    let _ = app.set_expand_beyond_image(true);
    let lines = click_action_lines(
        &mut app,
        2000.0,
        &["Generative Expand", Str::GenerateCanvas.t()],
        |app, ui| app.draw_generative_expand(ui),
    );
    assert_single_action_line(&lines, GuiAction::GenerateCanvas);
}

/// No-op-Log-Regression: an instrumented command must log its line even when it
/// re-applies the value it already holds (the round-2 debug log is a command
/// trace, not a change log).
#[test]
fn instrdbg_last_noop_repeats_still_log_one_line() {
    let mut app = new_app();
    let _ = take_gui_action_log();
    app.set_spot_distraction(SpotDistraction::default());
    app.set_spot_distraction(SpotDistraction::default());
    app.set_red_eye_pick_mode(false);
    app.set_red_eye_pick_mode(false);
    let lines = take_gui_action_log();
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert!(
        lines[0].starts_with("action=set_spot_distraction "),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].starts_with("action=set_spot_distraction "),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].starts_with("action=set_red_eye_pick_mode "),
        "{}",
        lines[2]
    );
    assert!(
        lines[3].starts_with("action=set_red_eye_pick_mode "),
        "{}",
        lines[3]
    );
}
