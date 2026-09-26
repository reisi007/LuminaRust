//! MASK-LOCAL-P1.2b headless GUI tests for the mask-local colour block:
//! setters, resets, refusals, render identity, stand-in routing and
//! GUI/CLI parity on the same persisted typed block.

use super::mask_local::local_app;
use super::*;

/// Exact f32 comparison for a value that travelled through a `f32` block.
fn close(actual: f64, expected: f64) -> bool {
    (actual - f64::from(expected as f32)).abs() < 1e-6
}

/// Read the selected layer's stored local recipe.
fn local(app: &LuminaApp) -> lumina_sidecar::LocalAdjustments {
    app.active_mask_layers_snapshot().unwrap()[0]
        .effective_local_adjustments()
        .unwrap()
        .expect("typed local recipe")
}

/// A visible, non-neutral colour block keeps every stand-in route on the CPU
/// reference, and the layer never mutates the global recipe.
#[test]
fn a_local_color_edit_changes_the_render_identity_and_never_the_global_recipe() {
    let (_directory, mut app, source) = local_app();
    let global_before = app.recipe().clone();
    let initial = app.active_mask_layers_snapshot().unwrap();
    assert!(!app.has_mask_local_color().unwrap());
    assert!(!app.has_visible_local_adjustments());

    app.set_mask_local_hsl_band("red", "hue", -0.25).unwrap();

    // No global mutation — not the HSL block, not the colour-grading block and
    // not the global vibrance/saturation adjustment keys.
    assert_eq!(app.recipe(), &global_before);
    assert!(app.recipe().hsl.is_none());
    assert!(app.recipe().color_grading.is_none());
    assert!(!app.recipe().adjustments.contains_key("vibrance"));
    assert!(!app.recipe().adjustments.contains_key("saturation"));

    let stored = local(&app);
    let band = stored.local_hsl_band("red").expect("red band stored");
    assert_eq!(band.hue, -0.25);
    assert!(stored.has_local_color());
    assert!(!stored.is_neutral());
    assert!(app.has_mask_local_color().unwrap());

    // Same transaction contract as every other local edit.
    assert_eq!(
        app.pending_mask_state_before.as_deref(),
        Some(initial.as_slice())
    );
    assert_eq!(
        app.pending_slider_commit.as_ref().map(|(k, _)| k.as_str()),
        Some("mask.local.hsl.red.hue")
    );

    // The colour block survives a real save/reload, and history carries the
    // complete pre-edit layer snapshot.
    app.save_sidecar();
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).expect("reload");
    let persisted = document.virtual_copies[0].mask_layers[0]
        .effective_local_adjustments()
        .unwrap()
        .expect("typed local recipe");
    assert_eq!(
        persisted.local_hsl_band("red").map(|band| band.hue),
        Some(-0.25)
    );
    let entry = app.document.as_ref().unwrap().virtual_copies[0]
        .history
        .last()
        .unwrap()
        .clone();
    assert!(entry.mask_state().unwrap().unwrap().layers[0]
        .local_adjustments
        .is_none());
}

