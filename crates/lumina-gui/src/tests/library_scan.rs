//! folder scanning, aggregation and cache exclusion tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// GUI-LIBRARY-SUBFOLDERS-1: `list_directory` aggregates the chosen
/// folder *including* subfolders; every entry carries its relative
/// subfolder as path badge (`""` for top-level files).
#[test]
fn library_list_directory_aggregates_subfolders_with_path_badges() {
    let root = tempfile::tempdir().unwrap();
    let sub = root.path().join("sub");
    let nested = sub.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    save_raw(&root.path().join("top.arw"));
    save_raw(&sub.join("mid.arw"));
    save_raw(&nested.join("deep.arw"));

    let mut app = new_app();
    app.set_directory(root.path().display().to_string());
    // Tree-click navigation stays flat (pre-existing behavior).
    assert_eq!(
        app.entries().len(),
        1,
        "set_directory must keep listing a single folder flat"
    );
    // Recursive aggregation shows all three with correct badges.
    app.list_directory();
    let mut badges: Vec<(String, String)> = app
        .entries()
        .iter()
        .map(|entry| (entry.name.clone(), entry.folder.clone()))
        .collect();
    badges.sort();
    let nested_badge = Path::new("sub").join("nested").display().to_string();
    assert_eq!(
        badges,
        vec![
            ("deep.arw".to_string(), nested_badge),
            ("mid.arw".to_string(), "sub".to_string()),
            ("top.arw".to_string(), String::new()),
        ]
    );
}

/// GUI-LIBRARY-SUBFOLDERS-1-SORT: `apply_listing` sorts the aggregated
/// entries globally by name — no folder grouping. Fixture names
/// interleave across folder boundaries (`sub/a.arw`,
/// `sub/nested/m.arw`, `top/z.arw`), so only a global name sort yields
/// `a, m, z` in `entries()` order.
#[test]
fn library_list_directory_sorts_aggregated_entries_globally_by_name() {
    let root = tempfile::tempdir().unwrap();
    let sub = root.path().join("sub");
    let nested = sub.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    save_raw(&sub.join("a.arw"));
    save_raw(&nested.join("m.arw"));
    save_raw(&root.path().join("z.arw"));

    let mut app = new_app();
    app.set_directory(root.path().display().to_string());
    app.list_directory();
    let names: Vec<&str> = app
        .entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, vec!["a.arw", "m.arw", "z.arw"]);
}

/// GUI-LIBRARY-SUBFOLDERS-1: a tree click keeps navigating per folder —
/// the clicked folder lists flat with empty badges.
#[test]
fn folder_tree_click_lists_single_folder_flat() {
    let root = tempfile::tempdir().unwrap();
    let sub = root.path().join("sub");
    let nested = sub.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    save_raw(&root.path().join("top.arw"));
    save_raw(&sub.join("mid.arw"));
    save_raw(&nested.join("deep.arw"));

    let mut app = new_app();
    app.set_directory(sub.display().to_string());
    let names: Vec<&str> = app
        .entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, vec!["mid.arw"]);
    assert!(
        app.entries().iter().all(|entry| entry.folder.is_empty()),
        "flat listings carry no path badge"
    );
    app.set_directory(nested.display().to_string());
    let names: Vec<&str> = app
        .entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, vec!["deep.arw"]);
    app.set_directory(root.path().display().to_string());
    let names: Vec<&str> = app
        .entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, vec!["top.arw"]);
}

/// GUI-LIBRARY-SUBFOLDERS-1: the recursive scan terminates on a symlink
/// cycle and never lists an entry twice.
#[cfg(unix)]
#[test]
fn library_list_directory_terminates_on_symlink_loop() {
    let root = tempfile::tempdir().unwrap();
    let sub = root.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    save_raw(&root.path().join("top.arw"));
    save_raw(&sub.join("mid.arw"));
    std::os::unix::fs::symlink(root.path(), sub.join("loop")).unwrap();

    let mut app = new_app();
    app.set_directory(root.path().display().to_string());
    app.list_directory();
    let mut names: Vec<&str> = app
        .entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    names.sort();
    assert_eq!(names, vec!["mid.arw", "top.arw"]);
}

/// GUI-LIBRARY-SUBFOLDERS-1: files deeper than `FOLDER_SCAN_DEPTH`
/// directory levels stay out of the aggregation.
#[test]
fn library_list_directory_respects_folder_scan_depth() {
    let root = tempfile::tempdir().unwrap();
    let l1 = root.path().join("l1");
    let l2 = l1.join("l2");
    let l3 = l2.join("l3");
    let l4 = l3.join("l4");
    std::fs::create_dir_all(&l4).unwrap();
    save_raw(&root.path().join("top.arw"));
    save_raw(&l1.join("one.arw"));
    save_raw(&l2.join("two.arw"));
    save_raw(&l3.join("three.arw"));
    save_raw(&l4.join("four.arw"));

    let mut app = new_app();
    app.set_directory(root.path().display().to_string());
    app.list_directory();
    let mut names: Vec<&str> = app
        .entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    names.sort();
    // Depth 3 scans root, l1, l2 — l3/l4 stay out (mirrors
    // `count_raw_files` with `FOLDER_SCAN_DEPTH`).
    assert_eq!(names, vec!["one.arw", "top.arw", "two.arw"]);
    assert_eq!(FOLDER_SCAN_DEPTH, 3);
}

