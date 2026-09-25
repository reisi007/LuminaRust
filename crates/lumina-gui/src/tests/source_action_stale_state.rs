//! GUI-SRCACC-1 stale-state regressions for persisted sidecar/artifact changes.

use super::*;
use lumina_core::ImageFileFormat;
use lumina_sidecar::{load_sidecar, save_sidecar, sidecar_path_for};
use std::path::Path;
use std::time::SystemTime;

fn restore_length_and_mtime(path: &Path, length: u64, modified: SystemTime) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_len(length).unwrap();
    file.set_modified(modified).unwrap();
}

fn revise_fixture(fixture: &ActionFixture, replacement: [u8; 8]) {
    let replacement = replace_fixture_artifact(fixture, replacement);
    let sidecar_path = sidecar_path_for(&fixture.source);
    let mut sidecar = load_sidecar(&sidecar_path).unwrap();
    sidecar.virtual_copies[0].recipe.source_actions[0]
        .artifact
        .checksum = replacement.checksum();
    save_sidecar(&sidecar_path, &sidecar).unwrap();
}

#[test]
fn neighbor_inflight_result_is_rejected_after_artifact_change() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let probe_id = fixture.source.display().to_string();
    let job = || crate::preview_ctrl::PreviewJob {
        probe_id: probe_id.clone(),
        source: fixture.source.clone(),
        name: "source-action.png".into(),
        virtual_copy: "vc-original".into(),
        target: (2, 1),
        kind: lumina_core::preview_cache::PreviewKind::Screen,
        priority: 0,
        denoise_policy: lumina_core::DenoisePolicy::Warn,
    };

    // Deterministically simulate a worker that prepared the valid red action,
    // then changed the sidecar and zdata before its result reached the UI.
    let stale = crate::preview_jobs::worker_preview(job()).unwrap();
    let stale_digest = stale.digest.clone();
    let crate::preview_ctrl::PreviewOutcome::Ready(stale_frame) = &stale.outcome else {
        panic!("initial neighbor action must render");
    };
    assert_eq!(first_pixel(stale_frame), [201, 0, 0, 255]);
    let (mut controller, result_tx) = crate::preview_ctrl::PreviewController::manual();
    assert!(controller.enqueue(job()), "old identity enters flight");

    revise_fixture(&fixture, [7, 8, 9, 255, 3, 4, 5, 255]);
    result_tx.send(stale).unwrap();
    controller.poll();
    assert!(
        !controller.lru().contains(&stale_digest),
        "raced output must never enter the RAM cache"
    );
    assert!(
        controller
            .neighbor_preview(&probe_id, &fixture.source)
            .unwrap()
            .is_none(),
        "raced output must never reach the paint path"
    );
    assert_eq!(
        controller.probe_state(&probe_id),
        crate::preview_ctrl::PreviewProbeState::Miss
    );
    assert!(controller.needs_job(&probe_id));
    assert!(
        controller.enqueue(job()),
        "the changed sidecar/artifact identity must requeue"
    );
}

#[test]
fn thumbnail_identity_change_resets_inflight_and_failure_state() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let key = fixture.source.display().to_string();
    let mut manager = crate::filmstrip::ThumbnailManager::new();
    let original = manager.refresh_source(&key, &fixture.source);

    manager.begin_job(&key);
    revise_fixture(&fixture, [7, 8, 9, 255, 3, 4, 5, 255]);
    let changed = manager.refresh_source(&key, &fixture.source);
    assert_ne!(original, changed);
    assert!(
        manager.needs_job(&key),
        "the old in-flight marker must clear"
    );

    manager.begin_job(&key);
    manager.mark_failed(&key, "old artifact failed");
    assert!(manager.failure(&key).is_some());
    revise_fixture(&fixture, [17, 18, 19, 255, 23, 24, 25, 255]);
    manager.refresh_source(&key, &fixture.source);
    assert!(manager.failure(&key).is_none(), "old failure must clear");
    assert!(
        manager.needs_job(&key),
        "the new identity gets a fresh budget"
    );
}

