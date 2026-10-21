//! Tests of the folder-cache disk tier (`disk.rs`).
//!
//! Pure move of the former inline `mod tests` block: the module grew
//! beyond the 500-line ratchet boundary with the SEC-4 fix, and the
//! anti-gaming rule keeps new logic out of files that are already at the
//! limit. No test was dropped or changed by the move.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

fn folder() -> PathBuf {
    std::env::temp_dir().join(format!(
        "lumina-cache-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .wrapping_add(u128::from(NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed)))
    ))
}
fn setup() -> (PathBuf, DiskFolderCache) {
    let path = folder();
    fs::create_dir_all(&path).unwrap();
    let cache = DiskFolderCache::in_folder(&path).unwrap();
    (path, cache)
}

#[test]
fn settings_roundtrip_and_parent_inheritance() {
    let (path, cache) = setup();
    cache
        .save_settings(&FolderCacheSettings {
            standard_preview: false,
            one_to_one_preview: true,
            ..Default::default()
        })
        .unwrap();
    assert!(cache.effective_settings().unwrap().one_to_one_preview);
    let child = path.join("child");
    fs::create_dir(&child).unwrap();
    let child_cache = DiskFolderCache::in_folder(&child).unwrap();
    child_cache
        .save_settings(&FolderCacheSettings {
            standard_preview: true,
            one_to_one_preview: false,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        child_cache.effective_settings().unwrap(),
        FolderCacheSettings::default()
    );
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn settings_control_standard_and_one_to_one() {
    let (path, cache) = setup();
    assert!(cache
        .store_preview("a.raw", "main", PreviewKind::Standard, b"s")
        .unwrap());
    assert!(!cache
        .store_preview("a.raw", "main", PreviewKind::OneToOne, b"1")
        .unwrap());
    cache
        .save_settings(&FolderCacheSettings {
            one_to_one_preview: true,
            ..Default::default()
        })
        .unwrap();
    assert!(cache
        .store_preview("a.raw", "main", PreviewKind::OneToOne, b"1")
        .unwrap());
    assert_eq!(
        cache
            .load_preview("a.raw", "main", PreviewKind::OneToOne)
            .unwrap(),
        Some(b"1".to_vec())
    );
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn partial_child_settings_inherit_the_other_parent_field() {
    let (path, parent) = setup();
    parent
        .save_settings(&FolderCacheSettings {
            standard_preview: false,
            one_to_one_preview: true,
            ..Default::default()
        })
        .unwrap();
    let child_path = path.join("child");
    let child = DiskFolderCache::in_folder(&child_path).unwrap();
    fs::write(child.settings_path(), br#"{"standard_preview":true}"#).unwrap();

    assert_eq!(
        child.effective_settings().unwrap(),
        FolderCacheSettings {
            standard_preview: true,
            one_to_one_preview: true,
            ..Default::default()
        }
    );
    fs::remove_dir_all(path).unwrap();
}

/// LRPAR-G01-BASIC: `reset_sliders_automatically` defaults to false,
/// roundtrips, and inherits field-wise (a partial child file keeps the
/// parent value instead of resetting it).
#[test]
fn reset_sliders_automatically_inherits_field_wise() {
    let (path, parent) = setup();
    assert!(
        !parent
            .effective_settings()
            .unwrap()
            .reset_sliders_automatically
    );
    parent
        .save_settings(&FolderCacheSettings {
            reset_sliders_automatically: true,
            ..Default::default()
        })
        .unwrap();
    assert!(
        parent
            .effective_settings()
            .unwrap()
            .reset_sliders_automatically
    );
    let child_path = path.join("child");
    let child = DiskFolderCache::in_folder(&child_path).unwrap();
    fs::write(child.settings_path(), br#"{"standard_preview":true}"#).unwrap();
    let effective = child.effective_settings().unwrap();
    assert!(effective.reset_sliders_automatically);
    assert!(effective.standard_preview);
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn identified_preview_rejects_legacy_and_wrong_source_content() {
    let (path, cache) = setup();
    cache
        .store_preview_with_source_hash(
            "a.raw",
            "main",
            PreviewKind::Standard,
            "blake3:source-a",
            b"a",
        )
        .unwrap();
    assert!(cache
        .load_preview_with_source_hash("a.raw", "main", PreviewKind::Standard, "blake3:source-b",)
        .unwrap()
        .is_none());
    assert_eq!(
        cache
            .load_preview_with_source_hash(
                "a.raw",
                "main",
                PreviewKind::Standard,
                "blake3:source-a",
            )
            .unwrap(),
        Some(b"a".to_vec())
    );
    cache
        .store_preview("a.raw", "main", PreviewKind::Standard, b"legacy")
        .unwrap();
    assert!(cache
        .load_preview_with_source_hash("a.raw", "main", PreviewKind::Standard, "blake3:source-a",)
        .unwrap()
        .is_none());
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn prune_removes_orphans_and_cache_is_non_authoritative() {
    let (path, cache) = setup();
    cache
        .store_preview("gone.raw", "main", PreviewKind::Standard, b"x")
        .unwrap();
    assert_eq!(cache.prune_orphans(&[]).unwrap(), 1);
    assert!(cache
        .load_preview("gone.raw", "main", PreviewKind::Standard)
        .unwrap()
        .is_none());
    fs::remove_dir_all(path).unwrap();
}

/// Q4 of LUMINA-CACHE-EXTRACT-70: the public layout must be the one the
/// writer actually uses — the associated `entry_paths` names exactly the
/// file pair `store_preview` commits, without a handle being required to
/// ask for it.
#[test]
fn entry_paths_name_the_files_the_writer_commits() {
    let (path, cache) = setup();
    assert!(cache
        .store_preview("a.raw", "vc-original", PreviewKind::Standard, b"s")
        .unwrap());
    let root = cache.path();
    let (meta, data) =
        DiskFolderCache::entry_paths(root, "a.raw", "vc-original", PreviewKind::Standard);
    assert!(meta.is_file(), "metadata entry exists: {}", meta.display());
    assert!(data.is_file(), "data entry exists: {}", data.display());
    assert_eq!(meta.file_stem(), data.file_stem());
    assert_eq!(
        data.file_stem().and_then(|stem| stem.to_str()),
        Some(entry_stem("a.raw", "vc-original", PreviewKind::Standard).as_str())
    );
    assert_eq!(meta.extension().and_then(|ext| ext.to_str()), Some("json"));
    assert_eq!(data.extension().and_then(|ext| ext.to_str()), Some("bin"));
    fs::remove_dir_all(path).unwrap();
}

/// SEC-4: a `.lumina/settings.json` above the hard limit must not be loaded
/// into memory — the **read itself** is bounded — and the unusable file must
/// fail loudly instead of silently continuing with default values.
///
/// Pinned here:
/// (a) a *valid* JSON file above the limit is refused with the named error
///     case (today it would be parsed completely and its flags used);
/// (b) a file of exactly `MAX_SETTINGS_BYTES` bytes is still read — no
///     off-by-one in the bound;
/// (c) an oversized file in an **ancestor** aborts the whole chain loudly:
///     an unusable level is never replaced by guessed defaults, so the
///     inheritance stays predictable instead of silently changing meaning.
#[test]
fn oversized_settings_file_is_rejected_without_reading_it() {
    let (path, cache) = setup();
    // (a) above the limit: valid JSON, so only the size makes it unusable.
    // The file is a whole MiB above the limit, and the reported `size` is the
    // number of bytes the reader pulled — that is the proof that the read
    // itself was bounded and not the file that was small.
    fs::write(
        cache.settings_path(),
        settings_payload(MAX_SETTINGS_BYTES + 1024 * 1024),
    )
    .unwrap();
    match cache.effective_settings().unwrap_err() {
        DiskCacheError::SettingsTooLarge {
            path: file,
            limit,
            size,
        } => {
            assert_eq!(file, cache.settings_path());
            assert_eq!(limit, MAX_SETTINGS_BYTES);
            assert_eq!(
                size,
                MAX_SETTINGS_BYTES + 1,
                "the bounded read stops one byte above the limit, whatever the file size is"
            );
        }
        other => panic!("an oversized settings file must be refused: {other:?}"),
    }
    // (b) exactly at the limit: still read, the flag is applied.
    let at_limit = settings_payload(MAX_SETTINGS_BYTES);
    assert_eq!(at_limit.len() as u64, MAX_SETTINGS_BYTES);
    fs::write(cache.settings_path(), &at_limit).unwrap();
    assert!(!cache.effective_settings().unwrap().standard_preview);
    fs::remove_dir_all(path).unwrap();

    // (c) an oversized file in an ancestor aborts the chain loudly.
    let (parent_path, parent) = setup();
    let child_path = parent_path.join("child");
    fs::create_dir_all(&child_path).unwrap();
    fs::write(
        parent.settings_path(),
        settings_payload(MAX_SETTINGS_BYTES + 1),
    )
    .unwrap();
    let child = DiskFolderCache::in_folder(&child_path).unwrap();
    assert!(
        matches!(
            child.effective_settings(),
            Err(DiskCacheError::SettingsTooLarge { .. })
        ),
        "an unusable ancestor level must not be silently replaced by defaults"
    );
    fs::remove_dir_all(parent_path).unwrap();
}

/// A valid settings JSON of exactly `bytes` length: the `standard_preview`
/// flag plus a padding field, so the file parses and only its size differs
/// from the legitimate payload.
fn settings_payload(bytes: u64) -> Vec<u8> {
    let prefix = br#"{"standard_preview":false,"pad":""#;
    let suffix = br#""}"#;
    let padding = bytes as usize - prefix.len() - suffix.len();
    let mut payload = prefix.to_vec();
    payload.extend(std::iter::repeat_n(b'x', padding));
    payload.extend_from_slice(suffix);
    assert_eq!(payload.len() as u64, bytes);
    payload
}
