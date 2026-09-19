//! R2-MODSWITCH-1 F7: Library↔Develop module-switch latency tests.
//!
//! Three properties are pinned headless (no GPU):
//!
//! 1. The thumbnail cache-hit path is **off the UI thread**: `ensure_thumbnail`
//!    performs a metadata-only probe and enqueues; no texture is produced
//!    synchronously. The worker decodes the cached preview and the result is
//!    filed under the correct key after the poll.
//! 2. The per-folder preview index is built **once per folder**, not once per
//!    cell/frame, and its memoized settings update only after a visible
//!    invalidation (never a silent stale gate).
//! 3. A full render that would otherwise fire in the module-switch frame is
//!    deferred by one frame; the "Stale" badge (`render_key.is_none()`) keeps
//!    the lag visible in between.

use super::*;
use crate::thumb_cache::THUMB_VIRTUAL_COPY;
use lumina_core::cache::disk::DiskFolderCache;
use lumina_core::cache::PreviewKind;

/// A distinct 4×4 red PNG (different pixels from `save_png`'s 2×1 fixture), so
/// a cache hit cannot be confused with a fresh source render.
fn red_png() -> Vec<u8> {
    let mut pixels = Vec::with_capacity(4 * 4 * 4);
    for _ in 0..16 {
        pixels.extend_from_slice(&[220, 30, 40, 255]);
    }
    ImageFrame::new(4, 4, pixels)
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap()
}

/// Pump the thumbnail worker results like the real `update` loop does, bounded
/// so a genuine failure cannot hang the suite.
fn drain_thumbnails(app: &mut LuminaApp, ctx: &egui::Context, key: &str) {
    for _ in 0..2000 {
        app.poll_thumbnails(ctx);
        if app.thumbnails.get(key).is_some() || app.thumbnails.failure(key).is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn module_switch_thumbnail_cache_hit_is_off_thread_and_assigned() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    // Seed the standard preview the scheduler will report as a metadata hit.
    let cache = DiskFolderCache::for_image(&source).unwrap();
    cache
        .store_preview(
            &source.file_name().unwrap().to_string_lossy(),
            THUMB_VIRTUAL_COPY,
            PreviewKind::Standard,
            &red_png(),
        )
        .unwrap();

    let mut app = new_app();
    app.set_directory(directory.path().display().to_string());
    let entry = app.entries()[0].clone();
    let key = entry.thumb_key.clone();
    let ctx = egui::Context::default();

    // The metadata probe is a hit…
    let probe = app.thumbnail_cache.probe(directory.path(), &entry.name);
    assert!(probe.cached, "seeded preview must be a metadata hit");

    // …and the UI thread must NOT load/decode it synchronously: the call only
    // enqueues, so no texture exists immediately after.
    assert!(app.ensure_thumbnail(&ctx, &entry));
    assert!(
        app.thumbnails.get(&key).is_none(),
        "the cache-hit decode must run in the worker, never synchronously on the UI thread"
    );
    assert!(
        !app.thumbnails.needs_job(&key),
        "the key must be marked in-flight so no duplicate job is scheduled"
    );

    // The worker result appears after the poll, filed under the correct key.
    drain_thumbnails(&mut app, &ctx, &key);
    assert!(
        app.thumbnails.get(&key).is_some(),
        "the worker must deliver the cached preview after the poll"
    );
    assert_eq!(app.thumbnails.failure(&key), None);
}

/// A metadata hit whose cached bytes are corrupt must surface as a visible
/// worker failure (bounded retry), never a silent gray/wrong cell.
#[test]
fn corrupt_cached_preview_is_a_visible_failure() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let name = source.file_name().unwrap().to_string_lossy().to_string();
    let cache = DiskFolderCache::for_image(&source).unwrap();
    cache
        .store_preview(
            &name,
            THUMB_VIRTUAL_COPY,
            PreviewKind::Standard,
            b"not-a-png",
        )
        .unwrap();

    let mut app = new_app();
    app.set_directory(directory.path().display().to_string());
    let entry = app.entries()[0].clone();
    let key = entry.thumb_key.clone();
    let ctx = egui::Context::default();
    assert!(app.ensure_thumbnail(&ctx, &entry));
    drain_thumbnails(&mut app, &ctx, &key);
    assert!(
        app.thumbnails.failure(&key).is_some(),
        "an unreadable cached preview must be a visible error, not a silent miss"
    );
}

