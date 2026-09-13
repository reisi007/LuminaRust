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
use lumina_core::cache::{disk::DiskFolderCache, PreviewKind};
use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_gui::{
    LibraryView, LuminaApp, Module, PinVisibility, ZoomMode, LIBRARY_BADGE_BG, SECTION_COLOR,
    SECTION_COUNT, SECTION_DETAIL, SECTION_EFFECTS, SECTION_GEOMETRY, SECTION_MASKING,
    SECTION_OPTICS, SECTION_TONE_CURVE,
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

/// Collect the visible filmstrip cells and assert that they form exactly one
/// horizontal row.
///
/// Each chip is an egui clickable area that surfaces in the accesskit tree as
/// `Role::Unknown` with the full cell rect (`CELL_W x CELL_H` = 140x110) in the
/// bottom band (`y > 500` of the 720-high window). GUI-VIEW-2: the navigator
/// rail is open by default and shows its own 120x90 thumbnail column on the
/// left — those nodes match the band filter too, so require the 140-wide
/// filmstrip cell geometry to keep this a filmstrip-only assertion. The row
/// check verifies all laid-out cells share (nearly) one y and advance strictly
/// to the right — no wrapping/stacking. Returns the sorted chips so callers can
/// add their own count/selection guards.
fn assert_filmstrip_single_row(harness: &mut Harness<'_, LuminaApp>) -> Vec<eframe::egui::Rect> {
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
    chips
}

/// The filmstrip is the bottom `Panel::bottom`; this pins the single-row
/// geometry for a 20-dummy strip (KITTEST-COVERAGE-STATES-1).
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
    assert_filmstrip_single_row(&mut harness);
}

/// UX-SLICE-1 (UXG-09): the shared filmstrip header shows an "n of N" counter
/// driven by the strip selection. Headless state assertion (no golden): the
/// counter text is a real accesskit label.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn filmstrip_counter_reflects_selection() {
    let mut harness = build_harness();
    let paths = setup_library_views(&mut harness);
    // Exactly the first entry selected: 1 of 3.
    harness
        .state_mut()
        .select_filmstrip_path(paths[0].clone(), false, false);
    harness.run_steps(3);
    assert!(
        harness.query_all_by_label("1 of 3").next().is_some(),
        "one selected strip entry must render the 1-of-N counter"
    );
    // Toggle-add the second entry: 2 of 3.
    harness
        .state_mut()
        .select_filmstrip_path(paths[1].clone(), true, false);
    harness.run_steps(3);
    assert!(
        harness.query_all_by_label("2 of 3").next().is_some(),
        "two selected strip entries must render the 2-of-3 counter"
    );
}

/// UX-SLICE-2 (F2/F5): the empty-state CTA is genuinely wired to the native
/// folder picker (injected headless — no display server), not the old no-op
/// re-list of the current directory. A picked folder with a RAW entry proves
/// the CTA leaves the empty state and adopts the picked path.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_empty_state_cta_picks_folder() {
    let picked = tempfile::tempdir().expect("picked temp dir");
    std::fs::write(picked.path().join("picked.arw"), b"lumina-raw-fixture")
        .expect("write picked raw sentinel");
    std::fs::create_dir_all(picked.path().join("sub")).expect("create picked subdir");
    let picked_path = picked.path().display().to_string();

    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    let injected = picked_path.clone();
    harness
        .state_mut()
        .set_folder_picker(move || Some(PathBuf::from(injected.clone())));
    harness.run();
    assert!(
        harness.query_all_by_label("No images").next().is_some(),
        "empty state must show its title before the CTA is used"
    );
    let clicked = harness
        .query_all_by_label("Open Folder")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(clicked, "empty-state CTA must be present and clickable");
    // Fixed frames (not `run()`): the picked sentinel starts a background
    // decode whose repaint requests would exceed `run`'s step budget.
    harness.run_steps(5);
    assert_eq!(
        harness.state_mut().directory(),
        picked_path,
        "CTA must adopt the folder returned by the picker (no-op re-list is not wiring)"
    );
    assert!(
        !harness.state_mut().entries().is_empty(),
        "the picked folder's RAW entry must be listed"
    );
    assert!(
        harness.query_all_by_label("No images").next().is_none(),
        "a non-empty picked folder must leave the empty state"
    );
}

/// UX-SLICE-2 (F2): a cancelled folder dialog is a deliberate no-op — the
/// directory and the empty state stay exactly as they were.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_empty_state_cta_cancel_is_noop() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    harness.state_mut().set_folder_picker(|| None);
    harness.run();
    let before = harness.state_mut().directory().to_owned();
    let clicked = harness
        .query_all_by_label("Open Folder")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(clicked, "empty-state CTA must be present and clickable");
    harness.run();
    assert_eq!(
        harness.state_mut().directory(),
        before,
        "a cancelled dialog must not change the directory"
    );
    assert!(
        harness.query_all_by_label("No images").next().is_some(),
        "a cancelled dialog must keep the empty state"
    );
}

/// UX-SLICE-2 (F3): Loupe/Compare/Survey use the same shared empty state as
/// the Grid (title + "Open Folder" CTA) instead of their former heading-only
/// text, so no view can drift into a divergent empty presentation.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn non_grid_library_views_share_the_empty_state() {
    for view in [
        LibraryView::Loupe,
        LibraryView::Compare,
        LibraryView::Survey,
    ] {
        let mut harness = build_harness();
        harness.state_mut().set_module(Module::Library);
        use_library_fixture(&mut harness);
        harness.state_mut().set_library_view(view);
        harness.run_steps(2);
        assert!(
            harness.query_all_by_label("No images").next().is_some(),
            "{view:?} must show the shared empty-state title"
        );
        assert!(
            harness.query_all_by_label("Open Folder").next().is_some(),
            "{view:?} must show the shared empty-state CTA"
        );
    }
}

/// UX-SLICE-2 (F5): the render hash exists **exactly once** and in the top
/// status line — the old canvas-edge copy is gone (a second node or a
/// lower-half rect would mean the canvas text came back).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn render_hash_moves_to_status_line() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    harness.run();
    harness.run();
    let nodes: Vec<_> = harness
        .query_all_by_label_contains("Render state current:")
        .collect();
    assert_eq!(
        nodes.len(),
        1,
        "the render hash must exist exactly once (status line), not also at the canvas"
    );
    let rect = nodes[0].rect();
    assert!(
        rect.max.y < 60.0,
        "the render hash must sit in the top status line, got {rect:?}"
    );
}

