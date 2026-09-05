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

use egui_kittest::kittest::NodeT;
use egui_kittest::{kittest::Queryable, Harness};
use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_gui::{
    LuminaApp, Module, SECTION_COLOR, SECTION_COUNT, SECTION_DETAIL, SECTION_EFFECTS,
    SECTION_GEOMETRY, SECTION_MASKING, SECTION_OPTICS, SECTION_TONE_CURVE,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
/// Classic panel order). The committed goldens were rebaselined to the
/// Detail-before-Effects layout (GUI-KIT-01-REFRESH).
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

/// Fixture directory for the deterministic Library snapshots below.
///
/// `library_empty` / `library_with_image` used to render with the default
/// workdir (`"."` = the live crate checkout), so the Folders tree listed
/// whatever the checkout contained (`src/`, `benches/`, …) and the goldens
/// broke on every unrelated file addition (e.g. a 415px diff from a single
/// `benches/` row).
///
/// A `tempfile::tempdir` was considered (preferred option in GUI-KIT-01-REFRESH)
/// but rejected: the folder tree renders both the full directory string (path
/// text field) and the root basename, so a random `tmp.XXXXXX` path would leak
/// nondeterministic pixels into every snapshot. This committed, read-only
/// fixture with a *relative* path keeps every rendered string fixed:
/// `is_supported_image` ignores `.gitkeep`, so the grid stays empty, the RAW
/// count stays 0, and the tree shows only the fixed root label `library`.
/// Cargo runs integration tests with CWD = the package root, so the relative
/// path resolves on every machine. The fixture is never written to (both
/// tests only list it), hence sharing it between the two tests is race-free.
const LIBRARY_FIXTURE_DIR: &str = "tests/fixtures/library";

/// Point the app at the deterministic fixture directory (see above).
fn use_library_fixture(harness: &mut Harness<'_, LuminaApp>) {
    harness
        .state_mut()
        .set_directory(LIBRARY_FIXTURE_DIR.to_owned());
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_empty() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    harness.run();
    harness.snapshot("library_empty");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_with_image() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
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

/// Open exactly one Develop section via the public `set_section_open`
/// setter (deterministic, no header clicks — the other seven stay closed)
/// and scroll `target_label` into view so its widgets are pixel-visible in
/// the golden at the default 1024x720 viewport.
///
/// KITTEST-EXPANDED-VIEWPORT-1: `develop_sections_expanded` above expands
/// Basic + Color + Masking at once, so Color/Masking content is pushed
/// below the viewport fold and never pixel-visible (1024x720 clipping, a
/// single frame cannot show the Basic top and the Masking bottom at
/// once). These per-section snapshots are the chosen "einzeln snapshotten"
/// alternative: one open section each, scrolled into view.
fn expand_and_scroll_to(harness: &mut Harness<'_, LuminaApp>, section: usize, target_label: &str) {
    assert!(
        section < SECTION_COUNT,
        "unknown Develop section index {section}; SECTION_COUNT = {SECTION_COUNT}"
    );
    for i in 0..SECTION_COUNT {
        harness.state_mut().set_section_open(i, i == section);
    }
    // Layout frame so the accesskit tree contains the opened section.
    harness.run();
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
        "scroll target {target_label:?} not found in headed harness (section {section})"
    );
    // One frame dispatches the ScrollIntoView event, the second settles the
    // scrolled layout before snapshotting.
    harness.run();
    harness.run();
}

/// Assert that `label` is laid out inside the 1024x720 window (not below
/// the ScrollArea fold): existence in the accesskit tree alone does not
/// prove pixel-visibility.
fn assert_label_on_screen(harness: &mut Harness<'_, LuminaApp>, label: &str) {
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

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_section_color() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only Color open: the HSL mixer renders near the panel top.
    expand_and_scroll_to(&mut harness, SECTION_COLOR, "HSL / Color Mixer");
    // Non-vacuous guard: the scrolled-to widgets must actually be on-screen,
    // otherwise the golden below could pass on clipped (invisible) pixels.
    assert_label_on_screen(&mut harness, "HSL / Color Mixer");
    harness.snapshot("develop_section_color");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_section_masking() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // `load_bytes` leaves `document` empty (lazy `ensure_document_loaded`),
    // and `draw_masking` returns early without one — so seed an in-memory
    // mask entry first. `create_mask` is pure session state (no sidecar
    // write, no disk), keeping this snapshot side-effect-free like the rest.
    harness
        .state_mut()
        .create_mask("Snapshot Mask")
        .expect("seed mask entry");
    // Only Masking open: the mask controls render near the panel top.
    expand_and_scroll_to(&mut harness, SECTION_MASKING, "New Mask");
    // Non-vacuous guard: the scrolled-to widgets must actually be on-screen.
    assert_label_on_screen(&mut harness, "New Mask");
    harness.snapshot("develop_section_masking");
}

/// KITTEST-COVERAGE-SECTIONS-1: one golden per remaining Develop section.
/// Each test opens exactly one section (the other seven stay closed) and
/// scrolls a section-unique static group label into view, following the
/// KITTEST-EXPANDED-VIEWPORT-1 pattern (`expand_and_scroll_to` +
/// `assert_label_on_screen` + non-vacuous guard). No production code is
/// touched; the five `section_open` sections below need no document seed
/// (they render on `original.is_some()` from `load_sample`).

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_section_tone_curve() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only Tone Curve open: the parametric regions + point-curve editor.
    // B1: scroll target is the first point row (not the "Point curve"
    // group label) so the P0/P1 editor rows are pixel-visible in the
    // golden; the group label alone left them below the fold.
    expand_and_scroll_to(&mut harness, SECTION_TONE_CURVE, "P0 (0.00)");
    // Non-vacuous guard: the scrolled-to widgets must actually be on-screen,
    // otherwise the golden below could pass on clipped (invisible) pixels.
    assert_label_on_screen(&mut harness, "P0 (0.00)");
    harness.snapshot("develop_section_tone_curve");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_section_detail() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only Detail open: Sharpening + Noise Reduction sliders.
    expand_and_scroll_to(&mut harness, SECTION_DETAIL, "Sharpening");
    // Non-vacuous guard: the scrolled-to widgets must actually be on-screen.
    assert_label_on_screen(&mut harness, "Sharpening");
    harness.snapshot("develop_section_detail");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_section_effects() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only Effects open: Vignette + Grain sliders ("Grain" is unique to
    // this section; Optics uses "Vignette (light falloff)").
    expand_and_scroll_to(&mut harness, SECTION_EFFECTS, "Grain");
    // Non-vacuous guard: the scrolled-to widgets must actually be on-screen.
    assert_label_on_screen(&mut harness, "Grain");
    harness.snapshot("develop_section_effects");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_section_optics() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only Optics open: lens profile picker + manual correction groups.
    expand_and_scroll_to(&mut harness, SECTION_OPTICS, "Lens Correction");
    // Non-vacuous guard: the scrolled-to widgets must actually be on-screen.
    assert_label_on_screen(&mut harness, "Lens Correction");
    harness.snapshot("develop_section_optics");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_section_optics2() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only Optics open, scrolled to its lower half: the CA group + Lens
    // Blur subgroup sit below the Vignette fold (`develop_section_optics`
    // ends at Vignette C1). Lens Blur is a nested collapsing header
    // (default closed) — scroll it into view first (a below-fold click
    // would be discarded), then click it open so its controls render.
    expand_and_scroll_to(&mut harness, SECTION_OPTICS, "Lens Blur");
    let clicked = harness
        .query_all_by_label("Lens Blur")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(clicked, "Lens Blur subgroup not found in headed harness");
    harness.run();
    // Re-scroll after expanding (the new content moved the layout), same
    // 2-frame settle as `expand_and_scroll_to`.
    let found = harness
        .query_all_by_label("Lens Blur")
        .next()
        .map(|node| {
            node.scroll_to_me();
            true
        })
        .unwrap_or(false);
    assert!(found, "scroll target \"Lens Blur\" lost after expanding");
    harness.run();
    harness.run();
    // Non-vacuous guard: the subgroup header must actually be on-screen,
    // otherwise the golden below could pass on clipped (invisible) pixels.
    assert_label_on_screen(&mut harness, "Lens Blur");
    harness.snapshot("develop_section_optics2");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_section_geometry() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only Geometry open: Crop / Straighten / Perspective controls (no
    // Auto-Upright control exists yet — LRPAR-G06-UPRIGHT-15, Release 1.5).
    expand_and_scroll_to(&mut harness, SECTION_GEOMETRY, "Crop");
    // Non-vacuous guard: the scrolled-to widgets must actually be on-screen.
    assert_label_on_screen(&mut harness, "Crop");
    harness.snapshot("develop_section_geometry");
}

/// Open exactly one of the Presets / History / Rating Develop headers.
///
/// Those three are plain `ui.collapsing` headers *outside* the eight
/// `section_open` F-100 sections, so `expand_and_scroll_to` cannot open
/// them: all eight sections are closed via `set_section_open` and the
/// target header is clicked open instead (default closed on a fresh
/// harness). `target_label` is then scrolled into view with the same
/// 2-frame settle as `expand_and_scroll_to` before snapshotting.
fn open_collapsing_and_scroll_to(
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
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(
        clicked,
        "Develop header {header_label:?} not found in headed harness"
    );
    harness.run();
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

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_section_presets() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only Presets open: file list + Create & Apply / Save-as-file buttons.
    // No seed needed (the section renders without a document); the preset
    // list itself mirrors the machine-global presets dir, so the folder row
    // carries a machine-specific path (known risk, see report).
    open_collapsing_and_scroll_to(&mut harness, "Presets", "Create & Apply Preset");
    // Non-vacuous guard: the expanded section must expose its action row,
    // otherwise the golden below could pass on a collapsed header.
    assert_label_on_screen(&mut harness, "Create & Apply Preset");
    harness.snapshot("develop_section_presets");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_section_history() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // `load_bytes` leaves `document` empty, and the History section shows
    // "No sidecar loaded" without one — so seed an in-memory document plus
    // one history step first. `create_mask` (pure session state, Masking
    // precedent above) ensures the document; `create_preset`/`apply_preset`
    // records `history-1` with `recorded_at: None` (deterministic label, no
    // timestamp pixels) and only re-renders — no sidecar write, no disk.
    harness
        .state_mut()
        .create_mask("Snapshot Seed")
        .expect("seed mask entry");
    let preset = harness
        .state_mut()
        .create_preset("Snapshot Seed")
        .expect("seed preset");
    harness
        .state_mut()
        .apply_preset(&preset)
        .expect("seed history entry");
    // Only History open: the recorded entry row.
    open_collapsing_and_scroll_to(&mut harness, "History", "1. history-1");
    // Non-vacuous guard: the entry row must actually be on-screen.
    assert_label_on_screen(&mut harness, "1. history-1");
    harness.snapshot("develop_section_history");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_section_rating() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Same in-memory document seed as History (`draw_rating_section`
    // shows "No sidecar loaded" without one). Deliberately no
    // `set_rating`: it calls `save_sidecar` (disk write) — rating 0 already
    // renders the star / flag / color-label rows deterministically.
    harness
        .state_mut()
        .create_mask("Snapshot Seed")
        .expect("seed mask entry");
    // Only Rating open: star buttons + flag + color-label rows.
    open_collapsing_and_scroll_to(&mut harness, "Rating", "Color Label");
    // Non-vacuous guard: the color-label row must actually be on-screen.
    assert_label_on_screen(&mut harness, "Color Label");
    harness.snapshot("develop_section_rating");
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

// ---------------------------------------------------------------------------
// F-103-N9 interaction tests (deterministic state assertions, no snapshots).
// Like the snapshots, these need a headless wgpu harness, so they are
// `#[ignore]`d and run with the `-- --ignored` flag on a GPU machine.
// ---------------------------------------------------------------------------

/// Create a temporary folder with `count` dummy RAW files (content does not
/// decode — the filmstrip/grid cells show placeholders, which is fine for
/// geometry/layout assertions).
fn temp_raw_dir(count: usize) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    for i in 0..count {
        std::fs::write(
            dir.path().join(format!("IMG_{:04}.ARW", i)),
            b"not a real raw file",
        )
        .expect("write dummy raw");
    }
    dir
}

/// The filmstrip is the bottom `Panel::bottom`, so its cells live in the lower
/// band of the window. Each chip is an egui clickable area that surfaces in the
/// accesskit tree as `Role::Unknown` with the full cell rect (`CELL_W x CELL_H`
/// = 140x110). The row assertion verifies that all laid-out cells share (nearly)
/// one y and advance strictly to the right — a single horizontal row, no
/// wrapping/stacking.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn filmstrip_is_single_row_horizontal() {
    let dir = temp_raw_dir(20);
    let mut harness = build_harness();
    harness
        .state_mut()
        .set_directory(dir.path().display().to_string());
    harness.state_mut().set_module(Module::Develop);
    // The app keeps requesting repaints while thumbnail jobs are scheduled, so
    // `run()` would exceed max_steps; run a fixed number of frames instead.
    harness.run_steps(3);

    // Collect filmstrip cells: Unknown-role nodes sized like a cell (~110 tall)
    // in the bottom band (y > 500 of a 720-high window). GUI-VIEW-2: the
    // navigator rail is open by default and shows its own 120x90 thumbnail
    // column on the left — those nodes match the band filter too, so require
    // the 140-wide filmstrip cell geometry to keep this a filmstrip-only
    // single-row assertion.
    let mut chips: Vec<eframe::egui::Rect> = harness
        .query_all_by(|n| n.role() == eframe::egui::accesskit::Role::Unknown)
        .filter(|n| n.accesskit_node().bounding_box().is_some())
        .map(|n| n.rect())
        .filter(|r| r.min.y > 500.0 && r.height() > 60.0 && r.width() > 130.0)
        .collect();
    assert!(
        chips.len() >= 2,
        "expected at least 2 visible filmstrip cells, got {chips:?}"
    );
    chips.sort_by(|a, b| a.min.x.partial_cmp(&b.min.x).unwrap());
    let row_y = chips[0].center().y;
    for (i, cell) in chips.iter().enumerate() {
        assert!(
            (cell.center().y - row_y).abs() < 12.0,
            "cell {i} is vertically offset: {:?} (row y = {row_y})",
            cell.center()
        );
        if i > 0 {
            assert!(
                cell.min.x > chips[i - 1].min.x,
                "cell {i} does not advance x: {:?} then {:?}",
                chips[i - 1].min,
                cell.min
            );
        }
    }
}

/// Changing an adjustment (`set_adjustment`) must invalidate the preview and
/// produce a *new* render, i.e. bump `preview_generation` — even outside a
/// pointer drag (the debounced full render path).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn slider_changes_preview_generation() {
    let mut harness = build_harness();
    load_sample(&mut harness);
    harness.run();
    let before = harness.state_mut().preview_generation();
    assert!(before >= 1, "loaded sample must render at least once");

    harness.state_mut().set_adjustment("exposure", 1.0);
    harness.run();
    let after = harness.state_mut().preview_generation();
    assert!(
        after > before,
        "set_adjustment must re-render (preview_generation {before} -> {after})"
    );
}

/// The Library Folders tree must be rooted at the current workdir (the
/// `directory` field), not at `$HOME` — the tree root label is the workdir's
/// basename.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_folders_root_is_workdir() {
    let dir = tempfile::tempdir().expect("temp dir");
    // A subfolder ensures the tree has at least one child node so the root is
    // clearly distinguishable.
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let base = dir
        .path()
        .file_name()
        .and_then(|n| n.to_str())
        .expect("basename")
        .to_owned();

    let mut harness = build_harness();
    harness
        .state_mut()
        .set_directory(dir.path().display().to_string());
    harness.state_mut().set_module(Module::Library);
    harness.run_steps(3);

    // The workdir's basename must appear as a folder node label in the tree.
    let found = harness.query_all_by_label_contains(&base).next().is_some();
    assert!(
        found,
        "Folder tree must show the workdir `{base}` as its root (not $HOME)"
    );
}

