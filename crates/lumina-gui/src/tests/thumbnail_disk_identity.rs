//! Persistent thumbnail records are bound to exact source bytes, and a
//! failed fresh render never falls back to an identified record for old bytes.

use super::*;
use lumina_core::cache::disk::DiskFolderCache;
use lumina_core::cache::PreviewKind;
use lumina_sidecar::{SidecarDocument, SourceFingerprint};

fn stale_identified_fixture() -> (
    ActionFixture,
    SidecarDocument,
    DiskFolderCache,
    String,
    ImageFrame,
) {
    let fixture = action_fixture(ActionFixtureMode::Valid);
    let sidecar_path = sidecar_path_for(&fixture.source);
    let mut document = load_sidecar(&sidecar_path).unwrap();
    document.virtual_copies[0].recipe.source_actions.clear();
    let name = fixture
        .source
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let cache = DiskFolderCache::for_image(&fixture.source).unwrap();
    let old_hash =
        crate::source_actions::source_fingerprint(&std::fs::read(&fixture.source).unwrap())
            .content_hash;
    let stale = ImageFrame::new(2, 1, vec![31, 32, 33, 255, 34, 35, 36, 255]).unwrap();
    cache
        .store_preview_with_source_hash(
            &name,
            crate::thumb_cache::THUMB_VIRTUAL_COPY,
            PreviewKind::Standard,
            &old_hash,
            &stale.encode(ImageFileFormat::Png).unwrap(),
        )
        .unwrap();
    (fixture, document, cache, name, stale)
}

fn replace_source(
    fixture: &ActionFixture,
    document: &mut SidecarDocument,
    bytes: &[u8],
) -> SourceFingerprint {
    std::fs::write(&fixture.source, bytes).unwrap();
    let live = crate::source_actions::source_fingerprint(bytes);
    document.source.content_hash = live.content_hash.clone();
    document.source.byte_length = live.byte_length;
    save_sidecar(&sidecar_path_for(&fixture.source), document).unwrap();
    live
}

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
fn persistent_thumbnail_cache_is_bound_to_exact_source_content() {
    let (fixture, mut document, cache, name, stale) = stale_identified_fixture();
    let replacement = ImageFrame::new(2, 1, vec![41, 42, 43, 255, 44, 45, 46, 255]).unwrap();
    let replacement_bytes = replacement.encode(ImageFileFormat::Png).unwrap();
    let live = replace_source(&fixture, &mut document, &replacement_bytes);

    let source_identity = crate::filmstrip::ThumbnailManager::source_identity(&fixture.source);
    let fresh_job = crate::thumb_worker::ThumbnailJob {
        source: fixture.source.clone(),
        name: name.clone(),
        key: fixture.source.display().to_string(),
        source_identity: source_identity.clone(),
        cache: None,
        cached: false,
        enqueued_at: std::time::Instant::now(),
    };
    let fresh = crate::thumb_worker::decode_thumbnail_frame(&fresh_job).unwrap();
    let cached_job = crate::thumb_worker::ThumbnailJob {
        source: fixture.source.clone(),
        name,
        key: fixture.source.display().to_string(),
        source_identity,
        cache: Some(cache.clone()),
        cached: true,
        enqueued_at: std::time::Instant::now(),
    };
    let cached = crate::thumb_worker::decode_thumbnail_frame(&cached_job).unwrap();
    assert_eq!(
        cached.pixels, fresh.pixels,
        "mismatched disk pixels must miss"
    );
    assert_ne!(cached.pixels, stale.pixels);
    assert!(cache
        .load_preview_with_source_hash(
            fixture.source.file_name().unwrap().to_str().unwrap(),
            crate::thumb_cache::THUMB_VIRTUAL_COPY,
            PreviewKind::Standard,
            &live.content_hash,
        )
        .unwrap()
        .is_some());
}

#[test]
fn failed_fresh_render_never_falls_back_to_identified_stale_pixels() {
    let (fixture, mut document, _cache, _name, _stale) = stale_identified_fixture();
    let invalid = b"not-a-decodable-image";
    let _live = replace_source(&fixture, &mut document, invalid);
    let entry = crate::library_scan::scan_entry(&fixture.source).unwrap();
    let key = entry.thumb_key.clone();
    let ctx = egui::Context::default();
    let mut app = new_app();

    assert!(app.ensure_thumbnail(&ctx, &entry));
    for attempt in 0..2 {
        if attempt > 0 {
            assert!(app.ensure_thumbnail(&ctx, &entry));
        }
        drain_thumbnails(&mut app, &ctx, &key);
        assert!(app.thumbnails.failure(&key).is_some());
        assert!(
            app.thumbnails.get(&key).is_none(),
            "fresh decode failure must not admit old source pixels"
        );
        assert!(
            !app.thumbnails.source_rebuild_pending(&key),
            "recipe-free hash mismatch, not an action barrier, must reject the record"
        );
    }
}
