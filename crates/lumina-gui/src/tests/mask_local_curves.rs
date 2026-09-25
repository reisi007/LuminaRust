//! MASK-LOCAL-P1.2a headless GUI tests for the mask-local tone curve:
//! setters, resets, refusals, routing and GUI/CLI parity.
//!
//! This module also owns the shared headless pointer harness, so the gesture
//! tests in `mask_local_curve_graph` drive the *identical* widget through
//! *identical* pointer events as the contract tests here.

use super::mask_local::local_app;
use super::*;
use crate::develop_tone::tone_curve_graph::local_tone_curve_graph_id;

/// Persistent headless context with an advancing clock.
pub(super) struct LocalCurveHarness {
    ctx: egui::Context,
    time: f64,
}

impl LocalCurveHarness {
    pub(super) fn new() -> Self {
        Self {
            ctx: egui::Context::default(),
            time: 0.0,
        }
    }

    pub(super) fn run(
        &mut self,
        app: &mut LuminaApp,
        events: Vec<egui::Event>,
        draw: &mut dyn FnMut(&mut LuminaApp, &mut egui::Ui),
    ) {
        self.time += 1.0 / 60.0;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 4096.0));
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ui| draw(app, ui),
        );
        // No GPU renderer consumes the per-frame texture deltas headless.
        output.textures_delta.clear();
    }
}

pub(super) fn press(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

pub(super) fn local_graph_rect(
    harness: &mut LocalCurveHarness,
    app: &mut LuminaApp,
    channel: &str,
    draw: &mut dyn FnMut(&mut LuminaApp, &mut egui::Ui),
) -> egui::Rect {
    harness.run(app, vec![], draw);
    harness
        .ctx
        .read_response(local_tone_curve_graph_id(channel))
        .unwrap_or_else(|| panic!("local tone curve graph for {channel:?} must be painted"))
        .rect
}

/// Map graph fractions (input right, output up) to a screen position.
pub(super) fn graph_pos(rect: egui::Rect, input: f32, output: f32) -> egui::Pos2 {
    egui::pos2(
        rect.left() + input.clamp(0.0, 1.0) * rect.width(),
        rect.bottom() - output.clamp(0.0, 1.0) * rect.height(),
    )
}

pub(super) fn local_point(input: f32, output: f32) -> CurvePoint {
    CurvePoint { input, output }
}

pub(super) fn draw_local_channel(app: &mut LuminaApp, ui: &mut egui::Ui) {
    app.draw_local_tone_curve_graph(ui, "master");
}

/// The draw callback the harness needs. A closure (not a `fn` item) because
/// `&mut dyn FnMut` requires a mutable place.
pub(super) fn local_draw() -> impl FnMut(&mut LuminaApp, &mut egui::Ui) {
    draw_local_channel
}

/// The GUI setter path persists a local curve, arms history/save, and never
/// mutates the global recipe.
#[test]
fn local_curve_setter_arms_history_save_and_never_touches_the_global_recipe() {
    let (_directory, mut app, source) = local_app();
    let global_before = app.recipe().clone();
    let initial = app.active_mask_layers_snapshot().unwrap();
    assert_eq!(app.selected_mask_local_curve("master").unwrap().len(), 2);
    assert!(!app.has_mask_local_curves().unwrap());

    app.set_mask_local_curve_channel(
        "master",
        vec![
            local_point(0.0, 0.0),
            local_point(0.5, 0.7),
            local_point(1.0, 1.0),
        ],
    )
    .unwrap();

    // The global recipe is untouched — not the curve block, not the adjustments.
    assert_eq!(app.recipe(), &global_before);
    assert!(app.recipe().curves.is_none());
    let points = app.selected_mask_local_curve("master").unwrap();
    assert_eq!(points.len(), 3);
    assert_eq!(points[1].output, 0.7);
    assert!(app.has_mask_local_curves().unwrap());

    // Same transaction contract as every other local edit: one coalesced
    // pre-edit snapshot plus a debounced save.
    assert_eq!(
        app.pending_mask_state_before.as_deref(),
        Some(initial.as_slice())
    );
    assert_eq!(
        app.pending_slider_commit.as_ref().map(|(k, _)| k.as_str()),
        Some("mask.local.curves.master")
    );

    // And it survives a real save/reload.
    app.save_sidecar();
    assert!(app.pending_mask_state_before.is_none());
    let entry = app.document.as_ref().unwrap().virtual_copies[0]
        .history
        .last()
        .unwrap()
        .clone();
    // The history snapshot is the *pre-edit* state, exactly like every other
    // local adjustment.
    assert_eq!(
        entry.mask_state().unwrap().unwrap().layers,
        MaskStateSnapshot::new(initial).layers
    );
    assert!(entry.mask_state().unwrap().unwrap().layers[0]
        .local_adjustments
        .is_none());
    // The reloaded file really carries the curve.
    let document =
        lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source)).expect("reload");
    let persisted = document.virtual_copies[0].mask_layers[0]
        .effective_local_adjustments()
        .unwrap()
        .expect("typed local recipe");
    assert_eq!(
        persisted.curves.as_ref().unwrap().master.len(),
        3,
        "the curve must survive the sidecar roundtrip"
    );
}