/// UX-SLICE-2 (F1): the header render hash is hidden in the Library module
/// while the grid empty state is shown, so it can never contradict "No
/// images". UX-SLICE-3 (F1 follow-up): the gate tracks the *filtered* raster
/// the empty state itself uses — a `\` query with zero matches hides the hash
/// even though RAW entries are listed, and clearing it restores both. The
/// underlying `render_key` stays valid; the hash returns with Develop.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_empty_suppresses_render_hash() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    load_sample(&mut harness);
    harness.run();
    harness.run();
    assert!(
        harness.state_mut().render_key().is_some(),
        "the sample must have a current render"
    );
    // Unfiltered empty: the fixture directory has no RAW entries.
    assert!(
        !harness.state_mut().render_hash_visible(),
        "F1 gate must hide the hash while the RAW grid is empty"
    );
    assert!(
        harness
            .query_all_by_label_contains("Render state current:")
            .next()
            .is_none(),
        "no render hash may be painted above the Library empty state"
    );
    // Filtered empty (UX-SLICE-3): a RAW grid exists, but the `\` query
    // matches none, so the same shared empty state is shown and the gate must
    // hide the hash just like the unfiltered case.
    ensure_library_views_fixture();
    harness
        .state_mut()
        .set_directory(LIBRARY_VIEWS_FIXTURE_DIR.to_owned());
    // `set_directory` lists flat; mirror the views tests' recursive listing.
    harness.state_mut().list_directory();
    harness.state_mut().set_library_filter("no-such-entry");
    harness.run();
    assert!(
        harness.state_mut().render_key().is_some(),
        "the gate must be judged against a current render, not an absent one"
    );
    assert!(
        harness.state_mut().filtered_library_order().is_empty(),
        "the `\\` filter must yield a zero-match raster for this guard"
    );
    assert!(
        !harness.state_mut().render_hash_visible(),
        "a zero-match filter must hide the hash despite listed RAW entries"
    );
    assert!(
        harness
            .query_all_by_label_contains("Render state current:")
            .next()
            .is_none(),
        "no render hash may be painted above the filtered empty state"
    );
    // Clearing the filter restores the non-empty raster and the hash.
    harness.state_mut().set_library_filter("");
    harness.run();
    assert!(
        !harness.state_mut().filtered_library_order().is_empty(),
        "clearing the filter must restore the listed raster"
    );
    assert!(
        harness.state_mut().render_hash_visible(),
        "a non-empty filtered raster must keep the render hash"
    );
    assert!(
        harness
            .query_all_by_label_contains("Render state current:")
            .next()
            .is_some(),
        "the render hash must be visible again on the non-empty raster"
    );
    // Same render, other module: the hash is meaningful again.
    harness.state_mut().set_module(Module::Develop);
    harness.run();
    assert!(
        harness.state_mut().render_hash_visible(),
        "Develop must keep the render hash for the loaded image"
    );
    assert!(
        harness
            .query_all_by_label_contains("Render state current:")
            .next()
            .is_some(),
        "the render hash must be visible again in Develop"
    );
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
    // The grid's thumbnail probe creates a gitignored `.lumina/` preview cache
    // inside the fixture tree; left behind it would appear as an extra folder
    // row on the *next* run and make this golden order-dependent. Removing it
    // makes every run start from the committed file set (fresh checkout and
    // re-run render identically).
    for cache in [
        "tests/fixtures/library_badges/.lumina",
        "tests/fixtures/library_badges/sub/.lumina",
        "tests/fixtures/library_badges/sub/nested/.lumina",
    ] {
        let _ = std::fs::remove_dir_all(cache);
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
// UX-SLICE-2 (F4): the LR-01 rating/flag/color-label badge is painted by the
// shared `paint_entry_badge` helper. The pre-existing "badge" fixtures are all
// unrated, so no badge was ever painted; this fixture carries exactly one
// rated/picked/red, one rejected/blue and one unrated/green entry (badge text
// asserted through the public `FileBrowserEntry::badge_text` accessor) and the
// golden plus an exact-`LIBRARY_BADGE_BG` pixel count prove the chip pixels.
// ---------------------------------------------------------------------------

const LIBRARY_RATED_FIXTURE_DIR: &str = "tests/fixtures/library_rated";

/// (Re-)write a deterministic, conflict-free rated fixture: three RAW
/// sentinels whose sidecars carry the default copy's rating/flag/label, each
/// with a seeded Standard preview (same `DiskFolderCache` pattern as
/// `ensure_library_views_fixture`). The seeded cache makes the `.lumina/` node
/// present before the first frame, so the folder tree and the thumbnail pixels
/// are stable (a cold cache would create `.lumina/` asynchronously and the
/// golden would flip between runs). The sidecar content hash matches the
/// sentinel bytes (`source_status` => `Unchanged`), so the golden pins the
/// badges, never a conflict state. Idempotent.
fn ensure_library_rated_fixture() {
    use lumina_sidecar::{DecodeFingerprint, Flag, GeometryFingerprint, SidecarDocument};
    let root = Path::new(LIBRARY_RATED_FIXTURE_DIR);
    std::fs::create_dir_all(root).expect("create rated fixture dir");
    // Fresh cache directory before re-seeding, so every run starts from the
    // same committed file set (no stale preview entries).
    let _ = std::fs::remove_dir_all(root.join(".lumina"));
    let bytes = b"lumina-raw-fixture";
    let content_hash = format!("blake3:{}", blake3::hash(bytes).to_hex());
    for (name, rating, flag, label, base) in [
        ("rated.arw", 5u8, Flag::Pick, 1u64, [200, 60, 50]),
        ("rejected.arw", 2, Flag::Reject, 4, [60, 170, 80]),
        ("labeled.arw", 0, Flag::Unflagged, 3, [70, 110, 200]),
    ] {
        std::fs::write(root.join(name), bytes).expect("write rated fixture");
        // Seed the exact Standard preview `ensure_thumbnail` probes
        // (`vc-original`), so the Grid cells and the filmstrip paint the
        // seeded pixels instead of the decode-failure placeholder. A decode
        // *does* run during the golden: the F-100 start behavior auto-loads
        // the first RAW (`labeled.arw`), whose sentinel bytes fail
        // deterministically in LibRaw, so the golden also pins the
        // deterministic LibRaw "opening input failed" banner in the status
        // line — exactly like `library_loupe.png` and the
        // `library_badges` fixture. A LibRaw message change needs a golden
        // refresh; the badge chip pixels remain the regression signal.
        let png = library_views_preview_png(base);
        let cache = DiskFolderCache::for_image(root.join(name)).expect("rated fixture cache");
        assert!(
            cache
                .store_preview(name, "vc-original", PreviewKind::Standard, &png)
                .expect("seed rated preview"),
            "Standard previews must be enabled for {name}"
        );
        let identity = lumina_sidecar::SourceIdentity {
            relative_name: name.to_owned(),
            content_hash: content_hash.clone(),
            byte_length: bytes.len() as u64,
            modified_at: None,
            raw_format: "ARW".to_owned(),
            orientation: 1,
            decode_fingerprint: DecodeFingerprint {
                decoder: "kittest".to_owned(),
                version: "1".to_owned(),
                parameters: BTreeMap::new(),
                extras: BTreeMap::new(),
            },
            geometry_fingerprint: GeometryFingerprint {
                width: 2,
                height: 2,
                orientation: 1,
                pixel_aspect_ratio: 1.0,
                extras: BTreeMap::new(),
            },
            extras: BTreeMap::new(),
        };
        let mut document = SidecarDocument::new(identity, "raster-mvp-1");
        document.virtual_copies[0].rating = rating;
        document.virtual_copies[0].flag = flag;
        document.virtual_copies[0]
            .extras
            .insert("color_label".to_owned(), serde_json::Value::from(label));
        let sidecar = lumina_sidecar::sidecar_path_for(&root.join(name));
        lumina_sidecar::save_sidecar(&sidecar, &document).expect("seed rated sidecar");
    }
}

/// Non-vacuous pixel guard for the F4 badge golden: count framebuffer pixels
/// equal to the shared chip fill [`LIBRARY_BADGE_BG`]. The fill is not part of
/// the theme palette and this flat fixture has no folder badges, so the only
/// source is `paint_entry_badge`. One 118x16 chip is ~1.9k raw pixels (minus
/// the glyphs); Grid + filmstrip together paint six, so `>= 2_000` is a
/// conservative lower bound that a clean (unrated) fixture cannot reach.
fn assert_badge_chips_painted(harness: &mut Harness<'_, LuminaApp>) {
    let rendered = harness.render().expect("kittest renders the frame");
    let (r, g, b) = (
        LIBRARY_BADGE_BG.r(),
        LIBRARY_BADGE_BG.g(),
        LIBRARY_BADGE_BG.b(),
    );
    let count = rendered
        .pixels()
        .filter(|pixel| {
            let [pr, pg, pb, _a] = pixel.0;
            pr == r && pg == g && pb == b
        })
        .count();
    assert!(
        count >= 2_000,
        "rated cells must paint the shared badge chip fill; got {count} exact-`LIBRARY_BADGE_BG` pixels"
    );
}

/// UX-SLICE-2 (F4): rated/flagged/labeled Library fixture — Grid cells and
/// filmstrip cells both paint the LR-01 badge, proven by the golden plus the
/// exact-fill pixel count. Badge text is asserted through the public accessor.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_rated_badges() {
    ensure_library_rated_fixture();
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    harness
        .state_mut()
        .set_directory(LIBRARY_RATED_FIXTURE_DIR.to_owned());
    // `set_directory` lists flat; the grid's shared order is RAW-only.
    harness.state_mut().list_directory();
    let mut badges: Vec<String> = harness
        .state_mut()
        .entries()
        .iter()
        .filter_map(|entry| entry.badge_text())
        .collect();
    badges.sort();
    assert_eq!(
        badges,
        vec![
            "★★★★★ P ●Red".to_owned(),
            "★★☆☆☆ X ●Blue".to_owned(),
            "☆☆☆☆☆ ●Green".to_owned(),
        ],
        "rated fixture must carry the three distinct badges (rating/flag/label)"
    );
    // Fixed frames (not `run()`): thumbnail jobs keep requesting repaints.
    harness.run_steps(3);
    // Non-vacuous pixel assert on the frame that is snapshotted below: a
    // mis-seeded (clean) fixture paints no chip fill and cannot reach the
    // threshold.
    assert_badge_chips_painted(&mut harness);
    harness.snapshot("library_rated_badges");
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

// ---------------------------------------------------------------------------
// KITTEST-COVERAGE-LIBRARY-1: Library Loupe / Compare / Survey goldens.
//
// The default Library grid already has goldens (`library_empty`,
// `library_with_image`, `library_subfolder_badges`); the three remaining
// G-09 views get one golden each here, following the 68215fd/7fd528b/dd73806
// pattern: deterministic seeds, non-vacuous guard + on-screen assert, then
// `snapshot`. Two minimal production layout fixes were required along the
// way (B1: the folder-tree path row claimed the whole panel height via a
// direction-changing `with_layout`, hiding the tree; B2: the fixed Loupe
// height buried the rating line under the filmstrip) — the 9 pre-existing
// Library goldens pinning that broken layout were rebaselined with them
// (diffs limited to the fixed regions, verified per golden).
//
// Vision baselines covered:
// * `library_with_image` shows no thumbnail (grid center = empty state):
//   `library_loupe` proves real image content — each sentinel's Standard
//   disk-cache preview is seeded with deterministic pixels and the guard
//   decodes them back (dims + distinct dominant channels), so Loupe paints
//   thumbnail textures, never the empty/placeholder text.
// * `subfolder_badges` cell texts squeezed/overlapping (minor): the views
//   fixture stays flat with short names (`a01.arw` …), so no badge row can
//   overlap; the badges themselves stay pinned by `library_subfolder_badges`.
// ---------------------------------------------------------------------------

/// Committed fixture directory for the Loupe/Compare/Survey snapshots below
/// (same rationale as `LIBRARY_BADGES_FIXTURE_DIR`: a relative path keeps
/// every rendered string fixed; a `tempfile::tempdir` would leak its random
/// prefix into the folder-tree + path-field pixels).
const LIBRARY_VIEWS_FIXTURE_DIR: &str = "tests/fixtures/library_views";

/// Sentinel files (flat, short names — see the badge note above) with the
/// base color of their seeded Standard preview. Distinct per file so Survey
/// shows three visibly different thumbnails.
const LIBRARY_VIEWS_FILES: &[(&str, [u8; 3])] = &[
    ("a01.arw", [200, 60, 50]),
    ("a02.arw", [60, 170, 80]),
    ("b01.arw", [70, 110, 200]),
];

/// Seeded preview dimensions. Large enough to stay clearly visible in the
/// Loupe/Compare panes (which paint the thumbnail texture at native size);
/// the golden pins these pixels.
const LIBRARY_VIEWS_PREVIEW_SIZE: (u32, u32) = (288, 192);

/// Deterministic preview pixels: vertical gradient around `base` (the ramp
/// proves non-trivial content; the per-file means stay distinct).
fn library_views_preview_png(base: [u8; 3]) -> Vec<u8> {
    let (width, height) = LIBRARY_VIEWS_PREVIEW_SIZE;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        // 192..255 ramp over the frame height (deterministic, no wall-clock).
        let factor = 192 + ((y * 63) / height.max(1));
        for _ in 0..width {
            for channel in base {
                pixels.push(((u32::from(channel) * factor) / 255) as u8);
            }
            pixels.push(255);
        }
    }
    ImageFrame::new(width, height, pixels)
        .expect("fixture frame")
        .encode(ImageFileFormat::Png)
        .expect("fixture preview encodes")
}

