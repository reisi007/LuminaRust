//! GPU-MASKPLANE-BRUSH-50: a failed live-brush upload must not leave a stale
//! VRAM plane looking current.
//!
//! Own module (not a slice of `gpu_state.rs`, which sits near its committed
//! size) and headless by construction: `vram_mask_plane_intent` is pure
//! session/render-state logic, so no adapter is involved anywhere here.
//!
//! The defect this pins: `preview_masks::handle_mask_tool_drag` sets
//! `drawing = true` *before* calling `gpu_upload_brush_tile`, and that upload
//! has three paths which write nothing (`ensure_vram` failed, the mark could
//! not be stamped, the full-plane upload failed). The resident plane is left
//! untouched by all three, while `vram_mask_plane_intent` still answers
//! `KeepLiveBrush` — so the "no stale plane resident" invariant held only
//! because it happened to hold before the gesture.
#![cfg(feature = "gpu")]

use super::*;
use crate::present_mask_plane::MaskPlaneIntent;

/// An app in the state a live brush drag is in: Masking section open, brush
/// armed, gesture active. This is exactly the triple
/// `vram_mask_plane_intent` requires before it keeps a plane.
fn live_brush_app() -> LuminaApp {
    let mut app = new_app();
    app.load_bytes(png(), "live-brush.png").unwrap();
    app.render().unwrap();
    app.set_section_open(SECTION_MASKING, true);
    app.drawing = true;
    app.mask_tool = MaskTool::Brush;
    assert!(
        app.mask_overlay_allowed(),
        "an open Masking section with an armed brush gesture must allow the matte"
    );
    app
}

/// The happy path must be untouched: a successful upload keeps the plane.
#[test]
fn a_successful_live_brush_upload_keeps_the_plane() {
    let mut app = live_brush_app();
    app.note_live_brush_upload_outcome(true);
    assert_eq!(
        app.vram_mask_plane_intent(),
        MaskPlaneIntent::KeepLiveBrush,
        "a written plane must survive the sync — this is the demo normal case"
    );
}

/// The acceptance criterion: after an upload that wrote nothing, the intent must
/// become `Clear`, so the per-render sync wipes the stale plane instead of
/// silently retaining it.
#[test]
fn a_failed_first_upload_clears_instead_of_keeping_a_stale_plane() {
    let mut app = live_brush_app();
    // The resident plane is still whatever it was before the gesture.
    app.note_live_brush_upload_outcome(false);

    assert_eq!(
        app.vram_mask_plane_intent(),
        MaskPlaneIntent::Clear,
        "an upload that wrote nothing must demote the intent to Clear, or a stale \
         plane is presented as current coverage"
    );

    // And the demotion is recoverable: the next successful dab re-arms the live
    // path, so the fix cannot strand the brush into a permanent clear.
    app.note_live_brush_upload_outcome(true);
    assert_eq!(
        app.vram_mask_plane_intent(),
        MaskPlaneIntent::KeepLiveBrush,
        "a later successful upload must re-arm the live brush plane"
    );
}

/// The demotion must not leak into unrelated states. A frame that carries
/// evaluated mask layers pushes them regardless of the live-brush flag, and a
/// gesture that is no longer running already clears.
#[test]
fn staleness_only_demotes_the_live_brush_arm() {
    let mut app = live_brush_app();
    app.note_live_brush_upload_outcome(false);

    // Gesture finished: the pre-existing Clear decision is unchanged.
    app.drawing = false;
    assert_eq!(app.vram_mask_plane_intent(), MaskPlaneIntent::Clear);

    // Evaluated layers present: Push wins, the flag is not consulted. This is
    // the ordering that keeps the stale-plane fix from suppressing a real push.
    let mut pushed = live_brush_app();
    pushed.note_live_brush_upload_outcome(false);
    pushed.render_mask_layers = vec![MaskLayerResult {
        layer_id: "layer-a".into(),
        plane: lumina_core::MaskPlane {
            width: 1,
            height: 1,
            values: vec![0],
        },
    }];
    assert_eq!(
        pushed.vram_mask_plane_intent(),
        MaskPlaneIntent::Push,
        "evaluated layers must still be pushed while the live plane is stale"
    );
}
