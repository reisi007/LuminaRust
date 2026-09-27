//! GUI-INSTRDBG-17c-Rework: click tests for the recipe-mutating controls the
//! 17c verification flagged — the generative canvas checkboxes, the shared
//! per-section Previous/Reset row (all eight Develop panels) and the Optics
//! "Enable lens blur" checkbox — plus the routing proof for both merge mode
//! buttons. Every button is clicked on its real panel and must route through
//! its instrumented command to exactly one debug action line.
//!
//! F-1 (H-1): the three filmstrip selection buttons (Sync Settings / Match
//! Total Exposures / Previous Image) mutate the *selected* images' recipes.
//! They are clicked on the real filmstrip row and must additionally prove the
//! persisted recipe consequence (Sync/Previous) resp. the equalisation (Match).
#![cfg(debug_assertions)]

use super::*;

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

// ---------------------------------------------------------------------------
// B-1: generative canvas checkboxes (recipe-mutating).
// ---------------------------------------------------------------------------

#[test]
fn f100_generative_expand_checkbox_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    let lines = click_action_lines(
        &mut app,
        2000.0,
        &["Generative Expand", Str::ExpandBeyondImage.t()],
        |app, ui| app.draw_generative_expand(ui),
    );
    assert_single_action_line(&lines, GuiAction::SetExpandBeyondImage);
    assert!(
        app.recipe()
            .generative_edit
            .as_ref()
            .is_some_and(|edit| edit.effective_expand()),
        "the checkbox must arm the expand role"
    );
}

#[test]
fn f100_generative_auto_fill_checkbox_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    let lines = click_action_lines(
        &mut app,
        2000.0,
        &["Generative Expand", Str::AutoFillTransparent.t()],
        |app, ui| app.draw_generative_expand(ui),
    );
    assert_single_action_line(&lines, GuiAction::SetAutoFillTransparent);
    assert_eq!(
        app.recipe()
            .generative_edit
            .as_ref()
            .and_then(|edit| edit.auto_fill_transparent),
        Some(true),
        "the checkbox must set the auto-fill role"
    );
}

// ---------------------------------------------------------------------------
// B-2: per-section Previous/Reset (all eight panels share the row).
// ---------------------------------------------------------------------------

type DrawSection = fn(&mut LuminaApp, &mut egui::Ui);

/// The eight Develop section draw functions in F-100 order. All call the shared
/// `draw_section_prev_reset`, so the loop below clicks the Previous/Reset
/// button of every panel.
fn section_draws() -> [(usize, DrawSection); SECTION_COUNT] {
    [
        (SECTION_BASIC, |app, ui| app.draw_basic(ui)),
        (SECTION_TONE_CURVE, |app, ui| app.draw_tone_curve(ui)),
        (SECTION_COLOR, |app, ui| app.draw_color(ui)),
        (SECTION_DETAIL, |app, ui| app.draw_detail(ui)),
        (SECTION_EFFECTS, |app, ui| app.draw_effects(ui)),
        (SECTION_OPTICS, |app, ui| app.draw_optics(ui)),
        (SECTION_GEOMETRY, |app, ui| app.draw_geometry(ui)),
        (SECTION_MASKING, |app, ui| app.draw_masking(ui)),
    ]
}

#[test]
fn f100_section_previous_button_logs_one_line_per_panel() {
    for (index, draw) in section_draws() {
        let (_directory, mut app) = persistent_app();
        app.set_section_open(index, true);
        let lines = click_action_lines(&mut app, 8000.0, &[Str::Previous.t()], draw);
        assert_single_action_line(&lines, GuiAction::RestoreSectionPrevious);
    }
}

#[test]
fn f100_section_reset_button_logs_one_line_per_panel() {
    for (index, draw) in section_draws() {
        let (_directory, mut app) = persistent_app();
        app.set_section_open(index, true);
        let lines = click_action_lines(&mut app, 8000.0, &[Str::Reset.t()], draw);
        assert_single_action_line(&lines, GuiAction::ResetSection);
    }
}