/// Serializes the (re-)write + cache seeding below: the three views tests
/// run in one process on threads and share the same fixture files, while
/// `DiskFolderCache::store_preview` stages through a pid-named temp file —
/// concurrent seeds of the same entry would race on that temp path.
static LIBRARY_VIEWS_FIXTURE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// (Re-)write the sentinel RAWs and seed each file's Standard disk-cache
/// preview (`vc-original`, the key `ensure_thumbnail` probes). RAW sentinel
/// bytes suffice for the scan (same as `ensure_library_badges_fixture` — no
/// decode runs during a directory scan); the seeded cache is what lets the
/// views paint real pixels without a native RAW decode. The cache lives
/// under the fixture's (gitignored) `.lumina/` dir, so re-seeding is stable
/// and never git churn. Idempotent: called on every run.
fn ensure_library_views_fixture() {
    let _guard = LIBRARY_VIEWS_FIXTURE_LOCK
        .lock()
        .expect("views fixture lock");
    let root = Path::new(LIBRARY_VIEWS_FIXTURE_DIR);
    std::fs::create_dir_all(root).expect("create views fixture dir");
    for &(name, base) in LIBRARY_VIEWS_FILES {
        std::fs::write(root.join(name), b"lumina-raw-fixture").expect("write views fixture");
        let png = library_views_preview_png(base);
        let cache = DiskFolderCache::for_image(root.join(name)).expect("views fixture cache");
        assert!(
            cache
                .store_preview(name, "vc-original", PreviewKind::Standard, &png)
                .expect("seed views preview"),
            "Standard previews must be enabled for {name}"
        );
    }
}

/// Dominant (mean-brightest) channel index of an RGBA buffer.
fn dominant_channel(pixels: &[u8]) -> usize {
    let mut means = [0u64; 3];
    let (chunks, _) = pixels.as_chunks::<4>();
    for pixel in chunks {
        for (index, mean) in means.iter_mut().enumerate() {
            *mean += u64::from(pixel[index]);
        }
    }
    means
        .iter()
        .enumerate()
        .max_by_key(|&(_, mean)| mean)
        .map(|(index, _)| index)
        .expect("non-empty pixels")
}

/// Non-vacuous guard: every seeded preview decodes back to real, distinct
/// pixels (expected dims, dominant channel matches the file's base color, a
/// non-trivial vertical ramp). If seeding broke, the views below would paint
/// the LibRaw-failure placeholder — this assert pins the real-pixel path.
fn assert_library_views_thumbnails() {
    for &(name, base) in LIBRARY_VIEWS_FILES {
        let cache = DiskFolderCache::for_image(Path::new(LIBRARY_VIEWS_FIXTURE_DIR).join(name))
            .expect("views fixture cache");
        let bytes = cache
            .load_preview(name, "vc-original", PreviewKind::Standard)
            .expect("load views preview")
            .unwrap_or_else(|| panic!("seeded preview missing for {name}"));
        let frame = ImageFrame::decode(&bytes).expect("seeded preview decodes");
        assert_eq!(
            frame.width, LIBRARY_VIEWS_PREVIEW_SIZE.0,
            "preview width for {name}"
        );
        assert_eq!(
            frame.height, LIBRARY_VIEWS_PREVIEW_SIZE.1,
            "preview height for {name}"
        );
        let expected = base
            .iter()
            .enumerate()
            .max_by_key(|&(_, channel)| channel)
            .map(|(index, _)| index)
            .expect("non-empty base");
        assert_eq!(
            dominant_channel(&frame.pixels),
            expected,
            "seeded preview for {name} must keep its base color"
        );
        // The vertical ramp must survive the PNG roundtrip: min/max mean-row
        // luminance spread proves non-empty, non-flat pixels.
        let (width, height) = (frame.width as usize, frame.height as usize);
        let mut brightest: u32 = 0;
        let mut darkest: u32 = u32::MAX;
        for y in 0..height {
            let mut row: u32 = 0;
            for x in 0..width {
                let offset = (y * width + x) * 4;
                row += u32::from(frame.pixels[offset])
                    + u32::from(frame.pixels[offset + 1])
                    + u32::from(frame.pixels[offset + 2]);
            }
            brightest = brightest.max(row);
            darkest = darkest.min(row);
        }
        assert!(
            brightest > darkest + 20 * width as u32,
            "seeded preview for {name} must carry the brightness ramp"
        );
    }
}

