//! thumbnail/preview pickers and filmstrip caches tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

// ---- REVIEW-GUI-THUMB-1 / THUMB-2 / PATHDESYNC-1 (headless) ----

/// REVIEW-GUI-THUMB-1: identical filenames in two folders must produce
/// distinct thumbnail keys so neither cell can show the other's image.
#[test]
fn thumbnail_keys_distinguish_same_filename_across_folders() {
    let root = tempfile::tempdir().unwrap();
    let dir_a = root.path().join("album-a");
    let dir_b = root.path().join("album-b");
    std::fs::create_dir(&dir_a).unwrap();
    std::fs::create_dir(&dir_b).unwrap();
    let path_a = dir_a.join("IMG_0001.png");
    let path_b = dir_b.join("IMG_0001.png");
    save_png(&path_a);
    save_png(&path_b);

    let entry_a = LuminaApp::scan_entry(&path_a).unwrap();
    let entry_b = LuminaApp::scan_entry(&path_b).unwrap();
    assert_eq!(entry_a.name, entry_b.name, "fixture must share a filename");
    assert_ne!(
        entry_a.thumb_key, entry_b.thumb_key,
        "same filename in two folders must not share a thumbnail key"
    );
    // Keys are stable across scans of the same file.
    assert_eq!(
        entry_a.thumb_key,
        LuminaApp::scan_entry(&path_a).unwrap().thumb_key
    );

    // Manager-level: inserting under key A never satisfies lookups for B.
    let ctx = egui::Context::default();
    let mut manager = crate::filmstrip::ThumbnailManager::new();
    let tex = ctx.load_texture(
        "test",
        egui::ColorImage::from_rgba_unmultiplied([1, 1], &[0, 0, 0, 255]),
        egui::TextureOptions::LINEAR,
    );
    manager.insert(&entry_a.thumb_key, tex);
    assert!(manager.get(&entry_a.thumb_key).is_some());
    assert!(manager.get(&entry_b.thumb_key).is_none());
}

/// REVIEW-GUI-PATHDESYNC-1: `open_file` must not adopt the new path while
/// the asynchronous decode is running; `finish_decode` commits it on
/// success only.
#[test]
fn open_file_commits_path_only_after_successful_decode() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);

    let mut app = new_app();
    assert!(app.path.is_empty());
    app.open_file(source.display().to_string());
    // Decode is in flight: the path is NOT yet committed, so any Save /
    // Export would operate on the previous (still consistent) state.
    assert!(app.decode_rx.is_some(), "decode must run asynchronously");
    assert_eq!(
        app.path, "",
        "path must not be adopted before decode success"
    );

    open_and_decode(&mut app, source.display().to_string());
    assert_eq!(app.path, source.display().to_string());
    assert!(app.error().is_none());
}

