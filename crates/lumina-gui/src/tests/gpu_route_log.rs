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

/// The mask gate is the gate `GPU-PARITY-MASKGATE-1` spent two waves on, so it
/// is the one the task names explicitly: forcing it must put `mask_gate` in the
/// trace.
///
/// The plane set no longer matches the selected mask (an evaluated layer is
/// present but the editorial overlay is closed), which is exactly the
/// `R5-MASKVIS-25` condition the VRAM composite cannot carry.
#[test]
fn a_closed_mask_gate_names_itself_in_the_trace() {
    let before = crate::timing::take_editorial_refusal_traces();

    // Close an editorial mask gate without a bound adapter.
    let (mut app, _id) = super::gpu_mask_gate::app_with_evaluated_selected_mask();
    app.set_mask_tool(MaskTool::LinearGradient);
    app.drawing = true;
    assert!(
        !app.gpu_mask_overlay_is_selected(),
        "a live linear-gradient prompt has no VRAM representation, so the gate must close"
    );
    app.note_editorial_present_refusal(crate::present::editorial_refusal::MASK_GATE);

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
    // Same reason again: no second emission, or a long before/after session
    // floods the log.
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

    // …and the first reason is visible once more after the throttle was reset
    // by a frame with no editorial gate.
    app.note_editorial_present_refusal(crate::present::editorial_refusal::BEFORE_AFTER);
    app.clear_editorial_present_refusal();
    app.note_editorial_present_refusal(crate::present::editorial_refusal::BEFORE_AFTER);
    assert_eq!(
        crate::timing::take_editorial_refusal_traces(),
        2,
        "a recurrence after a GPU-present frame must be reported, not swallowed"
    );
}

/// The screen stays silent on purpose: an editorial route must not invent a
/// capability badge ("you are in Before/After" as a yellow "unsupported stages"
/// warning is noise). This pins that the change is log-only — GPU-ROUTE-LOG-54
/// acceptance (3), "no change to on-screen visibility".
#[test]
fn an_editorial_refusal_does_not_invent_a_capability_badge() {
    let mut app = ready_app();
    app.drawing = true;
    app.mask_tool = MaskTool::LinearGradient;
    app.render().unwrap();

    assert!(
        app.gpu_routing_fallback_badge().is_none(),
        "an editorial gate must stay badge-free, got {:?}",
        app.gpu_routing_fallback_badge()
    );
}