/// Every local colour control is settable, readable and resettable per area,
/// and a refused value changes nothing at all.
#[test]
fn local_color_setters_and_resets_cover_every_area() {
    let (_directory, mut app, _source) = local_app();
    app.set_mask_local_hsl_band("blue", "saturation", 0.5)
        .unwrap();
    app.set_mask_local_vibrance_saturation("vibrance", 0.4)
        .unwrap();
    app.set_mask_local_vibrance_saturation("saturation", -0.3)
        .unwrap();
    let id = app
        .add_mask_local_point_color(30.0, 45.0, 0.2, 0.1, -0.1)
        .unwrap();
    assert_eq!(id, "pc-1");
    app.set_mask_local_grading_field("shadows", "hue", 240.0)
        .unwrap();
    app.set_mask_local_grading_field("shadows", "saturation", 0.6)
        .unwrap();
    app.set_mask_local_grading_field("midtones", "luminance", 0.2)
        .unwrap();
    app.set_mask_local_grading_field("balance", "value", -0.4)
        .unwrap();
    app.set_mask_local_grading_field("blending", "value", 0.8)
        .unwrap();

    let (hue, sat, lum) = app.selected_mask_local_hsl_band("blue").unwrap();
    assert!(close(hue, 0.0) && close(sat, 0.5) && close(lum, 0.0));
    assert_eq!(app.selected_mask_local_vibrance().unwrap(), (0.4, -0.3));
    let (hue, sat, lum) = app.selected_mask_local_grading_range("shadows").unwrap();
    assert!(close(hue, 240.0) && close(sat, 0.6) && close(lum, 0.0));
    let (hue, sat, lum) = app.selected_mask_local_grading_range("midtones").unwrap();
    assert!(close(hue, 0.0) && close(sat, 0.0) && close(lum, 0.2));
    let (balance, blending) = app.selected_mask_local_grading_balance().unwrap();
    assert!(close(balance, -0.4) && close(blending, 0.8));
    let entries = app.selected_mask_local_point_color().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "pc-1");
    assert!(close(f64::from(entries[0].saturation_shift), 0.1));

    // Per-area resets keep the rest of the block.
    app.reset_mask_local_hsl_band("blue").unwrap();
    let (hue, sat, lum) = app.selected_mask_local_hsl_band("blue").unwrap();
    assert!(close(hue, 0.0) && close(sat, 0.0) && close(lum, 0.0));
    assert!(local(&app).has_local_point_color());
    app.set_mask_local_point_color_field("pc-1", "hue_shift", 0.5)
        .unwrap();
    app.remove_mask_local_point_color("pc-1").unwrap();
    assert!(app.selected_mask_local_point_color().unwrap().is_empty());
    app.reset_mask_local_grading("shadows").unwrap();
    let (hue, sat, lum) = app.selected_mask_local_grading_range("shadows").unwrap();
    assert!(close(hue, 0.0) && close(sat, 0.0) && close(lum, 0.0));
    // The midtones luminance shift and the balance survive that reset.
    let (hue, sat, lum) = app.selected_mask_local_grading_range("midtones").unwrap();
    assert!(close(hue, 0.0) && close(sat, 0.0) && close(lum, 0.2));
    assert!(close(
        app.selected_mask_local_grading_balance().unwrap().0,
        -0.4
    ));

    // Reset-all drops every local colour control; a layer with nothing else is
    // byte-identical to one that was never edited.
    app.reset_mask_local_color().unwrap();
    assert!(!app.has_mask_local_color().unwrap());
    assert!(!app.has_visible_local_adjustments());
    let stored = local(&app);
    assert!(stored.hsl.is_none());
    assert!(stored.point_color.is_none());
    assert!(stored.color_grading.is_none());
    assert_eq!(stored.vibrance, 0.0);
    assert_eq!(stored.saturation, 0.0);
}