/// REVIEW-GUI-PATHDESYNC-1: a failed decode keeps the previously loaded
/// image/path pair intact and reports the failure visibly — no phantom
/// sidecar target, no silent fallback.
#[test]
fn failed_decode_keeps_previous_path_and_reports_error() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("good.png");
    save_png(&source);

    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    let loaded_path = app.path.clone();
    assert_eq!(loaded_path, source.display().to_string());

    let missing = directory.path().join("missing.png");
    app.open_file(missing.display().to_string());
    let mut decoded_or_failed = false;
    for _ in 0..2000 {
        app.poll_decode();
        if app.error().is_some() || app.decode_rx.is_none() {
            decoded_or_failed = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(decoded_or_failed, "failed decode must surface promptly");
    assert!(
        app.error().is_some(),
        "decode failure must be reported visibly"
    );
    // KITTEST-COVERAGE-STATES-2 (a): a background decode failure is a
    // header banner, never a blocking popup dialog (browsing a folder of
    // corrupt files must not stack dialogs). The loud signal is preserved
    // (status + error text + `error!` log).
    assert!(
        !app.error_dialog_open(),
        "a background decode failure must not open the error dialog"
    );
    assert_eq!(
        app.status(),
        Str::Error.t(),
        "the failed decode must flip the status line to Error"
    );
    assert_eq!(
        app.path, loaded_path,
        "a failed decode must not adopt the new path"
    );
}

/// KITTEST-COVERAGE-STATES-2 (a/c): the two error surfaces are distinct —
/// an explicit user-action failure opens the popup dialog, a background
/// failure only the header banner — and closing the dialog logs at `info!`
/// (DoD §4) while the header/`error` text stays until the next success.
#[test]
fn error_dialog_vs_banner_and_close() {
    let mut app = new_app();
    app.show_error("explicit action failed");
    assert!(
        app.error_dialog_open(),
        "an explicit user-action failure must open the dialog"
    );
    assert_eq!(app.status(), Str::Error.t());
    assert_eq!(app.error(), Some("explicit action failed"));

    app.close_error_dialog();
    assert!(
        !app.error_dialog_open(),
        "Close must dismiss the popup dialog"
    );
    assert_eq!(
        app.error(),
        Some("explicit action failed"),
        "the header banner stays until the next success"
    );
    // Closing an already-closed dialog is an idempotent no-op.
    app.close_error_dialog();
    assert!(!app.error_dialog_open());

    app.show_error_banner("background failure");
    assert!(
        !app.error_dialog_open(),
        "a background failure must not open the dialog"
    );
    assert_eq!(app.status(), Str::Error.t());
    assert_eq!(app.error(), Some("background failure"));
}

// ---- F-103-N3: Before/After + white-balance eyedropper ----

#[test]
fn before_after_toggle_does_not_mutate_recipe() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    app.set_adjustment("exposure", 1.5);
    let snapshot = app.recipe().adjustments.clone();
    app.toggle_before_after();
    assert!(app.before_after);
    // The toggle only swaps the displayed frame; the recipe is untouched.
    assert_eq!(app.recipe().adjustments, snapshot);
    assert_eq!(app.recipe().adjustments["exposure"], 1.5);
    app.toggle_before_after();
    assert!(!app.before_after);
    assert_eq!(app.recipe().adjustments["exposure"], 1.5);
}

#[test]
fn white_balance_eyedropper_sets_recipe_fields() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    // A colored picked point sets both WB fields and disarms the picker.
    app.set_white_balance_from_point(1.0, 0.5, 0.25).unwrap();
    assert!(app.recipe().adjustments.contains_key("wb_temperature"));
    assert!(app.recipe().adjustments.contains_key("wb_tint"));
    let temp = app.recipe().adjustments["wb_temperature"];
    assert!((1500.0..=12000.0).contains(&temp));
    assert!(!app.wb_pick_mode);
    // A neutral grey point is the documented default (6500 K, tint 0).
    app.set_white_balance_from_point(0.5, 0.5, 0.5).unwrap();
    assert_eq!(app.recipe().adjustments["wb_temperature"], 6500.0);
    assert_eq!(app.recipe().adjustments["wb_tint"], 0.0);
    // Non-positive channels cannot derive a white balance.
    assert!(app.set_white_balance_from_point(0.0, 0.5, 0.5).is_err());
}

// ---- Filmstrip helpers (headless) ----

#[test]
fn filmstrip_downscale_keeps_small_images_and_shrinks_large() {
    let small = vec![1u8, 2, 3, 255, 4, 5, 6, 255];
    let (out, w, h) = crate::filmstrip::downscale_rgba(&small, 2, 1, 160);
    assert_eq!((w, h), (2, 1));
    assert_eq!(out, small);

    let big = vec![0u8; (4 * 320 * 200) as usize];
    let (out2, w2, h2) = crate::filmstrip::downscale_rgba(&big, 320, 200, 160);
    assert_eq!((w2, h2), (160, 100));
    assert_eq!(out2.len(), 4 * 160 * 100);
}

#[test]
fn filmstrip_cache_miss_then_hit_roundtrip() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let name = source.file_name().unwrap().to_string_lossy().to_string();
    let cache = lumina_core::cache::disk::DiskFolderCache::for_image(&source).unwrap();
    // No preview on disk yet -> miss (no silent fallback to a wrong image).
    assert!(!crate::filmstrip::filmstrip_preview_cached(
        &cache,
        &name,
        "vc-original"
    ));
    let thumbnail = ImageFrame::new(2, 1, vec![10, 20, 30, 255, 200, 180, 160, 255])
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap();
    cache
        .store_preview(
            &name,
            "vc-original",
            lumina_core::cache::PreviewKind::Standard,
            &thumbnail,
        )
        .unwrap();
    // After storing, the same probe is a cache hit.
    assert!(crate::filmstrip::filmstrip_preview_cached(
        &cache,
        &name,
        "vc-original"
    ));
}
