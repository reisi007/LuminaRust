//! F-103-N9: headless UI snapshot regression tests via `egui_kittest`.
//!
//! These tests render the Lumina GUI with the wgpu backend and compare the
//! rendered frame against a committed golden PNG under `tests/snapshots/`.
//!
//! They require a working GPU / headless wgpu backend, so they are `#[ignore]`d
//! by default. CI without a GPU therefore stays green. Run them locally with:
//!
//! ```text
//! UPDATE_SNAPSHOTS=true cargo test -p lumina-gui --test kittest_snapshots -- --ignored
//! ```
//!
//! `UPDATE_SNAPSHOTS=true` writes/updates the goldens; without it the tests
//! compare against the committed goldens and fail (producing a `<name>.diff.png`)
//! when the rendered UI changed.
//!
//! A red snapshot means the rendered UI differs from the committed golden. Most
//! often this is a legitimate UI change that needs a golden refresh; occasionally
//! it is a regression. Inspect the generated `<name>.diff.png` and, if the change
//! is intended, re-run with `UPDATE_SNAPSHOTS=true`.

use egui_kittest::{kittest::Queryable, Harness};
use lumina_gui::{LuminaApp, Module};

/// Documented reason for `#[ignore]` so CI without a GPU stays green:
/// "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"
/// (repeated inline on each `#[ignore]` attribute below, since the attribute
/// requires a string literal).
///
/// Collapsing-header labels of the Develop panel, in draw order.
///
/// Kept in sync with `LuminaApp::DEVELOP_SECTIONS` (single source of truth in
/// `crates/lumina-gui/src/lib.rs`, labels via `Str::*` in
/// `crates/lumina-gui/src/i18n.rs`): `Presets` + `History` (top, collapsible)
/// followed by the eight F-100 sections `Basic` … `Masking`.
/// F-103-N10 (user decision 2026-08-25): Detail BEFORE Effects (Lightroom
/// Classic panel order). NOTE: the committed goldens still show the old
/// Effects-before-Detail layout and need a one-off refresh on a GPU machine:
/// `UPDATE_SNAPSHOTS=true cargo test -p lumina-gui --test kittest_snapshots -- --ignored`
const DEVELOP_SECTIONS: &[&str] = &[
    "Presets",
    "History",
    "Basic",
    "Tone Curve",
    "Color",
    "Detail",
    "Effects",
    "Optics",
    "Geometry",
    "Masking",
];

/// Create a headless harness running the Lumina app at a fixed window size.
fn build_harness() -> Harness<'static, LuminaApp> {
    Harness::builder()
        .with_size([1024.0_f32, 720.0_f32])
        .wgpu()
        .build_eframe(|cc| LuminaApp::new(cc.egui_ctx.clone()))
}

/// Load the bundled sample image so the preview / Develop / Export modules have
/// something to render.
fn load_sample(harness: &mut Harness<'_, LuminaApp>) {
    harness
        .state_mut()
        .load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .expect("sample image loads");
}

/// Expand exactly the Develop sections listed in `keep`.
///
/// egui 0.36 `CollapsingHeader` is **closed by default** (previously open), so
/// the old "collapse everything except keep" logic kept all ten sections closed.
/// The correct operation is now to *open* the keep-sections. Each `click()` is
/// followed by a `harness.run()` so the click event is dispatched before the
/// next query — bundling all clicks into a single final `run()` only scrolls the
/// `ScrollArea` and the clicks never register (verified via diff of the two
/// goldens: previously both showed all 9 sections collapsed).
fn collapse_except(harness: &mut Harness<'_, LuminaApp>, keep: &[&str]) {
    // Ensure the first frame is laid out so `query_all_by_label` can find the
    // headers (egui_kittest requires a `run()` before querying).
    harness.run();
    for section in keep {
        // `DEVELOP_SECTIONS` is the authoritative label list; keep-entries must
        // be a subset of it — a missing label is a test bug, not a silent skip.
        assert!(
            DEVELOP_SECTIONS.contains(section),
            "unknown Develop section label {:?}; expected one of {:?}",
            section,
            DEVELOP_SECTIONS
        );
        let clicked = {
            if let Some(node) = harness.query_all_by_label(section).next() {
                node.click();
                true
            } else {
                panic!("Develop section label not found in headed harness: {section:?}");
            }
        };
        if clicked {
            harness.run();
        }
    }
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_empty() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    harness.run();
    harness.snapshot("library_empty");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_with_image() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    load_sample(&mut harness);
    harness.run();
    harness.snapshot("library_with_image");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_basic() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only the Basic section is expanded; everything else is collapsed.
    collapse_except(&mut harness, &["Basic"]);
    harness.snapshot("develop_basic");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_sections_expanded() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Basic + Color + Masking expanded; the rest collapsed.
    collapse_except(&mut harness, &["Basic", "Color", "Masking"]);
    harness.snapshot("develop_sections_expanded");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn export_module() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Export);
    load_sample(&mut harness);
    harness.run();
    harness.snapshot("export_module");
}
