//! G-01 treatment/panel/histogram release tests tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// LRPAR-G01-BASIC (B2): the B&W-exit stash warning fires only on a
/// missing or corrupt stash — a clean restore is silent.
#[test]
fn g01_bw_stash_warning_only_on_missing_or_corrupt_stash() {
    // No B&W state at all: nothing to warn about.
    assert!(!LuminaApp::bw_restore_needs_stash_warning(
        &EditRecipe::default()
    ));
    // Clean stash (written by the shared path): silent restore.
    let mut clean = EditRecipe::default();
    clean.apply_treatment("bw").unwrap();
    assert!(!LuminaApp::bw_restore_needs_stash_warning(&clean));
    // Marker without stash: warn.
    let mut missing = EditRecipe::default();
    missing
        .extras
        .insert("treatment".into(), serde_json::Value::String("bw".into()));
    assert!(LuminaApp::bw_restore_needs_stash_warning(&missing));
    // Marker with corrupt stash: warn.
    let mut corrupt = EditRecipe::default();
    corrupt
        .extras
        .insert("treatment".into(), serde_json::Value::String("bw".into()));
    corrupt.extras.insert(
        BW_STASH_KEY.into(),
        serde_json::Value::String("corrupt".into()),
    );
    assert!(LuminaApp::bw_restore_needs_stash_warning(&corrupt));
}

/// LRPAR-G01-BASIC: Treatment + Profile end-to-end —
/// setter → commit → sidecar file → reload restores both; invalid values
/// fail loudly; the `V` toggle keeps working through the shared path.
#[test]
fn g01_treatment_profile_end_to_end() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    assert!(app.error().is_none());
    assert!(!app.bw_active());
    app.set_treatment("bw").unwrap();
    assert!(app.error().is_none());
    assert!(app.bw_active());
    app.set_profile("vivid").unwrap();
    assert!(app.error().is_none());
    assert_eq!(app.recipe().develop_profile(), "vivid");
    // Reload from disk: both fields restored.
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert!(reopened.bw_active());
    assert_eq!(reopened.recipe().develop_profile(), "vivid");
    assert_eq!(reopened.recipe().adjustments.get("saturation"), Some(&-1.0));
    // Invalid values fail loudly without touching the recipe.
    assert!(reopened.set_treatment("sepia").is_err());
    assert!(reopened.set_profile("adobe-color").is_err());
    assert!(reopened.bw_active());
    // The `V` toggle exits through the same stash path (identity again).
    reopened.toggle_black_white().unwrap();
    assert!(!reopened.bw_active());
    assert!(!reopened.recipe().adjustments.contains_key("saturation"));
    assert!(!reopened.recipe().adjustments.contains_key("vibrance"));
    let mut reread = new_app();
    open_and_decode(&mut reread, source.display().to_string());
    assert!(!reread.bw_active());
}

/// LRPAR-G01-BASIC: Basic panel Previous/Reset end-to-end — edit →
/// Previous restores the last saved state, Reset sets documented
/// defaults, reload confirms the file state.
#[test]
fn g01_panel_previous_reset_basic_end_to_end() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    // Uncommitted edit → Previous restores the load baseline (absent).
    app.set_adjustment("exposure", 1.5);
    app.restore_section_previous(SECTION_BASIC).unwrap();
    assert!(app.error().is_none());
    assert!(!app.recipe().adjustments.contains_key("exposure"));
    // Committed edit moves the baseline; a later uncommitted edit is
    // undone back to the committed value.
    app.set_adjustment("exposure", 2.0);
    app.commit_pending_slider_save([0, 0]);
    assert!(app.error().is_none());
    app.set_adjustment("exposure", 0.5);
    app.restore_section_previous(SECTION_BASIC).unwrap();
    assert_eq!(app.recipe().adjustments.get("exposure"), Some(&2.0));
    // Reset sets documented defaults (persisted).
    app.reset_section(SECTION_BASIC).unwrap();
    assert_eq!(app.recipe().adjustments.get("exposure"), Some(&0.0));
    assert_eq!(
        app.recipe().adjustments.get("wb_temperature"),
        Some(&6500.0)
    );
    assert_eq!(app.recipe().develop_profile(), "default");
    // Out-of-range sections fail loudly.
    assert!(app.restore_section_previous(SECTION_COUNT).is_err());
    assert!(app.reset_section(SECTION_COUNT).is_err());
    // Reload confirms the persisted reset state.
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert_eq!(reopened.recipe().adjustments.get("exposure"), Some(&0.0));
}

