//! GPU-PARITY-MASKGATE-1 applied to the **draft** frame kinds.
//!
//! `render_draft` is not one frame kind but two, and the gate has to judge each
//! of them by its own rules: a plain interactive draft runs with
//! `masks_context = None` and therefore carries no evaluated layer at all, while
//! a draft that trips one of the documented upgrade triggers delegates to
//! `render_full` and *does* carry one. Both live here so the rule in
//! `gpu_mask_gate.rs` stays readable and the frame-kind axis stays in one file.

#![cfg(feature = "gpu")]

use super::gpu_mask_gate::app_with_evaluated_selected_mask;

/// The draft sub-case of GPU-PARITY-MASKGATE-1 case 2: a plain interactive draft
/// render runs with `masks_context = None`, so no mask layer is evaluated into
/// the frame and the selected matte can only come from the CPU painter. A
/// selected mask must therefore keep the draft frame on the CPU path.
#[test]
fn a_selected_mask_keeps_the_plain_draft_frame_on_the_cpu_path() {
    let (mut app, _id) = app_with_evaluated_selected_mask();
    app.render_draft([1024, 720], None).unwrap();

    // Measured on the production path: the draft render produces no mask layer
    // result, which is exactly why the CPU painter has to draw it.
    assert!(
        app.preview_is_draft(),
        "the draft must be the interactive preview"
    );
    assert!(
        app.render_mask_layers.is_empty(),
        "render_draft runs with masks_context = None, so no mask layer can be \
         evaluated into the draft frame"
    );
    assert!(app.mask_overlay_allowed());
    assert!(
        app.effective_overlay_prompt().is_some(),
        "the selected matte is the CPU painter's job in the draft frame"
    );
    assert!(!app.gpu_mask_overlay_is_selected());
}

/// The draft **upgrade** sub-case: `render_draft` delegates to `render_full`
/// when a full-frame or local-mask stage is active
/// (`render_tick.rs`, generative stage / denoise / source actions / pending
/// sidecar / visible local adjustments). Then the frame *does* carry an
/// evaluated layer, and the gate must judge it by the same layer rules as any
/// other frame instead of blanket-routing drafts to CPU.
///
/// Measured here: the upgrade produces exactly the selected mask's single
/// layer, so with the plane resident the VRAM route is allowed and the VRAM
/// composite is the one showing the matte (`draw_mask_overlay` skips the CPU
/// rasterizer for `gpu_present_frame.is_some() && vram_mask_is_evaluated`) —
/// no divergence. Without the plane resident the gate refuses, which is the
/// conservative half.
#[test]
fn a_draft_upgraded_to_a_full_render_is_judged_by_its_layers() {
    let (mut app, id) = app_with_evaluated_selected_mask();
    // A visible, non-neutral local adjustment is one of the documented upgrade
    // triggers; it is also the only thing that makes the layer meaningful.
    app.set_mask_local_adjustment("exposure", 0.5).unwrap();
    app.render_draft([1024, 720], None).unwrap();

    // The upgrade really happened: `render_draft` delegated to `render_full`, so
    // the frame is no longer a draft and it now carries the evaluated layer.
    assert!(
        !app.preview_is_draft(),
        "an upgraded draft is rendered as a full render, so the draft marker is \
         cleared — measured, not assumed"
    );
    assert!(
        app.has_visible_local_adjustments(),
        "the local adjustment must be the upgrade trigger under test"
    );
    assert_eq!(
        app.render_mask_layers.len(),
        1,
        "an upgraded draft is a full render, so the mask layer is evaluated"
    );
    assert_eq!(
        app.render_mask_layers[0].layer_id,
        app.selected_mask_layer().expect("selected layer").id,
        "the evaluated layer must be the selected mask's own layer"
    );
    assert!(app.mask_overlay_allowed());
    assert!(app.effective_overlay_prompt().is_some());

    // Plane resident → the VRAM composite is the single source of the matte.
    app.vram_mask_is_evaluated = true;
    assert!(
        app.gpu_mask_overlay_is_selected(),
        "an upgraded draft whose only evaluated layer is the selected mask is \
         VRAM-presentable: the composite shows the matte and the CPU painter \
         skips it, so the two paths agree"
    );

    // Plane not resident → the CPU painter must not hand the matte over.
    app.vram_mask_is_evaluated = false;
    assert!(!app.gpu_mask_overlay_is_selected());

    // A closed editorial gate still wins, exactly as in the non-draft case.
    app.vram_mask_is_evaluated = true;
    app.set_show_mask_overlay(false);
    assert!(!app.mask_overlay_allowed());
    assert!(!app.gpu_mask_overlay_is_selected());
    app.set_show_mask_overlay(true);
    let _ = id;
}
