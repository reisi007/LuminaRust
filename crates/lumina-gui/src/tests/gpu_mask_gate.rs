//! GPU-PARITY-MASKGATE-1: the GPU-present mask gate answers a **pixel**
//! question — *does the frame to be presented carry an evaluated mask layer the
//! CPU painter would not draw?* — and it answers it from the **layers**
//! (`render_mask_layers`), never from the selection.
//!
//! The states it must keep apart:
//!
//! 1. the frame carries no evaluated mask layer → nothing composited that the
//!    CPU painter would miss → the readback-free VRAM path is pixel-equal and
//!    must be reachable (this is the shipped default Develop state the gate used
//!    to demote to CPU silently);
//! 2. the frame carries an evaluated layer the CPU painter would not draw → CPU
//!    present stays mandatory (`R5-MASKVIS-25`).
//!
//! Two traps this file guards, both measured at the production path:
//!
//! * **The selection is the wrong signal.** Deleting the *selected* mask leaves
//!   a prompted mask (and its evaluated layer) in the document while
//!   `selected_mask_id` is `None` and the CPU painter has no prompt to draw.
//!   Routing that frame from VRAM presented a mask tint no CPU path can
//!   reproduce — measured on a real adapter: `maxAbsDiff=67` over 127 707
//!   presented photo bytes. The gate must read the layers.
//! * **The live-gesture hazard** stays on CPU: a gradient/radial drag has no
//!   VRAM representation, a brush stamp is uploaded incrementally. That
//!   separation is the pre-existing `drawing` branch inside the gate.
//!
//! Every routing assertion is paired with the CPU painter's own decision
//! (`effective_overlay_prompt`), so a routing expectation can never be
//! satisfied by a state in which no matte would have been painted at all.
//!
//! **Mutation set (all eight run, results measured — not a claim).** Each entry
//! was applied to the production file, the suite was run, and the change was
//! reverted; the reaction column is what the run actually printed. Re-run with
//! `cargo test -p lumina-gui --lib` and
//! `cargo test -p lumina-gui --test kittest_parity -- --ignored` (adapter).
//!
//! | # | Mutation (in `src/mask_visibility.rs` / `src/present_mask_plane.rs`) | Measured reaction |
//! |---|---|---|
//! | M1 | case-1 relaxation `return self.render_mask_layers.is_empty()` → `return false` | red: `a_frame_without_an_evaluated_layer_takes_the_vram_path`; pixels: `cpu_gpu_path_parity_matrix`, `lensfun_corrector_cell_presents_gpu_without_badge`, `mask_gate_states_stay_pixel_equal` (`maskgate[control]: vram_present=None`). The stale-plane cell stays **green** — the proof that switching the gate off is not a repair |
//! | M2 | same branch → `return self.selected_mask_id.is_none()` | red: `an_evaluated_layer_without_a_selected_mask_stays_on_the_cpu_path`; pixels: `mask_gate_states_stay_pixel_equal` with `maskgate[diverging]: vram_present=Some([160, 120])`, `maxAbsDiff=67 differingBytes=127707 of 603904` |
//! | M3 | exact-set rule → `true` | red: `two_evaluated_layers_that_are_not_exactly_the_selected_one_stay_on_the_cpu_path` and `mask_visibility::gpu_present_respects_all_mask_overlay_gates` |
//! | M4 | `if !self.vram_mask_is_evaluated { return false }` removed | red: `a_draft_upgraded_to_a_full_render_is_judged_by_its_layers` (the "plane not resident" half) and `mask_visibility::gpu_present_respects_all_mask_overlay_gates` |
//! | M5 | live branch `return self.mask_tool == MaskTool::Brush` → `return true` | red: `a_live_gradient_gesture_without_a_mask_stays_on_the_cpu_path`, `gpu_mask_plane::a_live_brush_plane_is_preserved_while_the_present_gate_composites_it`, `mask_visibility::gpu_present_respects_all_mask_overlay_gates` |
//! | M6 | sync dispatch `MaskPlaneIntent::Clear => self.clear_active_vram_mask_plane()` → `{}` (i.e. the pre-repair early return) | pixels: `mask_gate_stale_pooled_mask_plane_is_an_open_finding` red at `maxAbsDiff=67 differingBytes=127707 of 603904`, `vram_present=Some([160, 120])`. **The headless suite stays green here** — the decision is covered by `gpu_mask_plane::a_frame_without_coverage_clears_the_plane_instead_of_skipping`, the *effect* only by the adapter cell |
//! | M7 | `KeepLiveBrush` predicate → `if self.drawing` | red: `gpu_mask_plane::a_live_brush_plane_is_preserved_while_the_present_gate_composites_it` |
//! | M8 | `MaskPlaneIntent::KeepLiveBrush => {}` → `=> self.clear_active_vram_mask_plane()` | **no test goes red** — an unobservable GPU write. Named gap, not a covered claim: a headless test cannot see a texture write, and the cancelled/live-brush state cannot be built in a pixel cell without pointer simulation. The intent mapping is covered by M7, the write itself is not covered at all |
//!
//! See `feature/architecture/pipeline.md`, section "Present-Pfad". The
//! end-to-end pixel counterpart of this file is
//! `tests/kittest_parity_support/mask_gate.rs`; the draft frame kinds are in
//! `gpu_mask_drafts.rs` (shared fixture: `app_with_evaluated_selected_mask`).