/// Assert that a label *containing* `needle` is laid out inside the
/// 1024x720 window (same rationale as `assert_label_on_screen`, but for
/// composite labels like the Loupe `name [status]` line whose status suffix
/// is not worth hardcoding).
///
/// NOTE (B1 lesson): a contains-query can match a *different* widget than
/// the intended one (the path text field also contains the fixture dir, a
/// covered line is still "on-screen" for accesskit). Guards for tree nodes
/// and the rating line therefore use exact labels (below) plus a coverage
/// assert — never a bare contains.
fn assert_contains_on_screen(harness: &mut Harness<'_, LuminaApp>, needle: &str) {
    let rect = harness
        .query_all_by_label_contains(needle)
        .next()
        .unwrap_or_else(|| panic!("label containing {needle:?} not found in headed harness"))
        .rect();
    assert!(
        rect.min.y >= 0.0 && rect.max.y <= 720.0 && rect.max.x <= 1024.0,
        "label containing {needle:?} must be pixel-visible in the 1024x720 viewport, got {rect:?}"
    );
}

/// B1 guard: the folder-tree root node, matched EXACTLY (the path text
/// field carries the same directory string, so a contains-query passes
/// vacuously on the field while the tree stays invisible). The root is
/// expanded by production code every frame (`open_folders.insert`), so an
/// exact on-screen match proves a pixel-visible tree node — no test scroll
/// involved.
fn assert_tree_root_on_screen(harness: &mut Harness<'_, LuminaApp>) {
    assert_label_on_screen(harness, "library_views (3)");
}

/// B2 guard: the Loupe rating line must paint strictly above the bottom
/// filmstrip panel. An accesskit on-screen rect alone does not prove it —
/// the line used to sit underneath the filmstrip while still reporting an
/// in-viewport rect (vacuous pass, invisible pixels).
fn assert_rating_above_filmstrip(harness: &mut Harness<'_, LuminaApp>) {
    let rating = harness
        .query_all_by_label_contains("Rating:")
        .next()
        .unwrap_or_else(|| panic!("rating line not found in headed harness"))
        .rect();
    let filmstrip = harness
        .query_all_by_label("Filmstrip")
        .next()
        .unwrap_or_else(|| panic!("filmstrip heading not found in headed harness"))
        .rect();
    assert!(
        rating.max.y <= filmstrip.min.y,
        "rating line {rating:?} must sit above the filmstrip (top {})",
        filmstrip.min.y
    );
}

/// Point a Library harness at the views fixture with a recursive listing.
///
/// Returns the three RAW display-string paths in raster order. Asserts the
/// relative directory (no tempdir-prefix leakage into folder-tree /
/// path-field pixels) plus the 3-file RAW order the views below share.
fn setup_library_views(harness: &mut Harness<'_, LuminaApp>) -> Vec<String> {
    ensure_library_views_fixture();
    assert_library_views_thumbnails();
    harness.state_mut().set_module(Module::Library);
    harness
        .state_mut()
        .set_directory(LIBRARY_VIEWS_FIXTURE_DIR.to_owned());
    // `set_directory` lists flat; the recursive aggregation is the views'
    // shared order (identical here — the fixture is flat).
    harness.state_mut().list_directory();
    assert_eq!(
        harness.state_mut().directory(),
        LIBRARY_VIEWS_FIXTURE_DIR,
        "views must render the relative fixture dir (no tmp-prefix pixels)"
    );
    assert_eq!(
        harness.state_mut().entries().len(),
        3,
        "views fixture must list all files"
    );
    let order = harness.state_mut().filtered_library_order();
    assert_eq!(order.len(), 3, "RAW-only order must see all files");
    let mut keys: Vec<String> = harness
        .state_mut()
        .entries()
        .iter()
        .map(|entry| entry.thumb_key().to_owned())
        .collect();
    keys.sort();
    assert!(keys[0].ends_with("a01.arw"), "unexpected key {}", keys[0]);
    assert!(keys[1].ends_with("a02.arw"), "unexpected key {}", keys[1]);
    assert!(keys[2].ends_with("b01.arw"), "unexpected key {}", keys[2]);
    // Display-string paths (`dir/name`, the unit `select_filmstrip_path`
    // compares against): the flat fixture joins deterministically. A wrong
    // assumption fails loudly at the selection asserts below, never silent.
    let mut paths: Vec<String> = LIBRARY_VIEWS_FILES
        .iter()
        .map(|(name, _)| format!("{LIBRARY_VIEWS_FIXTURE_DIR}/{name}"))
        .collect();
    paths.sort();
    paths
}

/// Loupe (`E`): the active selection shown large with real thumbnail pixels
/// (seeded cache, see above) plus the rating line. The rating line doubles
/// as the "Rating-Sektion im Grid-Kontext"/w visible rating UI of the
/// Library module (the Develop rating section has its own golden).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_loupe() {
    let mut harness = build_harness();
    setup_library_views(&mut harness);
    // A real in-memory decode alongside (Preview-Generations-Assert): proves
    // genuine image content flows while the Loupe view paints thumbnails.
    load_sample(&mut harness);
    harness.run();
    assert!(
        harness.state_mut().preview_generation() >= 1,
        "loaded sample must render at least once"
    );
    harness.state_mut().set_library_view(LibraryView::Loupe);
    // Fixed frames (not `run()`): thumbnail jobs keep requesting repaints.
    harness.run_steps(3);
    // Non-vacuous guards: the Loupe heading, the active file line, the
    // folder-tree root node (B1: exact match — a contains-query passes
    // vacuously on the path text field) and the rating line above the
    // filmstrip (B2) must actually be pixel-visible — otherwise the golden
    // below could pass on the empty-state text the Vision baseline flagged.
    assert_label_on_screen(&mut harness, "Loupe (E): single image");
    assert_contains_on_screen(&mut harness, "a01.arw");
    assert_tree_root_on_screen(&mut harness);
    assert_contains_on_screen(&mut harness, "Rating:");
    assert_rating_above_filmstrip(&mut harness);
    harness.snapshot("library_loupe");
}

/// Compare (`C`): Before/After of the active image side by side (same seeded
/// thumbnail texture twice, `before_after` held by `set_library_view`).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_compare() {
    let mut harness = build_harness();
    setup_library_views(&mut harness);
    harness.state_mut().set_library_view(LibraryView::Compare);
    // Fixed frames (not `run()`): thumbnail jobs keep requesting repaints.
    harness.run_steps(3);
    // Non-vacuous guards: the Compare heading, both pane labels, the
    // live status line (proves the `before_after` Compare branch, not the
    // empty state) and the folder-tree root node (B1: exact match) must
    // actually be on-screen.
    assert_label_on_screen(&mut harness, "Compare");
    assert_label_on_screen(&mut harness, "Before");
    assert_label_on_screen(&mut harness, "After");
    assert_contains_on_screen(&mut harness, "Compare view on (Compare)");
    assert_tree_root_on_screen(&mut harness);
    harness.snapshot("library_compare");
}

