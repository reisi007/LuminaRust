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
use crate::present_mask_plane::{
    zero_band_rows, MaskPlaneIntent, VramMaskPlaneResidency, ZERO_BAND_BYTES,
};

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

    // The same *intent* holds for the shipped default state, which never had a
    // mask: the frame carries no coverage, so the sync asks for `Clear`. The
    // **write** behind that intent is what GPU-CLEAR-TRANSITION
    // (`GPU-MASKPLANE-CLEAR-PERF-51`) narrowed — without a prior non-zero plane
    // this state performs no zero write at all, pinned by
    // `a_maskless_render_without_a_prior_plane_clears_nothing` below. So the
    // clear is not conditional on a mask having existed; the *number of writes*
    // is.
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

// ===========================================================================
// GPU-MASKPLANE-CLEAR-PERF-51 / `GPU-CLEAR-TRANSITION`: the zero write runs
// only on the transition "non-null plane resident → no layer".
//
// What is *not* pinned here: the bytes on the device. Half of the condition
// (reading the active pool entry key) and the write itself need an adapter, so
// the numbers below count decisions and runs of the clear routine — the device
// half is the `GPU-E2E` gate (`--features gpu-adapter-tests` plus the affected
// GUI-GPU tests).

/// An app in the state a live brush drag leaves behind: the gesture is running,
/// and the present path composites the plane it uploads into.
fn app_with_resident_live_brush_plane() -> LuminaApp {
    let mut app = new_app();
    app.load_bytes(png(), "gpu-mask-plane-transition.png")
        .unwrap();
    app.render().unwrap();
    app.set_section_open(SECTION_MASKING, true);
    app.set_mask_tool(MaskTool::Brush);
    app.drawing = true;
    app.sync_mask_plane_to_vram();
    assert_eq!(
        app.vram_mask_plane_clear.residency,
        VramMaskPlaneResidency::NonZero,
        "the live-brush arm must mark the composited plane as coverage — it is \
         what makes the maskless render after the gesture write zeros once"
    );
    app
}

/// (b) The transition writes zeros **exactly once**: the render that drops the
/// last coverage, and no render after it.
#[test]
fn a_maskless_render_after_coverage_clears_exactly_once() {
    let mut app = app_with_resident_live_brush_plane();

    // The gesture ends, so the frame carries no coverage any more while the
    // plane still holds what the brush wrote.
    app.drawing = false;
    assert_eq!(app.vram_mask_plane_intent(), MaskPlaneIntent::Clear);
    assert!(
        app.vram_mask_plane_needs_zero_write(),
        "the transition 'non-null plane resident → no layer' must write zeros"
    );
    let writes_before = app.vram_mask_plane_clear.zero_writes;
    app.sync_mask_plane_to_vram();
    assert_eq!(
        app.vram_mask_plane_clear.zero_writes,
        writes_before + 1,
        "the transition render performs the zero write"
    );

    // Every further maskless render sees a plane that reads zero. Before
    // `GPU-CLEAR-TRANSITION` each of these paid the write again — 37 µs / 722 µs
    // / 15,21 ms at 160×120 / 1280×853 / 6000×4000 (F-074-N10).
    for frame in 1..=4 {
        assert_eq!(
            app.vram_mask_plane_intent(),
            MaskPlaneIntent::Clear,
            "frame {frame} still carries no coverage"
        );
        assert!(
            !app.vram_mask_plane_needs_zero_write(),
            "frame {frame} must not rewrite a plane that reads zero"
        );
        app.sync_mask_plane_to_vram();
        assert_eq!(
            app.vram_mask_plane_clear.zero_writes,
            writes_before + 1,
            "frame {frame} must not add a second zero write"
        );
    }
}