#[test]
fn thumbnail_artifact_change_drops_ram_pixels_and_requeues_worker() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let mut app = new_app();
    open_and_decode(&mut app, fixture.source.display().to_string());
    let entry = crate::library_scan::scan_entry(&fixture.source).unwrap();
    let sidecar_path = sidecar_path_for(&fixture.source);
    let zdata_path = lumina_sidecar::zdata_path_for(&fixture.source);
    let sidecar_meta = std::fs::metadata(&sidecar_path).unwrap();
    let zdata_meta = std::fs::metadata(&zdata_path).unwrap();
    let sidecar_len = sidecar_meta.len();
    let zdata_len = zdata_meta.len();
    let sidecar_mtime = sidecar_meta.modified().unwrap();
    let zdata_mtime = zdata_meta.modified().unwrap();
    let original_identity = crate::filmstrip::ThumbnailManager::source_identity(&fixture.source);
    let ctx = egui::Context::default();

    assert!(app.ensure_thumbnail(&ctx, &entry));
    for _ in 0..2000 {
        app.poll_thumbnails(&ctx);
        if app.thumbnails.get(&entry.thumb_key).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(app.thumbnails.get(&entry.thumb_key).is_some());
    assert!(app.thumbnails.failure(&entry.thumb_key).is_none());
    assert!(
        !app.ensure_thumbnail(&ctx, &entry),
        "an unchanged artifact identity must preserve the RAM hit"
    );

    // Find a different valid artifact whose encoded zdata has exactly the
    // original byte length. Both files then get their original mtime restored,
    // so mtime+length identity would wrongly retain the old texture.
    let replacement = (0_u8..=255)
        .filter(|value| *value != 201)
        .find_map(|value| {
            let changed = replace_fixture_artifact(&fixture, [value, 8, 9, 255, 3, 4, 5, 255]);
            (std::fs::metadata(&zdata_path).unwrap().len() == zdata_len).then_some(changed)
        })
        .expect("a changed source action with an identical zdata length");
    let mut sidecar = load_sidecar(&sidecar_path).unwrap();
    sidecar.virtual_copies[0].recipe.source_actions[0]
        .artifact
        .checksum = replacement.checksum();
    save_sidecar(&sidecar_path, &sidecar).unwrap();
    restore_length_and_mtime(&zdata_path, zdata_len, zdata_mtime);
    restore_length_and_mtime(&sidecar_path, sidecar_len, sidecar_mtime);
    let changed_sidecar_meta = std::fs::metadata(&sidecar_path).unwrap();
    let changed_zdata_meta = std::fs::metadata(&zdata_path).unwrap();
    assert_eq!(changed_sidecar_meta.len(), sidecar_len);
    assert_eq!(changed_zdata_meta.len(), zdata_len);
    assert_eq!(changed_sidecar_meta.modified().unwrap(), sidecar_mtime);
    assert_eq!(changed_zdata_meta.modified().unwrap(), zdata_mtime);
    assert_ne!(
        original_identity,
        crate::filmstrip::ThumbnailManager::source_identity(&fixture.source),
        "content identity must change even with identical mtime/length"
    );

    assert!(
        app.thumbnail_for_entry(&entry).is_none(),
        "the draw lookup must invalidate stale pixels before repaint"
    );
    assert!(
        app.ensure_thumbnail(&ctx, &entry),
        "the changed artifact must enqueue a fresh resolver job"
    );
    assert!(
        app.thumbnails.get(&entry.thumb_key).is_none(),
        "stale in-memory pixels must be removed before repaint"
    );
    for _ in 0..2000 {
        app.poll_thumbnails(&ctx);
        if app.thumbnails.get(&entry.thumb_key).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(app.thumbnails.get(&entry.thumb_key).is_some());
    assert!(app.thumbnails.failure(&entry.thumb_key).is_none());
}

#[test]
fn neighbor_stand_in_rejects_same_dimension_source_replacement() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let job = || crate::preview_ctrl::PreviewJob {
        probe_id: fixture.source.display().to_string(),
        source: fixture.source.clone(),
        name: "source-action.png".into(),
        virtual_copy: "vc-original".into(),
        target: (2, 1),
        kind: lumina_core::preview_cache::PreviewKind::Screen,
        priority: 0,
        denoise_policy: lumina_core::DenoisePolicy::Warn,
    };

    let initial = crate::preview_jobs::worker_preview(job()).expect("initial action");
    let crate::preview_ctrl::PreviewOutcome::Ready(initial_frame) = initial.outcome else {
        panic!("initial neighbor must render");
    };
    assert_eq!(first_pixel(&initial_frame), [201, 0, 0, 255]);

    // Keep the decoded geometry identical: only the exact source bytes change.
    // A dimension-only cache key would therefore be vulnerable to stale pixels.
    let replacement = ImageFrame::new(2, 1, vec![1, 2, 3, 255, 4, 5, 6, 255]).unwrap();
    let replacement_bytes = replacement.encode(ImageFileFormat::Png).unwrap();
    assert_eq!(replacement.width, initial_frame.width);
    assert_eq!(replacement.height, initial_frame.height);
    std::fs::write(&fixture.source, replacement_bytes).unwrap();

    let error = match crate::preview_jobs::worker_preview(job()) {
        Ok(_) => panic!("replacement source must not reuse the old neighbor pixels"),
        Err(error) => error,
    };
    assert!(error.contains("source identity conflict"), "{error}");
}

