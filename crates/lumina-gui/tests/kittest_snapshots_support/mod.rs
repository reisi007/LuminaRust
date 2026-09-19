//! Shared helpers for the `kittest_snapshots` integration test, extracted from
//! `kittest_snapshots.rs` (file-size ratchet DoD §8: the >500-line test file
//! must not grow for the UX-LOOK-HISTORY-18 golden seed).
//!
//! Byte-identical move of the former file-local helpers; no semantic change.
//! `kittest_snapshots.rs` pulls them in through `use
//! kittest_snapshots_support::*;`.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use lumina_gui::{LuminaApp, SECTION_COUNT};

/// Create a headless harness running the Lumina app at a fixed window size.
pub(crate) fn build_harness() -> Harness<'static, LuminaApp> {
    Harness::builder()
        .with_size([1024.0_f32, 720.0_f32])
        .wgpu()
        .build_eframe(|cc| LuminaApp::new(cc.egui_ctx.clone()))
}

/// Load the bundled sample image so the preview / Develop / Export modules have
/// something to render.
pub(crate) fn load_sample(harness: &mut Harness<'_, LuminaApp>) {
    harness
        .state_mut()
        .load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .expect("sample image loads");
}

/// Assert that `label` is laid out inside the 1024x720 window (not below
/// the ScrollArea fold): existence in the accesskit tree alone does not
/// prove pixel-visibility.
pub(crate) fn assert_label_on_screen(harness: &mut Harness<'_, LuminaApp>, label: &str) {
    let rect = harness
        .query_all_by_label(label)
        .next()
        .unwrap_or_else(|| panic!("label {label:?} not found in headed harness"))
        .rect();
    assert!(
        rect.min.y >= 0.0 && rect.max.y <= 720.0 && rect.max.x <= 1024.0,
        "label {label:?} must be pixel-visible in the 1024x720 viewport, got {rect:?}"
    );
}

/// Open exactly one of the Presets / History / Rating Develop headers.
///
/// Those three are plain `ui.collapsing` headers *outside* the eight
/// `section_open` F-100 sections, so `expand_and_scroll_to` cannot open
/// them: all eight sections are closed via `set_section_open` and the
/// target header is clicked open instead (default closed on a fresh
/// harness). `target_label` is then scrolled into view with the same
/// 2-frame settle as `expand_and_scroll_to` before snapshotting.
pub(crate) fn open_collapsing_and_scroll_to(
    harness: &mut Harness<'_, LuminaApp>,
    header_label: &str,
    target_label: &str,
) {
    for i in 0..SECTION_COUNT {
        harness.state_mut().set_section_open(i, false);
    }
    // Layout frame so the accesskit tree contains the (closed) headers.
    harness.run();
    let clicked = harness
        .query_all_by_label(header_label)
        .next()
        .map(|node| {
            node.click_accesskit(); // UX-LOOK-LAYOUT-18: opens the rail panels too.
            true
        })
        .unwrap_or(false);
    assert!(
        clicked,
        "Develop header {header_label:?} not found in headed harness"
    );
    for _ in 0..5 {
        harness.run();
    }
    let found = harness
        .query_all_by_label(target_label)
        .next()
        .map(|node| {
            node.scroll_to_me();
            true
        })
        .unwrap_or(false);
    assert!(
        found,
        "scroll target {target_label:?} not found in headed harness (header {header_label})"
    );
    // One frame dispatches the ScrollIntoView event, the second settles the
    // scrolled layout before snapshotting.
    harness.run();
    harness.run();
}