/// Reset per channel and reset-all are real persisted/history edits, and the
/// per-channel reset drops the whole block once it is identity again.
#[test]
fn local_curve_reset_is_explicit_persisted_and_history_backed() {
    let (_directory, mut app, _source) = local_app();
    app.set_mask_local_curve_channel(
        "master",
        vec![
            local_point(0.0, 0.0),
            local_point(0.5, 0.7),
            local_point(1.0, 1.0),
        ],
    )
    .unwrap();
    // Each save closes one coalesced transaction, so the per-channel reset
    // gets its own history entry with its own pre-edit snapshot.
    app.save_sidecar();
    app.set_mask_local_curve_channel(
        "red",
        vec![
            local_point(0.0, 0.0),
            local_point(0.5, 0.4),
            local_point(1.0, 1.0),
        ],
    )
    .unwrap();
    app.save_sidecar();

    let before_reset = app.active_mask_layers_snapshot().unwrap();
    assert!(before_reset[0]
        .local_adjustments
        .as_ref()
        .unwrap()
        .curves
        .as_ref()
        .unwrap()
        .channels
        .red
        .is_some());
    app.reset_mask_local_curve_channel("red").unwrap();
    app.save_sidecar();
    let stored = app.selected_mask_local_curve("red").unwrap();
    assert_eq!(stored, vec![local_point(0.0, 0.0), local_point(1.0, 1.0)]);
    assert!(app.has_mask_local_curves().unwrap());
    // The master survived the per-channel reset.
    assert_eq!(app.selected_mask_local_curve("master").unwrap().len(), 3);

    // History restores the pre-reset state verbatim, curve included.
    let document = app.document.as_ref().unwrap().clone();
    let reset_entry = document.virtual_copies[0]
        .history
        .last()
        .unwrap()
        .id
        .clone();
    app.restore_history(&reset_entry).unwrap();
    assert_eq!(app.active_mask_layers_snapshot().unwrap(), before_reset);
    assert!(app.selected_mask_local_curve("red").unwrap().len() == 3);

    // Reset-all clears the block entirely.
    app.reset_mask_local_curves().unwrap();
    assert!(!app.has_mask_local_curves().unwrap());
    assert_eq!(
        app.selected_mask_local_adjustment("exposure").unwrap(),
        Some(0.0)
    );
    // A reset of an unknown channel is loud and changes nothing — in
    // particular it never falls back to "reset everything".
    app.set_mask_local_curve_channel(
        "master",
        vec![
            local_point(0.0, 0.0),
            local_point(0.5, 0.7),
            local_point(1.0, 1.0),
        ],
    )
    .unwrap();
    assert!(app.reset_mask_local_curve_channel("luma").is_err());
    assert!(app.has_mask_local_curves().unwrap());
    assert_eq!(app.selected_mask_local_curve("master").unwrap().len(), 3);
}

