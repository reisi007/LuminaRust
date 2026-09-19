//! filmstrip selection, sync/match/previous actions tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// GUI-FILMSTRIP-SYNC-1: pure click semantics — plain click selects exactly
/// the clicked image, Cmd/Ctrl-Click toggles membership, Shift-Click adds
/// the inclusive anchor→clicked range and keeps the anchor.
#[test]
fn filmstrip_click_toggle_and_range_semantics() {
    let order: Vec<String> = ["a", "b", "c", "d"]
        .iter()
        .map(|name| name.to_string())
        .collect();
    let empty = BTreeSet::new();
    // Plain click selects exactly one image and sets the anchor.
    let (selected, anchor) =
        LuminaApp::apply_filmstrip_click(&order, &empty, None, "b", false, false);
    assert_eq!(selected, BTreeSet::from(["b".to_string()]));
    assert_eq!(anchor.as_deref(), Some("b"));
    // Toggle adds a second image and moves the anchor.
    let (selected, anchor) =
        LuminaApp::apply_filmstrip_click(&order, &selected, anchor.as_deref(), "d", true, false);
    assert_eq!(selected, BTreeSet::from(["b".to_string(), "d".to_string()]));
    assert_eq!(anchor.as_deref(), Some("d"));
    // Toggling the same image again removes it.
    let (selected, anchor) =
        LuminaApp::apply_filmstrip_click(&order, &selected, anchor.as_deref(), "b", true, false);
    assert_eq!(selected, BTreeSet::from(["d".to_string()]));
    assert_eq!(anchor.as_deref(), Some("b"));
    // Range from the anchor adds the inclusive span and keeps the anchor.
    let (selected, anchor) =
        LuminaApp::apply_filmstrip_click(&order, &selected, anchor.as_deref(), "a", false, true);
    assert_eq!(
        selected,
        BTreeSet::from(["a".to_string(), "b".to_string(), "d".to_string()])
    );
    assert_eq!(anchor.as_deref(), Some("b"));
    // Range without an anchor covers only the clicked image.
    let (selected, anchor) =
        LuminaApp::apply_filmstrip_click(&order, &empty, None, "c", false, true);
    assert_eq!(selected, BTreeSet::from(["c".to_string()]));
    assert_eq!(anchor, None);
    // Unknown paths never mutate selection or anchor.
    let (kept, kept_anchor) =
        LuminaApp::apply_filmstrip_click(&order, &selected, anchor.as_deref(), "zzz", false, false);
    assert_eq!(kept, selected);
    assert_eq!(kept_anchor, anchor);
}

/// GUI-FILMSTRIP-SYNC-1, End-to-End (DoD §1): recipe → sync → N sidecar
/// files → reload → recipe restored. Every applied image also bumps
/// `preview_generation`.
#[test]
fn sync_settings_writes_each_selected_sidecar_and_reloads() {
    let directory = tempfile::tempdir().unwrap();
    let sources: Vec<PathBuf> = ["a.png", "b.png", "c.png"]
        .iter()
        .map(|name| directory.path().join(name))
        .collect();
    for source in &sources {
        save_png(source);
    }
    let mut app = new_app();
    open_and_decode(&mut app, sources[0].display().to_string());
    app.set_adjustment("exposure", 1.5);
    for source in &sources {
        app.filmstrip_selection.insert(source.display().to_string());
    }
    let generation = app.preview_generation();
    let report = app.sync_settings_to_selection();
    assert_eq!(report.applied_count(), 3);
    assert_eq!(report.failed_count(), 0);
    assert_eq!(app.preview_generation(), generation + 3);
    for source in &sources {
        let document =
            lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(source)).unwrap();
        let copy = document
            .virtual_copies
            .iter()
            .find(|copy| copy.is_default)
            .unwrap();
        assert_eq!(copy.recipe.adjustments["exposure"], 1.5);
    }
    // Reload anchor: reopening a synced image restores the recipe.
    let mut reopened = new_app();
    open_and_decode(&mut reopened, sources[1].display().to_string());
    assert_eq!(reopened.recipe().adjustments["exposure"], 1.5);
}