// ---------------------------------------------------------------------------
// B-3: Optics lens-blur enable checkbox (recipe-mutating).
// ---------------------------------------------------------------------------

#[test]
fn f100_optics_lens_blur_enable_logs_one_line() {
    let (_directory, mut app) = persistent_app();
    app.set_section_open(SECTION_OPTICS, true);
    let lines = click_action_lines(
        &mut app,
        8000.0,
        &[Str::LensBlur.t(), Str::LensBlurEnable.t()],
        |app, ui| app.draw_optics(ui),
    );
    assert_single_action_line(&lines, GuiAction::SetLensBlurEnabled);
    assert!(
        app.recipe()
            .lens_blur
            .as_ref()
            .is_some_and(|blur| blur.enabled),
        "the checkbox must enable the lens-blur stage"
    );
}

// ---------------------------------------------------------------------------
// B-7: both merge mode buttons route through the single instrumented
// `start_merge` (no second, untraced route). The click fails loudly (fewer
// than two sources) after the action line, so no merge thread is spawned.
// ---------------------------------------------------------------------------

#[test]
fn f100_merge_hdr_button_routes_to_start_merge() {
    let (_directory, mut app) = persistent_app();
    let lines = click_action_lines(
        &mut app,
        2000.0,
        &[Str::MergeSection.t(), Str::MergeHdr.t()],
        |app, ui| app.draw_merge_section(ui),
    );
    assert_single_action_line(&lines, GuiAction::StartMerge);
}

#[test]
fn f100_merge_pano_button_routes_to_start_merge() {
    let (_directory, mut app) = persistent_app();
    let lines = click_action_lines(
        &mut app,
        2000.0,
        &[Str::MergeSection.t(), Str::MergePano.t()],
        |app, ui| app.draw_merge_section(ui),
    );
    assert_single_action_line(&lines, GuiAction::StartMerge);
}

// ---------------------------------------------------------------------------
// No-op-Log-Regression: an instrumented command must log its line even when it
// re-applies the value it already holds (the debug log is a command trace, not
// a change log).
// ---------------------------------------------------------------------------

#[test]
fn instrdbg_rework_noop_repeats_still_log_one_line() {
    let mut app = new_app();
    let _ = take_gui_action_log();
    let _ = app.set_expand_beyond_image(false);
    let _ = app.set_expand_beyond_image(false);
    let _ = app.set_auto_fill_transparent(false);
    let _ = app.set_auto_fill_transparent(false);
    app.set_lens_blur_enabled(false);
    app.set_lens_blur_enabled(false);
    let lines = take_gui_action_log();
    assert_eq!(lines.len(), 6, "{lines:?}");
    let names = [
        "set_expand_beyond_image",
        "set_expand_beyond_image",
        "set_auto_fill_transparent",
        "set_auto_fill_transparent",
        "set_lens_blur_enabled",
        "set_lens_blur_enabled",
    ];
    for (line, name) in lines.iter().zip(names) {
        assert!(line.starts_with(&format!("action={name} ")), "{line}");
    }
}

// ---------------------------------------------------------------------------
// F-1 (H-1): the three filmstrip selection buttons. They live in the
// filmstrip header row (painted above the thumbnails in all three modules), so
// the click harness draws the real `draw_filmstrip`.
// ---------------------------------------------------------------------------

/// Paint the real filmstrip in the click harness (`draw_filmstrip` needs the
/// frame's `egui::Context` alongside the `Ui`).
fn draw_filmstrip_only(app: &mut LuminaApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    app.draw_filmstrip(&ctx, ui);
}

/// A solid-gray 2x1 PNG so the Match button has two measurably different
/// luminances to equalise.
fn write_gray_png(path: &Path, level: u8) {
    let pixels = vec![level, level, level, 255, level, level, level, 255];
    let png = ImageFrame::new(2, 1, pixels)
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap();
    std::fs::write(path, png).unwrap();
}