/// GUI-HISTOGRAM-1: the histogram renders as a real graphic (filled
/// 256-bin Painter curve with P01/P99 markers) in its own collapsible
/// Develop-panel section instead of the old clip bar in the module bar.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn histogram_graphic() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // The histogram section defaults to open; Basic stays collapsed so the
    // golden focuses on the graphic. Two frames: the first renders and
    // uploads the preview texture, the second paints it.
    harness.run();
    harness.run();
    harness.snapshot("histogram_graphic");
}

/// GUI-PREVIEW-NAV-1: the navigator shows the full image with the viewport
/// rectangle of the zoomed working area (Custom zoom so the rectangle is
/// strictly smaller than the overview).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn navigator_viewport() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    harness.state_mut().set_navigator_open(true);
    // Custom zoom above fit: the viewport rectangle must be smaller than the
    // navigator overview. `zoom_step` pins `Custom` like modifier-wheel zoom.
    // Two frames: the first settles the zoomed render and uploads the preview
    // texture, the second paints the navigator overview from it.
    harness.state_mut().zoom_step(4.0);
    harness.run();
    harness.run();
    harness.snapshot("navigator_viewport");
}

/// LRPAR-G15-IPTC-S8: the Library right-column Metadata panel (draft
/// editor expanded). The presets directory is overridden with an empty
/// tempdir so no machine-global preset names leak into the golden (the path
/// itself is never rendered, only preset names — determinism holds).
/// `load_sample` is path-less, so embedded reads stay on the deterministic
/// "unavailable" note.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_metadata() {
    let presets = tempfile::tempdir().expect("temp dir");
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    harness
        .state_mut()
        .set_meta_presets_dir(Some(presets.path().to_path_buf()));
    load_sample(&mut harness);
    harness.run();
    // Second layout frame before querying: the first frame after
    // `load_bytes` settles the image load, only the second lays out the
    // right-column panel headers, so the `collapsing` toggle exists when
    // clicked (same reason `collapse_except` runs before its first query).
    harness.run();
    // Expand the draft editor so the golden pins the field rows, not just
    // the collapsed headers. The label is unique to this section (the
    // preview "Draft" badge uses a different string).
    let clicked = harness
        .query_all_by_label("Metadata draft")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(
        clicked,
        "Metadata draft section not found in headed harness"
    );
    harness.run();
    // Non-vacuous guard: the expanded editor must expose its field rows
    // ("Date created" is unique to the draft editor); otherwise the golden
    // below could pass on a collapsed panel without any editor pixels.
    assert!(
        harness.query_all_by_label("Date created").next().is_some(),
        "draft editor did not expand: field row 'Date created' missing"
    );
    harness.snapshot("library_metadata");
}
///
/// Committed layout (`top.arw` + `sub/mid.arw` + `sub/nested/deep.arw`);
/// the files are (re-)written deterministically on every run so a fresh
/// checkout without the binaries still passes and re-writes are
/// byte-identical (no git churn). The relative path keeps every rendered
/// string fixed, and the relative badges (`""`, `"sub"`, `"sub/nested"`)
/// are machine-independent — unlike a `tempfile::tempdir` path, which
/// would leak nondeterministic pixels into the golden via the path field
/// and tree root label.
const LIBRARY_BADGES_FIXTURE_DIR: &str = "tests/fixtures/library_badges";

