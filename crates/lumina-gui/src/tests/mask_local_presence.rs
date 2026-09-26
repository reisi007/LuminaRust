//! MASK-LOCAL-P1.2c headless GUI tests for the mask-local presence block:
//! setters, resets, refusals, render identity, stand-in routing and GUI/CLI
//! parity on the same persisted typed block.

use super::mask_local::local_app;
use super::*;

/// Exact comparison for a value that travelled through an `f32` block.
fn close(actual: f64, expected: f64) -> bool {
    (actual - f64::from(expected as f32)).abs() < 1e-6
}

/// The same comparison for a raw `f32` presence amount.
fn close_f32(actual: f32, expected: f64) -> bool {
    close(f64::from(actual), expected)
}

/// Read the selected layer's stored local recipe.
fn local(app: &LuminaApp) -> lumina_sidecar::LocalAdjustments {
    app.active_mask_layers_snapshot().unwrap()[0]
        .effective_local_adjustments()
        .unwrap()
        .expect("typed local recipe")
}

/// A local presence edit changes the render identity and never touches the
/// global recipe.
#[test]
fn a_local_presence_edit_changes_the_render_identity_and_never_the_global_recipe() {
    let (_directory, mut app, source) = local_app();
    let global_before = app.recipe().clone();
    let initial = app.active_mask_layers_snapshot().unwrap();
    assert!(!app.has_mask_local_presence().unwrap());
    assert!(!app.has_visible_local_adjustments());

    app.set_mask_local_presence("texture", 0.5).unwrap();

    // No global mutation: not `EditRecipe::presence`, not the global
    // `adjustments["presence"]` key.
    assert_eq!(app.recipe(), &global_before);
    assert!(app.recipe().presence.is_none());
    assert!(!app.recipe().adjustments.contains_key("presence"));

    let stored = local(&app);
    assert!(close_f32(stored.presence.as_ref().unwrap().texture, 0.5));
    assert!(stored.has_local_presence());
    assert!(!stored.is_neutral());
    assert!(app.has_mask_local_presence().unwrap());

    // Same transaction contract as every other local edit.
    assert_eq!(
        app.pending_mask_state_before.as_deref(),
        Some(initial.as_slice())
    );
    assert_eq!(
        app.pending_slider_commit.as_ref().map(|(k, _)| k.as_str()),
        Some("mask.local.presence.texture")
    );

    // The block survives a real save/reload, and history carries the complete
    // pre-edit layer snapshot.
    app.save_sidecar();
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).expect("reload");
    let persisted = document.virtual_copies[0].mask_layers[0]
        .effective_local_adjustments()
        .unwrap()
        .expect("typed local recipe");
    assert!(close_f32(persisted.presence.as_ref().unwrap().texture, 0.5));
    // MASK-LOCAL-P1.2d raised the current version to 6; the presence gate stays
    // anchored at 5, so this document still owns its own presence block.
    assert_eq!(persisted.version, lumina_sidecar::LOCAL_ADJUSTMENTS_VERSION);
    assert_eq!(persisted.version, 6);
    let entry = app.document.as_ref().unwrap().virtual_copies[0]
        .history
        .last()
        .unwrap()
        .clone();
    assert!(entry.mask_state().unwrap().unwrap().layers[0]
        .local_adjustments
        .is_none());
}

/// All three presence amounts are settable and readable, and a single reset
/// clears the whole block.
#[test]
fn local_presence_setters_and_reset_cover_every_field() {
    let (_directory, mut app, _source) = local_app();
    assert_eq!(
        app.selected_mask_local_presence().unwrap(),
        (0.0, 0.0, 0.0),
        "an unedited layer reads the neutral triple"
    );
    app.set_mask_local_presence("texture", 0.5).unwrap();
    app.set_mask_local_presence("clarity", -0.25).unwrap();
    app.set_mask_local_presence("dehaze", 0.75).unwrap();
    let (texture, clarity, dehaze) = app.selected_mask_local_presence().unwrap();
    assert!(close(texture, 0.5) && close(clarity, -0.25) && close(dehaze, 0.75));
    assert_eq!(local(&app).presence_summary(), "texture+clarity+dehaze");

    // Typing one amount back to zero keeps the rest of the block.
    app.set_mask_local_presence("clarity", 0.0).unwrap();
    let (texture, clarity, dehaze) = app.selected_mask_local_presence().unwrap();
    assert!(close(texture, 0.5) && close(clarity, 0.0) && close(dehaze, 0.75));
    assert!(local(&app).has_local_presence());

    // The single reset clears everything.
    app.reset_mask_local_presence().unwrap();
    assert_eq!(app.selected_mask_local_presence().unwrap(), (0.0, 0.0, 0.0));
    assert!(local(&app).presence.is_none());
    assert!(!app.has_mask_local_presence().unwrap());
    assert!(local(&app).is_neutral());
}