/// GUI-LIBRARY-LUMINA-DIR-1: `.lumina/` cache directories stay out of
/// the Library scan — flat and recursive, on every level. Cache
/// artifacts (`.lumina/previews/*.preview.webp`, a `.lumina/index`
/// dummy, a nested `sub/.lumina/x.webp`) never list; real images next
/// to them keep listing. Sentinel bytes suffice — the scan never
/// decodes, it only matches supported extensions (WebP included).
#[test]
fn library_scan_excludes_lumina_cache_dirs_flat_and_recursive() {
    let root = tempfile::tempdir().unwrap();
    save_raw(&root.path().join("top.arw"));
    let sub = root.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    save_raw(&sub.join("mid.arw"));
    let previews = root.path().join(".lumina").join("previews");
    std::fs::create_dir_all(&previews).unwrap();
    std::fs::write(previews.join("top.preview.webp"), b"lumina-preview-fixture").unwrap();
    std::fs::create_dir_all(root.path().join(".lumina").join("index")).unwrap();
    std::fs::write(
        root.path().join(".lumina").join("index").join("index.db"),
        b"lumina-index-fixture",
    )
    .unwrap();
    let sub_cache = sub.join(".lumina");
    std::fs::create_dir_all(&sub_cache).unwrap();
    std::fs::write(sub_cache.join("x.webp"), b"lumina-preview-fixture").unwrap();

    // Unit level: cache webps rejected, the real image accepted.
    assert!(crate::library_scan::scan_entry(&previews.join("top.preview.webp")).is_none());
    assert!(crate::library_scan::scan_entry(&sub_cache.join("x.webp")).is_none());
    assert!(crate::library_scan::scan_entry(&root.path().join("top.arw")).is_some());

    let mut app = new_app();
    // Flat: only the real top-level image lists.
    app.set_directory(root.path().display().to_string());
    let names: Vec<&str> = app
        .entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, vec!["top.arw"]);
    // Recursive: the subfolder image joins; no cache file ever does.
    app.list_directory();
    let mut names: Vec<&str> = app
        .entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    names.sort();
    assert_eq!(names, vec!["mid.arw", "top.arw"]);
    // Sync/Match candidate pool is exactly the entry list — no cache
    // path is selectable, matchable, or sidecar-writable through it.
    assert!(app
        .entries()
        .iter()
        .all(|entry| !is_lumina_cache_path(&entry.path)));
    // Direct navigation into the cache dir itself lists nothing.
    app.set_directory(previews.display().to_string());
    assert!(app.entries().is_empty());
}

/// GUI-LIBRARY-LUMINA-DIR-1 rescan stability: preview-cache files
/// created *after* the first listing stay out of later rescans.
#[test]
fn library_rescan_stays_clean_after_cache_creation() {
    let root = tempfile::tempdir().unwrap();
    save_raw(&root.path().join("top.arw"));

    let mut app = new_app();
    app.set_directory(root.path().display().to_string());
    app.list_directory();
    assert_eq!(app.entries().len(), 1);

    // Simulate preview-cache generation after the first listing.
    let previews = root.path().join(".lumina").join("previews");
    std::fs::create_dir_all(&previews).unwrap();
    std::fs::write(previews.join("top.preview.webp"), b"lumina-preview-fixture").unwrap();

    app.list_directory();
    let names: Vec<&str> = app
        .entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, vec!["top.arw"]);
}

/// GUI-VIEW-2: saving refreshes the single browser entry in place —
/// no full directory rescan (which re-reads + re-hashes every source
/// file) and no unrelated entry churn.
#[test]
fn save_refreshes_single_browser_entry() {
    let directory = tempfile::tempdir().unwrap();
    let source_a = directory.path().join("a.png");
    let source_b = directory.path().join("b.png");
    save_png(&source_a);
    save_png(&source_b);
    let mut app = new_app();
    open_and_decode(&mut app, source_a.display().to_string());
    assert_eq!(app.entries().len(), 2);
    let b_before = app
        .entries()
        .iter()
        .find(|e| e.name == "b.png")
        .cloned()
        .expect("b listed");
    assert!(!b_before.has_sidecar);
    app.set_adjustment("exposure", 1.0);
    app.commit_pending_slider_save([0, 0]);
    assert!(app.error().is_none());
    assert_eq!(app.entries().len(), 2, "no entry churn on save");
    let a_after = app
        .entries()
        .iter()
        .find(|e| e.name == "a.png")
        .expect("a listed");
    assert!(a_after.has_sidecar, "saved entry reflects the sidecar");
    let b_after = app
        .entries()
        .iter()
        .find(|e| e.name == "b.png")
        .expect("b listed");
    assert_eq!(
        format!("{b_after:?}"),
        format!("{b_before:?}"),
        "unrelated entry untouched"
    );
    assert_eq!(app.status, Str::SidecarSaved.t());
}

