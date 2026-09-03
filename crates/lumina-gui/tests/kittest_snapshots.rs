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
    // in the bottom band (y > 500 of a 720-high window).
    let mut chips: Vec<eframe::egui::Rect> = harness
        .query_all_by(|n| n.role() == eframe::egui::accesskit::Role::Unknown)
        .filter(|n| n.accesskit_node().bounding_box().is_some())
        .map(|n| n.rect())
        .filter(|r| r.min.y > 500.0 && r.height() > 60.0 && r.width() > 60.0)
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