/// Survey (`N`): the multi-selection side by side (all three seeded files
/// selected, so the real multi-selection branch renders — not the
/// below-two fallback raster). Folder tree (left) and the expanded keyword
/// chips (right Metadata panel) are part of this golden: the grid-context
/// companions the task requires.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_survey() {
    let mut harness = build_harness();
    let paths = setup_library_views(&mut harness);
    // Select all three (first plain, rest toggle-add): proves the genuine
    // multi-selection branch. A wrong path is a loud no-op in
    // `apply_filmstrip_click`, so the length assert below guards the join
    // assumption in `setup_library_views` too.
    harness
        .state_mut()
        .select_filmstrip_path(paths[0].clone(), false, false);
    harness
        .state_mut()
        .select_filmstrip_path(paths[1].clone(), true, false);
    harness
        .state_mut()
        .select_filmstrip_path(paths[2].clone(), true, false);
    let selected = harness.state_mut().filmstrip_selection();
    assert_eq!(selected.len(), 3, "survey needs all three files selected");
    for path in &paths {
        assert!(
            selected.contains(path),
            "selection must contain {path} (got {selected:?})"
        );
    }
    harness.state_mut().set_library_view(LibraryView::Survey);
    // Fixed frames (not `run()`): thumbnail jobs keep requesting repaints.
    harness.run_steps(3);
    // Non-vacuous guards: heading + live count (proves the multi-selection
    // branch, not the fallback) and the folder-tree root node (B1: exact
    // match, not the path-field contains) as grid context.
    assert_label_on_screen(&mut harness, "Survey");
    assert_label_on_screen(&mut harness, "3 selected");
    assert_tree_root_on_screen(&mut harness);
    // Keyword chips (right Metadata panel): expand the section so the golden
    // pins the input row, not just the collapsed header. Same scroll-first
    // pattern as the metadata subpanels (a below-fold click is discarded).
    harness.run();
    let header_visible = harness
        .query_all_by_label("Keywords")
        .next()
        .map(|node| {
            node.scroll_to_me();
            true
        })
        .unwrap_or(false);
    assert!(
        header_visible,
        "Keywords header not found in headed harness"
    );
    harness.run();
    harness.run();
    let clicked = harness
        .query_all_by_label("Keywords")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(clicked, "Keywords header lost after scrolling");
    harness.run();
    let found = harness
        .query_all_by_label("Add keyword")
        .next()
        .map(|node| {
            node.scroll_to_me();
            true
        })
        .unwrap_or(false);
    assert!(found, "keyword input row missing after expanding");
    harness.run();
    harness.run();
    assert_label_on_screen(&mut harness, "Add keyword");
    // Kosmetik (c/K1): park the pointer fully outside the frame so the
    // rendered cursor cannot leave a tip on the golden (an in-frame park
    // position still showed a 1–2px arrow tip). Out-of-viewport positions
    // are clipped by the renderer and stay invisible.
    harness.hover_at(eframe::egui::Pos2::new(2000.0, 2000.0));
    harness.run_steps(2);
    harness.snapshot("library_survey");
}

// ---------------------------------------------------------------------------
// KITTEST-COVERAGE-OVERLAYS-1: Develop preview overlays + navigator states.
//
// Four overlay goldens (mask matte, edit pins, crop rect, lens-blur focus
// rect) plus the navigator closed golden; the open state is already pinned
// by `navigator_viewport`, and `navigator_open_closed_matrix` below drives
// both rail states headless (open → closed → open).
//
// Pattern per test (68215fd/7fd528b/dd73806/182dfc1): deterministic seeds
// through the public API, non-vacuous model guard + on-screen assert, cursor
// parked outside the frame, then `snapshot`. The mask-matte follow-up
// required a production fix (retained overlay texture) plus a rebaseline of
// `develop_overlay_mask.png`; no other golden was rebaselined.
//
// The overlays themselves are Painter content (invisible to AccessKit, like
// the G-11 pins documented on `visible_edit_pins`), so the on-screen assert
// is an overlay-widget label (panel control scrolled into view) while the
// model guard proves the overlay state is armed. Every preview loads a real
// source and renders it (`load_sample` synchronously, `open_file` pumped to
// `preview_generation() >= 1`, placeholder label asserted absent) — no
// empty-preview goldens. Snapshot-visible are both the painter primitives
// (pin circle, crop/lens strokes, observed) AND the texture blits: the
// `library_loupe` golden proves texture pixels are captured, and the mask
// matte is asserted directly by `assert_mask_overlay_visible` below. The mask
// golden therefore pins the armed state, the panel layout and the matte
// pixels; the DoD §6 Vision check confirms each layout.
//
// Disk discipline: `commit_spot_heal` + `create_mask` + `set_pin_visibility`
// are pure session state (`mark_dirty` only, the debounced commit renders
// without saving), so the pins test stays path-less. Prompt commits
// (`commit_gradient`), crop and lens-blur edits route through the save path
// (`save_sidecar` / debounced `mark_recipe_dirty`), which shows an error on
// a path-less harness — those three tests therefore open a real `photo.png`
// in a tempdir (fixed file name, so `Loaded: photo.png` stays
// deterministic) and point the browser back at `LIBRARY_FIXTURE_DIR`
// afterwards (`open_file_and_restore_fixture`); `assert_no_tmp_leak` proves
// the random tempdir prefix never reaches pixels.
// ---------------------------------------------------------------------------

/// Write the bundled sample PNG as `photo.png` into a fresh tempdir (fixed
/// file name → deterministic status/identity strings; the random tempdir
/// prefix never renders, see `assert_no_tmp_leak`).
fn photo_png_fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let photo = dir.path().join("photo.png");
    std::fs::write(&photo, LuminaApp::sample_image_png()).expect("write png fixture");
    (dir, photo)
}

/// Assert two preview-space points coincide within a subpixel epsilon.
/// Overlay rects are `f32` math (`x * width` on normalized seeds like 0.1),
/// so exact `assert_eq!` fails on 0.5-ulp rounding (e.g. `(0.1 + 0.8) * 400`
/// is not exactly `360.0`); 1e-3 is far below a pixel and keeps the mapping
/// guard exact in intent.
fn assert_pos_near(actual: eframe::egui::Pos2, expected: eframe::egui::Pos2) {
    assert!(
        (actual.x - expected.x).abs() < 1e-3 && (actual.y - expected.y).abs() < 1e-3,
        "expected {expected:?}, got {actual:?}"
    );
}

/// Proves the preview is past the empty state: a real source rendered
/// (`preview_generation() >= 1`) and the empty-state placeholder is absent
/// (its label is `Str::NoImage`: "Drop an image here or load a path").
///
/// Headless trait (corrected, KITTEST-COVERAGE-OVERLAYS-1): texture blits
/// **are** captured in this environment — the `library_loupe` golden paints
/// real thumbnail textures, and the sample preview's own 4x3 pixels are
/// present in the Develop goldens (a handful of native-size texels; the CPU
/// `Image` widget draws the texture at its exact size rather than the fitted
/// pane). Painter primitives (pin circles, rect strokes) are captured too.
/// The mask matte is a texture blit whose golden was previously blank only
/// because `draw_mask_overlay` dropped its per-frame texture handle before the
/// paint; that production bug is fixed and `assert_mask_overlay_visible`
/// now asserts the matte pixels directly.
fn assert_preview_loaded(harness: &mut Harness<'_, LuminaApp>) {
    assert!(
        harness.state_mut().preview_generation() >= 1,
        "a real source must render at least once (no empty-preview golden)"
    );
    assert!(
        harness
            .query_all_by_label("Drop an image here or load a path")
            .next()
            .is_none(),
        "empty-state placeholder must be gone (source loaded)"
    );
}

