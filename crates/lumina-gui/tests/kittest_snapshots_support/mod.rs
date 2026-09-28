//! Shared helpers for the `kittest_snapshots` integration test, extracted from
//! `kittest_snapshots.rs` (file-size ratchet DoD §8: the >500-line test file
//! must not grow for the UX-LOOK-HISTORY-18 golden seed).
//!
//! Byte-identical move of the former file-local helpers; no semantic change.
//! `kittest_snapshots.rs` pulls them in through `use
//! kittest_snapshots_support::*;`. GOLDEN-STALE-55 adds one new helper
//! (`settle_render`); the moved helpers are untouched.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use lumina_gui::{LuminaApp, SECTION_COUNT};

// KITT-SETTLE-UNIFY-53: the folder-scan wait, re-exported for
// `kittest_snapshots` (which sits at its committed size baseline and must not
// grow, so the `mod` line lives here). It has three consumers
// (`kittest_snapshots`, `kittest_crop_overlay`, `kittest_library_stack`) and
// therefore its own module: putting it in one consumer's support module would
// leave the rest of that module's helpers dead in the other two targets.
#[path = "../scan_settle_support/mod.rs"]
mod scan_settle_support;
pub(crate) use scan_settle_support::settle_scan;

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

/// R2-MODSWITCH-1 F8: `LuminaApp::set_directory` (flat listing) followed by
/// the shared folder-scan settle — the async scan must land before a
/// snapshot/assert.
///
/// KITT-SETTLE-UNIFY-53: the wait itself, with its GOLDEN-FIXT-31 wall-clock
/// bound, now lives once in `scan_settle_support`. This file used to carry a
/// copy that `kittest_library_stack` duplicated byte for byte, and
/// `kittest_crop_overlay` a third variant that could expire silently.
pub(crate) fn set_directory_and_settle(harness: &mut Harness<'_, LuminaApp>, directory: String) {
    harness.state_mut().set_directory(directory);
    settle_scan(harness);
}

/// R2-MODSWITCH-1 F8: `LuminaApp::list_directory` (recursive listing) followed
/// by the shared folder-scan settle.
pub(crate) fn list_directory_and_settle(harness: &mut Harness<'_, LuminaApp>) {
    harness.state_mut().list_directory();
    settle_scan(harness);
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

/// GOLDEN-STALE-55: pump single harness frames until the render armed by a
/// recipe/mask edit has landed (`render_key().is_some()`), then return.
///
/// A golden must capture the *settled* preview. `mark_dirty` drops the render
/// identity (`dirty.rs:42`) and only a completed render restores it, so the
/// "Stale" badge is painted exactly while the key is absent (`app_frame.rs`); a
/// snapshot taken before that render pins a transient pre-state. The golden
/// reference contract §7.2 (`feature/quality/golden-references.md`) makes this
/// the normative wait for every golden whose test triggers render work.
///
/// The wait aborts **loudly** with the state it found when the bounded step
/// count is exhausted: there is no silent fallback and the golden can never be
/// taken from an unsettled frame.
pub(crate) fn settle_render(harness: &mut Harness<'_, LuminaApp>) {
    // A full render is deferred at most once past a module-switch frame
    // (`render_schedule.rs`) and otherwise commits on the next frame; this is
    // orders of magnitude of reserve over the measured "lands in frame 0"
    // (golden-references.md §7.2). The count is bounded on purpose so a
    // regression can never hang the suite.
    const MAX_STEPS: usize = 200;
    for _ in 0..MAX_STEPS {
        if harness.state().render_key().is_some() {
            return;
        }
        harness.run_steps(1);
    }
    let app = harness.state();
    panic!(
        "settle_render: render_key stayed None after {MAX_STEPS} steps; refusing to \
         snapshot an unsettled render\n\
         found: render_key={:?}, preview_generation={}, status={:?}",
        app.render_key(),
        app.preview_generation(),
        app.status(),
    );
}