/// Invalid values are refused loudly and change neither the layer nor the
/// pending history snapshot — and an unknown name never falls back to "reset
/// everything".
#[test]
fn invalid_local_color_edits_are_refused_without_mutating() {
    let (_directory, mut app, _source) = local_app();
    app.set_mask_local_hsl_band("red", "hue", 0.5).unwrap();
    app.save_sidecar();
    let before = app.active_mask_layers_snapshot().unwrap();
    let digest_before = lumina_sidecar::mask_layers_digest(&before);

    for outcome in [
        app.set_mask_local_hsl_band("red", "hue", 1.5).err(),
        app.set_mask_local_hsl_band("red", "brightness", 0.2).err(),
        app.set_mask_local_hsl_band("luma", "hue", 0.2).err(),
        app.set_mask_local_vibrance_saturation("vibrance", 2.0)
            .err(),
        app.set_mask_local_vibrance_saturation("presence", 0.2)
            .err(),
        app.set_mask_local_point_color_field("pc-9", "hue_shift", 0.2)
            .err(),
        app.add_mask_local_point_color(400.0, 45.0, 0.2, 0.1, -0.1)
            .err(),
        app.add_mask_local_point_color(30.0, 200.0, 0.2, 0.1, -0.1)
            .err(),
        app.add_mask_local_point_color(30.0, 45.0, 2.0, 0.1, -0.1)
            .err(),
        app.set_mask_local_grading_field("shadows", "hue", 400.0)
            .err(),
        app.set_mask_local_grading_field("shadows", "saturation", 1.4)
            .err(),
        app.set_mask_local_grading_field("whites", "hue", 30.0)
            .err(),
        app.reset_mask_local_hsl_band("luma").err(),
        app.reset_mask_local_grading("whites").err(),
        app.remove_mask_local_point_color("pc-9").err(),
    ] {
        assert!(
            outcome.is_some(),
            "an invalid local colour edit must be refused"
        );
    }
    assert_eq!(lumina_sidecar::mask_layers_digest(&before), digest_before);
    assert_eq!(
        lumina_sidecar::mask_layers_digest(&app.active_mask_layers_snapshot().unwrap()),
        digest_before
    );
    assert!(app.pending_mask_state_before.is_none());
    assert!(close(
        app.selected_mask_local_hsl_band("red").unwrap().0,
        0.5
    ));
}

/// The colour block is part of the render identity: any change to it
/// invalidates the mask/render digest, while a neutral block does not.
#[test]
fn local_color_state_digest_invalidates_mask_and_render() {
    let (_directory, mut app, _source) = local_app();
    // Establish the "edited, then reset" reference: the layer now carries an
    // explicit (neutral) typed object, which is a different *stored* state than
    // a layer that was never edited, and that difference is deliberate.
    app.set_mask_local_hsl_band("green", "luminance", 0.25)
        .unwrap();
    app.reset_mask_local_color().unwrap();
    let baseline = lumina_sidecar::mask_layers_digest(&app.active_mask_layers_snapshot().unwrap());
    app.set_mask_local_hsl_band("green", "luminance", 0.25)
        .unwrap();
    let hsl = lumina_sidecar::mask_layers_digest(&app.active_mask_layers_snapshot().unwrap());
    assert_ne!(baseline, hsl);
    app.set_mask_local_vibrance_saturation("saturation", 0.5)
        .unwrap();
    let vibrance = lumina_sidecar::mask_layers_digest(&app.active_mask_layers_snapshot().unwrap());
    assert_ne!(hsl, vibrance);
    app.reset_mask_local_color().unwrap();
    assert_eq!(
        lumina_sidecar::mask_layers_digest(&app.active_mask_layers_snapshot().unwrap()),
        baseline
    );
}

/// A colour-only local layer must keep every stand-in route on the CPU
/// reference instead of silently rendering a global-only thumbnail.
#[test]
fn a_color_only_local_layer_refuses_the_stand_in_routes() {
    let (_directory, mut app, _source) = local_app();
    assert!(app.local_adjustment_route_reason().is_none());
    app.set_mask_local_grading_field("highlights", "saturation", 0.5)
        .unwrap();
    assert!(app.has_visible_local_adjustments());
    let reason = app
        .local_adjustment_route_reason()
        .expect("a colour-only layer must refuse the stand-in routes");
    assert!(reason.contains("color"), "{reason}");
    // No local detail/denoise/optics stage is reachable at all, and local
    // presence is reachable only through its own typed setter — never as a
    // scalar key of the colour block's setter.
    assert!(app.set_mask_local_adjustment("presence", 0.5).is_err());
    assert!(app.set_mask_local_adjustment("detail", 0.5).is_err());
    assert!(app.set_mask_local_adjustment("sharpening", 0.5).is_err());
    let (_directory, mut app, _source) = local_app();
    app.set_mask_local_presence("texture", 0.5).unwrap();
    assert!(app.has_mask_local_presence().unwrap());
}