/// Invalid presence values are refused without mutating the layer or the
/// pending history snapshot, and an unknown field is loud too.
#[test]
fn invalid_local_presence_edits_are_refused_without_mutating() {
    let (_directory, mut app, _source) = local_app();
    app.set_mask_local_presence("texture", 0.5).unwrap();
    let before = app.active_mask_layers_snapshot().unwrap();
    let pending = app.pending_mask_state_before.clone();

    for (field, value) in [
        ("texture", 1.5),
        ("texture", -1.000_001),
        ("clarity", 2.0),
        ("dehaze", -2.0),
        ("dehaze", f64::NAN),
        ("grain", 0.5),
        ("", 0.5),
    ] {
        let error = app
            .set_mask_local_presence(field, value)
            .expect_err("an out-of-range or unknown field must be refused");
        let message = error.to_string();
        assert!(!message.is_empty(), "{field}={value}");
        assert_eq!(
            app.active_mask_layers_snapshot().unwrap(),
            before,
            "{field}={value} must not mutate the layer"
        );
        assert_eq!(
            app.pending_mask_state_before.clone(),
            pending,
            "{field}={value} must not re-arm the history snapshot"
        );
    }
    assert!(close_f32(
        local(&app).presence.as_ref().unwrap().texture,
        0.5
    ));
}

/// The still-disabled local stages stay unreachable, and a presence-only layer
/// is refused by every stand-in route just like every other local adjustment.
#[test]
fn a_presence_only_local_layer_refuses_the_stand_in_routes() {
    let (_directory, mut app, _source) = local_app();
    assert!(!app.has_visible_local_adjustments());
    assert!(app.local_adjustment_route_reason().is_none());

    app.set_mask_local_presence("dehaze", 0.5).unwrap();
    assert!(app.has_visible_local_adjustments());
    let reason = app
        .local_adjustment_route_reason()
        .expect("a presence-only layer must refuse the stand-in routes");
    assert!(reason.contains("local mask adjustments"), "{reason}");
    assert!(reason.contains("CPU"), "{reason}");

    app.reset_mask_local_presence().unwrap();
    assert!(!app.has_visible_local_adjustments());
    assert!(app.local_adjustment_route_reason().is_none());

    // Detail, sharpening, noise reduction, AI-denoise and optics have no
    // setter, no field and no local key: they stay disabled.
    let json = serde_json::to_value(lumina_sidecar::LocalAdjustments::default()).unwrap();
    let keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
    for disabled in [
        "detail",
        "sharpening",
        "noise_reduction",
        "denoise_ai",
        "optics",
        "lens_correction",
    ] {
        assert!(
            !keys.iter().any(|key| key == disabled),
            "the local recipe must not carry a `{disabled}` field"
        );
        let mut recipe = lumina_sidecar::LocalAdjustments::default();
        assert!(recipe.set_value(disabled, 0.1).is_err());
    }
}

/// The presence block is part of the local state digest, so a presence edit
/// must invalidate the mask and render identities.
#[test]
fn local_presence_state_digest_invalidates_mask_and_render() {
    let (_directory, mut app, _source) = local_app();
    // A never-edited layer stores no typed object at all, so its baseline is
    // the default recipe's digest.
    let none = lumina_sidecar::LocalAdjustments::default().digest();
    app.set_mask_local_presence("texture", 0.5).unwrap();
    let textured = local(&app).digest();
    assert_ne!(none, textured);
    app.set_mask_local_presence("texture", -0.5).unwrap();
    let negative = local(&app).digest();
    assert_ne!(textured, negative, "the signed amount is part of identity");
    app.reset_mask_local_presence().unwrap();
    // The whole block is gone, and the layer is back to the exact typed identity
    // of "never edited" (the same contract every other local reset has).
    assert!(local(&app).presence.is_none());
    assert_eq!(
        local(&app).digest(),
        none,
        "a reset returns to the identity of `none`"
    );
    // And the mask-layer digest follows it.
    let layers = app.active_mask_layers_snapshot().unwrap();
    let plain = lumina_sidecar::mask_layers_digest(&layers);
    app.set_mask_local_presence("clarity", 0.5).unwrap();
    assert_ne!(
        plain,
        lumina_sidecar::mask_layers_digest(&app.active_mask_layers_snapshot().unwrap())
    );
}

