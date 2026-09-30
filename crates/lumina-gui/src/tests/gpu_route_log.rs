//! GPU-ROUTE-LOG-54: the **editorial** VRAM-present refusals are explainable in
//! the log (`RUST_LOG=trace`), throttled once per reason change.
//!
//! Own file rather than a slice of `tests/gpu_routing.rs`: that module sits at
//! its committed size under the file-size ratchet, and the project rule is to
//! extract a new thematic module instead of growing one (`Agents.md`, ratchet
//! note in `crop_display_and_denoise_policy.rs`).
//!
//! What is under test, and what is deliberately *not*:
//!
//! * The four editorial gates (`vram_stale`, `before_after`, `preview_roi`,
//!   `mask_gate`) each name themselves in the trace. Headless and adapter-free:
//!   all four are checked **before** the code touches `self.gpu`, so a plain
//!   `LuminaApp` with no adapter reaches them — the test needs no wgpu.
//! * The throttle emits on a reason *change* and stays quiet on a repeat, and a
//!   frame with no editorial gate re-arms it.
//! * **Not** under test: that the routing decision is unchanged (the badge and
//!   pixel behaviour are untouched by construction — the gate order is verbatim
//!   the historical combined condition), and anything requiring a real adapter.
//!   This is a local claim about the trace, not GPU parity.
#![cfg(feature = "gpu")]

use super::*;

/// A loaded, rendered app with a bound preview: the state in which an editorial
/// gate is the *only* thing that can stop a VRAM present.
fn ready_app() -> LuminaApp {
    let mut app = new_app();
    app.load_bytes(png(), "editorial-refusal.png").unwrap();
    app.render().unwrap();
    // The mask gate must not be closed by accident in the "no editorial gate"
    // cases: with no mask layers it evaluates to `true`
    // (`gpu_mask_overlay_is_selected` returns `render_mask_layers.is_empty()`).
    assert!(app.gpu_mask_overlay_is_selected());
    app
}

/// An app in the state a live editorial gesture is in: the Masking section open
/// and a gradient drag armed, so the editorial gate is *actually* closed.
///
/// Both halves are required, and the order matters. `mask_overlay_allowed` is
/// multiplicative: a **closed** section short-circuits to
/// `render_mask_layers.is_empty()` (i.e. `true`), so arming the tool alone never
/// closes the gate — the first version of these tests asserted a closed gate on a
/// state where it cannot close at all. And the live-gesture branch deliberately
/// tolerates "no mask selected yet" (a drag can begin before a default mask
/// exists) while the non-gesture branch requires `selected_mask_id`, which is
/// `None` on a fresh app. Opening the section and *then* arming the gesture is
/// the only ordering that reaches the branch under test.
fn live_editorial_app() -> LuminaApp {
    let mut app = ready_app();
    app.set_section_open(SECTION_MASKING, true);
    app.drawing = true;
    app.mask_tool = MaskTool::LinearGradient;
    assert!(
        app.mask_overlay_allowed(),
        "an open Masking section with an armed live gesture must allow the matte"
    );
    assert!(
        !app.gpu_mask_overlay_is_selected(),
        "a live gradient prompt has no VRAM representation, so the gate must close"
    );
    app
}

/// The mask gate is the gate `GPU-PARITY-MASKGATE-1` spent two waves on, so it
/// is the one the task names explicitly: forcing it must put `mask_gate` in the
/// trace. This is the `R5-MASKVIS-25` condition the VRAM composite cannot carry.
#[test]
fn a_closed_mask_gate_names_itself_in_the_trace() {
    let mut app = live_editorial_app();
    let before = crate::timing::take_editorial_refusal_traces();
    // Drive the REAL present path, not the throttle helper: calling
    // `note_editorial_present_refusal` from the test would pass even with the
    // production wiring deleted (measured — see the mutation in the task notes).
    // `gpu_present_if_ready` evaluates every editorial gate before it touches any
    // GPU state, so this reaches the mask gate on an adapter-less machine.
    app.vram_fresh = true;
    assert!(
        app.gpu_present_if_ready().is_none(),
        "a closed editorial gate must fall back to the CPU present"
    );

    assert_eq!(
        crate::timing::take_editorial_refusal_traces() - before,
        1,
        "closing the mask gate must emit exactly one editorial refusal trace"
    );
}

/// GPU-ROUTE-LOG-54 acceptance (2): the reason must be the **concrete branch**.
/// A blanket word like "gate closed" would satisfy a presence check and still
/// leave an `RUST_LOG=trace` run unable to tell the four gates apart, which is
/// the whole point of the task.
#[test]
fn each_editorial_gate_reports_its_own_reason() {
    // The four constants are the contract; pin them so a rename that collapses
    // two of them into one string fails here instead of silently degrading the
    // log's diagnostic value.
    let reasons = [
        crate::present::editorial_refusal::VRAM_STALE,
        crate::present::editorial_refusal::BEFORE_AFTER,
        crate::present::editorial_refusal::PREVIEW_ROI,
        crate::present::editorial_refusal::MASK_GATE,
    ];
    let unique: std::collections::BTreeSet<&str> = reasons.iter().copied().collect();
    assert_eq!(
        unique.len(),
        reasons.len(),
        "each editorial gate must report a distinct reason: {reasons:?}"
    );
    for expected in ["vram_stale", "before_after", "preview_roi", "mask_gate"] {
        assert!(
            unique.contains(expected),
            "the documented reason {expected} must exist verbatim, got {reasons:?}"
        );
    }
}