/// (Re-)write the deterministic badge-fixture files (idempotent).
///
/// RAW sentinel bytes suffice: `scan_entry`/`list_directory` only need a
/// supported extension (+ optional sidecar) — no decode runs during a
/// directory scan, and grid cells show placeholders for undecodable bytes.
/// PNGs do NOT work here: the Library grid is RAW-only, so `.png`
/// fixtures aggregate into `entries` (status count) but render an empty
/// grid with no badges. Tradeoff: the golden pins the deterministic LibRaw
/// "opening input failed" placeholder text for the sentinel bytes (a
/// LibRaw message change needs a golden refresh); the `sub` / `sub/nested`
/// badge pixels are the actual regression signal.
fn ensure_library_badges_fixture() {
    let root = std::path::Path::new(LIBRARY_BADGES_FIXTURE_DIR);
    std::fs::create_dir_all(root.join("sub/nested")).expect("create badge fixture dirs");
    // Remove stale pre-RAW fixtures (H1 first attempt used `.png`).
    for stale in [
        "tests/fixtures/library_badges/top.png",
        "tests/fixtures/library_badges/sub/mid.png",
        "tests/fixtures/library_badges/sub/nested/deep.png",
    ] {
        let _ = std::fs::remove_file(stale);
    }
    for path in [
        "tests/fixtures/library_badges/top.arw",
        "tests/fixtures/library_badges/sub/mid.arw",
        "tests/fixtures/library_badges/sub/nested/deep.arw",
    ] {
        std::fs::write(path, b"lumina-raw-fixture").expect("write badge fixture");
    }
}