#[test]
fn f100_filmstrip_sync_button_logs_one_line_and_applies_recipe() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    let target = directory.path().join("target.png");
    save_png(&source);
    save_png(&target);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("contrast", 0.3);
    app.filmstrip_selection.clear();
    app.filmstrip_selection.insert(target.display().to_string());
    // The label carries the selection counter once an image is selected.
    let label = format!("{} (1)", Str::SyncSettings.t());
    let lines = click_action_lines(&mut app, 720.0, &[&label], draw_filmstrip_only);
    assert_single_action_line(&lines, GuiAction::SyncSettingsToSelection);
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&target)).unwrap();
    let copy = document
        .virtual_copies
        .iter()
        .find(|copy| copy.is_default)
        .unwrap();
    assert_eq!(
        copy.recipe.adjustments["contrast"], 0.3,
        "Sync must persist the active recipe into the selected sidecar"
    );
}

#[test]
fn f100_filmstrip_match_button_logs_one_line_and_equalizes() {
    let directory = tempfile::tempdir().unwrap();
    let dark = directory.path().join("dark.png");
    let bright = directory.path().join("bright.png");
    write_gray_png(&dark, 30);
    write_gray_png(&bright, 220);
    let mut app = new_app();
    open_and_decode(&mut app, dark.display().to_string());
    app.filmstrip_selection.clear();
    app.filmstrip_selection.insert(dark.display().to_string());
    app.filmstrip_selection.insert(bright.display().to_string());
    let lines = click_action_lines(
        &mut app,
        720.0,
        &[Str::MatchSelection.t()],
        draw_filmstrip_only,
    );
    assert_single_action_line(&lines, GuiAction::MatchExposuresOfSelection);
    let exposure_of = |path: &PathBuf| {
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(path))
            .unwrap()
            .virtual_copies
            .iter()
            .find(|copy| copy.is_default)
            .unwrap()
            .recipe
            .adjustments["exposure"]
    };
    assert!(
        exposure_of(&dark) > exposure_of(&bright),
        "Match Total Exposures must favour the darker selected image"
    );
}

#[test]
fn f100_filmstrip_previous_button_logs_one_line_and_applies_reference() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first.png");
    let second = directory.path().join("second.png");
    save_png(&first);
    save_png(&second);
    let mut app = new_app();
    open_and_decode(&mut app, first.display().to_string());
    app.set_adjustment("contrast", 0.3);
    app.save_sidecar();
    open_and_decode_switch(&mut app, &second.display().to_string());
    app.filmstrip_selection.clear();
    app.filmstrip_selection.insert(second.display().to_string());
    let lines = click_action_lines(
        &mut app,
        720.0,
        &[Str::PreviousImage.t()],
        draw_filmstrip_only,
    );
    assert_single_action_line(&lines, GuiAction::ApplyPreviousToSelection);
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&second)).unwrap();
    let copy = document
        .virtual_copies
        .iter()
        .find(|copy| copy.is_default)
        .unwrap();
    assert_eq!(
        copy.recipe.adjustments["contrast"], 0.3,
        "Previous must apply the reference recipe to the selected sidecar"
    );
}

// ---------------------------------------------------------------------------
// F-A: the KI-Denoise enable checkbox (Detail section, recipe-mutating). The
// control is visible although the AI-Denoise stage itself is Release 2.0, so it
// is instrumented instead of exempted. The two sliders are slider-class and
// the fallback policy switch is session-only — both stay uninstrumented.
// ---------------------------------------------------------------------------

#[test]
fn f100_denoise_enable_checkbox_logs_one_line_and_enables_stage() {
    let (_directory, mut app) = persistent_app();
    let lines = click_action_lines(&mut app, 2000.0, &[Str::DenoiseEnable.t()], |app, ui| {
        app.draw_denoise_section(ui)
    });
    assert_single_action_line(&lines, GuiAction::SetDenoiseEnabled);
    assert!(
        app.recipe()
            .denoise_ai
            .as_ref()
            .is_some_and(|denoise| denoise.enabled),
        "the checkbox must enable the denoise_ai stage"
    );
}
