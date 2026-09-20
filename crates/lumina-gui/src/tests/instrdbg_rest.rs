//! GUI-INSTRDBG-17b-REST: headless click tests for the Spot-extras, Detail/
//! red-eye, Optics, Tone-Curve and Presets buttons. Every new button is
//! clicked on its real panel and must route through its instrumented command
//! to exactly one debug action line. The direct-trigger coverage lives in
//! `tests/instrdbg.rs`; both files stay inside the file-size ratchet.
#![cfg(debug_assertions)]

use super::*;

// ---------------------------------------------------------------------------
// GUI-INSTRDBG-17b-REST: click tests. Each new button is clicked on the real
// panel (Button -> instrumented command) and must produce exactly one line.
// ---------------------------------------------------------------------------

/// Click `labels` in order (section/sub-section headers first, then the action
/// button) on one persistent headless context and return the debug action
/// lines captured for that click. The capture is drained first, so preparation
/// clicks or preparation setters can never masquerade as the button's line.
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
fn f100_spot_quick_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    let lines = click_action_lines(&mut app, 4096.0, &["Quick"], |app, ui| {
        app.draw_spot_tool_options(ui)
    });
    assert_single_action_line(&lines, GuiAction::SetSpotMode);
}

#[test]
fn f100_spot_generative_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    let lines = click_action_lines(&mut app, 4096.0, &["Generative"], |app, ui| {
        app.draw_spot_tool_options(ui)
    });
    assert_single_action_line(&lines, GuiAction::SetSpotMode);
}

#[test]
fn f100_spot_visualize_off_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    let lines = click_action_lines(
        &mut app,
        4096.0,
        &["Remove options", "Visualize off"],
        |app, ui| app.draw_spot_tool_options(ui),
    );
    assert_single_action_line(&lines, GuiAction::ClearSpotVisualize);
}

#[test]
fn f100_spot_detect_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    let lines = click_action_lines(
        &mut app,
        4096.0,
        &["Remove options", "Detect objects"],
        |app, ui| app.draw_spot_tool_options(ui),
    );
    assert_single_action_line(&lines, GuiAction::DetectSpotCandidates);
}

#[test]
fn f100_spot_apply_detected_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    let lines = click_action_lines(
        &mut app,
        4096.0,
        &["Remove options", "Apply detected"],
        |app, ui| app.draw_spot_tool_options(ui),
    );
    assert_single_action_line(&lines, GuiAction::ApplyDetectedSpots);
}

#[test]
fn f100_spot_regenerate_variant_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    app.spot_gen_target = "spot-missing".into();
    let lines = click_action_lines(
        &mut app,
        4096.0,
        &["Remove options", "Regenerate variant"],
        |app, ui| app.draw_spot_tool_options(ui),
    );
    assert_single_action_line(&lines, GuiAction::RegenerateSpotVariant);
}

#[test]
fn f100_spot_clear_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    let lines = click_action_lines(
        &mut app,
        4096.0,
        &["Remove options", "Clear spots"],
        |app, ui| app.draw_spot_tool_options(ui),
    );
    assert_single_action_line(&lines, GuiAction::ClearSpotHeals);
}

#[test]
fn f100_detail_detect_pupils_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    app.set_section_open(SECTION_DETAIL, true);
    let lines = click_action_lines(&mut app, 8000.0, &[Str::RedEyeDetect.t()], |app, ui| {
        app.draw_detail(ui)
    });
    assert_single_action_line(&lines, GuiAction::DetectRedEye);
}

#[test]
fn f100_detail_apply_detected_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    app.set_section_open(SECTION_DETAIL, true);
    let lines = click_action_lines(
        &mut app,
        8000.0,
        &[Str::RedEyeApplyDetected.t()],
        |app, ui| app.draw_detail(ui),
    );
    assert_single_action_line(&lines, GuiAction::ApplyDetectedRedEyes);
}

#[test]
fn f100_detail_remove_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    let _ = app.add_red_eye_region(0.5, 0.5);
    app.set_section_open(SECTION_DETAIL, true);
    let lines = click_action_lines(&mut app, 8000.0, &[Str::RedEyeRemove.t()], |app, ui| {
        app.draw_detail(ui)
    });
    assert_single_action_line(&lines, GuiAction::RemoveRedEyeRegion);
}