#![cfg(feature = "gpu")]

use super::*;

/// A loaded source with one brushed mask whose prompt is evaluated into the
/// render, and the Masking section open: the state in which the VRAM route is
/// *eligible* (every gate open, the evaluated plane resident). The caller then
/// closes one gate at a time.
pub(super) fn app_with_evaluated_selected_mask() -> (LuminaApp, String) {
    let mut app = new_app();
    app.load_bytes(png(), "gpu-mask-gate.png").unwrap();
    let id = brush_mask(&mut app, "Selected", 0.5);
    app.set_section_open(SECTION_MASKING, true);
    app.render().unwrap();
    // The routing predicate is pure session/render-state logic; the evaluated
    // plane flag is set directly so this state test needs no adapter.
    app.vram_mask_is_evaluated = true;
    (app, id)
}

/// Create a mask at `x` carrying one centred brush stroke, and select it.
fn brush_mask(app: &mut LuminaApp, name: &str, x: f32) -> String {
    let id = app.create_mask(name).unwrap();
    app.commit_brush_stroke(vec![BrushMark {
        x,
        y: 0.5,
        radius: 0.2,
        sign: BrushMarkSign::Positive,
        softness: 0.25,
        flow: 0.75,
    }])
    .unwrap();
    id
}

/// A drag in progress for `tool`, with both drag endpoints set so
/// `current_overlay_prompt` yields a live prompt.
fn start_gesture(app: &mut LuminaApp, tool: MaskTool) {
    app.set_mask_tool(tool);
    app.drawing = true;
    app.drag_start = Some(Point2 { x: 0.2, y: 0.5 });
    app.drag_current = Some(Point2 { x: 0.8, y: 0.5 });
}

/// GPU-PARITY-MASKGATE-1 case 1: a frame without any evaluated mask layer has
/// nothing the VRAM composite could miss, so the readback-free present must be
/// reachable. This is the state `cpu_gpu_path_parity_matrix` and
/// `lensfun_corrector_cell_presents_gpu_without_badge` assert on a real
/// adapter; here the gate itself is pinned without one.
#[test]
fn a_frame_without_an_evaluated_layer_takes_the_vram_path() {
    let mut app = new_app();
    app.load_bytes(png(), "gpu-mask-gate-no-mask.png").unwrap();
    app.render().unwrap();

    // Precondition, measured on the production state: this really is the
    // maskless default Develop state, so a `true` below cannot be a by-product
    // of an unrelated closed gate.
    assert!(
        app.selected_mask_id().is_none(),
        "a fresh source must not carry a mask selection"
    );
    assert!(
        app.render_mask_layers.is_empty(),
        "a recipe without masks evaluates no mask layer"
    );
    assert!(
        !app.mask_overlay_allowed(),
        "a closed Masking section with no selection allows no matte"
    );
    assert!(
        app.effective_overlay_prompt().is_none(),
        "no mask and no gesture means there is nothing for the CPU painter to draw"
    );
    assert!(
        app.gpu_mask_overlay_is_selected(),
        "GPU-PARITY-MASKGATE-1 case 1: a frame without an evaluated mask layer \
         must present readback-free (this is the default Develop state)"
    );

    // Opening the Masking section introduces no selection, so it must not
    // change the routing either.
    app.set_section_open(SECTION_MASKING, true);
    assert!(app.mask_view_open());
    assert!(app.selected_mask_id().is_none());
    assert!(app.render_mask_layers.is_empty());
    assert!(app.gpu_mask_overlay_is_selected());
}