/// GUI-FILMSTRIP-SYNC-1: one unreadable target is a loud per-image entry
/// and never aborts the remaining targets.
#[test]
fn sync_settings_reports_per_image_failure_without_aborting_rest() {
    let directory = tempfile::tempdir().unwrap();
    let good = directory.path().join("good.png");
    save_png(&good);
    let missing = directory.path().join("gone.png");
    let mut app = new_app();
    open_and_decode(&mut app, good.display().to_string());
    app.set_adjustment("contrast", 0.3);
    app.filmstrip_selection.insert(good.display().to_string());
    app.filmstrip_selection
        .insert(missing.display().to_string());
    let report = app.sync_settings_to_selection();
    assert_eq!(report.applied_count(), 1);
    assert_eq!(report.failed_count(), 1);
    assert_eq!(report.failed[0].0, missing.display().to_string());
    assert!(app.error().is_some(), "failure must stay loud");
    let document = lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&good)).unwrap();
    let copy = document
        .virtual_copies
        .iter()
        .find(|copy| copy.is_default)
        .unwrap();
    assert_eq!(copy.recipe.adjustments["contrast"], 0.3);
}

/// GUI-FILMSTRIP-SYNC-1 (follow-up): an empty selection is a loud no-op
/// for both actions — empty report, "No images selected" status, no
/// `preview_generation` bump, no sidecar write.
#[test]
fn empty_selection_is_noop_for_sync_and_match() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    // GUI-STARTUP-SELECTION-1 (F-100): opening the only image of a fresh
    // session auto-selects it — clear back to empty to exercise the
    // no-selection no-op below.
    app.filmstrip_selection.clear();
    app.filmstrip_anchor = None;
    app.set_adjustment("exposure", 1.0);
    assert!(app.filmstrip_selection.is_empty());
    let generation = app.preview_generation();
    let synced = app.sync_settings_to_selection();
    assert_eq!(synced.applied_count(), 0);
    assert_eq!(synced.failed_count(), 0);
    let matched = app.match_exposures_of_selection();
    assert_eq!(matched.applied_count(), 0);
    assert_eq!(matched.failed_count(), 0);
    assert_eq!(app.status(), "No images selected");
    assert_eq!(app.preview_generation(), generation);
    assert!(
        !lumina_sidecar::sidecar_path_for(&source).exists(),
        "the no-op must not write a sidecar"
    );
}

/// GUI-FILMSTRIP-SYNC-1: Match Total Exposures over the selection — the
/// darker image gains more exposure than the brighter one, both sidecars
/// carry the same median target, and each image bumps `preview_generation`.
#[test]
fn match_exposures_equalizes_selection_around_median() {
    fn solid_gray(path: &Path, level: u8) {
        let pixels = vec![level, level, level, 255, level, level, level, 255];
        let png = ImageFrame::new(2, 1, pixels)
            .unwrap()
            .encode(ImageFileFormat::Png)
            .unwrap();
        std::fs::write(path, png).unwrap();
    }
    let directory = tempfile::tempdir().unwrap();
    let dark = directory.path().join("dark.png");
    let bright = directory.path().join("bright.png");
    solid_gray(&dark, 30);
    solid_gray(&bright, 220);
    let mut app = new_app();
    open_and_decode(&mut app, dark.display().to_string());
    app.filmstrip_selection.insert(dark.display().to_string());
    app.filmstrip_selection.insert(bright.display().to_string());
    let generation = app.preview_generation();
    let report = app.match_exposures_of_selection();
    assert_eq!(report.applied_count(), 2);
    assert_eq!(report.failed_count(), 0);
    assert_eq!(app.preview_generation(), generation + 2);
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
    let dark_exposure = exposure_of(&dark);
    let bright_exposure = exposure_of(&bright);
    assert!(
        dark_exposure > bright_exposure,
        "darker image must gain more exposure (dark={dark_exposure}, bright={bright_exposure})"
    );
    // Both copies share the same median target and carry their own delta.
    let target_of = |path: &PathBuf| {
        let copy = lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(path))
            .unwrap()
            .virtual_copies
            .into_iter()
            .find(|copy| copy.is_default)
            .unwrap();
        (
            copy.recipe.auto_features.target_luminance,
            copy.recipe.auto_features.matched_exposure,
        )
    };
    let (dark_target, dark_delta) = target_of(&dark);
    let (bright_target, bright_delta) = target_of(&bright);
    assert_eq!(dark_target, bright_target);
    assert!(dark_delta.is_some() && bright_delta.is_some());
    assert!(
        dark_delta.unwrap() > bright_delta.unwrap(),
        "Core delta must favour the darker image"
    );
    // Reload anchor: reopening a matched image restores its exposure.
    let mut reopened = new_app();
    open_and_decode(&mut reopened, dark.display().to_string());
    assert_eq!(reopened.recipe().adjustments["exposure"], dark_exposure);
}