#[test]
fn thumbnail_stand_in_rejects_same_dimension_source_replacement() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let name = fixture
        .source
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let cache = lumina_core::cache::disk::DiskFolderCache::for_image(&fixture.source).unwrap();
    let stale = ImageFrame::new(2, 1, vec![31, 32, 33, 255, 34, 35, 36, 255])
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap();
    cache
        .store_preview(
            &name,
            crate::thumb_cache::THUMB_VIRTUAL_COPY,
            lumina_core::cache::PreviewKind::Standard,
            &stale,
        )
        .unwrap();
    let source_identity = crate::filmstrip::ThumbnailManager::source_identity(&fixture.source);
    let replacement = ImageFrame::new(2, 1, vec![41, 42, 43, 255, 44, 45, 46, 255])
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap();
    std::fs::write(&fixture.source, replacement).unwrap();

    let job = crate::thumb_worker::ThumbnailJob {
        source: fixture.source.clone(),
        name,
        key: fixture.source.display().to_string(),
        source_identity,
        cache: Some(cache),
        cached: true,
        enqueued_at: std::time::Instant::now(),
    };
    let error = match crate::thumb_worker::decode_thumbnail_frame(&job) {
        Ok(_) => panic!("thumbnail must not consume pixels for a replaced source"),
        Err(error) => error,
    };
    assert!(error.contains("source identity conflict"), "{error}");
}