/// GPU-PARITY-MASKGATE-1 case 2 (the acceptance guard that the gate was
/// *corrected*, not switched off): a selected mask with any single closed
/// editorial gate keeps the frame on the CPU present path.
#[test]
fn a_selected_mask_with_a_closed_overlay_gate_stays_on_the_cpu_path() {
    let (mut app, id) = app_with_evaluated_selected_mask();
    assert_eq!(app.selected_mask_id(), Some(id.as_str()));

    // Baseline: everything open, prompt evaluated, exactly the selected layer
    // in VRAM → the VRAM route is reachable. Without this the assertions below
    // would pass for the trivial reason that the gate never opens at all.
    assert!(app.mask_overlay_allowed());
    assert_eq!(app.render_mask_layers.len(), 1);
    assert!(app.gpu_mask_overlay_is_selected());

    // 1) Masking section closed.
    app.set_section_open(SECTION_MASKING, false);
    assert!(!app.mask_view_open());
    assert!(!app.mask_overlay_allowed());
    assert!(app.effective_overlay_prompt().is_none());
    assert!(!app.gpu_mask_overlay_is_selected());
    app.set_section_open(SECTION_MASKING, true);

    // 2) Pins-only display mode (not `SelectedFull`).
    app.set_mask_overlay_mode(MaskOverlayMode::PinsOnly);
    assert!(!app.mask_overlay_allowed());
    assert!(app.effective_overlay_prompt().is_none());
    assert!(!app.gpu_mask_overlay_is_selected());
    app.set_mask_overlay_mode(MaskOverlayMode::SelectedFull);

    // 3) Show switch off.
    app.set_show_mask_overlay(false);
    assert!(!app.mask_overlay_allowed());
    assert!(app.effective_overlay_prompt().is_none());
    assert!(!app.gpu_mask_overlay_is_selected());
    app.set_show_mask_overlay(true);

    // 4) The selected mask's own eye.
    app.set_mask_visible(&id, false).unwrap();
    assert!(!app.mask_overlay_allowed());
    assert!(app.effective_overlay_prompt().is_none());
    assert!(!app.gpu_mask_overlay_is_selected());
    app.set_mask_visible(&id, true).unwrap();

    // Back to the fully open state, the gate opens again — the checks above
    // tested the gate, not a stuck value. An eye toggle is a recipe mutation,
    // so it correctly invalidates the evaluated VRAM plane (`mark_dirty`);
    // re-render to re-establish that precondition instead of forcing the flag.
    app.render().unwrap();
    app.vram_mask_is_evaluated = true;
    assert!(app.mask_overlay_allowed());
    assert!(app.effective_overlay_prompt().is_some());
    assert!(
        app.gpu_mask_overlay_is_selected(),
        "with every gate open again the VRAM route must be reachable — a \
         permanently closed gate would be an off switch, not a split"
    );
}