/// LRPAR-G01-BASIC (DoD Klassen-Vollständigkeit): Previous + Reset are
/// accepted for every one of the eight Develop sections (no sampling).
#[test]
fn g01_panel_previous_reset_all_sections_accept() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("exposure", 1.0);
    app.commit_pending_slider_save([0, 0]);
    assert!(app.error().is_none());
    for index in 0..SECTION_COUNT {
        app.restore_section_previous(index)
            .unwrap_or_else(|_| panic!("previous must accept section {index}"));
        app.reset_section(index)
            .unwrap_or_else(|_| panic!("reset must accept section {index}"));
        assert!(
            app.error().is_none(),
            "section {index} must stay error-free"
        );
    }
    // The section names cover the F-100 panel order exactly.
    let names: Vec<&str> = (0..SECTION_COUNT)
        .map(|i| section_name(i).unwrap())
        .collect();
    assert_eq!(
        names,
        vec![
            "Basic",
            "Tone Curve",
            "Color",
            "Detail",
            "Effects",
            "Optics",
            "Geometry",
            "Masking"
        ]
    );
}

/// LRPAR-G01-BASIC: the Original-Photo reference is deterministic —
/// two measurements of the same state are identical, and an edit moves
/// the edited side (delta present, never silent zero).
#[test]
fn g01_histogram_compare_is_deterministic() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("exposure", 1.0);
    app.commit_pending_slider_save([0, 0]);
    let (original_a, edited_a) = app
        .histogram_compare_data()
        .expect("compare needs both sides");
    let (original_b, edited_b) = app
        .histogram_compare_data()
        .expect("compare needs both sides");
    assert_eq!(original_a.bins, original_b.bins);
    assert_eq!(edited_a.bins, edited_b.bins);
    // The edit moved the edited side away from the unedited decode.
    assert_ne!(edited_a.bins, original_a.bins);
    let (mean_delta, l1) = app.histogram_delta().expect("delta needs both sides");
    assert!(mean_delta.is_finite() && l1.is_finite());
    assert!(l1 > 0.0, "an exposure edit must drift the histogram");
}

/// LRPAR-G01-BASIC: "Reset Sliders Automatically" — armed, an image
/// switch discards the pending edit (no sidecar for the old image);
/// disarmed, the same switch flushes it (sidecar carries the edit).
/// The flag itself roundtrips through `.lumina/settings.json`.
#[test]
fn g01_reset_sliders_automatically_drop_vs_flush() {
    let directory = tempfile::tempdir().unwrap();
    let source_a = directory.path().join("a.png");
    let source_b = directory.path().join("b.png");
    save_png(&source_a);
    save_png(&source_b);
    let sidecar_a = lumina_sidecar::sidecar_path_for(&source_a);
    let mut app = new_app();
    open_and_decode(&mut app, source_a.display().to_string());
    // Armed: the pending edit is discarded on switch.
    app.set_reset_sliders_automatically(true);
    assert!(app.reset_sliders_automatically());
    app.set_adjustment("exposure", 1.0);
    open_and_decode_switch(&mut app, &source_b.display().to_string());
    assert!(
        !sidecar_a.exists(),
        "armed switch must not save the old image"
    );
    // Disarmed: the pending edit is flushed to the old image's sidecar.
    open_and_decode_switch(&mut app, &source_a.display().to_string());
    app.set_reset_sliders_automatically(false);
    assert!(!app.reset_sliders_automatically());
    app.set_adjustment("exposure", 2.0);
    open_and_decode_switch(&mut app, &source_b.display().to_string());
    let document = lumina_sidecar::load_sidecar(&sidecar_a).unwrap();
    assert_eq!(
        document.virtual_copies[0]
            .recipe
            .adjustments
            .get("exposure"),
        Some(&2.0)
    );
    // The flag roundtripped through the folder settings file (the second
    // `open_file` refreshed it from disk and it stayed off).
    assert!(!app.reset_sliders_automatically());
}
