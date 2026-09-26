//! GUI-INT-MASKLOCAL-38: the **shared core** of the two headless mask-local
//! test targets — harness construction, the frame clock, accesskit label
//! lookups and the persisted-state readback.
//!
//! Two test targets consume this module, so it holds exactly what **both** of
//! them use: the value-clickable surfaces in `mask_local_editors.rs` and the
//! tone-curve/paint-provenance surface in `mask_local_editors_wiring.rs`.
//! Anything only one of them needs lives in a sibling module instead
//! (`mask_local_slider_support`, `mask_local_curve_graph_support`), because a
//! helper neither target calls is dead code and `-D warnings` must stay green
//! without an `allow`.
//!
//! # How widgets are addressed
//!
//! Through the egui **accesskit** tree, never through hard-coded coordinates.
//! The Masking panel repeats short labels — "Red" is both an HSL band and a
//! curve channel, "Saturation" is the HSL, vibrance *and* grading slider,
//! "Reset" appears six times — so a bare "first match" query would silently
//! address the wrong widget. The disambiguating lookups live with the test
//! that needs them (see `mask_local_slider_support::occurrence` and
//! `mask_local_curve_graph_support::below`), anchored on a panel-unique label
//! rather than a magic offset.
//!
//! # Viewport
//!
//! Deliberately **taller** than the 1024x720 golden reference: the four
//! mask-local blocks are one long column and the point of the interaction tests
//! is the input contract, not the pixel frame (`kittest_mask_local` pins the
//! pixels at the reference size). A tall viewport keeps every mask-local widget
//! on-screen, so a gesture can never be silently swallowed by a ScrollArea clip.

use eframe::egui::Rect;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use lumina_gui::{LuminaApp, Module, SECTION_COUNT, SECTION_MASKING};
use std::path::PathBuf;

/// Viewport for the interaction tests. Deliberately **taller** than the
/// 1024x720 golden reference: the four mask-local blocks are one long column
/// and the point of these tests is the input contract, not the pixel frame
/// (`kittest_mask_local` pins the pixels at the reference size). A tall
/// viewport keeps every mask-local widget on-screen so a gesture can never be
/// silently dropped by a ScrollArea clip.
pub const PANEL_VIEWPORT: [f32; 2] = [1100.0, 3600.0];

/// Simulated seconds per harness step.
///
/// The egui_kittest default is **0.25 s**, which is coarser than egui's
/// `max_double_click_delay` of 0.3 s: two clicks one step apart already exceed
/// the window, so `Response::double_clicked()` never fires and the curve
/// editor's documented *delete* gesture (double-click removes a point) cannot
/// be driven at all. 1/60 s is a real frame time and keeps a two-click gesture
/// inside the window. The debounced sidecar save (150 ms idle) is still
/// reachable — see [`DEBOUNCE_FRAMES`].
pub const STEP_DT: f32 = 1.0 / 60.0;

/// Frames to step so the 150 ms idle debounce has elapsed in simulated time.
///
/// 16 frames at [`STEP_DT`] is ~267 ms. Used instead of a fixed small number
/// because the debounce is measured in *simulated* seconds, so the frame count
/// only means something together with the step size.
pub const DEBOUNCE_FRAMES: usize = 16;

/// The mask name seeded for the layer under test. A fixed name keeps the
/// deterministic `mask-<blake3>` id stable for a fixed source, so a failure is
/// reproducible and the tempdir path never leaks into an assertion.
pub const MASK_NAME: &str = "Mask local subject";

/// Write the bundled 4x3 smoke PNG into `dir` and return its path.
///
/// A real *file* (not `load_bytes`) is deliberate: the mask-local editors arm
/// the debounced sidecar save, and an in-memory source makes that save fail
/// loudly with "the image must be loaded via a local path" — an artifact of the
/// fixture, not of the feature. With a real path the save succeeds and the
/// gesture can be verified all the way into the persisted bytes.
pub fn smoke_png(dir: &tempfile::TempDir) -> PathBuf {
    let path = dir.path().join("mask-local-source.png");
    std::fs::write(&path, LuminaApp::sample_image_png()).expect("write smoke PNG fixture");
    path
}