/// GUI-LIBRARY-SUBFOLDERS-1: the Library grid aggregates subfolders with a
/// visible relative-path badge (`sub`, `sub/nested`; empty for top-level).
/// Golden regression for the badge state (H1).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_subfolder_badges() {
    ensure_library_badges_fixture();
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    harness
        .state_mut()
        .set_directory(LIBRARY_BADGES_FIXTURE_DIR.to_owned());
    // `set_directory` lists flat; the recursive aggregation carries badges.
    harness.state_mut().list_directory();
    // Non-vacuous guard: the recursive listing (not the flat one) must see
    // all three files, otherwise the golden below could pass on an empty
    // grid without any badge pixels. (`FileBrowserEntry` fields are
    // private; `thumb_key()` is the public per-entry path accessor.)
    let entries = harness.state_mut().entries();
    assert_eq!(entries.len(), 3, "recursive aggregation must see all files");
    let mut keys: Vec<String> = entries
        .iter()
        .map(|entry| entry.thumb_key().to_owned())
        .collect();
    keys.sort();
    assert!(keys[0].ends_with("mid.arw"), "unexpected key {}", keys[0]);
    assert!(keys[1].ends_with("deep.arw"), "unexpected key {}", keys[1]);
    assert!(keys[2].ends_with("top.arw"), "unexpected key {}", keys[2]);
    // Fixed frames (not `run()`): thumbnail jobs keep requesting repaints.
    harness.run_steps(3);
    harness.snapshot("library_subfolder_badges");
}

