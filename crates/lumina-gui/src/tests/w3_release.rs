//! W3 release toggles (split/fullscreen/stack/snapshot/quick develop) tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

#[test]
fn w3_split_toggle_holds_before_image() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    assert!(!app.before_after_split);
    app.toggle_split_view();
    assert!(app.before_after_split);
    // Enabling holds the Before image through the existing path.
    assert!(app.before_after);
    assert!(app.recipe().adjustments.is_empty());
    app.toggle_split_view();
    assert!(!app.before_after_split);
}

#[test]
fn w3_fullscreen_toggle_settles_zoom_on_fit() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    app.zoom_step(2.0);
    assert_eq!(app.zoom_mode, ZoomMode::Custom);
    assert!(!app.chrome_hidden());
    app.toggle_fullscreen();
    assert!(app.fullscreen);
    // Entering fullscreen settles the zoom on Fit (the previous `F`
    // role) and hides the lights-out chrome.
    assert_eq!(app.zoom_mode, ZoomMode::Fit);
    assert!(app.chrome_hidden());
    assert!(app.recipe().adjustments.is_empty());
    app.toggle_fullscreen();
    assert!(!app.fullscreen);
    assert!(!app.chrome_hidden());
}

#[test]
fn w3_stack_group_toggle_roundtrip() {
    // LR-17 light: `Cmd+G` grouping proxy via `extras["stack_group"]`
    // (no schema change), persisted and restored across reopen.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    assert_eq!(app.stack_group_id(), None);
    let id = app
        .toggle_stack_group()
        .unwrap()
        .expect("first toggle groups");
    assert!(id.starts_with("stack-"));
    assert_eq!(app.stack_group_id(), Some(id.clone()));
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert_eq!(
        stack_id_of(&document.virtual_copies[0].extras),
        Some(id.clone())
    );
    let mut reopened = new_app();
    open_and_decode(&mut reopened, source.display().to_string());
    assert_eq!(reopened.stack_group_id(), Some(id));
    // A second press ungroups again and persists the removal.
    assert_eq!(reopened.toggle_stack_group().unwrap(), None);
    assert_eq!(reopened.stack_group_id(), None);
    // Tolerant read: missing, non-string or empty values are `None`.
    assert_eq!(stack_id_of(&BTreeMap::new()), None);
    let numeric = BTreeMap::from([("stack_group".to_string(), serde_json::Value::from(7))]);
    assert_eq!(stack_id_of(&numeric), None);
    let empty = BTreeMap::from([("stack_group".to_string(), serde_json::Value::from(""))]);
    assert_eq!(stack_id_of(&empty), None);
}

#[test]
fn w3_snapshot_freeze_list_restore() {
    // LR-12 light: named history freeze (extras marker), list, restore.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    assert!(app.snapshots().is_empty());
    assert!(app.create_snapshot("").is_err());
    assert!(app.create_snapshot("   ").is_err());
    app.set_adjustment("exposure", 1.5);
    let id = app.create_snapshot("grade-a").unwrap();
    assert!(id.starts_with("snapshot-"));
    assert_eq!(app.snapshots(), vec![(id.clone(), "grade-a".to_string())]);
    // The frozen recipe survives later edits and restores exactly.
    app.set_adjustment("exposure", -2.0);
    assert_eq!(app.recipe().adjustments["exposure"], -2.0);
    app.restore_snapshot(&id).unwrap();
    assert_eq!(app.recipe().adjustments["exposure"], 1.5);
    // Unknown ids and plain history fail loudly, never silently.
    assert!(app.restore_snapshot("nope").is_err());
    // Naming fallback (tolerant): an entry with a `snapshot-<n>` id but
    // no marker still restores; the marker is what lists it by name.
    app.active_copy_mut().unwrap().history.push(HistoryEntry {
        id: "snapshot-legacy".into(),
        recipe: EditRecipe::default(),
        recorded_at: None,
        extras: BTreeMap::new(),
    });
    app.restore_snapshot("snapshot-legacy").unwrap();
    assert!(app.recipe().adjustments.is_empty());
}

#[test]
fn w3_quick_develop_applies_saves_and_renders() {
    // LR-13 light: Quick Develop through the save/render path.
    let mut bare = new_app();
    assert!(bare.apply_quick_develop("exposure", 1.0).is_err());
    bare.load_bytes(png(), "test.png").unwrap();
    // Path-less (byte-drop) sessions and unknown keys fail loudly.
    assert!(bare.apply_quick_develop("exposure", 1.0).is_err());
    assert!(bare.apply_quick_develop("bogus", 1.0).is_err());
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let generation = app.preview_generation();
    app.apply_quick_develop("exposure", 2.0).unwrap();
    assert_eq!(app.recipe().adjustments["exposure"], 2.0);
    assert!(app.preview_generation() > generation);
    // The value reached the persisted sidecar copy, not just the session.
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    assert_eq!(
        document.virtual_copies[0].recipe.adjustments["exposure"],
        2.0
    );
    for key in ["contrast", "highlights", "shadows"] {
        app.apply_quick_develop(key, 0.5).unwrap();
        assert_eq!(app.recipe().adjustments[key], 0.5);
    }
}