/// GUI-FILMSTRIP-SYNC-1 (follow-up): an undecodable match target is a loud
/// per-image entry and never aborts the remaining targets.
#[test]
fn match_exposures_reports_per_image_failure_without_aborting_rest() {
    let directory = tempfile::tempdir().unwrap();
    let good = directory.path().join("good.png");
    save_png(&good);
    let missing = directory.path().join("gone.png");
    let mut app = new_app();
    open_and_decode(&mut app, good.display().to_string());
    app.filmstrip_selection.insert(good.display().to_string());
    app.filmstrip_selection
        .insert(missing.display().to_string());
    let report = app.match_exposures_of_selection();
    assert_eq!(report.applied_count(), 1);
    assert_eq!(report.failed_count(), 1);
    assert_eq!(report.failed[0].0, missing.display().to_string());
    assert!(app.error().is_some(), "failure must stay loud");
    assert!(
        lumina_sidecar::sidecar_path_for(&good).is_file(),
        "the decodable target must still be written"
    );
}

/// LRPAR-G08-PREVIOUS: edit → save A, open B — A becomes the Previous
/// reference; applying it writes B's sidecar (one `previous-0` history
/// step), adopts it in memory, and a reload restores it (DoD
/// End-to-End-Kette Edit → Commit → Datei → Reload).
#[test]
fn previous_applies_last_edited_recipe_to_current_image() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first.png");
    let second = directory.path().join("second.png");
    let third = directory.path().join("third.png");
    save_png(&first);
    save_png(&second);
    save_png(&third);
    let mut app = new_app();
    open_and_decode(&mut app, first.display().to_string());
    app.set_adjustment("exposure", 1.5);
    app.save_sidecar();
    open_and_decode_switch(&mut app, &second.display().to_string());
    assert_eq!(
        app.previous_source_path(),
        Some(first.to_str().unwrap()),
        "the displaced image is the Previous reference"
    );
    // Focus the selection on the loaded target plus one file-only target
    // (no sidecar yet — covers both the in-memory and the file path).
    app.filmstrip_selection.clear();
    app.filmstrip_selection.insert(second.display().to_string());
    app.filmstrip_selection.insert(third.display().to_string());
    let generation = app.preview_generation();
    let report = app.apply_previous_to_selection();
    assert_eq!(report.applied_count(), 2);
    assert_eq!(report.failed_count(), 0);
    assert_eq!(app.preview_generation(), generation + 2);
    // In-memory adopt: the visible recipe matches immediately.
    assert_eq!(app.recipe().adjustments["exposure"], 1.5);
    // File anchor: sidecars written with exactly one history step each,
    // carrying the portable reference file name (never a path).
    for (target, expected_id) in [(&second, "previous-0"), (&third, "previous-1")] {
        let document =
            lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(target)).unwrap();
        let copy = document
            .virtual_copies
            .iter()
            .find(|copy| copy.is_default)
            .unwrap();
        assert_eq!(copy.recipe.adjustments["exposure"], 1.5);
        let last = copy.history.last().unwrap();
        assert_eq!(last.id, expected_id);
        assert_eq!(
            last.extras["step"],
            serde_json::Value::String("previous".into())
        );
        let source = last.extras["source"].as_str().unwrap();
        assert_eq!(source, "first.png");
        assert!(!source.contains('/'));
    }
    // Reload anchor: reopening the target restores the recipe.
    let mut reopened = new_app();
    open_and_decode(&mut reopened, second.display().to_string());
    assert_eq!(reopened.recipe().adjustments["exposure"], 1.5);
}