// ---------------------------------------------------------------------------
// KITTEST-COVERAGE-META-1: Library Metadata subpanels + preset dialog.
//
// The draft editor already has a golden (`library_metadata` above); the four
// remaining subpanels (Embedded / History / Preset / Sync) get one expanded
// golden each (exactly one open, the rest collapsed), plus one golden with
// the dynamic-preset prompt dialog open. Pattern per test (68215fd/7fd528b):
// deterministic seeds, header click + scroll-into-view, non-vacuous
// accesskit guard + on-screen assert, then `snapshot`. No production code is
// touched; existing goldens are not rebaselined.
// ---------------------------------------------------------------------------

/// 2x1 JPEG bytes through the real encoder (same fixture pixels as the
/// lib `jpeg()` helper) so embedded-IPTC tests decode genuine JPEG bytes.
fn test_jpeg_bytes() -> Vec<u8> {
    ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
        .expect("fixture frame")
        .encode(ImageFileFormat::Jpeg)
        .expect("jpeg encodes")
}

/// Write `photo.jpg` with embedded IPTC into a fresh tempdir on the fly
/// (no repo binary). Returns the dir (keep alive until the snapshot is
/// taken) and the image path. The file name is fixed so the `Loaded:
/// photo.jpg` status line stays deterministic; the random tempdir prefix
/// never reaches pixels (see `open_file_and_restore_fixture`).
fn embedded_jpeg_fixture(title: &str, keywords: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let meta = lumina_iptc::IptcMetadata {
        title: Some(title.to_owned()),
        keywords: keywords.iter().map(|name| name.to_string()).collect(),
        ..Default::default()
    };
    let embedded =
        lumina_iptc::embed_metadata(&test_jpeg_bytes(), &meta).expect("embed IPTC fixture");
    let path = dir.path().join("photo.jpg");
    std::fs::write(&path, embedded).expect("write jpeg fixture");
    (dir, path)
}