/// Invalid point lists are refused loudly and leave the layer byte-for-byte
/// unchanged — including the pending history snapshot.
#[test]
fn invalid_local_curve_edits_are_refused_without_mutating() {
    let (_directory, mut app, _source) = local_app();
    app.set_mask_local_curve_channel(
        "master",
        vec![
            local_point(0.0, 0.0),
            local_point(0.5, 0.7),
            local_point(1.0, 1.0),
        ],
    )
    .unwrap();
    let before = app.active_mask_layers_snapshot().unwrap();
    app.save_sidecar();
    let history_before = app.document.as_ref().unwrap().virtual_copies[0]
        .history
        .len();

    for points in [
        // out of range
        vec![
            local_point(0.0, 0.0),
            local_point(0.5, 1.5),
            local_point(1.0, 1.0),
        ],
        // non-ascending
        vec![
            local_point(0.0, 0.0),
            local_point(0.7, 0.7),
            local_point(0.4, 0.4),
            local_point(1.0, 1.0),
        ],
        // missing (0,0)
        vec![local_point(0.1, 0.1), local_point(1.0, 1.0)],
        // too few
        vec![local_point(0.0, 0.0)],
    ] {
        let error = app
            .set_mask_local_curve_channel("master", points.clone())
            .expect_err("invalid local curve points must be refused");
        assert!(error.to_string().contains("tone curve"), "{error}");
        assert_eq!(app.active_mask_layers_snapshot().unwrap(), before);
    }
    assert!(app
        .set_mask_local_curve_channel("luma", vec![local_point(0.0, 0.0), local_point(1.0, 1.0)])
        .unwrap_err()
        .to_string()
        .contains("unknown local curve channel"));

    // Endpoints are mandatory in the editor, too.
    assert!(app
        .move_mask_local_curve_point("master", 0, 0.2, 0.2)
        .is_err());
    assert!(app
        .move_mask_local_curve_point("master", 2, 0.8, 0.8)
        .is_err());
    // Nothing was committed: no pending snapshot, no save, no history entry.
    assert!(app.pending_mask_state_before.is_none());
    assert!(app.pending_slider_commit.is_none());
    assert_eq!(
        app.document.as_ref().unwrap().virtual_copies[0]
            .history
            .len(),
        history_before
    );
    assert_eq!(app.active_mask_layers_snapshot().unwrap(), before);
}

/// A curve-only local layer keeps every stand-in/GPU route on the CPU
/// reference (CPU-first until hardware parity).
#[test]
fn a_curve_only_local_layer_refuses_the_stand_in_routes() {
    let (_directory, mut app, _source) = local_app();
    assert!(app.local_adjustment_route_reason().is_none());
    app.set_mask_local_curve_channel(
        "master",
        vec![
            local_point(0.0, 0.0),
            local_point(0.5, 0.7),
            local_point(1.0, 1.0),
        ],
    )
    .unwrap();
    let reason = app
        .local_adjustment_route_reason()
        .expect("a curve-only local layer must refuse the stand-in route");
    assert!(reason.contains("local mask adjustments"), "{reason}");
    assert!(reason.contains("tone curve"), "{reason}");

    // The draft route therefore upgrades to the mask-aware CPU render.
    app.render_draft([800, 600], None).unwrap();
    assert!(
        !app.preview_is_draft,
        "a local curve may not use the maskless draft route"
    );
    // And the zoomed navigator stays refused.
    app.preview_zoom = 2.0;
    assert!(app.navigator_zoomed_overview().is_none());
}

/// The GUI and the CLI must agree on the persisted local state. Both write the
/// same typed block through the same validator, so a CLI-written curve reads
/// back in the GUI with identical values.
#[test]
fn gui_and_cli_local_curves_share_the_same_persisted_typed_block() {
    let (_directory, mut app, source) = local_app();
    let points = vec![
        local_point(0.0, 0.0),
        local_point(0.25, 0.35),
        local_point(0.5, 0.7),
        local_point(1.0, 1.0),
    ];
    app.set_mask_local_curve_channel("master", points.clone())
        .unwrap();
    app.save_sidecar();
    app.save_sidecar_result().expect("sidecar save");

    // Read the very same bytes the CLI would read, and evaluate the same
    // typed setter the CLI uses.
    let document = lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(&source))
        .expect("reload sidecar");
    let layer = &document.virtual_copies[0].mask_layers[0];
    let stored = layer
        .effective_local_adjustments()
        .unwrap()
        .expect("typed local recipe");
    assert_eq!(stored.curves.as_ref().unwrap().master, points);
    assert_eq!(app.selected_mask_local_curve("master").unwrap(), points);

    // The CLI value grammar maps onto the same point list.
    let mut cli_side = stored.clone();
    cli_side.reset_local_curves();
    cli_side
        .set_local_curve_channel("master", points.clone())
        .unwrap();
    assert_eq!(cli_side, stored);
}

/// A local curve edit must change the render identity, so neither the preview
/// nor an export can reuse the cached frame of the pre-curve state.
#[test]
fn a_local_curve_edit_changes_the_render_identity() {
    let (_directory, mut app, _source) = local_app();
    app.render().unwrap();
    let before = app
        .render_key
        .as_ref()
        .expect("a completed render has an identity")
        .digest();

    app.set_mask_local_curve_channel(
        "master",
        vec![
            local_point(0.0, 0.0),
            local_point(0.5, 0.7),
            local_point(1.0, 1.0),
        ],
    )
    .unwrap();
    app.render().unwrap();
    let after = app
        .render_key
        .as_ref()
        .expect("a completed render has an identity")
        .digest();
    assert_ne!(
        before, after,
        "a local curve edit must invalidate the cached render identity"
    );
}