/// (c) A maskless render with **no** non-null plane behind it writes nothing —
/// not once, and not on any later frame. This is the shipped default state: a
/// session that never painted a mask.
#[test]
fn a_maskless_render_without_a_prior_plane_clears_nothing() {
    let mut app = new_app();
    app.load_bytes(png(), "gpu-mask-plane-cold.png").unwrap();
    app.render().unwrap();
    assert!(
        app.render_mask_layers.is_empty(),
        "precondition: no evaluated layer"
    );
    assert_eq!(app.vram_mask_plane_intent(), MaskPlaneIntent::Clear);
    assert_eq!(
        app.vram_mask_plane_clear.residency,
        VramMaskPlaneResidency::Zero,
        "nothing has been written in this session, and every pooled entry is \
         created with a zeroed mask texture"
    );

    for frame in 1..=4 {
        assert!(
            !app.vram_mask_plane_needs_zero_write(),
            "frame {frame} has no plane to zero"
        );
        app.sync_mask_plane_to_vram();
    }
    assert_eq!(
        app.vram_mask_plane_clear.zero_writes, 0,
        "a session without coverage never writes zeros"
    );
}

/// The zero verdict is bound to the entry it was made in. The pool keys entries
/// by dimensions and re-points its active entry on every `ensure_vram` without
/// clearing a reactivated entry, so a zeroed entry that is no longer the active
/// one carries no verdict at all — and a verdict that *did* carry over would
/// present a reactivated entry's old coverage.
#[test]
fn a_zeroed_entry_that_is_no_longer_active_arms_the_next_write() {
    let mut app = new_app();
    app.load_bytes(png(), "gpu-mask-plane-entry.png").unwrap();
    let zeroed = (1280u32, 853u32);
    app.vram_mask_plane_clear.residency = VramMaskPlaneResidency::ZeroedIn(zeroed);

    // Same entry: the verdict holds.
    assert!(!app.vram_mask_plane_clear.needs_zero_write(Some(zeroed)));
    // Source or resolution switch: a different entry is active now. It may hold
    // an old plane, so the conservative answer is to write it.
    assert!(app
        .vram_mask_plane_clear
        .needs_zero_write(Some((6000, 4000))));
}

/// A zero write that stopped part-way leaves coverage behind (the bands it did
/// write are zero, the rest is not), so the verdict must stay non-zero and keep
/// asking for the write. This pins the **state** the failure leaves, which is
/// what the next render's decision reads.
///
/// What is **not** tested, stated plainly rather than covered by a claim: the
/// failure itself. Reaching `ZeroWriteOutcome::Incomplete`
/// (`present_mask_plane.rs`, the `Incomplete` arm of
/// `clear_active_vram_mask_plane`) needs an adapter whose `upload_mask_tile`
/// fails, and no test in this tree drives it — measured by name:
/// `ZeroWriteOutcome` occurs only in `present_mask_plane.rs`, in no test file,
/// and the tests that attach a real `GpuContext` never make an upload fail. So
/// the earlier wording "is part of the `GPU-E2E` gate" is withdrawn: an adapter
/// run alone does not exercise this arm, it needs a device that refuses a write.
/// The band geometry it walks is pinned by
/// `zero_bands_cover_every_row_of_the_active_entry`; the *bytes* of a failed
/// clear are a named open gate.
#[test]
fn a_partial_zero_write_keeps_asking_for_the_write() {
    let mut app = new_app();
    app.load_bytes(png(), "gpu-mask-plane-partial.png").unwrap();
    app.vram_mask_plane_clear.residency = VramMaskPlaneResidency::NonZero;
    assert!(
        app.vram_mask_plane_needs_zero_write(),
        "a plane that is only partly zero still holds coverage"
    );
}

