//! R5-DUST-23-FOLLOWUP: headless regression tests for spot selection +
//! per-spot editing (select/update/remove), detail-only-when-selected and the
//! no-Clone-fallback contract (SOLL § R5-DUST-23-FOLLOWUP).
//!
//! Every test drives the production entry points end to end
//! (edit → sidecar file → fresh reopen → value restored, DoD §1); the
//! original file bytes are asserted untouched throughout.

use super::*;
use crate::spot_select::{spot_display_status, spot_type_label};

/// Two-spot app on a real sidecar file: spots heal visibly on the dark-block
/// fixture (`open_and_decode` + `commit_spot_heal` persist + render).
pub(super) fn two_spot_app() -> (tempfile::TempDir, LuminaApp, String) {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("followup.png");
    std::fs::write(&source, dark_block_png()).unwrap();
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.commit_spot_heal(
        lumina_sidecar::Point2 { x: 0.2, y: 0.2 },
        2.0,
        0.0,
        lumina_sidecar::Point2 { x: 0.1, y: 0.0 },
        1.0,
    )
    .unwrap();
    app.commit_spot_heal(
        lumina_sidecar::Point2 { x: 0.7, y: 0.7 },
        3.0,
        0.2,
        lumina_sidecar::Point2 { x: -0.1, y: 0.0 },
        0.8,
    )
    .unwrap();
    assert_eq!(app.spot_entries().len(), 2);
    let path = source.display().to_string();
    (directory, app, path)
}

pub(super) fn spot_ids(app: &LuminaApp) -> Vec<String> {
    app.spot_entries()
        .iter()
        .map(|entry| {
            entry
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string()
        })
        .collect()
}

#[test]
fn followup_type_and_status_labels_follow_the_soll() {
    // Heal vs. Generate (AI); no `Clone` label anywhere near the spot UI.
    assert_eq!(spot_type_label("heuristic"), "Heal");
    assert_eq!(spot_type_label("generative"), "Generate (AI)");
    assert_eq!(spot_type_label(""), "Heal");
    let heuristic = serde_json::json!({"id": "h", "mode": "heuristic", "status": "valid"});
    assert_eq!(spot_display_status(&heuristic), "valid");
    let stale = serde_json::json!({"id": "h", "mode": "heuristic", "status": "stale"});
    assert_eq!(spot_display_status(&stale), "stale");
    // A generative entry without an artifact reads `missing` even when its
    // stored status still claims `valid` — it stays visible, never rendered.
    let gen_bare =
        serde_json::json!({"id": "g", "mode": "generative", "status": "valid", "prompt": "x"});
    assert_eq!(spot_display_status(&gen_bare), "missing");
    let gen_linked = serde_json::json!({"id": "g", "mode": "generative", "status": "valid",
        "artifact": {"path": "x.lumina.zdata"}});
    assert_eq!(spot_display_status(&gen_linked), "valid");
}

#[test]
fn followup_typed_only_generative_entry_has_selectable_id_and_missing_status() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("typed-only.png");
    std::fs::write(&source, dark_block_png()).unwrap();
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.recipe.spot_removals.push(lumina_sidecar::SpotRemoval {
        id: "typed-generative-1".into(),
        version: lumina_sidecar::SPOT_REMOVAL_VERSION,
        mode: lumina_sidecar::SpotRemovalMode::Generative,
        artifact: None,
    });
    let entries = app.spot_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["id"], "typed-generative-1");
    assert_eq!(app.spot_status_text(&entries[0]), "missing");
    app.select_spot("typed-generative-1").unwrap();
    assert_eq!(app.selected_spot_id(), Some("typed-generative-1"));
    // Regeneration is addressable even without a geometry view; the shared
    // writer materializes the full extras/mirror pair for the next reload.
    app.set_spot_gen_inputs("remove dust".into(), 7, 2);
    app.regenerate_spot_variant("typed-generative-1").unwrap();
    let persisted = app.spot_entries();
    assert_eq!(
        persisted[0]["seed"],
        lumina_core::generative_variant_seed(7, 2)
    );
    assert_eq!(persisted[0]["id"], "typed-generative-1");
}

