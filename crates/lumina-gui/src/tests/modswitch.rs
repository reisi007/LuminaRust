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
