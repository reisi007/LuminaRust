//! startup/app lifecycle (sections, selection, decode, fullscreen) tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// F-100 / F-103-N10 (user decision 2026-08-25): the Develop sections are
/// drawn in Lightroom Classic panel order — **Detail BEFORE Effects**.
/// `draw_develop_panel` renders exactly this table, so this pins the real
/// render order without a GPU harness.
#[test]
fn develop_section_order_is_lightroom_conform() {
    let order: Vec<Str> = LuminaApp::DEVELOP_SECTIONS
        .iter()
        .map(|(label, _)| *label)
        .collect();
    assert_eq!(
        order,
        vec![
            Str::Basic,
            Str::ToneCurve,
            Str::Color,
            Str::Detail,
            Str::Effects,
            Str::Optics,
            Str::Geometry,
            Str::Masking,
        ]
    );
    let detail = order.iter().position(|s| *s == Str::Detail).unwrap();
    let effects = order.iter().position(|s| *s == Str::Effects).unwrap();
    assert!(detail < effects, "Detail must precede Effects");
}

/// Kittest table sync (F-103-N9, GPU-gated): `tests/kittest_snapshots.rs`
/// clicks Develop sections by label (`collapse_except`), so a label rename
/// or reorder here orphans those clicks. This headless test pins the same
/// table without a GPU harness. UX-LOOK-LAYOUT-18: the left-rail panels
/// (Navigator/Presets/Snapshot/History) precede the eight right-panel F-100
/// sections. Panel rects (position/size) genuinely need a laid-out harness
/// and stay covered by the kittest interaction tests
/// (`filmstrip_is_single_row_horizontal`, …).
#[test]
fn develop_section_labels_match_kittest_table() {
    let kittest_table = [
        "Navigator",
        "Presets",
        "Snapshot",
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
    let mut labels = vec![
        Str::Navigator.t(),
        Str::PresetsSection.t(),
        Str::SnapshotButton.t(),
        Str::History.t(),
    ];
    labels.extend(LuminaApp::DEVELOP_SECTIONS.iter().map(|(s, _)| s.t()));
    assert_eq!(labels, kittest_table);
    let detail = labels.iter().position(|l| *l == "Detail").unwrap();
    let effects = labels.iter().position(|l| *l == "Effects").unwrap();
    assert!(detail < effects, "Detail must precede Effects");
}

/// GUI-VISION-1 (F-100): the filmstrip is visible in all three modules
/// (Library, Develop, Export). `Tab` panels-hide keeps it; `L`
/// lights-out and `F` fullscreen hide it.
#[test]
fn filmstrip_visible_in_all_three_modules() {
    let mut app = new_app();
    for module in [Module::Library, Module::Develop, Module::Export] {
        app.set_module(module);
        assert!(
            app.shows_filmstrip(),
            "filmstrip must be visible in {module:?} (F-100)"
        );
    }
    app.set_module(Module::Export);
    app.lights_out = true;
    assert!(!app.shows_filmstrip(), "lights-out hides the filmstrip");
    app.lights_out = false;
    app.fullscreen = true;
    assert!(!app.shows_filmstrip(), "fullscreen hides the filmstrip");
    app.fullscreen = false;
    app.panels_hidden = true;
    assert!(app.shows_filmstrip(), "Tab panels-hide keeps the filmstrip");
}

/// GUI-STARTUP-SELECTION-1 (F-100 Startverhalten): an empty directory
/// selects nothing and loads nothing — no phantom selection, no decode.
#[test]
fn startup_empty_directory_selects_nothing_and_loads_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = new_app();
    app.set_directory(directory.path().display().to_string());
    assert!(app.entries().is_empty());
    assert!(
        app.filmstrip_selection().is_empty(),
        "empty directory must leave the selection empty"
    );
    assert!(app.original.is_none());
    assert!(
        app.decode_rx.is_none(),
        "empty directory must not start a decode"
    );
    assert!(app.error().is_none());
}

/// GUI-STARTUP-SELECTION-1 (F-100 Startverhalten): a single image is
/// selected synchronously (like the click path) and loaded through the
/// existing background decode — selection and path stay consistent.
#[test]
fn startup_single_image_is_selected_and_loaded() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("a.png");
    std::fs::write(&path, png()).unwrap();
    let wanted = path.display().to_string();
    let mut app = new_app();
    app.set_directory(directory.path().display().to_string());
    assert_eq!(
        app.filmstrip_selection(),
        vec![wanted.clone()],
        "single image must be selected right after the scan"
    );
    drain_auto_load(&mut app);
    assert!(
        app.error().is_none(),
        "unexpected decode error: {:?}",
        app.error()
    );
    assert!(app.original.is_some());
    assert_eq!(app.path, wanted);
    assert_eq!(app.filmstrip_selection(), vec![wanted]);
}