/// GPU-ROUTE-LOG-54 acceptance (2): debounced. The same reason on consecutive
/// frames emits once; a *change* of reason emits again. Without the second half
/// a session could pass this test with a throttle that never re-arms.
#[test]
fn the_editorial_refusal_trace_is_throttled_but_repeats_on_a_change() {
    let mut app = ready_app();
    let before = crate::timing::take_editorial_refusal_traces();

    app.note_editorial_present_refusal(crate::present::editorial_refusal::BEFORE_AFTER);
    // Same reason again: no second *counted* emit (an `unchanged` frame still
    // logs a line; what is throttled is the counted reason change).
    app.note_editorial_present_refusal(crate::present::editorial_refusal::BEFORE_AFTER);
    app.note_editorial_present_refusal(crate::present::editorial_refusal::BEFORE_AFTER);
    assert_eq!(
        crate::timing::take_editorial_refusal_traces() - before,
        1,
        "an unchanged reason must not re-emit"
    );

    // A different reason is a new fact and must be visible.
    app.note_editorial_present_refusal(crate::present::editorial_refusal::MASK_GATE);
    assert_eq!(
        crate::timing::take_editorial_refusal_traces(),
        1,
        "a changed reason must emit again"
    );

    // …and a reason that recurs after a frame with no editorial gate is a
    // genuinely new occurrence. The memo holds `MASK_GATE` from the block
    // above, so `clear` is what makes the repeat below observable; without the
    // clear this would still be throttled, which is the point.
    app.clear_editorial_present_refusal();
    app.note_editorial_present_refusal(crate::present::editorial_refusal::BEFORE_AFTER);
    assert_eq!(
        crate::timing::take_editorial_refusal_traces(),
        1,
        "a recurrence after a GPU-present frame must be reported, not swallowed"
    );
}

/// GPU-ROUTE-LOG-54 acceptance (1): the emitted **line text** is the deliverable
/// — an operator reading an `RUST_LOG=trace` run cannot see a counter. This test
/// is what was missing: `note_editorial_present_refusal` used to call `trace!`
/// directly, bypassing `emit`, so the line was never recorded and no in-process
/// test could read it. Deleting `reason={reason}` from both lines kept the whole
/// suite green — the acceptance criterion was half-pinned.
///
/// Both lines are reached through the **real** present path, not the throttle
/// helper: the first refusal takes the "keeping CPU route" branch, the second
/// hits the memo and takes the "unchanged" branch, so neither line is produced by
/// a call a production frame would not make.
///
/// The match is **exact**, which is the point: the string a developer greps for
/// is the contract. A presence check for `reason=` would pass on a line whose
/// branch is unnamed, which is exactly the degradation this task exists to stop.
#[test]
fn the_editorial_refusal_lines_carry_the_reason_a_trace_run_needs() {
    let mut app = live_editorial_app();
    let _ = crate::timing::take_timing_log();
    app.vram_fresh = true;

    for _ in 0..2 {
        assert!(
            app.gpu_present_if_ready().is_none(),
            "a closed editorial gate must fall back to the CPU present"
        );
    }

    let log = crate::timing::take_timing_log();
    assert!(
        log.contains(&format!(
            "GUI timing: editorial present refusal, keeping CPU route reason={}",
            crate::present::editorial_refusal::MASK_GATE
        )),
        "the first refusal must name the branch and the concrete reason: {log:?}"
    );
    assert!(
        log.contains(&format!(
            "GUI timing: editorial present refusal unchanged reason={}",
            crate::present::editorial_refusal::MASK_GATE
        )),
        "the throttled repeat must still name the reason, or a trace run cannot \
         tell which gate held: {log:?}"
    );
}

/// The screen stays silent on purpose: an editorial route must not invent a
/// capability badge ("you are in Before/After" as a yellow "unsupported stages"
/// warning is noise). This pins that the change is log-only — GPU-ROUTE-LOG-54
/// acceptance (3), "no change to on-screen visibility".
///
/// The test drives [`LuminaApp::update_texture`], **not** just the accessor:
/// `gpu_route_fallback` is only ever written at the two sites inside
/// `update_texture` (`present.rs`), so a test that merely reads
/// `gpu_routing_fallback_badge()` on a fresh app asserts `None` for a field
/// nothing has touched — it cannot detect a new badge. Going through
/// `update_texture` is what makes it non-vacuous: the same call that would
/// install a badge is the one that must instead produce the editorial trace.
///
/// It is headless: `update_texture` reaches `gpu_present_if_ready` and returns
/// before touching any wgpu state, because every editorial gate is evaluated
/// first.
#[test]
fn an_editorial_refusal_does_not_invent_a_capability_badge() {
    let mut app = live_editorial_app();
    let ctx = egui::Context::default();
    let before = crate::timing::take_editorial_refusal_traces();

    app.update_texture(&ctx);

    // Precondition for the badge assertion below: the call really did take the
    // editorial CPU route. Without this, "no badge" would also hold on a path
    // that never routed at all.
    assert_eq!(
        crate::timing::take_editorial_refusal_traces() - before,
        1,
        "update_texture must have taken the editorial CPU route, otherwise the \
         badge assertion below proves nothing"
    );
    assert!(
        app.gpu_routing_fallback_badge().is_none(),
        "an editorial route must stay badge-free, got {:?}",
        app.gpu_routing_fallback_badge()
    );
}
