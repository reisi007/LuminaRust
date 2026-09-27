//! GPU-PARITY-MASKGATE-1: the VRAM mask-plane lifecycle of the present
//! composite (`src/present_mask_plane.rs`).
//!
//! The pixel-level counterpart of this file is
//! `tests/kittest_parity_support/mask_gate.rs::mask_gate_stale_pooled_mask_plane_is_an_open_finding`,
//! which measures the same property at really presented frames. Here the
//! *decision* and the *band geometry* are pinned without an adapter, because
//! both are pure functions of session/render state:
//!
//! * [`a_frame_without_coverage_clears_the_plane_instead_of_skipping`] — the
//!   repaired defect: a frame that carries no coverage must **clear** the
//!   pooled plane, not skip the upload. Skipping is what left a deleted mask's
//!   coverage resident while case 1 presented it (`maxAbsDiff=67` over 127 707
//!   of 603 904 presented photo bytes on a real adapter).
//! * [`a_live_brush_plane_is_preserved_while_the_present_gate_composites_it`] —
//!   the one coverage the clear must not destroy: an incrementally uploaded
//!   live brush plane, which a full-plane write would wipe outside the current
//!   dab.
//! * [`evaluated_layers_still_take_the_push_path`] — the historical path is
//!   untouched.
//! * [`zero_bands_cover_every_row_of_the_active_entry`] — the clear is bounded
//!   in memory but must still cover the whole plane.

#![cfg(feature = "gpu")]

use super::*;
use crate::present_mask_plane::{zero_band_rows, MaskPlaneIntent, ZERO_BAND_BYTES};

/// A loaded source with one brushed mask whose prompt is evaluated into the
/// render. The caller then removes or keeps the coverage.
fn app_with_evaluated_mask() -> (LuminaApp, String) {
    let mut app = new_app();
    app.load_bytes(png(), "gpu-mask-plane.png").unwrap();
    let id = app.create_mask("Plane").unwrap();
    app.commit_brush_stroke(vec![BrushMark {
        x: 0.5,
        y: 0.5,
        radius: 0.2,
        sign: BrushMarkSign::Positive,
        softness: 0.25,
        flow: 0.75,
    }])
    .unwrap();
    app.set_section_open(SECTION_MASKING, true);
    app.render().unwrap();
    (app, id)
}

/// The repaired defect, pinned at the decision: a frame with no coverage must
/// ask for a **clear**, because the pooled entry keeps whatever was uploaded
/// last and the present path would then composite a deleted mask.
#[test]
fn a_frame_without_coverage_clears_the_plane_instead_of_skipping() {
    let (mut app, id) = app_with_evaluated_mask();
    assert_eq!(
        app.vram_mask_plane_intent(),
        MaskPlaneIntent::Push,
        "precondition: an evaluated layer is pushed"
    );

    // Deleting the last mask empties the layer list. The plane in the pool is
    // untouched by that, so the sync has to *write* the empty state.
    app.delete_mask(&id).unwrap();
    app.render().unwrap();
    assert!(
        app.render_mask_layers.is_empty(),
        "precondition: the frame carries no evaluated layer any more"
    );
    assert_eq!(
        app.vram_mask_plane_intent(),
        MaskPlaneIntent::Clear,
        "a frame without coverage must clear the pooled mask plane — skipping \
         the upload is what presented a deleted mask as the current frame \
         (measured maxAbsDiff=67 over 127707 of 603904 presented photo bytes)"
    );

    // The same holds for the shipped default state, which never had a mask:
    // the clear is not conditional on a mask having existed.
    let mut fresh = new_app();
    fresh
        .load_bytes(png(), "gpu-mask-plane-default.png")
        .unwrap();
    fresh.render().unwrap();
    assert!(fresh.render_mask_layers.is_empty());
    assert_eq!(fresh.vram_mask_plane_intent(), MaskPlaneIntent::Clear);
}