/// GPU-PARITY-MASKGATE-1, the measured divergence the gate must refuse: a mask
/// with a prompt lives in the document, but **none** is selected (the normal
/// flow: two brushed masks, then delete the selected one). The frame still
/// carries the remaining mask's evaluated layer, the CPU painter has no prompt
/// to draw, and the user cannot switch the tint off — `mask_overlay_allowed`
/// demands the selection that no longer exists.
///
/// A gate that asked "is a mask selected?" instead of "does the frame carry an
/// evaluated layer?" routes this frame from VRAM and the presented photo shows
/// a mask tint the CPU path cannot reproduce (measured on a real adapter:
/// `maxAbsDiff=67` over 127 707 presented photo bytes). This test is the
/// headless guard for that; the pixel counterpart is
/// `kittest_parity_support::mask_gate::mask_gate_states_stay_pixel_equal`.
#[test]
fn an_evaluated_layer_without_a_selected_mask_stays_on_the_cpu_path() {
    let mut app = new_app();
    app.load_bytes(png(), "gpu-mask-gate-orphan-layer.png")
        .unwrap();
    let first = brush_mask(&mut app, "First", 0.3);
    let remaining = brush_mask(&mut app, "Second", 0.7);
    app.set_section_open(SECTION_MASKING, true);
    // `create_mask` selects the newest mask; the *first* one is the one this
    // test deletes, so it must be selected explicitly.
    app.select_mask(&first).unwrap();
    app.render().unwrap();
    assert_eq!(app.render_mask_layers.len(), 2);
    app.delete_mask(&first).unwrap();
    app.render().unwrap();

    // The measured shape of the trap, asserted before the routing claim so a
    // future regression cannot make this test vacuous:
    assert!(
        app.selected_mask_id().is_none(),
        "deleting the selected mask must leave no selection behind"
    );
    assert!(
        app.document
            .as_ref()
            .and_then(|document| {
                document
                    .virtual_copies
                    .iter()
                    .find(|copy| copy.id == app.virtual_copy_id)
            })
            .is_some_and(|copy| {
                copy.mask_library
                    .iter()
                    .any(|mask| mask.id == remaining && mask.prompt.is_some())
            }),
        "the other prompted mask stays in the document — this is the state that \
         makes the selection the wrong signal"
    );
    assert_eq!(
        app.render_mask_layers.len(),
        1,
        "the remaining mask's layer is still evaluated into the presented frame"
    );
    assert!(
        !app.mask_overlay_allowed(),
        "without a selection the CPU painter's matte gate is closed"
    );
    assert!(
        app.effective_overlay_prompt().is_none(),
        "the CPU painter has nothing to draw — the VRAM tint would be the only \
         place that mask is visible"
    );
    assert!(
        !app.gpu_mask_overlay_is_selected(),
        "GPU-PARITY-MASKGATE-1: a frame carrying an evaluated mask layer that \
         the CPU painter would not draw must stay on the CPU present path"
    );
}

/// The **foreign-layer set** half of case 2: the frame carries evaluated layers,
/// but they are not exactly the selected mask's single layer. The VRAM overlay
/// pass combines *every* layer into one plane, while the CPU painter rasterizes
/// one selected prompt — so a set of two foreign layers (or the selected layer
/// plus a foreign one) cannot be pixel-equal and must stay on the CPU path.
///
/// Only the pre-existing single-layer mutation in
/// `tests::mask_visibility.rs::gpu_present_respects_all_mask_overlay_gates`
/// (which renames the one resident layer) covered the `layer_id` comparison, and
/// it cannot fail for a *longer* layer list: a gate that only asked "is a layer
/// resident?" would pass its assertions. This test is the guard for the
/// `render_mask_layers.len() == 1` half, built from real masks and real layer
/// evaluation rather than from a renamed field.
#[test]
fn two_evaluated_layers_that_are_not_exactly_the_selected_one_stay_on_the_cpu_path() {
    let (mut app, selected) = app_with_evaluated_selected_mask();
    app.select_mask(&selected).unwrap();
    app.render().unwrap();
    app.vram_mask_is_evaluated = true;

    // Baseline: one mask selected, exactly its own layer resident → VRAM.
    assert_eq!(app.selected_mask_id(), Some(selected.as_str()));
    assert_eq!(app.render_mask_layers.len(), 1);
    assert_eq!(
        app.render_mask_layers[0].layer_id,
        app.selected_mask_layer().expect("selected layer").id
    );
    assert!(app.mask_overlay_allowed());
    assert!(app.effective_overlay_prompt().is_some());
    assert!(app.gpu_mask_overlay_is_selected());

    // A second prompted mask: the frame now carries **both** layers, while the
    // CPU painter still rasterizes one prompt. The selected layer is present —
    // so a gate that asked "is the selected layer among the layers?" would pass
    // and diverge.
    brush_mask(&mut app, "Second", 0.75);
    app.select_mask(&selected).unwrap();
    app.render().unwrap();
    app.vram_mask_is_evaluated = true;
    assert_eq!(
        app.render_mask_layers.len(),
        2,
        "the frame now carries a combined layer set"
    );
    let selected_layer_id = app
        .selected_mask_layer()
        .expect("selected layer")
        .id
        .clone();
    assert!(
        app.render_mask_layers
            .iter()
            .any(|layer| layer.layer_id == selected_layer_id),
        "non-vacuity: the selected layer is one of the two, so only the \
         exact-set rule can refuse this frame"
    );
    assert!(app.mask_overlay_allowed());
    assert!(app.effective_overlay_prompt().is_some());
    assert!(
        !app.gpu_mask_overlay_is_selected(),
        "two evaluated layers are not the exact set the CPU painter draws: the \
         composite combines them, the CPU painter draws one prompt, so the frame \
         must stay on the CPU present path"
    );

    // Neither layer being the selected one is the same verdict — the length
    // check must not degrade into a "contains the selected layer" check.
    let foreign = app.render_mask_layers[0].layer_id.clone();
    for layer in &mut app.render_mask_layers {
        layer.layer_id = foreign.clone();
    }
    assert!(!app.gpu_mask_overlay_is_selected());

    // Restoring the exact single-layer set re-opens the gate, so the checks
    // above tested the rule and not a stuck value.
    app.render_mask_layers.truncate(1);
    app.render_mask_layers[0].layer_id = selected_layer_id;
    assert!(app.gpu_mask_overlay_is_selected());
}