/// Seed `photo.jpg`'s sidecar with exactly 10 metadata history entries
/// carrying fixed RFC 3339 UTC timestamps (no wall-clock): the History
/// panel renders `rev | timestamp | origin | changed`, so real timestamps
/// would leak nondeterministic pixels into the golden. Seeding goes through
/// the public sidecar API into a tempdir file — the snapshot itself (like
/// the `create_mask` precedents) performs no disk write.
fn seed_metadata_history_sidecar(photo: &Path) {
    use lumina_sidecar::{DecodeFingerprint, GeometryFingerprint, SidecarDocument, SourceIdentity};
    let identity = SourceIdentity {
        relative_name: "photo.jpg".to_owned(),
        content_hash: "blake3:kittest-meta-history".to_owned(),
        byte_length: 0,
        modified_at: None,
        raw_format: "JPG".to_owned(),
        orientation: 1,
        decode_fingerprint: DecodeFingerprint {
            decoder: "kittest".to_owned(),
            version: "1".to_owned(),
            parameters: BTreeMap::new(),
            extras: BTreeMap::new(),
        },
        geometry_fingerprint: GeometryFingerprint {
            width: 2,
            height: 1,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: BTreeMap::new(),
        },
        extras: BTreeMap::new(),
    };
    let mut document = SidecarDocument::new(identity, "raster-mvp-1");
    for index in 1..=10_u32 {
        let mut fields = BTreeMap::new();
        fields.insert("title".to_owned(), format!("Titel {index}"));
        let timestamp = format!("2026-01-{index:02}T12:00:00Z");
        assert!(
            document
                .apply_metadata_draft(&fields, "gui", &timestamp)
                .expect("seed history entry"),
            "history entry {index} must change the draft"
        );
    }
    let sidecar = lumina_sidecar::sidecar_path_for(photo);
    lumina_sidecar::save_sidecar(&sidecar, &document).expect("seed sidecar");
}

/// Rendered History line for a seeded entry (`MetadataHistoryEntryPattern`).
fn history_line(rev: u64, day: u32) -> String {
    format!("rev {rev} | 2026-01-{day:02}T12:00:00Z | gui | title")
}

/// Write a dynamic meta preset file (one placeholder) into `dir`.
fn write_dynamic_meta_preset(dir: &Path, file: &str, name: &str) {
    let preset = serde_json::json!({
        "format": "lumina-meta-preset",
        "version": 1,
        "name": name,
        "fields": { "title": "{event}" },
        "placeholders": [{ "name": "event", "description": "Event name" }],
    });
    std::fs::write(dir.join(file), serde_json::to_vec_pretty(&preset).unwrap())
        .expect("write preset");
}