/// The complementary half: a live brush plane is uploaded tile by tile, so the
/// clear must not run while the present gate composites it. The predicate is
/// the same triple the gate's live-gesture branch requires — section, mode,
/// Show switch and the legacy overlay mode included — which is what keeps the
/// two decisions from drifting apart.
#[test]
fn a_live_brush_plane_is_preserved_while_the_present_gate_composites_it() {
    let mut app = new_app();
    app.load_bytes(png(), "gpu-mask-plane-live.png").unwrap();
    app.set_section_open(SECTION_MASKING, true);
    app.set_mask_tool(MaskTool::Brush);
    app.drawing = true;
    app.drag_start = Some(Point2 { x: 0.2, y: 0.5 });
    app.drag_current = Some(Point2 { x: 0.8, y: 0.5 });
    app.pending_brush_marks.push(BrushMark {
        x: 0.5,
        y: 0.5,
        radius: 0.1,
        sign: BrushMarkSign::Positive,
        softness: 0.0,
        flow: 1.0,
    });

    // Non-vacuity: the gate really does let the VRAM composite this frame, so
    // preserving the plane is required rather than cosmetic.
    assert!(app.mask_overlay_allowed());
    assert!(app.effective_overlay_prompt().is_some());
    assert!(app.gpu_mask_overlay_is_selected());
    assert_eq!(
        app.vram_mask_plane_intent(),
        MaskPlaneIntent::KeepLiveBrush,
        "a live brush plane is the one coverage a full-plane write would destroy"
    );

    // A gradient/radial gesture has no VRAM representation at all, so there is
    // no plane to preserve — the clear is correct there. `set_mask_tool` also
    // ends the gesture, so the drag endpoints are re-armed like the
    // production sequence does.
    app.set_mask_tool(MaskTool::LinearGradient);
    app.drawing = true;
    app.drag_start = Some(Point2 { x: 0.2, y: 0.5 });
    app.drag_current = Some(Point2 { x: 0.8, y: 0.5 });
    assert!(
        !app.gpu_mask_overlay_is_selected(),
        "a live gradient prompt is composited by the CPU painter only"
    );
    assert_eq!(app.vram_mask_plane_intent(), MaskPlaneIntent::Clear);

    // The coupling that makes the case-1 relaxation sound: a closed editorial
    // gate with no evaluated layer lets the VRAM route through again, and that
    // is only pixel-equal because the sync zeroes the plane on that very
    // render. Both halves are asserted here, in the state where they meet.
    app.set_mask_tool(MaskTool::Brush);
    app.drawing = true;
    app.drag_start = Some(Point2 { x: 0.2, y: 0.5 });
    app.drag_current = Some(Point2 { x: 0.8, y: 0.5 });
    app.set_show_mask_overlay(false);
    assert!(!app.mask_overlay_allowed());
    assert_eq!(
        app.vram_mask_plane_intent(),
        MaskPlaneIntent::Clear,
        "with the Show switch closed the resident live plane is no longer what \
         the frame shows, so it must not survive the render"
    );
    app.set_show_mask_overlay(true);
    assert!(
        app.gpu_mask_overlay_is_selected(),
        "re-opening the gate re-composites the live plane, so this state is the \
         live-brush case and not the layerless one"
    );
    assert_eq!(app.vram_mask_plane_intent(), MaskPlaneIntent::KeepLiveBrush);
}

/// The historical path must be untouched by the clear: an evaluated layer is
/// still combined and pushed, and the per-frame "evaluated" flag is what the
/// gate's selection branch reads.
#[test]
fn evaluated_layers_still_take_the_push_path() {
    let (mut app, _id) = app_with_evaluated_mask();
    assert_eq!(app.render_mask_layers.len(), 1);
    assert_eq!(app.vram_mask_plane_intent(), MaskPlaneIntent::Push);

    // The flag is per frame and needs no adapter to observe: the sync clears it
    // at its start, so a stale `true` from an earlier frame cannot survive.
    app.vram_mask_is_evaluated = true;
    app.sync_mask_plane_to_vram();
    assert!(
        !app.vram_mask_is_evaluated,
        "a sync without an adapter pushes nothing, so the evaluated flag must \
         not claim a resident plane"
    );
}

/// The clear writes full-width row bands, so the plane is covered even when the
/// resolution is far above the memory budget. Checked here as arithmetic; the
/// cell in `kittest_parity_support/mask_gate.rs` measures the resulting pixels.
#[test]
fn zero_bands_cover_every_row_of_the_active_entry() {
    // A draft-sized entry is split into a handful of bands, never a single
    // unbounded write: 1 MiB / (1280 px * 2 bytes) = 409 rows.
    assert_eq!(zero_band_rows(1280, ZERO_BAND_BYTES), 409);
    assert!(
        zero_band_rows(1280, ZERO_BAND_BYTES) as usize * 1280 * 2 <= ZERO_BAND_BYTES,
        "a full band must stay within the budget"
    );

    // A full-resolution entry is split further, and the split still covers every
    // row exactly once: the loop is `y += rows` with the last band clipped.
    let (width, height) = (6000u32, 4000u32);
    let rows = zero_band_rows(width, ZERO_BAND_BYTES);
    assert!(rows < height, "a 45 MP entry must really be split");
    let mut covered = 0usize;
    let mut y = 0u32;
    while y < height {
        let band = rows.min(height - y);
        assert!(band >= 1, "a band must always make progress");
        assert!(
            (width as usize * band as usize) * 2 <= ZERO_BAND_BYTES,
            "every band must stay within the memory budget"
        );
        covered += band as usize;
        y += band;
    }
    assert_eq!(
        covered, height as usize,
        "every row is written exactly once"
    );

    // Degenerate inputs must not divide by zero or stall the loop.
    assert_eq!(zero_band_rows(0, ZERO_BAND_BYTES), 1);
    assert_eq!(
        zero_band_rows(64, 1),
        1,
        "a budget below one row still yields one row"
    );
}