/// The folder preview index is built once per folder and reused across cells
/// and frames — the former per-cell `create_dir_all`/settings/read cost.
#[test]
fn preview_index_is_built_once_per_folder_not_per_cell() {
    let (mut app, _dir, indices) = app_with_entries(20);
    let ctx = egui::Context::default();
    let builds_before = app.thumbnail_cache.builds();
    app.ensure_thumbnail_priority(&ctx, &indices, 0..10);
    assert_eq!(
        app.thumbnail_cache.builds(),
        builds_before + 1,
        "the whole visible window shares one index build"
    );
    app.ensure_thumbnail_priority(&ctx, &indices, 10..20);
    assert_eq!(
        app.thumbnail_cache.builds(),
        builds_before + 1,
        "later frames/cells reuse the warm index"
    );
}

/// Schedule a full render and drive the real scheduler: the module-switch frame
/// defers it (Stale stays visible), the next frame commits it.
#[test]
fn module_switch_defers_full_render_out_of_the_switch_frame() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.render().unwrap();
    let ctx = egui::Context::default();

    // Baseline scheduler run (Develop, nothing pending).
    app.schedule_render(&ctx);

    // Arm an edit (immediate full render, `last_edit_time == 0`) and switch.
    app.set_adjustment("exposure", 1.0);
    assert!(app.pending_full_render);
    assert!(app.render_key.is_none());
    app.set_module(Module::Library);

    // Switch frame: the scheduled render must be deferred, not run synchronously.
    app.schedule_render(&ctx);
    assert!(
        app.pending_full_render,
        "the full render must stay deferred in the switch frame"
    );
    assert!(
        app.render_key.is_none(),
        "the Stale badge must stay visible while the render is deferred"
    );

    // Next frame commits it.
    app.schedule_render(&ctx);
    assert!(
        !app.pending_full_render,
        "the deferred render must land on the next frame"
    );
    assert!(app.render_key.is_some(), "the full render must complete");
    assert!(app.error().is_none(), "render must not fail");
}