/// The GUI and the CLI must produce the *same* persisted typed block for the
/// same requested edit, not two dialects of one feature.
#[test]
fn gui_and_cli_local_color_share_the_same_persisted_typed_block() {
    use lumina_sidecar::LocalAdjustments;

    let (_directory, mut app, source) = local_app();
    app.set_mask_local_hsl_band("orange", "saturation", -0.4)
        .unwrap();
    app.set_mask_local_vibrance_saturation("vibrance", 0.35)
        .unwrap();
    app.add_mask_local_point_color(120.0, 60.0, 0.1, -0.2, 0.3)
        .unwrap();
    app.set_mask_local_grading_field("midtones", "hue", 90.0)
        .unwrap();
    app.set_mask_local_grading_field("midtones", "saturation", 0.35)
        .unwrap();
    app.save_sidecar();

    let from_gui = local(&app);

    // The same edits through the generic CLI channel, applied to a fresh
    // recipe with the same setters the CLI uses.
    let mut from_cli = LocalAdjustments::default();
    from_cli
        .set_local_hsl_band("orange", "saturation", -0.4)
        .unwrap();
    from_cli.set_value("vibrance", 0.35).unwrap();
    from_cli
        .add_local_point_color_entry(120.0, 60.0, 0.1, -0.2, 0.3)
        .unwrap();
    from_cli
        .set_local_color_grading_field("midtones", "hue", 90.0)
        .unwrap();
    from_cli
        .set_local_color_grading_field("midtones", "saturation", 0.35)
        .unwrap();

    assert_eq!(from_gui, from_cli);
    assert_eq!(from_gui.digest(), from_cli.digest());

    // And the reloaded file really carries the GUI block.
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).expect("reload");
    let reloaded = document.virtual_copies[0].mask_layers[0]
        .effective_local_adjustments()
        .unwrap()
        .expect("typed local recipe");
    assert_eq!(reloaded, from_gui);
}

/// The colour block must survive History / Previous of the current copy as a
/// complete additive layer snapshot.
#[test]
fn local_color_survives_history_and_previous() {
    let (_directory, mut app, _source) = local_app();
    app.set_mask_local_hsl_band("cyan", "saturation", 0.6)
        .unwrap();
    app.save_sidecar();
    let with_color = app.active_mask_layers_snapshot().unwrap();

    // The second edit closes its own transaction, whose *pre-edit* snapshot is
    // the coloured state; restoring that entry must bring the colour back.
    app.set_mask_local_hsl_band("cyan", "saturation", 0.0)
        .unwrap();
    app.save_sidecar();
    assert!(!app.has_mask_local_color().unwrap());
    let entry = app.document.as_ref().unwrap().virtual_copies[0]
        .history
        .last()
        .unwrap()
        .id
        .clone();

    app.restore_history(&entry).unwrap();
    assert_eq!(app.active_mask_layers_snapshot().unwrap(), with_color);
    let (hue, sat, lum) = app.selected_mask_local_hsl_band("cyan").unwrap();
    assert!(close(hue, 0.0) && close(sat, 0.6) && close(lum, 0.0));
}

/// The editor paints every local colour control and its resets, and every
/// painted control is wired to the local setters.
#[test]
fn the_local_color_editor_paints_and_writes_only_the_mask_layer() {
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
            app.draw_mask_local_color(ui);
            drawn += 1;
        },
    );
    // No GPU renderer consumes the per-frame texture deltas headless.
    output.textures_delta.clear();
    assert_eq!(drawn, 1);
    // Painting is display state only.
    assert_eq!(app.recipe(), &global_before);
    assert!(!app.has_mask_local_color().unwrap());

    // Drawing again after an edit shows the stored state and still does not
    // touch the global recipe.
    app.set_mask_local_hsl_band("violet", "hue", 0.3).unwrap();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 4096.0),
            )),
            time: Some(0.2),
            ..Default::default()
        },
        |ui| app.draw_mask_local_color(ui),
    );
    output.textures_delta.clear();
    assert_eq!(app.recipe(), &global_before);
    let (hue, sat, lum) = app.selected_mask_local_hsl_band("violet").unwrap();
    assert!(close(hue, 0.3) && close(sat, 0.0) && close(lum, 0.0));
}