/// The device half of the transition, so it is not carried by reasoning alone:
/// on a machine with a usable adapter the maskless render after a mask render
/// leaves a plane that reads back as **zero**, and the maskless render after
/// *that* performs no write at all. It also checks (c) on the device **first**:
/// a session that never painted a mask has no resident plane, so its maskless
/// render must not write at all — the mask is created only after that check.
/// The decision tests above pin the same
/// property without an adapter; this one measures the bytes. Without one it
/// skips loudly rather than passing silently (`Agents.headless.md` §1/§3) — a
/// test that does not run is neither green nor red.
///
/// FIXTURE-SKIP-VISIBLE-2: the skip reason goes to the **real** stderr through
/// `lumina_testskip`, because libtest discards the captured output of a
/// *passing* test. The plain `eprintln!` this replaces was measured invisible:
/// `cargo test -p lumina-gui --lib -- --test-threads=1 a_maskless_render_zeroes`
/// reported `ok` with **0** hits for the reason (`grep -c "no GPU adapter"` and
/// `grep -c "skipping the adapter half"` both 0; the second grep is the arm a
/// headless host actually takes — `GpuContext::new()` succeeds, `is_available()`
/// does not). This is not an `#[ignore]` gate — the test runs in the normal
/// suite and exits early when the adapter is absent — so
/// `check_env_gate_reasons.sh` does not cover it and the reporter is used
/// directly.
#[test]
#[cfg(feature = "gpu")]
fn a_maskless_render_zeroes_the_plane_once_on_a_real_adapter() {
    let mut app = new_app();
    app.load_bytes(png(), "gpu-mask-plane-cold.png").unwrap();
    let Some(gpu) = lumina_gpu::GpuContext::new().ok() else {
        lumina_testskip::report_env_skip(
            "a_maskless_render_zeroes_the_plane_once_on_a_real_adapter",
            "no GPU adapter; skipping the adapter half of the transition test",
        );
        return;
    };
    if !gpu.is_available() {
        lumina_testskip::report_env_skip(
            "a_maskless_render_zeroes_the_plane_once_on_a_real_adapter",
            "GPU unavailable; skipping the adapter half of the transition test",
        );
        return;
    }
    app.gpu = Some(gpu);

    // (c) on the device: this session never painted a mask, so the frame
    // carries no coverage and no entry holds one either — the maskless render
    // must not write zeros. The mask is created only *after* this check: an app
    // that already rendered a mask would be the *transition* state (b) below,
    // not the maskless-without-a-prior-plane state (c) is about.
    app.render().unwrap();
    assert!(
        app.render_mask_layers.is_empty(),
        "precondition: no evaluated layer"
    );
    assert_eq!(app.vram_mask_plane_intent(), MaskPlaneIntent::Clear);
    assert!(
        !app.vram_mask_plane_needs_zero_write(),
        "criterion (c) on the device: nothing resident, no write"
    );
    assert_eq!(app.vram_mask_plane_clear.zero_writes, 0);

    // Real coverage: the evaluated layer is combined and pushed into the entry
    // the present path composites.
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
    assert_eq!(app.vram_mask_plane_intent(), MaskPlaneIntent::Push);
    assert!(
        app.vram_mask_is_evaluated,
        "the combined plane must have been written"
    );

    // The transition: deleting the last mask empties the layer list, so this
    // render writes zeros — once — and the plane must read back as zero.
    app.delete_mask(&id).unwrap();
    app.render().unwrap();
    assert_eq!(
        app.vram_mask_plane_clear.zero_writes, 1,
        "the transition render is the one that writes zeros"
    );
    let (width, height, values) = app
        .gpu
        .as_ref()
        .expect("adapter attached above")
        .readback_mask_plane()
        .expect("mask plane readback");
    assert!(
        values.iter().all(|&value| value == 0),
        "the cleared {width}x{height} plane must read zero"
    );

    // The frame after it: same maskless state, no write. This is the half that
    // removes 37 µs / 722 µs / 15,21 ms per maskless render (F-074-N10).
    app.sync_mask_plane_to_vram();
    assert_eq!(
        app.vram_mask_plane_clear.zero_writes, 1,
        "a second maskless render must not rewrite a zero plane"
    );
}