/// Open a real file (async decode) and pump headed frames until `ready`
/// holds, then point the browser back at the deterministic fixture
/// directory: `open_file` adopts the file's parent as the browser
/// directory, and the tempdir prefix must never leak into folders/grid/
/// filmstrip pixels (same rationale as `use_library_fixture`).
fn open_file_and_restore_fixture(
    harness: &mut Harness<'_, LuminaApp>,
    path: &Path,
    mut ready: impl FnMut(&mut LuminaApp) -> bool,
) {
    harness.state_mut().open_file(path.display().to_string());
    for _ in 0..500 {
        harness.run_steps(1);
        if ready(harness.state_mut()) {
            break;
        }
    }
    assert!(
        ready(harness.state_mut()),
        "decode of {} never settled in headed harness",
        path.display()
    );
    harness
        .state_mut()
        .set_directory(LIBRARY_FIXTURE_DIR.to_owned());
    harness.run();
}

/// Click one Library Metadata collapsing header (all default closed) and
/// scroll a content label into view. The header itself is scrolled into
/// view first — a below-fold click would be discarded (7fd528b optics2
/// lesson) — with the same 2-frame settle as `expand_and_scroll_to`.
fn open_meta_section_and_scroll_to(
    harness: &mut Harness<'_, LuminaApp>,
    header_label: &str,
    target_label: &str,
) {
    harness.run();
    let header_visible = harness
        .query_all_by_label(header_label)
        .next()
        .map(|node| {
            node.scroll_to_me();
            true
        })
        .unwrap_or(false);
    assert!(
        header_visible,
        "Metadata header {header_label:?} not found in headed harness"
    );
    harness.run();
    harness.run();
    let clicked = harness
        .query_all_by_label(header_label)
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(
        clicked,
        "Metadata header {header_label:?} lost after scrolling"
    );
    harness.run();
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
    harness.run();
    harness.run();
}

/// Embedded subpanel expanded (read-only JPEG IPTC): the keywords line is
/// the section-unique content label. Draft stays collapsed, so its
/// per-field `Embedded: …` overlay labels are absent.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_meta_embedded() {
    let (_tmp, photo) = embedded_jpeg_fixture("Eingebettet", &["k1", "k2"]);
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    open_file_and_restore_fixture(&mut harness, &photo, |app| {
        app.preview_generation() >= 1 && app.embedded_metadata().unwrap().is_some()
    });
    // Non-vacuous in-memory guard: the file really carries IPTC.
    let meta = harness
        .state_mut()
        .embedded_metadata()
        .unwrap()
        .expect("JPEG carries IPTC");
    assert_eq!(meta.title.as_deref(), Some("Eingebettet"));
    assert_eq!(meta.keywords, vec!["k1".to_owned(), "k2".to_owned()]);
    open_meta_section_and_scroll_to(
        &mut harness,
        "Embedded (read-only)",
        "Embedded keywords: k1, k2",
    );
    // Non-vacuous guard: the keywords line must actually be on-screen,
    // otherwise the golden below could pass on a collapsed header.
    assert_label_on_screen(&mut harness, "Embedded keywords: k1, k2");
    harness.snapshot("library_meta_embedded");
}

/// History subpanel expanded with 10 fixed-timestamp entries (newest
/// first). The scroll target is the oldest row at the bottom.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_meta_history() {
    let dir = tempfile::tempdir().expect("temp dir");
    let photo = dir.path().join("photo.jpg");
    std::fs::write(&photo, test_jpeg_bytes()).expect("write jpeg");
    seed_metadata_history_sidecar(&photo);
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    open_file_and_restore_fixture(&mut harness, &photo, |app| {
        app.metadata_history().len() == 10
    });
    // Non-vacuous in-memory guard: newest-first order with fixed labels.
    // (Row-label existence is asserted after expanding below: collapsed
    // section content has no accesskit nodes.)
    let history = harness.state_mut().metadata_history();
    assert_eq!(history.len(), 10);
    assert_eq!(history[0].rev, 10);
    assert_eq!(history[9].rev, 1);
    let oldest = history_line(1, 1);
    let newest = history_line(10, 10);
    open_meta_section_and_scroll_to(&mut harness, "History", &oldest);
    assert!(
        harness.query_all_by_label(newest.as_str()).next().is_some(),
        "newest history row missing"
    );
    // Non-vacuous guard: the oldest row must actually be on-screen.
    assert_label_on_screen(&mut harness, &oldest);
    harness.snapshot("library_meta_history");
}

/// Preset subpanel expanded with an empty override dir (same pattern as
/// `library_metadata`): no machine-global preset name leaks into the
/// golden — the path itself is never rendered, only preset names.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_meta_preset() {
    let presets = tempfile::tempdir().expect("temp dir");
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    harness
        .state_mut()
        .set_meta_presets_dir(Some(presets.path().to_path_buf()));
    load_sample(&mut harness);
    harness.run();
    harness.run();
    // Non-vacuous in-memory guard: the override really yields no presets.
    assert!(
        harness.state_mut().meta_preset_names().is_empty(),
        "empty override dir must list no presets"
    );
    open_meta_section_and_scroll_to(&mut harness, "Meta preset", "Apply preset");
    // Non-vacuous guard: the action row must actually be on-screen,
    // otherwise the golden below could pass on a collapsed header.
    assert_label_on_screen(&mut harness, "Apply preset");
    harness.snapshot("library_meta_preset");
}