/// R3-DENOISE-1: one neighbor failure is one event — the controller emits no
/// warning, the app-level drain emits exactly one. The former two-level
/// double-`warn!` (controller + app) for the same failure is gone.
#[test]
fn one_neighbor_failure_emits_exactly_one_app_level_warn() {
    use crate::preview_ctrl::{PreviewController, PreviewJob};
    use lumina_core::preview_cache::PreviewKind;

    let mut app = new_app();
    let ctx = egui::Context::default();
    let mut ctrl = PreviewController::spawn(1).0;
    // A real worker round trip that must fail (source does not exist).
    ctrl.enqueue(PreviewJob {
        probe_id: "missing-neighbor".into(),
        source: std::path::PathBuf::from("/nonexistent/r3-denoise-1.png"),
        name: "r3-denoise-1.png".into(),
        virtual_copy: "vc-original".into(),
        target: (8, 8),
        kind: PreviewKind::Screen,
        priority: 0,
    });
    app.preview_ctrl = Some(ctrl);
    let _ = crate::timing::take_neighbor_failure_warns();

    // Pump the worker result into `pending_failed`, then let the app drain it.
    let mut drained = false;
    for _ in 0..2000 {
        if let Some(ctrl) = app.preview_ctrl.as_mut() {
            ctrl.poll();
        }
        app.poll_neighbor_previews(&ctx);
        if app
            .preview_ctrl
            .as_ref()
            .is_some_and(|ctrl| ctrl.failure("missing-neighbor").is_some())
        {
            drained = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(drained, "the worker failure must surface");
    assert_eq!(
        crate::timing::take_neighbor_failure_warns(),
        1,
        "one failed neighbor job = exactly one app-level warn"
    );
    assert!(
        app.preview_ctrl
            .as_ref()
            .and_then(|ctrl| ctrl.failure("missing-neighbor"))
            .is_some(),
        "the visible failure state must persist for the badge"
    );
}

/// R3-OPEN-1: pump the background decode until the in-flight request settles
/// (success or loud failure). `drain_auto_load` stops early when an image is
/// already loaded, so it cannot observe a later switch's decode.
fn drain_pending_decode(app: &mut LuminaApp) {
    for _ in 0..2000 {
        app.poll_decode();
        if app.decode_rx.is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// R3-OPEN-1 (Lightroom behaviour): a Develop switch with exactly one
/// selection (≠ loaded) opens that image through the shared `open_file` path;
/// zero or multiple selections leave the loaded image untouched.
#[test]
fn develop_switch_opens_single_selection_and_ignores_zero_or_many() {
    let directory = tempfile::tempdir().unwrap();
    // Fabricated RAW entries: the filmstrip selection is RAW-only, and the
    // decode itself is irrelevant here — the test pins the *open* (the decode
    // target), not a successful RAW decode.
    let mut app = new_app();
    app.directory = directory.path().display().to_string();
    app.entries = vec![
        raw_entry(directory.path(), "a.cr3"),
        raw_entry(directory.path(), "b.cr3"),
    ];
    let order = app.filmstrip_order();
    assert_eq!(order.len(), 2);
    app.path = order[0].clone();
    app.filmstrip_selection = BTreeSet::from([order[0].clone()]);

    // Exactly one selection ≠ loaded → the Develop switch opens it.
    app.set_module(Module::Library);
    app.select_filmstrip_path(order[1].clone(), false, false);
    assert_eq!(app.filmstrip_selection(), vec![order[1].clone()]);
    app.set_module(Module::Develop);
    assert!(
        app.decode_rx.is_some(),
        "the Develop switch must start the background decode"
    );
    assert_eq!(
        app.pending_load_path.as_deref(),
        Some(order[1].as_str()),
        "the decode must target the single selection"
    );
    // Settle the (expected to fail) fake-RAW decode before the next case.
    drain_pending_decode(&mut app);
    assert_eq!(app.path, order[0], "a failed decode keeps the loaded image");

    // 0 selected → the loaded image stays, no decode.
    app.filmstrip_selection.clear();
    app.set_module(Module::Library);
    app.set_module(Module::Develop);
    assert!(
        app.decode_rx.is_none(),
        "no selection must not start a decode"
    );
    assert_eq!(app.path, order[0], "the loaded image stays");

    // >1 selected → the loaded image stays, no decode.
    app.select_filmstrip_path(order[0].clone(), false, false);
    app.select_filmstrip_path(order[1].clone(), true, false);
    assert_eq!(app.filmstrip_selection().len(), 2);
    app.set_module(Module::Library);
    app.set_module(Module::Develop);
    assert!(
        app.decode_rx.is_none(),
        "a multi-selection must not open one image"
    );
    assert_eq!(app.path, order[0], "the loaded image stays");
}

/// R3-OPEN-1: a selection that points at a vanished file is a loud error
/// (the existing `open_file` failure path) and must not adopt the path.
#[test]
fn develop_switch_open_of_missing_file_is_loud() {
    let directory = tempfile::tempdir().unwrap();
    let a = directory.path().join("a.png");
    save_png(&a);
    let a_path = a.display().to_string();
    let missing = directory.path().join("missing.png").display().to_string();
    let mut app = new_app();
    app.set_directory(directory.path().display().to_string());
    drain_auto_load(&mut app);
    assert_eq!(app.path, a_path);

    // A vanished path cannot be selected through the order-based helper, so
    // seed the selection directly (the real stale-selection state).
    app.filmstrip_selection = BTreeSet::from([missing.clone()]);
    app.set_module(Module::Library);
    app.set_module(Module::Develop);
    drain_pending_decode(&mut app);
    assert!(
        app.original.is_some(),
        "the previously loaded image stays on screen"
    );
    assert_eq!(app.path, a_path, "a failed open must not adopt the path");
    let message = app.error().unwrap_or("").to_string();
    assert!(
        message.contains("missing.png"),
        "the failure must name the vanished file loudly, got: {message:?}"
    );
}

/// R3-OPEN-1: a repeat Develop switch (already active) must not re-open —
/// only an actual module transition reacts.
#[test]
fn repeated_develop_switch_does_not_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = new_app();
    app.directory = directory.path().display().to_string();
    app.entries = vec![
        raw_entry(directory.path(), "a.cr3"),
        raw_entry(directory.path(), "b.cr3"),
    ];
    let order = app.filmstrip_order();
    app.path = order[0].clone();
    app.filmstrip_selection = BTreeSet::from([order[0].clone()]);
    app.set_module(Module::Library);
    app.select_filmstrip_path(order[1].clone(), false, false);
    app.set_module(Module::Develop);
    drain_pending_decode(&mut app);

    // Already in Develop with the same lone (different) selection: the
    // transition gate must keep the switch from re-opening.
    app.set_module(Module::Develop);
    assert!(
        app.decode_rx.is_none(),
        "a repeat Develop switch must not restart the decode"
    );
    assert_eq!(app.path, order[0]);
}

/// R3-OPEN-1 guard (a): when the lone selection IS the loaded image, the
/// Develop switch must not reload it (no decode, no path change).
#[test]
fn develop_switch_does_not_reload_when_selection_is_loaded() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = new_app();
    app.directory = directory.path().display().to_string();
    app.entries = vec![
        raw_entry(directory.path(), "a.cr3"),
        raw_entry(directory.path(), "b.cr3"),
    ];
    let order = app.filmstrip_order();
    // Loaded image and lone selection are the same path.
    app.path = order[0].clone();
    app.filmstrip_selection = BTreeSet::from([order[0].clone()]);
    app.set_module(Module::Library);
    let _ = crate::timing::take_timing_log();

    app.set_module(Module::Develop);
    assert!(
        app.decode_rx.is_none(),
        "the already-loaded image must not be reloaded"
    );
    assert!(
        app.pending_load_path.is_none(),
        "no decode may be started for the loaded path"
    );
    assert_eq!(app.path, order[0], "the loaded path is unchanged");
    let log = crate::timing::take_timing_log();
    assert!(
        !log.iter().any(|line| line.contains("decode start")),
        "no decode may start: {log:?}"
    );
}

/// R3-OPEN-1 guard (b): the real grid double-click opens *before* switching, so
/// the Develop switch must reuse that in-flight decode instead of starting a
/// duplicate. Proved through the production entry point and the decode-start
/// timing lines (exactly one), not just the `pending_load_path` flag.
#[test]
fn grid_double_click_reuses_inflight_decode_without_duplicate() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = new_app();
    app.directory = directory.path().display().to_string();
    app.entries = vec![
        raw_entry(directory.path(), "a.cr3"),
        raw_entry(directory.path(), "b.cr3"),
    ];
    let order = app.filmstrip_order();
    app.path = order[0].clone();
    app.filmstrip_selection = BTreeSet::from([order[0].clone()]);
    app.set_module(Module::Library);
    let _ = crate::timing::take_timing_log();

    // Production double-click path: select + open, then switch to Develop.
    app.open_grid_entry_in_develop(order[1].clone());
    let log = crate::timing::take_timing_log();
    let decode_starts = log
        .iter()
        .filter(|line| line.contains("decode start"))
        .count();
    assert_eq!(
        decode_starts, 1,
        "exactly one decode must start (the in-flight one is reused): {log:?}"
    );
    assert_eq!(
        app.pending_load_path.as_deref(),
        Some(order[1].as_str()),
        "the reused decode still targets the double-clicked path"
    );
    assert!(app.decode_rx.is_some(), "the decode stays in flight");
    assert_eq!(app.active_module, Module::Develop);
}