/// The live-gesture hazard of the case-1 relaxation: a running gradient/radial
/// prompt has no VRAM representation, with or without a selected mask, so it
/// must stay on the CPU path. The separation is the gate's `drawing` branch
/// (a brush stamp is uploaded incrementally, a gradient/radial prompt is not).
#[test]
fn a_live_gradient_gesture_without_a_mask_stays_on_the_cpu_path() {
    for tool in [MaskTool::LinearGradient, MaskTool::Radial] {
        let mut app = new_app();
        app.load_bytes(png(), "gpu-mask-gate-live-gradient.png")
            .unwrap();
        app.set_section_open(SECTION_MASKING, true);
        start_gesture(&mut app, tool);

        // Non-vacuity: the CPU painter really does hold a live matte here, so
        // refusing the VRAM route is required, not cosmetic.
        assert!(app.selected_mask_id().is_none());
        assert!(
            app.mask_overlay_allowed(),
            "{tool:?}: a live gradient/radial drag is its own matte source"
        );
        assert!(
            app.effective_overlay_prompt().is_some(),
            "{tool:?}: the CPU painter must have a live prompt to draw"
        );
        assert!(
            !app.gpu_mask_overlay_is_selected(),
            "{tool:?}: a live gradient/radial prompt has no VRAM representation \
             and must be presented from the CPU texture even with no mask selected"
        );
    }
}

/// The brush half of the live-gesture separation: a live **brush** stroke is
/// uploaded incrementally into the VRAM plane, so it stays on the GPU path —
/// with and without a selected mask.
#[test]
fn a_live_brush_gesture_stays_on_the_gpu_path() {
    // With a selected mask, all gates open and the evaluated plane resident.
    let (mut app, _id) = app_with_evaluated_selected_mask();
    start_gesture(&mut app, MaskTool::Brush);
    app.pending_brush_marks.push(BrushMark {
        x: 0.5,
        y: 0.5,
        radius: 0.1,
        sign: BrushMarkSign::Positive,
        softness: 0.0,
        flow: 1.0,
    });
    assert!(app.mask_overlay_allowed());
    assert!(app.effective_overlay_prompt().is_some());
    assert!(
        app.gpu_mask_overlay_is_selected(),
        "a live brush stamp is uploaded incrementally and stays on the VRAM path"
    );

    // Without a selected mask the same holds: the stamp's VRAM upload is what
    // the composite shows.
    let mut app = new_app();
    app.load_bytes(png(), "gpu-mask-gate-live-brush.png")
        .unwrap();
    app.set_section_open(SECTION_MASKING, true);
    start_gesture(&mut app, MaskTool::Brush);
    app.pending_brush_marks.push(BrushMark {
        x: 0.5,
        y: 0.5,
        radius: 0.1,
        sign: BrushMarkSign::Positive,
        softness: 0.0,
        flow: 1.0,
    });
    assert!(app.selected_mask_id().is_none());
    assert!(app.mask_overlay_allowed());
    assert!(app.effective_overlay_prompt().is_some());
    assert!(
        app.gpu_mask_overlay_is_selected(),
        "a live brush stamp without a mask is uploaded incrementally and stays \
         on the VRAM path"
    );
}