/// Non-vacuous pixel guard for the mask matte (`KITTEST-COVERAGE-OVERLAYS-1`):
/// renders the current frame and asserts that red-dominant matte pixels are
/// present in quantity and extent. Before the `draw_mask_overlay` lifetime fix
/// the matte texture was freed in the same frame it was uploaded, so the tint
/// never reached the framebuffer and the preview area contained only the
/// sample image's single native-size red texel (roughly a handful of pixels).
///
/// Deliberately NOT a fixed pane coordinate / exact count: the matte spans the
/// fitted full-frame rect (hundreds of points in each dimension), so we assert
/// a lower-bound on the *order of magnitude* (`>= 5_000` pixels) plus a large
/// bounding box. Panel changes shift the constant ~600-pixel "Overlay color"
/// red swatch but can never produce a thousands-scale, several-hundred-point
/// wide red region — only the matte can. `r > g + 20 && r > b + 20` isolates
/// the `[255, 0, 0]` tint from the near-neutral UI while tolerating the
/// translucent composite over the preview pixels.
fn assert_mask_overlay_visible(harness: &mut Harness<'_, LuminaApp>) {
    let rendered = harness.render().expect("kittest renders the frame");
    let (width, height) = rendered.dimensions();
    let mut red_dominant = 0usize;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (width, height, 0u32, 0u32);
    for (x, y, pixel) in rendered.enumerate_pixels() {
        let [r, g, b, _a] = pixel.0;
        if r > g.saturating_add(20) && r > b.saturating_add(20) {
            red_dominant += 1;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    assert!(
        red_dominant >= 5_000,
        "mask matte must paint a thousands-scale red-dominant area, got {red_dominant} pixels"
    );
    let bbox_w = max_x - min_x;
    let bbox_h = max_y - min_y;
    assert!(
        bbox_w >= 200 && bbox_h >= 200,
        "mask matte must span the fitted preview rect, got a {bbox_w}x{bbox_h} red bounding box"
    );
}

/// Leakage proof for tempdir-backed overlay tests: the browser directory is
/// back on the relative fixture (the only path surfaces are the path field
/// and the folder/navigator tree, both fixture-driven after the reset) and
/// no accessible label contains the random tempdir component.
fn assert_no_tmp_leak(harness: &mut Harness<'_, LuminaApp>, tmp: &Path) {
    assert_eq!(
        harness.state_mut().directory(),
        LIBRARY_FIXTURE_DIR,
        "browser must render the relative fixture dir (no tmp-prefix pixels)"
    );
    let component = tmp
        .file_name()
        .and_then(|name| name.to_str())
        .expect("tmp basename");
    assert!(
        harness
            .query_all_by_label_contains(component)
            .next()
            .is_none(),
        "tempdir prefix {component:?} leaked into accessible labels"
    );
}

/// Mask matte overlay (`draw_mask_overlay`): a saved gradient prompt paints
/// the translucent tint over the preview (Show on + `OverlayMode::Always`
/// default, selected mask visible). Seed goes through the public
/// `commit_gradient` API (`ensure_selected_mask` auto-creates "Mask 1"; the
/// sidecar write lands in the tempdir only).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_overlay_mask() {
    let (tmp, photo) = photo_png_fixture();
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    open_file_and_restore_fixture(&mut harness, &photo, |app| app.preview_generation() >= 1);
    assert_no_tmp_leak(&mut harness, tmp.path());
    harness
        .state_mut()
        .commit_gradient(
            lumina_sidecar::Point2 { x: 0.1, y: 0.2 },
            lumina_sidecar::Point2 { x: 0.9, y: 0.8 },
        )
        .expect("seed gradient prompt");
    // `apply_mask_prompt` behind `commit_gradient` invalidates the render
    // identity but does not arm a re-render, leaving a stale frame (and a
    // stale status). `set_zoom_mode(Fit)` is display-neutral (already Fit)
    // and re-arms the render through public API, so the guards below prove
    // the post-seed state instead of the pre-seed frame. (The matte is a
    // retained texture blit that IS captured — see `assert_mask_overlay_visible`
    // — so this golden pins red matte pixels, the armed overlay state and the
    // Masking panel.)
    let ready_gen = harness.state_mut().preview_generation();
    harness.state_mut().set_zoom_mode(ZoomMode::Fit);
    harness.run();
    harness.run();
    harness.run();
    // Non-vacuous guards: the seeded recipe really re-rendered (no stale
    // frame), the source is loaded (no placeholder), the prompt persisted
    // (Valid) on the selected mask, and the Show+mode gate allows the
    // overlay — otherwise the golden below could pass on an unpainted
    // preview.
    assert!(
        harness.state_mut().preview_generation() > ready_gen,
        "seeded prompt must re-render (stale frames hide the overlay)"
    );
    assert_preview_loaded(&mut harness);
    assert!(
        harness.state_mut().selected_mask_id().is_some(),
        "gradient seed must leave a selected mask"
    );
    assert_eq!(
        harness
            .state_mut()
            .selected_mask_status()
            .map(|(status, _)| status),
        Some(lumina_sidecar::MaskStatus::Valid),
        "saved prompt must persist as Valid"
    );
    assert!(
        harness.state_mut().mask_overlay_allowed(),
        "Show switch + Always mode + visible selection must allow the overlay"
    );
    // On-screen assert: the Masking panel scrolled to its entry row, proving
    // the scrolled layout.
    expand_and_scroll_to(&mut harness, SECTION_MASKING, "New Mask");
    assert_label_on_screen(&mut harness, "New Mask");
    harness.hover_at(eframe::egui::Pos2::new(2000.0, 2000.0));
    harness.run_steps(2);
    // Non-vacuous pixel assert on exactly the frame that is snapshotted below:
    // the matte is a texture blit (captured in this environment,
    // KITTEST-COVERAGE-OVERLAYS-1) and must paint a thousands-scale red-dominant
    // area over the fitted preview rect — the earlier lifetime bug left the
    // preview blank (only the sample's single native-size red texel).
    assert_mask_overlay_visible(&mut harness);
    harness.snapshot("develop_overlay_mask");
}

/// Edit pins (`draw_edit_pins`): one spot-heal pin painted as a numbered
/// circle on the preview. `commit_spot_heal` is pure session state
/// (`mark_dirty` only — the debounced commit renders without saving), so
/// this test stays path-less on `load_sample`; `create_mask` (same
/// in-memory precedent as `develop_section_masking`) provides the document
/// the Masking section needs to render the "Edit pins" control. No prompt
/// is seeded, hence no mask pin — the single spot pin is the `>= 1` pin.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_overlay_pins() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    use_library_fixture(&mut harness);
    load_sample(&mut harness);
    harness
        .state_mut()
        .create_mask("Snapshot Pins")
        .expect("seed mask entry");
    harness
        .state_mut()
        .commit_spot_heal(
            lumina_sidecar::Point2 { x: 0.25, y: 0.5 },
            2.0,
            0.5,
            lumina_sidecar::Point2 { x: 0.5, y: 0.0 },
            1.0,
        )
        .expect("seed spot heal");
    harness
        .state_mut()
        .set_pin_visibility(PinVisibility::Always);
    // Non-vacuous guards: pins visible without an armed tool, exactly the
    // seeded spot pin at its normalized anchor with label "1" — otherwise
    // the golden below could pass on a pin-less preview.
    assert!(
        harness.state_mut().pins_visible(),
        "Always must show pins without an armed tool"
    );
    let pins = harness.state_mut().visible_edit_pins();
    assert_eq!(pins.len(), 1, "expected exactly the seeded spot pin");
    assert_eq!(pins[0].label, "1");
    assert!(
        (pins[0].pos.0 - 0.25).abs() < 1e-6 && (pins[0].pos.1 - 0.5).abs() < 1e-6,
        "spot pin must sit at its seeded anchor, got {:?}",
        pins[0].pos
    );
    harness.run();
    harness.run();
    // Non-vacuous guards: the seeded spot really re-rendered (no stale
    // frame), the source is loaded (no placeholder) — plus the pin model
    // above. The pin circle is a painter primitive (snapshot-visible,
    // observed in this golden); the preview texture blit is captured as well
    // (see `assert_preview_loaded`).
    assert!(
        harness.state_mut().preview_generation() > 1,
        "seeded spot must re-render past the load frame"
    );
    assert_preview_loaded(&mut harness);
    // On-screen assert: the pin-visibility control scrolled into view, and
    // the pin circle paints over the preview (primitive content).
    expand_and_scroll_to(&mut harness, SECTION_MASKING, "Edit pins");
    assert_label_on_screen(&mut harness, "Edit pins");
    harness.hover_at(eframe::egui::Pos2::new(2000.0, 2000.0));
    harness.run_steps(2);
    harness.snapshot("develop_overlay_pins");
}