/// LRPAR-G08-PREVIOUS: the reference tracks the last displaced image —
/// A → B → C leaves B as reference, and a same-path reload never
/// clobbers it.
#[test]
fn previous_reference_tracks_the_last_displaced_image() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("a.png");
    let second = directory.path().join("b.png");
    let third = directory.path().join("c.png");
    for source in [&first, &second, &third] {
        save_png(source);
    }
    let mut app = new_app();
    open_and_decode(&mut app, first.display().to_string());
    app.set_adjustment("exposure", 0.5);
    app.save_sidecar();
    open_and_decode_switch(&mut app, &second.display().to_string());
    assert_eq!(app.previous_source_path(), Some(first.to_str().unwrap()));
    app.set_adjustment("exposure", 2.0);
    app.save_sidecar();
    open_and_decode_switch(&mut app, &third.display().to_string());
    assert_eq!(app.previous_source_path(), Some(second.to_str().unwrap()));
    // Same-path reload keeps the reference.
    open_and_decode_switch(&mut app, &third.display().to_string());
    assert_eq!(app.previous_source_path(), Some(second.to_str().unwrap()));
}

/// LRPAR-G08-PREVIOUS: without a previously edited image the action is
/// a loud no-op (empty report + visible error, no sidecar write) — never
/// a silent default-apply.
#[test]
fn previous_without_reference_is_loud_noop() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    assert!(app.previous_source_path().is_none());
    let generation = app.preview_generation();
    let report = app.apply_previous_to_selection();
    assert_eq!(report.applied_count(), 0);
    assert_eq!(report.failed_count(), 0);
    assert!(app.error().is_some(), "missing reference must stay loud");
    assert_eq!(app.preview_generation(), generation);
    assert!(
        !lumina_sidecar::sidecar_path_for(&source).exists(),
        "the no-op must not write a sidecar"
    );
}

/// LRPAR-G08-PREVIOUS: one unreadable target is a loud per-image entry
/// and never aborts the remaining targets.
#[test]
fn previous_reports_per_image_failure_without_aborting_rest() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first.png");
    let good = directory.path().join("good.png");
    save_png(&first);
    save_png(&good);
    let missing = directory.path().join("gone.png");
    let mut app = new_app();
    open_and_decode(&mut app, first.display().to_string());
    app.set_adjustment("contrast", 0.3);
    app.save_sidecar();
    open_and_decode_switch(&mut app, &good.display().to_string());
    app.filmstrip_selection.clear();
    app.filmstrip_selection.insert(good.display().to_string());
    app.filmstrip_selection
        .insert(missing.display().to_string());
    let report = app.apply_previous_to_selection();
    assert_eq!(report.applied_count(), 1);
    assert_eq!(report.failed_count(), 1);
    assert_eq!(report.failed[0].0, missing.display().to_string());
    assert!(app.error().is_some(), "failure must stay loud");
    let document = lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&good)).unwrap();
    let copy = document
        .virtual_copies
        .iter()
        .find(|copy| copy.is_default)
        .unwrap();
    assert_eq!(copy.recipe.adjustments["contrast"], 0.3);
    // History id carries the per-target counter (`gone` sorts before
    // `good`, so the healthy target is index 1 here).
    assert!(
        copy.history.last().unwrap().id.starts_with("previous-"),
        "expected a previous history step, got {:?}",
        copy.history.last().unwrap().id
    );
}

/// GUI-FILMSTRIP-SYNC-1: the selection actions paint headless (no GPU) so
/// a missing button fails `cargo test -p lumina-gui --lib` instead of
/// only a visual review.
#[test]
fn filmstrip_selection_actions_are_visible() {
    let mut app = new_app();
    let ctx = egui::Context::default();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1024.0, 720.0),
        )),
        ..Default::default()
    };
    let mut output = ctx.run_ui(raw, |ui| {
        egui::CentralPanel::default().show(ui, |ui| app.draw_filmstrip(&ctx, ui));
    });
    output.textures_delta.clear();
    assert_fully_visible(&output.shapes, Str::SyncSettings.t());
    assert_fully_visible(&output.shapes, Str::MatchSelection.t());
    assert_fully_visible(&output.shapes, Str::PreviousImage.t());
}