#[test]
fn followup_duplicate_ids_fail_closed_for_selection_and_mutations() {
    let (_directory, mut app, _path) = two_spot_app();
    let duplicate = serde_json::json!([
        {"id":"same","version":1,"mode":"heuristic","center_x":0.2,"center_y":0.2,
         "radius":2.0,"offset_dx":0.0,"offset_dy":0.0,"status":"valid"},
        {"id":"same","version":1,"mode":"heuristic","center_x":0.7,"center_y":0.7,
         "radius":2.0,"offset_dx":0.0,"offset_dy":0.0,"status":"valid"}
    ]);
    app.recipe.extras.insert("spot_removals".into(), duplicate);
    app.recipe.spot_removals.clear();
    let before = serde_json::to_string(&app.spot_entries()).unwrap();
    assert!(app.select_spot("same").is_err());
    assert!(app
        .update_spot_heal(
            "same",
            3.0,
            0.0,
            1.0,
            lumina_sidecar::Point2 { x: 0.0, y: 0.0 }
        )
        .is_err());
    assert!(app.remove_spot("same").is_err());
    app.set_spot_gen_inputs("x".into(), 1, 1);
    assert!(app.regenerate_spot_variant("same").is_err());
    assert_eq!(serde_json::to_string(&app.spot_entries()).unwrap(), before);
}

#[test]
fn followup_select_resolves_and_rejects_loudly() {
    let (_directory, mut app, _path) = two_spot_app();
    let ids = spot_ids(&app);
    // A fresh dab selects itself (last commit wins).
    assert_eq!(app.selected_spot_id(), Some(ids[1].as_str()));
    app.select_spot(&ids[0]).unwrap();
    assert_eq!(app.selected_spot_id(), Some(ids[0].as_str()));
    assert_eq!(
        app.selected_spot_entry()
            .and_then(|entry| entry.get("id").and_then(|v| v.as_str()).map(str::to_string)),
        Some(ids[0].clone()),
    );
    // The selected pin paints accent-filled, like the selected mask pin
    // (pins need the Always gate headless: Auto only paints while armed).
    app.set_pin_visibility(PinVisibility::Always);
    let pins = app.visible_edit_pins();
    assert_eq!(pins.len(), 2);
    assert!(pins.iter().any(|pin| pin.selected));
    // Unknown ids fail loudly and keep the current selection.
    assert!(app.select_spot("spot-missing").is_err());
    assert_eq!(app.selected_spot_id(), Some(ids[0].as_str()));
    assert!(app.select_spot("").is_err());
    assert_eq!(app.selected_spot_id(), Some(ids[0].as_str()));
}