#[test]
fn thumbnail_rebuild_marker_survives_preexisting_cache_failure_until_fresh_success() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let name = fixture
        .source
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let cache = lumina_core::cache::disk::DiskFolderCache::for_image(&fixture.source).unwrap();
    let stale = ImageFrame::new(2, 1, vec![7, 8, 9, 255, 10, 11, 12, 255])
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap();
    cache
        .store_preview(
            &name,
            crate::thumb_cache::THUMB_VIRTUAL_COPY,
            lumina_core::cache::PreviewKind::Standard,
            &stale,
        )
        .unwrap();

    let mut app = new_app();
    let entry = crate::library_scan::scan_entry(&fixture.source).unwrap();
    let key = entry.thumb_key.clone();
    let ctx = egui::Context::default();
    // Capture the old source/sidecar identity as a warm manager would have it.
    app.thumbnails.refresh_source(&key, &fixture.source);

    // Replace the source at the same path and update only the sidecar's source
    // fingerprint. The recipe still points at the source-action bundle.
    let replacement = ImageFrame::new(2, 1, vec![20, 30, 40, 255, 50, 60, 70, 255])
        .unwrap()
        .encode(ImageFileFormat::Png)
        .unwrap();
    std::fs::write(&fixture.source, &replacement).unwrap();
    let mut document = load_sidecar(&sidecar_path_for(&fixture.source)).unwrap();
    let live = crate::source_actions::source_fingerprint(&replacement);
    document.source.content_hash = live.content_hash;
    document.source.byte_length = live.byte_length;
    save_sidecar(&sidecar_path_for(&fixture.source), &document).unwrap();

    // Capture the failed-resolver identity before enqueueing. This keeps the
    // worker's failure current, so the result is visibly recorded rather than
    // discarded as a raced input change.
    let zdata = lumina_sidecar::zdata_path_for(&fixture.source);
    let bundle = std::fs::read(&zdata).unwrap();
    std::fs::remove_file(&zdata).unwrap();
    app.thumbnails.refresh_source(&key, &fixture.source);

    assert!(app.ensure_thumbnail(&ctx, &entry));
    for _ in 0..2000 {
        app.poll_thumbnails(&ctx);
        if app.thumbnails.failure(&key).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(app.thumbnails.failure(&key).is_some());
    assert!(app.thumbnails.get(&key).is_none());
    assert!(
        app.thumbnails.source_rebuild_pending(&key),
        "a failed fresh render must keep legacy disk pixels untrusted"
    );

    // Restore the bundle and retry. The retry is still resolver-backed; only its
    // success is allowed to clear the marker and admit the newly rendered frame.
    std::fs::write(&zdata, bundle).unwrap();
    assert!(app.ensure_thumbnail(&ctx, &entry));
    for _ in 0..2000 {
        app.poll_thumbnails(&ctx);
        if app.thumbnails.get(&key).is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(app.thumbnails.get(&key).is_some());
    assert!(!app.thumbnails.source_rebuild_pending(&key));
    assert!(app.thumbnails.failure(&key).is_none());
}

#[test]
fn file_decode_keeps_previous_source_when_sidecar_identity_conflicts() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let old_path = fixture.source.display().to_string();
    let old_bytes = std::fs::read(&fixture.source).unwrap();
    let mut app = new_app();
    open_and_decode(&mut app, old_path.clone());
    let old_frame = app.original.clone().unwrap();
    let replacement = ImageFrame::new(2, 1, vec![1, 2, 3, 255, 4, 5, 6, 255]).unwrap();
    std::fs::write(
        &fixture.source,
        replacement.encode(ImageFileFormat::Png).unwrap(),
    )
    .unwrap();

    app.open_file(old_path.clone());
    for _ in 0..2000 {
        app.poll_decode();
        if app.error().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    assert!(app.error().unwrap().contains("source identity conflict"));
    assert_eq!(app.path, old_path, "the active path must be preserved");
    assert_eq!(app.source_bytes.as_deref(), Some(old_bytes.as_slice()));
    assert_eq!(app.original.as_ref().unwrap().pixels, old_frame.pixels);
    assert!(
        app.preview().is_none(),
        "stale render pixels must be cleared"
    );
    assert!(app.render_key().is_none());
}

#[test]
fn thumbnail_source_replacement_invalidates_cached_and_pending_state() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let key = fixture.source.display().to_string();
    let original = std::fs::read(&fixture.source).unwrap();
    let metadata = std::fs::metadata(&fixture.source).unwrap();
    let mtime = metadata.modified().unwrap();
    let before = crate::filmstrip::ThumbnailManager::source_identity(&fixture.source);
    let mut cached = crate::filmstrip::ThumbnailManager::new();
    cached.refresh_source(&key, &fixture.source);
    let ctx = egui::Context::default();
    cached.insert(&key, cached_texture(&ctx));
    let mut pending = crate::filmstrip::ThumbnailManager::new();
    pending.refresh_source(&key, &fixture.source);
    pending.begin_job(&key);
    let mut failed = crate::filmstrip::ThumbnailManager::new();
    failed.refresh_source(&key, &fixture.source);
    failed.mark_failed(&key, "old source failed");

    let mut replacement = original.clone();
    replacement[0] ^= 0x01;
    std::fs::write(&fixture.source, &replacement).unwrap();
    restore_length_and_mtime(&fixture.source, metadata.len(), mtime);

    let after = cached.refresh_source(&key, &fixture.source);
    assert_ne!(before, after);
    assert!(cached.get(&key).is_none());
    assert!(pending.refresh_source(&key, &fixture.source) != before);
    assert!(pending.needs_job(&key));
    assert!(failed.refresh_source(&key, &fixture.source) != before);
    assert!(failed.failure(&key).is_none());
    assert!(failed.needs_job(&key));
}