/// Crop-rectangle overlay (`draw_crop_overlay`): the active free-crop rect
/// paints as a white stroke over the preview (`OverlayMode::Always` default
/// shows it without arming crop mode). `set_crop_free` routes through the
/// debounced save path, hence the tempdir-backed harness (the save lands in
/// the tempdir only).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_overlay_crop() {
    let (tmp, photo) = photo_png_fixture();
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    open_file_and_restore_fixture(&mut harness, &photo, |app| app.preview_generation() >= 1);
    assert_no_tmp_leak(&mut harness, tmp.path());
    harness
        .state_mut()
        .set_crop_free(0.1, 0.1, 0.8, 0.6)
        .expect("seed free crop");
    // Non-vacuous guards: the recipe carries the rect, the seeded edit
    // really re-rendered (no stale frame), the source is loaded (no
    // placeholder), and the pure mapping helper resolves the rect onto a
    // 400x300 rect at (40,30)-(360,210) — otherwise the golden could pass
    // on a rect-less preview. Sample dims come from a real decode (4x3),
    // not a literal. The rect stroke is a painter primitive
    // (snapshot-visible, observed in this golden).
    let ready_gen = harness.state_mut().preview_generation();
    assert_preview_loaded(&mut harness);
    {
        let frame = ImageFrame::decode(&LuminaApp::sample_image_png()).expect("sample decodes");
        assert_eq!((frame.width, frame.height), (4, 3));
        let app = harness.state_mut();
        let crop = app
            .recipe()
            .geometry
            .as_ref()
            .and_then(|geometry| geometry.crop.as_ref())
            .expect("crop rect seeded");
        assert!(
            matches!(crop, lumina_sidecar::Crop::Free { .. }),
            "expected the seeded free rect"
        );
        let rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(400.0, 300.0),
        );
        let overlay = LuminaApp::crop_overlay_rect(rect, Some(crop), frame.width, frame.height)
            .expect("free crop must map");
        assert_pos_near(overlay.min, eframe::egui::pos2(40.0, 30.0));
        assert_pos_near(overlay.max, eframe::egui::pos2(360.0, 210.0));
    }
    // On-screen assert: the Geometry Crop controls scrolled into view; the
    // rect stroke is a painter primitive (snapshot-visible, observed here).
    // The scroll frames also settle the debounced post-seed render.
    expand_and_scroll_to(&mut harness, SECTION_GEOMETRY, "Crop");
    assert_label_on_screen(&mut harness, "Crop");
    assert!(
        harness.state_mut().preview_generation() > ready_gen,
        "seeded crop must re-render past the load frame"
    );
    harness.hover_at(eframe::egui::Pos2::new(2000.0, 2000.0));
    harness.run_steps(2);
    harness.snapshot("develop_overlay_crop");
}

/// Scroll the Optics "Lens Blur" nested subgroup into view and click it open
/// (same below-fold lesson as `develop_section_optics2`: scroll first, then
/// click, then re-scroll with the 2-frame settle).
fn open_lens_blur_subgroup(harness: &mut Harness<'_, LuminaApp>) {
    expand_and_scroll_to(harness, SECTION_OPTICS, "Lens Blur");
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
}

/// Lens-blur focus-rect overlay (`draw_lens_blur_overlay`): the enabled
/// stage's focus rect paints as an accent stroke over the preview. Both
/// setters route through the debounced save path, hence the tempdir-backed
/// harness (saves land in the tempdir only).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_overlay_lens_blur() {
    let (tmp, photo) = photo_png_fixture();
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    open_file_and_restore_fixture(&mut harness, &photo, |app| app.preview_generation() >= 1);
    assert_no_tmp_leak(&mut harness, tmp.path());
    harness.state_mut().set_lens_blur_enabled(true);
    harness
        .state_mut()
        .set_lens_blur_focus_rect(0.2, 0.2, 0.6, 0.5)
        .expect("seed focus rect");
    // Non-vacuous guards: the stage enabled, the source loaded (no
    // placeholder), and the pure mapping helper resolves the seeded rect
    // onto a 400x300 rect at (80,60)-(320,210) — otherwise the golden could
    // pass on a rect-less preview. The focus-rect stroke is a painter
    // primitive (snapshot-visible, observed in this golden).
    let ready_gen = harness.state_mut().preview_generation();
    assert_preview_loaded(&mut harness);
    {
        let blur = harness
            .state_mut()
            .lens_blur()
            .expect("lens blur stage seeded");
        assert!(blur.enabled, "lens blur must be enabled");
        let rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(400.0, 300.0),
        );
        let overlay =
            LuminaApp::lens_blur_focus_overlay(rect, Some(&blur)).expect("focus rect must map");
        assert_pos_near(overlay.min, eframe::egui::pos2(80.0, 60.0));
        assert_pos_near(overlay.max, eframe::egui::pos2(320.0, 210.0));
    }
    // On-screen assert: the Lens Blur subgroup header scrolled into view.
    // The scroll frames also settle the debounced post-seed render.
    open_lens_blur_subgroup(&mut harness);
    assert_label_on_screen(&mut harness, "Lens Blur");
    assert!(
        harness.state_mut().preview_generation() > ready_gen,
        "seeded lens blur must re-render past the load frame"
    );
    harness.hover_at(eframe::egui::Pos2::new(2000.0, 2000.0));
    harness.run_steps(2);
    harness.snapshot("develop_overlay_lens_blur");
}

/// Navigator closed: the rail is gone, the preview header offers the
/// "Navigator" reopen button. The "‹" collapse button exists exactly once
/// and only while the rail is open, so its absence is the non-vacuous
/// closed proof (the "Navigator" label alone is ambiguous — heading and
/// button share it).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn navigator_closed() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    use_library_fixture(&mut harness);
    load_sample(&mut harness);
    harness.state_mut().set_navigator_open(false);
    // Two frames: the first settles the closed layout and uploads the
    // preview texture, the second paints without the rail.
    harness.run();
    harness.run();
    // Non-vacuous guards: the source is loaded (no placeholder) and the
    // rail-only collapse button is gone (rail really closed, not just
    // scrolled away).
    assert_preview_loaded(&mut harness);
    assert!(
        harness.query_all_by_label("‹").next().is_none(),
        "rail collapse button must be gone when the navigator is closed"
    );
    // On-screen assert: the preview-header reopen button (the closed-state
    // navigator affordance).
    assert_label_on_screen(&mut harness, "Navigator");
    harness.hover_at(eframe::egui::Pos2::new(2000.0, 2000.0));
    harness.run_steps(2);
    harness.snapshot("navigator_closed");
}

/// Navigator state matrix, headless (no golden): open → closed → open via
/// the public setter. The open golden is `navigator_viewport`, the closed
/// golden is `navigator_closed`; this test proves both states are
/// reachably distinct through the rail-only "‹" collapse button plus the
/// shared "Navigator" label, with real pixels throughout.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn navigator_open_closed_matrix() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    use_library_fixture(&mut harness);
    load_sample(&mut harness);
    // Open: rail heading + collapse button pixel-visible, source loaded.
    harness.state_mut().set_navigator_open(true);
    harness.run();
    harness.run();
    assert_preview_loaded(&mut harness);
    assert_label_on_screen(&mut harness, "Navigator");
    assert_label_on_screen(&mut harness, "‹");
    // Closed: collapse button gone, reopen button in its place.
    harness.state_mut().set_navigator_open(false);
    harness.run();
    harness.run();
    assert!(
        harness.query_all_by_label("‹").next().is_none(),
        "rail collapse button must be gone when closed"
    );
    assert_label_on_screen(&mut harness, "Navigator");
    // Re-open: the rail comes back (no stuck-closed state).
    harness.state_mut().set_navigator_open(true);
    harness.run();
    harness.run();
    assert_label_on_screen(&mut harness, "‹");
    assert_label_on_screen(&mut harness, "Navigator");
}

