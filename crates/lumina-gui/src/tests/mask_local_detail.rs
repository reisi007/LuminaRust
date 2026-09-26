//! MASK-LOCAL-P1.2d headless GUI tests for the mask-local detail block:
//! setters, per-area and whole-block resets, refusals, render identity,
//! stand-in routing and GUI/CLI parity on the same persisted typed block.

use super::mask_local::local_app;
use super::*;

/// Exact comparison for a value that travelled through an `f32` block.
fn close_f32(actual: f32, expected: f64) -> bool {
    (f64::from(actual) - f64::from(expected as f32)).abs() < 1e-6
}

/// Read the selected layer's stored local recipe.
fn local(app: &LuminaApp) -> lumina_sidecar::LocalAdjustments {
    app.active_mask_layers_snapshot().unwrap()[0]
        .effective_local_adjustments()
        .unwrap()
        .expect("typed local recipe")
}

/// A local detail edit changes the render identity and never touches the global
/// recipe.
#[test]
fn a_local_detail_edit_changes_the_render_identity_and_never_the_global_recipe() {
    let (_directory, mut app, source) = local_app();
    let global_before = app.recipe().clone();
    let initial = app.active_mask_layers_snapshot().unwrap();
    assert!(!app.has_mask_local_sharpening().unwrap());
    assert!(!app.has_mask_local_noise_reduction().unwrap());
    assert!(!app.has_visible_local_adjustments());

    app.set_mask_local_sharpening("amount", 0.75).unwrap();

    // No global mutation: not `EditRecipe::sharpening`, not
    // `EditRecipe::noise_reduction`, and no global `adjustments` key either.
    assert_eq!(app.recipe(), &global_before);
    assert!(app.recipe().sharpening.is_none());
    assert!(app.recipe().noise_reduction.is_none());
    for forbidden in [
        "sharpening",
        "noise_reduction",
        "amount",
        "radius",
        "detail",
        "masking",
        "luminance",
        "color",
    ] {
        assert!(
            !app.recipe().adjustments.contains_key(forbidden),
            "the global adjustments must not carry `{forbidden}`"
        );
    }

    let stored = local(&app);
    assert!(stored.has_local_sharpening());
    assert!(!stored.has_local_noise_reduction());
    assert!(!stored.is_neutral());
    assert!(app.has_mask_local_sharpening().unwrap());
    let sharpening = stored
        .detail
        .as_ref()
        .and_then(|detail| detail.sharpening)
        .expect("sharpening");
    assert!(close_f32(sharpening.amount, 0.75));
    assert_eq!(sharpening.version, 1);

    // Same transaction contract as every other local edit.
    assert_eq!(
        app.pending_mask_state_before.as_deref(),
        Some(initial.as_slice())
    );
    assert_eq!(
        app.pending_slider_commit.as_ref().map(|(k, _)| k.as_str()),
        Some("mask.local.detail.sharpening.amount")
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
    assert!(persisted.has_local_detail());
    assert!(close_f32(
        persisted
            .detail
            .as_ref()
            .and_then(|detail| detail.sharpening)
            .expect("sharpening")
            .amount,
        0.75
    ));
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

/// Every sharpening and noise-reduction field is settable and readable, and the
/// resets work per area and for the whole block.
#[test]
fn local_detail_setters_and_resets_cover_every_field() {
    let (_directory, mut app, _source) = local_app();
    for (field, value) in [
        ("amount", 0.75),
        ("radius", 3.5),
        ("detail", 0.25),
        ("masking", 0.5),
    ] {
        app.set_mask_local_sharpening(field, value).unwrap();
    }
    for (field, value) in [("luminance", 0.4), ("color", 0.75)] {
        app.set_mask_local_noise_reduction(field, value).unwrap();
    }
    let sharpening = app.selected_mask_local_sharpening().unwrap();
    assert!(close_f32(sharpening.amount, 0.75));
    assert!(close_f32(sharpening.radius, 3.5));
    assert!(close_f32(sharpening.detail, 0.25));
    assert!(close_f32(sharpening.masking, 0.5));
    let noise = app.selected_mask_local_noise_reduction().unwrap();
    assert!(close_f32(noise.luminance, 0.4));
    assert!(close_f32(noise.color, 0.75));
    assert!(app.has_mask_local_sharpening().unwrap());
    assert!(app.has_mask_local_noise_reduction().unwrap());
    assert_eq!(local(&app).detail_summary(), "sharpening+noise_reduction");

    // A per-area reset removes exactly one sub-block.
    app.reset_mask_local_detail_field("noise_reduction")
        .unwrap();
    assert!(app.has_mask_local_sharpening().unwrap());
    assert!(!app.has_mask_local_noise_reduction().unwrap());
    assert_eq!(local(&app).detail_summary(), "sharpening");

    // Writing the amount back to zero drops the sharpening sub-block, and with
    // it the whole container.
    app.set_mask_local_sharpening("amount", 0.0).unwrap();
    assert!(!app.has_mask_local_sharpening().unwrap());
    assert!(!app.has_mask_local_noise_reduction().unwrap());
    assert!(local(&app).detail.is_none());

    // The whole-block reset does the same in one step.
    app.set_mask_local_sharpening("amount", 1.0).unwrap();
    app.set_mask_local_noise_reduction("luminance", 0.5)
        .unwrap();
    app.reset_mask_local_detail().unwrap();
    assert!(local(&app).detail.is_none());
    assert!(!app.has_mask_local_sharpening().unwrap());
    assert!(!app.has_mask_local_noise_reduction().unwrap());
    assert!(!app.has_visible_local_adjustments());

    // `masking = 0` with a non-zero amount is the strongest setting, never a
    // silent no-op: the editor writes it and the state keeps the block.
    app.set_mask_local_sharpening("amount", 1.0).unwrap();
    app.set_mask_local_sharpening("masking", 0.0).unwrap();
    assert!(app.has_mask_local_sharpening().unwrap());
    let block = local(&app)
        .detail
        .as_ref()
        .and_then(|detail| detail.sharpening)
        .expect("sharpening");
    assert_eq!(block.masking, 0.0);
    assert!(local(&app).has_local_detail());
}

/// Out-of-range and non-finite detail edits are refused without mutating
/// anything.
#[test]
fn invalid_local_detail_edits_are_refused_without_mutating() {
    let (_directory, mut app, _source) = local_app();
    app.set_mask_local_sharpening("amount", 0.5).unwrap();
    app.save_sidecar();
    let before = app.active_mask_layers_snapshot().unwrap();
    let recipe_before = app.recipe().clone();
    for (field, value) in [
        ("amount", 3.5),
        ("amount", -0.5),
        ("radius", 0.05),
        ("radius", 10.5),
        ("detail", 1.5),
        ("masking", -0.5),
        ("amount", f64::NAN),
        ("radius", f64::INFINITY),
        ("luminance", 0.5),
        ("grain", 0.5),
    ] {
        let error = app
            .set_mask_local_sharpening(field, value)
            .expect_err("an out-of-range or non-finite value must be refused");
        assert!(error.to_string().contains(field), "{error}");
        assert_eq!(
            app.active_mask_layers_snapshot().unwrap(),
            before,
            "`{field}={value}` must not mutate the layer"
        );
        assert_eq!(app.recipe(), &recipe_before);
    }
    for (field, value) in [
        ("luminance", 1.5),
        ("color", -0.5),
        ("luminance", f64::NAN),
        ("amount", 0.5),
    ] {
        let error = app
            .set_mask_local_noise_reduction(field, value)
            .expect_err("an out-of-range or non-finite value must be refused");
        assert!(error.to_string().contains(field), "{error}");
        assert_eq!(
            app.active_mask_layers_snapshot().unwrap(),
            before,
            "`{field}={value}` must not mutate the layer"
        );
    }
    // An unknown per-area reset is refused too, and changes nothing.
    for field in ["optics", "denoise_ai", "detail"] {
        let error = app
            .reset_mask_local_detail_field(field)
            .expect_err("only `sharpening` and `noise_reduction` may be reset");
        assert!(error.to_string().contains(field), "{error}");
        assert_eq!(app.active_mask_layers_snapshot().unwrap(), before);
    }
    // The two legal radius boundaries are accepted.
    app.set_mask_local_sharpening("radius", 0.1).unwrap();
    app.set_mask_local_sharpening("radius", 10.0).unwrap();
    assert!(close_f32(
        app.selected_mask_local_sharpening().unwrap().radius,
        10.0
    ));
}

/// A detail-only layer refuses the stand-in routes, exactly like a presence-only
/// or a colour-only layer.
#[test]
fn a_detail_only_local_layer_refuses_the_stand_in_routes() {
    let (_directory, mut app, _source) = local_app();
    assert!(!app.has_visible_local_adjustments());
    assert!(app.local_adjustment_route_reason().is_none());

    app.set_mask_local_sharpening("amount", 1.0).unwrap();
    assert!(app.has_visible_local_adjustments());
    let reason = app
        .local_adjustment_route_reason()
        .expect("a detail-only layer must refuse the stand-in routes");
    assert!(reason.contains("local mask adjustments"), "{reason}");
    assert!(reason.contains("CPU"), "{reason}");
    // The refusal text names the detail stage, so a user can see why.
    assert!(reason.contains("detail"), "{reason}");

    // A noise-reduction-only layer refuses too.
    app.reset_mask_local_detail().unwrap();
    app.set_mask_local_noise_reduction("luminance", 0.5)
        .unwrap();
    assert!(app.local_adjustment_route_reason().is_some());

    app.reset_mask_local_detail().unwrap();
    assert!(!app.has_visible_local_adjustments());
    assert!(app.local_adjustment_route_reason().is_none());

    // The still-disabled local AI-denoise and optics have no setter, no field and
    // no local key: they stay disabled.
    let json = serde_json::to_value(lumina_sidecar::LocalAdjustments::default()).unwrap();
    let keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
    for disabled in ["denoise_ai", "optics", "lens_correction"] {
        assert!(
            !keys.iter().any(|key| key == disabled),
            "the local recipe must not carry a `{disabled}` field"
        );
        let mut recipe = lumina_sidecar::LocalAdjustments::default();
        assert!(recipe.set_value(disabled, 0.1).is_err());
    }
}

/// The detail block is part of the local state digest, so a detail edit must
/// invalidate the mask and render identities.
#[test]
fn local_detail_state_digest_invalidates_mask_and_render() {
    let (_directory, mut app, _source) = local_app();
    let none = lumina_sidecar::LocalAdjustments::default().digest();
    app.set_mask_local_sharpening("amount", 0.5).unwrap();
    let sharpened = local(&app).digest();
    assert_ne!(none, sharpened);
    app.set_mask_local_noise_reduction("luminance", 0.3)
        .unwrap();
    let both = local(&app).digest();
    assert_ne!(
        sharpened, both,
        "the noise reduction is part of the identity"
    );
    app.set_mask_local_sharpening("radius", 2.0).unwrap();
    let radius_changed = local(&app).digest();
    assert_ne!(both, radius_changed, "the radius is part of the identity");
    app.reset_mask_local_detail().unwrap();
    assert!(local(&app).detail.is_none());
    assert_eq!(
        local(&app).digest(),
        none,
        "a reset returns to the identity of `none`"
    );
    // And the mask-layer digest follows it.
    let layers = app.active_mask_layers_snapshot().unwrap();
    let plain = lumina_sidecar::mask_layers_digest(&layers);
    app.set_mask_local_sharpening("amount", 0.5).unwrap();
    assert_ne!(
        plain,
        lumina_sidecar::mask_layers_digest(&app.active_mask_layers_snapshot().unwrap())
    );
}

/// The GUI and the CLI write the *same* persisted typed block, through the same
/// setter API, so the two surfaces cannot drift.
#[test]
fn gui_and_cli_local_detail_share_the_same_persisted_typed_block() {
    let (_directory, mut app, source) = local_app();
    for (field, value) in [
        ("amount", 0.75),
        ("radius", 3.5),
        ("detail", 0.25),
        ("masking", 0.5),
    ] {
        app.set_mask_local_sharpening(field, value).unwrap();
    }
    for (field, value) in [("luminance", 0.4), ("color", 0.75)] {
        app.set_mask_local_noise_reduction(field, value).unwrap();
    }
    app.save_sidecar();
    let from_gui = local(&app);

    // Exactly what the CLI channel
    // `--set-local-adjustment 'sharpening.<field>=<n>'` /
    // `'noise_reduction.<field>=<n>'` applies, through the very same sidecar
    // setters.
    let mut from_cli = lumina_sidecar::LocalAdjustments::default();
    for (field, value) in [
        ("amount", 0.75),
        ("radius", 3.5),
        ("detail", 0.25),
        ("masking", 0.5),
    ] {
        from_cli.set_local_sharpening_field(field, value).unwrap();
    }
    for (field, value) in [("luminance", 0.4), ("color", 0.75)] {
        from_cli
            .set_local_noise_reduction_field(field, value)
            .unwrap();
    }

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

/// The detail block must survive History / Previous of the current copy as a
/// complete additive layer snapshot.
#[test]
fn local_detail_survives_history_and_previous() {
    let (_directory, mut app, _source) = local_app();
    app.set_mask_local_sharpening("amount", 0.75).unwrap();
    app.set_mask_local_sharpening("radius", 2.0).unwrap();
    app.save_sidecar();
    let with_detail = app.active_mask_layers_snapshot().unwrap();

    // The second edit closes its own transaction, whose *pre-edit* snapshot is
    // the sharpening state; restoring that entry must bring it back.
    app.reset_mask_local_detail().unwrap();
    app.save_sidecar();
    assert!(!app.has_mask_local_sharpening().unwrap());
    let entry = app.document.as_ref().unwrap().virtual_copies[0]
        .history
        .last()
        .unwrap()
        .id
        .clone();

    app.restore_history(&entry).unwrap();
    assert_eq!(app.active_mask_layers_snapshot().unwrap(), with_detail);
    let sharpening = app.selected_mask_local_sharpening().unwrap();
    assert!(close_f32(sharpening.amount, 0.75));
    assert!(close_f32(sharpening.radius, 2.0));
}

/// The editor paints the detail block and its resets, and painting is display
/// state only.
#[test]
fn the_local_detail_editor_paints_and_writes_only_the_mask_layer() {
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
            app.draw_mask_local_detail(ui);
            drawn += 1;
        },
    );
    // No GPU renderer consumes the per-frame texture deltas headless.
    output.textures_delta.clear();
    assert_eq!(drawn, 1);
    // Painting is display state only.
    assert_eq!(app.recipe(), &global_before);
    assert!(!app.has_mask_local_sharpening().unwrap());
    assert!(!app.has_mask_local_noise_reduction().unwrap());

    // Drawing again after an edit shows the stored state and still does not
    // touch the global recipe.
    app.set_mask_local_sharpening("amount", 0.75).unwrap();
    app.set_mask_local_noise_reduction("luminance", 0.4)
        .unwrap();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 4096.0),
            )),
            time: Some(0.2),
            ..Default::default()
        },
        |ui| app.draw_mask_local_detail(ui),
    );
    output.textures_delta.clear();
    assert_eq!(app.recipe(), &global_before);
    assert!(app.has_mask_local_sharpening().unwrap());
    assert!(app.has_mask_local_noise_reduction().unwrap());
    // The editor has no scale control: the radius follows the global render
    // scale, so a second per-mask scale slider would be a second source of
    // truth for something the global stage already owns.
    let json = serde_json::to_string(&local(&app)).expect("serializable");
    for forbidden in ["scale", "render_scale", "effective_scale"] {
        assert!(
            !json.contains(forbidden),
            "the local detail block must not persist a scale: {json}"
        );
    }
}