/// GUI-STARTUP-SELECTION-1 (F-100 Startverhalten): with several images the
/// first in grid (name) sort order is selected and loaded — for every
/// supported format, not just RAW. The fake RAW lists (extension-only
/// scan, no decode at scan time) but is never picked over the earlier
/// PNG.
#[test]
fn startup_first_in_grid_order_is_selected_and_loaded_mixed_formats() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("a-first.png");
    let raw = directory.path().join("m-middle.arw");
    let last = directory.path().join("z-last.png");
    std::fs::write(&first, png()).unwrap();
    std::fs::write(&raw, b"not a real raw file").unwrap();
    std::fs::write(&last, png()).unwrap();
    let wanted = first.display().to_string();
    let mut app = new_app();
    app.set_directory(directory.path().display().to_string());
    assert_eq!(app.entries().len(), 3);
    assert_eq!(
        app.filmstrip_selection(),
        vec![wanted.clone()],
        "first grid entry must be selected right after the scan"
    );
    drain_auto_load(&mut app);
    assert!(
        app.error().is_none(),
        "unexpected decode error: {:?}",
        app.error()
    );
    assert_eq!(app.path, wanted);
    assert_eq!(app.filmstrip_selection(), vec![wanted]);
}

/// GUI-STARTUP-FOLLOWUP-1 (B4, F-100 Startverhalten): a leading JPEG is
/// selected in grid order and really decoded — same shape as the PNG
/// startup test, but with genuine JPEG bytes through `ImageFrame::decode`.
#[test]
fn startup_first_in_grid_order_is_selected_and_loaded_jpeg() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("a-first.jpg");
    let last = directory.path().join("z-last.png");
    std::fs::write(&first, jpeg()).unwrap();
    std::fs::write(&last, png()).unwrap();
    let wanted = first.display().to_string();
    let mut app = new_app();
    app.set_directory(directory.path().display().to_string());
    assert_eq!(app.entries().len(), 2);
    assert_eq!(
        app.filmstrip_selection(),
        vec![wanted.clone()],
        "first grid entry (JPEG) must be selected right after the scan"
    );
    drain_auto_load(&mut app);
    assert!(
        app.error().is_none(),
        "unexpected JPEG decode error: {:?}",
        app.error()
    );
    assert!(app.original.is_some(), "JPEG must really decode");
    assert_eq!(app.path, wanted);
    assert_eq!(app.filmstrip_selection(), vec![wanted]);
}

/// GUI-STARTUP-FOLLOWUP-1 (B4, F-100 Startverhalten): a leading WebP is
/// selected in grid order and really decoded — same shape as the PNG
/// startup test, but with genuine WebP bytes through `ImageFrame::decode`.
#[test]
fn startup_first_in_grid_order_is_selected_and_loaded_webp() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("a-first.webp");
    let last = directory.path().join("z-last.png");
    std::fs::write(&first, webp()).unwrap();
    std::fs::write(&last, png()).unwrap();
    let wanted = first.display().to_string();
    let mut app = new_app();
    app.set_directory(directory.path().display().to_string());
    assert_eq!(app.entries().len(), 2);
    assert_eq!(
        app.filmstrip_selection(),
        vec![wanted.clone()],
        "first grid entry (WebP) must be selected right after the scan"
    );
    drain_auto_load(&mut app);
    assert!(
        app.error().is_none(),
        "unexpected WebP decode error: {:?}",
        app.error()
    );
    assert!(app.original.is_some(), "WebP must really decode");
    assert_eq!(app.path, wanted);
    assert_eq!(app.filmstrip_selection(), vec![wanted]);
}

/// GUI-STARTUP-SELECTION-1 (F-100 Startverhalten): rescanning a populated
/// directory starts no second decode and never desyncs path vs.
/// selection — through both the flat and the recursive collector.
#[test]
fn rescan_is_stable_without_second_decode_or_selection_desync() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("a.png");
    let second = directory.path().join("b.png");
    std::fs::write(&first, png()).unwrap();
    std::fs::write(&second, png()).unwrap();
    let wanted = first.display().to_string();
    let mut app = new_app();
    app.set_directory(directory.path().display().to_string());
    drain_auto_load(&mut app);
    assert!(app.error().is_none());
    assert!(app.preview_generation() > 0);
    let generation = app.preview_generation();
    for _ in 0..2 {
        app.list_directory();
        assert!(
            app.decode_rx.is_none(),
            "rescan must not start a second decode"
        );
        assert_eq!(app.path, wanted);
        assert_eq!(app.filmstrip_selection(), vec![wanted.clone()]);
        assert_eq!(app.preview_generation(), generation);
        app.list_directory_flat();
        assert!(
            app.decode_rx.is_none(),
            "flat rescan must not start a second decode"
        );
        assert_eq!(app.path, wanted);
        assert_eq!(app.filmstrip_selection(), vec![wanted.clone()]);
        assert_eq!(app.preview_generation(), generation);
    }
}