/// Build the harness, decode the source, seed one selected mask and open
/// exactly the Masking section.
///
/// The async decode is drained by stepping the real frame loop (the production
/// `LuminaApp::ui` calls `poll_decode`), bounded by a wall-clock deadline so a
/// genuine hang fails instead of blocking the suite.
pub fn open_masking_panel(dir: &tempfile::TempDir) -> Harness<'static, LuminaApp> {
    let mut harness = Harness::builder()
        .with_size(PANEL_VIEWPORT)
        .with_step_dt(STEP_DT)
        .build_eframe(|cc| LuminaApp::new(cc.egui_ctx.clone()));
    harness.state_mut().set_module(Module::Develop);
    let source = smoke_png(dir);
    harness.state_mut().open_file(source.display().to_string());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while harness.state().decode_pending() {
        assert!(
            std::time::Instant::now() < deadline,
            "the smoke source did not decode within the bounded deadline"
        );
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    harness.step();
    harness
        .state_mut()
        .create_mask(MASK_NAME)
        .expect("seed the mask layer under test");
    for index in 0..SECTION_COUNT {
        let open = index == SECTION_MASKING;
        harness.state_mut().set_section_open(index, open);
    }
    // Settle before the first gesture. Two jobs, both time-based, so the frame
    // count only means something together with [`STEP_DT`]:
    //
    // 1. the layout: one frame paints the opened section, the next settles it;
    // 2. the render scheduler: `create_mask` armed `pending_full_render`, and
    //    the debounced full render is what first puts a real (masked) frame
    //    behind the panel. A gesture that starts before it lands would be
    //    dispatched against a layout that is about to change.
    //
    // `step` (not `run`) is used throughout, because the render scheduler
    // requests continuous repaints while the mask-aware render is in flight
    // and `run` would trip its max-steps bound.
    settle(&mut harness, DEBOUNCE_FRAMES);
    harness
}

/// Advance the real frame loop by `frames` steps.
pub fn settle(harness: &mut Harness<'_, LuminaApp>, frames: usize) {
    harness.run_steps(frames);
}

/// Advance past the app's 150 ms idle debounce so the pending sidecar save has
/// run (see [`DEBOUNCE_FRAMES`]).
pub fn settle_persisted(harness: &mut Harness<'_, LuminaApp>) {
    settle(harness, DEBOUNCE_FRAMES);
}

/// Every accesskit node carrying `label`, in paint order.
pub fn nodes(harness: &Harness<'_, LuminaApp>, label: &str) -> Vec<Rect> {
    harness
        .query_all_by_label(label)
        .map(|node| node.rect())
        .collect()
}

/// Panel-unique label lookup: fail loudly instead of skipping a control.
pub fn only_rect(harness: &Harness<'_, LuminaApp>, label: &str) -> Rect {
    let found = nodes(harness, label);
    assert_eq!(
        found.len(),
        1,
        "expected exactly one {label:?} node in the Masking panel, got {found:?}"
    );
    found[0]
}

/// Click a widget's center: hover, press, release.
///
/// Buttons and selectable labels register the click in the same frame, so this
/// single step is the real gesture for them. The extra settle frame lets the
/// editor's own draw code run against the new value.
pub fn click(harness: &mut Harness<'_, LuminaApp>, rect: Rect) {
    let pos = rect.center();
    harness.hover_at(pos);
    harness.drag_at(pos);
    harness.step();
    harness.drop_at(pos);
    settle(harness, 2);
}

/// The persisted `local_adjustments` of the first mask layer, read back from
/// the sidecar the debounced save actually wrote.
pub fn persisted_local_recipe(dir: &tempfile::TempDir) -> lumina_sidecar::LocalAdjustments {
    let sidecar = lumina_sidecar::sidecar_path_for(&smoke_png(dir));
    let document = lumina_sidecar::load_sidecar(&sidecar).unwrap_or_else(|error| {
        panic!(
            "the debounced save must have written {}: {error}",
            sidecar.display()
        )
    });
    document.virtual_copies[0].mask_layers[0]
        .effective_local_adjustments()
        .expect("typed local recipe")
        .expect("the mask layer must carry a local recipe after an edit")
}

/// A rounded value comparison for values that crossed a slider drag. The
/// getters return `f64`; the persisted sidecar fields are `f32`, so the shared
/// tolerance covers the one rounding the serialization does.
pub fn close(left: f64, right: f64) -> bool {
    (left - right).abs() < 1e-4
}