// ---------------------------------------------------------------------------
// KITTEST-COVERAGE-STATES-1: UI states pixel-visible per kittest golden —
// toast (info), error popup dialog, empty states, missing-sidecar hint, the
// metadata panel's own copy/paste system and the 20-dummy filmstrip.
//
// Pattern per test (KITTEST-COVERAGE-*): deterministic seeds through the
// public API, a non-vacuous model/label guard + on-screen assert, then
// `snapshot`. No existing golden is rebaselined.
// ---------------------------------------------------------------------------

/// Info toast (`show_toast`): the transient overlay owns the preview-ready
/// signal. `show_toast` is shown at an egui-time far above the headless clock
/// so it stays visible when the frame is read back (same trick as the
/// agent-harness `toast_overlap` probe).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn toast_info() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    use_library_fixture(&mut harness);
    load_sample(&mut harness);
    harness
        .state_mut()
        .show_toast("Preview ready".to_string(), 1000.0);
    harness.run();
    harness.run();
    // Non-vacuous model guard: the toast state machine is armed.
    assert!(
        harness.state_mut().toast_visible(1000.0),
        "info toast must be visible after show_toast"
    );
    // The overlay Area is exposed to AccessKit; when it is, guard the pixels.
    if harness.query_all_by_label("Preview ready").next().is_some() {
        assert_label_on_screen(&mut harness, "Preview ready");
        assert_label_on_screen(&mut harness, "Dismiss");
    }
    harness.snapshot("toast_info");
}

/// Error popup dialog: `show_error` surfaces a failure as a floating dialog
/// (and logs it at `error!`). Triggered through the Export panel's Export
/// button with the default empty destination → the fixed code literal
/// "Choose an export target first" (machine-independent golden text).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn error_dialog() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Export);
    load_sample(&mut harness);
    harness.run();
    // Two Button-role "Export" nodes exist (module bar + run button); the run
    // button is the one in the right panel.
    let clicked = harness
        .query_all_by_label("Export")
        .find(|node| {
            node.accesskit_node().role() == eframe::egui::accesskit::Role::Button
                && node.rect().min.x > 600.0
        })
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(clicked, "export run button not found in headed harness");
    harness.run();
    harness.run();
    // Non-vacuous model guard: the failure reached the loud path.
    assert!(
        harness.state_mut().error().is_some(),
        "empty destination must raise the loud export error"
    );
    assert_eq!(harness.state_mut().status(), "Error");
    // Dialog content is on-screen, not just in the tree.
    assert_label_on_screen(&mut harness, "Close");
    assert_label_on_screen(&mut harness, "Choose an export target first");
    harness.hover_at(eframe::egui::Pos2::new(2000.0, 2000.0));
    harness.run_steps(2);
    harness.snapshot("error_dialog");
}

/// Empty Develop state (no image loaded): the centered preview placeholder and
/// the honest empty filmstrip text — the counterpart to `library_empty`.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_empty() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    harness.run();
    // Non-vacuous guards: both empty-state texts must actually be on-screen.
    assert_label_on_screen(&mut harness, "Drop an image here or load a path");
    assert_label_on_screen(&mut harness, "No images in this folder");
    harness.snapshot("develop_empty");
}

/// Missing-sidecar hint: the Develop History section without a loaded document
/// shows "No sidecar loaded". `load_bytes` leaves `document` empty, so no seed
/// is needed (the existing `develop_section_history` golden seeds one).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn develop_history_no_sidecar() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    // Only History open (a plain collapsing header outside the eight F-100
    // sections); scroll its content into view.
    open_collapsing_and_scroll_to(&mut harness, "History", "No sidecar loaded");
    // Non-vacuous guard: the hint must actually be on-screen.
    assert_label_on_screen(&mut harness, "No sidecar loaded");
    harness.snapshot("develop_history_no_sidecar");
}

/// Export-panel variant: JPEG selected via the format setter (PNG is the
/// default) — the PNG-only "Quality applies to JPEG / WebP only" hint goes
/// away and the quality slider is live. No metadata flag exists in the GUI
/// (metadata is a Library concern); the Library metadata copy/paste state is
/// pinned separately.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn export_module_jpeg() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Export);
    harness.state_mut().set_export_format(ImageFileFormat::Jpeg);
    load_sample(&mut harness);
    harness.run();
    harness.run();
    // Non-vacuous guard: the PNG-only quality hint is gone (JPEG selected),
    // while the quality control itself stays visible.
    assert!(
        harness
            .query_all_by_label("Quality applies to JPEG / WebP only")
            .next()
            .is_none(),
        "JPEG must drop the PNG-only quality hint"
    );
    assert_label_on_screen(&mut harness, "Quality");
    harness.snapshot("export_module_jpeg");
}

/// Library metadata panel's own copy/paste system: a draft with entered values,
/// copied through the panel's Copy button (status "Metadata copied (2 field(s))")
/// and the Paste affordance pixel-visible. Uses a real path so the commit lands
/// in a tempdir only; `open_file_and_restore_fixture` keeps tmp prefixes out of
/// the golden.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_metadata_copy_paste() {
    let (tmp, photo) = photo_png_fixture();
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    open_file_and_restore_fixture(&mut harness, &photo, |app| app.preview_generation() >= 1);
    assert_no_tmp_leak(&mut harness, tmp.path());
    harness
        .state_mut()
        .set_metadata_buffer("title", "Startschuss".to_string())
        .expect("set title buffer");
    harness
        .state_mut()
        .set_metadata_buffer("city", "Berlin".to_string())
        .expect("set city buffer");
    assert!(
        harness
            .state_mut()
            .commit_metadata_draft()
            .expect("commit metadata draft"),
        "entered metadata must persist (sidecar write)"
    );
    // Layout frame + expand the draft editor (there is no library_metadata
    // helper that opens it after a real-path load).
    harness.run();
    harness.run();
    let opened = harness
        .query_all_by_label("Metadata draft")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(opened, "Metadata draft section not found in headed harness");
    harness.run();
    // Copy through the panel's own copy/paste system.
    let copied = harness
        .query_all_by_label("Copy metadata")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(copied, "Copy metadata button not found in headed harness");
    harness.run();
    // Non-vacuous guards: the clipboard captured both entered fields and the
    // status proves the copy ran.
    assert!(
        harness.state_mut().status().contains("Metadata copied"),
        "copy must announce the copied fields, got {:?}",
        harness.state_mut().status()
    );
    harness.run();
    // Both clipboard buttons sit in the draft editor's action row, on-screen
    // without scrolling (scrolling would clip the panel's "Metadata" heading).
    assert_label_on_screen(&mut harness, "Copy metadata");
    assert_label_on_screen(&mut harness, "Paste metadata");
    assert_label_on_screen(&mut harness, "Title");
    harness.snapshot("library_metadata_copy_paste");
}

/// Filmstrip with 20 dummy RAWs: the single-row geometry (asserted, see
/// `assert_filmstrip_single_row`) plus a pixel golden. The bundled sample is
/// loaded first so the F-100 auto-load never decodes an invalid dummy (which
/// would raise the error dialog); the strip still lists all 20 RAW cells.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn filmstrip_twenty_dummies() {
    let dir = temp_raw_dir(20);
    let mut harness = build_harness();
    // Load a valid source first: suppresses the RAW auto-load (which would
    // fail loudly on the sentinel bytes) and gives the preview real pixels.
    load_sample(&mut harness);
    harness
        .state_mut()
        .set_directory(dir.path().display().to_string());
    harness.state_mut().set_module(Module::Develop);
    // Fixed frames (not `run()`): thumbnail jobs keep requesting repaints.
    harness.run_steps(3);
    // Single-row geometry for all visible cells.
    let chips = assert_filmstrip_single_row(&mut harness);
    assert!(
        chips.len() >= 2,
        "20-dummy strip must lay out several cells, got {chips:?}"
    );
    // Non-vacuous guard: the strip knows all 20 entries (selection resolved to
    // the first entry, so the header shows "1 of 20").
    assert_contains_on_screen(&mut harness, "of 20");
    harness.snapshot("filmstrip_twenty_dummies");
}
