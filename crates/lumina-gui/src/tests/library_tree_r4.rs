//! R4-LIB-1 / R4-SWITCH-2 (F-103-N6 Runde 4) headless tests for the Library
//! folder tree and grid navigation: recursive aggregation, Up/breadcrumb,
//! empty-folder pruning, `.lumina/` invisibility and the traced folder walk.
//!
//! Split out of `tests/library_scan.rs` so both files stay within the strict
//! 500-line rule for new files (file-size ratchet).

use super::*;

/// R4-SWITCH-2: the folder-tree RAW count — the synchronous first-paint walk
/// that was the uninstrumented UI block behind the first Library paint — is
/// timed and traced once per folder, then served from the cache (no second
/// scan line on a later paint).
#[test]
fn folder_tree_raw_count_is_traced_once_and_cached() {
    use crate::timing::take_timing_log;

    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("a.arw"), b"x").unwrap();
    std::fs::write(directory.path().join("note.txt"), b"x").unwrap();
    let mut app = new_app();
    app.directory = directory.path().display().to_string();

    let _ = take_timing_log();
    headless_shapes_sized(&mut app, 2000.0, |app, ui| app.draw_folder_tree(ui));
    let log = take_timing_log();
    let scans: Vec<&String> = log
        .iter()
        .filter(|line| line.contains("folder raw count"))
        .collect();
    assert!(
        !scans.is_empty(),
        "the folder count must be traced: {log:?}"
    );
    assert!(scans[0].contains("files=1"), "{:?}", scans[0]);
    assert!(scans[0].contains("scan_ms="), "{:?}", scans[0]);

    // The count is cached: a second paint reuses it with no new scan line.
    headless_shapes_sized(&mut app, 2000.0, |app, ui| app.draw_folder_tree(ui));
    let log = take_timing_log();
    assert!(
        log.iter().all(|line| !line.contains("folder raw count")),
        "the cached count must not re-scan: {log:?}"
    );
}

/// R4-LIB-1(a): a recursive reload keeps the subfolder images (no navigation
/// resets the aggregation back to flat).
#[test]
fn library_recursive_reload_keeps_subfolder_images() {
    let root = tempfile::tempdir().unwrap();
    let sub = root.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    save_raw(&root.path().join("top.arw"));
    save_raw(&sub.join("mid.arw"));

    let mut app = new_app();
    app.set_directory(root.path().display().to_string());
    for _ in 0..2 {
        app.list_directory();
        let mut names: Vec<&str> = app
            .entries()
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        names.sort();
        assert_eq!(names, vec!["mid.arw", "top.arw"], "recursive reload");
    }
}

/// R4-LIB-1(b): the breadcrumb helper yields clickable `(label, target)`
/// segments; the Up button in the grid navigates to the parent folder without
/// editing the path field.
#[test]
fn library_up_button_navigates_to_parent() {
    let root = tempfile::tempdir().unwrap();
    let sub = root.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    save_raw(&sub.join("a.arw"));

    // Pure helper: one segment per path component, last = current folder.
    let crumbs = crate::library_tree::library_breadcrumb(&sub.display().to_string());
    assert!(crumbs.len() >= 2);
    assert_eq!(crumbs.last().unwrap().1, sub.display().to_string());
    assert_eq!(
        crumbs[crumbs.len() - 2].1,
        root.path().display().to_string()
    );

    let mut app = new_app();
    app.set_directory(sub.display().to_string());
    assert!(
        app.entries().iter().any(|entry| entry.name == "a.arw"),
        "the subfolder lists its image before navigating up"
    );
    let _ = headless_click_label(&mut app, "⬆", |app, ui| {
        let ctx = ui.ctx().clone();
        app.draw_library_grid(&ctx, ui);
    });
    assert_eq!(
        app.directory(),
        root.path().display().to_string(),
        "the Up button moves the workdir to the parent"
    );
}

/// R4-LIB-1(c/d): the folder tree hides folders whose whole (depth-limited)
/// subtree carries no supported image and never shows the deletable `.lumina/`
/// cache directory; a folder with images keeps its node.
#[test]
fn folder_tree_hides_empty_folders_and_lumina_cache() {
    let root = tempfile::tempdir().unwrap();
    let full = root.path().join("full");
    let empty = root.path().join("empty");
    std::fs::create_dir_all(empty.join("nested")).unwrap();
    std::fs::create_dir_all(&full).unwrap();
    save_raw(&full.join("a.arw"));
    // A `.lumina/` cache dir with a (supported) preview webp must still never
    // become a tree node.
    let previews = root.path().join(".lumina").join("previews");
    std::fs::create_dir_all(&previews).unwrap();
    std::fs::write(previews.join("a.preview.webp"), b"cache").unwrap();

    let mut app = new_app();
    app.directory = root.path().display().to_string();
    let shapes = headless_shapes_sized(&mut app, 2000.0, |app, ui| app.draw_folder_tree(ui));
    assert!(
        text_contains(&shapes, "full (1)"),
        "a folder with an image keeps its node: {:?}",
        painted_texts(&shapes)
    );
    assert!(
        !text_contains(&shapes, "empty (0)"),
        "an empty subtree must be pruned: {:?}",
        painted_texts(&shapes)
    );
    assert!(
        !text_contains(&shapes, ".lumina"),
        "the cache dir must never be a tree node: {:?}",
        painted_texts(&shapes)
    );
}