fn cached_texture(ctx: &egui::Context) -> egui::TextureHandle {
    ctx.load_texture(
        "stale-thumbnail",
        egui::ColorImage::from_rgba_unmultiplied([1, 1], &[1, 2, 3, 255]),
        egui::TextureOptions::LINEAR,
    )
}

#[test]
fn navigator_zoom_key_changes_for_same_dimension_source_replacement() {
    let mut app = new_app();
    let first = ImageFrame::new(2, 1, vec![10, 20, 30, 255, 40, 50, 60, 255]).unwrap();
    let first_bytes = first.encode(ImageFileFormat::Png).unwrap();
    app.load_bytes(first_bytes, "same-size.png").unwrap();
    let first_overview = app.navigator_zoomed_overview().unwrap();
    let first_key = app.navigator_overview_key.clone();

    let replacement = ImageFrame::new(2, 1, vec![90, 80, 70, 255, 60, 50, 40, 255]).unwrap();
    let replacement_bytes = replacement.encode(ImageFileFormat::Png).unwrap();
    app.original = Some(replacement.clone());
    app.source_bytes = Some(replacement_bytes);
    app.source_hash_memo = None;
    let replacement_overview = app.navigator_zoomed_overview().unwrap();

    assert_ne!(first_key, app.navigator_overview_key);
    assert_ne!(first_overview.pixels, replacement_overview.pixels);
}

#[test]
fn snapshot_selects_requested_copy_and_missing_copy_is_loud() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let path = sidecar_path_for(&fixture.source);
    let mut document = load_sidecar(&path).unwrap();
    document
        .duplicate_virtual_copy("vc-original", "vc-second", "Second")
        .unwrap();
    document.virtual_copies[1]
        .recipe
        .adjustments
        .insert("exposure".into(), 1.0);
    save_sidecar(&path, &document).unwrap();

    let snapshot =
        crate::source_actions::read_sidecar_recipe_snapshot(&fixture.source, "vc-second").unwrap();
    assert_eq!(snapshot.recipe.adjustments.get("exposure"), Some(&1.0));
    let error = crate::source_actions::read_sidecar_recipe_snapshot(&fixture.source, "vc-missing")
        .unwrap_err();
    assert!(error.contains("no virtual copy `vc-missing`"), "{error}");
}

#[test]
fn malformed_sidecar_identity_uses_exact_bytes_and_stable_classification() {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let path = sidecar_path_for(&fixture.source);
    std::fs::write(&path, b"{broken").unwrap();
    let first = crate::filmstrip::ThumbnailManager::source_identity(&fixture.source);
    std::fs::write(&path, b"{broken").unwrap();
    let same = crate::filmstrip::ThumbnailManager::source_identity(&fixture.source);
    std::fs::write(&path, b"{different").unwrap();
    let changed = crate::filmstrip::ThumbnailManager::source_identity(&fixture.source);
    assert_eq!(first, same);
    assert_ne!(first, changed);
}