/// GUI-STARTUP-SELECTION-1 (F-100 Startverhalten): deleting the selected
/// image on disk falls back to its successor on rescan; the selection is
/// empty only once no images remain. The loaded preview itself is
/// untouched by the rescan (no unload, no second decode).
#[test]
fn deleting_selected_image_falls_back_to_successor() {
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("a.png");
    let second = directory.path().join("b.png");
    std::fs::write(&first, png()).unwrap();
    std::fs::write(&second, png()).unwrap();
    let second_path = second.display().to_string();
    let mut app = new_app();
    app.set_directory(directory.path().display().to_string());
    drain_auto_load(&mut app);
    assert!(app.error().is_none());
    std::fs::remove_file(&first).unwrap();
    app.set_directory(directory.path().display().to_string());
    assert_eq!(app.entries().len(), 1);
    assert_eq!(
        app.filmstrip_selection(),
        vec![second_path.clone()],
        "selection must fall back to the successor, never go empty"
    );
    assert!(
        app.decode_rx.is_none(),
        "fallback selection must not trigger a decode"
    );
    assert!(app.original.is_some(), "loaded preview stays");
    std::fs::remove_file(&second).unwrap();
    app.set_directory(directory.path().display().to_string());
    assert!(app.entries().is_empty());
    assert!(
        app.filmstrip_selection().is_empty(),
        "selection is empty only when no images remain"
    );
    assert!(app.decode_rx.is_none());
}

/// GUI-STARTUP-SELECTION-1 (F-100 Startverhalten): an unloadable image is
/// a loud error, never a silent fallback — and the selection still covers
/// it while the image exists.
#[test]
fn startup_decode_failure_is_loud_and_keeps_selection() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("corrupt.png");
    std::fs::write(&path, b"not an image at all").unwrap();
    let wanted = path.display().to_string();
    let mut app = new_app();
    app.set_directory(directory.path().display().to_string());
    assert_eq!(app.filmstrip_selection(), vec![wanted.clone()]);
    drain_auto_load(&mut app);
    assert!(app.original.is_none());
    assert!(app.path.is_empty());
    let message = app.error().unwrap_or("").to_string();
    assert!(
        message.contains("corrupt.png"),
        "decode failure must name the file loudly, got: {message:?}"
    );
    assert_eq!(
        app.filmstrip_selection(),
        vec![wanted],
        "selection stays while the image exists, even unloadable"
    );
}

/// GUI-STARTUP-MODULEFLAGS-1 (F-100 Startverhalten): the default start is
/// Develop without fullscreen.
#[test]
fn startup_default_is_develop_without_fullscreen() {
    let app = new_app();
    assert_eq!(app.active_module, Module::Develop);
    assert!(!app.fullscreen);
    assert!(!app.chrome_hidden());
}

/// GUI-STARTUP-MODULEFLAGS-1 (F-100 Startverhalten): every `--module`
/// value maps to its module through the existing setter (no recipe or
/// sidecar side effects by construction of `set_module`).
#[test]
fn start_module_values_map_to_all_three_modules() {
    for module in [Module::Library, Module::Develop, Module::Export] {
        let mut app = new_app();
        app.set_module(module);
        assert_eq!(app.active_module, module);
    }
}

/// GUI-STARTUP-MODULEFLAGS-1 (F-100 Startverhalten): `set_fullscreen`
/// hides the working chrome exactly like the `F` toggle (zoom settles on
/// Fit on entry) and restores it on exit; repeating the current state is
/// a no-op that leaves the status line untouched.
#[test]
fn set_fullscreen_hides_working_chrome_and_restores() {
    let mut app = new_app();
    app.set_zoom_mode(ZoomMode::OneToOne);
    app.set_fullscreen(true);
    assert!(app.fullscreen);
    assert!(app.chrome_hidden());
    assert!(!app.shows_filmstrip());
    assert_eq!(app.zoom_mode, ZoomMode::Fit);
    let status = app.status().to_string();
    app.set_fullscreen(true);
    assert_eq!(app.status(), status, "re-setting must be a no-op");
    app.set_fullscreen(false);
    assert!(!app.fullscreen);
    assert!(!app.chrome_hidden());
    assert!(app.shows_filmstrip());
}