/// The GUI and the CLI write the *same* persisted typed block, through the same
/// setter API, so the two surfaces cannot drift.
#[test]
fn gui_and_cli_local_presence_share_the_same_persisted_typed_block() {
    let (_directory, mut app, source) = local_app();
    app.set_mask_local_presence("texture", 0.5).unwrap();
    app.set_mask_local_presence("clarity", -0.25).unwrap();
    app.set_mask_local_presence("dehaze", 0.75).unwrap();
    app.save_sidecar();
    let from_gui = local(&app);

    // Exactly what the CLI channel `--set-local-adjustment presence.<field>=<n>`
    // applies, through the very same sidecar setters.
    let mut from_cli = lumina_sidecar::LocalAdjustments::default();
    from_cli.set_local_presence_field("texture", 0.5).unwrap();
    from_cli.set_local_presence_field("clarity", -0.25).unwrap();
    from_cli.set_local_presence_field("dehaze", 0.75).unwrap();

    assert_eq!(from_gui, from_cli);
    assert_eq!(from_gui.digest(), from_cli.digest());
    assert_eq!(
        serde_json::to_string(&from_gui).expect("serializable"),
        serde_json::to_string(&from_cli).expect("serializable")
    );

    // And the reloaded file really carries the GUI block.
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).expect("reload");
    let reloaded = document.virtual_copies[0].mask_layers[0]
        .effective_local_adjustments()
        .unwrap()
        .expect("typed local recipe");
    assert_eq!(reloaded, from_gui);
}

/// The presence block must survive History / Previous of the current copy as a
/// complete additive layer snapshot.
#[test]
fn local_presence_survives_history_and_previous() {
    let (_directory, mut app, _source) = local_app();
    app.set_mask_local_presence("texture", 0.5).unwrap();
    app.save_sidecar();
    let with_presence = app.active_mask_layers_snapshot().unwrap();

    // The second edit closes its own transaction, whose *pre-edit* snapshot is
    // the presence state; restoring that entry must bring it back.
    app.set_mask_local_presence("texture", 0.0).unwrap();
    app.save_sidecar();
    assert!(!app.has_mask_local_presence().unwrap());
    let entry = app.document.as_ref().unwrap().virtual_copies[0]
        .history
        .last()
        .unwrap()
        .id
        .clone();

    app.restore_history(&entry).unwrap();
    assert_eq!(app.active_mask_layers_snapshot().unwrap(), with_presence);
    let (texture, clarity, dehaze) = app.selected_mask_local_presence().unwrap();
    assert!(close(texture, 0.5) && close(clarity, 0.0) && close(dehaze, 0.0));
}

/// The editor paints the presence block and its reset, and painting is display
/// state only.
#[test]
fn the_local_presence_editor_paints_and_writes_only_the_mask_layer() {
    let (_directory, mut app, _source) = local_app();
    let global_before = app.recipe().clone();
    let ctx = egui::Context::default();
    let mut drawn = 0usize;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 4096.0),
            )),
            time: Some(0.1),
            ..Default::default()
        },
        |ui| {
            app.draw_mask_local_presence(ui);
            drawn += 1;
        },
    );
    // No GPU renderer consumes the per-frame texture deltas headless.
    output.textures_delta.clear();
    assert_eq!(drawn, 1);
    // Painting is display state only.
    assert_eq!(app.recipe(), &global_before);
    assert!(!app.has_mask_local_presence().unwrap());

    // Drawing again after an edit shows the stored state and still does not
    // touch the global recipe.
    app.set_mask_local_presence("dehaze", 0.3).unwrap();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 4096.0),
            )),
            time: Some(0.2),
            ..Default::default()
        },
        |ui| app.draw_mask_local_presence(ui),
    );
    output.textures_delta.clear();
    assert_eq!(app.recipe(), &global_before);
    let (texture, clarity, dehaze) = app.selected_mask_local_presence().unwrap();
    assert!(close(texture, 0.0) && close(clarity, 0.0) && close(dehaze, 0.3));
}