/// GUI-VIEW-2: same-folder image switches reuse the live browser entries
/// (no rescan); an explicit `set_directory`/Refresh still rescans and
/// picks up external folder changes.
#[test]
fn same_folder_switch_skips_rescan_but_redirectory_rescans() {
    let directory = tempfile::tempdir().unwrap();
    let source_a = directory.path().join("a.png");
    let source_b = directory.path().join("b.png");
    save_png(&source_a);
    save_png(&source_b);
    let mut app = new_app();
    open_and_decode(&mut app, source_a.display().to_string());
    assert_eq!(app.entries().len(), 2);
    // External change while browsing: a new file appears on disk.
    let source_c = directory.path().join("c.png");
    save_png(&source_c);
    // Same-folder switch: no rescan, C stays unlisted.
    open_and_decode(&mut app, source_b.display().to_string());
    assert_eq!(app.entries().len(), 2, "same-folder switch must not rescan");
    assert!(app.entries().iter().all(|e| e.name != "c.png"));
    // Explicit redirectory: full rescan picks C up.
    app.set_directory(directory.path().display().to_string());
    assert_eq!(app.entries().len(), 3);
    assert!(app.entries().iter().any(|e| e.name == "c.png"));
}

// ---- Lightroom-like Library folder tree (pure helpers) ----

#[test]
fn library_root_is_the_workdir() {
    // Lightroom-parity: the Folders tree roots at the current workdir
    // (`directory` field), never at `$HOME` or an ancestor. Deterministic
    // regardless of the environment `$HOME`.
    assert_eq!(
        library_root("/var/folders/xy/ab/cd"),
        PathBuf::from("/var/folders/xy/ab/cd")
    );
    assert_eq!(library_root("/etc"), PathBuf::from("/etc"));
    assert_eq!(library_root("/"), PathBuf::from("/"));
    assert_eq!(library_root("relative/dir"), PathBuf::from("relative/dir"));
    // Empty workdir (unset) falls back to "." so the tree still has a root.
    assert_eq!(library_root(""), PathBuf::from("."));
}

#[test]
fn folder_scan_helpers_count_raw_files_with_depth_limit() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("sub");
    let deeper = sub.join("deeper");
    std::fs::create_dir_all(&deeper).unwrap();
    std::fs::write(dir.path().join("a.ARW"), b"x").unwrap();
    std::fs::write(dir.path().join("b.jpg"), b"x").unwrap();
    std::fs::write(sub.join("c.nef"), b"x").unwrap();
    std::fs::write(deeper.join("d.orf"), b"x").unwrap();

    assert_eq!(count_raw_files(dir.path(), 3), 3);
    // The depth limit stops the scan below `sub`.
    assert_eq!(count_raw_files(dir.path(), 2), 2);
    assert_eq!(count_raw_files(dir.path(), 1), 1);
    assert_eq!(count_raw_files(dir.path(), 0), 0);

    let subs = subdirectories(dir.path());
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0], sub);

    // Labels are root-relative; the root itself shows its final component.
    assert_eq!(folder_label(dir.path(), &sub), "sub");
    let root_name = dir.path().file_name().unwrap().to_string_lossy();
    assert_eq!(folder_label(dir.path(), dir.path()), root_name);
}

#[test]
fn folder_badge_display_fits_fixed_badge_box() {
    // Short badges pass through untouched (existing goldens unchanged).
    assert_eq!(folder_badge_display(""), "");
    assert_eq!(folder_badge_display("sub"), "sub");
    let nested = Path::new("sub").join("nested").display().to_string();
    assert_eq!(folder_badge_display(&nested), nested);
    // Long paths are middle-truncated with … and never exceed the box:
    // 17 monospace-11 chars ≈ 112px ≤ 118px box width.
    let long = "a_very_long_subfolder_name/nested_deep";
    let shown = folder_badge_display(long);
    assert!(
        shown.chars().count() <= FOLDER_BADGE_MAX_CHARS,
        "display badge {shown:?} exceeds {FOLDER_BADGE_MAX_CHARS} chars"
    );
    assert!(shown.contains('…'), "long badge must ellipsize: {shown:?}");
    assert_ne!(shown, long);
    // Head and tail survive so the truncated badge stays recognizable.
    assert!(shown.starts_with("a_very_l"));
    assert!(shown.ends_with("ted_deep"));
}

/// M2: `count_raw_files` terminates on a symlink cycle and counts the
/// looped subtree once (same visited-set convention as the recursive
/// listing scan).
#[cfg(unix)]
#[test]
fn count_raw_files_terminates_on_symlink_loop() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(dir.path().join("a.ARW"), b"x").unwrap();
    std::fs::write(sub.join("b.ARW"), b"x").unwrap();
    std::os::unix::fs::symlink(dir.path(), sub.join("loop")).unwrap();
    assert_eq!(count_raw_files(dir.path(), 3), 2);
    assert_eq!(count_raw_files(dir.path(), 2), 2);
}
