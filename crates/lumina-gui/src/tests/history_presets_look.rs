//! UX-LOOK-HISTORY-18: headless tests for the readable/clickable history
//! entries and the presets group tree (relative-folder grouping).

use super::*;
use lumina_sidecar::{HistoryChange, Preset};
use std::collections::BTreeMap;

/// A history step must render `name from → to — time` and stay clickable
/// (restore swaps the session recipe, non-destructively).
#[test]
fn history_entry_is_readable_and_restores_on_click() {
    let (_directory, mut app) = persistent_app();
    let mut stored = EditRecipe::default();
    stored.adjustments.insert("exposure".into(), 1.5);
    {
        let document = app.document.as_mut().unwrap();
        let copy = document
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == app.virtual_copy_id)
            .unwrap();
        let mut entry = HistoryEntry {
            id: "history-read-1".into(),
            recipe: stored,
            recorded_at: Some("2026-09-19T12:34:56Z".into()),
            extras: BTreeMap::new(),
        };
        entry
            .set_changes(vec![HistoryChange {
                parameter: "exposure".into(),
                from: "0".into(),
                to: "1.5".into(),
            }])
            .unwrap();
        copy.history.push(entry);
    }

    let label = "1. exposure 0 → 1.5 — 2026-09-19T12:34:56Z";
    let shapes = headless_click_labels(&mut app, &[Str::History.t(), label], |app, ui| {
        app.draw_history_section(ui)
    });
    assert!(
        text_contains(&shapes, label),
        "the readable entry must remain painted: {:?}",
        painted_texts(&shapes)
    );
    assert_eq!(
        app.recipe().adjustments["exposure"],
        1.5,
        "clicking the readable entry must restore its recipe"
    );
    assert_eq!(app.history_selected.as_deref(), Some("history-read-1"));
}

/// A legacy entry without structured changes stays visible via its stored
/// action label (nothing is invented) and is still clickable.
#[test]
fn legacy_history_entry_falls_back_to_action_label() {
    let (_directory, mut app) = persistent_app();
    {
        let document = app.document.as_mut().unwrap();
        let copy = document
            .virtual_copies
            .iter_mut()
            .find(|copy| copy.id == app.virtual_copy_id)
            .unwrap();
        let mut extras = BTreeMap::new();
        extras.insert("action".into(), Value::String("geometry.rotation".into()));
        copy.history.push(HistoryEntry {
            id: "geometry-1".into(),
            recipe: EditRecipe::default(),
            recorded_at: None,
            extras,
        });
    }
    let shapes = headless_click_label(&mut app, Str::History.t(), |app, ui| {
        app.draw_history_section(ui)
    });
    assert!(
        text_contains(&shapes, "1. geometry.rotation"),
        "legacy entries must stay readable: {:?}",
        painted_texts(&shapes)
    );
}

/// Presets in a relative sub-folder paint a group header; clicking the leaf
/// applies the preset exactly like the flat list did.
#[test]
fn preset_tree_groups_and_applies() {
    let directory = tempfile::tempdir().unwrap();
    let presets = directory.path().join("presets");
    let group = presets.join("Landscape");
    std::fs::create_dir_all(&group).unwrap();
    let mut recipe = EditRecipe::default();
    recipe.adjustments.insert("exposure".into(), 0.75);
    recipe
        .options
        .insert("exposure_semantics".into(), "absolute".into());
    let preset = Preset {
        id: "preset-warm".into(),
        name: "Warm".into(),
        recipe,
        extras: BTreeMap::new(),
    };
    presets::save_preset_file(&group, &preset, true).unwrap();

    let mut app = new_app();
    app.presets_dir = Some(presets);
    app.reload_preset_entries();
    assert_eq!(app.preset_entries.len(), 1);

    let shapes = headless_click_labels(&mut app, &[Str::PresetsSection.t(), "Warm"], |app, ui| {
        app.draw_presets_section(ui)
    });
    assert!(
        text_contains(&shapes, "Landscape"),
        "the relative folder must paint as a group header: {:?}",
        painted_texts(&shapes)
    );
    assert_eq!(
        app.recipe().adjustments["exposure"],
        0.75,
        "clicking the grouped leaf must apply the preset"
    );
    assert_eq!(app.status, Str::PresetApplied.format_arg("Warm"));
}

/// The static selection batch path persists the same structured step (name +
/// old→new + time) into the sidecar file — not just the in-memory GUI paths.
#[test]
fn selection_batch_adjustment_writes_structured_history_entry() {
    let (directory, mut app) = persistent_app();
    let source = directory.path().join("photo.png");
    app.save_sidecar();
    let changed =
        LuminaApp::apply_adjustment_to_selection(std::slice::from_ref(&source), "exposure", 0.6)
            .expect("batch adjustment");
    assert_eq!(changed, 1);

    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).unwrap();
    let entry = document.virtual_copies[0].history.last().unwrap();
    assert_eq!(
        entry.changes().unwrap(),
        vec![HistoryChange {
            parameter: "exposure".into(),
            from: String::new(),
            to: "0.6".into(),
        }]
    );
    assert!(
        entry.recorded_at.is_some(),
        "every persisted step carries a timestamp"
    );
}