#[test]
fn f100_detail_clear_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    let _ = app.add_red_eye_region(0.5, 0.5);
    app.set_section_open(SECTION_DETAIL, true);
    let lines = click_action_lines(&mut app, 8000.0, &[Str::RedEyeClear.t()], |app, ui| {
        app.draw_detail(ui)
    });
    assert_single_action_line(&lines, GuiAction::ClearRedEye);
}

#[test]
fn f100_optics_lens_profile_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    app.set_section_open(SECTION_OPTICS, true);
    // The combo's selected text ("none") opens the popup; the item selects the
    // profile.
    let lines = click_action_lines(&mut app, 8000.0, &["none", "wide-light"], |app, ui| {
        app.draw_optics(ui)
    });
    assert_single_action_line(&lines, GuiAction::SetLensProfile);
}

#[test]
fn f100_optics_clear_lens_profile_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    let _ = app.set_lens_profile("wide-light");
    app.set_section_open(SECTION_OPTICS, true);
    let lines = click_action_lines(&mut app, 8000.0, &["wide-light", "none"], |app, ui| {
        app.draw_optics(ui)
    });
    assert_single_action_line(&lines, GuiAction::ClearLensProfile);
}

#[test]
fn f100_optics_bokeh_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    app.set_section_open(SECTION_OPTICS, true);
    let lines = click_action_lines(
        &mut app,
        8000.0,
        &[Str::LensBlur.t(), Str::LensBlurBokehRound.t()],
        |app, ui| app.draw_optics(ui),
    );
    assert_single_action_line(&lines, GuiAction::SetLensBlurBokeh);
}

// UX-LOOK-TONECURVE-18: the former Tone-Curve add/remove *buttons* were
// replaced by graph gestures (click adds, double-click removes). Their
// click→instrumentation coverage now lives in
// `tests/tone_curve_graph.rs` (`graph_gesture_add_logs_curve_action` /
// `graph_gesture_remove_logs_curve_action`).

#[test]
fn f100_presets_apply_button_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    app.presets_dir = None;
    app.preset_name = "instrdbg-preset".into();
    let lines = click_action_lines(
        &mut app,
        720.0,
        &[Str::PresetsSection.t(), Str::ApplyPreset.t()],
        |app, ui| app.draw_presets_section(ui),
    );
    assert_single_action_line(&lines, GuiAction::ApplyPreset);
}

#[test]
fn f100_presets_save_file_button_logs_one_line() {
    let (directory, mut app) = persistent_app();
    let presets = directory.path().join("presets");
    std::fs::create_dir_all(&presets).unwrap();
    app.presets_dir = Some(presets);
    app.preset_name = "instrdbg-preset".into();
    let lines = click_action_lines(
        &mut app,
        720.0,
        &[Str::PresetsSection.t(), Str::SavePresetFile.t()],
        |app, ui| app.draw_presets_section(ui),
    );
    assert_single_action_line(&lines, GuiAction::SavePresetFile);
}

/// No-op-Log-Regression: an instrumented command must log its line even when it
/// re-applies the value it already holds (the round-2 debug log is a command
/// trace, not a change log).
#[test]
fn instrdbg_rest_noop_repeats_still_log_one_line() {
    let mut app = new_app();
    let _ = take_gui_action_log();
    app.set_spot_mode(SpotMode::Heuristic);
    app.set_spot_mode(SpotMode::Heuristic);
    app.set_lens_blur_bokeh(BokehShape::Round);
    app.set_lens_blur_bokeh(BokehShape::Round);
    let lines = take_gui_action_log();
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert!(
        lines[0].starts_with("action=set_spot_mode "),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].starts_with("action=set_spot_mode "),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].starts_with("action=set_lens_blur_bokeh "),
        "{}",
        lines[2]
    );
    assert!(
        lines[3].starts_with("action=set_lens_blur_bokeh "),
        "{}",
        lines[3]
    );
}