#[test]
fn followup_selection_resets_on_copy_switch_and_reopen() {
    let (directory, mut app, path) = two_spot_app();
    let ids = spot_ids(&app);
    app.select_spot(&ids[0]).unwrap();
    app.duplicate_virtual_copy("vc-2", "Copy 2").unwrap();
    app.select_virtual_copy("vc-2").unwrap();
    assert_eq!(app.selected_spot_id(), None);
    assert!(app.selected_spot_entry().is_none());
    // Reopen restores the removals, never the (session-only) selection.
    let mut reopened = new_app();
    open_and_decode(&mut reopened, path.clone());
    assert_eq!(reopened.spot_entries().len(), 2);
    assert_eq!(reopened.selected_spot_id(), None);
    // Opening another image resets too (no cross-file leak). NOTE: the
    // shared `open_and_decode` returns at once while an image is loaded, so
    // a second open drains until the new path is adopted instead.
    let other = directory.path().join("other.png");
    std::fs::write(&other, dark_block_png()).unwrap();
    reopened.select_spot(&ids[0]).unwrap();
    reopened.open_file(other.display().to_string());
    for _ in 0..2000 {
        reopened.poll_decode();
        if reopened.path == other.display().to_string() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(reopened.path, other.display().to_string());
    assert_eq!(reopened.selected_spot_id(), None);
}

#[test]
fn followup_update_spot_edits_params_and_reloads() {
    let (_directory, mut app, path) = two_spot_app();
    let ids = spot_ids(&app);
    let original_bytes = std::fs::read(&path).unwrap();
    let key_before = app.render_key().cloned().unwrap().digest();
    let generation = app.preview_generation();
    let before_pixels = app.preview().expect("preview after load").pixels.clone();
    app.update_spot_heal(
        &ids[0],
        5.0,
        0.4,
        0.6,
        lumina_sidecar::Point2 { x: 0.2, y: 0.1 },
    )
    .unwrap();
    // Recipe: exactly that entry changed, id/mode/status preserved.
    let entries = app.spot_entries();
    assert_eq!(entries.len(), 2);
    let edited = entries
        .iter()
        .find(|entry| entry.get("id").and_then(|v| v.as_str()) == Some(ids[0].as_str()))
        .expect("edited entry kept");
    assert_eq!(edited["radius"].as_f64(), Some(5.0));
    // NOTE: feather/opacity roundtrip through f32 JSON (`0.4` reads back as
    // `0.4000000059604645`), so the float asserts below use an epsilon.
    let approx = |entry: &serde_json::Value, key: &str, want: f64| {
        let got = entry[key].as_f64().unwrap_or(f64::NAN);
        assert!((got - want).abs() < 1e-6, "{key}: got {got}, want {want}");
    };
    approx(edited, "feather", 0.4);
    approx(edited, "opacity", 0.6);
    approx(edited, "offset_dx", 0.2);
    approx(edited, "offset_dy", 0.1);
    assert_eq!(edited["mode"].as_str(), Some("heuristic"));
    assert_eq!(edited["status"].as_str(), Some("valid"));
    let untouched = entries
        .iter()
        .find(|entry| entry.get("id").and_then(|v| v.as_str()) == Some(ids[1].as_str()))
        .expect("other entry kept");
    assert_eq!(untouched["radius"].as_f64(), Some(3.0));
    // Render leg: key changed, generation bumped, preview visibly rehealed.
    assert_ne!(app.render_key().unwrap().digest(), key_before);
    assert!(app.preview_generation() > generation);
    assert_ne!(
        before_pixels,
        app.preview().expect("preview after update").pixels.clone()
    );
    // Reload leg: the edit survived a fresh open; the original is untouched.
    let mut reopened = new_app();
    open_and_decode(&mut reopened, path.clone());
    let back = reopened
        .spot_entries()
        .into_iter()
        .find(|entry| entry.get("id").and_then(|v| v.as_str()) == Some(ids[0].as_str()))
        .expect("edited entry reloaded");
    assert_eq!(back["radius"].as_f64(), Some(5.0));
    assert!((back["opacity"].as_f64().unwrap() - 0.6).abs() < 1e-6);
    assert_eq!(std::fs::read(&path).unwrap(), original_bytes);
}

#[test]
fn followup_update_spot_rejects_loudly() {
    let (_directory, mut app, _path) = two_spot_app();
    let ids = spot_ids(&app);
    let center = lumina_sidecar::Point2 { x: 0.0, y: 0.0 };
    let before = serde_json::to_string(&app.spot_entries()).unwrap();
    // Unknown id, out-of-range and non-finite values: no state change.
    assert!(app
        .update_spot_heal("spot-missing", 5.0, 0.0, 1.0, center)
        .is_err());
    assert!(app
        .update_spot_heal(&ids[0], 0.0, 0.0, 1.0, center)
        .is_err());
    assert!(app
        .update_spot_heal(&ids[0], 513.0, 0.0, 1.0, center)
        .is_err());
    assert!(app
        .update_spot_heal(&ids[0], 5.0, 1.5, 1.0, center)
        .is_err());
    assert!(app
        .update_spot_heal(&ids[0], 5.0, 0.0, -0.5, center)
        .is_err());
    assert!(app
        .update_spot_heal(
            &ids[0],
            5.0,
            0.0,
            1.0,
            lumina_sidecar::Point2 { x: 2.0, y: 0.0 }
        )
        .is_err());
    assert!(app
        .update_spot_heal(&ids[0], f32::NAN, 0.0, 1.0, center)
        .is_err());
    assert_eq!(serde_json::to_string(&app.spot_entries()).unwrap(), before);
    // Generative entries carry no Heal geometry: editing them fails loudly
    // instead of silently rewriting them into a Clone.
    app.recipe.extras.insert(
        "spot_removals".into(),
        serde_json::json!([{"id": "g1", "version": 1, "mode": "generative", "prompt": "x"}]),
    );
    assert!(app
        .update_spot_heal("g1", 5.0, 0.0, 1.0, center)
        .unwrap_err()
        .to_string()
        .contains("not heuristic"));
}

#[test]
fn followup_remove_single_spot_and_reloads() {
    let (_directory, mut app, path) = two_spot_app();
    let ids = spot_ids(&app);
    let original_bytes = std::fs::read(&path).unwrap();
    app.select_spot(&ids[0]).unwrap();
    let generation = app.preview_generation();
    app.remove_spot(&ids[0]).unwrap();
    // Exactly that entry is gone; the selection followed.
    assert_eq!(spot_ids(&app), vec![ids[1].clone()]);
    assert_eq!(app.selected_spot_id(), None);
    assert!(app.preview_generation() > generation);
    let mut reopened = new_app();
    open_and_decode(&mut reopened, path.clone());
    assert_eq!(spot_ids(&reopened), vec![ids[1].clone()]);
    // Unknown ids fail loudly, nothing removed.
    assert!(app.remove_spot("spot-missing").is_err());
    assert_eq!(spot_ids(&app), vec![ids[1].clone()]);
    // Removing the last entry drops the whole key — done on the reopened
    // app, whose typed `spot_removals` mirror is populated from the load:
    // the mirror must clear too or the next reload fails validation.
    reopened.remove_spot(&ids[1]).unwrap();
    assert!(!reopened.recipe().extras.contains_key("spot_removals"));
    let mut reopened2 = new_app();
    open_and_decode(&mut reopened2, path.clone());
    assert!(!reopened2.recipe().extras.contains_key("spot_removals"));
    assert_eq!(std::fs::read(&path).unwrap(), original_bytes);
}

#[test]
fn followup_clear_with_stale_typed_mirror_reloads() {
    // Pre-existing `clear_spot_heals` gap (same mirror cause): a doc loaded
    // from file carries id-less typed shadows, so clearing extras alone
    // used to save an unloadable sidecar. Clearing must drop both.
    let (_directory, _, path) = two_spot_app();
    let mut reopened = new_app();
    open_and_decode(&mut reopened, path.clone());
    assert_eq!(reopened.spot_entries().len(), 2);
    assert!(
        !reopened.recipe.spot_removals.is_empty(),
        "load populates the mirror"
    );
    reopened.clear_spot_heals();
    let mut reopened2 = new_app();
    open_and_decode(&mut reopened2, path.clone());
    assert!(!reopened2.recipe().extras.contains_key("spot_removals"));
}

#[test]
fn followup_no_clone_fallback_for_generative_spots() {
    // A generative entry without an artifact must never heal via the Clone
    // path: the render rejects loudly (core `reject_unsupported_spot_modes`)
    // instead of painting a healed — or an unhealed — frame silently.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("gen.png");
    std::fs::write(&source, dark_block_png()).unwrap();
    let original_bytes = std::fs::read(&source).unwrap();
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.recipe.extras.insert(
        "spot_removals".into(),
        serde_json::json!([{"id": "g1", "version": 1, "mode": "generative", "prompt": "x"}]),
    );
    assert_eq!(
        spot_display_status(&app.spot_entries()[0]),
        "missing",
        "generative without artifact reads missing"
    );
    assert!(
        app.render().is_err(),
        "generative must fail loudly, never Clone-heal"
    );
    // Regeneration on a heuristic spot fails loudly too (never a silent
    // heuristic "regeneration" standing in for Generate).
    app.recipe.extras.insert(
        "spot_removals".into(),
        serde_json::json!([
            {"id": "g1", "version": 1, "mode": "generative", "prompt": "x"},
            {"id": "h1", "version": 1, "mode": "heuristic", "center_x": 0.5,
             "center_y": 0.5, "radius": 4.0, "offset_dx": 0.0, "offset_dy": 0.0,
             "status": "valid"},
        ]),
    );
    app.set_spot_gen_inputs("remove dust".into(), 7, 2);
    assert!(app
        .regenerate_spot_variant("h1")
        .unwrap_err()
        .to_string()
        .contains("not generative"));
    assert_eq!(std::fs::read(&source).unwrap(), original_bytes);
}