/// Sync subpanel expanded: all 12 field rows (11 draft fields + keywords)
/// render checked by default (`default_meta_sync_fields`; the checked
/// pixels are the golden's signal, the Vision check confirms them).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_meta_sync() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    load_sample(&mut harness);
    harness.run();
    harness.run();
    // At click time only the header carries this label (the action button
    // with the same text appears after expanding).
    open_meta_section_and_scroll_to(&mut harness, "Sync to selection", "Title");
    // Non-vacuous guard: every sync row must exist (draft stays collapsed,
    // so these labels are unique to the sync section).
    for label in [
        "Title",
        "Headline",
        "Description",
        "Copyright",
        "Creator",
        "Credit",
        "Source",
        "City",
        "State / Province",
        "Country",
        "Date created",
        "Keywords",
    ] {
        assert!(
            harness.query_all_by_label(label).next().is_some(),
            "sync row {label:?} missing"
        );
    }
    assert_label_on_screen(&mut harness, "Title");
    harness.snapshot("library_meta_sync");
}

/// Dynamic-preset prompt dialog open with its required field
/// (`draw_meta_preset_dialog` draws it as a floating window from `update`).
/// The preset is selected through the combo UI (no private state poking);
/// after the snapshot Cancel is clicked headless and the no-write path is
/// asserted on disk (dialog-cancel state semantics themselves are covered
/// by the lib `iptc_gui_dynamic_preset_requires_vars` test).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn draw_meta_preset_dialog() {
    let dir = tempfile::tempdir().expect("temp dir");
    let png = dir.path().join("photo.png");
    std::fs::write(&png, LuminaApp::sample_image_png()).expect("write png");
    let presets = tempfile::tempdir().expect("temp dir");
    write_dynamic_meta_preset(
        presets.path(),
        "KittestDialog.lumina-meta-preset.json",
        "KittestDialog",
    );
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    harness
        .state_mut()
        .set_meta_presets_dir(Some(presets.path().to_path_buf()));
    open_file_and_restore_fixture(&mut harness, &png, |app| app.preview_generation() >= 1);
    // Non-vacuous in-memory guard: the dynamic preset is listed. The
    // panel auto-refresh runs inside the (closed) section closure, so an
    // explicit refresh is needed before the section is opened below.
    harness.state_mut().refresh_meta_presets();
    assert_eq!(
        harness.state_mut().meta_preset_names(),
        vec!["KittestDialog".to_owned()]
    );
    open_meta_section_and_scroll_to(&mut harness, "Meta preset", "Apply preset");
    // Select the preset through the combo UI: egui exposes the ComboBox as
    // a `ComboBox`-role node carrying the selected text as its *value*
    // (not its label), so it is queried by value. Before the selection the
    // value is still the hint, keeping the popup item unambiguous.
    let combo_open = harness
        .query_all_by_value("Choose a preset…")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(combo_open, "preset combo not found in headed harness");
    harness.run();
    harness.run();
    let item_picked = harness
        .query_all_by_label("KittestDialog")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(item_picked, "preset popup item not found in headed harness");
    harness.run();
    harness.run();
    let apply_clicked = harness
        .query_all_by_label("Apply preset")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(apply_clicked, "Apply preset button lost after selection");
    harness.run();
    harness.run();
    // Non-vacuous guard: the dialog window with its required field must
    // actually be on-screen.
    assert!(
        harness
            .query_all_by_label("Preset variables (all required)")
            .next()
            .is_some(),
        "preset dialog did not open"
    );
    assert_label_on_screen(&mut harness, "Preset variables (all required)");
    assert_label_on_screen(&mut harness, "Event name");
    harness.snapshot("draw_meta_preset_dialog");
    // Cancel path headless: the dialog closes and no sidecar is written.
    let cancel_clicked = harness
        .query_all_by_label("Cancel")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(cancel_clicked, "dialog Cancel button not found");
    harness.run();
    assert!(
        harness
            .query_all_by_label("Preset variables (all required)")
            .next()
            .is_none(),
        "dialog must close on Cancel"
    );
    assert!(
        !lumina_sidecar::sidecar_path_for(&png).is_file(),
        "cancelled dialog must not write a sidecar"
    );
}
